use super::repo;
use crate::ctx::Ctx;
use crate::error::KeelError;
use crate::report::HumanStep;
use crate::step::{Check, Step};
use crate::tools::{failure, gh};

pub struct Token;

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The prefilled fine-grained PAT link (§7): Contents, Pull requests and Workflows, read and write.
pub fn pat_url(repo: &str) -> String {
    let owner = repo.split('/').next().unwrap_or(repo);
    format!(
        "https://github.com/settings/personal-access-tokens/new?name={}&description={}&target_name={}&expires_in=90&contents=write&pull_requests=write&workflows=write",
        enc(&format!("keel {repo}")),
        enc(&format!(
            "keel: lets the splice update bot open PRs that run CI on {repo}"
        )),
        enc(owner),
    )
}

impl Token {
    fn human(&self, repo: &str, secret: &str, env: &str) -> Check {
        Check::Human(HumanStep {
            id: self.id().into(),
            title: format!(
                "Create a fine-grained token for {repo} (Contents, Pull requests, Workflows: read and write; only that repo), then run the command and paste it; keel stores it as {secret} in the {env} environment"
            ),
            url: pat_url(repo),
            command: "keel up --token-stdin".into(),
        })
    }
}

impl Step for Token {
    fn id(&self) -> &'static str {
        "token"
    }

    fn check(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let repo = repo(cx)?;
        let (secret, env) = (&cx.manifest.token.secret, &cx.manifest.token.environment);
        if gh::secret_exists(cx.runner, &repo, env, secret) {
            return Ok(Check::Done(format!("{secret} is set in environment {env}")));
        }
        if cx.opts.token_stdin {
            Ok(Check::Todo(format!("store {secret} in environment {env}")))
        } else {
            Ok(self.human(&repo, secret, env))
        }
    }

    fn apply(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let repo = repo(cx)?;
        let (secret, env) = (
            cx.manifest.token.secret.clone(),
            cx.manifest.token.environment.clone(),
        );
        if !cx.opts.token_stdin {
            return Ok(self.human(&repo, &secret, &env));
        }
        let Ok(raw) = (cx.stdin)() else {
            return Ok(Check::Failed(
                "cannot read the token from stdin; run `keel up --token-stdin` and paste it".into(),
            ));
        };
        let value = raw.trim_end_matches(['\n', '\r']);
        if value.is_empty() {
            return Ok(Check::Failed(
                "the token on stdin is empty; run `keel up --token-stdin` and paste the token"
                    .into(),
            ));
        }
        let c = gh::secret_cmd(&repo, &env, &secret, value.as_bytes());
        let o = cx.runner.run(&c);
        if o.code != 0 {
            // The detail names the command (never its stdin); scrub the value from the tool's text.
            return Ok(Check::Failed(failure(&c, &o).replace(value, "***")));
        }
        drop(c);
        self.check(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;
    use crate::ctx::Opts;
    use crate::exec::{FakeRunner, Out};
    use std::time::Duration;
    use zeroize::Zeroizing;

    const API: &[&str] = &[
        "gh",
        "api",
        "repos/owner/name/environments/splice-bot/secrets/SPLICE_BOT_TOKEN",
    ];

    #[test]
    fn present_is_done() {
        let r = FakeRunner::new().on(API, Out::ok("{}"));
        let mut cx = ctx(
            &r,
            ".".into(),
            manifest("rust"),
            PINS,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        assert!(matches!(Token.check(&mut cx).unwrap(), Check::Done(_)));
    }

    #[test]
    fn missing_without_flag_is_human_with_exact_url() {
        let r = FakeRunner::new().on(API, Out::with(1, "", "404"));
        let mut cx = ctx(
            &r,
            ".".into(),
            manifest("rust"),
            PINS,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        let Check::Human(h) = Token.check(&mut cx).unwrap() else {
            panic!()
        };
        assert_eq!(h.command, "keel up --token-stdin");
        let (base, q) = h.url.split_once('?').unwrap();
        assert_eq!(
            base,
            "https://github.com/settings/personal-access-tokens/new"
        );
        let q: Vec<&str> = q.split('&').collect();
        assert_eq!(q[0], "name=keel%20owner%2Fname");
        assert!(q[1].starts_with("description="));
        assert_eq!(
            &q[2..],
            [
                "target_name=owner",
                "expires_in=90",
                "contents=write",
                "pull_requests=write",
                "workflows=write"
            ]
        );
    }

    fn with_token(r: &FakeRunner, token: &'static str, dur: &dyn Fn(Duration)) -> Check {
        let stdin = move || Ok(Zeroizing::new(token.to_string()));
        let opts = Opts {
            token_stdin: true,
            ..Opts::default()
        };
        let mut cx = ctx(r, ".".into(), manifest("rust"), PINS, opts, dur, &stdin);
        Token.apply(&mut cx).unwrap()
    }

    #[test]
    fn token_goes_on_stdin_only_never_argv() {
        let tok = "github_pat_SECRET123";
        let r = FakeRunner::new()
            .on(API, Out::with(1, "", "404"))
            .on(&["gh", "secret", "set"], Out::ok(""));
        let _ = with_token(&r, "github_pat_SECRET123\n", &no_sleep);
        let calls = r.calls();
        let set = calls
            .iter()
            .find(|c| c.args.first().map(String::as_str) == Some("secret"))
            .unwrap();
        assert_eq!(
            set.args,
            [
                "secret",
                "set",
                "SPLICE_BOT_TOKEN",
                "--env",
                "splice-bot",
                "--repo",
                "owner/name"
            ]
        );
        assert_eq!(
            set.stdin.as_deref().map(|v| v.as_slice()),
            Some(tok.as_bytes())
        );
        for c in &calls {
            assert!(
                !c.line().contains(tok) && !format!("{c:?}").contains(tok),
                "{c:?}"
            );
        }
    }

    #[test]
    fn empty_token_fails_without_calling_gh_secret_set() {
        let r = FakeRunner::new().on(API, Out::with(1, "", "404"));
        let Check::Failed(m) = with_token(&r, "\n", &no_sleep) else {
            panic!()
        };
        assert!(m.contains("empty"));
        assert!(
            !r.calls()
                .iter()
                .any(|c| c.args.first().map(String::as_str) == Some("secret"))
        );
    }

    #[test]
    fn failure_detail_never_contains_the_token() {
        let tok = "github_pat_LEAK";
        let r = FakeRunner::new().on(API, Out::with(1, "", "404")).on(
            &["gh", "secret", "set"],
            Out::with(1, "", "bad credentials github_pat_LEAK"),
        );
        let Check::Failed(m) = with_token(&r, "github_pat_LEAK", &no_sleep) else {
            panic!()
        };
        assert!(!m.contains(tok) && m.contains("gh secret set"), "{m}");
    }
}
