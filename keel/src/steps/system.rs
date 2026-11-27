//! `system`: the `keel doctor` check of `system.toml` (D-031): `hoist --json system check`.
//! Missing file: a warning item whose fix is `hoist system init`. Drift: one warning item each.
//! hoist not installed: skipped (preflight already reports the missing tool).
use crate::ctx::Ctx;
use crate::error::KeelError;
use crate::report::ReportItem;
use crate::step::{Check, Step};
use crate::tools::hoist;

pub struct System;

fn item(key: &str, action: &str, reason: String) -> ReportItem {
    ReportItem {
        step: "system".into(),
        area: "system".into(),
        kind: "system".into(),
        key: key.into(),
        action: action.into(),
        reason,
    }
}

impl Step for System {
    fn id(&self) -> &'static str {
        "system"
    }

    fn check(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        if !cx.dir.join("system.toml").is_file() {
            cx.items.push(item(
                "system.toml",
                "warning",
                "system.toml is missing; fix: hoist system init".into(),
            ));
            return Ok(Check::Todo(
                "system.toml is missing: run `hoist system init`".into(),
            ));
        }
        let p = match hoist::system_check(cx.runner, &cx.dir, cx.opts.tool_timeout()) {
            Ok(hoist::SystemCheck::Ran(p)) => p,
            Ok(hoist::SystemCheck::NotInstalled) => {
                return Ok(Check::Done(
                    "hoist not found: system.toml not checked".into(),
                ));
            }
            Ok(hoist::SystemCheck::TooOld) => {
                let m = "hoist too old; upgrade to a release that has `hoist system`";
                cx.items.push(item("hoist", "warning", m.into()));
                return Ok(Check::Done(format!("{m}: system.toml not checked")));
            }
            Err(e) => return Ok(Check::Failed(e)),
        };
        let bad: Vec<_> = p
            .items
            .iter()
            .filter(|i| !matches!(i.action.as_str(), "ok" | "create" | "update"))
            .collect();
        cx.items.extend(bad.iter().map(|i| {
            let mut r = item(&i.key, "warning", i.reason.clone());
            r.kind = i.kind.clone();
            r
        }));
        cx.pending.extend(p.humans.iter().cloned());
        Ok(match p.code {
            0 => Check::Done("system.toml is clean".into()),
            _ => Check::Todo(format!(
                "hoist system check reports {} item(s) to fix: see `items`",
                bad.len().max(p.humans.len())
            )),
        })
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

    fn run(r: &FakeRunner, with_file: bool) -> (Check, Vec<ReportItem>, usize) {
        let d = tempfile::tempdir().unwrap();
        if with_file {
            std::fs::write(d.path().join("system.toml"), "version = 1\n").unwrap();
        }
        let mut cx = ctx(
            r,
            d.path().into(),
            manifest("rust"),
            PINS,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        let c = System.check(&mut cx).unwrap();
        (c, cx.items.clone(), cx.pending.len())
    }

    #[test]
    fn missing_file_warns_with_the_init_fix_and_never_calls_hoist() {
        let r = FakeRunner::new();
        let (c, items, _) = run(&r, false);
        assert!(matches!(c, Check::Todo(m) if m.contains("hoist system init")));
        assert_eq!(items[0].action, "warning");
        assert!(items[0].reason.contains("fix: hoist system init"));
        assert!(r.calls().is_empty());
    }

    #[test]
    fn clean_check_is_done() {
        let r = FakeRunner::new().on(
            &["hoist", "--json", "system", "check"],
            Out::ok(r#"{"ok":true,"items":[{"area":"system","kind":"expiry","key":"a","action":"ok","reason":""}]}"#),
        );
        let (c, items, _) = run(&r, true);
        assert!(matches!(c, Check::Done(_)));
        assert!(items.is_empty());
    }

    #[test]
    fn drift_items_become_warnings_and_rotation_steps_are_pending() {
        let r = FakeRunner::new().on(
            &["hoist", "--json", "system", "check"],
            Out::with(
                1,
                r#"{"ok":false,"items":[{"area":"system","kind":"undeclared","key":"NPM_TOKEN","action":"drift","reason":"no credential"}],"human_steps":[{"id":"rotate:npm","title":"Rotate npm"}]}"#,
                "",
            ),
        );
        let (c, items, pending) = run(&r, true);
        assert!(matches!(c, Check::Todo(_)));
        assert_eq!(items.len(), 1);
        assert_eq!(
            (
                items[0].kind.as_str(),
                items[0].key.as_str(),
                items[0].action.as_str()
            ),
            ("undeclared", "NPM_TOKEN", "warning")
        );
        assert_eq!(pending, 1);
    }

    #[test]
    fn hoist_without_the_system_subcommand_warns_and_is_done() {
        let json_err =
            r#"{"ok":false,"error":{"message":"unknown subcommand `system`","fix":"x"}}"#;
        for out in [
            Out::with(
                3,
                "",
                "error: unrecognized subcommand 'system'\n\nUsage: hoist [OPTIONS] <COMMAND>",
            ),
            Out::with(3, json_err, ""),
            Out::with(2, "", "error: unrecognized subcommand 'system'"),
            Out::with(2, "Usage: hoist [OPTIONS]", ""),
        ] {
            let r = FakeRunner::new().on(&["hoist", "--json", "system", "check"], out);
            let (c, items, pending) = run(&r, true);
            assert!(
                matches!(&c, Check::Done(m) if m.contains("hoist too old")),
                "{c:?}"
            );
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].action, "warning");
            assert!(items[0].reason.contains("hoist too old"));
            assert!(items[0].reason.contains("hoist system"));
            assert_eq!(pending, 0);
        }
    }

    #[test]
    fn exit_2_without_usage_text_is_not_too_old() {
        for out in [Out::with(2, "", ""), Out::with(2, "garbled", "boom")] {
            let r = FakeRunner::new().on(&["hoist", "--json", "system", "check"], out);
            let (c, _, _) = run(&r, true);
            assert!(
                !matches!(&c, Check::Done(m) if m.contains("hoist too old")),
                "{c:?}"
            );
        }
    }

    #[test]
    fn absent_hoist_skips_and_a_config_error_fails() {
        let r = FakeRunner::new().on(
            &["hoist", "--json", "system", "check"],
            Out::with(127, "", ""),
        );
        assert!(matches!(run(&r, true).0, Check::Done(m) if m.contains("not found")));
        let r = FakeRunner::new().on(
            &["hoist", "--json", "system", "check"],
            Out::with(
                3,
                r#"{"ok":false,"error":{"message":"unknown key","fix":"edit system.toml"}}"#,
                "",
            ),
        );
        assert!(matches!(run(&r, true).0, Check::Failed(m) if m.contains("unknown key")));
    }
}
