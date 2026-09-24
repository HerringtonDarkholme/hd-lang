# Generalized Algebraic Data Types

Status: language specification draft.

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

Enum variants use this grammar:

```ebnf
enum_variant = { decorator_line }, identifier, [ generic_params ], [ variant_parameter_clause ],
               [ "->", variant_result ], NEWLINE ;

variant_result = named_type, [ argument_clause ] ;
```

The result's outer named type must be the enclosing enum; any other outer type
is a `variant-result-owner` error. Its type arguments
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
inferred from payload arguments and the expected result type. Variant
constructors do not accept explicit generic arguments; ambiguous inference is a
compile-time error.

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
        Expr.Scale(value, factor) => eval(value) * factor
        Expr.If(cond, then_value, else_value) =>
            if eval(cond):
                eval(then_value)
            else:
                eval(else_value)
```

Refinement is arm-local. It affects payload binding types, nested calls, and the
arm result check, then disappears after the arm. The complete match still has
the result type required by its expected type.

An arm whose variant result cannot unify with the subject type is rejected as
statically impossible. Exhaustiveness is
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

The design does not require higher-kinded types. A variant-local parameter that
does not occur in the result is existential when that variant is matched. It is
fresh for the selected arm, may be used through its declared bounds, and must
not escape the arm as an unconstrained concrete type.

## Runtime Representation

Refinements are compile-time facts. Runtime enum values still carry their
ordinary variant tag and payload. The backend need not preserve erased type
arguments unless a reified operation requires them.

## Refinement Algorithm

For a subject `E[S1, ..., Sn]` and a candidate variant result
`E[R1, ..., Rn]`, the checker performs first-order nominal unification after
expanding transparent aliases. Declaration parameters and variant-local
parameters may be solved; distinct nominal types never unify merely because
one converts to the other. A successful solution becomes a set of arm-local
type equalities and existential variables.

Exhaustiveness considers the closed set of variants with a successful
unification. A catch-all covers all remaining inhabitable variants. Nested GADT
patterns compose their equalities; contradictory equalities make the arm
statically impossible. Arm-local equalities do not change variance declarations
and are not runtime casts.

Dynamic trait erasure discards GADT refinements. Reification preserves only
descriptors explicitly carried by a reified operation; matching a GADT does not
manufacture a descriptor for an erased parameter.
