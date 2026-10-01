# F-401: Replay code identity is a per-function source hash, not the decided whole-module semantic hash
Severity: minor
Area: runtime
Duplicates: F-611 (merged: formatting edits break replay), F-264 (merged into F-611 earlier)
Evidence: audit/evidence/04-runtime/edits.tsv (row `v04-helper-body-change.hd`); `instantiate` in src/compiler.ts still hashes `source.slice(span)` per function and closure (re-checked 2026-09-26)
Effect: Two symptoms of one cause.
- A changed body in an executed non-suspending function is accepted. base.hd
  is recorded, and its `main` returns 42. Changing `helper` from `value * 2`
  to `value * 3` still replays with exit 0 and prints 63. Only functions that
  own a suspension event are checked.
- A formatting-only edit inside a checked function is rejected. Adding a
  comment line, or changing `value + 2` to `value  +  2`, makes replay fail
  with `replay event 1 does not match function code identity`, through an
  uncaught JavaScript stack trace (F-265). A formatter run invalidates every
  recording. Site IDs also embed source offsets (for example
  `child:provider:Counter.add:44`).
Recommendation: implementation change. Decided rule
(future-work/archive/RUNTIME_AND_LIBRARY.md, Replay Rules): code identity is a hash of
the whole module's semantic content, so any semantic change anywhere in the
module invalidates the history, and formatting and comment edits never do.
Hash a normalized form such as typed HIR, not raw source text, and drop source
offsets from site IDs.
