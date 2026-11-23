//! The quorum trust root is a property, not a list of files: everything that builds or decides
//! the judge (D-022, spec §8). This walks the files that exist on disk, so a file added under a
//! code root's `src/` or `profiles/` is covered without a list edit.

use bosun::core::risk::trust_root_changed;
use std::fs;
use std::path::Path;

fn files_under(base: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_under(base, &path, out);
        } else {
            let rel = path.strip_prefix(base).unwrap();
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn every_file_under_a_code_roots_src_profiles_and_channel_is_trust_root() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for d in ["src", "profiles", "channel"] {
        files_under(manifest, &manifest.join(d), &mut files);
    }
    assert!(files.iter().any(|f| f == "src/core/risk.rs"));
    assert!(files.iter().any(|f| f == "profiles/rust.toml"));
    assert!(files.iter().any(|f| f.starts_with("channel/")));
    assert!(files.len() > 50, "walked only {}", files.len());
    // The code root is the repo root (bosun's own repo) or `bosun/` (the monorepo).
    for (root, prefix) in [("", ""), ("bosun", "bosun/")] {
        let roots = vec![root.to_string()];
        for f in &files {
            let p = format!("{prefix}{f}");
            assert_eq!(
                trust_root_changed(&[(p.as_str(), None)], &roots),
                [p.as_str()],
                "{p}"
            );
            // A rename away from it counts too.
            let renamed = [("docs/z.md", Some(p.as_str()))];
            assert_eq!(trust_root_changed(&renamed, &roots).len(), 1, "{p}");
            // The channel copies of the trust-root workflows are trust root in every folder and
            // every repo (`**/workflows/risk.yml*`): the negative checks are for code files.
            if f.starts_with("channel/") {
                continue;
            }
            // Another folder's file of the same name is not trust root.
            let other = format!("splice/{f}");
            let other = [(other.as_str(), None)];
            assert!(trust_root_changed(&other, &roots).is_empty(), "{f}");
            // With no code root (a consumer repo) it is no trust root either.
            assert!(
                trust_root_changed(&[(f.as_str(), None)], &[]).is_empty(),
                "{f}"
            );
        }
    }
}
