# D-013: Bigger PRs for low-risk work during bootstrap mode

- Date: 2026-10-29
- Scope: all
- Status: accepted
- Supersedes: —

## Context

Each PR costs a review round, a CI run, a merge and handoffs between sessions. During bootstrap
mode (D-012) the owner wants faster progress. Bigger PRs save those costs, but a reviewer catches
less as a diff grows. In bootstrap mode the independent review is the only check on gated PRs, so
those must stay small.

## Decision

The rule applies while bootstrap mode is on (D-012, until 2026-11-30 23:59 UTC). Size is counted
as changed lines (added plus deleted), without tests, snapshots, lockfiles and generated files.

- **Low-risk PRs may be up to 1,500 lines** and may bundle related tasks from one plan:
  - docs;
  - site content;
  - new self-contained code;
  - tests;
  - generated files;
  - changes inside one package that touch no gated path.
- **Gated PRs stay at most 400 lines and hold one task.** Gated PRs are:
  - workflows, templates and `docs/template-format.md`;
  - governance and `.bosun/**`;
  - approval or risk code;
  - anything touching secrets, auth, environments or release paths.
- A PR with any gated file counts as gated. Split it rather than mixing.
- Over a limit, the author splits the PR before review. A reviewer may ask for a split at any
  size if the diff is too big to review well.
- After 2026-11-30, the repos' usual sizes apply again.

## Consequences

- Child sessions batch related low-risk tasks into one PR, with one push per PR (D-007).
- bosun's size checks or hints, where it has them, use these limits for the bootstrap window.
- Reviewers state in their verdict when a size made the review less certain.
