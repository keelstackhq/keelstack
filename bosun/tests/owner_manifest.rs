//! `bosun owner manifest` (B2 T1): the GitHub App manifest flow on a loopback callback, and the
//! manual fallback. The browser is a raw TCP client; GitHub is the fake.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use bosun::cli::owner::app_manifest::{Flow, FlowConfig, KeySource, manual};
use bosun::github::LiveGithub;
use common::fake::FakeGithub;
use common::{TOKEN_CANARY, bosun, one_object};

const KEY: &str = "-----BEGIN RSA PRIVATE KEY-----\nKEYMATERIALCANARY0123456789abcdef\n-----END RSA PRIVATE KEY-----\n";
const CODE: &str = "c0de1234abcd5678ef90";

fn fake_with_code() -> FakeGithub {
    let fake = FakeGithub::start();
    {
        let mut s = fake.state.lock().unwrap();
        s.manifest_codes.push(CODE.to_string());
        s.manifest_pem = KEY.to_string();
    }
    fake
}

fn cfg(timeout: Duration) -> FlowConfig {
    FlowConfig {
        app_name: "acme-bosun-gate".into(),
        org: None,
        timeout,
    }
}

fn gh(fake: &FakeGithub) -> LiveGithub {
    LiveGithub::with(&fake.url(), "test-token", "acme", "widget", 0)
}

/// One raw request; (status, body). `None` when the connection is refused or cut.
fn raw(port: u16, request: &str) -> Option<(u16, String)> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    s.write_all(request.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    let status = out.split(' ').nth(1)?.parse().ok()?;
    let body = out.split_once("\r\n\r\n").map_or("", |x| x.1).to_string();
    Some((status, body))
}

fn get(port: u16, target: &str) -> Option<(u16, String)> {
    raw(
        port,
        &format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"),
    )
}

/// The state the start page hands to GitHub.
fn state_of(port: u16) -> String {
    let (status, body) = get(port, "/start").expect("start page");
    assert_eq!(status, 200);
    let after = body.split("?state=").nth(1).expect("state in page");
    after.split('"').next().unwrap().to_string()
}

fn attempts(fake: &FakeGithub) -> usize {
    fake.state.lock().unwrap().manifest_attempts.len()
}

type FlowHandle = std::thread::JoinHandle<bosun::github::Result<bosun::github::AppCredentials>>;

fn spawn_flow(fake: &FakeGithub, timeout: Duration) -> (u16, FlowHandle) {
    let flow = Flow::bind(cfg(timeout)).unwrap();
    let port = flow.local_addr().port();
    let g = gh(fake);
    (port, std::thread::spawn(move || flow.run(&g)))
}

#[test]
fn binds_loopback_only() {
    let flow = Flow::bind(cfg(Duration::from_secs(1))).unwrap();
    let addr = flow.local_addr();
    assert!(addr.ip().is_loopback() && addr.ip().to_string() == "127.0.0.1");
    assert_ne!(addr.port(), 0);
    assert!(flow.start_url().starts_with("http://127.0.0.1:"));
}

#[test]
fn happy_path_returns_ids_and_key_and_debug_redacts() {
    let fake = fake_with_code();
    let (port, h) = spawn_flow(&fake, Duration::from_secs(20));
    let state = state_of(port);
    assert!(state.len() >= 32, "128 bits are 32 hex digits: {state}");
    let (status, body) = get(port, &format!("/callback?code={CODE}&state={state}")).unwrap();
    assert_eq!(status, 200);
    assert!(!body.contains(CODE) && !body.contains(KEY.trim()));
    let creds = h.join().unwrap().unwrap();
    assert_eq!(creds.id, 777);
    assert_eq!(creds.client_id, "Iv1.fakeclient");
    assert_eq!(creds.slug, "acme-bosun-gate");
    assert_eq!(creds.key.expose(), KEY);
    assert_eq!(attempts(&fake), 1);
    let shown = format!("{creds:?} {creds:#?} {:?}", creds.key);
    for secret in [
        "KEYMATERIALCANARY",
        "FAKE-CLIENT-SECRET",
        "FAKE-WEBHOOK-SECRET",
    ] {
        assert!(!shown.contains(secret), "{secret} in Debug: {shown}");
    }
    assert!(shown.contains("redacted"));
}

#[test]
fn state_mismatch_is_rejected_and_the_flow_goes_on() {
    let fake = fake_with_code();
    let (port, h) = spawn_flow(&fake, Duration::from_secs(20));
    let good = state_of(port);
    let wrong = "0".repeat(good.len());
    for target in [
        format!("/callback?code={CODE}&state={wrong}"),
        format!("/callback?code={CODE}&state=short"),
        format!("/callback?code={CODE}"),
        format!("/callback?state={good}"),
        format!("/callback?code={CODE}&state={good}&state={good}"),
    ] {
        let (status, _) = get(port, &target).unwrap();
        assert!((400..500).contains(&status), "{target}: {status}");
    }
    assert_eq!(attempts(&fake), 0, "no code was exchanged");
    let (status, _) = get(port, &format!("/callback?code={CODE}&state={good}")).unwrap();
    assert_eq!(status, 200);
    assert!(h.join().unwrap().is_ok());
}

#[test]
fn replay_is_rejected_and_the_code_is_exchanged_once() {
    let fake = fake_with_code();
    let (port, h) = spawn_flow(&fake, Duration::from_secs(20));
    let state = state_of(port);
    let target = format!("/callback?code={CODE}&state={state}");
    assert_eq!(get(port, &target).unwrap().0, 200);
    let again = get(port, &target);
    assert!(
        again.is_none_or(|(s, _)| s >= 400),
        "a replay must not succeed"
    );
    h.join().unwrap().unwrap();
    assert_eq!(attempts(&fake), 1);
}

#[test]
fn a_failed_exchange_is_an_error_without_the_code() {
    let fake = FakeGithub::start(); // no valid code
    let (port, h) = spawn_flow(&fake, Duration::from_secs(20));
    let state = state_of(port);
    get(port, &format!("/callback?code={CODE}&state={state}"));
    let e = h.join().unwrap().unwrap_err();
    let shown = format!("{e} {e:?}");
    assert!(!shown.contains(CODE), "the code leaked: {shown}");
    assert!(!e.fix.trim().is_empty());
    assert_eq!(attempts(&fake), 1);
}

#[test]
fn times_out_without_a_callback() {
    let fake = fake_with_code();
    let t0 = std::time::Instant::now();
    let (_, h) = spawn_flow(&fake, Duration::from_millis(300));
    let e = h.join().unwrap().unwrap_err();
    assert!(e.message.contains("timed out"), "{e}");
    assert!(!e.fix.trim().is_empty());
    assert!(t0.elapsed() < Duration::from_secs(10));
    assert_eq!(attempts(&fake), 0);
}

#[test]
fn wrong_host_method_and_path_are_refused() {
    let fake = fake_with_code();
    let (port, h) = spawn_flow(&fake, Duration::from_secs(20));
    let state = state_of(port);
    let cb = format!("/callback?code={CODE}&state={state}");
    let req = |method: &str, target: &str, host: &str| {
        raw(
            port,
            &format!("{method} {target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"),
        )
        .map(|r| r.0)
    };
    // A rebinding page or another name for the same loopback: refused, state untouched.
    for host in [
        "evil.example".to_string(),
        format!("evil.example:{port}"),
        "127.0.0.1".to_string(),
        format!("localhost:{port}"),
        format!("127.0.0.1:{}", port.wrapping_add(1)),
        format!("[::1]:{port}"),
    ] {
        assert_eq!(req("GET", &cb, &host), Some(421), "{host}");
    }
    // No Host header, and two of them.
    assert_eq!(
        raw(port, &format!("GET {cb} HTTP/1.1\r\n\r\n")).map(|r| r.0),
        Some(421)
    );
    assert_eq!(
        raw(
            port,
            &format!("GET {cb} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nHost: evil.example\r\n\r\n")
        )
        .map(|r| r.0),
        Some(421)
    );
    let ok_host = format!("127.0.0.1:{port}");
    for method in ["POST", "PUT", "DELETE", "HEAD"] {
        assert_eq!(req(method, &cb, &ok_host), Some(405), "{method}");
    }
    for target in [
        "/",
        "/callback/",
        "/x",
        "/callbackx",
        "/start?x=1",
        "//evil.example/callback",
    ] {
        let s = req("GET", target, &ok_host);
        assert!(matches!(s, Some(400 | 404)), "{target}: {s:?}");
    }
    // An absolute-form target (a proxy request) is not served either.
    let s = req("GET", &format!("http://evil.example{cb}"), &ok_host);
    assert_eq!(s, Some(400));
    assert_eq!(attempts(&fake), 0, "nothing reached the exchange");
    // The right request still works afterwards: the refusals did not use the state.
    assert_eq!(req("GET", &cb, &ok_host), Some(200));
    h.join().unwrap().unwrap();
}

#[test]
fn start_page_posts_the_manifest_to_github_only() {
    let flow = Flow::bind(FlowConfig {
        org: Some("keelstackhq".into()),
        ..cfg(Duration::from_millis(600))
    })
    .unwrap();
    let port = flow.local_addr().port();
    let fake = FakeGithub::start();
    let g = gh(&fake);
    let h = std::thread::spawn(move || flow.run(&g));
    let (_, page) = get(port, "/start").unwrap();
    assert!(page.contains("https://github.com/organizations/keelstackhq/settings/apps/new?state="));
    assert!(page.contains(&format!("http://127.0.0.1:{port}/callback")));
    assert!(!page.contains("hook_attributes"));
    let _ = h.join();
    for bad in ["a/b", "x y", "", "o?x=1"] {
        let r = Flow::bind(FlowConfig {
            org: Some(bad.into()),
            ..cfg(Duration::from_secs(1))
        });
        assert!(r.is_err(), "{bad:?}");
    }
}

#[test]
fn manual_reads_a_key_file_and_checks_its_shape() {
    let dir = tempfile::tempdir().unwrap();
    let good = dir.path().join("k.pem");
    std::fs::write(&good, KEY).unwrap();
    let c = manual(5, "Iv1.abc", "acme-gate", &KeySource::File(good)).unwrap();
    assert_eq!((c.id, c.slug.as_str()), (5, "acme-gate"));
    assert_eq!(c.key.expose(), KEY);
    assert!(!format!("{c:?}").contains("KEYMATERIALCANARY"));

    let bad = dir.path().join("bad.pem");
    std::fs::write(&bad, "SECRETTEXT not a key").unwrap();
    let e = manual(5, "Iv1.abc", "acme-gate", &KeySource::File(bad)).unwrap_err();
    assert!(!format!("{e:?}").contains("SECRETTEXT"));
    let missing = KeySource::File(dir.path().join("none.pem"));
    assert!(manual(5, "Iv1.abc", "acme-gate", &missing).is_err());
    let f = || KeySource::File(dir.path().join("k.pem"));
    assert!(manual(0, "Iv1.abc", "acme-gate", &f()).is_err());
    assert!(manual(5, "bad id", "acme-gate", &f()).is_err());
    assert!(manual(5, "Iv1.abc", "Bad Slug/", &f()).is_err());
}

fn no_key_material(stdout: &[u8], stderr: &[u8]) {
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    );
    for secret in [
        "KEYMATERIALCANARY",
        "BEGIN RSA PRIVATE KEY",
        "FAKE-CLIENT-SECRET",
        "FAKE-WEBHOOK-SECRET",
        CODE,
        TOKEN_CANARY,
    ] {
        assert!(!all.contains(secret), "{secret} leaked:\n{all}");
    }
}

#[test]
fn manual_cli_reads_the_key_from_stdin_and_prints_no_key() {
    let out = bosun()
        .args([
            "owner",
            "manifest",
            "--manual",
            "--app-id",
            "5",
            "--client-id",
            "Iv1.abc",
            "--slug",
            "acme-gate",
            "--key-stdin",
        ])
        .write_stdin(KEY)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let v = one_object(&out.stdout);
    assert_eq!(v["command"], "owner manifest");
    assert_eq!(v["data"]["app_id"], 5);
    assert_eq!(v["data"]["slug"], "acme-gate");
    no_key_material(&out.stdout, &out.stderr);
}

#[test]
fn manual_cli_refuses_bad_input_with_an_envelope() {
    let out = bosun()
        .args(["owner", "manifest", "--manual", "--app-id", "5"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    one_object(&out.stdout);
    let out = bosun()
        .args([
            "owner",
            "manifest",
            "--manual",
            "--app-id",
            "5",
            "--client-id",
            "Iv1.abc",
            "--slug",
            "s",
            "--key-stdin",
        ])
        .write_stdin("SECRETTEXT no pem here\n")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    one_object(&out.stdout);
    assert!(!String::from_utf8_lossy(&out.stdout).contains("SECRETTEXT"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("SECRETTEXT"));
}

#[test]
fn manifest_cli_end_to_end_prints_no_key_material() {
    let fake = fake_with_code();
    let dir = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_bosun"))
        .current_dir(dir.path())
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", dir.path())
        .env("GITHUB_TOKEN", TOKEN_CANARY)
        .env("BOSUN_GITHUB_API", fake.url())
        .env("BOSUN_REPO", "acme/widget")
        .env("BOSUN_SESSION", "session_test")
        .env("BOSUN_PIN_CHECK", "0")
        .env("BOSUN_NO_GH", "1")
        .env("BOSUN_VERBOSE", "1")
        .args(["owner", "manifest", "--timeout-secs", "30"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut err = BufReader::new(child.stderr.take().unwrap());
    let mut seen = String::new();
    let port = loop {
        let mut line = String::new();
        assert!(err.read_line(&mut line).unwrap() > 0, "no URL: {seen}");
        seen.push_str(&line);
        if let Some(rest) = line.split("http://127.0.0.1:").nth(1) {
            break rest.split('/').next().unwrap().parse::<u16>().unwrap();
        }
    };
    let state = state_of(port);
    assert_eq!(
        get(port, &format!("/callback?code={CODE}&state={state}"))
            .unwrap()
            .0,
        200
    );
    let mut rest = String::new();
    err.read_to_string(&mut rest).unwrap();
    let mut stdout = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    assert!(child.wait().unwrap().success());
    let v = one_object(&stdout);
    assert_eq!(v["data"]["app_id"], 777);
    assert_eq!(v["data"]["source"], "manifest");
    seen.push_str(&rest);
    no_key_material(&stdout, seen.as_bytes());
    assert!(!seen.contains(&state), "the state leaked");
}

fn manifest_cli(args: &[&str]) -> std::process::Output {
    let mut a = vec!["owner", "manifest"];
    a.extend_from_slice(args);
    bosun().args(a).output().unwrap()
}

#[test]
fn timeout_secs_is_range_checked_and_never_panics() {
    for v in ["0", "3601", "18446744073709551615", "-1", "x"] {
        let out = manifest_cli(&["--timeout-secs", v]);
        assert_eq!(out.status.code(), Some(2), "{v}: {out:?}");
        assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
    }
    let e = Flow::bind(cfg(Duration::from_secs(u64::MAX)))
        .unwrap()
        .run(&gh(&FakeGithub::start()))
        .unwrap_err();
    assert!(!e.fix.trim().is_empty());
}

#[test]
fn a_slow_client_cannot_extend_the_flow_deadline() {
    let fake = fake_with_code();
    let t0 = std::time::Instant::now();
    let (port, h) = spawn_flow(&fake, Duration::from_millis(1500));
    let drip = std::thread::spawn(move || {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        for _ in 0..40 {
            if s.write_all(b"G").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(400));
        }
    });
    let e = h.join().unwrap().unwrap_err();
    assert!(e.message.contains("timed out"), "{e}");
    assert!(t0.elapsed() < Duration::from_secs(8), "{:?}", t0.elapsed());
    drop(drip);
}

#[test]
fn start_page_is_served_once_then_gone() {
    let fake = fake_with_code();
    let (port, h) = spawn_flow(&fake, Duration::from_secs(20));
    let state = state_of(port);
    let (status, body) = get(port, "/start").unwrap();
    assert!(matches!(status, 404 | 410), "{status}");
    assert!(!body.contains(&state));
    assert_eq!(
        get(port, &format!("/callback?code={CODE}&state={state}"))
            .unwrap()
            .0,
        200
    );
    h.join().unwrap().unwrap();
}

#[cfg(unix)]
#[test]
fn key_file_must_be_a_small_regular_file_and_is_kept() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("k.pem");
    std::fs::write(&real, KEY).unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = dir.path().join("link.pem");
    symlink(&real, &link).unwrap();
    assert!(manual(5, "Iv1.abc", "acme-gate", &KeySource::File(link)).is_err());
    assert!(
        manual(
            5,
            "Iv1.abc",
            "acme-gate",
            &KeySource::File(dir.path().into())
        )
        .is_err()
    );
    let big = dir.path().join("big.pem");
    std::fs::write(&big, format!("{KEY}{}", "A".repeat(20_000))).unwrap();
    assert!(manual(5, "Iv1.abc", "acme-gate", &KeySource::File(big)).is_err());
    assert!(manual(5, "Iv1.abc", "acme-gate", &KeySource::File(real.clone())).is_ok());
    assert!(real.exists(), "the key file is not deleted here");

    // A group/other readable key warns on stderr, never with key material.
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o644)).unwrap();
    let out = manifest_cli(&[
        "--manual",
        "--app-id",
        "5",
        "--client-id",
        "Iv1.abc",
        "--slug",
        "acme-gate",
        "--key-file",
        real.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("readable"), "{err}");
    no_key_material(&out.stdout, &out.stderr);
    assert!(real.exists());
}
