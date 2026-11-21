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

- Chat approvals are retired in every repo. A chat yes no longer counts as an approval, and
  `bosun approve --chat`, `[approvals] chat` and `chat_signed` are not used. Agents never ask the
  owner to approve in chat.
- An owner approves in one of two ways: a GitHub comment `/bosun approve <sha>` (the full 40-character
  head SHA, posted as a login in `governance/owners.toml`), or by setting `[approvals] policy =
"quorum"` in `.bosun/bosun.toml` (D-022). The root `.bosun/` of keelstackhq/keelstack uses
  `quorum`.
- Hoist changesets (`.changeset/*.md` files that `hoist release prepare` reads) are an opt-in for
  consumer repos only. keelstackhq/keelstack stays lockstep: one version from the root
  `Cargo.toml`, bumped with `bash scripts/bump-version.sh X.Y.Z` (D-023).
- D-018 is superseded and its row leaves `decisions/DECISIONS.md`; the file stays for history.
  D-022 still governs signing and merging.

## Consequences

- bosun: the chat-record code and docs (bosun#642, #643, #644, #673, #680) are not ported; its
  docs name only the comment and quorum paths.
- hoist: the changeset commands are ported behind an opt-in; the keelstack release workflow does
  not read `.changeset/`.
- The trust root is unchanged: only the owner edits `.bosun/approvers.toml` and `[approvals]`.
