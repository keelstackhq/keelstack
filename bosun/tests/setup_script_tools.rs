//! BT: the channel's docs/setup-script.sh installs cargo-deny and cargo-nextest of a rust stack
//! from checksummed release binaries at the versions of `.bosun/pins.toml`, always, and never
//! runs `cargo install`. A missing pin or checksum, a failed download or a checksum mismatch
//! warns on stderr, skips that tool and leaves the exit at 0. No real network: fake `curl`, `npm`,
//! `cargo`, `rustup`; real `tar`, `sha256sum`, `install`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The template subset this script uses: `{% if var %}`, `{% if "piece" in pieces %}`,
/// `{% else %}`, `{% endif %}`, `{{ var }}`.
fn render(stack: &[&str], bosun_version: &str) -> String {
    let src = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("channel/pieces/core/files/docs/setup-script.sh.jinja"),
    )
    .unwrap();
    let var = |n: &str| match n {
        "bosun_version" => bosun_version.to_string(),
        "template.version" => "1.2.3".to_string(),
        "rust_path" | "ts_path" => ".".to_string(),
        other => panic!("unknown variable {other:?}"),
    };
    let mut out = String::new();
    // (outer active, condition, in else)
    let mut frames: Vec<(bool, bool, bool)> = Vec::new();
    let active = |f: &[(bool, bool, bool)]| f.last().is_none_or(|f| f.0 && f.1);
    let mut rest = src.as_str();
    while let Some(i) = rest.find('{') {
        let tail = &rest[i..];
        let (open, close) = if tail.starts_with("{{") {
            ("{{", "}}")
        } else if tail.starts_with("{%") {
            ("{%", "%}")
        } else {
            if active(&frames) {
                out.push_str(&rest[..=i]);
            }
            rest = &rest[i + 1..];
            continue;
        };
        if active(&frames) {
            out.push_str(&rest[..i]);
        }
        let end = tail.find(close).unwrap();
        let body = tail[open.len()..end].trim();
        rest = &tail[end + 2..];
        if open == "{{" {
            if active(&frames) {
                out.push_str(&var(body));
            }
            continue;
        }
        match body {
            "else" => {
                let f = frames.last_mut().unwrap();
                f.1 = !f.1;
                f.2 = true;
            }
            "endif" => {
                frames.pop().unwrap();
            }
            s => {
                let expr = s.strip_prefix("if ").unwrap_or_else(|| panic!("tag {s:?}"));
                let cond = match expr.split_once(" in ") {
                    Some((lit, "pieces")) => {
                        let p = lit.trim().trim_matches('"');
                        p == "core" || stack.contains(&p)
                    }
                    _ => !var(expr.trim()).is_empty(),
                };
                frames.push((active(&frames), cond, false));
            }
        }
    }
    if active(&frames) {
        out.push_str(rest);
    }
    out
}

struct Sandbox {
    root: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let s = Self {
            root: tempfile::tempdir().unwrap(),
        };
        for d in ["bin", "repo/.bosun"] {
            fs::create_dir_all(s.path(d)).unwrap();
        }
        let rec = format!(
            "echo \"$(basename \"$0\") $*\" >> {}",
            s.path("calls").display()
        );
        for t in ["cargo", "apt-get", "bosun", "rustup"] {
            s.stub(t, &rec);
        }
        s.stub(
            "npm",
            &format!("{rec}\n[ \"$1\" = pack ] && echo x.tgz; exit 0"),
        );
        s.stub("node", "[ \"$1\" = --version ] && echo v24.1.0; exit 0");
        s
    }

    fn path(&self, p: &str) -> PathBuf {
        self.root.path().join(p)
    }

    fn stub(&self, name: &str, body: &str) {
        let p = self.path("bin").join(name);
        fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.path("calls")).unwrap_or_default()
    }

    fn curls(&self) -> Vec<String> {
        self.calls()
            .lines()
            .filter(|l| l.starts_with("curl ") && l.contains("releases/download"))
            .map(str::to_string)
            .collect()
    }

    /// A release tarball with one executable `<name>`; returns its sha256.
    fn release(&self, name: &str) -> String {
        let src = self.path(&format!("src-{name}"));
        fs::create_dir_all(src.join("rel")).unwrap();
        let bin = src.join("rel").join(name);
        fs::write(&bin, format!("#!/bin/sh\necho '{name}'\n")).unwrap();
        fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let tgz = self.path(&format!("{name}.tgz"));
        let ok = Command::new("tar")
            .args([
                "-czf",
                tgz.to_str().unwrap(),
                "-C",
                src.to_str().unwrap(),
                ".",
            ])
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let sum = Command::new("sha256sum").arg(&tgz).output().unwrap().stdout;
        String::from_utf8(sum)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .to_string()
    }

    /// `curl` serves the fixture of the requested asset to its `-o` target.
    fn serve(&self) {
        self.stub(
            "curl",
            &format!(
                "echo \"curl $*\" >> {log}\nwhile [ $# -gt 0 ]; do [ \"$1\" = -o ] && o=\"$2\"; u=\"$1\"; shift; done\ncase \"$u\" in *cargo-deny*) cp {d} \"$o\" ;; *nextest*) cp {n} \"$o\" ;; *) exit 22 ;; esac",
                log = self.path("calls").display(),
                d = self.path("cargo-deny.tgz").display(),
                n = self.path("cargo-nextest.tgz").display(),
            ),
        );
    }

    fn pins(&self, text: &str) {
        fs::write(self.path("repo/.bosun/pins.toml"), text).unwrap();
    }

    fn rust_repo(&self) {
        fs::write(self.path("repo/Cargo.toml"), "[package]\nname = \"a\"\n").unwrap();
    }

    /// Run the script inside `repo/`; (exit ok, stderr).
    fn run(&self, script: &str) -> (bool, String) {
        fs::write(self.path("setup.sh"), script).unwrap();
        let path = format!(
            "{}:{}",
            self.path("bin").display(),
            std::env::var("PATH").unwrap()
        );
        let out = Command::new("bash")
            .arg(self.path("setup.sh"))
            .current_dir(self.path("repo"))
            .env("PATH", path)
            .env("SETUP_NODE_BIN_DIR", self.path("bin"))
            .env("SETUP_NODE_LINK_DIR", self.path("link"))
            .env("SETUP_NODE_PROFILE", self.path("profile.sh"))
            .env("SETUP_STATE_DIR", self.path("state"))
            .env("CARGO_HOME", self.path("cargo-home"))
            .output()
            .unwrap();
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

fn pins(nextest: &str, deny: &str) -> String {
    format!(
        "[rust]\nfallback = \"1.97\"\n\n[nextest]\nversion = \"0.9.1\"\nlinux_x64_sha256 = \"{nextest}\"\nlinux_arm64_sha256 = \"{nextest}\"\n\n[cargo-deny]\nversion = \"0.2.3\"\nlinux_x64_sha256 = \"{deny}\"\nlinux_arm64_sha256 = \"{deny}\"\n"
    )
}

fn no_cargo_install(sb: &Sandbox, ctx: &str) {
    let cargo: Vec<_> = sb
        .calls()
        .lines()
        .filter(|l| l.starts_with("cargo "))
        .map(str::to_string)
        .collect();
    assert!(cargo.is_empty(), "{ctx}: cargo was called: {cargo:?}");
}

#[test]
fn the_template_never_runs_cargo_install() {
    let script = render(&["rust"], "");
    assert!(
        !script.contains("cargo install"),
        "cargo install is still in the script"
    );
    assert!(!script.contains("cargo \"+"), "no cargo build fallback");
}

#[test]
fn a_rust_stack_installs_both_tools_from_checksummed_binaries_for_every_bosun_pin() {
    for bosun_version in ["0.11.0", "0.12.0", "0.24.0", "1.0.0", ""] {
        let sb = Sandbox::new();
        sb.rust_repo();
        let (d, n) = (sb.release("cargo-deny"), sb.release("cargo-nextest"));
        sb.pins(&pins(&n, &d));
        sb.serve();
        let (ok, err) = sb.run(&render(&["rust"], bosun_version));
        assert!(ok, "{bosun_version}: {err}");
        no_cargo_install(&sb, bosun_version);
        let urls = sb.curls();
        assert_eq!(urls.len(), 2, "{bosun_version}: {urls:?}");
        assert!(
            urls.iter().any(|u| u
                .contains("https://github.com/EmbarkStudios/cargo-deny/releases/download/0.2.3/")),
            "{urls:?}"
        );
        assert!(
            urls.iter()
                .any(|u| u.contains("/releases/download/cargo-nextest-0.9.1/cargo-nextest-0.9.1-")),
            "{urls:?}"
        );
        for t in ["cargo-deny", "cargo-nextest"] {
            assert!(
                sb.path(&format!("cargo-home/bin/{t}")).exists(),
                "{bosun_version}: {t}"
            );
        }

        // Warm: a rerun at the recorded checksums downloads nothing.
        let (ok, err) = sb.run(&render(&["rust"], bosun_version));
        assert!(ok, "{err}");
        assert_eq!(
            sb.curls().len(),
            2,
            "{bosun_version}: warm run downloaded again"
        );
    }
}

#[test]
fn a_checksum_mismatch_warns_skips_the_tool_and_exits_zero() {
    let sb = Sandbox::new();
    sb.rust_repo();
    let d = sb.release("cargo-deny");
    sb.release("cargo-nextest");
    sb.pins(&pins(&"0".repeat(64), &d)); // nextest's pin matches no fixture
    sb.serve();
    let (ok, err) = sb.run(&render(&["rust"], "0.24.0"));
    assert!(ok, "{err}");
    no_cargo_install(&sb, "mismatch");
    assert!(
        err.contains("cargo-nextest") && err.contains("sha256"),
        "{err}"
    );
    assert!(!sb.path("cargo-home/bin/cargo-nextest").exists());
    assert!(
        sb.path("cargo-home/bin/cargo-deny").exists(),
        "the other tool still installs"
    );
}

#[test]
fn a_failed_download_warns_skips_and_exits_zero() {
    let sb = Sandbox::new();
    sb.rust_repo();
    let (d, n) = (sb.release("cargo-deny"), sb.release("cargo-nextest"));
    sb.pins(&pins(&n, &d));
    sb.stub(
        "curl",
        &format!(
            "echo \"curl $*\" >> {}\nexit 22",
            sb.path("calls").display()
        ),
    );
    let (ok, err) = sb.run(&render(&["rust"], "0.24.0"));
    assert!(ok, "{err}");
    no_cargo_install(&sb, "download");
    for t in ["cargo-deny", "cargo-nextest"] {
        assert!(err.contains(t), "{t}: {err}");
        assert!(!sb.path(&format!("cargo-home/bin/{t}")).exists());
    }
}

#[test]
fn a_missing_pin_or_checksum_warns_skips_that_tool_and_exits_zero() {
    let cases: [(&str, String); 3] = [
        ("no pins file", String::new()),
        (
            "no nextest pin",
            "[cargo-deny]\nversion = \"0.2.3\"\nlinux_x64_sha256 = \"S\"\nlinux_arm64_sha256 = \"S\"\n".to_string(),
        ),
        (
            "no cargo-deny checksum",
            "[nextest]\nversion = \"0.9.1\"\nlinux_x64_sha256 = \"S\"\nlinux_arm64_sha256 = \"S\"\n\n[cargo-deny]\nversion = \"0.2.3\"\n".to_string(),
        ),
    ];
    for (name, text) in cases {
        let sb = Sandbox::new();
        sb.rust_repo();
        let (d, n) = (sb.release("cargo-deny"), sb.release("cargo-nextest"));
        if !text.is_empty() {
            sb.pins(&text.replace('S', if name == "no nextest pin" { &d } else { &n }));
        }
        sb.serve();
        let (ok, err) = sb.run(&render(&["rust"], "0.24.0"));
        assert!(ok, "{name}: {err}");
        no_cargo_install(&sb, name);
        assert!(err.contains("setup-script:"), "{name}: {err}");
        match name {
            "no pins file" => assert!(sb.curls().is_empty(), "{name}: {:?}", sb.curls()),
            "no nextest pin" => {
                assert!(!sb.path("cargo-home/bin/cargo-nextest").exists());
                assert!(
                    sb.path("cargo-home/bin/cargo-deny").exists(),
                    "{name}: {err}"
                );
            }
            _ => {
                assert!(!sb.path("cargo-home/bin/cargo-deny").exists(), "{name}");
                assert!(
                    sb.path("cargo-home/bin/cargo-nextest").exists(),
                    "{name}: {err}"
                );
            }
        }
    }
}

#[test]
fn a_checkout_without_rust_installs_neither_tool() {
    for stack in [&["rust"][..], &[][..]] {
        let sb = Sandbox::new(); // no Cargo.toml in repo/
        let (d, n) = (sb.release("cargo-deny"), sb.release("cargo-nextest"));
        sb.pins(&pins(&n, &d));
        sb.serve();
        let (ok, err) = sb.run(&render(stack, "0.24.0"));
        assert!(ok, "{stack:?}: {err}");
        assert!(sb.curls().is_empty(), "{stack:?}: {:?}", sb.curls());
        no_cargo_install(&sb, "non-rust");
    }
}
