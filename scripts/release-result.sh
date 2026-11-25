#!/usr/bin/env bash
# Turns the result of `hoist release publish --only <registry> --json` into the job outcome of
# release.yml's job `publish` (steps `crates` and `npm`).
#
#   bash scripts/release-result.sh <result.json> <hoist exit code> <crates|npm>
#
# Exit 0 (everything published or already there): `skipped=` (empty) goes to $GITHUB_OUTPUT.
# Exit 2 (a package is not on its registry yet, `hoist release bootstrap` is the owner's step):
#   `skipped=<names>` goes to $GITHUB_OUTPUT, a notice names them and the step succeeds; the
#   major tag (job `release`) waits until a re-run publishes them.
# Any other code is a failure: the job fails with that code.
set -euo pipefail

[ "$#" -eq 3 ] || { echo "usage: bash scripts/release-result.sh <result.json> <exit code> <crates|npm>" >&2; exit 2; }
file=$1 code=$2 what=$3
out=${GITHUB_OUTPUT:-/dev/null}
summary=${GITHUB_STEP_SUMMARY:-/dev/null}

case "$code" in
  0)
    echo "skipped=" >>"$out"
    ;;
  2)
    # Item keys are <name>@<version>; the last @ starts the version (scoped names begin with @).
    skipped=$(jq -r '[.items[] | select(.action == "pending") | .key | sub("@[^@/]+$"; "")] | unique | join(" ")' "$file")
    [ -n "$skipped" ] || { echo "::error::hoist exited 2 without a pending package: see the output above" >&2; exit 2; }
    echo "skipped=$skipped" >>"$out"
    echo "::notice::$what: not published yet: $skipped. Run \`hoist release bootstrap\` locally (docs/releasing.md), then re-run this workflow"
    echo "$what: not published yet (run \`hoist release bootstrap\`): $skipped" >>"$summary"
    ;;
  *)
    echo "::error::hoist release publish ($what) failed with exit $code: see the output above" >&2
    exit "$code"
    ;;
esac
