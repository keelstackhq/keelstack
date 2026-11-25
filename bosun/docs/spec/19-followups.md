## 19. Follow-ups (D-028)

Findings that do not block a PR must not be lost. `bosun followups` keeps them in one rolling
open issue labelled `bosun:followups`, titled "Follow-ups (non-blocking findings)".

- `bosun followups add <pr> --from <verdict.json|->` takes a reviewer verdict (the `review-pack`
  skeleton, validated like `verdict comment`; its `pr` must equal `<pr>`, else exit 2) and appends
  its non-blocking findings as unchecked items: with `[review] block_on = "major"` (default)
  the minor and nit findings, with `"blocker"` also the major ones (read at the PR's base, section 8).
  An item is one line `- [ ] #<pr> <file>:<line> [<severity>] <claim>` (newlines and `<!--`
  in the claim are neutralised, the claim is cut at 200 characters, a secret is refused like in
  `lesson capture`). An item whose text is already in the issue, ticked or not, is skipped, so
  re-adding a verdict never duplicates. The issue is the lowest-numbered open one with the label;
  with none, bosun creates the label and the issue, and if a parallel run created another, the
  newer one is closed as a duplicate. A verdict with nothing below the blocking level writes nothing.
  Data: `{pr, issue | null, created, added, skipped, items}`. Exit 0; 2 for a bad verdict or a PR
  mismatch; 3 for GitHub errors. The kill switch applies. The body is read, extended and
  written back: two simultaneous adds can lose one item, a later add of the same verdict restores it.
- `bosun followups list` prints `{issue | null, open, ticked, items: [{text, pr}]}` for the
  unchecked items. Read only, no kill switch.
- Ticking an item (or deleting the line) is done by whoever picks it up, in the PR of that area.
  bosun never closes the issue. The orchestrator runs `bosun followups add` for each posted verdict
  and a session starting work in an area runs `bosun followups list`.
