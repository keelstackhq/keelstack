use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Ok,
    Done,
    WouldRun,
    Pending,
    Failed,
    NotReached,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Ok => "ok",
            Action::Done => "done",
            Action::WouldRun => "would-run",
            Action::Pending => "pending",
            Action::Failed => "failed",
            Action::NotReached => "not-reached",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Done,
    Human,
    Failed,
    Planned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepResult {
    pub id: String,
    pub action: Action,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HumanStep {
    pub id: String,
    pub title: String,
    pub url: String,
    pub command: String,
}

/// A hoist item, with the keel step that read it (`items[]` of the JSON report).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReportItem {
    pub step: String,
    pub area: String,
    pub kind: String,
    pub key: String,
    pub action: String,
    pub reason: String,
}

/// One pending human step of the batched checklist (D-009).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChecklistEntry {
    pub id: String,
    pub title: String,
    pub url: String,
    pub command: String,
    /// `computer` or `phone`.
    pub device: String,
    pub verify: String,
}

/// Ids that need a phone: trusted publishers on npm and crates.io, and app installs.
fn device_of(id: &str, url: &str) -> &'static str {
    let phone = id.starts_with("npm-trust:")
        || id.starts_with("crates-trust:")
        || id.starts_with("app:")
        || id.starts_with("app-install")
        || id == "approve-pr"
        || url.contains("/installations/new");
    if phone { "phone" } else { "computer" }
}

fn verify_of(id: &str) -> String {
    if id.starts_with("npm-trust:") || id.starts_with("crates-trust:") {
        format!("hoist cannot read it: run keel up --confirm {id} once done")
    } else {
        "keel plan re-checks it".into()
    }
}

/// The checklist: every pending human step, deduped by id, minus `confirmed` ids; computer
/// entries first, otherwise in the given order.
pub fn build_checklist(steps: &[HumanStep], confirmed: &[String]) -> Vec<ChecklistEntry> {
    let mut seen: Vec<&str> = Vec::new();
    let mut all: Vec<ChecklistEntry> = Vec::new();
    for h in steps {
        if seen.contains(&h.id.as_str()) || confirmed.iter().any(|c| c == &h.id) {
            continue;
        }
        seen.push(&h.id);
        // hoist puts a command (`hoist secrets set NAME`) where a link would go: keep links only.
        let link = h.url.starts_with("https://") || h.url.starts_with("http://");
        let (url, command) = match (link, h.command.is_empty()) {
            (true, _) => (h.url.clone(), h.command.clone()),
            (false, true) => (String::new(), h.url.clone()),
            (false, false) => (String::new(), h.command.clone()),
        };
        all.push(ChecklistEntry {
            id: h.id.clone(),
            title: h.title.clone(),
            url,
            command,
            device: device_of(&h.id, &h.url).into(),
            verify: verify_of(&h.id),
        });
    }
    let (mut computer, phone): (Vec<_>, Vec<_>) =
        all.into_iter().partition(|e| e.device == "computer");
    computer.extend(phone);
    computer
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Healthz {
    Ok,
    Failed,
    NotRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Smoke {
    Passed,
    Failed,
    None,
    NotRun,
}

/// What the verify step saw at the deployed site (spec §3.2 `verify`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verify {
    pub url: String,
    pub healthz: Healthz,
    pub http_status: Option<u16>,
    pub smoke: Smoke,
}

impl Verify {
    pub fn not_run(host: &str) -> Verify {
        Verify {
            url: format!("https://{host}"),
            healthz: Healthz::NotRun,
            http_status: None,
            smoke: Smoke::NotRun,
        }
    }

    fn line(&self) -> String {
        let status = self
            .http_status
            .map(|c| format!(" ({c})"))
            .unwrap_or_default();
        let smoke = serde_json::to_value(self.smoke).unwrap();
        let healthz = serde_json::to_value(self.healthz).unwrap();
        format!(
            "verify: {} healthz {}{status}, smoke {}",
            self.url,
            healthz.as_str().unwrap_or(""),
            smoke.as_str().unwrap_or("")
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub command: String,
    pub status: Status,
    pub steps: Vec<StepResult>,
    pub human_step: Option<HumanStep>,
    pub exit_code: i32,
    /// hoist items read by the steps (JSON `items`).
    pub items: Vec<ReportItem>,
    /// Every pending human step (JSON `checklist`); `human_step` is its first entry.
    pub checklist: Vec<ChecklistEntry>,
    /// Present only when the manifest has `[deploy.fly]`.
    #[serde(skip)]
    pub verify: Option<Verify>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.exit_code == 0
    }
}

#[derive(Serialize)]
struct JsonReport<'a> {
    version: u32,
    command: &'a str,
    ok: bool,
    exit_code: i32,
    status: Status,
    steps: &'a [StepResult],
    human_step: &'a Option<HumanStep>,
    items: &'a [ReportItem],
    checklist: &'a [ChecklistEntry],
    #[serde(skip_serializing_if = "Option::is_none")]
    verify: &'a Option<Verify>,
}

/// The report as one JSON object (no trailing newline).
pub fn render_json(report: &Report) -> String {
    let view = JsonReport {
        version: 1,
        command: &report.command,
        ok: report.ok(),
        exit_code: report.exit_code,
        status: report.status,
        steps: &report.steps,
        human_step: &report.human_step,
        items: &report.items,
        checklist: &report.checklist,
        verify: &report.verify,
    };
    serde_json::to_string(&view).expect("report serializes")
}

#[derive(Serialize)]
struct JsonErrorBody<'a> {
    message: &'a str,
    fix: &'a str,
}

#[derive(Serialize)]
struct JsonError<'a> {
    version: u32,
    command: &'a str,
    ok: bool,
    exit_code: i32,
    status: &'static str,
    steps: [StepResult; 0],
    human_step: Option<HumanStep>,
    error: JsonErrorBody<'a>,
}

/// The `--json` object for an exit-2 or exit-3 error: the same envelope, `status:"error"`.
pub fn render_json_error(command: &str, exit_code: i32, message: &str, fix: &str) -> String {
    let view = JsonError {
        version: 1,
        command,
        ok: false,
        exit_code,
        status: "error",
        steps: [],
        human_step: None,
        error: JsonErrorBody { message, fix },
    };
    serde_json::to_string(&view).expect("error serializes")
}

/// The `--json` object for `--help` (`command:"help"`) and `--version` (`command:"version"`).
pub fn render_json_info(command: &str, key: &str, text: &str) -> String {
    let mut v = serde_json::json!({
        "version": 1, "command": command, "ok": true, "exit_code": 0,
    });
    v[key] = text.trim_end().into();
    v.to_string()
}

/// One line per step, then the human step or `done`. Ends with a newline.
pub fn render_text(report: &Report) -> String {
    let mut out = String::new();
    for s in &report.steps {
        out.push_str(&format!(
            "{:<12} {:<11} {}\n",
            s.id,
            s.action.as_str(),
            s.detail
        ));
    }
    if let Some(v) = &report.verify {
        out.push_str(&format!("\n{}\n", v.line()));
    }
    if let Some(h) = &report.human_step {
        out.push_str(&format!("\nnext: {}\n", h.title));
        if !h.url.is_empty() {
            out.push_str(&format!("  open: {}\n", h.url));
        }
        if !h.command.is_empty() {
            out.push_str(&format!("  then run: {}\n", h.command));
        }
    } else if report.status == Status::Done {
        out.push_str("\ndone\n");
    }
    if !report.checklist.is_empty() {
        out.push_str("\nchecklist:\n");
        for (i, e) in report.checklist.iter().enumerate() {
            let mut line = format!("  {}. [{}] {}", i + 1, e.device, e.title);
            for extra in [&e.url, &e.command] {
                if !extra.is_empty() {
                    line.push_str(" | ");
                    line.push_str(extra);
                }
            }
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Report {
        Report {
            command: "up".into(),
            status: Status::Human,
            steps: vec![
                StepResult {
                    id: "preflight".into(),
                    action: Action::Ok,
                    detail: "node 24".into(),
                },
                StepResult {
                    id: "apps".into(),
                    action: Action::Pending,
                    detail: "Renovate missing".into(),
                },
                StepResult {
                    id: "render".into(),
                    action: Action::NotReached,
                    detail: String::new(),
                },
            ],
            human_step: Some(HumanStep {
                id: "apps".into(),
                title: "Install the Renovate GitHub App".into(),
                url: "https://github.com/apps/renovate/installations/new".into(),
                command: "keel up".into(),
            }),
            exit_code: 5,
            items: vec![],
            checklist: vec![],
            verify: None,
        }
    }

    #[test]
    fn checklist_orders_dedupes_and_drops_confirmed() {
        let h = |id: &str, url: &str| HumanStep {
            id: id.into(),
            title: format!("t {id}"),
            url: url.into(),
            command: "c".into(),
        };
        let steps = [
            h("npm-trust:a", ""),
            h("secret:X", ""),
            h("npm-trust:a", ""),
            h("crates-trust:k", ""),
            h("app", "https://github.com/apps/x/installations/new"),
            h("variable:V", ""),
        ];
        let c = build_checklist(&steps, &["crates-trust:k".into()]);
        let ids: Vec<&str> = c.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["secret:X", "variable:V", "npm-trust:a", "app"]);
        let dev: Vec<&str> = c.iter().map(|e| e.device.as_str()).collect();
        assert_eq!(dev, ["computer", "computer", "phone", "phone"]);
        assert_eq!(
            c[2].verify,
            "hoist cannot read it: run keel up --confirm npm-trust:a once done"
        );
        assert_eq!(c[0].verify, "keel plan re-checks it");
        let mut r = sample();
        r.checklist = c;
        let v: serde_json::Value = serde_json::from_str(&render_json(&r)).unwrap();
        assert_eq!(v["checklist"][0]["device"], "computer");
        assert_eq!(v["items"], serde_json::json!([]));
        let t = render_text(&r);
        assert!(
            t.contains("checklist:\n  1. [computer] t secret:X | c"),
            "{t}"
        );
        assert!(t.contains("  3. [phone] t npm-trust:a"), "{t}");
    }

    #[test]
    fn json_shape() {
        let v: serde_json::Value = serde_json::from_str(&render_json(&sample())).unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["ok"], false);
        assert_eq!(v["exit_code"], 5);
        assert_eq!(v["status"], "human");
        assert_eq!(v["steps"][1]["action"], "pending");
        assert_eq!(v["steps"][2]["action"], "not-reached");
        assert_eq!(v["human_step"]["command"], "keel up");
    }

    #[test]
    fn json_done_has_null_human_step() {
        let mut r = sample();
        r.status = Status::Done;
        r.human_step = None;
        r.exit_code = 0;
        let v: serde_json::Value = serde_json::from_str(&render_json(&r)).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v["human_step"].is_null());
        assert_eq!(v["status"], "done");
    }

    #[test]
    fn json_error_envelope() {
        let v: serde_json::Value =
            serde_json::from_str(&render_json_error("up", 2, "bad", "fix it")).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["exit_code"], 2);
        assert_eq!(v["ok"], false);
        assert_eq!(v["steps"].as_array().unwrap().len(), 0);
        assert!(v["human_step"].is_null());
        assert_eq!(v["error"]["message"], "bad");
        assert_eq!(v["error"]["fix"], "fix it");
    }

    #[test]
    fn text_lists_steps_then_human_step() {
        let t = render_text(&sample());
        assert!(t.contains("apps         pending"), "{t}");
        assert!(t.contains("next: Install the Renovate GitHub App"));
        assert!(t.contains("open: https://github.com/apps/renovate"));
        assert!(t.ends_with('\n'));
        let mut r = sample();
        r.status = Status::Done;
        r.human_step = None;
        assert!(render_text(&r).ends_with("\ndone\n"));
    }

    #[test]
    fn verify_is_in_json_and_text_only_when_present() {
        let mut r = sample();
        let v: serde_json::Value = serde_json::from_str(&render_json(&r)).unwrap();
        assert!(v.get("verify").is_none());
        assert!(!render_text(&r).contains("verify:"));
        r.verify = Some(Verify {
            url: "https://a.io".into(),
            healthz: Healthz::Ok,
            http_status: Some(200),
            smoke: Smoke::NotRun,
        });
        let v: serde_json::Value = serde_json::from_str(&render_json(&r)).unwrap();
        assert_eq!(
            v["verify"],
            serde_json::json!({"url":"https://a.io","healthz":"ok","http_status":200,"smoke":"not-run"})
        );
        assert!(render_text(&r).contains("verify: https://a.io healthz ok (200), smoke not-run"));
        r.verify = Some(Verify::not_run("a.io"));
        let v: serde_json::Value = serde_json::from_str(&render_json(&r)).unwrap();
        assert_eq!(v["verify"]["http_status"], serde_json::Value::Null);
    }

    #[test]
    fn checklist_moves_a_command_out_of_url() {
        let h = HumanStep {
            id: "secret:X".into(),
            title: "set secret X".into(),
            url: "hoist secrets set X".into(),
            command: String::new(),
        };
        let c = build_checklist(&[h], &[]);
        assert_eq!(
            (c[0].url.as_str(), c[0].command.as_str()),
            ("", "hoist secrets set X")
        );
    }
}
