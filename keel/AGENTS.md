# AGENTS.md

keel bootstraps a repo across the keelstackhq tools from one manifest (`keel up|plan|doctor`).
Read `CLAUDE.md` for layout, commands and how to work; the spec is
`docs/superpowers/specs/2026-11-20-keel-design.md`. Short version of the rules:

- Exit codes: 0 done, 1 step failed or drift, 2 usage error, 3 manifest/env/API error, 5 human step pending.
- `--json` prints exactly one object on stdout (spec §3.2); logs go to stderr.
- keel calls tools as subprocesses and never reimplements them.
- The PAT never reaches argv, files, logs or output. No HTTP/TLS crate; `#![forbid(unsafe_code)]`.
- Tests never call real tools: FakeRunner or stub scripts on `PATH`.
- Never merge, tag, release, publish or force-push.
- Run `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings` and `cargo test`
  before every commit. Human-only items go to Blocked in `docs/PROGRESS.md`.
