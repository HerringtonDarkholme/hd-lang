# Function Values: A Capture-Free `fn` And Counted Closures

Proposal, 2026-11-21; domain: language (function types, closures), std,
backend ABI; not accepted. Issues #1436 (Route A) and #1449 on top.

## The proposal

Split today's one function value into three forms, as Rust does:

- `fn(i32) -> i32` is a **capture-free** function: one word, a table
  index, never counted.
- Each closure literal has its **own anonymous type** that implements
  `Fn`/`FnMut`. A generic `F < FnMut(i32) -> i32` takes it by value, and
  the call is direct.
- A capturing closure stored or shared at run time is written
  `*mut dyn FnMut(i32) -> i32`: counted, the cost visible in the type.

```text
struct Mapper:
    transform: fn(i32) -> i32                      # capture-free, one word

struct Filter[I, F < FnMut(&I) -> bool]:
    keep: F                                        # one instance per closure

struct Handlers:
    on_event: List[*mut dyn FnMut(i32) -> void]   # counted, stored

fn scale(k: i32) -> *mut dyn FnMut(i32) -> i32:
    box(fn(x: i32) -> i32: x * k)                  # captures k: boxed
```

#1449 then decides a closure's environment from escape on the instance:
a closure that never reaches a `*mut dyn` keeps its captures in place, with
no environment object.

## Open questions

1. Accept Route A, the three forms above? **Recommendation:** yes.
2. Does a capture-free closure literal coerce to `fn(...)`, as rustc's
   `ClosureFnPointer`? **Recommendation:** yes.
3. A capturing closure where `fn(...)` is expected: an error with a fix-it
   (`F < FnMut(..)` or `box(..)`), or an implicit box? **Recommendation:**
   the error; a box is a cost the type should show.
4. Does `*mut dyn FnMut(..)` need sugar? **Recommendation:** no; it is
   rare in std (Appendix D census).
5. Ship #1449 in the same batch? **Recommendation:** no; after Route A
   lands, as its own step.

## Problems it solves

**1. A field read takes a count it never needs.** Today `fn(i32) -> i32`
is two words, (code, environment), like a `*mut dyn FnMut`. Reading
`self.transform` copies both and, in the checker's TIR, takes a count
(`Read copy` then `Drop`, #1436); `dyn_iter` made about 200M such calls
that found no environment. With Route A the field is one word and its read
is a load.

**2. Every closure shares one shape.** `Filter`'s `keep` is the structural
`fn(...)` type, so `MapIter`/`Filter` have one instance that calls through
the field, and no closure is inlined (#1449, 2026-11-07). With a type per
closure, `Filter[I, F]` gets an instance per closure, and the call is
direct, as rustc's `apply_generic::<{closure}>` loads the capture in place
(Appendix B).

**3. Capture cost is invisible.** `fn scale(k) -> fn(i32) -> i32` returns
an allocated environment today with nothing in the type. Route A writes it
`*mut dyn FnMut(i32) -> i32`, which is what it costs.

## How it works

**Types.** `fn(A) -> R` keeps its spelling and loses its environment. A
closure literal's type is fresh and unnameable, like a rustc closure; it
implements `Fn` when it only reads captures, else `FnMut`. `Fn` and
`FnMut` already exist in `std.function`, with `F < Fn() -> bool` bounds in
use (41 bounds in std today).

**Coercions** (checker, at an expected type):

| From | To | When |
|---|---|---|
| named function, method reference, one-field variant | `fn(..)` | always (today's `fn.type.named-value`) |
| closure literal | `fn(..)` | it captures nothing |
| closure literal | `F < Fn/FnMut(..)` | always: `F` is its type |
| `box(closure)` | `*dyn Fn(..)` / `*mut dyn FnMut(..)` | always (today's trait-value boxing) |
| `fn(..)` | `F < Fn(..)` | always (`fn` implements `Fn`) |

A capturing closure where `fn(..)` is expected is `closure-captures`, with
two fix-its: make the parameter generic, or box the closure.

**Representation and ABI** (`hd_wasm`):

| Form | Value | Call |
|---|---|---|
| `fn(..)` | `i32` table index | `call_indirect (args)`: no environment, no ctx word |
| closure type | struct of its captures, by value | direct call of its body with the captures |
| `*mut dyn FnMut(..)` | counted object: captures + code | load code, `call_indirect (env, args)` (today's shape) |

**Lifetimes.** A `fn(..)` holds no origin, so the capture-position rules
for `fn(..)` parameters (`ref.dep.capture-position`,
`ref.local.capture-parameter`) move to generic parameters, where
`ref.generic.outlives.*` already decides what a type argument may hold.

**#1449 on top.** With a closure type per literal, the instance knows
whether a closure escapes: one that never reaches a `*mut dyn` or a
stored field passes its captures in place, and needs no environment
object or capture counts.

## Cost

Bottom-up, against main 657cbff56. Lines are estimates by analogy with the
existing trait-value and generic machinery; bytes are measured where
stated.

| Component | Lines | Notes |
|---|---:|---|
| Checker: closure types, `Fn`/`FnMut` impls, coercions, `closure-captures` | +350 to +500 | the largest part |
| Lifetimes: capture positions move to generic params; delete `fn` capture rules | -150 to -50 | `dep_results` capture-position code goes |
| Mono: one instance per closure type | +50 to +100 | instance keys already include type args |
| Wasm: one-word `fn`; `*mut dyn FnMut` object | +100 to +150, -100 | env word and ctx argument leave every `fn` call |
| Std migration (Appendix D) | about 110 type sites | fields and params become `F <` or `*mut dyn` |
| Spec (07, 13) and conformance | ~25 rules touched | Appendix E |
| #1449 (later) | +150 to +250 | escape on the instance |

**Bytes and time.** A `fn` call drops two arguments (environment and ctx,
about 4 bytes) and a field read drops one load (about 3 bytes): measured on
today's `Mapper.apply` (release WAT: 2 loads, 4 call arguments). The bench
has 27 to 54 `call_indirect` per module, some of them trait calls, so the
one-word `fn` is worth at most about 1% per module. Instances per closure
add code where generic adaptors take closures; the 4 capturing closures in
the bench all go to `map`/`filter` (#1449), so `iter-pipeline` grows by a
few instances and gains direct, inlinable calls. Compile time rises with
instances, linearly in the closure literals. Per-run cost: each closure
literal is one type and at most one instance per generic it reaches; no
pass iterates.

---

## A. Stress test

1. **`std.iter` adaptors** (`map`, `filter`, `fold`): today `keep: fn(&I)
   -> bool`; becomes `F < FnMut(&I) -> bool`. Every adaptor gains a type
   parameter; call sites are unchanged. Works.
2. **`std.testing` generators** (`gen: fn(&mut Choices) -> T`, stored in
   structs and combined): generators built from captured config would no
   longer fit `fn`. They become `*mut dyn FnMut(&mut Choices) -> T`, or the
   struct goes generic. Found: 7 fields here, the densest std module.
3. **An event registry** (a `List` of callbacks, JS `addEventListener`):
   `List[*mut dyn FnMut(Event) -> void]`, each pushed as `box(fn ...)`.
   Works; the box is now written at each registration.
4. **A recursive closure** (`fn.closure.recursive-result`): its type names
   itself. Rust rejects this; hd's closure must then be a local function
   or go through `*mut dyn`. Found: needs an owner rule (question 3's
   error covers it).
5. **A closure capturing a local reference passed straight to a `fn`
   parameter** (`ref.local.capture-parameter`): legal today. With Route A
   the parameter must be `F < FnMut(..)`, and `ref.generic.outlives` lets
   the type argument hold the reference for the call. Works, through a
   rule that already exists.
6. **A suspending closure** (`fn!(..)`, `SuspendFn`): the same split
   applies to `SuspendFn`; a stored suspending callback is `*mut dyn
   SuspendFn(..)`. Needs the same coercion table.
7. **Variance** (`fn.type.declared-variance`): unchanged for `fn(..)`;
   a closure type is invariant like any nominal type.

## B. Prior art, with code

**Rust** (rustc 1.97.1, `rustc -O --emit=llvm-ir,mir`): a capture-free
closure coerces to `fn` (`ClosureFnPointer`); `Box<dyn FnMut>` allocates;
a generic `F` is monomorphized and inlined.

```rust
pub struct Mapper { pub transform: fn(i32) -> i32 }          // LLVM: one `ptr`
let c = |x: i32| x + 1;  let m2 = Mapper { transform: c };   // MIR: ClosureFnPointer
let d = Box::new(move |x| x + k) as Box<dyn FnMut(i32) -> i32>; // {ptr, vtable}, __rust_alloc
fn apply_generic<F: FnMut(i32) -> i32>(mut f: F, x: i32) -> i32 { f(x) }
// apply_generic::<{closure}>: `%_3.i = load i32, ptr %0` (the capture, in place)
```

hd takes all three forms. Source: the probe's MIR and LLVM IR.

**Swift** (swiftc 6.4, `-O -emit-sil`): every function value is "thick",
(code, refcounted context), like hd today; a stored capturing closure is a
`partial_apply` released later; a non-escaping parameter closure is
specialized away.

```swift
struct Mapper { var transform: (Int32) -> Int32 }       // thick: code + context
let m = Mapper(transform: double)  // SIL: thin_to_thick_function, no retain
let n = Mapper(transform: { $0 + k })  // SIL: partial_apply, then strong_release
func applyNow(_ f: (Int32) -> Int32, _ x: Int32) -> Int32 { f(x) }
// applyNow specialized to (Int32, Int32): closure context passed as an argument
```

hd rejects the thick default (problem 1) and takes Swift's non-escaping
specialization as #1449's goal.

**Go** stores every `func` value as a pointer to a closure record, heap or
stack by escape analysis; one shape, cost hidden. hd takes Go's escape
analysis (#1449), not its single shape.

## C. Alternatives considered

- **Keep one shape, skip counts at call sites** (today's `count_tests`):
  already done; it removes the dynamic cost but not the second word, the
  shared instance, or the hidden allocation.
- **Swift's thick functions with a null-context fast path:** same two
  words; inlining still needs a type per closure.
- **Implicit box when a capturing closure meets `fn(..)`:** fewer edits,
  but the allocation stays invisible (question 3).

## D. Today in hd

- `fn(..)` is (table index, counted environment), environment 0 when
  nothing is captured (`crates/hd_wasm/src/emit/closure.rs`). Release WAT
  of `(self.transform)(x)`: two loads, then `call_indirect` with
  (environment, x, ctx). A capturing closure literal calls
  `rt:new/class-1` for its environment.
- Census of `lib/std` (rg over `*.hd`, 657cbff56): 161 `fn(` mentions,
  149 of them types: 36 struct fields, about 62 parameters, 3 results,
  14 inside type arguments; 16 closure literals; 41 `Fn`/`FnMut` bounds.
  Densest: `http` (29), `collections` (27), `iter` (15), `format` (14).
- Spec: `fn.type.values`, `fn.type.named-value`, `fn.capture.*`,
  `ref.dep.capture-position`, `ref.local.capture-parameter` describe the
  one shape.

## E. Details

- **Spec (07, 13):** `fn.type.form` gains "captures nothing"; new
  `fn.closure.type` (a type per literal), `fn.closure.coerce-fn`,
  `fn.closure.captures-error`; `ref.dep.capture-*` and
  `ref.local.capture-parameter` move to the generic rules or go.
- **Checker:** closure type identity per literal and instance; `Fn`/`FnMut`
  impls by capture use; the coercion table; the fix-its.
- **Ownership:** `fn` values are never counted; closure types count their
  captures as struct fields do; `*mut dyn FnMut` as a trait value.
- **Diagnostics:** `closure-captures` at the closure, naming the first
  capture and the expected `fn` type.
- **Migration:** the compiler's fix-its rewrite std's 110 type sites; a
  conformance pass per spec rule.
- **Phases** (each about an hour, each with its test): closure types and
  coercions in the checker; the error and fix-its; mono instances; the
  one-word `fn` ABI; `*mut dyn FnMut` objects; std migration by module;
  spec and conformance; bench report. #1449 follows separately.
- **Parse log:** the one `text` block parses with
  `scripts/hd-conformance.sh parse` (syntax only; `FnMut` needs `use
  std.function.FnMut` to check).
