use crate::ctx::Ctx;
use crate::error::KeelError;
use crate::report::HumanStep;
use crate::step::{Check, Step};
use crate::tools::bosun::{self, Res};

pub struct AgentEnv;

pub const FILE: &str = ".keel/cloud-setup.sh";
const PRELUDE: &str = include_str!("../proxy_ca.sh");

/// Single-quote a value for bash.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

impl Step for AgentEnv {
    fn id(&self) -> &'static str {
        "agent-env"
    }

    fn check(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        Ok(if cx.manifest.confirmed.agent_env {
            Check::Done("cloud setup script confirmed".into())
        } else {
            Check::Todo(format!("write {FILE}"))
        })
    }

    fn apply(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let c = bosun::setup_snippet_cmd(&cx.dir);
        let pin = cx.pins.tool("bosun").map(|p| p.version).unwrap_or_default();
        let data = match bosun::run(cx.runner, &c, &pin) {
            Res::Ok(d) => d,
            Res::Human { title, command } => {
                return Ok(Check::Human(HumanStep {
                    id: self.id().into(),
                    title,
                    url: String::new(),
                    command,
                }));
            }
            Res::Failed(e) => return Ok(Check::Failed(e)),
        };
        let Some(script) = data["script"].as_str().filter(|s| !s.trim().is_empty()) else {
            return Ok(Check::Failed(
                "`bosun --json setup-snippet` returned no `data.script`; upgrade bosun".into(),
            ));
        };
        let path = cx.dir.join(FILE);
        let name = cx
            .manifest
            .repo
            .as_deref()
            .and_then(|r| r.rsplit('/').next());
        let text = format!(
            "KEEL_REPO_DIR={}\n{PRELUDE}\n{script}\n",
            shell_quote(name.unwrap_or(""))
        );
        let wrote = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, text));
        if let Err(e) = wrote {
            return Ok(Check::Failed(format!("cannot write {FILE}: {e}")));
        }
        Ok(Check::Human(HumanStep {
            id: self.id().into(),
            title: format!(
                "Paste {FILE} into claude.ai > Code > your environment > Setup script, save it, then confirm"
            ),
            url: "https://claude.ai/code".into(),
            command: "keel up --confirm agent-env".into(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;
    use crate::ctx::Opts;
    use crate::exec::{FakeRunner, Out};

    fn run(r: &FakeRunner, dir: &std::path::Path, confirmed: bool, apply: bool) -> Check {
        let mut m = manifest("rust");
        m.confirmed.agent_env = confirmed;
        let mut cx = ctx(
            r,
            dir.into(),
            m,
            PINS,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        if apply {
            AgentEnv.apply(&mut cx)
        } else {
            AgentEnv.check(&mut cx)
        }
        .unwrap()
    }

    #[test]
    fn confirmed_is_done_else_todo_and_check_writes_nothing() {
        let d = tempfile::tempdir().unwrap();
        let r = FakeRunner::new();
        assert!(matches!(run(&r, d.path(), true, false), Check::Done(_)));
        assert!(matches!(run(&r, d.path(), false, false), Check::Todo(_)));
        assert!(!d.path().join(FILE).exists());
    }

    #[test]
    fn apply_writes_prelude_plus_snippet_and_asks_a_human() {
        let d = tempfile::tempdir().unwrap();
        let r = FakeRunner::new().on(
            &["bosun", "--json", "setup-snippet"],
            Out::ok(
                r#"{"version":1,"command":"setup-snippet","ok":true,"exit_code":0,"data":{"script":"echo BOSUN_SNIPPET"},"error":null,"next":[]}"#,
            ),
        );
        let Check::Human(h) = run(&r, d.path(), false, true) else {
            panic!()
        };
        assert_eq!(h.command, "keel up --confirm agent-env");
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(text.contains("proxy-ca.sh") && text.ends_with("echo BOSUN_SNIPPET\n"));
        assert!(text.find("proxy-ca.sh") < text.find("BOSUN_SNIPPET"));
        assert!(text.starts_with("KEEL_REPO_DIR='name'\n"), "{text}");
    }

    #[test]
    fn snippet_failure_is_failed_with_its_fix() {
        let d = tempfile::tempdir().unwrap();
        let r = FakeRunner::new().on(
            &["bosun", "--json", "setup-snippet"],
            Out::with(
                1,
                r#"{"ok":false,"error":{"message":"boom","fix":"run bosun init"}}"#,
                "",
            ),
        );
        let Check::Failed(m) = run(&r, d.path(), false, true) else {
            panic!()
        };
        assert!(m.contains("run bosun init"), "{m}");
        assert!(!d.path().join(FILE).exists());
    }

    #[test]
    fn prelude_is_valid_bash_and_never_fails() {
        assert!(PRELUDE.contains("keel-proxy-ca.sh"));
        for v in [
            "NODE_EXTRA_CA_CERTS",
            "SSL_CERT_FILE",
            "REQUESTS_CA_BUNDLE",
            "CARGO_HTTP_CAINFO",
            "GIT_SSL_CAINFO",
            "npm config set cafile",
        ] {
            assert!(PRELUDE.contains(v), "{v}");
        }
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("p.sh");
        std::fs::write(&f, PRELUDE).unwrap();
        match std::process::Command::new("bash")
            .arg("-n")
            .arg(&f)
            .status()
        {
            Ok(s) => assert!(s.success()),
            Err(_) => eprintln!("bash not found; skipped `bash -n`"),
        }
    }

    /// The checkout is `<cwd>/repo`; the hook (when present) records where it ran.
    fn run_prelude(hook_code: Option<i32>, path: &str) -> Option<(bool, String, bool)> {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("repo/scripts")).unwrap();
        if let Some(code) = hook_code {
            std::fs::write(
                d.path().join("repo/scripts/proxy-ca.sh"),
                format!("pwd > ../hook-cwd\nexit {code}\n"),
            )
            .unwrap();
        }
        let ca = d.path().join("ca.pem");
        std::fs::write(&ca, "x").unwrap();
        let prof = d.path().join("profile");
        std::fs::create_dir(&prof).unwrap();
        let f = d.path().join("p.sh");
        std::fs::write(
            &f,
            format!("KEEL_REPO_DIR=repo\n{PRELUDE}\necho \"CA=${{GIT_SSL_CAINFO:-}}\"\necho \"PWD=$PWD\"\n"),
        )
        .unwrap();
        let o = std::process::Command::new("bash")
            .arg(&f)
            .current_dir(d.path())
            .env("PATH", path)
            .env_remove("SSL_CERT_FILE")
            .env("PROXY_CA_BUNDLE", &ca)
            .env("KEEL_PROFILE_DIR", &prof)
            .output()
            .ok()?;
        let mut out = String::from_utf8_lossy(&o.stdout).to_string();
        if let Ok(h) = std::fs::read_to_string(d.path().join("hook-cwd")) {
            out.push_str(&format!("HOOK={}", h.trim()));
        }
        Some((
            o.status.success(),
            out,
            prof.join("keel-proxy-ca.sh").exists(),
        ))
    }

    #[test]
    fn hook_success_exports_in_this_shell_and_skips_the_fallback_file() {
        let Some((ok, out, wrote)) =
            run_prelude(Some(0), &std::env::var("PATH").unwrap_or_default())
        else {
            return;
        };
        assert!(ok);
        assert!(out.contains("CA=/"), "{out}");
        assert!(!wrote);
        assert!(
            out.contains("HOOK=") && out.contains("/repo"),
            "hook ran in the checkout: {out}"
        );
        let pwd = out.lines().find_map(|l| l.strip_prefix("PWD=")).unwrap();
        assert!(!pwd.ends_with("/repo"), "cwd restored: {out}");
    }

    #[test]
    fn missing_hook_runs_the_fallback_and_missing_repo_dir_uses_the_cwd() {
        let bin = tempfile::tempdir().unwrap();
        let Some(b) = ["/usr/bin/bash", "/bin/bash"]
            .iter()
            .find(|p| std::path::Path::new(p).exists())
        else {
            return;
        };
        std::os::unix::fs::symlink(b, bin.path().join("bash")).unwrap();
        let Some((ok, out, wrote)) = run_prelude(None, bin.path().to_str().unwrap()) else {
            return;
        };
        assert!(ok && wrote && out.contains("CA=/"), "{out}");
        // Repo dir absent: the prelude stays in the cwd (no hook there) and still succeeds.
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("p.sh");
        std::fs::write(
            &f,
            format!("KEEL_REPO_DIR=nope\n{PRELUDE}\necho \"PWD=$PWD\"\n"),
        )
        .unwrap();
        let Ok(o) = std::process::Command::new("bash")
            .arg(&f)
            .current_dir(d.path())
            .env("KEEL_PROFILE_DIR", d.path())
            .output()
        else {
            return;
        };
        assert!(o.status.success());
    }

    #[test]
    fn hook_failure_does_not_run_the_fallback_and_still_exits_zero() {
        let bin = tempfile::tempdir().unwrap();
        let Ok(b) = ["/usr/bin/bash", "/bin/bash"]
            .iter()
            .find(|p| std::path::Path::new(p).exists())
            .ok_or(())
        else {
            return;
        };
        std::os::unix::fs::symlink(b, bin.path().join("bash")).unwrap();
        let Some((ok, out, wrote)) = run_prelude(Some(1), bin.path().to_str().unwrap()) else {
            return;
        };
        assert!(ok, "never a non-zero exit");
        assert!(out.contains("CA=/"), "{out}");
        assert!(!wrote, "fallback runs only when hull's script is absent");
    }

    #[test]
    fn prelude_calls_only_the_root_hook_and_trusts_only_the_proxy_bundle() {
        assert!(PRELUDE.contains("./scripts/proxy-ca.sh install"));
        assert!(!PRELUDE.contains("./*/"), "no per-checkout loop");
        assert!(
            !PRELUDE.contains("${SSL_CERT_FILE"),
            "SSL_CERT_FILE is not a CA source"
        );
        assert!(
            PRELUDE.contains("PROXY_CA_BUNDLE") && PRELUDE.contains("/root/.ccr/ca-bundle.crt")
        );
        // A hook in a sub-directory never runs; SSL_CERT_FILE alone writes no fallback file.
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("sub/scripts")).unwrap();
        std::fs::write(
            d.path().join("sub/scripts/proxy-ca.sh"),
            "touch ../../ran\n",
        )
        .unwrap();
        let ca = d.path().join("ca.pem");
        std::fs::write(&ca, "x").unwrap();
        let prof = d.path().join("profile");
        std::fs::create_dir(&prof).unwrap();
        let f = d.path().join("p.sh");
        std::fs::write(&f, PRELUDE).unwrap();
        let Ok(o) = std::process::Command::new("bash")
            .arg(&f)
            .current_dir(d.path())
            .env_remove("PROXY_CA_BUNDLE")
            .env("SSL_CERT_FILE", &ca)
            .env("KEEL_PROFILE_DIR", &prof)
            .output()
        else {
            return;
        };
        assert!(o.status.success());
        assert!(!d.path().join("ran").exists());
        if !std::path::Path::new("/root/.ccr/ca-bundle.crt").exists() {
            assert!(!prof.join("keel-proxy-ca.sh").exists());
        }
    }
}
