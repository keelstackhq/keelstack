//! `curl` for the post-deploy health probe (keel has no HTTP crate).
use crate::exec::{Cmd, Runner};

/// What one probe saw: the HTTP status (`None` when no response), and why it is not a 200.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub status: Option<u16>,
    pub error: String,
}

impl Probe {
    pub fn ok(&self) -> bool {
        self.status == Some(200)
    }
}

/// `curl -fsS -o /dev/null -w %{http_code} --max-time 10 URL`.
pub fn probe(r: &dyn Runner, url: &str) -> Probe {
    let c = Cmd::new(
        "curl",
        &[
            "-fsS",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "--max-time",
            "10",
            url,
        ],
    );
    let o = r.run(&c);
    // 000 means no response was received.
    let status = o.stdout.trim().parse::<u16>().ok().filter(|s| *s != 0);
    let error = if o.code == 0 && status == Some(200) {
        String::new()
    } else if let Some(s) = status {
        format!("HTTP {s}")
    } else if super::not_found(&o) {
        "curl is not installed".to_string()
    } else {
        let why = o
            .stderr
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        if why.is_empty() {
            format!("curl exited {}", o.code)
        } else {
            why.trim().to_string()
        }
    };
    Probe { status, error }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::{FakeRunner, Out};

    #[test]
    fn reads_the_status_or_the_error() {
        let r = FakeRunner::new().on(&["curl"], Out::ok("200"));
        let p = probe(&r, "https://h/healthz");
        assert!(p.ok());
        assert!(
            r.calls()[0]
                .line()
                .ends_with("--max-time 10 https://h/healthz")
        );
        let r = FakeRunner::new().on(
            &["curl"],
            Out::with(
                22,
                "503",
                "curl: (22) The requested URL returned error: 503",
            ),
        );
        let p = probe(&r, "u");
        assert_eq!(
            (p.ok(), p.status, p.error.as_str()),
            (false, Some(503), "HTTP 503")
        );
        let r = FakeRunner::new().on(
            &["curl"],
            Out::with(6, "000", "curl: (6) Could not resolve host: h"),
        );
        let p = probe(&r, "u");
        assert_eq!(p.status, None);
        assert!(p.error.contains("Could not resolve host"));
        assert!(
            probe(&FakeRunner::new(), "u")
                .error
                .contains("not installed")
        );
    }
}
