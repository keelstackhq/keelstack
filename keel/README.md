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
`--token-stdin`, `--confirm agent-env`.

## Steps

| # | Step | What it does |
|---|------|--------------|
| 1 | preflight | checks node, gh, git and the repo; installs pinned tools (checksum verified) |
| 2 | apps | Claude and Renovate GitHub Apps installed (human gate) |
| 3 | render | renders the template (at `template_ref`) and pieces with splice on a branch, commits only the files it wrote |
| 4 | governance | `bosun init`, optional plan import |
| 5 | settings | `hoist apply` for repo settings |
| 6 | token | stores the bot PAT as an environment secret (human gate) |
| 7 | agent-env | writes `.keel/cloud-setup.sh` for the cloud environment (human gate) |
| 8 | doctor | `bosun doctor --remote`, push the branch, open the PR |
| 9 | merge | a human approves and merges the PR (human gate) |

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
```

Unknown keys are an error.

## Exit codes

0 done · 1 a step failed or drift remains · 2 usage error (clap errors too) · 3 manifest,
environment or API error · 5 a human step is pending. `keel --json --help` prints one JSON object.

## JSON

`--json` prints exactly one object on stdout:

```json
{"version":1,"command":"up","ok":false,"exit_code":5,"status":"human",
 "steps":[{"id":"preflight","action":"ok","detail":"node 24.1.0, gh as octo, git 2.47"},
          {"id":"apps","action":"pending","detail":"Renovate app not installed on owner/name"}],
 "human_step":{"id":"apps","title":"Install the Renovate GitHub App on owner/name",
               "url":"https://github.com/apps/renovate/installations/new","command":"keel up"}}
```

## Security: the PAT

Pass the token with `keel up --token-stdin` (paste, then Enter). keel gives it to
`gh secret set` (the one environment secret keel sets; repo secrets go through hoist) on stdin only. It never appears in argv, files, logs, stdout, stderr or JSON, and
it is zeroized after use. keel has no HTTP client; `gh` talks to GitHub.

This PAT step is a deliberate D-014 exception and the one human gate for credentials; it goes away once the splice bot uses the provisioner app's token.

## License

MIT, see [LICENSE](LICENSE).
