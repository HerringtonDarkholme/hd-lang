# Values And References

Status: language specification draft.

This chapter defines reference types and reference expressions: their syntax, their typing, and what they do at run time. A **reference type** is written `&T` or `&mut T`. Its values refer to a `T` that lives elsewhere.

`&T` reads the `T` it refers to, and `&mut T` also writes it. A reference to a variable or a field shares that place. A reference to any other value refers to a new object that holds a copy of it.

```hd
struct Body:
    pos: Point
    vel: Point

fn drift(body: &mut Body, step: &Point) -> void:
    body.pos = *step

fn spawn(pos: Point) -> &mut Body:
    &mut Body { pos: pos, vel: Point { x: 0, y: 0 } }
```

## Reference Types

A reference type is `&` or `&mut` followed by a type.

```hd
fn first(items: List[&Point]) -> Option[&mut Point]:
    .None

fn each(f: fn(&Point) -> &mut Point, pending: &Point?) -> void: pass
```

1. r[ref.type.forms] `&T` is a reference type, and `&mut T` is a writable reference type. The operand `T` is a named, `dyn`, tuple, or grouped type.
2. r[ref.type.position] A reference type may appear wherever a type may: a parameter, a result, a field, a binding annotation, a type argument, and a function type's parameters and result.
3. r[ref.type.optional] `?` applies to the whole reference type. `&T?` is an optional `&T`, as `mut T?` is an optional `mut T`.
4. r[ref.type.group] To refer to an optional or to another reference, group the operand: `&(T?)` and `&(&T)`.
5. r[ref.type.no-nesting] `&&T` is a syntax error, because `&&` is one token. So are `&mut mut T` and `mut &T`. Error: `syntax-error`.
6. r[ref.type.no-function] `&` never directly precedes `fn`: `&fn() -> i32` and `&mut fn() -> i32` are errors. Error: `syntax-error`.
7. r[ref.type.distinct] `&T` and `&mut T` are distinct types. Each is invariant in `T`: `&A` is `&B` only when `A` is `B`.
8. r[ref.type.view] A `mut` view written on the operand is no part of a reference type, so `&(mut Draft)` is `&Draft`. Whether a read through the reference may write is decided by the reference's own `mut` alone.
9. r[ref.type.any-ref] A reference type implements `AnyRef`.

```text
fn bad_and(p: &&Point) -> void: pass  # error: syntax-error
fn bad_mut(p: mut &Point) -> void: pass  # error: syntax-error
fn bad_fn(f: &fn() -> i32) -> void: pass  # error: syntax-error
```

See also: [Modifiers, Optionality, And Grouping](02-grammar.md#modifiers-optionality-and-grouping).

## Reference Expressions

A reference expression is a prefix operator and its operand. `&e` and `&mut e` make a reference, and `*e` reads through one.

```hd
fn demo(r: &mut Point, items: List[Point], i: i32) -> void:
    view := &items[i]
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
    one := &items[i].x  # &(items[i].x)
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

`&e` and `&mut e` refer to `e` when `e` is a place, and to a new object holding a copy of `e` otherwise.

```hd
struct Point:
    x: i32
    y: i32

fn make() -> Point:
    Point { x: 1, y: 2 }

fn demo() -> void:
    let mut p = Point { x: 0, y: 0 }
    r := &mut p        # refers to p
    r.x = 5            # p.x is now 5
    q := &mut p.y      # refers to the field y of p
    *q = 7             # p.y is now 7
    n := &make()       # a new object holding the result
    m := &mut Point { x: 3, y: 4 }  # a new object
```

1. r[ref.place.forms] Locals and parameters of any binding form, and top-level bindings, are places for `&` and `&mut`. A struct field of a place, a field of a data value, and a value reached through a reference are places too.
2. r[ref.place.through] `*r` with a reference `r` is a place, and so is a field read through `r`, such as `r.x`. The reference `r` is evaluated once, when the reference to the place is taken.
3. r[ref.expr.place] `&e` and `&mut e` on a place refer to that place. A later write to the place, through any path, is seen through the reference.
4. r[ref.expr.allocate] `&e` and `&mut e` on any other expression evaluate `e` and refer to a new object that holds the value. Each evaluation makes a new object.
5. r[ref.expr.allocate.writable] A new object is writable, so `&mut T { ... }` and `&mut make()` are valid.
6. r[ref.expr.allocate.view] `&mut e` on a value that is a readonly view is an error, since it would grant writes the view lacks. Error: `mutable-upgrade`.
7. r[ref.expr.type] The type of `&e` is `&T`, and of `&mut e` is `&mut T`, where `T` is the type of `e` without an outer `mut` view.
8. r[ref.place.alias] Two references to one place, or two copies of one reference, see each other's writes.
9. r[ref.place.lifetime] A reference keeps the place it refers to alive. A reference to a local may outlive the block that declares the local, and may be returned.
10. r[ref.place.receiver] A call of a `&self` or `&mut self` method takes a reference to its receiver as `&` and `&mut` would, with the place rules of this section. [Reference Receivers](07-functions.md#reference-receivers) states the call's rules.

> **Why.** A reference to a value that is not a place still needs storage
> to refer to, so it gets a new object, as Go's `&T{...}` does.

## Writable Places Of References

`&mut e` on a place needs the place to be writable.

1. r[ref.place.writable] `&mut e` on a place requires a writable place. A `let mut` local, a `mut` parameter, `mut self`, and a value reached through a `&mut` reference are writable. A struct field of a writable place, and a field of a data value with mutable access, are writable too.
2. r[ref.place.writable.top-level] A top-level binding is writable when it is bound with `let mut`.
3. r[ref.place.writable.view] When the place holds a readonly view, such as a readonly `List[T]`, `&mut` of it is an error, since it would grant writes the view lacks. Error: `mutable-upgrade`.
4. r[ref.place.shared-any] `&e` needs no writable place: it may refer to a `:=` binding, a plain `let` binding, or a value parameter.

`&mut` of a place that is not writable is an error, by what holds the place:

| Rule | The place is held by | Error |
| --- | --- | --- |
| r[ref.place.not-writable.short] Short binding | a `:=` or `using` binding | `non-reassignable-binding` |
| r[ref.place.not-writable.param] Parameter | a value parameter or `self` that is not `mut` | `non-reassignable-parameter-binding` |
| r[ref.place.not-writable.let] Plain `let` | a `let` binding without `mut` | `readonly-root` |
| r[ref.place.not-writable.shared] Read-only reference | a value reached through a `&T` reference | `readonly-root` |
| r[ref.place.not-writable.data] Data field | a field of a data value without mutable access | `readonly-root` |

5. r[ref.place.not-writable.fix] For a plain `let` binding and a value parameter, the diagnostic's fix-it writes `mut` before the binding's name.

```text
fn invalid(count: i32) -> void:
    let total = 0
    r := &mut total  # error: readonly-root
    fixed := 1
    s := &mut fixed  # error: non-reassignable-binding
    t := &mut count  # error: non-reassignable-parameter-binding
```

See also: [Writable Places](04-type-system.md#writable-places).

## Conversions

A `&mut T` converts to `&T` where a `&T` is expected.

1. r[ref.convert.weaken] A `&mut T` converts implicitly to `&T`, and to `&T?` where an optional is expected. The conversion makes no new reference: both refer to the same place.
2. r[ref.convert.no-upgrade] A `&T` never converts to `&mut T`. Using one where a `&mut T` is expected is an error. Error: `mutable-upgrade`.
3. r[ref.convert.no-other] A reference converts to no other type, and no other type converts to a reference.

```text
fn edit(r: &mut i32) -> void:
    *r += 1

fn view(r: &i32) -> i32:
    *r

fn invalid(r: &i32) -> &mut i32:
    edit(r)  # error: mutable-upgrade
    r        # error: mutable-upgrade
```

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
3. r[ref.deref.not-reference] `*e` on a value that is not a reference is an error. Error: `type-mismatch`.
4. r[ref.read.type] A read through `&mut T` has type `T`, with mutable access when `T` has views, as a data type or a list has.
5. r[ref.read.weaken] Read-only is transitive: a read through `&T` has type `T` with every `&mut U` inside it weakened to `&U`, and every `mut U` view to `U`, at any depth. A field of a value read through `&T` is weakened the same way.
6. r[ref.read.function] The weakening does not enter a function type: a function value read through `&T` keeps its type, and a call of it may change what it captured.
7. r[ref.read.copy-out] Copying a whole value out of a `&T` is an error when the value holds a `&mut` at any depth, inside struct, data, or enum fields included. Error: `mutable-upgrade`.
8. r[ref.read.copy-out.projection] A field read that is itself the base of another field read copies nothing, so `t.inner.lead` reads through `t.inner` without copying it.

```text
struct User:
    name: string

struct Team:
    lead: &mut User

fn lead_name(t: &Team) -> string:
    t.lead.name  # valid: a field read, weakened to &User

fn invalid(t: &Team) -> Team:
    *t  # error: mutable-upgrade
```

> **Why.** Without the copy-out rule, `*t` would be an ordinary `Team`
> whose `lead` is writable again, and read-only would not be transitive.

## Writing Through A Reference

`*r = v` replaces the value `r` refers to, and a field assignment through a `&mut` reference writes the referenced value's field.

```hd
struct Point:
    x: i32
    y: i32

fn reset(r: &mut Point) -> void:
    *r = Point { x: 0, y: 0 }
    r.x += 1
```

1. r[ref.deref.write] `*r = v` and `*r op= v` replace the value `r` refers to. `r` must be a `&mut` reference. Error: `readonly-root`.
2. r[ref.write.field] Assigning a field through a reference, as in `r.x = v`, writes the field of the referenced value. The reference must be a `&mut`. Error: `readonly-root`.
3. r[ref.write.seen] The write is seen through every reference to the place, and through the place itself.
4. r[ref.write.no-exclusivity] A `&mut T` is not exclusive: other references to the same place may exist and be used, and no check limits them.
5. r[ref.write.copy-assign] `*r ...= v` is an error, since `...=` stores into an embedded field only. Error: `copy-into-ordinary-field`.

```text
struct Point:
    x: i32
    y: i32

fn invalid(r: &Point) -> void:
    *r = Point { x: 0, y: 0 }  # error: readonly-root
    r.x = 1                    # error: readonly-root
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
2. r[ref.identity.places] Two references refer to the same place when they start at the same root and read the same struct fields from it. A root is one execution of a local's declaration, one call's parameter, a top-level binding, a field of one data value, or one new object.
3. r[ref.identity.copies] A copy of a reference, a `&mut T` converted to `&T`, and a reference passed to or returned from a function refer to the place the original refers to. So do `&*r` and `&mut *r`.
4. r[ref.identity.mutability] `is` compares a `&T` with a `&mut T`, since both may refer to one place.
5. r[ref.identity.incompatible] `is` between references to different types is an error. Error: `incompatible-identity-operands`.
6. r[ref.identity.non-reference] `is` between a reference and a value without identity, such as a struct, is an error. Error: `identity-requires-references`.
7. r[ref.identity.non-reference.identity] `is` between a reference and another value with identity, such as a data value, is an error too. Error: `incompatible-identity-operands`.
8. r[ref.identity.values] A struct, tuple, or enum value has no identity, even when it holds references, so `is` on one is an error. Error: `identity-requires-references`.
9. r[ref.identity.optional] An optional reference, `&T?`, holds one reference or none. The reference matched out of `.Some(r)` refers to the place `r` refers to.

```text
struct Point:
    x: i32

data User:
    name: string

fn invalid(a: &i32, b: &i64, p: &Point, v: Point, u: &User, w: User) -> void:
    one := a is b  # error: incompatible-identity-operands
    two := p is v  # error: identity-requires-references
    three := u is w  # error: incompatible-identity-operands
```

> **Why.** A reference in hd is counted and never dangles, so code may
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

## Not Yet Checked

1. r[ref.check.not-yet-elements] Two forms are errors until later jobs define them. They are a reference to a list element or a map value, as in `&xs[i]`, and a reference to a tuple element, as in `&pair._0`. Error: `references-not-yet-checked`.
2. r[ref.check.formatter] `hd fmt` formats a file that writes references. It keeps each reference token as written, `&mut` included, and a second run changes nothing.

```text
fn element(items: List[i32], pair: (i32, i32)) -> void:
    first := &items[0]  # error: references-not-yet-checked
    left := &pair._0  # error: references-not-yet-checked
```

> **Note.** An element reference pins its list, which needs rules of its
> own.
