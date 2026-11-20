# keel — design

Status: draft 1 (2026-11-20). Source of truth for keel's behaviour. Update it in the same commit
when behaviour changes.

## 1. Why

The keelstackhq tools (splice, bosun, hoist, berth, hull templates) work well one by one. Nothing
sets them up together, so the owner becomes the glue. A recent bootstrap of a client repo took 4
owner round-trips, 10+ commands from 3 CLIs and a hand-written `gh` script.

keel configures an existing repo. The org provisioner app (`keelstackhq/provision`, D-014) creates
repos and org settings; keel never creates a repo or touches org settings.

keel is a thin orchestrator. `keel up` reads one manifest, runs each tool as a subprocess, reads
its JSON, and stops at the first step only a human can do. keel never reimplements a tool.

## 2. Principles

- **Idempotent.** Each step checks first and skips what is done. A re-run resumes.
- **One answer.** `--json` prints exactly one object: done, the single human step left (with the
  exact link or command), a failed step, or an error.
- **Subprocesses only.** keel calls `gh`, `git`, `npm`, `node`, `cargo`, `splice`, `bosun` and
  `hoist`. It has no HTTP client of its own (so no TLS stack: the "rustls only" family rule holds
  trivially). GitHub reads and writes go through `gh api` / `gh secret` / `gh pr`.
- **Adapters.** Each tool has one small module (`src/tools/<tool>.rs`) that builds argv and
  parses output. When a tool lacks a feature keel needs (old version), the adapter degrades to a
  clear human step or a warning, never a crash.
- **Secrets.** The PAT is read from stdin (hidden prompt on a TTY) and written to `gh secret set`
  on its stdin. It never appears in argv, files, logs, stdout, stderr or JSON, and it is zeroized
  after use.
- `#![forbid(unsafe_code)]`. Tests never reach a real API or a real tool: they use a fake runner
  (unit) or stub scripts on `PATH` (integration).

## 3. CLI

```
keel [-C DIR] [--json] [-v] up     [--repo OWNER/NAME] [--template rust|ts|SOURCE] [--pieces ID[:NAME],...]
                                   [--plan FILE] [--wait SECS] [--token-stdin] [--confirm STEP]...
keel [-C DIR] [--json] [-v] plan   (same flags as up; writes nothing)
keel [-C DIR] [--json] [-v] doctor
```

- `-C DIR`: the repo checkout (default `.`).
- `--repo`, `--template`, `--pieces`, `--plan`: written to `keel.toml` on the first run. Later runs
  read `keel.toml`; a flag that disagrees with it is a usage error (exit 2, names the key).
- `--repo` must be `OWNER/NAME` (`^[A-Za-z0-9-]+/[A-Za-z0-9._-]+$`, no `..`); anything else is a
  usage error (exit 2) before `keel.toml` is written.
- `--template-ref REF`: the template's git ref (tag, branch or sha), passed to `splice new --ref`.
  Default for `rust`/`ts`: the `hull` pin (§6), written to `keel.toml` as `template_ref` on the
  first write (a repo `pins.toml` `hull` key overrides the embedded pin). A custom source has no
  default: without `template_ref` (flag or `keel.toml`) the render step fails and asks for one.
- `--pieces` takes a comma list; each entry is a splice piece `ID` or `ID:NAME` (the instance name,
  e.g. `rust-cli:my-cli`).
- `--template` (required, flag or `keel.toml`; missing is a usage error, exit 2, with a fix): `rust` = `gh:keelstackhq/hull//template-rust`, `ts` = `gh:keelstackhq/hull//template`;
  anything else is passed to `splice new` as the source.
- `--wait SECS` (default 300): how long a GitHub-App gate polls before it returns exit 5. `0` = check
  once. Poll interval: `KEEL_POLL_SECS` (default 10).
- `--token-stdin`: read the PAT for step 6 from stdin (one line, trailing newline trimmed).
- `--confirm agent-env`: record that the cloud environment setup script is attached (step 7);
  stored in `keel.toml` `[confirmed]`.
- Example: `npx @keelstackhq/keel up --repo owner/name --template rust --pieces rust-cli:my-cli`.

### 3.1 Exit codes

0 done · 1 a step failed or (for `plan`/`doctor`) work or drift remains · 2 usage error (clap
errors too) · 3 manifest, environment or API error (message says how to fix) · 5 a human step is
pending (D-020; 5 matches bosun's needs-human). Help and version exit 0. With `--json`,
`--help` and `--version` print one object (`{"version":1,"command":"help","ok":true,
"exit_code":0,"help":"<text>"}`; `command:"version"` carries `text`) and exit 0.

### 3.2 JSON

```json
{"version":1,"command":"up","ok":false,"exit_code":2,"status":"human",
 "steps":[{"id":"preflight","action":"ok","detail":"node 24.1.0, gh as octo, git 2.47"},
          {"id":"apps","action":"pending","detail":"Renovate app not installed on owner/name"}],
 "human_step":{"id":"apps","title":"Install the Renovate GitHub App on owner/name",
               "url":"https://github.com/apps/renovate/installations/new","command":"keel up"}}
```

- `status`: `done` | `human` | `failed` | `planned` | `error`.
- `steps[].action`: `ok` (already done, skipped) · `done` (keel did it now) · `would-run` (`plan`)
  · `pending` (human gate) · `failed` · `not-reached`.
- `human_step`: null, or `{id, title, url, command}` (`url`/`command` may be `""`). At most one.
- On exit 2 or 3: same envelope, `status:"error"`, `steps:[]`, `human_step:null`, plus
  `error:{message, fix}`.
- Without `--json`: one line per step, then the human step or `done`. Logs go to stderr.

## 4. Manifest: `keel.toml`

```toml
repo = "owner/name"
template = "rust"                 # or a splice source
template_ref = "v0.13.0"          # git ref of the template; set from the hull pin for rust/ts
pieces = ["rust-cli:my-cli"]
plan = "docs/plan.md"             # optional: bosun plan import
branch = "keel/bootstrap"         # optional; default below

[token]                           # optional; defaults shown
secret = "SPLICE_BOT_TOKEN"
environment = "splice-bot"

[confirmed]                       # written by --confirm
agent_env = true
```

Unknown keys are an error (exit 3, names the key). keel writes the file atomically (a temp file in the same directory, then a rename) on the first run (before
any other write) and commits it with the render.

## 5. Steps of `keel up`

Each step is `check` (read-only) then, when needed, `apply`, then `check` again. `up` stops at the
first step that is pending (human) or failed; the remaining steps are `not-reached`. `plan` runs
every `check` only and marks work `would-run`; it stops after the first human gate.

| # | id | check (read-only) | apply |
|---|----|-------------------|-------|
| 1 | `preflight` | `node --version` ≥ 24, `gh auth status`, `gh api user` (login), `git --version`; repo exists (`gh repo view`; if not, a human step: request the repo from the provisioner, never `gh repo create`); `-C` is a git checkout whose `origin` is the repo; each tool's `--version` equals its pin | install each tool not at its pin (§6) |
| 2 | `apps` | Claude and Renovate apps installed on the repo (`gh api user/installations`, then the installation's repositories when `repository_selection` is `selected`) | none: human gate; prints the install link and polls up to `--wait` |
| 3 | `render` | `.splice/lock.toml` exists, no `.keel/render-files` is left (an uncommitted render), and `Cargo.lock` exists when `Cargo.toml` does | branch (§5.1, switched before any commit); commit `keel.toml` alone (splice refuses a dirty tree); `splice --json pieces <source> --ref <template_ref>` validates piece names (unknown id, or a `multiple` piece with no `default_name` given without `:NAME`, fails before any write; an old splice without the command only warns; malformed or missing JSON, exit 1 or 3 fail); for a hull source `git ls-remote https://github.com/keelstackhq/hull refs/tags/<ref> refs/tags/<ref>^{}` must give (peeled, else plain) the sha pinned in `[refs.hull]` (§6), else failed ("hull tag <ref> moved: expected …, got …"; an unpinned ref or a failing ls-remote also fail); `splice --json --yes new <source> . --ref <template_ref> --force --piece …` (rust: `--data no_placeholder=true`); `cargo generate-lockfile` when needed; ensure `.keel/` is in `.gitignore`; commit only the render's files (`changes[].path` of the `splice new --json` output whose action is not `skip`, plus `.splice/lock.toml`) and what keel itself wrote (`.gitignore`, `Cargo.lock`); a lockfile-only re-run commits just `Cargo.lock`. Right after `splice new` the file list is saved to `.keel/render-files`; if the lock exists but the render was never committed (crash in between), the next run commits that list (re-filtered) plus `.splice/lock.toml` via explicit paths and removes the list; with no list it commits only the lock and fails telling the user to commit the files `git status` shows |
| 4 | `governance` | `.bosun/version` exists; with `plan`, `bosun plan import FILE --dry-run` has no `data.epics[]`/`data.tasks[]` item with `action: "would-create"` | `bosun init --owner <gh login> --branch <work branch> --no-pr` (+ `--stack rust` for the rust template); labels fallback (§10); then `bosun plan import FILE` |
| 5 | `settings` | `hoist --json check` exits 0 | `hoist --json apply --yes`; hoist human steps become keel's human step |
| 6 | `token` | `gh api repos/R/environments/ENV/secrets/NAME` succeeds | with `--token-stdin`: read stdin, `gh secret set NAME --env ENV --repo R` (value on stdin). Without: human gate with the PAT link (§7) |
| 7 | `agent-env` | `keel.toml` `[confirmed] agent_env = true` | write `.keel/cloud-setup.sh` (§8); human gate: attach it, then `keel up --confirm agent-env` |
| 8 | `doctor` | `bosun doctor --remote` (`--all` tried first, §10) has no `missing` item (any missing item is a failed step, never a push or PR); a PR from the work branch exists. Offline doctor (exit 5, `data.offline`) is a human step: `export GH_TOKEN=$(gh auth token)`, then `keel up` | commit keel's own files only (`keel.toml`, `.gitignore`, `Cargo.lock`; nothing keel-owned changed means no commit), `git push -u origin <branch>` (never force), `gh pr create` |
| 9 | `merge` | the PR is merged (`gh pr view --json state,headRefOid`) | none: human gate "approve head SHA `<sha>` and merge PR #N" |

### 5.1 Branch

Steps 3–8 work on one branch, resolved by `steps::work_branch`: the current branch when it is not
the default branch and HEAD is not detached, else `keel.toml` `branch` (default `keel/bootstrap`).
Render, governance and doctor call `git::ensure_branch` before they commit or push: it checks the
branch out when it exists locally, else fetches `origin/<branch>` (best effort) and tracks it when
the remote has it (a re-run from a fresh clone resumes), else creates it. It fails, so nothing is
committed or pushed, when the target is the default branch. Doctor and merge only resolve the
name (no switching): after the merge, on the default branch, they look up the PR of the manifest
branch. Commits never include `.keel/` (`git add -A -- . ':(exclude).keel'`). keel never merges,
tags, releases, publishes or force-pushes.

### 5.2 Tool exit codes

- splice: 0 ok, 1 conflict/drift → step failed, 2 usage (old splice: feature missing), 3 error. For `pieces`, only exit 2 without a `command:"pieces"` JSON object (or a missing binary) means the command is absent; anything else that is not a valid piece list is a failure.
- bosun: 0 ok, 1 error, 2 usage (feature missing), 3 conflict, 4 halted, 5 needs-human → human
  step from `error.fix` / `next`, 6 limit. JSON `{version, command, ok, exit_code, data, error, next}`.
- hoist: 0 ok, 1 drift/failed, 2 human steps (`human_steps[0]` becomes keel's), 3 error.
  JSON `{version, command, ok, exit_code, items, human_steps}`.
A non-JSON or missing output is a failed step whose detail names the command (never its stdin).

### 5.3 `keel doctor`

Runs step 1's checks (no installs) and step 8's checks alone. Exit 0 healthy, 1 drift, 3 error, 5 a human step is pending
(for example `gh` not logged in, or offline doctor): the JSON `status` is `human` with `human_step`
set (drift without a human step is `planned`, exit 1).

## 6. Pins and installs

Versions follow hull's `pins.toml` (hull#132): a generated repo has `pins.toml` at its root with
one `[pins]` table of exact string versions (`rust`, `nextest`, `cargo_deny`, `splice`, `bosun`,
`hoist`; keys `[a-z_][a-z0-9_]*`). keel embeds a default (`src/pins.toml`, compiled in) with the
same `[pins]` table for `splice`, `bosun`, `hoist`, `berth` and `hull` (the template tag, `v0.13.0` =
commit `3f4a65fda568cdd20c76168ed156d25a28ba5630`, which has `template-rust/` and `template/`), plus `[sha512.<tool>]` tables
(`version`, `launcher`, `linux_x64`, `linux_arm64`, …: npm `dist.integrity`, base64).
`[refs.hull] tag, sha` records the commit that tag must peel to; keel checks it with `git ls-remote` before rendering a hull source (a tag with no pinned sha, e.g. a repo `pins.toml` moving `hull`, fails with a fix naming `KEEL_PINS` or updating keel). `KEEL_PINS=FILE` replaces the embedded file. The repo's `pins.toml` `[pins]` versions override the
embedded ones (other tables and keys are ignored). A version with no recorded sha512 (repo pins a
version keel has no sha512s for) is a failed step: update keel or point `KEEL_PINS` at a file
with that version's sha512s. keel never installs an unverified tool.

Install of a tool not at its pin: in a temp dir `npm pack @keelstackhq/<tool>@<v>` and
`npm pack @keelstackhq/<tool>-<os>-<arch>@<v>`; the base64 sha512 of each tarball must equal its
pin (mismatch: step failed, nothing installed); then
`npm install -g --ignore-scripts --omit=optional <launcher.tgz> <platform.tgz>`. A platform with
no sha512 pin (e.g. macOS, Windows) is a human step (id `preflight`) naming the manual
`npm install -g @keelstackhq/<tool>@<version>` and saying keel could not verify it.

## 7. The PAT gate

Link (fine-grained PAT, prefilled):
`https://github.com/settings/personal-access-tokens/new?name=keel%20OWNER%2FNAME&description=…&target_name=OWNER&expires_in=90&contents=write&pull_requests=write&workflows=write`
(Contents, Pull requests, Workflows: read and write; repository access: only the repo).
Command: `keel up --token-stdin` (paste, then Enter). keel stores it with
`gh secret set NAME --env ENV --repo R` reading the value on stdin. D-010 covers repo secrets
through hoist; this one environment secret is the exception, set by `gh secret set` (stdin) because
the PAT must never reach hoist's argv, files or logs.

The owner decided to keep this PAT step as a D-014 exception and the one human gate for
credentials, until the splice bot uses the provisioner app's token; then this step goes away.

## 8. Agent environment

`.keel/cloud-setup.sh` = a proxy-CA prelude + the `data.script` of `bosun setup-snippet`. The
prelude starts with `KEEL_REPO_DIR='<name>'` (the repo name of `owner/name`); the setup script starts in the parent of the checkouts, so the prelude `cd`s into that directory (cwd if absent), runs only `./scripts/proxy-ca.sh install` there (hull#132's hook; no other checkout is searched) and restores the cwd; it never exits non-zero. Only when the hook is absent (not when it failed) and
`$PROXY_CA_BUNDLE` or `/root/.ccr/ca-bundle.crt` exists (nothing else is trusted, not
`SSL_CERT_FILE`), it writes
`/etc/profile.d/keel-proxy-ca.sh` exporting `NODE_EXTRA_CA_CERTS`, `SSL_CERT_FILE`,
`REQUESTS_CA_BUNDLE`, `CARGO_HTTP_CAINFO`, `GIT_SSL_CAINFO` and sets `npm config set cafile`, so
tools behind the agent proxy verify TLS. The human step says where to paste it (claude.ai → Code →
environment settings → Setup script) and to run `keel up --confirm agent-env` afterwards. The file
is not committed (`.keel/` is in `.gitignore`).

## 9. npm launcher

`@keelstackhq/keel` with `optionalDependencies` on `@keelstackhq/keel-{linux-x64,linux-arm64,
darwin-x64,darwin-arm64,win32-x64}`, same launcher as hoist (`npm/keel/bin/keel.js`). Publishing
is human-only (Blocked in `docs/PROGRESS.md`). Launcher environment errors (unsupported platform, platform package missing, binary cannot
start) exit 3, like keel's own environment errors. With `--json` in the arguments (before `--`) the
launcher prints exactly one JSON object on stdout for its own errors: `{version:1, command, ok:false,
exit_code:3, status:"error", steps:[], human_step:null, error:{message, fix}}`.

## 10. Open questions (for the orchestrator)

Answered: pins (§6, hull#132 `pins.toml`), PAT secret `SPLICE_BOT_TOKEN` in `splice-bot`, proxy-CA
hook (§8, `scripts/proxy-ca.sh install`), labels: `bosun init` creates labels and owners from the
gh user; keel prefers the labels bosun reports (`data.labels_created`, or `data.labels`) and creates the
others from a hard-coded fallback list with `gh label create … --force` (with `--no-pr` bosun
reports none). bosun 0.21.0 has no `labels` command; a unit test pins the list to
`registry::LABEL`, `bosun:halt` and `State::ALL`, and an ignored test (`KEEL_BOSUN_SRC=<bosun
checkout> cargo test -- --ignored bosun_source`) checks it against bosun's source. The
rust template gets `--data no_placeholder=true` (skips `crates/common`). Answered from the sources:

- `bosun plan import --dry-run` JSON (was Q3): `data.epics[]` and `data.tasks[]`, each with an
  `action`; keel counts `would-create`.
- `bosun doctor` has `--full`, not `--all`. keel still tries `--all` first (the upstream bosun
  change adds `doctor --all`) and retries without it only on exit 2 whose output mentions `--all`.
- `bosun doctor` runs offline (exit 5, `data.offline: true`, missing 0) without `GH_TOKEN` or
  `GITHUB_TOKEN`: keel shows a human step.
- The pinned splice 0.6.0 has no `pieces` subcommand (its commands: new, adopt, add, update,
  check, diff, explain, channel). Piece names are not validated up front at this pin: keel warns
  and relies on `splice new` rejecting a bad name. Validation starts working with a splice that
  has `pieces`.

Pending upstream:

- bosun 0.21.0 `init --branch B` exits 3 when `B` already exists, and keel has created the work
  branch by then. The upstream bosun change makes init reuse the current branch. keel keeps
  `--branch <work branch>` and maps that exit 3 to a human step: "bosun <version> cannot init on
  an existing branch; upgrade bosun to a release where init reuses the current branch", command
  `npm install -g @keelstackhq/bosun@latest`. keel's pin moves once that release exists.

Still assumed, behind adapters:

1. `splice pieces --json` shape (splice#54, not merged yet): `{ok, command:"pieces", changes, messages, pieces:[{id, description, base, multiple, default_name, requires, vars}], template}`; exit 0 ok, 2 bad source or usage, 3 template/env error. keel tells an old splice (exit 2 without a `pieces` object) from a bad source (exit 2 with one)
   (not available at pin 0.6.0, see above).
