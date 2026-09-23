# Ownership, Escape, And Compile-Time Concurrency Research

## Table Of Contents

- [Status](#status)
- [Executive Summary](#executive-summary)
- [Comparison At A Glance](#comparison-at-a-glance)
- [Scenario Coverage Matrix](#scenario-coverage-matrix)
- [Shared Vocabulary](#shared-vocabulary)
- [Concept Surface](#concept-surface)
  - [Other Languages And Narrow Designs](#other-languages-and-narrow-designs)
- [Main Findings By Language](#main-findings-by-language)
  - [Rust](#rust-1)
  - [Swift](#swift-1)
  - [Scala Capture Checking](#scala-capture-checking)
  - [Kotlin Local Lifetimes](#kotlin-local-lifetimes-1)
  - [Pony Reference Capabilities](#pony-reference-capabilities)
- [Tentative hd-lang Preferences](#tentative-hd-lang-preferences)
- [What A Narrow Non-Escape Checker Would Prove](#what-a-narrow-non-escape-checker-would-prove)
- [Potential Design Shapes](#potential-design-shapes)
- [Interactions That Must Be Designed Together](#interactions-that-must-be-designed-together)
- [Reasons To Defer](#reasons-to-defer)
- [Conditions For Reopening The Design](#conditions-for-reopening-the-design)
- [Questions For Later Investigation](#questions-for-later-investigation)
- [Production Evidence And References](#production-evidence-and-references)

## Status

This document records research and tentative design preferences for possible future hd-lang work. It does not define accepted syntax or semantics.

The current language design remains unchanged:

- Managed composite values use Wasm GC.
- `T` versus `mut T` expresses read versus write permission through a reference path.
- `mut` does not express ownership, exclusive access, lifetime, deep immutability, or concurrency safety.
- Multiple mutable aliases may exist.
- `$` capabilities describe external authority and dependency availability. They are unrelated to Pony-style reference capabilities.

Ownership, borrowing, resource lifetime, non-escape checking, sendability, and data-race prevention should be revisited only after more foundational features are implemented.

## Executive Summary

The surveyed systems overlap, but they solve different static-validation problems:

- **Rust** is primarily a temporal ownership system. It tracks who owns a value, which accesses overlap, whether a reference remains valid, and whether a type can be transferred or shared across threads through `Send` and `Sync`.
- **Swift** combines a value-ownership system with a separate concurrency-isolation system. Its common path is highly inferred, but the complete model includes copyability, consuming and borrowing conventions, non-escapable values, sendability, isolation regions, and actors.
- **Scala capture checking** tracks retained authority: which capabilities a value or closure may keep. Its separation checker selectively adds exclusivity and non-interference. It is not a global ownership or lifetime system.
- **Kotlin local lifetimes** is an early research design for tracking values that are safe only for a limited lifetime. Its `local` parameters provide a small non-escape entry point, while dependent result lifetimes and `local class` declarations propagate restrictions through closures, views, and aggregates. It does not add ownership, exclusivity, or concurrency transfer.
- **Pony** uses reference capabilities to constrain mutation, aliasing, and actor transfer. It is not a lexical lifetime or use-after-close system, but it demonstrates why consuming one binding is not enough for exclusive transfer unless the type system also constrains other aliases.

Among mature implemented general-purpose systems covering lifetime validity, aliasing, ownership transfer, and concurrency, **Rust has the smallest effective concept surface**. Its essential model is ownership and moves, shared/exclusive borrowing, lifetime relationships, and structural `Send`/`Sync` traits. Most local facts are inferred. For the narrower problem of preventing escape, Kotlin's proposal has a smaller common entry point—`local`—but its full dependent-lifetime algebra remains research rather than a shipping language feature.

That does not imply that hd-lang should adopt Rust's full model. A narrower resource-escape problem may be better served by Swift-style non-escapability or a small affine/noncopyable discipline. The design should be selected from concrete hd-lang use cases rather than from a goal of matching another language's complete safety story.

## Comparison At A Glance

| Property | Rust | Swift | Scala capture/separation | Kotlin local lifetimes | Pony |
|---|---|---|---|---|---|
| Dangling-reference prevention | Yes for safe references | Checked borrows and non-escapable views; ordinary classes use ARC | Capability escape only, not general pointer lifetime | Proposed local values and dependent results cannot outlive their tracked lifetime; not general memory ownership | No source-level borrowed-lifetime discipline |
| Use after ownership transfer | Rejected | Rejected for noncopyable values and explicit consumption | Selective `consume` under separation checking | No ownership transfer or consumption rule | A consumed binding becomes unusable |
| Mutable alias control | Shared-or-exclusive borrowing | `inout` exclusivity and ownership modes; deep aliases can remain | Only for modeled exclusive capabilities | None; locality is independent of mutation and aliasing | Encoded by reference capabilities, especially `iso`, `trn`, `ref`, and `box` |
| Concurrent immutable sharing | `Sync` | `Sendable` plus isolation | Shared/read-only capabilities when modeled | No special answer | Deeply immutable `val` graphs may be shared |
| Concurrent transfer | `Send` | `Sendable` or disconnected `sending` value | Requires explicit capability/separation modeling | No special answer | `iso` plus `consume` transfers an isolated mutable graph |
| Ownership-driven deterministic cleanup | `Drop` at ownership end; deliberate leaks remain possible | `deinit` on noncopyable values; ordinary classes remain ARC-managed | No general ownership-driven cleanup | No general ownership-driven deterministic destruction | No general ownership-driven deterministic destruction |
| Closure escape checking | Captured borrows and moves follow ordinary lifetime rules | `~Escapable`, ownership, and concurrency rules propagate into captures | Closure capture sets record retained capabilities | Proposed restricted function lifetimes and result dependencies track retained callbacks | No temporal closure-lifetime check; captures retain capability-qualified aliases |
| Runtime fallback for the same check | `RefCell` can defer borrow conflicts to runtime | Some exclusivity checks can be dynamic | None within capture/separation checking | None in the proposed static discipline | None within the reference-capability checker |
| Ordinary annotation burden | Low | Very low | Low after opting in | Intended to be very low for callers; `local` appears at API boundaries | Low inside an actor; visible at transfer boundaries |
| Advanced API burden | Medium to high | High to very high | High | High | High |
| Production maturity | Very high | High for concurrency; newer for ownership APIs | Experimental | Research design note; not a formal proposal or compiler feature | Real systems exist, but Pony remains pre-1.0 with a small ecosystem |

Only compile-time or static-verifier guarantees are relevant here. Garbage collection, ARC, dynamic exclusivity checks, runtime locks, actor schedulers, and runtime race detectors do not count as static validation.

## Scenario Coverage Matrix

This matrix maps the same practical questions onto each language. “Not applicable” means that the language does not attempt a temporal lifetime proof for that scenario, not that the operation is universally safe.

| Scenario | Rust | Swift | Scala capture/separation | Kotlin local lifetimes | Pony |
|---|---|---|---|---|---|
| Transfer or consume | Move; source becomes unusable | `consuming`/`consume`, observable for `~Copyable` | Selective `consume` under separation checking | No answer; `local` does not consume and the caller may keep using its argument | `consume` invalidates a binding; `iso` supplies the alias guarantee needed for exclusive transfer |
| Read-only temporary access | `&T` shared borrow | `borrowing`; `inout` for exclusive mutation | Bare stateful type or `x.rd` read capability | No read-only mode; a local reference keeps its ordinary Kotlin permissions | `box` is a read-only viewpoint, not a temporal borrow |
| Escape by return | Lifetimes tie a returned reference to inputs; invalid local return rejected | `~Escapable` result needs a lifetime dependency | Result capture set cannot mention an out-of-scope capability | `T_{x}` ties the result to an input; `T_{x&y}` intersects dependencies | Not applicable: returned references retain capabilities but have no lexical lifetime dependency |
| Escape by closure | Closure cannot outlive captured borrows; `move` can transfer owned captures | Captures obey ownership, escapability, and isolation rules | Closure type records captured capabilities | `(...) ->_{x} R` restricts a function value to lifetime `x`; returned wrappers carry dependencies | Not applicable temporally; captured references retain their capability restrictions |
| Escape by field or aggregate | Stored references carry lifetime parameters | Containing struct/enum must propagate non-escapability | Capture sets propagate through classes and generic containers | A `local class` may store fields such as `T_{this}`, restricting the aggregate's lifetime | Viewpoint adaptation prevents an aggregate from exposing stronger authority than its receiver permits |
| Concurrency transfer | `Send`; shared access additionally needs `Sync` | `Sendable` or one-time disconnected `sending` | Must be modeled with capture and separation capabilities | No answer; the proposal does not model thread or actor transfer | `iso` transfers mutable graphs; `val` shares immutable graphs; `tag` shares identity |
| Ownership-driven cleanup | `Drop` follows ownership on normal control flow | Noncopyable `deinit` follows value ownership | No general answer; scoped APIs normally use `try`/`finally` | No ownership-driven answer; locality can strengthen `use`-style scoped APIs | No answer; reference capabilities do not encode an `Open` to `Closed` transition |

The walkthroughs below cover each applicable cell. When a language has no corresponding static lifetime feature, its section says so rather than forcing an analogy.

## Shared Vocabulary

The four main languages use different words for several recurring ideas. The following terms are used consistently in this document:

- **Owner:** the program location responsible for a value. Moving ownership changes that location; borrowing does not.
- **Alias:** another reference or path that reaches the same value. Aliases matter because mutation or cleanup through one path may invalidate assumptions made through another.
- **Move or consume:** transfer a value so the source can no longer be used. This is a compile-time rule; it does not necessarily mean copying bytes in memory.
- **Borrow:** grant temporary access while ownership remains elsewhere. A shared borrow normally permits reading; an exclusive borrow permits mutation while excluding competing access.
- **Reborrow:** derive a usually shorter borrow from an existing borrow. This temporarily changes how the original reference may be used without moving its underlying ownership.
- **Lifetime:** the portion of the program during which a borrow or capability may validly be used. It is a static relationship, not a runtime timer. This document avoids using bare “region” as a synonym because Swift's **isolation region** is different: it is a set of values connected by possible aliases for concurrency analysis, not the lifetime of a borrow.
- **Escape:** make a value reachable from a place that may live longer than its permitted lifetime—for example, by returning it, storing it in a longer-lived location, or capturing it in a long-lived closure.
- **Affine:** a value may be used at most once but may also be discarded. Rust moves and Swift noncopyable values are broadly affine; Kotlin local lifetimes are not.
- **Linear:** a value must be used exactly once. This is stricter than affine usage, which permits discarding the value.
- **Deep immutability:** neither the current reference nor any other reference can mutate the reachable object graph. This is stronger than a read-only view.
- **Sendability:** permission to cross a concurrency boundary. A value can be locally valid yet still be unsafe to transfer or share between threads or actors.
- **Capability:** a reference that represents authority, such as permission to access a file, mutate state, or invoke an effect. Scala and Pony make this authority especially visible in types.
- **Auto trait:** a Rust trait such as `Send` or `Sync` whose implementation is normally derived structurally from a type's fields instead of being written by the programmer.
- **Typestate:** representing an object's protocol state in its static type so invalid operations—for example, reading a closed handle—are unrepresentable at that program point.
- **Ephemeral reference:** Pony's temporary post-`consume` form, written with `^`, which preserves stronger alias guarantees long enough to transfer or recover a value.

The common intuition is that cleanup and concurrency are hard whenever multiple paths can keep using the same state. Each language therefore limits either **when** a path is valid, **what** it can do, **where** it may travel, or some combination of the three.

## Concept Surface

Concept counts are approximate families a programmer must understand, not literal keyword counts.

### Rust

The common surface is:

1. Ownership and moves.
2. `Copy` versus explicit `Clone`.
3. Shared borrowing with `&T`.
4. Exclusive borrowing with `&mut T`.
5. Inferred or named lifetime relationships.
6. `Send` for transfer and `Sync` for sharing.

Advanced libraries add higher-ranked lifetimes, variance, interior mutability, `Pin`, explicit `'static` bounds, and unsafe implementations. Rust's ordinary ownership rules compose consistently, but the full system is deliberately layered rather than unified: lifetimes validate temporal access, `Send` and `Sync` are orthogonal auto traits, `Pin` concerns address stability, and interior mutability moves selected conflicts behind library abstractions. Effective consistency remains high for callers because inference hides most layers; library authors see the distinctions explicitly.

### Swift

Ordinary Swift hides most of the machinery, but the complete surface includes:

1. `Copyable` and `~Copyable`.
2. `borrowing`, `consuming`, and `inout`.
3. Explicit `copy` and `consume`.
4. `Escapable` and `~Escapable`.
5. Lifetime dependencies for borrowed views.
6. `Sendable` and `@Sendable`.
7. `sending` and region-based isolation.
8. Actors, global actors, `isolated`, and `nonisolated`.

Swift often has the least visible syntax but not the fewest concepts. Its ownership, escape, and concurrency layers evolved separately.

### Scala

The comparable surface is:

1. Capabilities and capture sets such as `T^{x}` and `T^`.
2. Pure `A -> B` versus capturing `A => B` function arrows.
3. Lexical capability scope and capture propagation.
4. Stateful, mutable, shared, and exclusive capability classification.
5. Read-only projections such as `x.rd` and mutating `update` methods.
6. Separation, `consume`, `fresh`, and `freeze` for selective alias control.

The algebra is general and attractive for effect or authority tracking, but the full system is experimental and substantially larger than its basic escape-checking story.

### Kotlin Local Lifetimes

The comparable surface is:

1. `local` parameters and receivers.
2. Lifetime-restricted types such as `T_{x}`.
3. Lifetime-restricted functions such as `(A) ->_{x} B`.
4. `local class` declarations for aggregates with restricted fields.
5. Implicit `this`, current-call `local`, and unrestricted `global` lifetimes.
6. Lifetime intersections and advanced locality parameters with bounds.

The common path is deliberately small: callers mostly encounter ordinary Kotlin, while library signatures add `local`. The complete proposal becomes substantially more sophisticated when a library returns lazy values, stores local fields, or writes explicit locality-polymorphic APIs. Unlike Rust or Swift ownership, the system does not control copying, mutation, or transfer.

### Pony

The comparable surface is:

1. Six reference capabilities: `iso`, `trn`, `ref`, `val`, `box`, and `tag`.
2. Alias guarantees associated with each capability.
3. `consume` and ephemeral transfer forms.
4. `recover` expressions for constructing isolated or immutable graphs.
5. Receiver capabilities and destructive reads.
6. Viewpoint adaptation for fields and generic abstractions.

Pony unifies local mutation rules with actor-safe sharing and transfer. Its conceptual cost is a capability matrix plus alias and recovery rules. This is a concurrency and aliasing model rather than a lexical lifetime or resource-state model.

### Other Languages And Narrow Designs

This subsection is intentionally a pointer list rather than a second tutorial. It does not include code examples; each design would need its own version-sensitive treatment to make small snippets genuinely comparable.

- **Austral** has the smallest deliberately designed theoretical core: free versus linear values and region-bound read/write borrowing. It is explicit, restrictive, and not production-mature.
- **Move** has a small resource-oriented model: `copy`, `drop`, `store`, and `key` abilities plus ephemeral `&`/`&mut` references. It remains small partly because stored references are forbidden and the language solves a narrower problem.
- **Clean** uses inferred uniqueness attributes. The source can be terse, while higher-order uniqueness propagation remains conceptually complex.
- **Linear Haskell** has a small central idea—multiplicity on function arrows—but practical use requires a parallel linearity-aware library vocabulary.
- **Mojo** uses explicit ownership conventions such as borrowed, inout, and owned arguments, together with origin tracking for borrowed results. It is relevant as a newer systems-language design, but its language and documentation are still evolving too quickly to treat it as a stable baseline.

## Main Findings By Language

The Rust examples target rustc 1.97 and should work on recent stable Rust. The Swift examples target Swift 6.3.3; `Span` and `~Escapable` require Swift 6.2 or later, and the positive lifetime-dependency example additionally enables the experimental `Lifetimes` feature. The `inout` overlap and lifetime-escape errors are emitted only in a full compile, not by `swiftc -typecheck`. Scala examples target the experimental capture/separation checker used by Scala 3.9.0. Kotlin local lifetimes is research in progress rather than a released Kotlin compiler feature, so its blocks illustrate the design note's proposed syntax and cannot currently be compiled or assigned real diagnostics. Pony examples target the current Pony 0.69-era reference-capability syntax; Pony remains pre-1.0.

### Rust

#### Overall Idea

Rust treats every nontrivial value as a responsibility held by some owning place. Passing that value somewhere else normally transfers the responsibility, so the old place becomes unusable. When code only needs temporary access, it borrows instead: many readers may coexist, or one exclusive writer may act, but conflicting access may not overlap.

A useful mental picture is an owned box with temporary loan tickets. Moving the box transfers both the contents and the obligation to clean them up. A reference is only a ticket; the compiler must prove that the box remains present for the whole time the ticket can be used. Rust lifetimes describe that proof relationship rather than manually scheduling destruction.

This one model explains use-after-move rejection, dangling-reference prevention, and much mutation safety. Thread transfer is a separate structural question expressed by `Send` and `Sync`. Rust does not automatically encode every logical resource state—such as open versus closed—but ownership, consuming operations, `Drop`, and typestate can be used to build those protocols.

#### Beginner Roadmap

Read the Rust walkthroughs in this order:

1. A variable such as `message: String` owns its value. Passing it by value normally moves that ownership.
2. `&T` temporarily reads without taking ownership; `&mut T` temporarily mutates while excluding conflicting access.
3. A lifetime such as `'a` connects the validity of references. It does not keep an object alive or perform cleanup.
4. `Drop` runs when ownership ends on ordinary control flow.
5. `Send` asks whether ownership may cross a thread boundary; `Sync` asks whether shared references may cross one.

When reading Rust code, first locate the owner, then identify borrows, and only then inspect explicit lifetime names. Most programs rely on inferred lifetimes, so apostrophe syntax is not the starting point of the model.

#### Core Static Model

Rust begins with ownership rather than shared references. Every ordinary non-`Copy` value has one owning place. Library types such as `Rc` and `Arc` can share ownership of an allocation, but each handle is still a separately owned value whose clone is explicit. Assignment, argument passing, and returning a value normally move ownership, invalidating the previous place. `Copy` permits implicit duplication for types whose duplication is intended to be cheap and semantically unsurprising; `Clone` makes other duplication explicit. `Drop` connects ownership loss to deterministic cleanup, although Rust remains affine: a value may be discarded, and deliberate leaks are possible.

Borrowing temporarily grants access without transferring ownership:

- `&T` is a shared borrow. Multiple shared borrows may coexist, but ordinary mutation through them is forbidden.
- `&mut T` is an exclusive borrow. While it remains live, competing access to the same borrowed place is forbidden.
- Reborrowing can temporarily derive a shorter borrow from an existing reference without transferring the original reference permanently.
- The compiler can often borrow disjoint fields or disjoint slices independently.

The most important point is that the checker reasons about access paths and overlapping use, not just variables. A mutable borrow of one struct field need not block an unrelated field, while two indices into an arbitrary slice are conservatively considered possibly overlapping unless an API such as `split_at_mut` proves separation.

#### Feature Walkthrough: Moves, Copies, And Clones

A move gives a new place responsibility for a value. The old variable becomes unusable, so two parts of the program cannot both believe that they own its eventual destruction:

```rust
fn print_owned(message: String) {
    println!("{message}");
}

fn main() {
    let message = String::from("ready");
    print_owned(message);

    // Rejected if uncommented: ownership moved into print_owned.
    // println!("{message}");
}
```

“Move” is a source-language ownership fact, not a promise that the runtime physically relocates bytes. The optimizer may leave the allocation exactly where it is. The important result is that `message` has one usable owner at a time.

Small scalar types opt into `Copy`, so ordinary assignment duplicates them. Heap-owning types such as `String` require an explicit `clone()` when two independent owners are genuinely wanted:

```rust
fn main() {
    let count: u32 = 3;
    let copied = count;
    assert_eq!(count, copied); // u32 is Copy.

    let first = String::from("report");
    let second = first.clone();
    assert_eq!(first, second); // Two separately owned strings.
}
```

This distinction turns potentially expensive or semantically important duplication into a visible review point without making cheap value types noisy.

#### Feature Walkthrough: Ownership-Driven Cleanup

`Drop` attaches cleanup to the end of ownership. The compiler inserts the call on ordinary control-flow exits, including early returns and unwinding:

```rust
struct Trace(&'static str);

impl Drop for Trace {
    fn drop(&mut self) {
        println!("closing {}", self.0);
    }
}

fn main() {
    let _outer = Trace("outer");

    {
        let _inner = Trace("inner");
        println!("using resources");
    } // inner is dropped here.

    println!("inner is already closed");
} // outer is dropped here.
```

This is the RAII intuition: owning the resource and owing its cleanup are the same responsibility, so a move also moves that responsibility. It is deterministic on normal Rust control flow, but it is still not an exactly-once theorem for every execution—safe code can deliberately leak a value, and abrupt process termination need not run destructors.

#### Feature Walkthrough: Shared And Exclusive Borrows

A borrow lets a function use a value without taking it away. `&T` means temporary read access, while `&mut T` means temporary exclusive access:

```rust
fn length(text: &String) -> usize {
    text.len()
}

fn add_suffix(text: &mut String) {
    text.push_str(".log");
}

fn main() {
    let mut name = String::from("server");

    let before = length(&name);
    add_suffix(&mut name);

    assert_eq!(before, 6);
    assert_eq!(name, "server.log");
}
```

The owner remains `main`; neither helper is responsible for destroying the string. Exclusive borrowing is the static reason `add_suffix` can mutate without another ordinary path observing a conflicting access.

Multiple readers may coexist, and the compiler ends their borrows after their final use rather than at the end of the block:

```rust
fn main() {
    let mut values = vec![10, 20];
    let first_reader = &values;
    let second_reader = &values;

    assert_eq!(first_reader[0] + second_reader[1], 30);
    // Both shared borrows have had their last use.

    let writer = &mut values;
    writer.push(30);
    assert_eq!(values, [10, 20, 30]);
}
```

The intuition is not “mutable values may never be aliased.” It is “mutation must not overlap an access that would conflict with it.”

The compiler therefore rejects a mutation that might invalidate a still-live shared reference:

```rust,compile_fail
fn main() {
    let mut values = vec![10, 20];
    let first = &values[0];

    // Rejected: push needs an exclusive borrow while first is live.
    values.push(30);
    println!("{first}");
}
```

```text
error[E0502]: cannot borrow `values` as mutable because it is also borrowed as immutable
```

If the final `println!` is removed or moved before `push`, the shared borrow ends before the mutation and the program is accepted. This last-use sensitivity is why many ordinary programs do not need explicit nested scopes to satisfy the borrow checker.

#### Lifetimes And Escape

Every reference has a lifetime, but the lifetime is normally inferred. A lifetime is not runtime duration and an annotation does not keep a value alive. It is a static relationship such as “this result cannot outlive the input from which it was borrowed.”

Named lifetimes appear mainly when an API exposes a relationship the compiler cannot select from local structure:

- a struct stores a reference;
- a result may be derived from one of several input references;
- a callback must work for any caller-selected lifetime;
- one lifetime must outlive another;
- a trait object or asynchronous task retains borrowed data.

Non-lexical lifetime analysis ends most borrows after their last relevant use, not necessarily at the closing brace. This keeps ordinary code much less verbose than a lexical region system.

`'static` has two related meanings that are easy to confuse. `&'static T` is a reference valid for the program's duration. A bound such as `T: 'static` merely says that `T` contains no shorter-lived borrowed data; an owned `String` satisfies it even if that particular value is immediately dropped.

#### Feature Walkthrough: Lifetime Relationships

Most functions need no named lifetime. A name becomes useful when the return value could be tied to more than one input:

```rust
fn longer<'a>(left: &'a str, right: &'a str) -> &'a str {
    if left.len() >= right.len() { left } else { right }
}

fn main() {
    let first = String::from("elephant");
    let second = String::from("cat");
    let result = longer(&first, &second);

    assert_eq!(result, "elephant");
}
```

`'a` does not request that both strings live equally long. It says that both input borrows are valid for some common period and that the returned borrow is valid for no longer than that period. In practice, the caller experiences the result as being limited by the shorter usable input lifetime.

Returning a reference to a local value has no such valid relationship and is rejected:

```rust,compile_fail
fn bad_reference() -> &'static str {
    let local = String::from("temporary");
    &local
}

fn main() {}
```

The local string is destroyed when the function returns. A lifetime annotation can describe a valid relationship, but it cannot manufacture one or extend an object's runtime existence.

A real diagnostic for that rejected example begins:

```text
error[E0515]: cannot return reference to local variable `local`
```

Named lifetimes also appear when a field stores a reference. The lifetime becomes part of the containing type because every `Parser<'a>` is valid only while its input remains borrowed:

```rust
struct Parser<'a> {
    input: &'a str,
}

impl<'a> Parser<'a> {
    fn input(&self) -> &'a str {
        self.input
    }
}

fn main() {
    let source = String::from("name = value");
    let parser = Parser { input: &source };
    assert_eq!(parser.input(), "name = value");
}
```

The two uses of `'static` can be contrasted in a complete program:

```rust
static LABEL: &str = "ready";
fn needs_no_borrow<T: 'static>(_: T) {}
fn main() {
    let forever: &'static str = LABEL;
    needs_no_borrow(String::from(forever));
}
```

`forever` is itself a reference valid for the program's duration. The owned `String` passed to `needs_no_borrow` is not required to live forever; `T: 'static` only says that its type contains no borrowed reference with a shorter lifetime.

#### Feature Walkthrough: Escaping Closures

A closure normally borrows local variables it uses. Returning that closure would let the borrow outlive the local owner, so this version is rejected:

```rust,compile_fail
fn invalid_counter() -> impl Fn() -> usize {
    let local = String::from("temporary");

    // Rejected: the returned closure may outlive borrowed local.
    || local.len()
}

fn main() {}
```

```text
error[E0373]: closure may outlive the current function, but it borrows `local`, which is owned by the current function
```

`move` fixes the relationship by transferring the owned string into the closure environment:

```rust
fn make_counter() -> impl Fn() -> usize {
    let owned = String::from("persistent");
    move || owned.len()
}

fn main() {
    let count = make_counter();
    assert_eq!(count(), 10);
}
```

The closure now escapes safely because it owns what it needs. `move` does not make every capture thread-safe or `'static`; it only changes capture from borrowing a place to taking its value. The captured types still determine the closure's lifetimes, `Send`, and `Sync` properties.

#### `Send` And `Sync`

Rust keeps cross-thread admissibility separate from borrowing:

- `T: Send` means ownership of a `T` may be transferred to another thread.
- `T: Sync` means shared access is safe across threads; formally, `T` is `Sync` when `&T` is `Send`.

Both are auto traits. A struct made entirely from `Send` fields is normally `Send` without any written conformance, and the same applies to `Sync`. `Rc`, `Cell`, and `RefCell` block the relevant automatic traits; `Arc`, atomics, and synchronization types support them under appropriate bounds. Closures and futures inherit the properties of their captured environment.

This separation is a significant reason Rust's total concept surface remains manageable. References and lifetimes answer whether an access is valid; `Send` and `Sync` answer whether a valid access may cross a concurrency boundary.

Detached work commonly requires `Send + 'static` because it may outlive the caller. Scoped concurrency can instead borrow stack data because the API proves that all child work completes before the scope exits.

#### Feature Walkthrough: Structural `Send` And `Sync`

Most programs never implement `Send` or `Sync`. The compiler derives them from fields and captured values:

```rust
use std::rc::Rc;
use std::sync::Arc;

fn require_send<T: Send>(_: T) {}
fn require_sync<T: Sync>(_: &T) {}

fn main() {
    require_send(String::from("owned data"));

    let shared = Arc::new(String::from("shared data"));
    require_send(shared.clone());
    require_sync(&shared);

    let local_only = Rc::new(String::from("one-thread count"));
    // Rejected if uncommented: Rc's count is not thread-safe.
    // require_send(local_only);
    drop(local_only);
}
```

The intuition is structural permission. `String` owns transferable data, and `Arc<T>` supplies thread-safe shared ownership when `T` has the required properties. `Rc<T>` uses a non-atomic reference count, so merely wrapping safe data in `Rc` makes the whole value non-`Send`. The check follows the representation instead of relying on a convention at each call site.

#### What Rust Proves

Safe Rust statically prevents:

- dangling safe references and use after move;
- two owners of the same non-duplicated value and consequent double destruction;
- overlapping safe mutable access;
- mutation through ordinary shared references;
- transferring a non-`Send` value across a thread boundary;
- sharing a non-`Sync` value through a shared cross-thread reference;
- a scoped borrow outliving the scope that joins its child work.

Rust does not prove:

- that every resource is explicitly closed rather than dropped or leaked;
- deadlock freedom, lock ordering, starvation, or liveness;
- correctness of a multi-step concurrent protocol;
- absence of logical races involving atomics or correctly synchronized state;
- safety inside `unsafe` code, FFI, or an incorrect manual `Send`/`Sync` implementation.

`RefCell` is also relevant to a compile-time-only comparison: it deliberately moves borrow conflict checking to runtime. It is a safe Rust abstraction, but its dynamic borrow validation should not be counted as a compile-time guarantee.

#### Annotation Burden And Production Evidence

Ordinary Rust code usually writes only `&`, `&mut`, explicit `clone()`, and occasionally `move` on a closure. Moves, local lifetimes, and `Send`/`Sync` conformances are inferred. Annotation density becomes substantial at reusable boundaries: named lifetimes, `for<'a>` higher-ranked bounds, `'static`, `T: Send + Sync`, associated-type bounds, and trait-object bounds.

Production code follows that division:

- Tokio's task API exposes `Future + Send + 'static` because a task may move among worker threads and outlive its spawning stack.
- Serde's `Deserialize<'de>` exposes the input-data lifetime because a decoded result may borrow directly from its input.
- Ripgrep's parallel walker stores callbacks with both a scoped lifetime and `Send`; its application code captures ordinary stack references with little or no explicit lifetime syntax.

This is a strong form of feature consistency: callers see a small vocabulary, while library signatures state the relationships that callers rely upon.

#### Complete Illustrative Example

This complete program uses scoped threads to mutate disjoint portions of a borrowed vector:

```rust
use std::thread;

fn increment_in_parallel(values: &mut [u64]) -> u64 {
    let midpoint = values.len() / 2;
    let (left, right) = values.split_at_mut(midpoint);

    thread::scope(|scope| {
        let left_worker = scope.spawn(move || {
            for value in left.iter_mut() {
                *value += 1;
            }
            left.iter().sum::<u64>()
        });

        let right_worker = scope.spawn(move || {
            for value in right.iter_mut() {
                *value += 1;
            }
            right.iter().sum::<u64>()
        });

        left_worker.join().expect("left worker panicked")
            + right_worker.join().expect("right worker panicked")
    })
}

fn main() {
    let mut values = vec![1, 2, 3, 4];
    let sum = increment_in_parallel(&mut values);

    assert_eq!(values, [2, 3, 4, 5]);
    assert_eq!(sum, 14);
    println!("{values:?}, sum = {sum}");
}
```

No named lifetime, `Send`, `Sync`, or `'static` annotation appears in the source. Nevertheless, the compiler proves that the two mutable slices are disjoint, each may be transferred into one worker, neither escapes the scope, and the parent cannot access the original vector until the workers finish. Trying to send the same mutable slice to both workers is rejected.

#### Relevance To hd-lang

The best Rust lesson for hd-lang is not necessarily move-by-default. It is that lifetime validity, access exclusivity, and concurrency admissibility can be orthogonal facts with aggressive local inference. If hd-lang later needs only resource non-escape, adopting all three layers would be disproportionate. If it eventually needs general borrowed views plus shared-memory concurrency, Rust is the clearest mature baseline.

### Swift

The examples in this section were checked with Apple Swift 6.3.3 in Swift 6 language mode. `~Escapable`, `Span`, and standard-library span properties require Swift 6.2 or newer. User-authored `@_lifetime` dependencies remain experimental in Swift 6.3.3 and require `-enable-experimental-feature Lifetimes`.

#### Overall Idea

Swift starts from a permissive application-language model: ordinary structs are copyable values, and ordinary class instances may have many reference-counted aliases. Newer static features let selected APIs impose stronger rules without changing that default everywhere.

It helps to view Swift as three related but distinct layers. Ownership asks whether a value may be copied, borrowed, mutated temporarily, or consumed. Escapability asks whether a borrowed view may outlive the storage on which it depends. Concurrency isolation asks whether a value or connected group of aliases may cross an actor boundary. `~Copyable`, `~Escapable`, and `Sendable` therefore answer different questions; none implies the other two.

For a resource wrapper, noncopyability can ensure that responsibility is not silently duplicated, a consuming operation can invalidate the caller's value, and `deinit` can perform cleanup at ownership end. For a borrowed buffer view, `~Escapable` and a lifetime dependency are the relevant tools instead. Swift keeps common call sites terse, but library authors must choose and compose the correct layers.

#### Beginner Roadmap

Read the Swift walkthroughs as three passes:

1. Start with value ownership: `~Copyable` prevents implicit duplication, `borrowing` lends access, `inout` lends exclusive mutation, and `consuming` transfers the value.
2. Then study lifetime escape: `~Escapable` marks a value that cannot become independently long-lived, while a lifetime dependency ties a returned view to an input.
3. Finally add concurrency: `Sendable` describes generally transferable values, `@Sendable` constrains closures, and `sending` supports a one-time transfer of a disconnected isolation region.

Do not read the prefixes as a single hierarchy. `~Copyable` does not imply `~Escapable`; neither property implies `Sendable`. The examples intentionally introduce those layers separately before combining them.

#### Copyability And Value Ownership

Swift historically begins from copyable value semantics and reference-counted class objects. Its ownership features strengthen selected APIs without changing that default everywhere.

A concrete struct or enum may suppress the implicit `Copyable` conformance with `~Copyable`. Such a value cannot be implicitly duplicated. Assignment, return, explicit `consume`, or a consuming call can transfer it, after which a later reachable use is rejected. The restriction is structural for the value representation, but it does not guarantee exclusive reachability through every field: a noncopyable struct may contain a class reference that remains aliased elsewhere.

Generic parameters and protocols implicitly require `Copyable`. A declaration such as `T: ~Copyable` removes that implicit requirement so the abstraction can accept both copyable and noncopyable values. It is not a proof that every accepted `T` is noncopyable. This negative-looking suppression rule reduces migration friction but is less conceptually direct than a positive kind or capability.

#### Feature Walkthrough: Noncopyable Values

A `~Copyable` declaration removes the operation that would silently duplicate a value. The program must either borrow it or transfer it:

```swift
struct Ticket: ~Copyable {
    let number: Int
}

func inspect(_ ticket: borrowing Ticket) {
    print("ticket \(ticket.number)")
}

func redeem(_ ticket: consuming Ticket) {
    print("redeemed \(ticket.number)")
}

func example() {
    let ticket = Ticket(number: 42)
    inspect(ticket) // Temporary access; ticket is still ours.
    redeem(ticket)  // Ownership leaves this scope.

    // Rejected if uncommented: ticket was consumed.
    // inspect(ticket)
}

example()
```

The intuition is “this value represents one responsibility.” A file wrapper, transaction token, or explicit hand-off should not accidentally acquire two apparent owners through an ordinary assignment. This does not automatically make everything reachable through the value exclusively owned; a stored class reference can still have aliases outside the noncopyable struct.

This complete example makes that shallow boundary concrete. The `Handle` cannot be copied, but its class instance is still reached through `alias`:

```swift
final class Box { var value = 0 }
struct Handle: ~Copyable { let box: Box }
func demonstrateAliasing() {
    let shared = Box()
    let alias = shared
    let handle = Handle(box: shared)
    alias.value = 42
    print(handle.box.value)
}
demonstrateAliasing()
```

Noncopyability controls duplication of the struct value; it does not recursively turn reference-typed fields into exclusive references.

The negative-looking generic constraint suppresses the usual implicit `Copyable` requirement. It lets one implementation accept either kind of value:

```swift
struct Slot<T: ~Copyable>: ~Copyable {
    var value: T

    consuming func take() -> T {
        consume value
    }
}

struct Token: ~Copyable { let id: Int }

func useSlot() {
    let slot = Slot(value: Token(id: 7))
    let token = slot.take()
    print(token.id)
}

useSlot()
```

`T: ~Copyable` does not assert that `T` lacks copying; it removes the generic declaration's default demand that `T` be copyable. `Slot<Int>` and `Slot<Token>` are therefore both permitted.

For an ordinary copyable value, Swift can insert copies as needed. The explicit `copy` operator is available when an API or reader benefits from making that duplication unambiguous:

```swift
struct Note {
    var text: String
}

func duplicateNote() {
    let original = Note(text: "ready")
    let duplicate = copy original

    print(original.text)
    print(duplicate.text)
}

duplicateNote()
```

`copy` cannot be applied to `Ticket` because `Ticket` opted out of `Copyable`. Thus `copy` and `consume` are explicit operations at opposite ends of the ownership choice: preserve another usable value, or transfer the existing one.

#### Feature Walkthrough: Ownership-Driven Cleanup

A noncopyable struct may define `deinit`. Its cleanup follows the value as ownership moves and runs when the final owner ends on ordinary control flow:

```swift
struct Resource: ~Copyable {
    let id: Int

    deinit {
        print("automatic close: \(id)")
    }

    consuming func close() {
        print("explicit close: \(id)")
        discard self
    }
}

func demonstrateCleanup() {
    do {
        let automatic = Resource(id: 1)
        print(automatic.id)
    } // deinit runs when automatic's ownership lifetime ends.

    let explicit = Resource(id: 2)
    explicit.close()
}

demonstrateCleanup()
```

The first value reaches the end of its ownership lifetime, so `deinit` performs automatic cleanup. Swift may end that lifetime after the value's last use rather than exactly at the closing brace. `close()` performs cleanup explicitly and then uses `discard self` to end the consumed value without invoking its `deinit` body a second time; it is safe only after the method has fully discharged the resource responsibility. As with Rust, this is ownership-driven cleanup on ordinary language control flow, not a proof that every process termination runs cleanup.

`discard self` is currently permitted only when every stored property is trivially destroyed, so a struct holding a `String` or class reference cannot use it yet.

#### Borrow, Mutate, And Consume

Swift's main ownership conventions are:

- `borrowing`: temporary shared, non-consuming access;
- `inout`: temporary exclusive mutable access;
- `consuming`: the callee receives owned access and may end the input's lifetime.

Methods borrow `self` by default unless declared `mutating` or `consuming`. Noncopyable function parameters must make their ownership convention explicit. Ordinary copyable parameters are usually easy to call because Swift can insert a copy when a consuming operation needs its own value.

This last point differs from Rust. Passing a copyable Swift value to a `consuming` parameter does not necessarily invalidate the caller's binding; the compiler may consume a copy. With `~Copyable`, there is no implicit copy available, so consumption becomes an observable ownership transfer.

#### Feature Walkthrough: `borrowing`, `inout`, And `consuming`

The three conventions answer three different questions at a call boundary:

```swift
struct Counter: ~Copyable {
    var value: Int
}

func read(_ counter: borrowing Counter) -> Int {
    counter.value
}

func increment(_ counter: inout Counter) {
    counter.value += 1
}

func finish(_ counter: consuming Counter) -> Int {
    counter.value
}

func run() {
    var counter = Counter(value: 10)
    print(read(counter))
    increment(&counter)
    print(finish(counter))

    // Rejected if uncommented: finish consumed counter.
    // print(read(counter))
}

run()
```

`borrowing` promises to give the value back, `inout` creates a temporary exclusive mutation window, and `consuming` says the callee receives the value's remaining lifetime. The `&` at an `inout` call is useful local punctuation: it makes mutation visible even though ownership returns to the caller afterward.

Two `inout` arguments cannot overlap when both accesses are statically visible:

```swift
func incrementBoth(_ left: inout Int, _ right: inout Int) {
    left += 1
    right += 1
}

func invalidOverlap() {
    var value = 0
    // Rejected: inout arguments are not allowed to alias each other.
    incrementBoth(&value, &value)
}

invalidOverlap()
```

```text
error: inout arguments are not allowed to alias each other
```

Swift diagnoses this as overlapping modification requiring exclusive access. Some less statically visible exclusivity conflicts use runtime enforcement instead; those dynamic checks are intentionally excluded from this compile-time comparison.

#### `Escapable`, `~Escapable`, And Lifetime Dependencies

Swift's non-escapable types are the most direct precedent for the tentative hd-lang feature. A `~Escapable` value is not generally allowed to outlive the context or storage on which it depends. Standard borrowed views such as `Span` use this machinery to expose memory without making the view an independently owning value.

Non-escapability and noncopyability are distinct:

- a value may be noncopyable because it represents exclusive ownership;
- a value may be non-escapable because it borrows storage owned elsewhere;
- a type may need both properties.

Lifetime dependencies connect a returned view to a source argument or `self`. Swift usually infers local scopes, so ordinary users do not write Rust-style lifetime variables. Low-level library authors, however, encounter lifetime-dependent accessors and currently specialized or underscored annotations. The user experience is therefore asymmetrical: consuming a noncopyable resource is relatively simple, while authoring a novel borrowed-view abstraction is considerably more advanced.

Containment must preserve the restriction. A struct that stores a non-escapable field cannot become an ordinary freely escaping wrapper. Generic abstractions also have to suppress the default `Escapable` requirement when they intend to accept such values. This is closely aligned with the tentative hd-lang preference.

#### Feature Walkthrough: Declaring `~Escapable`

A user-declared non-escapable type can wrap a borrowed standard-library view without erasing its lifetime restriction:

```swift
enum BorrowedInts: ~Escapable {
    case values(Span<Int>)
}

func firstBorrowed(_ values: borrowing [Int]) -> Int? {
    let borrowed = BorrowedInts.values(values.span)

    // ~Escapable is independent of Copyable. This local copy is
    // valid, and both values remain confined to this context.
    let localCopy = borrowed
    switch localCopy {
    case .values(let view):
        _ = view.count
    }

    switch borrowed {
    case .values(let view):
        return view.isEmpty ? nil : view[0]
    }
}

print(firstBorrowed([10, 20, 30]) as Any)
```

`BorrowedInts` is copyable by default but not escapable. Its enum payload inherits the lifetime dependency of the `Span`, while `: ~Escapable` prevents the wrapper from erasing that restriction. This illustrates why copyability and escapability are separate axes: several temporary views may be useful inside one valid context even though none may outlive their borrowed storage.

A plain return type has no declared lifetime dependency, so this version is rejected:

```swift
// Rejected: a function cannot return a ~Escapable result unless
// the result has an appropriate lifetime dependency.
func invalidReturn(
    _ borrowed: borrowing BorrowedInts
) -> BorrowedInts {
    borrowed
}
```

The compiler's diagnostic is:

```text
error: a function cannot return a ~Escapable result
```

Containment must also preserve the property:

```swift
// Rejected: an ordinary struct is Escapable, so it cannot hide a
// non-escapable value in a stored property.
struct InvalidWrapper {
    let borrowed: BorrowedInts
}
```

The relevant diagnostic is:

```text
error: stored property 'borrowed' of 'Escapable'-conforming struct
       'InvalidWrapper' has non-Escapable type 'BorrowedInts'
```

The compiler diagnostic suggests making the wrapper `~Escapable` too. In current Swift, authoring a struct initializer or accessor that returns such a wrapper may additionally require lifetime-dependency machinery whose stable public spelling is still evolving. Enum cases receive built-in lifetime propagation, which is why the complete example above can express the idea using only public source syntax.

An escaping closure is another indirect return path. Capturing a lifetime-dependent view in a returned closure is rejected:

```swift
func invalidClosure(
    _ values: borrowing [Int]
) -> () -> Int {
    let view = values.span

    // Rejected: lifetime-dependent variable view escapes its scope.
    return { view.count }
}
```

```text
error: lifetime-dependent variable 'view' escapes its scope
```

Swift 6.3.3 reports that `view` depends on the lifetime of `values` and that the closure capture causes it to escape. A nonescaping callback invoked entirely inside the function may borrow the same view safely.

#### Feature Walkthrough: A Borrowed `Span`

`Span` is a standard non-escapable view over contiguous storage. The function below may read the view while the borrowed array storage is available:

```swift
func first(_ values: borrowing [Int]) -> Int? {
    let view = values.span
    return view.isEmpty ? nil : view[0]
}

print(first([10, 20, 30]) as Any)
```

The span does not retain or copy the array storage into an independent owner. Conceptually, its validity is a loan from `values`. Consequently, this superficially similar function is rejected:

```swift
// Rejected: the returned view would outlive this function's
// permitted access to the borrowed parameter.
func invalidView(_ values: borrowing [Int]) -> Span<Int> {
    values.span
}
```

Swift can permit borrowed results when an API declares an appropriate lifetime dependency on a caller-owned source. The important intuition is that `~Escapable` alone is only the prohibition; a dependency describes the positive route by which a view may safely leave a smaller scope while remaining bounded by some longer-lived input.

Swift 6.3.3 can express that positive route with the current underscored, experimental lifetime attribute:

```swift
@_lifetime(borrow values)
func view(_ values: borrowing [Int]) -> Span<Int> {
    values.span
}

func demonstrateView() {
    let values = [10, 20, 30]
    let borrowed = view(values)
    print(borrowed[0])
}

demonstrateView()
```

This example requires `-enable-experimental-feature Lifetimes`. `@_lifetime(borrow values)` states that the returned span is valid only while its borrow of `values` remains valid. The underscored spelling is not a stable source-level commitment and may change before the feature is finalized.

#### `Sendable`, `sending`, And Actor Isolation

Swift's concurrency system is a separate layer:

- `Sendable` is a property of a type whose values are safe for general use across concurrency isolation boundaries.
- `@Sendable` constrains function values and their captures for concurrent use.
- Actors and global actors isolate state.
- `sending` describes a particular value whose entire isolation region—the values connected by possible aliases—is disconnected at a function boundary, allowing safe transfer even when the type is not generally `Sendable`.
- Region-based isolation infers when a newly created non-`Sendable` object is disconnected and can therefore be transferred once.

`consuming` and `sending` are orthogonal. The former transfers value ownership; the latter transfers an isolation region. A parameter may need both. This layered terminology is Swift's largest concept-surface cost.

#### Feature Walkthrough: `Sendable` Versus One-Time `sending`

A type that conforms to `Sendable` promises that values of that type are generally safe to pass across isolation boundaries. A newly created class instance can instead be transferred as a disconnected isolation region even when its type is not generally sendable:

```swift
final class Draft {
    var text: String

    init(_ text: String) {
        self.text = text
    }
}

actor Archive {
    private var draft: Draft?

    func store(_ value: sending Draft) {
        draft = value
    }
}

func transfer() async {
    let draft = Draft("ready")
    let archive = Archive()

    await archive.store(draft)

    // Rejected if uncommented: the disconnected isolation region was sent.
    // print(draft.text)
}
```

`await archive.store(draft)` crosses from the caller's isolation domain into the `Archive` actor's isolated state. `Draft` is mutable reference state, so freely sharing arbitrary aliases would be unsafe. Region analysis sees that this fresh instance is disconnected, permits the one-way send, and then prevents the caller from using its former access path. By contrast, a `Sendable` value needs no one-time disconnected proof at each crossing.

`@Sendable` applies the same concern to closures. Capturing an ordinary mutable class instance is rejected because the closure might execute in another isolation domain:

```swift
final class Counter {
    var value = 0
}

func run(_ operation: @Sendable () -> Void) {
    operation()
}

func invalidCapture() {
    let counter = Counter()
    run {
        // Rejected: Counter is not Sendable.
        print(counter.value)
    }
}
```

```text
error: capture of 'counter' with non-Sendable type 'Counter' in a '@Sendable' closure
```

#### What Swift Proves

Within checked Swift 6 code, the compiler can reject:

- copying or reusing a consumed noncopyable value;
- escaping a non-escapable borrowed view beyond its permitted lifetime;
- overlapping statically visible exclusive `inout` access;
- unsafe actor or isolation-boundary crossings;
- captures that violate an `@Sendable` closure's requirements;
- later source-domain use after a disconnected isolation region has been sent.

The checker does not make ordinary class references exclusively owned, and `~Copyable` does not imply exclusive reachability through the object graph. Some exclusivity enforcement can fall back to runtime checks, which should not count as compile-time validation here. `@unchecked Sendable`, `nonisolated(unsafe)`, unsafe pointers, C/Objective-C interoperability, and compatibility annotations form explicit trust boundaries. Actor reentrancy, deadlocks, liveness, and logical races are not eliminated.

#### Annotation Burden And Production Evidence

Swift has very low ordinary-call-site verbosity. The burden is concentrated in types and public APIs:

- a simple exclusively owned resource may need `: ~Copyable` and a few consuming operations;
- a concurrency API commonly adds `Sendable`, `@Sendable`, or actor isolation;
- a generic ownership abstraction repeats `T: ~Copyable` and conditional conformances;
- a custom borrowed-view API may require non-escapability and explicit lifetime dependencies.

Production code reflects the maturity split. Swift System's noncopyable Mach port is a real OS-resource wrapper with a consuming relinquish operation. Foundation's `Data` borrowed views use lifetime-dependent accessors and show substantially more library-author ceremony. Swift Async Algorithms uses concise `Sendable` generic constraints, while SwiftNIO contains `@preconcurrency`, unavailable conformances, and documented `@unchecked Sendable` bridges required by migration and protocol constraints.

#### Complete Illustrative Example

This complete Swift 6 program combines noncopyability, borrowing, mutation, ownership transfer, sendability, and actor isolation. The earlier `Draft` example demonstrates inferred one-time transfer of a non-`Sendable` disconnected isolation region.

```swift
struct Report: ~Copyable, Sendable {
    let title: String
    var lines: [String]

    init(title: String, lines: [String]) {
        self.title = title
        self.lines = lines
    }
}

func lineCount(of report: borrowing Report) -> Int {
    report.lines.count
}

func addFooter(to report: inout Report) {
    report.lines.append("-- end --")
}

actor Archive {
    private var titles: [String] = []

    func store(_ report: consuming Report) {
        titles.append(report.title)
    }

    func storedTitles() -> [String] {
        titles
    }
}

func submit(
    _ report: consuming sending Report,
    to archive: Archive
) async {
    await archive.store(report)
}

@main
struct Main {
    static func main() async {
        var report = Report(
            title: "September",
            lines: ["ready", "shipped"]
        )

        print("\(lineCount(of: report)) lines")
        addFooter(to: &report)

        let archive = Archive()
        await submit(report, to: archive)
        print(await archive.storedTitles())

        // Rejected if uncommented: report was consumed.
        // print(lineCount(of: report))
    }
}
```

`~Copyable` makes the final call a real transfer rather than an implicit copy. `borrowing` preserves ownership, `inout` grants temporary mutation, and `consuming` transfers value ownership. `Archive.store` is isolated to the `archive` actor, while `submit` is not, so `await archive.store(report)` crosses an actor-isolation boundary and may suspend while execution is scheduled on the actor. `Report: Sendable` makes the argument admissible there. `sending` appears in the `submit` boundary, but because `Report` is already `Sendable`, this example does not rely on one-time disconnected-isolation-region analysis. The example does not itself define a `~Escapable` view; that more specialized machinery is described above because it is the part most relevant to a future hd-lang file-borrow design.

#### Relevance To hd-lang

Swift shows that lexical non-escape can be added without turning every value into a Rust-style owned value. It also shows that propagation cannot stop at `return`: aggregates, generic constraints, closures, erased values, accessors, and isolation regions all need rules. hd-lang should borrow the narrow idea, not automatically inherit Swift's full accumulated vocabulary.

### Scala Capture Checking

**Version note (September 2026).** The examples below use the Scala 3.9.0 experimental surface syntax. Both `captureChecking` and `separationChecking` remain opt-in, and the current documentation recommends nightlies when testing the newest changes. Older material may use `cap` where current documentation uses `any`; examples from different compiler generations should not be mixed mechanically.

#### Overall Idea

Scala capture checking asks what authority a value retains rather than who owns the value. If a closure remembers a file handle, or an object stores a mutable service, its type records that retained capability. A value may escape only when every capability named by its type remains meaningful at the destination.

The mental model is a label attached to each value saying “this value may still reach these things.” Returning a closure, placing it in another object, or passing it through a generic container does not erase the label. A locally introduced capability therefore cannot be smuggled outside its scope merely by adding wrappers.

Core capture checking does not prevent ordinary aliases, transfer ownership, or run cleanup. The experimental separation layer selectively adds stronger rules such as exclusive capabilities, read-only projections, and consumption. Scala is consequently the most general retained-authority model in this survey, but also one of the easiest to over-apply when the desired feature is only binary non-escape.

#### Beginner Roadmap: Capabilities And Capture Sets

Begin with four definitions:

1. A **capability** is a reference whose retention the checker tracks—for example, a file stream passed as `FileOutputStream^`.
2. A **capture** means keeping such a capability inside a returned or stored value. Merely using the capability immediately is not the same as retaining it.
3. A **capture set** lists what a value may retain. `T^{output}` means a `T` that may keep the particular capability named `output`.
4. A thin function arrow, `A -> B`, promises an empty capture set. `A ->{output} B` may retain `output`. The ordinary fat arrow, `A => B`, permits arbitrary capabilities visible at the call site.

The first complete program contrasts immediate use, retained use, and a pure closure:

```scala
import language.experimental.captureChecking
import java.io.FileOutputStream

def writeNow(output: FileOutputStream^): Unit =
  output.write(65)

def writeLater(
    output: FileOutputStream^
): () ->{output} Unit =
  () => output.write(66)

def pureMessage(): () -> String =
  () => "ready"

@main def beginnerCaptures(): Unit =
  val output = FileOutputStream("capture-demo.log")

  try
    writeNow(output)

    val later = writeLater(output)
    later()

    println(pureMessage()())
  finally
    output.close()
```

`writeNow` exercises the stream but returns only `Unit`, so it does not return anything that retains `output`. `writeLater` returns a closure with a hidden field pointing to the stream; its type therefore says `->{output}`. `pureMessage` captures no tracked capability, so it can promise the pure type `() -> String`.

The next program introduces scope. `usingLogFile` creates a capability, permits a callback to use it, and closes it before returning:

```scala
import language.experimental.captureChecking
import java.io.FileOutputStream

def usingLogFile[T](
    operation: FileOutputStream^ => T
): T =
  val output = FileOutputStream("beginner.log")
  try operation(output)
  finally output.close()

@main def scopedCapability(): Unit =
  val bytesWritten =
    usingLogFile { output =>
      output.write(65)
      1
    }

  println(bytesWritten)

  // Rejected if uncommented: the returned closure would retain
  // output after usingLogFile closes it.
  // val writeLater =
  //   usingLogFile { output =>
  //     () => output.write(66)
  //   }
  // writeLater()
```

The accepted callback uses `output` now and returns an ordinary integer. The rejected callback tries to return a closure whose inferred capture set contains `output`. Because that parameter exists only inside the callback, it cannot appear in the caller's result type. This is the central capture-checking rule; classes, containers, and separation checking generalize it later.

#### Capture Sets As Retained Authority

Scala capture checking extends types with sets of capabilities that a value may retain. A type `T^{x, y}` says that the value may retain references to capabilities `x` and `y`; `T^` is shorthand for a value that may capture an arbitrary capability rooted at `any`.

Capabilities are path-dependent values rather than separate lifetime names. Their lexical scope supplies the escape boundary: if a result's type would mention a local capability that is no longer visible, that result cannot escape. This lets the same mechanism describe a closure retaining a file, an object retaining a mutable cell, or a computation retaining some other authority.

The compiler runs a propagation constraint solver after ordinary type checking. Local capture sets are heavily inferred. Explicit sets mainly appear where a reusable API promises what a returned value or stored callback may retain.

The subtyping direction is effect-like: a value known to capture a smaller set can be used where a larger allowed set is expected. Capture polymorphism lets higher-order APIs such as collection operations work with both pure and capturing callbacks without manually adding an effect parameter to every signature.

#### Feature Walkthrough: Reading A Capture Type

Capture checking distinguishes function-type arrows. `A -> B` is a pure function type with an empty capture set, while `A ->{x, y} B` may retain the named capabilities. Ordinary Scala `A => B` is capture-permissive shorthand for `A ->{any} B`: it accepts pure functions too, but does not promise purity. Lambda expressions still use the ordinary value-level syntax `x => ...`; the thin arrow appears in types.

Consider an object that keeps a file stream in one of its fields:

```scala
import language.experimental.captureChecking
import caps.ExclusiveCapability

final class LogFile extends ExclusiveCapability:
  def writeLine(message: String): Unit = println(message)

final class Writer(output: LogFile):
  def writeLine(message: String): Unit =
    output.writeLine(message)

def makeWriter(output: LogFile): Writer^{output} =
  Writer(output)
```

Read `Writer^{output}` as “a `Writer` that may retain the particular capability named `output`.” The annotation does not say that `Writer` owns the file or is its only alias; it exposes the path-dependent authority retained in the field. Because `LogFile` extends a capability trait, references received through parameters are tracked without spelling `LogFile^` at every parameter.

The intuition resembles an effect annotation attached to a value rather than only to an execution step. Calling `writeLine` exercises the authority now; returning the `Writer` retains authority that can be exercised later. Capture checking records the latter fact.

#### Scope, Escape, And Closures

Scala's approach is broader than a `NonEscapable` marker. It does not merely classify a value as escapable or non-escapable; it records what the value may retain. Two values may both be locally scoped for different reasons because their types mention different local capability paths.

This naturally closes indirect escape holes:

- returning a closure that captures a local file retains the file capability in the closure's type;
- wrapping that closure in another object propagates the capture;
- generic containers preserve capture information through inferred boxing: the container itself may remain capture-pure, while accessing a capturing element makes its retained capabilities visible again;
- an explicitly pure result cannot silently retain a capability.

The design can also support checked exceptions and other effects when invoking an effect requires possession of a capability.

#### Feature Walkthrough: Closures Carry Authority Too

A closure is an object with hidden fields for its captured variables, so its type must retain the same capability information:

```scala
import language.experimental.captureChecking
import java.io.FileOutputStream

def writeNow(output: FileOutputStream^): Unit =
  val operation: () ->{output} Unit =
    () => output.write(65)

  operation()

def pureMessage(): () -> String =
  val message = "ready"
  () => message
```

`operation` has type `() ->{output} Unit`: invoking it may use the particular stream capability `output`. The lambda expression itself still uses `=>`; `->{output}` is the type-level capture annotation. It is safe to invoke locally, but it cannot be returned where `() -> Unit` is required because that type promises an empty capture set.

`pureMessage` explicitly returns `() -> String`. Its closure retains only an ordinary untracked immutable value, so it satisfies the empty-capture promise. By contrast, spelling either return type as `() => ...` would permit arbitrary captures and would not state purity.

This propagation is the main benefit over a rule that checks only direct `return file`. The compiler follows the authority through closure environments, object fields, and generic containers rather than letting an extra wrapper hide the escape.

#### Separation Checking

Core capture checking does not establish exclusive ownership. Two aliases may retain the same capability, and ordinary Scala mutable objects remain ordinary aliased objects.

The experimental separation checker adds selective ownership-like reasoning:

- `ExclusiveCapability` participates in interference checks;
- `SharedCapability` is explicitly exempt and may be shared;
- `Mutable` identifies stateful exclusive capabilities;
- `update` identifies mutating methods;
- `x.rd` is a capture-set element recording read-only use derived from `x`;
- `consume x: T` marks a parameter whose actual argument and covered aliases become unusable after transfer;
- `fresh` in a function-type result states that every invocation produces a distinct isolated capture root;
- `freeze` consumes mutable authority and produces a capture-empty, read-only view for an appropriately designed mutable type.

Distinct universal capability roots in a function signature create separation requirements at a call. A function taking two independently rooted mutable parameters can reject a call that passes the same object twice. This is selective non-interference requested by the API, not a global shared-or-exclusive borrow rule.

#### Feature Walkthrough: Selective Separation

With separation checking enabled, an API can request two independently rooted mutable capabilities:

```scala
import language.experimental.captureChecking
import language.experimental.separationChecking
import caps.Mutable

final class Counter(initial: Int) extends Mutable:
  private var current = initial

  def value: Int = current

  update def increment(): Unit =
    current += 1

def incrementBoth(left: Counter^, right: Counter^): Unit =
  left.increment()
  right.increment()

def delayedValue(counter: Counter^): () ->{counter.rd} Int =
  () => counter.value

def incrementAndTransfer(
    consume counter: Counter^
): Counter^ =
  counter.increment()
  counter

@main def demo(): Unit =
  val first: Counter^ = Counter(0)
  val second: Counter^ = Counter(10)

  incrementBoth(first, second)

  // Rejected if uncommented: the two universal roots cannot be
  // instantiated with the same exclusive capability.
  // incrementBoth(first, first)

  val moved = incrementAndTransfer(first)

  // Rejected if uncommented: first was consumed by the call above.
  // println(first.value)

  val readMoved = delayedValue(moved)
  println(s"${readMoved()}, ${second.value}")
```

The core capture checker would be content to say that both references retain the same `Counter`. The separation checker adds the stronger fact required by this signature: mutations through `left` and `right` must not interfere. Unlike Rust, Scala does not impose this rule on every mutable reference; the types participating in the capability discipline opt into it.

`counter.rd` occurs in a capture set; it records that `delayedValue`'s returned closure retains only read authority derived from `counter`. It is not an expression that converts `counter` into a new reference. For a class extending `Mutable`, a bare `Counter` reference is implicitly read-only, whereas `Counter^` carries full authority and can call `update` methods.

`incrementAndTransfer` marks its parameter `consume`. Once the call succeeds, the caller's `first` path cannot be used again, and the returned `Counter^` becomes the continuing full-access path. This is selective affine transfer for participating capabilities, not a global Scala move rule.

#### What Scala Proves

Core capture checking can prove that modeled capabilities do not escape their scopes or appear in values whose declared capture sets exclude them. With careful modeling, it can track effect retention through closures, classes, and higher-order functions.

It does not by itself prove:

- exclusive ownership or absence of aliases;
- deterministic cleanup or exactly-once `close()`;
- global data-race freedom for ordinary Scala state;
- safe behavior of Java APIs, reflection, unchecked casts, or untracked globals;
- that a user-declared shared capability is internally thread-safe.

Separation checking can add non-interference and consumption for participating capabilities, but all relevant state and boundaries must be modeled. Ordinary JVM references outside that discipline remain freely aliased.

Scala capture checking has no blanket structural equivalent of Rust's `Send` and `Sync`. A concurrency API must express the relevant capture and separation requirements, and the guarantee covers only mutable authority represented inside that model. Likewise, lexical non-escape can support a `try`/`finally` resource API, but the type system does not itself provide ownership-driven deterministic cleanup.

#### Annotation Burden And Production Evidence

For clients of a well-designed scoped API, annotation burden can be extremely low. Routine API authors expose capture types and function arrows. Advanced generic code may need explicit capture-set parameters, reach capabilities, classifiers, read-only projections, and separate-compilation declarations.

A reach capability such as `xs*` names capabilities stored inside and reachable through a generic value `xs`; `@use` marks a parameter whose reachable contents the implementation accesses. This is principally library-author syntax for effect-retaining generic abstractions such as lazy `flatMap`, not routine application syntax.

The standard-library evaluation is unusually useful evidence. Approximately 31,395 lines of collections and related code were migrated with roughly 3% of lines changed; most classes and methods required no signature change, while difficult cases clustered around lazy structures, iterators, builders, and effect-retaining abstractions. This supports low notation for mainstream functional collections, not low conceptual complexity for the entire system.

TACIT and Orca are serious non-demo uses of capture checking and safe/separation modes, but both remain research-oriented and expose gaps at unchecked dependency boundaries. The feature should still be treated as experimental rather than as a production baseline comparable to Rust.

#### Complete Illustrative Example

This complete Scala 3.9 program grants a file capability only to a scoped callback:

```scala
//> using scala 3.9.0

import language.experimental.captureChecking

import java.io.FileOutputStream
import java.nio.charset.StandardCharsets

def usingLogFile[T](
    path: String
)(
    operation: FileOutputStream^ => T
): T =
  val stream = FileOutputStream(path)
  try operation(stream)
  finally stream.close()

@main def scopedLog(): Unit =
  val bytesWritten =
    usingLogFile("app.log") { output =>
      val bytes =
        "started\n".getBytes(StandardCharsets.UTF_8)

      output.write(bytes)
      bytes.length
    }

  println(s"wrote $bytesWritten bytes")

  // Rejected if uncommented: the returned closure would retain the
  // local output capability after usingLogFile closes the stream.
  // val writeLater =
  //   usingLogFile("app.log") { output =>
  //     () => output.write('!'.toInt)
  //   }
  // writeLater()
```

The callback deliberately uses the permissive fat arrow: `FileOutputStream^ => T` allows the callback itself to capture caller-visible authority. The important restriction is lexical. The caller chooses `T`, so `T`'s capture set may mention names visible at the call site, but the lambda parameter `output` exists only inside the callback and is not one of them. Returning `() => output.write(...)` would produce a value with the inferred capture type `() ->{output} Unit`; the local path cannot be included in the outer result capture set, so the compiler rejects the call.

`T` need not be globally pure: it may retain capabilities already visible to the caller. What it cannot do is smuggle out the newly introduced callback capability. `close()` has no special static meaning here; the checker is enforcing capability scope, not Java stream typestate.

#### Relevance To hd-lang

Scala demonstrates why a robust escape checker is transitive through closures and aggregates. A simple binary escapable/non-escapable property may be enough for hd-lang's first use case, while path-specific capture sets would become valuable if different borrowed origins must coexist in returned values. The full Scala algebra should be avoided unless concrete hd-lang APIs require that extra precision.

### Kotlin Local Lifetimes

Kotlin local lifetimes is a research design note, not an accepted KEEP or a released Kotlin compiler feature. The syntax below therefore illustrates the proposal as written; unlike the Rust and Swift blocks, these examples cannot currently be compiled or assigned verified diagnostics. The useful evidence is the design's standard-library localization exercise, not production deployment.

#### Overall Idea

The Kotlin proposal begins with a simple API promise: a `local` parameter will not be retained beyond the current call. The caller can therefore pass a value or closure that is valid only temporarily without requiring the callee to be inlined or inspected. Ordinary Kotlin values remain unrestricted and are treated as globally usable unless an API opts into locality.

Local does not always mean “must disappear inside this function.” A returned iterator, view, or closure may retain a local input when the result type states that dependency. The caller may then use the result only while the relevant input remains valid. A `local class` applies the same idea to fields: the aggregate's inferred lifetime cannot exceed the local values it stores.

This is deliberately lifetime-only. It does not consume the caller's binding, exclude aliases, make a reference read-only, or establish thread safety. Its attraction for hd-lang is the narrow progression from “do not retain this parameter” to “this returned or stored value remains tied to that parameter,” without adopting Rust's complete ownership model.

#### Beginner Roadmap

Read the proposed Kotlin feature in three steps:

1. `local parameter: T` promises that the callee will not retain that argument beyond the current call.
2. `T_{parameter}` permits a result or field to retain the argument while making the dependency visible in its type.
3. `local class` gives an aggregate its own inferred lifetime so fields such as `T_{this}` may be stored safely.

Stop there for the ordinary model. Restricted function types, lifetime intersections such as `x&y`, and explicit locality parameters are library-author machinery. None of these forms moves the value or controls mutation; a separate ownership rule would still be needed for hd-lang's consuming `Suspend[T]` case.

#### Core Static Model

Ordinary Kotlin references are treated as having a `global` lifetime: the language does not normally constrain where they may be retained. The proposal adds `local` to a parameter or extension receiver. A function with a local parameter is checked without assuming that parameter is globally valid, so the value cannot flow into a location that may outlive the call.

The important separation is:

- `local x: T` modifies the parameter's permitted use. It is the common non-escape interface.
- `T_{x}` modifies a type with a lifetime dependency. It means that value may be used only within the lifetime represented by `x`.
- `(A) ->_{x} B` gives a function value a restricted lifetime.
- `local class` permits each instance to have its own inferred lifetime and to store fields restricted to that instance's `this` lifetime.
- `global` denotes the ordinary unrestricted lifetime; `local` can also name the current call's lifetime; `this` names the receiver or object's lifetime.

This is lifetime tracking without ownership. A local reference can still be copied, aliased, and mutated according to ordinary Kotlin rules. Calling a function with a local parameter does not consume the caller's value, make it read-only, or make its reachable graph exclusive.

#### Feature Walkthrough: Local Parameters And Receivers

A local parameter promises that the callee will not retain that argument beyond the call. This complete file uses proposal syntax:

```kotlin
private var saved: (() -> Unit)? = null

fun runNow(local action: () -> Unit) {
    action()
}

fun invalid(local action: () -> Unit) {
    // Rejected: global storage may outlive this call.
    saved = action
}

fun main() {
    runNow { println("now") }
}
```

`runNow` may invoke `action`, pass it to another suitably local API, or place it inside an intermediate object whose lifetime is also restricted to this call. It may not assign the closure to `saved` because that location expects a globally usable value.

The proposal has no released compiler implementation from which to quote a real diagnostic. Its intended error would identify the flow from the local parameter into a global location.

A receiver can be local for the same reason. The design note's motivating `fold` shape is:

```kotlin
fun <E, R> local Iterator<E>.foldLocal(
    initial: R,
    local combine: (R, E) -> R
): R {
    var result = initial
    while (hasNext()) {
        result = combine(result, next())
    }
    return result
}

fun main() {
    val total = listOf(10, 20, 30)
        .iterator()
        .foldLocal(0) { sum, value -> sum + value }

    println(total)
}
```

Neither the iterator receiver nor `combine` may escape `foldLocal`. This guarantee is part of the signature, so another function can rely on it without inlining or inspecting the implementation. That is the main improvement over an intraprocedural “called in place” analysis.

#### Transfer, Read-Only Access, And Concurrency

Kotlin local lifetimes has no transfer or consume operation. After `runNow(callback)` returns, the caller may use `callback` again. The modifier restricts what the callee may retain; it does not invalidate the caller's binding.

It likewise has no shared-versus-exclusive borrow distinction. A `local MutableList<T>` remains mutable and may have aliases. Locality answers “how long may this path be used?” rather than “who else may access it?” or “may it be mutated?”

The proposal also has no `Send`, `Sendable`, or actor-transfer equivalent. A value remaining within a lexical lifetime is not thereby safe to use from another thread. Thread confinement, synchronization, and concurrency transfer require a separate system.

#### Dependent Results And Escape By Return

`local` is not simply a ban on returning anything related to an argument. A result may retain local inputs when its type exposes the dependency. The proposal writes `T_{x}` for a value bounded by lifetime `x` and combines dependencies with `&`.

This complete proposal-syntax example returns a lazy iterator retaining both its source and transformation:

```kotlin
fun <A, B> local Iterator<A>.mapLocal(
    local transform: (A) -> B
): Iterator<B>_{this&transform} =
    object : Iterator<B> {
        override fun hasNext(): Boolean =
            this@mapLocal.hasNext()

        override fun next(): B =
            transform(this@mapLocal.next())
    }

fun main() {
    val source = listOf(1, 2, 3).iterator()
    val doubled = source.mapLocal { value -> value * 2 }

    println(doubled.next())
}
```

The returned object captures `this` and `transform`. Consequently it is valid only where both dependencies remain valid. The result may escape the body of `mapLocal`, but it cannot be widened silently into a globally usable `Iterator<B>`.

This is the proposal's most important lesson for hd-lang: a non-escapable value can be returned safely when the result remains explicitly dependent on an input. Swift expresses the same broad idea with non-escapability and lifetime dependencies; Kotlin makes the dependency algebra visible as a type modifier.

#### Local Classes And Aggregate Escape

A constructor parameter marked only `local` must not escape the constructor call, so it cannot be stored for later use. Storing a restricted value requires a `local class` whose instance lifetime bounds the field:

```kotlin
local class MappingIterator<A, B>(
    private val source: Iterator<A>_{this},
    private val transform: (A) ->_{this} B
) : Iterator<B> {
    override fun hasNext(): Boolean =
        source.hasNext()

    override fun next(): B =
        transform(source.next())
}

fun <A, B> local Iterator<A>.mapped(
    local transform: (A) -> B
): Iterator<B>_{this&transform} =
    MappingIterator(this, transform)

fun main() {
    val values = listOf(2, 4, 6).iterator()
    val text = values.mapped { value -> "value=" + value }

    println(text.next())
}
```

Each `MappingIterator` instance has an implicit `this` lifetime. Both stored fields must remain valid for at least that lifetime, so the constructed object's inferred lifetime cannot exceed either input. Containment therefore propagates the restriction rather than erasing it.

This directly covers a struct-like aggregate holding a non-escapable field. It is more expressive than making the aggregate unconditionally non-escapable: the object receives a particular lifetime computed from what it stores.

Local classes have ecosystem consequences. A local class may inherit only local classes because an unrestricted superclass could leak `this`. The design therefore proposes localizing foundational interfaces such as `Any` while allowing ordinary global instances to keep working. This is a large language and library commitment hidden behind the small `local` entry point.

#### Feature Walkthrough: Closures And Local Control

Closures are ordinary objects that may retain local values, so their function types and containing objects carry the same lifetime information. This allows local callbacks to compose through helpers rather than requiring the compiler to inline the whole call chain.

The proposal uses this to permit non-local control through a callback known not to outlive the call:

```kotlin
fun <E, R> local Iterator<E>.foldLocal(
    initial: R,
    local combine: (R, E) -> R
): R {
    var result = initial
    while (hasNext()) {
        result = combine(result, next())
    }
    return result
}

fun local Iterator<Int?>.sumUntilNull(): Int? =
    foldLocal(0) { total, element ->
        total + (element ?: return null)
    }

fun main() {
    val result = listOf(1, 2, null, 4)
        .iterator()
        .sumUntilNull()

    println(result)
}
```

The lambda captures the continuation of `sumUntilNull`. That continuation is valid only while the call is active, and `foldLocal` promises not to retain the lambda. The static point is the lifetime proof; the design note separately discusses possible runtime implementations for lexical aborts and suspension, which are outside this compile-time-only comparison.

A local callback does not automatically make its ordinary arguments local. The `initial` value in `foldLocal` is deliberately not marked local because `combine` could store an argument somewhere global. Locality must be expressed at every boundary through which a restricted reference may flow.

#### Feature Walkthrough: Cleanup

Local lifetimes can make scoped cleanup APIs more expressive, but they do not provide ownership-driven destruction. The design note localizes Kotlin's function-based `AutoCloseable` constructor so its returned object may retain a limited-lifetime cleanup closure:

```kotlin
fun makeCloseAction(
    local logger: (String) -> Unit
): AutoCloseable_{logger} =
    AutoCloseable {
        logger("closed")
    }

fun main() {
    makeCloseAction { message -> println(message) }.use {
        println("working")
    }
}
```

The returned `AutoCloseable` is bounded by `logger` because its `close` operation retains that callback. The ordinary `use` pattern still supplies the `try`/`finally` behavior that calls `close`. The lifetime system prevents invalid escape of the retained callback; it does not prove that every resource is closed, make `close` exactly once, or attach cleanup to ownership loss.

A safe file API also needs care beyond marking its callback value local. Saying that a callback itself cannot escape does not by itself restrict what the callback does with its arguments. If the callback receives a restricted file handle, that parameter boundary must carry locality too, or the callback could store the handle globally.

#### What Kotlin Local Lifetimes Would Prove

Within code and APIs modeled by the proposal, the type checker could reject:

- storing a local parameter in a global or otherwise longer-lived location;
- returning an unrestricted value that retains a local input;
- hiding local dependencies inside a closure, lazy iterator, delegate, or aggregate;
- keeping a restricted field in an object whose lifetime is longer than that field;
- invoking a lifetime-restricted callback after its captured continuation or input has expired.

It would not prove:

- exclusive ownership, absence of aliases, or use-after-move safety;
- read-only access or mutation non-interference;
- deterministic or exactly-once cleanup;
- data-race freedom or safe concurrency transfer;
- deep immutability;
- that unrestricted Kotlin or foreign APIs respect locality unless their signatures are modeled.

The proposal's safety boundary is temporal reachability. It intentionally leaves ownership, mutation, and concurrency orthogonal.

#### Annotation Burden And Standard-Library Evidence

The intended ordinary surface is small. Callers generally write normal Kotlin; API authors add `local` to parameters or receivers that are not retained. Explicit `_{...}` lifetime modifiers appear when an API returns a view, lazy computation, or aggregate retaining local inputs.

Advanced library code is much denser. It may require `local class`, `T_{this}` fields, `T_{x&y}` results, restricted function types, explicit locality parameters, and upper or lower lifetime bounds. The proposal also distinguishes a parameter modifier from a type modifier, which is semantically clean but adds two related forms users must learn.

The design note reports a research exercise localizing Kotlin's collection library. Most changes were signature changes: marking parameters or receivers local and tying lazy results to inputs. Its detailed `Sequence.flatten` case required localizing internal sequence classes and propagating `this` through stored sequences, transformers, iterators, and results without changing implementations. This is valuable non-demo design evidence, but it is not compiled production evidence because the feature has no released implementation.

The exercise also found broad library effects. Views such as `subList` and map key/value collections naturally inherit `this`; builders can take local actions; some `crossinline` uses can become `local`. Conversely, globally returning methods such as `String.toString` constrain which classes can have local instances. Adoption is therefore incremental at call sites but foundational in the standard-library model.

#### Complete Illustrative Example

The following proposal-syntax file combines a local receiver, a retained closure, a dependent result, and rejection of aggregate escape:

```kotlin
private var leaked: Iterator<Int>? = null

fun <A, B> local Iterator<A>.mapLocal(
    local transform: (A) -> B
): Iterator<B>_{this&transform} =
    object : Iterator<B> {
        override fun hasNext(): Boolean =
            this@mapLocal.hasNext()

        override fun next(): B =
            transform(this@mapLocal.next())
    }

fun consumeLocally(local source: Iterator<Int>) {
    val incremented = source.mapLocal { value -> value + 1 }

    println(incremented.next())

    // Rejected: incremented retains local source, while leaked
    // requires a globally usable iterator.
    leaked = incremented
}

fun main() {
    consumeLocally(listOf(10, 20).iterator())
}
```

The local source may be retained by `incremented` because that result is used within the same call. Assignment to `leaked` is rejected because the global variable would erase the result's dependency on `source`. No move occurs: if the function did not leak it, the caller could continue using its original iterator after `consumeLocally` returned.

#### Relevance To hd-lang

Kotlin local lifetimes is the closest surveyed design to hd-lang's narrow non-escape goal. It begins with one user-facing promise—this parameter is not retained—and extends only when an API intentionally returns or stores a dependent value.

The strongest ideas to borrow are transitive propagation through closures and aggregates, a distinction between non-retaining parameters and fields that retain values for an object's lifetime, and a limited positive route for returning a dependent value. hd-lang could initially support only “result lives no longer than parameter `x`” rather than Kotlin's full intersections, locality polymorphism, inheritance rules, and bounds.

The proposal does not solve hd-lang's special `Suspend[T]` requirement. Because it has no consume operation, storing a local suspension would restrict the stored object's lifetime but would not invalidate the caller's alias. A resource-owning cold suspension still needs a separate affine or consuming rule if the caller must relinquish responsibility.

### Pony Reference Capabilities

Pony is included as an adjacent compile-time aliasing and concurrency design, not as another lexical lifetime system. These examples target the current Pony 0.69-era syntax documented by the Pony tutorial. Pony remains pre-1.0, so surface details are more version-sensitive than Rust or Swift.

#### Overall Idea

Pony starts from actors. An actor is an object with private state that processes asynchronous messages one at a time. Code within one actor therefore runs sequentially, so ordinary local mutation is not the central danger. The hard problem is deciding which references may cross from one actor to another without creating shared mutable state and a data race.

A Pony reference capability describes an entire alias situation, not only what the current variable may do. `ref` means locally aliased mutable data and therefore stays inside one actor. `val` means the whole reachable graph is immutable and may be shared. `iso` means there is one usable read/write path to an isolated graph, so that path may be consumed and transferred. `tag` exposes identity and actor messaging without granting field access.

The key mental model is “what can every alias to this graph do?” rather than “how long does this reference live?” `consume` moves a binding while preserving a strong capability; it does not close a resource or change an object from open to closed. Pony is therefore valuable for understanding exclusive transfer and thread safety, especially for `Suspend[T]`, but it is not the main precedent for lexical non-escape or file cleanup.

#### Beginner Roadmap

Group Pony's six capabilities by purpose instead of memorizing a flat list:

1. **Local mutable access:** `ref` is an ordinary mutable alias; `box` is a read-only view of possibly mutable local data.
2. **Build then freeze:** `trn` provides one writer during construction; consuming it as `val` produces a deeply immutable, shareable graph.
3. **Transfer mutable data:** `iso` provides one usable read/write path and can be consumed into another actor.
4. **Identity only:** `tag` allows comparison and actor behavior sends but no field access.

For every example, ask two questions: “What may this reference do?” and “What may other aliases do?” The answer to both determines whether the reference may cross an actor boundary. The following walkthroughs present one complete program for each group.

#### Core Static Model

In Pony, a reference capability is a compiler-checked label attached to a reference type, as in `Document ref` or `Document val`. The label tells the compiler two things at once: which operations this reference may perform, and which kinds of aliases to the same object graph are allowed to exist elsewhere. An object graph means the object plus other objects reachable through its fields. A reference capability is not a lifetime and does not say whether a resource is open or closed.

Every Pony reference has one of six principal capabilities:

| Capability | Current access and alias guarantee | Actor-sendable |
|---|---|---:|
| `iso` | Isolated readable/writable graph; no competing usable read/write alias | Yes |
| `trn` | Sole local writer; read-only aliases may coexist | No |
| `ref` | Ordinary locally aliased mutable reference | No |
| `val` | Deeply immutable graph | Yes |
| `box` | Read-only viewpoint; another local alias may mutate | No |
| `tag` | Identity only; fields cannot be read or written | Yes |

The capabilities are primarily a data-race discipline. Mutable `ref` aliases may coexist inside one actor because that actor runs sequentially, but those references cannot be sent to another actor. An `iso` graph may cross an actor boundary because the type system rules out a competing readable or writable alias; a `val` graph may be shared because no alias can mutate it.

This model does not attach a lexical lifetime to a reference. Returning an object, storing it in a field, or capturing it in a closure is not inherently an escape error. Those operations preserve or adapt the reference capability instead.

#### Feature Walkthrough: `ref` And `box`

`ref` is Pony's ordinary locally mutable reference. Code may read and write through it, and other `ref` aliases may reach the same object. It is safe only inside one actor, where methods do not execute concurrently.

`box` is a read-only viewpoint. The holder of a `box` reference cannot mutate through that path, but another alias in the same actor may still mutate the object. It is therefore weaker than deep immutability.

This complete program keeps both views of one counter:

```pony
class Counter
  var _value: U64 = 0

  fun box value(): U64 =>
    _value

  fun ref increment() =>
    _value = _value + 1

actor Main
  new create(env: Env) =>
    let writable: Counter ref = Counter
    let readable: Counter box = writable

    writable.increment()
    env.out.print(readable.value().string())

    // Rejected if uncommented: increment requires a ref receiver.
    // readable.increment()
```

The same object is reachable through both variables. `writable` may mutate because it is `ref`; `readable` may only invoke methods whose receiver is `box`. The output is `1`, demonstrating that `box` does not freeze the object—the mutation performed through `writable` is visible through `readable`.

The receiver annotation on a method states the weakest capability that may invoke it. `fun box value()` promises to perform only operations allowed through a read-only viewpoint, while `fun ref increment()` requires ordinary mutable access.

#### Feature Walkthrough: `trn` And `val`

`trn`, short for “transition,” represents a graph under construction with one writable path. Read-only `box` aliases may exist, but no second writer may. The important intended transition is to consume the writer and obtain `val`.

`val` is deeply immutable: neither the current reference nor any other alias can mutate the reachable graph. That global immutability is what makes a `val` safe to share between actors.

`recover trn ... end` creates a restricted construction area in which the compiler can prove that the new graph has no uncontrolled outside aliases. `consume building` then moves the resulting reference out of its binding, making `building` unusable and preserving the strong alias guarantee during the transition.

This complete program builds an array mutably and then freezes it:

```pony
actor Main
  new create(env: Env) =>
    let building: Array[U8] trn =
      recover trn
        Array[U8]
      end

    building.push(10)
    building.push(20)

    let frozen: Array[U8] val = consume building

    for byte in frozen.values() do
      env.out.print(byte.string())
    end

    // Rejected if uncommented: val is deeply immutable.
    // frozen.push(30)

    // Rejected if uncommented: building was consumed.
    // building.push(30)
```

Before the transition, `building` is the sole writable path. `consume building` removes that path and permits the graph to be viewed as `val`. Afterward, the data can be shared safely because there is no remaining way to mutate it. This resembles a builder that becomes an immutable finished value; it does not mean that arbitrary `ref` data can be frozen while mutable aliases remain.

#### Feature Walkthrough: `tag`

`tag` preserves identity but grants no field access. A `tag` reference cannot read or write an object's data. Actor references are normally `tag` because callers should send behaviors rather than reach directly into actor state. In Pony syntax, `be` declares a behavior: an asynchronous message handler that another actor may invoke.

```pony
actor Greeter
  be greet(out: OutStream) =>
    out.print("hello")

actor Main
  new create(env: Env) =>
    let greeter: Greeter tag = Greeter

    greeter.greet(env.out)
```

Although `greeter` exposes no readable or writable actor fields, it can receive the asynchronous `greet` behavior. `tag` is safe to distribute between actors because identity and message sending do not grant concurrent memory access.

The remaining capability, `iso`, is the mutable-transfer capability. The next walkthrough gives it a complete actor-to-actor example. Unlike `trn`, which is commonly consumed into immutable `val`, `iso` is designed to remain mutable while exclusive control moves from one actor to another.

#### Feature Walkthrough: Isolation, Consumption, And Transfer

`consume` removes a value from a binding so that binding cannot be read again until reassigned. For an `iso` value, this preserves the stronger fact needed for exclusive actor transfer:

```pony
actor Receiver
  let _out: OutStream

  new create(out: OutStream) =>
    _out = out

  be accept(values: Array[U64] iso) =>
    let local: Array[U64] ref = consume values
    local.push(30)
    _out.print(local.size().string())

actor Main
  new create(env: Env) =>
    let values: Array[U64] iso =
      recover iso
        let building = Array[U64]
        building.push(10)
        building.push(20)
        building
      end

    let receiver = Receiver(env.out)
    receiver.accept(consume values)

    // Rejected if uncommented: values was consumed by the send.
    // env.out.print(values.size().string())
```

`recover iso` constructs a graph while temporarily restricting connections to outside mutable aliases. `consume values` then invalidates the sender's binding and supplies the isolated reference to the message. The receiver consumes its incoming `iso` into an ordinary local `ref`, which is safe because mutation now occurs inside one actor.

Two facts are doing different work. `consume` prevents reuse of this binding; `iso` ensures that some other usable read/write alias was not left behind. Invalidating only one variable name would not establish exclusive transfer if unrestricted aliases could still reach the graph.

Pony also supports `trn` for a sole writer with possible read-only aliases, `recover val` for constructing deeply immutable graphs, and destructive reads for replacing an isolated field while extracting its old value. These mechanisms all preserve alias guarantees rather than tracking a resource's logical open or closed state.

#### Why This Is Not Use-After-Close Checking

A reference capability describes access and aliases, not typestate. An `iso File` means that the file object is reached through one usable read/write path; it does not mean the file is currently open. Likewise, `consume file` means that one binding has been moved, not that the operating-system handle has been closed.

A library could require an isolated handle to be consumed into a closing function, making that particular caller binding unusable afterward. That would combine Pony's use-after-move rule with an API convention. The reference-capability system itself still would not prove that every handle is closed, automatically run cleanup at ownership end, or represent an `Open` to `Closed` state transition.

This distinction matters for comparison:

- lexical lifetime checking asks whether a path outlives its source;
- typestate asks whether an operation is valid in the resource's current protocol state;
- Pony asks what authority and aliases exist, especially across actors.

The questions interact, but none implies the others.

#### Returns, Closures, And Aggregates

Pony has no Rust-style borrowed-result lifetime or Kotlin-style dependent return. A returned reference retains its capability, and a closure capture retains a capability-qualified alias. Lexical scope alone does not make either operation invalid.

Aggregates are controlled through receiver capabilities and viewpoint adaptation. A field read through a `box` receiver cannot expose a stronger mutable reference than that viewpoint permits. Generic APIs write relationships such as `this->A` to adapt the stored field's capability through the capability of `this`. This closes authority-escalation holes in wrappers, but it is not containment propagation for a non-escapable lifetime.

#### What Pony Proves

For ordinary checked references, Pony's actor and capability rules prevent unsynchronized data races. Mutable graphs can be transferred when isolated, immutable graphs can be shared, and identity-only actor references can be distributed without exposing actor state.

Pony does not prove:

- lexical non-escape or borrowed-reference validity;
- that a resource is open, closed exactly once, or eventually closed;
- deterministic cleanup tied to ownership end;
- deadlock freedom, liveness, message ordering, or higher-level protocol correctness.

Its compile-time contribution is alias and actor-transfer safety.

#### Annotation Burden And Production Evidence

Defaults keep routine code relatively light: ordinary classes default to `ref`, primitives commonly use `val`, and actors use `tag`. Capability notation becomes visible at actor messages, isolated fields, recovery boundaries, and generic libraries. Advanced code adds ephemeral and alias forms, receiver capabilities, recovery, destructive reads, viewpoint adaptation, and capability constraints.

Pony has non-demo evidence. Wallaroo was a substantial distributed stream-processing system written in Pony, and projects such as Stallion and Corral exercised isolated buffers and capability-sensitive APIs. This demonstrates that the model can support real systems, while their archived or limited status and Pony's pre-1.0 ecosystem make the evidence historical rather than a sign of broad current adoption.

#### Relevance To hd-lang

Pony should not drive hd-lang's proposed `NonEscapable` or file-cleanup design. Kotlin local lifetimes, Swift non-escapability, and ownership or typestate mechanisms are closer precedents for those problems.

Pony is relevant to the proposed resource-owning `Suspend[T]`. If capturing a suspension consumes the caller's binding, hd-lang must decide whether that invalidation represents genuine exclusive transfer or merely removes one name while aliases remain. Pony's narrow lesson is that consumption and alias guarantees must be designed together when transferring mutable or resource-owning state across a concurrency boundary.

## Tentative hd-lang Preferences

The following are tentative preferences from the language designer. They are hypotheses for later investigation, not accepted decisions.

### Current Direction: Kotlin-Style Locality

As of 2026-09-18, the leading direction is to follow the semantic shape of Kotlin's local-lifetimes research rather than Swift's ownership model or a general Rust-style borrow checker.

The intended core is **locality tracking, not ownership tracking**:

- A restricted parameter promises that the callee will not retain it beyond the lifetime admitted by the signature.
- A returned value may remain dependent on one or more restricted inputs. The dependency is part of the function contract even if some simple cases are inferred or elided.
- Closures, structs, enum payloads, containers, and other aggregates transitively retain the dependencies of the values they capture or store.
- Restricted values remain ordinarily copyable and aliasable inside their permitted lifetime. Locality does not imply exclusive access, read-only access, consumption, or thread safety.
- A result depending on several inputs is valid only within their common lifetime. A bare binary `NonEscapable` property is therefore insufficient for general returned values; the compiler needs dependency provenance analogous to Kotlin's dependent lifetimes.
- hd-lang is not currently expected to gain a general user-visible `NonCopyable` facility as part of this work.

No concrete syntax or sigil has been selected. Terms such as `local`, `NonEscapable`, `T_{x}`, `from x`, and the notation in examples below are semantic placeholders only. Syntax should be considered separately after the behavior and common API shapes are understood.

Resource cleanup should likewise remain distinct from locality. The likely file-oriented design is a compiler-controlled lexical cleanup construct: aliases may be copied and used inside the region, cleanup occurs at region exit, and locality checking proves that none of those aliases survives the cleanup boundary. This does not by itself prove exactly-once manual close or prevent use after an early explicit close; the simplest design may therefore omit early manual close inside such a region.

`Suspend[T]` remains a possible narrow exception. A cold suspension must store its captured environment, so hd-lang may eventually give compiler-generated suspension frames a special transfer or invalidation rule. That would be a compiler-intrinsic rule for this carrier, not evidence that hd-lang should expose general noncopyable types. Its alias semantics, cancellation cleanup, abandonment, and nesting behavior remain unresolved.

### 1. A Narrow Locality Category

hd-lang may introduce restricted values analogous to Kotlin local lifetimes, initially motivated by scoped resources such as an open file.

The intended first problem is narrower than general ownership:

- A resource acquired for a lexical scope must not remain reachable after cleanup.
- The compiler should reject storing or returning aliases that outlive that scope.
- Ordinary GC-managed values should keep the current shared-reference model, including copying and aliasing within an allowed region.

The restriction is relative to a lifetime or dependency, not an absolute statement that the value can never cross a function boundary. A dependent result may cross the callee boundary while remaining bounded by caller-visible inputs.

### 2. Non-Escapability Propagates Through Containment

A data type, enum payload, tuple, collection element, closure environment, or other aggregate containing a non-escapable value should normally be non-escapable itself.

This property must be transitive. Otherwise an aggregate becomes a trivial smuggling mechanism:

```text
data HiddenFile:
    file: NonEscapableFile

fn leak(file: NonEscapableFile) -> HiddenFile:
    HiddenFile { file=file }  # must not turn the file into an escapable value
```

The same rule must be defined for:

- generic arguments and generic fields;
- enum payloads and tuples;
- lists, maps, optional values, and results;
- dynamic trait values and type erasure;
- closures and compiler-generated state machines;
- recursive types and indirect storage.

An aggregate may be allowed to expose a weaker borrowed view, but it cannot erase the underlying escape restriction.

### 3. Ordinary Longer-Lived Escape Is Rejected

A non-escapable value should not flow into storage that may outlive its permitted region. At minimum, reject:

- assignment into a binding declared in an outer lexical scope;
- assignment to a global or static value;
- return from a function when the result would outlive the originating scope;
- storage into an escapable object's field or collection;
- capture by an escaping closure;
- transfer into a detached task, callback, handler, or registration whose lifetime is not bounded by the region;
- erasure into a trait value that hides the restriction.

This should be a type/dataflow rule, not only a syntax rule. Passing through a helper function or aggregate must not bypass it.

Returning a restricted value is permitted only when the result contract preserves enough provenance for the caller to determine its lifetime. With one eligible source, the relationship might eventually be inferred or elided. With several sources, the signature must encode which inputs the result may retain, or conservatively depend on all of them. Returning a value backed only by a callee local remains invalid.

### 4. `Suspend[T]` May Be A Controlled Locality Carrier

hd-lang's `Suspend[T]` represents a cold computation. Constructing it evaluates and stores its arguments, captured dependencies, and any other required environment before execution begins. Therefore a blanket rule that non-escapable values can never enter stored data would make suspending functions unable to retain scoped resources across their own suspension points.

The tentative exception is:

- A compiler-generated `Suspend[T]` frame may capture a non-escapable value.
- Capturing it may need to invalidate the caller's usable path under a compiler-intrinsic rule, because the cold computation stores the value for later use.
- The frame becomes responsible for keeping the captured value within its permitted lifetime and participating in cleanup on completion, cancellation, or destruction.
- If invalidation is required, the compiler must define what happens to other aliases; invalidating one binding alone does not establish exclusive ownership.

Conceptually, this is not an ordinary unrestricted escape. It is controlled storage inside a compiler-generated carrier. The current direction does not generalize this mechanism into user-defined noncopyable values.

This preference creates requirements that the current `Suspend[T]` design does not yet satisfy statically:

1. **Consuming the caller binding is insufficient if other aliases remain.** The design needs uniqueness, an unaliased resource capability, or a shared handle whose cleanup state is checked dynamically.
2. **The suspension may need a special single-driver or single-transfer rule.** Current runtime panics prevent simultaneous driving but do not provide a compile-time guarantee. This need not become a general `NonCopyable` facility.
3. **Abandonment needs semantics.** Dropping an unstarted or pending cold suspension must either clean up its captures, make abandonment a compile-time error, or transfer responsibility somewhere else.
4. **Cancellation must finalize captures.** Synchronous cancellation should release owned non-escapable resources on every terminal path.
5. **Nested suspensions must propagate responsibility.** If one frame owns a child frame, completion and cancellation must not duplicate or lose cleanup ownership.
6. **The result type is separate.** A `Suspend[T]` may own non-escapable captures even when `T` is escapable; conversely, returning a non-escapable `T` introduces a new lifetime/ownership boundary.
7. **Durable replay remains incompatible with live resources unless explicitly bridged.** A process-local file handle cannot simply be serialized into workflow history. It must be completed before a durable boundary, prohibited, or represented by a reacquirable logical capability.

The leading preference is to keep any such exception specific to compiler-generated suspension frames unless concrete use cases later justify a general carrier mechanism.

### 5. Generalize Escape Checking To Closures And Possibly Returns

Non-escape checking should naturally extend to closures because a closure is an aggregate containing captured values.

Possible closure rules, from least to most expressive, are:

1. A closure capturing a non-escapable value becomes non-escapable and may be called only within the same region.
2. A scoped callback parameter may borrow a non-escapable value but the callback itself cannot be retained by the callee.
3. A compiler-controlled closure or suspension may receive a narrow invalidating-capture rule, without making general closures or user-defined values noncopyable.
4. A closure may escape only when its environment is proven independent of the original region and it owns all required cleanup responsibility.

Returning a non-escapable value has several distinct meanings and should not be treated as one feature:

- **Return a borrow tied to an input.** The result may be returned because it cannot outlive a caller-provided owner. This requires inferred regions or explicit lifetime dependencies.
- **Return a fresh caller-scoped value.** The callee creates a value whose permitted region is established at the call site. This requires a precise region-opening rule and is distinct from a result borrowing an input.
- **Return an existentially scoped value.** The lifetime is fresh and hidden. In practice, the caller usually needs a scoped callback or region-opening construct to use it safely.
- **Return a shared runtime handle.** Aliases may escape, but operations after close return a typed runtime error. This is not a compile-time use-after-close guarantee.

The conservative starting point is to support input-dependent returns only when their dependency is represented in the function contract. Closure propagation should be designed at the same time so a wrapper cannot erase that dependency.

## What A Narrow Non-Escape Checker Would Prove

If designed soundly, a narrow checker could prove:

- a scoped resource reference does not outlive its region;
- aggregates cannot hide and leak the reference;
- an escaping closure cannot retain it;
- a compiler-controlled carrier cannot erase the locality of captured values;
- cleanup at lexical scope exit leaves no statically usable alias outside the scope.

It would not automatically prove:

- that cleanup runs unless paired with a cleanup protocol or compiler construct;
- that a resource is closed exactly once;
- that an alias is not used after an early manual `close()` inside the same region;
- that there are no multiple mutable aliases within the region;
- thread or task data-race freedom;
- `Send`/`Sync`-style concurrency admissibility;
- deadlock freedom, liveness, or protocol correctness.

This distinction is crucial. **Non-escape checking prevents aliases from outliving a cleanup boundary; it does not by itself create deterministic cleanup or typestate.**

The simplest sound file design might therefore combine:

1. a compiler-recognized or protocol-defined scoped resource;
2. cleanup guaranteed at scope exit;
3. a non-escape rule for the resource and every value retaining it;
4. no early manual close, or a separate state/alias policy for early close;
5. a separately specified compiler-intrinsic rule if suspensions must invalidate captured caller paths.

## Potential Design Shapes

### Shape A: Pure Scoped Non-Escape

- Resources exist only inside a `using`-like lexical region.
- They may be freely aliased inside that region.
- Neither the resource nor an aggregate/closure retaining it may escape.
- Cleanup runs at region exit.
- No user-visible ownership or lifetime parameters.

This is the smallest surface and directly addresses the common close-file case. It cannot transfer resource responsibility to callers or detached work.

### Shape B: Kotlin-Style Locality Plus A Suspension Exception

- Shape A remains the default.
- Dependent result lifetimes allow values to remain tied to caller-visible inputs.
- `Suspend[T]` may receive a compiler-intrinsic stored-capture and invalidation rule.
- Cancellation and abandonment semantics must clean up or otherwise discharge captured scoped resources.

This fits the current direction without introducing general user-defined noncopyability, but the suspension rule still needs a precise account of aliases.

### Shape C: General Noncopyable Ownership And Borrowing

- User-defined types can opt out of copying.
- Parameters distinguish borrow, mutable borrow, and consume.
- Non-escape becomes one property of borrowed views.
- Suspensions and closures use the same general transfer rules as other aggregates.

This is more uniform and expressive, but approaches Swift's full ownership surface and affects much more of the language.

### Shape D: Full Ownership Plus Concurrency Traits

- Move/borrow/lifetime rules apply broadly.
- Structural traits govern task transfer and shared access.
- The compiler can provide Rust-like resource, memory, and data-race guarantees.

This has the strongest general guarantees and the largest implementation impact. It should not be selected merely to close files safely.

The tentative preferences currently point toward **Shape A plus Kotlin-style dependent results**, with only the narrow `Suspend[T]` part of Shape B under consideration. Shapes C and D are not current goals.

## Interactions That Must Be Designed Together

### `T` And `mut T`

Non-escapability must remain separate from mutation permission. A value can be read-only and non-escapable, or mutable and non-escapable. Weakening `mut T` to `T` must not erase an escape restriction.

### Generics And Variance

Generic types must propagate non-escapability from their arguments and fields. Covariant weakening cannot turn `Container[NonEscapable]` into an escapable container. Generic bounds need a way to preserve or intentionally require escape behavior without infecting ordinary APIs with unnecessary annotations.

### Dynamic Trait Values

Erasure cannot hide non-escapability. A dynamic trait value must either retain the escape property in its erased representation/type, reject the conversion, or be bounded by the same region.

### Closures

Closure types need to record at least whether their environment may escape. This is related to, but distinct from, the existing `fn` versus `mut fn` distinction and from captured `$` requirements.

### `Suspend[T]`

The type may need properties derived from both its result and its captured frame. `Suspend[EscapableResult]` is not necessarily escapable if its frame owns a scoped resource. Conversely, an otherwise escapable frame producing a non-escapable result creates an obligation when driven.

### Cleanup And Cancellation

A cleanup protocol must define normal return, `Result` propagation, defects, cancellation, never-started suspensions, pending suspension destruction, and multiple cleanup failures. Non-escape checking cannot substitute for these control-flow semantics.

### Durable Replay And Serialization

Live process resources normally cannot cross persistence boundaries. Escape safety inside one process does not make a file descriptor serializable or replayable. The compiler should distinguish process-local owned frames from durable workflow state.

### Concurrency

Non-escape does not imply sendability. If a resource-owning `Suspend[T]` moves between tasks or threads, hd-lang still needs a rule analogous to `Send`, disconnected-region transfer, actor isolation, or a guarantee that scheduling remains single-threaded.

## Reasons To Defer

Even a narrow checker affects:

- assignment and parameter passing;
- generic substitution, variance, and containers;
- trait objects and type erasure;
- closure environment inference;
- cold suspension frame construction;
- cancellation and destruction;
- resource cleanup syntax and protocols;
- workflow replay and serializability;
- task and concurrency APIs;
- Wasm GC representation and host handles;
- diagnostics and backward compatibility.

A partial feature described as general safety would be worse than keeping hd-lang's shared-reference behavior explicit. The language should first obtain concrete resource and concurrency APIs against which candidate rules can be tested.

## Conditions For Reopening The Design

Revisit this area only after:

1. `T`/`mut T`, generics, variance, traits, dynamic trait values, and standard containers are implemented and tested.
2. Closure capture and compiler-generated `Suspend[T]` frame representations are concrete.
3. Cancellation and destruction paths for cold computations are specified.
4. Resource cleanup has concrete file, network, database, and host-handle examples.
5. Durable replay boundaries distinguish live process resources from replayable logical state.
6. hd-lang has selected an initial concurrency model.
7. The compiler has control-flow and dataflow infrastructure suitable for escape, lifetime-dependency, and capture analysis, plus any special invalidation analysis ultimately required by `Suspend[T]`.
8. The desired guarantee is stated precisely: lexical non-escape, exactly-once cleanup, use-after-close prevention, ownership transfer, data-race freedom, or an explicit combination.

## Questions For Later Investigation

1. Is non-escapability inferred from resource protocols, declared on types, or both?
2. Is the property attached to a value, a reference, a type, or a region-qualified use of a type?
3. How should diagnostics explain that restricted values remain copyable and aliasable inside their region even though their dependencies propagate?
4. Is manual early close prohibited, statically tracked, or a typed runtime operation?
5. Does every aggregate propagate non-escapability structurally, and how is that represented after type erasure?
6. Are closure escape properties part of function types?
7. What exact compiler-intrinsic privilege, if any, does `Suspend[T]` receive for stored restricted captures?
8. Can the suspension rule remain narrow without introducing a general affine/noncopyable type feature?
9. What happens when such a suspension is never driven, cancelled, dropped, or persisted?
10. Can a non-escapable result be returned when its lifetime is tied to an input?
11. Can fresh ownership be returned, and if so, how is cleanup responsibility represented?
12. Can a scoped callback provide most borrowed-return use cases without named lifetime syntax?
13. Does task transfer require a separate structural `Send`-like property?
14. What amount of annotation is acceptable at ordinary call sites and library boundaries?

## Production Evidence And References

### Rust

- [Ownership](https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html)
- [Lifetime relationships](https://doc.rust-lang.org/book/ch10-03-lifetime-syntax.html)
- [Closure capture and `move`](https://doc.rust-lang.org/reference/expressions/closure-expr.html)
- [`E0373`: borrowed capture may outlive its function](https://doc.rust-lang.org/error_codes/E0373.html)
- [`E0502`: conflicting mutable and immutable borrows](https://doc.rust-lang.org/error_codes/E0502.html)
- [`E0515`: returning a reference to a local](https://doc.rust-lang.org/error_codes/E0515.html)
- [`Send` and `Sync`](https://doc.rust-lang.org/nomicon/send-and-sync.html)
- [Tokio task boundary](https://docs.rs/tokio/latest/tokio/task/fn.spawn.html)
- [Serde borrowed deserialization](https://docs.rs/serde/latest/serde/trait.Deserialize.html)
- [Ripgrep parallel walker](https://github.com/BurntSushi/ripgrep/blob/master/crates/ignore/src/walk.rs)

### Swift

- [Parameter ownership modifiers](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0377-parameter-ownership-modifiers.md)
- [Noncopyable structs and enums](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0390-noncopyable-structs-and-enums.md)
- [Noncopyable generics](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0427-noncopyable-generics.md)
- [Non-escapable types](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0446-non-escapable.md)
- [Standard-library `Span` properties and lifetime dependencies](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0456-stdlib-span-properties.md)
- [Nonescapable standard-library primitives](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0465-nonescapable-stdlib-primitives.md)
- [Borrowing iteration and experimental `@_lifetime`](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0516-borrowing-sequence.md)
- [`Sendable`](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0302-concurrent-value-and-concurrent-closures.md)
- [Region-based isolation](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0414-region-based-isolation.md)
- [`sending`](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0430-transferring-parameters-and-results.md)
- [Swift System noncopyable Mach port](https://github.com/apple/swift-system/blob/main/Sources/System/MachPort.swift)
- [Foundation lifetime-dependent `Data` views](https://github.com/swiftlang/swift-foundation/blob/main/Sources/FoundationEssentials/Data/Data.swift)

### Scala

- [Capture-checking overview](https://docs.scala-lang.org/scala3/reference/experimental/capture-checking/index.html)
- [Capture-checking basics](https://docs.scala-lang.org/scala3/reference/experimental/capture-checking/basics.html)
- [Capability polymorphism](https://docs.scala-lang.org/scala3/reference/experimental/capture-checking/polymorphism.html)
- [Capability classifiers](https://docs.scala-lang.org/scala3/reference/experimental/capture-checking/classifiers.html)
- [Scoped capabilities](https://docs.scala-lang.org/scala3/reference/experimental/capture-checking/scoped-capabilities.html)
- [Separation checking](https://docs.scala-lang.org/scala3/reference/experimental/capture-checking/separation-checking.html)
- [Stateful capabilities](https://docs.scala-lang.org/scala3/reference/experimental/capture-checking/mutability.html)
- [`E223`: capture-set diagnostic](https://docs.scala-lang.org/scala3/reference/error-codes/E223.html)
- [Standard-library evaluation](https://bracevac.org/assets/pdf/oopsla25full.pdf)
- [TACIT](https://github.com/lampepfl/tacit)
- [Orca](https://github.com/VirtusLab/orca#experimental-capabilities--compile-time-concurrency-checking)

### Kotlin Local Lifetimes

- [Local Lifetimes for Kotlin design notes](https://github.com/Kotlin/KEEP/blob/main/notes/0007-local-lifetimes.md)
- [Local Lifetimes design discussion](https://github.com/Kotlin/KEEP/discussions/485)
- [Current Kotlin `callsInPlace` contract](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.contracts/-contract-builder/calls-in-place.html)
- [Current Kotlin `AutoCloseable.use`](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin/use.html)

### Pony

- [Pony reference-capability overview](https://tutorial.ponylang.io/reference-capabilities/reference-capabilities.html)
- [Pony capability matrix](https://tutorial.ponylang.io/reference-capabilities/capability-matrix.html)
- [Pony consume and destructive reads](https://tutorial.ponylang.io/reference-capabilities/consume-and-destructive-read.html)
- [Pony viewpoint adaptation](https://tutorial.ponylang.io/reference-capabilities/arrow-types.html)
- [Stallion request parser](https://github.com/ponylang/stallion/blob/main/stallion/_request_parser.pony)
- [Corral project model](https://github.com/ponylang/corral/blob/main/corral/bundle/project.pony)
- [Wallaroo](https://github.com/WallarooLabs/wally)
- [Pony compiler releases](https://github.com/ponylang/ponyc/releases)

### Other Languages And Designs

- [Clean uniqueness typing](https://wiki.clean.cs.ru.nl/download/html_report/CleanRep.2.2_11.htm)
- [GHC linear types](https://downloads.haskell.org/ghc/latest/docs/users_guide/exts/linear_types.html)
- [Move abilities](https://move-book.com/reference/abilities.html/)
- [Move references](https://move-book.com/reference/primitive-types/references/)
- [Austral specification](https://austral-lang.org/spec/spec.html)
- [Mojo ownership](https://mojolang.org/docs/manual/values/ownership/)
- [Mojo lifetimes and origins](https://mojolang.org/docs/manual/values/lifetimes/)
