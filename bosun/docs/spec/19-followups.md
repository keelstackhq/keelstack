## 19. Follow-ups (D-028)

Findings that do not block a PR must not be lost. `bosun followups` keeps them in one rolling
open issue labelled `bosun:followups`, titled "Follow-ups (non-blocking findings)".

- `bosun followups add <pr> --from <verdict.json|->` takes a reviewer verdict (the `review-pack`
  skeleton, validated like `verdict comment`; its `pr` must equal `<pr>`, else exit 2) and appends
  its non-blocking findings as unchecked items: with `[review] block_on = "major"` (default)
  the minor and nit findings, with `"blocker"` also the major ones (read at the PR's base, section 8).
  An item is one line `- [ ] #<pr> <file>:<line> [<severity>] <claim>` (newlines and `<!--`
  in the claim are neutralised, the claim is cut at 200 characters, a secret is refused like in
  `lesson capture`). Every add first drops the ticked items (`- [x]` or `- [X]`) and keeps the open ones; an item whose
  text is already open is skipped, so re-adding a verdict never duplicates (a ticked item is
  done: adding its verdict again re-adds it). `@` in a claim gets a zero-width space after it, so
  a finding never notifies a user or team. A body that would pass 60000 characters is refused
  (exit 6, nothing written, fix: tick items and add again). After writing, the issue is read back;
  if a parallel add lost the write, it is redone once from the fresh body (the issue is also re-read right before each write, so a box ticked meanwhile is kept), else exit 3 with the fix
  to run the same add again. The issue is the lowest-numbered open one with the label;
  with none, bosun creates the label and the issue, and if a parallel run created another, the
  newer one is closed as a duplicate. A verdict with nothing below the blocking level writes nothing.
  Data: `{pr, issue | null, created, duplicates, added, skipped, items}`; `duplicates` lists other open `bosun:followups` issues (items go to the oldest; close the others). Exit 0; 2 for a bad verdict or a PR
  mismatch; 1 for GitHub errors; 3 when a parallel add lost the write twice. The kill switch applies. The body is read, extended and
  written back: a lost write is detected by the read-back (see above).
- `bosun followups list` prints `{issue | null, open, ticked, items: [{text, pr}]}` for the
  unchecked items. Read only, no kill switch.
- Ticking an item (or deleting the line) is done by whoever picks it up, in the PR of that area.
  bosun never closes the issue. The orchestrator runs `bosun followups add` for each posted verdict
  and a session starting work in an area runs `bosun followups list`.
