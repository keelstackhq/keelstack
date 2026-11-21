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
        Res::Upgrade => Check::Failed("hoist does not take --area; upgrade hoist".into()),
    }
}

impl Step for Settings {
    fn id(&self) -> &'static str {
        "settings"
    }

    fn check(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        Ok(convert(hoist::check_settings(
            cx.runner,
            &cx.dir,
            cx.opts.tool_timeout(),
        )))
    }

    fn apply(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        match hoist::apply_settings(cx.runner, &cx.dir, cx.opts.tool_timeout()) {
            Res::Ok => self.check(cx),
            other => Ok(convert(other)),
        }
    }
}

/// `settings` for `template = "none"` (an existing repo): hoist over all its areas. With
/// `CLOUDFLARE_API_TOKEN` set (presence only) no `--area`; without it, when `hoist explain` lists
/// cloudflare items, every area but `cloudflare` plus the `cloudflare-token` checklist entry.
/// Every hoist human step goes to the checklist, not only the first.
pub struct SettingsAll;

impl SettingsAll {
    /// Run hoist, record its items and the pending human steps in `cx`; the decision is `Parsed`.
    fn read(cx: &mut Ctx, apply: bool) -> Result<Result<hoist::Parsed, String>, KeelError> {
        let t = cx.opts.tool_timeout();
        let mut skipped: Vec<hoist::Item> = Vec::new();
        if !cx.opts.cloudflare_token {
            match hoist::explain_cloudflare(cx.runner, &cx.dir, t) {
                Ok(i) => skipped = i,
                Err(e) => return Ok(Err(e)),
            }
        }
        let all = cx.opts.cloudflare_token || skipped.is_empty();
        let p = match hoist::run_all(cx.runner, &cx.dir, apply, all, t) {
            Ok(p) => p,
            Err(e) => return Ok(Err(e)),
        };
        cx.items = p
            .items
            .iter()
            .chain(skipped.iter())
            .map(|i| crate::report::ReportItem {
                step: "settings".into(),
                area: i.area.clone(),
                kind: i.kind.clone(),
                key: i.key.clone(),
                action: i.action.clone(),
                reason: i.reason.clone(),
            })
            .collect();
        cx.pending.clear();
        if !skipped.is_empty() {
            cx.pending.push(cloudflare_token_entry(&skipped));
        }
        // Only trust steps (hoist cannot read them back) are filtered by confirmations.
        let confirmed = &cx.manifest.confirmed.steps;
        cx.pending.extend(
            p.humans
                .iter()
                .filter(|h| !(is_trust(&h.id) && confirmed.contains(&h.id)))
                .cloned(),
        );
        Ok(Ok(p))
    }

    fn decide(cx: &Ctx, p: &hoist::Parsed) -> Check {
        if let Some(first) = cx.pending.first()
            && p.code != 1
        {
            return Check::Human(first.clone());
        }
        match p.code {
            0 => Check::Done("hoist check: no drift".into()),
            1 => {
                let why = drift_reasons(p);
                Check::Todo(if why.is_empty() {
                    "hoist check reports drift".into()
                } else {
                    format!("hoist check reports drift: {why}")
                })
            }
            _ if p.humans.is_empty() => Check::Failed(
                "`hoist` exited 2 without a human step; run it by hand to see why".into(),
            ),
            _ => Check::Done("hoist: only confirmed human steps remain".into()),
        }
    }
}

fn is_trust(id: &str) -> bool {
    id.starts_with("npm-trust:") || id.starts_with("crates-trust:")
}

/// "kind/key: reason" for the first few items that are not in sync.
fn drift_reasons(p: &hoist::Parsed) -> String {
    let lines: Vec<String> = p
        .items
        .iter()
        .filter(|i| !matches!(i.action.as_str(), "ok" | "unknown"))
        .take(3)
        .map(|i| {
            let k = if i.key.is_empty() {
                i.kind.clone()
            } else {
                format!("{}/{}", i.kind, i.key)
            };
            format!("{k}: {}", i.reason)
        })
        .collect();
    lines.join("; ")
}

/// The checklist entry for the cloudflare items hoist would handle with the token.
fn cloudflare_token_entry(items: &[hoist::Item]) -> HumanStep {
    let mut keys: Vec<&str> = Vec::new();
    for i in items {
        let k = if i.key.is_empty() { &i.kind } else { &i.key };
        if !keys.contains(&k.as_str()) {
            keys.push(k);
        }
    }
    HumanStep {
        id: "cloudflare-token".into(),
        title: format!(
            "Export CLOUDFLARE_API_TOKEN (Pages Write + Account API Tokens Write) and CLOUDFLARE_ACCOUNT_ID for the Cloudflare items: {}",
            keys.join(", ")
        ),
        url: "https://dash.cloudflare.com/profile/api-tokens".into(),
        command: "export CLOUDFLARE_API_TOKEN=... CLOUDFLARE_ACCOUNT_ID=... then keel up".into(),
    }
}

impl Step for SettingsAll {
    fn id(&self) -> &'static str {
        "settings"
    }

    fn check(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        Ok(match SettingsAll::read(cx, false)? {
            Ok(p) => SettingsAll::decide(cx, &p),
            Err(e) => Check::Failed(e),
        })
    }

    fn apply(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        match SettingsAll::read(cx, true)? {
            Err(e) => Ok(Check::Failed(e)),
            Ok(p) if p.code == 0 => self.check(cx),
            // Exit 2 hides drift behind the human steps: when all of them are confirmed, only a
            // clean re-check is done.
            Ok(p) if p.code == 2 => match SettingsAll::decide(cx, &p) {
                Check::Done(_) => self.check(cx),
                other => Ok(other),
            },
            Ok(p) => {
                let why = drift_reasons(&p);
                Ok(Check::Failed(if why.is_empty() {
                    "`hoist --json apply --yes` exited 1; run it by hand to see why".into()
                } else {
                    format!("`hoist --json apply --yes` exited 1: {why}")
                }))
            }
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
        assert_eq!(
            r.calls()[0].line(),
            "hoist --json apply --yes --area github --area npm --area crates --area release"
        );
        assert_eq!(
            r.calls()[1].line(),
            "hoist --json check --area github --area npm --area crates --area release"
        );
    }

    #[test]
    fn leaves_cloudflare_out_and_retries_when_hoist_rejects_the_flag() {
        let r = FakeRunner::new()
            .on(
                &["hoist", "--json", "check", "--area"],
                Out::with(2, "", "error: unexpected argument '--area'"),
            )
            .on(
                &["hoist", "--json", "check"],
                Out::ok(r#"{"ok":true,"items":[]}"#),
            );
        assert!(matches!(run(&r, false), Check::Done(_)));
        let lines: Vec<String> = r.calls().iter().map(|c| c.line()).collect();
        assert_eq!(
            lines,
            [
                "hoist --json check --area github --area npm --area crates --area release",
                "hoist --json check"
            ]
        );
        // Exit 2 with hoist JSON is a human step, not a rejected flag: no retry.
        let body = r#"{"ok":false,"items":[],"human_steps":[{"id":"a","title":"t","url":"u"}]}"#;
        let r = FakeRunner::new().on(&["hoist", "--json", "apply"], Out::with(2, body, ""));
        assert!(matches!(run(&r, true), Check::Human(_)));
        assert_eq!(r.calls().len(), 1);
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
    fn deploy_keys_policy_step_from_hoist_apply_is_a_human_step() {
        let body = r#"{"ok":false,"exit_code":2,"items":[{"area":"github","kind":"tag_deploy_key","key":"hoist: release tags","action":"pending","reason":"allow deploy keys for o/r (organization settings), then re-run keel up"}],"human_steps":[{"id":"deploy-keys-policy","title":"allow deploy keys for o/r (organization settings), then re-run keel up","url":"https://github.com/organizations/o/settings","status":"pending"}]}"#;
        let r = FakeRunner::new().on(&["hoist", "--json", "apply"], Out::with(2, body, ""));
        let Check::Human(h) = run(&r, true) else {
            panic!("expected a human step")
        };
        assert!(
            h.title.starts_with("allow deploy keys for o/r"),
            "{}",
            h.title
        );
        assert_eq!(h.url, "https://github.com/organizations/o/settings");
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

    fn none_ctx<'a>(r: &'a FakeRunner, token: bool, confirmed: &[&str]) -> crate::ctx::Ctx<'a> {
        let mut m = manifest("none");
        m.pieces.clear();
        m.confirmed.steps = confirmed.iter().map(|s| s.to_string()).collect();
        let mut cx = ctx(
            r,
            ".".into(),
            m,
            PINS,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        cx.opts.cloudflare_token = token;
        cx
    }

    const BODY: &str = r#"{"items":[{"area":"cloudflare","kind":"pages_project","key":"site-x","action":"create","reason":"r"},{"area":"github","kind":"secret","key":"K","action":"pending","reason":"r"}],"human_steps":[{"id":"secret:K","title":"Set K: `hoist secrets set K`","url":"u"},{"id":"npm-trust:p","title":"Trust p","url":"v"}]}"#;

    #[test]
    fn none_mode_collects_every_human_step_and_skips_cloudflare_without_a_token() {
        let r = FakeRunner::new()
            .on(&["hoist", "--json", "explain"], Out::ok(BODY))
            .on(&["hoist", "--json", "check"], Out::with(2, BODY, ""));
        let mut cx = none_ctx(&r, false, &["npm-trust:p"]);
        let Check::Human(h) = SettingsAll.check(&mut cx).unwrap() else {
            panic!()
        };
        assert_eq!(h.id, "cloudflare-token");
        assert!(h.title.contains("site-x"), "{}", h.title);
        let ids: Vec<&str> = cx.pending.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["cloudflare-token", "secret:K"]);
        assert_eq!(cx.pending[1].command, "hoist secrets set K");
        assert_eq!(cx.items.len(), 3);
        assert_eq!(cx.items[0].step, "settings");
        assert_eq!(
            r.calls()[1].line(),
            "hoist --json check --area github --area npm --area crates --area release"
        );
        // With the token: no --area, no explain call.
        let r = FakeRunner::new().on(&["hoist", "--json", "check"], Out::with(2, BODY, ""));
        let mut cx = none_ctx(&r, true, &[]);
        SettingsAll.check(&mut cx).unwrap();
        assert_eq!(r.calls().len(), 1);
        assert_eq!(r.calls()[0].line(), "hoist --json check");
        assert_eq!(cx.pending.len(), 2);
    }

    #[test]
    fn confirmations_filter_only_trust_steps_and_never_keels_own_ids() {
        let r = FakeRunner::new()
            .on(&["hoist", "--json", "explain"], Out::ok(BODY))
            .on(&["hoist", "--json", "check"], Out::with(2, BODY, ""));
        let mut cx = none_ctx(
            &r,
            false,
            &["npm-trust:q", "npm-trust:p", "cloudflare-token"],
        );
        SettingsAll.check(&mut cx).unwrap();
        let ids: Vec<&str> = cx.pending.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["cloudflare-token", "secret:K"]);
    }

    #[test]
    fn apply_exit_1_lists_the_drift_reasons() {
        let body = r#"{"items":[{"area":"github","kind":"ruleset","key":"main","action":"drift","reason":"bypass differs"},{"area":"github","kind":"x","key":"","action":"ok","reason":"fine"}],"human_steps":[]}"#;
        let r = FakeRunner::new().on(&["hoist"], Out::with(1, body, ""));
        let mut cx = none_ctx(&r, true, &[]);
        let Check::Failed(m) = SettingsAll.apply(&mut cx).unwrap() else {
            panic!()
        };
        assert!(m.contains("ruleset/main: bypass differs"), "{m}");
        assert!(!m.contains("fine"), "{m}");
    }

    #[test]
    fn none_mode_exit_codes() {
        let run = |code: i32, body: &str, confirmed: &[&str]| {
            let r = FakeRunner::new().on(&["hoist"], Out::with(code, body, ""));
            let mut cx = none_ctx(&r, true, confirmed);
            SettingsAll.check(&mut cx).unwrap()
        };
        let none = r#"{"items":[],"human_steps":[]}"#;
        assert!(matches!(run(0, none, &[]), Check::Done(_)));
        assert!(matches!(run(1, BODY, &[]), Check::Todo(_)));
        assert!(matches!(run(0, BODY, &[]), Check::Human(_)));
        assert!(matches!(run(2, none, &[]), Check::Failed(_)));
        assert!(matches!(run(0, BODY, &["npm-trust:p"]), Check::Human(_)));
        assert!(matches!(run(2, BODY, &["npm-trust:p"]), Check::Human(_)));
        assert!(matches!(run(3, "", &[]), Check::Failed(_)));
    }

    #[test]
    fn apply_exit_2_with_every_human_step_confirmed_is_done_only_after_a_clean_recheck() {
        let all = ["npm-trust:p", "npm-trust:q"];
        let trust_only =
            r#"{"items":[],"human_steps":[{"id":"npm-trust:p","title":"Trust p","url":"v"}]}"#;
        let drift = r#"{"items":[{"area":"github","kind":"ruleset","key":"main","action":"drift","reason":"bypass differs"}],"human_steps":[]}"#;
        let apply = |check: Out, confirmed: &[&str]| {
            let r = FakeRunner::new()
                .on(&["hoist", "--json", "apply"], Out::with(2, trust_only, ""))
                .on(&["hoist", "--json", "check"], check);
            let mut cx = none_ctx(&r, true, confirmed);
            let c = SettingsAll.apply(&mut cx).unwrap();
            (c, r.calls().iter().map(|c| c.line()).collect::<Vec<_>>())
        };
        // Check still exits 2 with only confirmed trust steps: done, after apply and a re-check.
        let (c, calls) = apply(Out::with(2, trust_only, ""), &all);
        assert!(matches!(c, Check::Done(_)), "{c:?}");
        assert_eq!(calls, ["hoist --json apply --yes", "hoist --json check"]);
        // The re-check finds drift: not done.
        let (c, calls) = apply(Out::with(1, drift, ""), &all);
        assert!(
            matches!(&c, Check::Todo(m) if m.contains("ruleset/main")),
            "{c:?}"
        );
        assert_eq!(calls.len(), 2);
        // The re-check fails: failed.
        let (c, _) = apply(Out::with(3, "", ""), &all);
        assert!(matches!(c, Check::Failed(_)), "{c:?}");
        // Not all confirmed: the human step, no re-check.
        let (c, calls) = apply(Out::with(2, trust_only, ""), &[]);
        assert!(matches!(c, Check::Human(_)), "{c:?}");
        assert_eq!(calls, ["hoist --json apply --yes"]);
    }
}
