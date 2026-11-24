//! This repo's switch to `[checks] mode = "checks"` (spec §15, steps 6 and 7) and the CI signer
//! (spec §8): the config, hoist.toml's pinned required checks and the workflows agree.

use std::fs;

fn root(path: &str) -> String {
    fs::read_to_string(format!("{}/../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn config() -> toml::Value {
    toml::from_str(&root(".bosun/bosun.toml")).unwrap()
}

#[test]
fn the_signer_is_on_only_with_checks_mode_and_the_gate_app() {
    let c = config();
    let signer = c["approvals"].get("signer").and_then(|v| v.as_bool()) == Some(true);
    let mode = c["checks"]["mode"].as_str().unwrap();
    if signer {
        assert_eq!(mode, "checks", "the CI signer needs checks mode");
        assert!(c["checks"]["app_id"].as_integer().is_some_and(|i| i > 0));
        assert!(root(".github/workflows/signer.yml").contains("bosun gate ci-ok"));
        let keys = root(".bosun/approvers.toml");
        assert!(keys.contains("role = \"signer\""), "no signer key enrolled");
    }
}

#[test]
fn checks_mode_has_the_ruleset_pins_and_the_verdict_record_permission() {
    let c = config();
    // This repo runs in checks mode (spec §15 step 7); a move back to `dual` edits this test.
    assert_eq!(c["checks"]["mode"].as_str(), Some("checks"));
    let slug = c["checks"]["app_slug"].as_str().unwrap().to_string();
    let hoist: toml::Value = toml::from_str(&root("hoist.toml")).unwrap();
    let checks = hoist["github"]["required_checks"].as_array().unwrap();
    let pin = |ctx: &str| {
        checks
            .iter()
            .find(|e| e.get("context").and_then(|v| v.as_str()) == Some(ctx))
            .cloned()
            .unwrap_or_else(|| panic!("{ctx} is not pinned in hoist.toml"))
    };
    for ctx in ["risk-check", "review-verdict", "main-open"] {
        assert_eq!(pin(ctx)["app"].as_str(), Some(slug.as_str()), "{ctx}");
    }
    // ci-ok stays GitHub Actions' (ci.yml's job); the gate App's mirror (`bosun-ci-ok`) is the
    // signer's input only and never named `ci-ok`.
    assert_eq!(pin("ci-ok")["integration_id"].as_integer(), Some(15368));
    assert!(
        root(".github/workflows/verdict.yml").contains("permission-pull-requests: write"),
        "checks mode: the App writes the verdict record comment"
    );
}
