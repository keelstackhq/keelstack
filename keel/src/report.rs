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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub command: String,
    pub status: Status,
    pub steps: Vec<StepResult>,
    pub human_step: Option<HumanStep>,
    pub exit_code: i32,
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
        }
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
}
