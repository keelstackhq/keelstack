use crate::exec::{Cmd, Runner};
use crate::pins::{ToolPin, platform_key};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha512};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

pub fn sha512_b64(bytes: &[u8]) -> String {
    STANDARD.encode(Sha512::digest(bytes))
}

/// `linux_x64` -> `linux-x64`, `darwin_arm64` -> `darwin-arm64` (`platform_key` already uses
/// npm's OS names).
pub fn npm_platform(key: &str) -> String {
    let (os, arch) = key.split_once('_').unwrap_or((key, ""));
    format!("{os}-{arch}")
}

struct TmpDir(PathBuf);
impl TmpDir {
    fn new(tool: &str) -> std::io::Result<TmpDir> {
        static N: AtomicUsize = AtomicUsize::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let p = std::env::temp_dir().join(format!(
            "keel-npm-{tool}-{}-{nanos}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p)?;
        Ok(TmpDir(p))
    }
}
impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `npm pack --silent <spec>` in `dir`; verify the tarball's sha512 against `want`.
fn pack_verified(r: &dyn Runner, dir: &Path, spec: &str, want: &str) -> Result<PathBuf, String> {
    let c = Cmd::new("npm", &["pack", "--silent", spec]).dir(dir);
    let o = r.run(&c);
    let name = o
        .stdout
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .map(str::trim);
    let (0, Some(name)) = (o.code, name) else {
        return Err(super::failure(&c, &o));
    };
    let path = dir.join(name);
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read the packed {spec}: {e}"))?;
    let got = sha512_b64(&bytes);
    if got != want {
        return Err(format!(
            "sha512 mismatch for {spec}: expected {want}, got {got}; nothing was installed. \
             Update keel or point KEEL_PINS at a pins file that matches the registry"
        ));
    }
    Ok(path)
}

/// Install `tool` at its pin (§6): verify both tarballs, then `npm install -g` them.
/// Never installs an unverified tarball.
pub fn install_pinned(r: &dyn Runner, tool: &str, pin: &ToolPin) -> Result<(), String> {
    let v = &pin.version;
    let key = platform_key();
    let unverified = |why: String| {
        format!(
            "cannot install {tool} {v}: {why}; update keel or set KEEL_PINS to a pins file with \
             its sha512s (keel never installs an unverified tool)"
        )
    };
    let Some(sha) = &pin.sha512 else {
        return Err(unverified("no sha512 is recorded for this version".into()));
    };
    let Some(plat_sha) = sha.platform.get(&key) else {
        return Err(unverified(format!(
            "no sha512 is recorded for platform {key}"
        )));
    };
    let tmp = TmpDir::new(tool).map_err(|e| format!("cannot make a temp dir: {e}"))?;
    let launcher = pack_verified(
        r,
        &tmp.0,
        &format!("@keelstackhq/{tool}@{v}"),
        &sha.launcher,
    )?;
    let platform = pack_verified(
        r,
        &tmp.0,
        &format!("@keelstackhq/{tool}-{}@{v}", npm_platform(&key)),
        plat_sha,
    )?;
    let (l, p) = (launcher.to_string_lossy(), platform.to_string_lossy());
    let c = Cmd::new(
        "npm",
        &[
            "install",
            "-g",
            "--ignore-scripts",
            "--omit=optional",
            &l,
            &p,
        ],
    )
    .dir(&tmp.0);
    let o = r.run(&c);
    if o.code != 0 {
        return Err(super::failure(&c, &o));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_names() {
        assert_eq!(npm_platform("linux_x64"), "linux-x64");
        assert_eq!(npm_platform("darwin_arm64"), "darwin-arm64");
        assert_eq!(npm_platform("win32_x64"), "win32-x64");
    }
}
