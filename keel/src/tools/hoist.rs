use super::{failure, json, not_json};
use crate::exec::{Cmd, Out, Runner};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

pub enum Res {
    Ok,
    /// Drift (exit 1) or pending human steps (exit 2) as reported by `check`.
    Drift,
    /// The first of `human_steps` (exit 2).
    Human {
        title: String,
        url: String,
        /// `gh variable set …` from the matching item's reason, else `keel up`.
        command: String,
    },
    Failed(String),
    /// `--area` is not understood: this hoist predates it.
    Upgrade,
}

fn envelope(cmd: &Cmd, out: &Out) -> Result<Value, String> {
    json(out).ok_or_else(|| not_json(cmd))
}

/// For a human step `variable:<NAME>`: the `gh variable set …` text of the item
/// `{kind:"variable", key:<NAME>}`'s `reason`.
fn variable_command(v: &Value, human: &Value) -> Option<String> {
    let name = human["id"].as_str()?.strip_prefix("variable:")?;
    let item = v["items"]
        .as_array()?
        .iter()
        .find(|i| i["kind"] == "variable" && i["key"].as_str() == Some(name))?;
    let reason = item["reason"].as_str()?;
    let at = reason.find("gh variable set")?;
    Some(reason[at..].trim().to_string())
}

/// An old hoist answers `--area` with a usage error (exit 2, no hoist JSON).
fn old_hoist(o: &Out) -> bool {
    o.code == 2
        && json(o).is_none_or(|v| v.get("human_steps").is_none() && v.get("items").is_none())
}

/// hoist's areas except `cloudflare`, which the dns and backup steps own.
const SETTINGS_AREAS: [&str; 4] = ["github", "npm", "crates", "release"];
const CLOUDFLARE: &str = "cloudflare";

/// `["--area", A, "--area", B, ...]`. The one place `--area` is spelled.
fn area_args(areas: &[&str]) -> Vec<String> {
    areas
        .iter()
        .flat_map(|a| ["--area".to_string(), a.to_string()])
        .collect()
}

fn cmd(base: &[&str], extra: Vec<String>, dir: &Path, t: Duration) -> Cmd {
    let mut a: Vec<&str> = base.to_vec();
    a.extend(extra.iter().map(String::as_str));
    Cmd::new("hoist", &a).dir(dir).timeout(t)
}

/// The part of the `cloudflare` area one step owns: hoist has no sub-areas, so a step reads
/// its own JSON items (by `kind`) out of the whole area. Exit 2 is `human_steps[0]` for both.
pub struct Slice {
    pub kinds: &'static [&'static str],
}

/// `[cloudflare.dns]`: items `dns_record`.
pub const DNS: Slice = Slice {
    kinds: &["dns_record"],
};

/// `[cloudflare.r2]`: items `r2_bucket`, `r2_lifecycle`, `r2_token`.
pub const R2: Slice = Slice {
    kinds: &["r2_bucket", "r2_lifecycle", "r2_token"],
};

/// `hoist --json check --area github --area npm --area crates --area release`. A hoist that
/// rejects `--area` (exit 2 without hoist JSON) gets the plain `check`.
pub fn check_settings(r: &dyn Runner, dir: &Path, t: Duration) -> Res {
    let c = cmd(&["--json", "check"], area_args(&SETTINGS_AREAS), dir, t);
    let o = r.run(&c);
    if old_hoist(&o) {
        return check_plain(r, dir, t);
    }
    check_res(&c, &o)
}

fn check_plain(r: &dyn Runner, dir: &Path, t: Duration) -> Res {
    let c = Cmd::new("hoist", &["--json", "check"]).dir(dir).timeout(t);
    check_res(&c, &r.run(&c))
}

fn check_res(c: &Cmd, o: &Out) -> Res {
    match o.code {
        0..=2 => match envelope(c, o) {
            Ok(_) if o.code == 0 => Res::Ok,
            Ok(_) => Res::Drift,
            Err(e) => Res::Failed(e),
        },
        _ => Res::Failed(failure(c, o)),
    }
}

/// `hoist --json apply --yes` plus the same `--area` flags as [`check_settings`], same fallback.
pub fn apply_settings(r: &dyn Runner, dir: &Path, t: Duration) -> Res {
    let c = cmd(
        &["--json", "apply", "--yes"],
        area_args(&SETTINGS_AREAS),
        dir,
        t,
    );
    let o = r.run(&c);
    if old_hoist(&o) {
        let c = Cmd::new("hoist", &["--json", "apply", "--yes"])
            .dir(dir)
            .timeout(t);
        return apply_res(&c, &r.run(&c));
    }
    apply_res(&c, &o)
}

/// An item is done only when its `action` (create|update|ok|drift|pending|unknown) is "ok".
fn in_drift(item: &Value) -> bool {
    item["action"].as_str() != Some("ok")
}

/// `hoist --json check --area cloudflare`, read for one [`Slice`]: exit 0 or 1 is done only when
/// at least one item of the slice's kinds exists and every such item is done (see [`in_drift`]);
/// no such item, or one that is not "ok", is drift. Exit 2 is `human_steps[0]` (never done). A
/// hoist that rejects `--area` is `Upgrade`.
pub fn check_cloudflare(r: &dyn Runner, dir: &Path, s: &Slice, t: Duration) -> Res {
    let c = cmd(&["--json", "check"], area_args(&[CLOUDFLARE]), dir, t);
    let o = r.run(&c);
    if old_hoist(&o) {
        return Res::Upgrade;
    }
    match o.code {
        0 | 1 => match envelope(&c, &o) {
            Err(e) => Res::Failed(e),
            Ok(v) => {
                let mine: Vec<&Value> = v["items"].as_array().map_or_else(Vec::new, |a| {
                    a.iter()
                        .filter(|i| s.kinds.contains(&i["kind"].as_str().unwrap_or("")))
                        .collect()
                });
                if !mine.is_empty() && !mine.iter().any(|i| in_drift(i)) {
                    Res::Ok
                } else {
                    Res::Drift
                }
            }
        },
        2 => match envelope(&c, &o) {
            Err(e) => Res::Failed(e),
            Ok(v) => human_res(&v, &v["human_steps"][0], &c),
        },
        _ => Res::Failed(failure(&c, &o)),
    }
}

/// `hoist --json apply --yes --area cloudflare --secrets-to fly:APP`. The sink is always passed:
/// the area also mints the R2 key when `[cloudflare.r2]` exists, and minting without a sink
/// exits 3. Exit 2 is `human_steps[0]`.
pub fn apply_cloudflare(r: &dyn Runner, dir: &Path, fly_app: &str, t: Duration) -> Res {
    let mut extra = area_args(&[CLOUDFLARE]);
    extra.extend(["--secrets-to".to_string(), format!("fly:{fly_app}")]);
    let c = cmd(&["--json", "apply", "--yes"], extra, dir, t);
    let o = r.run(&c);
    if old_hoist(&o) {
        return Res::Upgrade;
    }
    match o.code {
        2 => match envelope(&c, &o) {
            Ok(v) => human_res(&v, &v["human_steps"][0], &c),
            Err(e) => Res::Failed(e),
        },
        // Key delivery failed and so did the revoke: hoist adds a human step for it.
        1 => match json(&o) {
            Some(v) if v["human_steps"][0]["title"].is_string() => {
                human_res(&v, &v["human_steps"][0], &c)
            }
            _ => Res::Failed(failure(&c, &o)),
        },
        _ => apply_res(&c, &o),
    }
}

fn human_res(v: &Value, h: &Value, c: &Cmd) -> Res {
    match h["title"].as_str() {
        Some(t) => Res::Human {
            title: t.to_string(),
            url: h["url"].as_str().unwrap_or("").to_string(),
            command: variable_command(v, h).unwrap_or_else(|| "keel up".into()),
        },
        None => Res::Failed(format!(
            "`{}` exited 2 without a human step; run it by hand",
            c.line()
        )),
    }
}

fn apply_res(c: &Cmd, o: &Out) -> Res {
    match o.code {
        0 => match envelope(c, o) {
            Ok(_) => Res::Ok,
            Err(e) => Res::Failed(e),
        },
        2 => match envelope(c, o) {
            Ok(v) => human_res(&v, &v["human_steps"][0], c),
            Err(e) => Res::Failed(e),
        },
        _ => Res::Failed(failure(c, o)),
    }
}

pub const FILE: &str = "hoist.toml";

/// A TOML basic string for `s` (quoted and escaped). The one way keel writes a value into
/// `hoist.toml`.
pub fn toml_str(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

/// Whether `hoist.toml` already has the `[header]` table.
pub fn has_section(dir: &Path, header: &str) -> bool {
    std::fs::read_to_string(dir.join(FILE))
        .is_ok_and(|t| t.lines().any(|l| l.trim() == format!("[{header}]")))
}

/// The line range of the `[cloudflare]` table: (index of header, index after its last line).
fn cloudflare_table(lines: &[&str]) -> Option<(usize, usize)> {
    let h = lines.iter().position(|l| l.trim() == "[cloudflare]")?;
    let end = lines[h + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |n| h + 1 + n);
    Some((h, end))
}

fn is_account_id(line: &str) -> bool {
    line.trim_start()
        .strip_prefix("account_id")
        .is_some_and(|r| r.trim_start().starts_with('='))
}

/// Whether `hoist.toml` has `account_id` in its top-level `[cloudflare]` table.
pub fn has_account_id(dir: &Path) -> bool {
    let t = std::fs::read_to_string(dir.join(FILE)).unwrap_or_default();
    let lines: Vec<&str> = t.lines().collect();
    cloudflare_table(&lines).is_some_and(|(h, e)| lines[h + 1..e].iter().any(|l| is_account_id(l)))
}

/// Make sure `hoist.toml` has `[cloudflare] account_id`: append the table when it has no
/// `[cloudflare]` header, insert the line after the header when the table lacks the key, else
/// leave the file alone. Text edits only.
pub fn ensure_account_id(dir: &Path, id: &str) -> Result<(), String> {
    let path = dir.join(FILE);
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    let lines: Vec<&str> = have.lines().collect();
    let line = format!("account_id = {}", toml_str(id));
    match cloudflare_table(&lines) {
        None => append_section(dir, &format!("[cloudflare]\n{line}\n")),
        Some((h, e)) if !lines[h + 1..e].iter().any(|l| is_account_id(l)) => {
            let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
            out.insert(h + 1, line);
            let mut t = out.join("\n");
            t.push('\n');
            std::fs::write(&path, t).map_err(|e| format!("cannot write {FILE}: {e}"))
        }
        Some(_) => Ok(()),
    }
}

/// Append `text` (a whole table) to `hoist.toml`, creating the file when it is missing.
pub fn append_section(dir: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let path = dir.join(FILE);
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    let sep = if have.is_empty() || have.ends_with("\n\n") {
        ""
    } else if have.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| write!(f, "{sep}{text}"))
        .map_err(|e| format!("cannot write {FILE}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::FakeRunner;

    const T: Duration = Duration::from_secs(42);

    fn all_timed(r: &FakeRunner) {
        assert!(!r.calls().is_empty());
        assert!(r.calls().iter().all(|c| c.timeout == Some(T)));
    }

    fn lines(r: &FakeRunner) -> Vec<String> {
        r.calls().iter().map(|c| c.line()).collect()
    }

    const AREAS: &str = "--area github --area npm --area crates --area release";

    #[test]
    fn settings_names_every_area_but_cloudflare_and_falls_back_on_old_hoist() {
        let r = FakeRunner::new().on(
            &["hoist", "--json", "check", "--area"],
            Out::with(2, "", "error: unexpected argument '--area'"),
        );
        let r = r.on(&["hoist", "--json", "check"], Out::ok(r#"{"ok":true}"#));
        assert!(matches!(check_settings(&r, Path::new("."), T), Res::Ok));
        all_timed(&r);
        assert_eq!(
            lines(&r),
            [
                format!("hoist --json check {AREAS}"),
                "hoist --json check".into()
            ]
        );
        let r = FakeRunner::new().on(&["hoist"], Out::ok(r#"{"ok":true,"items":[]}"#));
        apply_settings(&r, Path::new("."), T);
        all_timed(&r);
        assert_eq!(lines(&r), [format!("hoist --json apply --yes {AREAS}")]);
        // The old-hoist fallback is timed too.
        let r = FakeRunner::new()
            .on(
                &["hoist", "--json", "apply", "--yes", "--area"],
                Out::with(2, "", "unexpected argument"),
            )
            .on(
                &["hoist", "--json", "apply", "--yes"],
                Out::ok(r#"{"items":[]}"#),
            );
        apply_settings(&r, Path::new("."), T);
        assert_eq!(r.calls().len(), 2);
        all_timed(&r);
        // Exit 2 with hoist JSON is a human step, not an old hoist.
        let r = FakeRunner::new().on(
            &["hoist"],
            Out::with(2, r#"{"items":[],"human_steps":[]}"#, ""),
        );
        assert!(matches!(check_settings(&r, Path::new("."), T), Res::Drift));
        assert_eq!(r.calls().len(), 1);
    }

    #[test]
    fn cloudflare_check_reads_the_slice_out_of_the_whole_area() {
        let chk = |out: Out, s: &Slice| {
            let r = FakeRunner::new().on(&["hoist"], out);
            let res = check_cloudflare(&r, Path::new("."), s, T);
            all_timed(&r);
            assert_eq!(lines(&r), ["hoist --json check --area cloudflare"]);
            res
        };
        // Done needs at least one item of the slice's kinds, all with action "ok".
        assert!(matches!(chk(Out::ok(r#"{"items":[]}"#), &DNS), Res::Drift));
        assert!(matches!(chk(Out::ok(r#"{"ok":true}"#), &R2), Res::Drift));
        let ok = r#"{"items":[{"kind":"dns_record","key":"k","action":"ok"}]}"#;
        assert!(matches!(chk(Out::ok(ok), &DNS), Res::Ok));
        assert!(matches!(chk(Out::ok(ok), &R2), Res::Drift));
        let weird = r#"{"items":[{"kind":"dns_record","key":"k","action":"weird"}]}"#;
        assert!(matches!(chk(Out::ok(weird), &DNS), Res::Drift));
        assert!(matches!(chk(Out::with(1, weird, ""), &DNS), Res::Drift));
        let mixed = r#"{"items":[{"kind":"dns_record","key":"a","action":"ok"},{"kind":"dns_record","key":"b","action":"weird"}]}"#;
        assert!(matches!(chk(Out::ok(mixed), &DNS), Res::Drift));
        let drift = r#"{"items":[{"kind":"r2_bucket","key":"b","action":"create"},{"kind":"dns_record","key":"h A 1.2.3.4","action":"ok"}]}"#;
        assert!(matches!(chk(Out::with(1, drift, ""), &DNS), Res::Ok));
        assert!(matches!(chk(Out::with(1, drift, ""), &R2), Res::Drift));
        for a in ["create", "update", "drift", "pending", "unknown"] {
            let d = format!(r#"{{"items":[{{"kind":"dns_record","key":"k","action":"{a}"}}]}}"#);
            assert!(matches!(chk(Out::with(1, &d, ""), &DNS), Res::Drift), "{a}");
        }
        let n =
            r#"{"items":[{"kind":"dns_record","key":"h A 1.2.3.4","status":"noop","ok":true}]}"#;
        assert!(matches!(chk(Out::with(1, n, ""), &DNS), Res::Drift));
        assert!(matches!(
            chk(Out::with(1, "boom", ""), &DNS),
            Res::Failed(_)
        ));
        assert!(matches!(chk(Out::with(3, "", "x"), &DNS), Res::Failed(_)));
        assert!(matches!(
            chk(Out::with(2, "", "unexpected argument '--area'"), &DNS),
            Res::Upgrade
        ));
        let h = r#"{"items":[],"human_steps":[{"id":"cloudflare:token","title":"Add a token","url":"u"},{"id":"dns-cname:x","title":"second","url":""}]}"#;
        for s in [&DNS, &R2] {
            match chk(Out::with(2, h, ""), s) {
                Res::Human { title, url, .. } => {
                    assert_eq!((title.as_str(), url.as_str()), ("Add a token", "u"))
                }
                _ => panic!(),
            }
        }
        assert!(matches!(
            chk(Out::with(2, r#"{"items":[]}"#, ""), &DNS),
            Res::Failed(_)
        ));
    }

    #[test]
    fn cloudflare_apply_passes_the_sink_and_reads_exit_codes() {
        let run = |out: Out| {
            let r = FakeRunner::new().on(&["hoist"], out);
            let res = apply_cloudflare(&r, Path::new("."), "app", T);
            all_timed(&r);
            assert_eq!(
                lines(&r),
                ["hoist --json apply --yes --area cloudflare --secrets-to fly:app"]
            );
            res
        };
        assert!(matches!(run(Out::ok(r#"{"items":[]}"#)), Res::Ok));
        assert!(matches!(run(Out::with(3, "", "no sink")), Res::Failed(_)));
        assert!(matches!(
            run(Out::with(2, "", "unexpected argument '--area'")),
            Res::Upgrade
        ));
        let h = r#"{"items":[],"human_steps":[{"id":"dns:app.example.com A","title":"Add A","url":""}]}"#;
        assert!(matches!(run(Out::with(2, h, "")), Res::Human { .. }));
        // Key delivery failed (exit 1) and the revoke failed too: hoist's human step wins.
        let rev = r#"{"ok":false,"items":[],"human_steps":[{"id":"cloudflare:revoke","title":"Revoke the R2 token by hand","url":"https://dash.cloudflare.com/"}]}"#;
        match run(Out::with(1, rev, "")) {
            Res::Human {
                title,
                url,
                command,
            } => assert_eq!(
                (title.as_str(), url.as_str(), command.as_str()),
                (
                    "Revoke the R2 token by hand",
                    "https://dash.cloudflare.com/",
                    "keel up"
                )
            ),
            _ => panic!(),
        }
        assert!(matches!(run(Out::with(1, "boom", "")), Res::Failed(_)));
        assert!(matches!(
            run(Out::with(1, r#"{"ok":false,"items":[]}"#, "")),
            Res::Failed(_)
        ));
    }

    #[test]
    fn ensure_account_id_appends_inserts_or_leaves_the_cloudflare_table() {
        let d = tempfile::tempdir().unwrap();
        let rd = || std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(!has_account_id(d.path()));
        ensure_account_id(d.path(), "A1").unwrap();
        assert_eq!(rd(), "[cloudflare]\naccount_id = \"A1\"\n");
        assert!(has_account_id(d.path()));
        ensure_account_id(d.path(), "B2").unwrap();
        assert_eq!(rd(), "[cloudflare]\naccount_id = \"A1\"\n");
        std::fs::write(d.path().join(FILE), "[github]\nx = 1\n").unwrap();
        ensure_account_id(d.path(), "A1").unwrap();
        assert_eq!(
            rd(),
            "[github]\nx = 1\n\n[cloudflare]\naccount_id = \"A1\"\n"
        );
        let t = "# c\n[cloudflare]\n# keep\npages = 1\n\n[cloudflare.dns]\nzone = \"a.io\"\n";
        std::fs::write(d.path().join(FILE), t).unwrap();
        assert!(!has_account_id(d.path()));
        ensure_account_id(d.path(), "A1").unwrap();
        assert_eq!(
            rd(),
            "# c\n[cloudflare]\naccount_id = \"A1\"\n# keep\npages = 1\n\n[cloudflare.dns]\nzone = \"a.io\"\n"
        );
        // An account_id in a sub-table is not the top-level one.
        std::fs::write(d.path().join(FILE), "[cloudflare.r2]\naccount_id = \"x\"\n").unwrap();
        assert!(!has_account_id(d.path()));
    }

    #[test]
    fn toml_str_escapes_quotes_backslashes_and_newlines() {
        assert_eq!(toml_str("a"), "\"a\"");
        let v = "x\"\nbad = 1\\";
        let t = format!("k = {}\n", toml_str(v));
        let parsed: toml::Table = toml::from_str(&t).unwrap();
        assert_eq!(parsed["k"].as_str(), Some(v));
    }

    #[test]
    fn appends_a_section_once_and_keeps_the_file() {
        let d = tempfile::tempdir().unwrap();
        assert!(!has_section(d.path(), "cloudflare.dns"));
        std::fs::write(d.path().join(FILE), "[github]\nx = 1").unwrap();
        append_section(d.path(), "[cloudflare.dns]\nzone = \"a.io\"\n").unwrap();
        let t = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert_eq!(t, "[github]\nx = 1\n\n[cloudflare.dns]\nzone = \"a.io\"\n");
        assert!(has_section(d.path(), "cloudflare.dns"));
        assert!(!has_section(d.path(), "cloudflare.r2"));
    }
}
