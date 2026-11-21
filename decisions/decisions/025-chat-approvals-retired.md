# D-025: Chat approvals are retired; hoist changesets are opt-in

- Date: 2026-11-21
- Scope: all
- Status: accepted
- Supersedes: D-018

## Context

D-018 let the owner's chat yes count as an approval, recorded by an orchestrator session with
`bosun approve <pr>@<head> --chat`. D-022 then made the orchestrator sign and merge on two
approving verdicts, so the chat form no longer serves a purpose, and a chat record is forgeable
evidence: it is trust in a session, not a signature. On 2026-11-21 the owner decided to retire it
everywhere. The owner also decided how hoist changesets (hoist#63-#71) fit D-023's single version.

## Decision

- Chat approvals are retired in every repo. A chat yes (a `bosun approve --chat` record, or an
  answer to an `AskUserQuestion` form) no longer counts as an approval. Agents do not ask the owner
  for one. This overrides the batched chat approvals clause of D-009.
- An owner approves with a GitHub comment `/bosun approve <sha>` (the full 40-character head SHA,
  posted as a login in `governance/owners.toml`), or the repo uses the quorum approvals policy
  (D-022, bosun#705: `[approvals] policy = "quorum"`). The root `.bosun/` of keelstackhq/keelstack
  switches to quorum once that policy is in bosun; until then `autonomous` stays.
- Hoist changesets (`.changeset/*.md` files that `hoist release prepare` reads) are an opt-in for
  consumer repos only. keelstackhq/keelstack stays lockstep: one version from the root
  `Cargo.toml`, bumped with `bash scripts/bump-version.sh X.Y.Z` (D-023).
- D-018 is superseded and its row leaves `decisions/DECISIONS.md`; the file stays for history.
  D-022 still governs signing and merging.

## Consequences

- bosun: the existing chat-record path (`bosun/src/cli/approve_chat.rs`, chat counting in
  `risk_check.rs`, spec 04 and 08, `bosun/AGENTS.md`, README, `docs/adopt-existing-repo.md`)
  stays until a follow-up PR removes it; until then set `[approvals] chat = false`. Unported:
  bosun#642, #643, #644, #673, #680.
- hoist: the changeset commands are ported behind an opt-in; the keelstack release workflow does
  not read `.changeset/`.
- The trust root is unchanged: only the owner edits `.bosun/approvers.toml` and `[approvals]`.
