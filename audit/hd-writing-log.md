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
| 2026-09-30 | lib/std/iter.hd, FromIterator (COLLECT) | type | `let payloads = Iterator::from_fn(...)`, then `C::from_iter(payloads)`, whose parameter is `mut Iterator[T]` | `readonly-argument-to-mutable-parameter: readonly argument 'Iterator[generic:T]' cannot satisfy mutable parameter 'mut:Iterator[generic:T]'` | yes | `let mut payloads = Iterator::from_fn(...)`: `let` infers the readonly view | opus |
| 2026-09-30 | lib/std/text.hd, string slice (STR) | type | `@intrinsic("index_out_of_bounds") fn index_out_of_bounds() -> never` | internal compiler error: `cannot emit unknown type 'never'` (a stack trace, no diagnostic) | partly | `-> void`; a caller that needs a value adds `panic("unreachable")` after the call | opus |
| 2026-09-30 | test/std/text.hd, string::from_utf8 (STR) | api | `assert_equal(string::from_utf8(bytes), .Ok("hé"), reason=...)`, where `Utf8Error` derives nothing | `missing-eq: type 'Result[string,Utf8Error]' does not implement Eq` | yes | a helper that matches the `Result` and returns a `string` to compare | opus |
| 2026-09-30 | lib/std/testing.hd, Arbitrary for f64 (PT) | syntax | `zero: f64 = 0.0` as a local binding | none; noticed before compiling | no | `let zero: f64 = 0.0` | opus |
| 2026-09-30 | lib/std/testing.hd, Choices.map (PT) | type | `map[K < Eq & Hash, V]` in a std module that does not import `Hash` | `unknown-trait: unknown trait 'Hash'` | yes | `use std.hash.Hash` | opus |
| 2026-09-30 | lib/std/testing.hd, Arbitrary for f32 (PT) | type | match arms `f32(c.int(1, 1000)) * 1.4e-45` and `c.float(-1.0, 1.0) * 3.4028235e38` in a function returning `f32` | `no-common-type: match arms have types f32 and f64 with no common type` | partly | bind each constant first: `let tiny: f32 = 1.4e-45` | opus |
| 2026-09-30 | lib/std/testing.hd, Arbitrary for List (PT) | type | `c.list(8, fn(inner: mut Choices) -> T: T::arbitrary(inner))` in `impl[T < Arbitrary] Arbitrary for List[T]` | `unknown-type: unknown associated-function owner 'T'` | no | a private bounded method, `c.list_of[T](8)`; a closure does not see the implementation's bounds | opus |
| 2026-09-30 | lib/std/testing.hd, Arbitrary for List (PT) | type | `c.list(8, arbitrary_of[T])`, a bounded generic function as a value | `unsatisfied-trait-bound: generic parameter 'T' does not implement __std_testing_Arbitrary, required by the bound on 'T' of '__std_testing_arbitrary_of'` | no | a private bounded method; the prototype passes no bound into a function value | opus |
| 2026-09-30 | lib/std/testing.hd, arbitrary.with (PT) | api | `use std.inspect.{Inspectable, TypeId, downcast_val}` in a std module | `unknown-trait: unknown trait '__std_inspect_Inspectable'` | partly | import `std.inspect` into the program that uses `arbitrary.with` instead; the prototype imports it under its standard names only | opus |
| 2026-09-30 | lib/std/arbitrary.hd, with (PT) | type | `Generator { draw: fn(c: mut Choices) -> Inspectable: gen(c) }` in `with[T < Inspectable]` | `type-mismatch: expected trait:Inspectable, found generic:T` | no | a generic `Drawer[T]` with `impl[T < Inspectable] Draw for Drawer[T]`, erased to a `Draw` trait value | opus |
| 2026-09-30 | derived Arbitrary code (PT) | api | `(fact.draw)(c)`, reading a private field of `std.testing.arbitrary.Generator` | `private-member: field 'draw' is not visible from this module` | yes | a std function, `draw_with(generator, c)` | opus |
| 2026-09-30 | lib/std/testing.hd, Arbitrary template (SR1) | type | `ArbitrarySource { c: c, at: 0 }` for `data ArbitrarySource[S]`, whose parameter `S` no field names | `unresolved-generic-placeholder: could not infer data parameter S`, reported at the std source's line and column (162:55) in the deriving program's file | partly | `ArbitrarySource[T] { c: c, at: 0 }` | opus |
| 2026-09-30 | derived code for a function-typed member (AT-with) | syntax | `let hd_m0: fn() -> i32? = .None`, meant as an optional function, from `${type}?` in `checker/typed-derivation.ts` | `missing-contextual-enum-type: variant '.None' requires an expected enum type` and `type-mismatch: expected fn()->(fn()->i32)?, found fn()->(fn()->i32?)` | partly | `(fn() -> i32)?`: `?` binds to the result type, so parenthesize the function type | opus-5.5 |
| 2026-09-30 | template reading the target's name (ST8) | api | `Structure::name()` inside `impl[T] Named for T by Structure` | `unknown-type: unknown associated-function owner 'Structure'` | no | `T::name()`; the qualified form has no rule for its `Self` yet (ST8-self) | opus-5.5 |
