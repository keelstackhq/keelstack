# BA: trusted authors for intake (#143)

Done when UC-BA-1: with `[intake] trusted_authors = "members"`, an outside user's issue is never claimed or spawned, and `wave plan` lists it as skipped.
touch: bosun/src/config.rs, bosun/src/cli/claim.rs, bosun/src/core/intake_trust.rs, bosun/tests/intake_trust.rs (+ minimum: core/mod.rs, wave/spawn/comment callers, spec section)

1. Test first (intake_trust.rs): outsider issue not claimed, listed skipped with reason; owner/owners.toml/OWNER|MEMBER|COLLABORATOR trusted; login list; default "any" unchanged; comments from outsiders ignored by bosun commands.
2. Config `[intake] trusted_authors` = "any" | "members" | [logins]; pure `core/intake_trust.rs` decision fn (no I/O).
3. Wire into claim, wave plan, spawn, issue pick-up, comment-command parsing; skipped items listed with reason.
4. fmt, clippy, tests keel-bosun (+ keel). Reviewer opus vs Done when.
