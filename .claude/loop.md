One task per iteration (bosun's loop: `bosun/AGENTS.md`, "The loop for one task").

1. Read the root `AGENTS.md` and the folder's `CLAUDE.md`; take the next task (plan order, skip
   Blocked ones). Delegate searches to `explorer`.
2. Dispatch `implementer` (test-first, that task only), then `reviewer` on its diff; fix findings
   via `implementer`. A red CI run goes to `ci-fixer`.
3. Verify with real output from the repo root: `cargo fmt --all --check`,
   `cargo clippy -p <crate> --all-targets --locked -- -D warnings`, `cargo test -p <crate> --locked`;
   hull: `pnpm --dir hull test && pnpm --dir hull lint && pnpm --dir hull format:check`.
4. Stop after three failed fixes of the same failure and report it in at most 5 lines. A
   human-only step → Blocked with the exact action. Never merge, tag or release.
