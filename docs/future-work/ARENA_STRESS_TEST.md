# Arena Stress Test

**Status:** design review, 2026-11-23; changes no decision, design
record, spec text, or compiler code. Tests [ARENA.md](ARENA.md)'s owner
API, `Arena::alloc[T](&self, val: T) -> &mut T` (owner, 2026-11-23, on
[#2326](https://github.com/HerringtonDarkholme/hd-lang/pull/2326)),
against [`ref.excl.call`](../../spec/lang/13-values-and-references.md#r-ref.excl.call).

## Contents

1. Surface Being Tested
2. Method
3. Summary
4. Cases
5. Comparison With Rust
6. Problems, Ranked
7. Parse Log

## Surface Being Tested

Every case starts from a stub `Arena` (checking reads only signatures):

```text
pub struct Arena:
    state: usize

impl !Copy for Arena

impl Arena:
    pub fn new() -> Arena:
        Arena { state: 0 }

    pub fn alloc[T < ?Copy](&self, val: T) -> &mut T:
        panic("todo")

    pub fn reset(&mut self) -> void:
        pass
```

Variants, by `alloc`'s result and the checker:

- **M+**: `&mut T` with the proposed shared hold of a `&Arena` (the
  `@interior` type marker, or the inert rule, which give every caller the
  same loan; the inert-rule prototype ran the checks). A `@disjoint`
  method attribute, also prototyped, checks cases 1, 2, 4 and 5 alike and
  fails case 3: `parse`'s own `&mut` result locks the arena again.
- **M0**: `&mut T` on today's checker (main at 363420be2).
- **R**: `&T` (the first draft).
- **H**: `*mut T`, a counted handle; `alloc` is `box(val)`.

## Method

Five cases cover what arena code does after `alloc`: link a child into
its parent, close a cycle, patch nodes after the parse, intern, and churn
trees. Each case was checked (`hd check`), not just parsed, in each
variant. Verdicts: *works*; *friction* (checks after a change a user
would see); *breaks* (no change keeps the arena's point). "Counts back"
names the values that had to become counted, the cost an arena removes.

## Summary

| Case | Library shape | M+ | M0 | R | H |
| --- | --- | --- | --- | --- | --- |
| 1 tree linked after allocation | dodrio, html5ever into bumpalo | works | breaks: `conflicting-access` | friction: `children` becomes `*mut List`, counted | friction: every node counted |
| 2 graph with cycles | typed-arena's "safe cycles" | friction: `roads` becomes `*mut List` and nodes bind as `&City` | breaks | friction: `roads` `*mut` | friction: edges count, a count cycle that leaks |
| 3 parser patching nodes | rustc's resolver writing a typed arena's nodes | works | breaks | friction: `ty` becomes `*mut string`, counted | friction: every node counted |
| 4 interner | rustc's symbol table, bumpalo `alloc_str` | friction: the text is a counted `string`, no `alloc_str` | works | works | friction |
| 5 `binary_trees` | benchmarks game | works | works | works | friction: today's counts, 297 ms |

M+ works or has the friction Rust has (a cycle needs interior mutability,
Rust's `Cell`); the others give counts back on every case that writes.

## Cases

### 1. A tree linked after allocation

```text
# Case 1: children linked after the parent is allocated (a DOM builder,
# as dodrio and html5ever build into bumpalo).
pub struct Element['a]:
    pub tag: string
    pub children: List[&'a Element['a]]

pub fn list['a](arena: &'a Arena, items: &List[string]) -> &'a Element['a]:
    ul := arena.alloc(Element { tag: "ul", children: [] })
    for item in items:
        li := arena.alloc(Element { tag: "li $item", children: [] })
        ul.children.push(li)
    ul

pub fn count(e: &Element) -> usize:
    let mut n: usize = 1
    for c in e.children:
        n += count(c)
    n
```

M+ checks. M0: `conflicting-access` at the child's `alloc`, since `ul`
holds `&mut arena`. R: `ul.children.push` is `not-writable`; `children`
must become `*mut List[...]`, which counts. H: every `Element` is a box.

### 2. A graph with cycles

```text
# Case 2: a graph with cycles, typed-arena's "safe cycles" (a road map).
pub struct City['a]:
    pub name: string
    pub roads: List[&'a City['a]]

pub fn map_size(arena: &Arena) -> usize:
    a := arena.alloc(City { name: "Ames", roads: [] })
    b := arena.alloc(City { name: "Boone", roads: [] })
    a.roads.push(b)
    b.roads.push(a)
    a.roads.len() + b.roads.len()
```

Every variant rejects this. M+: `b.roads.push(a)` is `conflicting-access`,
since `a.roads` holds `b` shared. It checks once `roads` is a `*mut` and
the cities bind shared, as Rust's typed-arena needs `Cell` for cycles:

```text
# Case 2: a graph with cycles, typed-arena's "safe cycles" (a road map).
pub struct City['a]:
    pub name: string
    pub roads: *mut List[&'a City['a]]

pub fn map_size(arena: &Arena) -> usize:
    let a: &City = arena.alloc(City { name: "Ames", roads: box([]) })
    let b: &City = arena.alloc(City { name: "Boone", roads: box([]) })
    a.roads.push(b)
    b.roads.push(a)
    a.roads.len() + b.roads.len()
```

H checks with `roads: List[*mut City]`, but the edges count: a -> b -> a
is a count cycle, collected only by the cycle collector.

### 3. A parser that patches its nodes

```text
# Case 3: a parser that patches nodes after creation: parse, then a pass
# writes each node's type, as rustc's resolver fills a typed arena's nodes.
pub struct Expr['a]:
    pub word: string
    pub left: (&'a mut Expr['a])?
    pub right: (&'a mut Expr['a])?
    pub ty: string

pub fn parse['a](arena: &'a Arena, words: &List[string], at: &mut usize) -> &'a mut Expr['a]:
    word := words[*at]
    *at += 1
    if word == "+":
        left := parse(arena, words, at)
        right := parse(arena, words, at)
        return arena.alloc(Expr { word: word, left: .Some(left), right: .Some(right), ty: "" })
    arena.alloc(Expr { word: word, left: .None, right: .None, ty: "" })

pub fn annotate(e: &mut Expr) -> void:
    match &mut e.left:
        .Some(l) => annotate(*l)
        .None => pass
    match &mut e.right:
        .Some(r) => annotate(*r)
        .None => pass
    e.ty = "i64"

pub fn run(source: string) -> string:
    arena := Arena::new()
    let mut at: usize = 0
    words := source.split(" ")
    tree := parse(&arena, &words, &mut at)
    annotate(tree)
    tree.ty
```

M+ checks: children are `&'a mut`, and `annotate` writes every node
through them. M0: `conflicting-access` at the second `parse`. R: the
`&'a mut` children are a `mutable-upgrade`; the R version needs
`ty: *mut string`, one count per node:

```text
# Case 3: a parser that patches nodes after creation: parse, then a pass
# writes each node's type, as rustc's resolver fills a typed arena's nodes.
pub struct Expr['a]:
    pub word: string
    pub left: (&'a Expr['a])?
    pub right: (&'a Expr['a])?
    pub ty: *mut string

pub fn parse['a](arena: &'a Arena, words: &List[string], at: &mut usize) -> &'a Expr['a]:
    word := words[*at]
    *at += 1
    if word == "+":
        left := parse(arena, words, at)
        right := parse(arena, words, at)
        return arena.alloc(Expr { word: word, left: .Some(left), right: .Some(right), ty: box("") })
    arena.alloc(Expr { word: word, left: .None, right: .None, ty: box("") })

pub fn annotate(e: &Expr) -> void:
    match e.left:
        .Some(l) => annotate(l)
        .None => pass
    match e.right:
        .Some(r) => annotate(r)
        .None => pass
    *e.ty = "i64"

pub fn run(source: string) -> string:
    arena := Arena::new()
    let mut at: usize = 0
    words := source.split(" ")
    tree := parse(&arena, &words, &mut at)
    annotate(tree)
    *tree.ty
```

### 4. An interner

```text
# Case 4: an interner, as rustc's symbol table and bumpalo's `alloc_str`.
pub struct Symbol:
    pub text: string
    pub id: u32

pub struct Interner['a]:
    arena: &'a Arena
    ids: Map[string, &'a Symbol]

impl['a] Interner['a]:
    pub fn intern(&mut self, text: string) -> &'a Symbol:
        match self.ids.get(text):
            .Some(found) => return found
            .None => pass
        sym := self.arena.alloc(Symbol { text: text, id: u32(self.ids.len()) })
        self.ids[text] = sym
        sym
```

Checks in every variant: interned symbols are read-only. bumpalo's
`alloc_str` and slices have no hd form: `string` and `List` are counted
built-ins, so the text keeps one counted block per distinct string.

### 5. `binary_trees`

```text
# Case 5: binary_trees, from the benchmarks game.
struct Node['a]:
    left: (&'a Node['a])?
    right: (&'a Node['a])?

fn make['a](arena: &'a Arena, depth: i32) -> &'a Node['a]:
    if depth == 0:
        return arena.alloc(Node { left: .None, right: .None })
    l := make(arena, depth - 1)
    r := make(arena, depth - 1)
    arena.alloc(Node { left: .Some(l), right: .Some(r) })

fn check(node: &Node) -> i64:
    match (node.left, node.right):
        (.Some(l), .Some(r)) => 1 + check(l) + check(r)
        _ => 1

pub fn rounds(depth: i32, n: usize) -> i64:
    let mut scratch = Arena::new()
    let total: i64 = 0
    for _ in 0..n:
        total += check(make(&scratch, depth))
        scratch.reset()
    total
```

Checks in every variant: nodes are written once, then read. Measured in
[ARENA.md](ARENA.md#problems-it-solves): 297 ms today, 197 ms with the
untyped arena. The H form is today's program:

```text
# Case 5 with counted handles: `alloc` returns `*mut T`, so links count.
struct Node:
    left: (*mut Node)?
    right: (*mut Node)?

fn make(arena: &Arena, depth: i32) -> *mut Node:
    if depth == 0:
        return arena.alloc(Node { left: .None, right: .None })
    l := make(arena, depth - 1)
    r := make(arena, depth - 1)
    arena.alloc(Node { left: .Some(l), right: .Some(r) })

fn check(node: *mut Node) -> i64:
    match (node.left, node.right):
        (.Some(l), .Some(r)) => 1 + check(l) + check(r)
        _ => 1
```

## Comparison With Rust

| Case | Rust (typed-arena or bumpalo) | hd M+ |
| --- | --- | --- |
| 1 | `parent.children.push(child)` through `&mut` | the same |
| 2 | `Cell<Option<&'a Node>>` fields | `*mut` edge lists, shared bindings |
| 3 | `&'a mut` children, patched in a pass | the same |
| 4 | `bump.alloc_str(s)` | a counted `string` per symbol |
| 5 | `arena.alloc(Node { .. })` | the same |

## Problems, Ranked

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | AR1 | A `&mut` result locks its receiver | breaks | 1, 2, 3 (M0) |
| 2 | AR2 | `&T` results move counts into every written field | friction, silent cost | 1, 2, 3 (R) |
| 3 | AR3 | A cycle needs a counted edge list | friction | 2 (M+, R) |
| 4 | AR4 | No arena strings or slices | friction | 4 |

### AR1. A `&mut` result locks its receiver

**Effect.** Cases 1 to 3 fail on today's checker at the second `alloc`
(`ref.excl.call`'s last sentence).

**Resolved (2026-11-24).** Neither candidate was needed: the owner
removed the caller-side upgrade from `ref.excl.call` and made a reference
through a `*T` local (`ref.local.pointer`). The text below is the record.

**Candidates.** Should std mark the `Arena` type `@interior`, as
`UnsafeCell` marks a field? Or mark only `alloc` `@disjoint`, which case 3
shows is not enough? Or should every inert
receiver keep its loan shared, with no name?

### AR2. `&T` results move counts into every written field

**Effect.** With `&T` (R), cases 1 to 3 check only after each written
part becomes `*mut`: a count per value, the cost the arena exists to
remove. Nothing reports it.

**Candidates.** Drop the `&T` form for `&mut T` (ARENA.md's proposal)?

### AR3. A cycle needs a counted edge list

**Effect.** Case 2: closing a cycle writes a node another node already
holds shared. Rust needs `Cell` there too.

**Candidates.** Accept it, as Rust does? Or add a std `Cell[T]` for
copyable fields, which is interior mutability over a scalar slot?

### AR4. No arena strings or slices

**Effect.** Case 4: `alloc_str` and `alloc_slice_copy` have no form; the
text stays a counted block.

**Candidates.** Accept it (strings are counted built-ins, owner
2026-11-12)? Or add an arena-backed string view later, with its own
design?

## Parse Log

Every `text` block above parses (`scripts/hd-conformance.sh parse`), and
each case checks as the Summary says, with the stub prepended.
