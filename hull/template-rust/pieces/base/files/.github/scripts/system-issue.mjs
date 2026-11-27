// Opens, updates or closes ONE system-check issue for system-check.yml. Node built-ins and fetch only.
//   SYSTEM_EXIT=<exit code of `hoist system check --live --json`>  SYSTEM_REPORT=<file with its JSON>
//   GITHUB_TOKEN, GITHUB_REPOSITORY, GITHUB_API_URL (set by Actions). The token is never logged;
//   hoist never prints secret values, so the report holds names and metadata only.
// Exit 1 (drift), 2 (rotation due) or 3 (invalid system.toml): create the issue (label
// `system-check`, fixed title) or update its body; exit 0 comments and closes it. Exits 1 only
// when an API call fails.
import { readFileSync, realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";

export const TITLE = "system.toml: drift or credentials to rotate";
export const LABEL = "system-check";
const MAX_BODY = 60000;
const MEANING = {
  1: "drift",
  2: "a human step is pending",
  3: "invalid system.toml or usage error",
};

/** A fenced block whose fence is longer than any backtick run in `text`. */
function fence(text) {
  const longest = Math.max(2, ...[...text.matchAll(/`+/g)].map((m) => m[0].length));
  const f = "`".repeat(longest + 1);
  return `${f}\n${text}\n${f}`;
}

const cell = (s) =>
  String(s ?? "")
    .replace(/\|/g, "\\|")
    .replace(/\s+/g, " ");

function body(exit, report, raw) {
  const lines = [
    `\`hoist system check --live --json\` exited ${exit} (${MEANING[exit] ?? "unexpected"}); weekly run of system-check.yml.`,
    "Fix `system.toml` (record dates with `hoist system stamp`), rotate what is due, or the workflows.",
    "This issue closes itself when a run is clean (`unknown` alone is clean).",
    "",
  ];
  if (!report) {
    lines.push(fence((raw || "(no report captured; see the workflow run)").trim()));
  } else {
    const items = (report.items ?? []).filter((i) => i.action !== "ok");
    if (items.length) {
      lines.push("| action | kind | key | reason | source |", "| --- | --- | --- | --- | --- |");
      for (const i of items)
        lines.push(
          `| ${cell(i.action)} | ${cell(i.kind)} | ${cell(i.key)} | ${cell(i.reason)} | ${cell(i.source)} |`,
        );
    }
    const steps = report.human_steps ?? [];
    if (steps.length) {
      lines.push("", "Human steps:", "");
      for (const s of steps) lines.push(`- ${cell(s.title)}${s.url ? ` (${s.url})` : ""}`);
    }
  }
  let text = lines.join("\n");
  if (text.length > MAX_BODY) text = `${text.slice(0, MAX_BODY)}\n\n… (truncated)`;
  return text;
}

/** Decide what to do. `issues` are the open issues labelled `system-check`. */
export function plan({ exit, report, raw = "", issues }) {
  const mine = issues.filter((i) => i.title === TITLE).sort((a, b) => a.number - b.number)[0];
  if (Number(exit) === 0) {
    if (!mine) return { action: "none" };
    return {
      action: "close",
      number: mine.number,
      comment: "The weekly system check is clean. Closing.",
    };
  }
  const text = body(Number(exit), report, raw);
  if (!mine) return { action: "create", title: TITLE, body: text };
  if (mine.body === text) return { action: "none", number: mine.number };
  return { action: "update", number: mine.number, body: text };
}

async function main(env) {
  const api = (env.GITHUB_API_URL || "https://api.github.com").replace(/\/$/, "");
  const repo = env.GITHUB_REPOSITORY;
  if (!env.GITHUB_TOKEN || !repo)
    throw new Error("GITHUB_TOKEN and GITHUB_REPOSITORY are required");
  const call = async (method, path, payload, okStatus = [200, 201]) => {
    const res = await fetch(`${api}/repos/${repo}${path}`, {
      method,
      headers: {
        authorization: `Bearer ${env.GITHUB_TOKEN}`,
        accept: "application/vnd.github+json",
        "content-type": "application/json",
        "x-github-api-version": "2022-11-28",
      },
      body: payload ? JSON.stringify(payload) : undefined,
    });
    if (!okStatus.includes(res.status)) throw new Error(`${method} ${path}: HTTP ${res.status}`);
    return res.status === 204 ? null : res.json();
  };
  const exit = /^\d+$/.test(env.SYSTEM_EXIT ?? "") ? Number(env.SYSTEM_EXIT) : 3;
  let raw = "";
  let report = null;
  try {
    raw = readFileSync(env.SYSTEM_REPORT || "system-check.json", "utf8");
    report = JSON.parse(raw);
  } catch {
    report = null;
  }
  const found = await call("GET", `/issues?labels=${LABEL}&state=open&per_page=100`);
  const issues = found.filter((i) => !i.pull_request);
  const p = plan({ exit, report, raw, issues });
  if (p.action === "create") {
    // The label may not exist yet; 422 means it does.
    await call("POST", "/labels", { name: LABEL, color: "fbca04" }, [200, 201, 422]);
    await call("POST", "/issues", { title: p.title, body: p.body, labels: [LABEL] });
  } else if (p.action === "update") {
    await call("PATCH", `/issues/${p.number}`, { body: p.body });
  } else if (p.action === "close") {
    await call("POST", `/issues/${p.number}/comments`, { body: p.comment });
    await call("PATCH", `/issues/${p.number}`, { state: "closed", state_reason: "completed" });
  }
  console.log(`system-check: ${p.action}${p.number ? ` #${p.number}` : ""}`);
}

const isMain = () => {
  const entry = process.argv[1];
  if (!entry) return false;
  try {
    return realpathSync(entry) === realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
};
if (isMain()) {
  main(process.env).catch((e) => {
    console.error(`system-check: ${e.message}`);
    process.exit(1);
  });
}
