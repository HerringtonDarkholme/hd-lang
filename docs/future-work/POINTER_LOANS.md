# Pointer Loans: `*T` As `Rc<RefCell<T>>`

Accepted 2026-11-03 (model A); domain: references, ownership passes, runtime.
Being built under #1336, part of #1205. The owner chose A: a garbage-collected
language iterates `for a in arr`, a copy, never `for a in &arr`, so the friction
cases are written as value loops over copy-on-write lists and take no loan.

## The Proposal

This note covers aliasing through a `*T`: two names that hold the same
pointer. Paths with no `*T` are settled, checked statically at no cost
(#1320). Two models are compared, and both keep every conflict the
checker can see a compile error.

- **A. Loans.** Every counted object carries a borrow state, as a Rust
  `RefCell` does: an exclusive bit and a shared count in six spare
  descriptor bits, so no object grows. A reference through a `*T` takes a
  **loan** on each object it crosses until its last use, unless nothing
  in its live range can write those types through a pointer
  (**per-reference elision**). An alias's conflicting access panics
  `borrow-conflict`.
- **B. Keep-alive, as Go.** A reference through a `*T` counts the
  storage it points into, which is never freed while counted. There is no
  exclusivity: an alias may write at any time, and a reference may see a
  replaced or grown collection's old storage, stale but never freed. A
  `UniqList`/`UniqMap` room keeps the pin (`collection-pinned`).

```text
struct Team:
    lead: *mut User
    log: List[string]

struct User:
    name: string
    age: i32

fn rename(t: *mut Team, alias: *mut Team) -> string:
    n := &t.lead.name           # A: shared loans on the Team and the User; B: a count of the User
    t.log.push("renamed")       # both run: t's own path, a field the checker proves apart
    alias.lead.age += 1         # A: panics borrow-conflict; B: runs
    *n

fn same_root(t: *mut Team) -> string:
    n := &t.lead.name
    t.lead = box(User { name: "b", age: +1 })  # both: conflicting-access at compile time
    *n

fn greet(u: *User) -> void $ Console:
    n := &u.name                # A: no loan, nothing in range writes a User
    println(*n)
```

**Recommendation: A.** It keeps `&mut` exclusive and every read of a
reference the value that its own writes left, and it panics where B
silently loses a write. With elision it adds nothing to any bench
program, and where a loan stays it costs 0.5 ns against B's 8 ns count.
B is about 1,500 lines smaller to build and removes the friction of
object granularity, at the cost of stale reads and lost writes. The
owner leans to B; its plan is [E.4](#e4-model-b-implementation-phases).

## Open Questions

1. **A or B?** Recommended: A, for the reasons above.
2. **Per-reference elision** by per-type release write summaries?
   Recommended: yes; it removes every loan in the 22 bench programs.
3. **Object granularity (A).** A write to another field of a borrowed
   object through an alias panics, where today it runs. Accept, as
   `RefCell` does? Recommended: yes; the fix is a value loop or a boxed
   field.
4. **Own loans (A).** May a write through the loan's own pointer, to a
   field the checker proves apart, skip the function's own loans?
   Recommended: yes; else `for x in &p.items: p.count += 1` panics.
5. **Element references (A).** No copy of a collection while a `&mut`
   element reference lives, and no write while a `&` one lives?
   Recommended: yes; it replaces the pin.
6. **Captured variables.** Treat a variable a closure shares as behind a
   `*mut` (its cell)? Recommended: yes, in both models.
7. **Shared-count overflow (A)** past 31: spill to a side table or
   panic? Recommended: spill.

## Problems It Solves

**1. Freed memory behind a pointer.** Today a `UniqList` behind a `*T`
can be replaced while an element reference into it lives, and the
reference reads freed memory: 27 matrix cases on main (two of them
#1327). A panics `borrow-conflict` first; B panics `collection-pinned`.

**2. One rule for every alias.** Today a field reference registers an
address range, a `List`/`Map` element reference pins its buffer, and a
`UniqList` one counts its room. Of the 283 matrix conflicts on `*T`
paths, 103 run with no panic today and 56 panic `collection-pinned`. A
gives all of them one borrow state; B makes all but the `UniqList` ones
legal.

**3. The cost of a spanning reference.** Today one costs a page-table
count and a loan-table entry, 16.6 ns in the micro. A makes it a 0.5 ns
loan, or nothing when elided; B keeps the 8.0 ns count.

## How It Works

**Static first, in both models.** Every rule of
[Writes While Borrowed](../../spec/lang/13-values-and-references.md#writes-while-borrowed)
applies through pointers, since `ref.excl.place` covers fields and
dereferences of one root, and two more cases are compile errors:

- A pointer argument is an access to its whole pointee for the call's
  other arguments: `*mut T` a write, `*T` a read.
  `mutate(boxed, &boxed.field)` and `inspect(view, &mut view.field)` are
  `conflicting-access`; `inspect(view, &view.field)` is two reads. At
  run time passing a pointer is no access.
- A call result that depends on a `*T` argument is a local reference to
  that argument's root (`ref.dep.call`): `r := borrow(boxed)` then
  `boxed.change()` conflicts, and `r` cannot outlive the local `boxed`.

Only a second name for the same pointer (`alias := a`, a field, a call
result, a parameter) reaches the run-time model.

**A: state.** Descriptor bits 16 to 21 (§23.4 of
`crates/hd_wasm_linear/DESIGN.md`): bit 21 exclusive, bits 16 to 20 the
shared count. The release glue's slot keeps bits 0 to 15; the largest
bench program, `json`, has 48 table entries.

**A: loans.** A reference that crosses `*T` objects `o1 … on` takes a
shared loan on `o1 … o(n-1)` and, on `on`, a shared loan for `&` or an
exclusive one for `&mut`. An argument's loans run from its evaluation to
the return; a `&mut self` receiver is shared until the call starts. A
loan ends after the last use of every value holding the reference.

**A: checks.** Taking a shared loan checks the exclusive bit, an
exclusive one the whole state. A write through a `*T` checks the whole
state, a read the exclusive bit, each against the function's own loans
through the same pointer local.

**A: per-reference elision.** A reference takes no loan, and needs no
matching check, when nothing between taking it and its last use can
write any type it crosses through a pointer: no write through a `*T` of
that type, and every call has a release write summary that writes none
(for `&mut`, neither reads nor writes). A function value, a trait call
whose implementations are not all known, a suspension point and an
erased body count as writing everything. A type none of whose loans
survive needs no check anywhere.

**A: element references.** With no pin, only a copy can detach a
reference from its slot, so a copy while a `&mut` element reference
lives and a write while a `&` one lives are `conflicting-access`.

**B: counts.** A reference through a `*T` that leaves a short loan
counts each object it crosses and the `List`/`Map` buffer or string it
points into (#1320's rule, kept). An alias's write, growth or
replacement goes ahead; the reference reads the storage it counted. An
element store is seen in place, and a copy of a list with a live
element reference still gets its own elements (the pin's eager copy
stays; only its length panic goes). A
`UniqList`/`UniqMap` growth, shrink or replacement while an element
reference lives panics `collection-pinned`, since its elements move.

**Both: roots.** The local or parameter holding the first `*T` of a live
reference's path lives until the reference's last use; a suspending
body's frame owns its pointer parameters (#1327 is today's bug).

| A's operation (`o` the payload address) | Code |
| --- | --- |
| shared loan begin / end | `s = load(o-4)`; `if s & X: panic`; `store(o-4, s + 1<<16)` / subtract |
| exclusive loan begin / end | `if load(o-4) & M != own: panic`; set `X` / clear it |
| write through `*T` | `if load(o-4) & M != own: panic`, then the store |
| read through `*T` | `if load(o-4) & X != own: panic`, then the load |

## `*T` Where `&T` Is Expected

```text
struct Shape:
    w: f64
    h: f64

impl Shape:
    fn area(&self) -> f64: self.w * self.h

fn total(p: *Shape) -> f64:
    p.area()                    # the receiver is &*p
```

In A, `p.area()` lends `&*p` with a shared call loan for the call, and
elision removes it because `area` writes nothing through a pointer. In
B it lends `&*p` as a short loan, with no count. Std passes a `*T` to a
`&T` parameter 26 times, and calls a `&self` method on a `*T` 66 times
and a `&mut self` one on a `*mut T` 188 times (the 18 std files every
std import reaches). The bench's own code does it 19 times, the stress
cases 5 times.

## Two Models Compared

| Below the pointer | A: loans, with elision | B: keep-alive |
| --- | --- | --- |
| a plain field | an alias's conflicting access panics | runs; the reference sees the write |
| data behind a nested `*T` | replacing the inner pointer panics | runs on the old object, stale |
| a `List`/`Map` | a length change or element write panics | an element store is seen in place; a grown or replaced list leaves the reference on the old buffer, stale, and its later writes are lost |
| a `UniqList`/`UniqMap` | a length change or replacement panics | panics `collection-pinned` |

| Measure | A | B |
| --- | --- | --- |
| 362 matrix cases on `*T` paths | panics in 283, runs in 79 (all 79 elided) | runs 302 (74 of them stale), panics 60 |
| stress friction (Appendix A) | A2 and A4 panic | none |
| semantics lost | none | `&mut` exclusivity behind a pointer; writes through stale element references |
| bench time, release, default V8 tier | elision keeps 0 loans, so it adds nothing; it also drops main's counts (not measured) | byte-identical to main: its counts stay, and no bench program emits the loan table it removes |
| bench size | 0 after elision; +1,024 B in `json` without | 0 |
| per spanning reference (micro) | 0.5 ns, or 0 elided | 8.0 ns |
| implementation (estimated) | about −1,800 and +1,700 lines: checker, ownership insertion, release summaries, emitter, runtime, std | about −1,500 and +150 lines: checker, emitter, runtime |

## Delta From Main

Main (156cba91c, after #1320) already has parts of both. A reference
through a `*T` counts the object through the page table (B's keep-alive,
stale when replaced). A `List`/`Map` element reference pins its buffer,
and a `UniqList`/`UniqMap` one counts its room, so a length change panics
`collection-pinned`; replacing or dropping a unique collection is still
unsound. A binding reference registers an address range in the loan
table, which is A's exclusivity, field-precise; the call and path loans
run in debug and test builds only, so verdicts differ by mode (#1331).

| | A: loans | B: keep-alive |
| --- | --- | --- |
| added | loan bits and checks in every build; own loans, the element rules and cells in the checker; per-type write summaries for elision | a pin check on replacing or dropping a pinned unique collection; the roots rule |
| removed | page-table counts on references through `*T`, the loan table, the `List`/`Map` pin, the room count | the loan table; the `List`/`Map` pin's length panic behind a `*T` (its eager copy stays, so `ref.elem.copy` holds) |
| behaviour, 362 matrix cases on `*T` paths | 100 `borrow-conflict` panics stay; 56 `collection-pinned` become `borrow-conflict`; 103 that run start to panic; 24 unsound ones panic; 79 still run | 92 `borrow-conflict` panics run instead and 8 become `collection-pinned`; 28 `List`/`Map` pins run on stale buffers; 28 unique pins stay; 24 unsound ones panic `collection-pinned`; 182 still run |
| all builds agree | yes | yes |
| per spanning reference (micro, against main's 16.6 ns) | 0.5 ns, or 0 elided | 8.0 ns |
| bench time and size against main | adds nothing after elision; removes main's counts (not measured) | byte-identical: no bench program emits the loan table |

## Cost

| Component | A | B |
| --- | --- | --- |
| code removed | the loan table (`emit/excl.rs`, 1,151 lines, and `rt:loan_*`, about 220), the `List`/`Map` pin, #1320's room count, page-table counts on references through `*T` | the loan table, and the `List`/`Map` pin's length panic behind a `*T` |
| code added (estimated) | loans and checks inline, about 400 lines; own loans, the element rules and cells in the checker, about 500; loan ends in the ownership insertion, about 300; per-type write summaries, about 250; `rt:loan_spill` | the same-root checker rules both share, about 150; the unique replace and drop check and the roots rule, about 100 |
| runtime, bench (measured) | nothing added after elision; without it `binary_trees` +7%, `json` +1.5% | byte-identical to main |
| compile time (estimated) | per-type summaries in release, under 1% | none |
| web compiler (estimated) | under +10 KB gzip, far below the 2 MB cap | smaller |

Method and numbers: [E.3](#e3-overhead-measured).

## Appendix A. Stress Test

Design review, 2026-11-03; it changes no decision, spec text or
compiler code. Under test: the proposal above, against
[Conflicts Through Aliases](../../spec/lang/13-values-and-references.md#conflicts-through-aliases)
and [Element References](../../spec/lang/13-values-and-references.md#element-references)
as they stand. Each case is a real library's shape, ported to hd and
checked with `hd check` on main; library shapes are approximations.

**Verdicts.** *Works*: same answer, no rewrite. *Friction*: panics
where the original does not, or needs a rewrite. *Stricter*: panics
where today's hd silently gives a wrong or freed value. Costs count only
references through a `*T`: a **loan** is a begin and an end, two
read-modify-writes of the header; a **check** is a load, a mask and a
branch; a **count** is a page-table retain and release.

| Case | Library | A: loans | A: loans elided | B: keep-alive |
| --- | --- | --- | --- | --- |
| A1 depth-first search | petgraph `Dfs` | works | none: the loop's range writes `Node` | works; a count per loop |
| A2 in-degree count | petgraph `neighbors` | friction: a self-loop panics | none | works |
| A3 tree with parent links | DOM, `rctree` | works | all: `total` and `weak_mut` write no `Node` | works |
| A4 event listeners | Node `EventEmitter` | friction: a listener writing the bus panics | none: a trait call writes `Bus` | works; an unsubscribe leaves the loop on the old list |
| A5 ECS system | bevy, specs | works (needs own loans) | the `&mut` one; the `&` one stays | works; a room count per element |
| A6 memo recursion | `functools.lru_cache` | compile error, as Rust | all | compile error |
| A7 doubly linked list | Go `container/list` | works | all; no check stays | works |
| A8 interpreter scopes | Lox `Environment` | stricter: panics | none: the callee writes `Env` | runs, and adds the value changed mid-expression |
| A9 self-replacing list | the #1320 `UniqList` repro | stricter: panics `borrow-conflict` | none | panics `collection-pinned` |

Adversarial cases are the `pointer-loan-extra-*` fixtures and the
matrix itself (Appendix D); their verdicts are in the table at the end of
this appendix.

**Conclusion.** By its rules A panics in every case the matrix finds
unsound today and keeps all of today's static rejections; with elision it
costs nothing in four of the nine cases. This is reasoned, since nothing
is built. B removes the friction of A2 and A4, and in exchange `&mut` no
longer means that no one else changes the place (A8), and a write
through a stale element reference is lost.

### A1. Depth-First Search

```rust
// petgraph: let mut dfs = Dfs::new(&graph, start); while let Some(n) = dfs.next(&graph) { ... }
fn visit(n: &Rc<RefCell<Node>>, order: &mut Vec<i32>) {
    n.borrow_mut().seen = true;
    order.push(n.borrow().id);
    for next in n.borrow().edges.iter() {
        if !next.borrow().seen { visit(next, order) }
    }
}
```

```text
struct Node:
    id: i32
    edges: List[*mut Node]
    seen: bool

fn visit(n: *mut Node, order: &mut List[i32]) -> void:
    n.seen = true
    order.push(n.id)
    for next in &n.edges:      # a shared loan on n for the loop
        if !next.seen:          # a read check on next: no exclusive loan
            visit(*next, order) # writes next.seen; next has no loan
```

Works, with cycles and self-loops: a node is marked before its loop, so
the loop never writes a node it holds a loan on. Today the loop pins
`n.edges`; under B it counts the buffer.

### A2. In-Degree Count

```text
struct Node:
    edges: List[*mut Node]
    indegree: i32

fn count_in(nodes: List[*mut Node]) -> void:
    for n in nodes:
        for e in &n.edges:
            e.indegree += 1     # panics borrow-conflict when e is n
```

Friction: a self-loop makes `e` the node whose loop holds the loan, and
the write panics. The original Rust `RefCell` port panics the same way
(`already borrowed`). Today it runs. The fix is `for e in n.edges`, a
value loop, which takes no loan.

### A3. Tree With Parent Links

```text
use std.weak.{WeakMut, weak_mut}

struct Node:
    size: i32
    parent: WeakMut[Node]?
    children: List[*mut Node]

fn add_child(parent: *mut Node, child: *mut Node) -> void:
    child.parent = weak_mut(&mut *parent)
    parent.children.push(child)

fn total(node: &Node) -> i32:
    let sum = node.size
    for c in &node.children:
        sum += total(*c)        # a call loan on each child, shared
    sum
```

Works. `weak_mut(&mut *parent)` takes an exclusive loan for one call;
the size walk takes shared loans only. An upgraded `WeakMut` is an
ordinary `*mut` local, so a reference through it is rooted there.

### A4. Event Listeners

```javascript
// Node: emitter.on("tick", () => { emitter.fired += 1 }); emitter.emit("tick")
```

```text
trait Listener:
    fn on(&mut self) -> void

struct Bus:
    listeners: List[*mut dyn Listener]
    fired: i32

struct Counter:
    bus: *mut Bus
    seen: i32

impl Listener for Counter:
    fn on(&mut self) -> void:
        self.seen += 1
        self.bus.fired += 1     # panics: emit's loop holds a loan on the bus

fn emit(bus: *mut Bus) -> void:
    for l in &bus.listeners:
        l.on()

fn emit_snapshot(bus: *mut Bus) -> void:
    for l in bus.listeners:     # a copy of the list: no loan
        l.on()
```

Friction. Today `emit` runs and prints 2; a listener that unsubscribes
panics `collection-pinned`. Under the proposal both panic
`borrow-conflict`, and `emit_snapshot` runs both, with the snapshot
semantics of Java's `CopyOnWriteArrayList` and the DOM's listener list.
A GUI widget tree that writes app state from a handler during a render
loop over `&state.items` is the same case.

### A5. ECS System

```text
use std.collections.UniqList

struct Vec2:
    x: f64
    y: f64

struct World:
    pos: UniqList[Vec2]
    vel: UniqList[Vec2]

fn step(w: *mut World, dt: f64) -> void:
    let i: usize = 0
    while i < w.pos.len():
        v := w.vel.get_ref(i).expect("velocity")  # shared loan on the world
        p := w.pos.get_mut(i).expect("position")  # exclusive: only own loans
        p.x += v.x * dt
        i += 1
```

Works only with own loans (question 2): both references go through `w`,
and the checker proves `pos` and `vel` apart. Two systems holding two
pointers to one world would panic, as two `RefCell` borrows of one
`World` do; bevy splits component storage for that reason.

### A6. Memo Recursion

```text
use std.collections.UniqMap

struct Memo:
    table: UniqMap[u64, u64]

fn fib(m: *mut Memo, n: u64) -> u64:
    if n < 2:
        return n
    match m.table.get(n):
        .Some(hit) => return hit
        .None => pass
    value := fib(m, n - 1) + fib(m, n - 2)
    _ := m.table.insert(n, value)
    value
```

Works. The tempting form, `slot := m.table.get_or_insert(n, 0)` then
`*slot = fib(m, n - 1) + ...`, is `conflicting-access` today and under
the proposal, as Rust's E0499 is. Through a second pointer to the memo it
would panic `borrow-conflict` instead of writing a freed slot.

### A7. Doubly Linked List

```text
use std.weak.{WeakMut, weak_mut}

struct Node:
    value: i32
    prev: WeakMut[Node]?
    next: (*mut Node)?

struct Deque:
    head: (*mut Node)?

fn push_front(d: *mut Deque, value: i32) -> void:
    node := box(Node { value: value, prev: .None, next: d.head })
    match d.head:
        .Some(old) => old.prev = weak_mut(&mut *node)
        .None => pass
    d.head = .Some(node)
```

Works: link updates are plain writes, each one check (four per insert
with `tail`). No loan lives past an expression, so release elides every
check on `Node` (elision).

### A8. Interpreter Scopes

```text
struct Env:
    values: Map[string, i64]
    enclosing: (*mut Env)?

struct Interp:
    env: *mut Env

fn slot(env: &Env, name: string) -> &i64:
    &env.values[name]

fn run_side_effect(it: *mut Interp) -> i64:
    it.env.values["x"] = 100    # panics: `current` holds a loan on the env
    1

fn add_assign(it: *mut Interp, env: *mut Env, name: string) -> i64:
    current := slot(env, name)
    rhs := run_side_effect(it)
    *current + rhs
```

Stricter. Today this panics too, at the call that passes `it`, because a
call that takes a `*mut` counts as a write of its whole object. Under the
proposal passing a pointer is no access, so the panic moves to the write
that conflicts. The Lox port's idiom, reading values by copy
(`get(env, name) -> i64?`), takes no loan and runs.

### A9. Replacing A List Behind A Pointer

```text
use std.collections.UniqList

struct Bag:
    items: UniqList[string]

fn replace_then_read(first: &string, alias: *mut Bag) -> string:
    alias.items = UniqList::[string].new()  # panics: first's loan
    *first

fn demo(bag: *mut Bag, alias: *mut Bag) -> string:
    match bag.items.get_ref(0):
        .Some(first) => replace_then_read(first, alias)
        .None => ""
```

Stricter: today `*first` reads freed memory (#1320's repro). The loan
`get_ref` takes through `bag` makes the replacement panic before the old
list is dropped.

### Adversarial Cases

| Fixture (`pointer-loan-extra-…`) | A | A: elision | B: keep-alive | Today |
| --- | --- | --- | --- | --- |
| callee-only-reads | runs | elided | runs | panics |
| callee-writes-other-field-panics | panics | loan needed | runs | as A |
| captured-variable-grows-panics | panics | loan needed | runs, stale | panics `collection-pinned` |
| captured-variable-replaced-panics | panics | loan needed | runs, stale | runs |
| copy-under-element-loan-panics | panics | loan needed | runs, the copy sees the write | runs |
| deep-shared-loans | runs | elided | runs | as A |
| dependent-result-alias-panics | panics | loan needed | runs | as A |
| dyn-receiver-alias-write-panics | panics | loan needed | runs, sees the write | runs |
| elide-callback-panics | panics | loan needed | runs, sees the write | as A |
| elide-closure-captured-pointer-panics | panics | loan needed | runs, sees the write | as A |
| elide-dyn-call-panics | panics | loan needed | runs, sees the write | as A |
| elide-erased-generic-panics | panics | loan needed | runs, sees the write | as A |
| elide-getter-in-loop | runs | elided | runs | as A |
| elide-other-task-panics | panics | loan needed | panics collection-pinned | reads freed memory |
| elide-read-only-visitor | runs | elided | runs | as A |
| elide-read-then-print | runs | elided | runs | as A |
| elide-recursive-call-panics | panics | loan needed | runs, sees the write | as A |
| elide-writes-holder-type-panics | panics | loan needed | runs, stale | as A |
| elide-writes-through-field-reference-panics | panics | loan needed | runs, sees the write | as A |
| generic-argument-alias-write-panics | panics | loan needed | runs, sees the write | runs |
| later-argument-replaces-panics | panics | loan needed | runs, stale | runs |
| observer-notify-writes-owner-panics | panics | loan needed | runs | runs |
| own-path-loop-other-field | runs | loan needed | runs | as A |
| own-path-other-field | runs | loan needed | runs | as A |
| pass-alias-mut-with-shared-panics | panics | loan needed | runs, sees the write | runs |
| pass-alias-read-with-mut-panics | panics | loan needed | runs | runs |
| pass-read-with-shared | runs | elided | runs | as A |
| receiver-argument-reads-the-receiver | runs | loan needed | runs | as A |
| root-pointer-last-use-before-reference | runs | elided | runs | reads freed memory |
| root-pointer-moved-into-a-task | runs | elided | runs | reads freed memory |
| sort-comparator-alias-write-panics | panics | loan needed | runs | runs |
| weak-upgrade-root | runs | elided | runs | as A |

The 14 `typing/invalid/pointer-loan-same-root-*` cases are compile
errors in both models; three of them are not built yet
(`pass-read-with-mut`, `dependent-method`, `dependent-outlives-root`).

### Problems, Ranked

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | PL1 | object granularity panics on a disjoint field | friction, loud | A2, A4, observer, sort comparator, 14 matrix `other`/`elem` rows |
| 2 | PL2 | a write through the loan's own pointer | false panic without own loans | A5, own-path extras |
| 3 | PL3 | element references lose the copy rule | silent if unanswered | copy-under-element-loan, `ref.elem.copy` fixtures |
| 4 | PL4 | captured variables are hidden aliases | silent if unanswered | captured-variable extras |
| 5 | PL5 | roots released at their last use | silent today (#1327) | root-pointer extras |
| 6 | PL6 | elision that a call defeats | silent if wrong | the eight `elide-*-panics` extras |
| 7 | PL7 | model B gives up exclusivity | silent | A8, every `runs, stale` row |

**PL1. Object granularity.** Effect: a write to another field through
another path panics where today it runs, in the cases listed. Candidates:
accept it, as `RefCell` does (questions 1); give each `*mut` field its
own object, which users do by hand today; keep today's address table for
a field-precise slow path, at its cost.

**PL2. Own loans.** Effect: without the discount, a statically valid
write through the same pointer panics, as in A5. Candidates: count own
loans per pointer local (question 2); or make the checker reject such
writes, as Rust would through a `RefMut`.

**PL3. Element copies.** Effect: with no pin, a copy shares the buffer
and copy-on-write detaches the reference. Candidates: the two static
rules of How It Works (question 3); or keep the pin for `List`/`Map` and
pay it on local code too.

**PL4. Captured variables.** Effect: a closure that shares a variable
changes it behind a reference with no `*T` in sight. Candidates: treat
the cell as a `*mut` (question 4); or reject a call of any closure value
while a reference into a captured variable lives, which rejects much
more.

**PL5. Roots.** Effect: a reference with no count dies with its root.
Candidates: keep the root alive to the loan's end (How It Works,
"Roots"); or count the root's object once per loan, which is today's
count by another name.

**PL6. Elision soundness.** Effect: a loan wrongly elided is a freed
read, with no panic. Every way through the rule has a runtime case that
must still panic: a callback, a trait call, a write of the type holding
the crossed pointer, a `&mut` of a containing type's field, a captured
pointer, another task, recursion, and an erased generic. Candidates:
summaries per written type, conservative for function values, trait
calls and suspension (as proposed); or elide only in bodies with no
call at all, which keeps most of the bench's loans.

**PL7. Model B.** Effect: a `&mut` no longer excludes other writers,
so `*current + rhs()` in A8 adds the new value, and a write through an
element reference into a grown list lands in the old buffer and is lost.
Candidates: keep exclusivity (model A); or adopt B and document
`&mut` as non-exclusive behind a pointer, as Go and Java do.

### Parse Log

Every `text` block of this document was checked with
`scripts/hd-conformance.sh parse`, which checks syntax only.

| Block | Result |
| --- | --- |
| 1. The Proposal: `rename`, `same_root`, `greet` | parses; `hd check` reports the intended `conflicting-access` in `same_root` only |
| 2. `*T` where `&T` is expected: `area` | parses; `hd check` also passes on main |
| 3. A1 `visit` | parses; `hd check` also passes on main |
| 4. A2 `count_in` | parses; `hd check` also passes on main |
| 5. A3 `add_child`, `total` | parses; `hd check` also passes on main |
| 6. A4 listeners | parses; `hd check` also passes on main |
| 7. A5 `step` | parses; `hd check` also passes on main |
| 8. A6 `fib` | parses; `hd check` also passes on main |
| 9. A7 `push_front` | parses; `hd check` also passes on main |
| 10. A8 `add_assign` | parses; `hd check` also passes on main |
| 11. A9 `replace_then_read` | parses; `hd check` also passes on main |

No block uses syntax outside the chapters, and none is rejected.

## Appendix B. Prior Art

**Rust `RefCell`.** One borrow flag per cell: a shared count, or a
writing mark. A conflicting borrow panics.

```rust
let cell = RefCell::new(vec![1, 2]);
let first = cell.borrow();      // flag: one shared borrow
cell.borrow_mut().push(3);      // panics: already borrowed
drop(first);
```

hd takes the flag and its object granularity, and infers each borrow from
a reference instead of a `borrow()` call.
[RefCell](https://doc.rust-lang.org/std/cell/struct.RefCell.html).

**Swift dynamic exclusivity.** A long access to a class property (an
`inout` argument, a mutating call) is recorded by address for its
duration; an overlapping access traps.

```swift
class Bag { var items = [1, 2]; var count = 0 }
func grow(_ xs: inout [Int], _ bag: Bag) { bag.items.append(3) }
let bag = Bag()
grow(&bag.items, bag)   // traps: simultaneous accesses to bag.items
bag.count += 1          // an instantaneous access: no record
```

hd takes the split between instantaneous and long accesses. It rejects
Swift's per-address access set, which is today's loan table, for a header
state that costs one load per check.
[SE-0176](https://github.com/apple/swift-evolution/blob/main/proposals/0176-enforce-exclusive-access-to-memory.md).

**Rust borrow checker.** Paths with no shared ownership are checked at
compile time only.

```rust
let mut v = vec![1];
let first = &v[0];
v.push(2);              // error[E0502]: cannot borrow `v` as mutable
println!("{first}");
```

hd keeps this for every path with no `*T`, which is why such paths emit
nothing. [E0502](https://doc.rust-lang.org/error_codes/E0502.html).

**Java fail-fast iterators.** A collection counts its structural changes;
an iterator that sees the count move throws.

```java
for (String s : list) {
    list.add("x");      // the next step throws ConcurrentModificationException
}
```

hd's check is exact and covers every write, not only length changes.
[ConcurrentModificationException](https://docs.oracle.com/javase/8/docs/api/java/util/ConcurrentModificationException.html).

**Go.** No check at all: a slice taken before an `append` may keep the old
array, and writes through it no longer reach the slice.

```go
xs := make([]int, 1, 1)
p := &xs[0]
xs = append(xs, 2)      // a new array: p still points at the old one
*p = 9                  // xs[0] is still 0
```

Go's garbage collector keeps whatever an interior pointer points into
alive, so `p` is stale but never dangling. Model B takes exactly this:
a count keeps the storage alive, and nothing keeps the place exclusive.
The proposal forbids this silent detachment instead.
[Go slices](https://go.dev/blog/slices-intro),
[Go FAQ: stack or heap](https://go.dev/doc/faq#stack_or_heap).

**Java.** No interior references at all: a field is read and written
through its object, which the collector keeps alive while any
reference holds it. A method on data behind a reference is the usual
call; aliasing is unrestricted, and only collections check structural
changes (above).

```java
Shape s = scene.shapes.get(0);
scene.shapes.set(0, new Shape());   // s is the old shape, alive and stale
double a = s.area();
```

Model B is this rule with interior references added: a count keeps the
storage a reference points into alive, as the collector keeps an object.

**Swift `unowned`.** A reference that keeps nothing alive and traps on
use after its object is freed, by design.

```swift
class Node { unowned let parent: Node; init(parent: Node) { self.parent = parent } }
var root: Node? = Node(parent: placeholder)
let child = Node(parent: root!)
root = nil
print(child.parent)     // traps: attempted to read an unowned reference but the object was already deallocated
```

The contrast with model B: Swift chose a crash for a dangling access,
where C keeps the storage alive and returns stale data. hd's `Weak`
upgrade (`ref.weak.upgrade.dead`) refuses instead of trapping.
[Swift: Unowned References](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/automaticreferencecounting/#Unowned-References).

## Appendix C. Alternatives Considered

- **Unique `*mut T`**, Rust's `Box`/`Rc`/`RefCell` split: rejected by the owner, "definitely a no go".
- **Call-scoped references only**, a `&*p` living for one call and spanning ones rejected: rejected by the owner; it still needs call loans, and rejects 306 of the 362 matrix cases.
- **No references through `*T` at all**: impossible, since `p.area()` with `fn area(&self)` needs `&*p`; copying out works only for `Copy` types.
- **Today's three mechanisms, patched**: field-precise, but 27 matrix cases read freed memory today, each new path needs its own fix, and the loan table's check scans every live loan.
- **Field-granular state**, a bit per field group as Swift's per-address access set: fixes object granularity, but a struct with more fields than spare bits grows by a header word.

## Appendix D. Today In hd

The rules under replacement are
[Conflicts Through Aliases](../../spec/lang/13-values-and-references.md#conflicts-through-aliases)
(`ref.excl.alias.*`), the pin rules of
[Call-Scoped Access](../../spec/lang/13-values-and-references.md#call-scoped-access)
(`ref.elem.access.pin`, `ref.elem.pinned*`) and
[Element Reference Loops](../../spec/lang/06-control-flow.md#element-reference-loops)
(`flow.for.ref.pinned`). The runtime is `crates/hd_wasm_linear/DESIGN.md`
§23.4 (header), §23.20 (references, pins, short loans), and
`emit/excl.rs` with `rt:loan_begin`, `rt:loan_count` and `rt:loan_check`.

### D.1 The Matrix On Main

`spec/tools/generate-pointer-loan-matrix.ts` writes 424 cases: reference
kind (`&`, `&mut`) × path × lifetime × alias action (378 runtime cases,
16 of them on a local path that crosses no `*T`), 32 adversarial runtime
cases, and 14 same-root typing cases. Each runtime case reads the
reference after the action and checks that it holds a value some write
stored. Every fixture states both models' verdicts in a `# Verdicts:`
line.
Results at 156cba91c (after #1320), dev build, with the checked build's
count audit for freed blocks; local-path rows are left out:

| Below the pointer | Cases | A panics | A elides | B: runs / stale / pinned | Today: as A / runs / pinned / unsound |
| --- | --- | --- | --- | --- | --- |
| a plain field | 98 | 64 | 34 | 98 / 0 / 0 | 86 / 12 / 0 / 0 |
| data behind a nested `*T` | 48 | 39 | 9 | 34 / 14 / 0 | 38 / 10 / 0 / 0 |
| a `List`/`Map` | 108 | 90 | 18 | 48 / 60 / 0 | 27 / 53 / 28 / 0 |
| a `UniqList`/`UniqMap` | 108 | 90 | 18 | 48 / 0 / 60 | 28 / 28 / 28 / 24 |
| **all** | **362** | **283** | **79** | **228 / 74 / 60** | **179 / 103 / 56 / 24** |

| Lifetime (`*T` paths) | A: loan needed | A: elided |
| --- | --- | --- |
| one expression | 0 | 14 |
| across a statement | 85 | 11 |
| call argument | 33 | 9 |
| returned by a method | 33 | 9 |
| stored in a lifetime struct | 33 | 9 |
| across a loop iteration | 33 | 9 |
| across a bang call | 33 | 9 |
| captured by a closure | 33 | 9 |

- **Elision column.** The rule keeps the loan in each of the 283 rows
  where A panics, and elides it in all 79 where A runs: a read under `&`,
  one-expression references, and a write to the type that holds the root
  pointer (`drop`). All eight cases built to defeat it keep the loan.
- **Today**, *runs* means the alias's access runs where A panics, and
  the reference sees some stored value. *Unsound* means it read freed
  memory: a value no write stored, or a freed block only the checked
  build reports. All 24 are `UniqList`/`UniqMap` replaced or shrunk
  behind a `*T`; three adversarial cases add #1327's two roots and
  another task's replacement.
- `KNOWN_FAILURES.tsv` lists the 200 failures, each citing #1323, or
  #1327 for the two roots; three are same-root typing cases not built
  yet. Default, checked and optimized runs agree on them.

### D.2 Other Findings On Main

- **#1327.** A `UniqList` element reference through a local `*T` reads
  freed memory once the local's last use passes, with no alias; a
  `join!` task's pointer parameter is released before its suspension
  the same way.
- **Mode-dependent verdicts (#1331).** At 6e7976b7d a `join!` case where one
  task wrote a field while another held a loan across `sleep!` panicked
  in the dev build and ran in the checked one, so the matrix uses
  sequential bang calls; `extra-elide-other-task-panics` covers another
  task with a `UniqList`, which fails the same way in every mode.
- **Optimized pipeline (#1331).** At 6e7976b7d, assigning a new
  `UniqList` through a `*mut` in a suspending body failed the copy
  verifier; the focused optimized run does not show it at 156cba91c.

## Appendix E. Details

### E.1 What Each Rule Becomes

| Rule today | Becomes |
| --- | --- |
| `ref.excl.alias.loan` | a pointer loan: any reference crossing a `*T` that outlives its expression, element references included, on each object it crosses |
| `ref.excl.alias.counted-access` | the access check: a write checks the whole state, a read the exclusive bit; passing a pointer is a static access for the call's other arguments only, no access at run time |
| `ref.excl.alias.conflict` | kept as the panic `borrow-conflict`, object-granular |
| `ref.excl.alias.shared-counted-loan` | absorbed: `*T` and `*mut T` crossings loan alike |
| `ref.excl.alias.call-loan` | kept in every build, from the argument's evaluation to the return; a `&mut self` receiver is two-phase |
| `ref.excl.alias.path-pointer` | absorbed: the shared loans on every object before the last, in every build |
| `ref.excl.alias.untracked-path` | a reference that crosses no `*T` and no cell takes no loan |
| `ref.elem.access.pin`, `ref.elem.pinned`, `.pinned.map`, `.pinned.allowed`, `.pinned.end`, `flow.for.ref.pinned` | removed: a length change through an alias is a write through a `*T` |
| `collection-pinned` | removed; those panics are `borrow-conflict` |
| `ref.elem.copy` | replaced: a copy while a `&mut` element reference lives is `conflicting-access` |
| `ref.elem.access.live-length` | narrowed: while a `&` element reference lives, a write to its collection is `conflicting-access` |
| `ref.elem.replace` | removed: replacing the collection while a reference lives is `conflicting-access` (`ref.excl.shared`) |
| `std-collections.uniq-list.pinned` (#1320) | not needed |
| `ref.count.pointer` (#1320) | replaced by the loan and the root rule |

Under model B instead: every `ref.excl.alias.*` rule is removed (no
exclusivity behind a pointer), `ref.count.pointer` and
`std-collections.uniq-list.pinned` stay, the `List`/`Map` pin rules go,
and `ref.elem.copy` holds by copy-on-write. Both models add the two
static rules of How It Works.

The fixtures `element-reference-copy-independent.hd` and
`a-copy-of-a-pinned-collection-is-its-own-list.hd` become reject cases.

### E.2 The Mechanism In Detail

- **Header.** Descriptor bits 16 to 21: bit 21 exclusive, bits 16 to 20
  the shared count. The release glue's slot narrows to bits 0 to 15 (0 to
  14 in a weak-linked module, whose side-entry bit is 15); the
  collector's colors and flags keep bits 22 to 31. Every loan update is a
  read-modify-write that keeps the other bits, as a count update is. A
  static object has the same header. A static cell (§23.20) has only a
  count word, so one that a closure shares gains a descriptor word: 4
  bytes of static data, no heap object.
- **Spill.** A shared begin at count 31 calls `rt:loan_spill`, which adds
  to a side table keyed by address and leaves the count at 31, so the
  inline check still sees a shared loan; the matching end consults the
  table first.
- **Own loans.** The checker counts, per pointer local and program point,
  the loans its references hold, and the emitted check compares the state
  with that constant. Where the count is not one constant (a loan taken
  on one branch), a local counter holds it.
- **Suspension.** A loan held across a bang call stays set; the frame
  keeps the object's address for the end. Abandoning a task runs its
  loan ends, as it runs its drops.
- **Closures.** A closure passed directly to a parameter may hold a
  reference; its loan lasts the call that receives it. A write through a
  `*T` the closure captured is checked like any other, against the
  creator's own loans through the same pointer.
- **Erased generics and trait values.** The state is in the header, so an
  erased body or a vtable method takes loans and checks with no witness.
  A loan is taken where the reference crosses the pointer, usually the
  concrete caller; a `*mut dyn` receiver takes a call loan.
- **Views.** A view object stands for an open reference argument for one
  call, inside the caller's call loan, so it needs none of its own; #1320
  gives it an explicit owner.
- **Cycles.** A borrowed object is reachable from the live root of its
  loan, so the collector never frees it; trace glue reads fields without
  checks.
- **`Weak`.** An upgrade gives an ordinary `*T` local, which roots the
  loans taken through it; a weak entry roots nothing.
- **Elision (release).** A type `T` drops its loans and checks when no
  reference through a `*T` to `T` outlives its expression and no call that
  lends one can reach a `T` through another pointer, or when no write
  through a `*T` reaches a `T`. Erased bodies count against every type
  they are instantiated at.

### E.3 Overhead, Measured

Method: a prototype in the emitter, kept out of this pull request, that
emits each check before every access through a `*T` the backend's loan
pass sees (`loan_access`), a begin and an end at each reference `Ref`
through one, and logs each site with its function. No bit is ever set,
so no check fires; this times the checks. Release builds,
`--engine node`, the default V8 tier, Node 24 on this Mac, 11
interleaved runs, median wall time with Node's 78 to 81 ms start
subtracted; load average 8 to 9. Base is main at 36071e13d.

**Model A, without elision** (an upper bound):

| Program | Base | Loans and write checks | Loans, write and read checks | Sites (write, read, loan) |
| --- | --- | --- | --- | --- |
| micro: 200 M writes through `*mut` | 196 ms | +16% | +59% | 1, 2, 1 |
| micro: 100 M call loans | 632 ms | +7% | +6% | 0, 0, 1 |
| micro: 20 M loops over `&p.items` | 798 ms | 0% | +2% | 1, 2, 0 |
| `json` | 262 ms | +1.5%, +800 B | +1.5%, +1,024 B | 0, 13, 26 |
| `structure_walk` | 154 ms | noise | noise, +90 B | 0, 5, 0 |
| `binary_trees`, call loans by hand | 202 ms | +7%, +53 B | +7% | 6 lends |
| the other 16 programs | | byte-identical | byte-identical | 0 |

**Model A, with per-reference elision.** Each logged site was judged by
the rule against its program: `json`'s 26 loans are references into
`*FieldRecord` and like derivation records, which nothing in the program
writes through any pointer (the prototype logs no write site in `json`
at all); `binary_trees`' six lends reach a `Node` that is never written;
the `&mut self` calls on pointers in `string_build` (2),
`string_builder` (2), `map_count` (3) and `structure_walk` (6) write only
through their own receiver; `dyn_iter`'s one loan per chain reaches
iterators that write no pointer. So no loan stays, no type keeps a
check, and the prototype emits nothing: every program is byte-identical
to base, and needs no timing. The micros elide the same way.

**Model B.** Its cost is a page-table count for each reference through
a `*T` that leaves a short loan, which main already pays, and it drops
the loan table, which no bench program emits (none has a write that
could conflict with a registered loan). So B is byte-identical to main on
bench too. A micro with one spanning reference per iteration and a write
of another type in its range measures the per-reference costs, 50 M
iterations:

| Version | Time | Per reference |
| --- | --- | --- |
| no reference (`c.name.len()`) | 69 ms | 0 |
| main: a count and a loan-table entry | 898 ms | 16.6 ns |
| model B: the count only (the `rt:loan_begin` call removed by hand) | 469 ms | 8.0 ns |
| model A: elided (no write of `Named` in range) | as the first row | 0 |
| model A: a loan kept | estimated from the call-loan micro | about 0.5 ns |

- A check costs 0.16 to 0.21 ns and a loan 0.47 to 0.5 ns, where V8
  predicts the branch. The prototype places a loan's end next to its
  begin, so V8 may fold some of it: the loan cost is a lower bound.
- The prototype does not see a whole `*T` lent as `&T` (a conversion,
  no `Ref`), a loop's loan over `&p.items`, or erased bodies; the
  checker logged the lends (19 in bench code, 26 in std).
  `binary_trees`' lends were added by hand to the Wasm (binaryen
  `wasm-dis` and `wasm-as`), about 30 M of them.
- Not measured: what A saves by removing main's counts on references
  through `*T`, for example `json`'s 26, which point into static records
  where a count returns at once.

### E.4 Model B Implementation Phases

Each about an hour, with its test. The counts are the matrix rows each
phase turns green once phase 1 has rewritten the matrix to B's
verdicts; 244 of its 410 runtime rows already pass under B on main.

| Phase | Change | Where | Test, rows turned green |
| --- | --- | --- | --- |
| 1 | The matrix asserts B: the generator writes each case's B verdict (`runs`, with the stored-value check, or `panic:collection-pinned`); `KNOWN_FAILURES.tsv` is recomputed | `spec/tools/generate-pointer-loan-matrix.ts` | `--check`; focused runs in three modes agree |
| 2 | Same-root static rules: a `*T` argument reads its pointee for the call's other arguments; a method result through a pointer names the pointer local as its origin, and cannot outlive it | `crates/hd_check` (`ref.excl.args`, `ref.dep.call`) | the 3 typing cases not built today |
| 3 | Remove the `ref.excl.alias.*` loans: `emit/excl.rs`, `rt:loan_begin`, `rt:loan_count`, `rt:loan_check`, and the call and path loans of debug builds, so every build agrees (closes #1331's second item) | `crates/hd_wasm_linear` | 102 rows that panic `borrow-conflict` today and run under B |
| 4 | Drop the `List`/`Map` pin's length panic behind a `*T`; keep the owner word's eager copy; a variable a closure shares counts as behind a `*mut` (question 6) | `rt:list_pinned`, `emit/list.rs`, `emit/map.rs`, `hd_tir` insertion rule 6 | 29: 28 `List`/`Map` grow and shrink rows that panic `collection-pinned` today, and `extra-captured-variable-grows` |
| 5 | Unique rooms behind a `*T`: replacing or dropping a pinned `UniqList`/`UniqMap` panics `collection-pinned` before any element moves | `lib/std/collections.hd` (`Drop`, assignment through a place), `std.rt.memory` | 33: the 25 that read freed memory today, and 8 that panic `borrow-conflict` today (after phase 3) |
| 6 | Roots: a pointer local or a frame's pointer parameter lives until the last use of a reference through it (#1327) | `hd_tir` own passes, the frame layout | 2: `extra-root-pointer-*` |
| 7 | Spec: `ref.excl.alias.*` become the non-exclusive aliasing rules (an alias's access runs; a reference reads the storage it counted); `ref.elem.pinned*` and `flow.for.ref.pinned` apply to unique rooms only; `ref.elem.copy` stays; `collection-pinned` names unique rooms; `std-collections.uniq-list.pinned` and `.uniq-map.pinned` add replace and drop | `spec/lang/13-values-and-references.md`, `06-control-flow.md`, `spec/std/collections.md`, README Diagnostics | `bash spec/check.sh`; the `spec-update` skill |
| 8 | Docs and the size and time table | `crates/hd_wasm_linear/DESIGN.md` §23.20, this note removed | bench: byte-identical to main expected |

### E.5 Model A Implementation Phases, If Chosen

Each about an hour, with its test; the matrix rows leave
`KNOWN_FAILURES.tsv` as they pass.

1. Header bits: narrow the glue slot, add the state; a stage case for
   the descriptor. A weak-linked module's side-entry bit moves from 21
   to 15. The narrower glue mask shrinks every program by 1 or 2 bytes.
2. Access checks on writes and reads through `*T`. Alone it turns no
   row green: every failing `other`, `write` and `read` row needs a loan
   set, which phase 3 does.
3. Loans on bound references, with own loans in the checker; the `stmt`
   and `loop` rows, with their `other`, `write` and `read` rows, and
   the `struct` rows on a `List`/`Map` and `extra-copy-under-element-loan`.
   The `UniqList`/`UniqMap` rows wait for phase 8: `get_mut` is inlined,
   so its result no longer shows that it depends on the receiver.
4. Call loans with two-phase receivers; the `arg`, `closure` and
   `extra-later-argument` rows, but not their `UniqList`/`UniqMap` ones
   (phase 8). It needed two fixes first: an impl on a `*T` lowered every
   reference in its signatures to a local one, and the structure
   templates kept a reborrowed `&mut` as a `*mut`, so their walkers now
   hold `&'a mut`. A closure still checks a write through a pointer it
   captured against no loan rather than its creator's own (E.2), which
   phase 9 adds.
5. Returned and stored references; the `ret` and `struct` rows, but not
   the `struct` `UniqList`/`UniqMap` ones (phase 8). The object a
   returned reference points into comes from the page table; an inlined
   getter's result takes its `self`'s loans.
6. Suspension and abandonment; the `bang` rows, but not their
   `UniqList`/`UniqMap` ones (phase 8). A `join!` task that writes an
   object another task holds across a suspension now panics in every
   build (#1331, item 2).
7. Element references: remove the pin's checks and add the static
   rules; the `List`/`Map` rows stay green and the copy fixtures become
   reject cases. The `collection-pinned` length panics of `List` and
   `Map` go (`rt:list_pinned`, the map store's and `remove`'s). A
   `&mut` element reference excludes, as Rust's does, any use of its
   collection or of a place holding it and any other reference into it;
   a `&` one excludes a change of the collection or a `&mut` into it;
   stores to other elements stay valid, and a `for` loop over element
   references is one access for the whole loop. A captured collection is
   no longer exempt from the static check. The pin's bookkeeping stays
   until phase 10: `Pin`, the owner word that keeps `rt:list_unique` from
   copying a buffer whose extra counts are element references, and the
   eager copy of a pinned collection. Two shapes the length panic alone
   had caught take loans now: a reference rooted at a read through a
   `*T`, as a loop over `&mut list` with `list: *mut List[T]` makes,
   crosses that read's dereferences, and a binding a `match` makes from a
   call's result holds the result's loans, as a direct holder does.
8. `UniqList`/`UniqMap` through loans; drop #1320's room count. That
   retires `std-collections.uniq-list.pinned`, `.uniq-list.basic-pinned`,
   `.uniq-map.pinned` and `.uniq-map.basic-pinned` with
   `module.rt.buffer.pinned` and the `collection-pinned` panic category;
   since phase 7 the `uniq-map` pinned fixtures already panic
   `borrow-conflict`, as a match-bound value reference holds the map's
   object. A reference an inlined getter returns holds the getter's
   argument (the call's held rows survive inlining on its `Scope`); a
   call's result of one reference holds the references its by-value
   arguments carry, as `get_mut(1).expect("slot")` does; and an argument
   carried that way lends for the call it is passed to. The room's
   reference counts stay, as plain bookkeeping, until phase 10.
9. Roots and cells; the `extra-root-*` and `extra-captured-*` rows.
   Roots (`ref.box.root`) extend the root's live range rather than
   count: the ownership passes treat each use of a value that may point
   into a `*T` local's object, through a reference or a borrowed read or
   an address read through it, as a use of that local (rule 5 of
   `hd_tir/src/own/frame.rs`), so its drop, or a move of it, follows the
   reference's last use, in a suspending body's frame too. No count is
   needed where the checker cannot see that last use: a local reference
   never leaves the function that holds its root
   (`local-reference-escapes`), and a call's counted result keeps its own
   count.
   The cell part comes before phase 7, so removing the pin leaves no
   reference into a captured variable unchecked: a variable a closure
   shares is an object, its cell, whose descriptor holds the loans of
   the references into it, and each use of it is a checked access
   (`ref.excl.alias.shared-variable`). That turns the
   `extra-captured-*` rows green, and every bench program stays
   byte-identical. A top-level binding a reference reaches and a place
   writes is an object the same way (`ref.excl.alias.global`): its
   static slot follows a header, so a write from another function sees
   the loans of references into it. One no reference reaches stays in a
   Wasm global with no check.
10. Remove the loan table and page-table counts on references, and with
    the element references' counts of their buffers the rest of the pin:
    `Tag::Pin`, the owner word and the eager copy in `coll_copy`; and the
    counts references take of a `Buffer`'s room.
    The rule: a representation view is the only reference that owns
    anything; every other local reference is a plain address that counts
    nothing, and the ownership passes keep its root alive until its last
    use. The new object `&e` makes belongs to the reference that made it:
    a hidden local the passes keep alive, a static cell in a top-level
    binding's initializer, or, for a default argument's `&mut []`, an
    object the caller releases after the call (`Target::Default`'s
    `owned`). A plain reference's count is `Walk::View`: a retain or
    release only when `rt:ref_is_view` says it is a view, which folds to
    nothing in a module that makes no view.
    The passes that keep roots alive: a `*T` coerced to `&T` lends
    (`OwnCx::lends`); a call's result keeps alive the counted references it
    was lent; an element place roots its collection's local; a call that
    takes a `&mut` of a local is taken to store its other arguments'
    references there (sound but coarse; using the callee's signature
    lifetimes or release summaries instead is a later follow-up); a
    closure carries its captured references' origins; a suspending call's
    lent arguments live until its await; and a local reference never
    takes its local's cell (`TAKES`) unless its sub-body returns it.
    A reference to a suspending body's parameter (which lives in no
    memory between suspension points) is still a copy, which that
    reference owns and its `Drop` releases.
    Two counts remain on references. A `&dyn` reference's data address
    still takes a page-table count, as a trait value does. And a
    suspending function's frame owns its reference parameters (E.2): its
    cold constructor takes a page-table count of each one's object, which
    the frame releases at its end or when it is dropped, so a frame kept
    past its call (`map_join!`'s workers, a frame stored in an object)
    still reaches it; only suspending calls that take reference
    parameters pay it. A map probe's reference takes no count of its
    table.
    The pin goes too: no `Pin` statement, no eager copy in `coll_copy`, and
    no owner-word test in `rt:list_unique`, `rt:map_unique` or a push; the
    word stays in the layout as a reserved 0. Since a copy can no longer be
    made eager, insertion makes the base's lists of an element reference's
    path unique again after any index evaluation that ran since the base
    was read (`set(&mut a[0][snapshot()][0], 9)`), as a push does. A
    `Buffer`'s room keeps no count (#1321): `free_room` frees it outright,
    and its 8-byte slot stays only as a reserved header (count 1,
    descriptor 0) that a bytes-to-string transfer takes as the string's,
    zero-copy, as Rust's `String::from_utf8(Vec)` is.
11. Release elision and its type summary; the size and time table.
    Built right after phase 4, in every build: a holder's live range is
    its whole function, and a call loan's runs from its argument to the
    callee's return. With it every bench program is byte-identical to
    the build before phase 4 except json, 128 bytes smaller.
12. Spill past 31 shared loans; `extra-deep-shared-loans`. The side
    table is an open-addressed map keyed by the descriptor's address, in
    a raw block allocated on first use, doubled at three quarters full
    and freed when empty, so only memory limits it
    (`extra-deep-shared-loans-kept`, `-many-objects`: seventy objects
    each over 31 at once).

A closure checks an access through a `*T` it captured against its
creator's own loans through that pointer (E.2). They live in one cell
that the creator and every closure it makes change together: the
creator makes it at its first such closure, each change through that
pointer adds to the cell, and each check reads it. The environment holds
a count of it, and so does a suspending closure's frame, which keeps it
across suspensions and releases it at its end or abandonment; a
suspending creator keeps its count in its frame as well. There is no
gap: a snapshot of the creator's loans taken when the closure is made
could miss a conflict
(`extra-suspending-creator-closure-write-panics`), and a suspending
closure that started with none panicked where a plain one runs
(`extra-suspending-closure-writes-another-field`).

### E.6 Checklist

- **Grammar, typing:** no new syntax; three checker rules (own loans, the
  element copy and write rules, cells as pointers).
- **Diagnostics:** `conflicting-access` for the new static rules; the
  panic `borrow-conflict` names the object's type.
- **Spec:** `ref.excl.alias.*` rewritten, the pin rules removed, the
  element rules narrowed (E.1); the matrix is the conformance suite.
- **Runtime:** header bits (E.2); host ABI unchanged; checked builds
  audit that every loan ends.
- **Rollback:** the state bits are inert at zero, so each phase can be
  reverted alone.
