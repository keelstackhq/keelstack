//! The gate jobs run the bosun release pinned in `.bosun/version`, and its config parser rejects
//! unknown keys. So bosun's own `.bosun/bosun.toml` may only use tables that release knows: a
//! newer key breaks every gate on main until a release that reads it is pinned.
//!
//! In the keelstack monorepo (D-023) bosun runs on the root `.bosun/`, which uses `[[folders]]`.
//! No release reads that yet (the first is 0.22.0), so the pin is `unreleased` (spec §11): every
//! workflow builds bosun from the workspace and none reads the pin, and an `npx
//! @keelstackhq/bosun@unreleased` fallback fails instead of running a release that cannot read
//! the config. Pin a release only once it knows every key here; these tests then hold it to that.

/// The top-level keys each pinned release accepts. Add a release only when it accepts new keys
/// (see `known_for`).
const KNOWN: &[(&str, &[&str])] = &[
    ("0.2.3", &["project", "stack", "review", "slack", "prices"]),
    (
        "0.3.0",
        &["project", "stack", "review", "slack", "prices", "approvals"],
    ),
    (
        "0.4.0",
        &["project", "stack", "review", "slack", "prices", "approvals"],
    ),
    (
        "0.5.0",
        &[
            "project",
            "stack",
            "review",
            "slack",
            "cloud",
            "prices",
            "approvals",
            "intake",
        ],
    ),
    (
        "0.6.0",
        &[
            "project",
            "stack",
            "review",
            "slack",
            "cloud",
            "prices",
            "approvals",
            "intake",
        ],
    ),
    (
        "0.7.0",
        &[
            "project",
            "stack",
            "review",
            "slack",
            "cloud",
            "prices",
            "approvals",
            "intake",
        ],
    ),
    (
        "0.8.0",
        &[
            "project",
            "stack",
            "review",
            "slack",
            "cloud",
            "prices",
            "approvals",
            "intake",
        ],
    ),
    (
        "0.9.0",
        &[
            "project",
            "stack",
            "review",
            "slack",
            "cloud",
            "prices",
            "approvals",
            "intake",
            "trailers",
        ],
    ),
    (
        "0.10.0",
        &[
            "project",
            "stack",
            "review",
            "slack",
            "cloud",
            "prices",
            "approvals",
            "intake",
            "trailers",
        ],
    ),
    (
        "0.13.0",
        &[
            "project",
            "stack",
            "review",
            "slack",
            "cloud",
            "prices",
            "approvals",
            "intake",
            "trailers",
            "checks",
            "session",
        ],
    ),
    (
        "0.14.0",
        &[
            "project",
            "stack",
            "review",
            "slack",
            "cloud",
            "prices",
            "approvals",
            "intake",
            "trailers",
            "checks",
            "session",
            "mode",
        ],
    ),
    (
        "0.22.0",
        &[
            "project",
            "stack",
            "review",
            "slack",
            "cloud",
            "prices",
            "approvals",
            "intake",
            "trailers",
            "checks",
            "session",
            "mode",
            "ci",
            "folders",
        ],
    ),
];

/// The `.bosun/limits.toml` keys (`table.key`) each pinned release accepts.
const KNOWN_LIMITS: &[(&str, &[&str])] = &[
    (
        "0.4.0",
        &[
            "sessions.max_depth",
            "sessions.max_live_sessions",
            "sessions.max_children",
            "waves.max_parallel",
            "turns.build",
            "turns.build_cap",
            "turns.review",
            "turns.fix_round",
            "models.read",
            "models.build",
            "models.review",
            "models.hard",
            "budget.daily_usd",
            "budget.per_task_usd",
        ],
    ),
    (
        "0.5.0",
        &[
            "sessions.max_depth",
            "sessions.max_live_sessions",
            "sessions.max_children",
            "waves.max_parallel",
            "waves.relay_waves",
            "turns.build",
            "turns.build_cap",
            "turns.review",
            "turns.fix_round",
            "models.read",
            "models.build",
            "models.review",
            "models.hard",
            "budget.daily_usd",
            "budget.per_task_usd",
        ],
    ),
    (
        "0.6.0",
        &[
            "sessions.max_depth",
            "sessions.max_live_sessions",
            "sessions.max_children",
            "waves.max_parallel",
            "waves.relay_waves",
            "turns.build",
            "turns.build_cap",
            "turns.review",
            "turns.fix_round",
            "models.read",
            "models.build",
            "models.review",
            "models.hard",
            "budget.daily_usd",
            "budget.per_task_usd",
        ],
    ),
    (
        "0.7.0",
        &[
            "sessions.max_depth",
            "sessions.max_live_sessions",
            "sessions.max_children",
            "waves.max_parallel",
            "waves.relay_waves",
            "turns.build",
            "turns.build_cap",
            "turns.review",
            "turns.fix_round",
            "models.read",
            "models.build",
            "models.review",
            "models.hard",
            "budget.daily_usd",
            "budget.per_task_usd",
        ],
    ),
    (
        "0.8.0",
        &[
            "sessions.max_depth",
            "sessions.max_live_sessions",
            "sessions.max_children",
            "waves.max_parallel",
            "waves.relay_waves",
            "turns.build",
            "turns.build_cap",
            "turns.review",
            "turns.fix_round",
            "models.read",
            "models.build",
            "models.review",
            "models.hard",
            "budget.daily_usd",
            "budget.per_task_usd",
        ],
    ),
    (
        "0.9.0",
        &[
            "sessions.max_depth",
            "sessions.max_live_sessions",
            "sessions.max_children",
            "waves.max_parallel",
            "waves.relay_waves",
            "turns.build",
            "turns.build_cap",
            "turns.review",
            "turns.fix_round",
            "models.read",
            "models.build",
            "models.review",
            "models.hard",
            "budget.daily_usd",
            "budget.per_task_usd",
        ],
    ),
    (
        "0.10.0",
        &[
            "sessions.max_depth",
            "sessions.max_live_sessions",
            "sessions.max_children",
            "waves.max_parallel",
            "waves.relay_waves",
            "turns.build",
            "turns.build_cap",
            "turns.review",
            "turns.fix_round",
            "models.read",
            "models.build",
            "models.review",
            "models.hard",
            "budget.daily_usd",
            "budget.per_task_usd",
        ],
    ),
];

/// The `table.key` names of a limits file that `known` does not list.
fn unknown_limits(text: &str, known: &[&str]) -> Vec<String> {
    let table: toml::Table = text.parse().expect("limits.toml");
    let mut out = Vec::new();
    for (name, value) in &table {
        match value.as_table() {
            Some(t) => out.extend(
                t.keys()
                    .map(|k| format!("{name}.{k}"))
                    .filter(|k| !known.contains(&k.as_str())),
            ),
            None => out.push(name.clone()),
        }
    }
    out
}

/// The keys of `[approvals]` each pinned release accepts (`policy` from 0.5.0).
const KNOWN_APPROVALS: &[(&str, &[&str])] = &[
    ("0.3.0", &["bootstrap_until"]),
    ("0.4.0", &["bootstrap_until"]),
    ("0.5.0", &["bootstrap_until", "policy"]),
    ("0.6.0", &["bootstrap_until", "policy"]),
    ("0.7.0", &["bootstrap_until", "policy"]),
    ("0.8.0", &["bootstrap_until", "policy"]),
    ("0.9.0", &["bootstrap_until", "policy"]),
    ("0.10.0", &["bootstrap_until", "policy"]),
    ("0.11.0", &["bootstrap_until", "policy", "review_models"]),
    // `signer` (the opt-in for the CI signer) is read from 0.15.0: pin that release first.
    (
        "0.15.0",
        &["bootstrap_until", "policy", "review_models", "signer"],
    ),
    // `chat` (a chat approval record counts as the owner's approval) and `bosun_repo` (the upstream repo of release tags) are read from 0.22.0 (0.21.0 reads neither).
    (
        "0.22.0",
        &[
            "bootstrap_until",
            "policy",
            "review_models",
            "signer",
            "chat",
            "bosun_repo",
        ],
    ),
];

/// The `[approvals] policy` values each pinned release accepts (`autonomous` from 0.8.0).
const KNOWN_POLICIES: &[(&str, &[&str])] = &[
    ("0.5.0", &["owner", "owner-gated", "orchestrator"]),
    ("0.6.0", &["owner", "owner-gated", "orchestrator"]),
    ("0.7.0", &["owner", "owner-gated", "orchestrator"]),
    (
        "0.8.0",
        &["owner", "owner-gated", "orchestrator", "autonomous"],
    ),
    (
        "0.9.0",
        &["owner", "owner-gated", "orchestrator", "autonomous"],
    ),
    (
        "0.10.0",
        &["owner", "owner-gated", "orchestrator", "autonomous"],
    ),
    (
        "0.22.0",
        &[
            "owner",
            "owner-gated",
            "orchestrator",
            "autonomous",
            "quorum",
        ],
    ),
];

/// `major.minor.patch` as numbers, so `0.10.0` sorts after `0.9.0`. A pre-release or `v`-prefixed
/// pin (`0.10.0-rc.1`, `v0.10.0`) panics: the test fails closed.
fn semver(v: &str) -> Vec<u64> {
    v.split('.')
        .map(|p| {
            p.parse()
                .unwrap_or_else(|_| panic!("`{v}` is not an x.y.z version"))
        })
        .collect()
}

/// The entry of `version` in a release table, else of the newest older release: a later release
/// only adds keys (`release_tables_only_grow` checks the listed ones), so the older set is a safe
/// subset and a pin bump needs no new entry. Add one when a release accepts new keys. A release
/// that removes or renames a key breaks `release_tables_only_grow`: change that test and this
/// function with it, so no lookup falls back across that release (exact match from it on).
/// Backstop for a removal: `tests/governance.rs` parses the `.bosun` config with the current
/// code, which is the next release. `None` before the first entry.
fn known_for(
    table: &[(&'static str, &'static [&'static str])],
    version: &str,
) -> Option<&'static [&'static str]> {
    (table.iter())
        .filter(|(v, _)| semver(v) <= semver(version))
        .max_by_key(|(v, _)| semver(v))
        .map(|(_, keys)| *keys)
}

/// Whether release `version` accepts `[approvals] policy = value`; `None` before the first entry.
fn policy_known(version: &str, value: &str) -> Option<bool> {
    let values = known_for(KNOWN_POLICIES, version)?;
    Some(values.contains(&value))
}

/// `path` as committed at HEAD, relative to the monorepo root (whose `.bosun/` bosun runs on):
/// CI swaps the base branch's `.bosun/` into the checkout.
fn committed(path: &str) -> String {
    let out = std::process::Command::new("git")
        .args(["show", &format!("HEAD:{path}")])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("git show");
    assert!(out.status.success(), "git show HEAD:{path}");
    String::from_utf8(out.stdout).expect("utf-8")
}

/// The committed pin; `None` for `unreleased` (no release: bosun is built from the workspace).
/// Anything else must be an exact x.y.z (`semver` panics otherwise).
fn pin() -> Option<String> {
    let v = committed(".bosun/version").trim().to_string();
    if v == "unreleased" {
        return None;
    }
    semver(&v);
    Some(v)
}

/// The non-comment lines of the root workflows that would run a released bosun: read the pin,
/// install it from npm, or download a bosun release asset (other tools' downloads are fine).
fn release_fetches(files: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, text) in files {
        for line in text.lines() {
            let code = line.trim_start();
            if code.starts_with('#') || code.contains("::error::") {
                continue;
            }
            let download = ["releases/download", "gh release download"]
                .iter()
                .any(|k| code.contains(k))
                && code.contains("bosun");
            if download
                || [".bosun/version", "@keelstackhq/bosun"]
                    .iter()
                    .any(|k| code.contains(k))
            {
                out.push(format!("{name}: {}", line.trim()));
            }
        }
    }
    out
}

fn root_workflows() -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows");
    let mut files: Vec<(String, String)> = std::fs::read_dir(&dir)
        .expect("root .github/workflows")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "yml" || x == "yaml"))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read_to_string(&p).unwrap())
        })
        .collect();
    files.sort();
    files
}

/// The monorepo's workflows build bosun from the workspace; none reads the pin or runs a
/// released bosun, which cannot read the root config (.github/scripts/gate-workflows.test.sh
/// checks the build steps). Holds whatever the pin says: a pin is no reason to fetch a release.
#[test]
fn no_root_workflow_runs_a_released_bosun() {
    let files = root_workflows();
    assert!(files.len() >= 5, "{files:?}");
    let found = release_fetches(&files);
    assert!(
        found.is_empty(),
        "these run a released bosun, which cannot read .bosun/bosun.toml ([[folders]]): {found:?}"
    );
}

#[test]
fn an_unreleased_pin_needs_keys_a_listed_release_reads() {
    // `unreleased`: no pinned release to check against, so each key must at least be one the
    // newest entry of KNOWN (the first release that will read it) lists.
    if pin().is_some() {
        return;
    }
    // Every key is still one some future release lists: a key no entry of KNOWN has means the
    // table (and this rule) is out of date.
    let newest = KNOWN.last().unwrap().1;
    let config: toml::Table = committed(".bosun/bosun.toml").parse().expect("bosun.toml");
    for key in config.keys() {
        assert!(
            newest.contains(&key.as_str()),
            "`{key}` in .bosun/bosun.toml is in no release of KNOWN: add the first release that reads it"
        );
    }
}

#[test]
fn a_workflow_that_runs_a_release_is_caught() {
    let f = |t: &str| vec![("x.yml".to_string(), t.to_string())];
    assert!(release_fetches(&f("      - run: cargo build -p keel-bosun\n")).is_empty());
    assert!(release_fetches(&f("  # reads .bosun/version\n")).is_empty());
    let other =
        "        run: curl -fsSLO https://github.com/rhysd/actionlint/releases/download/v1/a.tgz\n";
    assert!(release_fetches(&f(other)).is_empty());
    let asset = "        run: curl -fsSLO https://github.com/keelstackhq/bosun/releases/download/v0.21.0/bosun-x\n";
    assert_eq!(release_fetches(&f(asset)).len(), 1);
    assert_eq!(
        release_fetches(&f("        run: v=\"$(cat .bosun/version)\"\n")).len(),
        1
    );
    assert_eq!(
        release_fetches(&f("      - run: npx -y @keelstackhq/bosun@0.21.0 gate\n")).len(),
        1
    );
}

#[test]
fn folders_is_known_from_0_22_0() {
    // The monorepo's root config needs it: a 0.21.0 pin (the release that is out) is refused.
    let has = |v: &str| known_for(KNOWN, v).is_some_and(|k| k.contains(&"folders"));
    assert!(!has("0.21.0"));
    assert!(has("0.22.0"));
}

#[test]
fn bosun_toml_uses_only_keys_the_pinned_release_knows() {
    let Some(version) = pin() else {
        return; // `unreleased`: an_unreleased_pin_needs_keys_a_listed_release_reads
    };
    let Some(keys) = known_for(KNOWN, &version) else {
        panic!(
            "add the top-level keys release {version} accepts to KNOWN in tests/pinned_config.rs"
        );
    };
    let config: toml::Table = committed(".bosun/bosun.toml").parse().expect("bosun.toml");
    for key in config.keys() {
        assert!(
            keys.contains(&key.as_str()),
            "`{key}` in .bosun/bosun.toml is unknown to the pinned release {version}: the gates on main would fail; set it in the pin PR of a release that reads it"
        );
    }
}

#[test]
fn limits_toml_uses_only_keys_the_pinned_release_knows() {
    let Some(version) = pin() else {
        return; // `unreleased`: bosun is built from the workspace (governance.rs parses it)
    };
    let Some(keys) = known_for(KNOWN_LIMITS, &version) else {
        panic!(
            "add the limits keys release {version} accepts to KNOWN_LIMITS in tests/pinned_config.rs"
        );
    };
    let unknown = unknown_limits(&committed(".bosun/limits.toml"), keys);
    assert!(
        unknown.is_empty(),
        "{unknown:?} in .bosun/limits.toml are unknown to the pinned release {version}: the gates on main would fail; set them in the pin PR of a release that reads them"
    );
}

#[test]
fn a_key_newer_than_the_pin_is_caught() {
    let known = KNOWN_LIMITS[0].1;
    assert_eq!(
        unknown_limits(
            "[waves]\nmax_parallel = 6\nrelay_waves = 6\n[cloud]\nx = 1\n",
            known
        ),
        ["cloud.x", "waves.relay_waves"]
    );
    assert!(unknown_limits("[waves]\n# relay_waves = 6\nmax_parallel = 6\n", known).is_empty());
}

#[test]
fn approvals_uses_only_keys_the_pinned_release_knows() {
    let Some(version) = pin() else {
        return; // `unreleased`: bosun is built from the workspace (governance.rs parses it)
    };
    let config: toml::Table = committed(".bosun/bosun.toml").parse().expect("bosun.toml");
    let Some(approvals) = config.get("approvals").and_then(|a| a.as_table()) else {
        return;
    };
    let Some(keys) = known_for(KNOWN_APPROVALS, &version) else {
        panic!(
            "add the `[approvals]` keys release {version} accepts to KNOWN_APPROVALS in tests/pinned_config.rs"
        );
    };
    for key in approvals.keys() {
        assert!(
            keys.contains(&key.as_str()),
            "`approvals.{key}` in .bosun/bosun.toml is unknown to the pinned release {version}: the gates on main would fail; set it in the pin PR of a release that reads it"
        );
    }
}

#[test]
fn approvals_policy_uses_only_values_the_pinned_release_knows() {
    let Some(version) = pin() else {
        return; // `unreleased`: bosun is built from the workspace (governance.rs parses it)
    };
    let config: toml::Table = committed(".bosun/bosun.toml").parse().expect("bosun.toml");
    let Some(policy) = config
        .get("approvals")
        .and_then(|a| a.get("policy"))
        .and_then(|p| p.as_str())
    else {
        return;
    };
    let known = policy_known(&version, policy).unwrap_or_else(|| {
        panic!("add the `[approvals] policy` values release {version} accepts to KNOWN_POLICIES in tests/pinned_config.rs")
    });
    assert!(
        known,
        "`approvals.policy = \"{policy}\"` in .bosun/bosun.toml is unknown to the pinned release {version}: the gates on main would fail; set it in the pin PR of a release that reads it"
    );
}

#[test]
fn policy_known_follows_the_release_table() {
    assert_eq!(policy_known("0.7.0", "autonomous"), Some(false));
    assert_eq!(policy_known("0.8.0", "autonomous"), Some(true));
    assert_eq!(policy_known("0.7.0", "owner-gated"), Some(true));
    assert_eq!(policy_known("0.4.0", "owner"), None);
}

#[test]
fn an_unlisted_release_falls_back_to_the_newest_older_entry() {
    // Key sets only grow, so the newest older release is a safe subset: no hand-added entry.
    assert_eq!(known_for(KNOWN, "0.99.0"), known_for(KNOWN, "0.22.0"));
    assert_eq!(known_for(KNOWN, "0.21.0"), known_for(KNOWN, "0.14.0"));
    assert_eq!(known_for(KNOWN, "0.20.0"), known_for(KNOWN, "0.14.0"));
    assert_eq!(known_for(KNOWN, "0.12.0"), known_for(KNOWN, "0.10.0"));
    assert_eq!(
        known_for(KNOWN_LIMITS, "0.9.3"),
        known_for(KNOWN_LIMITS, "0.9.0")
    );
    assert_eq!(policy_known("0.99.0", "autonomous"), Some(true));
    // Numeric order, not string order: 0.99.0 is newer than 0.10.0, 0.2.9 older than 0.3.0.
    assert_eq!(known_for(KNOWN, "0.2.9"), known_for(KNOWN, "0.2.3"));
    assert_eq!(known_for(KNOWN, "0.2.2"), None);
}

#[test]
fn release_tables_only_grow() {
    // The fallback in `known_for` is safe only while each release accepts every key the one
    // before it accepted.
    for (name, table) in [
        ("KNOWN", KNOWN),
        ("KNOWN_LIMITS", KNOWN_LIMITS),
        ("KNOWN_APPROVALS", KNOWN_APPROVALS),
        ("KNOWN_POLICIES", KNOWN_POLICIES),
    ] {
        for pair in table.windows(2) {
            let ((old, old_keys), (new, new_keys)) = (pair[0], pair[1]);
            assert!(
                semver(old) < semver(new),
                "{name}: {old} must come before {new}"
            );
            for key in old_keys {
                assert!(
                    new_keys.contains(key),
                    "{name}: {new} drops `{key}` that {old} accepts; known_for would fall back wrongly"
                );
            }
        }
    }
}

#[test]
fn review_models_is_known_from_0_11_0() {
    let has = |v: &str| known_for(KNOWN_APPROVALS, v).is_some_and(|k| k.contains(&"review_models"));
    assert!(!has("0.10.0"));
    assert!(has("0.11.0"));
}

#[test]
fn signer_is_known_from_0_15_0() {
    let has = |v: &str| known_for(KNOWN_APPROVALS, v).is_some_and(|k| k.contains(&"signer"));
    assert!(!has("0.14.0"));
    assert!(has("0.15.0"));
}

#[test]
fn quorum_is_known_from_0_22_0() {
    assert_eq!(policy_known("0.21.0", "quorum"), Some(false));
    assert_eq!(policy_known("0.22.0", "quorum"), Some(true));
    assert_eq!(policy_known("0.22.0", "autonomous"), Some(true));
}

#[test]
fn chat_is_known_from_0_22_0() {
    let has = |v: &str| known_for(KNOWN_APPROVALS, v).is_some_and(|k| k.contains(&"chat"));
    assert!(!has("0.21.0"));
    assert!(has("0.22.0"));
}

#[test]
fn bosun_repo_is_known_from_0_22_0() {
    let has = |v: &str| known_for(KNOWN_APPROVALS, v).is_some_and(|k| k.contains(&"bosun_repo"));
    assert!(!has("0.21.0"));
    assert!(has("0.22.0"));
}
