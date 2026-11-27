//! The `--live` seam (spec §17.5): one probe per provider reads the provider's view of a
//! credential (GET only, token values from env, never printed). Phase 1 registers none, so
//! every active credential gets an `unknown` item.

use super::Credential;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveResult {
    /// The provider agrees with system.toml; the detail says what was read.
    Ok(String),
    /// The provider disagrees (expired, revoked, other scopes); the detail says how.
    Drift(String),
    /// Not readable (no probe, no token, API error); never a failure on its own.
    Unknown(String),
}

pub trait LiveProbe {
    /// The `[[provider]] id` this probe answers for.
    fn provider(&self) -> &str;
    fn probe(&self, cred: &Credential) -> LiveResult;
}

/// The registered probes. Phase 2 adds github, cloudflare, fly, npm, crates.
pub fn probes() -> Vec<Box<dyn LiveProbe>> {
    Vec::new()
}

/// The probe for `cred`'s provider, or `unknown`.
pub fn probe(probes: &[Box<dyn LiveProbe>], cred: &Credential) -> LiveResult {
    probes
        .iter()
        .find(|p| p.provider() == cred.provider)
        .map_or_else(
            || {
                LiveResult::Unknown(format!(
                    "no live probe for provider `{}` yet",
                    cred.provider
                ))
            },
            |p| p.probe(cred),
        )
}
