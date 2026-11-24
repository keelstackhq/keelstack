# keel

keel sets up an existing repo across the keelstackhq tools in one command. It reads one manifest
(`keel.toml`), runs splice, bosun and hoist as subprocesses, reads their JSON, and stops at the
first step only a human can do, then tells you exactly what that step is. It is idempotent: each
step checks first, so a re-run resumes where the last one stopped.

keel configures a repo that the org provisioner app (`keelstackhq/provision`) already created. It
never creates repos or org settings; if the repo is missing, the human step says to request it
from the provisioner.

## Install

```bash
npx @keelstackhq/keel up --repo owner/name --template rust --pieces rust-cli:my-cli
```

Or `npm install -g @keelstackhq/keel`. The launcher needs Node 18+; the tools keel installs need
Node 24+ (checked in preflight).

## Commands

- `keel up`: run the steps, stop at the first human step or failure.
- `keel plan`: same checks, writes nothing, shows what `up` would do.
- `keel doctor`: check the toolchain and the repo for drift (no installs).

Global flags: `-C DIR` (checkout, default `.`), `--json`, `-v`. `up` and `plan` also take
`--repo`, `--template rust|ts|SOURCE`, `--template-ref REF`, `--pieces ID[:NAME],...`, `--plan FILE`, `--wait SECS`,
`--token-stdin`, `--confirm ID` (record a human step as done: `agent-env`, `dns`, `backup`, or any checklist id hoist cannot read back, such as `npm-trust:<pkg>` or `crates-trust:<crate>`; stored in `[confirmed] steps`).
`KEEL_POLL_SECS` sets the gate poll interval; `KEEL_TOOL_TIMEOUT_SECS` (default 600) kills a `hoist`, `fly-setup.sh` or `smoke.sh` call that runs longer.

## Steps

| # | Step | What it does |
|---|------|--------------|
| 1 | preflight | checks node, gh, git and the repo; installs pinned tools (checksum verified) |
| 2 | apps | Claude and Renovate GitHub Apps installed (human gate) |
| 3 | render | renders the template (at `template_ref`) and pieces with splice on a branch, commits only the files it wrote |
| 4 | governance | `bosun init`, optional plan import |
| 5 | settings | `hoist apply` for repo settings for everything but the `cloudflare` area (left to dns and backup) |
| 6 | token | stores the bot PAT as an environment secret (human gate) |
| 7 | agent-env | writes `.keel/cloud-setup.sh` for the cloud environment (human gate) |
| 8 | deploy | `[deploy.fly]`: runs the hull `scripts/fly-setup.sh` (app, volume, secrets, certificate; exit 3 = log in and install flyctl/gh) and stores the production deploy token (needs `fly auth login`) |
| 9 | dns | A/AAAA records for the host, through hoist `[cloudflare.dns]` (`targets = "fly"`; `no_mail = true` in `[deploy.fly]` adds SPF and DMARC) when `dns = "cloudflare"` and `CLOUDFLARE_API_TOKEN` is set (Zone Read + DNS Write); else it lists the records to add (a shared v4 is used when the app has no dedicated one; with `dns = "none"` the step never fails); `keel up --confirm dns` records that you added them (for the current host: a changed host voids it) |
| 10 | backup | `[backup]`: R2 bucket `<app>-backup` with a lifecycle rule and a scoped key that hoist delivers to the fly app as `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `R2_BUCKET`, `R2_ENDPOINT` (`provider = "r2"`; needs the Cloudflare account id: env `CLOUDFLARE_ACCOUNT_ID` (hoist reads it), else `[backup] account_id`, which keel writes to `hoist.toml` as `[cloudflare] account_id`); `keel up --confirm backup` records a bucket made by hand (for the current app: a changed app voids it) |
| 11 | doctor | `bosun doctor --remote`, push the branch, open the PR |
| 12 | merge | a human approves and merges the PR (human gate) |
| 13 | verify | after the merge: the deploy workflow succeeded, `https://HOST/healthz` answers 200, `scripts/smoke.sh` passes (`keel plan` does not run it). Prints a `verify` object in `--json` |

The deploy, dns, backup and verify steps do nothing without `[deploy.fly]` / `[backup]` in `keel.toml`.

keel never merges, tags, releases, publishes or force-pushes.

## keel.toml

```toml
repo = "owner/name"
template = "rust"                 # or a splice source
template_ref = "v0.13.0"          # git ref; from the hull pin for rust/ts, required for a custom source
pieces = ["rust-cli:my-cli"]
plan = "docs/plan.md"             # optional
branch = "keel/bootstrap"         # optional

[token]                           # optional
secret = "SPLICE_BOT_TOKEN"
environment = "splice-bot"

[deploy.fly]                      # optional; add the `fly` piece to `pieces`
app = "my-app"
region = "fra"
host = "app.example.com"          # no scheme
dns = "cloudflare"                # or "none" (default)
zone = "example.com"              # optional; HOST or a parent domain. Default: last two labels
                                  # host, zone, app are checked: lowercase DNS names / [a-z0-9-]+
                                  # of host (set it for hosts such as app.example.co.uk)
no_mail = false                   # true: also SPF and DMARC records for the host

[backup]                          # optional; needs [deploy.fly]
provider = "r2"                   # or "s3" (not automated)
retention_days = 30
account_id = "..."                # optional; 32 lowercase hex chars; env CLOUDFLARE_ACCOUNT_ID wins (not a secret, same rule)
```

Unknown keys are an error. keel never reads or prints `CLOUDFLARE_API_TOKEN` or a Fly token: it
only checks that the Cloudflare one is set, and hoist and the hull script move the secrets.

## Existing repo (`template = "none"`)

`keel.toml` with `repo = "OWNER/NAME"` and `template = "none"` sets up a repo that already exists
(no render; `pieces` is an error, `template_ref` and `plan` are not needed). The steps are
`preflight` (gh logged in with admin on the repo, git `origin` is the repo, `hoist --version`
runs; no pin install) and `settings` (`hoist --json check`, `hoist --json apply --yes`, all areas).
Without `CLOUDFLARE_API_TOKEN` and when `hoist --json explain` lists `cloudflare` items, hoist runs
with `--area github --area npm --area crates --area release` and the checklist gets
`cloudflare-token`. Every human step of every step goes into one batched checklist (D-009):
computer entries first, then phone ones (trusted publishers, app installs). Steps hoist cannot
read back are marked with `keel up --confirm <id>` and listed under `[confirmed] steps`.
Exit 5 while the checklist is not empty, 0 when it is empty and hoist reports no drift. `keel plan`
only reads (hoist check/explain, gh reads). Text output ends with `checklist:`, one numbered line
per entry.

## Exit codes

0 done · 1 a step failed or drift remains · 2 usage error (clap errors too) · 3 manifest,
environment or API error · 5 a human step is pending · 130/143 interrupted by
SIGINT/SIGTERM (the `--json` error object is still printed). `keel --json --help` prints one JSON object.

## JSON

`--json` prints exactly one object on stdout:

```json
{"version":1,"command":"up","ok":false,"exit_code":5,"status":"human",
 "steps":[{"id":"preflight","action":"ok","detail":"node 24.1.0, gh as octo, git 2.47"},
          {"id":"apps","action":"pending","detail":"Renovate app not installed on owner/name"}],
 "human_step":{"id":"apps","title":"Install the Renovate GitHub App on owner/name",
               "url":"https://github.com/apps/renovate/installations/new","command":"keel up"}}
```

Every report also has `"items"` (hoist items: `{step,area,kind,key,action,reason}`) and
`"checklist"` (every pending human step, deduped by id, minus `[confirmed] steps`:
`{id,title,url,command,device,verify}`, `device` `computer`|`phone`, computer first);
`human_step` is `checklist[0]`.

With `[deploy.fly]` the object also has `"verify":{"url":"https://HOST","healthz":"ok","http_status":200,"smoke":"none"}`
(`healthz`: `ok`|`failed`|`not-run`; `smoke`: `passed`|`failed`|`none`|`not-run`).

## Security: the PAT

Pass the token with `keel up --token-stdin` (paste, then Enter). keel gives it to
`gh secret set` (the one environment secret keel sets; repo secrets go through hoist) on stdin only. It never appears in argv, files, logs, stdout, stderr or JSON, and
it is zeroized after use. keel has no HTTP client; `gh` talks to GitHub.

This PAT step is a deliberate D-014 exception and the one human gate for credentials; it goes away once the splice bot uses the provisioner app's token.

## License

MIT, see [LICENSE](LICENSE).
