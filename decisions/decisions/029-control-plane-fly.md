# D-029: One control plane for six services; Fly joins hoist

- Date: 2026-11-26
- Scope: all
- Status: accepted
- Supersedes: —

## Context

This decision amends D-026 (its line "Fly and other personal services stay out of hoist"); it
supersedes nothing, so D-026 stays in the index. On 2026-11-26 the owner required (issue
`keelstackhq/keelstack#65`) that the CLI talk to the services he uses regularly and "no more
services": GitHub org, Cloudflare, npmjs, crates.io, Google Search Console and Fly.io. `hoist org`
(D-026) already covers the first five from `org.toml`; Fly was left to a private repo.

## Decision

- The service list is closed: GitHub org, Cloudflare, npm, crates.io, Google Search Console, Fly.io.
  Adding a seventh needs a new decision.
- Fly.io is section `[fly]` of `org.toml` in `hoist/` (spec §16.7), with the same engine, exit codes
  (D-020: 2 = human step, `unknown` when unreadable) and rules as the other sections: `hoist org
plan` and `check` send only GET requests, `apply` writes after the preflight, no secret is ever
  printed. It manages the org slug, apps (name, primary region), app secrets by name only and
  certificates for custom domains, through the Fly Machines HTTP API with `FLY_API_TOKEN` (a
  proxy-injected dummy value works, spec §16.5). hoist never sets a secret value on Fly, never
  deletes and never moves machines.
- `hoist org apply` deploys the Search Console bridge Worker from the binary (`[search_console.worker]`,
  spec §16.8), replacing the hand-deployed reference. Its two Worker secrets (bearer token,
  service-account key) come from the environment or a hidden prompt, never from argv.
- Humans still do what no API can: create the Fly org, create the Fly token, create the Google
  service account and key, create the npm org, add the tokens to the cloud environment's
  credential form. hoist lists and verifies them (D-009), never performs them.

## Consequences

- `hoist/docs/superpowers/specs/2026-10-16-hoist-design.md` §16.7 and §16.8 are the contract.
- D-026 keeps its status; only its Fly exclusion is amended.
- The owner's first real `hoist org check` needs `FLY_API_TOKEN` besides the D-026 credentials
  (steps in `hoist/docs/PROGRESS.md`).
