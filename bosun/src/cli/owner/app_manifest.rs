//! The GitHub App manifest flow with a loopback callback (B2 T1), and its manual fallback.
//!
//! Security rules (plan B2, binding): the listener binds 127.0.0.1 on a random port only; a
//! one-time state of 256 bits from the system CSPRNG is checked in constant time and used once;
//! only `Host: 127.0.0.1:<port>`, `GET`, `/start` and `/callback` are served; the whole flow
//! gives up after the timeout; the manifest `code` is exchanged once. The private key is a
//! [`Secret`] (redacting `Debug`, wiped on drop). Nothing here prints the key, the code or the
//! state; the client and webhook secrets are never kept.

use std::io::{BufRead, IsTerminal, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ring::rand::{SecureRandom, SystemRandom};
use serde_json::json;
use zeroize::Zeroize;

use crate::github::{AppCredentials, Github, Secret};
use crate::output::BosunError;

type Result<T> = std::result::Result<T, BosunError>;

/// How long the owner has to confirm the App on GitHub.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_HEAD: usize = 8192;
const MAX_PEM: usize = 16 * 1024;
/// One request gets this long in total (never per read), and never past the flow deadline.
const REQUEST_BUDGET: Duration = Duration::from_secs(5);
const GITHUB: &str = "https://github.com";

pub struct FlowConfig {
    /// The App's name (its slug is derived from it; slugs are global).
    pub app_name: String,
    /// Create the App in this organization instead of the owner's user account.
    pub org: Option<String>,
    pub timeout: Duration,
}

/// The state parameter: random, shown only on the loopback page, valid for one callback.
struct OneTimeState {
    value: String,
    used: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum StateCheck {
    Accepted,
    Mismatch,
    Replay,
}

impl OneTimeState {
    fn new() -> Result<Self> {
        let mut raw = [0u8; 32];
        SystemRandom::new().fill(&mut raw).map_err(|_| {
            BosunError::error(
                "the system random generator failed",
                "retry; if it persists the machine cannot make secure keys: use another computer",
            )
        })?;
        let value = raw.iter().map(|b| format!("{b:02x}")).collect();
        raw.zeroize();
        Ok(OneTimeState { value, used: false })
    }

    /// Constant-time compare; a right state works once.
    fn consume(&mut self, candidate: &str) -> StateCheck {
        let same = ct_eq(self.value.as_bytes(), candidate.as_bytes());
        match (same, self.used) {
            (false, _) => StateCheck::Mismatch,
            (true, true) => StateCheck::Replay,
            (true, false) => {
                self.used = true;
                StateCheck::Accepted
            }
        }
    }
}

/// Equal without an early exit on the first differing byte (the length is not secret: it is
/// fixed and public).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && std::hint::black_box(a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y))) == 0
}

pub struct Flow {
    listener: TcpListener,
    port: u16,
    state: OneTimeState,
    cfg: FlowConfig,
    start_served: bool,
}

enum Served {
    Continue,
    Code(String),
}

impl Flow {
    /// Bind 127.0.0.1 on a random port. There is no way to bind another address.
    pub fn bind(cfg: FlowConfig) -> Result<Flow> {
        if let Some(org) = &cfg.org
            && (org.is_empty()
                || org.len() > 39
                || !org.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        {
            return Err(BosunError::usage(
                "the organization name must be 1-39 characters of A-Z a-z 0-9 -",
                "pass the organization's login, e.g. `--org keelstackhq`",
            ));
        }
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .and_then(|l| l.set_nonblocking(true).map(|()| l))
            .map_err(|e| {
                BosunError::error(
                    format!("cannot listen on 127.0.0.1: {}", e.kind()),
                    "use `bosun owner manifest --manual` instead",
                )
            })?;
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        Ok(Flow {
            listener,
            port,
            state: OneTimeState::new()?,
            cfg,
            start_served: false,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, self.port))
    }

    /// The page the owner opens; it carries no secret.
    pub fn start_url(&self) -> String {
        format!("http://127.0.0.1:{}/start", self.port)
    }

    /// Serve until a valid callback arrives (exchanged once through `gh`) or the timeout ends.
    pub fn run(mut self, gh: &dyn Github) -> Result<AppCredentials> {
        let Some(deadline) = Instant::now().checked_add(self.cfg.timeout) else {
            return Err(BosunError::usage(
                "the timeout is too large",
                "pass `--timeout-secs` between 1 and 3600",
            ));
        };
        loop {
            if Instant::now() >= deadline {
                return Err(BosunError::error(
                    format!(
                        "timed out after {} s waiting for the GitHub App to be confirmed",
                        self.cfg.timeout.as_secs()
                    ),
                    "rerun the command and confirm the App in the browser, or use `--manual`",
                ));
            }
            match self.listener.accept() {
                Ok((stream, peer)) => {
                    if !peer.ip().is_loopback() {
                        continue;
                    }
                    if let Served::Code(mut code) = self.serve(stream, deadline)? {
                        let result = gh.convert_manifest(&code);
                        code.zeroize();
                        return result;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(15));
                }
                Err(e) => {
                    return Err(BosunError::error(
                        format!("the loopback listener failed: {}", e.kind()),
                        "rerun the command, or use `--manual`",
                    ));
                }
            }
        }
    }

    fn serve(&mut self, mut stream: TcpStream, flow_deadline: Instant) -> Result<Served> {
        let _ = stream.set_nonblocking(false);
        let request_deadline = flow_deadline.min(Instant::now() + REQUEST_BUDGET);
        let _ = stream.set_write_timeout(Some(
            request_deadline
                .saturating_duration_since(Instant::now())
                .max(Duration::from_millis(1)),
        ));
        let Some(req) = read_request(&mut stream, request_deadline) else {
            respond(&mut stream, 400, "text/plain", "bad request");
            return Ok(Served::Continue);
        };
        if req.hosts.len() != 1 || req.hosts[0] != format!("127.0.0.1:{}", self.port) {
            respond(&mut stream, 421, "text/plain", "wrong host");
            return Ok(Served::Continue);
        }
        if req.method != "GET" {
            respond(&mut stream, 405, "text/plain", "method not allowed");
            return Ok(Served::Continue);
        }
        let (path, query) = req.target.split_once('?').unwrap_or((&req.target, ""));
        match path {
            "/start" if query.is_empty() && self.start_served => {
                respond(&mut stream, 410, "text/plain", "already used");
                Ok(Served::Continue)
            }
            "/start" if query.is_empty() => {
                self.start_served = true;
                let page = self.start_page();
                respond(&mut stream, 200, "text/html", &page);
                Ok(Served::Continue)
            }
            "/callback" => Ok(self.callback(&mut stream, query)),
            _ => {
                respond(&mut stream, 404, "text/plain", "not found");
                Ok(Served::Continue)
            }
        }
    }

    fn callback(&mut self, stream: &mut TcpStream, query: &str) -> Served {
        let Some(params) = parse_query(query) else {
            respond(stream, 400, "text/plain", "bad request");
            return Served::Continue;
        };
        let get = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        let (Some(state), Some(code)) = (get("state"), get("code")) else {
            respond(stream, 400, "text/plain", "bad request");
            return Served::Continue;
        };
        let code_ok = !code.is_empty()
            && code.len() <= 128
            && code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if !code_ok {
            respond(stream, 400, "text/plain", "bad request");
            return Served::Continue;
        }
        match self.state.consume(state) {
            StateCheck::Accepted => {
                respond(
                    stream,
                    200,
                    "text/html",
                    "<!doctype html><title>bosun</title><p>The App is created. You can close this tab and return to the terminal.",
                );
                Served::Code(code.to_string())
            }
            StateCheck::Mismatch => {
                respond(stream, 403, "text/plain", "forbidden");
                Served::Continue
            }
            StateCheck::Replay => {
                respond(stream, 410, "text/plain", "already used");
                Served::Continue
            }
        }
    }

    /// An auto-submitting form that posts the manifest to GitHub with our state.
    fn start_page(&self) -> String {
        let mut perms = json!({
            "checks": "write", "pull_requests": "write", "contents": "read", "issues": "read",
        });
        let action = match &self.cfg.org {
            Some(org) => {
                perms["members"] = json!("read");
                format!("{GITHUB}/organizations/{org}/settings/apps/new")
            }
            None => format!("{GITHUB}/settings/apps/new"),
        };
        let manifest = json!({
            "name": self.cfg.app_name,
            "url": "https://github.com/keelstackhq/keelstack",
            "redirect_url": format!("http://127.0.0.1:{}/callback", self.port),
            "public": false,
            "default_permissions": perms,
        });
        format!(
            "<!doctype html><title>bosun</title><form method=\"post\" action=\"{action}?state={}\">\
             <input type=\"hidden\" name=\"manifest\" value=\"{}\">\
             <noscript><button>Create the GitHub App</button></noscript></form>\
             <script>document.forms[0].submit()</script>",
            self.state.value,
            html_escape(&manifest.to_string()),
        )
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

struct Request {
    method: String,
    target: String,
    hosts: Vec<String>,
}

fn read_request(stream: &mut TcpStream, deadline: Instant) -> Option<Request> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        if buf.len() > MAX_HEAD {
            return None;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
            return None;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8(buf).ok()?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target, version) = (first.next()?, first.next()?, first.next()?);
    if first.next().is_some() || !version.starts_with("HTTP/1.") || !target.starts_with('/') {
        return None;
    }
    if target.starts_with("//") {
        return None;
    }
    let hosts = lines
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.split_once(':'))
        .filter(|(n, _)| n.eq_ignore_ascii_case("host"))
        .map(|(_, v)| v.trim().to_string())
        .collect();
    Some(Request {
        method: method.to_string(),
        target: target.to_string(),
        hosts,
    })
}

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        410 => "Gone",
        _ => "Misdirected Request",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}; charset=utf-8\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; form-action https://github.com\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

/// `a=b&c=d` with percent-decoding; `None` on a bad escape or a repeated name.
fn parse_query(q: &str) -> Option<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = Vec::new();
    for pair in q.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let (k, v) = (decode(k)?, decode(v)?);
        if out.iter().any(|(n, _)| *n == k) {
            return None;
        }
        out.push((k, v));
    }
    Some(out)
}

fn decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = s.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Where the manual fallback reads the private key from (never argv).
#[derive(Debug, Clone)]
pub enum KeySource {
    File(PathBuf),
    /// Standard input; on a terminal the echo is turned off while the key is typed or pasted.
    Stdin,
}

/// The fallback when the browser flow cannot run: the owner created the App by hand and gives
/// its id, client id and slug (public values) and the key from a file or stdin.
pub fn manual(id: u64, client_id: &str, slug: &str, key: &KeySource) -> Result<AppCredentials> {
    let plain = |s: &str, extra: &[u8]| {
        !s.is_empty()
            && s.len() <= 100
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || extra.contains(&b))
    };
    if id == 0 || !plain(client_id, b"._-") || !plain(slug, b"-") {
        return Err(BosunError::usage(
            "the App id, client id or slug has an unexpected form",
            "copy them from the App's settings page: id (number), client id (Iv1...), slug (the name in its URL)",
        ));
    }
    let pem = match key {
        KeySource::File(path) => read_key_file(path)?,
        KeySource::Stdin => read_key_stdin()?,
    };
    if !looks_like_pem(pem.expose()) {
        return Err(BosunError::usage(
            "the key is not a PEM private key",
            "pass the .pem file GitHub downloaded (it starts with -----BEGIN RSA PRIVATE KEY-----)",
        ));
    }
    Ok(AppCredentials {
        id,
        client_id: client_id.to_string(),
        slug: slug.to_string(),
        key: pem,
    })
}

/// Open the key file without following a symlink, require a regular file of at most
/// `MAX_PEM` bytes (checked before reading, and the read is capped), warn when it is group or
/// other readable. The file is never deleted here.
fn read_key_file(path: &std::path::Path) -> Result<Secret> {
    let bad = |why: &str| {
        BosunError::error(
            format!("cannot use the key file {}: {why}", path.display()),
            "pass the path of the .pem file GitHub downloaded (a regular file, not a link, under 16 KiB)",
        )
    };
    let link = std::fs::symlink_metadata(path).map_err(|e| bad(&format!("{:?}", e.kind())))?;
    if !link.file_type().is_file() {
        return Err(bad("not a regular file or it is a symlink"));
    }
    let file = std::fs::File::open(path).map_err(|e| bad(&format!("{:?}", e.kind())))?;
    let meta = file
        .metadata()
        .map_err(|e| bad(&format!("{:?}", e.kind())))?;
    {
        use std::os::unix::fs::MetadataExt;
        if !meta.is_file() || meta.dev() != link.dev() || meta.ino() != link.ino() {
            return Err(bad("the file changed while opening"));
        }
        if meta.mode() & 0o077 != 0 {
            eprintln!(
                "bosun: warning: {} is readable by group or others; run `chmod 600` on it",
                path.display()
            );
        }
    }
    if meta.len() > MAX_PEM as u64 {
        return Err(bad("it is larger than 16 KiB"));
    }
    let mut text = Secret::new(String::new());
    file.take(MAX_PEM as u64 + 1)
        .read_to_string(&mut text.0)
        .map_err(|e| bad(&format!("{:?}", e.kind())))?;
    if text.0.len() > MAX_PEM {
        return Err(bad("it is larger than 16 KiB"));
    }
    Ok(text)
}

fn looks_like_pem(s: &str) -> bool {
    let t = s.trim();
    t.len() <= MAX_PEM
        && t.starts_with("-----BEGIN ")
        && t.contains("PRIVATE KEY-----")
        && t.lines()
            .last()
            .is_some_and(|l| l.starts_with("-----END ") && l.ends_with("PRIVATE KEY-----"))
}

/// Turns terminal echo off while alive; `stty` restores it on every exit path.
struct NoEcho(bool);

/// `stty` by absolute path (no PATH lookup); `/bin` first, then `/usr/bin`.
fn stty(arg: &str) -> bool {
    ["/bin/stty", "/usr/bin/stty"].iter().any(|p| {
        std::process::Command::new(p)
            .arg(arg)
            .status()
            .is_ok_and(|s| s.success())
    })
}

impl NoEcho {
    fn on() -> Result<NoEcho> {
        if !std::io::stdin().is_terminal() {
            return Ok(NoEcho(false));
        }
        if stty("-echo") {
            Ok(NoEcho(true))
        } else {
            Err(BosunError::error(
                "cannot turn off the terminal echo, so the key would be shown",
                "use `--key-file <path>` or pipe the key to stdin",
            ))
        }
    }
}

impl Drop for NoEcho {
    fn drop(&mut self) {
        if self.0 {
            let _ = stty("echo");
        }
    }
}

/// Read lines up to and including the `-----END ...-----` line.
fn read_key_stdin() -> Result<Secret> {
    let _echo = NoEcho::on()?;
    let mut line = Secret::new(String::new());
    let mut text = Secret::new(String::new());
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    loop {
        line.0.clear();
        let n = input.read_line(&mut line.0).map_err(|e| {
            BosunError::error(
                format!("cannot read the key from stdin: {}", e.kind()),
                "use `--key-file <path>`",
            )
        })?;
        text.0.push_str(&line.0);
        if n == 0 || line.0.starts_with("-----END ") || text.0.len() > MAX_PEM {
            return Ok(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_is_256_bits_unique_and_single_use() {
        let mut a = OneTimeState::new().unwrap();
        let b = OneTimeState::new().unwrap();
        assert_eq!(a.value.len(), 64);
        assert_ne!(a.value, b.value);
        let v = a.value.clone();
        assert_eq!(a.consume("nope"), StateCheck::Mismatch);
        assert_eq!(a.consume(&v), StateCheck::Accepted);
        assert_eq!(a.consume(&v), StateCheck::Replay);
    }

    #[test]
    fn ct_eq_compares_whole_values() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }
}
