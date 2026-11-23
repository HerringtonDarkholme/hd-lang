# Arena As An Effect

**Status:** design exploration, 2026-11-23; nothing here is decided or in
the specification. The question comes from the owner on
[#2326](https://github.com/HerringtonDarkholme/hd-lang/pull/2326) ("is it
possible to make Arena an Effect instead? so it does not need pass stuff
around"). It builds on [ARENA.md](https://github.com/HerringtonDarkholme/hd-lang/blob/design/1410-arena/docs/future-work/ARENA.md) (PR #2326) (the untyped `Arena` and its
`@disjoint` primitive) and on spec chapter 11
([Requirements](../../spec/lang/11-requirements-and-suspension.md)).

## Contents

1. The Domain
2. Problem, and What hd Has Today
3. Survey
4. Options: provider-scope lifetime; implicit borrow parameter; counted
   handles; callback-scoped `&mut`; explicit parameter
5. Comparison
6. Recommendation
7. Questions For The Owner
8. Sources, and Parse Log

## The Domain

An arena hands out memory that dies all at once. The hard part is never
the bump; it is proving that no value outlives the arena. Rust and Zig
make every function that allocates take the arena as a parameter, so the
borrow checker (Rust) or the programmer (Zig) sees the lifetime. Go's
proposal stalled on exactly that: "Using an arena explicitly at all means
that it is going to be infectious to some degree" (Go #51317).

An effect system offers the other road: a function names the arena in its
row, as `$ Console` names a console, and the caller installs it once.
The catch is the lifetime. A value from `$.use(K)` today is a local
reference that "is never stored, returned, or captured"
([`req.use.value.local`](../../spec/lang/11-requirements-and-suspension.md#r-req.use.value.local)),
so a helper cannot return the node it allocated. Region systems (Effekt,
Koka, Cyclone) solve this with a lifetime tied to a scope, not to a value.

## Problem, and What hd Has Today

The question: can `alloc` come from the row, `$.use(Arena).alloc(v)`, with
no arena passed, and keep ARENA.md's guarantees (results disjoint and
writable, none outliving the arena)?

Use cases, fixed for every option:

| # | Use case |
| --- | --- |
| U1 | `binary_trees`' `make(depth)`, which returns the nodes it allocates |
| U2 | a parser helper returning `&mut Expr` that a later pass patches |
| U3 | a request handler whose deep callees allocate into one frame arena |
| U4 | a scratch arena nested inside a request arena |
| U5 | an async handler that allocates across `!` calls |

What blocks it today, in chapter 11:

| Rule | Effect on an arena provider |
| --- | --- |
| `req.key.traits` | a key is a trait, so `$.use` yields `&mut dyn K` (`req.use.value.ordinary-type.reference-values`), and a generic `alloc[T]` cannot be called on a trait value (owner decision: no generic calls on `dyn`) |
| `req.use.value.local` | the provider is a local reference: `make` cannot return what it allocates |
| `req.use.value.outlives-scope` | a counted provider outlives its scope, the opposite of freeing at the block's end |
| `req.mut.install-no-local` | `$.with(K = &arena)` is `local-reference-escapes`, so an installer cannot lend its own arena |
| `req.with.nearest`, `req.with.dynamic` | the provider is chosen where a call happens, so one closure can see two arenas |
| `req.bind.construction` | a suspension binds its providers when it is built |

Two probes on main at 363420be2, with ARENA.md's
untyped probe module:

- A non-generic `bump(size, align)` on a `dyn` trait, wrapped by a std
  generic `alloc_in[T](arena: &dyn Bump, val: T) -> &mut T`, checks and
  runs. On `binary_trees` it costs **209 ms against 192 ms** for the
  concrete call (V8 release, median of 21, empty program 87 ms): **16%
  more work**, one indirect call per node that nothing devirtualizes.
- **A soundness hole:** `fn leak['a]() -> &'a i64 $ Holder:
  $.use(Holder).slot()` checks, and a caller that returns it from inside
  `$.with(Holder = box(...))` reads freed memory (prints `41 41`). The
  elided form is rejected. So today's checker already lets a `$.use`
  result escape through a free result lifetime; any option here must close
  it. It is filed with a known-failure test.

## Survey

| Language | How an allocation finds its arena | Lifetime check | Source |
| --- | --- | --- | --- |
| Rust bumpalo | a `&'a Bump` parameter; `alloc(&self, val: T) -> &mut T` | borrow checker; `reset(&mut self)` needs no borrows active | [bumpalo](https://github.com/fitzgen/bumpalo/blob/main/src/lib.rs) |
| Zig | an `Allocator` parameter; `ArenaAllocator` wraps one | none: the programmer frees | [arena_allocator.zig](https://github.com/ziglang/zig/blob/master/lib/std/heap/arena_allocator.zig) |
| Go arenas | `arena.New[T](a)` | a fault "typically"; proposal "on hold indefinitely due to serious API concerns" (2023-01-17); "infectious" | [#51317](https://github.com/golang/go/issues/51317) |
| Odin | the implicit `context`, "implicitly passed by pointer to any procedure call"; `context.allocator = ...` for a block | none: "no automatic memory management"; `free_all(context.temp_allocator)` | [overview](https://odin-lang.org/docs/overview/#implicit-context-system) |
| Scala 3 | `using` parameters; capture checking tracks capabilities, `T^{c}` | experimental: an escaping closure is "capability f cannot be included in outer capture set" | [capture checking](https://docs.scala-lang.org/scala3/reference/experimental/capture-checking/basics.html) |
| Koka | `ref<h,a>` in heap `h`, effect `st<h>` | `run(action : forall<h> () -> <alloc<h>,read<h>,write<h>|e> a) : e a` erases an unobservable heap | [std/core/types](https://koka-lang.github.io/koka/doc/std_core_types.html) |
| Effekt | `region r { var x in r = 1 }` | capture sets: a closure over `x` "may only ever be unboxed where region `r` is still in scope" | [regions](https://effekt-lang.org/tour/regions) |
| Cyclone | lexical `{ region r; ... }` and dynamic regions | static: "each pointer type has a region", and no run-time liveness test | [regions](https://cyclone.thelanguage.org/wiki/Introduction%20to%20Regions/) |
| MLKit | inferred: "allocation and deallocation ... are inferred by the compiler" | region inference, with tracing GC beside it | [MLKit](https://elsman.com/mlkit/) |
| Silk | the allocator is a service, `Effect.provideMut<Allocator>(&mut arena)` | "An allocation never borrows its arena": each chunk counts its live allocations; a provider "stays borrowed until the Effect is consumed" | [arena](https://silklang.org/docs/std/mem-arena), [services](https://silklang.org/docs/reference/services-and-providers) |

Takeaways:

1. Every statically safe design ties allocations to a **scope** (Koka's
   `forall<h>`, Effekt's and Cyclone's regions) or to a **borrow**
   (Rust). None ties them to a provider value that outlives its scope.
2. The implicit-context designs (Odin) are unsafe; the safe effect design
   (Silk) pays a count per allocation, the cost an arena removes.
3. Parameter passing (Rust, Zig) is the industry default; Go shelved
   arenas over its infectiousness, not over safety.

## Option 1: Provider-Scope Lifetime

A `$.with` that installs a key gives that key a lifetime `'r`, the
block's. A row entry carries it (`$ Alloc['r]`), and every value
`$.use(Alloc['r])` lends, or that depends on it, lives exactly as long as
the block. The arena is installed by value and dropped at the block's end;
there is no `reset` in this form. `alloc` is a std function over the
row, since the key is a trait and `$.use` yields `dyn`.

```text
use std.arena.{Alloc, alloc}

struct Node['r]:
    left: (&'r Node['r])?
    right: (&'r Node['r])?

fn make['r](depth: i32) -> &'r Node['r] $ Alloc['r]:  # hypothetical syntax
    if depth == 0:
        return alloc(Node { left: .None, right: .None })
    l := make(depth - 1)
    r := make(depth - 1)
    alloc(Node { left: .Some(l), right: .Some(r) })

fn checksum(depth: i32) -> i64:
    $.with(Alloc = Arena::new()):
        nodes(make(depth))
```

**Rules.** Language tier: a lifetime per installed key; row entries take a
lifetime argument; `req.use.value.local` gives way, for such keys, to "the
value depends on the provider scope"; `req.use.value.outlives-scope` does
not apply to them. The origin model carries it: the installed provider is
a hidden local of the `$.with` block, and a callee's row entry is a hidden
reference parameter, so `ref.dep.*`, origin sets and `ref.excl` apply
unchanged. Frozen loans need nothing new. Stdlib tier: `std.arena.Alloc`
and `alloc[T]`.

**Escape routes**, each with the rule that rejects it:

```text
# 1. Returning past the block.
fn leak() -> i64:
    let mut kept: (&Node)? = .None
    $.with(Alloc = Arena::new()):
        kept = .Some(make(3))  # error: local-reference-escapes, 'r ends with the block
    0

# 2. Storing into a longer-lived struct, a global, or an outer arena.
struct Cache:
    root: (&'static Node['static])?

fn store(cache: &mut Cache) -> void $ Alloc:
    cache.root = .Some(make(3))  # error: local-reference-escapes

fn nest['o](outer: &'o mut Node['o]) -> void:
    $.with(Alloc = Arena::new()):
        outer.left = .Some(make(1))  # error: 'r of the inner scope does not outlive 'o

# 3. A closure, and a closure whose row is resolved where it is called.
fn later['r]() -> fn() -> i64 $ Alloc['r]:  # hypothetical syntax
    n := make(2)
    fn() -> i64: nodes(n)  # error: the capture holds 'r; the closure may not outlive the call

fn dynamic[$R](f: fn() -> &Node $ R + Alloc) -> i64 $ R:
    $.with(Alloc = Arena::new()):
        nodes(f())  # valid: `f` allocates in this scope's arena, and its result dies here

# 4. A suspension across a bang call, and its cancellation.
fn fetch!['r](id: i64) -> &'r Node['r] $ Alloc['r] + Net:  # hypothetical syntax
    body := get!("/node/$id")
    make(body.len())

fn handler!() -> i64 $ Net:
    $.with(Alloc = Arena::new()):
        pending := fetch(7)  # the cold suspension holds 'r: it cannot leave the block
        nodes(fetch!(8))

# 5. A Send crossing.
fn parallel(depth: i32) -> i64 $ Alloc:
    tree := make(depth)
    scope(fn(s: &mut Scope) -> i64:
        t := s.spawn(fn() -> i64: nodes(make(1)))  # error: `$ Alloc` is not Send
        t.join() + nodes(tree)
    )

# 6. Nested scopes of one key.
fn nested() -> i64:
    $.with(Alloc = Arena::new()):
        outer := make(1)
        $.with(Alloc = Arena::new()):
            inner := make(1)  # the nearest scope: the inner arena
            outer.left = .Some(inner)  # error: the inner 'r ends first
        nodes(outer)
```

**Costs.** A `dyn` call per allocation: 16% more work on `binary_trees`,
unless the compiler specializes a function per installed concrete
provider. A dynamic-row closure (`req.with.dynamic`) resolves `'r` where
it is called, so its type abstracts over `'r`. Checker price: lifetimes on
row entries, hidden parameters, and the hole above: several hundred lines
(estimate, not prototyped).

## Option 2: Implicit Borrow Parameter

A row entry may be a borrow of a concrete type, `$ &'a Arena`, as Silk's
rows are borrows. `$.with(Arena = &arena)` lends the installer's own
arena for the block, and every callee receives it as a hidden ordinary
parameter, as Odin passes `context` and Scala passes `using`. Calls are
static, so `alloc[T]` is ARENA.md's method.

```text
use std.arena.Arena

struct Node['a]:
    left: (&'a Node['a])?
    right: (&'a Node['a])?

fn make['a](depth: i32) -> &'a Node['a] $ &'a Arena:  # hypothetical syntax
    if depth == 0:
        return $.use(Arena).alloc(Node { left: .None, right: .None })  # hypothetical syntax
    l := make(depth - 1)
    r := make(depth - 1)
    $.use(Arena).alloc(Node { left: .Some(l), right: .Some(r) })  # hypothetical syntax

fn checksum(depth: i32) -> i64:
    arena := Arena::new()
    $.with(Arena = &arena):  # hypothetical syntax
        nodes(make(depth))
```

**Rules.** Language tier: a key may be a concrete type borrowed as a row
entry (reopens `req.key.traits`, for the new evidence that a `dyn` key
cannot call `alloc[T]`); `req.mut.install-no-local` allows a borrow for
such a key; `$.use` of it is the hidden parameter. Every lifetime rule is
today's, since the borrow is an ordinary local reference of the installer.
The escape routes of option 1 are rejected by the same rules, with `'a`
for `'r`.

**Costs.** No `dyn` call: 192 ms on `binary_trees`. A hidden parameter per
function whose row lists the key. Checker price: borrow keys, hidden
parameters, install rule; about half of option 1 (estimate).

## Option 3: Counted Handles From An Effect

Silk's arena: an allocation "never borrows its arena", and each chunk
counts its live allocations. `$.use(Alloc).handle(v)` returns a counted
`Handle[T]`; the chunk is freed when its last handle drops.

```text
use std.arena.{Alloc, Handle}

struct Node:
    left: Handle[Node]?
    right: Handle[Node]?

fn make(depth: i32) -> Handle[Node] $ Alloc:
    if depth == 0:
        return $.use(Alloc).handle(Node { left: .None, right: .None })
    l := make(depth - 1)
    r := make(depth - 1)
    $.use(Alloc).handle(Node { left: .Some(l), right: .Some(r) })
```

**Rules.** None new in the language; `Handle[T]` is std, over today's
provider rules. **Costs.** A count per handle copy and drop, and a chunk
count per allocation: `binary_trees`' 29,971,824 releases come back as
handle releases, so most of the arena's gain (297 to 197 ms) is lost.
Sound with no lifetime at all.

## Option 4: Callback-Scoped `&mut`

Silk's `Shared.withMut(self: &Self, use: once fn(&mut T) -> A) -> A`: "The
borrow cannot escape the callback", and re-entry "traps before the nested
callback starts". An arena scope lends a frame to a callback; nothing
allocated leaves it.

```text
use std.arena.Scratch

struct Point:
    x: i64
    y: i64

fn sum_points(n: i64) -> i64 $ Scratch:
    $.use(Scratch).scope(fn(s: &mut Frame) -> i64:
        a := s.alloc(Point { x: n, y: 0 })
        b := s.alloc(Point { x: 0, y: n })
        a.x += b.y
        a.x
    )
```

**Rules.** None new; a run-time re-entry flag. **Costs.** U1 and U2 break:
`make` cannot return its nodes, so the tree must be built and consumed in
one callback. A trap check per scope.

## Option 5: Explicit Parameter (Radical Simplification)

No effect. The arena is a value, passed as `&'a Arena` or held in a
lifetime struct (`Parser['a]`), as Rust and Zig do and as ARENA.md
proposes. It costs a parameter per function, the "infectiousness" Go
named.

```text
use std.arena.Arena

struct Node['a]:
    left: (&'a Node['a])?
    right: (&'a Node['a])?

fn make['a](arena: &'a Arena, depth: i32) -> &'a Node['a]:
    if depth == 0:
        return arena.alloc(Node { left: .None, right: .None })
    l := make(arena, depth - 1)
    r := make(arena, depth - 1)
    arena.alloc(Node { left: .Some(l), right: .Some(r) })
```

**Rules.** None beyond ARENA.md's `@disjoint`. **Costs.** None at run
time; one parameter or one context struct per call chain.

## Comparison

| | 1 Provider-scope lifetime | 2 Implicit borrow parameter | 3 Counted handles | 4 Callback-scoped | 5 Explicit parameter |
| --- | --- | --- | --- | --- | --- |
| U1 return nodes | yes | yes | yes, counted | no | yes |
| U2 patch `&mut` | yes | yes | through handles | in one callback | yes |
| U3 no passing | yes | yes | yes | yes | no |
| U4 nested | yes, nearest wins | yes | yes | yes | yes, two parameters |
| U5 across `!` | yes, suspension holds `'r` | yes | yes | no | yes |
| Rules added | lifetime per installed key; row lifetimes; hidden parameters | borrow keys; install of a borrow; hidden parameters | none (std type) | none (std) | none |
| Soundness | static; needs the `$.use` hole closed | static; same | dynamic counts | run-time trap | static |
| `binary_trees` | 209 ms (`dyn`) | 192 ms | about today's 297 ms (counts back) | not expressible | 192 ms |
| Agent-writability | row plus lifetime argument | row borrow | plain | callback nesting | one more parameter |
| Readability | lifetimes appear in rows | lifetimes appear in rows | plain | nested blocks | explicit |
| Checker price | several hundred lines | about half of 1 | 0 | 0 | 0 (plus `@disjoint`) |
| Evolution | other region-like providers | any borrowed context | none | none | a later effect stays possible |

## Recommendation

**Recommendation:** ship option 5 now (ARENA.md as proposed), and if
passing proves costly in real programs, add option 2, not option 1.

- Option 5 costs no new rule and matches Rust and Zig, the industry
  practice; ARENA.md's `@disjoint` is needed by every option anyway.
- Option 2 is the provider-scope lifetime the owner leans toward, with
  a concrete key: the borrow's lifetime is the `$.with` block, the
  checker reuses today's local-reference rules, and calls stay static.
- Option 1 adds a lifetime kind to rows and pays 16% on `binary_trees` for
  `dyn`, or a specialization pass to remove it.
- Options 3 and 4 give back the counts, or the ability to return nodes,
  that an arena exists for.
- Gives up: U3's "no passing" until option 2 lands. Next best: option 2
  directly.
- Independent of the choice: the `$.use` escape hole must be closed.

## Questions For The Owner

1. **Arena as an effect now, or a parameter first?** Effect: no arena in
   signatures. Candidates: option 5 now (recommended), option 2 now,
   option 1 now. Example: `fn make['a](arena: &'a Arena, depth: i32)` versus
   `fn make['a](depth: i32) -> &'a Node['a] $ &'a Arena`.
2. **If an effect, a trait key or a concrete borrow key?** Effect: a trait
   key costs a `dyn` call per `alloc` (16% on `binary_trees`) and needs
   `alloc` as a free function. Candidates: concrete borrow key
   (recommended), trait key with a lifetime. Example: `$ &'a Arena` versus
   `$ Alloc['r]`.
3. **Close the `$.use` escape through a free result lifetime as a bug?**
   Effect: today `fn leak['a]() -> &'a i64 $ Holder` returns a provider's
   insides. Candidates: yes, as `local-reference-escapes` (recommended).
   Example: `$.use(Holder).slot()` returned as `&'a i64`.

## Sources

Every survey row links its source, fetched on 2026-11-23. Measurements:
`binary_trees` at depth 17, release build, V8 default tier, median of 21
fresh processes, with ARENA.md's untyped probe module plus a `dyn Bump`
trait; the probe and programs stay in scratch.

## Parse Log

Every `text` block parses with `scripts/hd-conformance.sh parse`, except
option 2's, whose row `$ &'a Arena` (marked `# hypothetical syntax`) is a
syntax error today: "expected a type name, found '&'". Parsing checks
syntax only; the escape-route errors are what each option's rules would
report, not compiler output.
