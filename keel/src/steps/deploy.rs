use super::repo;
use crate::ctx::Ctx;
use crate::error::KeelError;
use crate::exec::Out;
use crate::report::HumanStep;
use crate::step::{Check, Step};
use crate::tools::{failure, fly, gh, not_found};

pub struct Deploy;

fn login_step() -> HumanStep {
    HumanStep {
        id: "deploy".into(),
        title: "Log in to Fly.io".into(),
        url: "https://fly.io/docs/flyctl/auth-login/".into(),
        command: LOGIN_COMMAND.into(),
    }
}

const LOGIN_COMMAND: &str =
    "fly auth login   # or: export FLY_API_TOKEN=$(fly tokens create org) then keel up";

fn missing_piece() -> Check {
    Check::Failed(format!(
        "{} is missing: add the `fly` piece (pieces = [..., \"fly\"]) and rerun",
        fly::SETUP_SCRIPT
    ))
}

/// What a script exit means: 2 is an old script that does not take `flag`; 3 is a missing tool
/// or auth (human); 4 is a failed fly/gh step.
fn script_result(cx: &Ctx, args: &[String], flag: &str, o: &Out) -> Option<Check> {
    if o.code == 0 {
        return None;
    }
    let cmd = fly::setup_cmd(&cx.dir, args, cx.opts.tool_timeout());
    Some(if o.code == 3 {
        Check::Human(HumanStep {
            id: "deploy".into(),
            title: "Log in to Fly.io and install flyctl/gh".into(),
            url: "https://fly.io/docs/flyctl/auth-login/".into(),
            command: LOGIN_COMMAND.into(),
        })
    } else if o.code == 2 {
        Check::Human(HumanStep {
            id: "deploy".into(),
            title: format!(
                "Upgrade the hull fly piece: {} does not take {flag}",
                fly::SETUP_SCRIPT
            ),
            url: String::new(),
            command: "splice --json --yes upgrade   # then keel up".into(),
        })
    } else if not_found(o) {
        Check::Failed(format!(
            "`{}` could not start; make it executable (chmod +x {})",
            cmd.line(),
            fly::SETUP_SCRIPT
        ))
    } else {
        Check::Failed(failure(&cmd, o))
    })
}

impl Step for Deploy {
    fn id(&self) -> &'static str {
        "deploy"
    }

    fn check(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let Some(f) = cx.manifest.fly().cloned() else {
            return Ok(Check::Done("no [deploy.fly]".into()));
        };
        if !cx.dir.join(fly::SETUP_SCRIPT).is_file() {
            return Ok(missing_piece());
        }
        if !fly::logged_in(cx.runner, &cx.dir) {
            return Ok(Check::Human(login_step()));
        }
        let repo = repo(cx)?;
        let mut missing = Vec::new();
        if !fly::app_exists(cx.runner, &cx.dir, &f.app) {
            missing.push(format!("fly app {}", f.app));
        }
        if !fly::cert_exists(cx.runner, &cx.dir, &f.app, &f.host) {
            missing.push(format!("certificate for {}", f.host));
        }
        if !gh::secret_exists(cx.runner, &repo, "production", "FLY_API_TOKEN") {
            missing.push("FLY_API_TOKEN in the production environment".into());
        }
        Ok(if missing.is_empty() {
            Check::Done(format!("fly app {} at {}", f.app, f.host))
        } else {
            Check::Todo(format!("missing: {}", missing.join(", ")))
        })
    }

    fn apply(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let Some(f) = cx.manifest.fly().cloned() else {
            return self.check(cx);
        };
        // Never `--deploy`: the first merge deploys.
        let args = fly::setup_args(&f.app, &f.region, &f.host, None);
        let o = fly::setup(
            cx.runner,
            &cx.dir,
            &f.app,
            &f.region,
            &f.host,
            cx.opts.tool_timeout(),
        );
        if let Some(c) = script_result(cx, &args, "--region", &o) {
            return Ok(c);
        }
        let args = fly::setup_args(&f.app, &f.region, &f.host, Some("production"));
        let o = fly::set_gh_secret(
            cx.runner,
            &cx.dir,
            &f.app,
            &f.region,
            &f.host,
            "production",
            cx.opts.tool_timeout(),
        );
        if let Some(c) = script_result(cx, &args, "--set-gh-secret", &o) {
            return Ok(c);
        }
        self.check(cx)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::testutil::*;
    use super::*;
    use crate::ctx::Opts;
    use crate::exec::{FakeRunner, Out};
    use crate::manifest::{Deploy as D, Fly, Manifest};

    pub fn fly_manifest() -> Manifest {
        Manifest {
            deploy: Some(D {
                fly: Some(Fly {
                    app: "app".into(),
                    region: "fra".into(),
                    host: "app.example.com".into(),
                    dns: Default::default(),
                    zone: None,
                    no_mail: false,
                }),
            }),
            ..manifest("rust")
        }
    }

    fn dir(script: bool) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        if script {
            std::fs::create_dir(d.path().join("scripts")).unwrap();
            std::fs::write(d.path().join(fly::SETUP_SCRIPT), "").unwrap();
        }
        d
    }

    fn run(r: &FakeRunner, d: &tempfile::TempDir, m: Manifest, apply: bool) -> Check {
        let mut cx = ctx(
            r,
            d.path().into(),
            m,
            PINS,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        if apply {
            Deploy.apply(&mut cx)
        } else {
            Deploy.check(&mut cx)
        }
        .unwrap()
    }

    fn all_ok() -> FakeRunner {
        FakeRunner::new()
            .on(&["fly", "auth", "whoami"], Out::ok("{}"))
            .on(&["fly", "status"], Out::ok("{}"))
            .on(&["fly", "certs", "show"], Out::ok("{}"))
            .on(
                &[
                    "gh",
                    "api",
                    "repos/owner/name/environments/production/secrets/FLY_API_TOKEN",
                ],
                Out::ok("{}"),
            )
    }

    #[test]
    fn no_fly_is_done() {
        let r = FakeRunner::new();
        let c = run(&r, &dir(false), manifest("rust"), false);
        assert_eq!(c, Check::Done("no [deploy.fly]".into()));
        assert!(r.calls().is_empty());
    }

    #[test]
    fn missing_script_names_the_piece() {
        let r = all_ok();
        let Check::Failed(m) = run(&r, &dir(false), fly_manifest(), false) else {
            panic!()
        };
        assert!(m.contains("`fly` piece"), "{m}");
    }

    #[test]
    fn not_logged_in_or_no_binary_is_the_login_human_step() {
        for out in [Out::with(1, "", "not logged in"), Out::with(-1, "", "nf")] {
            let r = FakeRunner::new().on(&["fly", "auth", "whoami"], out);
            let Check::Human(h) = run(&r, &dir(true), fly_manifest(), false) else {
                panic!()
            };
            assert_eq!(h.title, "Log in to Fly.io");
            assert!(h.command.contains("fly auth login"));
        }
    }

    #[test]
    fn everything_present_is_done_else_todo() {
        let d = dir(true);
        assert!(matches!(
            run(&all_ok(), &d, fly_manifest(), false),
            Check::Done(_)
        ));
        let r = FakeRunner::new()
            .on(&["fly", "auth", "whoami"], Out::ok("{}"))
            .on(&["fly", "status"], Out::with(1, "", "no app"));
        let Check::Todo(m) = run(&r, &d, fly_manifest(), false) else {
            panic!()
        };
        assert!(
            m.contains("fly app app") && m.contains("certificate") && m.contains("FLY_API_TOKEN")
        );
    }

    #[test]
    fn apply_runs_setup_then_token_never_deploy_then_rechecks() {
        let r = all_ok().on(&["scripts/fly-setup.sh"], Out::ok(""));
        assert!(matches!(
            run(&r, &dir(true), fly_manifest(), true),
            Check::Done(_)
        ));
        let lines: Vec<String> = r.calls().iter().map(|c| c.line()).collect();
        assert_eq!(
            lines[0],
            "scripts/fly-setup.sh --app app --region fra --host app.example.com"
        );
        assert_eq!(
            lines[1],
            "scripts/fly-setup.sh --app app --region fra --host app.example.com --set-gh-secret production"
        );
        assert!(r.calls()[..2].iter().all(|c| c.stdin.is_some()));
        assert!(
            r.calls()[..2]
                .iter()
                .all(|c| c.timeout == Some(std::time::Duration::from_secs(600)))
        );
        assert!(lines.iter().all(|l| !l.contains("--deploy")));
        assert!(lines[2].starts_with("fly auth whoami"));
    }

    #[test]
    fn script_exit_2_asks_for_a_hull_upgrade() {
        let r = all_ok().on(&["scripts/fly-setup.sh"], Out::with(2, "", "unknown flag"));
        let Check::Human(h) = run(&r, &dir(true), fly_manifest(), true) else {
            panic!()
        };
        assert!(h.title.contains("Upgrade the hull fly piece") && h.title.contains("--region"));
        let r = all_ok().on(&["scripts/fly-setup.sh"], Out::with(4, "", "boom"));
        assert!(
            matches!(run(&r, &dir(true), fly_manifest(), true), Check::Failed(m) if m.contains("boom"))
        );
    }

    #[test]
    fn script_exit_3_is_the_login_and_install_human_step() {
        let r = all_ok().on(&["scripts/fly-setup.sh"], Out::with(3, "", "no flyctl"));
        let Check::Human(h) = run(&r, &dir(true), fly_manifest(), true) else {
            panic!()
        };
        assert_eq!(h.id, "deploy");
        assert_eq!(h.title, "Log in to Fly.io and install flyctl/gh");
        assert_eq!(h.command, login_step().command);
    }
}
