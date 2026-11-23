//! The owner-only paths agree everywhere they are written down: `.github/CODEOWNERS` names
//! exactly the paths `.bosun/risk.toml` and the critical floor give to the owner, `keel.toml`
//! is governance, and `.claude/settings.json` denies what agents never do (D-006, D-022).

use bosun::config::Config;
use bosun::core::glob::Glob;
use bosun::core::risk::{Approval, Rules, critical_globs};
use serde_json::Value;
use std::fs;
use std::path::Path;

/// `path` relative to the monorepo root (the working tree).
fn read(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(path))
        .unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// risk-check's matching: a glob matches a path in any case.
fn any_case(g: &Glob, path: &str) -> bool {
    let lower = path.to_lowercase();
    g.matches(path)
        || g.matches(&lower)
        || Glob::parse(&g.as_str().to_lowercase()).is_ok_and(|l| l.matches(&lower))
}

/// Lower-case risk globs of names that are conventionally written in another case: CODEOWNERS is
/// case-sensitive, so it lists both spellings.
const CANONICAL: [(&str, &str); 5] = [
    ("agents.md", "AGENTS.md"),
    ("claude.md", "CLAUDE.md"),
    ("claude.local.md", "CLAUDE.local.md"),
    ("jenkinsfile", "Jenkinsfile"),
    ("codeowners", "CODEOWNERS"),
];

/// The CODEOWNERS patterns (one owner each), in file order.
fn codeowners() -> Vec<String> {
    read(".github/CODEOWNERS")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut w = l.split_whitespace();
            let p = w.next().unwrap().to_string();
            assert_eq!(w.collect::<Vec<_>>(), ["@urbanisierung"], "{l}");
            p
        })
        .collect()
}

/// A CODEOWNERS pattern (gitignore rules: a leading or middle `/` anchors it at the root, else
/// it matches at any depth; a trailing `/` is a directory; a name also covers what is under it)
/// as bosun globs, matched case-sensitively like GitHub does.
fn pattern_globs(p: &str) -> Vec<Glob> {
    let dir = p.ends_with('/');
    let core = p.trim_end_matches('/');
    let body = core.trim_start_matches('/');
    let anchored = core.starts_with('/') || body.contains('/');
    let base = if anchored {
        body.to_string()
    } else {
        format!("**/{body}")
    };
    let texts = if dir {
        vec![format!("{base}/**")]
    } else {
        vec![base.clone(), format!("{base}/**")]
    };
    texts.iter().map(|t| Glob::parse(t).unwrap()).collect()
}

/// Concrete paths a risk glob matches: `**/` at the root and deeper, `/**` with a file under it,
/// `*` as one letter, and the canonical spelling of a lower-case name.
fn concrete(g: &str) -> Vec<String> {
    let (prefixes, rest) = match g.strip_prefix("**/") {
        Some(r) => (vec!["", "a/b/"], r),
        None => (vec![""], g),
    };
    let rest = match rest.strip_suffix("/**") {
        Some(d) => format!("{d}/inner.rs"),
        None => rest.to_string(),
    };
    let rest = rest.replace('*', "a");
    let mut out = Vec::new();
    for p in prefixes {
        out.push(format!("{p}{rest}"));
        let (dir, name) = rest
            .rsplit_once('/')
            .map_or(("", rest.as_str()), |(d, n)| (d, n));
        if let Some((_, canon)) = CANONICAL.iter().find(|(l, _)| *l == name) {
            let dir = if dir.is_empty() {
                String::new()
            } else {
                format!("{dir}/")
            };
            out.push(format!("{p}{dir}{canon}"));
        }
    }
    out
}

/// The owner-only globs: every owner-class rule of `.bosun/risk.toml` re-rooted under bosun's
/// code root (as risk-check does), and the critical floor there, where no delegate approves.
fn owner_globs() -> Vec<Glob> {
    let roots = vec!["bosun".to_string()];
    let rules = Rules::parse(&read(".bosun/risk.toml"))
        .unwrap()
        .rooted(&roots);
    let mut out: Vec<Glob> = (rules.rules.iter())
        .filter(|r| r.approval == Approval::Owner)
        .flat_map(|r| r.paths.clone())
        .collect();
    out.extend(critical_globs(&roots));
    // Under quorum (D-022) bosun's own code root is trust root too, and a rule: everything under
    // `src/**`, `profiles/**` and `channel/**` (compiled in), the manifest, build script, launchers.
    let quorum = bosun::core::folders::under("bosun", &bosun::core::approve::QUORUM_CODE_FILES);
    out.extend(quorum.iter().map(|g| Glob::parse(g).unwrap()));
    // And the quorum trust root list itself (`risk::TRUST_ROOT`, D-022): every glob of it has a
    // line, every line maps to a glob.
    out.extend(
        bosun::core::risk::TRUST_ROOT
            .iter()
            .map(|g| Glob::parse(g).unwrap()),
    );
    // `trust_root_changed` also reads the list and the trust files under each code root
    // (`bosun/.bosun/**`, `bosun/scripts/**`, ...): the copies a folder's own file shadows are
    // filtered by `other_folder_copy` where the globs are used.
    let all: Vec<&str> = (bosun::core::risk::TRUST_ROOT.iter())
        .chain(bosun::core::approve::TRUST_FILES.iter())
        .copied()
        .collect();
    out.extend(
        bosun::core::folders::under("bosun", &all)
            .iter()
            .map(|g| Glob::parse(g).unwrap()),
    );
    out
}

/// A re-rooted copy of another folder's own glob (`bosun/splice/.bosun/**`) names no real path.
fn other_folder_copy(g: &str) -> bool {
    let folders = Config::folder_paths_lenient(&read(".bosun/bosun.toml"));
    g.strip_prefix("bosun/").is_some_and(|rest| {
        folders
            .iter()
            .any(|f| rest.starts_with(&format!("{f}/")) || rest == f)
    })
}

#[test]
fn every_owner_only_risk_glob_has_a_codeowners_line() {
    let lines = codeowners();
    let globs: Vec<Glob> = lines.iter().flat_map(|p| pattern_globs(p)).collect();
    let mut missing = Vec::new();
    for g in owner_globs() {
        if other_folder_copy(g.as_str()) {
            continue;
        }
        for c in concrete(g.as_str()) {
            assert!(any_case(&g, &c), "{}: {c}", g.as_str());
            if !globs.iter().any(|x| x.matches(&c)) {
                missing.push(format!("{} ({c})", g.as_str()));
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "no CODEOWNERS line covers:\n{}",
        missing.join("\n")
    );
}

#[test]
fn every_codeowners_line_maps_to_an_owner_only_rule() {
    let owner = owner_globs();
    let mut stray = Vec::new();
    for p in codeowners() {
        let core = p.trim_end_matches('/');
        let body = core.trim_start_matches('/').replace('*', "a");
        let anchored = core.starts_with('/') || body.contains('/');
        let leaf = if p.ends_with('/') {
            format!("{body}/x")
        } else {
            body
        };
        let mut reps = vec![leaf.clone()];
        if !anchored {
            reps.push(format!("a/b/{leaf}"));
        }
        for r in reps {
            if !owner.iter().any(|g| any_case(g, &r)) {
                stray.push(format!("{p} ({r})"));
            }
        }
    }
    assert!(
        stray.is_empty(),
        "CODEOWNERS lines with no owner-only rule:\n{}",
        stray.join("\n")
    );
}

/// The pattern conversion follows GitHub's rules.
#[test]
fn codeowners_patterns_follow_gitignore_anchoring() {
    let m = |p: &str, path: &str| pattern_globs(p).iter().any(|g| g.matches(path));
    assert!(m("/.bosun/", ".bosun/risk.toml"));
    assert!(!m("/.bosun/", "x/.bosun/risk.toml"));
    assert!(m("AGENTS.md", "AGENTS.md") && m("AGENTS.md", "a/b/AGENTS.md"));
    assert!(!m("AGENTS.md", "a/agents.md"));
    assert!(m("hoard/action.yml", "hoard/action.yml"));
    assert!(!m("hoard/action.yml", "x/hoard/action.yml"));
    assert!(m("**/.yarn/plugins/", "a/.yarn/plugins/p.cjs"));
    assert!(m("*.sops.*", "a/b/x.sops.yaml"));
}

/// `keel up` applies `keel.toml` (with hoist.toml): governance, owner only.
#[test]
fn keel_toml_is_r_gov() {
    let rules = Rules::parse(&read(".bosun/risk.toml")).unwrap();
    let ids: Vec<&str> = (rules.rules.iter())
        .filter(|r| r.paths.iter().any(|g| any_case(g, "keel.toml")))
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(ids, ["R-gov"]);
    let gov = rules.rules.iter().find(|r| r.id == "R-gov").unwrap();
    assert_eq!(gov.approval, Approval::Owner);
    assert!(codeowners().iter().any(|p| p == "/keel.toml"));
}

/// The site's own governance paths are R-gov like every other folder's (D-024).
#[test]
fn site_governance_paths_are_r_gov() {
    let rules = Rules::parse(&read(".bosun/risk.toml")).unwrap();
    let gov = rules.rules.iter().find(|r| r.id == "R-gov").unwrap();
    for p in [
        "site/docs/setup-script.sh",
        "site/docs/agents/project.md",
        "site/REVIEW.md",
        "site/.bosun/risk.toml",
        "site/governance/owners.toml",
        "site/.github/CODEOWNERS",
        "site/.claude-plugin/marketplace.json",
        "site/.splice/state.toml",
    ] {
        assert!(gov.paths.iter().any(|g| any_case(g, p)), "{p}");
    }
}

/// What agents never do (D-006, D-022): merge, tag, release, force-push. Deny rules only add.
#[test]
fn settings_deny_merges_tags_releases_and_force_pushes() {
    let s: Value = serde_json::from_str(&read(".claude/settings.json")).unwrap();
    let deny: Vec<&str> = s["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for rule in [
        "Bash(rm -rf /)",
        "Bash(git push --force:*)",
        "Bash(git push -f:*)",
        "Bash(git push --force-with-lease:*)",
        "Bash(git tag:*)",
        "Bash(npm publish:*)",
        "Bash(gh release create:*)",
        "Bash(gh pr merge:*)",
        "Bash(git push --tags:*)",
        "Bash(git push origin --tags:*)",
        "Bash(git push * --tags*)",
        "Bash(git push origin v*)",
        "Bash(git push * v*)",
        "Bash(git push origin refs/tags/*)",
        "Bash(git push * refs/tags/*)",
        "Bash(git push origin +*)",
        "Bash(git push +*)",
        "Bash(git push * +*)",
        "Read(./.env)",
        "Read(./.env.*)",
    ] {
        assert!(deny.contains(&rule), "missing deny rule {rule}");
    }
    assert!(
        s["permissions"]["allow"].is_null(),
        "settings.json only denies"
    );
    let m = &s["extraKnownMarketplaces"]["keelstack"]["source"];
    assert_eq!(m["source"], "github");
    assert_eq!(m["repo"], "keelstackhq/keelstack");
    assert_eq!(m["ref"], "main");
    assert_eq!(m["path"], "bosun/.claude-plugin/marketplace.json");
    assert_eq!(m["sparsePaths"][0], "bosun/.claude-plugin");
}
