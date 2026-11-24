# Unchecked: User-Level Raw Memory

Status: proposal, 2026-11-24; domain: language feature and std surface; not
accepted. Nothing here is in the specification. It builds on
[`ref.local.pointer`](../../spec/lang/13-values-and-references.md#r-ref.local.pointer),
[`ref.excl.call`](../../spec/lang/13-values-and-references.md#r-ref.excl.call),
[Typed Memory](../../spec/lang/10-modules.md#typed-memory) and
[`std.arena`](../../spec/std/arena.md) (#1410).

## The Proposal

User code gets `Ptr[T]` and raw memory, inside `unchecked`. Today only std
may use them ([`module.rt.std-only`](../../spec/lang/10-modules.md#r-module.rt.std-only)),
so nobody but std can write an arena or an allocator; Rust lets anyone
write bumpalo. Three parts: an `unchecked:` block and `unchecked fn`; a
public module `std.ptr` whose four dereferencing methods are `unchecked
fn`; and `Ptr[T]` and `Buffer[T]` become neither `Send` nor `Sync`, so a
type that keeps its state behind one is neither unless an `impl` says so.
The last part also closes a hole std has today (`UniqList`, below).

A user's bumpalo, the key lines (the full arena is
[unchecked/src/bump.hd](unchecked/src/bump.hd)):

```text
use std.ptr.{Buffer, Ptr, align_of, size_of}

pub struct Bump:
    state: *mut State           # neither Send nor Sync, as std.arena

impl Bump:
    pub fn alloc[T < ?Copy](&self, val: T) -> &mut T:
        at := self.bump(size_of::[T](), align_of::[T]()).cast::[T]()
        unchecked:              # hypothetical syntax
            at.write(val)       # fresh bytes, aligned for T
            at.as_mut()         # no other result reaches them
```

The `&mut T` result holds `self` shared by `ref.excl.call`, so `reset`
waits for it and it never outlives the arena; this checks today.

## Open Questions

1. Syntax: an `unchecked:` block and an `unchecked fn`, whose body is not itself unchecked (Rust 2024)? **Recommendation:** yes.
2. The marker gap: make `Ptr[T]` and `Buffer[T]` neither `Send`, `Sync` nor `Wire`, let user code make a `Ptr` from a number only through `with_address` on a `Ptr`, and allow `unchecked impl Send` and `unchecked impl Sync`? **Recommendation:** yes.
3. The user surface: `std.ptr` re-exports `Ptr`, `Buffer`, `size_of` and `align_of`, and nothing else from the runtime? **Recommendation:** yes.
4. Only `Ptr`'s `read`, `write`, `as_ref` and `as_mut` become `unchecked fn`; the other memory intrinsics stay std-only and unmarked? **Recommendation:** yes.
5. A user drop hook (`unchecked impl Drop`) is out of this design, so user arenas drop no values, as bumpalo? **Recommendation:** yes, a later design.
6. An `unchecked:` block that holds no unchecked call is an error (`unused-unchecked`)? **Recommendation:** yes; hd has no lint levels.
7. `unchecked` is a reserved word, not a contextual one? **Recommendation:** yes; no std, example or guide code names anything `unchecked`.

## Problems It Solves

**A user arena or allocator.** Today the first line fails:

```text
use std.rt.memory.{Ptr, size_of}  # error: std-only-use
```

With the proposal, [unchecked/src/bump.hd](unchecked/src/bump.hd) is the
arena: about 80 lines with two unchecked calls. Its chunks are `Buffer[u8]` rooms
that free themselves, so it needs no drop hook.

**Raw state that threads share by mistake.** The marker walk reads only
field types, and `Buffer[T]` holds two `usize` fields. So std's
`UniqList[*mut i32]` is `Send` today, though a `*mut` never is, and
`UniqList[string]` is `Sync`. This checks on main, while the same program
over `List` is `unsatisfied-trait-bound`; both threads count `shared`:

```text
fn count(held: UniqList[*mut i32]) -> usize:
    held.len()

pub fn main!() -> void:
    shared := box(7)
    let mut held = UniqList::[*mut i32]::new()
    held.push(shared)
    n := background!(fn(): count(held))  # accepted today
    other := shared
```

A user `struct Counter { cell: usize }` written through `&self` would race
the same way. Under the proposal `Buffer` and `Ptr` are neither, so
`UniqList` and `UniqMap` take their markers from `T` by std `impl`s, and
the call above is `unsatisfied-trait-bound`. User code cannot make a `Ptr`
from a bare number, so its raw state sits in a `Ptr` field too.

## How It Works

**Syntax.** `unchecked:` opens a block expression, on its own lines or as
`unchecked: expr`, valued as its last expression. An `unchecked fn`'s
callers keep conditions nobody checks; its body is checked as any other.
`unchecked impl` writes `Send` or `Sync`. A closure inside the block is
inside it, as in Rust.

**Operations.**

| Kind | Items | Why |
| --- | --- | --- |
| needs `unchecked:` | `Ptr.read`, `Ptr.write`, `Ptr.as_ref`, `Ptr.as_mut`, and any user `unchecked fn` | they reach memory through an address the checker cannot follow |
| safe in `std.ptr` | `Ptr.address`, `with_address`, `cast`, `offset`; `size_of`, `align_of`; `Buffer::new`, `capacity`, `at`, `grow` | arithmetic and owned rooms; nothing is read or written |
| std-only, unchanged | `std.rt.alloc` (alloc, free, retain, release, block_of, size classes) | counts are the runtime's representation, which the optimizer and collector rely on; `Buffer` frees safely instead |
| std-only, unchanged | `std.rt.cycle`, `Drop` | a hook runs while the collector marks ([`module.rt.drop.trace`](../../spec/lang/10-modules.md#r-module.rt.drop.trace)); that protocol is not a user contract |
| std-only, unchanged | `std.rt.thread`, atomics, waits | a user thread would bypass the `Send` crossing's copy of shared buffers; `std.sync` is the user surface |
| std-only, unchanged | loads, stores, `copy`, `fill`, `address_of`, `string_bytes`, `new_string` | `Ptr` over an integer type already reads and writes bytes; the rest serve std's own types |

**Rules and tiers.** All are language tier, since the compiler knows each
name.

| Rule (proposed ID) | Chapter | Says |
| --- | --- | --- |
| `lex.keyword.unchecked` | 1 | `unchecked` is a reserved word |
| `grammar.unchecked` | 2 | the block, the fn qualifier and the impl qualifier |
| `fn.unchecked.call` | 7 | calling or naming an `unchecked fn` outside an `unchecked:` block is an error: `unchecked-call` |
| `fn.unchecked.body` | 7 | an `unchecked fn`'s body is not an `unchecked:` block |
| `fn.unchecked.value` | 7 | an `unchecked fn` is only called, never used as a function value: `unchecked-call` |
| `fn.unchecked.unused` | 7 | a block with no unchecked call is an error: `unused-unchecked` |
| `trait.unchecked.match` | 9 | a trait's `unchecked fn` is implemented by an `unchecked fn`, and only by one: `unchecked-mismatch` |
| `trait.sealed.unchecked-impl` | 9 | `unchecked impl` is allowed for `Send` and `Sync` only: `unchecked-impl` |
| `module.marker.table.raw` | 10 | `Ptr[T]` and `Buffer[T]` are neither `Wire`, `Send` nor `Sync` |
| `module.ptr.public` | 10 | `std.ptr` re-exports the items above; its `Ptr` has no public field |

**The author's obligations.** Each `unchecked:` block states, by being
written, that these hold for the calls in it:

1. Every address read, written or lent is room for a `T` inside memory the author owns, aligned to `align_of[T]()`.
2. `read`, `as_ref` and `as_mut` reach only a place that holds a value: written, and not yet read out.
3. An `as_mut` result is the only live path to its value for its lifetime. Nothing writes the value an `as_ref` result reaches while it lives.
4. The value stays at its address, and its room stays allocated, for the result's lifetime. The signature ties that lifetime to a borrow that keeps the owner alive, as `alloc(&self) -> &mut T` does.
5. A value read out is never read again or dropped where it was. Writing over a value leaks it, which is allowed.
6. No write reaches the bytes of a live `&` referent. State that changes through `&self` lives behind a `Ptr` or a `*mut`, never in the struct's own fields.
7. An address made with `with_address` lies in memory the type reaches through its own `Ptr` or `*mut` fields.
8. `unchecked impl Sync`: nothing changes through `&self`, since user code has no atomics. `unchecked impl Send`: the raw memory holds no counted buffer that another holder on this thread counts.

An `unchecked fn` states its callers' conditions in its doc comment, as a
Rust `# Safety` section does.

## Cost

| Component | Change | Lines |
| --- | --- | --- |
| lexer, parser, formatter | reserved word; block expression; fn and impl qualifiers | +60 |
| resolver and checker | a context flag on the body walk; a flag per callee; trait matching; the value rule; one bool per open block | +90 |
| markers | the `Ptr` row; the sealed-impl exception | +20 |
| std | `std.ptr` (+12); `Ptr.address` private, std's 20 literals use `at_address` and about 50 reads call `address()` (+5); about 110 call lines wrapped (+35); explicit `Send` and `Sync` for `TypeId`, `UniqList`, `UniqMap` (+6) | +58 |
| spec | 10 rules, 4 diagnostic codes, 4 reworded rules | +70 |
| conformance | one fixture per rule and code | +14 cases |
| deleted | the std-only wording of `module.rt.ptr.holds`, `.as-ref`, `.write.shared`, `buffer.at`; the arena chapter's "only std" sentence | −5 |

Per run: O(1) checker work per call and per block, none per total state.
`unchecked` is gone after checking, so TIR, runtime and Wasm size do not
change; the web compiler grows an estimated 2 KB gzip (cap 2 MB). Net
growth needs Growth-OK: the std-only gate is a use rule that stays, so no
compiler special case is deleted.

---

## Appendix A. Stress Test

Probes ran `hd check` on the package with `std.ptr` stubbed as a local
module ([unchecked/src/ptr.hd](unchecked/src/ptr.hd)), with a debug build of
main at 311172046; Rust probes used rustc 1.97.1, edition 2024.

| # | Case | Result | What the design does |
| --- | --- | --- | --- |
| 1 | bumpalo's `Bump` as user code ([bump.hd](unchecked/src/bump.hd)) | checks; two calls need `unchecked:` | values are never dropped, as bumpalo's "No `Drop`s" says; chunks are `List[*mut Buffer[u8]]`, since `List` needs `Copy` elements |
| 2 | `bank.reset()` while an `alloc` result is used later | `conflicting-access` | `ref.excl.call` holds the arena shared |
| 3 | an `alloc` result returned past a local arena | `local-reference-escapes` | nothing new |
| 4 | two live `alloc` results, both written | accepted | obligation 3: each `alloc` gives fresh bytes |
| 5 | `fn get(&self) -> &mut u32` handing out the same address twice | would check | obligation 3 broken; Rust's `holder.rs` probe compiles too and prints "3 3" |
| 6 | `struct Counter { cell: usize }` | `Send` and `Sync` today (probed with a `T < Send & Sync` bound) | proposal: a `Ptr` field makes it neither; a number becomes a `Ptr` only through `with_address` |
| 6b | `UniqList[*mut i32]` into `background!`; `UniqList[string]`, `UniqMap[string, string]` as `Sync` | all accepted today (probed) | proposal: rejected; std's explicit `impl`s give the markers from `T`; both lose `Wire`, which sent raw addresses |
| 7 | the same type in Rust, `next: usize` read with `with_exposed_provenance_mut` | Rust says `Sync` (probed, rustc 1.97.1); `*mut u8` field: E0277 | hd matches Rust's default; laundering stays obligation 7 in both |
| 8 | `f := Ptr.read` as a value, called later outside `unchecked:` | proposal: `unchecked-call` | `fn.unchecked.value` |
| 9 | a closure made inside `unchecked:` that escapes | allowed | Rust allows it; the author's obligations cover every later call |
| 10 | a trait `unchecked fn free(..)` implemented by a plain `fn` | proposal: `unchecked-mismatch` | a generic caller sees the trait's flag |
| 11 | a user slab of `Conn`s dropped while full ([slab.hd](unchecked/src/slab.hd)) | values leak | no user drop hook (question 5); `clear` reads them out first; a safe slab over `UniqList[T?]` needs no `unchecked` |
| 12 | `unchecked impl Send for Bump` holding a `string` | would compile | unsound: the string's buffer is shared with a copy on this thread; obligation 8 forbids it, as bumpalo's `Send` relies on Rust's single owner |
| 13 | `unchecked impl Sync` for a type written through `&self` | would compile | unsound without atomics; obligation 8 |
| 14 | `y := unchecked: x + 1` on today's parser | `missing-let`; the block form parses as a call of `unchecked` | needs the reserved word and `grammar.unchecked` |
| 15 | std's own `Ptr` calls | about 110 lines in 7 files (collections 85, arena 9, sync 5, rt/thread 5, inspect 2, ops 1, rt/crossing 1; grep) | each gets `unchecked:`; the step's diagnostic gives the exact list |
| 16 | `TypeId` holds a `Ptr[TypeInfo]` | it would lose `Send`, `Sync` and `Wire` | std writes `Send` and `Sync`; losing `Wire` matches its comment that it is not a boundary tag |

## Appendix B. Prior Art

**Rust: `unsafe` blocks and functions.**

```rust
pub unsafe fn get(p: *const u32) -> u32 { unsafe { *p } }
let v = unsafe { get(&x) };
unsafe impl Send for Bump {}            // bumpalo, src/lib.rs
pub fn alloc<T>(&self, val: T) -> &mut T // bumpalo's signature
```

Unsafe functions "have extra safety conditions, which are requirements that
must be upheld by all callers and that the compiler does not check"
([Reference](https://doc.rust-lang.org/reference/unsafe-keyword.html)).
Edition 2024 warns on an unsafe operation in an unsafe fn body without a
block ([edition guide](https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-op-in-unsafe-fn.html)); rustc 1.97.1 gives E0133 as a warning. hd takes the
block, the fn, `unsafe impl` for markers, and makes the 2024 lint an error.

**Rust: raw pointers and markers.** "raw pointers are neither Send nor Sync
(because they have no safety guards)", marked so "as more of a lint"
([Nomicon](https://doc.rust-lang.org/nomicon/send-and-sync.html)). Rust 1.84
stabilized `addr`, `with_addr` and `map_addr` so code keeps a pointer
instead of an integer ([release notes](https://blog.rust-lang.org/2025/01/09/Rust-1.84.0/)).
hd takes both: `Ptr` lacks the markers, and `with_address` is the only
number-to-`Ptr` path for users.

**Swift: SE-0458, strict memory safety** (Swift 6.2):

```swift
func sum() -> Int {
  withUnsafeBufferPointer { buffer in
    unsafe sumIntBuffer(buffer.baseAddress, buffer.count, 0)
  }
}
```

An `unsafe` expression covers every unsafe use inside it; diagnostics are
warnings under `-strict-memory-safety`
([SE-0458](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0458-strict-memory-safety.md)).
hd rejects opt-in warnings: the check is always on and an error.

**Go: package `unsafe`.**

```go
import "unsafe"
p := unsafe.Pointer(&x)
q := (*uint32)(unsafe.Add(p, 4))
```

"A package using unsafe must be vetted manually for type safety and may
not be portable" ([spec](https://go.dev/ref/spec#Package_unsafe)). The
import is the only marker. hd's std-only gate is this model for std;
the keyword marks lines, not files.

## Appendix C. Alternatives Considered

- **Go's model: a public raw module, no keyword.** Cheapest by the Design Cost Order, but it marks a file, not the lines to audit; the owner chose a keyword.
- **The body of an `unchecked fn` is unchecked** (Rust before 2024). Rust moved away because it hides which lines are unchecked.
- **Every memory intrinsic an `unchecked fn`.** About 300 more std wraps (314 raw-call lines by grep) for no user gain, since users cannot reach them.
- **The marker gap as an obligation only.** Leaves the natural code wrong by default: a `usize` field gives `Send` and `Sync` silently.
- **A type with an `unchecked:` block in its impl loses its markers.** Action at a distance, and a helper function outside the impl bypasses it.
- **Raw `alloc` and `free` for users.** `free` adds a double-free hazard; `Buffer` already frees its room exactly once.

## Appendix D. Today In hd

- `Ptr[T]` is an ordinary struct with a public `address: usize`, so it is `Send`, `Sync` and `Wire` ([`module.marker.sync.structural`](../../spec/lang/10-modules.md#r-module.marker.sync.structural)).
- Only std reads or writes through a `Ptr` ([`module.rt.ptr.holds`](../../spec/lang/10-modules.md#r-module.rt.ptr.holds)), and std keeps an `as_mut` value unshared ([`module.rt.ptr.as-ref`](../../spec/lang/10-modules.md#r-module.rt.ptr.as-ref)).
- A reference made through a `*T` or `*mut T` is local ([`ref.local.pointer`](../../spec/lang/13-values-and-references.md#r-ref.local.pointer)); a `Ptr` is not a counted reference, so `as_mut` may be returned.
- `Send` and `Sync` are sealed ([`trait.sealed.user-impl`](../../spec/lang/09-traits.md#r-trait.sealed.user-impl)).
- `std.arena` ships without `unchecked`, over `Ptr.as_mut`; its state is a `*mut State`, so it is neither `Send` nor `Sync` ([`std-arena.type`](../../spec/std/arena.md#r-std-arena.type)).
- The checker knows `Ptr`'s methods by intrinsic key (`hd_resolve/src/intrinsics.rs`); the std-only gate is the use rule (`hd_resolve/src/lower.rs`, `std-only-use`), not a `Ptr` check.

## Appendix E. Details

- **Grammar and formatting:** `unchecked_expr = "unchecked" ":" (expr | block)`; qualifiers sit after `pub` (`pub unchecked fn`, `unchecked impl`). The formatter keeps a one-expression block on one line.
- **Typing:** the block's type is its last expression's; `unchecked` changes no type and no inference.
- **Patterns, derives:** no interaction; a derive never emits `unchecked`.
- **Closures:** lexical; a closure inside the block is inside it.
- **Modules:** `std.ptr` is a `pub use` of `std.rt.memory` names; `module.rt.std-only` gains one sentence: a name a std module re-exports is that module's.
- **Ownership and the inliner:** erased after checking; an inlined body keeps no marker.
- **Soundness:** the eight obligations; the checker proves none of them.
- **Diagnostics:** `unchecked-call`, `unused-unchecked`, `unchecked-mismatch`, `unchecked-impl`, each with a fix-it where one exists (wrap the call, drop the block, add the qualifier).
- **No-feature alternative:** users box each value (`*mut T`) and never touch raw memory; std keeps the only arena.

Phases, each about an hour, each with its fixtures:

1. Reserved word, grammar, formatter; parse fixtures.
2. `unchecked fn` flags, `unchecked-call` for calls and values, `unused-unchecked`; std's `Ptr` methods marked and its ~110 sites wrapped in the same commit.
3. Trait matching and `unchecked impl Send|Sync`; marker row for `Ptr`; `TypeId` impls.
4. `Ptr.address` private, `at_address` for std, `std.ptr` re-exports; spec rewording; the package here becomes a conformance case.

Package checks: `cd docs/future-work/unchecked && hd check && hd fmt --check`.
The package leaves out the `unchecked:` wrappers, which do not parse yet;
a comment marks each place.

Parse log (`scripts/hd-conformance.sh parse` on every `text` block; syntax
only, not types): blocks 1 to 3 parse. Block 1's `unchecked:` parses today
only as a call of a function named `unchecked` with a block argument, so it
keeps its `# hypothetical syntax` mark. The one-line form fails (row 14).
