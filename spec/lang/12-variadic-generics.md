# Variadic Generics

Status: language specification draft.

hd has no variadic generics: no type packs, no value packs, and no pack
expansion. This chapter keeps its number so that links to it survive, and
it states no rules.

Code that works over every arity uses ordinary tuples instead:

| Need | Mechanism |
| --- | --- |
| a parameter that collects any number of arguments of any types | a vararg whose type is a type parameter bounded by `Tuple`, as in `args...: Args` ([Varargs](07-functions.md#varargs)) |
| a function of any arity | `Fn[Args, O, $ R]` or `SuspendFn[Args, O, $ R]` with `Args < Tuple` ([Function Type Constructors](07-functions.md#function-type-constructors)) |
| a call with a tuple of arguments | a tuple spread, `f(args...)` ([Positional Spreads](05-expressions.md#positional-spreads)) |
| awaiting children of different result types | the intrinsic `all!`, whose result is a tuple ([Standard Combinators](11-requirements-and-suspension.md#standard-combinators)) |
| equality, ordering, and hashing of tuples | derived by the compiler for every arity ([Implementation Targets](09-traits.md#implementation-targets)) |

```text
use std.function.{Fn, Tuple}

fn call[Args < Tuple, O, $R](f: Fn[Args, O, $ R], args...: Args) -> O $ R:
    f(args...)

fn add(a: i32, b: i32) -> i32: a + b

fn three() -> i32: call(add, 1, 2)
```

> **Note.** Packs were removed by owner decision Q9 of batch 31, on
> 2026-09-30.
