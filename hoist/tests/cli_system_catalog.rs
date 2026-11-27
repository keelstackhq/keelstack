//! `hoist system explain` with the org catalog of alternatives (spec §17.6, §17.7).

mod common;

use std::fs;
use std::path::Path;

const SYSTEM: &str = r#"version = 1

[system]
name = "acme/app"
owner = "alice"

[[provider]]
id = "cloudflare"
capability = "dns+cdn"
alternatives = ["bunny", "ghost"]

[[provider]]
id = "npm"
capability = "registry.npm"
"#;

const CATALOG: &str = r#"version = 1

[[capability]]
id = "dns+cdn"
description = "DNS and a CDN"

[[alternative]]
id = "bunny"
capability = "dns+cdn"
name = "Bunny.net"
docs = "https://docs.bunny.net/"
keyless = []
migration = "Move the zone, then point the NS records at Bunny"

[[alternative]]
id = "fastly"
capability = "dns+cdn"
name = "Fastly"
docs = "https://www.fastly.com/documentation/"
keyless = ["oidc"]
migration = "Create a service, then switch DNS"
"#;

fn repo(catalog: Option<&str>) -> tempfile::TempDir {
    let d = tempfile::tempdir_in(common::tmpdir()).unwrap();
    fs::write(d.path().join("system.toml"), SYSTEM).unwrap();
    if let Some(c) = catalog {
        fs::write(d.path().join("catalog.toml"), c).unwrap();
    }
    d
}

fn explain(dir: &Path, extra: &[&str], code: i32) -> (String, String) {
    let out = common::hoist()
        .arg("-C")
        .arg(dir)
        .args(["system", "explain"])
        .args(extra)
        .assert()
        .code(code)
        .get_output()
        .clone();
    (
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

#[test]
fn without_a_catalog_the_output_has_no_alternatives_section() {
    let d = repo(None);
    let (s, _) = explain(d.path(), &[], 0);
    assert!(!s.contains("Alternatives"), "{s}");
    assert!(!s.contains("Bunny.net"), "{s}");
}

#[test]
fn catalog_next_to_system_toml_lists_name_and_migration() {
    let d = repo(Some(CATALOG));
    let (s, _) = explain(d.path(), &[], 0);
    assert!(s.contains("**Alternatives** for dns+cdn"), "{s}");
    assert!(
        s.contains("| bunny | Bunny.net | https://docs.bunny.net/ |"),
        "{s}"
    );
    assert!(
        s.contains("Move the zone, then point the NS records at Bunny"),
        "{s}"
    );
    // An id the catalog does not know stays visible.
    assert!(
        s.contains("| ghost | — | — | — | not in the catalog |"),
        "{s}"
    );
    // Fastly is in the catalog but not listed by the provider.
    assert!(!s.contains("Fastly"), "{s}");
    // npm lists no alternatives: no section for it.
    assert_eq!(s.matches("**Alternatives**").count(), 1, "{s}");
}

#[test]
fn flag_and_env_select_the_catalog_flag_first() {
    let d = repo(None);
    let other = d.path().join("elsewhere.toml");
    fs::write(&other, CATALOG).unwrap();
    let (s, _) = explain(d.path(), &["--catalog", other.to_str().unwrap()], 0);
    assert!(s.contains("Bunny.net"), "{s}");
    let out = common::hoist()
        .env("HOIST_CATALOG", &other)
        .arg("-C")
        .arg(d.path())
        .args(["system", "explain"])
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8(out).unwrap().contains("Bunny.net"));
    // The flag wins over the env var (a missing env file is not read).
    let out = common::hoist()
        .env("HOIST_CATALOG", d.path().join("missing.toml"))
        .arg("-C")
        .arg(d.path())
        .args(["system", "explain", "--catalog"])
        .arg(&other)
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8(out).unwrap().contains("Bunny.net"));
}

#[test]
fn write_and_check_round_trip_with_the_catalog() {
    let d = repo(Some(CATALOG));
    explain(d.path(), &["--check"], 1);
    explain(d.path(), &["--write"], 0);
    let md = fs::read_to_string(d.path().join("SYSTEM.md")).unwrap();
    assert!(md.contains("Bunny.net"), "{md}");
    explain(d.path(), &["--check"], 0);
    // The same SYSTEM.md is stale for a run without the catalog.
    fs::remove_file(d.path().join("catalog.toml")).unwrap();
    explain(d.path(), &["--check"], 1);
}

#[test]
fn bad_catalogs_are_exit_3_and_say_why() {
    let d = repo(None);
    let bad = |text: &str, expect: &str| {
        let p = d.path().join("bad.toml");
        fs::write(&p, text).unwrap();
        let (_, e) = explain(d.path(), &["--catalog", p.to_str().unwrap()], 3);
        assert!(e.contains(expect), "{e}");
    };
    bad("version = 2\n", "version");
    bad("version = 1\nsurprise = 1\n", "surprise");
    bad(
        "version = 1\n[[alternative]]\nid = \"x\"\ncapability = \"nope\"\nname = \"X\"\nmigration = \"m\"\n",
        "nope",
    );
    bad(
        "version = 1\n[[capability]]\nid = \"a\"\ndescription = \"d\"\n[[capability]]\nid = \"a\"\ndescription = \"d\"\n",
        "duplicate",
    );
    let (_, e) = explain(d.path(), &["--catalog", "no-such.toml"], 3);
    assert!(e.contains("no-such.toml"), "{e}");
}

/// The shipped catalog parses (through `explain`), and every capability and alternative id the
/// keelstack system.toml names exists in it.
#[test]
fn shipped_catalog_covers_keelstack_system_toml() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let cat: toml::Value = toml::from_str(
        &fs::read_to_string(root.join("decisions/channel/pieces/catalog/files/catalog.toml"))
            .unwrap(),
    )
    .unwrap();
    let ids = |key: &str| -> Vec<String> {
        cat[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap().to_string())
            .collect()
    };
    let (caps, alts) = (ids("capability"), ids("alternative"));
    for a in cat["alternative"].as_array().unwrap() {
        let docs = a["docs"].as_str().unwrap();
        assert!(docs.starts_with("https://"), "{docs}");
        assert!(!a["migration"].as_str().unwrap().is_empty());
    }
    let sys: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("system.toml")).unwrap()).unwrap();
    for p in sys["provider"].as_array().unwrap() {
        let cap = p["capability"].as_str().unwrap();
        assert!(
            caps.iter().any(|c| c == cap),
            "capability {cap} not in the catalog"
        );
        for a in p
            .get("alternatives")
            .and_then(|a| a.as_array())
            .into_iter()
            .flatten()
        {
            let a = a.as_str().unwrap();
            assert!(
                alts.iter().any(|c| c == a),
                "alternative {a} not in the catalog"
            );
        }
    }
}
