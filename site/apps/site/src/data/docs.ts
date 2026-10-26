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
      "bosun/docs/spec.md",
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
    slug: "getting-started",
    title: "Getting started",
    summary:
      "Step by step: template, channels, bosun, repo settings, CI cache, berth and the first task in your own repo.",
    sources: [
      "splice/README.md",
      "splice/docs/getting-started.md",
      "hull/README.md",
      "hull/CHANGELOG.md",
      "hull/docs/pieces.md",
      "hull/rigging/pieces/core/piece.toml",
      "decisions/decisions/004-decisions-channel.md",
      "bosun/docs/spec.md",
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
    slug: "tasks",
    title: "Tasks and sessions",
    summary:
      "How bosun cuts work into plans, epics and tasks, the task states, leases, sessions and waves, and what stops work.",
    sources: [
      "bosun/docs/spec.md",
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
      "bosun/docs/spec.md",
      "bosun/CHANGELOG.md",
      "bosun/channel/pieces/core/files/.github/workflows/bosun.yml.jinja",
      "bosun/channel/pieces/core/files/hoist.toml.jinja",
      "bosun/profiles/rust.toml",
      "bosun/profiles/ts-pnpm.toml",
      "hull/README.md",
      "hull/CHANGELOG.md",
      "hoard/README.md",
      "rust-canary/docs/canary-findings.md",
    ],
    ready: true,
  },
  {
    slug: "risk",
    title: "Risk and review",
    summary:
      "Risk rules and approval classes, review verdicts, owner approvals (and why cloud-session approvals do not count), asking the owner, and the kill switch.",
    sources: [
      "bosun/docs/spec.md",
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
      "hoist/README.md",
      "bosun/docs/releasing.md",
      "splice/.github/workflows/release.yml",
      "hull/template/pieces/base/files/.github/workflows/release.yml",
      "hull/template/pieces/ts-lib/files/AGENTS.md",
      "hull/CHANGELOG.md",
      "bosun/docs/spec.md",
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
      "bosun/docs/spec.md",
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
    summary: "TODO(copy): troubleshooting placeholder.",
    sources: [],
    ready: false,
  },
];

export const docHref = (slug: string): string => (slug === "" ? "/docs/" : `/docs/${slug}/`);

export const readyDocs = (): DocPage[] => docs.filter((d) => d.ready);

export function sourceUrl(source: string): string {
  const i = source.indexOf("/");
  return `${org.url}/${source.slice(0, i)}/blob/main/${source.slice(i + 1)}`;
}
