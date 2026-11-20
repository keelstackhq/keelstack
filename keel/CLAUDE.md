# keel

Bootstraps a repo across the keelstackhq tools (splice, bosun, hoist, hull templates) from one
manifest, `keel.toml`: `keel up|plan|doctor`. keel runs each tool as a subprocess, reads its JSON
and stops at the first step only a human can do. One Rust binary. Part of the keelstackhq family.

- Spec: `docs/superpowers/specs/2026-11-20-keel-design.md` — source of truth (§3 CLI, §3.2 JSON,
  §4 manifest, §5 steps, §6 pins, §7 PAT gate, §8 agent env). Read only the section you touch;
  update it in the same commit when behaviour changes.
- Plans: `docs/superpowers/plans/` (M1: `2026-11-20-keel-m1.md`). Tick tasks as they land.
- Progress: `docs/PROGRESS.md` — Status and Blocked (human-only items), ≤ 40 lines.

## Layout

```
src/main.rs      parse (clap errors -> exit 2), run, print lines or JSON, exit code
src/cli.rs       Cli, Command (up|plan|doctor); global -C, --json, -v
src/lib.rs       run(Cli) -> Result<Report, KeelError>
src/error.rs     KeelError (Usage exit 2, others exit 3, message says how to fix)
src/report.rs    Action, StepResult, HumanStep, Report; render_json, render_text
src/exec.rs      Runner trait: real subprocess runner and FakeRunner for tests
src/manifest.rs  keel.toml read/write, unknown keys are an error
src/pins.rs      pins parser; src/pins.toml  embedded default pins (KEEL_PINS overrides)
src/ctx.rs       run context (repo, branch, flags); src/step.rs Step trait, Outcome
src/engine.rs    runs steps in order, stops at the first pending or failed one
src/tools/       adapters: gh, npm, splice, bosun, hoist (argv builders, output parsers)
src/steps/       preflight, apps, render, governance, settings, token, agent_env, doctor, merge
src/proxy_ca.sh  proxy-CA prelude for the agent environment script (include_str!)
npm/keel, npm/keel-<platform>   launcher (bin/keel.js, node:test) and 5 platform packages
tests/           cli_*.rs integration tests with stub tool scripts on PATH
```

## Commands

```bash
export PATH=/root/.cargo/bin:$PATH
cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings
cargo test                          # unit + integration
cargo test --test cli_up            # one file
node --test npm/keel/test/*.test.js # launcher
```

Run the fast checks (fmt, clippy, test) before every commit.

## Rules that must never break

- **Exit codes (D-020):** 0 done · 1 step failed or drift · 2 usage (clap errors too) · 3 manifest,
  environment or API error (message says how to fix it) · 5 human step pending.
- **`--json` stdout is exactly one object** (spec §3.2). Logs go to stderr.
- **Subprocesses only:** keel calls tools and never reimplements them.
- **Never leak the PAT:** it never reaches argv, files, logs, stdout, stderr or JSON; it goes to
  `gh secret set` on stdin only and is zeroized after use.
- **No HTTP or TLS crate** (`gh` does GitHub); `#![forbid(unsafe_code)]`.
- **Tests never call real tools:** unit tests use FakeRunner, integration tests use stub scripts
  on `PATH`. The dev environment has real tokens.
- **Never merge, tag, release, publish or force-push.** keel opens the PR; a human merges.
- **`keel.toml` unknown keys are an error** (exit 3, names the key).

## How to work here

- Test first: failing test, watch it fail, implement, watch it pass; one task per commit,
  message as in the plan (`feat:`, `fix:`, `test:`, `ci:`, `docs:`).
- Pin crates exactly (see the plan's Tech Stack). Ask before adding any other crate.
- Read big files by section (offset/limit or grep), not whole.
- Public behaviour (flags, JSON, exit codes) changes only with the spec and README.
- Show real command output before claiming something works.
- Human-only items (tokens, repo settings, releases, npm scopes) go to Blocked in
  `docs/PROGRESS.md` with the exact action.

## Gotchas

- `TMPDIR` is `/` in some containers; set it for `cargo` and for child processes in tests.
- clap exits 2 on usage errors by default; `main` keeps that (help/version exit 0).
- keel itself needs Node 24+ only for the tools it installs; the launcher runs on Node 18+.
- Tool features may be missing in old versions (splice/bosun exit 2): adapters degrade to a human
  step or warning, never a crash.
