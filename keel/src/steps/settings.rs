use crate::ctx::Ctx;
use crate::error::KeelError;
use crate::report::HumanStep;
use crate::step::{Check, Step};
use crate::tools::hoist::{self, Res};

pub struct Settings;

fn convert(res: Res) -> Check {
    match res {
        Res::Ok => Check::Done("hoist check: no drift".into()),
        Res::Drift => Check::Todo("hoist check reports drift".into()),
        Res::Human {
            title,
            url,
            command,
        } => Check::Human(HumanStep {
            id: "settings".into(),
            title,
            url,
            command,
        }),
        Res::Failed(e) => Check::Failed(e),
    }
}

impl Step for Settings {
    fn id(&self) -> &'static str {
        "settings"
    }

    fn check(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        Ok(convert(hoist::check(cx.runner, &cx.dir)))
    }

    fn apply(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        match hoist::apply(cx.runner, &cx.dir) {
            Res::Ok => self.check(cx),
            other => Ok(convert(other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;
    use crate::ctx::Opts;
    use crate::exec::{FakeRunner, Out};

    fn run(r: &FakeRunner, apply: bool) -> Check {
        let mut cx = ctx(
            r,
            ".".into(),
            manifest("rust"),
            PINS,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        if apply {
            Settings.apply(&mut cx)
        } else {
            Settings.check(&mut cx)
        }
        .unwrap()
    }

    #[test]
    fn check_exit_0_is_done_1_is_todo() {
        let r = FakeRunner::new().on(
            &["hoist", "--json", "check"],
            Out::ok(r#"{"ok":true,"exit_code":0,"items":[],"human_steps":[]}"#),
        );
        assert!(matches!(run(&r, false), Check::Done(_)));
        let r = FakeRunner::new().on(
            &["hoist", "--json", "check"],
            Out::with(1, r#"{"ok":false,"exit_code":1,"items":[]}"#, ""),
        );
        assert!(matches!(run(&r, false), Check::Todo(_)));
        let r = FakeRunner::new().on(&["hoist", "--json", "check"], Out::with(1, "boom", ""));
        assert!(matches!(run(&r, false), Check::Failed(m) if m.contains("hoist --json check")));
    }

    #[test]
    fn apply_uses_yes_and_rechecks() {
        let r = FakeRunner::new()
            .on(
                &["hoist", "--json", "apply"],
                Out::ok(r#"{"ok":true,"items":[]}"#),
            )
            .on(&["hoist", "--json", "check"], Out::ok(r#"{"ok":true}"#));
        assert!(matches!(run(&r, true), Check::Done(_)));
        assert_eq!(r.calls()[0].line(), "hoist --json apply --yes");
        assert_eq!(r.calls()[1].line(), "hoist --json check");
    }

    #[test]
    fn apply_exit_2_becomes_the_first_human_step() {
        let body = r#"{"ok":false,"exit_code":2,"items":[],"human_steps":[{"id":"app","title":"Install the hoist app","url":"https://github.com/apps/x","status":"pending"},{"id":"b","title":"second","url":"","status":"pending"}]}"#;
        let r = FakeRunner::new().on(&["hoist", "--json", "apply"], Out::with(2, body, ""));
        let Check::Human(h) = run(&r, true) else {
            panic!()
        };
        assert_eq!(
            (h.title.as_str(), h.url.as_str()),
            ("Install the hoist app", "https://github.com/apps/x")
        );
        assert_eq!(h.id, "settings");
        assert_eq!(h.command, "keel up");
    }

    #[test]
    fn variable_human_step_carries_the_gh_variable_set_command() {
        let body = r#"{"ok":false,"exit_code":2,"items":[{"area":"github","kind":"variable","key":"OTHER","action":"pending","reason":"gh variable set OTHER --body 'x'"},{"area":"github","kind":"variable","key":"SPLICE_BOT","action":"pending","reason":"missing; run: gh variable set SPLICE_BOT --repo o/r --body 'v1' (then rerun) "}],"human_steps":[{"id":"variable:SPLICE_BOT","title":"Set SPLICE_BOT","url":"https://github.com/o/r/settings/variables/actions","status":"pending"}]}"#;
        let r = FakeRunner::new().on(&["hoist", "--json", "apply"], Out::with(2, body, ""));
        let Check::Human(h) = run(&r, true) else {
            panic!()
        };
        assert_eq!(
            h.command,
            "gh variable set SPLICE_BOT --repo o/r --body 'v1' (then rerun)"
        );
        assert_eq!(h.title, "Set SPLICE_BOT");
        assert_eq!(h.url, "https://github.com/o/r/settings/variables/actions");
    }
}
