use super::repo;
use crate::ctx::Ctx;
use crate::error::KeelError;
use crate::report::HumanStep;
use crate::step::{Check, Step};
use crate::tools::gh;
use std::time::Duration;

pub struct Apps;

/// `(app slug, display name)`, in the order keel asks for them.
const APPS: [(&str, &str); 2] = [("claude", "Claude"), ("renovate", "Renovate")];

impl Apps {
    fn once(&self, cx: &Ctx, repo: &str) -> Check {
        let slugs: Vec<&str> = APPS.iter().map(|a| a.0).collect();
        let found = match gh::installed_apps(cx.runner, repo, &slugs) {
            Ok(f) => f,
            Err(e) => return Check::Failed(e),
        };
        match APPS
            .iter()
            .find(|(slug, _)| !found.iter().any(|f| f == slug))
        {
            None => Check::Done(format!("Claude and Renovate apps installed on {repo}")),
            Some((slug, name)) => Check::Human(HumanStep {
                id: self.id().into(),
                title: format!("Install the {name} GitHub App on {repo}"),
                url: format!("https://github.com/apps/{slug}/installations/new"),
                command: "keel up".into(),
            }),
        }
    }
}

impl Step for Apps {
    fn id(&self) -> &'static str {
        "apps"
    }

    /// Read-only; with `--wait` it polls (via `cx.sleep`) up to `wait/poll` more times, except in
    /// a dry run.
    fn check(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let repo = repo(cx)?;
        let mut res = self.once(cx, &repo);
        if cx.opts.dry_run {
            return Ok(res);
        }
        let poll = cx.opts.poll_secs.max(1);
        for _ in 0..cx.opts.wait_secs / poll {
            if !matches!(res, Check::Human(_)) {
                break;
            }
            (cx.sleep)(Duration::from_secs(poll));
            res = self.once(cx, &repo);
        }
        Ok(res)
    }

    fn apply(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        self.check(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;
    use crate::ctx::Opts;
    use crate::exec::{FakeRunner, Out};
    use std::cell::Cell;
    use std::path::PathBuf;

    const INST: &str = "user/installations";

    fn both_all() -> String {
        "{\"id\":1,\"app_slug\":\"claude\",\"repository_selection\":\"all\"}\n\
         {\"id\":2,\"app_slug\":\"renovate\",\"repository_selection\":\"all\"}\n"
            .into()
    }

    fn run(r: &FakeRunner, opts: Opts, sleep: &dyn Fn(Duration)) -> Check {
        let mut cx = ctx(
            r,
            PathBuf::from("."),
            manifest("rust"),
            PINS,
            opts,
            sleep,
            &no_stdin,
        );
        Apps.check(&mut cx).unwrap()
    }

    #[test]
    fn both_installed_is_done() {
        let r = FakeRunner::new().on(&["gh", "api", INST], Out::ok(&both_all()));
        assert!(matches!(
            run(&r, Opts::default(), &no_sleep),
            Check::Done(_)
        ));
    }

    #[test]
    fn missing_app_is_human_with_link_and_polls() {
        let r = FakeRunner::new().on(
            &["gh", "api", INST],
            Out::ok("{\"id\":1,\"app_slug\":\"claude\",\"repository_selection\":\"all\"}\n"),
        );
        let slept = Cell::new(0u32);
        let sleep = |d: Duration| {
            assert_eq!(d, Duration::from_secs(10));
            slept.set(slept.get() + 1);
        };
        let opts = Opts {
            wait_secs: 30,
            poll_secs: 10,
            ..Opts::default()
        };
        let Check::Human(h) = run(&r, opts, &sleep) else {
            panic!()
        };
        assert_eq!(h.url, "https://github.com/apps/renovate/installations/new");
        assert!(h.title.contains("Renovate") && h.title.contains("owner/name"));
        assert_eq!(slept.get(), 3);
        assert_eq!(r.calls().len(), 4);
    }

    #[test]
    fn wait_zero_checks_once_and_dry_run_never_sleeps() {
        let r = FakeRunner::new().on(&["gh", "api", INST], Out::ok(""));
        let sleep = |_: Duration| panic!("must not sleep");
        assert!(matches!(run(&r, Opts::default(), &sleep), Check::Human(_)));
        let opts = Opts {
            wait_secs: 30,
            poll_secs: 10,
            dry_run: true,
            ..Opts::default()
        };
        assert!(matches!(run(&r, opts, &sleep), Check::Human(_)));
    }

    #[test]
    fn selected_installation_must_list_the_repo() {
        let inst = "{\"id\":7,\"app_slug\":\"claude\",\"repository_selection\":\"selected\"}\n\
                    {\"id\":8,\"app_slug\":\"renovate\",\"repository_selection\":\"all\"}\n";
        let r = FakeRunner::new()
            .on(&["gh", "api", INST], Out::ok(inst))
            .on(
                &["gh", "api", "user/installations/7/repositories"],
                Out::ok("owner/other\n"),
            );
        let Check::Human(h) = run(&r, Opts::default(), &no_sleep) else {
            panic!()
        };
        assert!(h.title.contains("Claude"));
    }

    #[test]
    fn selected_installation_with_repo_is_done() {
        let inst = "{\"id\":7,\"app_slug\":\"claude\",\"repository_selection\":\"selected\"}\n\
                    {\"id\":8,\"app_slug\":\"renovate\",\"repository_selection\":\"all\"}\n";
        let r = FakeRunner::new()
            .on(&["gh", "api", INST], Out::ok(inst))
            .on(
                &["gh", "api", "user/installations/7/repositories"],
                Out::ok("owner/other\nOwner/Name\n"),
            );
        assert!(matches!(
            run(&r, Opts::default(), &no_sleep),
            Check::Done(_)
        ));
    }
}
