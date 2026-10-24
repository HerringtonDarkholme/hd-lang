# Local References: The First Cut Of Tier 2

Status: design proposal, 2026-10-24; domain: language feature (spec/lang
chapter 13, the checker, the ownership passes, the linear emitter); not
accepted. Nothing here is accepted behavior. It investigates the first
cut of local references for #834, follows the owner's decisions recorded
on #704 and #834 up to 2026-10-24 23:24, and will fold into
[Object Model, Tier 2](OBJECT_MODEL_TIER2.md). It cites the current
rules in [Values And References](../../spec/lang/13-values-and-references.md)
and changes no spec, std or compiler text.

## Contents

1. [The First Cut And The Owner's Decisions](#1-the-first-cut-and-the-owners-decisions)
2. [Use Cases And What They Cost Today](#2-use-cases-and-what-they-cost-today)
3. [Prior Art](#3-prior-art)
4. [Semantics And Edge Cases](#4-semantics-and-edge-cases)
5. [The Rules](#5-the-rules)
6. [Soundness Without An Exclusivity Check](#6-soundness-without-an-exclusivity-check)
7. [Architecture](#7-architecture)
8. [Cost](#8-cost)
9. [Stress Test](#9-stress-test)
10. [Prototype And Complexity](#10-prototype-and-complexity)
11. [What Tier 2 Keeps, Drops And Changes](#11-what-tier-2-keeps-drops-and-changes)
12. [When Local References Get In The Way](#12-when-local-references-get-in-the-way)
13. [Implementation Plan](#13-implementation-plan)
14. [Recommendation](#14-recommendation)
15. [Open Questions For The Owner](#15-open-questions-for-the-owner)
16. [Sources And Parse Log](#16-sources-and-parse-log)

## 1. The First Cut And The Owner's Decisions

The first cut under review (#704, 2026-10-24 21:27), numbered so the
findings can name what they changed:

| Rule | First-cut text |
| --- | --- |
| R1 | `&{} T` and `&{} mut T` are an opt-in on parameters and receivers. They are uncounted, and the callee can't store, capture or return them. |
| R2 | Interior references (`&x.f`, `&xs[i]`) and borrows through scoped parameters are scoped, may be bound with `let`, and live to their last use. |
| R3 | While a scoped reference is live, nothing else writes the borrowed root: one static check per body. |
| R4 | Passing a scoped reference where `&T` is expected is an error. |

The first draft of this document stress-tested those rules and a
prototype. The owner then decided, in order:

1. R3 is cut: no exclusivity check and no "no other writer" analysis.
   The cut must be sound without it.
2. Local references across awaited `!` calls are required.
3. Counting local references is fine, with rules: section 6's
   construction (#834, 23:24).
4. The spelling (#834, 23:09, replacing `&{}` and `&{global}`): "take
   addr always &t, deref always *t; gc ref is *T, and local ref is &".
   `*T` and `*mut T` are counted references, always to a whole object;
   `&T` and `&mut T` are local. A method that keeps `self` takes `*self`.
5. Element borrows of a `List` and of a `Map` keep today's pin plus one
   fast-path count of the buffer, and copy-on-write still triggers.
6. `&T` is never a type argument: no `List[&T]`, no `Option[&T]`, no
   `T = &Foo`, as with Swift's `inout`. This replaced an earlier
   decision that allowed scoped type arguments.
7. std uses local references on every parameter; the parameters that
   keep their argument, about 17, are `*mut T`.

The owner also answered the criticism of the first draft: `&xs[i]` and
`&x.f` are local only, so counted references never need `block_of`; the
learning cost, a per-parameter escape choice, and the new `*T` spelling
are accepted; and the first cut is specifically for non-escapable
parameters.

Everything below was checked against main at `0c1b055d2` and against a
throwaway prototype on branch `lane/834-proto`, built on the same commit
before the spelling decision (section 10). In the final spelling, `&T`
parameters parse today, with today's counted meaning; lines that write
`*T` carry `# hypothetical syntax`. Section 16 records how each block
was checked.

## 2. Use Cases And What They Cost Today

### 2.1 What today costs

Today every reference is counted
([`ref.place.lifetime`](../../spec/lang/13-values-and-references.md#r-ref.place.lifetime)).
A reference parameter is lent: neither the caller nor the callee counts
an argument that is already a reference
([docs/compiler/tir-v2.md](../compiler/tir-v2.md), "Calls and pass
modes"). Four things still cost:

| Cost | When | Measured unit |
| --- | --- | --- |
| a count pair on the new reference | `f(&local)`, `f(&mut local.field)`, `f(&mut xs[i])`, `local.method()` for a `&self` method: each makes a reference the callee might keep | a general `retain`/`release` through the page table (`block_of`), since the address may be inside a cell or a buffer |
| a counted cell for the local | any local some reference refers to (`closure::cell_locals`, linear DESIGN §23.20) | one allocation per execution of the declaration |
| a pin and a buffer count | each element reference (`ref.elem.access.pin`) | an `IsUnique`, a `Pin` and one buffer count pair |
| a new object | `f(&temporary)` | one allocation (`ref.expr.allocate`) |

Lent parameters passed on cost nothing:
`crates/hd_driver/tests/cases/counts/borrowed-argument-call.hd` records
no count operations for `depth(left)`. One gap costs a pair even there:
passing a `&mut P` parameter where `&P` is expected inserts a `Weaken`
conversion, which copies its operand with a count. The prototype's
`scoped-834/w-weaken.hd` records two retains for `read(p) + read(p)`;
this borrow-pass gap is independent of local references.

### 2.2 The cases

Each case was written as a full program, checked and run with the
prototype. The full sources are fixtures in
`crates/hd_driver/tests/cases/counts/scoped-834/` on the prototype
branch, in a counted variant (`-g`, today's code) and a local variant
(`-s`). The counts columns are the count operations the `counts` view
lists for the whole file: a static count of sites, not of executions.
The excerpts below are shortened and written in the final spelling.

| Case | Program | Today (`-g`) | Local (`-s`) | What goes |
| --- | --- | ---: | ---: | --- |
| U1 | a tokenizer walking a string | 24 | 22 | the pair on `lexer.next()` |
| U2 | an in-place insertion sort with `swap` | 11 | 6 | the pairs on `swap(&mut xs[j - 1], &mut xs[j])` in the local sort, and on `&mut xs` |
| U3 | mutating list elements in a loop | 12 | 9 | the pair per `ps[i].step(dt)` and on `&mut ps` |
| U4 | a builder stored in a field | does not compile local | — | see F5 |
| U5 | BFS over adjacency lists; updating a node in a tree | 12 | 12 | nothing: elements behind a parameter keep their pins |
| U6 | a grid update | 5 | 3 | the pairs on `&mut g` |
| U7 | JSON DOM traversal and editing | 36 | 30 | the pairs on `&mut child`, `&item` |
| U8 | std collection methods | 11 | 8 | the pairs on `inv.total()` and friends |
| U9 | an accumulator passed down a recursive walk | 6 | 5 | the pair on `&mut stats` |
| U10 | string building | 10 | 5 | the pairs on `html.open(..)` |

No case loses its cells: a local that some reference refers to still
lives in a counted cell, because #702 is deferred. No case loses an
allocation. Under rule L4 (section 5), element borrows behind a
parameter (U2's in-place variant, U5) keep their pin and take a
fast-path pair instead of today's general pair; the prototype counts
them as today.

**U1, a tokenizer.** Every method borrows the lexer for one call, so
every receiver is local.

```text
struct Lexer:
    src: string
    pos: usize

impl Lexer:
    fn at_end(&self) -> bool:
        self.pos >= self.src.len()

    fn skip_space(&mut self) -> void:
        while !self.at_end() && self.src[self.pos] == 32u8:
            self.pos += 1

fn words(text: string) -> usize:
    let mut lexer = Lexer { src: text, pos: 0 }
    let n: usize = 0
    while !lexer.at_end():
        lexer.skip_space()
        n += 1
    n
```

Today `lexer.skip_space()` takes a counted reference to `lexer`'s cell
per call, and `lexer` lives in a heap cell. Local, the pair goes and the
cell stays. Inside a method, each `self.at_end()` still costs a pair in
both variants, because a `&mut` receiver weakened to `&` copies (the gap
of 2.1). Token texts are `slice` copies of `src` in both; views that
borrow the source need tier 2's lifetime parameters
([Tier 2 §11.1](OBJECT_MODEL_TIER2.md#111-a-tokenizer-whose-tokens-borrow-the-source)).

**U2, sort and swap.**

```text
fn swap(a: &mut i64, b: &mut i64) -> void:
    t := *a
    *a = *b
    *b = t

fn sort_local(input: List[i64]) -> List[i64]:
    let mut xs = input
    for i in 1..xs.len():
        let j = i
        while j > 0 && xs[j - 1] > xs[j]:
            swap(&mut xs[j - 1], &mut xs[j])
            j -= 1
    xs
```

Today each `swap` call costs an `IsUnique`, a `Pin` and a buffer count
pair per element reference: four count operations. With `xs` a local
that nothing else reaches and that no other argument names, the local
variant takes no count. Inside `fn insertion_sort(xs: &mut List[i64])`
the list is behind a reference that an alias may change, so each
element borrow keeps its pin and takes one fast-path count (L4). The
idiomatic hd sort writes `xs[j] = xs[j - 1]` and takes no reference at
all, as std's `sorted_by` does.

**U3, elements in a loop.** `for p in &mut ps: p.step(dt)` and
`ps[i].step(dt)` both pass element references. On a local list,
`ps[i].step(dt)` drops its buffer count; behind a parameter it keeps
the pin and a fast-path count. The prototype does not make a `for`
loop's binding local, so `for p in &mut ps` keeps its per-step count
there.

```text
struct Particle:
    x: f64
    v: f64

impl Particle:
    fn step(&mut self, dt: f64) -> void:
        self.x += self.v * dt

fn advance(ps: &mut List[Particle], dt: f64) -> void:
    for p in &mut ps:
        p.step(dt)

fn advance_local(input: List[Particle], dt: f64) -> List[Particle]:
    let mut ps = input
    for p in &mut ps:
        p.x += p.v * dt
    for i in 0..ps.len():
        ps[i].step(dt)
    ps
```

**U4, a builder in a field.**

```text
struct Request:
    url: string
    headers: List[(string, string)]

impl Request:
    fn header(*mut self, name: string, value: string) -> *mut Request:  # hypothetical syntax
        self.headers.push((name, value))
        self

struct Client:
    request: Request

fn configure(client: &mut Client, token: string) -> void:
    _ := client.request.header("accept", "json").header("auth", token)
```

This compiles today. In the final spelling, `header` returns `self`, so
it keeps its receiver and takes `*mut self`. `client.request` is an
interior place, which is only ever a local reference, so
`client.request.header(..)` is an error (finding F5). std's builders
(`StringBuilder`, `Command`, `Request`) are `*mut` handles to their own
objects, so a field holding one is a counted reference and is
unaffected.

**U5, graphs and trees.**

```text
fn bfs(adj: &List[List[usize]], start: usize) -> List[usize]:
    let mut dist: List[usize] = [for _ in adj => 1_000_000]
    let mut queue: List[usize] = [start]
    dist[start] = 0
    let head: usize = 0
    while head < queue.len():
        u := queue[head]
        head += 1
        for v in adj[u]:
            if dist[v] == 1_000_000:
                dist[v] = dist[u] + 1
                queue.push(v)
    dist

struct Dir:
    name: string
    size: i64
    children: List[Dir]

fn add_size(node: &mut Dir, path: List[string], at: usize, delta: i64) -> void:
    node.size += delta
    if at < path.len():
        for child in &mut node.children:
            if child.name == path[at]:
                add_size(child, path, at + 1, delta)
```

`adj[u]` copies the inner list header, a count pair, in both models. The
tree update descends through element references behind a local
parameter, so each step keeps its pin, now with a fast-path count: the
prototype's counts are equal (12 against 12).

**U6, a grid.** `relax(g: &mut Grid)` reads and writes `g.cells[r][c]`
by index and takes no reference. The calls `warm_edges(&mut g)` and
`relax(&mut g)` lose their pairs; `heat(&mut g.cells[0][c], 1.0)` is an
element borrow behind `g` and keeps its pin.

```text
struct Grid:
    w: usize
    h: usize
    cells: List[List[f64]]

fn relax(g: &mut Grid) -> void:
    for r in 1..g.h - 1:
        for c in 1..g.w - 1:
            avg := (g.cells[r - 1][c] + g.cells[r + 1][c] + g.cells[r][c - 1] + g.cells[r][c + 1]) / 4.0
            g.cells[r][c] = avg

fn heat(cell: &mut f64, by: f64) -> void:
    *cell += by

fn warm_edges(g: &mut Grid) -> void:
    for c in 0..g.w:
        heat(&mut g.cells[0][c], 1.0)
```

**U7, JSON DOM traversal and editing.**

```text
use std.json.{Json, JsonObject}

fn set_path(doc: &mut Json, path: List[string], at: usize, value: Json) -> void:
    if at == path.len():
        *doc = value
        return
    let fields = match *doc:
        .Object(fields) => fields
        _ => JsonObject::new()
    *doc = Json.Null
    let mut f = fields
    let mut child = f.get(path[at]).unwrap_or(Json.Null)
    set_path(&mut child, path, at + 1, value)
    f.set(path[at], child)
    *doc = Json.Object(f)
```

A payload is no place
([`ref.match.copy`](../../spec/lang/13-values-and-references.md#r-ref.match.copy)),
so editing inside an object copies the object out, edits it and writes
it back. `*doc = Json.Null` drops the document's hold on the entries
before `f.set`, so the set finds the buffer unique and copies nothing.
Local references change none of that; they only remove the pairs on
`&mut child` and on `count_strings(&item)`. `&fields.get(key)...`, a
temporary, still allocates a new object for its reference.

**U8, std collection methods.** std's `List` and `Map` methods are value
methods: `get(self, index) -> T?`, `iter(self)`, `filter(self, ..)`,
`sorted_by(self, ..)` ([spec/std/collections.md](../../spec/std/collections.md)).
They take no reference, so local references change nothing in them. The
exceptions are `view`, `chunks` and `windows`, which keep `self` inside
the `ListView` they return and so take `*self`. A call on a field is
then an error:

```text
use std.collections.ListView

struct Inventory:
    items: List[i64]

impl Inventory:
    fn head(&self) -> ListView[i64]:
        self.items.view(0, 2)
```

The prototype reports `scoped-reference-escapes` at `self.items`.

**U9, an accumulator down a recursive walk.**

```text
struct Node:
    name: string
    children: List[*Node]  # hypothetical syntax

struct Stats:
    count: usize
    names: List[string]

fn walk(n: &Node, acc: &mut Stats) -> void:
    acc.count += 1
    acc.names.push(n.name)
    for c in n.children:
        walk(c, acc)
```

Today the only costs are at the top: `stats` lives in a cell and
`walk(root, &mut stats)` takes one pair. The recursive calls pass a lent
parameter on and cost nothing, in both models. A call of a `&mut self`
method on `acc`, such as `acc.note(n.name, depth)`, is valid: the
receiver is local. The prototype, which needed `&{} mut self` written,
rejects it.

**U10, string building.** `self.out.push(..)` on an
`out: *mut StringBuilder` field reads a counted reference out of the
field and calls `push` on it, so it compiles in both models. Each
`html.open("li")` loses its pair.

```text
use std.text.StringBuilder

struct Html:
    out: *mut StringBuilder  # hypothetical syntax
    depth: usize

impl Html:
    fn open(&mut self, tag: string) -> void:
        _ := self.out.push("<").push(tag).push(">")
        self.depth += 1

    fn text(&mut self, s: string) -> void:
        _ := self.out.push(s)

fn render(items: List[string]) -> string:
    let mut html = Html { out: StringBuilder::new(), depth: 0 }
    html.open("ul")
    for item in items:
        html.open("li")
        html.text(item)
    html.out.build()
```

### 2.3 What the cases show

Local references remove the count pair at the call that creates a
reference to a local, a field of a local, a field of `self`, or an
element of an unaliased local list. Element borrows behind a reference
keep their pin and take a cheaper pair. Local references do not remove
the cell, the new object for a temporary, or the pair from weakening.
The wins are real per call (section 8) and small per program, unless
#702's frame storage comes with them.

## 3. Prior Art

### 3.1 Swift: `inout`, `borrowing`, `consuming`, exclusivity

**Design.** An `inout` parameter is a mutable access to a variable for
the duration of the call, and a `mutating` method takes `self` the same
way. Neither can escape: a closure that escapes cannot capture an
`inout` parameter. SE-0377 added `borrowing` and `consuming` for
parameters of noncopyable and copyable types. SE-0176's Law of
Exclusivity says that two accesses to one variable may not overlap
unless both are reads.

**Enforcement.** Locals, `inout` parameters and struct properties are
checked statically. Class properties, globals and escaping captures are
checked dynamically, by a runtime record of in-progress accesses
(SE-0176). Accesses to different stored properties of a struct may
overlap; elements of an array may not, so `swap(&a[0], &a[1])` is
rejected in favour of `swapAt`. An `inout` access begins after the
other operands are evaluated, so `x += x` is fine.

**Coverage.** Every use case of section 2 except U4's chaining and U7's
in-place payload edits. A `mutating` method on a struct field works,
because `self` of a value type is always `inout`.

**Limitations.** The dynamic checks are the price for class properties.
Without them, a mutable access into an array element reached through a
class could see the array reallocated by an alias. Swift accepts the
runtime cost there; hd's first cut has no such mechanism (F2).

### 3.2 Rust: NLL, two-phase borrows, reborrows

**Design.** A reference carries a lifetime; `&mut` is exclusive. The
non-lexical lifetimes checker (RFC 2094) ends a borrow at its last use,
computed by liveness over the control-flow graph, so loops are handled
by the back edges. Two-phase borrows (RFC 2025) let
`vec.push(vec.len())` compile: the receiver's `&mut` is reserved
first and activated at the call. Passing a `&mut` parameter on reborrows
it for the inner call.

**Coverage.** Everything except shared mutation: two `&mut` to one
place are never live together, so `swap(&mut v[i], &mut v[j])` needs
`v.swap(i, j)` or `split_at_mut`. Disjoint fields work inside a body but
not across a method call that borrows all of `self`.

**Limitations.** NLL problem case 3 (a conditional return of a borrow)
is still rejected. The checker is large: hd's own estimate is 10k to
15k lines (the Rust model study, quoted in
[Tier 2 §13](OBJECT_MODEL_TIER2.md#13-costs)).

### 3.3 Hylo: mutable value semantics and projections

**Design.** Hylo has no first-class references. A parameter is passed
`let` (immutable, notionally by value, really by address), `inout`,
`sink` (ownership moves in) or `set` (initialize). `let` and `inout`
values "cannot escape the callee without first being copied". A
receiver defaults to `let`, and a method may have `let`, `inout` and
`sink` variants. A subscript yields a projection of part of a value to
the caller for the rest of the expression, instead of returning a
reference. Exclusivity is the default: an `inout` argument must be
unique among the arguments.

**Coverage.** All of section 2, U7 included: a subscript can project a
mutable member of an object, so editing in place needs no returned
reference. U4's chaining is a subscript or a `sink` builder.

**What it gave up.** Shared mutable state needs explicit unsafe pointers
or library types. There is no Go-style counted `*mut T` to keep in a
field, so a graph with shared nodes is an arena with indices.

**As a model for hd.** Hylo is the simplest model that covers the cases:
every parameter and receiver is second-class. hd cannot adopt it whole,
because hd keeps Go's counted references as its storable sharing
mechanism. The final spelling takes Hylo's default anyway: `&T` is the
second-class kind, and the storable one gets its own spelling, `*T`.
Three of Hylo's ideas fit the first cut:

- a receiver is local, which is what makes methods on fields work;
- a projection that lives to the end of the calling expression covers
  the builder exception, which the owner's 20:52 comment on #704 called
  "a returned chaining reference usable within the expression" (F5);
- read-only parameters are already values in hd (`p: Point`); Hylo
  shows they can be passed by address when nothing writes the argument
  during the call, which #702's interprocedural plan already proposes
  for release builds.

### 3.4 Mojo: argument conventions and origins

**Design.** A parameter is `read` (the default, an immutable
reference), `mut` (a mutable reference), `var` (owned) or `ref`
(mutability follows the argument). A returned `ref` must name its
origin, as `ref [self]`, and the compiler tracks origins at compile
time. Mojo enforces argument exclusivity for mutable references: passing
one string as both `mut` and read is a compile error.

**Coverage.** Rust's, with origins inferred more often and returned
references allowed through origin specifiers.

**What it gave up.** Shared mutable references without unsafe types;
wildcard origins switch the exclusivity check off.

### 3.5 Vale: region borrowing

**Design.** Non-owning references are generational: each dereference
checks a generation number. A `pure` function freezes all memory that
existed before the call, so references into that region skip the
checks. Region markers name regions for functions that are not pure.

**Coverage.** Read-only traversals become free; mutation goes through
isolates and runtime checks. The author notes that part 1 was
prototyped and the rest is a preview.

**Lesson.** Freezing a whole region for a call is a cheap static rule,
but only for reads. The owner's cut R3 was a narrow version of it.

### 3.6 Go and C++, for contrast

Go has one pointer kind, counted by the garbage collector, interior
pointers included; escape analysis decides stack or heap and is not
visible in signatures. `append` may move a slice's array, and an old
pointer silently stops reaching the slice: the problem hd's
element-reference rule prevents
([`ref.elem` Why](../../spec/lang/13-values-and-references.md#element-references)).
C++ references are unchecked: `push_back` invalidates references into a
`vector`, and a `string_view` may dangle, with undefined behavior.
Neither restricts aliasing, and neither proves anything.

### 3.7 What each covers

| Use case | Swift | Rust | Hylo | Go | hd first cut |
| --- | --- | --- | --- | --- | --- |
| tokenizer (U1) | yes | yes | yes | yes | yes |
| swap two elements (U2) | `swapAt` | `v.swap(i, j)` | a method | yes | yes |
| method on a field (U3, U9) | yes | yes | yes | yes | yes: `&self` is local |
| builder on a field (U4) | yes | yes | a `sink` method | yes | needs the exception (F5) |
| element behind a shared owner | dynamic check | `RefCell` | no shared owner | unchecked | today's pin and a fast-path count (R3) |
| edit inside an enum payload (U7) | copy out, write back | in place | in place, by a projection | — | copy out, write back |
| callback that writes a captured `inout` | yes, if non-escaping | yes | yes | yes | no (F6) |
| a reference as a type argument | no (`inout` is no type) | yes | no | yes | no: `&T` is never a type argument |

Mojo covers the same cases as Rust and is left out of the table; Go has
no enums with payloads.

## 4. Semantics And Edge Cases

Each case gives the answer under the rules of section 5, an hd example
in the final spelling, and what the prototype reported. "Today" is
main's behavior, where every reference is counted.

### 4.1 Two-phase borrows

```text
struct Stack:
    items: List[i64]

impl Stack:
    fn push(&mut self, x: i64) -> void:
        self.items.push(x)

    fn size(&self) -> i64:
        i64(self.items.len())

fn demo() -> i64:
    let mut s = Stack { items: [] }
    s.push(s.size())
    let mut v: List[i64] = [1]
    v.push(i64(v.len()))
    s.size()
```

**Answer: valid, and no two-phase rule is needed.** Under R3 as written,
the receiver borrow of `s` would start before `s.size()` and conflict
with it; Rust needs RFC 2025 for exactly this. With R3 cut, nothing
checks the overlap, and L4 needs none: the receiver borrows a local's
storage. `cs[0].add(cs.len())` is valid too; `cs[0].add(cs.pop())`
stays today's `conflicting-access`
([`ref.elem.conflict.method`](../../spec/lang/13-values-and-references.md#r-ref.elem.conflict.method)).
Prototype: checks.

### 4.2 Disjoint fields and disjoint elements

```text
struct V:
    x: i64
    y: i64

fn both(a: &mut i64, b: &mut i64) -> void:
    *a += 1
    *b += 1

fn demo(i: usize, j: usize) -> i64:
    let mut v = V { x: 1, y: 2 }
    both(&mut v.x, &mut v.y)
    both(&mut v.x, &mut v.x)
    let mut xs: List[i64] = [1, 2, 3]
    both(&mut xs[i], &mut xs[j])
    v.x + xs[0]
```

**Answer: all valid, as today.** hd's `&mut` is not exclusive
([`ref.write.no-exclusivity`](../../spec/lang/13-values-and-references.md#r-ref.write.no-exclusivity)),
and two references to one slot cannot dangle. R3 as written would have
rejected these calls; with it cut, all are valid. Here hd is more
permissive than Swift and Rust, which reject
`both(&mut xs[i], &mut xs[j])`. Prototype: checks.

### 4.3 Reborrowing a local parameter

```text
struct P:
    n: i64

fn inner(p: &mut P) -> void:
    p.n += 1

fn global_read(p: *P) -> i64:  # hypothetical syntax
    p.n

fn outer(p: &mut P) -> i64:
    inner(p)
    q := p
    inner(q)
    global_read(p)
```

**Answer:** passing `p` to another local parameter is valid and costs
nothing, and `q := p` makes `q` local too. `global_read(p)` is the
error: a local reference never becomes a counted one (L3). Prototype:
`scoped-reference-escapes: in argument: expected &P, found &{} mut P`.
The fix-its are, in order: make the callee's parameter `&P` when it
keeps nothing; pass a copy, `*p`; or make the caller hold a `*P`.

### 4.4 Across `!` calls, `defer`, `using` and `?`

**Suspension frames.** A bang function may hold a local parameter across
a suspension. Its frame is a counted object owned by the task, and the
caller is itself suspended inside the call, so the referent outlives
the use: a bang call completes or is cancelled before its caller goes
on. This holds for the cell form and for #702's frame slots, since a
suspending body's locals live in its frame
([linear DESIGN §23.21](../../crates/hd_wasm_linear/DESIGN.md)). A
fast-path count that L4 takes for a borrow is held across the
suspension like any count.

```text
use std.time.{Clock, sleep, ms}

fn bump!(p: &mut i64) -> void $ Clock:
    sleep!(ms(1))
    *p += 1

fn outer!(n: i64) -> i64 $ Clock:
    let mut local: i64 = n
    bump!(&mut local)
    local
```

The prototype checks and runs this (`outer!(1)` prints 2), and main
does too. One gap: borrowing a suspending function's own `mut`
parameter, `fn outer!(mut n: i64)` with `bump!(&mut n)`, fails on main
and in the prototype with `unsupported: a mutably captured parameter of
a suspending function in the linear backend is not supported`. The
owner's decision needs that parameter given a frame slot (F10).

**Cold suspensions are an escape.** A plain call of a bang function
builds a cold `Suspend[T]` that captures its arguments
([`req.suspend.cold.captures`](../../spec/lang/11-requirements-and-suspension.md#r-req.suspend.cold.captures)),
and that value may be stored or returned:

```text
use std.time.{Clock, sleep, ms}

struct Buf:
    n: i64

fn fill!(b: &mut Buf) -> void $ Clock:
    sleep!(ms(1))
    b.n += 1

fn later(b: &mut Buf) -> mut Suspend[void] $ Clock:
    fill(b)
```

The prototype accepts `later`, which returns a suspension holding the
caller's local reference. **Answer:** a plain call of a bang function
with a local argument is `local-reference-escapes` (L6); `fill!(b)`
stays valid. The same applies to children built for `std.task.all` and
`race`.

**`defer`, `using` and `?`.** A `defer` suite may use a local
reference; the borrow's count, if L4 takes one, is released after the
last use, which a defer suite extends to each exit of its block. `?`
and `return` end the function and every borrow. `using x = open(&cfg)`
is valid; the `Disposable` it returns is no reference type and cannot
hold `&cfg`.

```text
struct Log:
    lines: List[string]

fn step(log: &mut Log, text: string) -> Result[i32, string]:
    defer:
        log.lines.push("done")
    n := text.parse::[i32]().map_err(fn(e) -> string: "bad")?
    log.lines.push("ok")
    .Ok(n)
```

Prototype: checks.

### 4.5 `match` bindings

```text
enum Expr:
    Num(n: i64)
    Add(left: *Expr, right: *Expr)  # hypothetical syntax

fn eval(e: &Expr) -> i64:
    match e:
        .Num(n) => n
        .Add(l, r) => eval(l) + eval(r)

fn keep(e: *Expr) -> *Expr:  # hypothetical syntax
    e

fn whole(e: &Expr) -> i64:
    match e:
        w =>
            k := keep(w)
            eval(k)
```

**Answer:** a payload binding is a copy
([`ref.match.copy`](../../spec/lang/13-values-and-references.md#r-ref.match.copy)),
so `l` and `r` are counted `*Expr` values read out of the payload, and
they may go anywhere; they lend to `eval`'s `&Expr`. A binding of the
whole scrutinee is the reference itself
([`ref.match.binding`](../../spec/lang/13-values-and-references.md#r-ref.match.binding)),
so `w` is local and `keep(w)` is L3's error. Prototype: exactly this.

### 4.6 Loops: a borrow that crosses iterations

```text
fn bump(p: &mut i64) -> void:
    *p = *p + 1

fn loop_cross(n: usize) -> i64:
    let mut xs: List[i64] = [1, 2, 3]
    let first: &mut i64 = &mut xs[0]
    for _i in 0..n:
        bump(first)
        xs.push(4)
    xs[0]
```

**Answer (open question Q2):** under the recommended answer this is
valid, and the push panics at run time: `first` is an element borrow,
which holds today's pin while its binding lives, so `xs.push(4)` while
`first` is still used panics `collection-pinned`, as a call-scoped
element reference does today. If element borrows may not be bound with
`let`, the `let` is today's `element-reference-outlives-call`. The
prototype, which implemented the cut R3, reports
`scoped-borrow-conflict` at `xs.push(4)` instead.

A field or local borrow may cross iterations:

```text
struct Acc:
    total: i64
    items: List[i64]

fn add(p: &mut i64, x: i64) -> void:
    *p += x

fn sum_all(xs: List[i64]) -> i64:
    let mut acc = Acc { total: 0, items: [] }
    let t: &mut i64 = &mut acc.total
    for x in xs:
        add(t, x)
        acc.items.push(x)
    acc.total
```

**Answer:** valid. `t` points into `acc`'s own storage, which no write
can free, so it needs no count; `acc` is dropped after `t`'s last use
rather than at its own (L4). Writing `acc.total = 0` or
`acc = Acc { .. }` inside the loop is valid too, and `t` then sees the
new value. Prototype: checks; with `acc.total = 0` added it reports the
cut R3's `scoped-borrow-conflict`.

Because a local reference can be neither a type argument nor stored in
any value (L1), its copies are other local bindings and call arguments
only. The last use of a borrow is therefore the last use of a small set
of bindings in one body, which the ownership passes already compute for
locals. No liveness over the control-flow graph is needed (F7).

### 4.7 Closures that do not escape

```text
fn each(xs: &List[i64], f: fn(&i64) -> void) -> void:
    for x in &xs:
        f(x)

fn sum(xs: List[i64]) -> i64:
    let total: i64 = 0
    each(&xs, fn(x: &i64) -> void: total += *x)
    total

fn sum_into(xs: List[i64], acc: &mut i64) -> void:
    each(&xs, fn(x: &i64) -> void: *acc += *x)
```

**Answer:** a function type may take local parameters, and a closure
receives them; `sum` is valid and runs (prints 6). `sum_into` is an
error: the closure captures `acc`, and a closure value may escape (L6).
Swift accepts the equivalent: a non-escaping closure may capture an
`inout` parameter. The first cut has no non-escaping closure type, so
the error stays; the fix is a loop or an explicit parameter (open
question Q5).

### 4.8 Generics and traits

```text
fn id[T](x: T) -> T:
    x

fn gen(p: &i64) -> i64:
    q := id(p)
    *q
```

**Answer:** an error. `&T` is never a type argument (L1), so `id(p)`
would instantiate `T = &i64` and is `local-reference-escapes`, as
Swift's `inout` cannot be a generic argument. `dbg(r)`,
`assert_equal(r, ..)`, `[r]` and `.Some(r)` are errors the same way; the
fix is `*r`, a copy, or a counted `*T`. A generic parameter
`xs: &List[T]` with `T` a type parameter is valid: the reference is
local, `T` is not. An optional local reference, `&T?`, is `Option[&T]`
and so is an error; such a parameter becomes `*T?` or splits into two
functions. Prototype: `scoped-reference-escapes` at `id(p)`.

Trait methods take local receivers, and generic code calls them through
a bound, as `x.grow(1.0)` does for `T < Shape`. A trait method may take
`*self`; an impl may then take `&self`, which promises more, but never
the reverse (open question Q6).

### 4.9 `&dyn` and `*dyn`

```text
trait Shape:
    fn area(&self) -> f64

struct Sq:
    side: f64

impl Shape for Sq:
    fn area(&self) -> f64:
        self.side * self.side

fn dyn_area(s: &dyn Shape) -> f64:
    s.area()

fn demo() -> f64:
    q := Sq { side: 2.0 }
    let shapes: List[*dyn Shape] = [&Sq { side: 3.0 }]  # hypothetical syntax
    dyn_area(&q) + dyn_area(shapes[0])
```

**Answer:** `&dyn Tr` is a local trait value reference, data address
and vtable, uncounted; `*dyn Tr` is a counted one, storable in a list. A
`*dyn Shape` lends to `&dyn Shape`, so `dyn_area(shapes[0])` is valid.
`[&Sq { .. }]` with the expected type `List[*dyn Shape]` makes a new
counted object (L2). A provider installed with `$.with(T=&mut fake)`
may be captured once fetched
([`fn.capture.provider-value`](../../spec/lang/07-functions.md#r-fn.capture.provider-value)),
so providers are `*dyn` values: `&mut fake` there is a counted
reference to `fake`'s cell, as today. Prototype: checks.

### 4.10 Counted to local, and back

`*T` lends to `&T` wherever a local reference is expected: `*mut T` to
`&mut T`, and either to `&T`. `&T` never becomes `*T`, whether as an
argument, a binding with a written `*T`, a field, an element or a
result. An interior place (`&x.f`, `&xs[i]`) is only ever `&`, so a
counted reference always points at a whole object.

### 4.11 The diagnostics

| Code | When | Fix-it |
| --- | --- | --- |
| `local-reference-escapes` (new) | a local reference where a counted one is expected (argument, binding, field, element, result); a local reference as a type argument; a capture; a `*self` method on an interior place or through `&`; a plain call of a bang function with a local argument | make the callee's parameter `&`; `*r`; make the field its own object; a loop instead of a closure |
| `element-reference-outlives-call` (today) | only if Q2 keeps element borrows out of `let` | keep the index |
| `conflicting-access` (today) | a length change in the same call as an element borrow | unchanged |
| `collection-pinned` (today, a panic) | a length change of a list while an element borrow lives | unchanged |

The prototype's two codes, `scoped-reference-escapes` and
`scoped-borrow-conflict`, belong to its earlier spelling and to the cut
R3; the first becomes `local-reference-escapes`, the second goes.

## 5. The Rules

These are the rules the owner's decisions give, refined by the findings
of section 9. They replace the first cut's R1 to R4.

**L1. Two kinds of reference.** `*T` and `*mut T` are counted
references: always to a whole object (a heap object, a local's cell, a
new object), storable in fields, lists and maps, returnable, capturable,
and usable as type arguments. `&T` and `&mut T` are local references:
written on parameters, as receivers (`&self`, `&mut self`), on `let`
bindings and on function types' parameters. A local reference is never
a field, an element, a payload, a result or a type argument, so there
is no `List[&T]`, `Option[&T]`, `&T?` or `T = &Foo`.

**L2. Taking an address.** `&x` always takes an address, and `*p`
always dereferences either kind. The kind of `&x` comes from the
expected type. Where `*T` is expected, `x` must be a whole object: a
local (its cell), the referent of a `*T`, or a new object. An interior
place, `&x.f`, `&xs[i]`, `&m[k]` or anything reached through a `&`, is
only ever `&`. With no expected type, `&x` is local (open question Q1
for a new object).

**L3. Conversions.** `*T` lends to `&T` implicitly, and `mut` weakens
as today. `&T` never becomes `*T`: `local-reference-escapes`.

**L4. Counting, by construction.** There is no exclusivity check:
writes to a borrowed place, or around it, are always valid, and `&mut`
stays non-exclusive. What replaces R3 is a rule applied where each
borrow is made: nothing may free or move storage a live local reference
points into.

| The borrowed address is in | Example | How it stays valid | Cost per borrow |
| --- | --- | --- | --- |
| a local's own storage, or a lent reference parameter's referent, through fields only | `energy(&b)`, `self.items.push(x)`, `f(&mut p.pos)` | a borrowed local is dropped after the borrow's last use, not at its own; a reassignment writes the same storage, so the borrow sees the new value | none |
| an object reached through a `*T` that a local holds and no closure captures, for a call argument | `bump(&mut o.f)` with `o: *mut Obj` | the local's own count holds the object, and nothing can change the local during the call | none |
| an object reached any other way: through a field, an element or a captured binding, or by a `let`-bound borrow | `bump(&mut h.child.f)`; `let r = &mut o.f` | one fast-path count (`retain_known`) of the object, released after the borrow's last use; `o = other` then leaves `r` on the old object, as Go's `p := &o.f` does | one known pair, about 2.7 ns |
| a list element or a map value | `xs[i].step(dt)`, `swap(&mut xs[i], &mut xs[j])` | today's pin and one fast-path count of the buffer: a length change during the borrow panics `collection-pinned`, and a copy of a pinned list copies its elements ([`ref.elem.copy`](../../spec/lang/13-values-and-references.md#r-ref.elem.copy)) | one known pair and the pin; both omitted for an unaliased local list that no other argument of the call names |

```text
struct Obj:
    n: i64

fn demo() -> i64:
    let o: *mut Obj = &mut Obj { n: 1 }  # hypothetical syntax
    let r: &mut i64 = &mut o.n
    o = &mut Obj { n: 2 }
    *r = 5  # writes the first object, which r keeps alive
    o.n
```

A borrow's last use is the last use of its binding and of the local
bindings it was copied to, within one body; a parameter received from a
caller is valid for the whole call, so its copies cost nothing.

**L5. Receivers.** `&self` and `&mut self` are local. A method that
keeps `self` takes `*self` or `*mut self`, as Go's pointer receivers do;
such a method needs a whole object, so calling it on an interior place
or through a `&` is `local-reference-escapes`.

**L6. Escapes beyond the type.** A local reference cannot be captured by
a closure, and a plain call of a bang function with a local argument is
an error, since the cold suspension keeps its arguments. A bang call
`f!(..)` with local arguments is valid across any number of
suspensions.

**L7. std.** Every std reference parameter is `&` or `&mut`, except the
parameters that keep their argument, which are `*mut T` (about 17,
section 9.3). std's 19 reference-typed fields become `*T`, its returned
references `*T`, and its methods that keep `self` take `*self`.

## 6. Soundness Without An Exclusivity Check

With R3 cut, an uncounted local reference must not dangle. The two
failures to prevent are `let r = &mut xs[i]` followed by `xs.push(..)`,
and `let r = &mut o.f` followed by `o = other`, where `o` holds a `*T`.
Four candidates were priced; the owner chose counting with rules (L4).

**A: one fast-path count of the root at the borrow.** The borrow counts
the block it points into, with `retain_known` on the block's start, and
passes on with no count. Nothing can be freed under it. Alone, a list's
push would copy the buffer the borrow shares, and a write through the
stale borrow would land in the old buffer: memory-safe, but the Go
behavior hd's spec rejects for elements.

**B: tier 2's borrow flag.** Set a header bit at the borrow, clear it at
the end, and panic on a resize, a replacement or a release of a flagged
block ([Tier 2 §6.4](OBJECT_MODEL_TIER2.md#64-the-borrow-flag)). Setting
and clearing cost about a known pair. For lists this is today's pin; for
objects it turns `o = other` into a panic where A keeps the old object.

**C: restrict what can be a root.** Free, but alone it forbids
`xs[i].step(dt)` and `h.child.items.push(x)`.

**D: a static invalidation check.** Liveness of each borrow and an error
on a length change or a replacement while it lives: about 430 lines,
and the analysis the owner cut.

**L4 combines C, A and today's pin.** The unit costs were measured on
main, 50,000,000 calls per loop, seven runs each on Node with
`--release` (load 6 to 15):

| Loop body | Count operations per call | Median | Per call over the empty loop |
| --- | --- | ---: | ---: |
| `s += take(xs[0])`, a value | none | 243 ms | — |
| `s += take(xs)` with `take(mut v: List[i64])` | one known pair on the buffer | 379 ms | 2.7 ns |
| `s += field(&o.f)` with `field(p: &i64)` | one general pair, 2 `block_of` | 541 ms | 6.0 ns |

| | A alone | B alone | C alone | D | L4 |
| --- | --- | --- | --- | --- | --- |
| local or `self` field borrow | known pair | flag set and clear | free | free | free |
| object behind a stored `*T` | known pair | flag, and a panic on replacement | not allowed | free when local | known pair |
| list element | known pair; stale writes after a resize | the pin | not allowed | free when local | known pair and the pin |
| `let r = &mut xs[i]; xs.push(..)` | silently stale | panic | error | error | panic, as today's pin (Q2) |
| `let r = &mut o.f; o = other` | `r` keeps the old object | panic | error | error | `r` keeps the old object |
| per-body analysis | none | none | none | ~430 lines | none |

The copy case is kept too: `write_then_read(&mut ys[0], ys)` prints
`1 99` today. The prototype, which made element borrows uncounted,
printed `99 99`; L4's pin and count restore today's eager copy (F3).

## 7. Architecture

### 7.1 Where it lives

| Stage | Crate | Change |
| --- | --- | --- |
| parse | `hd_syntax` | `*T`, `*mut T`, `*dyn Tr` in types; `*self`, `*mut self` receivers; the formatter keeps them |
| types | `hd_types` | `TyData::Ref { inner, mutable, counted }`; two pool tags and two wire tags; display; unify, solver and join compare `counted` |
| headers | `hd_resolve` | lower both kinds in parameters, results, fields and receivers |
| check | `hd_check` | `&x` typed by the expected type, `*T` lending to `&T`, `local-reference-escapes` for positions, type arguments, captures, `*self` on interior places and cold calls |
| TIR | `hd_tir` | the Ref instruction's kind, and the L4 row of each local borrow |
| ownership | `hd_tir::own` | per local borrow: free, a known count of the containing block, or the pin with a known buffer count; a borrowed local's drop after its borrows' last use |
| emit | `hd_wasm_linear` | `retain_known` and `release_known` of the containing block's start for L4 rows 3 and 4; every `*T` count through the known path |

Reference types are lowered to `std.core.Ref[T]` before the ownership
passes (`Types::lower_refs`). Since a local reference is never a type
argument, the lowering needs no second type: the kind is a property of
each `Ref` instruction and of each local's declared type, which the
prototype carries as `ref_word::SCOPED`.

### 7.2 Data structures

```rust
/// How a local borrow stays valid (L4), decided per `Ref` instruction.
enum BorrowRow {
    Free,                 // a local's storage or a lent parameter's referent, fields only
    HeldByLocal,          // through an uncaptured local's `*T`, call argument only
    CountBlock,           // one retain_known of the containing block
    PinBuffer,            // a list element or map value: the pin and one buffer count
}

/// Per body, after the borrow pass.
struct LocalBorrows {
    row: Vec<(u32, BorrowRow)>,     // Ref instruction and its row
    group: Vec<(u32, Vec<LocalId>)>,// a let-bound borrow and the local bindings it reaches
    root_locals: Vec<LocalId>,      // locals a let-bound borrow points into
}
```

### 7.3 Classifying a borrow

```rust
fn classify(body: &Body, r: Inst, bound: bool) -> BorrowRow {
    let place = body.place_of(r);
    if place.has_elem_step() {
        return if !bound && unaliased_local_list(body, &place) && !other_arg_names(body, r, &place) {
            BorrowRow::Free          // the spec's note: a pin may be omitted when provably unneeded
        } else {
            BorrowRow::PinBuffer
        };
    }
    match place.last_reference_hop() {
        None => BorrowRow::Free,                                   // a local's own storage
        Some(Hop::LentParam(_)) => BorrowRow::Free,                // the caller holds it for the call
        Some(Hop::Local(l)) if !bound && !body.is_captured(l) => BorrowRow::HeldByLocal,
        Some(_) => BorrowRow::CountBlock,                          // a field, an element, a cell, or bound
    }
}

fn lower(body: &mut Body, borrows: &LocalBorrows) {
    for &(r, row) in &borrows.row {
        let end = last_use_of_group(body, borrows, r);  // the statement after the group's last read
        match row {
            BorrowRow::Free | BorrowRow::HeldByLocal => mark_lent(body, r),
            BorrowRow::CountBlock => insert_known_count(body, r, end),
            BorrowRow::PinBuffer => keep_pin_with_known_count(body, r, end),
        }
    }
    for &l in &borrows.root_locals {
        delay_drop_past_group(body, l);  // the local's Drop moves after its borrows' last use
    }
}
```

`last_reference_hop` is the dereference nearest the address, which the
emitter's place walk already computes for `Ref` (`Em::place_start`,
`Em::walk_place`). `last_use_of_group` reuses insertion's rule for a
local's `Drop` (docs/compiler/tir-v2.md, "Insertion" rules 2 to 4),
applied to the union of the group's accesses.

### 7.4 Caching and incremental behaviour

The kind of every reference is part of signatures, so it is in the
interface hash: a header change rechecks callers. A body edit rechecks
that body only; the classification reads callee signatures, never
callee bodies.

### 7.5 Testing

- conformance fixtures for each row of 4.11 and each edge case of
  section 4, valid and invalid;
- runtime fixtures for each row of L4: `o = other` under a bound
  borrow, a push under a borrowed element (`collection-pinned`), and
  the copy case (`1 99`);
- `counts` stage fixtures for U1 to U10 in both kinds;
- the std runtime's allocation test from #702, once frame slots land.

## 8. Cost

### 8.1 Run time, per use

| What | Today | Under L4 | Source |
| --- | --- | --- | --- |
| `f(&local)` to a local parameter | 1 cell per declaration + 1 general pair (2 `block_of`) per call | 1 cell per declaration, no pair | `scoped-834/a-micro` |
| `f(&mut local.field)`, `self.items.push(x)` | pair through `rt:ref_project` per call | none | `scoped-834/b-field` |
| `bump(&mut o.f)`, `o` an uncaptured local holding a `*T` | general pair | none | L4 row 2 |
| `bump(&mut h.child.f)`, or any `let`-bound borrow behind a `*T` | general pair | one known pair | L4 row 3 |
| `f(&mut xs[i])`, `xs` an unaliased local named by no other argument | `IsUnique`, `Pin`, general buffer pair | `IsUnique` only | `scoped-834/c-elems` |
| `f(&mut xs[i])`, `xs` behind a reference | `IsUnique`, `Pin`, general buffer pair | `IsUnique`, `Pin`, known buffer pair | U5 |
| `local.method()` with a `&self` receiver | pair per call | none | U1, U3, U10 |
| passing a local parameter on | nothing | nothing | U9 |
| `&mut` to `&` weakening of a parameter | pair | pair (independent gap) | `w-weaken` |
| `f(&temporary)` | 1 new object | 1 new object | U7 |
| any count of a `*T` | general pair through `block_of` | known pair, no `block_of` | L1 (#704) |

The known pair costs 2.7 ns and the general pair 6.0 ns (section 6). On
the microbenchmark of section 10.4 (row 1) the median falls from 527 ms
to 273 ms, about 5 ns per call.

**`block_of` on #704's programs.** binary_trees' 11.2 million and
json's 6.6 million `block_of` calls are counts of counted references
and views. Local references remove almost none of them; L1's rule that
a `*T` always points at a whole object removes the lookup from every
reference count, which #704's measurement found to be 7% of
binary_trees' samples and 14% to 22% of json's. Of the study
benchmarks, only nbody's `advance(&mut bodies, 0.01)` (1.15 million
calls, one pair each, about 2% of its time) and spectral_norm's 20 calls
take a reference to a local.

**Cells (#702).** The cell stays in every case. A local whose every
reference is local could live in a frame slot, which is #702's rule
with the escape fact now written in the header. Pairing the two removes
one allocation per declaration execution, and temporaries passed to `&`
parameters would use the same slots.

### 8.2 Compile time

L4 adds no per-body analysis: classifying each local `Ref` is one look
at its place, and the group of a `let`-bound borrow is found from local
assignments, both linear in the body. The prototype's per-body check,
which belonged to the cut R3, was quadratic in the worst case; measured
with it, a cold `hd build --release --jobs 1` of bench/study's json and
nbody, std included, 15 runs each, each with a fresh `HD_CACHE`:

| Program | main, min / median | prototype, min / median |
| --- | ---: | ---: |
| json | 293 / 303 ms | 289 / 316 ms |
| nbody | 231 / 249 ms | 239 / 254 ms |

The differences, under 5%, are within run-to-run noise (machine load
about 6). Neither program writes a local reference, so the cost
measured is the extra type tag, the scan and the coercion branches.

### 8.3 Code, bottom-up

| Component | Prototype lines | Budget | Notes |
| --- | ---: | ---: | --- |
| syntax: `*T`, `*mut T`, `*dyn`, `*self`, formatter round trip | 11 | 50 | |
| type flag: pool, wire, display, unify, solver, join | 105 | 145 | 127 match sites on `TyData::Ref` in 30 files |
| headers: lowering of both kinds, receivers | 21 | 40 | |
| `&x` typed by the expected type, lending, L3's error, trait variance | 45 | 100 | |
| escapes: positions, fields, type arguments, captures, `*self` on interior places, cold calls | 90 | 120 | |
| per-body borrow check | 150 | 0 | the cut R3 |
| ownership: L4 rows, borrow groups, borrowed locals' drops | 25 | 180 | |
| emitter: known counts of the containing block; every `*T` count on the known path | 0 | 100 | |
| diagnostics codes and fix-its | 8 | 40 | |
| suspending functions' borrowed `mut` parameters in frame slots (F10) | 0 | 40 | |
| **total** | **576** | **~815** | stop rule: a slice past 1.5 times its row stops for review |

The respelling is outside these lines: about 1,100 type sites across
std, the spec, the fixtures and the docs (section 9.3), mostly turning a
stored or returned `&T` into `*T`. Web compiler size grows with
`hd_check` by the line count above, a few kilobytes compressed; the 2 MB
cap is not near.

## 9. Stress Test

### 9.1 Method

Ten real-code shapes were translated into the rules. Library shapes are
approximations, written from the public behavior. Each hd block was
checked with the prototype (`hd check`, and run where it has a `main`),
after translating it to the prototype's spelling (section 16), and its
counted variant with main's compiler. Verdicts: *works*, *friction*
(works with a workaround or a visible difference), *breaks* (the rules
cannot express it). These are type-checked, which is stronger than
parsing.

| Case | Library | Verdict | Rule |
| --- | --- | --- | --- |
| S1 `retain` | Rust `Vec::retain` | works; the prototype, which needed `&{}` on `pop`, rejects it | L5 |
| S2 `sift_up` | Rust `BinaryHeap` | works; element borrows behind `self` cost a known pair | L4 |
| S3 `partition` | Swift `MutableCollection.partition(by:)` | works | — |
| S4 scanner | Go `go/scanner.Scanner` | works | L1, L5 |
| S5 `pointer_mut` | serde_json `Value::pointer_mut` | breaks | L1 (no local result) |
| S6 entry counter | Rust `HashMap::entry` | friction | — (#683 idiom) |
| S7 nested `iter_mut` | Rust `Vec<Vec<f64>>` | friction | today's diagnostic |
| S8 visitor | syn `VisitMut` | works | — |
| S9 value builder in a field | reqwest `RequestBuilder` shape | breaks | L5 (F5) |
| S10 cold read into a buffer | tokio `read(&mut buf)` | works with L6; the prototype lacked it | L6 |

### 9.2 Cases

**S1, Rust's `Vec::retain`.**

```rust
pub fn retain<F: FnMut(&T) -> bool>(&mut self, mut f: F) {
    let len = self.len(); let mut del = 0;
    for i in 0..len {
        if !f(&self[i]) { del += 1; } else if del > 0 { self.swap(i - del, i); }
    }
    if del > 0 { self.truncate(len - del); }
}
```

```text
fn retain[T](xs: &mut List[T], keep: fn(&T) -> bool) -> void:
    n := xs.len()
    let deleted: usize = 0
    for i in 0..n:
        if !keep(&xs[i]):
            deleted += 1
        else if deleted > 0:
            tmp := xs[i - deleted]
            xs[i - deleted] = xs[i]
            xs[i] = tmp
    for _ in 0..deleted:
        _ := xs.pop()
```

`xs.pop()` calls std's `pop(&mut self)`, a local receiver: valid. The
prototype, built before the spelling decision, treated an unwritten
receiver as counted and reported `scoped-reference-escapes` there. The
predicate taking `&T` costs nothing per element on a local list, and
keeps the pin behind `xs`.

**S2, Rust's `BinaryHeap::sift_up`.** `push(&mut self, ..)` and
`sift_up(&mut self, ..)` call `swap(&mut self.items[pos], &mut
self.items[parent])`. It checks and runs (`[9, 7, 1, 3]`). The element
borrows sit behind `self`, so they keep their pins and take a known pair
each, under half of today's general pair. Rust's own version uses an
unsafe `Hole`; hd's index form (`self.items[pos] = ..`) takes no
reference and is the fast form in both models.

```text
struct MaxHeap:
    items: List[i64]

impl MaxHeap:
    fn push(&mut self, x: i64) -> void:
        self.items.push(x)
        self.sift_up(self.items.len() - 1)

    fn sift_up(&mut self, start: usize) -> void:
        let pos = start
        while pos > 0:
            parent := (pos - 1) / 2
            if self.items[pos] <= self.items[parent]:
                break
            swap(&mut self.items[pos], &mut self.items[parent])
            pos = parent

fn swap(a: &mut i64, b: &mut i64) -> void:
    t := *a
    *a = *b
    *b = t
```

**S3, Swift's `partition(by:)`.** Two indices walking inward with index
stores; no references inside. Works; the only saving is the pair on
`partition(&mut xs, ..)` at the call.

```text
fn partition(xs: &mut List[i64], belongs_second: fn(i64) -> bool) -> usize:
    let lo: usize = 0
    let hi = xs.len()
    while true:
        while lo < hi && !belongs_second(xs[lo]):
            lo += 1
        while lo < hi && belongs_second(xs[hi - 1]):
            hi -= 1
        if lo >= hi:
            break
        t := xs[lo]
        xs[lo] = xs[hi - 1]
        xs[hi - 1] = t
        lo += 1
        hi -= 1
    lo
```

**S4, Go's scanner.**

```go
func (s *Scanner) next() {
    if s.rdOffset < len(s.src) {
        s.offset = s.rdOffset
        if s.ch == '\n' { s.lineOffset = s.offset; s.file.AddLine(s.offset) }
        ...
```

`init` stores its `file` parameter, so the parameter is `*mut File` and
the field `*mut File`; every method takes `&mut self`.

```text
struct File:
    name: string
    lines: List[usize]

struct Scanner:
    file: *mut File  # hypothetical syntax
    src: string
    report: fn(usize, string) -> void
    ch: i32
    offset: usize
    rd: usize
    errors: usize

impl Scanner:
    fn init(&mut self, file: *mut File, src: string, report: fn(usize, string) -> void) -> void:  # hypothetical syntax
        self.file = file
        self.src = src
        self.report = report
        self.ch = +32
        self.offset = 0
        self.rd = 0
        self.next()

    fn error(&mut self, at: usize, msg: string) -> void:
        (self.report)(at, msg)
        self.errors += 1

    fn next(&mut self) -> void:
        if self.rd < self.src.len():
            self.offset = self.rd
            if self.ch == +10:
                self.file.lines.push(self.offset)
            self.ch = i32(self.src[self.rd])
            self.rd += 1
        else:
            self.offset = self.src.len()
            self.ch = -1

    fn skip_space(&mut self) -> void:
        while self.ch == +32 || self.ch == +10:
            self.next()

    fn scan(&mut self) -> string:
        self.skip_space()
        start := self.offset
        if self.ch < +0:
            return ""
        while self.ch > +32:
            self.next()
        if self.offset == start:
            self.error(start, "empty token")
        self.src.slice(start, self.offset)
```

Storing a `*mut File` parameter into a field through a local `self` is
valid: only the receiver is local. `self.file.lines.push(..)` reads a
counted reference out of a field and is unaffected. Works, and runs.

**S5, serde_json's `pointer_mut`.**

```rust
if let Some(v) = doc.pointer_mut("/a/b") { *v = json!(2); }
```

The direct translation returns a local reference, which L1 forbids:

```text
use std.json.Json

fn pointer_mut(doc: &mut Json, key: string) -> &mut Json:
    doc
```

A callback form, `with_member(doc, key, fn(v: &mut Json) -> void: ...)`,
checks, but it still copies the object out of the payload and writes it
back, because a payload is no place. Breaks: in-place editing inside an
enum payload needs a returned projection (Hylo's subscript, tier 2's
results) or payloads as places; neither is in the first cut.

**S6, `entry`.** `*counts.entry(w).or_insert(0) += 1` becomes
`counts[w] = counts.get(w).unwrap_or(0) + 1` with
`counts: &mut Map[string, i64]`: two lookups, the idiom decided for
#683. Friction, unchanged by the first cut.

```text
fn count_words(text: string, counts: &mut Map[string, i64]) -> void:
    for w in text.split_whitespace():
        counts[w] = counts.get(w).unwrap_or(0) + 1
```

**S7, nested `iter_mut`.**

```text
fn scale(grid: &mut List[List[f64]], k: f64) -> void:
    for row in &mut grid:
        for x in &mut *row:
            *x *= k
```

Works and runs. Writing `for x in &mut row` fails in both compilers with
`element-reference-outlives-call`, because `&mut row` refers to the
binding; the message should suggest `&mut *row`. This is a diagnostic
note for today, not a first-cut rule.

**S8, syn's `VisitMut`.** A trait `VisitMut` with
`visit_name(&mut self, name: string) -> string`, an impl `Renamer`, and
`walk(v: &mut dyn VisitMut, e: &mut Expr)` recursing through the AST's
counted `*mut Expr` children. Works and runs (`z 1`). The visitor is
borrowed for the whole walk and passed on uncounted.

```text
enum Expr:
    Var(name: string)
    Call(callee: *mut Expr, args: List[*mut Expr])  # hypothetical syntax

trait VisitMut:
    fn visit_name(&mut self, name: string) -> string

struct Renamer:
    from: string
    to: string
    hits: usize

impl VisitMut for Renamer:
    fn visit_name(&mut self, name: string) -> string:
        if name == self.from:
            self.hits += 1
            return self.to
        name

fn walk(v: &mut dyn VisitMut, e: &mut Expr) -> void:
    match e:
        .Var(name) => *e = Expr.Var(v.visit_name(name))
        .Call(callee, args) =>
            walk(v, callee)
            for a in args:
                walk(v, a)
```

**S9, a value builder in a field.** U4 above: breaks unless a local
receiver may return `self` (open question Q3).

**S10, a cold read into a borrowed buffer.** tokio's
`let n = stream.read(&mut buf).await?` becomes `fill!(&mut buf)`, which
is valid. Building the suspension without driving it, `fill(b)`, is the
case of 4.4; L6 makes it an error, which the prototype did not.

### 9.3 How much is respelled

Counted over `lib/std` (41 files, 19,259 lines):

| Item | Count | In the final spelling |
| --- | ---: | --- |
| methods with a `&mut self` receiver | 516 | local; unchanged text |
| methods with a `&self` receiver | 58 | local; unchanged text |
| of the 574, methods whose body uses `self` as a whole value | 58 | candidates for `*self`: builders (about 25), iterator adapters, `view`, `chunks`, `windows`, `DebugWriter` methods that build a `DebugStruct` holding `self` |
| non-receiver reference parameters (`&mut T` / `&T`) | 237 / 11 | local; about 17 that store their argument become `*mut T` |
| struct fields of reference type | 19 | `*T` |
| functions returning a reference | 69 | `*T` |
| method calls on a field of `self`, `self.f.m(..)` | 339 | compile unchanged, the receiver being local |

Reference types in stored or returned positions across the repository,
counted with a pattern scan (a result type, a field, a type argument or
an optional), are the respelling's size:

| Tree | Result `-> &` | Field-like `x: &` | Type argument `[&` | Optional `&T?` |
| --- | ---: | ---: | ---: | ---: |
| `lib/std` | 69 | 33 | 33 | 0 |
| `spec/lang` | 18 | 13 | 34 | 10 |
| `spec/std` | 50 | 3 | 24 | 1 |
| `spec/conformance` | 203 | 137 | 247 | 34 |
| `website`, `examples`, crate tests | 18 | 6 | 16 | 2 |
| `docs/future-work` | 17 | 51 | 19 | 0 |

The "field-like" column also counts `let` annotations and some
parameters, which stay `&`; the checker's `local-reference-escapes`
after the switch (slice 9 of section 13) finds every site the scan
misses.

### 9.4 Findings, ranked

| Rank | ID | Finding | Status | Changed |
| --- | --- | --- | --- | --- |
| 1 | F2 | Without an exclusivity check, an uncounted borrow dangles when its storage is freed or moved. | Answered: L4 by construction. | R1, R3 |
| 2 | F1 | Interior places cannot become counted references, so a method or parameter that keeps its argument cannot take a field or element. | Answered by the spelling: such methods take `*self` and such parameters `*mut T`, and the rest are local; the cost is the respelling of section 9.3. | R1, R4 |
| 3 | F3 | Uncounted element borrows break value semantics: `write_then_read(&mut ys[0], ys)` printed `99 99` in the prototype, `1 99` today. | Answered: L4 keeps the pin and a buffer count. | R1 |
| 4 | F4 | Store, capture and return are not the only escapes: a cold suspension keeps its arguments, and generic code keeps a `T`. | Answered: L6, and no local type arguments (L1). | R1 |
| 5 | F5 | Value builders on fields need a local receiver that may return `self`. | Open: Q3. | R1 |
| 6 | F10 | Borrowing a suspending function's own `mut` parameter across a bang call is `unsupported` in the linear backend today. | Required; slice 14. | — |
| 7 | F6 | No closure may capture a local reference, so a callback cannot write through a local parameter. | Open: Q5. | R1 |
| 8 | F12 | `&` on a new object with no expected type has no kind to take from context. | Open: Q1. | L2 |
| 9 | F7 | R3 and uncounted `let` borrows needed liveness over the control-flow graph. | Answered: with no local type arguments, a borrow's copies are a few local bindings, whose last use insertion already computes. | R2 |
| 10 | F8 | Trait conformance needs a variance rule for receivers. | Open: Q6. | R1 |
| 11 | F9 | The savings are count pairs; cells, temporaries' new objects and the weakening gap remain. | Open: Q4. | — |
| 12 | F11 | Two-phase borrows were needed only by R3. | Answered by the cut. | R3 |

## 10. Prototype And Complexity

### 10.1 What was built

Branch `lane/834-proto`, commit `d912acbf0` on `0c1b055d2`, not merged
and not pushed. It predates the spelling decision: it spells a local
reference `&{} T`, keeps `&T` as today's counted reference, and has no
`*T`. Its parts map onto the final design as follows:

| Prototype | Final design |
| --- | --- |
| `&{} T`, `&{} mut T` on parameters, receivers and `let` | `&T`, `&mut T`; receivers local with no mark |
| `&T` | `*T`, still allowed to point inside an object |
| `scoped-reference-escapes` for local-to-counted, captures, type arguments, containers, positions | `local-reference-escapes`, the same checks |
| `&e` local where a local type is expected or the place is behind a local reference | L2, with the kind from the expected type |
| `let` bindings with a written local type, element borrows included | L1, L4 rows and Q2 |
| `scoped-borrow-conflict`, a per-body check (about 150 lines) | gone with the cut R3 |
| no count for a local `Ref` whose every use lends, except element borrows behind a reference | L4 rows 1, 2 and 5; rows 3 and 4 add a known count it does not take |

It does not implement these, and the first four leave holes in it:

- struct fields typed `&{}`, which it accepts;
- the cold-call rule (L6);
- the copy rule (F3);
- L4's known count for borrows behind stored references or `let`-bound
  borrows behind a local's reference: it makes them uncounted;
- trait variance, `*self`, and diagnostic notes.

The count savings of section 2.2 and the microbenchmark of 10.4 are
therefore the savings of L4's free rows; rows 3 and 4 add a known pair
where the prototype has none.

### 10.2 Lines and stages

Code lines counted with `codelines.py` (no blanks, comments or
`#[cfg(test)]` items), new minus old per file:

| Crate | Lines | Stage |
| --- | ---: | --- |
| `hd_check` | 417 | checker: new `scoped.rs` (246), coercion, receivers, `let` |
| `hd_types` | 74 | type representation |
| `hd_tir` | 25 | `ref_word::SCOPED`, borrow pass, verifier |
| `hd_resolve` | 21 | header lowering |
| `hd_syntax` | 11 | parser |
| `hd_mono` | 10 | display, mechanical |
| `hd_diag` | 8 | two codes |
| `hd_driver` | 6 | `OwnCx::scoped` |
| `hd_structure` | 4 | mechanical |
| `hd_wasm_linear` | 0 | emitter reuses loans |
| **total** | **576** | 40 Rust files |

### 10.3 What was harder than expected

1. **The type flag touches most of the 40 files.** `TyData::Ref` has 127
   match sites in 30 files; every exhaustive pattern and constructor
   changed, mostly mechanically. Equality in unify, the solver, joins
   and permission fitting each needed a decision.
2. **The flag dies before the ownership passes.** The checker lowers
   reference types to `std.core.Ref[T]`, so the passes cannot see the
   kind; the prototype marks the `Ref` instruction. With no local type
   arguments, that is enough for the final design.
3. **Receivers take a separate path.** An automatic address is made in
   `receiver.rs`, not through argument coercion, so the receiver's kind
   needs the method's signature there.
4. **Uncounted element references disable the pin.** The pin is "the
   buffer's count is over one"; a reference without a count leaves the
   list unpinned, which gave the `99 99` result of F3. L4 keeps the
   count for that reason.
5. **The checker has no control-flow graph.** The cut R3's check had to
   recover loops from `Topology`. L4 needs no control flow.
6. **A cold suspension hides its arguments.** Its type, `mut
   Suspend[void]`, mentions none of them, so the type-based escape
   checks miss it; L6 checks the call.

### 10.4 The microbenchmark

```text
struct Body:
    x: f64
    y: f64
    vx: f64

fn energy(b: &Body) -> f64:
    b.x * b.vx + b.y

pub fn main() -> void $ Console:
    let mut b = Body { x: 1.0, y: 2.0, vx: 3.0 }
    let sum = 0.0
    for _i in 0..50_000_000:
        b.x += 1.0
        sum += energy(&b)
    println("$sum")
```

Built with `hd build --release`, the counted variant by main and the
local one by the prototype, and run nine times each on Node,
alternating. Both print `3750000325000000.0`.

| Variant | Count operations per call | Allocations | Median | Runs (ms, sorted) |
| --- | --- | --- | ---: | --- |
| counted, main | 1 general retain + 1 release, 2 `block_of` | 1 cell per run | 527 ms | 516, 516, 523, 523, 527, 529, 532, 590, 778 |
| local, prototype | none | 1 cell per run | 273 ms | 265, 269, 269, 273, 273, 276, 277, 490, 528 |

The difference, 254 ms over 50 million calls, is about 5 ns per call
(machine load about 6). The cell is made once, before the loop, so
neither variant allocates per call.

### 10.5 What the prototype breaks

It breaks no existing test that was run:

- the crate tests of `hd_syntax`, `hd_types`, `hd_tir`, `hd_check`,
  `hd_driver`, `hd_resolve`, `hd_mono`, `hd_wasm_linear` and `hd_diag`
  pass, stage fixtures included;
- `hd_driver`'s conformance gate,
  `specification_conformance_passes_except_known_failures`, passes in
  the dev profile;
- `cargo clippy --all-targets -- -D warnings` passes.

`bash spec/check.sh` fails at "compiler diagnostic codes are stale",
because its two new codes are not in the spec's normative table, which
generates the compiler's code enum. The prototype adds `counts`
fixtures for U1 to U10 in both kinds and for the weakening gap, under
`crates/hd_driver/tests/cases/counts/scoped-834/`. The wasmtime engine
and the web compiler were not built.

## 11. What Tier 2 Keeps, Drops And Changes

| Tier 2 rule | First cut |
| --- | --- |
| [§2](OBJECT_MODEL_TIER2.md#2-the-model-on-one-page) item 1: `&{a, b} T` with sets | changed: one local kind, spelled `&T`; tier 2's global `&T` is spelled `*T` |
| §2 item 2: sets name parameters, paths, lifetime parameters, locals, `global` | dropped |
| §2 item 3: global converts to scoped; views are promoted when stored | the lending conversion kept; promotion dropped |
| §2 item 4: types with lifetime parameters | dropped |
| §2 item 5: `&{} mut` admits no other writer; `&{}` freezes its place | dropped: no exclusivity, no freezing |
| §2 items 6 and 7: closure sets, `~Copy` | dropped |
| §2 item 8 and [§6.4](OBJECT_MODEL_TIER2.md#64-the-borrow-flag): the borrow flag | dropped; L4's known counts for objects, and today's pin for elements, which is the flag for lists |
| [§3.1](OBJECT_MODEL_TIER2.md#31-what-a-set-may-name): what a set may name | nothing: the local kind has no set |
| [§3.2](OBJECT_MODEL_TIER2.md#32-results-name-their-sources): results name their sources | dropped: no local result (S5) |
| [§3.3](OBJECT_MODEL_TIER2.md#33-inside-a-body) rule 1, sets of locals | kept in degenerate form: a `let` of a local reference is local |
| §3.3 rule 2, extent by NLL liveness | replaced: a borrow lasts to its binding group's last use (L4) |
| §3.3 rule 3, `&e` is scoped when a scoped type is expected | kept, reversed in default: `&e` is local unless a `*T` is expected (L2) |
| §3.3 rule 4, reborrows | kept for parameters (4.3) |
| §3.3 rules 5 and 6, shrinking and temporaries | not needed; temporaries still allocate (F9) |
| [§4.2](OBJECT_MODEL_TIER2.md#42-a-value-containing-a-scoped-reference-is-scoped), [§5.3](OBJECT_MODEL_TIER2.md#53-generics), [§8.2](OBJECT_MODEL_TIER2.md#82-generic-parameters-may-be-scoped), [§8.3](OBJECT_MODEL_TIER2.md#83-monomorphization): scoped values, generics and instances | dropped: a local reference is never a type argument |
| [§6.1](OBJECT_MODEL_TIER2.md#61-the-rules) to [§6.3](OBJECT_MODEL_TIER2.md#63-errors-and-how-often-programs-meet-them): loans, liveness, conflicts, two-phase | dropped; only the invalidation idea survives, enforced by L4 |
| [§7](OBJECT_MODEL_TIER2.md#7-closures): closure kinds and sets | dropped; captures are errors (F6) |
| [§8.1](OBJECT_MODEL_TIER2.md#81-unannotated-parameters-stay-global): unannotated parameters stay global | reversed: a parameter is local unless written `*T` |
| [§8.4](OBJECT_MODEL_TIER2.md#84-std-written-lifetime-aware): std written lifetime-aware | moved into the first cut: std's parameters are local, its keepers `*mut T` (L7) |
| [§10](OBJECT_MODEL_TIER2.md#10-suspension-providers-and-boundaries): a cold suspension with scoped arguments is scoped; bang calls may borrow | the bang call kept; the cold suspension is an error (L6) |

## 12. When Local References Get In The Way

The owner's advice: "if users hate, just use *T or read, copy, write
back". Both are ordinary programs.

**Take a counted `*T`.** An object of its own may be stored, returned
and captured, at one count per copy:

```text
struct Counter:
    n: i64

struct Watch:
    counters: List[*mut Counter]  # hypothetical syntax

fn remember(w: &mut Watch, c: *mut Counter) -> void:  # hypothetical syntax
    w.counters.push(c)

fn demo() -> i64:
    let mut w = Watch { counters: [] }
    let c: *mut Counter = &mut Counter { n: 0 }  # hypothetical syntax
    remember(&mut w, c)
    c.n += 1
    w.counters[0].n
```

**Copy the value out, change it, write it back.** No reference at all,
only value semantics; a struct copy is a few word moves, and a list's
copy-on-write keeps it cheap:

```text
struct Player:
    name: string
    score: i32

fn reward(players: List[Player], best: usize) -> List[Player]:
    let mut out = players
    let mut p = out[best]  # a copy of the element
    p.score += 10
    out[best] = p  # written back
    out
```

## 13. Implementation Plan

Each slice is about an hour of agent work, green on its own, in this
order. The respelling slices (4 to 8) come before the switch (slice 9):
until then `*T` and `&T` both mean today's counted reference, so every
slice keeps the suites green, and the switch's own errors find any site
the respelling missed. "Lines" are compiler code lines; "sites" are
edited type spellings outside the compiler.

| # | Slice | Files | Spec rules and fixtures | Done check | Budget |
| ---: | --- | --- | --- | --- | --- |
| 1 | Spec: the two kinds | `spec/lang/13-values-and-references.md` (Reference Types, Places, Conversions), `02-grammar.md` (`*T`), `07-functions.md` (`*self`), `09-traits.md` (`*dyn`), `spec/README.md` (the new code) | new rules `ref.kind.counted`, `ref.kind.local`, `ref.local.positions`, `ref.local.no-type-argument`, `ref.expr.address-kind`, `ref.convert.lend`, `ref.convert.no-raise`, `ref.counted.whole-object`, `fn.method.receiver.counted`; parse and typing fixtures, each a `KNOWN_FAILURES.tsv` row until slice 9 | `bash spec/check.sh` | spec text |
| 2 | Spec: counting and escapes | chapter 13 (Element References, a new Local Counting section), `06-control-flow.md` (element loops), `11-requirements-and-suspension.md` (cold calls) | rules for L4's rows, `ref.local.no-capture`, `req.suspend.cold.local`; runtime fixtures `o = other`, `collection-pinned` under a borrow, `write_then_read` printing `1 99` | `bash spec/check.sh` | spec text |
| 3 | Parser and types | `hd_syntax` parser `types.rs`, `items.rs`; `hd_types` `pool.rs`, `wire.rs`, `unify.rs`, `solver.rs`; `hd_resolve` `lower.rs`, `header.rs`, `iface.rs`; display in `hd_mono` and `hd_structure` | slice 1's parse fixtures | `*T` round-trips through `hd fmt`; crate tests pass; `*T` means today's counted reference | 235 lines |
| 4 | Respell std | `lib/std` (19 fields, 69 results, type arguments, `*self` for the keepers, about 17 `*mut T` parameters), `spec/std` signatures | std's doc tests | std builds; `cargo test`; the conformance gate unchanged | ~250 sites |
| 5 | Respell the language chapters and parse fixtures | `spec/lang/*.md` examples; `spec/conformance/parse/` | examples inventory | `bash spec/check.sh`; the conformance gate | ~150 sites |
| 6 | Respell typing fixtures | `spec/conformance/typing/` | — | the conformance gate | ~300 sites |
| 7 | Respell runtime and CLI fixtures | `spec/conformance/runtime/`, `spec/conformance/cli/` | — | the conformance gate | ~300 sites |
| 8 | Respell the rest | `website/`, `examples/`, crate test sources and stage fixtures, `docs/future-work` | — | `cargo test`; `pnpm run website:build` | ~100 sites |
| 9 | The switch: `&T` is local | `hd_check` `refs.rs`, `body.rs` (coercion), `receiver.rs`, `call.rs`, a new `local.rs` for positions and type arguments; `hd_diag` codes | slice 1's typing fixtures leave `KNOWN_FAILURES.tsv` | the conformance gate; fix-its for `local-reference-escapes` | 200 lines |
| 10 | Escapes | `hd_check` `expr.rs` (captures), `call.rs` (cold calls), `dynref.rs` (`*dyn` providers) | slice 2's capture and cold-call fixtures | the conformance gate | 60 lines |
| 11 | L4 free rows | `hd_tir` `own/borrow.rs`, `own/insert.rs` (borrow groups, delayed drops of borrowed locals), `ir/owned.rs` (verifier) | `counts` stage fixtures U1 to U10 | no pair on rows 1, 2 and 5; runtime fixtures pass | 90 lines |
| 12 | L4 counted rows | `hd_tir` `own/insert.rs`; `hd_wasm_linear` `emit/refs.rs`, `emit/count.rs` (known count of the containing block; pin with a known buffer count) | slice 2's runtime fixtures | `o = other` keeps the old object; a push under a borrow panics; `1 99` | 150 lines |
| 13 | `*T` on the known path | `hd_wasm_linear` `emit/count.rs`, `rt.rs`; a verifier check that a `*T` address is a block start | a `counts` fixture per counted kind | #704's instrumentation shows no `block_of` call from a reference count on binary_trees | 40 lines |
| 14 | Suspending `mut` parameters | `hd_wasm_linear` `emit/susp.rs`, `emit.rs` (`ref_roots`) | a runtime fixture of 4.4's gap | `fn outer!(mut n: i64)` with `bump!(&mut n)` runs | 40 lines |
| 15 | Measure | `bench/study`, the microbenchmark | — | hd-only `--quick` before and after for binary_trees, json, nbody; hello's size; json and nbody compile time; posted on #834 | — |

Compiler total: about 815 lines over slices 3 and 9 to 14, the rows of
section 8.3. A slice past 1.5 times its budget stops for review. #702's frame slots for
locals borrowed only by local references follow as their own job (Q4).

## 14. Recommendation

**Recommendation: build the first cut as L1 to L7, in the order of
section 13.** It is sound with no per-body analysis:

- borrows of a local, of a field of `self` and of a lent parameter's
  field are free;
- borrows of an object behind a stored `*T`, and `let`-bound borrows
  behind a local's `*T`, take one known pair, 2.7 ns;
- element borrows keep today's pin, with a known pair instead of
  today's general one;
- every `*T` count takes the known path, with no `block_of`.

It keeps today's semantics: a resize under a borrowed element panics,
and a borrow taken through a `*T` keeps its object as a counted
reference does. It gives up local results, local type arguments and
in-place edits through returned references; those need tier 2's
results or Hylo's projections, and section 12's two escape hatches
cover them in user code.

Per call, the saving is about 5 ns and two `block_of` lookups for a free
borrow, half the microbenchmark's run time, and 3.3 ns for a counted
one. Per program it is small for local references alone: nbody gains
about 2%. The larger effect on binary_trees and json comes from L1's
whole-object rule for `*T` (slice 13).

## 15. Open Questions For The Owner

The owner's decisions answered the first draft's questions on
receivers, exclusivity, soundness, cold suspensions, type arguments,
std's keepers and interior references. These remain.

**Q1. `&` on a new object with no expected type (F12).** `n := &mut
Node { .. }` has no expected type to give it a kind. Candidates: (a) it
is local, as every unannotated `&` is, and storing it is L3's error with
the fix-it `let n: *mut Node = ...`; (b) a new object with no expected
type is counted, as today. **Recommended: (a)**, one rule for every
`&`; the fix-it makes the respelling mechanical.

```text
struct Node:
    value: i64

struct Graph:
    nodes: List[*mut Node]  # hypothetical syntax

fn add(g: &mut Graph) -> void:
    n := &mut Node { value: 1 }
    g.nodes.push(n)  # (a): L3's error; the fix-it writes `let n: *mut Node`
```

**Q2. `let`-bound element borrows (L4).** Candidates: (a) allowed: the
borrow holds its pin and buffer count to its binding group's last use,
so a push while it lives panics, as today's call-scoped pin does; (b)
not allowed, as today (`element-reference-outlives-call`).
**Recommended: (a)**; with no local type arguments its extent is a few
bindings in one body.

```text
fn first_twice(xs: &mut List[i64]) -> void:
    let r: &mut i64 = &mut xs[0]
    *r += 1  # (a): valid; the pin lasts to r's last use
```

**Q3. A local receiver that returns `self` (F5).** Value builders on
fields break without it. Candidates: (a) a `&mut self` method may return
`self` for use within the calling expression, and the result has the
caller's receiver kind; (b) builders take `*mut self` and cannot be
called on a field. **Recommended: (a).**

```text
struct Request:
    headers: List[(string, string)]

impl Request:
    fn header(&mut self, name: string, value: string) -> &mut Request:
        self.headers.push((name, value))
        self
```

**Q4. Frame slots (F9).** The first cut alone keeps every cell.
Candidates: (a) schedule #702's frame slots for locals borrowed only by
local references right after slice 15; (b) keep #702 in the release-only
interprocedural plan. **Recommended: (a)**, since the header now states
the escape fact #702 would compute.

```text
fn sort_in_place(xs: &mut List[i64]) -> void:
    *xs = xs.sorted()

fn report(scores: List[i64]) -> i64:
    let mut xs = scores
    sort_in_place(&mut xs)  # a cell today; a frame slot under (a)
    xs[0]
```

**Q5. Closures (F6).** Candidates: (a) no capture of a local reference;
(b) a closure literal passed directly as an argument may capture one.
**Recommended: (a)**; (b) is tier 2's capture sets in miniature.

```text
fn sum_into(xs: List[i64], acc: &mut i64) -> void:
    for x in xs:
        *acc += x  # the loop form that (a) leaves
```

**Q6. Trait impl variance (F8).** Candidates: (a) an impl may take
`&self` where its trait says `*self`, never the reverse; (b) an exact
match. **Recommended: (a).**

```text
trait Registry:
    fn register(*mut self) -> void  # hypothetical syntax

struct Counter:
    n: i64

impl Registry for Counter:
    fn register(&mut self) -> void:
        self.n += 1
```

## 16. Sources And Parse Log

### 16.1 Sources

- Swift SE-0176, Enforce Exclusive Access to Memory:
  https://github.com/swiftlang/swift-evolution/blob/main/proposals/0176-enforce-exclusive-access-to-memory.md
- Swift SE-0377, `borrowing` and `consuming` parameter ownership
  modifiers:
  https://github.com/swiftlang/swift-evolution/blob/main/proposals/0377-parameter-ownership-modifiers.md
- Swift SE-0446, Nonescapable Types:
  https://github.com/swiftlang/swift-evolution/blob/main/proposals/0446-non-escapable.md
- Rust RFC 2094, non-lexical lifetimes:
  https://rust-lang.github.io/rfcs/2094-nll.html
- Rust RFC 2025, two-phase borrows:
  https://rust-lang.github.io/rfcs/2025-nested-method-calls.html, and the
  rustc dev guide:
  https://rustc-dev-guide.rust-lang.org/borrow_check/two_phase_borrows.html
- Hylo language tour, functions and methods (parameter conventions):
  https://docs.hylo-lang.org/language-tour/functions-and-methods; subscripts:
  https://docs.hylo-lang.org/language-tour/subscripts; bindings:
  https://docs.hylo-lang.org/language-tour/bindings
- Racordon et al., Implementation Strategies for Mutable Value
  Semantics, Journal of Object Technology 21(2), 2022.
- Mojo manual, ownership and argument conventions:
  https://mojolang.org/docs/manual/values/ownership; lifetimes, origins
  and references: https://mojolang.org/docs/manual/values/lifetimes
- Vale, Zero-Cost Memory Safety with Vale Regions:
  https://verdagon.dev/blog/zero-cost-memory-safety-regions-overview
- Go escape analysis: `go build -gcflags=-m`; append semantics:
  https://go.dev/ref/spec#Appending_and_copying_slices
- C++ iterator invalidation: https://en.cppreference.com/w/cpp/container/vector/push_back
- hd: #704 and #834 (the decisions and the `block_of` measurement), #702 (cells),
  #683 (entry idiom), [Object Model, Tier 2](OBJECT_MODEL_TIER2.md),
  [Values And References](../../spec/lang/13-values-and-references.md),
  [docs/compiler/tir-v2.md](../compiler/tir-v2.md),
  [crates/hd_wasm_linear/DESIGN.md](../../crates/hd_wasm_linear/DESIGN.md)
  §23.20.


### 16.2 Parse log

Every `text` block of this document, in order:

- "main: parse" is `scripts/hd-conformance.sh parse` on main at
  `0c1b055d2`, on the block as written. In the final spelling `&T`
  parses today, with today's counted meaning; each `syntax-error` is on
  a line marked `# hypothetical syntax`, which was checked line by line,
  and every other block parses.
- "prototype: check" is `hd check` with the prototype of section 10,
  after a mechanical translation into its spelling: `&` in a type
  position becomes `&{}`, `*` in a type position and `*self` become
  `&` and `&self`. It type-checks as well as parses.
- A note marks an error the text intends, or a result where the
  prototype, built before the owner's decisions, differs from the rules.

| Block | Section | `*T` written | main: parse | prototype: check | Note |
| ---: | --- | --- | --- | --- | --- |
| 1 | 2.2 | no | ok | ok |  |
| 2 | 2.2 | no | ok | ok |  |
| 3 | 2.2 | no | ok | ok |  |
| 4 | 2.2 | yes | line 6: `syntax-error` | line 14: `scoped-reference-escapes` | intended: L5 (F5) |
| 5 | 2.2 | no | ok | ok |  |
| 6 | 2.2 | no | ok | ok |  |
| 7 | 2.2 | no | ok | ok |  |
| 8 | 2.2 | no | ok | line 8: `scoped-reference-escapes` | intended (U8) |
| 9 | 2.2 | yes | line 3: `syntax-error` | ok |  |
| 10 | 2.2 | yes | line 4: `syntax-error` | ok |  |
| 11 | 4.1 | no | ok | ok |  |
| 12 | 4.2 | no | ok | ok |  |
| 13 | 4.3 | yes | line 7: `syntax-error` | line 14: `scoped-reference-escapes` | intended (4.3) |
| 14 | 4.4 | no | ok | ok |  |
| 15 | 4.4 | no | ok | ok | the prototype accepts `later`; L6 rejects it |
| 16 | 4.4 | no | ok | ok |  |
| 17 | 4.5 | yes | line 3: `syntax-error`; line 10: `syntax-error` | line 16: `scoped-reference-escapes` | intended (4.5) |
| 18 | 4.6 | no | ok | line 9: `scoped-borrow-conflict` | the prototype implements the cut R3; under Q2 (a) this checks and the push panics |
| 19 | 4.6 | no | ok | ok |  |
| 20 | 4.7 | no | ok | line 11: `scoped-reference-escapes` | intended: `sum_into` (4.7) |
| 21 | 4.8 | no | ok | line 5: `scoped-reference-escapes` | intended (4.8) |
| 22 | 4.9 | yes | line 16: `syntax-error` | ok |  |
| 23 | 5 | yes | line 5: `syntax-error` | ok | the prototype counts this `let` borrow with a general pair; L4 takes a known one |
| 24 | 9.2 | no | ok | line 12: `scoped-reference-escapes` | valid under L5; the prototype needed `&{}` on std's `pop` |
| 25 | 9.2 | no | ok | ok |  |
| 26 | 9.2 | no | ok | ok |  |
| 27 | 9.2 | yes | line 6: `syntax-error`; line 15: `syntax-error` | ok |  |
| 28 | 9.2 | no | ok | line 4: `scoped-reference-escapes` | intended (S5) |
| 29 | 9.2 | no | ok | ok |  |
| 30 | 9.2 | no | ok | ok |  |
| 31 | 9.2 | yes | line 3: `syntax-error` | ok |  |
| 32 | 10.4 | no | ok | ok |  |
| 33 | 12 | yes | line 5: `syntax-error`; line 7: `syntax-error`; line 12: `syntax-error` | ok |  |
| 34 | 12 | no | ok | ok |  |
| 35 | 15 | yes | line 5: `syntax-error` | ok | the prototype makes the new object counted; Q1 (a) rejects the push |
| 36 | 15 | no | ok | ok |  |
| 37 | 15 | no | ok | line 7: `scoped-reference-escapes` | open question Q3 |
| 38 | 15 | no | ok | ok |  |
| 39 | 15 | no | ok | ok |  |
| 40 | 15 | yes | line 2: `syntax-error` | line 8: `trait-method-signature` | exact match in the prototype; Q6 (a) accepts it |

The full programs behind sections 2 and 9 also ran with the prototype:
U1 to U10 as the `counts` fixtures named in 2.2, and S1 to S8 as scratch
programs whose outputs the text quotes. Their counted variants were
checked with main. Rust, Swift and Go code is in its own fences and was
not compiled.
