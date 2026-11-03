# Pointer Loans: `*T` As `Rc<RefCell<T>>`

Proposal, 2026-11-03; domain: references, ownership passes, runtime;
not accepted. Issue #1323, part of #1205, after #1320.

## The Proposal

Every counted object carries a borrow state, as a Rust `RefCell` does:
an exclusive bit and a shared-loan count, in six spare bits of its
descriptor word, so no object grows. A reference whose place crosses a
`*T` and lives past its expression takes a **loan** on each object it
crosses, until its last use. An access through a `*T` takes no loan: a
write checks that the object has no loan but the function's own, and a
read checks that it has no exclusive one. A conflict panics
`borrow-conflict`. A reference that crosses no `*T` emits nothing: no
count, no pin, no check. The page-table counts on references, the
`List`/`Map` pin and the address-range loan table go.

```text
struct Team:
    lead: *mut User
    log: List[string]

struct User:
    name: string
    age: i32

fn rename(t: *mut Team, alias: *mut Team) -> string:
    n := &t.lead.name               # shared loans on the Team and on the User
    t.log.push("renamed")           # runs: a field of t's own loan, proved apart
    alias.lead.age += 1             # panics borrow-conflict: the User has n's loan
    alias.lead = box(User { name: "b", age: +1 })  # would panic too: the Team has one
    *n                              # n's last use: both loans end here

fn local_only(u: &mut User) -> usize:
    n := &u.name                    # no *T on the path: no code at all
    u.age += 1
    n.len()
```

## Open Questions

1. **Object granularity.** A write to another field of a borrowed object
   through an alias panics, where today it runs. Accept, as `RefCell`
   does? Recommended: yes; the fix is a value loop or a boxed field.
2. **Own loans.** May a write through the pointer the loan was taken
   through, to a field the checker proves apart, skip the function's own
   loans? Recommended: yes; without it `for x in &p.items: p.count += 1`
   panics with no alias in sight.
3. **Element references.** Replace the pin with two compile-time rules:
   no copy of a collection while a `&mut` element reference lives, and no
   write to it while a `&` one lives? Recommended: yes.
4. **Captured variables.** Treat a variable a closure shares as behind a
   `*mut` (its cell), so a reference into it takes loans? Recommended:
   yes; it is the one hidden alias the checker cannot see.
5. **Release.** Keep every loan and check in release, with the per-type
   elision of variant 2? Recommended: variant 2.
6. **Shared-count overflow.** Spill past 31 nested shared loans on one
   object to a side table, or panic? Recommended: spill.

## Problems It Solves

**1. Freed memory behind a pointer.** Today a `UniqList` behind a `*T`
can be replaced while an element reference into it lives
(`alias.items = UniqList::new()`, the #1320 finding), and the reference
reads freed memory. With loans, `get_ref` through `p` takes a shared loan
on `p`'s object, and the write `alias.items = ...` checks it and panics.
The matrix has 54 cases on main that read freed memory, 52 through an
alias and 2 through a released root (#1327); none does under the proposal.

**2. Three mechanisms for one question.** Today a reference into `*T`
data takes a page-table count, a `List` element reference pins its
buffer and panics `collection-pinned`, and a field reference registers an
address range that every access scans. Each covers part of the matrix:
of its 292 conflicts, 110 run with no panic today and 29 panic with the
other category. One borrow state covers all of them.

**3. Cost where no pointer is.** Today `&mut xs[i]` on a local list pins
its buffer and counts it, though no alias can exist. Under the proposal
a path with no `*T` is checked at compile time only and emits nothing.

**4. Checks that vanish in release.** Today the call loan and the path
loan (`ref.excl.alias.call-loan`, `.path-pointer`) are not checked in
release. Under the proposal every build checks the same rules; release
only drops what the whole program proves cannot conflict.

## How It Works

**State.** Bits 16 to 21 of the descriptor word (§23.4 of
`crates/hd_wasm_linear/DESIGN.md`): bit 21 exclusive, bits 16 to 20 the
shared count. The release glue's slot keeps bits 0 to 15 (65,535 slots;
the largest bench program, `json`, has 48 table entries). A cell of a
captured variable has the same header, so no object grows.

**Loans.** A reference that crosses `*T` objects `o1 … on` and is bound,
stored, returned, captured or passed takes a shared loan on `o1 … o(n-1)`
and, on `on`, a shared loan for `&` or an exclusive one for `&mut`. A
`&`/`&mut` argument takes its loans from its evaluation until the call
returns; a `&mut self` receiver takes a shared loan first and becomes
exclusive when the call starts (two-phase, so `p.items.push(alias.items.len())`
runs). A loan ends after the last use of every value holding the
reference, placed like a `Drop`: exits, `break`, a task's abandonment.

**Checks.** Taking a shared loan checks the exclusive bit; taking an
exclusive one checks the whole state. A write through a `*T` (an
assignment, a built-in change) checks the whole state, and a read checks
the exclusive bit. Each check compares with the function's **own** loans
through the same pointer local, a constant the checker knows, so a write
the static rules allow never panics on the writer's own reference.

**Roots.** The local or parameter holding the first `*T` of a live loan's
path is released after the loan ends, and a `*T` argument read from a
place the call may write is counted for the call. A suspending body's
frame owns its pointer parameters (#1327 is today's bug of that kind).

**Element references.** With no pin, only a copy can detach a reference
from its slot (copy-on-write). So a copy of a collection while a `&mut`
element reference lives is `conflicting-access`, and so is a write to it
while a `&` one lives. Through a `*T`, an alias's copy is a read, which a
`&mut` loan stops, and an alias's write is stopped by any loan. A
reference derived through a `&T` or `&mut T` parameter needs no count or
pin: a caller that reached the list through a `*T` holds a call loan for
the whole call, and the reference cannot outlive it.

**Emitted code**, per operation (`o` is the object's payload address):

| Operation | Code |
| --- | --- |
| reference with no `*T` on its path | nothing |
| shared loan begin / end | `s = load(o-4)`; `if s & X: panic`; `store(o-4, s + 1<<16)` / `store(o-4, load(o-4) - 1<<16)` |
| exclusive loan begin / end | `s = load(o-4)`; `if s & M != own: panic`; `store(o-4, s \| X)` / clear `X` |
| write `p.f = v` through `*T` | `if load(o-4) & M != own: panic`, then the store |
| read `p.f` through `*T` | `if load(o-4) & X != own: panic`, then the load |

`X` is bit 21, `M` bits 16 to 21. A begin at count 31 calls
`rt:loan_spill`, which keeps the rest in a side table.

**Elision.** Debug and test builds check everything above. Release, over
the whole program, drops the loans and checks on a pointee type `T`
through which no reference outlives its expression (variant 2), and on a
type written through no `*T` at all.

## Cost

| Component | Change |
| --- | --- |
| code removed | the loan table (`emit/excl.rs`, 1,151 lines, and `rt:loan_*`, about 220), the pin (`Pin`, `rt:list_pinned`, owner-word copies), page-table counts on references |
| code added (estimated) | loan begin/end and checks inline, about 600 lines; the own-loan count and three checker rules; the release type summary; `rt:loan_spill` |
| runtime, variant 1, always check (measured) | a check 0.16 to 0.21 ns, a loan about 0.5 ns; `binary_trees` +7% (30 M call loans), `json` +1%, the rest within noise or byte-identical |
| runtime, variant 2, per-type elision (measured) | `json` +1%, the rest byte-identical; `binary_trees` keeps its call loans (+7%) by the reference rule alone, and drops them by the no-write rule (its `Node` is never written) |
| runtime, variant 3, no references (estimated) | 0, after rewriting what it rejects ([C.1](#c1-the-fallback-no-references-through-t)) |
| Wasm size (measured) | variant 1: `json` +1,024 B (+1.5%), `binary_trees` +53 B (+0.6%), `structure_walk` +90 B (+0.4%); variant 2: `json` +988 B, 0 elsewhere |
| compile time (estimated) | one release pass over the program's places, under 1% |
| web compiler (estimated) | under +10 KB gzip, far below the 2 MB cap |

Measurements and their method are in [E.3](#e3-overhead-measured).

Appendix A holds the stress test and its verdict table; Appendix D the
matrix results on main; Appendix E the rule changes and the phases.

## Appendix A. Stress Test

Design review, 2026-11-03; it changes no decision, spec text or
compiler code. Under test: the proposal above, against
[Conflicts Through Aliases](../../spec/lang/13-values-and-references.md#conflicts-through-aliases)
and [Element References](../../spec/lang/13-values-and-references.md#element-references)
as they stand. Each case is a real library's shape, ported to hd and
checked with `hd check` on main; library shapes are approximations.

**Verdicts.** *Works*: same answer, no rewrite. *Friction*: panics
where the original does not, or needs a rewrite. *Stricter*: panics
where today's hd silently gives a wrong or freed value. Run-time cost is
counted in operations per step: a **check** is one load, mask and branch;
a **loan** is a begin and an end, two read-modify-writes of the header;
a **pin** is today's `IsUnique`, `Pin`, and a buffer count and release.

| Case | Library | Proposal | Today | Fallback (no refs through `*T`) | Cost per step, proposal vs today |
| --- | --- | --- | --- | --- | --- |
| A1 depth-first search | petgraph `Dfs` | works | works | rewrite: value loop | 1 loan and 1 write check per node, 1 read check per edge, vs 1 pin per node |
| A2 in-degree count | petgraph `neighbors` | friction: a self-loop panics | runs | rewrite: value loop | 1 loan + 1 write check per edge vs 1 pin |
| A3 tree with parent links | DOM, `rctree` | works | works | rewrite: `*Node` parameters | 1 loan per child visit vs none (a lend) |
| A4 event listeners | Node `EventEmitter` | friction: a listener writing the bus panics | runs | rewrite: value loop | 1 loan per emit vs 1 pin |
| A5 ECS system | bevy, specs | works (needs own loans) | works | rejected: no in-place element update | 2 loans per entity vs none (short loans) |
| A6 memo recursion | `functools.lru_cache` | compile error, as Rust | compile error | same | 0 |
| A7 doubly linked list | Go `container/list` | works | works | works | 4 write checks per insert vs 0 |
| A8 interpreter scopes | Lox `Environment` | stricter: panics | panics, other line | rewrite: copy out | 1 loan per lookup vs a count and a table entry |
| A9 self-replacing list | the #1320 `UniqList` repro | stricter: panics | reads freed memory | rejected at compile time | 1 loan vs 0 (unsound) |

Adversarial cases are the 17 `pointer-loan-extra-*` fixtures and the
matrix itself (Appendix D); their verdicts are in the table at the end of
this appendix.

**Conclusion.** By its rules the proposal panics in every case the
matrix finds unsound today, and it keeps all of today's static
rejections; this is reasoned, since nothing is built. Its cost in behavior is object
granularity: a write to any field of a borrowed object through another
path panics (A2, A4), as `RefCell` does and as today's field-granular
table does not. The fix is always the same, a value loop or a copy out,
and it is the one Rust users apply to `RefCell`.

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
`n.edges`. Fallback: `for next in n.edges` copies the list header.

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
check on `Node` (variant 2).

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

| Fixture (`pointer-loan-extra-…`) | Proposal | Today |
| --- | --- | --- |
| own-path-other-field | runs (own loans) | runs |
| own-path-loop-other-field | runs (own loans) | runs |
| root-pointer-last-use-before-reference | runs (roots) | reads freed memory (#1327) |
| root-pointer-moved-into-a-task | runs (roots) | reads freed memory (#1327) |
| callee-writes-other-field-panics | panics | panics at the call |
| callee-only-reads | runs: passing a pointer is no access | panics at the call |
| later-argument-replaces-panics | panics: the loan starts at evaluation | runs |
| receiver-argument-reads-the-receiver | runs (two-phase receiver) | runs |
| captured-variable-grows-panics | panics (cells) | panics `collection-pinned` |
| captured-variable-replaced-panics | panics (cells) | runs, reading the old list |
| deep-shared-loans | runs: 41 loans spill past 31 | runs |
| dyn-receiver-alias-write-panics | panics | runs |
| generic-argument-alias-write-panics | panics | runs |
| copy-under-element-loan-panics | panics | runs; the pin copies |
| sort-comparator-alias-write-panics | panics | runs |
| observer-notify-writes-owner-panics | panics (object granularity) | runs |
| weak-upgrade-root | runs | runs |

### Problems, Ranked

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | PL1 | object granularity panics on a disjoint field | friction, loud | A2, A4, observer, sort comparator, 14 matrix `other`/`elem` rows |
| 2 | PL2 | a write through the loan's own pointer | false panic without own loans | A5, own-path extras |
| 3 | PL3 | element references lose the copy rule | silent if unanswered | copy-under-element-loan, `ref.elem.copy` fixtures |
| 4 | PL4 | captured variables are hidden aliases | silent if unanswered | captured-variable extras |
| 5 | PL5 | roots released at their last use | silent today (#1327) | root-pointer extras |

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

### Parse Log

Every `text` block of this document, appendix C included, was checked with
`scripts/hd-conformance.sh parse`, which checks syntax only.

| Block | Result |
| --- | --- |
| 1. The Proposal: `rename`, `local_only` | parses; `hd check` also passes on main |
| 2. A1 `visit` | parses; `hd check` also passes on main |
| 3. A2 `count_in` | parses; `hd check` also passes on main |
| 4. A3 `add_child`, `total` | parses; `hd check` also passes on main |
| 5. A4 listeners | parses; `hd check` also passes on main |
| 6. A5 `step` | parses; `hd check` also passes on main |
| 7. A6 `fib` | parses; `hd check` also passes on main |
| 8. A7 `push_front` | parses; `hd check` also passes on main |
| 9. A8 `add_assign` | parses; `hd check` also passes on main |
| 10. A9 `replace_then_read` | parses; `hd check` also passes on main |
| 11. C.1 `add`, `keep_short` | parses; `hd check` also passes on main |

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

This is the silent detachment the element rules of the proposal forbid.
[Go slices](https://go.dev/blog/slices-intro).

## Appendix C. Alternatives Considered

### C.1 The Fallback: No References Through `*T`

A reference into data behind a `*T` would be an error unless it ends
within its expression, so such data is read and written by value. A call
whose receiver or argument reaches through a `*T` is allowed only when
the callee, transitively, makes no access through a `*T` and calls no
function value; else the user moves the value out and back.

```text
struct Bag:
    items: List[string]

fn add(p: *mut Bag, word: string) -> void:
    p.items.push(word)          # allowed: push runs no user code

fn keep_short(p: *mut Bag) -> void:
    let mut items = p.items     # a copy of the header, by value
    items.retain(fn(w: &string) -> bool: w.len() < 8)
    p.items = items
```

It costs no run-time check, and it rejects every long reference:
`for x in &p.items` (A1, A4: a value loop), a lent `*Node` (A3: a `*Node`
parameter), `get_ref` and `get_mut` through a pointer (A5: a `UniqList` of
non-copyable elements then has no in-place update at all), and every
`&self` method on boxed data whose body calls back. In the bench it
rejects `json`'s 27 reference sites in std and `binary_trees`' five lends.
The proposal accepts all of these and pays a load and a branch for each.

### C.2 Today's Three Mechanisms, Fixed

Keep page-table counts, pins and the loan table, and patch each hole (the
#1320 room count, #1327). Field-precise, but 54 matrix cases read freed
memory today, and each new path needs its own mechanism. The loan table's
check is a scan of every live loan.

### C.3 Field-Granular State

A borrow bit per field group, as Swift's access set is per address. It
fixes PL1, but a struct with more fields than spare bits needs a header
word: objects grow by 8 bytes, which the owner's constraint forbids.

### C.4 Counting Instead Of Loans

Keep the reference's count of the object (#1320) and add no exclusivity.
Memory stays valid, but a write through an alias still reaches a live
`&` silently, and copy-on-write still detaches element references.

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

`spec/tools/generate-pointer-loan-matrix.ts` writes 395 runtime cases:
reference kind (`&`, `&mut`) × path × lifetime × alias action, and 17
adversarial cases. Each case reads the reference after the action and
checks that it holds a value some write stored. Results at 6e7976b7d (the same at 36071e13d), dev
build (`hd FILE`), with the checked build's count audit for freed blocks:

| Path | Cases | Pass | Runs, should panic | `collection-pinned` | Unsound | False panic |
| --- | --- | --- | --- | --- | --- | --- |
| local struct field | 16 | 16 | 0 | 0 | 0 | 0 |
| field of a boxed struct | 48 | 43 | 5 | 0 | 0 | 0 |
| field of a struct field behind `*T` | 50 | 43 | 7 | 0 | 0 | 0 |
| behind `*T` then `*T` | 48 | 38 | 10 | 0 | 0 | 0 |
| `List` element behind `*T` | 54 | 13 | 27 | 14 | 0 | 0 |
| `Map` value behind `*T` | 54 | 14 | 26 | 14 | 0 | 0 |
| `UniqList` element behind `*T` | 54 | 14 | 14 | 0 | 26 | 0 |
| `UniqMap` value behind `*T` | 54 | 14 | 14 | 0 | 26 | 0 |
| adversarial | 17 | 6 | 7 | 1 | 2 | 1 |
| **all** | **395** | **201** | **110** | **29** | **54** | **1** |

| Lifetime | Cases | Pass | Runs, should panic | `collection-pinned` | Unsound |
| --- | --- | --- | --- | --- | --- |
| one expression | 16 | 16 | 0 | 0 | 0 |
| across a statement | 98 | 32 | 46 | 8 | 12 |
| call argument | 44 | 24 | 8 | 4 | 8 |
| returned by a method | 44 | 40 | 4 | 0 | 0 |
| stored in a lifetime struct | 44 | 24 | 8 | 4 | 8 |
| across a loop iteration | 44 | 24 | 8 | 4 | 8 |
| across a bang call | 44 | 24 | 8 | 4 | 8 |
| captured by a closure | 44 | 11 | 21 | 4 | 8 |

- **Unsound** means the reference read freed memory: a value no write
  stored (40 cases), or a freed block that only the checked build reports
  (14 `UniqMap` cases). All are `UniqList`/`UniqMap` replaced, grown or
  shrunk behind a `*T`, plus the two #1327 roots.
- **Runs, should panic**: the alias's access runs and the reference sees a
  value some write stored. These are object-granularity rows (`other`,
  `elem`), element writes, `&mut` loans read by an alias, and closures.
- The one **false panic** is `extra-callee-only-reads`: today passing a
  `*mut` to a call counts as a write of the whole object.
- `KNOWN_FAILURES.tsv` lists the 194 failures, each citing #1323, or #1327
  for the two roots. Default, checked and optimized runs agree on them.

### D.2 Other Findings On Main

- **#1327.** A `UniqList` element reference through a local `*T` reads
  freed memory once the local's last use passes, with no alias at all;
  and a `join!` task's pointer parameter is released before its
  suspension the same way.
- **Optimized pipeline.** Assigning a new `UniqList` or `UniqMap` through
  a `*mut` in a suspending body fails the copy verifier
  (`unsupported:Collect`, four `bang-*-replace` cases).
- **Mode-dependent verdicts.** With `join!`, a write by one task while the
  other holds a loan across `sleep!` panicked in the dev build and ran in
  the checked one, so the matrix holds that case only as a sequential
  bang call.

## Appendix E. Details

### E.1 What Each Rule Becomes

| Rule today | Becomes |
| --- | --- |
| `ref.excl.alias.loan` | a pointer loan: any reference crossing a `*T` that outlives its expression, element references included, on each object it crosses |
| `ref.excl.alias.counted-access` | the access check: a write checks the whole state, a read the exclusive bit; passing a pointer to a call is no access |
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
| `std-collections.uniq-list.pinned` (#1320 branch) | not needed |
| `ref.count.pointer` (#1320 branch) | replaced by the loan and the root rule |

The fixtures `element-reference-copy-independent.hd` and
`a-copy-of-a-pinned-collection-is-its-own-list.hd` become reject cases.

### E.2 The Mechanism In Detail

- **Header.** Descriptor bits 16 to 21: bit 21 exclusive, bits 16 to 20
  the shared count. The release glue's slot narrows to bits 0 to 15; the
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
pass sees (`loan_access`), and a begin and an end at each reference
`Ref` through one. No bit is ever set, so no check fires; this times the
checks. Release builds, `--engine node`, Node 24 on this Mac, 11
interleaved runs, median wall time with Node's 81 ms start subtracted;
load average about 8.

| Program | Base | Variant 1 (writes, loans) | Variant 1 (with reads) | Variant 2 | Sites (write, read, loan) |
| --- | --- | --- | --- | --- | --- |
| micro: 200 M writes through `*mut` | 196 ms | +16% | +59% | +62% (same code) | 1, 2, 1 |
| micro: 100 M call loans | 632 ms | +7% | +6% | +6% | 0, 0, 1 |
| micro: 20 M loops over `&p.items` | 798 ms | 0% | +2% | 0% | 1, 2, 0 |
| `json` | 262 ms | +1.5% | +1.5% | +1.1% | 0, 13, 27 |
| `structure_walk` | 154 ms | noise | noise | noise | 0, 5, 0 |
| `binary_trees`, call loans by hand | 202 ms | +7% | +7% | 0 (never written) | 5 lends |
| the other 17 programs | | byte-identical | byte-identical | byte-identical | 0 |

- A check costs 0.16 to 0.21 ns and a loan 0.47 to 0.5 ns, where V8 predicts the
  branch. The prototype places a loan's end next to its begin, so V8 may
  fold some of it: the loan cost is a lower bound.
- The prototype does not see a whole `*T` lent as `&T` (a conversion, no
  `Ref`), a loop's loan over `&p.items`, or erased bodies. `binary_trees`'
  lends were added by hand to the Wasm (binaryen `wasm-dis` and
  `wasm-as`): a shared begin and end on the node at `check`'s entry and
  exit, about 30 M of them. `dyn_iter` takes one loan per chain, 100 K in
  all, estimated under 0.1 ms.
- Variant 2 used the reference rule only, from the first build's sites;
  the no-write rule is estimated.
- Variant 3 costs no check; its cost is the rewrites of C.1.
- Not measured: what removing today's page-table counts and pins saves.
  It is a saving, so the variant 1 numbers are an upper bound against
  main.

### E.4 Implementation Phases

Each about an hour, with its test; the matrix rows leave
`KNOWN_FAILURES.tsv` as they pass.

1. Header bits: narrow the glue slot, add the state; a stage case for
   the descriptor.
2. Access checks on writes and reads through `*T`; the `other`, `write`
   and `read` rows of the matrix.
3. Loans on bound references, with own loans in the checker; the `stmt`
   and `loop` rows.
4. Call loans with two-phase receivers; the `arg`, `closure` and
   `extra-later-argument` rows.
5. Returned and stored references; the `ret` and `struct` rows.
6. Suspension and abandonment; the `bang` rows.
7. Element references: remove the pin, add the two static rules; the
   `List`/`Map` rows and the copy fixtures.
8. `UniqList`/`UniqMap` through loans; drop #1320's room count.
9. Roots and cells; the `extra-root-*` and `extra-captured-*` rows.
10. Remove the loan table and page-table counts on references.
11. Release elision and its type summary; the size and time table.
12. Spill past 31 shared loans; `extra-deep-shared-loans`.

### E.5 Checklist

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
