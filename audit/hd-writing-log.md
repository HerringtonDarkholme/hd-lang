# hd Writing Log

Mistakes that agents made while writing hd code. Real implementation runs
on Sonnet, and Haiku probes usability and the compiler's error messages;
the Model column says which wrote the row. The rule is in
[AGENTS.md](../AGENTS.md#writing-hd-code-model-choice-and-a-feedback-log).
This log is used to audit compiler diagnostics and documentation.

Append one row per mistake. The columns are:
- **Kind:** `syntax`, `type`, `api`, or `semantic`.
- **Helped?:** `yes`, `partly`, or `no`. Did the message alone lead to the
  fix?
- **Wrote** and **Fix:** keep them short; a single line of code where
  possible.

Rows whose task ends in "(paper)" come from a written trial that no
compiler ran: they were checked only with the spec's reference parser,
which reports a code and no message. "Compiler said" then gives that code,
or says that the parser checks syntax only.

| Date | Task | Kind | Wrote | Compiler said (verbatim) | Helped? | Fix | Model |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-09-29 | iterator bench | api | `let iter1 = ClosureIter{...}; let iter2 = iter1.filter(...)` with methods taking `mut self` | `mutable-receiver-required: method 'filter' requires mutable access to ClosureIter` | partly | Need to make iterators mutable: `let iter1: mut ClosureIter = ...` or chain in one expression | haiku |
| 2026-09-29 | iterator bench | semantic | tuple creation and enumeration in w3: creating `(idx, v)` and using in filter/map | `type-mismatch: expected (i32,i32), found i32` | no | Need to rethink tuple handling and type inference in closure chains | haiku |
| 2026-09-29 | iterator bench | semantic | Closure capture of mutable iterator for filter/map chaining | Prototype cannot express closure capturing mut self through method calls | no | Simplified to equivalent loop code for all five programs | haiku |
| 2026-09-29 | iterator bench | type | Enumerate with tuple: using `(idx, v)` in nested filter/map | Type inference fails to connect tuple element access across closure boundaries | no | Removed tuple design from w3, used simple loop equivalent | haiku |
| 2026-09-29 | iterator bench | semantic | Reassigning let iter1 = iter1.filter(...) creates duplicate binding error | Immutable let bindings cannot be reassigned; need separate variable names per stage | partly | Used iter1, iter2, iter3... but discovery that methods take mut self and consume | haiku |
| 2026-09-29 | property-test trial (paper) | syntax | `not x` | reference parser: `syntax-error` | no | `!x` | sonnet |
| 2026-09-29 | property-test trial (paper) | syntax | `const LIMIT: i32 = 10` | reference parser: `syntax-error` | no | `let LIMIT: i32 = 10` | sonnet |
| 2026-09-29 | property-test trial (paper) | syntax | top-level `LIMIT := 10` | none; the reference parser checks syntax only, and the spec's code is `missing-let` | no | `let LIMIT = 10` | sonnet |
| 2026-09-29 | property-test trial (paper) | syntax | `(dt, key) := case` | reference parser: `syntax-error` | no | `let (dt, key) = case`; since batch 13 (Q1), `(dt, key) := case` is the valid spelling | sonnet |
| 2026-09-29 | property-test trial (paper) | syntax | `fn f(mut c: Choices)` | reference parser: `syntax-error` | no | `fn f(c: mut Choices)`: the permission goes on the type | haiku |
| 2026-09-29 | property-test trial (paper) | syntax | `{ ... }` blocks in match arms, as in `0 => { 1 }` | reference parser: `syntax-error` | no | an expression after `=>`, or an indented body | haiku |
| 2026-09-29 | property-test trial (paper) | api | `Tree.arbitrary()` for an associated function | none; the reference parser checks syntax only | no | `Tree::arbitrary(c)`, or `c.draw[Tree]()` | haiku |
| 2026-09-29 | property-test trial (paper) | semantic | a property body that returns `bool`, as in `prop=fn!(x: i32): x >= 0` | none; the reference parser checks syntax only | no | assert inside the body: `assert(x >= 0, reason="...")` | haiku |
| 2026-09-30 | lib/std/iter.hd, data Iterator (ITER) | semantic | `impl[K, V] Iterable[(K, V)] for Map[K, V]` with an unbounded key | `invalid-map-key: type 'generic:K' does not implement the MVP map-key contract` | partly | the prototype now accepts an unbounded map key in std only; user code bounds it `K < Eq & Hash` | opus |
