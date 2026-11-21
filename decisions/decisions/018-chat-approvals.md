# D-018: The owner's chat yes is the approval

- Date: 2026-11-10
- Scope: all
- Status: accepted
- Supersedes: D-012

## Context

Under D-012 an owner's chat answer only counted once the orchestrator signed it with an ed25519 key
enrolled in `.bosun/approvers.toml`. Enrolling the key needs a PR the owner approves on GitHub,
which a cloud session cannot get: its PRs are opened under the owner's own login, so GitHub refuses
the owner's review. The `/bosun approve <sha>` comment fallback is unmemorable. On 2026-11-10 the
owner said: "if there is a need for an approval, Claude asks the user (in any scenario: local,
web/cloud, Claude Tag) and if the user agrees, it's approved", and chose option A of
keelstackhq/bosun `docs/superpowers/specs/2026-11-10-chat-approval-design.md`: the chat yes is the
evidence, guarded by who may record it. No key.

## Decision

- **Asking.** Every owner approval is asked in chat: `AskUserQuestion` in local and cloud sessions,
  a thread post in Claude Tag (the owner answers `approve <short sha>`). The question names repo,
  PR number and title, risk rules hit, head SHA and the reviewer verdict, and offers approve and
  hold. Several PRs may share one question (D-009). Agents never ask the owner to approve on
  GitHub, write a comment, or paste a command.
- **Recording.** Only an orchestrator session records the yes: a root session (depth 1, no parent)
  or origin `local`/`slack` in the bosun registry. It runs `bosun approve <pr>@<head> --chat`
  (Claude Tag: plus `--slack-user <id>`), which posts one comment naming the exact head SHA the
  owner saw and the orchestrator session id. Child sessions, reviewers and wave agents never
  record or post one; they hand the question to the orchestrator (`bosun handoff-pr`).
- **Counting.** A chat record is the owner's approval under every `[approvals] policy` (D-014),
  R-weaken (D-017) and gated paths included. It counts only for the head it names; a new push
  makes it stale. Signed approvals, owner GitHub reviews and owner-written approval comments still
  count; `.bosun/approvers.toml` and signing keys are optional. A repo may turn chat records off
  with `[approvals] chat = false` in `.bosun/intake.toml`.
- **Hold** is remembered for that head and not asked again; a new head asks again.
- **Bootstrap mode (kept from D-012 until 2026-11-30 23:59 UTC).** The orchestrator may approve
  gated PRs itself when all required checks (or, without CI, the D-019 local checks) are green on
  the head and an independent reviewer session it dispatched approved that head; the record says
  `bootstrap mode (D-018, until 2026-11-30)` and does not claim an owner answer. Not covered:
  environment approvals (`release`, `prod`, `preview`, `splice-bot`), hand-pushed tags,
  keelstackhq/decisions merges (D-002), branch protection, rulesets, required checks, secrets and
  age keys (D-010). After the date bosun stops accepting bootstrap records.

## Consequences

- bosun: `approve` list mode returns the question (`ask`, `next`); `approve --chat` refuses
  non-orchestrators, stale heads and heads without a clean verdict from another session;
  risk-check counts unedited chat records by an owner login whose session is an orchestrator;
  the PreToolUse hook refuses chat-record bodies from any other command. These land behind this
  decision and count only from a pinned bosun release that knows them.
- Trust is in the orchestrator, not a signature: a lying session could fake a record. The owner
  accepted this; signed keys stay as the stronger option.
- D-017 (autonomous mode) is unchanged; when it ends, this decision applies.
