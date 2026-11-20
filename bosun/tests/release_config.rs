//! The release files must agree with each other (spec D24, docs/releasing.md). No network: the
//! tests read the repo's own files.

use std::fs;
use std::path::{Path, PathBuf};

const PLATFORMS: [(&str, &str, &str, &str); 5] = [
    ("linux-x64", "x86_64-unknown-linux-musl", "linux", "x64"),
    (
        "linux-arm64",
        "aarch64-unknown-linux-musl",
        "linux",
        "arm64",
    ),
    ("darwin-x64", "x86_64-apple-darwin", "darwin", "x64"),
    ("darwin-arm64", "aarch64-apple-darwin", "darwin", "arm64"),
    ("win32-x64", "x86_64-pc-windows-msvc", "win32", "x64"),
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_toml(path: &Path) -> toml::Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn read_json(path: &Path) -> serde_json::Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn cargo_version() -> String {
    // `version.workspace = true`: the compiled-in package version is the workspace's.
    env!("CARGO_PKG_VERSION").to_string()
}

fn npm_dir(name: &str) -> PathBuf {
    root().join("npm").join(name)
}

fn string_list(v: &serde_json::Value) -> Vec<&str> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .collect()
}

#[test]
fn cargo_package_is_publishable() {
    let cargo = read_toml(&root().join("Cargo.toml"));
    let package = &cargo["package"];
    assert!(
        package.get("publish").is_none(),
        "publish = false must be gone"
    );
    for key in [
        "readme",
        "keywords",
        "categories",
        "include",
        "description",
        "license",
    ] {
        assert!(package.get(key).is_some(), "[package] needs {key}");
    }
    let include: Vec<&str> = package["include"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        include
            .iter()
            .all(|p| !p.contains("tests") && !p.contains("npm")),
        "{include:?}"
    );
}

#[test]
fn hoist_toml_targets_are_the_five_platforms() {
    let hoist = read_toml(&root().join("hoist.toml"));
    let publish = &hoist["publish"];
    assert!(publish["crates"].as_table().unwrap().is_empty());
    let release = &publish["github_release"];
    assert_eq!(release["bin"].as_str(), Some("bosun"));
    let mut targets: Vec<&str> = release["targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    targets.sort();
    let mut want: Vec<&str> = PLATFORMS.iter().map(|p| p.1).collect();
    want.sort();
    assert_eq!(targets, want);
}

#[test]
fn launcher_and_platform_packages_agree_with_cargo() {
    let version = cargo_version();
    let launcher = read_json(&npm_dir("bosun").join("package.json"));
    assert_eq!(launcher["name"], "@keelstackhq/bosun");
    assert_eq!(launcher["version"].as_str(), Some(version.as_str()));
    assert_eq!(launcher["bin"]["bosun"], "bin/bosun.js");

    let opt = launcher["optionalDependencies"].as_object().unwrap();
    let mut keys: Vec<String> = opt.keys().cloned().collect();
    keys.sort();
    let mut want: Vec<String> = PLATFORMS
        .iter()
        .map(|p| format!("@keelstackhq/bosun-{}", p.0))
        .collect();
    want.sort();
    assert_eq!(keys, want);
    for (name, value) in opt {
        assert_eq!(
            value.as_str(),
            Some(version.as_str()),
            "optionalDependencies {name}"
        );
    }

    let hoist = read_toml(&npm_dir("bosun").join("hoist.toml"));
    assert_eq!(hoist["publish"]["npm"]["launcher"].as_bool(), Some(true));

    for (platform, _target, os, cpu) in PLATFORMS {
        let pkg = read_json(&npm_dir(&format!("bosun-{platform}")).join("package.json"));
        assert_eq!(pkg["name"], format!("@keelstackhq/bosun-{platform}"));
        assert_eq!(
            pkg["version"].as_str(),
            Some(version.as_str()),
            "{platform}"
        );
        assert_eq!(string_list(&pkg["os"]), [os], "{platform} os");
        assert_eq!(string_list(&pkg["cpu"]), [cpu], "{platform} cpu");
        assert_eq!(string_list(&pkg["files"]), ["bin/"], "{platform} files");
        assert_eq!(pkg["publishConfig"]["access"], "public");
    }
}

#[test]
fn every_npm_package_is_a_known_one() {
    let mut found: Vec<String> = fs::read_dir(root().join("npm"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    found.sort();
    let mut want: Vec<String> = PLATFORMS.iter().map(|p| format!("bosun-{}", p.0)).collect();
    want.push("bosun".into());
    // Upstream cargo-deny republished (cargo-deny-npm.yml), outside bosun's release.
    for p in [
        "cargo-deny",
        "cargo-deny-linux-x64",
        "cargo-deny-linux-arm64",
    ] {
        want.push(p.into());
    }
    want.sort();
    assert_eq!(found, want);
}

#[test]
fn pnpm_workspace_lists_the_npm_packages() {
    let text = fs::read_to_string(root().join("pnpm-workspace.yaml")).unwrap();
    // Each bosun package by path; npm/cargo-deny* is not bosun's, so hoist must not release it.
    let listed: Vec<&str> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- "))
        .collect();
    let mut want: Vec<String> = PLATFORMS
        .iter()
        .map(|p| format!("npm/bosun-{}", p.0))
        .collect();
    want.insert(0, "npm/bosun".into());
    assert_eq!(listed, want, "{text}");
    assert!(text.contains("linkWorkspacePackages: true"));
}

#[test]
fn crate_is_keel_bosun_and_ships_what_the_binary_embeds() {
    let cargo = read_toml(&root().join("Cargo.toml"));
    assert_eq!(cargo["package"]["name"].as_str(), Some("keel-bosun"));
    let include: Vec<&str> = cargo["package"]["include"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    // src/gates embeds profiles/*.toml with include_str!, so the .crate needs them to build.
    for want in ["/src/**", "/profiles/**", "/LICENSE"] {
        assert!(include.contains(&want), "include lacks {want}: {include:?}");
    }
    assert!(
        root().join("LICENSE").is_file(),
        "license = \"MIT\" needs a LICENSE file"
    );
}

/// `## v<version>` (written by hand, as v0.1.0) or `## <version> (<date>)`, the heading
/// `hoist release prepare` writes and `hoist release publish` reads the release notes from.
#[test]
fn changelog_has_the_cargo_version() {
    let text = fs::read_to_string(root().join("CHANGELOG.md")).unwrap();
    let v = cargo_version();
    let (plain, dated) = (format!("## v{v}"), format!("## {v} ("));
    assert!(
        text.lines()
            .any(|l| l.trim_end() == plain || l.starts_with(&dated)),
        "CHANGELOG.md has no `{plain}` or `{dated}<date>)`"
    );
}

fn workflow(name: &str) -> String {
    fs::read_to_string(root().join("../.github/workflows").join(name)).unwrap()
}

/// The text of one top-level job: from `  <name>:` up to the next two-space-indented key.
fn job(text: &str, name: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in text.lines() {
        let is_key =
            line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':');
        if is_key {
            inside = line.trim() == format!("{name}:");
        }
        if inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    assert!(!out.is_empty(), "job {name}");
    out
}

#[test]
fn every_action_is_pinned_to_a_commit_sha() {
    // bosun's older workflows pin tags; the one that holds publish rights pins commits.
    for file in ["release.yml"] {
        // A comment that names an action (`uses: x@v0` in prose) is not a step.
        for line in workflow(file)
            .lines()
            .filter(|l| l.contains("uses:") && !l.trim_start().starts_with('#'))
        {
            let spec = line.split("uses:").nth(1).unwrap().trim();
            let (reference, comment) = spec.split_once(" # ").unwrap_or((spec, ""));
            let sha = reference.rsplit('@').next().unwrap();
            assert!(
                sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()),
                "{file}: not pinned: {line}"
            );
            assert!(
                !comment.is_empty(),
                "{file}: needs a version comment: {line}"
            );
        }
    }
}

#[test]
fn release_workflow_is_gated_so_a_merge_publishes_nothing() {
    // The monorepo's lockstep release (D-023): a merge publishes only when it changed the
    // workspace version; job `plan` decides, and every publishing job needs its verdict.
    let text = workflow("release.yml");
    let plan = job(&text, "plan");
    assert!(
        plan.contains("if [ \"$version\" != \"$prev\" ]; then release=true; fi"),
        "a merge releases only a changed workspace version:\n{plan}"
    );
    assert!(plan.contains("echo \"release=$release\""));
    assert!(!plan.contains("environment:") && !plan.contains(": write"));
    // A manual run from a branch never publishes, even before the environment has a policy.
    assert!(
        plan.contains("if [ \"$REF\" != refs/heads/main ]; then"),
        "{plan}"
    );
    for name in ["build", "release", "crates", "npm", "major"] {
        let j = job(&text, name);
        assert!(
            j.contains(
                "github.event_name == 'workflow_dispatch' && needs.plan.outputs.release == 'true'"
            ),
            "{name} must require the plan:\n{j}"
        );
    }
    for name in ["release", "crates", "npm", "major"] {
        assert!(
            job(&text, name).contains("    environment: release\n"),
            "{name} publishes from environment release"
        );
    }
    let code: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    // A step ends at the next step or at the next job (a line at job indent).
    let steps = code.split("\n      - ").map(|s| {
        let end = s
            .match_indices("\n  ")
            .find(|(i, _)| !s[i + 3..].starts_with(' '))
            .map_or(s.len(), |(i, _)| i);
        &s[..end]
    });
    for step in steps.filter(|s| s.contains("GH_TOKEN")) {
        assert!(
            !step.contains("cargo "),
            "a step with GH_TOKEN runs cargo:\n{step}"
        );
    }
    // A job-level GH_TOKEN reaches every step of the job.
    for name in [
        "plan", "dispatch", "build", "action", "release", "crates", "npm", "major",
    ] {
        let j = job(&text, name);
        let head = &j[..j.find("\n    steps:\n").unwrap_or(j.len())];
        if head.contains("GH_TOKEN") {
            let code: Vec<&str> = j
                .lines()
                .filter(|l| !l.trim_start().starts_with('#'))
                .collect();
            assert!(
                !code.iter().any(|l| l.contains("cargo ")),
                "job {name} has GH_TOKEN and runs cargo"
            );
        }
    }
}

#[test]
fn release_job_builds_the_bosun_binary_and_ci_runs_the_launcher_tests() {
    let text = workflow("release.yml");
    let build = job(&text, "build");
    assert!(
        build.contains("for pair in bosun:keel-bosun ")
            && build.contains("cp \"$rel/$t$ext\" \"out/$t-$TARGET$ext\""),
        "build copies the bosun binary"
    );
    assert!(
        build.contains("for t in bosun berth; do")
            && build.contains("test \"$got\" = \"$t $VERSION\""),
        "smoke test of --version"
    );
    let ci = workflow("ci.yml");
    let bosun = &ci[ci.find("\n  ci-bosun:\n").expect("a ci-bosun job")..];
    let bosun = &bosun[..bosun[1..].find("\n  ci-").map_or(bosun.len(), |i| i + 1)];
    assert!(
        bosun.contains("node --test npm/*/test/*.test.js"),
        "ci.yml runs the launcher tests"
    );
}
