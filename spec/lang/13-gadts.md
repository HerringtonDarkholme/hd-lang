# Generalized Algebraic Data Types

Status: language specification draft.

This chapter defines GADT-style enums, whose variants refine the result type.

1. r[gadt.intro.refine] GADT-style enums let each variant refine the result instantiation of its enclosing generic enum.
2. r[gadt.intro.recover] Pattern matching recovers that refinement inside the selected arm.

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

1. r[gadt.result.declare] A variant may declare an explicit result type after `->`.
2. r[gadt.result.owner] The result's outer named type must be the enclosing enum. Any other outer type is an error. Error: `variant-result-owner`.
3. r[gadt.result.refine] The result's type arguments may refine declaration parameters to concrete types or variant-local generic parameters.
4. r[gadt.result.shared-data] The optional argument clause initializes shared enum constructor data.
5. r[gadt.result.default] A variant without an explicit result constructs the enclosing enum with its declaration type arguments, exactly as in the core enum model.

```text
enum Other[T]:
    Value(T)

enum Expr[T]:
    Broken(value: T) -> Other[T]  # error: variant-result-owner
```

See also: [Generic And Recursive Enums](08-data-and-enums.md#generic-and-recursive-enums).

## Shared Constructor Data

GADT refinement composes with enum-level constructor data:

```text
enum Box[T](contents: T):
    IntBox(n: i64) -> Box[i64](0)
    BoolBox(b: bool) -> Box[bool](false)
```

In this example, the result type refines `T`, while the call argument
initializes `contents`.

1. r[gadt.shared.assignable] The initialized expression must be assignable to the shared field type after applying the variant's refinement.

See also: [Shared Enum Constructor Data](08-data-and-enums.md#shared-enum-constructor-data).

## Construction

Construction uses the same enum-qualified function-call syntax as ordinary
variants:

```text
number := Expr.IntLit(42)  # Expr[i64]
flag := Expr.BoolLit(true) # Expr[bool]
```

1. r[gadt.construct.syntax] Construction uses the same enum-qualified function-call syntax as ordinary variants.
2. r[gadt.construct.order] Positional arguments precede named arguments.
3. r[gadt.construct.infer] Generic variant arguments are inferred from payload arguments and the expected result type.
4. r[gadt.construct.no-explicit] Variant constructors do not accept explicit generic arguments.
5. r[gadt.construct.ambiguous] Ambiguous inference is a compile-time error.

See also: [Variant Construction](08-data-and-enums.md#variant-construction).

## Pattern Refinement

Matching a GADT variant introduces type equalities for that arm. Given
`expr: Expr[T]`, the `IntLit` arm checks under `T = i64`, the `BoolLit` arm
under `T = bool`, and the `If` arm under its locally introduced result type:

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

1. r[gadt.refine.equalities] Matching a GADT variant introduces type equalities for that arm.
2. r[gadt.refine.arm-local] Refinement is arm-local.
3. r[gadt.refine.scope] Refinement affects payload binding types, nested calls, and the arm result check, then disappears after the arm.
4. r[gadt.refine.match-type] The complete match still has the result type required by its expected type.
5. r[gadt.refine.impossible] An arm whose variant result cannot unify with the subject type is an error, because it is statically impossible. Error: `impossible-gadt-pattern`.
6. r[gadt.refine.exhaustive] Exhaustiveness is checked over variants whose result types can inhabit the subject type.

```text
fn invalid(expr: Expr[i64]) -> i64:
    match expr:
        Expr.IntLit(value) => value
        Expr.BoolLit(value) => 0  # error: impossible-gadt-pattern
```

See also: [Refinement Algorithm](#refinement-algorithm),
[Match Expressions](06-control-flow.md#match-expressions).

## Payload Pattern Conventions

GADTs do not change enum pattern syntax:

```text
match expr:
    Expr.Add(l, r) => eval(l) + eval(r)
    Expr.Sub(left=l, right=r) => eval(l) - eval(r)
    Expr.Scale(value, factor=2) => eval(value) * 2
```

1. r[gadt.pattern.syntax] GADTs do not change enum pattern syntax.
2. r[gadt.pattern.positional] Bare positional identifiers bind new names and need not match declaration names.
3. r[gadt.pattern.named] Only `field=pattern` is a named payload pattern.
4. r[gadt.pattern.literal] Literals constrain exact payload values.
5. r[gadt.pattern.order] Positional patterns must precede named patterns.

See also: [Payload Patterns](08-data-and-enums.md#payload-patterns).

## Type-Checking Requirements

For each variant, the compiler must verify each of these requirements:

1. r[gadt.check.owner] The explicit result is an instantiation of the enclosing enum.
2. r[gadt.check.well-formed] Every result type argument is well formed under declaration and variant-local generic parameters.
3. r[gadt.check.payload] Payload types and shared constructor arguments are valid under that result refinement.
4. r[gadt.check.construct] Construction produces exactly the declared result instantiation.
5. r[gadt.check.no-escape] Pattern-arm equalities do not escape their arm.

> **Note.** The design does not require higher-kinded types.

### Existential Parameters

1. r[gadt.existential.def] A variant-local parameter that does not occur in the result is existential when that variant is matched.
2. r[gadt.existential.fresh] An existential parameter is fresh for the selected arm.
3. r[gadt.existential.bounds] An existential parameter may be used through its declared bounds.
4. r[gadt.existential.no-escape] An existential parameter must not escape the arm as an unconstrained concrete type.

## Runtime Representation

1. r[gadt.runtime.compile-time] Refinements are compile-time facts.
2. r[gadt.runtime.tag] Runtime enum values still carry their ordinary variant tag and payload.
3. r[gadt.runtime.erased] The backend need not preserve erased type arguments.
4. r[gadt.runtime.evidence] Constructing a variant whose existential parameter has bounds stores the evidence for those bounds in the value, as a trait value stores its dispatch metadata.
5. r[gadt.runtime.evidence.match] An arm that matches the variant uses that stored evidence for every call through the existential parameter's bounds.

```text
enum Shown:
    Item[U < Display](value: U) -> Shown

fn show(shown: Shown) -> string:
    match shown:
        Shown.Item(value) => value.to_string()

fn make() -> string:
    show(Shown.Item(42))
```

## Refinement Algorithm

This section defines how a variant result is unified with a match subject.

1. r[gadt.unify.first-order] For a subject `E[S1, ..., Sn]` and a candidate variant result `E[R1, ..., Rn]`, the checker performs first-order nominal unification.
2. r[gadt.unify.aliases] Unification happens after expanding transparent aliases.
3. r[gadt.unify.solvable] Declaration parameters and variant-local parameters may be solved.
4. r[gadt.unify.nominal] Distinct nominal types never unify merely because one converts to the other.
5. r[gadt.unify.solution] A successful solution becomes a set of arm-local type equalities and existential variables.

### Exhaustiveness And Nesting

1. r[gadt.unify.exhaustive] Exhaustiveness considers the closed set of variants with a successful unification.
2. r[gadt.unify.catch-all] A catch-all covers all remaining inhabitable variants.
3. r[gadt.unify.nested] Nested GADT patterns compose their equalities.
4. r[gadt.unify.contradiction] Contradictory equalities make the arm statically impossible.
5. r[gadt.unify.variance] Arm-local equalities do not change variance declarations.
6. r[gadt.unify.no-cast] Arm-local equalities are not runtime casts.

### Erasure And Reification

1. r[gadt.erasure.dynamic] Dynamic trait erasure discards GADT refinements.
2. r[gadt.erasure.no-descriptor] Matching a GADT does not supply `Inspectable` evidence for an erased parameter.

See also: [Variance](04-type-system.md#variance), which states when a variant
result makes a declaration parameter invariant.
