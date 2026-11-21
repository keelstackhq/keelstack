//! CI reads test-integrity's allowed kinds from the PR's linked task in a step of its own, before
//! the gates, so the token never reaches the step where lint and test run PR code (spec §7).
use std::path::Path;

fn read(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap()
}

/// The text of the step whose `name:` is `name`, up to the next step at the same indent.
fn step<'a>(yaml: &'a str, name: &str) -> &'a str {
    let at = yaml
        .find(&format!("name: {name}\n"))
        .unwrap_or_else(|| panic!("no step {name}"));
    // A step may start mid-line, after a template tag like `{% raw %}`.
    let start = yaml[..at].rfind("      - ").unwrap();
    let end = yaml[at..].find("\n      - ").map_or(yaml.len(), |i| at + i);
    &yaml[start..end]
}

#[test]
fn the_allow_list_is_read_before_the_gates_and_only_there() {
    // The monorepo's ci.yml runs them in job ci-gates, whose gates step names the gates it runs.
    for (rel, gates_step) in [
        (
            "../.github/workflows/ci.yml",
            "bosun gates (test-integrity, ci-minutes)",
        ),
        (
            "channel/pieces/core/files/.github/workflows/bosun.yml.jinja",
            "bosun gates",
        ),
    ] {
        let y = read(rel);
        let allow = step(&y, "task allow list");
        let gates = step(&y, gates_step);
        assert!(
            y.find(allow).unwrap() < y.find(gates).unwrap(),
            "{rel}: order"
        );
        assert!(allow.contains("id: allow"), "{rel}");
        assert!(allow.contains("GITHUB_TOKEN: ${{ github.token }}"), "{rel}");
        assert!(allow.contains("test-integrity --pr \"$PR\""), "{rel}");
        assert!(
            allow.contains("github.event_name == 'pull_request'"),
            "{rel}"
        );
        assert!(allow.contains("sha256sum -c"), "{rel}: the checked binary");
        // Only the four kinds can come out, whatever the task says.
        assert!(
            allow.contains(r#"select(IN("delete-test","skip","only","fewer-asserts"))"#),
            "{rel}"
        );
        assert!(
            gates.contains("BOSUN_INTEGRITY_ALLOW: ${{ steps.allow.outputs.kinds }}"),
            "{rel}"
        );
        for leak in ["GITHUB_TOKEN", "GH_TOKEN", "github.token", "secrets."] {
            assert!(!gates.contains(leak), "{rel}: {leak} in the gates step");
        }
        // The workflow's permissions, or (monorepo) those of the job that reads the allow list.
        let perms = match y.find("\n  ci-gates:\n") {
            Some(job) => {
                let job = &y[job..];
                let at = job
                    .find("\n    permissions:\n")
                    .expect("ci-gates permissions");
                job[at + 1..]
                    .lines()
                    .skip(1)
                    .take_while(|l| l.starts_with("      "))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            None => {
                let perms = &y[y.find("\npermissions:\n").unwrap()..];
                perms[..perms[1..].find("\n\n").unwrap() + 1].to_string()
            }
        };
        for p in ["contents: read", "issues: read", "pull-requests: read"] {
            assert!(perms.contains(p), "{rel}: {p}");
        }
        assert!(!perms.contains("write"), "{rel}: read only");
    }
}
