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

1. r[ref.type.forms] `&T` is a local reference type, and `&mut T` is a writable local reference type. The operand `T` is a named, `dyn`, tuple, or grouped type, or a counted reference type with no parentheses: `&*T`, `&*mut T`, and `&mut *T`.
2. r[ref.type.counted-forms] `*T` is a counted reference type, and `*mut T` is a writable counted reference type. Their operand is a named, `dyn`, tuple, or grouped type.
3. r[ref.type.kinds] Counted and local are the two kinds of reference. A rule of this chapter that says "a reference" holds for both kinds.
4. r[ref.type.kinds.elsewhere] A rule in another chapter that writes `&T` or `&mut T` for a reference that is read, written, matched, compared, or printed holds for `*T` and `*mut T` too. It does not extend which implementations a reference type has: [`ref.type.kind-impl`](#r-ref.type.kind-impl) alone decides that. This rule is temporary, as the note below says.
5. r[ref.type.local-uncounted] A local reference counts nothing at run time: making, copying, or ending one changes no count. Only a counted reference holds a count of the object it refers to.
6. r[ref.type.position] A counted reference type may appear wherever a type may: a parameter, a result, a field, a binding annotation, a type argument, and a function type's parameters and result.
7. r[ref.type.optional] `?` applies to the whole reference type: `*T?` is an optional `*T`.
8. r[ref.type.group] To refer to an optional, to a local reference, or from a counted reference to another reference, group the operand: `&(T?)`, `&(&T)`, and `*(*T)`. `&(*T)` is the same type as `&*T`.
9. r[ref.type.no-nesting] `&&T` is a syntax error, because `&&` is one token. So are `&mut mut T`, `mut &T`, and `*&T`. Error: `syntax-error`.
10. r[ref.type.counted-no-nesting] `**T` is a syntax error too, because `**` is one token, so a counted reference to one is written `*(*T)`. So are `*mut mut T` and `mut *T`. Error: `syntax-error`.
11. r[ref.type.no-function] `&` never directly precedes `fn`: `&fn() -> i32` and `&mut fn() -> i32` are errors. Error: `syntax-error`.
12. r[ref.type.counted-no-function] `*` never directly precedes `fn` either: `*fn() -> i32` and `*mut fn() -> i32` are errors. Error: `syntax-error`.
13. r[ref.type.distinct] `&T` and `&mut T` are distinct types. Each is invariant in `T`: `&A` is `&B` only when `A` is `B`.
14. r[ref.type.kind-distinct] `*T` and `*mut T` are distinct types, and distinct from `&T` and `&mut T`. Each is invariant in `T`.
15. r[ref.type.kind-impl] An implementation for a reference type covers that kind only. One for `*T` does not cover `&T`, and one for `&T` does not cover `*T`; so too for `*mut T` and `&mut T`. Using a `&T` where only `*T` implements a bound is an error. Error: `unsatisfied-trait-bound`.
16. r[ref.type.any-ref] A reference type implements `AnyRef`.
17. r[ref.type.dyn] `&dyn Tr` and `&mut dyn Tr` are trait value references, whose conversions and members [Trait Value References](09-traits.md#trait-value-references) states.
18. r[ref.type.dyn-counted] `*dyn Tr` and `*mut dyn Tr` are counted trait value references. They take the same conversions and members, with `*` in place of `&` on both sides of each conversion.
19. r[ref.type.dyn.no-fn] `Fn` has no trait-value reference form, so local and counted `dyn Fn` references are errors. Error: `trait-not-dynamically-safe`.
20. r[ref.type.dyn.no-fn-mut] `FnMut` has no trait-value reference form either. Error: `trait-not-dynamically-safe`.

> **Note.** [`ref.type.kinds.elsewhere`](#r-ref.type.kinds.elsewhere)
> bridges the chapters until the respelling slices of
> [#834](https://github.com/HerringtonDarkholme/hd-lang/issues/834) spell
> each such rule for both kinds. Those slices then remove it.

> **Why.** A counted `*T` holds a count and a local `&T` holds none, so
> one compiled body cannot serve both kinds. As in Rust, an implementation
> for one reference type covers that type alone, and code that needs both
> kinds writes both implementations.

```text
use std.function.{Fn, FnMut}

trait Named:
    fn name(&self) -> string

impl Named for *Point:
    fn name(&self) -> string: "point"

fn label[T < Named](value: T) -> string: value.name()

fn bad_kind(p: &Point) -> string: label(p)  # error: unsatisfied-trait-bound
fn bad_and(p: &&Point) -> void: pass  # error: syntax-error
fn bad_mut(p: mut &Point) -> void: pass  # error: syntax-error
fn bad_fn(f: &fn() -> i32) -> void: pass  # error: syntax-error
fn bad_power(p: **Point) -> void: pass  # error: syntax-error
fn bad_dyn(f: &dyn Fn(i32) -> i32) -> void: pass  # error: trait-not-dynamically-safe
fn bad_dyn_mut(f: &mut dyn FnMut(i32) -> i32) -> void: pass  # error: trait-not-dynamically-safe
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

1. r[ref.prec.binary-tighter] `&` and `&mut` are `unary_expression` operators. They bind more tightly than every binary operator except `**`, and less tightly than every postfix suffix.
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
9. r[ref.expr.kind.no-counted.advice] The diagnostic tells the writer to box the value at its declaration, as `p := box(Point { x: 0 })`. It offers no fix-it, since boxing at the use would copy the value.
10. r[ref.place.alias] Two references to one place, or two copies of one reference, see each other's writes.
11. r[ref.place.receiver.local] A call of a `&self` or `&mut self` method on a place takes a local reference to it, as `&` and `&mut` would.
12. r[ref.place.receiver.counted] A call of a `*self` or `*mut self` method on a place is an error, as [`fn.method.counted-self.place`](07-functions.md#r-fn.method.counted-self.place) states. [Reference Receivers](07-functions.md#reference-receivers) states the rules of both calls.
13. r[ref.place.tuple-element] A tuple element of a place is a place for `&` and `&mut`, as a struct field is: `&mut pair._1` refers to the element inside `pair`. The rules this chapter states for a field reference apply to it.

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

1. r[ref.box.declaration] `std.core` declares `box` as `@intrinsic pub fn box[T](value: T) -> *mut T`, and the prelude supplies it. The declaration is ordinary hd, and the compiler supplies only its body.
2. r[ref.box.check] A `box` call is checked by the plain call rules, so `T` is inferred from the argument and the expected type as for any generic call.
3. r[ref.box.new] `box(e)` evaluates `e` once and returns a `*mut T` to a new object that holds the value. Each call makes a new object.
4. r[ref.box.copy] The new object holds a copy of the value. When `e` is a place, a later write to that place is not seen through the result, and a write through the result does not change the place.
5. r[ref.box.weaken] The result converts by the rules of [Conversions](#conversions) and [Trait Value References](09-traits.md#trait-value-references), so it may be used where `*T` or `*mut dyn Tr` is expected.
6. r[ref.box.local] When the type of `e` holds a local reference, so does the type of `box(e)`, by [`ref.local.holding.counted`](#r-ref.local.holding.counted). The result is then local: it follows every rule of [Local References](#local-references), as a `List[&T]` does.
7. r[ref.box.object] Every counted reference refers to a whole object: one that `box` made, or one that another rule names as a new object. No expression makes a counted reference to a local, a parameter, a top-level binding, a field, or an element.
8. r[ref.box.lifetime] A counted reference keeps the object it refers to alive. It may outlive the block that made it, and may be returned.
9. r[ref.box.root] The local or parameter that holds the first counted reference a live local reference's place passes through, its **root**, stays alive until the reference's last use: its release, or a move of its value, waits for that use, in a suspending function's frame too.
10. r[ref.count.pointer] A local reference whose place passes through a counted reference keeps nothing alive. Its root keeps the object alive ([`ref.box.root`](#r-ref.box.root)), and a replacement through another alias panics ([`ref.excl.alias.conflict`](#r-ref.excl.alias.conflict)).
11. r[ref.count.local] A local reference is an address that keeps nothing alive. The local, parameter, field, element or object it points into outlives it ([Local Scope](#local-scope)).
12. r[ref.count.new-object] The new object that `&e` makes ([`ref.expr.allocate`](#r-ref.expr.allocate)) lives until the last use of the reference that made it and of every copy of that reference.

```text
struct Point:
    x: i32

struct Holder:
    point: *(&Point)  # error: local-reference-escapes

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
4. r[ref.place.writable.enum] `&mut` of an enum value in a writable place is valid: a `let mut` local, a `mut` parameter, or a struct field of one. `*r = v` through `&mut E` replaces the whole value. A copied payload binding is no place (`ref.match.copy`), so `&mut` of one is an error by the rows below.
5. r[ref.place.shared-any] `&e` needs no writable place: it may refer to a `:=` binding, a plain `let` binding, or a value parameter.

`&mut` of a place that is not writable is an error, by what holds the place:

| Rule | The place is held by | Error |
| --- | --- | --- |
| r[ref.place.not-writable.using-split] Short binding | a `:=`, `using`, or `using!` binding | `non-reassignable-binding` |
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
7. r[ref.convert.readonly-payload] The same weakening enters an enum argument beneath a readonly reference when every field of the enum that mentions that type parameter is exactly the parameter, a whole payload or shared field, as `T` is in `Option[T]`; a parameter inside a field's type, as in `*mut T` or `List[T]`, keeps its declared variance.
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
3. r[ref.local.holding] A type **holds** a local reference when it is a local reference type, or a tuple, an optional, an instance of a generic type, or a type alias whose type or type argument holds one. An opaque type that [borrows](04-type-system.md#r-types.opaque.borrows) holds one too.
4. r[ref.local.holding.counted] A counted reference type, `*T` or `*mut T`, holds a local reference when `T` holds one.
5. r[ref.local.self.keep] A `&self` or `&mut self` receiver is a local reference, so these rules apply to `self` in such a method. A method that keeps `self` takes `*self` or `*mut self` ([Reference Receivers](07-functions.md#reference-receivers)). A `&self` method may still return a value that depends on `self` ([Dependent Results](#dependent-results)).
6. r[ref.local.escape.counted] A local reference never converts to a counted one. Using one where a counted reference is expected, as for a `*T` parameter or field, is an error. Error: `local-reference-escapes`.

A local reference escapes when it reaches one of these positions, and each is an error:

| Rule | A local reference | Example | Error |
| --- | --- | --- | --- |
| r[ref.local.escape.field] Field | is stored in a struct field or an enum payload field, or a field's declared type holds one | `point: &Point` | `local-reference-escapes` |
| r[ref.local.escape.element] Collection element | is stored as an element of a list, a map, or a set whose element type holds no local reference, by a literal or a method such as `push` | `points.push(p)` | `local-reference-escapes` |
| r[ref.local.escape.top-level] Top-level binding | is bound by a top-level binding | `let origin_ref: &Point = &origin` | `local-reference-escapes` |
| r[ref.local.escape.fn-value-result] Function value result | is the result of a function value whose type gives it no source among the inputs, as for a method bound to its receiver | `p::x_ref` for `fn x_ref(&self) -> &i32` | `local-reference-escapes` |
| r[ref.local.escape.capture-stored] Stored closure capture | is captured by a closure outside a direct `fn(...)` argument, a [type parameter field](#r-ref.generic.outlives.field), and a [type parameter argument](#r-ref.generic.outlives.argument), a captured `self` included | `fn() -> i32: p.x` | `local-reference-escapes` |
| r[ref.local.escape.trait-argument] Trait argument | is a type argument of a trait or a trait value, written or inferred | `dyn Store[&Point]` | `local-reference-escapes` |

7. r[ref.local.escape.position-message] The diagnostic's message names the position the reference escapes to, a row of the table above or a counted reference.
8. r[ref.local.capture-parameter] A closure supplied directly for a `fn(...)` parameter may capture a local reference under [`fn.capture.local-parameter`](07-functions.md#r-fn.capture.local-parameter).
9. r[ref.local.capture-origin] Such a function value carries the captured reference's origin while it remains in the call's dependent scope.

```text
struct Point:
    x: i32

struct Holder:
    point: &Point  # error: local-reference-escapes

origin := Point { x: 0 }
let origin_ref: &Point = &origin  # error: local-reference-escapes

fn keep(points: &mut List[*Point], p: &Point) -> void:
    points.push(p)  # error: local-reference-escapes

impl Point:
    fn x_ref(&self) -> &i32:
        &self.x

fn bound(p: Point) -> i32:
    f := p::x_ref  # error: local-reference-escapes
    *f()

fn reader(p: &Point) -> fn() -> i32:
    fn() -> i32: p.x  # error: local-reference-escapes
```

> **Why.** Most references are lent for one call, as Swift's `inout`
> parameters are. A counted reference is a separate ownership choice, not
> a mechanical repair for a local reference that escapes.

See also: [Captured References](#captured-references),
[Reference Receivers](07-functions.md#reference-receivers).

## Dependent Results

A function's result may hold a local reference that it borrows from its receiver or a parameter, its **source**. Such a result is a **dependent result**.

```hd
struct User:
    name: string
    tags: List[string]

impl User:
    fn name(&self) -> &string:          # depends on self
        &self.name

    fn tags_mut(&mut self) -> &mut List[string]:
        &mut self.tags

    fn named(&mut self, name: string) -> &mut User:
        self.name = name
        self

fn tagged(u: &User) -> &List[string]?:  # one reference position: depends on u
    if u.tags.len() > 0: .Some(&u.tags) else: .None

fn demo(u: &mut User) -> void:
    n := u.name()        # n borrows all of u while it is used
    t := u.tags_mut()
    t.push("x")          # writes u.tags in place
```

1. r[ref.dep.result] A function's or a method's result type may hold a local reference: `&T`, `&mut T`, a tuple of them, or an instance of a generic struct or enum, such as `&T?` or `Result[&T, E]`.
2. r[ref.dep.result.opaque] An opaque result type that borrows is a dependent result too, and its hidden value follows these rules ([Opaque Results That Borrow](04-type-system.md#opaque-results-that-borrow)).
3. r[ref.dep.position] Each `&` or `&mut` in a parameter's type is one **reference position**: in `&self`, in `p: &T`, in `opt: &T?`, and in each element of `pair: (&A, &B)`. A `&` inside a function type is none.
4. r[ref.dep.capture-position] A direct `fn(...)` parameter is one **capture position** with a fresh lifetime, and a direct function result has one capture position too.
5. r[ref.dep.capture-elide] An elided direct function result depends on the one direct function parameter's capture position.
6. r[ref.dep.capture-elide.ambiguous] A direct function result with two or more direct function parameters is an error, since it has no one capture position to depend on. Error: `ambiguous-reference-source`.
7. r[ref.dep.elide] In a signature that writes no lifetime, a dependent result's source is `self` when the receiver is `&self` or `&mut self`. Otherwise it is the parameter of the one reference position.
8. r[ref.dep.elide.none] In a signature that writes no lifetime, a dependent result with no reference receiver and no reference position is an error. Error: `reference-result-without-source`.
9. r[ref.dep.elide.ambiguous] In a signature that writes no lifetime, a dependent result with no reference receiver and two or more reference positions is an error. Error: `ambiguous-reference-source`.
10. r[ref.dep.return] Every value a function returns, by `return` or as its body's value, may depend only on its source. Error: `local-reference-escapes`.
11. r[ref.dep.return.message] The diagnostic's message names the root the value depends on, as a local, a parameter's own storage, or another parameter. It has no fix-it.
12. r[ref.dep.call] A call's dependent result depends on what its source argument depends on.
13. r[ref.dep.call.local] The result is a local reference value of the caller, so every rule of [Local References](#local-references) applies to it. The caller may return it only as [`ref.dep.return`](#r-ref.dep.return) allows.
14. r[ref.dep.call.pointer] A dependent result whose source argument is a `*T` or `*mut T` is a local reference to that argument's root, the local or parameter the pointer was read from, as `&*p` is. So after `r := boxed.slot()` or `r := borrow(boxed)`, a write through `boxed` while `r` is in use is an error ([`ref.excl.call`](#r-ref.excl.call)), and so is a use of `r` outside the scope of the local `boxed` ([`ref.dep.scope`](#r-ref.dep.scope)). Error: `conflicting-access` for the write, `local-reference-outlives-local` for the use.
15. r[ref.dep.trait] An impl's method has the source of the trait method it implements. A call through a bound, an impl, or a trait value reference takes its source from the trait's signature.
16. r[ref.dep.store] A value that holds a local reference, stored through a reference, must land in a place of the function. A store into a place the caller owns, as through `out: &mut (&T)`, is an error, unless the place's type names the value's lifetime ([`ref.lifetime.store`](#r-ref.lifetime.store)). Error: `local-reference-escapes`.
17. r[ref.dep.caller] While a dependent result is in use, the caller may read its source. It may write the source only as [Writes While Borrowed](#writes-while-borrowed) allows. A read through the result sees the source's current value.
18. r[ref.dep.temporary] A dependent result of a call on a temporary, as in `make().name()`, keeps the temporary alive while the result is in use.
19. r[ref.dep.suspension] A suspension, the `Suspend[R]` of a bang call made without `!`, holds every reference that its call's arguments hold, the receiver's included. It holds them in one position of its own, after the positions of `R`.
20. r[ref.dep.suspension.local] A suspension that holds a local reference is a local reference value of the caller, so every rule of [Local References](#local-references) and [`ref.dep.return`](#r-ref.dep.return) applies to it. Error: `local-reference-escapes`.
21. r[ref.dep.suspension.output] Each position of `R` depends on what the call's result would depend on with `!`, by [`ref.dep.call`](#r-ref.dep.call).
22. r[ref.dep.suspension.counted] A suspension retains a counted reference that its call lends, by itself or as the trait value behind it, by [`ref.box.lifetime`](#r-ref.box.lifetime). So it holds no origin for that argument.
23. r[ref.dep.suspension.result] A `Suspend[R]` in a declared result type has that position too. It depends on the one lifetime of the parameters' positions, or on every parameter under two or more. With no parameter position, it depends on nothing.
24. r[ref.dep.suspension.lifetime] `Suspend` takes one optional lifetime argument after its result type, as in `Suspend[R, 'a]`. It names the suspension's own position, as a lifetime struct's argument names one of the struct's positions ([`ref.lifetime.struct.position`](#r-ref.lifetime.struct.position)). More than one is an error. Error: `argument-count`.
25. r[ref.dep.suspension.lifetime.field] A field typed `Suspend[R, 'a]` in a lifetime struct may hold a suspension that holds `'a` data. A field that writes no lifetime for its suspension holds one that holds no local reference.
26. r[ref.dep.suspension.store] A suspension that holds a local reference never reaches a box, a provider, a host call, a top-level binding, or a struct field that is neither of a type parameter nor written with a lifetime. A list, a map, a tuple, or an optional keeps it in its own positions. Error: `local-reference-escapes`.

```hd
fn count!(s: &string) -> usize:
    s.len()

fn later(s: &string) -> Suspend[usize]:  # holds what s refers to
    count(s)

fn now!() -> usize:
    local := "x"
    count!(&local)

struct Pending['a]:
    task: Suspend[usize, 'a]

fn soon(s: &string) -> Pending:  # holds what s refers to
    Pending { task: count(s) }
```

```text
struct User:
    name: string

fn bad_local() -> &string:  # error: reference-result-without-source
    u := User { name: "a" }
    &u.name

fn bad_pick(a: &User, b: &User) -> &User:  # error: ambiguous-reference-source
    a

fn bad_compose(f: fn(i32) -> i32, g: fn(i32) -> i32) -> fn(i32) -> i32:  # error: ambiguous-reference-source
    fn(x: i32) -> i32: g(f(x))

fn bad_value(u: User, other: &User) -> &string:
    &u.name  # error: local-reference-escapes

fn bad_store(out: &mut (&User), u: &User) -> void:
    *out = u  # error: local-reference-escapes

fn hold!(r: &string) -> usize:
    r.len()

fn bad_suspension() -> Suspend[usize]:
    local := "x"
    hold(&local)  # error: local-reference-escapes
```

> **Why.** These are Rust's elision rules for a result's lifetime, with no
> lifetime written. [Lifetime Variables](#lifetime-variables) gives the
> rules for a signature that writes one. A write to the source replaces the value in its place,
> and the place stays where it is, so the caller needs no exclusive access.
> A suspension holds its arguments as a Rust `async fn` future captures
> every input lifetime.

### Origins

An **origin** is a storage root that a local reference may point into. It is what a parameter refers to, which the caller owns, or a local of the function, the hidden local of `&e` on a value included. It may also be a parameter's own storage, which the call owns.

1. r[ref.dep.origin.set] Each value that holds a local reference has an origin set. The checker solves the sets once per function, per binding, regardless of the order of statements.
2. r[ref.dep.origin.least] The sets are the least solution of the constraints in the table below. A reassignment adds origins and never removes them.
3. r[ref.dep.origin.position] A value has one origin set per reference position outside another reference, so the positions of `(&A, &B)` and of `Result[&A, &B]` have separate sets.
4. r[ref.dep.origin.position.parts] A construction and a projection keep each part in its own positions: a tuple element, an optional's payload, and a field whose declared type is a type parameter. So does a lifetime struct's field of one lifetime ([`ref.lifetime.struct.read`](#r-ref.lifetime.struct.read)).
5. r[ref.dep.origin.position.union] Every position takes the union of all of them for any other part and for the hidden local of `&e`. A reborrow of a part, shared or `&mut`, is not such a part: `r := &t.x` and `r := &mut t.inner` keep that part's own positions. Nor is a store through a reference: `*r = v` and `r.x = v` put each position of `v` into its own position of the place.

| Rule | Expression | Origins |
| --- | --- | --- |
| r[ref.dep.origin.param] Parameter | `p`, for a parameter whose type holds a local reference | what `p` refers to |
| r[ref.dep.origin.place] Place | `&x` or `&x.f.g`, or the automatic address a method call takes of such a place, for a local or a parameter `x` | `x`'s own storage |
| r[ref.dep.origin.element] Element place | `&xs[i]`, `&m[k]`, or an element reference loop binding | the origin of the collection's root place |
| r[ref.dep.origin.reborrow] Reborrow | `&r.f`, `&*r`, or an automatic address through a reference value `r` | the origins of `r` |
| r[ref.dep.origin.counted] Counted crossing | a place reached through a counted reference, as `&h.p.x` with `p: *mut P` | the origins of the place the counted reference was read from |
| r[ref.dep.origin.lend] Lent counted reference | a counted reference read from a place and lent as a source argument | that place's origins |
| r[ref.dep.origin.temporary] Temporary | `&e` on a value, and a `&self` call on a temporary | a hidden local |
| r[ref.dep.origin.binding] Binding | a read of a binding `b` | the union of every value stored into `b` anywhere in the function |
| r[ref.dep.origin.parts] Construction | a tuple, `.Some(a)`, or a struct or enum literal | the union of its parts, each in its positions |
| r[ref.dep.origin.projection] Projection | `t._0`, or a pattern binding of a scrutinee `s` | the origins of the positions of `t`, or of `s`, that it reaches |
| r[ref.dep.origin.branch] Branch | `if` or `match` as a value | the union of its arms |
| r[ref.dep.origin.call] Call | a call with a dependent result, whose source argument is `a` | the origins of `a` |
| r[ref.dep.origin.try] Wrap | `x?`, and an implicit `.Some` | the origins of `x` |

```text
struct User:
    name: string

fn walk(u: &User) -> &string:
    let mut r = &u.name            # r gets what u refers to
    spare := User { name: "x" }
    if r.len() == 0:
        r = &spare.name            # r also gets spare
    r  # error: local-reference-escapes

fn optional(u: &User) -> &string?:
    let o: &string? = .Some(&u.name)
    match o:
        .Some(n) => .Some(n)       # valid: n has the origins of o
        .None => .None
```

> **Why.** Per-binding sets are the regions of Rust's non-lexical
> lifetimes without their flow sensitivity. A later stage may make them
> flow-sensitive and accept more programs.

### Local Scope

A local reference may point into a local of the function only while that local exists.

1. r[ref.dep.scope] Every use of a value whose origin set holds a local of the function, other than a parameter, must lie in that local's scope. Error: `local-reference-outlives-local`.
2. r[ref.dep.scope.binding] A binding is in scope from the end of the statement that declares it to the end of the suite that holds that statement. A `match` arm's binding is in scope in its arm, and a `for` loop's binding in the loop's body.
3. r[ref.dep.scope.temporary] The hidden local of `&e` is in scope from the start of the statement that holds `&e` to the end of the suite that holds that statement.
4. r[ref.dep.scope.loop] A local declared in a loop's body is out of scope in the loop's later iterations. Since origin sets ignore the order of statements, a read inside the loop of a binding declared outside it, whose origin set holds such a local, is a use in a later iteration.
5. r[ref.dep.scope.message] The diagnostic's message names the binding and the local, as in "`a` may point into `c`, whose block has ended". It has no fix-it.

```text
fn pick(x: i32, b: bool) -> i32:
    let a: &i32 = &x
    if b:
        c := +123
        a = &c
    *a  # error: local-reference-outlives-local

fn last(x: i32) -> i32:
    let a: &i32 = &x
    for i in 0..3:
        c := +4
        a = &c
    *a  # error: local-reference-outlives-local

fn total(x: i32) -> i32:
    let a: &i32 = &x
    let sum = +0
    for i in 0..3:
        sum += *a  # error: local-reference-outlives-local
        c := +4
        a = &c
    sum
```

Storage that must outlive its block is boxed at its declaration, and the binding holds a `*T`.

> **Why.** This is Rust's E0597. Without it, such a local would need counted
> storage that no `box` made, and `&` would not be truly local.

See also: [Element References](#element-references),
[Reference Receivers](07-functions.md#reference-receivers).

### Writes While Borrowed

While a shared reference to a place is in use, the place may be read but not changed or moved. While a `&mut` reference to it is in use, the place may not be used at all except through that reference.

```text
struct User:
    name: string
    age: i32

impl User:
    fn name(&self) -> &string:
        &self.name
    fn age_mut(&mut self) -> &mut i32:
        &mut self.age

fn rename(u: &mut User) -> void $ Console:
    n := &u.name
    u.age += 1    # valid: another field
    u.name = "b"  # error: conflicting-access
    println(*n)   # the last use of n
    u.name = "c"  # valid

fn birthday(u: &mut User) -> void $ Console:
    n := u.name() # n borrows all of u
    println(u.age) # valid: a read
    u.age += 1    # error: conflicting-access
    println(*n)   # the last use of n
    u.age += 1    # valid

fn tag(u: &mut User) -> void $ Console:
    a := u.age_mut() # a borrows all of u exclusively
    println(u.age)   # error: conflicting-access
    *a += 1          # valid: through a, its last use
    println(u.age)   # valid

fn set_name(to: &mut string, from: &string) -> void:
    *to = *from

fn copy_name(u: &mut User, v: &User) -> void:
    set_name(&mut u.name, &u.name) # error: conflicting-access
    set_name(&mut u.name, &v.name) # valid: another place
```

1. r[ref.excl.shared] While a shared reference `&p` is live, a write to `p` or to a place that overlaps it is an error. Error: `conflicting-access`.
2. r[ref.excl.mut] While a `&mut p` is live, a use of `p` or of a place that overlaps it is an error, except through that reference. A use is a read, a write, a `&p`, another `&mut p`, or a closure that captures it. Error: `conflicting-access`.
3. r[ref.excl.place] Explicit references and implicit counted-to-local conversions follow these rules for local or parameter places, including fields and dereferences. Two places of one local overlap unless their paths name two different fields at the same depth.
4. r[ref.excl.write] A write is an assignment to the place or a part of it, a `&mut` of it that a binding keeps, or a call that takes `&mut` of it or a `&mut` binding to it. A `&mut self` method call is such a call. A built-in change writes too: a list's `push` or `reserve`, a map store, and a map's `remove`. A call's `&mut` writes when the call runs, after its other arguments.
5. r[ref.excl.move] While a reference to `p` is live, a move of `p` or of a place that overlaps it is an error. A move is a read that [`ref.move.sites`](#r-ref.move.sites), [`ref.move.field`](#r-ref.move.field) or [`ref.move.pattern`](#r-ref.move.pattern) makes a move. A copied value does not move, so reading it is valid. Error: `conflicting-access`.
6. r[ref.excl.live] The reference is live until the last use of a binding whose origin set holds it, not until the end of the block. A use that the write or move cannot reach does not count: one in another arm of the same branch, or one after a `return`. A use earlier in a loop counts when the binding is declared outside the loop.
7. r[ref.excl.reassign] Assigning the binding a value whose origin set does not hold the reference ends the reference's use through it.
8. r[ref.excl.declare] A `let` or `:=` makes a new local, so it writes no place a reference from an earlier iteration of its loop points into.
9. r[ref.excl.call] The `&` a call takes for a `&self` receiver or a `&` parameter is a shared reference to the whole place it was taken from. A `&mut` binding that a call takes as `&` lends the place it refers to in the same way. A `&mut` a call takes, a passed-on `&mut` binding included, lends the whole place as a `&mut` reference. A call's result that depends on that reference holds it ([`ref.dep.call`](#r-ref.dep.call)), even where the result points into one field, unless its signature names that field ([`ref.result.path.caller`](#r-ref.result.path.caller)). A result of type `&mut T` holds that reference as a `&mut` one after the call, whatever the receiver or parameter kind, as when a `&self` method returns a `&mut` through a `*mut` field.
10. r[ref.excl.args] While a call takes a `&mut` of a place, its other arguments and its receiver may not hold a reference to an overlapping place. An element reference falls under [`ref.elem.conflict`](#r-ref.elem.conflict) instead. Error: `conflicting-access`.
11. r[ref.excl.args.after] An argument after a written `&mut p` argument may not use `p` or an overlapping place. The arguments of a `&mut self` method call may read the receiver's place, as in `xs.push(xs.len())`. Error: `conflicting-access`.
12. r[ref.excl.args.shared] An argument after a `&` of `p` that the call takes, written or for a `&self` receiver, may not write or move `p` or an overlapping place: `referred(&ys, ys.pop())` is an error, and `referred(&ys, ys.len())` is valid. An element reference falls under [`ref.elem.conflict`](#r-ref.elem.conflict) instead. Error: `conflicting-access`.
13. r[ref.excl.args.pointer] A `*T` or `*mut T` argument accesses its whole pointee for the call's other arguments: a `*mut` writes it and a `*T` reads it. So `inspect(view, &mut view.n)` and `mutate(boxed, &boxed.n)` are errors, and `look(view, &view.n)` is valid. Passing a pointer is no access at run time. Error: `conflicting-access`.

```text
struct Bag:
    labels: List[string]

impl !Copy for Bag

fn consume(bag: Bag) -> usize:
    bag.labels.len()

fn count(bag: Bag, names: List[string]) -> usize:
    r := &bag
    s := &names
    copy := names     # valid: a list is copied
    k := r.labels.len() + s.len() + copy.len()
    n := consume(bag) # valid: after the last use of r
    n + k

fn early(bag: Bag) -> usize:
    r := &bag
    n := consume(bag) # error: conflicting-access
    n + r.labels.len()
```

> **Why.** This is Rust's E0506, E0503, E0499 and E0505 with non-lexical
> lifetimes. A write while a shared reference is live would change what the
> reader sees behind its back, and a `&mut` holder expects the same of its
> place. A move would leave the reference pointing at a value that another
> owner may free.
>
> A call's result locks its whole source, as in Rust, unless its
> signature names the field the result points into ([Result Paths](#result-paths)). The fixes are
> Rust's: make the change after the last use, copy the value out first
> (`name := *u.name()`), or borrow only the field the code reads (`&u.name`).

### Conflicts Through Aliases

Another `*mut` to the same object is a path the compile-time rules do not see. Through it, a conflicting access to an object that a binding borrows panics at run time, as Rust's `RefCell` does.

```text
struct Team:
    lead: *mut User

fn rename(t: &Team, alias: *mut User) -> void $ Console:
    n := &t.lead.name
    alias.age += 1     # panics borrow-conflict when alias is t.lead: n holds all of it
    println(*n)        # the last use of n
    alias.name = "c"   # valid
    m := t.lead.name() # a result of `fn name(&self) -> &string`
    alias.age += 1     # panics borrow-conflict: m holds all of t.lead
    println(*m)
```

1. r[ref.excl.alias.loan] A **run-time loan** is a `&p` or `&mut p` that a `:=` or `let` binding of type `&T` or `&mut T` holds, or that an optional, tuple or struct holds. It holds it directly, through a call's result that depends on it ([`ref.excl.call`](13-values-and-references.md#r-ref.excl.call)), or from another such holder by a copy or a move. The loan holds each object that a dereference of a `*mut` or a `*T` in `p`'s path reaches. A `&mut p` holds the last such object exclusively; every other object, and every object of a `&p`, is held shared. A `*mut` that a call takes for a `&self` or `&mut self` receiver lends the whole object it refers to. The loan is live until the last use of every binding that holds it and the release of every value that contains it ([`ref.excl.live`](#r-ref.excl.live)).
2. r[ref.excl.alias.counted-access] An **access through a counted reference** is a use of a place whose path dereferences a `*mut` or a `*T`. An assignment, a `&mut` and a built-in change of the place write the object its last dereference reaches; a read and a `&` read it. Every earlier object on the path is read. Passing a `*mut` or a `*T` to a call is no access at run time; the callee's uses of it are.
3. r[ref.excl.alias.conflict] An access through a counted reference panics when it writes an object a live run-time loan holds, or reads an object a live loan holds exclusively. A loan that the accessing function took through the same pointer, with the same root and path, does not conflict with it. [`ref.excl.place`](#r-ref.excl.place) already checks that pair. Panic: `borrow-conflict`.

4. r[ref.excl.alias.shared-counted-loan] A reference held directly or through a call's result from a place reached through a `*T` takes the same run-time loan as one reached through a `*mut`.
5. r[ref.excl.alias.shared-variable] A local variable that a closure shares is an object, as a `*mut` pointee is. A reference whose place starts at the variable holds that object, and each use of the variable, in the closure or in its own function, is an access through a counted reference.
6. r[ref.excl.alias.global] A top-level binding is an object. A reference whose place starts at the binding holds that object, and each use of the binding, from any function, is an access through a counted reference.
7. r[ref.excl.alias.call-loan] A `&` or `&mut` argument or auto-ref receiver whose path dereferences a `*mut` or a `*T` takes a run-time loan from its evaluation until its call returns. A `&mut self` receiver's loan is shared until the call begins, then exclusive. A closure argument holds the run-time loans of the references it captures until its call returns.
8. r[ref.excl.alias.path-pointer] A run-time loan holds each object its place's path reads a `*mut` or a `*T` from, so replacing such a pointer slot conflicts until the loan ends, in every build.
9. r[ref.excl.alias.untracked-path] A reference whose place passes through no `*T`, no variable a closure shares and no top-level binding a reference reaches takes no run-time loan, so an alias reads or writes its place in place.

### Lifetime Variables

A **lifetime variable** `'a`, written after `&`, names the lifetime of a local reference. The reference positions that write one variable share its lifetime:

```text
struct User:
    name: string

fn choose['a](a: &'a User, b: &'a User, first: bool) -> &'a User:
    if first: a else: b  # valid: a and b both have 'a

fn third['a](a: &'a User, b: &'a User, c: &User) -> &'a User:
    c  # error: local-reference-escapes

fn pick(x: &User, first: bool) -> usize:
    let mut r = x
    if first:
        local := User { name: "local" }
        r = choose(x, &local, first)
    r.name.len()  # error: local-reference-outlives-local

trait Named['a]:
    fn name(&self) -> usize
    fn borrowed_name(&'a self) -> &'a string

fn view(u: User) -> i32:
    let r: &'a User = &u  # error: references-not-yet-checked
    0
```

1. r[ref.lifetime.form] A local reference type may write a lifetime variable after `&`: `&'a T` and `&'a mut T`.
2. r[ref.lifetime.receiver] A reference receiver may write one too: `&'a self` and `&'a mut self`.
3. r[ref.lifetime.counted] A counted reference takes no lifetime variable, so `*'a T` is an error. Error: `syntax-error`.
4. r[ref.lifetime.declared] A lifetime variable names a lifetime parameter in scope, as [Lifetime Parameters](04-type-system.md#lifetime-parameters) states.
5. r[ref.lifetime.position] Each reference position of a signature has a lifetime: the variable it writes, or else a lifetime of its own.
6. r[ref.lifetime.shared] Positions that write one variable share its lifetime, so a result position with that variable may depend on any of them.
7. r[ref.lifetime.elide] A result position that writes no lifetime takes a `&self` or `&mut self` receiver's lifetime, or else the parameters' one lifetime.
8. r[ref.lifetime.elide.none] Such a position with no reference receiver and no parameter reference position is an error. Error: `reference-result-without-source`.
9. r[ref.lifetime.elide.ambiguous] Such a position with no reference receiver, under two or more lifetimes of the parameters' positions, is an error. Error: `ambiguous-reference-source`.
10. r[ref.lifetime.return] Each reference position of a returned value may depend only on the parameter positions of its lifetime, and on references reached through them. Error: `local-reference-escapes`.
11. r[ref.lifetime.return.message] The diagnostic's message names the root, the lifetime, and the parameters with that lifetime. It has no fix-it.
12. r[ref.lifetime.call] A call's result position depends on what every argument in a parameter position of its lifetime depends on.
13. r[ref.lifetime.declarations] A top-level function, an implementation, and a method of an implementation or a trait may declare lifetime parameters.
14. r[ref.lifetime.not-yet.local-or-newtype-owner] Any other lifetime parameter is not checked yet, so it is an error: a newtype's, a local function's, a local struct's, a local enum's, or a local alias's. A top-level struct's follows [Lifetime Structs](#lifetime-structs), a top-level enum's [Lifetime Enums](#lifetime-enums), a trait's [Lifetime Traits](#lifetime-traits), and a top-level alias's [Lifetime Aliases](#lifetime-aliases). Error: `references-not-yet-checked`.
15. r[ref.lifetime.alias-in-order] A type alias passes on the lifetimes written in its arguments when its type uses each parameter once, in order, outside function types, and writes no `&` of its own. So `Option[&'a T]` is `&'a T?`.
16. r[ref.lifetime.not-yet.alias] A lifetime written in the arguments of any other type alias is an error too. Error: `references-not-yet-checked`.
17. r[ref.lifetime.not-yet.body] A lifetime variable written inside a function body is an error too. Error: `references-not-yet-checked`.

```hd
struct User:
    name: string

fn swap['a, 'b](x: &'a User, y: &'b User) -> (&'b User, &'a User):
    (y, x)  # (x, y) is an error: each position keeps its own origins

fn either['a](a: &'a User, b: &'a User, first: bool) -> &User:  # the result takes 'a
    if first: a else: b
```

> **Why.** These are Rust's lifetime rules for functions: elision fills
> only the positions that write no lifetime, and a reference's referent
> outlives it. A lifetime is only checked; it adds nothing at run time.

See also: [Lifetimes In Function Types](07-functions.md#lifetimes-in-function-types),
[Method Generic Parameters](09-traits.md#method-generic-parameters),
[Lifetime Traits](#lifetime-traits), [Lifetime Aliases](#lifetime-aliases),
[Lifetime Implementation Targets](#lifetime-implementation-targets),
[Lifetime Structs](#lifetime-structs).

### Result Paths

A result reference may name the field of a reference parameter it points into, written after the `&` where a lifetime goes: `&'self.name string`. The result still has that parameter's lifetime. The body may return only a reference into that field.

```text
struct Name:
    first: string
    last: string

struct User:
    name: Name
    age: i32
    tags: List[string]

impl User:
    fn first(&self) -> &'self.name string:
        &self.name.first           # valid: inside self.name
    fn tags_mut(&mut self) -> &'self.tags mut List[string]:
        &mut self.tags             # valid
    fn wrong(&self) -> &'self.name List[string]:
        &self.tags                 # error: local-reference-escapes
    fn whole(&self) -> &'self.name User:
        self                       # error: local-reference-escapes

fn lead(a: &User, b: &User) -> &'b.name Name:
    &b.name                        # valid: the result depends only on b

fn copy(u: User) -> &'u.name Name:  # error: unknown-lifetime
    &u.name

trait Named:
    fn first(&self) -> &'self.name string  # error: trait-method-signature

struct Team:
    lead: User
    size: i32

struct Crew:
    lead: *mut User
    size: i32

fn lead_name(c: &Crew) -> &'c.lead.name Name:
    &c.lead.name                   # valid: the path crosses the `*mut`

fn birthday(u: &mut User, t: &mut Team) -> void $ Console:
    n := u.first()     # n borrows only u.name
    u.age += 1         # valid: another field
    u.tags.push("x")   # valid
    u.name.last = "b"  # error: conflicting-access
    println(*n)
    tags := u.tags_mut()  # tags borrows only u.tags, as `&mut`
    println(u.age)        # valid
    tags.push("y")
    m := t.lead.first()   # m borrows only t.lead.name
    t.size += 1           # valid
    t.lead = *u           # error: conflicting-access
    println(*m)
```

1. r[ref.result.path.form] A **result path** is a tick, a parameter's name, and one or more `.field` steps, written in the lifetime position of a local reference type: `&'p.f T` and `&'p.f mut T`. Without a step, `'p` is a lifetime variable ([`ref.lifetime.declared`](#r-ref.lifetime.declared)).
2. r[ref.result.path.position] A result path is written only in a function's result type. Anywhere else it is an error. Error: `unknown-lifetime`.
3. r[ref.result.path.param] The name is a `&` or `&mut` parameter's, `self` included. Any other name is an error. Error: `unknown-lifetime`.
4. r[ref.result.path.fields] Each step names a field of the struct the previous step reaches, starting from the parameter's referent. Any other step is an error. Error: `unknown-field`.
5. r[ref.result.path.counted] A step may cross a `*T` or `*mut T` field: the next step names a field of its referent, as `'c.lead.name` with `lead: *mut User`. The path's place goes through the counted reference, as a written `&c.lead.name` does. The callee and caller rules treat the crossing as one more step of that place.
6. r[ref.result.path.not-yet] A step through a local reference (`&T` or `&mut T`), and a result path on a reference inside another reference of the result, are not checked yet, so they are errors. Error: `references-not-yet-checked`.
7. r[ref.result.path.visible] Each field must be visible where the signature is written. Error: `private-member`.
8. r[ref.result.path.public] A `pub` signature names only public fields, so private layout never enters an API. Error: `private-type-leak`.
9. r[ref.result.path.lifetime] The result position has the parameter's lifetime, as if the signature wrote one variable on both.
10. r[ref.result.path.callee] A returned value in that position must point into the path's place. It is a `&` or `&mut` of a place that starts with the parameter and the path's fields, returned directly or through a binding. A value that may point elsewhere in the parameter's referent is an error. A call's result counts as pointing anywhere in its source. Error: `local-reference-escapes`.
11. r[ref.result.path.trait] A trait's method names the whole `self`, so its signature writes no result path. An implementation's method of a trait writes none either, since a bound or `&dyn` caller sees only the trait. Error: `trait-method-signature`.
12. r[ref.result.path.caller] A call's result in a result path's position holds only that part of the place its argument was taken from: the place followed by the path's fields ([`ref.excl.call`](#r-ref.excl.call)). For `u.first()` the result holds `u.name`, and for `t.lead.first()` it holds `t.lead.name`. A `&mut` result holds its part as `&mut`.
13. r[ref.result.path.caller.whole] The result holds the whole place, as without a path, when the parameter's lifetime is also on a result reference that names no path of it, or on another parameter. When several result references name paths of one parameter, the result holds the fields those paths share.
14. r[ref.result.path.tuple-element] A step may name an element of the tuple the previous step reaches, as in `-> &'t._1 string` with `t: &(i32, string)`.

> **Why.** A signature that says which field a result points into lets a
> caller lock only that field, as a written `&u.name` does. Private fields stay out of public signatures,
> so a layout change never breaks a caller. Only the body is checked against
> the path; a caller never depends on a callee's body.

### Lifetime Traits

A **lifetime trait** declares lifetimes which its methods may write:

```hd
struct Parser[T, 'src]:
    src: &'src T

trait Source[T, 'src]:
    fn source(&self) -> &'src T

impl[T, 'src] Source[T, 'src] for Parser[T, 'src]:
    fn source(&self) -> &'src T:
        self.src
```

1. r[ref.lifetime.trait] Each lifetime argument of a trait instantiates that lifetime in its methods for implementations, bounds, and trait values.
2. r[ref.lifetime.trait.impl-target] A trait implementation's target may write lifetimes which the implementation passes as the trait's lifetime arguments.
3. r[ref.lifetime.trait.position] A trait value holds one reference position for each lifetime argument, after the positions of its type arguments.
4. r[ref.lifetime.trait.bound-argument] A bound's trait arguments may write `&'a T` or `&'a mut T` with a lifetime in scope, as in `S < Store[&'a Point]`. Such an argument is no escape to a trait argument. A call through the bound passes that argument at `'a`, so with `s: &mut S`, `s.put(v)` stores `v` into a place the caller owns at `'a` ([`ref.lifetime.store.call`](#r-ref.lifetime.store.call)), and `v` must depend only on what a result of `'a` may depend on. A result of type `T` is at `'a` too ([`ref.lifetime.return`](#r-ref.lifetime.return)). A call that instantiates such a parameter relates each slot of its type argument to `'a`, and where the argument is a type parameter whose bound writes a lifetime for the same slot, that lifetime to `'a`. An associated type's bound writes its trait's lifetimes, which the bound of the projection's base gives. With `trait Shelf['b]: type Item < Store[&'b Point]` and `S < Shelf['a]`, a call through `S::Item`'s bound passes at `'a`. Each slot of an argument of type `S::Item` is at `'a` as well. A projection is also at each lifetime its trait's arguments name, as `S::Item` of `trait Plain['b]: type Item` with `S < Plain['a]` is at `'a`, and a value of a projection type returns as that type. A result of a projection type writes no reference, so no lifetime elides to it ([`ref.dep.elide`](#r-ref.dep.elide)). With `fn make(&self, v: &'b Point) -> Self::Item` in `Shelf['b]`, the result is at `'b`, which `v` is passed at, and not at the receiver's lifetime. A bound's lifetime arguments are passed the same way: with `S < Store['a]` and `fn put(&mut self, v: &'b Point)` in `Store['b]`, `s.put(v)` stores `v` at `'a`. A trait's own bodies see `Self` bounded by the trait over its own parameters, so in `Shelf['b]`'s default methods `Self::Item` is at `'b` and `self.put(v)` stores at `'b`. Error: `local-reference-escapes`.
5. r[ref.lifetime.trait.bound-unnamed] A `&T` or `&mut T` that writes no lifetime in a bound's trait arguments escapes to a trait argument, as [`ref.local.escape.trait-argument`](#r-ref.local.escape.trait-argument) states. A type parameter's bound, a supertrait, and an associated type's bound are each a bound. Error: `local-reference-escapes`.
6. r[ref.lifetime.trait.impl-argument] A trait implementation's trait arguments may write `&'a T` or `&'a mut T` with a lifetime the implementation declares, as in `impl['a] Store[&'a Point] for Tally`. Such an argument is no escape to a trait argument. The trait's methods take that argument at `'a`, as they take the target's lifetimes: with `impl['b] Id[&'b Point] for Sh`, `fn get(&self, v: V) -> V` is implemented by a `get` that returns its `v: &'b Point`. A call through the implementation passes each slot of one such lifetime as one, so with `impl['b] Put[&'b Point] for Cell['b]`, `c.put(v)` stores `v` into `c`'s `'b` place ([`ref.lifetime.store.call`](#r-ref.lifetime.store.call)). Error: `local-reference-escapes`.
7. r[ref.lifetime.trait.impl-unnamed] A `&T` or `&mut T` that writes no lifetime there has a fresh lifetime parameter of the implementation, so `impl Store[&Point] for Tally` means `impl['a] Store[&'a Point] for Tally`. It is no escape to a trait argument either.

> **Why.** This is Rust's rule. A bound states what a caller passes, so an
> unnamed reference there would say nothing about how long it lives. An
> implementation's header holds for every lifetime, so an unnamed reference
> there is general.

See also: [Generic Traits](09-traits.md#generic-traits),
[Lifetime Parameters](04-type-system.md#lifetime-parameters).

### Lifetime Aliases

A top-level type alias may name and pass its declared lifetimes:

```hd
type Borrowed[T, 'src] = &'src T
type OptionalBorrowed[T, 'src] = Option[Borrowed[T, 'src]]
```

1. r[ref.lifetime.alias] Each lifetime argument of a type alias instantiates that lifetime wherever the alias's body writes or passes it.

See also: [Lifetime Parameters](04-type-system.md#lifetime-parameters),
[Transparent Aliases](04-type-system.md#transparent-aliases-and-newtypes).

### Lifetime Implementation Targets

An implementation target may write a declared lifetime inside any reference position:

```hd
struct Wrap[T]:
    value: T

trait Identity:
    fn identity(self) -> Self

impl['a] Identity for Wrap[&'a string]:
    fn identity(self) -> Self:
        self
```

1. r[ref.lifetime.impl-target] An implementation target may write any lifetime declared by the implementation, including in a type argument, and every method's `self` and `Self` have those lifetimes.
2. r[ref.lifetime.impl-target.assoc] A trait implementation's trait arguments and associated types may name a lifetime its target writes. A method whose trait result is built from such a trait argument or associated type, as `Item?` in `impl['a] Iterator[&'a T] for RefIter[T, 'a]` or `Self::Out`, may return a reference of that lifetime. A call's result in that position depends on `self`'s positions of it.

See also: [Lifetime Parameters](04-type-system.md#lifetime-parameters),
[Lifetime Traits](#lifetime-traits), [Lifetime Structs](#lifetime-structs).

### Lifetime Structs

A **lifetime struct** declares lifetime parameters, and its fields may hold local references of those lifetimes:

```text
struct Parser[T, 'src]:
    src: &'src T
    pos: usize

struct Holder:
    parser: Parser[string]  # error: local-reference-escapes

fn source['a](p: &Parser[string, 'a]) -> &'a string:
    p.src

struct Two['a, 'b]:
    x: &'a string
    y: &'b string

fn left['a, 'b](t: &Two['a, 'b]) -> &'a string:
    t.y  # error: local-reference-escapes

fn src(p: &Parser[string]) -> &string:  # error: ambiguous-reference-source
    p.src

impl[T] Parser[T]:  # error: references-not-yet-checked
    fn position(&self) -> usize: self.pos

fn scan(fallback: &string) -> usize:
    let mut p = Parser { src: fallback, pos: 0 }
    if fallback.len() > 0:
        text := "hello"
        p = Parser { src: &text, pos: 0 }
    p.pos  # error: local-reference-outlives-local
```

1. r[ref.lifetime.struct] A top-level struct may declare lifetime parameters, as in `struct Parser[T, 'src]`.
2. r[ref.lifetime.struct.field] A field of a lifetime struct may write `&'a T` or `&'a mut T` with one of the struct's lifetimes. Such a field is no escape to a field.
3. r[ref.lifetime.struct.argument] A type names a lifetime struct with one lifetime argument per lifetime parameter, as in `Parser[string, 'a]`. The lifetime arguments take the lifetime parameters in order.
4. r[ref.lifetime.struct.argument.count] A struct type with lifetime arguments, but not one per lifetime parameter of the struct, is an error. Error: `argument-count`.
5. r[ref.lifetime.struct.holds] A lifetime struct holds a local reference ([`ref.local.holding`](#r-ref.local.holding)), so every rule of [Local References](#local-references) applies to its values.
6. r[ref.lifetime.struct.position] Each lifetime parameter is one reference position of the struct's type, after the positions of its type arguments. A written lifetime argument names that position's lifetime ([`ref.lifetime.position`](#r-ref.lifetime.position)).
7. r[ref.lifetime.struct.construct] A struct literal adds the origins of a field typed `&'a T` to the value's `'a` position only.
8. r[ref.lifetime.struct.read] Reading a field typed `&'a T` gives a reference whose origins are the origins of the value's `'a` position.
9. r[ref.lifetime.struct.read.one-lifetime] A field of another type whose reference positions all name one lifetime, as `Option[&'a T]` or `Parser[T, 'a]`, is constructed and read the same way.
10. r[ref.lifetime.struct.read.mixed] A field whose reference positions name two or more lifetimes, as `inner: Two['b, 'a]`, is constructed and read position by position: each position has the origins of the lifetime it names. A field nested inside such a field follows the same rule.
11. r[ref.lifetime.struct.field-elided] A field whose type names a lifetime struct without lifetime arguments holds a local reference, so it is an error. Error: `local-reference-escapes`.
12. r[ref.lifetime.struct.elide] A signature may name a lifetime struct without lifetime arguments. Each elided argument is a reference position that writes no lifetime ([`ref.lifetime.position`](#r-ref.lifetime.position)).
13. r[ref.lifetime.struct.elide.parameter] In a parameter's type, each elided argument has a lifetime of its own, so `p: &Parser[string]` has two reference positions.
14. r[ref.lifetime.struct.elide.result] In the result type, each elided argument takes its lifetime as [`ref.lifetime.elide`](#r-ref.lifetime.elide) states, as in `fn make(s: &string) -> Parser[string]`.
15. r[ref.lifetime.struct.elide.ambiguous] So a result that writes no lifetime, with no reference receiver and two lifetimes among the parameters' positions, is an error, as in `fn src(p: &Parser[string]) -> &string`. Error: `ambiguous-reference-source`.
16. r[ref.lifetime.struct.impl] An inherent implementation's target may write the implementation's lifetimes, as in `impl[T, 'a] Parser[T, 'a]`. Its methods' `self` and `Self` have those lifetimes.
17. r[ref.lifetime.struct.impl.elided] An implementation's target that names a lifetime struct without lifetime arguments is not checked yet, so it is an error. Error: `references-not-yet-checked`.
18. r[ref.lifetime.struct.body] A function body names a lifetime struct without lifetime arguments, as in `let p: Parser[string] = q`.

```hd
struct Parser[T, 'src]:
    src: &'src T
    pos: usize

fn parser['a](src: &'a string) -> Parser[string, 'a]:
    Parser { src: src, pos: 0 }

fn make(src: &string) -> Parser[string]:  # the result takes src's lifetime
    Parser { src: src, pos: 0 }

fn position(p: &Parser[string]) -> usize:  # two lifetimes of its own
    p.pos

impl[T, 'a] Parser[T, 'a]:
    fn source(&self) -> &'a T:
        self.src

fn length(text: string) -> usize:
    p := parser(&text)
    q := make(&text)
    p.src.len() - position(&q) + q.source().len()  # valid: text is in scope

struct Two['a, 'b]:
    x: &'a string
    y: &'b string

fn left['a, 'b](t: &Two['a, 'b]) -> &'a string:
    t.x  # valid: the `'a` position only

fn first(outer: string) -> usize:
    let mut kept = &outer
    if outer.len() > 0:
        inner := "hello"
        kept = left(&Two { x: &outer, y: &inner })
    kept.len()  # valid: kept does not point into inner
```

> **Why.** These are Rust's struct lifetimes, as in `struct Parser<'a>`.
> A lifetime struct has the representation of the same struct without
> lifetimes; only the checker tells its lifetimes apart.
> Elision is Rust's too: `&Parser<'_, T>` has two input lifetimes, so a
> result needs one named.

> **Note.** Lifetime arguments elided in an implementation's target come
> in a later stage.

See also: [Lifetime Parameters](04-type-system.md#lifetime-parameters),
[Fields](08-data-and-enums.md#fields), [Lifetime Enums](#lifetime-enums).

### Lifetime Enums

A **lifetime enum** declares lifetime parameters, and its variants' payloads may hold local references of those lifetimes:

```text
enum Token[T, 'src]:
    Word(&'src T)
    End

enum Pair['a, 'b]:
    Left(&'a string)
    Right(&'b string)

fn right['a, 'b](p: Pair['a, 'b], fallback: &'a string) -> &'a string:
    match p:
        Pair.Right(y) => y  # error: local-reference-escapes
        _ => fallback

fn count(fallback: &string) -> usize:
    let mut t = Token.Word(fallback)
    if fallback.len() > 0:
        text := "hello"
        t = Token.Word(&text)
    match t:  # error: local-reference-outlives-local
        Token.Word(w) => w.len()
        Token.End => 0
```

1. r[ref.lifetime.enum] A top-level enum may declare lifetime parameters, as in `enum Token[T, 'src]`.
2. r[ref.lifetime.enum.payload] A variant's payload field may write `&'a T` or `&'a mut T` with one of the enum's lifetimes. Such a field is no escape to a field.
3. r[ref.lifetime.enum.type] A type names a lifetime enum as it names a lifetime struct: [`ref.lifetime.struct.argument`](#r-ref.lifetime.struct.argument), [`.argument.count`](#r-ref.lifetime.struct.argument.count), [`.position`](#r-ref.lifetime.struct.position) and [`.field-elided`](#r-ref.lifetime.struct.field-elided) apply.
4. r[ref.lifetime.enum.holds] A lifetime enum holds a local reference ([`ref.local.holding`](#r-ref.local.holding)), so every rule of [Local References](#local-references) applies to its values.
5. r[ref.lifetime.enum.construct] A variant construction adds the origins of a payload field typed `&'a T` to the value's `'a` position only.
6. r[ref.lifetime.enum.match] A match binding of a payload field typed `&'a T` has the origins of the value's `'a` position, since it is a copy of the payload ([`ref.match.copy`](#r-ref.match.copy)).
7. r[ref.lifetime.enum.other-payload] A payload field of another type is constructed and bound as [`ref.lifetime.struct.read.one-lifetime`](#r-ref.lifetime.struct.read.one-lifetime) and [`.read.mixed`](#r-ref.lifetime.struct.read.mixed) state for a field.
8. r[ref.lifetime.enum.fn-value] A variant constructor as a function value has a result whose lifetimes take the payload's lifetime, as an elided result does ([`ref.lifetime.struct.elide.result`](#r-ref.lifetime.struct.elide.result)). So `Token.Word` is a `fn(&T) -> Token[T]`.
9. r[ref.lifetime.enum.signature] A signature, an implementation's target and a function body name a lifetime enum without lifetime arguments as they name a lifetime struct: [`ref.lifetime.struct.elide`](#r-ref.lifetime.struct.elide) to [`.body`](#r-ref.lifetime.struct.body) apply.

```hd
enum Token[T, 'src]:
    Word(&'src T)
    End

fn word['a](t: Token[string, 'a], fallback: &'a string) -> &'a string:
    match t:
        Token.Word(w) => w
        Token.End => fallback

fn make(src: &string) -> Token[string]:  # the result takes src's lifetime
    Token.Word(src)

impl[T, 'a] Token[T, 'a]:
    fn or(self, fallback: &'a T) -> &'a T:
        match self:
            Token.Word(w) => w
            Token.End => fallback

enum Pair['a, 'b]:
    Left(&'a string)
    Right(&'b string)

fn left['a, 'b](p: Pair['a, 'b], fallback: &'a string) -> &'a string:
    match p:
        Pair.Left(x) => x  # valid: the `'a` position only
        Pair.Right(_) => fallback

fn first(outer: string) -> usize:
    let mut kept = &outer
    if outer.len() > 0:
        inner := "hello"
        kept = left(Pair.Right(&inner), &outer)
    kept.len() + word(make(&outer), &outer).len()  # valid: kept does not point into inner
```

> **Why.** These are Rust's enum lifetimes, as in `enum Token<'a>`. A
> payload is a copy, so a match binding holds the payload's references
> with their origins, as reading a struct's field does.

See also: [Variant Payloads](08-data-and-enums.md#variant-payloads).

### Lifetime Stores

A function may store a local reference into a place its caller owns when the place's type names the reference's lifetime:

```text
struct User:
    name: string

fn bad['a, 'b](seen: &mut List[&'a User], u: &'b User) -> void:
    seen.push(u)  # error: local-reference-escapes

struct Two['a, 'b]:
    x: &'a string
    y: &'b string

fn bad_x['a, 'b](t: &mut Two['a, 'b], s: &'b string) -> void:
    t.x = s  # error: local-reference-escapes

fn remember['a](seen: &mut List[&'a User], u: &'a User) -> void:
    seen.push(u)

fn count(b: bool) -> usize:
    let mut seen: List[&User] = []
    if b:
        local := User { name: "local" }
        remember(&mut seen, &local)
    seen.len()  # error: local-reference-outlives-local
```

1. r[ref.lifetime.store] A store through a parameter's reference into a place the caller owns checks each reference position of the stored value against the lifetime that position of the place names. So `*t = Two { x: a, y: b }` is valid with `t: &mut Two['a, 'b]`, `a: &'a string` and `b: &'b string`. A store into a field checks the field's positions only ([`ref.lifetime.store.field`](#r-ref.lifetime.store.field)). The value in each position must depend only on what a result of that position's lifetime may depend on ([`ref.lifetime.return`](#r-ref.lifetime.return)). Any other such store of a value that holds a local reference is an error ([`ref.dep.store`](#r-ref.dep.store)). Error: `local-reference-escapes`.
2. r[ref.lifetime.store.collection] A list's `push` and a map's index assignment store their value into the list's or the map's place, so `seen.push(u)` follows [`ref.lifetime.store`](#r-ref.lifetime.store) when `seen` is such a parameter.
3. r[ref.lifetime.store.call] A `&mut` parameter whose referent has a reference position of a lifetime variable, as `seen: &mut List[&'a T]`, is a place the call may store into. So the argument's place takes what the call's arguments in that lifetime's positions depend on, as for a type parameter ([`ref.generic.store`](#r-ref.generic.store)). A referent with no reference position of its own that still holds references, as `d: &mut S::Item` with `S < Shelf['a]`, is one position at its projection's lifetimes, `'a` here. So `set(d, v)` with `fn set[T](dst: &mut T, x: T)`, `*d = v`, and a store into a field of `*d` each take a `v` that depends only on what a result of `'a` may depend on. Through a bound, a trait method's `&mut Self::Item` parameter stands for the caller's place: with `S < Mk[&'a Point]`, `s.fill(d, v)` stores `v` into `*d` at `'a`.
4. r[ref.lifetime.store.argument] A type argument of a generic type may hold a local reference wherever the instance may hold one, by [`ref.local.holding`](#r-ref.local.holding): in a function body, a parameter's type, and a function's result. A list and a map follow this rule as any generic struct does, so `xs: List[&User]` and `seen: &mut List[&User]` are valid parameter types.
5. r[ref.lifetime.store.element] A value read from a list or a map, as `seen[0]` or the result of `get`, has the origins of the collection's elements.
6. r[ref.lifetime.store.field] A store into a field through such a reference, as `t.x = s` with `t: &mut Two['a, 'b]`, checks each reference position of the field against the lifetime that position names. This holds at any depth, as `t.inner.x = s`, through a `&mut` of a part, as `r := &mut t.inner` then `r.x = s`, and for a field of several lifetimes, as a field read does ([`ref.lifetime.struct.read.mixed`](#r-ref.lifetime.struct.read.mixed)). The value in each position must depend only on what a result of that position's lifetime may depend on.
7. r[ref.lifetime.store.field.call] At a call, each position of a `&mut` argument's place takes only what the arguments in that position's lifetime depend on. So after `set_x(&mut t, &s)`, `t`'s `'a` position holds `s`'s origins and its `'b` position does not.

```hd
struct User:
    name: string

fn remember[T, 'a](seen: &mut List[&'a T], u: &'a T) -> void:
    seen.push(u)  # u has 'a, as the list's elements do

fn index['a](by: &mut Map[string, &'a User], u: &'a User) -> void:
    by[u.name] = u

fn set['a](out: &mut (&'a User), u: &'a User) -> void:
    *out = u

struct Two['a, 'b]:
    x: &'a string
    y: &'b string

fn set_x['a, 'b](t: &mut Two['a, 'b], s: &'a string) -> void:
    t.x = s  # s has 'a, as x does

fn other(outer: string) -> usize:
    let mut kept = &outer
    if outer.len() > 0:
        inner := "inner"
        let mut t = Two { x: &outer, y: &outer }
        set_x(&mut t, &inner)
        kept = t.y
    kept.len()  # valid: t.y never held inner

fn total(a: User, b: User) -> usize:
    let mut seen: List[&User] = []
    remember(&mut seen, &a)
    remember(&mut seen, &b)
    let mut by: Map[string, &User] = {}
    index(&mut by, &a)
    seen[0].name.len() + seen[1].name.len() + by.len()  # valid: a and b are in scope
```

> **Why.** This is Rust's `fn remember<'a>(seen: &mut Vec<&'a User>, u:
> &'a User)`: the place's type says which references it may hold, so the
> caller's list holds what the call stores, and its uses stay in their
> sources' scopes.

See also: [Generic Calls](#generic-calls), [Local Scope](#local-scope).

## Generic Locality

The bound `'static` marks a type parameter whose values may be kept past the call, because the type it stands for holds no local reference:

```hd
struct User:
    name: string

fn keep[T < 'static](value: T) -> *mut T:
    box(value)         # valid: T holds no local reference

fn later[T < 'static](value: T) -> fn() -> T:
    fn() -> T: value   # valid: the capture holds no local reference

fn demo(u: User) -> string:
    kept := keep(u)    # User meets 'static
    later(kept)().name
```

1. r[ref.generic.static] A type parameter bounded by `'static`, as in `T < 'static` or `W < Display & 'static`, stands only for a type that meets `'static`.
2. r[ref.generic.static.no-local] A type meets `'static` when it holds no local reference, by [`ref.local.holding`](#r-ref.local.holding) and [`ref.local.holding.counted`](#r-ref.local.holding.counted), and each type parameter it names is bounded by `'static`. A function type always meets it, since a closure captures no local reference.
3. r[ref.generic.static.error] At a call, a written or inferred type argument that does not meet `'static`, for a parameter bounded by it, is an error. Error: `local-reference-escapes`.
4. r[ref.generic.static.impl] An implementation whose type parameter is bounded by `'static` applies only where that parameter's argument meets `'static`, as for any other bound.
5. r[ref.generic.static.capture] A closure's capture whose type names a type parameter must meet `'static`. Error: `local-reference-escapes`.
6. r[ref.generic.static.no-fix] Neither diagnostic has a fix-it.
7. r[ref.generic.static.bound-only] `'static` is written only as a type parameter's bound. A reference type that writes it, as `&'static T`, is an error. Error: `references-not-yet-checked`.
8. r[ref.generic.static.fn-conjunction] A generic callback that is kept past its call is bounded by both `'static` and `Fn(...)`. The bounds join with `&` in either order: `F < 'static & Fn(A) -> R` and `F < Fn(A) -> R & 'static` are equivalent.

```text
struct User:
    name: string

fn later[T](value: T) -> fn() -> T:
    fn() -> T: value  # error: local-reference-escapes

fn hold[T < 'static](value: T) -> void:
    pass

fn wrap(u: &User) -> void:
    hold(box(u))  # error: local-reference-escapes

fn view(u: &'static User) -> void:  # error: references-not-yet-checked
    pass
```

> **Why.** This is Rust's `T: 'static`. A function value carries no
> lifetime, so a value kept in one must hold no local reference. A counted
> reference carries its referent's, as Rust's `Rc<&'a T>` does.

### Lifetime Bounds

A type parameter bounded by a declared lifetime stands for a type whose values may hold local references of that lifetime, so a struct may store a closure that captures one:

```hd
use std.function.FnMut

struct Counter:
    n: i32

struct Button[F < FnMut() -> void & 'a, 'a]:
    on_click: F

fn press[F < FnMut() -> void & 'a, 'a](mut b: Button[F, 'a]) -> void:
    (b.on_click)()

fn bump(c: &mut Counter) -> void:
    b := Button { on_click: fn(): c.n += 1 }  # captures the local reference c
    press(b)

fn twice[F < FnMut() -> void](mut f: F) -> void:
    f()
    f()

fn bump_twice(c: &mut Counter) -> void:
    twice(fn(): c.n += 1)  # captures c for the call

struct Plain[F < FnMut() -> void]:
    on_click: F

fn click(c: &mut Counter) -> void:
    let mut p = Plain { on_click: fn(): c.n += 1 }  # p holds c
    (p.on_click)()

struct Shift:
    offset: i32

impl Shift:
    fn apply(&self, xs: List[i32]) -> List[i32]:
        xs.iter().map(fn(x: i32) -> i32: x + self.offset).collect()  # the adapter holds self
```

1. r[ref.generic.outlives] A type parameter may be bounded by one lifetime that is in scope, as in `F < FnMut() -> void & 'a`. Its argument may hold local references of that lifetime.
2. r[ref.generic.outlives.one] A second lifetime bound on one type parameter is not checked yet, so it is an error. Error: `references-not-yet-checked`.
3. r[ref.generic.outlives.field] A struct literal may give a function value with captured origins to a field whose type is a type parameter that `'static` does not bound. A closure written there may capture local references. The struct value holds the captured origins in that parameter's positions, as [`ref.generic.outlives.instance`](#r-ref.generic.outlives.instance) states. Error: `local-reference-escapes`.
4. r[ref.generic.outlives.position] A field whose type parameter has a lifetime bound lies in that lifetime's position. A literal adds the closure's captured origins to that position, and reading the field gives a function value with that position's origins.
5. r[ref.generic.outlives.read] A function value with captured origins follows [`fn.capture.local-uses`](07-functions.md#r-fn.capture.local-uses) and [`fn.capture.local-store`](07-functions.md#r-fn.capture.local-store). Returning it where the result has no source for those origins is an error. Error: `local-reference-escapes`.
6. r[ref.generic.outlives.box] Boxing a function value with captured origins, or passing it for a type parameter bounded by `'static`, is an error. Error: `local-reference-escapes`.
7. r[ref.generic.outlives.read-only] A store through a captured `&mut` reference, as `c.n += 1` above, changes a capture. Such a closure needs an `FnMut` bound, as [`fn.trait.read-only`](07-functions.md#r-fn.trait.read-only) states. Error: `not-writable`.
8. r[ref.generic.outlives.argument] A closure written as a call argument for a parameter of a bare type parameter `F`, as `mut f: F`, may capture local references. The callee's signature must not name `F` inside a function type or an opaque type, and `'static` must not bound `F`. Error: `local-reference-escapes`.
9. r[ref.generic.outlives.argument-origins] The call's result holds the captured origins in `F`'s positions, as [`ref.generic.call`](#r-ref.generic.call) states. A lifetime bound on `F` puts them in that lifetime's positions too.
10. r[ref.generic.outlives.instance] A function type that a type argument puts inside another type, as `F` in `MapIter[I, T, U, F]` or `Plain[F]`, is a position of that type. It holds what its closure captured, so a result such as `xs.iter().map(...)` carries the captures and follows [`ref.generic.outlives.read`](#r-ref.generic.outlives.read).
11. r[ref.generic.outlives.written] A function type written inside another type of a parameter, as in `Option[fn() -> void]` or `MapIter[ListIter[i32], i32, i32]`, holds no captured origins. Passing a value whose function value there captures local references is an error. Error: `local-reference-escapes`.

```text
struct Plain[F < FnMut() -> void]:
    on_click: F

struct Label[F < Fn() -> void & 'a, 'a]:
    on_show: F

fn view['a, 'b, T < 'a & 'b](value: T) -> void:  # error: references-not-yet-checked
    pass

fn plain(flag: bool) -> void:
    let mut p = Plain { on_click: fn(): pass }
    if flag:
        let mut inner = Counter { n: +2 }
        r := &mut inner
        p = Plain { on_click: fn(): r.n += 1 }
    (p.on_click)()  # error: local-reference-outlives-local

fn show(c: &mut Counter) -> void:
    l := Label { on_show: fn(): c.n += 1 }  # error: not-writable

fn keep(c: &mut Counter) -> void:
    b := Button { on_click: fn(): c.n += 1 }
    kept := box(b.on_click)  # error: local-reference-escapes

fn make() -> fn() -> void:
    let mut x = Counter { n: +0 }
    r := &mut x
    b := Button { on_click: fn(): r.n += 1 }
    b.on_click  # error: local-reference-escapes

fn wrap[F < FnMut() -> void](f: F) -> Plain[F]:
    Plain { on_click: f }

fn same[F < FnMut() -> void](f: F) -> F:
    f

fn first[F < Fn() -> i32 & 'a, 'a](c: &'a Counter, f: F) -> &'a Counter:
    c

fn nested(c: &mut Counter) -> Plain[fn() -> void]:
    wrap(fn(): c.n += 1)  # error: local-reference-escapes

fn escape() -> fn() -> void:
    let mut x = Counter { n: +0 }
    r := &mut x
    same(fn(): r.n += 1)  # error: local-reference-escapes

fn pick(outer: &Counter) -> i32:
    let mut r: &Counter = outer
    if outer.n > 0:
        inner := Counter { n: +2 }
        p := &inner
        r = first(outer, fn() -> i32: p.n)
    r.n  # error: local-reference-outlives-local
```

> **Why.** This is Rust's `F: FnMut() + 'a`. A `fn(...)` type carries no
> lifetime, so the struct's lifetime holds what its closure captures. A
> closure argument for a bare `F` is Rust's inferred closure lifetime: the
> call's result carries its captures, also inside an adapter such as
> `Map<I, F>`. A struct literal infers `F` as such a call does. A `fn(...)` type written in a parameter's type is Rust's
> function pointer, which captures nothing.

See also: [Static Fn Bounds](07-functions.md#static-fn-bounds),
[Lifetime Structs](#lifetime-structs).

### Generic Calls

A type argument may hold a local reference, so a generic function or method works on references as on any other value:

```hd
struct User:
    name: string

struct Pair:
    left: User
    right: User

fn id[T](value: T) -> T:
    value

fn left_or_right(pair: &Pair, left: bool) -> &User:
    let o: (&User)? = if left: .Some(&pair.left) else: .None
    o.unwrap_or(&pair.right)  # borrows from pair

fn demo(u: &User) -> usize:
    n := id(&u.name)          # n borrows from u
    n.len()
```

1. r[ref.generic.local] A written or inferred type argument of a function, a method, or an implementation may hold a local reference.
2. r[ref.generic.local.local-function] A type argument of a local function never holds one yet. Error: `local-reference-escapes`.
3. r[ref.generic.call] A reference position that a type argument adds to a call's result depends on the argument positions that the same type parameter adds to the call's parameters, as a lifetime variable's positions do ([`ref.lifetime.call`](#r-ref.lifetime.call)). It depends on no other argument. A position the type parameter adds to no argument depends on nothing. A type argument with no reference position of its own that still holds references, as a projection `S::Item` does, passes them whole: a result of type `T`, as of `fn id[T](x: T) -> T`, holds what the argument for `T` held, and no lifetime elides to it ([`ref.dep.elide`](#r-ref.dep.elide)). Through a bound that fixes the type argument, as `S < Put[T::Item]`, the argument is at that type's lifetimes.
4. r[ref.generic.call.line-up] Each argument lines up with its parameter's type instantiated at the call. A trait value given for `&T` lines up with that reference, and a position of a projection such as `P::Out` is a position of `P`.
5. r[ref.generic.store] A parameter `&mut P`, whose `P` names a type parameter, is a place the call may store into. So the argument's place takes what that type parameter's other argument positions depend on, as `*p = v` does ([`ref.dep.store`](#r-ref.dep.store)).
6. r[ref.generic.body] A generic body treats a value of a type parameter as one that may hold a local reference: it may return it or pass it on, and keeps it past the call only under `'static`.

```text
struct User:
    name: string

struct Holder[T]:
    value: T

fn id[T](value: T) -> T:
    value

fn put[T](h: &mut Holder[T], value: T) -> void:
    h.value = value

fn bad_id(u: &User) -> &string:
    local := User { name: "x" }
    id(&local.name)  # error: local-reference-escapes

fn bad_put(u: &User) -> usize:
    let mut h = Holder { value: u }
    if u.name.len() > 0:
        local := User { name: "y" }
        put(&mut h, &local)
    h.value.name.len()  # error: local-reference-outlives-local

fn bad_store(h: &mut Holder[&User], u: &User) -> void:
    put(h, u)  # error: local-reference-escapes

fn bad_local(u: &User) -> void:
    fn same[T](value: T) -> T:
        value
    _ := same(u)  # error: local-reference-escapes
```

> **Why.** This is Rust's model: a type argument's lifetimes travel with
> it, so a call's result borrows what its arguments in that parameter's
> positions borrow.

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
2. r[ref.read.auto.levels] The read goes through every reference level, `&`, `&mut`, `*`, and `*mut` alike: with `x: &*mut User`, `x.name` reads `(*(*x)).name`, and `x.describe()` calls `describe` on the `User`.
3. r[ref.read.auto.method-level] A method call stops at the first level whose referent has the method itself, as Rust's auto-deref does: with `x: &*T` and an `impl Display for *T`, `x.to_string()` calls that impl's method.
4. r[ref.read.auto.write] A field write or a `&mut self` call through several levels needs only the last level to be writable, as `(*x).f = v` does by [`ref.read.shallow`](#r-ref.read.shallow): `x.age = 1` is valid with `x: &*mut User` and an error with `x: &mut *User`. Error: `not-writable`.
5. r[ref.read.auto.explicit] Only field access and method calls read through every level. A condition, a cast, a range bound, `?`, a spread, and a call of a function value keep an explicit `*`.
6. r[ref.deref.read] `*r` reads the whole value `r` refers to. The result is a copy.
7. r[ref.deref.kinds] `*r` reads through a counted and a local reference alike.
8. r[ref.deref.not-reference] `*e` on a value that is not a reference is an error. Error: `type-mismatch`.
9. r[ref.read.type] A read through a writable reference, `*mut T` or `&mut T`, has type `T`, and the value it reaches is a writable place.
10. r[ref.read.shallow] A read through a read-only reference, `*T` or `&T`, keeps its declared type; the reference makes only the value it refers to read-only.
11. r[ref.read.index.declared] An index on a reference to a list, map, or string reads an element of the referenced value.

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
4. r[ref.match.copy] A copied payload or field that a pattern binds below such a read is a copy of it, read at the time of the match.
5. r[ref.match.declared-type] A binding copied by a pattern through a reference keeps its declared type.
6. r[ref.match.borrow] Below a `&T` or `&mut T`, a pattern binds a payload or field whose type is not copied as a reference to it. The binding has type `&F` below a `&T`, and `&mut F` below a `&mut T` with no `&` above it. Error: `type-mismatch` where an `F` is expected.
7. r[ref.match.borrow.excl] Such a binding is a reference to the matched place, like `&*r` or `&mut *r`. [`ref.excl.mut`](#r-ref.excl.mut) and [`ref.excl.shared`](#r-ref.excl.shared) apply to it while it is live. Error: `conflicting-access`.

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

```text
struct Photo:
    width: i32

impl !Copy for Photo

enum Slot:
    Empty
    Full(Photo)

fn widen(r: &mut Slot) -> void:
    match r:
        .Full(p) => p.width += 1   # p: &mut Photo
        .Empty => pass
```

> **Why.** A copied binding is a value, so a later write to the matched
> place does not change it. A payload that is not copied cannot be read
> out of a place behind a reference, so the pattern lends it instead,
> and the exclusivity rules keep the enum's variant fixed while the loan
> is live.

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
4. r[ref.write.no-exclusivity] A counted writable reference, `*mut T`, is not exclusive: other references to the same place may exist and be used. A `&mut T` is exclusive while it is live, as [Writes While Borrowed](#writes-while-borrowed) states.
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
    p := Point { x: 1, y: 2 }
    q := Point { x: 1, y: 2 }
    r := &p
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

`==` between two counted references is an error, because `is` and `*r == *s`
say which comparison is meant. Between two local references it compares
values.

1. r[ref.identity.no-equality] `==` and `!=` with two counted reference operands, of types `*T` or `*mut T`, are an error. Error: `reference-equality`.
2. r[ref.identity.no-equality.fix] The diagnostic has two fix-its: one writes `r is s`, or `!(r is s)` for `!=`, and compares places; the other writes `*r == *s`, or `*r != *s`, and compares values.
3. r[ref.identity.generic-eq] A reference type implements `Eq` when its value type does, and `Hash` when its value type does. They compare and hash the values the references refer to, as [Reference Equality](../std/cmp.md#reference-equality) states. So generic code and a derived `Eq` compare through references.
4. r[ref.identity.local-equality] `==` and `!=` with two local reference operands, of types `&T` or `&mut T`, compare the values they refer to, by [`expr.op.ref-read.equal-values`](05-expressions.md#r-expr.op.ref-read.equal-values).

```text
fn same(a: *i32, b: *i32) -> bool:
    a == b  # error: reference-equality

fn equal(a: &i32, b: &i32) -> bool:
    a == b  # valid: compares the values
```

> **Why.** Go's `==` on pointers compares addresses, and Rust's compares
> contents. A counted reference is a shared object, where an author from
> either language would guess wrong half the time, so neither guess
> compiles. A local reference lends a value to one call, so it compares as
> that value, as in Rust.

See also: [Derived Implementations](09-traits.md#derived-implementations),
[Identity](05-expressions.md#identity).

## Element References

`&xs[i]` and `&mut xs[i]` refer to the slot `i` of the list `xs`, and `&m[k]` and `&mut m[k]` to the value of the key `k` in the map `m`. An element reference lives until its last use, including when a binding or dependent result holds it.

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
9. r[ref.elem.identity] Two element references refer to the same place when they refer to the same slot of the same list's elements and read the same struct fields from it.
10. r[ref.elem.trait-accessors] A reference returned by `Index.index` or `IndexMut.index_mut` is an element reference and follows the same lifetime and access rules as a built-in element reference.

```text
fn bump(score: &mut i32) -> void:
    *score += 1

fn invalid() -> void:
    xs := [+1, 2]
    bump(&mut xs[0])  # error: not-writable
```

> **Why.** A list grows by moving its elements. Go keeps the old array
> alive, so a write through an old pointer silently stops reaching the
> slice. An element reference instead holds its collection while it
> lives, so nothing can move or detach its slot. A handle that survives a
> sort is a reference stored in the list, `List[*mut Player]`, or an ID.

### Call-Scoped Access

An element reference is a local reference value. Its collection's length
holds still until the reference's last use.

```text
fn bump(score: &mut i32) -> void:
    *score += 1

fn record(scores: &mut List[i32], score: &mut i32) -> void:
    scores.push(*score)

fn demo() -> void:
    let mut scores = [+1, 2]
    bump(&mut scores[0])                 # valid: the access ends after this use
    record(&mut scores, &mut scores[0])  # error: conflicting-access
    best := &mut scores[1]
    scores.push(3)                       # error: conflicting-access
    n := scores.len()                    # error: conflicting-access
    scores[0] = 5                        # valid: a store to another element
    *best += 1
    scores.push(4)                       # valid: after best's last use
```

1. r[ref.elem.local-value] An element reference is a local reference value. It may be bound, assigned, passed, and used as the source or result of a dependent call, subject to every rule of [Local References](#local-references) and [Dependent Results](#dependent-results).
2. r[ref.elem.access.live] The reference's **access** to its collection begins when the reference is taken and ends after its last use.
3. r[ref.elem.access.live-length] While an access lasts, nothing may change the collection, or a place that holds it, other than by a store to one of its elements. Reads of the collection, and stores to its elements, the referenced one included, stay valid.
4. r[ref.elem.access.length] While a binding that holds an element reference is live ([`ref.excl.live`](#r-ref.excl.live)), a change of its collection or of a place that holds it is an error, and so is a `&mut` of another element of it. Error: `conflicting-access`.
   - The collection is a place that [`ref.excl.place`](#r-ref.excl.place) covers, so it may be reached through a `&mut` parameter.
   - Such a change is a call that takes `&mut` of the collection or of a place that holds it, as `push`, `insert`, `remove_at`, `pop`, `clear` and `reserve` do, or an assignment of a whole value to it, as in `xs = []`.
   - A map's `remove` is one, and so is a map store of a key other than the referenced one.
   - Two keys are one key when both are one constant or reads of one `:=` binding.
5. r[ref.elem.access.mut] While a binding that holds a `&mut` element reference is live, any use of its collection or of a place that holds it is an error: a copy, a `&` of it, a `len()` or another method call. So is any other reference into the collection, `&` or `&mut`, at any index. A read of another element or a store to one stays valid. Error: `conflicting-access`.
6. r[ref.elem.unaliased] A collection is **unaliased** when it is a local binding or a value parameter, or a struct field or element reached from one through no reference.
7. r[ref.elem.conflict] During an access to an unaliased collection, another argument or the receiver of the same call that may change the collection, or a later argument that moves it ([`ref.excl.move`](#r-ref.excl.move)), is an error. For a `&mut` element reference, so is another argument that refers into the collection, and a later argument that uses it ([`ref.elem.access.mut`](#r-ref.elem.access.mut)). Error: `conflicting-access`.
   - The collection may also be a place that [`ref.excl.place`](#r-ref.excl.place) covers, so one reached through a `&mut` parameter, as in `change(list, &mut list[2])`.
   - An argument that holds an element reference is an access, whether the call takes the reference or a binding holds it, as in `grow(&mut xs, first)` after `first := &xs[0]`.
8. r[ref.elem.access.alias] Where a `*T` or a variable a closure shares reaches the collection, the element reference holds that object as [Conflicts Through Aliases](#conflicts-through-aliases) says, so a change through another path panics. Panic: `borrow-conflict`.

An argument may change the collection's length when it does one of the
following to the collection, or to a place that holds it:

| Rule | The argument or receiver | Example |
| --- | --- | --- |
| r[ref.elem.conflict.reference] Reference | takes `&mut` of it | `record(&mut scores, &mut scores[0])` |
| r[ref.elem.conflict.method] Method call | calls a `&mut self` method on it | `place(&mut scores[0], scores.remove_at(1))` |

> **Why.** This is Swift's Law of Exclusivity, and for a `&mut` element
> reference Rust's borrow rules. A conflict is caught at compile time
> where every path to the collection is in view, and by a pointer loan
> where a pointer or a shared variable may reach it. Stores to other
> elements cannot move the referenced slot, so they stay legal.

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

4. r[ref.capture.local-scoped] A closure supplied directly for a function parameter may capture a local reference under [`ref.local.capture-parameter`](#r-ref.local.capture-parameter).
5. r[ref.capture.local-stored] Any other closure capture of a local reference is an error under [`ref.local.escape.capture-stored`](#r-ref.local.escape.capture-stored).

See also: [Copied And Shared Captures](07-functions.md#copied-and-shared-captures),
[`ref.read.shallow`](#r-ref.read.shallow).

## Moves

A `&mut T` value is never copied. A binding that gives its value away **moves** it, and the binding may not be used again until it is assigned. A call that expects `&mut` **reborrows** the binding instead: it lends what the binding refers to for the call.

```hd
struct Counter:
    count: i32

fn bump(c: &mut Counter) -> void:
    c.count += 1

fn keep[T < ?Copy](x: T) -> void: pass

fn demo(c: &mut Counter) -> void:
    bump(c)        # a reborrow: c stays usable
    bump(c)
    keep(&mut *c)  # an explicit reborrow
    d := c         # c moves into d
    bump(d)
```

1. r[ref.move.mut] A `&mut T` value is moved, not copied. A `*mut T` or `&T` value is copied.
2. r[ref.move.holds] A value whose type holds a `&mut` is moved, not copied. A type holds a `&mut` when it is `&mut T`, a struct with a field whose type holds one, an enum with such a payload, or a tuple, optional, or union with such a member. `SpanMut[T, 'a]` holds one. [Copy](#copy) names the other types whose values move.
3. r[ref.move.sites] A read of a whole binding that is not copied moves its value at each site in the table below. No `&mut` type is expected at these sites.
4. r[ref.move.field] A read of a field of a binding, where the field's type is not copied, moves the field out at each site in the table below. The binding is then partly moved. A site that expects a `&mut` type reborrows the field instead, as for a binding. A field of a field moves the same way.
5. r[ref.move.pattern] A `match` arm or `let` pattern that binds a value whose type is not copied moves that value out of the matched value. When the matched value is a binding, or a field of one, a bound payload or field moves out as a field does. The binding is then partly moved. A `_` pattern moves nothing, and neither does binding only copied parts. A pattern matched through a reference binds as [Matching Through A Reference](#matching-through-a-reference) states.
6. r[ref.move.out-of-place] A read of a value that is not copied, from a place behind a `&T`, `&mut T`, `*T` or `*mut T`, is an error at each site in the table below. Such a place is `*r`, or a field reached through `r`, such as `r.target`. A site that expects a `&mut` type reborrows instead. Error: `move-out-of-place`.
7. r[ref.move.out-of-place.capture] A read of a value that is not copied, from a binding the enclosing closure captures or a field of it, is an error at each site in the table below, since the closure may be called any number of times ([`fn.capture.call.any`](07-functions.md#r-fn.capture.call.any)). Borrowing the binding and calling a `&self` method on it read nothing out. Error: `move-out-of-place`.
8. r[ref.move.reborrow] An argument whose parameter is declared `&mut T` reborrows the binding for the call and does not move it. A `&mut self` receiver is such an argument.
9. r[ref.move.reborrow.explicit] `&mut *r` reborrows `r` explicitly. It makes a new `&mut` to the place `r` refers to, and `r` stays usable.
10. r[ref.move.reborrow.typed] A site that expects a written `&mut T` type reborrows the value instead of moving it. These sites are a `let` that writes the type, an assignment to a place of a `&mut` type, and a struct field or variant payload declared `&mut`. Only a `&mut` value reborrows: a value of another type that is not copied moves at a `let` that writes its type, at an assignment, and into a field.
11. r[ref.move.reborrow.shared] Converting a `&mut T` value to `&T`, as in `let t: &T = r`, reborrows it as a shared reference and does not move it.
12. r[ref.move.reborrow.excl] A reborrow that a `let`, an assignment, a field or a payload keeps is a reference to the place `r` refers to, like `&mut *r` or `&*r`. [`ref.excl.mut`](#r-ref.excl.mut) and [`ref.excl.shared`](#r-ref.excl.shared) apply to it while it is live. An argument's reborrow lasts as [`ref.excl.call`](#r-ref.excl.call) states, so `bump(c); bump(c)` is valid. Error: `conflicting-access`.
13. r[ref.move.use] A use of a binding whose value moved on some path to the use is an error. The path may run through an earlier iteration of a loop. Error: `use-of-moved-value`.
14. r[ref.move.use.partial] A use of a partly moved binding is an error when it reaches a moved field: a use of the whole binding, of the moved field, or of a place inside that field. Its other fields stay usable. A moved payload counts as a moved field, so matching the binding again is such a use. Error: `use-of-moved-value`.
15. r[ref.move.use.forms] A use is a read of the binding or of a place through it, a write through it, a reference to it, or a closure that captures it. A closure captures the whole binding.
16. r[ref.move.reassign] Assigning the binding a new value makes it usable again. Assigning a moved field a new value makes that field usable again, and the binding whole when no other field is moved.

| Site | Example |
| --- | --- |
| A `:=` binding, or a `let` without a written type | `d := c`, `let d = c` |
| An argument for a parameter not declared `&mut`, such as a `T < ?Copy` | `keep(c)` |
| A tuple element, a list element, or an optional's payload | `(c, 1)`, `.Some(c)` |
| A struct field or variant payload whose declared type is a type parameter | `Wrap { value: c }` |
| A returned value, or the value of a `break` | `return c` |

```text
struct Counter:
    count: i32

fn bump(c: &mut Counter) -> void:
    c.count += 1

fn keep[T < ?Copy](x: T) -> void: pass

fn after_let(c: &mut Counter) -> void:
    d := c
    bump(c)  # error: use-of-moved-value

fn after_generic(c: &mut Counter) -> void:
    keep(c)
    c.count = 1  # error: use-of-moved-value

fn after_branch(c: &mut Counter, early: bool) -> void:
    if early:
        keep(c)
    bump(c)  # error: use-of-moved-value

fn in_loop(c: &mut Counter) -> void:
    for i in 0..3:
        keep(c)  # error: use-of-moved-value

fn after_typed_let(c: &mut Counter) -> void:
    let d: &mut Counter = c
    c.count = 1  # error: conflicting-access
    d.count += 1
```

```text
struct Holder['a]:
    target: &'a mut i32
    count: i32

fn take(h: Holder) -> void:
    pass

fn moved(x: &mut i32) -> void:
    h := Holder { target: x, count: 1 }
    t := h.target  # moves the field out
    n := h.count   # another field stays usable
    take(h)        # error: use-of-moved-value

fn demo(x: &mut i32, y: &mut i32) -> void:
    let mut h = Holder { target: x, count: 1 }
    t := h.target
    h.target = y   # h is whole again
    take(h)
```

```text
struct Holder['a]:
    target: &'a mut i32

fn take(h: Holder) -> void:
    pass

fn twice(x: &mut i32) -> void:
    let o: Holder? = .Some(Holder { target: x })
    match o:
        .Some(h) => take(h)  # moves the payload out of o
        .None => pass
    match o:  # error: use-of-moved-value
        .Some(h) => take(h)
        .None => pass

fn refill(x: &mut i32, y: &mut i32) -> void:
    let o: Holder? = .Some(Holder { target: x })
    match o:
        .Some(_) => pass  # `_` moves nothing
        .None => pass
    let .Some(h) = o else: return
    take(h)
    o = .Some(Holder { target: y })  # o is whole again
    match o:
        .Some(g) => take(g)
        .None => pass
```

```text
struct Holder['a]:
    target: &'a mut i32
    count: i32

fn peek(h: &Holder) -> i32:
    h.count

fn keep[T < ?Copy](x: T) -> void: pass

fn twice['a, 'b](r: &'b Holder['a]) -> (Holder['a], Holder['a]):
    (*r, *r)  # error: move-out-of-place

fn whole(r: &mut Holder) -> i32:
    n := (*r).count   # an i32 is copied
    keep(*r)          # error: move-out-of-place
    peek(&*r) + peek(r)

fn field(r: &mut Holder) -> void:
    t := r.target     # error: move-out-of-place
    let u: &mut i32 = r.target  # a reborrow
```

```text
struct Photo:
    width: u32

impl !Copy for Photo

struct Pair:
    a: Photo

fn shared(r: *Pair) -> Photo:
    r.a  # error: move-out-of-place

fn mutable(r: *mut Pair) -> Photo:
    r.a  # error: move-out-of-place

fn copied(r: *Pair) -> Photo:
    Photo { width: r.a.width }  # a new value from copied fields
```

> **Why.** A reference lends a place but does not own the value there.
> Moving a value that holds a `&mut` out of it would leave two values
> that write one place. Rust reports the same read as "cannot move out of
> `*r`" (E0507). Borrow the place with `&*r` or `&mut *r`, or pass `r` on.
> A counted reference shares its object, so the other references to it
> still read the field. Rust's `Rc` refuses the same move (E0507).

> **Why.** This is Rust's rule (E0382) for `&mut`. Two copies of one
> `&mut` would both write the same place, which the exclusivity rules
> forbid. The implicit reborrow keeps the common case, `bump(c); bump(c)`,
> valid.

A value that holds a `&mut` moves the same way. Its `&mut self` methods
borrow it for the call, as with any `let mut` binding:

```text
use std.collections.SpanMut

fn spans(items: &mut List[i32]) -> void:
    let mut span = items.span_mut()
    span.swap(0, 1)      # a `&mut self` receiver: span stays usable
    let mut part = span  # span moves into part
    part.fill(0)
    span.fill(1)         # error: use-of-moved-value
```

> **Why.** Rust's rule: a type is `Copy` only when all its
> fields are, and `&mut T` is not. Copying a span would give two views
> that write one list.

See also: [Writes While Borrowed](#writes-while-borrowed).

### Copy

`Copy` is the prelude's marker for a type whose values are copied. Every
type is `Copy` unless it holds a `&mut`, is declared `impl !Copy`, or is a
type parameter declared `?Copy`. A value of any other type moves.

```hd
struct Photo:
    width: u32
    pixels: List[u8]

impl !Copy for Photo

fn keep[T](x: T) -> void: pass

fn hold[T < ?Copy](x: T) -> T: x

fn export(photo: Photo) -> Photo:
    keep(photo.width)  # a `u32` is copied
    hold(photo)        # photo moves
```

1. r[ref.copy.marker] `Copy` is a sealed marker trait that [`std.core`](10-modules.md#core-layer) declares and `std.ops` re-exports, in the prelude. It has no members, and the compiler supplies its implementations.
2. r[ref.copy.structure] A type is `Copy` unless it is `&mut T`, a type declared `impl !Copy`, a type parameter that is not `Copy`, or a type that holds one as [`ref.move.holds`](#r-ref.move.holds) defines.
3. r[ref.copy.moves] A value whose type is not `Copy` is moved, not copied, at each site of [`ref.move.sites`](#r-ref.move.sites).
4. r[ref.copy.opt-out] `impl !Copy for T` declares that the struct or enum `T` is not `Copy`. It has no body and belongs in the module that declares `T`.
5. r[ref.copy.opt-out.invalid] A negative implementation of a trait other than `Copy`, of a type that is not a struct or an enum, outside its target's module, or with a body is an error. Error: `invalid-negative-impl`.
6. r[ref.copy.no-impl] `impl Copy for T` is an error, as for every sealed trait. Error: `sealed-trait-implementation`.
7. r[ref.copy.generic.default] Every type parameter is `Copy` unless it is declared `?Copy`: a function's, a method's, a struct's, an enum's, an alias's, an implementation's, and a trait's.
8. r[ref.copy.generic.argument] A type argument for a `Copy` parameter must be `Copy`. This holds for the inferred arguments of a call, a struct literal, and a variant construction. Error: `unsatisfied-trait-bound`.
9. r[ref.copy.generic.relaxed] `T < ?Copy` lifts the default bound. `T` then accepts every type, and the body moves a `T` value as one that is not `Copy`.
10. r[ref.copy.generic.relaxed.only] A `?` before a trait other than `Copy` is an error. Error: `invalid-relaxed-bound`.
11. r[ref.copy.trait.default] A trait's `Self` is `Copy`, so a bound on the trait implies `Copy`. An implementation for a type that is not `Copy` is an error. Error: `missing-supertrait-implementation`.
12. r[ref.copy.trait.reference] A `&mut dyn` value meets no bound on a trait whose `Self` is `Copy`. Error: `unsatisfied-trait-bound`.
13. r[ref.copy.trait.relaxed] `trait Name < ?Copy` lifts the `Copy` bound on `Self`. A type that is not `Copy` may implement the trait, and its default bodies move `Self`. `Index`, `IndexSet`, and `Inspectable` are declared so.
14. r[ref.copy.collections] `List`, `Map`, `Set`, `Deque`, and `Heap` hold `Copy` items. A list or map literal whose items are not `Copy` is an error. Error: `unsatisfied-trait-bound`.

```text
struct Photo:
    width: u32

struct Writer['a]:
    target: &'a mut List[i32]

struct Holder['a]:
    target: &'a mut i32

impl !Copy for Photo

impl !Eq for Photo  # error: invalid-negative-impl

trait Sized2D:
    fn area(&self) -> u32

impl Sized2D for Photo:  # error: missing-supertrait-implementation
    fn area(&self) -> u32: self.width

fn keep[T < ?Copy](photo: T) -> void:
    pass

fn twice[T < ?Copy](x: T) -> (T, T):
    (x, x)  # error: use-of-moved-value

fn show[T < ?Eq](x: T) -> void:  # error: invalid-relaxed-bound
    pass

fn dup[T](x: T) -> (T, T): (x, x)

fn demo(items: &mut List[i32], x: &mut i32) -> void:
    writer := Writer { target: items }
    pair := dup(writer)  # error: unsatisfied-trait-bound
    hs := [Holder { target: x }]  # error: unsatisfied-trait-bound
    photo := Photo { width: 2 }
    keep(photo)
    w := photo.width  # error: use-of-moved-value
```

```text
struct Photo:
    width: u32
    pixels: List[u8]

impl !Copy for Photo

trait Stored < ?Copy:
    fn size(&self) -> u32

impl Stored for Photo:
    fn size(&self) -> u32: self.width

fn handed[T < ?Copy & Stored](item: T) -> (T, u32):
    size := item.size()
    (item, size)
```

> **Why.** Copy is the default, as in Swift, because most generic code
> copies its values and a `Copy` bound on every declaration would be noise.
> `?Copy` is Rust's spelling for lifting a default bound, as `?Sized` is.
> A trait's `Self` is `Copy` for the same reason: only then may a default
> body copy `self`.

See also: [Sealed Traits](09-traits.md#sealed-traits),
[Generic Bounds And Static Dispatch](09-traits.md#generic-bounds-and-static-dispatch).

## Not Yet Checked

2. r[ref.check.formatter] `hd fmt` formats a file that writes references. It keeps each reference token as written, `&mut` and `*mut` included, and a second run changes nothing.

```text
fn slice(items: List[i32]) -> void:
    part := &items[0..1]  # error: type-mismatch
```
