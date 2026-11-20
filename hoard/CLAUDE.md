# hoard

One-line CI cache for TypeScript monorepos (Turborepo, Vite+). A Rust binary serves the Turborepo
remote-cache API on localhost and stores artifacts in the GitHub Actions cache, a Cloudflare R2
bucket, or the local filesystem. A small node24 GitHub Action downloads, starts and stops it.
Lives in the `hoard/` folder of the keelstack monorepo; run Rust commands from the repo root.

- Design: `docs/superpowers/specs/2026-10-14-hoard-design.md` — the source of truth. Read the
  section you touch before changing behaviour; update the spec in the same PR when behaviour changes.
- Plans: `docs/superpowers/plans/` — one per milestone; tick checkboxes as tasks land.
- Progress: `docs/PROGRESS.md` — live status and anything blocked on a human (≤ 40 lines).

## Layout

```
src/               Rust crate (lib + bin "hoard")
  config.rs        CLI flags, HOARD_* env vars, defaults
  key.rs           hash validation, key layout {prefix}{tool}/{version}/{hash}
  envelope.rs      HRD1 blob format (header + raw artifact body)
  store.rs         Store over an OpenDAL Operator (ghac | s3→R2 | fs)
  gha_twirp.rs     GitHub cache v2 Twirp client (vp archives, packs, Range reads)
  pack/            packing (spec §6.5): index, names, backend, reader, writer
  writebehind.rs   spooled background uploads (pack=off)
  ratelimit.rs     429 pause gate; redact.rs: secret-free errors; doctor.rs
  serve.rs, standalone.rs, stats.rs   server loop, `serve --detach`/`stop`, counters
  server/          axum router: Turborepo /v8 API, auth, /_hoard/*
  r2/              lifecycle rule (aws-sdk-s3), size-cap prune, refresh-on-hit
  vp/              Vite+ task-cache restore/save
tests/             integration tests (http.rs, http_pack.rs, pack.rs, r2.rs, vp.rs, …), e2e/, fixtures/
action.yml         GitHub Action entry (repo root)
action/            TypeScript action source (src/, test/) and committed dist/
scripts/           release and benchmark helpers
```

## Commands

```bash
docker compose up -d minio                       # S3/R2 tests need it
export HOARD_TEST_S3_ENDPOINT=http://127.0.0.1:9000
cargo fmt --all --check && cargo clippy -p keel-hoard --all-targets --locked -- -D warnings
cargo test -p keel-hoard --locked                # unit + integration
cargo test -p keel-hoard --locked -- --ignored   # large-artifact streaming test
tests/e2e/turbo.sh                               # real turbo, fs storage
PACK=on tests/e2e/turbo.sh                       # real turbo, fs storage, packing
STORAGE=r2 tests/e2e/turbo.sh                    # real turbo, MinIO as R2
pnpm --dir action install && pnpm --dir action lint && pnpm --dir action test
pnpm --dir action build                          # must keep action/dist ≤ 204800 bytes
```

Run the fast checks (fmt, clippy, the tests of the crate or package you changed) before every
commit. Run the e2e script before pushing anything that touches `server/`, `store.rs` or `envelope.rs`.

## Rules that must never break

- **Never fail the user's build because of the cache.** Storage errors: GET/HEAD → 404 (miss),
  PUT → 200 and count `put_errors`. Only setup errors (bad config, missing GitHub env, missing R2
  lifecycle rule in `auto`/`verify`) fail, and their message must say how to fix it.
- **Listen on 127.0.0.1 only.** Every route except `/_hoard/health` checks the bearer token in
  constant time.
- **Never log secrets:** the token, R2 keys, `ACTIONS_RUNTIME_TOKEN`, or any URL with `sig=` or
  `X-Amz-Signature`. Secrets reach the binary through env vars, never argv.
- **Stream, don't buffer.** No artifact is ever held fully in memory. Never recompress the
  artifact body (Turborepo already sends zstd).
- **No silent fallbacks.** If GitHub cache env vars are missing, fail; do not cache to disk instead.
- **The R2 bucket must stay bounded.** Keep the lifecycle rule (expire after `max-age-days`,
  abort multipart after 1 day) and the size-cap prune working; never drop other lifecycle rules.
- **Keys are immutable.** Writes use if-not-exists; an existing key is success.
- **Hashes match `^[a-fA-F0-9]{1,128}$`** before they reach any key.
- `rustls` only; no OpenSSL (static musl release builds).

## How to work here

- Test first: write the failing test, watch it fail, implement, watch it pass, commit
  (superpowers:test-driven-development).
- Execute plans task by task (superpowers:subagent-driven-development or executing-plans); one
  commit per task, message as in the plan (`feat:`, `fix:`, `test:`, `ci:`, `docs:`).
- Keep files focused; a new responsibility gets a new module, not a bigger file.
- Public behaviour (flags, env vars, routes, key layout, envelope, action inputs/outputs) changes
  only together with the spec and the README.
- Dependencies: ask before adding a crate or npm package not listed in the plan's Tech Stack.
- Before claiming something works, show the command output (superpowers:verification-before-completion).

## Gotchas

- Only JavaScript actions receive `ACTIONS_RUNTIME_TOKEN` / `ACTIONS_RESULTS_URL`; the action
  passes them to the detached binary. `run:` steps cannot use the GitHub cache backend directly.
- GitHub cache limits: 10 GB per repo (free), 200 uploads/min and 1,500 downloads/min, branch
  scoping, immutable keys. On 429 pause uploads, keep reads.
- R2 lifecycle age counts from the last write, not the last read.
- OpenDAL ignores `if_not_exists` unless the service reports `write_with_if_not_exists`; fall back
  to `stat` first.
- Releases are lockstep for all tools: one version, `scripts/bump-version.sh`, tag created by the
  release workflow only (root `AGENTS.md`). Never tag or publish by hand.
- Vite+ remote caching is unreleased upstream; until it ships we only save/restore its
  task-cache directory.

Org decisions: read `../decisions/DECISIONS.md` in place; change one by a PR to `decisions/` (D-002).
