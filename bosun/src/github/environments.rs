//! Environments, environment variables and sealed environment secrets (B2 T2, `owner setup`).
//!
//! A secret reaches GitHub only as a libsodium sealed box (`crypto_box_seal`) made with the
//! environment's own public key; the plaintext is never sent, logged or put in an error.

use crypto_box::PublicKey;
use crypto_box::aead::OsRng;
use serde_json::{Value, json};

use super::b64;
use super::http::enc;
use super::{LiveGithub, Result, Secret};
use crate::output::BosunError;

const BAD_KEY: &str = "GitHub sent an invalid environment public key";

/// Seal `value` for the base64 `public_key` (32 bytes); returns the base64 sealed box.
pub fn seal(public_key_b64: &str, value: &Secret) -> Result<String> {
    let bad = || {
        BosunError::error(
            BAD_KEY,
            "rerun; if it persists the token cannot read secrets",
        )
    };
    let bytes = b64::decode(public_key_b64.trim()).ok_or_else(bad)?;
    let raw: [u8; 32] = bytes.try_into().map_err(|_| bad())?;
    let sealed = PublicKey::from(raw)
        .seal(&mut OsRng, value.expose().as_bytes())
        .map_err(|_| BosunError::error("could not seal the secret value", "rerun the command"))?;
    Ok(b64::encode(&sealed))
}

impl LiveGithub {
    fn env_path(&self, env: &str, rest: &str) -> String {
        self.path(&format!("/environments/{}{rest}", enc(env)))
    }

    /// Create or update environment `name`: no reviewers, deployment branches "selected" with
    /// exactly the branch policy `main`. The call is idempotent.
    pub(super) fn put_env_main_only(&self, name: &str) -> Result<()> {
        let body = json!({
            "reviewers": [],
            "deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true},
        });
        self.http
            .send_json("PUT", &self.env_path(name, ""), &body)?;
        // 422: the policy `main` already exists.
        self.http.send_accepting(
            "POST",
            &self.env_path(name, "/deployment-branch-policies"),
            &json!({"name": "main", "type": "branch"}),
            &[422],
        )?;
        Ok(())
    }

    /// `(key_id, key)` of the environment's secrets public key.
    pub(super) fn env_public_key(&self, env: &str) -> Result<(String, String)> {
        let v = self
            .http
            .get_json(&self.env_path(env, "/secrets/public-key"))?;
        let field = |n: &str| v.as_ref().and_then(|v| v[n].as_str()).map(str::to_string);
        match (field("key_id"), field("key")) {
            (Some(id), Some(key)) => Ok((id, key)),
            _ => Err(BosunError::error(
                format!("GitHub has no secrets public key for environment {env}"),
                "check that the environment exists and the token has Secrets and Environments write access",
            )),
        }
    }

    pub(super) fn put_env_secret_sealed(
        &self,
        env: &str,
        name: &str,
        key_id: &str,
        sealed: &str,
    ) -> Result<()> {
        let body = json!({"encrypted_value": sealed, "key_id": key_id});
        self.http
            .send_json(
                "PUT",
                &self.env_path(env, &format!("/secrets/{}", enc(name))),
                &body,
            )
            .map(|_: Value| ())
    }

    pub(super) fn put_env_variable(&self, env: &str, name: &str, value: &str) -> Result<()> {
        let body = json!({"name": name, "value": value});
        let one = self.env_path(env, &format!("/variables/{}", enc(name)));
        match self.http.get_json(&one)? {
            // Exists: update it only when the value differs.
            Some(v) if v["value"].as_str() == Some(value) => Ok(()),
            Some(_) => self
                .http
                .send_accepting("PATCH", &one, &body, &[])
                .map(|_| ()),
            None => self
                .http
                .send_json("POST", &self.env_path(env, "/variables"), &body)
                .map(|_: Value| ()),
        }
    }

    /// Whether the environment has a `required_reviewers` protection rule.
    pub(super) fn env_has_reviewers(&self, name: &str) -> Result<bool> {
        let v = self.http.get_json(&self.env_path(name, ""))?;
        Ok(v.is_some_and(|v| {
            v["protection_rules"]
                .as_array()
                .is_some_and(|r| r.iter().any(|x| x["type"] == "required_reviewers"))
        }))
    }
}
