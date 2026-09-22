# Generalized Algebraic Data Types

Status: provisional design.

GADT-style enums let each variant refine the result instantiation of its
enclosing generic enum. Pattern matching recovers that refinement inside the
selected arm.

## Variant Result Types

A variant may declare an explicit result type after `->`:

```text
enum Expr[T]:
    IntLit(value: i64) -> Expr[i64]
    BoolLit(value: bool) -> Expr[bool]
    Add(left: Expr[i64], right: Expr[i64]) -> Expr[i64]
    Sub(left: Expr[i64], right: Expr[i64]) -> Expr[i64]
    Scale(value: Expr[i64], factor: i64) -> Expr[i64]
    If[T](cond: Expr[bool], then_value: Expr[T], else_value: Expr[T]) -> Expr[T]
```

This chapter extends the core enum grammar:

```ebnf
gadt_variant = identifier, [ generic_params ],
               [ variant_parameter_clause ],
               [ "->", gadt_result ], NEWLINE ;

gadt_result = named_type, [ argument_clause ] ;
```

The result's outer named type must be the enclosing enum. Its type arguments
may refine declaration parameters to concrete types or variant-local generic
parameters. The optional argument clause initializes shared enum constructor
data.

A variant without an explicit result constructs the enclosing enum with its
declaration type arguments, exactly as in the core enum model.

## Shared Constructor Data

GADT refinement composes with enum-level constructor data:

```text
enum Box[T](contents: T):
    IntBox(n: i64) -> Box[i64](n)
    BoolBox(b: bool) -> Box[bool](b)
```

The result type refines `T`, while the call argument initializes `contents`.
The initialized expression must be assignable to the shared field type after
applying the variant's refinement.

## Construction

Construction uses the same enum-qualified function-call syntax as ordinary
variants:

```text
number := Expr.IntLit(42)  # Expr[i64]
flag := Expr.BoolLit(true) # Expr[bool]
```

Positional arguments precede named arguments. Generic variant arguments are
normally inferred from payload arguments and expected result type. If explicit
generic arguments are allowed, they use the ordinary square-bracket call
position; partial explicit arguments remain unsupported.

## Pattern Refinement

Matching a GADT variant introduces type equalities for that arm. Given
`expr: Expr[T]`, the `IntLit` arm checks under `T = i64`, the `BoolLit` arm under
`T = bool`, and the `If` arm under its locally introduced result type:

```text
fn eval[T](expr: Expr[T]) -> T:
    match expr:
        Expr.IntLit(value) => value
        Expr.BoolLit(value) => value
        Expr.Add(left, right) => eval(left) + eval(right)
        Expr.Sub(left, right) => eval(left) - eval(right)
        Expr.If(cond, then_value, else_value) =>
            if eval(cond):
                eval(then_value)
            else:
                eval(else_value)
```

Refinement is arm-local. It affects payload binding types, nested calls, and the
arm result check, then disappears after the arm. The complete match still has
the result type required by its surrounding context.

An arm whose variant result cannot unify with the subject type is unreachable
and should be rejected or diagnosed as statically impossible. Exhaustiveness is
checked over variants whose result types can inhabit the subject type.

## Payload Pattern Conventions

GADTs do not change enum pattern syntax:

```text
match expr:
    Expr.Add(l, r) => eval(l) + eval(r)
    Expr.Sub(left=l, right=r) => eval(l) - eval(r)
    Expr.Scale(value, factor=2) => eval(value) * 2
```

Bare positional identifiers bind new names and need not match declaration
names. Only `field=pattern` is a named payload pattern. Literals constrain exact
payload values. Positional patterns must precede named patterns.

## Type-Checking Requirements

For each variant, the compiler must verify:

1. the explicit result is an instantiation of the enclosing enum;
2. every result type argument is well formed under declaration and
   variant-local generic parameters;
3. payload types and shared constructor arguments are valid under that result
   refinement;
4. construction produces exactly the declared result instantiation;
5. pattern-arm equalities do not escape their arm.

The initial design does not require higher-kinded types. Whether existential
variant parameters are permitted beyond values hidden by ordinary
variant-local generics remains open.

## Runtime Representation

Refinements are compile-time facts. Runtime enum values still carry their
ordinary variant tag and payload. The backend need not preserve erased type
arguments unless a reified operation requires them.

## Open Issues

1. A formal unification and exhaustiveness algorithm for refined variants.
2. Explicit generic arguments on variant constructors.
3. Existential payload types and what pattern matching reveals.
4. Interaction with variance, dynamic trait values, and reified type metadata.
