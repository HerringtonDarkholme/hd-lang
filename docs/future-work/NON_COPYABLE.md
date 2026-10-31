# Non-Copyable Types

Status: proposal, 2026-10-31; domain: language (values and moves) and std
traits; not accepted. Nothing here is specified behavior.

This addresses [#1141](https://github.com/HerringtonDarkholme/hd-lang/issues/1141).
It builds on [Moves](../../spec/lang/13-values-and-references.md#moves)
(M1 #1137, M2 #1138) and the `copies` predicate in
`crates/hd_check/src/moves.rs`. Transfer to a domain with separate memory
(PR #1139, `EXECUTION_BOUNDARIES.md` question 5) needs it.

## The Proposal

- A struct or enum declared `< ~Copyable` is never copied. Its values move
  under today's Moves rules, as a `&mut` does.
- A type parameter is copyable unless it says `< ~Copyable`, as in Swift.
  This also closes today's hole where generic code copies a `SpanMut`.
- A non-copyable value leaves a place only as a whole binding. A field, an
  element, `*r` or a closure capture lends it, or `replace` swaps it out.
- Std traits whose methods only read take `&self`, as in Rust, starting
  with `Index` (#1140).
- No destructor. A `using` binding that moved is not disposed on that path.

```text
struct Photo < ~Copyable:  # hypothetical syntax
    width: u32
    height: u32
    pixels: List[u8]

fn thumbnail(photo: Photo, side: u32) -> (Photo, Photo):
    small := shrink(&photo, side)
    (photo, small)

pub fn on_export!(photo: Photo) -> Result[Photo, WorkerError] $ Workers:
    let (back, small) = $.use(Workers).call!(thumbnail, photo, 256)?
    w := photo.width  # error: use-of-moved-value; `back` holds the photo now
    .Ok(small)
```

## Open Questions

1. **Syntax.** `struct Photo < ~Copyable:` and `[T < ~Copyable]`, reusing
   the `<` of bounds and supertraits? **Recommended: yes.**
2. **Inferred holders.** A type that holds a non-copyable value is
   non-copyable without the marker, as M2 does for `&mut`? **Recommended:
   yes**; Swift requires the marker on the holder.
3. **Generics.** `[T]` takes only copyable arguments, and `[T < ~Copyable]`
   takes any and moves `T` in its body? **Recommended: yes** (Swift SE-0427).
   `keep(c)` with a `&mut`, ch. 13's own example, then needs the marker.
4. **No partial moves.** Moving out of a field, an element, `*r` or a
   capture is `move-out-of-place`; code swaps with `std.ops.replace`?
   **Recommended: yes**; Rust allows field moves out of a local.
5. **By-value `self` traits.** Read-only std trait methods move to `&self`
   (and `other: &Self`), in three slices: `Index`, then `Eq`/`Ord`/`Hash`,
   then `Display`/`Debug`/`Serialize`? **Recommended: yes.**
6. **Cleanup.** `using` skips the dispose on an exit where its binding
   moved, as Rust's drop flags do? **Recommended: yes**; otherwise moving a
   `using` binding is `use-of-moved-value`.
7. **Std types.** No existing type becomes non-copyable; `List` and `Map`
   take only copyable items for now, as Swift's `Array`; the planned
   `SharedBytes` is `~Copyable`? **Recommended: yes.**
8. **Matching through a reference.** A non-copyable payload binds as a
   reference to it instead of a copy, as Rust's binding modes do?
   **Recommended: yes.**

## Problems It Solves

**1. A photo handed to a worker (EXECUTION_BOUNDARIES example 1).** Today
`Image` copies, so the crossing copies 50 MB and the page may keep using
it. Transfer is for non-copyable types only (owner, 2026-10-31), and no
owned type is non-copyable. With `Photo < ~Copyable`, the call above moves
`photo`, and a later use is a compile error.

**2. A shared-memory chunk used after it was sent.** The planned
`SharedBytes` closes itself at run time when sent (`PARALLELISM_AND_LOCALITY.md`
§3.2). A declared non-copyable handle makes the mistake a compile error.

```text
fn ship!(to: Sender[SharedBytes], size: usize) -> void:
    chunk := SharedBytes::alloc(size)
    fill(&chunk)
    to.send!(chunk)  # transfer: chunk moves
    fill(&chunk)     # today: run-time Closed; proposed: error: use-of-moved-value
```

**3. Generic code duplicates a `SpanMut`.** `copies` says every type
parameter copies, so this checks today and gives two writers of one loan.

```text
fn dup[T](x: T) -> (T, T): (x, x)

fn split(items: &mut List[i32]) -> void:
    span := items.span_mut()
    let (a, b) = dup(span)  # today: accepted; proposed: error: not-copyable
```

Four more holes check today (Appendix D): a field read `h.target`, an
element read `hs[0]`, `*r` through a `&`, and matching one binding twice.
Rule 4 below closes them.

**4. Indexing a span twice (#1140).** `Index.index(self, key)` consumes a
non-copyable receiver. With `index(&self, key)` both reads borrow.

```text
fn first_two(items: &mut List[i32]) -> void:
    span := items.span_mut()
    a := span[0]
    b := span[1]  # today: error: use-of-moved-value; proposed: valid
```

## How It Works

1. **Copyable.** These are not copyable: `&mut T`, a struct, enum or type
   parameter declared `< ~Copyable`, and a type holding one. "Holds" is M2's
   [`ref.move.holds`](../../spec/lang/13-values-and-references.md#r-ref.move.holds):
   a field, a payload, a tuple, optional or union member, a type argument
   held by value, or a closure's capture. `&T`, `*T` and `*mut T` copy, so
   `box(photo)` moves the photo into a counted object that may be shared.
2. **Moves.** The sites, the reborrow of `&mut` and `use-of-moved-value`
   are M1 and M2's, unchanged. A crossing is one more move site
   (EXECUTION_BOUNDARIES). A by-value `self` or `mut self` receiver takes
   the value, as in Rust.
3. **Type parameters.** Every generic parameter is copyable by default: of
   a function, struct, enum, `impl` block or method. An argument for one
   must be copyable. Error: `not-copyable`. `< ~Copyable` lifts the bound,
   and the body moves such a `T` like any non-copyable value. A trait's
   default bodies treat `Self` as possibly non-copyable.
4. **Places.** Moving a non-copyable value out of a field, an element,
   `*r`, or a capture in a closure body is an error.
   Error: `move-out-of-place`. `&place` and `&mut place` lend it, and
   `replace(&mut place, value)` returns the old value. A `match` or `let`
   pattern on a whole binding moves the binding.
5. **Captures.** A closure that captures a non-copyable binding moves it
   at creation, whether the binding is `:=` or `let`. The closure stays
   callable many times ([`fn.capture.call.any`](../../spec/lang/07-functions.md#r-fn.capture.call.any)),
   so its body only lends the capture.
6. **Patterns through references.** Below a `&` or `&mut`, a pattern binds
   a non-copyable payload as a reference of the same kind. It is a copy
   for a copyable payload, as [`ref.match.copy`](../../spec/lang/13-values-and-references.md#r-ref.match.copy) says today.
7. **Cleanup.** No destructor runs; the counting runtime frees memory as
   today. `using` disposes at an exit only if its binding still holds its
   value there. Where it moved on only some paths, a hidden flag decides.

```text
struct Client < ~Copyable:  # hypothetical syntax
    conn: Conn

fn keep[T](x: T) -> void: pass
fn hold[T < ~Copyable](x: T) -> void: pass  # hypothetical syntax

fn close(client: &mut Client) -> Conn:
    replace(&mut client.conn, .Closed)  # `client.conn` alone: error: move-out-of-place

fn demo(c: &mut Counter, photo: Photo) -> void:
    keep(c)      # error: not-copyable; today c moves into keep
    hold(c)      # valid: c moves
    hold(photo)  # valid: photo moves
```

`std.ops.replace` is new, a stdlib-tier item like Rust's `mem::replace`.
Question 5's traits read:

```text
pub trait Index[K]:
    type Out
    fn index(&self, key: K) -> Self::Out

pub trait Eq:
    fn eq(&self, other: &Self) -> bool
```

## Cost

Estimated bottom-up from the files named; nothing is measured yet.

| Component | Change | Estimate |
| --- | --- | --- |
| Parser, `hd fmt`, item flag | `< ~Copyable` on struct and enum headers and in bound lists | +60 lines |
| `copies` | read the flag and the parameter bound | +15 lines |
| Generic default | `not-copyable` at each instantiation | +90 lines |
| Places, captures, patterns | rules 4 to 6 in the move walk | +210 lines |
| `using` and moves | skip, or a flag where the move is conditional | +40 lines, one local per such binding |
| `Index` to `&self` | the trait, 41 std impls, 35 elsewhere, ch. 05 | about 80 one-line edits |
| `Eq`, `Ord`, `Hash` to `&self` | 74 std impls, 66 elsewhere, the derive templates, `==` passes `&` | about 160 edits, +20 compiler lines |
| `Display`, `Debug`, `Serialize` and the rest | 123 std impls, 164 elsewhere, the derive templates | about 300 edits |
| `Option`, `Result` | an `impl[T < ~Copyable]` block for the methods every payload allows | about 20 methods move |
| Compile time | one flag per type; the checks join the existing move walk | not measurable, expected |
| Runtime and Wasm size | none: a move is the same bits as a copy, with no count change | 0 |
| Web compiler gzip | about 400 Rust lines | about +2 KB, far under the 2 MB cap |

Two compiler gaps: `~` is not a type token (`syntax-error: expected a type
name, found '~'`). A struct header rejects `<` (`expected ':', found '<'`).

## Appendix A. Stress Test

Each case is written in the proposal; "today" is the current build.

| # | Case | What happened | What the design does |
| --- | --- | --- | --- |
| 1 | Rust `std::fs::File`: a handle owns an fd; `using f = open!(p)?` then `archive(f)` hands it on | Without rule 7 the dispose at scope exit is a use after the move: an error on the most common hand-off | Rule 7: the hand-off path skips the dispose; the receiver disposes |
| 2 | `PARALLELISM_AND_LOCALITY.md`'s chunk loop: `using chunk = alloc(..)`, `read_into!(..)?`, `send!(chunk.take(n))` | Read with `using`, the early `?` disposes and the send skips it, which is right. With `:=`, the `?` path leaks the block | Keep `using` (rule 7). `unbound-disposable` already denies the `:=` form |
| 3 | Swift SE-0390's `FileDescriptor` with `consuming func close()` | hd's `self` is "the method's own copy"; [`fn.method.no-ownership-receiver`](../../spec/lang/07-functions.md#r-fn.method.no-ownership-receiver) says no receiver takes ownership | Rule 2: for a non-copyable type, `self` takes the value. That rule and the receiver table change |
| 4 | A tokio-style state machine: `self.state = match self.state: ...` | Moves out of a field: `move-out-of-place` | `match replace(&mut self.state, .Closed):`, Rust's `mem::replace` idiom |
| 5 | A pool of connections, `List[Conn]` with `Conn < ~Copyable` | `List`'s `T` is copyable by default: `not-copyable`. No container of non-copyable values can be written in v1 | Accepted gap (question 7). Swift 6.0 has the same gap for `Array`. A later design gives `List` a `~Copyable` API |
| 6 | `let p = photo; f := fn() -> u32: p.width; send(p); f()` | A `let` capture is a shared cell, so `f()` would read a moved value | Rule 5: the capture moves `p`, so `send(p)` is `use-of-moved-value` |
| 7 | `t.lead` through `t: &Team` where `lead: Photo` | Shallow `&` (#535) would hand out a copy | Rule 4: `move-out-of-place`; `&t.lead` lends it |
| 8 | `@derive(Eq)` on a `~Copyable` struct before slice 5 | The derived `eq(self, other)` consumes both sides: `a == b && a == b` is `use-of-moved-value` (probe p6, with a `&mut` holder) | Slice order: `Eq` moves to `&self` before user types can opt out |
| 9 | `photo.is_some()` on `Photo?`, `opt.map(f)` | `is_some(self)` consumes; `impl[T] T?` takes copyable `T` only | Methods every payload allows move to `impl[T < ~Copyable] T?`; `is_some` takes `&self` |
| 10 | `for x in [photo]` | A list literal of a non-copyable type | `not-copyable`, as 5 |
| 11 | `b := box(photo); c := *b` | `*b` would copy the photo out of a shared box | `move-out-of-place`; boxes share, they never duplicate |
| 12 | ch. 13's `keep(c)` and std calls that pass a `&mut` to a plain `T`, as `.expect` on `get_mut`'s `(&mut T)?` | Valid today (a move); `not-copyable` under rule 3 | Mark `Option` and `Result` first; the generic slice lists every flipped site and stops past 15 |
| 13 | A `pub struct` gains a non-copyable field in a patch release | Every caller that copies it breaks, with no change at the declaration (question 2) | `RELEASE.md`'s API comparison reports copyability changes as breaking |

## Appendix B. Prior Art, With Code

**Swift** ([SE-0390](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0390-noncopyable-structs-and-enums.md),
[SE-0427](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0427-noncopyable-generics.md),
[SE-0432](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0432-noncopyable-switch.md)):

```swift
struct FileDescriptor: ~Copyable {
    private var fd: Int32
    consuming func close() { ... }
    deinit { ... }
}
func use<T: ~Copyable>(_ x: borrowing T) { ... }   // T defaults to Copyable without `~`
enum Box<T: ~Copyable>: ~Copyable { case some(T) }
extension Box: Copyable where T: Copyable {}
```

Every type is copyable unless it suppresses it, and every generic
parameter and protocol defaults to `Copyable`. hd takes the default and the
`~Copyable` spelling. It rejects the explicit holder marker and the
conditional `extension` (question 2), and `deinit`. Swift 6.0 kept
`Array`, `Equatable` and `Hashable` copyable-only.

**Rust** ([moves](https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html),
[`mem::replace`](https://doc.rust-lang.org/std/mem/fn.replace.html),
[`Index`](https://doc.rust-lang.org/std/ops/trait.Index.html)):

```rust
let a = photo;            // moves; `photo` is unusable (E0382)
let t = &team;
let l = t.lead;           // E0507: cannot move out of `t.lead`
let old = std::mem::replace(&mut self.state, State::Closed);
trait Index<Idx> { type Output; fn index(&self, i: Idx) -> &Self::Output; }
trait PartialEq<Rhs = Self> { fn eq(&self, other: &Rhs) -> bool; }
```

Everything moves; `Copy` is an explicit derive. hd keeps copy as the
default and takes E0382, E0507, `replace` and the `&self` read traits.
Rust's drop flags are rule 7's model. Rust allows moving a field out of a
local; hd does not (question 4).

**Go** ([copylocks](https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/copylock)):

```go
type noCopy struct{}
func (*noCopy) Lock()   {}
func (*noCopy) Unlock() {}

type WaitGroup struct { noCopy noCopy; ... }
// go vet: "assignment copies lock value"
```

Go copies everything and catches "must not copy" with a vet lint. hd's M1
and M2 already went further: a type error, not a lint.

**JavaScript** ([transferable objects](https://developer.mozilla.org/en-US/docs/Web/API/Web_Workers_API/Transferable_objects)):

```js
const buf = new ArrayBuffer(50_000_000);
worker.postMessage(buf, [buf]);  // transfer
buf.byteLength;                  // 0: detached at run time
```

The sender's buffer is detached at run time. `SharedBytes` copies this
today; a non-copyable `SharedBytes` turns the detach into a compile error.

## Appendix C. Alternatives Considered

- **Rust's default: `T` moves unless `T < Copy`.** Most std generic bodies
  copy `T` (`fill`, `unwrap_or`, `map`). The owner fixed copy as the default
  (2026-10-19); a `Copy` bound everywhere was rejected then.
- **Check copies per instantiation.** It catches `dup[SpanMut]` without a
  bound, but only after substitution. The owner rejected it on 2026-10-19
  to keep header-only checking.
- **Run-time detach only, as JavaScript.** It is the planned `SharedBytes`.
  It finds the bug in production, and transfer of a user type stays
  impossible.
- **`self` borrows when the type is non-copyable (Swift's default `self`).**
  The std traits would stay as written. But `self`'s meaning would depend
  on the type, and `thumbnail(photo) -> (photo, small)` needs a consuming
  form anyway.
- **A call-site `transfer(x)` marker instead of a type property.** Owner,
  2026-10-31: transfer is for non-copyable types only.
- **Traits default `Self` to copyable (Swift).** The anti case: Swift 6.0
  could not make a non-copyable type `Equatable` until the protocol was
  reopened. In hd, `[T < Eq]` already makes `T` copyable by rule 3, so the
  trait needs no default. The trade-off: a default body never copies `self`.
- **A required marker on every holder (Swift).** It shows the property at
  the declaration. It also needs Swift's conditional `extension ... where
  T: Copyable` for generic holders, and M2 already infers for `&mut`.

## Appendix D. Today In hd

- **The rule.** Every type copies except `&mut T` and a type holding one
  ([`ref.move.mut`](../../spec/lang/13-values-and-references.md#r-ref.move.mut),
  [`ref.move.holds`](../../spec/lang/13-values-and-references.md#r-ref.move.holds)).
  `copies` in `crates/hd_check/src/moves.rs` walks fields, payloads and
  members, and returns `true` for a type parameter and for `List` and `Map`.
- **Receivers.** `self` is "the method's own copy"
  ([`fn.method.receiver.self`](../../spec/lang/07-functions.md#r-fn.method.receiver.self)),
  and no receiver takes ownership
  ([`fn.method.no-ownership-receiver`](../../spec/lang/07-functions.md#r-fn.method.no-ownership-receiver)).
- **By-value `self` std traits** (`lib/std`): `Index`, `Eq`, `PartialOrd`,
  `Ord`, `DeepEq`, `Hash`, `Display`, `Debug`, `Serialize`, `MapKey`,
  `Inspectable`, `Error.source`, `Iterable.iter`, `Apply`, `Termination` and
  the operator traits. Operators, `Iterable` and `Apply` consume in Rust
  too, and stay.
- **Cleanup.** `using` registers `dispose(&mut self)`
  ([`flow.using.place.v2`](../../spec/lang/06-control-flow.md#r-flow.using.place.v2)).
  `unbound-disposable` denies a disposable bound by `:=` or `let`.

Probes on the current build (`target/debug/hd check`, at 78c1ee262), each
with a `struct Holder['a]` holding a `&'a mut List[i32]`:

| Probe | Code | Result |
| --- | --- | --- |
| p1 | `dup(span)` with `fn dup[T](x: T) -> (T, T): (x, x)` | accepted: two writers |
| p1 | `span[0]` then `span[1]` on a `SpanMut` | `use-of-moved-value` (#1140) |
| p3 | `t := h.target` then `take(h)` | accepted: a field copied out |
| p4 | `hs[0]` twice, `hs: List[Holder]` | accepted: `List[Holder]` copies |
| p5 | `match opt` twice, each arm moving the payload | accepted |
| p6 | `x == y && x == y`, `@derive(Eq)` on a holder | `use-of-moved-value`: `eq(self, other)` consumes |
| p7 | `let mut a = *r; let mut b = *r` with `r: &Holder` | accepted: two copies out of a `&` |
| p8 | `o := .Some(holder)` then `o2 := o; o3 := o` | `use-of-moved-value`, as M2 says |

## Appendix E. Details

**Language checklist.**

- Grammar and formatting: `< ~Copyable` after a struct or enum's name and
  generics, and as a bound `T < ~Copyable` or `T < Eq & ~Copyable`;
  `hd fmt` keeps it in place.
- Typing: use sites only; nothing is inferred on a declaration except
  question 2's holder rule.
- Patterns: rule 4 (a pattern on a binding moves it) and rule 6.
- Derives: the derive templates follow question 5's receivers.
- Closures: rule 5; no `FnOnce` is needed.
- Modules: copyability is part of a public type's API (stress case 13).
- Ownership and the inliner: moves are checked before TIR; the inliner and
  the counting runtime see the same code as today.
- Soundness: rules 3 and 4 close the five holes of Appendix D for `&mut`
  holders too.
- Diagnostics: `not-copyable` and `move-out-of-place` are new; messages
  name the field or parameter that makes a type non-copyable.
- Spec: ch. 13 Moves (rules 1 to 7), ch. 04 Value Types, ch. 07 receivers
  and captures, ch. 05 `Index`, ch. 06 `using`, `spec/std/ops.md` for
  `replace`; fixtures per rule, valid and invalid.
- No-feature alternative: `box` the value and rely on run-time `Closed`.

**Implementation slices,** each about an hour with its fixtures:

1. `Index.index(&self)`, with #1140's `SpanMut` API.
2. Syntax, the item flag and `copies`; a user struct moves.
3. Places (rule 4) and `std.ops.replace`.
4. Patterns: a match moves its binding; binding through a reference (rule 6).
5. `Eq`, `Ord`, `Hash` to `&self`, with the derive templates.
6. Generic default and `~Copyable` parameters; `Option` and `Result`
   blocks; `List` and `Map` reject non-copyable items.
7. Captures (rule 5).
8. `using` and moves (rule 7).
9. `Display`, `Debug`, `Serialize` and the remaining read traits.

**Parse log.** Each `text` block was parsed with
`scripts/hd-conformance.sh parse`; parsing checks syntax only.

| Block | Result |
| --- | --- |
| 1, the proposal | `syntax-error` at line 1 (`< ~Copyable`, hypothetical); parses with the marker removed |
| 2, `ship!` | parses |
| 3, `dup` | parses; also checks today (`hd check`: ok) |
| 4, `first_two` | parses; `hd check` today: `use-of-moved-value` at line 4 |
| 5, rules | `syntax-error` at lines 1 and 5 (`< ~Copyable`, hypothetical); parses with the marker removed |
| 6, `Index` and `Eq` | parses |
