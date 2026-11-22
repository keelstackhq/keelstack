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
  (D-022, bosun#705: `[approvals] policy = "quorum"`). The port PR that carries this decision adds
  that policy to bosun and switches the root `.bosun/` of keelstackhq/keelstack to it; the
  switch is a trust-root edit, so the owner approves it.
- Hoist changesets (`.changeset/*.md` files that `hoist release prepare` reads) are an opt-in for
  consumer repos only. keelstackhq/keelstack stays lockstep: one version from the root
  `Cargo.toml`, bumped with `bash scripts/bump-version.sh X.Y.Z` (D-023).
- D-018 is superseded and its row leaves `decisions/DECISIONS.md`; the file stays for history.
  D-022 still governs signing and merging.

## Consequences

- bosun: the chat-record path is removed in the port PR that carries this decision:
  `bosun approve --chat` is gone and `risk-check` no longer counts a chat approval record.
  `[approvals] chat` is still parsed, so old configs load, but it is ignored. Unported:
  bosun#642, #644, #673, #680.
- hoist: the changeset commands are ported behind an opt-in; the keelstack release workflow does
  not read `.changeset/`.
- The trust root is unchanged: only the owner edits `.bosun/approvers.toml` and `[approvals]`.
