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
18. r[ref.type.dyn.no-fn] `Fn` has no trait-value reference form, so local and counted `dyn Fn` references are errors. Error: `trait-not-dynamically-safe`.

> **Note.** [`ref.type.kinds.elsewhere`](#r-ref.type.kinds.elsewhere)
> bridges the chapters until the respelling slices of
> [#834](https://github.com/HerringtonDarkholme/hd-lang/issues/834) spell
> each such rule for both kinds. Those slices then remove it.

```text
use std.function.Fn

fn bad_and(p: &&Point) -> void: pass  # error: syntax-error
fn bad_mut(p: mut &Point) -> void: pass  # error: syntax-error
fn bad_fn(f: &fn() -> i32) -> void: pass  # error: syntax-error
fn bad_power(p: **Point) -> void: pass  # error: syntax-error
fn bad_dyn(f: &dyn Fn(i32) -> i32) -> void: pass  # error: trait-not-dynamically-safe
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
9. r[ref.expr.kind.no-counted.advice] The diagnostic tells the writer to box the value at its declaration, as `p := box(Point { x: 0 })`. It offers no fix-it, since boxing at the use would copy the value.
10. r[ref.place.alias] Two references to one place, or two copies of one reference, see each other's writes.
11. r[ref.place.receiver.local] A call of a `&self` or `&mut self` method on a place takes a local reference to it, as `&` and `&mut` would.
12. r[ref.place.receiver.counted] A call of a `*self` or `*mut self` method on a place is an error, as [`fn.method.counted-self.place`](07-functions.md#r-fn.method.counted-self.place) states. [Reference Receivers](07-functions.md#reference-receivers) states the rules of both calls.

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

1. r[ref.box.signature] `std.core` declares `box` as `@intrinsic pub fn box[T < 'static](value: T) -> *mut T`, and the prelude supplies it. The declaration is ordinary hd, and the compiler supplies only its body.
2. r[ref.box.check] A `box` call is checked by the plain call rules, so `T` is inferred from the argument and the expected type as for any generic call.
3. r[ref.box.new] `box(e)` evaluates `e` once and returns a `*mut T` to a new object that holds the value. Each call makes a new object.
4. r[ref.box.copy] The new object holds a copy of the value. When `e` is a place, a later write to that place is not seen through the result, and a write through the result does not change the place.
5. r[ref.box.weaken] The result converts by the rules of [Conversions](#conversions) and [Trait Value References](09-traits.md#trait-value-references), so it may be used where `*T` or `*mut dyn Tr` is expected.
6. r[ref.box.no-local] `box(e)` is an error when the type of `e` holds a local reference, by [`ref.local.holding`](#r-ref.local.holding), since that type does not meet the bound `'static` ([Generic Locality](#generic-locality)). Error: `local-reference-escapes`.
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
3. r[ref.local.holding] A type **holds** a local reference when it is a local reference type, or a tuple, an optional, an instance of a generic type, or a type alias whose type or type argument holds one. An opaque type that [borrows](04-type-system.md#r-types.opaque.borrows) holds one too.
4. r[ref.local.self.keep] A `&self` or `&mut self` receiver is a local reference, so these rules apply to `self` in such a method. A method that keeps `self` takes `*self` or `*mut self` ([Reference Receivers](07-functions.md#reference-receivers)). A `&self` method may still return a value that depends on `self` ([Dependent Results](#dependent-results)).
5. r[ref.local.escape.counted] A local reference never converts to a counted one. Using one where a counted reference is expected, as for a `*T` parameter or field, is an error. Error: `local-reference-escapes`.

A local reference escapes when it reaches one of these positions, and each is an error:

| Rule | A local reference | Example | Error |
| --- | --- | --- | --- |
| r[ref.local.escape.field] Field | is stored in a struct field or an enum payload field, or a field's declared type holds one | `point: &Point` | `local-reference-escapes` |
| r[ref.local.escape.element] Collection element | is stored as an element of a list, a map, or a set whose element type holds no local reference, by a literal or a method such as `push` | `points.push(p)` | `local-reference-escapes` |
| r[ref.local.escape.top-level] Top-level binding | is bound by a top-level binding | `let origin_ref: &Point = &origin` | `local-reference-escapes` |
| r[ref.local.escape.fn-value-result] Function value result | is the result of a function value whose type gives it no source among the inputs, as for a method bound to its receiver | `p::x_ref` for `fn x_ref(&self) -> &i32` | `local-reference-escapes` |
| r[ref.local.escape.capture] Closure capture | is captured by a closure, a captured `self` included | `fn() -> i32: p.x` | `local-reference-escapes` |
| r[ref.local.escape.type-argument] Type argument | is a type argument of a trait or a trait value, or of a list or a map where [`ref.lifetime.store.type`](#r-ref.lifetime.store.type) does not allow one, written or inferred | `List[&Point]`, `dyn Store[&Point]` | `local-reference-escapes` |

6. r[ref.local.escape.position-message] The diagnostic's message names the position the reference escapes to, a row of the table above or a counted reference.

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

fn first(points: List[&Point]) -> i32:  # error: local-reference-escapes
    0
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
    n := u.name()
    u.name = "changed"   # n refers to the field, and sees the write
    t := u.tags_mut()
    t.push("x")          # writes u.tags in place
```

1. r[ref.dep.result] A function's or a method's result type may hold a local reference: `&T`, `&mut T`, a tuple of them, or an instance of a generic struct or enum, such as `&T?` or `Result[&T, E]`.
2. r[ref.dep.result.opaque] An opaque result type that borrows is a dependent result too, and its hidden value follows these rules ([Opaque Results That Borrow](04-type-system.md#opaque-results-that-borrow)).
3. r[ref.dep.position] Each `&` or `&mut` in a parameter's type is one **reference position**: in `&self`, in `p: &T`, in `opt: &T?`, and in each element of `pair: (&A, &B)`. A `&` inside a function type is none.
4. r[ref.dep.elide] In a signature that writes no lifetime, a dependent result's source is `self` when the receiver is `&self` or `&mut self`. Otherwise it is the parameter of the one reference position.
5. r[ref.dep.elide.none] In a signature that writes no lifetime, a dependent result with no reference receiver and no reference position is an error. Error: `reference-result-without-source`.
6. r[ref.dep.elide.ambiguous] In a signature that writes no lifetime, a dependent result with no reference receiver and two or more reference positions is an error. Error: `ambiguous-reference-source`.
7. r[ref.dep.return] Every value a function returns, by `return` or as its body's value, may depend only on its source. Error: `local-reference-escapes`.
8. r[ref.dep.return.message] The diagnostic's message names the root the value depends on, as a local, a parameter's own storage, or another parameter. It has no fix-it.
9. r[ref.dep.call] A call's dependent result depends on what its source argument depends on.
10. r[ref.dep.call.local] The result is a local reference value of the caller, so every rule of [Local References](#local-references) applies to it. The caller may return it only as [`ref.dep.return`](#r-ref.dep.return) allows.
11. r[ref.dep.trait] An impl's method has the source of the trait method it implements. A call through a bound, an impl, or a trait value reference takes its source from the trait's signature.
12. r[ref.dep.store] A value that holds a local reference, stored through a reference, must land in a place of the function. A store into a place the caller owns, as through `out: &mut (&T)`, is an error, unless the place's type names the value's lifetime ([`ref.lifetime.store`](#r-ref.lifetime.store)). Error: `local-reference-escapes`.
13. r[ref.dep.caller] While a dependent result is in use, the caller may read, write, and reassign its source, and pass `&mut` of it. A read through the result sees the source's current value.
14. r[ref.dep.temporary] A dependent result of a call on a temporary, as in `make().name()`, keeps the temporary alive while the result is in use.

```text
struct User:
    name: string

fn bad_local() -> &string:  # error: reference-result-without-source
    u := User { name: "a" }
    &u.name

fn bad_pick(a: &User, b: &User) -> &User:  # error: ambiguous-reference-source
    a

fn bad_value(u: User, other: &User) -> &string:
    &u.name  # error: local-reference-escapes

fn bad_store(out: &mut (&User), u: &User) -> void:
    *out = u  # error: local-reference-escapes
```

> **Why.** These are Rust's elision rules for a result's lifetime, with no
> lifetime written. [Lifetime Variables](#lifetime-variables) gives the
> rules for a signature that writes one. A write to the source replaces the value in its place,
> and the place stays where it is, so the caller needs no exclusive access.

### Origins

An **origin** is a storage root that a local reference may point into. It is what a parameter refers to, which the caller owns, or a local of the function, the hidden local of `&e` on a value included. It may also be a parameter's own storage, which the call owns.

1. r[ref.dep.origin.set] Each value that holds a local reference has an origin set. The checker solves the sets once per function, per binding, regardless of the order of statements.
2. r[ref.dep.origin.least] The sets are the least solution of the constraints in the table below. A reassignment adds origins and never removes them.
3. r[ref.dep.origin.position] A value has one origin set per reference position outside another reference, so the positions of `(&A, &B)` and of `Result[&A, &B]` have separate sets.
4. r[ref.dep.origin.position.parts] A construction and a projection keep each part in its own positions: a tuple element, an optional's payload, and a field whose declared type is a type parameter. So does a lifetime struct's field of one lifetime ([`ref.lifetime.struct.read`](#r-ref.lifetime.struct.read)).
5. r[ref.dep.origin.position.union] Every position takes the union of all of them for any other part, for a value stored through a reference, and for the hidden local of `&e`.

| Rule | Expression | Origins |
| --- | --- | --- |
| r[ref.dep.origin.param] Parameter | `p`, for a parameter whose type holds a local reference | what `p` refers to |
| r[ref.dep.origin.place] Place | `&x` or `&x.f.g`, for a local or a parameter `x` | `x`'s own storage |
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

trait Named['a]:  # error: references-not-yet-checked
    fn name(&self) -> usize

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
14. r[ref.lifetime.not-yet.other-owner] Any other lifetime parameter is not checked yet, so it is an error: a trait's own, a `type` declaration's, a local function's, a local struct's, or a local enum's. A top-level struct's follows [Lifetime Structs](#lifetime-structs), and a top-level enum's [Lifetime Enums](#lifetime-enums). Error: `references-not-yet-checked`.
15. r[ref.lifetime.alias-in-order] A type alias passes on the lifetimes written in its arguments when its type uses each parameter once, in order, outside function types, and writes no `&` of its own. So `Option[&'a T]` is `&'a T?`.
16. r[ref.lifetime.not-yet.alias] A lifetime written in the arguments of any other type alias is an error too. Error: `references-not-yet-checked`.
17. r[ref.lifetime.not-yet.trait-target] A lifetime written in a trait implementation's target is an error too. Error: `references-not-yet-checked`.
18. r[ref.lifetime.not-yet.body] A lifetime variable written inside a function body is an error too. Error: `references-not-yet-checked`.

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
[Lifetime Structs](#lifetime-structs).

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

fn keep(text: string) -> void:
    p := Parser { src: &text, pos: 0 }
    b := box(p)  # error: local-reference-escapes
```

1. r[ref.lifetime.struct] A top-level struct may declare lifetime parameters, as in `struct Parser[T, 'src]`.
2. r[ref.lifetime.struct.field] A field of a lifetime struct may write `&'a T` or `&'a mut T` with one of the struct's lifetimes. Such a field is no escape to a field.
3. r[ref.lifetime.struct.argument] A type names a lifetime struct with one lifetime argument per lifetime parameter, as in `Parser[string, 'a]`. The lifetime arguments take the lifetime parameters in order.
4. r[ref.lifetime.struct.argument.count] A struct type with lifetime arguments, but not one per lifetime parameter of the struct, is an error. Error: `argument-count`.
5. r[ref.lifetime.struct.argument.other] A lifetime argument of a trait or a type alias is not checked yet, so it is an error. Error: `references-not-yet-checked`.
6. r[ref.lifetime.struct.holds] A lifetime struct holds a local reference ([`ref.local.holding`](#r-ref.local.holding)), so every rule of [Local References](#local-references) applies to its values.
7. r[ref.lifetime.struct.position] Each lifetime parameter is one reference position of the struct's type, after the positions of its type arguments. A written lifetime argument names that position's lifetime ([`ref.lifetime.position`](#r-ref.lifetime.position)).
8. r[ref.lifetime.struct.construct] A struct literal adds the origins of a field typed `&'a T` to the value's `'a` position only.
9. r[ref.lifetime.struct.read] Reading a field typed `&'a T` gives a reference whose origins are the origins of the value's `'a` position.
10. r[ref.lifetime.struct.read.one-lifetime] A field of another type whose reference positions all name one lifetime, as `Option[&'a T]` or `Parser[T, 'a]`, is constructed and read the same way.
11. r[ref.lifetime.struct.read.mixed] A field whose reference positions name two or more lifetimes is one of the other parts of [`ref.dep.origin.position.union`](#r-ref.dep.origin.position.union).
12. r[ref.lifetime.struct.field-elided] A field whose type names a lifetime struct without lifetime arguments holds a local reference, so it is an error. Error: `local-reference-escapes`.
13. r[ref.lifetime.struct.elide] A signature may name a lifetime struct without lifetime arguments. Each elided argument is a reference position that writes no lifetime ([`ref.lifetime.position`](#r-ref.lifetime.position)).
14. r[ref.lifetime.struct.elide.parameter] In a parameter's type, each elided argument has a lifetime of its own, so `p: &Parser[string]` has two reference positions.
15. r[ref.lifetime.struct.elide.result] In the result type, each elided argument takes its lifetime as [`ref.lifetime.elide`](#r-ref.lifetime.elide) states, as in `fn make(s: &string) -> Parser[string]`.
16. r[ref.lifetime.struct.elide.ambiguous] So a result that writes no lifetime, with no reference receiver and two lifetimes among the parameters' positions, is an error, as in `fn src(p: &Parser[string]) -> &string`. Error: `ambiguous-reference-source`.
17. r[ref.lifetime.struct.impl] An inherent implementation's target may write the implementation's lifetimes, as in `impl[T, 'a] Parser[T, 'a]`. Its methods' `self` and `Self` have those lifetimes.
18. r[ref.lifetime.struct.impl.elided] An implementation's target that names a lifetime struct without lifetime arguments is not checked yet, so it is an error. Error: `references-not-yet-checked`.
19. r[ref.lifetime.struct.body] A function body names a lifetime struct without lifetime arguments, as in `let p: Parser[string] = q`.

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

> **Note.** Lifetimes on traits, a trait implementation for a lifetime
> struct, and lifetime arguments elided in an implementation's target come
> in later stages.

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

fn keep(text: string) -> void:
    t := Token.Word(&text)
    b := box(t)  # error: local-reference-escapes
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

fn unnamed(seen: &mut List[&User], u: &User) -> void:  # error: local-reference-escapes
    pass

fn remember['a](seen: &mut List[&'a User], u: &'a User) -> void:
    seen.push(u)

fn count(b: bool) -> usize:
    let mut seen: List[&User] = []
    if b:
        local := User { name: "local" }
        remember(&mut seen, &local)
    seen.len()  # error: local-reference-outlives-local
```

1. r[ref.lifetime.store] A store through a parameter's reference into a place the caller owns is valid when the type that reference refers to has one lifetime in all its reference positions. The stored value must depend only on what a result of that lifetime may depend on ([`ref.lifetime.return`](#r-ref.lifetime.return)). Any other such store of a value that holds a local reference is an error ([`ref.dep.store`](#r-ref.dep.store)). Error: `local-reference-escapes`.
2. r[ref.lifetime.store.collection] A list's `push` and a map's index assignment store their value into the list's or the map's place, so `seen.push(u)` follows [`ref.lifetime.store`](#r-ref.lifetime.store) when `seen` is such a parameter.
3. r[ref.lifetime.store.call] A `&mut` parameter whose referent has a reference position of a lifetime variable, as `seen: &mut List[&'a T]`, is a place the call may store into. So the argument's place takes what the call's arguments in that lifetime's positions depend on, as for a type parameter ([`ref.generic.store`](#r-ref.generic.store)).
4. r[ref.lifetime.store.type] A list's or a map's type argument may hold a local reference in a function body. It may hold one in the type of a `&mut` parameter too, when each other `&` of that type writes a lifetime variable, as in `seen: &mut List[&'a User]`.
5. r[ref.lifetime.store.element] A value read from a list or a map, as `seen[0]` or the result of `get`, has the origins of the collection's elements.

```hd
struct User:
    name: string

fn remember[T, 'a](seen: &mut List[&'a T], u: &'a T) -> void:
    seen.push(u)  # u has 'a, as the list's elements do

fn index['a](by: &mut Map[string, &'a User], u: &'a User) -> void:
    by[u.name] = u

fn set['a](out: &mut (&'a User), u: &'a User) -> void:
    *out = u

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
2. r[ref.generic.static.meets] A type meets `'static` when it holds no local reference, by [`ref.local.holding`](#r-ref.local.holding), and each type parameter it names is bounded by `'static`. A counted reference and a function type always meet it: `box` made the object from a value that met it, and a closure captures no local reference.
3. r[ref.generic.static.error] At a call, a written or inferred type argument that does not meet `'static`, for a parameter bounded by it, is an error. Error: `local-reference-escapes`.
4. r[ref.generic.static.impl] An implementation whose type parameter is bounded by `'static` applies only where that parameter's argument meets `'static`, as for any other bound.
5. r[ref.generic.static.capture] A closure's capture whose type names a type parameter must meet `'static`. Error: `local-reference-escapes`.
6. r[ref.generic.static.no-fix] Neither diagnostic has a fix-it.
7. r[ref.generic.static.bound-only] `'static` is written only as a type parameter's bound. A reference type that writes it, as `&'static T`, is an error, and so is any other lifetime bound. Error: `references-not-yet-checked`.

```text
struct User:
    name: string

fn keep[T](value: T) -> *mut T:
    box(value)  # error: local-reference-escapes

fn later[T](value: T) -> fn() -> T:
    fn() -> T: value  # error: local-reference-escapes

fn wrap(u: &User) -> void:
    b := box(u)  # error: local-reference-escapes

fn view(u: &'static User) -> void:  # error: references-not-yet-checked
    pass
```

> **Why.** This is Rust's `T: 'static`. A counted reference and a function
> value carry no lifetime, so a value kept in one must hold no local
> reference.

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
3. r[ref.generic.call] A reference position that a type argument adds to a call's result depends on the argument positions that the same type parameter adds to the call's parameters, as a lifetime variable's positions do ([`ref.lifetime.call`](#r-ref.lifetime.call)).
4. r[ref.generic.call.unaligned] Where an argument's type does not line up with its parameter's declared type, such a result position depends on every argument.
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

## Weak Reference Runtime

Runtime support for a weak value records a counted allocation's identity
without keeping the allocation alive. The public weak-reference API is in
the standard library; these rules define the runtime operations beneath it.

1. r[ref.weak.intrinsics.construction] `std.weak` privately declares `weak_new[T](target: &T) -> Weak[T]?` with `@intrinsic("weak_new")` and `weak_new_mut[T](target: &mut T) -> WeakMut[T]?` with `@intrinsic("weak_new_mut")`; the compiler supplies only their bodies.
2. r[ref.weak.intrinsics.upgrade] `std.weak` privately declares `weak_upgrade[T](value: Weak[T]) -> (*T)?` with `@intrinsic("weak_upgrade")` and `weak_upgrade_mut[T](value: WeakMut[T]) -> (*mut T)?` with `@intrinsic("weak_upgrade_mut")`; the compiler supplies only their bodies.
3. r[ref.weak.intrinsics.weaken] `std.weak` privately declares `weak_weaken[T](value: WeakMut[T]) -> Weak[T]` with `@intrinsic("weak_weaken")`; the compiler supplies only its body.
4. r[ref.weak.intrinsics.private] These declarations are implementation hooks; the public functions, methods, and conversion in `std.weak` are ordinary hd wrappers over them.

### Weak Target Semantics

1. r[ref.weak.target.whole] A weak-target check succeeds only for the start of a whole counted allocation whose stored concrete type is the requested type.
2. r[ref.weak.target.refuse] Otherwise it refuses without making a weak value or panicking, including for interior fields, elements, and zero-offset first fields passed through parameters.
3. r[ref.weak.entry.shared] Every successful weak-target check for one live allocation names one shared **weak entry**; a later allocation that reuses the target's address has another entry.
4. r[ref.weak.non-owning] A weak entry does not retain its target, and the cycle collector does not count it as an edge.
5. r[ref.weak.target.free] Before an allocation is freed by reference counting or cycle collection, every weak entry for it becomes dead and forgets the target address.
6. r[ref.weak.upgrade.live] Upgrading a live weak entry retains its target and returns an ordinary counted reference with the requested access.
7. r[ref.weak.upgrade.dead] Upgrading a dead weak entry refuses and creates no counted reference.
8. r[ref.weak.upgrade.lifetime] A counted reference returned by an upgrade keeps the target alive by [`ref.box.lifetime`](#r-ref.box.lifetime), including while a suspension retains that reference.
9. r[ref.weak.entry.lifetime] A weak entry remains usable while any weak value names it; it is freed after the last such value, whether its target lives or not.

> **Why.** A run-time whole-object check works through helper functions,
> while the stored concrete type rejects a first field whose address equals
> its containing allocation. Clearing before reuse keeps an old weak value
> from observing a later object at the same address.

See also: [Weak References](../std/weak.md).

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
> slice. An element reference instead pins the collection while it lives,
> so a length change cannot silently detach the reference. A handle that
> survives a sort is a reference stored in the list, `List[*mut Player]`,
> or an ID.

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
    *best += 1
```

1. r[ref.elem.local-value] An element reference is a local reference value. It may be bound, assigned, passed, and used as the source or result of a dependent call, subject to every rule of [Local References](#local-references) and [Dependent Results](#dependent-results).
2. r[ref.elem.access.live] The reference's **access** to its collection begins when the reference is taken and ends after its last use.
3. r[ref.elem.access.live-length] While an access lasts, nothing may change the collection's length. Reads of the collection, and writes to its elements, the referenced one included, stay valid.
4. r[ref.elem.unaliased] A collection is **unaliased** when it is a local binding or a value parameter, or a struct field or element reached from one through no reference, and no escaping closure captures that binding.
5. r[ref.elem.escaping] A closure is **escaping** when it is passed as an argument, returned, stored in a field, an element, a payload, or a top-level binding, or captured by an escaping closure.
6. r[ref.elem.conflict] During an access to an unaliased collection, another argument or the receiver of the same call that may change the collection's length is an error. Error: `conflicting-access`.
7. r[ref.elem.access.pin] Every access **pins** its collection while it lasts, so a length change that no rule rejects at compile time panics.
8. r[ref.elem.pinned] While a list is pinned, an operation that changes its length is a checked runtime panic: `push`, `insert`, `remove_at`, and a `pop` or a `clear` that removes an element. Panic: `collection-pinned`.
9. r[ref.elem.pinned.map] While a map is pinned, a store of a key it does not hold and a `remove` of a key it holds are checked runtime panics. Panic: `collection-pinned`.
10. r[ref.elem.pinned.allowed] Replacing an element keeps the length, so it is allowed while the list is pinned: an element store, a write through an element reference, and a sort or a swap written with element stores.
11. r[ref.elem.pinned.end] A collection that several accesses pin stays pinned until each has ended.

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
