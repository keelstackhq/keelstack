import { org } from "./tools";

export type Repo =
  | "hull"
  | "splice"
  | "bosun"
  | "hoist"
  | "hoard"
  | "berth"
  | "decisions"
  | "rust-canary";

export interface DocPage {
  /** "" for the overview (URL `/docs/`), else e.g. "getting-started". */
  slug: string;
  title: string;
  summary: string;
  /** Repo-relative paths, first segment is the repo (see `Repo`). */
  sources: string[];
  ready: boolean;
}

export const docs: DocPage[] = [
  {
    slug: "",
    title: "Overview",
    summary: "What each keelstack tool does and how they fit together.",
    sources: [
      "hull/README.md",
      "hull/CHANGELOG.md",
      "splice/README.md",
      "bosun/README.md",
      "bosun/docs/spec",
      "bosun/docs/releasing.md",
      "hoist/README.md",
      "hoard/README.md",
      "berth/README.md",
      "decisions/README.md",
      "decisions/DECISIONS.md",
    ],
    ready: true,
  },
  {
    slug: "start-here",
    title: "Start here",
    summary:
      "For agents: the ordered path from an empty GitHub repo to the first merged task, linking every detail page.",
    sources: [
      "splice/README.md",
      "hull/README.md",
      "bosun/README.md",
      "bosun/docs/spec",
      "bosun/AGENTS.md",
      "hoist/README.md",
      "decisions/DECISIONS.md",
    ],
    ready: true,
  },
  {
    slug: "start-project",
    title: "Start a project",
    summary:
      "Every owner step before agents work on a new hull repo, with how the ready check verifies each one (hull's start checklist).",
    sources: ["hull/docs/start-checklist.md", "hull/template/pieces/base/files/scripts/ready.mjs"],
    ready: true,
  },
  {
    slug: "greenfield-bosun",
    title: "Greenfield repo with bosun",
    summary:
      "A new hull repo that bosun runs: the files, where the spec and the plan go, the order of owner and agent steps, and a first session prompt.",
    sources: [
      "bosun/docs/spec/03-layout.md",
      "bosun/docs/spec/05-spawn.md",
      "bosun/docs/spec/11-seed.md",
      "bosun/docs/spec/12-plan.md",
      "bosun/docs/plan-format.md",
      "bosun/src/cli/init.rs",
      "hull/docs/start-checklist.md",
    ],
    ready: true,
  },
  {
    slug: "getting-started",
    title: "Getting started",
    summary:
      "Step by step: template, channels, bosun, repo settings, CI cache, berth and the first task in your own repo.",
    sources: [
      "hull/template/pieces/base/files/.claude/skills/ready/SKILL.md",
      "hull/template/pieces/base/files/scripts/ready.mjs",
      "splice/README.md",
      "splice/docs/getting-started.md",
      "hull/README.md",
      "hull/CHANGELOG.md",
      "hull/docs/pieces.md",
      "hull/rigging/pieces/core/piece.toml",
      "decisions/decisions/004-decisions-channel.md",
      "bosun/docs/spec",
      "bosun/docs/PROGRESS.md",
      "bosun/docs/plan-format.md",
      "bosun/src/cli/init.rs",
      "bosun/channel/pieces/core/files/docs/setup-script.sh.jinja",
      "bosun/channel/pieces/core/files/hoist.toml.jinja",
      "hoist/README.md",
      "hoard/README.md",
      "berth/README.md",
      "rust-canary/docs/canary-findings.md",
    ],
    ready: true,
  },
  {
    slug: "cloud",
    title: "Cloud environment",
    summary:
      "A dedicated Claude Code cloud environment: setup script, network allow-list, secrets and keys, and the owner's phone steps.",
    sources: [
      "bosun/docs/setup-script.sh",
      "bosun/channel/pieces/core/files/docs/setup-script.sh.jinja",
      "bosun/docs/spec",
      "bosun/docs/PROGRESS.md",
      "bosun/.bosun/approvers.toml",
    ],
    ready: true,
  },
  {
    slug: "tasks",
    title: "Tasks and sessions",
    summary:
      "How bosun cuts work into plans, epics and tasks, the task states, leases, sessions and waves, and what stops work.",
    sources: [
      "bosun/docs/spec",
      "bosun/docs/plan-format.md",
      "bosun/docs/defaults.md",
      "bosun/AGENTS.md",
      "bosun/CHANGELOG.md",
      "rust-canary/docs/canary-findings.md",
    ],
    ready: true,
  },
  {
    slug: "ci",
    title: "CI and gates",
    summary:
      "Required checks, the gate contract, test-integrity, pinned workflows, own CI, the docs-only fast path and caching.",
    sources: [
      "bosun/docs/spec",
      "bosun/CHANGELOG.md",
      "bosun/channel/pieces/core/files/.github/workflows/bosun.yml.jinja",
      "bosun/docs/ci-minutes.md",
      "bosun/channel/pieces/core/files/hoist.toml.jinja",
      "bosun/profiles/rust.toml",
      "bosun/profiles/ts-pnpm.toml",
      "hull/README.md",
      "hull/CHANGELOG.md",
      "hoard/README.md",
      "hull/docs/actions.md",
      "hoard/docs/circleci.md",
      "rust-canary/docs/canary-findings.md",
    ],
    ready: true,
  },
  {
    slug: "risk",
    title: "Risk and review",
    summary:
      "Risk rules and approval classes, review verdicts, owner approvals on GitHub or signed from chat, delegated decisions, asking the owner, and the kill switch.",
    sources: [
      "bosun/.bosun/approvers.toml",
      "bosun/docs/spec",
      "bosun/README.md",
      "bosun/REVIEW.md",
      "bosun/CHANGELOG.md",
      "bosun/docs/PROGRESS.md",
      "bosun/channel/pieces/core/files/.bosun/risk.toml.jinja",
      "bosun/channel/pieces/core/piece.toml",
      "rust-canary/docs/canary-findings.md",
    ],
    ready: true,
  },
  {
    slug: "releases",
    title: "Releases and updates",
    summary:
      "Releasing with hoist (and the one-time first publish), tag-on-merge, hull template releases, pinning bosun, the splice update bot and the decisions channel.",
    sources: [
      "hull/.github/workflows/release.yml",
      "bosun/CHANGELOG.md",
      "bosun/changelog.d/README.md",
      "bosun/.github/workflows/release.yml",
      "hoist/README.md",
      "bosun/docs/releasing.md",
      "splice/.github/workflows/release.yml",
      "hull/template/pieces/base/files/.github/workflows/release.yml",
      "hull/template/pieces/ts-lib/files/AGENTS.md",
      "hull/template/pieces/secrets/files/docs/agents/secrets.md",
      "hull/CHANGELOG.md",
      "bosun/docs/spec",
      "splice/README.md",
      "splice/docs/getting-started.md",
      "decisions/README.md",
      "decisions/CLAUDE.md",
      "decisions/DECISIONS.md",
    ],
    ready: true,
  },
  {
    slug: "playbook",
    title: "Playbook",
    summary:
      "How to bootstrap and run a repo with agents: the orchestrator session, child sessions, waves, owner decisions and approvals.",
    sources: [
      "bosun/README.md",
      "bosun/CHANGELOG.md",
      "bosun/channel/pieces/core/files/sections/agents.md",
      "bosun/docs/spec",
      "bosun/docs/defaults.md",
      "bosun/AGENTS.md",
      "bosun/docs/PROGRESS.md",
      "decisions/DECISIONS.md",
      "splice/.github/workflows/release.yml",
      "hoist/README.md",
      "rust-canary/docs/canary-findings.md",
    ],
    ready: true,
  },
  {
    slug: "troubleshooting",
    title: "Troubleshooting",
    summary:
      "Known pitfalls with cause and fix: repo settings, approvals, bot tokens, templates, bosun exit 4, and exit codes per tool.",
    sources: [
      "bosun/tests/governance.rs",
      "rust-canary/docs/canary-findings.md",
      "bosun/docs/spec",
      "bosun/CHANGELOG.md",
      "bosun/docs/PROGRESS.md",
      "hoist/README.md",
      "hoist/docs/PROGRESS.md",
      "splice/README.md",
      "splice/docs/getting-started.md",
      "splice/.github/workflows/release.yml",
      "splice/action/action.yml",
      "hull/README.md",
      "berth/README.md",
    ],
    ready: true,
  },
];

export const docHref = (slug: string): string => (slug === "" ? "/docs/" : `/docs/${slug}/`);

export const readyDocs = (): DocPage[] => docs.filter((d) => d.ready);

export function sourceUrl(source: string): string {
  const i = source.indexOf("/");
  return `${org.url}/${source.slice(0, i)}/blob/main/${source.slice(i + 1)}`;
}
