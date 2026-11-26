# D-026: hoist covers the org as well as the repo

- Date: 2026-11-23
- Scope: all
- Status: accepted
- Supersedes: —

## Context

hoist set up one repo from `hoist.toml`. The org around the repos (provision prerequisites H1-H5,
npm and crates.io registry setup, Cloudflare zones, DNS and Pages domains, Google Search Console)
had no tool and was done by hand from a README. Issue #65 asked for one command that converges an
org once its secrets and a short list of one-click human steps exist, without an eighth tool.

## Decision

- hoist's scope is "a repo's and its org's setup". The org half is `hoist org plan|check|apply`
  in `hoist/`, driven by `org.toml` (separate from `hoist.toml`; it may live in a private owner
  repo). It reuses hoist's engine, clients, `--json`, the D-020 hoist exit codes (2 = human step)
  and the D-009 checklist: human steps are listed and verified, never performed.
- GitHub org writes run only in a GitHub Actions workflow with the provisioner app's
  installation token (D-014). Local and cloud-session runs are read-only for GitHub; anything
  hoist cannot read is `unknown`. No PAT.
- Static-header APIs (Cloudflare, npm, crates.io, the Search Console bridge) work with a
  proxy-injected credential; the process holds a dummy value (hoist spec §16.5).
- Google Search Console goes through a bridge Worker the owner deploys from
  `hoist/examples/search-console-bridge/` (reference code, not a tool or hull piece); hoist
  verifies domains with a DNS TXT record it writes through its Cloudflare client.
- ~~Fly and other personal services stay out of hoist.~~ Amended by D-029 (2026-11-26): Fly.io
  joins as `[fly]`; the service list is closed (GitHub, Cloudflare, npm, crates.io, Search
  Console, Fly). Everything else in this decision stands; its status stays accepted.

## Consequences

- `hoist/docs/superpowers/specs/2026-10-16-hoist-design.md` §16 is the contract for `org.toml`.
- `hoist org check --area github` replaces the provision README checklist (H1-H5).
- A new top-level tool for org setup is off the table; extend hoist instead.
