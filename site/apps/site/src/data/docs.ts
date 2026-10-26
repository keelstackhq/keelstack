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
    summary: "TODO(copy): tasks and sessions placeholder.",
    sources: [],
    ready: false,
  },
  {
    slug: "ci",
    title: "CI and gates",
    summary: "TODO(copy): CI and gates placeholder.",
    sources: [],
    ready: false,
  },
  {
    slug: "risk",
    title: "Risk and review",
    summary: "TODO(copy): risk and review placeholder.",
    sources: [],
    ready: false,
  },
  {
    slug: "releases",
    title: "Releases and updates",
    summary: "TODO(copy): releases and updates placeholder.",
    sources: [],
    ready: false,
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
