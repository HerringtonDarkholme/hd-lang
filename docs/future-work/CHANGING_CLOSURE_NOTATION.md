# Changing Closures: A Notation

This is a proposal for issue #163. Nothing in it is decided. Every
spelling and diagnostic name is a placeholder, and the hd code in `text`
blocks is proposed syntax that no compiler checks. Ownership is on hold
(see the note at the top of [Ownership And Borrows](OWNERSHIP_DESIGN.md));
this proposal does not depend on it.

## 1. Motivation

hd's rule of thumb is that `mut` marks where values change. Function
values break it. Today any closure may assign captured `let` storage or
change a captured `mut T` value, and its type is still a plain `fn`
([Functions: Captures](../../spec/lang/07-functions.md#captures),
`fn.capture.no-mut-form`). So a readonly path can change state, which a
readonly data path otherwise cannot (`types.mut.mut-field`):

```text
data Counter:
    tick: fn() -> void

fn poke(c: Counter) -> void:     # `c` is readonly
    (c.tick)()                   # ...and this changes `count`

pub fn main() -> void $ Console:
    let count = +0
    c := Counter { tick: fn() -> void: count = count + 1 }
    poke(c)
    poke(c)
    println("${count}")          # prints 2 with today's compiler
```

The owner rejected `mut fn` as the fix. On data, `mut` is a permission:
`mut T <: T`, and a `mut` view can always be used as a readonly one. On a
function it would be an effect: a function that changes nothing can
always stand in for one that may change something, so `fn <: mut fn`.
One word with two opposite subtyping directions is worse than two words.
This proposal compares notations for the effect and recommends one.

## 2. The Model

One idea: **a function type says whether calling it may change state
that the function captured.**

- A plain `fn` changes nothing it captured. It may still read captures,
  change its own parameters through `mut` parameter types, and use
  providers through its row.
- A **changing** function may also assign captured `let` storage, change
  a captured `mut T` value (a `mut self` call, a field assignment, passing
  it to a `mut T` parameter), or call a captured changing function.
- Every plain function is accepted where a changing one is expected. The
  reverse is a type error.
- A changing function stored in a data field cannot be taken out through
  a readonly path. That closes the hole in section 1.

Providers are not captures: a closure that calls a `mut` trait method of
a provider already says so through its row, so the marker covers
captured state only. Top-level `let` bindings that named functions
assign are also out of scope; they are not captured.

## 3. The Options

Each option is shown on the same three things: `List.map`, `retry!`, and
a literal that counts calls.

**A. An effect word after the return type.**

```text
pub fn map[U](self, transform: fn(T) -> U mutates) -> List[U]
pub fn retry![T, E, $R](times: usize, when: fn(E) -> bool mutates = fn(error: E): true, attempt: fn!() -> Result[T, E] $ R mutates) -> Result[T, E] $ R
counted := xs.map(fn(x: i32) -> i32 mutates: calls = calls + 1; x * 2)
```

It reads as English and groups with `$ R` as "what calling it does". But
it lands at the far end of the type, after a possibly long result and
row, where a reader scanning the parameters misses it. `$ R mutates`
looks as if `mutates` were a row key. And the word grows every std
callback signature by nine characters.

**B. A sigil on the `fn` head, beside `!`.**

```text
pub fn map[U](self, transform: fn~(T) -> U) -> List[U]
pub fn retry![T, E, $R](times: usize, when: fn~(E) -> bool = fn(error: E): true, attempt: fn~!() -> Result[T, E] $ R) -> Result[T, E] $ R
counted := xs.map(fn~(x: i32) -> i32: calls = calls + 1; x * 2)
```

It sits where `!` sits, at the head, so both call-protocol marks are seen
first. It costs one character in signatures that std repeats dozens of
times. Its cost is one more sigil to learn, and `~` says nothing by
itself; a diagnostic with a fix-it has to teach it. The order with `!`
must be fixed: `fn~!` keeps `!(` together, as at a bang call site
`attempt!()`, and avoids `!~`, which reads as a "does not match" operator
in Perl and Ruby. `~` is bitwise complement only in prefix expression
position, so `fn~` does not clash.

**C. A requirement-row entry.**

```text
pub fn map[U](self, transform: fn(T) -> U $ Mutate) -> List[U]
pub fn retry![T, E, $R](times: usize, when: fn(E) -> bool $ Mutate = fn(error: E): true, attempt: fn!() -> Result[T, E] $ R + Mutate) -> Result[T, E] $ R
counted := xs.map(fn(x: i32) -> i32: calls = calls + 1; x * 2)   # `Mutate` inferred
```

This reuses machinery: closure rows are inferred
(`req.row.omitted.closure-row`) and row subsumption already gives the
right subtyping (`req.row.subsume`). Effect-row languages work this way.
But a row lists requirements that a `$.with` provides. Nobody provides
`Mutate`, so it would be a key with its own rules. Worse, row variables
carry it outward: `fold[A, $R]` passes the step's row to its own call, so
a public function that folds with a counting closure would have to
declare `$ Mutate` itself, although it changed only its own local. Koka
solves that by discharging local state at the scope that owns it. That
is a region system, which is far more than this problem needs. And an
inferred mark is not visible on the literal, which is the point.

**D. A capture list on the literal only.**

```text
counted := xs.map(fn(x: i32) -> i32 with mut calls: calls = calls + 1; x * 2)
```

C++ lambdas name their captures, and `mutable` lets a by-copy capture
change. A capture list says exactly which captures change, which no
other option does. But it marks only the literal. The type stays `fn`, so
`poke` in section 1 still changes state through a readonly path: the
hole is in the type, and D does not reach the type. It could be added to
B later; it does not replace it. `with` also collides in reading with
`$.with`.

**E. Radical: no changing captures.** A closure could read captures but
never change them, as Java requires captured locals to be effectively
final, and here also no capture of a `mut` view. Changing state would go
through named data with `mut self` methods. Then no marker is needed at
all. But every `Iterator::from_fn` step in std changes captured state
(section 5), so every lazy adapter and every range iterator would become
a named data type with a step method. That is a large rewrite of std for
a rule users would hit at every call counter and every `results.push`.

**F. Mark the restriction instead.** Keep `fn` as "may change" and add a
mark for "changes nothing", as Swift's `@Sendable` forbids capturing
mutable variables and C++23 `std::move_only_function<void() const>` asks
for a const call. This changes no existing code. But the change itself
stays unmarked, so it does not serve "mut marks where values change".

| | Visible on literal | Visible in type | Fixes readonly path | Signature cost | New machinery |
| --- | --- | --- | --- | --- | --- |
| A word after result | yes | yes, at the end | yes | +9 chars | contextual keyword |
| B sigil `fn~` | yes | yes, at the head | yes | +1 char | sigil, two constructors |
| C row key `Mutate` | no (inferred) | yes | yes | +9 chars | key with own rules, leaks through `$R` |
| D capture list | yes, per capture | no | no | none | literal syntax |
| E no changing captures | n/a | n/a | yes | none | rewrites std iterators |
| F mark the restriction | no | yes | partly | on few sites | none |

## 4. Recommendation: B, `fn~`

B is the only option that marks both the literal and the type, at the
place a reader looks first, at a cost of one character in the signatures
std repeats most. It composes with `!` (`fn~!`), with `$ R` (untouched,
at the end), and with the local mode of
[Parallelism And Locality](PARALLELISM_AND_LOCALITY.md) (`^fn~`). It
follows the `!` precedent: a frequent call-protocol property is a sigil
on the head, and agents already handle `fn!`.

### How std reads

A callback that std calls during the call, one call after another, takes
`fn~`: it then accepts both kinds, as Rust's eager adapters take `FnMut`.

```text
pub fn map[U](self, transform: fn~(T) -> U) -> List[U]
pub fn filter(self, keep: fn~(T) -> bool) -> List[T]
pub fn sorted_by(self, compare: fn~(T, T) -> Ordering) -> List[T]
pub fn fold[A, $R](mut self, initial: A, step: fn~(A, T) -> A $ R) -> A $ R
pub fn retry![T, E, $R](times: usize, when: fn~(E) -> bool = fn(error: E): true, attempt: fn~!() -> Result[T, E] $ R) -> Result[T, E] $ R
pub fn timeout![T, $R](limit: Duration, work: fn~!() -> T $ R) -> Result[T, TimedOut] $ R + Clock
pub fn it_each[A, T < Termination, $R](name: string, rows: List[A], ..., body: fn~!(A) -> T $ R) -> void $ R

pub data Iterator[T]:
    step: fn~() -> T?                         # `next(mut self)` already needs a mut path
pub fn from_fn(step: fn~() -> T?) -> mut Iterator[T]
```

A plain `fn` parameter stays where std needs the promise: a callback that
may run in parallel later, or one stored where readonly code calls it,
such as the derive accessors in `std.structure`. A plain `fn` in a
signature then means something a reader can rely on.

### What a literal writes

```text
let calls = +0
doubled := xs.map(fn~(x: i32) -> i32:
    calls = calls + 1
    x * 2)

total := xs.fold(0, fn(sum: i32, x: i32) -> i32: sum + x)   # changes nothing: plain
```

A literal whose body changes a capture and lacks `~` is an error with a
fix-it that adds it (placeholder `closure-changes-capture`). A local
named function that changes a capture carries the mark on its name, as
`!` does: `fn bump~() -> void:`. A literal written with `~` that changes
nothing is allowed; its type is just wider.

### Non-escaping closures

The two marks are independent: `^` says where a closure may go, `~` says
what calling it does. The four types are `fn`, `fn~`, `^fn` and `^fn~`;
`fn` is accepted everywhere, and `^fn~` accepts all four.

[Parallelism And Locality](PARALLELISM_AND_LOCALITY.md) section 3.2
spells a local closure that captures a `^mut` value `^mut fn`, with
`^fn` accepted where `^mut fn` is expected. That is the same
effect-direction subtyping the owner rejected for `mut fn`, so under this
proposal it becomes `^fn~`: changing a `^mut` capture is a change like
any other. Its rule that such a closure's calls run one at a time stays
with that study. One consequence for it: `map_all!` would take plain
`^fn!` work to allow parallel calls, which rejects work closures that
change plain captures too, and today's `map_all!` fixture passes one
(`measured!(gauge, id)` with a captured `mut Gauge`). Whether `map_all!`
takes `fn~!` while children are cooperative is a std call for that job.

### Prior art

| Language | Mechanism | What this takes |
| --- | --- | --- |
| Rust | `Fn`, `FnMut`, `FnOnce`; a closure's kind is inferred from its body; calling `FnMut` needs `&mut`; `Fn` is a subtrait of `FnMut`; eager std adapters take `FnMut` | the subtyping direction, and `fn~` on eager callbacks; not inference |
| Koka, Flix | effect rows inferred on functions; local state is an effect discharged at its scope (Koka `st<h>`, Flix regions); Flix writes effects after the result, `a -> b \ ef` | the comparison for options A and C |
| Swift | closures change captured `var`s freely; `@Sendable` closures may not capture mutable variables | the "mark the restriction" option F |
| Kotlin | lambdas change captured `var`s freely through boxes; no mark | the status quo hd has today |
| C++ | capture lists, `mutable` lambdas; C++23 `move_only_function<R() const>` puts const-callability in the type | option D, and evidence that the mark belongs in the type |
| Scala 3 | experimental capture checking: `A -> B` is pure, `A => B` may capture anything | a mark on the arrow; rejected here, since `->` is hd's result arrow |

## 5. Migration

The counts come from a scan of closure literals with a heuristic script.
It misses a change made by passing a captured `mut T` binding to a `mut T`
parameter (the `map_all!` worker closures, for one), so they are lower
bounds.

- **std closures:** 34 of 50 closure literals change captured state. Most
  are `Iterator::from_fn` steps: the lazy adapters in `iter.hd`, the range
  iterators in `ops.hd`, the character iterators in `text.hd`, and two in
  `collections.hd`. Each gains `~`.
- **std signatures:** 37 public functions take a callback; nearly all
  become `fn~` (the `List`, `Iterator`, `Option` and `Result` adapters,
  `retry!`, `retry_with!`, `timeout!`, `it_each`, `it_prop`), plus the
  `Iterator.step` field. Callers break nowhere, since a plain `fn` fits
  a `fn~` parameter.
- **Fixtures:** about 25 of 344 closure literals change captures, in
  about 20 files: call counters in the `map` and `and_then` tests, the
  closure-capture typing cases, and the iterator tests. Each gains `~`.
  A few new cases cover the readonly-path rule and the missing-mark
  error.
- **Examples and website:** none of 14 closures change captures.
- **Spec:** "Mutable Closures" in Functions is rewritten
  (`fn.capture.no-mut-form` goes), `fn.type.no-permission` gains the
  readonly-path rule, and the sugar table gains two constructors for
  `fn~` and `fn~!`, siblings of `Fn` and `SuspendFn` (names to choose).
  `fn.type.no-mut` stays: `mut fn` is still an error.
- **Compiler:** a per-body check that a capture is changed, in the same
  walk that already decides captures; two constructors and the
  subtyping; one readonly-path check at field reads. No inference crosses
  a function.

## 6. The Rules In Plain Words

1. A function type, a closure literal and a local named function may be
   marked changing with `~`: `fn~(A) -> O`, `fn~!(A) -> O $ R`,
   `fn~(x: A) -> O: ...`, `fn bump~() -> void:`.
2. A closure or local named function that assigns captured storage,
   changes a captured `mut T` value, or calls a captured changing function
   must be marked changing.
3. A plain function value is accepted where a changing one is expected,
   with the same inputs, output and row. The reverse is a type error.
4. A data field whose declared type is a changing function cannot be read
   through a readonly path. A generic field follows
   `types.mut.generic-field`, as `mut U` does.
5. `^` and `~` combine freely; `^fn~` accepts every function value whose
   inputs, output and row fit.
6. Provider use is shown by the row, not by `~`.

## 7. Questions For The Owner

1. **Spelling.** Mark a changing function with a `~` on the `fn` head,
   written `fn~(A) -> O`, `fn~!() -> O $ R` and `fn~(x: A) -> O:` on a
   literal, with `~` before `!`? Recommended: yes. It marks both the type
   and the literal at the head, costs one character in the signatures std
   repeats most, and follows the `!` precedent; options A, C, D, E and F
   are weighed in section 3.
2. **Std callbacks.** Should std callbacks that are called during the
   call, one call after another (`map`, `filter`, `sorted_by`, `fold`,
   `retry!`, `timeout!`, `it_each`, `Iterator::from_fn`), take `fn~`, with
   plain `fn` kept for callbacks that may run in parallel or are called
   through readonly data? Recommended: yes. It keeps every existing caller
   working, as Rust's `FnMut` adapters do, and a plain `fn` in a signature
   then promises something.
3. **Captured `let` storage.** Does assigning a captured `let` binding make
   a closure changing, as changing a captured `mut T` value does?
   Recommended: yes. The call site cannot see either change, and most
   changing closures in std (the iterator steps) change only `let`
   counters.
