# Values And References

Status: language specification draft, syntax only.

This chapter defines the syntax of reference types and reference expressions. A **reference type** is written `&T` or `&mut T`. Its values refer to a `T` that lives elsewhere.

What a reference means is not specified yet: its typing, its conversions, and its run-time behavior come with the checking job that follows the syntax. Until then, a program that writes a reference is rejected after parsing ([Not Yet Checked](#not-yet-checked)).

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

## Not Yet Checked

1. r[ref.check.not-yet] A reference type, `&e`, `&mut e`, or `*e` that reaches the checker is an error, once at each occurrence. Error: `references-not-yet-checked`.
2. r[ref.check.formatter] `hd fmt` formats a file that writes references. It keeps each reference token as written, `&mut` included, and a second run changes nothing.

```text
struct Point:
    x: i32
    y: i32

fn show(p: &Point) -> i32:  # error: references-not-yet-checked
    0

fn make() -> i32:
    p := Point { x: 1, y: 2 }
    q := &p  # error: references-not-yet-checked
    *q = Point { x: 3, y: 4 }  # error: references-not-yet-checked
    0
```

> **Note.** The checking job replaces this section with the rules for
> reference typing, conversion, identity, and writes through a reference.
