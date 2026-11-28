# Function Values: A Static `fn(..)` First

Accepted by the owner and implemented, 2026-11-28: step 1 in
d7469337e..4a87d1bfb, steps 2 and 3 in 4dc807c89..faf87690e. The
specification (chapters 07 and 13) is the source of truth; this record
keeps the design's reasoning. Issues #1436, #2409, #1449 (closed).

## The proposal

Three steps. Step 1 is the owner's decided form; this doc asks for its
approval with the census and the plan.

1. **`fn(..)` is always static, and it is one pointer.** A function value
   never holds a local reference (`&T`, `&mut T`). A closure that captures
   one goes only to `F < Fn/FnMut(..)` or `impl Fn/FnMut(..)`. A `fn(..)`
   value is the address of a closure object `[count][descriptor][code][captures…]`.
   A capture-free function is one static object, count 0.
2. **A type per closure** through `F < Fn/FnMut(..)` (no new syntax).
3. **#1449**: a closure type's `let` cells become frame slots.

```hd
use std.ops.Fn

struct Point:
    x: i32

struct Handler:
    run: fn() -> i32

fn keep(f: fn() -> i32) -> Handler:
    Handler { run: f } # #2409: always valid, `f` holds no local reference

fn call_now(f: impl Fn() -> i32) -> i32:
    f()

fn borrowed(p: &Point) -> i32:
    call_now(fn() -> i32: p.x) # valid: an `impl Fn` parameter may borrow

fn copied(p: &Point) -> Handler:
    p := *p # fix-it 2: capture a copy
    keep(fn() -> i32: p.x) # without the copy: local-reference-escapes
```

## Open questions

1. Approve step 1 as below, rules and plan included? **Recommendation:** yes.
2. `Scope.spawn`'s crossing parameter `fn() -> T + Send & 's` keeps its
   written lifetime in step 1, and moves to `F` in step 2? **Recommendation:**
   yes. `F < Fn() -> T & Send` fails today (`fn() -> i32 does not implement
   Send`), and a closure type that is `Send` by its captures is step 2.
3. The callee fix-it is offered only for a callee in the checked package;
   a std callee gets the copy fix-it alone? **Recommendation:** yes.

## Problems it solves

**1. A `fn` parameter cannot be stored (#2409).** gojq's dispatch table
wraps each function through a helper, `argFunc1(funcHas)`. Today the store
`Function { callback: f }` is `local-reference-escapes`, because `f` might
hold a caller's borrow. After step 1 the store is valid: nothing to track.

**2. Two `fn` parameters cannot make a result.** `compose(f, g) -> fn(..)`
is `ambiguous-reference-source` today: "the result may hold what `f` or `g`
captures". After step 1 a `fn` result depends on nothing.

**3. A stored callback is two words.** A field, a list element or a map
value of type `fn(..)` is 8 bytes (code slot, environment), and a named
function's value is `(slot, 0)`. Layout A makes it 4 bytes.

## How it works

**The rules**, applied to [13 Local References](../../spec/lang/13-values-and-references.md#local-references) and [07 Copied And Shared Captures](../../spec/lang/07-functions.md#copied-and-shared-captures); item 11 below is `fn.capture.fn-static` there, since `fn.capture.fn-static` was retired before:

```markdown
<!-- 13-values-and-references.md, Local References table: replaces ref.local.escape.capture-stored -->
| r[ref.local.escape.fn-capture] Function value capture | is captured by a closure or a bound method reference whose value is a `fn(...)` value, a captured `self` included | `keep(fn() -> i32: p.x)` | `local-reference-escapes` |

8. r[ref.local.fn-static] A `fn(...)` value holds no local reference, so it may be stored, returned and copied wherever its type may.
9. r[ref.local.escape.fn-capture.fix] The diagnostic offers two fix-its: the callee's parameter as `impl Fn(...)`, and a copy of the referent bound before the closure.
10. r[ref.local.escape.fn-capture.fix-callee] The callee fix-it is offered only when the callee is declared in the checked package.
11. r[ref.local.escape.fn-capture.fix-copy] The copy fix-it is offered only when the closure does not write through the reference.

<!-- 07-functions.md, Copied And Shared Captures: replaces items 11 to 14 and 17; local-field and local-generic stay -->
11. r[fn.capture.fn-static] A closure whose value is a `fn(...)` value must not capture a local reference, as the Function value capture row of [Local References](13-values-and-references.md#local-references) states. Error: `local-reference-escapes`.
```

Step 1 (d7469337e) retired thirteen rules and added six; that commit
lists the retired IDs. Their meaning goes: the stored-capture
row, the parameter and origin capture rules, the scoped and stored capture
rules, the dependent-result capture position and elision, and the
local-parameter capture rules. The borrowing forms keep their rules: `ref.generic.outlives.field`
and `.argument`, `fn.capture.local-field` and `fn.capture.local-generic`.

**The representation.** A `fn(..)` value is one `i32`: the payload address
of a closure object, never 0.

| Word | Layout A | Today |
|---|---|---|
| `-8` count | 1+ on the heap; 0 for a static object | the environment's |
| `-4` descriptor | release glue slot, 0 without counted captures | the same |
| `0` code | the code's table slot | (in the value's first word) |
| `4…` captures | as today | from offset 0 |

- **A call** pushes the pointer as the code's first argument, then the
  arguments and the context, then `local.get f; i32.load offset=0;
  call_indirect`. The code's ABI `(env, params…, ctx)` is unchanged; `env`
  is now the object.
- **A capture-free function** (a named function, a method reference, a
  closure that captures nothing) is one static object per code instance,
  deduplicated by its key as vtables are (`Module::vtable`). Same function,
  same address. hd has no `fn` equality today (`expr.is.value-operand`);
  this keeps `is` by address open.
- **Count 0 is never changed.** A `fn` value's count leaf is a known payload
  start (`hd_layout/src/shims.rs:183`), so it takes `RetainKnown` and
  `ReleaseKnown`. Each skips count 0, as the static empty buffer relies on
  (`rt.rs:1152-1162`). Cited in Appendix D.
- **A niche gained:** the pointer is never 0, so `fn(..)?` can use 0 for
  `none` (today `fn(..)` has no niche, `hd_layout/src/lib.rs:670-672`).
- **Fast paths unchanged.** `devirt_known`, `closure_spec`, the
  literal-consumer copies (`hd_opt/src/inline.rs`) and `Target::KnownClosure`
  resolve a `CallValue` on TIR value flow, before any layout exists. A
  known call still calls the code directly, passing the object as `env`.
  The one new cost is the code load of an unresolved call; it is
  loop-invariant, so `loop_loads` (#2374) hoists it.

**Layout B**, finding the code through the descriptor, is the A/B gate's
other arm (Appendix C).

## Cost

Measured at `1290ef6e1` (main `6e89c61fa` merged), release, V8, from a
stage fixture's WAT (Appendix D). Layout A's rows are derived from the
same instructions, not measured.

| Case | Today | Layout A |
|---|---|---|
| call of a `fn(i32) -> i32` field | 11 instructions, 25 bytes, 2 loads | 8 instructions, 19 bytes, 2 dependent loads |
| call of a `fn(i32) -> i32` local | 5 instructions, 11 bytes, 0 loads | 6 instructions, 14 bytes, 1 load (hoistable) |
| a stored callback (field, element) | 8 bytes | 4 bytes |
| a named function's value | `(slot, 0)`, no heap | a 12-byte static object, once per module |
| a closure capturing one `i32` | 8 bytes + a 16-byte block | 4 bytes + a 16-byte block (payload code + `k`) |

A heap closure grows by 4 bytes. With 16-byte size classes
(`lib/std/rt/alloc.hd:375`), that crosses a class only when the captures
take 9 to 12 bytes past a multiple of 16.

**The A/B gate**, the done check for step 1's implementation (not for this
lane): `closures`, `dyn_iter`, `function-value-inputs` and md-reader, in
release on V8 and Wasmtime, A against B against today. It must show no
runtime loss and halved storage for stored callbacks (8 to 4 bytes).

**The census** (Appendix D lists every site): the step-1 check, patched into
the checker, against `lib/std`, the conformance corpus, `../md-reader` and
`../hdjq`.

| Corpus | New errors | Fix |
|---|---:|---|
| `lib/std` bodies | 0 | none; one signature, `thread.scope`, for its callers |
| conformance, 9,051 files | 194 files | 176 by one generator edit, 7 callee edits, 6 `std.thread`, 3 retired-rule fixtures, 2 moved errors |
| md-reader | 0 | |
| hdjq | 0 | |

**Step 1's implementation:** about 130 to 190 lines added and 275 to 395
deleted, net −85 to −265 (component table in Appendix E).

---

## A. Stress test

1. **gojq's dispatch table** (#2409): `map[string]function{argFunc1(funcHas),
   …}` as `Map[string, Function]` built through `arg_func1(f: fn(..))`.
   Works after step 1, with no enum workaround.
2. **`compose(f, g)`**: checks after step 1 (today `ambiguous-reference-source`).
3. **The pointer-loan matrix** (176 generated fixtures): each passes a
   borrowing closure to `apply(.., read: fn() -> string)`. **Found:** the
   generator (`spec/tools/generate-pointer-loan-matrix.ts`) writes
   `read: impl Fn() -> string`, and the runtime loan checks still apply.
4. **Scoped threads:** `scope(fn(s): s.spawn(fn(): point.x).join())`.
   **Found:** `scope` needs `impl FnMut`, and `spawn` needs question 2.
5. **A `&mut self` method reference** (`twice(p::shift, 1.0)` with
   `p: &mut Point`): error; the callee takes `impl FnMut`.
6. **A `let mut` capture** (`xs.each(fn(x): total += x)`): a cell, not a
   local reference. Unchanged.
7. **`self` in a `&self` method**, passed to a `fn(..)` parameter: error with
   both fix-its; std's iteration methods already take `F` (97 bounds).
8. **A static closure in a cycle:** the collector's `visit` finds no block
   for a static address and stops (`lib/std/rt/cycle.hd:169-171`).
9. **A suspending value** (`fn() -> T !`): its code word is the cold
   constructor, as its table slot is today.
10. **A `&mut` capture with the copy fix-it:** the copy would lose the
    writes, so rule `fix-copy` withholds it.
11. **A local closure value** (`f := fn() -> i32: p.x; f()`): an error today
    (the old rule's own example) and still one. The copy fix-it applies;
    step 2 does not change it, since a binding stays `fn(..)`.

## B. Prior art, with code

**Go** (go1.27.3 darwin/arm64, `go build -gcflags=-S`; layout A exactly).
A func value is a pointer to a `funcval` whose first word is the code.

```go
type Mapper struct{ transform func(int32) int32 }
func apply(m *Mapper, x int32) int32 { return m.transform(x) }
// MOVD (R0), R26   load the func value; R26 is the closure context
// MOVD (R26), R2   load the code word at offset 0
// CALL (R2)
func makeNamed() Mapper { return Mapper{transform: named} }
// MOVD $main.named·f(SB), R0   a static funcval, one per function
func makeClosure(k int32) Mapper { return Mapper{transform: func(x int32) int32 { return x * k }} }
// allocates struct { F uintptr; X0 int32 }, stores the code at offset 0
```

hd takes the layout, the static object per function, and the object as
the code's context argument.

**Rust** (rustc 1.97.1). A borrowing closure goes to `impl Fn`, never to a
stored function type: this is step 1's rule.

```rust
fn call_ptr(f: fn() -> i32) -> i32 { f() }
fn call_impl(f: impl Fn() -> i32) -> i32 { f() }
fn demo(p: &Point) -> i32 {
    call_impl(|| p.x) + call_ptr(|| p.x)
    // E0308: expected fn pointer, found closure
}
```

Rust's `fn` pointer is capture-free; hd's `fn(..)` may still capture
copies and counted references. The previous revision's `FnPtr` was Rust's
`fn` pointer; one pointer to a static object makes it unneeded.

**Swift** (swiftc 6.4): a stored (`@escaping`) closure cannot capture an
`inout` parameter; a non-escaping one may.

```swift
func store(_ f: @escaping () -> Int) { kept.append(f) }
func run(_ f: () -> Int) -> Int { f() }
func demo(_ n: inout Int) -> Int {
    let a = run { n + 1 }  // valid
    store { n + 1 }        // error: escaping closure captures 'inout' parameter 'n'
    return a
}
```

hd's `fn(..)` is Swift's `@escaping` always; hd's borrowing form is a
generic parameter, not an attribute. Swift values are two words.

**C++** ([cppreference](https://en.cppreference.com/w/cpp/language/lambda)):
a lambda has a unique closure type; `std::function` erases it (32 bytes
in clang 23, may allocate); a template parameter keeps it.

```cpp
template <class F> int apply(F f, int x) { return f(x); }  // per lambda, may borrow
std::function<int(int)> erased = [k](int x) { return x + k; };
```

## C. Alternatives considered

- **Keep the direct-argument exception:** #2409 and `compose` stay
  errors, and the checker keeps the capture slot. Rejected by the owner.
- **Layout B**, the measured alternative: no code word; the code is found
  through the descriptor. It saves 4 bytes per heap closure, but costs about
  3 instructions per indirect call (load the descriptor at `-4`, mask its
  glue field, add the code's offset) and puts every closure's code into the
  16-bit glue slot range (#1536). Variant to measure with A: a static object
  recognised by address, so a named function's value points into a dense
  code array.

## D. Today in hd

**Census method.** The patch (not committed) made a closure or bound
method reference scoped only for a bare type parameter whose bound is
`Fn`/`FnMut` (`FnWant::scoped`), in the three argument sites of
`hd_check/src/call.rs` (positional, named, trailing). `lib/std`:
`every_std_body_checks_with_no_diagnostic` passes. Corpus: `hd check` on
each `cases.tsv` file with both builds, diffing `local-reference-escapes`
lines. md-reader and hdjq: `hd check src/main.hd` and the tests file, on
copies. Each local reports once per body, so a body may hide a second site.

| Fixture (`spec/conformance/`) | Fix |
|---|---|
| `lang/references/pointer-loans/closure/*` (176) | the generator: `read: impl Fn() -> string` |
| `lang/functions/closures/lifetime-closure-call-loan-lasts-the-call`, `lifetime-closure-call-stores-a-capture` | `fill` takes `impl FnMut` |
| `lang/functions/function-types-and-values/method-references/receiver-method-references` | `twice`, `read` take `impl FnMut`/`impl Fn` |
| `lang/references/boxes/captured-reference-list-reads` | `apply` takes `impl Fn` |
| `lang/references/dependent-results/frozen-loans/frozen-loan-for-scope` | `total` takes `impl Fn` |
| `lang/references/moves/mut-reference-capture-after-move` | `run` takes `impl Fn`, so the move stays the test |
| `std/sync/arc/writes-through-a-mut-field-of-a-get-reference-are-kept` | `each` takes `impl FnMut` |
| `std/thread/a-thread-borrows-a-reference-that-outlives-its-scope`, `a-place-a-thread-borrows-is-not-written-before-its-scope-returns`, `a-thread-may-not-borrow-a-local-of-its-scopes-closure` | `scope` takes `impl FnMut`; `spawn` by question 2 |
| `lang/references/dependent-results/frozen-loans/frozen-loan-blocks-element-store`, `frozen-loan-ends-with-its-lifetime`, `std/arena/an-arena-is-not-lent-to-another-thread` | `spawn`, by question 2 |
| `lang/functions/closures/function-parameter-captures-local-reference`, `function-parameter-stored-in-a-local-through-a-reference`, `lang/references/dependent-results/function-parameter-capture-dependent-return` | fixtures of retired rules: become reject cases of `fn-capture` |
| `lang/references/generic-locality/function-parameter-capture-boxed`, `lang/references/local-references/for-element-references-capture` | still rejected; the error moves to the call, or names the capture |

**Today's calls** (`# @build: v8 release` stage fixture, `hd_wasm` `wat`
view; the fixture is not in the repo). A field call:

```wat
local.get $self
i32.load $heap            ;; code slot
local.get $self
i32.load $heap offset=4   ;; environment
local.set $self
local.set $t0
local.get $self
local.get $x
i32.const 0               ;; context
local.get $t0
call_indirect $table0 (param i32 i32 i32) (result i32)
```

A local `f(x)`: `local.get $f.1; local.get $x; i32.const 0; local.get
$f.0; call_indirect`. A named function's value: `i32.const 2; i32.const
0`. A closure capturing `k`: `rt:new/class-1` with 4 payload bytes, then
`i32.store`.

**Count 0 is skipped**, at every layer a `fn` value's count reaches:

- `lib/std/rt/alloc.hd:276-282`, `retain_known`: `if count != 0` before the
  increment.
- `lib/std/rt/alloc.hd:299-305`, `release_known`: `if count == 0: return 0`.
- `crates/hd_wasm/src/opt.rs:5097-5107`, `inline_counts`' inline retain:
  `i32.eqz; br_if 0` on the loaded count; its release decrements only above
  1 (`5108-5143`), and else calls `ReleaseKnown`.
- `crates/hd_wasm/src/count_tests.rs:120-132`: the test before a known count
  call loads the count and calls only when it is not 0 (or `Tracing`).
- `crates/hd_wasm/src/emit/count.rs:504-533`, `count_locals_inner`: a known
  address, as a `fn` value's is, emits `RetainKnown` or `ReleaseKnown`
  (or `ReleaseFresh`, for a block this body allocated, never a static one).

## E. Details

- **Spec:** 07 and 13 as in How it works; `std/thread.md`
  `std-thread.spawn.borrow` follows question 2. A retired ID is deleted and
  named in the commit (`spec/STYLE.md` Stability).
- **Diagnostics:** `local-reference-escapes` at the closure, naming the
  captured reference, with the fix-its of rules `fix`, `fix-callee`,
  `fix-copy`.

**Step 1's implementation** (lines estimated from the sites named):

| Crate | Files | Added | Deleted | What |
|---|---|---:|---:|---|
| `hd_check` | `call.rs`, `trial.rs`, `fnref.rs` | 5 | 40 | the direct `fn(...)` argument exception |
| `hd_check` | `dep_results.rs`, `dep_results/constraints.rs`, `regions.rs` | 0 | 200 to 300 | a function type's capture slot: `capture_elided`, `FieldCapture`, the #2409 message, the capture ambiguity |
| `hd_check` | `local_refs.rs` | 40 to 60 | 0 | the two fix-its |
| `hd_layout` | `lib.rs`, `shims.rs` | 5 | 5 | `Fn` is one non-null address; count leaf 0 |
| `hd_wasm` | `emit/closure.rs`, `emit.rs`, `module.rs`, `rt.rs` | 80 to 120 | 30 to 50 | the code word, static objects, the call's load, `rt:collect`'s three hooks |
| `lib/std` | `thread.hd` | 2 | 2 | `scope` takes `impl FnMut` |
| **Total** | | **130 to 190** | **275 to 395** | **net −85 to −265** |

It deletes today's `(code, env)` pair (two words in `hd_layout`, the
`(slot, 0)` builds in `emit.rs` and `rt.rs`), the environment-0 case of
every capture-free value, and the direct-argument exception with the
capture slot behind it. Cost per run: the check is a constant per closure
argument, less than today's origin tracking; static objects cost one hash
lookup per function value, linear in the values a module names.

- **Phases of step 1** (each about an hour, each with its test):
  1. the check and the fix-its, with the 194 fixtures and the generator,
     deleting the exception (conformance `lang/functions`, `lang/references`,
     `std/thread`);
  2. the capture slot's deletion in `dep_results` (the #2409 and `compose`
     cases as accept fixtures);
  3. layout A: the code word, static objects, the call's load (stage cases
     for a field call, a local call, a named value, `fn(..)?`);
  4. the A/B gate report: A, B and today on the four programs.
- **Steps 2 and 3** keep the previous revision's design: a closure literal
  at a bare `F` gets its own type, implementing `Fn` or `FnMut` by
  `fn.trait.read-only.*`, `Copy` when its captures are; #1449 then turns its
  `let` cells into frame slots when its holder never leaves the frame. Both
  are priced when step 1 lands.
