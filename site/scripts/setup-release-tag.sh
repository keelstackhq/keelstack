#!/usr/bin/env bash
# One-time setup for docspack releases from the Actions tab (docs/agents/project.md, Blocked →
# docspack releases, steps 2 and 3). Run it as a repo admin with an authenticated `gh`:
#
#   bash scripts/setup-release-tag.sh --app-id <App ID> --client-id <Client ID> --key <app.pem> [--dry-run]
#
# Idempotent. It sets the repository variable HOIST_RELEASE_APP_CLIENT_ID, creates the
# environment `release-tag` limited to branch `main`, stores the App's private key as its
# environment secret HOIST_RELEASE_APP_KEY (read from the file on stdin, never echoed, never in
# argv), adds the App to the `release-tags` ruleset bypass list if that ruleset exists, and checks
# that the App's installation covers the repo. It never changes the `release` environment, which
# stays limited to the release tags. --dry-run only reads and prints what it would do.
set -euo pipefail

REPO="keelstackhq/keelstack"
ORG="${REPO%%/*}"
ENV_NAME="release-tag"
RULESET="release-tags"
APP_ID=""
CLIENT_ID=""
KEY=""
DRY_RUN=0
SUMMARY=()

usage() {
  echo "usage: bash scripts/setup-release-tag.sh --app-id <id> --client-id <id> --key <pem> [--repo owner/name] [--dry-run]" >&2
  exit 2
}

while [ $# -gt 0 ]; do
  case "$1" in
    --app-id) APP_ID="${2:-}"; shift 2 ;;
    --client-id) CLIENT_ID="${2:-}"; shift 2 ;;
    --key) KEY="${2:-}"; shift 2 ;;
    --repo) REPO="${2:-}"; ORG="${REPO%%/*}"; shift 2 ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h | --help) usage ;;
    *) echo "unknown argument: $1" >&2; usage ;;
  esac
done
if [ -z "$APP_ID" ] || [ -z "$CLIENT_ID" ] || [ -z "$KEY" ]; then usage; fi
[[ "$APP_ID" =~ ^[0-9]+$ ]] || { echo "--app-id must be the numeric App ID (not the Client ID)" >&2; exit 2; }
[ -r "$KEY" ] || { echo "cannot read the private key file: $KEY" >&2; exit 2; }
command -v gh > /dev/null || { echo "gh is not on PATH" >&2; exit 2; }
command -v jq > /dev/null || { echo "jq is not on PATH" >&2; exit 2; }

note() { SUMMARY+=("$1"); echo "$1"; }

# write <description> <command...>: run a write, or only describe it on --dry-run.
write() {
  local what="$1"
  shift
  if [ "$DRY_RUN" = 1 ]; then
    note "dry run: would $what"
  else
    "$@" > /dev/null
    note "done: $what"
  fi
}

# 1. Repository variable with the App's Client ID (not a secret).
write "set repository variable HOIST_RELEASE_APP_CLIENT_ID" \
  gh variable set HOIST_RELEASE_APP_CLIENT_ID --repo "$REPO" --body "$CLIENT_ID"

# 2. Environment release-tag: custom deployment branch policies, exactly branch `main`.
if gh api "repos/$REPO/environments/$ENV_NAME" > /dev/null 2>&1; then
  note "environment $ENV_NAME exists"
fi
write "create or update environment $ENV_NAME (custom branch policies)" \
  gh api -X PUT "repos/$REPO/environments/$ENV_NAME" --input - <<< \
  '{"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}}'

policies="$(gh api "repos/$REPO/environments/$ENV_NAME/deployment-branch-policies" 2> /dev/null || echo '{}')"
if jq -e '.branch_policies // [] | any(.name == "main" and (.type // "branch") == "branch")' <<< "$policies" > /dev/null; then
  note "environment $ENV_NAME already allows branch main"
else
  write "allow branch main in environment $ENV_NAME" \
    gh api -X POST "repos/$REPO/environments/$ENV_NAME/deployment-branch-policies" --input - <<< \
    '{"name":"main","type":"branch"}'
fi
while read -r id name; do
  [ -n "$id" ] || continue
  write "remove deployment policy '$name' from environment $ENV_NAME (main only)" \
    gh api -X DELETE "repos/$REPO/environments/$ENV_NAME/deployment-branch-policies/$id"
done < <(jq -r '.branch_policies // [] | .[] | select(.name != "main" or (.type // "branch") != "branch") | "\(.id) \(.name)"' <<< "$policies")

# 3. The App's private key as an environment secret, from the file on stdin.
if [ "$DRY_RUN" = 1 ]; then
  note "dry run: would set secret HOIST_RELEASE_APP_KEY in environment $ENV_NAME from $KEY"
else
  gh secret set HOIST_RELEASE_APP_KEY --repo "$REPO" --env "$ENV_NAME" < "$KEY" > /dev/null
  note "done: set secret HOIST_RELEASE_APP_KEY in environment $ENV_NAME"
fi

# 4. Ruleset bypass: add the App (Integration) to release-tags, if the ruleset exists.
ruleset_id="$(gh api "repos/$REPO/rulesets" 2> /dev/null | jq -r --arg n "$RULESET" '.[]? | select(.name == $n and .source_type == "Repository") | .id' | head -n1)"
if [ -z "$ruleset_id" ]; then
  note "ruleset $RULESET not found: create it (docs/agents/project.md, Blocked → docspack releases, step 3), then re-run"
else
  ruleset="$(gh api "repos/$REPO/rulesets/$ruleset_id")"
  # Without bypass_actors (only admins see it), a PUT could drop the existing bypass entries.
  if ! jq -e 'has("bypass_actors")' <<< "$ruleset" > /dev/null; then
    echo "cannot read the bypass list of ruleset $RULESET (are you a repo admin?); nothing changed in it" >&2
    exit 1
  fi
  if jq -e --argjson app "$APP_ID" '.bypass_actors // [] | any(.actor_id == $app and .actor_type == "Integration")' <<< "$ruleset" > /dev/null; then
    note "ruleset $RULESET already lets App $APP_ID bypass"
  else
    body="$(jq -c --argjson app "$APP_ID" \
      '{bypass_actors: ((.bypass_actors // []) + [{actor_id: $app, actor_type: "Integration", bypass_mode: "always"}])}' <<< "$ruleset")"
    write "add App $APP_ID to the bypass list of ruleset $RULESET" \
      gh api -X PUT "repos/$REPO/rulesets/$ruleset_id" --input - <<< "$body"
  fi
fi

# 5. The App's installation must cover the repo.
installations="$(gh api "orgs/$ORG/installations" --paginate 2> /dev/null || echo '{}')"
inst="$(jq -r --argjson app "$APP_ID" '[.installations // [] | .[] | select(.app_id == $app)] | first // empty | "\(.id) \(.repository_selection)"' <<< "$installations")"
installs_url="https://github.com/organizations/$ORG/settings/installations"
if [ -z "$inst" ]; then
  note "App $APP_ID is not installed on $ORG (or not visible to you): install it on $REPO at $installs_url"
else
  read -r inst_id selection <<< "$inst"
  if [ "$selection" = "all" ]; then
    note "App $APP_ID is installed on all $ORG repositories"
  elif gh api "user/installations/$inst_id/repositories" --paginate 2> /dev/null |
    jq -e --arg r "$REPO" '[.repositories // [] | .[] | .full_name] | index($r) != null' > /dev/null; then
    note "App $APP_ID is installed on $REPO"
  else
    note "App $APP_ID is installed on $ORG but not on $REPO: add the repo at $installs_url"
  fi
fi

# 6. `release` stays limited to the release tags: report, never change.
release_policies="$(gh api "repos/$REPO/environments/release/deployment-branch-policies" 2> /dev/null || echo '{}')"
if jq -e '.branch_policies // [] | any((.type // "branch") == "branch")' <<< "$release_policies" > /dev/null; then
  note "warning: environment release allows a branch; keep it to the *-v* tag rule (not changed here)"
else
  note "environment release left as is (tags only)"
fi

echo
echo "Summary ($REPO$([ "$DRY_RUN" = 1 ] && echo ', dry run')):"
printf '  - %s\n' "${SUMMARY[@]}"
