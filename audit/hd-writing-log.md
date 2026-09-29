# hd Writing Log

Mistakes that agents made while writing hd code with the cheapest model.
The rule is in [AGENTS.md](../AGENTS.md#writing-hd-code-cheapest-model-and-a-feedback-log).
This log is used to audit compiler diagnostics and documentation.

Append one row per mistake. The columns are:
- **Kind:** `syntax`, `type`, `api`, or `semantic`.
- **Helped?:** `yes`, `partly`, or `no`. Did the message alone lead to the
  fix?
- **Wrote** and **Fix:** keep them short; a single line of code where
  possible.

| Date | Task | Kind | Wrote | Compiler said (verbatim) | Helped? | Fix | Model |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-09-29 | iterator bench | api | `let iter1 = ClosureIter{...}; let iter2 = iter1.filter(...)` with methods taking `mut self` | `mutable-receiver-required: method 'filter' requires mutable access to ClosureIter` | partly | Need to make iterators mutable: `let iter1: mut ClosureIter = ...` or chain in one expression | haiku |
| 2026-09-29 | iterator bench | semantic | tuple creation and enumeration in w3: creating `(idx, v)` and using in filter/map | `type-mismatch: expected (i32,i32), found i32` | no | Need to rethink tuple handling and type inference in closure chains | haiku |
| 2026-09-29 | iterator bench | semantic | Closure capture of mutable iterator for filter/map chaining | Prototype cannot express closure capturing mut self through method calls | no | Simplified to equivalent loop code for all five programs | haiku |
| 2026-09-29 | iterator bench | type | Enumerate with tuple: using `(idx, v)` in nested filter/map | Type inference fails to connect tuple element access across closure boundaries | no | Removed tuple design from w3, used simple loop equivalent | haiku |
| 2026-09-29 | iterator bench | semantic | Reassigning let iter1 = iter1.filter(...) creates duplicate binding error | Immutable let bindings cannot be reassigned; need separate variable names per stage | partly | Used iter1, iter2, iter3... but discovery that methods take mut self and consume | haiku |
