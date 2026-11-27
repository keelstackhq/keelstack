# Plan: one-shot setup (keel up)

project: keelstack
pr: one

Goal: one agent command plus one owner command take any repo (new, or on an old keelstack
release with legacy sources) to the latest release in autonomous mode with the CI signer, with
at most two owner touches: one owner command on a computer and one approval. Today the anu
migration (0.13.0 to 0.25.0) took about 20 manual steps, 3 PRs, a hand-written owner script and
2 source-repoint tricks, and it found the blocking GitHub plan limit last.

Friction seen (anu, 2026-11-27), each fixed by the task named:

| # | Friction | Task |
|---|---|---|
| 1 | `keel migrate` said "up to date": legacy `gh:keelstackhq/{hull,bosun}` sources stop at 0.13.0/0.21.0 | S1, K2 |
| 2 | bosun has no legacy-tags map; the 0.21.0 commit had to be found in the git log | S1 |
| 3 | `--dry-run` fails when splice/bosun/hoist are not installed | K1 |
| 4 | Drift: local notes in an owned `AGENTS.md` moved to the seed `CLAUDE.md` by hand; deleted channel files excluded by hand | K3 |
| 5 | Channel post command `pnpm install` ran at the repo root (no package.json) and failed the update | S2 |
| 6 | A channel var change (`bosun_app=1`) has no re-render: owned files had to be deleted, then `--force` | S3 |
| 7 | `--mode autonomous` does not write the D-028 knobs (quorum, delegation, 1 verdict, blocker-only, main_health, [cloud]) | B1 |
| 8 | CI signer: App, key, 2 environments, variable, 2 secrets, keygen, 3 PRs, a ruleset apply, in a fixed order | B2, B3 |
| 9 | GitHub Free + private repo (no rulesets, no auto-merge, maybe no env secrets) found after the owner did everything | K1 |
| 10 | `hoist check` cannot resolve a private App slug from a cloud session | H1 |
| 11 | Cloud container had no cargo-deny/cargo-nextest; pinned versions not installed | BT |
| 12 | Seed text goes stale (`Generated from hull-rust 0.13.0`, pins.toml splice/bosun values) | S4 |
| 13 | `keel migrate --pr` makes its own branch; cloud sessions must push to an assigned branch | K1 |
| 15 | Public repo: no way to make agents act only on issues and comments from owners/members | BA |
| 14 | Merged-branch reuse: agent needed a force push to restart the session branch | K4 |

## Global constraints

- One version (lockstep); exact pins; `--json` prints one object; exit codes 0/1/2/3/5 as today.
- Every new command has `--dry-run` that writes nothing, and its JSON lists the owner steps with `device`.
- Owner secrets never pass through an agent session: the owner command handles keys locally and deletes them.
- The guard rails (no merge, tag, release, force push by agents) stay unchanged.

## S1: Legacy sources repoint themselves

area: splice
depends: seed

`splice update` sees a legacy source (`gh:keelstackhq/<tool>//<dir>`) and rewrites it to the
monorepo form, mapping the locked commit through `<tool>/legacy-tags.txt`; one commit
`chore(splice): repoint <source> to the monorepo`.

### Tasks

- T1: Legacy source map and repoint in the lock (main and channels)
  touch: splice/src/sources.rs, splice/src/source.rs, splice/src/migrate.rs, splice/tests/legacy_repoint.rs
  size: 250
- T2: `bosun/legacy-tags.txt` (every bosun tag to its monorepo commit; 0.21.0 = f8941aa1) and a test that each listed commit exists and carries that version
  touch: bosun/legacy-tags.txt, bosun/tests/legacy_tags.rs
  size: 120

### Done when

- UC-S1-1: a lock on `gh:keelstackhq/hull//template-rust` 0.13.0 (commit 3f4a65f) updates to the latest release in one `splice update --all`, with no hand edit.
- UC-S1-2: an unknown legacy commit fails with exit 1 and names the legacy-tags file, writing nothing.

## S2: Post commands run where their manifest is

area: splice
depends: seed

### Tasks

- T1: A channel post command runs in the channel's `only` root (its manifest dir); no manifest there: skipped with a message, never a failed update
  touch: splice/src/post.rs, splice/tests/post_cwd.rs
  size: 120

### Done when

- UC-S2-1: a channel with `only = ["apps/site/**"]` and no root package.json updates with exit 0.

## S3: Set a variable and re-render

area: splice
depends: seed

### Tasks

- T1: `splice set [--channel NAME] K=V...`: validates against the template's var pattern, re-renders the owned files the var touches, updates the lock hashes; drifted owned files are refused like `update`
  touch: splice/src/cli.rs, splice/src/vars.rs, splice/src/ops/set.rs, splice/tests/set_var.rs
  size: 300

### Done when

- UC-S3-1: `splice set --channel bosun bosun_app=1` rewrites risk, verdict, main-open, main-health and signer.yml and nothing else; `splice check` is clean after.

## S4: Seed text that tracks the version

area: hull
depends: S3

### Tasks

- T1: The AGENTS.md seed line names no template version (the lock `.splice/lock.toml` is the only source; the always-loaded byte ceiling leaves no room for a pointer); pins.toml is already fully owned and stays current by `splice update`
  touch: hull/template-rust/pieces/base/files/AGENTS.md.jinja, hull/template/pieces/base/files/AGENTS.md.jinja, hull/tests/seed_sections.test.mjs
  size: 150

### Done when

- UC-S4-1: AGENTS.md names no template version; the lock is the only source. pins.toml (owned) carries the new splice and bosun pins after an update.

## K1: Preflight first, and dry-run without installs

area: keel
depends: seed

`keel preflight` runs first in every keel command and as its own command. It reads, never writes:
plan and visibility (rulesets, auto-merge, environment secrets available), token scopes, admin
rights, tools and pins, legacy sources, drift, cloud-session limits (paths the proxy refuses).
A blocking limit stops before any change and lists the choices (for #9: upgrade plan, make
public, or `[ci] provider = "none"`).

### Tasks

- T1: `keel preflight [--json]` with the checks above; each finding has `severity` (block, warn), `fix` and `device`
  touch: keel/src/steps/preflight.rs, keel/src/tools/gh.rs, keel/src/cli.rs, keel/tests/preflight.rs
  size: 350
- T2: `--dry-run` fetches the tools into a temp cache (checksums as today) instead of failing; `--branch NAME` commits and pushes to that branch (cloud sessions)
  touch: keel/src/migrate.rs, keel/src/tools/npm.rs, keel/tests/dry_run_tools.rs
  size: 200

### Done when

- UC-K1-1: on a private repo of a Free org, every keel command exits 5 before any write with the three choices.
- UC-K1-2: `keel migrate --dry-run` on a machine without splice, bosun and hoist lists the full plan.

## K2: keel migrate uses the repoint

area: keel
depends: S1, K1

### Tasks

- T1: The splice step runs the repoint first and reports it as its own step (`repoint`), then the updates
  touch: keel/src/steps/splice.rs, keel/src/tools/splice.rs, keel/tests/migrate_legacy.rs
  size: 150

### Done when

- UC-K2-1: `keel migrate` on a 0.13.0 legacy repo ends `done` at the target, not "up to date".

## K3: Drift settles itself

area: keel
depends: K2

### Tasks

- T1: `--drift seed` (default): an owned file with local edits gets its local lines moved to the seed file the template names for it (`<dir>/CLAUDE.md`) and is then restored; an owned channel file that was deleted or edited is added to the channel's `exclude`; `--drift take` and `--drift keep` force one side; every move is listed in the JSON and the PR body
  touch: keel/src/steps/drift.rs, splice/src/ops/exclude.rs, splice/tests/exclude_add.rs, keel/tests/drift_seed.rs
  size: 300

### Done when

- UC-K3-1: the anu base (4 drifted paths) migrates with no checklist entry `splice-drift`, and no local line is lost (test: every removed line appears in a seed file).

## K4: Session branch reuse without force

area: keel
depends: K1

### Tasks

- T1: When `--branch` names a branch whose PR merged and the remote branch is gone or merged, keel starts it fresh from the base and pushes without force
  touch: keel/src/tools/git.rs, keel/tests/branch_reuse.rs
  size: 100

### Done when

- UC-K4-1: a second `keel up --branch B` after B's PR merged pushes with a plain push.

## B1: Mode presets write the whole flow

area: bosun
depends: seed

### Tasks

- T1: `bosun migrate --to autonomous` writes the D-028 set: `[approvals] policy = "quorum"`, `owner_delegation = "all"`, `[review] verdicts_needed = 1`, `block_on = "blocker"`, `[main_health] block = false`, and `[cloud] environment_id` from the calling session (or `--cloud-env`); `--flow gatekeeper` keeps today's behaviour
  touch: bosun/src/cli/migrate.rs, bosun/src/config.rs, bosun/tests/migrate_preset.rs
  size: 200

### Done when

- UC-B1-1: anu's hand edits to `.bosun/bosun.toml` (PR #117) equal the output of `bosun migrate --to autonomous --direct`.

## B2: One owner command for App and signer

area: bosun
depends: B1

`bosun owner setup` (run by the owner on a computer) replaces the 7-step transition and the
hand-written script: it creates the gate App with the GitHub App manifest flow (a local
callback receives id, client id, slug and key: no copy and paste), opens the install page for
this repo only, creates `bosun-app` and `bosun-signer` (main only, no reviewers), sets the
variable and both secrets, generates and enrolls the signer key, deletes every private key,
and pushes `bosun/gate-app` with approvers.toml and `.bosun/gate-app.toml` (id, slug).

### Tasks

- T1: Manifest flow with a loopback callback (127.0.0.1, one-time state, 10 min timeout) and a fallback that asks for the 4 values
  touch: bosun/src/cli/owner/app_manifest.rs, bosun/tests/owner_manifest.rs
  size: 300
- T2: Environments, variable, secrets (libsodium sealed box via the API), keygen, push; idempotent: a rerun skips what exists and never recreates the App
  touch: bosun/src/cli/owner/setup.rs, bosun/src/github/environments.rs, bosun/tests/owner_setup.rs
  size: 350

### Done when

- UC-B2-1: on a fake GitHub, one run creates all of it, and a second run changes nothing.
- UC-B2-2: no private key exists on disk or in output after the run (test scans the temp HOME and stdout).

## B3: The transition advances itself

area: bosun
depends: B2, S3, H1

### Tasks

- T1: `bosun gate-app advance` (agent): reads `.bosun/gate-app.toml` and the repo state and does the next step only: PR A (`splice set bosun_app=1`, `[checks] mode = "dual"`, pinned hoist.toml); after the App's runs are seen on a probe PR, `next` names the owner's `hoist apply`; after the ruleset is pinned, PR B (`mode = "checks"`, `signer = true`), signed by the signer itself under `owner_delegation = "all"`
  touch: bosun/src/cli/gate_app.rs, bosun/tests/gate_app_advance.rs
  size: 350
- T2: `bosun owner setup` ends by waiting for PR A to merge (poll, 30 min cap) and then runs `hoist apply --area github`, so the owner's one run covers H5 too
  touch: bosun/src/cli/owner/setup.rs, bosun/tests/owner_apply.rs
  size: 120

### Done when

- UC-B3-1: from `.bosun/gate-app.toml` present to signer active with one owner approval (PR A) and no other owner touch.

## BT: Setup script installs the pinned tools

area: bosun
depends: seed

### Tasks

- T1: `docs/setup-script.sh` installs cargo-deny and cargo-nextest at the versions in pins.toml (checksummed release binaries, no `cargo install`) when the repo has a rust stack
  touch: bosun/channel/pieces/core/files/docs/setup-script.sh.jinja, bosun/tests/setup_script_tools.rs
  size: 120

### Done when

- UC-BT-1: a fresh cloud session of a rust repo runs every CI step without installing anything.

## BA: Trusted authors for intake

area: bosun
depends: seed

### Tasks

- T1: `[intake] trusted_authors = "members"` (default `"any"`; also a login list): `bosun claim`, `wave plan`, spawn and issue pick-up skip issues, and `bosun` commands ignore comments, whose author is not the owner, a `governance/owners.toml` owner or an org member/collaborator (`author_association` OWNER, MEMBER, COLLABORATOR); skipped items are listed with the reason
  touch: bosun/src/config.rs, bosun/src/cli/claim.rs, bosun/src/core/intake_trust.rs, bosun/tests/intake_trust.rs
  size: 250

### Done when

- UC-BA-1: with `trusted_authors = "members"`, an issue opened by an outside user is never claimed or spawned, and `wave plan` lists it as skipped.

## H1: hoist pins by id

area: hoist
depends: seed

### Tasks

- T1: `hoist check` accepts `app = "<slug>"` with `app_id` from `.bosun/gate-app.toml` (or `integration_id`) when the slug cannot be resolved, and never fails on a read the cloud proxy refuses (marks it `unknown`)
  touch: hoist/src/verify.rs, hoist/src/github.rs, hoist/tests/pin_by_id.rs
  size: 150

### Done when

- UC-H1-1: `hoist check` in a cloud session on anu exits 0 with the pinned checks.

## K5: keel up

area: keel
depends: K3, K4, B1, B3, BT, S4

`keel up [--to X.Y.Z] [--mode autonomous] [--branch B] [--json]` (new name for the whole flow;
`keel migrate` stays as today's subset): preflight, repoint, migrate, settle drift, mode
preset, gate-app PR A, then ONE owner line (`npx -y @keelstackhq/keel@X owner`, which runs
`bosun owner setup` and pastes nothing). `keel up` again after the owner's run finishes PR B.

### Tasks

- T1: The `up` pipeline over the existing steps; the PR body lists each step and the one owner line
  touch: keel/src/cli.rs, keel/src/steps/up.rs, keel/tests/up_e2e.rs
  size: 300
- T2: docs: `docs/migrate.md` becomes "keel up"; the old manual steps move to an appendix
  touch: docs/migrate.md, keel/README.md
  size: 80

### Done when

- UC-K5-1: an end-to-end test on a fake GitHub takes a copy of anu's 0.13.0 base to 0.25.0+ autonomous with the signer active: one agent command, one owner command, one owner approval.
- UC-K5-2: on a private Free-org repo it stops at preflight before any write.

## Notes

Cloud environment setup script: claude.ai environments have no owner API, so `keel owner`
prints the snippet path and the one paste stays; it is needed only when the bootstrap changed.
