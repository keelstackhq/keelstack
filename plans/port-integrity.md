# Plan: port open PRs, test-integrity grant

project: keelstack

keelstackhq/keelstack#4 ports the open PRs of the archived repos and retires chat approvals
(D-025). Retiring them deletes the chat-approval code with its tests
(`bosun/tests/cli_approve_chat.rs`, `bosun/tests/cli_chat_count.rs`) and drops the chat cases
from `bosun/src/core/approve.rs`, `bosun/src/core/ask.rs`, `bosun/tests/cli_approve_ask.rs`
and `bosun/tests/config.rs`. Merging this plan is the owner's grant for exactly those
test-integrity findings in PR #4, which links the task below.

## PI: Port open PRs
area: repo
shape: single

### Tasks

- T1: port: open PRs from the archived repos (keelstackhq/keelstack#4)
  touch: .bosun/**, .github/**, .gitignore, Cargo.lock, MIGRATION.md, bosun/**, decisions/**, docs/**, hoist/**, hull/**, site/**, splice/**
  kind: feature
  risk: high
  allow: delete-test, fewer-asserts

### Done when

- UC-PI-1: PR #4 passes test-integrity with only the findings listed above allowed.
