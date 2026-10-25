# Local References: Final Design And Implementation Plan

Status: step 1 in progress (#834). The owner cut the first step to two
changes: today's reference is respelled `*T` / `*mut T`, and `&T` /
`&mut T` becomes non-escaping, as a checker rule only, with counting
and codegen unchanged. Step 1's spec (slice 1, a30e29b9d) and parser
(slice 2, d5fcdd786) are on main; the respelling and the escape check
are next. Everything else below (uncounted local references, the
counting rules, overlap, `&fn`, interpolation, frame slots) is a later
step and needs its own decision. The design is in git history at
`591ee34a9`; this document will fold into
[Object Model, Tier 2](OBJECT_MODEL_TIER2.md).

**Step 1.** The owner cut the first step to two changes
([#834](https://github.com/HerringtonDarkholme/hd-lang/issues/834)):
today's reference is spelled `*T`, and `&T` is a non-escaping reference
that is still counted. Its spec is in
[Local References](../../spec/lang/13-values-and-references.md#local-references).
Everything else here is a later step.

## Contents

1. [The Design](#1-the-design)
2. [The Rules](#2-the-rules)
3. [Error Messages And Fix-Its](#3-error-messages-and-fix-its)
4. [Counting Rules](#4-counting-rules)
5. [Escape Hatches](#5-escape-hatches)
6. [Architecture](#6-architecture)
7. [Cost](#7-cost)
8. [Implementation Plan](#8-implementation-plan)
9. [Decisions](#9-decisions)
10. [Prior Art](#10-prior-art)
11. [Sources And Parse Log](#11-sources-and-parse-log)

## 1. The Design

hd gets two kinds of reference. A **counted** reference, `*T` or
`*mut T`, is Go's pointer to a whole object. It may be stored, returned,
captured and used as a type argument. A **local** reference, `&T` or
`&mut T`, is Swift's `inout`: a parameter, a call argument or a
receiver, and never counted.

`&x` takes an address, and the expected type picks its kind. `*p`
dereferences either kind. A field or an element has only a local
reference. So every counted reference points at an object's start, and
every count takes the fast path, with no `block_of`.

The design is sound with no lifetimes and no flow analysis. A local
reference lives for one call. A compile-time check rejects an argument
that may replace what another argument of the call borrows. A borrow
reached through a heap alias takes one fast-path count of its object.

| | Counted: `*T`, `*mut T` | Local: `&T`, `&mut T` |
| --- | --- | --- |
| refers to | a whole object: a local, a value parameter, the referent of a `*T`, a new object | any place: a whole object, a field, an element, a map value |
| counted | yes, always on the fast path | never |
| written as a type | anywhere a type may be | a parameter's type only, a function type's parameter included |
| as a value | anywhere | a call argument, a receiver, or read and written through |
| stored, returned, bound, captured, a type argument | yes | no; a returned `self` and a closure passed to `&fn` are the two exceptions |
| made by | `&x` where a `*T`, a type variable or no type is expected | `&x` where a `&T` is expected, and every `&self` or `&mut self` call |
| receivers | `*self`, `*mut self`: methods that keep `self` | `&self`, `&mut self` |
| like | Go's `*T` | Swift's `inout` |

What it gives: a call that borrows a local, a field or an element of a
local list takes no count. Borrows behind a heap alias, and every count
of a `*T`, take the fast path. What it gives up: local results other
than `self`, bound aliases of fields and elements, local type arguments,
and in-place edits through a returned reference. [Escape
Hatches](#5-escape-hatches) shows what to write instead.

## 2. The Rules

The rules are numbered for this document; each carries the rule ID that
slices 1 and 2 give it in the spec. Comments in the examples name the
rule a line shows. Lines that write `*T`, `*self` or `&fn`, which today's
parser rejects, end in `# hypothetical syntax`.

### 2.1 Two kinds

1. `ref.kind.counted` `*T` and `*mut T` are counted references. A
   counted reference always refers to a whole object.
2. `ref.kind.local` `&T` and `&mut T` are local references. A local
   reference may refer to any place, and is never counted.
3. `ref.kind.whole-object` A whole object is a local, a value parameter,
   the referent of a `*T`, or a new object. Every other place is an
   **interior place**: a field, a list element, a map value, or anything
   reached through a `&T`.
4. `ref.kind.interior-local` An interior place has only a local
   reference. So a count of a `*T` always lands on an object's start and
   takes the fast path, with no `block_of`.

```text
struct Body:
    x: f64
    v: f64

struct World:
    bodies: List[*mut Body]  # hypothetical syntax

fn push_x(x: &mut f64, by: f64) -> void:
    *x += by

fn demo(w: &mut World) -> void:
    let b = &mut Body { x: 0.0, v: 1.0 }  # 1, 3: a counted *mut Body to a new object
    w.bodies.push(b)  # 1: a counted reference may be stored
    push_x(&mut b.x, 1.0)  # 2, 4: b.x is an interior place, so the borrow is local
```

### 2.2 Taking an address and lending

5. `ref.expr.address` `&x` and `&mut x` take an address. Where a `&T` is
   expected the result is a local reference, and where a `*T` is
   expected a counted one.
6. `ref.expr.address.counted-default` Where a type variable is expected,
   or no type is, `&x` is counted. So `let a = &s`, `xs.push(&a)` and
   `[&a, &b]` make counted references.
7. `ref.expr.address.whole` A counted `&x` needs a whole object. On an
   interior place it is an error, so `let a = &obj.f` and
   `let a = &xs[i]` are errors. Error: `local-reference-escapes`.
8. `ref.expr.deref` `*p` dereferences either kind. Field reads, method
   calls and patterns read through either kind, as today.
9. `ref.convert.lend` A `*T` converts implicitly to `&T` where a local
   reference is expected, and a `*mut T` to `&mut T` and to `&T`. The
   conversion takes no count.
10. `ref.convert.no-raise` A `&T` never converts to a `*T`. Error:
    `local-reference-escapes`.
11. `ref.convert.weaken` `&mut T` weakens to `&T`, and `*mut T` to `*T`,
    as today. Weakening a local reference takes no count.

```text
struct P:
    n: i64

fn read(p: &P) -> i64:
    p.n

fn keep(p: *P) -> *P:  # hypothetical syntax
    p

fn demo(a: P, b: P) -> i64:
    let mut xs = []
    xs.push(&a)  # 6: T is a type variable, so xs is a List[*P]
    ys := [&a, &b]  # 6: also a List[*P]
    k := keep(&a)  # 5: *P is expected, and a is a whole object
    read(&a) + read(k) + read(xs[0])  # 5: local; 9: a *P lends to &P

fn invalid(p: &P, o: P) -> void:
    let a = &o.n  # 7: error: an interior place has no counted form
    k := keep(p)  # 10: error: a local reference never becomes a *P
```

### 2.3 Where a local reference may appear

12. `ref.local.positions` A local reference type is written only as the
    type of a parameter, a function type's parameter included. A local
    reference value appears only as a call argument, as a receiver, or
    as the operand of a read or a write through it.
13. `ref.local.parameter` A local reference parameter is valid for the
    whole call. The body may read and write through it, and pass it on
    as an argument any number of times, at no cost.
14. `ref.local.no-binding` A local reference is never bound to a name.
    That covers `let`, `:=`, an assignment, and a pattern that binds a
    whole local-reference scrutinee. `_ :=` discards a value and binds
    nothing. Error: `local-reference-escapes`.
15. `ref.local.no-result` A local reference is never a result, except a
    method's returned `self` (rule 21). Error: `local-reference-escapes`.
16. `ref.local.no-store` A local reference is never a field, an element,
    a payload, a tuple element or a top-level binding. Error:
    `local-reference-escapes`.
17. `ref.local.no-type-argument` A local reference type is never a type
    argument, as Swift's `inout` is no type. There is no `List[&T]`,
    `Option[&T]`, `&T?` or `T = &Foo`. Error: `local-reference-escapes`.
18. `flow.for.ref.binding` (**proposed**: #834 does not mention loops)
    `for p in &mut xs` binds a local reference to each element for one
    step. This keeps today's
    [Element Reference Loops](../../spec/lang/06-control-flow.md#element-reference-loops).
    The body uses `p` as it uses a local parameter.

```text
struct Lexer:
    src: string
    pos: usize

impl Lexer:
    fn at_end(&self) -> bool:
        self.pos >= self.src.len()

    fn skip_space(&mut self) -> void:
        while !self.at_end() && self.src[self.pos] == 32u8:  # 13: self passed on, free
            self.pos += 1

fn count_words(lexer: &mut Lexer) -> usize:  # 12: a local parameter
    let n: usize = 0
    while !lexer.at_end():
        lexer.skip_space()  # 13
        n += 1
    n

enum Expr:
    Num(n: i64)
    Neg(inner: *Expr)  # hypothetical syntax

struct Holder:
    lexer: &Lexer  # 16: error: write *Lexer

fn invalid(lexer: &mut Lexer, e: &Expr, all: List[&Lexer]) -> &mut Lexer:  # 15, 17: errors
    q := lexer  # 14: error: a local reference bound to a name
    match e:
        .Num(n) => pass  # a payload binding is a copy, as today
        whole => pass  # 14: error: whole would bind the local reference e
    q

struct Particle:
    x: f64
    v: f64

fn advance(ps: &mut List[Particle], dt: f64) -> void:
    for p in &mut ps:  # 18: one element per step
        p.x += p.v * dt
```

### 2.4 Receivers

19. `fn.method.receiver.local` `&self` and `&mut self` are local
    receivers. So a method call on a field works, as in
    `client.request.header(..)` and `self.items.push(x)`.
20. `fn.method.receiver.counted` A method that keeps `self` takes
    `*self` or `*mut self`, as Go's pointer receivers do. It needs a
    whole object, so a call on an interior place or through a local
    reference, `&dyn` included, is an error. Error:
    `local-reference-escapes`.
21. `fn.method.returns-self` A method with a local receiver may return
    `self`, with the result type `&Self` or `&mut Self`. The result has
    the caller's kind: counted when the receiver is a whole object, and
    local, for the rest of the calling expression, otherwise. Only
    `self` itself is returned this way.

```text
struct Request:
    url: string
    headers: List[(string, string)]

impl Request:
    fn header(&mut self, name: string, value: string) -> &mut Request:  # 21
        self.headers.push((name, value))
        self

    fn register(*mut self, log: &mut List[*mut Request]) -> void:  # hypothetical syntax
        log.push(self)

struct Client:
    request: Request

fn configure(client: &mut Client, token: string, log: &mut List[*mut Request]) -> void:  # hypothetical syntax
    _ := client.request.header("accept", "json").header("auth", token)  # 19, 21: local in this expression
    let mut r = Request { url: "/", headers: [] }
    rr := r.header("a", "b")  # 21: r is a whole object, so rr is a counted *mut Request
    r.register(log)  # 20: valid on a whole object
    client.request.register(log)  # 20: error: client.request is an interior place
```

### 2.5 Overlapping access

22. `ref.access.overlap` Within one call, an argument that may write a
    place conflicts with another argument that borrows a place inside
    it. The receiver counts as an argument. Error: `conflicting-access`,
    with the message "overlapping accesses to `o`".
23. `ref.access.write` An argument may write a place when it takes
    `&mut` of it, or calls a `&mut self` or `*mut self` method on it.
    These are the two rows of today's
    [`ref.elem.conflict`](../../spec/lang/13-values-and-references.md#call-scoped-access)
    table. A place is inside another when the other is a proper prefix
    of it, through fields, elements or `*T` referents.
24. `ref.access.disjoint` Two borrows of one place, or of places neither
    of which is inside the other, do not conflict. `&mut` stays
    non-exclusive otherwise
    ([`ref.write.no-exclusivity`](../../spec/lang/13-values-and-references.md#writing-through-a-reference)).
25. `ref.access.heap` The check follows places from a local or a
    parameter of the calling body. A borrow through a `*T` read out of a
    field or an element is out of its sight. Such a borrow takes one
    fast-path count of its object instead
    ([Counting Rules](#4-counting-rules)), and nothing is checked at run
    time.

```text
struct Obj:
    n: i64

fn add(p: &mut i64, x: i64) -> void:
    *p += x

fn reset(o: &mut (*mut Obj)) -> i64:  # hypothetical syntax
    *o = &mut Obj { n: 0 }
    5

fn both(a: &mut i64, b: &mut i64) -> void:
    *a += 1
    *b += 1

fn demo(i: usize, j: usize) -> void:
    let mut o: *mut Obj = &mut Obj { n: 1 }  # hypothetical syntax
    add(&mut o.n, reset(&mut o))  # 22, 23: error: overlapping accesses to `o`
    x := reset(&mut o)
    add(&mut o.n, x)  # valid: the write ended before the borrow
    let mut xs: List[i64] = [1, 2, 3]
    both(&mut xs[i], &mut xs[j])  # 24: valid, even when i == j
    add(&mut xs[0], xs.remove_at(1))  # 22, 23: error: remove_at writes xs
```

> **Why.** Without rule 22, `reset` could free the object that
> `&mut o.n` points into while `add` still writes through it. Rejecting
> only writes to an enclosing place keeps `swap(&mut xs[i], &mut xs[j])`
> valid, which Swift rejects.

### 2.6 Element borrows

26. `ref.elem.local` `&xs[i]`, `&mut xs[i]`, `&m[k]` and `&mut m[k]` are
    local references, so an element borrow lasts for one call, as today.
27. `ref.elem.access.pin` The borrow pins its collection and takes one
    fast-path count of the buffer while the call runs. A length change
    through an alias panics `collection-pinned`, and a copy of the
    pinned list gets elements of its own
    ([`ref.elem.copy`](../../spec/lang/13-values-and-references.md#element-references)).

```text
fn write_then_read(slot: &mut i64, copy: List[i64]) -> i64:
    *slot = 99
    copy[0]  # 1: the copy has elements of its own

fn demo() -> i64:
    let mut ys: List[i64] = [1, 2]
    write_then_read(&mut ys[0], ys)  # 27: the pin makes the copy eager
```

### 2.7 Closures

28. `fn.type.local` `&fn(A) -> B` is a non-escaping function parameter.
    The callee may only call it: storing, returning, binding or
    capturing it is an error. Error: `local-reference-escapes`.
29. `fn.type.lend` (**proposed**) A plain `fn` value may be passed where
    a `&fn` is expected, as a `*T` lends to `&T`. Without it, every call
    that passes a named function to an eager helper breaks.
30. `fn.closure.local-capture` A closure literal written as the argument
    for a `&fn` parameter may capture local references, `self` of a
    `&self` or `&mut self` method included.
31. `fn.closure.no-local-capture` Any other closure that captures a
    local reference is an error. A plain `fn(A) -> B` stays a storable
    value. Error: `local-reference-escapes`.

```text
struct Scaler:
    k: f64
    total: f64

fn each(xs: &List[f64], f: &fn(f64) -> void) -> void:  # hypothetical syntax
    for x in xs:
        f(x)  # 28: the callee only calls f

impl Scaler:
    fn add_all(&mut self, xs: List[f64]) -> void:
        each(&xs, fn(x: f64) -> void: self.total += x * self.k)  # 30: captures self
        lazy := xs.iter().map(fn(x: f64) -> f64: x * self.k)  # 31: error: map keeps a plain fn
        k := self.k
        later := xs.iter().map(fn(x: f64) -> f64: x * k)  # 31: valid: captures a copy
```

### 2.8 Generics, traits and trait values

32. `trait.dyn.kinds` `&dyn Tr` and `&mut dyn Tr` are local trait value
    references, and `*dyn Tr` and `*mut dyn Tr` counted ones. A `*dyn`
    lends to a `&dyn` by rule 9.
33. `trait.dyn.provider` A provider installed with `$.with` is a
    `*dyn` value, since a fetched provider may be captured
    ([`fn.capture.provider-value`](../../spec/lang/07-functions.md#captures)).
34. `trait.impl.more-local` An impl method may be more local than its
    trait: `&self` or `&T` where the trait says `*self` or `*T`. The
    reverse is an error. Error: `trait-method-signature`.
35. `trait.dyn.counted-receiver` A trait with a `*self` method is still
    usable as `&dyn Tr`. Calling that method through a `&dyn` is rule
    20's error.

A generic parameter such as `xs: &List[T]` is valid: the reference is
local, and `T` is an ordinary type argument. `id(p)` with a local `p`
would make `T = &P`, which is rule 17's error.

```text
trait Shape:
    fn area(&self) -> f64

trait Registry:
    fn register(*mut self) -> void  # hypothetical syntax

struct Sq:
    side: f64

impl Shape for Sq:
    fn area(&self) -> f64:
        self.side * self.side

impl Registry for Sq:
    fn register(&mut self) -> void:  # 34: more local than the trait
        self.side += 0.0

fn total(shapes: &List[*dyn Shape]) -> f64:  # hypothetical syntax
    let sum = 0.0
    for s in shapes:
        sum += s.area()  # 32: a *dyn Shape lends to &dyn Shape
    sum

fn id[T](x: T) -> T:
    x

fn twice(p: &i64) -> i64:
    *id(p)  # 17: error: T = &i64
```

### 2.9 Suspension

36. `req.suspend.local-arguments` A bang call `f!(..)` may pass local
    references. They stay valid across any number of suspensions,
    because the caller is suspended inside the call.
37. `req.suspend.cold.local` A plain call of a bang function builds a
    cold suspension that keeps its arguments
    ([`req.suspend.cold.captures`](../../spec/lang/11-requirements-and-suspension.md)).
    A local reference argument there is an error. Error:
    `local-reference-escapes`.

```text
use std.time.{Clock, sleep, ms}

fn bump!(p: &mut i64) -> void $ Clock:
    sleep!(ms(1))
    *p += 1

fn outer!(n: i64) -> i64 $ Clock:
    let mut local: i64 = n
    bump!(&mut local)  # 36: valid across the sleep
    local

fn later(p: &mut i64) -> mut Suspend[void] $ Clock:
    bump(p)  # 37: error: the suspension would keep p
```

### 2.10 Boundaries

38. `ref.local.boundary` (**proposed**: a review finding #834 does not
    decide) An exported or host-imported function takes no local
    reference parameter. The host may keep what it receives, and
    `boundary.rs` counts each reference that crosses. Write `*T` or a
    value. Error: `local-reference-escapes`.

```text
pub fn store_name(name: &string) -> void:  # 38: error if exported to the host; write *string
    pass
```

### 2.11 std

39. `std.ref.parameters` Every std reference parameter is `&T` or
    `&mut T`, except the about 17 that keep their argument, which are
    `*mut T`. std's 19 reference-typed fields and its returned
    references are `*T`. Its methods that keep `self` take `*self`.
40. `std.fn.eager` std's eager helpers take `&fn`, and its lazy adapters,
    such as `Iterator.map`, keep a plain `fn`. #834 names `fold`,
    `retain`, `sort_by`, `any` and `each`; in today's std the eager ones
    include `List.map`, `filter`, `any`, `all`, `find`, `sorted_by` and
    `Iterator.fold`.
41. `std.format.debug-writer` `debug_struct`, `debug_tuple`, `debug_list`
    and `debug_map` return the writer itself, `&mut DebugWriter`, under
    rule 21. The open struct or list state lives in `DebugWriter`, so no
    `debug` signature changes.
42. `std.text.template` The `r` prefix takes `Template[*dyn Display]`. A
    part that is not a whole object, such as `${p.name}`, is copied into
    a new object. That costs one allocation per such part, until #702's
    frame storage removes it.
43. `std.error.erased` The erased error is `Result[T, *dyn Error]`, since
    `&dyn Error` would be a type argument. `?` already copies an error
    value into a new object, so errors lose nothing but spelling.

Merging the four builders into `DebugWriter` puts two pairs of
same-named methods on one type. The struct's `field(name, value)` meets
the tuple's `field(value)`, and the list's `entry(value)` meets the
map's `entry(key, value)`. **Proposed** std naming: `field(name, value)`
for named fields (absorbing `named_field`), `item(value)` for positional
ones, `entry(key, value)` for map pairs, and one `finish()`.

```text
use std.format.DebugWriter

struct Point:
    x: i64
    y: i64

impl Debug for Point:
    fn debug(self, out: &mut DebugWriter) -> void:
        out.debug_struct("Point").field("x", self.x).field("y", self.y).finish()  # 41: unchanged
```

## 3. Error Messages And Fix-Its

`local-reference-escapes` is the one new code. Each situation has its own
message, and fix-its are listed in the order the diagnostic offers them.
**Proposed:** today's `element-reference-outlives-call` retires into it,
since an element borrow is now an ordinary local reference.

| # | Case | Written | Code | Message | Fix-its |
| ---: | --- | --- | --- | --- | --- |
| E1 | a `let` of an interior place (7) | `let a = &obj.f`, `let a = &xs[i]` | `local-reference-escapes` | "`obj.f` is a field, and `let` makes a counted reference, which needs a whole object" | copy: `let a = obj.f`; pass the borrow straight to the call: `bump(&obj.f)`; make the field its own object: `f: *T` |
| E2 | an interior place where a type variable is expected (6, 7) | `xs.push(&o.f)`, `[&o.f]`, `id(&o.f)` | `local-reference-escapes` | "a type parameter takes a counted reference, and `o.f` is a field" | copy into a local, `c := o.f`, then `xs.push(&c)`; make the field `*T` |
| E3 | a local reference where a `*T` is expected (10) | `keep(p)` with `p: &P` and `fn keep(p: *P)` | `local-reference-escapes` | "`p` is a local reference, and `keep` keeps its argument" | make the callee's parameter `&P` when it keeps nothing; copy: `c := *p`, then `keep(&c)`; take `p: *P` here too |
| E4 | a local reference bound to a name (14) | `q := p`, `r = p`, `whole =>` on a `&T` scrutinee | `local-reference-escapes` | "a local reference can't be bound to a name; use `p` itself" | use `p` at each use; copy the value: `q := *p` |
| E5 | a local reference as a result (15) | `fn f(p: &mut P) -> &mut P: p` | `local-reference-escapes` | "a local reference can't be returned; only a method's `self` can" | take and return `*mut P`; return a copy, `-> P` |
| E6 | a local reference type in a field, element, payload or type argument (16, 17) | `r: &T` in a struct, `List[&T]`, `&T?`, `.Some(p)`, `[p]`, `dbg(p)` | `local-reference-escapes` | "a field can't hold a local reference" (or "an element", "a type argument") | in a type, write `*T`; for a value, copy it with `*p` |
| E7 | a `*self` method on an interior place or through a local reference (20, 35) | `client.request.register(log)` | `local-reference-escapes` | "`register` keeps `self`, so it needs a whole object, and `client.request` is a field" | make the field its own object: `request: *mut Request`; make the method `&mut self` when it keeps nothing |
| E8 | a closure capturing a local reference, not passed to a `&fn` (31) | `xs.iter().map(fn(x: f64) -> f64: x * self.k)` | `local-reference-escapes` | "the closure captures `self`, a local reference, and `map` may keep the closure" | copy the field first: `k := self.k`; make the callee's parameter `&fn` when it only calls it; write a loop |
| E9 | a `&fn` parameter kept (28) | `self.handler = f` with `f: &fn(i64) -> void` | `local-reference-escapes` | "`f` is a `&fn` parameter, which can only be called" | make the parameter a plain `fn` |
| E10 | a plain call of a bang function with a local argument (37) | `fill(b)` with `b: &mut Buf` | `local-reference-escapes` | "a plain call of `fill!` builds a suspension that keeps `b`" | call it: `fill!(b)`; take `b: *mut Buf` |
| E11 | overlapping accesses in one call (22) | `add(&mut o.n, reset(&mut o))` | `conflicting-access` | "overlapping accesses to `o`: `reset(&mut o)` may replace what `&mut o.n` points into" | evaluate the writer first: `x := reset(&mut o)`, then `add(&mut o.n, x)` |
| E12 | an impl less local than its trait (34) | `fn area(*self) -> f64` for a trait's `fn area(&self)` | `trait-method-signature` | "`Shape.area` takes `&self`; an impl may be more local than its trait, never less" | take `&self` |
| E13 | a local reference in a boundary signature (38, proposed) | an exported `fn store_name(name: &string)` | `local-reference-escapes` | "a host may keep what an exported function receives, so `name` can't be a local reference" | write `*string`, or take the value |

At run time, a length change of a collection under an element borrow,
made through an alias the checker can't see, panics `collection-pinned`
as today.

## 4. Counting Rules

The rows say where a borrow's address is and what keeps that storage
alive for the call. They are the earlier R3″ table, reduced. With
local references only in argument position, there are no bound borrows,
no binding groups and no delayed drops. Every count is released when
the call returns.

| Row | The borrowed address is in | Example | How it stays valid | Cost per borrow |
| ---: | --- | --- | --- | --- |
| 1 | a local's or value parameter's own storage, or a local reference parameter's referent, through fields only | `energy(&b)`, `lexer.skip_space()`, `f(&mut p.pos)`, `self.items.push(x)` | the local, or the caller's lent referent, outlives the call; rule 22 stops another argument replacing it | none |
| 2 | an object reached through a `*T` that a local or parameter of this body holds, when no closure captures that binding and no `*T` refers to it | `bump(&mut o.n)`, `o.step(dt)` with `o: *mut Obj` | the binding's own count holds the object; only this body can write the binding, and rule 22 rejects an argument that does | none |
| 3 | an object reached through any other `*T`: a field, an element, a captured binding, or a binding some `*T` refers to | `bump(&mut h.leaf.n)`, `bump(&mut nodes[i].n)`, `self.leaf.step(dt)` | one fast-path count of the object, taken at the borrow and released when the call returns | one known pair |
| 4 | a list element or a map value, with no `*T` step after the element step | `ps[i].step(dt)`, `swap(&mut xs[i], &mut xs[j])`, `for p in &mut ps` | today's pin and one fast-path count of the buffer: a length change panics `collection-pinned`, and a copy of the pinned list copies its elements | one known pair and the pin; both omitted for an unaliased local list that no other argument names |

A borrow is classified by the steps after the last `*T` dereference in
its place, since those say which block the address is in. So
`bump(&mut nodes[i].n)` with `nodes: List[*mut Node]` counts the node
(row 3) and needs no pin. An element loop is one borrow for the whole
loop, as [`flow.for.ref.access`](../../spec/lang/06-control-flow.md#element-reference-loops)
states today.

Three rules hold for every row. Every count of a `*T`, whether a copy,
a store or a row 3 count, lands on an object's start and takes the known
path. Weakening a local reference, and passing a local parameter on,
emit no count. No count ever has a local reference as its operand,
which the ownership verifier checks.

```text
struct Leaf:
    n: i64

struct Holder:
    leaf: *mut Leaf  # hypothetical syntax
    counts: List[i64]

fn bump(p: &mut i64) -> void:
    *p += 1

fn demo(h: &mut Holder, o: *mut Leaf) -> void:  # hypothetical syntax
    let mut local = Leaf { n: 0 }
    bump(&mut local.n)  # row 1: free
    bump(&mut o.n)  # row 2: o's own count holds the leaf
    bump(&mut h.leaf.n)  # row 3: one fast-path count of the leaf
    bump(&mut h.counts[0])  # row 4: the pin and a fast-path count of the buffer
```

## 5. Escape Hatches

The owner's advice: "if users hate, just use *T or read, copy, write
back". Both are ordinary programs.

**Take a counted `*T`.** An object of its own may be stored, returned
and captured, at one fast-path count per copy.

```text
struct Counter:
    n: i64

struct Watch:
    counters: List[*mut Counter]  # hypothetical syntax

fn remember(w: &mut Watch, c: *mut Counter) -> void:  # hypothetical syntax
    w.counters.push(c)

fn demo() -> i64:
    let mut w = Watch { counters: [] }
    let c = &mut Counter { n: 0 }
    remember(&mut w, c)
    c.n += 1
    w.counters[0].n
```

**Copy the value out, change it, write it back.** This takes no
reference at all. A struct copy is a few word moves, and a list's
copy-on-write keeps it cheap.

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

What the first cut gives up, and what to write instead:

| Gone | Instead |
| --- | --- |
| a `let`-bound alias of a field or an element, `let r = &mut o.f` | name the place at each use, or pass `&mut o.f` to a helper |
| a returned local reference, as in serde_json's `pointer_mut` or Rust's `split_at_mut` | a `*T` node, indices, or a `&fn` callback |
| an in-place edit inside an enum payload, such as std's `Json` | copy out, change, write back: a payload is no place ([`ref.match.copy`](../../spec/lang/13-values-and-references.md#matching-through-a-reference)) |
| `List[&T]`, `Option[&T]`, `&T?` | `List[*T]`, `*T?`, or two functions |
| a lazy adapter's closure that reads `self` | copy the field first, `k := self.k` |
| a `*self` method called on a field | make the field `*mut T` |

Tier 2's lifetime parameters, or Hylo-style projections, would bring
back local results and bound aliases later.

## 6. Architecture

### 6.1 Where it lives

| Stage | Crate | Change |
| --- | --- | --- |
| parse | `hd_syntax` | `*T`, `*mut T`, `*dyn Tr` and `&fn(..) -> ..` in types; `*self` and `*mut self` receivers; `hd_fmt` and `hd_doc` print them |
| types | `hd_types` | `TyData::Ref` gains a kind; pool and wire tags; display; unify, solver and join compare kinds |
| headers | `hd_resolve` | lowering of both kinds in parameters, results, fields and receivers |
| check | `hd_check` | `&x` typed by the expected type (`Ck::ref_expr`), lending, receivers (`receiver.rs`), positions and escapes in a new `localref.rs` (`local.rs` holds local functions), the overlap check grown from `elem_refs.rs`, cold calls and captures |
| TIR | `hd_tir` | nothing new: the kind is in the lowered type (6.2) |
| ownership | `hd_tir::own` | the row of each argument borrow; row 3's hidden owner local; row 4's pin with a known buffer count; the verifier's no-count-on-a-local check |
| emit | `hd_wasm_linear` | `retain_known` and `release_known` for rows 3 and 4; every `*T` count on the known path; local-reference captures stored uncounted |
| boundary | `hd_wasm_linear` `boundary.rs` | only `*T` crosses (rule 38, proposed) |

### 6.2 The kind is a lowered type

`Types::lower_refs` turns each reference type into a cell ADT before the
ownership passes, and the passes decide counting by type
(`OwnCx::counted`). `*T` keeps today's def; `&T` lowers to a second def,
a plain address whose layout is not counted. Every copy, move, weakening
and drop of a local reference is then count-free with no pass change,
and `fn(&T)` stays distinct from `fn(*T)` after monomorphization.

Until slice 15 the second def is counted like the first. So the checker
switch (slice 9) is green and sound before the counting rows land, and
slice 15 flips one layout bit.

### 6.3 Classifying a borrow

```rust
/// How an argument borrow stays valid for its call (section 4).
enum BorrowRow { Free, HeldByBinding, CountObject, PinBuffer }

fn classify(body: &Body, arg: Inst) -> BorrowRow {
    let place = body.place_of(arg);
    // Only the steps after the last `*T` dereference say which block holds the address.
    let (hop, tail) = place.split_at_last_counted_deref();
    if tail.has_elem_step() {
        let unseen = hop.is_none()
            && unaliased_local_list(body, &place)
            && !other_arg_names(body, arg, &place);
        return if unseen { BorrowRow::Free } else { BorrowRow::PinBuffer };
    }
    match hop {
        None => BorrowRow::Free, // row 1
        Some(Hop::Binding(b)) if !body.is_captured(b) && !body.has_counted_ref_to(b) => {
            BorrowRow::HeldByBinding // row 2
        }
        Some(_) => BorrowRow::CountObject, // row 3
    }
}

fn lower_call(body: &mut Body, call: Inst) {
    for arg in body.borrow_args(call) {
        match classify(body, arg) {
            BorrowRow::Free | BorrowRow::HeldByBinding => {}
            BorrowRow::CountObject => {
                let owner = body.hidden_local_of_object_start(arg); // a *T local, counted as any
                body.release_after(call, owner);
            }
            BorrowRow::PinBuffer => body.pin_with_known_count(arg, call),
        }
    }
}
```

`split_at_last_counted_deref` reuses the emitter's place walk
(`Em::place_start`, `Em::walk_place`). The overlap check runs before
this, in the checker, so row 2 may assume no argument writes its
binding. Row 3's hidden local holds the object's start, so its count and
release use the known path with no new count kind.

### 6.4 Caching and testing

A reference's kind is part of a signature, so it is in the interface
hash, and a header change rechecks callers. The overlap check and the
classification read callee signatures, never callee bodies. Section 8
lists the tests per slice. They are typing fixtures per rule and error
case, runtime fixtures for rows 3 and 4, and `counts` stage fixtures.

## 7. Cost

Per use, against main at `7c969d711`:

| What | Main | First cut |
| --- | --- | --- |
| `f(&local)`, or `local.method()` with a local receiver | a general pair (two `block_of`) per call, and a cell per declaration | no count; the cell stays until slice 19 |
| `f(&mut p.f)` with `p` a reference parameter, or `self.items.push(x)` | none (lent since #676) | none |
| `bump(&mut o.n)`, `o` an unaliased binding holding a `*T` | a general pair | none |
| `bump(&mut h.leaf.n)` | a general pair | a known pair |
| `f(&mut xs[i])`, `xs` behind a reference | `IsUnique`, `Pin`, a general buffer pair | `IsUnique`, `Pin`, a known buffer pair |
| `f(&mut xs[i])`, `xs` an unaliased local named by no other argument | `IsUnique`, `Pin`, a general buffer pair | `IsUnique` only |
| a `&mut self` method calling a `&self` one (weakening) | a pair | none |
| any count of a `*T` | a general pair through `block_of` | a known pair |
| `f(&temporary)` | a new object | a new object, until slice 19 |
| `${p.name}` in an `r` template | a general pair | a new object, until #702's temporaries |

A known pair costs 2.7 ns and a general pair 6.0 ns. Both were
measured on main, 50,000,000 calls per loop, seven runs each on Node
with `--release`. A loop of `energy(&b)` calls ran in 527 ms counted and 273 ms with the
prototype's uncounted borrows, about 5 ns per call.

Per program the gain from local references alone is small: nbody's
`advance(&mut bodies, 0.01)` is about 2% of its time. The larger gain is
rule 4's, which removes `block_of` from every `*T` count. #704 measured
that lookup at 7% of binary_trees' samples and 14% to 22% of json's.

Compile time: there is no flow analysis. The overlap check compares the
argument places of each call pairwise, which is quadratic only in one
call's argument count. Classification is one look per borrow.

## 8. Implementation Plan

### 8.1 Order

The spec comes first (slices 1 and 2), with each new fixture a
`KNOWN_FAILURES.tsv` row until the slice that makes it pass. Examples
that write `*T` stay `text` blocks until slice 3 parses them.

Slice 9, the switch, is written first on a branch. Its errors over the
repository are the site list for the respelling slices 4 to 8, and it
merges after them. Until then `*T` and `&T` both mean today's counted
reference, so every slice keeps the suites green.

The ownership slices first add the counts of rows 3 and 4 (slice 14),
while local references are still counted. Slice 15 then flips the
lowered type. Each slice is about an hour of agent work. A slice past
1.5 times its budget stops for review. "Lines" are compiler code lines;
"sites" are edited type spellings outside the compiler.

### 8.2 Slices

**Slice 1. Spec: kinds, addresses, positions and receivers.**
- Files: `spec/lang/13-values-and-references.md` (Reference Types,
  Reference Expressions, Places And New Objects, Conversions, Matching
  Through A Reference, Element References), `02-grammar.md` (`*T`,
  `*mut T`), `07-functions.md` (Reference Receivers), `spec/README.md`
  (the new code).
- Rules: 1 to 21 and 26. Retired: `ref.type.position` for `&T`,
  `ref.place.lifetime` for `&T` (it stays for `*T`), `ref.match.binding`
  for a local scrutinee, `ref.elem.kept`, `ref.elem.callee-copy` and
  `ref.elem.callee-copy.declared`.
- Fixtures: a typing fixture per rule; invalid ones for E1 to E7.
- Done: `bash spec/check.sh`. Budget: spec text, about 25 rules.

**Slice 2. Spec: overlap, element borrows, closures, traits, suspension
and std.**
- Files: chapter 13 (Call-Scoped Access becomes the overlap rule;
  Captured References), `06-control-flow.md` (Element Reference Loops),
  `07-functions.md` (Function Type Constructors, Captures),
  `09-traits.md` (Trait Value References, `trait.impl.signature`),
  `11-requirements-and-suspension.md` (cold calls),
  `spec/std/format.md`, `spec/std/text.md`, `spec/std/error.md`.
- Rules: 22 to 25, 27 to 43. Retired: `ref.type.no-function`, which
  makes `&fn` a syntax error today; `ref.elem.conflict` folds into rule
  22.
- Fixtures: invalid E8 to E12. Runtime: `write_then_read` returns `1`;
  a length change through an alias under an element borrow panics; a
  callee that replaces the leaf through a second `*mut Holder` alias,
  while `&mut a.leaf.n` is borrowed, leaves the old leaf alive for the
  call.
- Done: `bash spec/check.sh`. Budget: spec text, about 20 rules.

**Slice 3. Parser and types, with the second lowered type.**
- Files: `hd_syntax` parser (`types.rs`, `items.rs`); `hd_types`
  (`pool.rs`, `wire.rs`, `unify.rs`, `solver.rs`); `hd_resolve`
  (`lower.rs`, `header.rs`, `iface.rs`); display in `hd_mono`,
  `hd_structure`, `hd_fmt`, `hd_doc`.
- Work: `*T`, `*mut T`, `*dyn`, `*self` parse and print; `TyData::Ref`
  gets its kind (127 match sites in 43 files); `lower_refs` maps `&T` to
  a second def, still counted (6.2).
- Fixtures: slice 1's parse fixtures leave `KNOWN_FAILURES.tsv`.
- Done: `*T` round-trips through `hd fmt`; crate tests pass; `*T` means
  today's reference. Budget: 250 lines (syntax and printing 50, the
  kind 130, the second def 30, headers 40).

**Slice 4. Respell std.**
- Files: `lib/std` and `spec/std` signatures: the 19 fields and the
  returned references to `*T`, the about 17 keeping parameters to
  `*mut T`, `*self` on keepers, `Template[*dyn Display]`,
  `Result[T, *dyn Error]`, `impl Display for *T`, and the `DebugWriter`
  builders of rule 41.
- Fixtures: std's doc tests; `debug-writer-builders.hd`,
  `debug-builders-behind-ref.hd`.
- Done: std builds; `cargo test`; the conformance gate unchanged.
  Budget: about 400 sites, from slice 9's list.

**Slice 5. Respell the language chapters and parse fixtures.**
- Files: `spec/lang/*.md` examples; `spec/conformance/parse/`.
- Done: `bash spec/check.sh`; the conformance gate. Budget: about 150
  sites.

**Slice 6. Respell typing fixtures.**
- Files: `spec/conformance/typing/`.
- Done: the conformance gate. Budget: about 300 sites.

**Slice 7. Respell runtime and CLI fixtures.**
- Files: `spec/conformance/runtime/`, `spec/conformance/cli/`.
- Done: the conformance gate. Budget: about 300 sites.

**Slice 8. Respell the rest.**
- Files: `website/`, `examples/`, crate test sources and stage
  fixtures, `docs/future-work`.
- Done: `cargo test`; `pnpm run website:build`. Budget: about 150 sites.

**Slice 9. The switch: `&T` is local.**
- Files: `hd_check` `refs.rs` (`Ck::ref_expr`: rules 5 to 7), `body.rs`
  (coercion: rules 9 to 11), `call.rs`, a new `localref.rs` (rules 12
  to 17); `hd_diag` (the code, a message per case).
- Fixtures: slice 1's typing fixtures for rules 5 to 17 and E1 to E6
  leave `KNOWN_FAILURES.tsv`.
- Done: the conformance gate; each fix-it of E1 to E6 applies with
  `hd fix`. Budget: 240 lines (address kind 50, lending 40, positions
  90, messages and fix-its 60).

**Slice 10. The overlap check, folding `elem_refs.rs` into it.**
- Files: `hd_check` `elem_refs.rs` (the access frame grows from
  collection paths with length changes to any place with prefix
  writes), `call.rs` (the `ref.elem.callee-copy` check goes),
  `localref.rs` (element borrows take the general position rules);
  `hd_wasm_linear` `rt.rs` and `emit/list.rs` (the `LIST_EXPOSED` bit,
  which marked buffers exposed to kept element references).
- Rules: 22 to 24; `flow.for.ref.conflict` as rule 22's loop case.
- Fixtures: E11; today's `conflicting-access` fixtures unchanged.
- Done: the conformance gate; `elem_ref_kept`, `elem_ref_captures` and
  the exposure bit are gone. Budget: 90 lines added, about 110 deleted.

**Slice 11. Receivers: `*self`, returned `self`, impl kinds.**
- Files: `hd_check` `receiver.rs` (`auto_address`: rule 20), `body.rs`
  (rule 21's result kind), `conform.rs` (rule 34), `dynref.rs` (rule
  35).
- Fixtures: E7, E12; valid chains on a field and on a local.
- Done: the conformance gate; rule 21's chain on `client.request`
  checks. Budget: 110 lines (`*self` 30, returned `self` 40, impl kinds
  25, `&dyn` 15).

**Slice 12. Escapes: captures, cold calls, boundaries.**
- Files: `hd_check` `expr.rs` (captures: rule 31), `call.rs` (cold
  calls: rule 37), `dynref.rs` (providers: rule 33); `hd_wasm_linear`
  `boundary.rs` (rule 38, once approved).
- Fixtures: E8, E10, E13.
- Done: the conformance gate. Budget: 85 lines (captures 25, cold calls
  30, providers 10, boundary 20).

**Slice 13. `&fn`.**
- Files: `hd_syntax` (`&fn` in a type), `hd_types` (a local reference to
  a function type, which K2's def covers), `hd_check` `call.rs` and
  `expr.rs` (rules 28 to 30), `hd_wasm_linear` `emit/closure.rs` (a
  closure literal for a `&fn` stores its local-reference captures
  uncounted); `lib/std` eager helpers (rule 40).
- Fixtures: E9; a valid `each` capturing `self`; a valid named
  function passed to a `&fn`.
- Done: the conformance gate; a `List.fold` whose closure reads `self`
  checks. Budget:
  105 lines (syntax 10, types 15, call through `&fn` 10, captures 40,
  emitter 30) and about 30 std sites.

**Slice 14. Counted rows: 3 and 4.**
- Files: `hd_tir` `own/borrow.rs` (`classify`, by the steps after the
  last `*T` dereference), `own/insert.rs` (row 3's hidden owner local,
  released after the call); `hd_wasm_linear` `emit/count.rs`,
  `emit/list.rs` (row 4's pin with a known buffer count).
- Fixtures: slice 2's runtime fixtures; `counts` fixtures for
  `bump(&mut h.leaf.n)` and `bump(&mut nodes[i].n)`.
- Done: `write_then_read` returns `1`; the aliased length change
  panics; the replaced leaf stays alive. Budget: 120 lines
  (classification 50, hidden owner 40, pin with a known count 30).

**Slice 15. Free rows: the lowered type flips.**
- Files: `hd_types` `pool.rs` (the second def becomes uncounted),
  `hd_tir` `own/borrow.rs` (row 2's condition), `ir/owned.rs` (the
  verifier rejects a count with a local operand); `counts` stage
  fixtures.
- Fixtures: `counts` fixtures for the prototype's ten use-case programs
  (a tokenizer, an in-place sort, element loops, a grid, a JSON walk and
  others), in both kinds. They are under
  `crates/hd_driver/tests/cases/counts/scoped-834/` on `lane/834-proto`.
- Done: no count on rows 1 and 2 or on weakening; checked mode passes.
  Budget: 70 lines (flip 5, row 2 25, weakening 5, verifier 25, fixture
  plumbing 10).

**Slice 16. `*T` on the known path.**
- Files: `hd_wasm_linear` `emit/count.rs`, `rt.rs`; a verifier check
  that a `*T` operand is a block start.
- Fixtures: a `counts` fixture per counted kind.
- Done: #704's instrumentation shows no `block_of` from a reference
  count on binary_trees. Budget: 40 lines.

**Slice 17. Suspending functions' borrowed `mut` parameters.**
- Files: `hd_wasm_linear` `emit/susp.rs`, `emit.rs` (`ref_roots`).
- Fixtures: a runtime fixture: `fn outer!(mut n: i64)` calls
  `bump!(&mut n)`.
- Done: it runs; today it fails with "a mutably captured parameter of a
  suspending function in the linear backend is not supported". Budget:
  40 lines.

**Slice 18. Measure.**
- Files: `bench/study`, the `energy(&b)` microbenchmark.
- Done: hd-only `--quick` before and after for binary_trees, json and
  nbody; hello's size; json and nbody compile time; posted on #834.
  Budget: none.

**Slice 19. #702 frame slots.** The owner placed these right after the
first cut (#834, Q4, "its slice 14" before two slices were added).
- Files: `hd_check` (a local is frame-eligible when no `*T` and no plain
  closure refers to it), `hd_tir` (`PlaceRoot::Frame`, reserved today),
  `own/insert.rs`, `hd_wasm_linear` `emit/refs.rs`, `emit/scope.rs` and
  the frame layout; temporaries passed to `&T` parameters use slots too.
- Fixtures: the std runtime's allocation test from #702; a `counts`
  fixture for a loop that calls `advance(&mut i)` on a scalar local.
- Done: no cell for a local borrowed only locally; the allocation test
  shows one fewer allocation per such declaration. Budget: about 250
  lines (eligibility 30, lowering 60, frame addressing 80, temporaries
  40, suspending bodies 40). Its first decision is where slots live:
  in the heap memory, or in the shadow stack (linear DESIGN §23.29).
  Today a `&T` is an address in one memory.

### 8.3 Estimate

| Slices | What | Budget |
| --- | --- | ---: |
| 1, 2 | spec text | about 45 rules |
| 3 | parser and types | 250 lines |
| 4 to 8 | respelling | about 1,300 sites |
| 9 | the switch | 240 lines |
| 10 | overlap check and fold | 90 lines, about 110 deleted |
| 11 | receivers | 110 lines |
| 12 | escapes | 85 lines |
| 13 | `&fn` | 105 lines, about 30 sites |
| 14, 15 | counting rows | 190 lines |
| 16 | `*T` known path | 40 lines |
| 17 | suspending `mut` parameters | 40 lines |
| **3, 9 to 17** | **the first cut's compiler** | **1,150 lines added, about 110 deleted** |
| 19 | #702 frame slots | about 250 lines |

The review's estimate was about 920 lines. The growth is returned
`self` and impl kinds (65 lines), `&fn` (105) and the overlap check's
generalization (30). The argument-position rule removed the
binding-group work, which offsets part of it. The web compiler grows by
a few kilobytes compressed; the 2 MB cap is not near.

## 9. Decisions

Each item points to the owner's comment on #834 that decided it. Times
are the comments' UTC stamps, from 22:52 on 2026-10-24 to 00:27 the
next day; the owner dated them all 2026-10-24.

- [2026-10-24 22:52](https://github.com/HerringtonDarkholme/hd-lang/issues/834#issuecomment-6390554663):
  no exclusivity check; references across `!` calls are required; std
  uses local references on every parameter. Its third item, generics
  instantiated with local references, was replaced at 23:24.
- [23:09](https://github.com/HerringtonDarkholme/hd-lang/issues/834#issuecomment-6390708630):
  the spelling: `*T` counted and always a whole object, `&T` local; `&x`
  takes an address and `*p` dereferences; `*T` lends to `&T`, never the
  reverse; `*self` keeps `self`; std's keepers are `*mut T`. It replaced
  the `&{}` spellings of 22:53 and 23:03.
- [23:24](https://github.com/HerringtonDarkholme/hd-lang/issues/834#issuecomment-6390845601):
  element borrows keep the pin and a fast-path buffer count; no
  `Generic[&T]`; interior places are local only, so `*T` needs no
  `block_of`; counting local references is fine, with rules.
- [00:16](https://github.com/HerringtonDarkholme/hd-lang/issues/834#issuecomment-6391309697):
  `let a = &s` is counted and `let a = &obj.f` is an error; local
  references only in argument position and as receivers; overlapping
  access in one call is a compile-time error; heap aliases take a
  fast-path count; the review's recommendation B (call-scoped) adopted.
- [00:23](https://github.com/HerringtonDarkholme/hd-lang/issues/834#issuecomment-6391378059):
  Q8, the Debug writer returns itself; Q9, interpolated interior parts
  are copied; Q10, `&x` where a type variable is expected is counted;
  Q3, a local receiver may return `self`; Q5, `&fn` for non-escaping
  closures.
- [00:27](https://github.com/HerringtonDarkholme/hd-lang/issues/834#issuecomment-6391410635):
  Q6, an impl may be more local than its trait; Q4, #702's frame slots
  follow the first cut.

The review's K2, a separate lowered type for local references, is the
implementation choice of section 6.2.

## 10. Prior Art

| Language | Second-class reference | Stored reference | How a borrow stays valid | What hd takes |
| --- | --- | --- | --- | --- |
| Swift | `inout`, `borrowing` parameters, `mutating` methods | class references | the call; exclusivity, static for locals and dynamic for class properties (SE-0176) | the argument-position rule and a static overlap check, with no dynamic one |
| Rust | none: all references carry lifetimes | `&T` with a lifetime, `Rc<T>` | NLL liveness (RFC 2094), two-phase borrows (RFC 2025) | nothing; its checker is the cost tier 2 avoids |
| Hylo | `let`, `inout`, `sink` parameters; subscript projections | none: no shared mutable references | the call, or the projecting expression | local receivers; a returned `self` used within one expression |
| Mojo | `read`, `mut`, `ref` arguments | `ref` with origins | origins tracked at compile time | nothing beyond Swift's model |
| C# | `ref` parameters and `ref` locals | class references | ref-safe contexts, a static scope comparison | nothing: hd has no bound local references |
| Vale | region borrows in `pure` functions | generational references | a frozen region, or a generation check per dereference | nothing |
| Go | none | `*T`, interior pointers included | garbage collection; escape analysis | the `*T` spelling and pointer receivers |
| C++ | references | pointers | nothing: `push_back` may invalidate | the counterexample element pins prevent |

## 11. Sources And Parse Log

### 11.1 Sources

- Swift SE-0176, Enforce Exclusive Access to Memory:
  https://github.com/swiftlang/swift-evolution/blob/main/proposals/0176-enforce-exclusive-access-to-memory.md
- Swift SE-0377, `borrowing` and `consuming` parameter ownership
  modifiers:
  https://github.com/swiftlang/swift-evolution/blob/main/proposals/0377-parameter-ownership-modifiers.md
- Rust RFC 2094, non-lexical lifetimes:
  https://rust-lang.github.io/rfcs/2094-nll.html; RFC 2025, two-phase
  borrows: https://rust-lang.github.io/rfcs/2025-nested-method-calls.html
- Hylo language tour, functions and methods:
  https://docs.hylo-lang.org/language-tour/functions-and-methods;
  subscripts: https://docs.hylo-lang.org/language-tour/subscripts
- Mojo manual, ownership: https://mojolang.org/docs/manual/values/ownership;
  lifetimes and origins: https://mojolang.org/docs/manual/values/lifetimes
- C# specification §9.7.2, ref-safe contexts:
  https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/language-specification/variables
- Vale, Zero-Cost Memory Safety with Vale Regions:
  https://verdagon.dev/blog/zero-cost-memory-safety-regions-overview
- Go specification, appending and copying slices:
  https://go.dev/ref/spec#Appending_and_copying_slices
- C++ `vector::push_back`:
  https://en.cppreference.com/w/cpp/container/vector/push_back
- hd: #834 (the decisions), #704 (the `block_of` measurement), #702
  (cells and frame slots), #676 (lent fields), [Object Model, Tier
  2](OBJECT_MODEL_TIER2.md),
  [Values And References](../../spec/lang/13-values-and-references.md),
  [docs/compiler/tir-v2.md](../compiler/tir-v2.md),
  [crates/hd_wasm_linear/DESIGN.md](../../crates/hd_wasm_linear/DESIGN.md)
  §23.20 and §23.29. The measurements of section 7 were taken on main
  at `0c1b055d2` and `7c969d711` and with the prototype branch
  `lane/834-proto`.

### 11.2 Parse log

Every `text` block, in order, parsed with `scripts/hd-conformance.sh
parse` on `a592c5591`. Parsing checks syntax only; it does not
type-check, and the lines marked `# error` are errors of the rules, not
of the parser. Each `syntax-error` below is on a line marked
`# hypothetical syntax`. To check that no other line fails, each block
was parsed again with its marked lines in today's syntax (`*` as `&`,
`&fn` as `fn`). All 14 parse.

| Block | Section | Result |
| ---: | --- | --- |
| 1 | 2.1 | `syntax-error` at line 6, `*mut` |
| 2 | 2.2 | `syntax-error` at line 7, `*P` |
| 3 | 2.3 | `syntax-error` at line 22, `*Expr` |
| 4 | 2.4 | `syntax-error` at lines 10 and 16, `*mut self` and `*mut` |
| 5 | 2.5 | `syntax-error` at lines 7 and 16, `*mut` |
| 6 | 2.6 | parse |
| 7 | 2.7 | `syntax-error` at line 5: "'&' cannot directly precede 'fn'", today's [`ref.type.no-function`](../../spec/lang/13-values-and-references.md#reference-types) |
| 8 | 2.8 | `syntax-error` at lines 5 and 18, `*mut self` and `*dyn` |
| 9 | 2.9 | parse |
| 10 | 2.10 | parse |
| 11 | 2.11 | parse |
| 12 | 4 | `syntax-error` at lines 5 and 11, `*mut` |
| 13 | 5 | `syntax-error` at lines 5 and 7, `*mut` |
| 14 | 5 | parse |

Rust code in section 6.3 is pseudo-code and was not compiled.
