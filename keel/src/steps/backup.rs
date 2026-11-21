use super::{cloudflare_token_step, hoist_check};
use crate::ctx::Ctx;
use crate::error::KeelError;
use crate::manifest::{Backup as B, Provider};
use crate::report::HumanStep;
use crate::step::{Check, Step};
use crate::tools::hoist;

pub struct Backup;

const SECTION: &str = "cloudflare.r2";

/// `[cloudflare.r2]`: hoist makes the bucket and its lifecycle rule and a key with a fixed scope,
/// and delivers R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY, R2_BUCKET and R2_ENDPOINT to the fly app
/// (`apply --secrets-to fly:APP`; keel does no name mapping). The key never passes through keel.
/// The account id is not a key of this table (see [`account`]).
pub(crate) fn section_text(bucket: &str, days: u32) -> String {
    format!(
        "[{SECTION}]\nbucket = {}\nretention_days = {days}\n",
        hoist::toml_str(bucket)
    )
}

enum Account {
    /// hoist reads `CLOUDFLARE_ACCOUNT_ID` itself, or `hoist.toml` already has it.
    Have,
    /// Write `[cloudflare] account_id` from `[backup] account_id`.
    Write(String),
}

/// Env `CLOUDFLARE_ACCOUNT_ID` (nothing to write), else `[backup] account_id` (to write into
/// `[cloudflare]`), else the `[cloudflare] account_id` already in `hoist.toml`; else the human step.
fn account(cx: &Ctx, b: &B) -> Result<Account, Check> {
    let set = |v: &Option<String>| v.clone().filter(|v| !v.trim().is_empty());
    if let Some(id) = set(&cx.opts.cloudflare_account_id) {
        if !crate::manifest::valid_account_id(&id) {
            return Err(Check::Failed(
                "env CLOUDFLARE_ACCOUNT_ID must be the 32-character lowercase hex Cloudflare account id: fix or unset it".into(),
            ));
        }
        return Ok(Account::Have);
    }
    if let Some(id) = set(&b.account_id) {
        return Ok(Account::Write(id));
    }
    if hoist::has_account_id(&cx.dir) {
        return Ok(Account::Have);
    }
    Err(Check::Human(HumanStep {
        id: "backup".into(),
        title: "Set [backup] account_id (Cloudflare account id) in keel.toml".into(),
        url: "https://dash.cloudflare.com/".into(),
        command: "keel up".into(),
    }))
}

impl Step for Backup {
    fn id(&self) -> &'static str {
        "backup"
    }

    fn check(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let (Some(b), Some(f)) = (cx.manifest.backup.clone(), cx.manifest.fly().cloned()) else {
            return Ok(Check::Done("no [backup]".into()));
        };
        if cx.manifest.confirmed.backup.as_deref() == Some(f.app.as_str()) {
            return Ok(Check::Done(
                "backup bucket confirmed as created by hand".into(),
            ));
        }
        if b.provider == Provider::S3 {
            return Ok(Check::Done(format!(
                "s3 backup is not automated: create the bucket and set the secrets on fly app {} by hand",
                f.app
            )));
        }
        if !cx.opts.cloudflare_token {
            let what = format!("create the R2 bucket {} by hand", B::bucket(&f.app));
            return Ok(cloudflare_token_step(self.id(), &what));
        }
        if let Err(h) = account(cx, &b) {
            return Ok(h);
        }
        if !hoist::has_section(&cx.dir, SECTION) {
            return Ok(Check::Todo(format!(
                "add [{SECTION}] to hoist.toml and apply it (bucket {})",
                B::bucket(&f.app)
            )));
        }
        let res = hoist::check_cloudflare(cx.runner, &cx.dir, &hoist::R2, cx.opts.tool_timeout());
        Ok(hoist_check(
            res,
            self.id(),
            SECTION,
            &format!("R2 bucket {}, key in fly:{}", B::bucket(&f.app), f.app),
        ))
    }

    fn apply(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let (Some(b), Some(f)) = (cx.manifest.backup.clone(), cx.manifest.fly().cloned()) else {
            return self.check(cx);
        };
        match account(cx, &b) {
            Err(h) => return Ok(h),
            Ok(Account::Write(id)) => {
                if let Err(e) = hoist::ensure_account_id(&cx.dir, &id) {
                    return Ok(Check::Failed(e));
                }
            }
            Ok(Account::Have) => {}
        }
        if !hoist::has_section(&cx.dir, SECTION) {
            let text = section_text(&B::bucket(&f.app), b.retention_days);
            if let Err(e) = hoist::append_section(&cx.dir, &text) {
                return Ok(Check::Failed(e));
            }
        }
        let res = hoist::apply_cloudflare(cx.runner, &cx.dir, &f.app, cx.opts.tool_timeout());
        match hoist_check(res, self.id(), SECTION, "applied") {
            Check::Done(_) => self.check(cx),
            other => Ok(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::deploy::tests::fly_manifest;
    use super::super::testutil::*;
    use super::*;
    use crate::ctx::Opts;
    use crate::exec::{FakeRunner, Out};
    use crate::manifest::{Backup as B, Manifest};

    const ACC: &str = "0123456789abcdef0123456789abcdef";

    fn with_backup(p: Provider) -> Manifest {
        let mut m = fly_manifest();
        m.backup = Some(B {
            provider: p,
            retention_days: 14,
            account_id: Some(ACC.into()),
        });
        m
    }

    fn run(r: &FakeRunner, d: &std::path::Path, m: Manifest, token: bool, apply: bool) -> Check {
        let opts = Opts {
            cloudflare_token: token,
            ..Opts::default()
        };
        let mut cx = ctx(r, d.into(), m, PINS, opts, &no_sleep, &no_stdin);
        if apply {
            Backup.apply(&mut cx)
        } else {
            Backup.check(&mut cx)
        }
        .unwrap()
    }

    #[test]
    fn no_backup_is_done() {
        let d = tempfile::tempdir().unwrap();
        let c = run(&FakeRunner::new(), d.path(), fly_manifest(), true, false);
        assert_eq!(c, Check::Done("no [backup]".into()));
    }

    #[test]
    fn s3_is_done_with_a_by_hand_warning() {
        let d = tempfile::tempdir().unwrap();
        let Check::Done(m) = run(
            &FakeRunner::new(),
            d.path(),
            with_backup(Provider::S3),
            false,
            false,
        ) else {
            panic!()
        };
        assert!(
            m.contains("not automated") && m.contains("fly app app"),
            "{m}"
        );
    }

    #[test]
    fn r2_without_token_is_human() {
        let d = tempfile::tempdir().unwrap();
        let Check::Human(h) = run(
            &FakeRunner::new(),
            d.path(),
            with_backup(Provider::R2),
            false,
            false,
        ) else {
            panic!()
        };
        assert!(
            h.title.starts_with("Set CLOUDFLARE_API_TOKEN or"),
            "{}",
            h.title
        );
        assert_eq!(h.id, "backup");
        assert!(
            h.command.contains("keel up --confirm backup"),
            "{}",
            h.command
        );
    }

    #[test]
    fn confirmed_backup_is_done() {
        let d = tempfile::tempdir().unwrap();
        let mut m = with_backup(Provider::R2);
        m.confirmed.backup = Some("app".into());
        let c = run(&FakeRunner::new(), d.path(), m.clone(), false, false);
        assert!(matches!(c, Check::Done(m) if m.contains("confirmed")));
        // Confirmed for another app: void.
        m.confirmed.backup = Some("old-app".into());
        let c = run(&FakeRunner::new(), d.path(), m, false, false);
        assert!(matches!(c, Check::Human(_)), "{c:?}");
    }

    #[test]
    fn apply_writes_the_r2_section_and_applies_the_cloudflare_area_with_the_sink() {
        let d = tempfile::tempdir().unwrap();
        let r = FakeRunner::new()
            .on(
                &["hoist", "--json", "apply"],
                Out::ok(r#"{"ok":true,"items":[]}"#),
            )
            .on(&["hoist", "--json", "check"], Out::ok(r#"{"items":[{"kind":"r2_bucket","key":"b","action":"ok"},{"kind":"r2_lifecycle","key":"l","action":"ok"},{"kind":"r2_token","key":"t","action":"ok"}]}"#));
        assert!(matches!(
            run(&r, d.path(), with_backup(Provider::R2), true, true),
            Check::Done(_)
        ));
        let t = std::fs::read_to_string(d.path().join("hoist.toml")).unwrap();
        assert_eq!(
            t,
            format!(
                "[cloudflare]\naccount_id = \"{ACC}\"\n\n[cloudflare.r2]\nbucket = \"app-backup\"\nretention_days = 14\n"
            )
        );
        assert_eq!(
            r.calls()[0].line(),
            "hoist --json apply --yes --area cloudflare --secrets-to fly:app"
        );
        assert_eq!(r.calls()[1].line(), "hoist --json check --area cloudflare");
        // The check is read-only: no section, no write.
        let d2 = tempfile::tempdir().unwrap();
        let c = run(
            &FakeRunner::new(),
            d2.path(),
            with_backup(Provider::R2),
            true,
            false,
        );
        assert!(matches!(c, Check::Todo(_)));
        assert!(!d2.path().join("hoist.toml").exists());
    }

    #[test]
    fn the_bucket_name_is_the_lowercased_app() {
        let d = tempfile::tempdir().unwrap();
        let mut m = with_backup(Provider::R2);
        m.deploy.as_mut().unwrap().fly.as_mut().unwrap().app = "My-App".into();
        let r = FakeRunner::new().on(&["hoist"], Out::ok(r#"{"ok":true,"items":[]}"#));
        run(&r, d.path(), m, true, true);
        let t = std::fs::read_to_string(d.path().join("hoist.toml")).unwrap();
        assert!(t.contains("bucket = \"my-app-backup\""), "{t}");
        assert!(r.calls()[0].line().ends_with("--secrets-to fly:My-App"));
    }

    #[test]
    fn missing_account_id_is_a_human_step_and_the_env_value_is_used_when_the_manifest_has_none() {
        let mut m = with_backup(Provider::R2);
        m.backup.as_mut().unwrap().account_id = None;
        for apply in [false, true] {
            let d = tempfile::tempdir().unwrap();
            let r = FakeRunner::new();
            let Check::Human(h) = run(&r, d.path(), m.clone(), true, apply) else {
                panic!()
            };
            assert_eq!(
                h.title,
                "Set [backup] account_id (Cloudflare account id) in keel.toml"
            );
            assert_eq!((h.id.as_str(), h.command.as_str()), ("backup", "keel up"));
            assert!(r.calls().is_empty() && !d.path().join("hoist.toml").exists());
        }
        let d = tempfile::tempdir().unwrap();
        let opts = Opts {
            cloudflare_token: true,
            cloudflare_account_id: Some(ACC.into()),
            ..Opts::default()
        };
        let r = FakeRunner::new().on(&["hoist"], Out::ok(r#"{"ok":true,"items":[]}"#));
        let m2 = m.clone();
        let mut cx = ctx(&r, d.path().into(), m, PINS, opts, &no_sleep, &no_stdin);
        Backup.apply(&mut cx).unwrap();
        let t = std::fs::read_to_string(d.path().join("hoist.toml")).unwrap();
        assert!(!t.contains("account_id"), "{t}");
        assert!(t.contains("[cloudflare.r2]"), "{t}");
        // hoist.toml's own [cloudflare] account_id also satisfies it.
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("hoist.toml"),
            "[cloudflare]\naccount_id = \"x\"\n",
        )
        .unwrap();
        let r = FakeRunner::new().on(&["hoist"], Out::ok(r#"{"items":[{"kind":"r2_bucket","key":"b","action":"ok"},{"kind":"r2_lifecycle","key":"l","action":"ok"},{"kind":"r2_token","key":"t","action":"ok"}]}"#));
        assert!(matches!(run(&r, d.path(), m2, true, true), Check::Done(_)));
    }

    #[test]
    fn a_malformed_env_account_id_fails_naming_the_variable() {
        for apply in [false, true] {
            let d = tempfile::tempdir().unwrap();
            let r = FakeRunner::new();
            let opts = Opts {
                cloudflare_token: true,
                cloudflare_account_id: Some("a\"b".into()),
                ..Opts::default()
            };
            let mut cx = ctx(
                &r,
                d.path().into(),
                with_backup(Provider::R2),
                PINS,
                opts,
                &no_sleep,
                &no_stdin,
            );
            let c = if apply {
                Backup.apply(&mut cx)
            } else {
                Backup.check(&mut cx)
            }
            .unwrap();
            assert!(
                matches!(&c, Check::Failed(m) if m.contains("CLOUDFLARE_ACCOUNT_ID")),
                "{c:?}"
            );
            assert!(r.calls().is_empty() && !d.path().join("hoist.toml").exists());
        }
    }

    #[test]
    fn apply_exit_3_is_failed() {
        let d = tempfile::tempdir().unwrap();
        let r = FakeRunner::new().on(&["hoist"], Out::with(3, "", "r2 key needs a sink"));
        let c = run(&r, d.path(), with_backup(Provider::R2), true, true);
        assert!(matches!(c, Check::Failed(m) if m.contains("hoist --json apply")));
    }

    #[test]
    fn check_reads_only_the_r2_kinds_out_of_the_cloudflare_area() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("hoist.toml"), "[cloudflare.r2]\n").unwrap();
        let chk = |o: Out| {
            let r = FakeRunner::new().on(&["hoist"], o);
            run(&r, d.path(), with_backup(Provider::R2), true, false)
        };
        let dns = r#"{"items":[{"kind":"dns_record","key":"k","action":"create"}]}"#;
        assert!(matches!(chk(Out::with(1, dns, "")), Check::Todo(_)));
        let human =
            r#"{"items":[],"human_steps":[{"id":"r2:x","title":"Make a token","url":"u"}]}"#;
        let Check::Human(h) = chk(Out::with(2, human, "")) else {
            panic!()
        };
        assert_eq!(
            (h.id.as_str(), h.title.as_str()),
            ("backup", "Make a token")
        );
        let tok =
            r#"{"items":[{"kind":"r2_token","key":"hoist:o/r:r2:app-backup","action":"create"}]}"#;
        assert!(matches!(chk(Out::with(1, tok, "")), Check::Todo(_)));
    }

    #[test]
    fn old_hoist_is_an_upgrade_human_step() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("hoist.toml"), "[cloudflare.r2]\n").unwrap();
        let r = FakeRunner::new().on(&["hoist"], Out::with(2, "", "unexpected argument '--area'"));
        let Check::Human(h) = run(&r, d.path(), with_backup(Provider::R2), true, false) else {
            panic!()
        };
        assert!(h.title.contains("[cloudflare.r2]"));
    }
}
