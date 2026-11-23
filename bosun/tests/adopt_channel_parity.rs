//! `bosun adopt` writes every `.bosun/*` file the channel's rendered workflows and scripts read
//! (or the reader tolerates its absence), and the pins file it writes parses with the readers' awk.

use bosun::cli::adopt::{render_all, vars_for};
use std::path::Path;
use std::process::Command;

/// `.bosun/` paths a channel file reads that adopt does not write, with the reason each is safe.
const TOLERATED: &[(&str, &str)] = &[(
    ".bosun/profiles/rust.toml",
    "optional repo override; read with `grep -qs` / a missing-file branch",
)];

fn adopted(stacks: &[&str]) -> Vec<(&'static str, String)> {
    let stacks: Vec<String> = stacks.iter().map(|s| s.to_string()).collect();
    render_all(&vars_for("app", "alice", "0.7.0", &stacks, false)).unwrap()
}

#[test]
fn adopt_writes_pins_toml_and_the_readers_awk_parses_it() {
    let files = adopted(&["rust"]);
    let (_, pins) = files
        .iter()
        .find(|(p, _)| *p == ".bosun/pins.toml")
        .expect("adopt must write .bosun/pins.toml");
    let v: toml::Value = toml::from_str(pins).unwrap();
    assert!(v["nextest"]["version"].as_str().is_some());
    // The pin() function of the rendered bosun.yml, run on the written file.
    let wf = &files
        .iter()
        .find(|(p, _)| *p == ".github/workflows/bosun.yml")
        .unwrap()
        .1;
    let awk = wf
        .lines()
        .find(|l| l.trim_start().starts_with("pin() { awk"))
        .expect("pin() in bosun.yml");
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".bosun")).unwrap();
    std::fs::write(dir.path().join(".bosun/pins.toml"), pins).unwrap();
    let script = format!(
        "f=.bosun/pins.toml\n{awk}\npin nextest version; pin cargo-deny version; pin rust fallback"
    );
    let out = Command::new("bash")
        .arg("-c")
        .arg(script)
        .current_dir(dir.path())
        .output()
        .unwrap();
    let got: Vec<String> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    assert_eq!(
        got,
        vec![
            v["nextest"]["version"].as_str().unwrap().to_string(),
            v["cargo-deny"]["version"].as_str().unwrap().to_string(),
            v["rust"]["fallback"].as_str().unwrap().to_string(),
        ]
    );
}

fn scan(dir: &Path, out: &mut Vec<(String, String)>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if name != "sections" {
                scan(&p, out);
            }
        } else if name != "piece.toml" && !name.starts_with("CODEOWNERS") {
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            let mut rest = text.as_str();
            while let Some(i) = rest.find(".bosun/") {
                rest = &rest[i..];
                let end = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || "_./-".contains(c)))
                    .unwrap_or(rest.len());
                let tok = rest[..end].trim_end_matches('.');
                let tok = tok.strip_suffix(".jinja").unwrap_or(tok);
                if tok.len() > ".bosun/".len() && !tok.ends_with('/') {
                    out.push((p.display().to_string(), tok.to_string()));
                }
                rest = &rest[end.max(1)..];
            }
        }
    }
}

#[test]
fn every_bosun_file_the_channel_reads_is_written_by_adopt_or_tolerated() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("channel/pieces");
    let mut refs = Vec::new();
    scan(&root, &mut refs);
    assert!(refs.len() > 10, "the scan found {} references", refs.len());
    let mut written: Vec<String> = Vec::new();
    for stacks in [&["rust"][..], &["ts-pnpm"][..], &["rust", "ts-pnpm"][..]] {
        written.extend(adopted(stacks).into_iter().map(|(p, _)| p.to_string()));
    }
    for (file, tok) in refs {
        let ok = written.contains(&tok.to_string()) || TOLERATED.iter().any(|(t, _)| *t == tok);
        assert!(
            ok,
            "{file} reads {tok}: not written by adopt, not in TOLERATED"
        );
    }
}

fn ci_paths(text: &str) -> std::collections::BTreeSet<String> {
    let v: toml::Value = toml::from_str(text).unwrap();
    let rules = v["rule"].as_array().unwrap();
    let ci = rules
        .iter()
        .find(|r| r["id"].as_str() == Some("R-ci"))
        .unwrap();
    ci["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect()
}

/// The channel's R-ci (rust stack) lists every path of the built-in R-ci, and nothing more.
#[test]
fn the_channel_r_ci_equals_the_built_in_one_for_a_rust_repo() {
    let files = adopted(&["rust"]);
    let channel = ci_paths(
        &files
            .iter()
            .find(|(p, _)| *p == ".bosun/risk.toml")
            .unwrap()
            .1,
    );
    let builtin = ci_paths(include_str!("../src/core/risk_default.toml"));
    // `scripts/deny.mjs` is bosun's own script, covered by `scripts/**` in the channel.
    let missing: Vec<_> = builtin
        .difference(&channel)
        .filter(|p| *p != "scripts/deny.mjs")
        .collect();
    assert!(missing.is_empty(), "channel R-ci lacks {missing:?}");
    let extra: Vec<_> = channel.difference(&builtin).collect();
    assert!(extra.is_empty(), "channel R-ci has extra {extra:?}");
}
