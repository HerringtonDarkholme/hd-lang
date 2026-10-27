# Values And References

Status: language specification draft.

This chapter defines reference types and reference expressions: their syntax, their typing, and what they do at run time. A **reference type** is a counted reference type, `*T` or `*mut T`, or a local reference type, `&T` or `&mut T`. Its values refer to a `T` that lives elsewhere.

`*T` and `&T` read the `T` they refer to, and `*mut T` and `&mut T` also write it. A **counted reference** may be stored, returned, and captured. A **local reference** is lent to a call and never kept past it.

`&e` and `&mut e` make local references. A counted reference refers to an object that `box` makes, which holds a copy of a value.

```hd
struct Body:
    pos: Point
    vel: Point

fn drift(body: &mut Body, step: &Point) -> void:
    body.pos = *step

fn spawn(pos: Point) -> *mut Body:
    box(Body { pos: pos, vel: Point { x: 0, y: 0 } })
```

## Reference Types

A reference type is `*`, `*mut`, `&`, or `&mut` followed by a type.

```hd
fn first(items: List[*Point]) -> Option[*mut Point]:
    .None

fn each(f: fn(&Point) -> *mut Point, pending: *Point?) -> void: pass
```

1. r[ref.type.forms] `&T` is a local reference type, and `&mut T` is a writable local reference type. The operand `T` is a named, `dyn`, tuple, or grouped type.
2. r[ref.type.counted-forms] `*T` is a counted reference type, and `*mut T` is a writable counted reference type. Their operand is as for `&T`.
3. r[ref.type.kinds] Counted and local are the two kinds of reference. A rule of this chapter that says "a reference" holds for both kinds.
4. r[ref.type.kinds.elsewhere] A rule in another chapter that writes `&T` or `&mut T` for a reference that is read, written, matched, compared, or printed holds for `*T` and `*mut T` too. This rule is temporary, as the note below says.
5. r[ref.type.kind-runtime] A local reference is counted at run time, as a counted reference is. Only the rules of [Local References](#local-references) tell the two kinds apart.
6. r[ref.type.position] A counted reference type may appear wherever a type may: a parameter, a result, a field, a binding annotation, a type argument, and a function type's parameters and result.
7. r[ref.type.optional] `?` applies to the whole reference type: `*T?` is an optional `*T`.
8. r[ref.type.group] To refer to an optional or to another reference, group the operand: `&(T?)` and `&(&T)`.
9. r[ref.type.no-nesting] `&&T` is a syntax error, because `&&` is one token. So are `&mut mut T` and `mut &T`. Error: `syntax-error`.
10. r[ref.type.counted-no-nesting] `**T` is a syntax error too, because `**` is one token, so a counted reference to one is written `*(*T)`. So are `*mut mut T` and `mut *T`. Error: `syntax-error`.
11. r[ref.type.no-function] `&` never directly precedes `fn`: `&fn() -> i32` and `&mut fn() -> i32` are errors. Error: `syntax-error`.
12. r[ref.type.counted-no-function] `*` never directly precedes `fn` either: `*fn() -> i32` and `*mut fn() -> i32` are errors. Error: `syntax-error`.
13. r[ref.type.distinct] `&T` and `&mut T` are distinct types. Each is invariant in `T`: `&A` is `&B` only when `A` is `B`.
14. r[ref.type.kind-distinct] `*T` and `*mut T` are distinct types, and distinct from `&T` and `&mut T`. Each is invariant in `T`.
15. r[ref.type.any-ref] A reference type implements `AnyRef`.
16. r[ref.type.dyn] `&dyn Tr` and `&mut dyn Tr` are trait value references, whose conversions and members [Trait Value References](09-traits.md#trait-value-references) states.
17. r[ref.type.dyn-counted] `*dyn Tr` and `*mut dyn Tr` are counted trait value references. They take the same conversions and members, with `*` in place of `&` on both sides of each conversion.

> **Note.** [`ref.type.kinds.elsewhere`](#r-ref.type.kinds.elsewhere)
> bridges the chapters until the respelling slices of
> [#834](https://github.com/HerringtonDarkholme/hd-lang/issues/834) spell
> each such rule for both kinds. Those slices then remove it.

```text
fn bad_and(p: &&Point) -> void: pass  # error: syntax-error
fn bad_mut(p: mut &Point) -> void: pass  # error: syntax-error
fn bad_fn(f: &fn() -> i32) -> void: pass  # error: syntax-error
fn bad_power(p: **Point) -> void: pass  # error: syntax-error
```

See also: [Modifiers, Optionality, And Grouping](02-grammar.md#modifiers-optionality-and-grouping).

## Reference Expressions

A reference expression is a prefix operator and its operand. `&e` and `&mut e` make a reference, and `*e` reads through one.

```hd
fn demo(r: &mut Point, items: List[Point], i: i32) -> void:
    inspect(&items[i])
    edit := &mut r.x
    copy := *r
    *r = Point { x: 1, y: 2 }
    made := &Point { x: 3, y: 4 }
```

1. r[ref.expr.make] `&e` and `&mut e` are prefix expressions. Their operand is a `unary_expression`.
2. r[ref.expr.literal] A data expression is an ordinary operand, so `&Point { x: 1, y: 2 }` and `&mut Box::[i32] { value: 1 }` are reference expressions around a literal.
3. r[ref.expr.deref-operand] `*e` is a prefix expression. Its operand is a postfix expression, another `*e`, or a prefix expression such as `&x` or `-x`.
4. r[ref.expr.deref.target] `*e` may be the left side of `=` and of a compound assignment, as in `*r = v` and `*r += 1`.
5. r[ref.expr.star-binary] `*` is a prefix operator only. Between two operands it stays multiplication, and `*=` stays compound assignment.

## Precedence

The prefix `&` and `&mut` bind like the prefix `!` and the unary `-`. The prefix `*` binds like a postfix suffix, tighter than `**`.

1. r[ref.prec.unary-ref] `&` and `&mut` are `unary_expression` operators. They bind more tightly than every binary operator and less tightly than every postfix suffix.
2. r[ref.prec.deref] `*` binds more tightly than every binary operator, `**` included, and less tightly than every postfix suffix.
3. r[ref.prec.postfix] The operand includes the postfix suffixes after it. `&a.b[i]` is `&(a.b[i])`, and `*r.x` is `*(r.x)`.
4. r[ref.prec.group] To dereference before a suffix, group the operand: `(*r).x`.
5. r[ref.prec.power-ref] The operand of `&` and `&mut` includes `**`, as for unary `-`. `&x ** 2` is `&(x ** 2)`.
6. r[ref.prec.power-deref] The operand of `**` may be a dereference. `*r ** 2` is `(*r) ** 2`, and `2 ** *r` is `2 ** (*r)`.
7. r[ref.prec.nested] Prefix operators nest from the right. `&*r` is `&(*r)`, `*&x` is `*(&x)`, and `-*r` is `-(*r)`.
8. r[ref.prec.operand-position] A `&` or `*` that begins an operand is a prefix operator. After an operand it is the binary bitwise and or multiplication, so `a & &b` is `a & (&b)`.

```hd
fn precedence(r: &mut Point, items: List[Point], i: i32, a: i32, b: i32) -> void:
    inspect(&items[i].x)  # inspect(&(items[i].x))
    two := *r.x         # *(r.x)
    three := (*r).x     # a field of the value
    four := a & &b      # bitwise and of a with a reference
    five := a * *b      # multiplication of a by a dereference
    six := *r ** 2      # (*r) ** 2
    seven := &r ** 2    # &(r ** 2)
```

> **Why.** A reader who knows `-a.b` and `!a.b` already knows `&a.b`. A
> dereference reads a value, so `*r ** 2` squares the value `*r`, as
> `r.x ** 2` squares a field.

## Operator Tokens

1. r[ref.token.and-and] `&&` is the logical and operator and nothing else. `&&x` is a syntax error, so a reference to a reference is written `& &x` or `&(&x)`. Error: `syntax-error`.
2. r[ref.token.caret] `^` is only the exclusive-or operator. It has no prefix form and no meaning in a type. Error: `syntax-error`.
3. r[ref.token.and-bound] In a generic bound list, `&` still joins bounds, as in `T < Display & Debug`.

```hd
fn tokens(a: bool, b: bool, x: i32, y: i32) -> void:
    both := a && b
    bits := x & y
    flip := x ^ y
```

```text
fn bad_expr(x: i32) -> void:
    r := &&x  # error: syntax-error
    s := ^x  # error: syntax-error
```

See also: [Unary And Binary Operators](05-expressions.md#unary-and-binary-operators).

## Places And New Objects

`&e` and `&mut e` make a local reference: to `e` when `e` is a place, and to a new object holding a copy of `e` otherwise.

```hd
struct Point:
    x: i32
    y: i32

fn make() -> Point:
    Point { x: 1, y: 2 }

fn demo() -> i32:
    let mut p = Point { x: 0, y: 0 }
    r := &mut p        # refers to p, as a local &mut Point
    r.x = 5            # p.x is now 5
    q := &mut p.y      # refers to the field y of p
    *q = 7             # p.y is now 7
    n := &make()       # a new object holding the result
    m := &mut Point { x: 3, y: 4 }  # a new object
    p.x + n.y + m.x
```

1. r[ref.place.forms] Locals and parameters of any binding form, and top-level bindings, are places for `&` and `&mut`. A struct field of a place and a value reached through a reference are places too.
2. r[ref.place.through] `*r` with a reference `r` is a place, and so is a field read through `r`, such as `r.x`. The reference `r` is evaluated once, when the reference to the place is taken.
3. r[ref.place.element] A list element and a map value are places too. A reference to one is an [element reference](#element-references), which lives for one call.
4. r[ref.expr.place] `&e` and `&mut e` on a place refer to that place. A later write to the place, through any path, is seen through the reference.
5. r[ref.expr.allocate] `&e` and `&mut e` on any other expression evaluate `e` and refer to a new object that holds the value. Each evaluation makes a new object.
6. r[ref.expr.allocate.writable] A new object is writable, so `&mut T { ... }` and `&mut make()` are valid.
7. r[ref.expr.kind.always-local] `&e` is always a local reference `&T`, and `&mut e` a `&mut T`, where `T` is the type of `e`. The expected type never makes it counted.
8. r[ref.expr.kind.no-counted] `&e` or `&mut e` where a counted reference type is expected is an error, as [`ref.local.escape.counted`](#r-ref.local.escape.counted) states. Error: `local-reference-escapes`.
9. r[ref.place.alias] Two references to one place, or two copies of one reference, see each other's writes.
10. r[ref.place.receiver.local] A call of a `&self` or `&mut self` method on a place takes a local reference to it, as `&` and `&mut` would.
11. r[ref.place.receiver.counted] A call of a `*self` or `*mut self` method on a place is an error, as [`fn.method.counted-self.place`](07-functions.md#r-fn.method.counted-self.place) states. [Reference Receivers](07-functions.md#reference-receivers) states the rules of both calls.

```text
struct Point:
    x: i32

fn keep(p: *mut Point) -> void: pass

fn invalid() -> *Point:
    let mut p = Point { x: 0 }
    keep(&mut p)  # error: local-reference-escapes
    &p  # error: local-reference-escapes
```

> **Why.** A reference to a value that is not a place still needs storage
> to refer to, so it gets a new object, as Go's `&T{...}` does. Sharing
> beyond one call starts with [`box`](#boxes), so `&` never decides between
> a local and a counted reference.

## Boxes

`box(e)` makes a new object that holds a copy of the value of `e`, and returns a counted reference to it.

```hd
struct Point:
    x: i32
    y: i32

struct Holder:
    point: *mut Point

fn demo() -> i32:
    shared := box(Point { x: 0, y: 0 })  # a new object, as a *mut Point
    holder := Holder { point: shared }    # the holder shares the object
    shared.x = 5                          # holder.point.x is now 5
    let view: *Point = shared             # the same object, read-only
    let mut p = Point { x: 1, y: 1 }
    copy := box(p)                        # a new object holding a copy of p
    p.x = 9                               # copy.x is still 1
    holder.point.x + view.x + copy.x      # 11
```

1. r[ref.box.signature] `std.core` declares `box` as `@intrinsic pub fn box[T](value: T) -> *mut T`, and the prelude supplies it. The declaration is ordinary hd, and the compiler supplies only its body.
2. r[ref.box.check] A `box` call is checked by the plain call rules, so `T` is inferred from the argument and the expected type as for any generic call.
3. r[ref.box.new] `box(e)` evaluates `e` once and returns a `*mut T` to a new object that holds the value. Each call makes a new object.
4. r[ref.box.copy] The new object holds a copy of the value. When `e` is a place, a later write to that place is not seen through the result, and a write through the result does not change the place.
5. r[ref.box.weaken] The result converts by the rules of [Conversions](#conversions) and [Trait Value References](09-traits.md#trait-value-references), so it may be used where `*T` or `*mut dyn Tr` is expected.
6. r[ref.box.no-local] `box(e)` is an error when the type of `e` holds a local reference, by [`ref.local.holds`](#r-ref.local.holds). Error: `local-reference-escapes`.
7. r[ref.box.object] Every counted reference refers to a whole object: one that `box` made, or one that another rule names as a new object. No expression makes a counted reference to a local, a parameter, a top-level binding, a field, or an element.
8. r[ref.box.lifetime] A counted reference keeps the object it refers to alive. It may outlive the block that made it, and may be returned.

```text
struct Point:
    x: i32

fn wrap(p: &Point) -> void:
    b := box(p)  # error: local-reference-escapes

fn shadow() -> void:
    box := Point { x: 1 }  # error: prelude-name-shadow
```

> **Note.** No counted reference refers to a field, so no function hands
> out a writable reference into a field of its parameter: `&mut pair.left`
> is local and cannot be returned. A field that several holders share is
> declared `*mut T` and holds a box, as a Swift class field holds a
> reference. `&mut pair.left` still lends the field for one call.

> **Why.** Every counted reference starts at an object, so the runtime
> counts and frees whole objects and never follows a reference into the
> middle of one. A box copies, as Go's `new` and OCaml's `ref` do, so
> sharing starts where the value is boxed, not where it is used.

See also: [Prelude Functions](10-modules.md#prelude-functions),
[Captured References](#captured-references).

## Writable Places Of References

`&mut e` on a place needs the place to be writable.

1. r[ref.place.writable] `&mut e` on a place requires a writable place. A `let mut` local, a `mut` parameter, `mut self`, and a value reached through a writable reference, `*mut` or `&mut`, are writable. A struct field of a writable place is writable too.
2. r[ref.place.writable.top-level] A top-level binding is writable when it is bound with `let mut`.
3. r[ref.place.writable.collection] `&mut` of a list or a map value in a writable place is valid. A change through the reference changes that place.
4. r[ref.place.writable.enum] `&mut` of an enum value in a writable place is valid: a `let mut` local, a `mut` parameter, or a struct field of one. `*r = v` through `&mut E` replaces the whole value. A payload is no place (`ref.match.copy`), so `&mut` of a payload binding is an error by the rows below.
5. r[ref.place.shared-any] `&e` needs no writable place: it may refer to a `:=` binding, a plain `let` binding, or a value parameter.

`&mut` of a place that is not writable is an error, by what holds the place:

| Rule | The place is held by | Error |
| --- | --- | --- |
| r[ref.place.not-writable.short] Short binding | a `:=` or `using` binding | `non-reassignable-binding` |
| r[ref.place.not-writable.param] Parameter | a value parameter or `self` that is not `mut` | `non-reassignable-parameter-binding` |
| r[ref.place.not-writable.let] Plain `let` | a `let` binding without `mut` | `not-writable` |
| r[ref.place.not-writable.shared-referent] Read-only reference | the value directly referred to by a read-only reference, `*T` or `&T` | `not-writable` |

6. r[ref.place.not-writable.fix] For a plain `let` binding and a value parameter, the diagnostic's fix-it writes `mut` before the binding's name.

```text
fn invalid(count: i32) -> void:
    let total = 0
    r := &mut total  # error: not-writable
    fixed := 1
    s := &mut fixed  # error: non-reassignable-binding
    t := &mut count  # error: non-reassignable-parameter-binding
```

See also: [Writable Places](04-type-system.md#writable-places).

## Conversions

A writable reference converts to a read-only one, and a counted reference lends itself as a local one.

1. r[ref.convert.weaken] A writable reference converts implicitly to the read-only one of its kind: `*mut T` to `*T`, and `&mut T` to `&T`. A `*mut T` also converts to `*T?` where an optional is expected. The conversion makes no new reference: both refer to the same place.
2. r[ref.convert.lend] A counted reference converts implicitly to a local one where a local reference is expected: `*T` to `&T`, and `*mut T` to `&mut T` or `&T`. Both refer to the same place.
3. r[ref.convert.no-upgrade] A read-only reference, `*T` or `&T`, never converts to a writable one, `*mut T` or `&mut T`. Using one where a writable reference is expected is an error. Error: `mutable-upgrade`.
4. r[ref.convert.no-counted] A local reference never converts to a counted one, as [`ref.local.escape.counted`](#r-ref.local.escape.counted) states.
5. r[ref.convert.listed-only] Reference conversions are limited to this section and [Trait Value References](09-traits.md#trait-value-references).
6. r[ref.convert.readonly-covariant] A readonly container reference permits `*mut U` to `*U` weakening through covariant container arguments and tuple elements.
7. r[ref.convert.readonly-option] The same weakening enters an optional payload beneath a readonly reference.
8. r[ref.convert.readonly-invariant] The conversion preserves invariant arguments and the operands of mutable references retained in the target.
9. r[ref.convert.readonly-representation] The conversion preserves representation and reference identity, without copying elements or inserting wrappers.

```text
fn edit(r: &mut i32) -> void:
    *r += 1

fn view(r: &i32) -> i32:
    *r

fn invalid(r: *i32) -> *mut i32:
    edit(r)  # error: mutable-upgrade
    r        # error: mutable-upgrade
```

## Local References

A local reference, `&T` or `&mut T`, is lent to a call: a function may read and write through it, and pass it on, but never keep it.

```hd
struct Point:
    x: i32
    y: i32

struct Holder:
    point: *mut Point

fn nudge(p: &mut Point) -> void:
    p.x += 1

fn bump(p: &mut Point) -> void:
    q := p              # a local binding of a local reference
    nudge(q)            # passed on to a &mut parameter

fn demo(h: Holder) -> i32:
    let mut origin = Point { x: 0, y: 0 }
    nudge(&mut origin)  # a local reference argument
    let r: &mut Point = &mut origin  # local: the annotation expects &mut
    bump(r)
    nudge(h.point)      # a *mut Point lends itself as a &mut Point
    origin.x
```

1. r[ref.local.uses] A local reference may be the value of a parameter, a local binding, a call argument, or a receiver.
2. r[ref.local.argument] Passing a local reference to a `&T` or `&mut T` parameter is valid, so a function may pass on what it was lent.
3. r[ref.local.holds] A type **holds** a local reference when it is a local reference type, or a tuple type or a type alias whose type holds one.
4. r[ref.local.self] A `&self` or `&mut self` receiver is a local reference, so these rules apply to `self` in such a method. A method that keeps or returns `self`, such as a builder, takes `*self` or `*mut self` ([Reference Receivers](07-functions.md#reference-receivers)).
5. r[ref.local.escape.counted] A local reference never converts to a counted one. Using one where a counted reference is expected, as for a `*T` parameter or field, is an error. Error: `local-reference-escapes`.

A local reference escapes when it reaches one of these positions, and each is an error:

| Rule | A local reference | Example | Error |
| --- | --- | --- | --- |
| r[ref.local.escape.field] Field | is stored in a struct field or an enum payload field, or a field's declared type holds one | `point: &Point` | `local-reference-escapes` |
| r[ref.local.escape.element] Collection element | is stored as an element of a list, a map, or a set, by a literal or a method such as `push` | `points.push(p)` | `local-reference-escapes` |
| r[ref.local.escape.top-level] Top-level binding | is bound by a top-level binding | `let origin_ref: &Point = &origin` | `local-reference-escapes` |
| r[ref.local.escape.return] Result | is returned, or a function's or a function type's declared result type holds one | `fn pick(p: &Point) -> &Point` | `local-reference-escapes` |
| r[ref.local.escape.capture] Closure capture | is captured by a closure, a captured `self` included | `fn() -> i32: p.x` | `local-reference-escapes` |
| r[ref.local.escape.type-argument] Type argument | is a type argument, written or inferred, an optional's included | `List[&Point]`, `&Point?` | `local-reference-escapes` |

6. r[ref.local.escape.fix] The diagnostic's fix-it writes `*` in place of `&` in the local reference type it reports: the position's own type, as in `point: *Point`, or the escaping value's declared type, as in `p: *Point`.
7. r[ref.local.escape.fix.inferred] For a binding whose type is inferred from a local reference, as `q := p` with `p: &Point`, the fix-it writes `*` in the declared type it was inferred from, so `p: *Point`.
8. r[ref.local.escape.message] The diagnostic's message names the position the reference escapes to, a row of the table above or a counted reference, and says that writing `*` makes the reference counted.

```text
struct Point:
    x: i32

struct Holder:
    point: &Point  # error: local-reference-escapes

origin := Point { x: 0 }
let origin_ref: &Point = &origin  # error: local-reference-escapes

fn keep(points: &mut List[*Point], p: &Point) -> void:
    points.push(p)  # error: local-reference-escapes

fn pick(p: &Point) -> &Point:  # error: local-reference-escapes
    p

fn reader(p: &Point) -> fn() -> i32:
    fn() -> i32: p.x  # error: local-reference-escapes

fn first(points: List[&Point]) -> i32:  # error: local-reference-escapes
    0
```

> **Why.** Most references are lent for one call, as Swift's `inout`
> parameters are. Writing `*` marks the few that are kept, so a reader
> sees which references outlive a call. Both kinds are still counted, so
> the check changes nothing at run time.

See also: [Captured References](#captured-references),
[Reference Receivers](07-functions.md#reference-receivers).

## Reading Through A Reference

Field access and method calls read through a reference automatically, and `*r` reads the whole value.

```hd
struct Vec2:
    x: f64
    y: f64

impl Vec2:
    fn len2(self) -> f64:
        self.x * self.x + self.y * self.y

fn demo(r: &Vec2) -> f64:
    copy := *r     # a copy of the whole value
    r.x + r.len2() + copy.y
```

1. r[ref.read.auto] A field read or a method call on a reference applies to the value it refers to: `r.x` reads the field `x` of that value, and `r.len2()` calls `len2` on it.
2. r[ref.deref.read] `*r` reads the whole value `r` refers to. The result is a copy.
3. r[ref.deref.kinds] `*r` reads through a counted and a local reference alike.
4. r[ref.deref.not-reference] `*e` on a value that is not a reference is an error. Error: `type-mismatch`.
5. r[ref.read.type] A read through a writable reference, `*mut T` or `&mut T`, has type `T`, and the value it reaches is a writable place.
6. r[ref.read.shallow] A read through a read-only reference, `*T` or `&T`, keeps its declared type; the reference makes only the value it refers to read-only.
7. r[ref.read.index.declared] An index on a reference to a list, map, or string reads an element of the referenced value.

With `Team { name: string, lead: *mut User }` and `t: &Team`:

| Expression | Result |
| --- | --- |
| `t.name = "x"` | Error: `not-writable`; the referenced value is read-only. |
| `t.lead.age = +1` | Valid; `lead` retains `*mut User`. |
| `t.lead.birthday()` | Valid for a `&mut self` method. |
| `x := t.lead` | `x` has type `*mut User`. |
| `*t` | A copy of type `Team`, with its declared field types. |

```text
struct User:
    age: i32

struct Team:
    name: string
    lead: *mut User

fn copy(t: &Team) -> Team:
    *t

fn dup[T](t: &T) -> T:
    *t

fn edit(t: &Team) -> void:
    x := t.lead
    x.age = +1
```

## Matching Through A Reference

A pattern that tests a value reads through a reference, as a field access does, so a recursive enum is matched through its `*` links.

```hd
enum Expr:
    Num(n: i64)
    Add(left: *Expr, right: *Expr)

fn eval(e: &Expr) -> i64:
    match e:
        .Num(n) => n
        .Add(.Num(a), r) => a + eval(r)
        .Add(l, r) => eval(l) + eval(r)
```

1. r[ref.match.read] A variant, tuple, data, literal, or range pattern matched against a reference of either kind, `*T`, `*mut T`, `&T`, or `&mut T`, matches the value the reference refers to.
2. r[ref.match.nested] The rule applies at every depth: a nested pattern reads through a payload field, a tuple element, or a struct field of reference type.
3. r[ref.match.binding] A binding or `_` matched against a reference takes the reference itself, so `whole => whole is e` is `true`.
4. r[ref.match.copy] A payload or field that a pattern binds below such a read is a copy of it, read at the time of the match. A payload is not a place.
5. r[ref.match.declared-type] A binding copied by a pattern through a reference keeps its declared type.

```text
enum Holder:
    Empty
    Cell(value: *mut i64)

fn reset(h: &Holder) -> void:
    match h:
        .Cell(value) =>
            *value = 0
        .Empty => pass
```

> **Why.** Payloads are not places, so a match never lends part of an
> enum. A binding is a copy, and a later write to the matched place does
> not change it; an enum's layout can then change between variants under
> no reference.

See also: [Payload Bindings](06-control-flow.md#payload-bindings),
[Exhaustiveness](06-control-flow.md#exhaustiveness).

## Writing Through A Reference

`*r = v` replaces the value `r` refers to, and a field assignment through a writable reference writes the referenced value's field.

```hd
struct Point:
    x: i32
    y: i32

fn reset(r: &mut Point) -> void:
    *r = Point { x: 0, y: 0 }
    r.x += 1
```

1. r[ref.deref.write] `*r = v` and `*r op= v` replace the value `r` refers to. `r` must be a writable reference, `*mut` or `&mut`. Error: `not-writable`.
2. r[ref.write.field] Assigning a field through a reference, as in `r.x = v`, writes the field of the referenced value. The reference must be a writable one, `*mut` or `&mut`. Error: `not-writable`.
3. r[ref.write.seen] The write is seen through every reference to the place, and through the place itself.
4. r[ref.write.no-exclusivity] A writable reference, `*mut T` or `&mut T`, is not exclusive: other references to the same place may exist and be used, and no check limits them.
5. r[ref.write.index] An element store through a reference to a list or a map, as in `r[i] = v` or `r[i] op= v`, stores into the referenced value. A change of that value through a method call, as in `r.push(x)`, changes it in place as well. The reference must be a writable one, `*mut` or `&mut`. Error: `not-writable`.

```text
struct Point:
    x: i32
    y: i32

fn invalid(r: &Point) -> void:
    *r = Point { x: 0, y: 0 }  # error: not-writable
    r.x = 1                    # error: not-writable
```

> **Why.** A counted reference never dangles, so a write through another
> path cannot free what a reference reads; exclusivity would forbid only
> aliasing that is safe.

## Identity

`r is s` is true when the references `r` and `s` refer to the same place.

```hd
@derive(Eq)
struct Point:
    x: i32
    y: i32

fn demo() -> bool:
    let mut p = Point { x: 1, y: 2 }
    let mut q = Point { x: 1, y: 2 }
    r := &mut p
    s := &p
    t := &q
    r is s && !(s is t) && *s == *t
```

1. r[ref.identity.same-place] `r is s` with two references is true when both refer to the same place, and false otherwise. Two places that hold equal values are different places.
2. r[ref.identity.places] Two references refer to the same place when they start at the same root and read the same struct fields from it. A root is one execution of a local's declaration, one call's parameter, a top-level binding, or one new object.
3. r[ref.identity.copies] A copy of a reference, a reference converted to another kind or to read-only, and a reference passed to or returned from a function refer to the place the original refers to. So do `&*r` and `&mut *r`.
4. r[ref.identity.mutability] `is` compares references of either kind and either mutability, such as a `&T` with a `*mut T`, since all may refer to one place.
5. r[ref.identity.incompatible] `is` between references to different types is an error. Error: `incompatible-identity-operands`.
6. r[ref.identity.non-reference] `is` between a reference and a value without identity, such as a struct, is an error. Error: `identity-requires-references`.
7. r[ref.identity.values] A struct, tuple, or enum value has no identity, even when it holds references, so `is` on one is an error. Error: `identity-requires-references`.
8. r[ref.identity.optional] An optional reference, `*T?`, holds one reference or none. The reference matched out of `.Some(r)` refers to the place `r` refers to.

```text
struct Point:
    x: i32

fn same(a: &i32, b: &i64, p: &Point, v: Point) -> void:
    one := a is b  # error: incompatible-identity-operands
    two := p is v  # error: identity-requires-references
```

> **Why.** A counted reference in hd never dangles, so code may
> keep one in a field or a list. Code that keeps references needs to ask
> whether two of them are the same object, as a graph walk or an
> observer list does; `is` answers without reading the values.

### Equality Of References

`==` between two references is an error, because `is` and `*r == *s` say which comparison is meant.

1. r[ref.identity.no-equality] `==` and `!=` with two reference operands are an error. Error: `reference-equality`.
2. r[ref.identity.no-equality.fix] The diagnostic has two fix-its: one writes `r is s`, or `!(r is s)` for `!=`, and compares places; the other writes `*r == *s`, or `*r != *s`, and compares values.
3. r[ref.identity.generic-eq] A reference type implements `Eq` when its value type does, and `Hash` when its value type does. They compare and hash the values the references refer to, as [Reference Equality](../std/cmp.md#reference-equality) states. So generic code and a derived `Eq` compare through references.

```text
fn same(a: &i32, b: &i32) -> bool:
    a == b  # error: reference-equality
```

> **Why.** Go's `==` on pointers compares addresses, and Rust's compares
> contents. An author coming from either language would guess wrong half
> the time, so neither guess compiles.

See also: [Derived Implementations](09-traits.md#derived-implementations),
[Identity](05-expressions.md#identity).

## Element References

`&xs[i]` and `&mut xs[i]` refer to the slot `i` of the list `xs`, and `&m[k]` and `&mut m[k]` to the value of the key `k` in the map `m`. An element reference lives for one call: it is passed to the call, and no name keeps it.

```hd
struct Player:
    name: string
    score: i32

fn reward(player: &mut Player, points: i32) -> void:
    player.score += points

fn rewarded(players: List[Player], best: usize) -> List[Player]:
    let mut out = players
    reward(&mut out[best], 10)  # changes out[best] during the call
    out[best].score += 1        # the element is a place without a reference too
    out.push(Player { name: "new", score: 0 })  # the call's access has ended
    out
```

1. r[ref.elem.place] `&xs[i]` and `&mut xs[i]`, where `xs` is a list, are **element references**: they refer to the slot `i` of the list. A struct field read from the element extends the place, as in `&mut xs[i].pos`, and an element of an inner list is a slot of that list, as in `&mut grid[r][c]`.
2. r[ref.elem.map] `&m[k]` and `&mut m[k]`, where `m` is a map, are element references too: they refer to the value of the key `k` in `m`. Each rule of this section holds for them, with the key in place of the index.
3. r[ref.elem.index] The index is evaluated and checked once, when the reference is taken. An index that is not less than the list's length is a checked runtime panic. Panic: `index-out-of-bounds`.
4. r[ref.elem.map.key] A key that the map does not hold is a checked runtime panic when the reference is taken, as a read of `m[k]` is. Panic: `index-out-of-bounds`.
5. r[ref.elem.slot] An element reference names a slot, never the value in it. A read through it sees whatever value its slot holds, and a write replaces that value. After elements move between slots, as a sort or a swap moves them, it sees the element in its slot.
6. r[ref.elem.writable] `&mut xs[i]` needs `xs` in a writable place, as an element store does ([`types.coll.writable.store`](04-type-system.md#r-types.coll.writable.store)). Its fix-it makes the binding writable. Error: `not-writable`.
7. r[ref.elem.writable.shared] `&xs[i]` needs no writable place, so it may refer into a `:=` list or a value parameter.
8. r[ref.elem.through-reference] `xs` may be a reference to a list: `&r[i]` and `&mut r[i]` refer to the slot `i` of the list `r` refers to. Through a read-only reference, `*T` or `&T`, `&mut r[i]` is an error. Error: `not-writable`.
9. r[ref.elem.copy] Copying a pinned list gives a list with elements of its own. A change to the copy never reaches a reference into the original, and a write through such a reference never reaches the copy.
10. r[ref.elem.replace] Assigning a whole list to the pinned list's place, as in `xs = []`, is allowed. The old references keep the old elements and no longer refer into `xs`.
11. r[ref.elem.identity] Two element references refer to the same place when they refer to the same slot of the same list's elements and read the same struct fields from it.

```text
fn bump(score: &mut i32) -> void:
    *score += 1

fn invalid() -> void:
    xs := [+1, 2]
    bump(&mut xs[0])  # error: not-writable
```

> **Why.** A list grows by moving its elements. Go keeps the old array
> alive, so a write through an old pointer silently stops reaching the
> slice. A reference that lives for one call never sees the list move.
> Code that keeps an element between calls keeps its index. A handle that
> survives a sort is a reference stored in the list, `List[*mut Player]`,
> or an ID.

### Call-Scoped Access

An element reference is an argument of one call, and the collection's
length holds still until that call returns.

```text
fn bump(score: &mut i32) -> void:
    *score += 1

fn record(scores: &mut List[i32], score: &mut i32) -> void:
    scores.push(*score)

fn demo() -> void:
    let mut scores = [+1, 2]
    bump(&mut scores[0])                 # valid: the access ends with the call
    record(&mut scores, &mut scores[0])  # error: conflicting-access
    best := &mut scores[1]               # error: element-reference-outlives-call
```

1. r[ref.elem.call] An element reference may be written only as an argument of a function or method call, or as the receiver of a method call.
2. r[ref.elem.access] The reference's **access** to its collection begins when the reference is taken and ends when the call returns.
3. r[ref.elem.kept] Any other use of an element reference is an error: binding it to a name, assigning it, storing it in a field, an element, or a payload, and returning it. Error: `element-reference-outlives-call`.
4. r[ref.elem.kept.fix] The diagnostic's fix-it keeps the index instead: each use of the reference names the element again, as `xs[i]`.
5. r[ref.elem.access.length] While an access lasts, nothing may change the collection's length. Reads of the collection, and writes to its elements, the referenced one included, stay valid.
6. r[ref.elem.unaliased] A collection is **unaliased** when it is a local binding or a value parameter, or a struct field or element reached from one through no reference, and no escaping closure captures that binding.
7. r[ref.elem.escaping] A closure is **escaping** when it is passed as an argument, returned, stored in a field, an element, a payload, or a top-level binding, or captured by an escaping closure.
8. r[ref.elem.conflict] During an access to an unaliased collection, another argument or the receiver of the same call that may change the collection's length is an error. Error: `conflicting-access`.
9. r[ref.elem.access.pin] Every access **pins** its collection while it lasts, so a length change that no rule rejects at compile time panics.
10. r[ref.elem.pinned] While a list is pinned, an operation that changes its length is a checked runtime panic: `push`, `insert`, `remove_at`, and a `pop` or a `clear` that removes an element. Panic: `collection-pinned`.
11. r[ref.elem.pinned.map] While a map is pinned, a store of a key it does not hold and a `remove` of a key it holds are checked runtime panics. Panic: `collection-pinned`.
12. r[ref.elem.pinned.allowed] Replacing an element keeps the length, so it is allowed while the list is pinned: an element store, a write through an element reference, and a sort or a swap written with element stores.
13. r[ref.elem.pinned.end] A collection that several accesses pin stays pinned until each has ended.
14. r[ref.elem.callee-copy.declared] A reference parameter is one whose declared type is a reference, `*T`, `*mut T`, `&T`, or `&mut T`. A parameter declared as a type parameter `T` is not one, even when `T` is a reference type: an element reference passed for it is a value the callee may store, so passing it is keeping it ([`ref.elem.kept`](#r-ref.elem.kept)). Error: `element-reference-outlives-call`.

An argument may change the collection's length when it does one of the
following to the collection, or to a place that holds it:

| Rule | The argument or receiver | Example |
| --- | --- | --- |
| r[ref.elem.conflict.reference] Reference | takes `&mut` of it | `record(&mut scores, &mut scores[0])` |
| r[ref.elem.conflict.method] Method call | calls a `&mut self` method on it | `place(&mut scores[0], scores.remove_at(1))` |

> **Note.** An implementation may omit a pin where it can prove that no
> reference reaches the collection and that no callee keeps the element
> reference.

> **Why.** This is Swift's Law of Exclusivity, narrowed to length. A
> conflict is caught at compile time where every path to the collection
> is in view, and by the pin where a reference or a global may reach it.
> Writes to other elements cannot move the referenced slot, so they stay
> legal.

See also: [Element Reference Loops](06-control-flow.md#element-reference-loops),
[Collection Values](04-type-system.md#collection-values),
[Map Values](04-type-system.md#map-values).

## Captured References

A closure that captures a counted reference copies the reference, so the closure and its scope refer to the same place:

```hd
struct Point:
    x: i32
    y: i32

fn bump(p: *mut Point) -> fn() -> void:
    fn() -> void: p.x = p.x + 1

fn demo() -> i32:
    origin := box(Point { x: 0, y: 0 })
    step := bump(origin)
    step()
    step()
    origin.x
```

1. r[ref.capture.copy] Capturing a counted reference copies the reference. The copy and the original refer to the same place, as any two copies of a reference do ([`ref.place.alias`](#r-ref.place.alias)).
2. r[ref.capture.write-through] A write through a captured `*mut T` changes the place it refers to, and every other path to that place sees the write.
3. r[ref.capture.variable] A captured `let` variable that holds a counted reference is shared like any variable. Assigning it another reference changes the place that the closure and its scope both use.

> **Why.** A reference already names a place, so copying it shares that
> place.

4. r[ref.capture.local] A closure never captures a local reference, as [`ref.local.escape.capture`](#r-ref.local.escape.capture) states.

See also: [Copied And Shared Captures](07-functions.md#copied-and-shared-captures),
[`ref.read.shallow`](#r-ref.read.shallow).

## Not Yet Checked

1. r[ref.check.not-yet-tuple-and-index] Two forms are errors until later jobs define them. One is a reference to a tuple element, as in `&pair._0`. The other is a reference to an index result that is neither a list element nor a map value, such as a slice `&xs[1..3]` or a user `Index` call. Error: `references-not-yet-checked`.
2. r[ref.check.formatter] `hd fmt` formats a file that writes references. It keeps each reference token as written, `&mut` and `*mut` included, and a second run changes nothing.

```text
fn element(items: List[i32], pair: (i32, i32)) -> void:
    part := &items[0..1]  # error: references-not-yet-checked
    left := &pair._0  # error: references-not-yet-checked
```
