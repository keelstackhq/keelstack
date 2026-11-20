use super::{failure, json, not_found};
use crate::exec::{Cmd, Out, Runner};

/// None: `gh` is not installed.
pub fn auth_ok(r: &dyn Runner) -> Option<bool> {
    let o = r.run(&Cmd::new("gh", &["auth", "status"]));
    if not_found(&o) {
        None
    } else {
        Some(o.code == 0)
    }
}

pub fn login(r: &dyn Runner) -> Option<String> {
    let o = r.run(&Cmd::new("gh", &["api", "user", "--jq", ".login"]));
    let l = o.stdout.trim();
    (o.code == 0 && !l.is_empty()).then(|| l.to_string())
}

/// The repo's default branch; an error detail when the repo does not exist or is not readable.
pub fn default_branch(r: &dyn Runner, repo: &str) -> Result<String, String> {
    let c = Cmd::new("gh", &["repo", "view", repo, "--json", "defaultBranchRef"]);
    let o = r.run(&c);
    if o.code != 0 {
        return Err(failure(&c, &o));
    }
    json(&o)
        .and_then(|v| v["defaultBranchRef"]["name"].as_str().map(str::to_string))
        .ok_or_else(|| super::not_json(&c))
}

/// Of `slugs`, the GitHub Apps installed on `repo` for the user: `gh api user/installations`,
/// then each `selected` installation's repositories.
pub fn installed_apps(r: &dyn Runner, repo: &str, slugs: &[&str]) -> Result<Vec<String>, String> {
    let c = Cmd::new(
        "gh",
        &[
            "api",
            "user/installations",
            "--paginate",
            "--jq",
            ".installations[]",
        ],
    );
    let o = r.run(&c);
    if o.code != 0 {
        return Err(failure(&c, &o));
    }
    let mut found = Vec::new();
    for line in o.stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).map_err(|_| super::not_json(&c))?;
        let Some(slug) = v["app_slug"].as_str() else {
            continue;
        };
        if !slugs.contains(&slug) || found.iter().any(|f| f == slug) {
            continue;
        }
        let covers = if v["repository_selection"].as_str() == Some("selected") {
            let id = v["id"].as_i64().unwrap_or_default().to_string();
            let rc = Cmd::new(
                "gh",
                &[
                    "api",
                    &format!("user/installations/{id}/repositories"),
                    "--paginate",
                    "--jq",
                    ".repositories[].full_name",
                ],
            );
            let ro: Out = r.run(&rc);
            if ro.code != 0 {
                return Err(failure(&rc, &ro));
            }
            ro.stdout
                .lines()
                .any(|l| l.trim().eq_ignore_ascii_case(repo))
        } else {
            true
        };
        if covers {
            found.push(slug.to_string());
        }
    }
    Ok(found)
}

pub fn secret_exists(r: &dyn Runner, repo: &str, env: &str, name: &str) -> bool {
    r.run(&Cmd::new(
        "gh",
        &[
            "api",
            &format!("repos/{repo}/environments/{env}/secrets/{name}"),
        ],
    ))
    .code
        == 0
}

pub fn label_create(r: &dyn Runner, repo: &str, name: &str) -> Result<(), String> {
    let c = Cmd::new("gh", &["label", "create", name, "--repo", repo, "--force"]);
    let o = r.run(&c);
    if o.code == 0 {
        Ok(())
    } else {
        Err(failure(&c, &o))
    }
}

/// `gh secret set NAME --env ENV --repo R`; the value goes on stdin, never into argv.
pub fn secret_cmd(repo: &str, env: &str, name: &str, value: &[u8]) -> Cmd {
    Cmd::new("gh", &["secret", "set", name, "--env", env, "--repo", repo]).stdin(value.to_vec())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pr {
    pub number: u64,
    pub url: String,
    /// `OPEN`, `MERGED` or `CLOSED`.
    pub state: String,
    pub head: String,
}

/// The PR of `branch`, if any: `gh pr view <branch> --repo R --json number,url,state,headRefOid`.
pub fn pr_view(r: &dyn Runner, dir: &std::path::Path, repo: &str, branch: &str) -> Option<Pr> {
    let o = r.run(
        &Cmd::new(
            "gh",
            &[
                "pr",
                "view",
                branch,
                "--repo",
                repo,
                "--json",
                "number,url,state,headRefOid",
            ],
        )
        .dir(dir),
    );
    if o.code != 0 {
        return None;
    }
    let v = json(&o)?;
    Some(Pr {
        number: v["number"].as_u64()?,
        url: v["url"].as_str()?.to_string(),
        state: v["state"].as_str()?.to_string(),
        head: v["headRefOid"].as_str().unwrap_or("").to_string(),
    })
}

/// `gh pr create --repo R --head B --title T --body BODY`
pub fn pr_create_cmd(
    dir: &std::path::Path,
    repo: &str,
    branch: &str,
    title: &str,
    body: &str,
) -> Cmd {
    Cmd::new(
        "gh",
        &[
            "pr", "create", "--repo", repo, "--head", branch, "--title", title, "--body", body,
        ],
    )
    .dir(dir)
}
