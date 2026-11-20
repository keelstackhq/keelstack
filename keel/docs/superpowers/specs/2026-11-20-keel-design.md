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
- **Subprocesses only.** keel calls `gh`, `git`, `npm`, `node`, `cargo`, `splice`, `bosun`,
  `hoist`, `fly` and `curl`. It has no HTTP client of its own (so no TLS stack: the "rustls only" family rule holds
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
- Tool timeout: every `hoist` call and `scripts/fly-setup.sh` and `scripts/smoke.sh` are killed after `KEEL_TOOL_TIMEOUT_SECS` (default 600; a value that is not a positive integer is ignored). A timed-out call fails with "timed out after N s" (exit 124 internally).
- `--token-stdin`: read the PAT for step 6 from stdin (one line, trailing newline trimmed).
- `--confirm agent-env|dns|backup`: record that the cloud environment setup script is attached
  (step 7), that the DNS records were added by hand (step 9) or that the backup bucket was created
  by hand (step 10); stored in `keel.toml` `[confirmed]`. A confirmed step is done. `agent_env` is a boolean; `dns` and `backup` store what was confirmed, `dns = "<host>"` and `backup = "<fly app>"` (`--confirm dns|backup` needs `[deploy.fly]`, `backup` also `[backup]`). The confirmation is void (the step runs as if unconfirmed) when the value differs from the manifest's current `[deploy.fly] host` or `app`; an old `dns = true` or `backup = true` is void too, and is dropped the next time keel writes `keel.toml`.
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
- `verify`: present only when the manifest has `[deploy.fly]`:
  `{"url":"https://HOST","healthz":"ok"|"failed"|"not-run","http_status":200|null,"smoke":"passed"|"failed"|"none"|"not-run"}`.
  `not-run` until the `verify` step has reached that check. Text output adds a `verify:` line.
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
dns = "app.example.com"            # DNS records added by hand, for this host
backup = "my-app"                   # backup bucket created by hand, for this app

[deploy.fly]                      # optional; written by hand
app = "my-app"                    # fly app name
region = "fra"
host = "app.example.com"          # public hostname, no scheme
dns = "cloudflare"                # "cloudflare" | "none" (default "none")
zone = "example.com"              # optional Cloudflare zone; HOST or a parent domain of it
                                  # (default: the last two labels of HOST; set it for example.co.uk)
no_mail = false                   # true: hoist also adds SPF and DMARC records for HOST

[backup]                          # optional; needs [deploy.fly]
provider = "r2"                   # "r2" | "s3"
retention_days = 30               # >= 1; default 30
account_id = "..."                # optional Cloudflare account id for r2; written to hoist.toml [cloudflare]; env CLOUDFLARE_ACCOUNT_ID wins (hoist reads it)
```

The pieces may include `sqlite` and `fly` (hull: `rust-service` needs `base`; `sqlite` and `fly` need `rust-service`; each has the var `service`, default `server`); splice validates them. With `[deploy.fly]` and a `fly` or `fly:NAME` piece, keel passes `--data app=<app>` to `splice new`. A bad enum value, an empty
`app`/`region`/`host`, a `host` or `zone` that is not a DNS name (lowercase labels of a-z, 0-9 and `-`, 1-63 characters, no leading or trailing `-`, single dots, at least two labels, at most 253 characters: no scheme, path, port, `@`, `?`, `#`, whitespace or uppercase), an `app` outside `^[a-z0-9-]{1,63}$`, a `[backup] account_id` outside `^[0-9a-f]{32}$` (the env `CLOUDFLARE_ACCOUNT_ID` has the same rule: the backup step fails naming the variable), a `zone` that is neither `host` nor a parent domain of it at a label boundary, `retention_days = 0`, `[backup]` without `[deploy.fly]` or, with `provider = "r2"`, an `app` whose
lowercased `<app>-backup` is not 3-63 characters of `a-z0-9-` (an R2 bucket name) is a manifest error
(exit 3, names the key).

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
| 3 | `render` | `.splice/lock.toml` exists, no `.keel/render-files` is left (an uncommitted render), and `Cargo.lock` exists when `Cargo.toml` does | branch (§5.1, switched before any commit); commit `keel.toml` alone (splice refuses a dirty tree); `splice --json pieces <source> --ref <template_ref>` validates piece names (unknown id, or a `multiple` piece with no `default_name` given without `:NAME`, fails before any write; an old splice without the command only warns; malformed or missing JSON, exit 1 or 3 fail); for a hull source (`splice::is_hull_source`: `gh:keelstackhq/hull`, `https://github.com/keelstackhq/hull` and `git@github.com:keelstackhq/hull`, each with or without `.git`, any `//subdir`; `keel plan` runs this read-only check too and shows a moved tag as `render` `failed`) `git ls-remote https://github.com/keelstackhq/hull refs/tags/<ref> refs/tags/<ref>^{}` must give (peeled, else plain) the sha pinned in `[refs.hull]` (§6), else failed ("hull tag <ref> moved: expected …, got …"; an unpinned ref or a failing ls-remote also fail); `splice --json --yes new <source> . --ref <template_ref> --force --piece …` (rust: `--data no_placeholder=true`); `cargo generate-lockfile` when needed; ensure `.keel/` is in `.gitignore`; commit only the render's files (`changes[].path` of the `splice new --json` output whose action is not `skip`, plus `.splice/lock.toml`) and what keel itself wrote (`.gitignore`, `Cargo.lock`); a lockfile-only re-run commits just `Cargo.lock`. A resume (below) also commits `Cargo.lock` (when `Cargo.toml` exists) and `.gitignore`, but only when keel changed them in this run or they are untracked (`git ls-files --error-unmatch` fails): a tracked file with the user's own uncommitted edits is left alone. Right after `splice new` the file list is saved to `.keel/render-files`; if the lock exists but the render was never committed (crash in between), the next run commits that list (re-filtered) plus `.splice/lock.toml` via explicit paths and removes the list; with no list it commits only the lock and fails telling the user to commit the files `git status` shows |
| 4 | `governance` | `.bosun/version` exists; with `plan` (or, when `plan` is unset and `plans/*.md` exist, bosun's default with no FILE), `bosun plan import [FILE] --dry-run` has no `data.epics[]`/`data.tasks[]` item with `action: "would-create"` | `bosun init --owner <gh login> --branch <work branch> --no-pr` (+ `--stack rust` for the rust template); labels: exactly `data.labels_todo` when bosun reports it, else the fallback (§10); then `bosun plan import [FILE] --fix` (retried without `--fix` on exit 2 mentioning it; `data.prereqs_fixed` goes to the step detail, as do `data.branch_reused` and `data.owner` of init) |
| 5 | `settings` | `hoist --json check --area github --area npm --area crates --area release` exits 0 (everything but `cloudflare`, which steps 9 and 10 own; hoist has no sub-areas. A hoist that rejects `--area`, exit 2 without hoist JSON, gets the plain command) | `hoist --json apply --yes` with the same `--area` flags (same fallback); hoist human steps become keel's human step |
| 6 | `token` | `gh api repos/R/environments/ENV/secrets/NAME` succeeds | with `--token-stdin`: read stdin, `gh secret set NAME --env ENV --repo R` (value on stdin). Without: human gate with the PAT link (§7) |
| 7 | `agent-env` | `keel.toml` `[confirmed] agent_env = true` | write `.keel/cloud-setup.sh` (§8); human gate: attach it, then `keel up --confirm agent-env` |
| 8 | `deploy` | no `[deploy.fly]`: done. Else `scripts/fly-setup.sh` exists (no: failed, "add the `fly` piece"); `fly auth whoami --json` exits 0 (no: human "Log in to Fly.io"); `fly status --app APP --json`, `fly certs show HOST --app APP --json` exit 0; `gh api repos/R/environments/production/secrets/FLY_API_TOKEN` succeeds | `scripts/fly-setup.sh --app APP --region R --host H` (app, volume, secrets, IPs, certificate), then the same call plus `--set-gh-secret production` (the script pipes the deploy token into `gh secret set`; keel never sees it). Never `--deploy`: the first merge deploys. Both calls get an empty stdin (the script takes secrets as `KEY=VALUE` lines there; keel has none). Script exit 2 (old script) is a human step: upgrade the hull fly piece; exit 3 (missing tool or auth) is the human step "Log in to Fly.io and install flyctl/gh"; exit 4 (a fly/gh step failed) is failed |
| 9 | `dns` | no `[deploy.fly]`: done. `dns = "none"` (or no token, to print the records): `fly ips list --app APP --json` gives the `v4`/`v6` addresses (the A target is a dedicated `v4` when present, else the `shared_v4`, Fly's default); records `A HOST v4`, `AAAA HOST v6`. `dns = "none"`: always done, never failed; the detail lists the records to add by hand, or, when the IPs cannot be read (no app yet, no v4), says to run `fly ips list --app APP` and add A/AAAA for HOST. `dns = "cloudflare"` without `CLOUDFLARE_API_TOKEN` (presence only): human "Set CLOUDFLARE_API_TOKEN or add these DNS records" (command mentions `keel up --confirm dns`; `[confirmed] dns = "HOST"` equal to the current host makes the step done, checked first). With it (no `fly ips` call; the token needs Zone Read + DNS Write): `hoist.toml` has `[cloudflare.dns]` and `hoist --json check --area cloudflare` is read for items of kind `dns_record`: exit 0 or 1 is done only when at least one `dns_record` item exists and every one has `action` "ok" (`action` is create|update|ok|drift|pending|unknown; any other value, or no `dns_record` item at all, is drift); exit 2 (check or apply) is never done: `human_steps[0]` ({id,title,url}) becomes the human step (id `dns`, command `keel up`, whatever hoist's step id) | append `[cloudflare.dns]` (`zone` = `[deploy.fly] zone`, default the last two labels of HOST; `host`; `targets = "fly"`; `fly_app = "APP"`; `proxied = false`; `no_mail = true` when `[deploy.fly] no_mail = true`) to `hoist.toml` when absent; `hoist --json apply --yes --area cloudflare --secrets-to fly:APP` (the sink is always passed: the area also mints the R2 key when `[cloudflare.r2]` exists, and minting without a sink exits 3). keel never passes `--prune`: a `dns_record` still in drift after apply (a stray A/AAAA record) is a human step "Remove the old A/AAAA records for HOST in Cloudflare, or rerun with `hoist apply --prune`". A hoist that rejects `--area` is a human step: `npm install -g @keelstackhq/hoist@latest` |
| 10 | `backup` | no `[backup]`: done. `s3`: done with a warning (not automated: create the bucket and set the secrets on the fly app by hand). `[confirmed] backup = "APP"` equal to the current app (`keel up --confirm backup`, for a bucket made by hand): done. `r2`: needs `CLOUDFLARE_API_TOKEN` (else human, command mentions `keel up --confirm backup`); an account id: env `CLOUDFLARE_ACCOUNT_ID` (hoist reads it itself), else `[backup] account_id`, else `[cloudflare] account_id` already in `hoist.toml` (else human "Set [backup] account_id (Cloudflare account id) in keel.toml", command `keel up`); `hoist.toml` has `[cloudflare.r2]`; `hoist --json check --area cloudflare` read as for dns, for items of kind `r2_bucket`, `r2_lifecycle`, `r2_token`; exit 2 is `human_steps[0]` as human step id `backup` | when env `CLOUDFLARE_ACCOUNT_ID` is unset and `[backup] account_id` is set, ensure top-level `[cloudflare] account_id = "ID"` in `hoist.toml` (no `[cloudflare]` header: append the table; header without the key: insert the line right after the header; key present: leave it; text edits only; every value keel writes into `hoist.toml` is a TOML string made by the `toml` crate, never spliced in by hand; `account_id` is not a key of `[cloudflare.r2]`); append `[cloudflare.r2]` (`bucket = "<lowercased APP>-backup"`, `retention_days = N`); `hoist --json apply --yes --area cloudflare --secrets-to fly:APP` (exit 3 is a failed step; exit 1 with `human_steps[0]` in the JSON, as when key delivery failed and the revoke failed too (`cloudflare:revoke`), is that human step instead of a failure). hoist mints the key (fixed scope) and delivers `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `R2_BUCKET`, `R2_ENDPOINT` to the fly app; keel does no name mapping and the key never passes through keel. hull's service will read those names |
| 11 | `doctor` | `bosun doctor --remote` (`--all` tried first, §10; `data.tools` entries with `ok: false` are summarised as a missing item) has no `missing` item (any missing item is a failed step, never a push or PR); a PR from the work branch exists. Offline doctor (exit 5, `data.offline`) is a human step: `export GH_TOKEN=$(gh auth token)`, then `keel up` | commit keel's own files only (`keel.toml`, `hoist.toml`, `.gitignore`, `Cargo.lock`; nothing keel-owned changed means no commit); `hoist.toml` goes in only when it is untracked, or when `git diff --unified=0 HEAD -- hoist.toml` removes no line and adds only blank lines and lines keel's section generators write for the current manifest (else it stays out and the detail says so), `git push -u origin <branch>` (never force), `bosun --json pr --single --ready` (exit 2, an old bosun: `gh pr create`) |
| 12 | `merge` | the PR is merged (`gh pr view --json state,headRefOid`) | none: human gate "approve head SHA `<sha>` and merge PR #N" |
| 13 | `verify` | no `[deploy.fly]`: done. After the merge: the `deploy.yml` run of the merged commit is `completed`: keel reads the merge commit (`gh pr view WORKBRANCH --repo R --json mergeCommit`, `mergeCommit.oid`; unreadable is failed), lists `gh run list --repo R --workflow deploy.yml --branch B --limit 20 --json status,conclusion,url,headSha` and takes the run whose `headSha` equals it (runs of other commits are ignored), polled every `KEEL_POLL_SECS` up to `--wait`; a conclusion other than `success` is failed (with the run URL); no run of that commit yet (after `--wait`) is a human step "wait for the first deploy". Then `curl -fsS -o /dev/null -w %{http_code} --max-time 10 https://HOST/healthz` gives 200 (polled the same way; else failed). Then `scripts/smoke.sh` with `SMOKE_URL=https://HOST` (the base URL; `SMOKE_TIMEOUT_SECS` is left at the script default, 60) when it exists (`keel plan` never runs it: `smoke` is `not-run`). The outcome is the report's `verify` key | none: read-only |

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

Answered (hull `fly` piece, relayed): `scripts/fly-setup.sh --app A --region R --host H [--dry-run]
[--deploy] [--set-gh-secret ENV]`, secrets on stdin as `KEY=VALUE`; exit 0 ok, 2 usage, 3 missing tool
or auth, 4 a fly/gh step failed. `scripts/smoke.sh` reads `SMOKE_URL` and `SMOKE_TIMEOUT_SECS`
(default 60). The deploy workflow is `.github/workflows/deploy.yml` and uses `FLY_API_TOKEN` in the
`production` environment.

Answered (upstream bosun, flags assumed until relayed; each argv is built in one adapter fn in
`src/tools/bosun.rs`):

- `bosun init --branch B` reuses the current branch (`data.branch_reused`), reports `data.owner`,
  `data.pr` (null with `--no-pr`) and `data.labels_todo` (labels it could not create; keel creates
  exactly those). `doctor --remote --all` adds `data.tools` (`[{name, version, ok}]`).
  `plan import --fix` reports `data.prereqs_fixed`; `plan import` with no FILE reads `plans/*.md`.
  `bosun pr --single --ready` opens the PR (`data.pr`). An old bosun (exit 2) falls back as above.
- For a bosun without branch reuse, keel keeps `--branch <work branch>` and maps the exit 3
  (branch exists) to a human step: "bosun <version> cannot init on
  an existing branch; upgrade bosun to a release where init reuses the current branch", command
  `npm install -g @keelstackhq/bosun@latest`. keel's pin moves once that release exists.

Still assumed, behind adapters:

1. `splice pieces --json` shape (splice#54, not merged yet): `{ok, command:"pieces", changes, messages, pieces:[{id, description, base, multiple, default_name, requires, vars}], template}`; exit 0 ok, 2 bad source or usage, 3 template/env error. keel tells an old splice (exit 2 without a `pieces` object) from a bad source (exit 2 with one)
   (not available at pin 0.6.0, see above).

Answered (hoist, relayed; each argv is built in one adapter fn in `src/tools/hoist.rs`): hoist has no
`--only`/`--skip`. `--area <github|cloudflare|npm|crates|release>` (repeatable) is on `plan`,
`check`, `apply` and `import`; there are no sub-areas, so `--area cloudflare` covers pages, workers,
the deploy token, dns and r2. `[cloudflare.dns]`: `zone`, `host`, `targets = ["ip", ...] | "fly"`,
`fly_app` (only with "fly"), `proxied = false` (true exits 3), `no_mail = true` adds SPF and DMARC.
`[cloudflare.r2]`: `bucket`, `retention_days`, optional `prefix`; unknown keys exit 3. The account id is top-level `[cloudflare] account_id = "ID"`, or env `CLOUDFLARE_ACCOUNT_ID`, which hoist reads itself. The secret sink
is an apply flag (`--secrets-to fly:APP`); minting an r2 key without it exits 3. JSON items (area
`cloudflare`): `dns_record` key `<name> <TYPE> <content>`, `r2_bucket` and `r2_lifecycle` key
`<bucket>`, `r2_token` key `hoist:<owner>/<repo>:r2:<bucket>`; human step ids `dns:<name> <TYPE>`
(check and apply exit 2) and `dns-cname:<host>`. Each item has `action` in
create|update|ok|drift|pending|unknown; only "ok" is done (no per-item `ok`, no "noop"). Exit 2
(check or apply) means `human_steps[]` are pending, never done; keel surfaces `human_steps[0]`.
