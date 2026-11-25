# Uncheck: Raw Memory As A Capability

Status: proposal, 2026-11-24; domain: language feature and std surface; not
accepted. Nothing here is in the specification. It replaces the `unchecked`
keyword design of PR #2344, on the owner's direction of 2026-11-24: "try
adding Uncheck as a builtin capability with only stdlib impl". It builds on
[requirement rows](../../spec/lang/11-requirements-and-suspension.md#requirement-rows),
[capability grants](../../spec/cli/command-line.md#capability-grants),
[`ref.local.pointer`](../../spec/lang/13-values-and-references.md#r-ref.local.pointer),
[Typed Memory](../../spec/lang/10-modules.md#typed-memory) and
[`std.arena`](../../spec/std/arena.md) (#1410).

## The Proposal

User code gets `Ptr[T]` and `Buffer[T]` from a public `std.ptr`. The four
calls the checker cannot follow, `Ptr`'s `read`, `write`, `as_ref` and
`as_mut`, need `Uncheck` in the caller's row. `Uncheck` is a requirement key
like `Console`: `main` gets it from `Uncheck = true` in `hd.toml`, and it
propagates through rows. Only std implements it, so only std makes a
provider. A value can keep the caller's `Uncheck` as a `Grant`, and its
methods discharge it with `$.with`. No keyword, block or `unchecked fn` is
added, and `Uncheck` costs nothing at run time.

A user's bumpalo (the full arena is [uncheck/src/bump.hd](uncheck/src/bump.hd)):

```text
use std.ptr.{Grant, Ptr, Uncheck, align_of, size_of}

pub struct Bump:
    state: *mut State
    grant: Grant                       # no bytes; made only under `$ Uncheck`

impl Bump:
    pub fn new() -> Bump $ Uncheck:    # whoever makes an arena shows it
        Bump { state: box(State { next: 1, end: 0 }), grant: Grant::new() }

    pub fn alloc[T < ?Copy](&self, val: T) -> &mut T:   # a safe API
        at := Ptr::[T] { address: self.bump(size_of::[T](), align_of::[T]()) }
        $.with(Uncheck=self.grant):    # the one unchecked region
            at.write(val)
            at.as_mut()
```

```toml
# hd.toml of the program that makes a Bump
[capabilities]
Uncheck = true
```

## Open Questions

1. Grant at the entry: `Uncheck` joins the host capability keys, boolean only, and `main` names it in its row? **Recommendation:** yes.
2. User discharge: a value keeps a `Grant`, which only `Grant::new() $ Uncheck` makes, and `$.with(Uncheck=grant)` discharges it? **Recommendation:** yes; without it a user arena's every caller up to `main` names `Uncheck`.
3. `$.use(Uncheck)` is an error, so no provider exists at run time? **Recommendation:** yes; `Grant::new()` is the one way to keep it.
4. `Ptr[T]` and `Buffer[T]` are neither `Send`, `Sync` nor `Wire`, and std writes `Send` and `Sync` for `Buffer` from `T`? **Recommendation:** yes.
5. A user type that holds a `Ptr` can never be `Send` or `Sync`, since user code cannot implement a marker? **Recommendation:** yes for now; bumpalo's `unsafe impl Send` waits for a use.
6. `Uncheck = false`, or a `--cap Uncheck` flag, is decided at build, since `Uncheck` has no Wasm import? **Recommendation:** yes.

## Problems It Solves

**A user arena or allocator.** Today `use std.rt.memory.Ptr` is
`std-only-use`, so only std can write an arena. With the proposal the user
arena above, a slab ([slab.hd](uncheck/src/slab.hd)) and std.arena's shape
([arena.hd](uncheck/src/arena.hd)) check against a stub of `std.ptr`.

**An audit that reaches every dependency.** A public signature always writes
its row ([`req.row.omitted.empty-pub`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.omitted.empty-pub)),
so a dependency that touches raw memory shows `$ Uncheck` on the function
that takes it. The root package cannot call it without `Uncheck` in a row,
which reaches `main`, which `hd.toml` must grant
([`cli.cap.table.required`](../../spec/cli/command-line.md#r-cli.cap.table.required)):

```text
fn settle() -> i64 $ Uncheck:          # Bump::new needs it, so settle names it
    bank := Bump::new()
    open(&bank, "alice", 100).balance  # alloc needs nothing: the arena holds the grant

pub fn main() -> void $ Console + Uncheck:  # hypothetical syntax: today nonhost-entry-requirement
    println("${settle()}")
```

**std keeps its safe APIs.** std holds its own grant in `std.rt`, which no
other package may use ([`module.rt.std-only`](../../spec/lang/10-modules.md#r-module.rt.std-only)).
So `Arena::new`, `UniqList.push` and the rest keep today's rows, and a
program that uses only std grants nothing.

## How It Works

**The capability.** `std.ptr` declares `pub trait Uncheck` with no members.
It is sealed: only std implements it, for `Grant` alone. `Grant` has one
private `void` field, so it has no bytes and user code cannot build one
([`data.vis.private-fields`](../../spec/lang/08-data-and-enums.md#r-data.vis.private-fields)).
It is `Send` and `Sync`, and never `Wire` or `Deserialize`, so no bytes from
outside become a grant.

**Where a program gets it.** Two sources, both written in source:

| Source | Who | Shows in |
| --- | --- | --- |
| the entry grant | `main`, a test or a task | its row, and `Uncheck = true` in `[capabilities]` or `[dev-capabilities]` |
| std's grant | std only, from `std.rt` | nothing: std is the trusted base, as today |

**Who discharges it.** A requirement is discharged only by `$.with` with a
provider. Every provider is a `Grant`. User code gets a `Grant` only from
`Grant::new() -> Grant $ Uncheck`, so it can discharge only an `Uncheck` it
was given. A value that keeps one is Pony's `TCPConnectAuth(env.root)`: its
constructor shows the authority, and its methods use it. Rust's `unsafe`
block inside a safe function is the `$.with` block; the author vouches for
every call in it.

**What it relaxes.** Exactly one thing: who may read, write or lend through
a `Ptr`. Today [`module.rt.ptr.holds`](../../spec/lang/10-modules.md#r-module.rt.ptr.holds)
and [`module.rt.ptr.as-ref`](../../spec/lang/10-modules.md#r-module.rt.ptr.as-ref)
say "only std"; with the proposal they say "a caller with `Uncheck`", and
the obligations stay as written. `as_ref` and `as_mut` are the only source
of a reference that depends on no place, so they are the only non-local
`&mut` a function can return. The pointer-loan rule does not change: `&mut *p`
through a `*mut T` stays local under `Uncheck`
([`ref.local.pointer`](../../spec/lang/13-values-and-references.md#r-ref.local.pointer)).
The borrow checker still checks every result once a signature ties it to a
parameter, as `alloc(&self) -> &mut T` ties it to the arena.

**Markers.** `Ptr[T]` counts `T` as a part today
([`module.marker.address`](../../spec/lang/10-modules.md#r-module.marker.address)),
so `struct Counter { cell: Ptr[i32] }` is `Send` and `Sync` (probed on the
stub). A user who writes through `&self` would then race across threads.
With `Ptr` neither, std writes `impl[T < Send] Send for Buffer[T]` and the
same for `Sync`, and `UniqList` and `UniqMap` follow from `Buffer`.

**Rules, simplest set.** Checked against the cases in the table after it.

| Rule (proposed ID) | Tier | Says |
| --- | --- | --- |
| `trait.sealed.uncheck` | lang, ch. 9 | `Uncheck` is sealed; std implements it for `Grant` |
| `req.key.uncheck` | lang, ch. 11 | `Uncheck` is a host capability with no run-time provider; `$.use(Uncheck)` is an error: `uncheck-use` |
| `module.marker.table.raw` | lang, ch. 10 | `Ptr[T]` and `Buffer[T]` are neither `Send`, `Sync` nor `Wire`; replaces `module.marker.address` |
| `module.rt.ptr.holds` (reworded) | lang, ch. 10 | "only std" becomes "a caller with `Uncheck`" |
| `cli.cap.uncheck` | CLI | a boolean key, decided at build |
| `std-ptr.*` | std | the module, the four rows, `Grant::new` |

| Case | Result |
| --- | --- |
| `p.read()` in a public fn with no `Uncheck` | `missing-requirement` (checks today on the stub) |
| `Grant { sealed: () }` outside `std.ptr` | `private-member` (checks today) |
| `impl Uncheck for Mine` | proposed `unsatisfied`-style sealed error; accepted on the stub |
| `bank.reset()` while an `alloc` result lives | `conflicting-access` (checks today) |
| `main() $ Uncheck` with no `hd.toml` key | `missing-capability`, with a fix-it |

## Cost

| Component | Change | Lines |
| --- | --- | --- |
| checker | one sealed-trait row; `uncheck-use`; drop `Uncheck` from provider lists after checking | +25 |
| markers | the `Ptr` and `Buffer` row; `module.marker.address` deleted | +10, −8 |
| CLI | one capability key; deny at build | +15 |
| std | `std.ptr` (+20); four `$ Uncheck` rows; one `$.with` per std function with a raw call, about 65 functions (+65); `Buffer` marker impls (+4) | +93 |
| spec | 5 rules, 1 diagnostic code, 2 reworded rules | +40 |
| conformance | one case per rule and code | +7 cases |

Run time: none. `Uncheck` passes no provider (`$.use` cannot observe one),
`Grant` has no bytes, and `$.with(Uncheck=…)` emits nothing; the Wasm is
byte-identical. Compile time: one key per row, compared as any key, O(1) per
call; no pass, no fixpoint, nothing proportional to total state. Web
compiler: under 1 KB gzip (cap 2 MB). Net growth needs Growth-OK; it
deletes the `unchecked` grammar the old design would have added.

---

## Appendix A. Stress Test

Probes ran `hd check` on the package with `std.ptr` stubbed as a local
module ([uncheck/src/ptr.hd](uncheck/src/ptr.hd)), with a debug build of
main at 0a28d0656; Rust probes used rustc 1.97.1, edition 2024.

| # | Case | Result | What the design does |
| --- | --- | --- | --- |
| 1 | bumpalo's `Bump` as user code ([bump.hd](uncheck/src/bump.hd)) | checks; one `$.with` block | values never dropped, as bumpalo's "No `Drop`s" |
| 2 | a slab of `!Copy` values ([slab.hd](uncheck/src/slab.hd)) | checks; three `$.with` blocks | leaks values dropped while full; `clear` reads them out |
| 3 | std.arena's shape ([arena.hd](uncheck/src/arena.hd)) | checks; `Arena::new` keeps its empty row | std's grant stands in as `stub_grant` |
| 4 | a private fn calls `p.read()` with no clause | accepted: its row is inferred as `$ Uncheck` | callers see it; at a public fn it must be written |
| 5 | a public fn calls `p.read()` with no clause | `missing-requirement` | the audit point |
| 6 | `Grant { sealed: () }` in another module | `private-member` | unforgeable without a new rule |
| 7 | `G := Grant::new()` at top level | `missing-requirement` | no ambient grant in a global |
| 8 | `impl Uncheck for Mine`, then `$.with(Uncheck=Mine {..})` | accepted on the stub | `trait.sealed.uncheck` rejects it |
| 9 | `$.use(Uncheck)` kept as `*dyn Uncheck`, later reinstalled | accepted on the stub | `uncheck-use`; `Grant::new` is the one form |
| 10 | `fn lend[$R](g: Grant, cb: fn() -> i32 $ R + Uncheck) -> i32 $ R` | accepted: `cb` gets `g` | fine for a holder; std must never write this with its own grant, or any callback gets `Uncheck` free; a std review check (grep `+ Uncheck` in std fn types) |
| 11 | `bank.reset()` while an `alloc` result lives | `conflicting-access` | `ref.excl.call` holds the arena |
| 12 | an `alloc` result returned past a local arena | `reference-result-without-source` | nothing new |
| 13 | `fn get(&self) -> &mut u32` lending one address twice | would check | the author's obligation, as in Rust (`&mut *p` twice compiles) |
| 14 | `struct Counter { cell: Ptr[i32] }` as `Send & Sync` | accepted on the stub | rejected under question 4 |
| 15 | `UniqList[*mut i32]` as `Send`; `UniqList[string]` as `Sync` | both `unsatisfied-trait-bound` on main | the old design's hole is closed by `module.marker.address` |
| 16 | `UniqList[i32]` as `Wire` | accepted on main | a separate-memory crossing must copy the room, not the address; question 4 removes `Wire` until #2288 does |
| 17 | `main() $ Uncheck` today | `nonhost-entry-requirement` | question 1 adds the key |
| 18 | a stored suspension `fn f!() $ Uncheck` run later | its provider is bound at construction | erased like any `Uncheck`; nothing to keep |
| 19 | a closure holding a `Grant` returned from a `$ Uncheck` fn | accepted | authority moves with values, as Pony's; the maker's row shows it |
| 20 | `Uncheck = false` on a prebuilt Wasm | no import to refuse | question 6: the build decides |

## Appendix B. Prior Art

**Rust: `unsafe` blocks in safe functions.** Probed with rustc 1.97.1:

```rust
pub fn alloc<T>(&self, val: T) -> &mut T {
    let at = /* bump arithmetic */;
    unsafe { at.write(val); &mut *at }   // callers write no `unsafe`
}
unsafe fn get(p: *const u32) -> u32 { *p }
get(&x);   // error[E0133]: call to unsafe function `get` is unsafe and requires unsafe block
```

A struct with a `*mut u8` field moved into `thread::spawn` is E0277,
"`*mut u8` cannot be sent between threads safely"; raw pointers lack the
markers "as more of a lint" ([Nomicon](https://doc.rust-lang.org/nomicon/send-and-sync.html)).
`std::ptr::with_exposed_provenance_mut(64)` is a safe call. hd takes the
discharge in a safe function and the missing markers. It rejects `unsafe` as
a keyword: the owner asked for a capability, which also reaches `main`.

**Pony: authority passed to constructors.**

```pony
actor Main
  new create(env: Env) =>
    Connect(env.out, TCPConnectAuth(env.root))
```

`env.root` holds `AmbientAuth`; a token's constructor is private, so only
the runtime supplies the root
([tutorial](https://tutorial.ponylang.io/object-capabilities/derived-authority.html)).
C calls are limited by `ponyc --safe=files:net:process`
([trust boundary](https://tutorial.ponylang.io/object-capabilities/trust-boundary.html)).
hd's `Grant` is the token, and the entry grant is `env.root`.

**Austral: a root capability and unsafe modules.**

```text
function main(root: Root_Capability): ExitCode is   -- Austral, not hd
```

"Capabilities cannot be created out of thin air"; FFI needs
`pragma Unsafe_Module`, so the build can list every unsafe module of every
dependency ([spec](https://austral-lang.org/spec/spec.html)). hd gets the
same list from rows, at function grain, with no linear types.

**Scala 3: `CanThrow`, an erased capability.**

```scala
erased class CanThrow[-E <: Exception]
def f(x: Double): Double throws LimitExceeded = ...
```

"All `CanThrow` capabilities are compile-time only artifacts; they do not
have a runtime footprint"; `try` introduces one as an erased given
([reference](https://docs.scala-lang.org/scala3/reference/experimental/canthrow.html)).
hd's `Uncheck` is erased the same way; `$.with` plays `try`.

**Effekt: handlers introduce capabilities.**

```effekt
try { val res = divide(10, 0) {exc}; println(res) }
with exc: Exception { def throw(msg) = println("ERROR: " ++ show(msg)) }
```

"Handlers (implicitly or explicitly) introduce capabilities that are then
passed to the position where an effect is used"
([tour](https://effekt-lang.org/tour/captures)). In hd any user may handle
`Clock`, but only a `Grant` holder may handle `Uncheck`.

## Appendix C. Alternatives Considered

- **The `unchecked` keyword (PR #2344).** Owner rejected it: a new keyword, block and fn qualifier, four more diagnostics, and nothing reaches `main`.
- **No user discharge.** Simplest, but a user arena's every caller up to `main` names `Uncheck`; std would be the only safe-API author.
- **Keep `*dyn Uncheck` from `$.use` instead of `Grant`.** No new std type, but a two-word counted field per holder and a provider passed on every call.
- **`Uncheck` provided by std only, never at the entry.** Then a user dependency gets it from some std call for free, and nothing shows in `hd.toml`.
- **A per-dependency grant** (`"dep.bumpalo.Uncheck" = true`). Finer, but rows already name the caller; a later step if owners want it.

## Appendix D. Today In hd

- `Ptr[T]` has a public `address: usize`; its four calls are std-only by [`module.rt.ptr.holds`](../../spec/lang/10-modules.md#r-module.rt.ptr.holds), enforced by [`std-only-use`](../../spec/lang/10-modules.md#r-module.rt.std-only.error).
- `Buffer[T]` now holds a `ptr: Ptr[T]`, so markers read its values as parts (`module.marker.address`, landed after PR #2344).
- Host capability keys are in `crates/hd_project/src/manifest.rs` (`CAPABILITY_KEYS`); total deny reads the Wasm import list ([`cli.cap.total.needs`](../../spec/cli/command-line.md#r-cli.cap.total.needs)).
- Providers are passed as `(data, vtable)` pairs (`crates/hd_wasm/DESIGN.md` §23.8), so erasing `Uncheck` is needed for zero cost.
- Since #2359, markers are `std.meta.{Send, Sync, Wire}`; `std.arena` imports `std.rt.{Ptr, …}`.

## Appendix E. Details

- **Grammar, formatting:** none; `$ Uncheck` and `$.with(Uncheck=g)` parse today.
- **Typing and inference:** ordinary rows; a private fn infers `Uncheck` ([`req.row.omitted.inferred-private`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.omitted.inferred-private)).
- **Closures and suspensions:** ordinary row rules; nothing kept at run time.
- **Modules:** `std.ptr` re-exports from `std.rt.memory` and declares `Uncheck` and `Grant`; std's own grant stays in `std.rt`.
- **Soundness:** the author of each `$.with(Uncheck=…)` block, or of a function that names `Uncheck`, keeps the obligations of `module.rt.ptr.*`; std never lends its grant to a callback whose row adds `Uncheck` (case 10).
- **Workers:** a registered boundary lists `Uncheck` in its profile only if its contract does (`req.use.registered-profile-set`).

Phases, each about an hour, each with its fixtures:

1. `Uncheck`, `Grant`, `std.ptr`; the four rows; std's ~65 `$.with` lines in the same commit.
2. Sealed row, `uncheck-use`, erasure in provider lists; a WAT fixture showing no provider parameter.
3. Capability key and build-time deny; CLI conformance.
4. Marker rows for `Ptr` and `Buffer`; `Buffer` impls; the package here becomes a conformance case.

Package checks: `cd docs/future-work/uncheck && hd check && hd fmt --check`.
