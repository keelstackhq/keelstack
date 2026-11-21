//! The release-tags deploy key (spec §6, `tags_bypass = "deploy_key"`): an ed25519 keypair
//! made in memory with ring, as an OpenSSH public key line and an unencrypted
//! `openssh-key-v1` private key that `ssh` and git read. The private key is a [`Secret`]: it
//! is only ever sealed into a GitHub secret, never printed, logged or written to disk.

use crate::auth::Secret;
use crate::error::HoistError;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{Ed25519KeyPair, KeyPair};
use zeroize::Zeroizing;

const KEY_TYPE: &[u8] = b"ssh-ed25519";
const BEGIN: &str = "-----BEGIN OPENSSH PRIVATE KEY-----";
const END: &str = "-----END OPENSSH PRIVATE KEY-----";

/// A new deploy keypair.
pub struct TagKey {
    /// `ssh-ed25519 <base64>`: what `POST /repos/{o}/{r}/keys` takes.
    pub public: String,
    /// The OpenSSH private key (PEM-like text, trailing newline).
    pub private: Secret,
}

impl std::fmt::Debug for TagKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TagKey")
            .field("public", &self.public)
            .field("private", &"***")
            .finish()
    }
}

/// Generate an ed25519 keypair: a 32-byte seed from the system RNG, the public key from ring.
pub fn generate() -> Result<TagKey, HoistError> {
    let rng = SystemRandom::new();
    let fail = || HoistError::Usage("could not generate an ed25519 key (system RNG)".into());
    let mut seed = Zeroizing::new([0u8; 32]);
    rng.fill(seed.as_mut()).map_err(|_| fail())?;
    let pair = Ed25519KeyPair::from_seed_unchecked(seed.as_ref()).map_err(|_| fail())?;
    let public: [u8; 32] = pair.public_key().as_ref().try_into().map_err(|_| fail())?;
    let mut check = [0u8; 4];
    rng.fill(&mut check).map_err(|_| fail())?;
    Ok(encode(&seed, &public, u32::from_ne_bytes(check)))
}

fn put_u32(buf: &mut Vec<u8>, n: u32) {
    buf.extend_from_slice(&n.to_be_bytes());
}

/// An SSH wire `string`: uint32 length, then the bytes.
fn put_str(buf: &mut Vec<u8>, s: &[u8]) {
    put_u32(buf, s.len() as u32);
    buf.extend_from_slice(s);
}

/// The public key blob: string "ssh-ed25519", string public key.
fn public_blob(public: &[u8; 32]) -> Vec<u8> {
    let mut blob = Vec::new();
    put_str(&mut blob, KEY_TYPE);
    put_str(&mut blob, public);
    blob
}

/// Encode a keypair (PROTOCOL.key of OpenSSH): magic `openssh-key-v1\0`, cipher and kdf
/// `none`, empty kdf options, one key, the public blob, then the private section (two equal
/// check ints, key type, public key, seed || public, empty comment, padding 1, 2, 3, ... to
/// the block size 8). Base64, 70 characters a line, between the BEGIN and END lines.
pub fn encode(seed: &[u8; 32], public: &[u8; 32], check: u32) -> TagKey {
    let blob = public_blob(public);
    let mut section = Zeroizing::new(Vec::with_capacity(160));
    put_u32(&mut section, check);
    put_u32(&mut section, check);
    put_str(&mut section, KEY_TYPE);
    put_str(&mut section, public);
    let mut both = Zeroizing::new([0u8; 64]);
    both[..32].copy_from_slice(seed);
    both[32..].copy_from_slice(public);
    put_str(&mut section, both.as_ref());
    put_str(&mut section, b"");
    let mut pad = 1u8;
    while section.len() % 8 != 0 {
        section.push(pad);
        pad += 1;
    }
    let mut raw = Zeroizing::new(Vec::with_capacity(320));
    raw.extend_from_slice(b"openssh-key-v1\0");
    put_str(&mut raw, b"none");
    put_str(&mut raw, b"none");
    put_str(&mut raw, b"");
    put_u32(&mut raw, 1);
    put_str(&mut raw, &blob);
    put_str(&mut raw, &section);
    let b64 = Zeroizing::new(STANDARD.encode(raw.as_slice()));
    let mut text = String::with_capacity(b64.len() + 80);
    text.push_str(BEGIN);
    text.push('\n');
    for line in b64.as_bytes().chunks(70) {
        // Base64 is ASCII: every chunk is valid UTF-8.
        text.push_str(std::str::from_utf8(line).unwrap_or_default());
        text.push('\n');
    }
    text.push_str(END);
    text.push('\n');
    TagKey {
        public: format!("ssh-ed25519 {}", STANDARD.encode(&blob)),
        private: Secret::from(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 8032 §7.1, test 1.
    const SEED: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
    const PUBLIC: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

    fn hex(s: &str) -> [u8; 32] {
        let v: Vec<u8> = (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect();
        v.try_into().unwrap()
    }

    #[test]
    fn ring_derives_the_rfc_8032_public_key() {
        let pair = Ed25519KeyPair::from_seed_unchecked(&hex(SEED)).unwrap();
        assert_eq!(pair.public_key().as_ref(), hex(PUBLIC));
    }

    #[test]
    fn the_public_line_is_the_openssh_format() {
        let k = encode(&hex(SEED), &hex(PUBLIC), 7);
        // ssh-keygen -y prints this line for the RFC 8032 test-1 key.
        assert_eq!(
            k.public,
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINdamAGCsQq31Uv+08lkBzoO4XLz2qYjJa8CGmj3B1Ea"
        );
    }

    #[test]
    fn the_private_key_layout_is_openssh_key_v1() {
        let k = encode(&hex(SEED), &hex(PUBLIC), 0x0102_0304);
        let text = k.private.expose();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], BEGIN);
        assert_eq!(*lines.last().unwrap(), END);
        assert!(lines[1..lines.len() - 1].iter().all(|l| l.len() <= 70));
        assert!(lines[1].len() == 70);
        let raw = STANDARD.decode(lines[1..lines.len() - 1].concat()).unwrap();
        let mut want = b"openssh-key-v1\0".to_vec();
        put_str(&mut want, b"none");
        put_str(&mut want, b"none");
        put_str(&mut want, b"");
        put_u32(&mut want, 1);
        put_str(&mut want, &public_blob(&hex(PUBLIC)));
        // The private section: 8 + 15 + 36 + 68 + 4 = 131 bytes, padded with 1..5 to 136.
        put_u32(&mut want, 136);
        want.extend([1, 2, 3, 4, 1, 2, 3, 4]);
        put_str(&mut want, b"ssh-ed25519");
        put_str(&mut want, &hex(PUBLIC));
        let mut both = hex(SEED).to_vec();
        both.extend(hex(PUBLIC));
        put_str(&mut want, &both);
        put_str(&mut want, b"");
        want.extend([1, 2, 3, 4, 5]);
        assert_eq!(raw, want);
    }

    #[test]
    fn debug_hides_the_private_key() {
        let k = generate().unwrap();
        let shown = format!("{k:?}");
        assert!(!shown.contains("OPENSSH"), "{shown}");
        assert!(
            k.public
                .starts_with("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI")
        );
    }

    /// When `ssh-keygen` is installed, it reads the key back and derives the same public line.
    #[test]
    fn ssh_keygen_reads_the_private_key_when_installed() {
        let Ok(dir) = tempfile::tempdir() else { return };
        let probe = std::process::Command::new("ssh-keygen")
            .arg("-?")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if probe.is_err() {
            eprintln!("ssh-keygen not installed: skipped");
            return;
        }
        let k = generate().unwrap();
        let path = dir.path().join("key");
        std::fs::write(&path, k.private.expose()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let out = std::process::Command::new("ssh-keygen")
            .arg("-y")
            .arg("-f")
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let line = String::from_utf8(out.stdout).unwrap();
        assert_eq!(
            line.split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" "),
            k.public
        );
    }

    /// When Python's `cryptography` is installed, it loads the key as an OpenSSH private key.
    #[test]
    fn python_cryptography_loads_the_private_key_when_installed() {
        let Ok(dir) = tempfile::tempdir() else { return };
        let k = generate().unwrap();
        let path = dir.path().join("key");
        std::fs::write(&path, k.private.expose()).unwrap();
        let script = "import sys\n\
            from cryptography.hazmat.primitives import serialization as s\n\
            key = s.load_ssh_private_key(open(sys.argv[1], 'rb').read(), password=None)\n\
            sys.stdout.write(key.public_key().public_bytes(s.Encoding.OpenSSH, s.PublicFormat.OpenSSH).decode())\n";
        let out = std::process::Command::new("python3")
            .arg("-I")
            .arg("-c")
            .arg(script)
            .arg(&path)
            .output();
        let Ok(out) = out else {
            eprintln!("python3 not installed: skipped");
            return;
        };
        if !out.status.success()
            && String::from_utf8_lossy(&out.stderr).contains("No module named 'cryptography'")
        {
            eprintln!("python cryptography not installed: skipped");
            return;
        }
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8(out.stdout).unwrap(), k.public);
    }
}
