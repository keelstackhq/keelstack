//! Trusted authors for intake (spec §3, BA): who may start work by opening an issue or by
//! commenting a bosun command. Pure: the caller reads the config, the owners and the author's
//! `author_association` and passes them in.

use serde::{Deserialize, Serialize};

/// `[intake] trusted_authors`: `"any"` (default, today's behaviour), `"members"`, or a login list.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(untagged)]
pub enum TrustedAuthors {
    #[default]
    Any,
    Members,
    Logins(Vec<String>),
}

impl<'de> Deserialize<'de> for TrustedAuthors {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Word(String),
            List(Vec<String>),
        }
        match Raw::deserialize(d)? {
            Raw::Word(w) if w == "any" => Ok(TrustedAuthors::Any),
            Raw::Word(w) if w == "members" => Ok(TrustedAuthors::Members),
            Raw::Word(w) => Err(serde::de::Error::custom(format!(
                "intake.trusted_authors {w:?} is not \"any\", \"members\" or a list of logins"
            ))),
            Raw::List(l) if l.iter().all(|x| !x.trim().is_empty()) => Ok(TrustedAuthors::Logins(l)),
            Raw::List(_) => Err(serde::de::Error::custom(
                "intake.trusted_authors must not list an empty login",
            )),
        }
    }
}

/// Who wrote an issue or comment, and what the caller knows about them.
#[derive(Debug, Clone, Copy)]
pub struct Author<'a> {
    pub login: &'a str,
    /// GitHub's `author_association`; `None` or `""` when GitHub left it out.
    pub association: Option<&'a str>,
    /// The owner (user or org) of the repository.
    pub repo_owner: &'a str,
    /// The logins of `governance/owners.toml` (users and readable team members).
    pub owners: &'a [String],
}

/// The answer of [`decide`]; `reason` always says why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trust {
    pub trusted: bool,
    pub reason: String,
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Whether `author` may start work under `policy`. The repo owner and the `owners.toml` owners
/// are always trusted. `"members"` also trusts `OWNER`, `MEMBER` and `COLLABORATOR`; a missing or
/// unknown association is not trusted (fail closed, `author_association` is no proof of more).
pub fn decide(policy: &TrustedAuthors, author: &Author) -> Trust {
    let yes = |r: &str| Trust {
        trusted: true,
        reason: r.into(),
    };
    let no = |r: String| Trust {
        trusted: false,
        reason: r,
    };
    if *policy == TrustedAuthors::Any {
        return yes("trusted_authors is any");
    }
    let login = author.login.trim();
    if login.is_empty() {
        return no("the author is unknown".into());
    }
    if same(login, author.repo_owner) {
        return yes("repository owner");
    }
    if author.owners.iter().any(|o| same(o, login)) {
        return yes("owner in governance/owners.toml");
    }
    match policy {
        TrustedAuthors::Any => yes("trusted_authors is any"),
        TrustedAuthors::Logins(list) => {
            if list.iter().any(|l| same(l.trim(), login)) {
                yes("listed in intake.trusted_authors")
            } else {
                no(format!("{login} is not listed in intake.trusted_authors"))
            }
        }
        TrustedAuthors::Members => match author.association.map(str::trim) {
            Some(a)
                if ["OWNER", "MEMBER", "COLLABORATOR"]
                    .iter()
                    .any(|t| a.eq_ignore_ascii_case(t)) =>
            {
                yes("author_association is OWNER, MEMBER or COLLABORATOR")
            }
            Some(a) if !a.is_empty() => {
                no(format!("{login} is not a member (author_association {a})"))
            }
            _ => no(format!(
                "{login} has no known author_association (fail closed)"
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a<'a>(login: &'a str, assoc: Option<&'a str>, owners: &'a [String]) -> Author<'a> {
        Author {
            login,
            association: assoc,
            repo_owner: "acme",
            owners,
        }
    }

    #[test]
    fn decisions() {
        let owners = vec!["Ada".to_string()];
        let m = TrustedAuthors::Members;
        assert!(decide(&TrustedAuthors::Any, &a("x", None, &owners)).trusted);
        assert!(decide(&m, &a("ACME", None, &owners)).trusted);
        assert!(decide(&m, &a("ada", None, &owners)).trusted);
        for ok in ["OWNER", "MEMBER", "COLLABORATOR"] {
            assert!(decide(&m, &a("x", Some(ok), &owners)).trusted, "{ok}");
        }
        for bad in [None, Some(""), Some("NONE"), Some("CONTRIBUTOR"), Some("?")] {
            assert!(!decide(&m, &a("x", bad, &owners)).trusted, "{bad:?}");
        }
        assert!(!decide(&m, &a("", Some("OWNER"), &owners)).trusted);
        let l = TrustedAuthors::Logins(vec!["Bo".into()]);
        assert!(decide(&l, &a("bo", None, &owners)).trusted);
        assert!(!decide(&l, &a("x", Some("OWNER"), &owners)).trusted);
        assert!(decide(&l, &a("ada", None, &owners)).trusted);
    }
}
