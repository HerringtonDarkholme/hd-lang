# Object Model, Tier 1: Design Study

Status: proposal (#426). The spec and the compiler are unchanged.

hd is leaving Wasm GC. Objects will live in Wasm linear memory, managed
by reference counting plus a cycle collector, a runtime that
REFCOUNT_RUNTIME.md prices in a companion study. On top of it hd adopts
Go's object model: every aggregate is a value stored inline, and sharing
comes only from references. There is no `data` any more. A record is a
`struct`, its fields change in place as in Go, and a program shares one
by taking a reference to it, read-only `&T` or writable `&mut T`.

The model has two tiers. **Tier 1**, this study, is Go: every reference
is global, counted, free to escape and free to alias. **Tier 2**, a
separate study, layers opt-in lifetimes on top: a reference may name
what it depends on, `&{a, self.x} T`, and is then uncounted, and a
scoped `&{l} mut T` is exclusive. An unannotated reference is always
tier 1, so tier 1 must stand alone: a complete, safe language that an
agent writes without marks, whose costs tier 2 removes where they
matter. One rule binds both tiers: nothing a caller relies on is
inferred from a function's body; it is written in the header, and the
body is checked against it. Escape analysis may still optimize inside a
body.

Part 1 designs tier 1 from scratch and justifies each choice by the
programs it serves and its run-time cost. It uses an indentation syntax
close to hd's so that Part 2 reads easily; every spelling is a
placeholder, and no code block is checked by a compiler. Part 2 maps the
design onto hd and plans the work in hour-sized jobs. Unit costs that
REFCOUNT_RUNTIME.md will measure are stated as assumptions and marked
*(to be confirmed by REFCOUNT_RUNTIME.md)*.

# Part 1: From Scratch

## 1. What The Model Must Serve

- **Records in bulk.** A million points should be a million values in
  one buffer, not a million objects: most of Rust's lead on
  allocation-heavy code is layout (RUST_MODEL_STUDY.md section 8).
- **Shared mutable objects.** A connection table or a UI's state is
  changed by several holders, each seeing the others' changes.
- **Graphs with cycles.** A tree with parent links is a cycle; the cycle
  collector reclaims it, so programs need not break cycles by hand.
- **Closures that outlive their call**, stored in the objects they
  capture.

Go serves all four with one idea: values by default, pointers when
sharing is meant, and a runtime that keeps alive whatever a pointer
reaches. Its FAQ promises that "from a correctness standpoint, you
don't need to know" whether a variable lives on the stack or the heap.
Tier 1 keeps that promise with counting in place of tracing.

## 2. The Model On One Page

```text
struct Vec2:
    x: f64
    y: f64

struct Body:
    pos: Vec2
    vel: Vec2

fn demo() -> f64:
    let a = Vec2 { x: 1.0, y: 2.0 }
    b := a                              # a copy; nothing is shared
    a.x = 5.0                           # changes a only
    r := &mut Body { pos: a, vel: b }   # a new counted object
    s := r                              # a second reference to it
    s.pos.x = 7.0                       # r sees it: one object
    r.pos.x + b.x                       # 7.0 + 1.0
```

1. Structs, tuples and enums are values without identity, copied on
   assignment, passing and return. So are `List`, `Map` and `string`,
   with copy-on-write.
2. A `let` binding is mutable, fields included; `:=` and parameters are
   not.
3. `&place` refers to a place and `&T { ... }` allocates an object. A
   reference keeps its whole object alive and may go anywhere.
4. `&T` reads; `&mut T` also writes, and is not exclusive.
5. An element reference pins its list: while it lives, the length is
   fixed.
6. Closures share the variables they name, with one call kind.
7. No null: `Option[&T]`. No weak references: the collector takes cycles.

## 3. Values

A **struct** is a named record, a **tuple** an anonymous one whose
elements are fields like a struct's, and an **enum** a tagged union. All
three are values stored inline: a `Body` field is four `f64`s in its
parent, a `List[Body]` is one buffer of `Body`s, and a `Body` local is
four numbers in registers or on the stack.

Copying a value copies its bytes and increments the count of each
counted thing inside it: each string, list, map, reference and closure.
A struct of four `f64`s costs four moves and no count; one holding a
`string` and a `List` also costs two increments, and two decrements when
the copy dies. The compiler removes copies it can prove unobservable (a
copy whose source is never used again is a move), and passes a value
argument by pointer when nothing can change the source during the call
(section 8).

A struct may not contain itself through struct and tuple fields alone;
a recursive type goes through a reference, an `Option[&T]` or a
collection, as in Go and Rust. An enum that contains itself, such as an
expression tree, is boxed implicitly: enum payloads are not places
(section 6.1), so nobody can tell whether a payload is inline or in an
immutable counted box. Such boxes are built bottom-up and never form a
cycle.

**Equality and null.** A struct's `==` comes from `@derive(Eq)` or a
written implementation; values have no `is`. There is no null:
`Option[&T]` costs one pointer, `.None` being the null address.

**Large structs.** A copy is never hidden behind a reference, so a
struct of 40 fields costs 40 moves at every copy, as in C# and Go. An
optional `large-struct` warning at the declaration says so; measurement
sets its threshold.

## 4. References

| Type | Permits | Converts to |
| --- | --- | --- |
| `&T` | reading `T` through it | nothing else |
| `&mut T` | reading and writing `T` through it | `&T`, implicitly |

```text
let p = Vec2 { x: 0.0, y: 0.0 }
r1 := &p                              # refers to p's storage
r2 := &mut p                          # writable; needs a `let`
r3 := &mut Vec2 { x: 1.0, y: 1.0 }    # a new object, count 1
r4 := &make_point()                   # not a place: a new object holding the result
```

1. `&e` and `&mut e` on a place refer to it: a variable, a field of a
   place, a list element, or a place reached through a reference.
2. On any other expression they allocate a new counted object holding
   the value, as Go's `&T{...}` does for a composite literal.
3. `&mut` needs a writable place (section 5). A new object is writable.
4. Field access and method calls dereference automatically: `r.x`,
   `r.len()`. `*r` reads the whole value, a copy; `*r = v` replaces it.

**Identity.** `r is s` is true when both refer to the same place, and
applies only to references. `==` between two references is an error
with two fix-its, `r is s` and `*r == *s`: Go's `==` on pointers
compares addresses and Rust's compares contents, so an agent from either
would guess wrong half the time, and contents can recurse around a
cycle. A derived `Eq` or `Hash`
compares a reference field by identity, as Go's struct `==` compares
pointer fields, and a map keyed by identity uses `Id[T]`, a wrapper
whose `Eq` and `Hash` are the reference's identity.

**Representation.** A reference into the middle of an object keeps the
whole object alive: either a fat reference holds base and offset (one
Wasm `i64`), or a thin address finds its header through the allocator's
size-class page, as Go's does. The companion study's draft picks the
thin address and a header of one 32-bit count, plus a collector word for
types that can be in a cycle; release and trace code come from the
static type, or from the vtable of a `dyn` value *(to be confirmed by
REFCOUNT_RUNTIME.md)*.

## 5. Mutability

hd marks where values change. In tier 1 the marks are few:

| Mark | Meaning |
| --- | --- |
| `let x = ...` | `x` may be reassigned and its fields assigned |
| `x := ...` | `x` never changes, as a whole or in part |
| a parameter, `self` | immutable: a copy or a borrow of the caller's value |
| `&mut e` | a writable reference is made here, visible at the call site |
| `&mut T` in a type | this position may write the object |
| `&mut self` | this method changes its receiver in place |

A place is **writable** when it is a `let` variable or a field of a
writable place, or when it is reached through a `&mut` reference.
Assigning a field, replacing a list element, calling a `&mut self`
method and taking `&mut` all need a writable place:

```text
fn shift(v: Vec2) -> Vec2:
    v.x += 1.0              # error: `v` is a parameter; copy it into a `let` to change it
    let w = v
    w.x += 1.0              # fine: w is the function's own copy
    w

fn shift_in_place(v: &mut Vec2) -> void:
    v.x += 1.0              # fine: writes the caller's value
```

**Read-only is transitive through references.** Reading through `&T`
weakens every `&mut` in the value read to `&`. With
`struct Team { lead: &mut User }` and `t: &Team`, `t.lead` is a `&User`,
and copying it out cannot recover write access; a `List[&mut User]` read
through `&` is a `List[&User]`, with the same layout. Function values
are the exception: a closure read through `&` can still be called and
may change what it captured. Read-only is a permission on paths, not a
freeze of the object graph.

**No exclusivity for global `&mut T`.** Rust needs exclusivity because a
borrow holds no count, so a write through another path could free what
the borrow reads. A counted reference cannot dangle. Swift draws the
same line: it enforces exclusivity for values in variables, but "we
never try to enforce exclusivity of access on the whole object" for a
class instance, because "references can be copied pretty arbitrarily
throughout a program" (SE-0176). What aliasing could still break is
closed by rules that need no exclusivity:

| Hazard | Rule that closes it |
| --- | --- |
| A write changes an enum's variant under a reference into the old payload | payloads are not places; nothing points into one |
| A list grows or is copied under an element reference | the reference pins the list (section 6.3) |
| A field holding a reference is overwritten while a callee reads the old target | the callee's reference counts the old target, which stays alive |
| A value argument changes during the call through an alias | the caller passes a pointer only when no alias can exist, and copies otherwise |
| `*p = *q` where both refer to one object | assignment increments the new contents before releasing the old |

Exclusivity is the price of not counting, so it belongs to tier 2, whose
scoped `&{l} mut T` holds no count and is checked to be exclusive.

## 6. Interior References And Collections

### 6.1 Fields, locals and payloads

`&mut p` on a `let` local gives the local an address: semantically it
is now a cell that lives as long as any reference to it, so returning
`&mut p` is legal and safe; where the cell lives is the compiler's
choice (section 8). `&mut r.pos` with `r: &mut Body` points into `r`'s
object and keeps all of it alive. A struct never moves or resizes, so a
field reference never goes stale; a later `r.pos = v` writes the same
memory, and the reference sees it.

Enum payloads are not places. A payload is read by matching, which
binds copies, and changed by replacing the whole enum value, so an
enum's layout can change between variants under no reference.

### 6.2 Collections are values

`List[T]`, `Map[K, V]` and `string` are values. A list is a small header
that refers to a counted buffer. Copying the list copies the header and
increments the buffer's count; changing a list whose buffer is shared
first copies the buffer, so the change is seen only through that list.
This is Swift's copy-on-write, which needs exactly the count that linear
memory now provides; the test is a load and a compare per change *(to be
confirmed by REFCOUNT_RUNTIME.md)*.

Shared list objects, as in hd today or Python, make every list an
implicit reference and let a callee change a caller's list with no mark.
Go's slices share their array but not their length, so
`b := append(a, x)` aliases `a` or not depending on spare capacity. With
value collections, a shared list is written `&mut List[T]`.

`string` is immutable UTF-8. A string slice is a view that counts the
shared buffer, so a small view keeps a large buffer alive, the retention
cost Go documents for slices; `s.compact()` copies a view out. A list
slice, `xs.slice(a, b)`, likewise shares its buffer until either side
changes.

### 6.3 Element references pin

`xs[i].x = 1.0` and `xs[i] = v` write in place: they make the buffer
unique if it is shared, store, and hold nothing afterwards.

`&xs[i]` and `&mut xs[i]` point into a buffer that moves when the list
grows. Go keeps the old array alive, so writes through an old pointer
silently stop reaching the slice once `append` reallocates, and whether
it reallocates depends on capacity. Tier 1 forbids the move instead:

1. Taking an element reference makes the buffer unique, then **pins**
   it: the reference's count on the buffer is a pin.
2. While a buffer is pinned, an operation that changes the list's
   length or moves its elements (push, insert, remove, clear, truncate,
   reserve) panics with `collection-pinned`. Replacing an element,
   sorting and swapping are allowed; a reference sees whatever is in its
   slot.
3. Copying a list whose buffer is pinned copies the buffer at once, so a
   pinned buffer is never shared, and a reference into one list never
   sees another list's writes.
4. Replacing the whole list, `xs = []`, is allowed: the old buffer lives
   on for its references, which no longer refer to `xs`.
5. A reference is released after its last use, not at the end of its
   block, so the source decides how long a pin lasts, never the
   optimizer.

It is the iterator-invalidation check of Java's collections, extended
to every element reference, and it is the dynamic twin of tier 2, where a scoped element reference freezes the list
statically and holds no count. A buffer's header keeps two counts:
owners (list values sharing it, for copy-on-write) and references
(pins); it is freed when both are zero *(to be confirmed by
REFCOUNT_RUNTIME.md)*.

Maps rehash, so map values are not addressable, as in Go: `m[k] = v`
and `m[k] += 1` work; `m[k].x = v` and `&m[k]` are errors whose fix is
to read, change and store, or to keep `&mut V` values in the map.

### 6.4 Iteration

`for p in xs` iterates `xs` as it was when the loop began. The loop
holds a count on the buffer, so a shared buffer never changes under it,
and `p`, an immutable binding, can alias the element without a copy. A
body that changes `xs` copies it once and the loop goes on over the old
buffer. `for p in &mut xs` binds a writable element reference per step
and pins `xs` for the loop, so a push in its body panics; `for p in &xs`
is the read-only form, for code that keeps the references.

## 7. Methods

```text
impl Vec2:
    fn len(self) -> f64:                  # a copy, or a borrow of the caller's value
        (self.x * self.x + self.y * self.y).sqrt()
    fn translate(&mut self, d: Vec2) -> void:
        self.x += d.x
        self.y += d.y
```

1. `self` is immutable, like every parameter. Writing `self.x` in a
   `self` method is an error that suggests `&mut self`, so Go's lost
   write through a value receiver cannot compile.
2. Calling a `&self` or `&mut self` method on a place takes its address,
   as Go does: `p.translate(d)` means `(&mut p).translate(d)`. No mark
   appears at the call; the method and the `let` binding say that `p`
   changes.
3. Calling a `&mut self` method on what is not a writable place, such as
   a `:=` binding or a call result, is an error.
4. `bodies[i].translate(d)` pins the list for the call.

A trait value is a reference, `&dyn Shape` or `&mut dyn Shape`.
Converting a struct value to one copies it into a new object, as Go
copies a value stored in an interface; converting `&mut Circle` shares
the object.

## 8. Escape Analysis

Escape analysis decides placement and counting. It never changes what a
program means and never appears in an interface. In each body the
compiler decides:

| Question | Decided by |
| --- | --- |
| Does an address-taken `let` live on the stack or in a heap cell? | the stack if every reference to it provably dies before the function returns |
| Is a new object, `&T { ... }`, on the stack? | the same test |
| Which increments and decrements go? | a reference passed on and never used again is moved; one only read in the body is never counted |
| Is a value argument passed by pointer or copied? | by pointer when its source is a local without an address or a temporary |

Three conventions keep the analysis local without inference:

- **Parameters are passed at +0.** The caller keeps every reference
  argument alive during the call; the callee increments only what it
  stores or returns. One rule for every function is part of every header
  without being written (Swift's default "guaranteed" convention).
  Results are returned at +1.
- **A stack object starts with a count of one, held by the frame**, so a
  callee that counts it up and down never frees it. It is placed on the
  stack only when no reference can outlive the frame.
- **Headers are the only facts about callees.** A tier 1 header says
  nothing about escape, so a reference passed to a call that is not
  inlined is assumed to escape. Go infers what each function does with
  its parameters and records it as "parameter tags" for its callers
  (`cmd/compile/internal/escape`); that is the body-to-interface
  inference hd rules out. A caller's fact must come from a header, which
  tier 2 writes.

The cost of that rule is concrete. `p.translate(d)` on a local `p`, with
`translate` in another module and not inlined, puts `p` in a heap cell:
an allocation and a free each time the function runs. Go would keep
`p` on the stack. Three things shrink the cost: small methods are
inlined, after which the reference is local; std's methods carry tier 2
headers that promise not to keep their receivers; and section 18 lists a
further optimization over same-module bodies, still open.

hd compiles modules separately, where Go analyzes a package at a time;
only headers cross a module boundary, so a body edit never changes a
caller's meaning or type check.

**What the programmer sees** is nothing in semantics. For performance,
`hd build --explain-escapes` lists each heap cell and its reason, as
`go build -gcflags=-m` does:

```text
geometry.hd:14: `p` lives in a heap cell: `&mut p` is passed to shapes.Rect.translate,
  whose header lets it keep the reference
  help: a tier 2 receiver `&{} mut self` on `translate` keeps `p` on the stack
```

## 9. Closures

### 9.1 What a closure holds

A closure is a function pointer and an environment: a struct with one
field per captured variable, in a counted object, absent when nothing is
captured. Copying a closure increments the environment's count; a
closure has no identity. Closures share the variables they name with
their scope, as Go specifies: "Those variables are then shared between
the surrounding function and the function literal, and they survive as
long as they are accessible." Swift, Kotlin and JavaScript agree, so it
is what an agent expects.

1. A captured immutable binding (`:=`, a parameter) is copied into the
   environment; it never changes, so copying and sharing look the same.
2. A captured `let` variable is shared: it moves to a counted cell that
   the environment refers to, and either side sees the other's writes.
   Swift boxes captured variables the same way, and may copy a value "if
   that value isn't mutated by a closure, and if the value isn't mutated
   after the closure is created"; tier 1 allows the same optimization.
3. A `for` loop's variable is a new binding on every iteration, Go's 1.22
   fix, so closures made in a loop each see their own iteration.
4. Capturing a reference copies it: the environment counts its object.

### 9.2 One call kind

Rust tells `Fn`, `FnMut` and `FnOnce` apart because calling a closure
that mutates its captures needs exclusive access to it, and one that
moves a capture out can run only once. Neither reason exists in tier 1.
Mutation goes through counted cells and references that may alias, so
calling a mutating closure through two copies at once is as safe as two
`&mut` references to one object; and nothing is moved out of a closure,
since every captured value is copyable or counted.

So tier 1 has one function type, `fn(A) -> B`, callable any number of
times through any copy: Rust's `Fn` plus `'static`, with mutation
through shared cells. Tier 2 adds `FnMut` and `FnOnce` as wider types
for closures over exclusive or non-copyable captures, and a tier 1 `fn`
is accepted wherever they are expected.

### 9.3 Cycles through closures

A callback stored in the object it captures is a cycle:

```text
button := &mut Button { label: "OK", clicks: 0, on_click: .None }
button.on_click = .Some(fn() -> void:
    button.clicks += 1)       # the environment holds a count on `button`
```

Swift leaks this unless the closure writes `[weak self]`. Tier 1 needs
no annotation: once the last outside reference to `button` goes, the
collector finds the cycle and frees both. Each closure literal's
environment layout is known where it is made, so the compiler emits its
release and trace code, reached from the closure value as a `dyn` value
reaches its vtable.

A type that holds no reference, trait value or function value at any
depth through values can never be in a cycle, and its decrements skip
the collector's candidate buffer. A `fn` field makes its struct a
candidate, since a `fn` type does not say what its closure captured;
tier 2's capture sets can say more.

**No weak references in tier 1.** The collector breaks cycles. For
caches, Go added weak pointers only in Go 1.24, as "a low-level
primitive" for weak maps, and hd can add a `Weak[T]` the same way later.
A `Disposable` caught in a cycle is reclaimed at the collector's next
run, so `using` remains the way to release a resource at a known point.

## 10. Keeping Go's Known Bugs Rare

| Go bug | Go | Tier 1 |
| --- | --- | --- |
| Writing to the loop copy: `for _, v := range xs { v.n++ }` | compiles; the write is lost | `p` is immutable: error, with the fix `for p in &mut xs` |
| Closures sharing one loop variable | fixed in Go 1.22 | per-iteration variables from the start |
| A value receiver that writes `self` | compiles; the write is lost | error, with the fix `&mut self` |
| A pointer method on a non-addressable value | error | error, with the reason |
| An accidental large copy | silent | parameters pass by pointer when safe; `large-struct` warns at the declaration; `--explain-copies` lists copies in loops |
| A pointer into a slice that `append` reallocates | later writes are lost, depending on capacity | the reference pins the list; the push panics with `collection-pinned` |
| `append` aliasing two slices | depends on capacity | lists are values; no aliasing without `&mut` |
| Nil pointer dereference | panic | no null; `Option[&T]` |
| Map element not addressable | error | the same error, with the read-change-store fix |

The compiler also warns when a local element reference is still live at
a push to the same list in one body, the one case of the pin panic
visible within a single function.

## 11. Tour

### 11.1 Points and geometry

```text
struct Vec2:
    x: f64
    y: f64

impl Vec2:
    fn scale(self, k: f64) -> Vec2: Vec2 { x: self.x * k, y: self.y * k }
    fn translate(&mut self, d: Vec2) -> void:
        self.x += d.x
        self.y += d.y

struct Body:
    pos: Vec2
    vel: Vec2

fn step(bodies: &mut List[Body], dt: f64) -> void:
    for b in &mut bodies:              # pins `bodies` for the loop
        b.pos.translate(b.vel.scale(dt))
```

`List[Body]` is one buffer of 32-byte elements, the layout
VALUE_STRUCTS.md could only emulate on Wasm GC with one array per field.
Nothing here is counted except the list's buffer, since `Body` holds no
reference. `b.pos.translate(...)` takes an interior reference that the
loop's pin already covers. The small methods are inlined, so no count
changes per element, the `Vec2` arguments travel in registers, and
nothing is allocated.

### 11.2 A tree with parent links

```text
struct Node:
    name: string
    parent: Option[&mut Node]
    children: List[&mut Node]

fn add_child(parent: &mut Node, name: string) -> &mut Node:
    child := &mut Node { name, parent: .Some(parent), children: [] }
    parent.children.push(child)
    child

fn depth(n: &Node) -> i32:
    match n.parent:                    # read through `&Node`: an Option[&Node]
        .Some(p) => 1 + depth(p)
        .None => 0

fn build() -> i32:
    root := &mut Node { name: "root", parent: .None, children: [] }
    src := add_child(root, "src")
    file := add_child(src, "lexer.hd")
    depth(file)                        # 2
```

Parents and children refer to each other, so the tree is one cycle; when
`build` returns, the collector frees it at its next run, with no `weak`
and no edge chosen as the owner. `Node` holds references, so its objects are
cycle candidates; a `List[Vec2]` in the same program is not. `depth`
takes `&Node`, so code that only reads the tree cannot change it,
however it was built.

### 11.3 A request handler with buffers

```text
struct Response:
    status: i32
    headers: List[Header]
    body: List[u8]

struct Router:
    routes: Map[string, fn(&Request, &mut Response) -> void]

impl Router:
    fn serve(&self, req: &Request, out: &mut Response) -> void:
        out.status = 200
        out.headers.clear()            # keeps capacity: the buffer is unique
        out.body.clear()
        match self.routes.get(req.path):
            .Some(route) => route(req, out)
            .None => out.status = 404

fn connection!(conn: &mut Conn, router: &Router) -> Result[void, NetError]:
    let out = Response { status: 0, headers: [], body: [] }
    loop:
        req := conn.read_request!()?
        router.serve(&req, &mut out)
        conn.write_response!(&out)?
```

One `Response` per connection serves every request, and `clear()` keeps
its buffers' capacity because nothing else shares them. A `serve` that
returned a copy of a scratch `Response` would share the buffers with
each copy, so the next `clear()` would leave them to the copy and
allocate new ones; the copy in the source shows where that happens. `&req` and `&mut out` go
at +0, so serving a request changes no count. If `write_response!` has a
tier 1 header in another module, `out` lives in a heap cell: one
allocation per connection, not per request.

### 11.4 A parser holding slices of its input

```text
@derive(Eq)
enum Kind: Ident; Number; Punct; End

struct Token:
    kind: Kind
    text: string                       # a view into the source

struct Lexer:
    src: string
    pos: usize

impl Lexer:
    fn next(&mut self) -> Token:
        self.pos = skip_spaces(self.src, self.pos)
        start := self.pos
        kind := classify(self.src, start)
        self.pos = token_end(self.src, start, kind)
        Token { kind, text: self.src.slice(start, self.pos) }

fn tokens(src: string) -> List[Token]:
    let lexer = Lexer { src, pos: 0 }
    let out: List[Token] = []
    loop:
        t := lexer.next()
        if t.kind == .End:
            return out
        out.push(t)
```

No byte is copied: each token's text is a view into the source. Each
view costs an increment when made and a decrement when its token dies,
so a million tokens are two million count operations on one hot header,
and keeping one token keeps the whole source alive. Tier 2 helps this
program most: `text: &{src} string` makes the view an uncounted borrow
that cannot outlive the source.

### 11.5 An event emitter whose callbacks capture their owner

```text
struct Emitter:
    handlers: List[fn(Event) -> void]

impl Emitter:
    fn on(&mut self, handler: fn(Event) -> void) -> void:
        self.handlers.push(handler)

    fn emit(&self, e: Event) -> void:
        for h in self.handlers:        # a snapshot: a handler may add handlers
            h(e)

struct ClickCounter:
    clicks: i32
    bus: &mut Emitter

fn attach(bus: &mut Emitter) -> &mut ClickCounter:
    counter := &mut ClickCounter { clicks: 0, bus }
    bus.on(fn(e: Event) -> void:
        if e.name == "click":
            counter.clicks += 1)
    counter
```

The counter refers to the bus, the bus holds the handler, and the
handler's environment holds the counter: a cycle through a closure,
reclaimed by the collector once both are unreachable, with no
`[weak self]`. `emit` iterates a snapshot, so a handler that calls
`bus.on` during an emit is safe: the push copies the shared buffer and
the loop finishes over the old one. The `fn` elements make `Emitter` a
cycle candidate.

## 12. Costs

Run-time costs, with unit prices assumed until REFCOUNT_RUNTIME.md
measures them:

| Operation | Cost in tier 1 | Assumption |
| --- | --- | --- |
| Copy a struct | its bytes, plus an increment per counted field | an increment is a non-atomic load, add and store *(to be confirmed by REFCOUNT_RUNTIME.md)* |
| Pass a reference | nothing, at +0 | |
| Store or return a reference | an increment; later a decrement and a zero test | |
| `&mut T { ... }` | an allocation and an object header | a 4-byte count, plus a collector word for cyclic types *(to be confirmed by REFCOUNT_RUNTIME.md)* |
| `&mut local` passed to a call in another module, not inlined | the local moves to a heap cell | |
| Interior reference | a fat `i64`, or a header lookup per count operation | *(to be confirmed by REFCOUNT_RUNTIME.md)* |
| Change a list or map | a uniqueness test; a full copy if the buffer is shared | |
| Element reference | the buffer made unique, a pin increment, later a decrement | a second 4-byte count per buffer header *(to be confirmed by REFCOUNT_RUNTIME.md)* |
| Push, insert, remove | a pin test: a load and a branch | |
| A closure literal with captures, a captured `let` that is written | an environment object, a cell | none, when the closure is inlined |
| An object of a possibly cyclic type | a candidate entry on a decrement that does not reach zero | collector pauses priced in REFCOUNT_RUNTIME.md |

At compile time, the writable-path check and escape analysis each walk
one body and read only callee headers: no contention, a bit per binding
and per reference, and a graph over one body's locations that is near
linear in practice, as Go's is. Headers carry everything callers use, so
a body edit rechecks only that body.

The surface it adds: two reference types, `&` and `&mut` expressions,
`*` for whole values, `is` on references, `Id[T]` and one panic,
`collection-pinned`. What it removes, `data`, permission views, `let
mut`, readonly edges and part copies, is listed in section 14.

## 13. Where Tier 2 Plugs In

Tier 2 adds a dependency set to reference types and changes nothing in
tier 1's meaning:

| Tier 1 | What tier 2 adds |
| --- | --- |
| every reference is counted | `&{l} T` is uncounted and cannot outlive `l` |
| `&mut T` aliases freely | `&{l} mut T` is exclusive, checked statically |
| an element reference pins at run time | a scoped element reference freezes the list statically, with no count |
| a header says nothing about escape, so `&mut local` may go to the heap | a header taking `&{} mut T` promises not to keep it, so the local stays on the stack |
| one closure kind, `fn` | capture sets on closure types; `FnMut` and `FnOnce` for scoped or non-copyable captures |
| a string view is counted | `&{src} string` is a borrowed view |
| a `fn` field makes its struct a cycle candidate | a capture set tells the collector what a closure can reach |

A global reference converts to any scoped one, as Rust's `'static`
outlives every lifetime, so a library written with tier 2 headers serves
tier 1 callers unchanged.

# Part 2: Retrofit

## 14. What Survives, Changes And Goes

| hd today | Under tier 1 |
| --- | --- |
| `data D`, a shared object with identity | `struct D`, a value; sharing is `&D` or `&mut D` |
| `mut D` and readonly `D` views | `&mut D` and `&D`, or `D` as a value |
| `let mut x = D { ... }` | `let x = D { ... }`; `x := &mut D { ... }` to share |
| `:=` gives a readonly view | `:=` gives an immutable binding |
| `mut self`, `self` | `&mut self`; `self` (a value) or `&self` |
| a field `f: mut U` of a data type `U` | `f: &mut U` |
| a field `f: U`, a readonly edge | `f: U` inline, or `f: &U` to share |
| `List` and `Map` are shared objects; `mut List[T]` | values with copy-on-write; `&mut List[T]` to share |
| a length change panics an iterator | `for x in xs` iterates a snapshot; element references and `for x in &mut xs` pin |
| tuples have no places | tuples are anonymous structs; a `let` tuple's elements are places |
| enums are immutable values; self-recursive payloads boxed | unchanged |
| `is` on `AnyRef` values | `is` on references only; `AnyRef` means reference types |
| embedding with `...` part copies | an embedded struct is an inline field with promotion; every copy copies, so the markers go |
| closures share captured storage; one `fn` type, no `mut` form | unchanged, plus per-iteration loop variables |
| `string`, an immutable view | unchanged |
| the Wasm GC backend | linear memory with counts (REFCOUNT_RUNTIME.md) |

The spec loses `data` declarations, permission views on composites,
freshness rules, `let mut` with `mut-on-primitive`, `mut-on-tuple`,
`let-mut-readonly-type` and `redundant-let-mut`, `readonly-edge`,
`embedded-copy-required`, `copy-into-ordinary-field`,
`old-struct-declaration`, and `mut-on-type-parameter`, since `&mut T` on
a type parameter now means something. `mutable-upgrade` stays for
`&T` to `&mut T`, and `readonly-root` and `mutable-receiver-required`
become one error: a write to a place that is not writable.

VALUE_STRUCTS.md is superseded where it assumed Wasm GC (immutable
fields, `struct` next to `data`, one array per field); its measurements
and its table of std types that should be values carry over. The removed
`^` mode (chapter 13, deleted in R4) survives in two ideas: lending
marks nothing at the call site, which tier 1's auto-address keeps, and
"stays in this call", which tier 2's dependency sets make precise.

## 15. Migration

**A conservative rewrite keeps meaning.** `hd fix
--object-model` turns every composite of today into a reference:

1. `data D` becomes `struct D`.
2. A type `D` becomes `&D` and `mut D` becomes `&mut D`; `List[T]` and
   `Map[K, V]` positions become `&List[T]` or `&mut List[T]`, so a list
   shared today stays shared.
3. A literal `D { ... }` becomes `&mut D { ... }`, a copy-update
   `D { ...d, x }` becomes `&mut D { ...*d, x }`, and a list literal used
   as `mut` becomes `&mut [...]`.
4. `mut self` becomes `&mut self`, `self` on a former data type becomes
   `&self`, and `let mut` becomes `let`.
5. `a == b` on two former data values becomes `*a == *b`.

The result behaves as today, except that a loop which today panics on a
length change now iterates a snapshot, and allocates about as today, one
object per former data value: a starting point, not a destination.

**Cleanup turns records into values.** A struct never shared or changed
through two names loses its `&`, and a `&List[T]` parameter that is only
read becomes `List[T]`; the tool lists types never compared with `is`
and never written through a stored reference. Things that are
interchangeable are values; things that change while others watch sit
behind `&mut`.

**std is migrated by hand**, since its layout is everyone's speed:
records become values, builders and handles stay behind references, and
borrowing methods get tier 2 headers once tier 2 lands.

## 16. Implementation Plan

Each job is about an hour, ships its spec rules and conformance
fixtures, and keeps every check green. The language lands first, on the
current backend; the backend moves after, in the jobs of
REFCOUNT_RUNTIME.md. `data` stays accepted next to `struct` until job
18. Run fixtures of jobs 2 to 10 that the current backend cannot run yet
enter `KNOWN_FAILURES.tsv`, one issue each, and jobs 11 and 12 remove
them. A std migration chunk that runs long splits into further jobs.

| # | Job | Spec and fixtures |
| --- | --- | --- |
| 1 | Grammar: `&T` and `&mut T` types, `&e`, `&mut e`, `&T { ... }`, prefix `*`; parser | a "Values And References" chapter skeleton and grammar productions; parse fixtures, valid and invalid |
| 2 | `struct` values with mutable fields: literals, copies, field writes on `let`, immutable `:=` and parameters | struct and copy rules; check and run fixtures, one per error |
| 3 | References in the checker: forming them on places and non-places, writable places, `&mut` to `&`, transitive weakening, auto-dereference, `*r` | reference and writable-path rules; check fixtures per error, with fix-its |
| 4 | Identity and equality: `is` on references only, `==` between references rejected, derived `Eq` and `Hash` by identity on reference fields, `Id[T]`, `Option[&T]` | identity rules; check and run fixtures |
| 5 | Methods: `self`, `&self`, `&mut self`, auto-address, non-writable receivers, method sets in trait implementations | receiver rules; a fixture per error |
| 6 | Trait values as `&dyn` and `&mut dyn`, boxing a value into an object, `AnyVal` and `AnyRef` redefined | trait value rules; fixtures |
| 7 | Recursive types: the struct cycle error, implicit boxing of self-recursive enums, payloads not places | recursion rules; an invalid fixture per error |
| 8 | Lists and maps as copy-on-write values: element places, map values not addressable, snapshot `for` | std collection rules and `for` rules; run fixtures |
| 9 | Element references and pins: `&xs[i]`, `for p in &mut xs`, `collection-pinned`, eager copy of a pinned list, release at last use | pin rules; a run fixture for each operation that panics and each that does not |
| 10 | Closures: capture rules restated for values and cells, per-iteration loop variables | capture rules; run fixtures |
| 11 | Bridge on Wasm GC, part 1: struct values as exclusively owned GC objects copied on copy; `&` and `&mut` to struct-typed places | the run fixtures of jobs 2 to 7 pass |
| 12 | Bridge, part 2: element references with a pin count; references to scalar fields as boxed cells | the run fixtures of jobs 8 to 10 pass, or `KNOWN_FAILURES.tsv` rows with an issue each |
| 13 | `hd fix --object-model`, the conservative rewrite | before-and-after fixture pairs; rewritten examples run unchanged |
| 14 | std migration 1: collections, iterators, `Option` and `Result` helpers | std rules that name `data` or `mut` views; fixtures |
| 15 | std migration 2: text, formatting, encoding, hashing, time, with values per VALUE_STRUCTS.md section 8 | std rules; fixtures |
| 16 | std migration 3: I/O, streams, net, processes, with handles behind references | std rules; fixtures |
| 17 | `examples/`, the guide and the website: section 11's programs in place of the `data` examples | none |
| 18 | Remove `data`, permission views, freshness rules, `let mut`, readonly edges and part copies from spec and checker | removed rules and fixtures; a rule inventory diff |
| 19 | After the RC backend: stack placement of address-taken locals and new objects within one body | none (unobservable); allocation counts on the tour programs |
| 20 | After the RC backend: `hd build --explain-escapes` and `--explain-copies`, and the `large-struct` warning | CLI rules; fixtures for the report format |

Jobs 1 to 10 are the language; 11 and 12 let it run before the backend
moves (question 5); 13 to 18 migrate; 19 and 20 follow the backend.

## 17. Questions For The Owner

1. **Should global `&mut T` be checked for exclusivity?** A counted
   reference cannot dangle, and the remaining hazards close without
   exclusivity: no references into enum payloads, pins on collections,
   copies of aliased value arguments. Swift likewise never enforces
   exclusivity on a whole class instance. Recommendation: no;
   exclusivity belongs to tier 2's uncounted scoped references.
2. **Should `List`, `Map` and `string` be values with copy-on-write?**
   The alternatives are shared list objects, as in hd today, or Go's
   slices, which share an array but not a length. Recommendation: values
   with copy-on-write, as in Swift; a shared list is written
   `&mut List[T]`, and only a shared buffer is ever copied.
3. **What happens when a list grows under an element reference?** Go
   keeps the old array alive, and later writes silently miss the list,
   depending on capacity. The alternative is the pin: changing the
   length of a list with a live element reference panics.
   Recommendation: the pin. Its panic is deterministic because
   references are released at their last use, and tier 2 makes the same
   rule a static check.
4. **What does `==` mean between references?** Recommendation: an error
   with two fix-its, `is` for identity and `*a == *b` for contents;
   derived `Eq` and `Hash` compare reference fields by identity, as Go's
   struct `==` does, and identity-keyed maps use `Id[T]`.
5. **Should the new semantics run on the Wasm GC backend before the RC
   backend exists?** Struct values can be emulated there as GC objects
   copied on every copy: slow but correct. The language, std and
   examples then migrate with every fixture running, and the backend
   moves under a green suite. Recommendation: yes, jobs 11 and 12; the
   alternative is check-only fixtures and a long known-failures list
   until the RC backend lands.

## 18. Open Points

- **Same-module escape summaries.** The optimizer could use callee
  bodies within one module to keep `&mut local` on the stack, as Go does
  per package, with no change to headers, types or errors; a body edit
  would then recompile same-module callers. Recommendation: decide after
  measuring how much tier 2 headers on std already remove.
- **Whole-value access**: `*r` as in Go and Rust, or `r.*` as in Zig,
  since `*` also multiplies.
- **Map value references** that pin a map against rehashing, as element
  references pin a list.
- **`Weak[T]`** for caches, once a program needs one.

## Sources

- Go: [spec](https://go.dev/ref/spec) (composite literals, function
  literals, method sets, address operators), [FAQ](https://go.dev/doc/faq)
  (heap or stack; methods on values or pointers),
  [loop variables in 1.22](https://go.dev/blog/loopvar-preview),
  [slices](https://go.dev/blog/slices-intro),
  [escape analysis](https://github.com/golang/go/blob/master/src/cmd/compile/internal/escape/escape.go),
  [Go 1.24 notes](https://go.dev/doc/go1.24) (the `weak` package).
- Swift: [ARC](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/automaticreferencecounting/),
  [Closures](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/closures/),
  [Memory Safety](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/memorysafety/),
  [SE-0176](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0176-enforce-exclusive-access-to-memory.md)
  (exclusivity), [SE-0446](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0446-non-escapable.md)
  (nonescapable types, for tier 2).
- Rust: [Closures](https://doc.rust-lang.org/book/ch13-01-closures.html),
  [RefCell](https://doc.rust-lang.org/book/ch15-05-interior-mutability.html).
  Kotlin: [Lambdas](https://kotlinlang.org/docs/lambdas.html#closures).
- C#: [ref struct](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/builtin-types/ref-struct),
  [structs](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/builtin-types/struct).
  Scala 3: [capture checking](https://docs.scala-lang.org/scala3/reference/experimental/cc.html).
  OxCaml: [stack allocation](https://oxcaml.org/documentation/stack-allocation/intro/).
  D: [scope parameters](https://dlang.org/spec/function.html#scope-parameters).
  Hylo: Racordon et al., [Implementation Strategies for Mutable Value Semantics](https://www.jot.fm/issues/issue_2022_02/article2.pdf), JOT 2022.
- hd: [VALUE_STRUCTS.md](VALUE_STRUCTS.md), [RUST_MODEL_STUDY.md](RUST_MODEL_STUDY.md),
  [PARALLELISM_AND_LOCALITY.md](PARALLELISM_AND_LOCALITY.md), REFCOUNT_RUNTIME.md,
  the removed chapter 13 (`git show 5d0b21e0a^:spec/lang/13-ownership.md`),
  [04-type-system.md](../../spec/lang/04-type-system.md),
  [07-functions.md](../../spec/lang/07-functions.md),
  [08-data-and-enums.md](../../spec/lang/08-data-and-enums.md).
