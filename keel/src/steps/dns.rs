use super::{cloudflare_token_step, hoist_check};
use crate::ctx::Ctx;
use crate::error::KeelError;
use crate::manifest::{Dns as Provider, Fly};
use crate::report::HumanStep;
use crate::step::{Check, Step};
use crate::tools::fly::{self, Ips};
use crate::tools::hoist;

pub struct Dns;

const SECTION: &str = "cloudflare.dns";

/// `(type, content)` of the records for the app's addresses.
fn records(ips: &Ips) -> Vec<(&'static str, String)> {
    let a = ips.v4.iter().map(|i| ("A", i.clone()));
    let aaaa = ips.v6.iter().map(|i| ("AAAA", i.clone()));
    a.chain(aaaa).collect()
}

/// The zone of the host: the manifest's `zone`, else the last two labels of the host (wrong for
/// public suffixes such as `co.uk`: set `zone`).
fn zone(f: &Fly) -> String {
    f.zone.clone().unwrap_or_else(|| {
        let labels: Vec<&str> = f.host.split('.').collect();
        labels[labels.len().saturating_sub(2)..].join(".")
    })
}

/// hoist resolves the targets itself (`targets = "fly"` reads the fly app's addresses), so the
/// section carries no addresses.
pub(crate) fn section_text(f: &Fly) -> String {
    let no_mail = if f.no_mail { "no_mail = true\n" } else { "" };
    format!(
        "[{SECTION}]\nzone = {}\nhost = {}\ntargets = \"fly\"\nfly_app = {}\nproxied = false\n{no_mail}",
        hoist::toml_str(&zone(f)),
        hoist::toml_str(&f.host),
        hoist::toml_str(&f.app)
    )
}

fn list(host: &str, recs: &[(&str, String)], sep: &str) -> String {
    recs.iter()
        .map(|(t, c)| format!("{t} {host} {c}"))
        .collect::<Vec<_>>()
        .join(sep)
}

impl Dns {
    /// Records to create, or the step result that stops us.
    fn records(&self, cx: &Ctx) -> Result<Vec<(&'static str, String)>, Check> {
        let f = cx.manifest.fly().expect("checked by the caller");
        match fly::ips(cx.runner, &cx.dir, &f.app) {
            Ok(ips) if ips.v4.is_empty() => Err(Check::Failed(format!(
                "fly app {} has no IPv4 address; run `fly ips allocate-v4 --app {}` or rerun the deploy step",
                f.app, f.app
            ))),
            Ok(ips) => Ok(records(&ips)),
            // `plan` runs before the deploy step has made the app.
            Err(e) if cx.opts.dry_run => Err(Check::Todo(format!("needs the fly app's IPs: {e}"))),
            Err(e) => Err(Check::Failed(e)),
        }
    }
}

impl Step for Dns {
    fn id(&self) -> &'static str {
        "dns"
    }

    fn check(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let Some(f) = cx.manifest.fly().cloned() else {
            return Ok(Check::Done("no [deploy.fly]".into()));
        };
        if cx.manifest.confirmed.dns.as_deref() == Some(f.host.as_str()) {
            return Ok(Check::Done(format!(
                "DNS records for {} confirmed as added by hand",
                f.host
            )));
        }
        if f.dns == Provider::None {
            return Ok(match self.records(cx) {
                // `dns = "none"` is never a failure: the records are the user's to add.
                Ok(recs) => Check::Done(format!(
                    "add these DNS records at your DNS provider: {}",
                    list(&f.host, &recs, "; ")
                )),
                Err(_) => Check::Done(format!(
                    "could not read the fly app's IPs; run `fly ips list --app {}` and add A/AAAA records for {} at your DNS provider",
                    f.app, f.host
                )),
            });
        }
        if !cx.opts.cloudflare_token {
            let recs = match self.records(cx) {
                Ok(r) => r,
                Err(c) => return Ok(c),
            };
            let what = format!(
                "add these DNS records (DNS only): {}",
                list(&f.host, &recs, "; ")
            );
            return Ok(cloudflare_token_step(self.id(), &what));
        }
        if !hoist::has_section(&cx.dir, SECTION) {
            return Ok(Check::Todo(format!(
                "add [{SECTION}] to hoist.toml and apply it ({} on fly app {})",
                f.host, f.app
            )));
        }
        let res = hoist::check_cloudflare(cx.runner, &cx.dir, &hoist::DNS, cx.opts.tool_timeout());
        Ok(hoist_check(
            res,
            self.id(),
            SECTION,
            &format!("DNS records for {} are in place", f.host),
        ))
    }

    fn apply(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let Some(f) = cx.manifest.fly().cloned() else {
            return self.check(cx);
        };
        if !hoist::has_section(&cx.dir, SECTION)
            && let Err(e) = hoist::append_section(&cx.dir, &section_text(&f))
        {
            return Ok(Check::Failed(e));
        }
        let res = hoist::apply_cloudflare(cx.runner, &cx.dir, &f.app, cx.opts.tool_timeout());
        match hoist_check(res, self.id(), SECTION, "applied") {
            // keel never passes `--prune`: a record still in drift after apply is a stray
            // A/AAAA record hoist will not delete on its own.
            Check::Done(_) => Ok(match self.check(cx)? {
                Check::Todo(_) => Check::Human(HumanStep {
                    id: self.id().into(),
                    title: format!(
                        "Remove the old A/AAAA records for {} in Cloudflare, or rerun with `hoist apply --prune`",
                        f.host
                    ),
                    url: "https://dash.cloudflare.com/".into(),
                    command: format!(
                        "hoist --json apply --yes --area cloudflare --prune --secrets-to fly:{}   # then keel up",
                        f.app
                    ),
                }),
                other => other,
            }),
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
    use crate::manifest::Manifest;

    const IPS: &str = r#"[{"Address":"1.2.3.4","Type":"v4"},{"Address":"2a09::1","Type":"v6"}]"#;

    fn cf_manifest() -> Manifest {
        let mut m = fly_manifest();
        m.deploy.as_mut().unwrap().fly.as_mut().unwrap().dns = Provider::Cloudflare;
        m
    }

    fn run(r: &FakeRunner, d: &std::path::Path, m: Manifest, token: bool, apply: bool) -> Check {
        let opts = Opts {
            cloudflare_token: token,
            ..Opts::default()
        };
        let mut cx = ctx(r, d.into(), m, PINS, opts, &no_sleep, &no_stdin);
        if apply {
            Dns.apply(&mut cx)
        } else {
            Dns.check(&mut cx)
        }
        .unwrap()
    }

    fn fly_ips() -> FakeRunner {
        FakeRunner::new().on(&["fly", "ips", "list"], Out::ok(IPS))
    }

    #[test]
    fn zone_defaults_to_the_last_two_labels_and_can_be_set() {
        let fly = |host: &str, zone: Option<&str>| Fly {
            host: host.into(),
            zone: zone.map(Into::into),
            ..fly_manifest().fly().unwrap().clone()
        };
        assert_eq!(zone(&fly("app.example.com", None)), "example.com");
        assert_eq!(zone(&fly("example.com", None)), "example.com");
        assert_eq!(
            zone(&fly("app.example.co.uk", Some("example.co.uk"))),
            "example.co.uk"
        );
    }

    #[test]
    fn apply_writes_the_manifest_zone() {
        let d = tempfile::tempdir().unwrap();
        let mut m = cf_manifest();
        let f = m.deploy.as_mut().unwrap().fly.as_mut().unwrap();
        f.host = "app.example.co.uk".into();
        f.zone = Some("example.co.uk".into());
        let r = fly_ips()
            .on(
                &["hoist", "--json", "apply"],
                Out::ok(r#"{"ok":true,"items":[]}"#),
            )
            .on(&["hoist", "--json", "check"], Out::ok(r#"{"items":[{"kind":"dns_record","key":"app.example.com A 1.2.3.4","action":"ok"}]}"#));
        run(&r, d.path(), m, true, true);
        let t = std::fs::read_to_string(d.path().join("hoist.toml")).unwrap();
        assert!(t.contains("zone = \"example.co.uk\""), "{t}");
    }

    #[test]
    fn no_fly_is_done() {
        let d = tempfile::tempdir().unwrap();
        let c = run(&FakeRunner::new(), d.path(), manifest("rust"), false, false);
        assert_eq!(c, Check::Done("no [deploy.fly]".into()));
    }

    #[test]
    fn provider_none_lists_the_records_and_does_not_block() {
        let d = tempfile::tempdir().unwrap();
        let Check::Done(m) = run(&fly_ips(), d.path(), fly_manifest(), false, false) else {
            panic!()
        };
        assert!(
            m.contains("A app.example.com 1.2.3.4; AAAA app.example.com 2a09::1"),
            "{m}"
        );
        // IPs not known yet (plan before deploy): still not blocking.
        let opts = Opts {
            dry_run: true,
            ..Opts::default()
        };
        let none = FakeRunner::new();
        let mut cx = ctx(
            &none,
            d.path().into(),
            fly_manifest(),
            PINS,
            opts,
            &no_sleep,
            &no_stdin,
        );
        assert!(matches!(Dns.check(&mut cx).unwrap(), Check::Done(_)));
    }

    #[test]
    fn provider_none_is_done_when_the_ips_cannot_be_read() {
        let d = tempfile::tempdir().unwrap();
        for r in [
            FakeRunner::new().on(&["fly", "ips", "list"], Out::with(1, "", "no such app")),
            FakeRunner::new().on(&["fly", "ips", "list"], Out::ok("[]")),
        ] {
            let Check::Done(m) = run(&r, d.path(), fly_manifest(), false, false) else {
                panic!()
            };
            assert!(
                m.contains("fly ips list --app app")
                    && m.contains("A/AAAA records for app.example.com"),
                "{m}"
            );
        }
        // Cloudflare with a token needs no addresses: hoist reads them from fly.
        let r = FakeRunner::new();
        assert!(matches!(
            run(&r, d.path(), cf_manifest(), true, false),
            Check::Todo(_)
        ));
        assert!(r.calls().is_empty());
    }

    #[test]
    fn cloudflare_without_token_is_human_listing_the_records() {
        let d = tempfile::tempdir().unwrap();
        let Check::Human(h) = run(&fly_ips(), d.path(), cf_manifest(), false, false) else {
            panic!()
        };
        assert!(
            h.title
                .starts_with("Set CLOUDFLARE_API_TOKEN or add these DNS records")
        );
        assert!(
            h.title.contains("A app.example.com 1.2.3.4")
                && h.title.contains("AAAA app.example.com 2a09::1")
        );
        assert!(h.command.contains("CLOUDFLARE_API_TOKEN") && h.command.contains("keel up"));
        assert!(h.command.contains("keel up --confirm dns"), "{}", h.command);
    }

    #[test]
    fn confirmed_dns_is_done_without_a_token_or_fly_ips() {
        let d = tempfile::tempdir().unwrap();
        let mut m = cf_manifest();
        m.confirmed.dns = Some("app.example.com".into());
        let r = FakeRunner::new();
        let c = run(&r, d.path(), m.clone(), false, false);
        assert!(matches!(c, Check::Done(m) if m.contains("confirmed")));
        assert!(r.calls().is_empty());
        // Confirmed for another host: void, the step asks for the token again.
        m.confirmed.dns = Some("old.example.com".into());
        let c = run(&FakeRunner::new(), d.path(), m, false, false);
        assert!(
            !matches!(&c, Check::Done(m) if m.contains("confirmed")),
            "{c:?}"
        );
    }

    #[test]
    fn check_is_read_only_and_asks_for_the_section() {
        let d = tempfile::tempdir().unwrap();
        let c = run(&fly_ips(), d.path(), cf_manifest(), true, false);
        assert!(matches!(c, Check::Todo(_)));
        assert!(!d.path().join("hoist.toml").exists());
    }

    #[test]
    fn apply_writes_the_fly_targets_section_then_applies_the_cloudflare_area() {
        let d = tempfile::tempdir().unwrap();
        let r = FakeRunner::new()
            .on(
                &["hoist", "--json", "apply"],
                Out::ok(r#"{"ok":true,"items":[]}"#),
            )
            .on(&["hoist", "--json", "check"], Out::ok(r#"{"items":[{"kind":"dns_record","key":"app.example.com A 1.2.3.4","action":"ok"}]}"#));
        assert!(matches!(
            run(&r, d.path(), cf_manifest(), true, true),
            Check::Done(_)
        ));
        let t = std::fs::read_to_string(d.path().join("hoist.toml")).unwrap();
        assert_eq!(
            t,
            "[cloudflare.dns]\nzone = \"example.com\"\nhost = \"app.example.com\"\ntargets = \"fly\"\nfly_app = \"app\"\nproxied = false\n"
        );
        let lines: Vec<String> = r.calls().iter().map(|c| c.line()).collect();
        assert_eq!(
            lines,
            [
                "hoist --json apply --yes --area cloudflare --secrets-to fly:app",
                "hoist --json check --area cloudflare"
            ]
        );
    }

    #[test]
    fn stray_records_left_after_apply_are_a_human_step_never_prune() {
        let d = tempfile::tempdir().unwrap();
        let drift = r#"{"ok":false,"items":[{"kind":"dns_record","key":"app.example.com A 5.6.7.8","action":"drift"}],"human_steps":[]}"#;
        let r = FakeRunner::new()
            .on(
                &["hoist", "--json", "apply"],
                Out::ok(r#"{"ok":true,"items":[]}"#),
            )
            .on(&["hoist", "--json", "check"], Out::with(1, drift, ""));
        let Check::Human(h) = run(&r, d.path(), cf_manifest(), true, true) else {
            panic!("want a human step")
        };
        assert_eq!(h.id, "dns");
        assert!(h.title.contains("app.example.com"), "{}", h.title);
        assert!(h.title.contains("--prune"), "{}", h.title);
        assert!(
            !r.calls().iter().any(|c| c.line().contains("--prune")),
            "keel never passes --prune"
        );
    }

    #[test]
    fn no_mail_adds_the_flag_to_the_section() {
        let d = tempfile::tempdir().unwrap();
        let mut m = cf_manifest();
        m.deploy.as_mut().unwrap().fly.as_mut().unwrap().no_mail = true;
        let r = FakeRunner::new().on(&["hoist"], Out::ok(r#"{"ok":true,"items":[]}"#));
        run(&r, d.path(), m, true, true);
        let t = std::fs::read_to_string(d.path().join("hoist.toml")).unwrap();
        assert!(t.ends_with("proxied = false\nno_mail = true\n"), "{t}");
    }

    #[test]
    fn check_reads_dns_drift_and_human_steps_out_of_the_cloudflare_area() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("hoist.toml"), "[cloudflare.dns]\n").unwrap();
        let chk = |o: Out| {
            let r = FakeRunner::new().on(&["hoist"], o);
            let c = run(&r, d.path(), cf_manifest(), true, false);
            assert_eq!(r.calls()[0].line(), "hoist --json check --area cloudflare");
            c
        };
        let ok =
            r#"{"items":[{"kind":"dns_record","key":"app.example.com A 1.2.3.4","action":"ok"}]}"#;
        assert!(matches!(chk(Out::ok(ok)), Check::Done(_)));
        let drift = r#"{"items":[{"kind":"dns_record","key":"app.example.com A 1.2.3.4","action":"create"}]}"#;
        assert!(matches!(chk(Out::with(1, drift, "")), Check::Todo(_)));
        // Items of another kind (r2) are the backup step's: dns has none of its own, so it is not done.
        let r2 = r#"{"items":[{"kind":"r2_bucket","key":"b","action":"create"}]}"#;
        assert!(matches!(chk(Out::with(1, r2, "")), Check::Todo(_)));
        let human = r#"{"items":[],"human_steps":[{"id":"dns:app.example.com A","title":"Add the A record","url":"https://dash.cloudflare.com"}]}"#;
        let Check::Human(h) = chk(Out::with(2, human, "")) else {
            panic!()
        };
        assert_eq!(
            (h.id.as_str(), h.title.as_str()),
            ("dns", "Add the A record")
        );
        let other = r#"{"items":[],"human_steps":[{"id":"pages:x","title":"t","url":"u"}]}"#;
        let Check::Human(h) = chk(Out::with(2, other, "")) else {
            panic!()
        };
        assert_eq!(
            (h.id.as_str(), h.title.as_str(), h.command.as_str()),
            ("dns", "t", "keel up")
        );
    }

    #[test]
    fn old_hoist_is_an_upgrade_human_step() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("hoist.toml"), "[cloudflare.dns]\n").unwrap();
        let r = fly_ips().on(&["hoist"], Out::with(2, "", "unexpected argument '--area'"));
        let Check::Human(h) = run(&r, d.path(), cf_manifest(), true, false) else {
            panic!()
        };
        assert_eq!(h.title, "Upgrade hoist to a release with [cloudflare.dns]");
        assert_eq!(h.command, "npm install -g @keelstackhq/hoist@latest");
    }
}
