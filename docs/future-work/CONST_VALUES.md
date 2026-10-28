# Compile-Time Values

Status: design exploration, 2026-10-28; nothing here is decided or in the specification.

This record explores [issue #875](https://github.com/HerringtonDarkholme/hd-lang/issues/875).
It reviews const generic parameters, compile-time-only value parameters, and validation of static inputs.

It reviews the accepted [Generics](../../spec/lang/04-type-system.md#generics), [Type Inference Boundaries](../../spec/lang/04-type-system.md#type-inference-boundaries), and [Facts](../../spec/lang/14-annotations.md#facts) rules.
It also uses the SIMD evidence in the [Performance Roadmap](PERFORMANCE_ROADMAP.md#24-what-the-simd-probe-settled) and the repository design-cost order.

## Contents

- [The Domain](#the-domain)
- [Problem](#problem)
- [What hd Has Today](#what-hd-has-today)
- [Survey](#survey)
- [Option: Typed Const Parameters And Const Functions](#option-typed-const-parameters-and-const-functions)
- [Option: Unified Comptime Evaluation](#option-unified-comptime-evaluation)
- [Option: Promoted Literal Types](#option-promoted-literal-types)
- [Option: Purpose-Built Compiler Operations](#option-purpose-built-compiler-operations)
- [Comparison](#comparison)
- [Recommendation](#recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Sources](#sources)
- [Parse Log](#parse-log)

## The Domain

A runtime value exists while a program runs. A compile-time value is known while the compiler checks and specializes the program.

A const generic makes a value part of a declaration's parameters. `Array[u8, 16]` and `Array[u8, 32]` are then different static types.

A compile-time-only value parameter instead constrains one call. A regex constructor can require a static pattern without making that pattern part of `Regex`'s type.

These uses share constant evaluation, but they do not require identical type rules. Type identity needs equality and normalization; call validation only needs a definite value before code generation.

Consider a packet with exactly sixteen bytes. Its size affects layout, so a dynamic list and a runtime length check cannot provide the same guarantee.

```text
struct Packet:
    bytes: List[u8]

fn checked_packet(bytes: List[u8]) -> Packet?:
    if bytes.len() != 16:
        return .None
    .Some(Packet { bytes: bytes })
```

The check proves one value, not every `Packet`. A fixed-size type moves the invariant into construction and permits stack layout or SIMD lowering.

Naive designs fail in four places:

1. Evaluating any ordinary function at compile time admits I/O, requirements, suspension, and mutable identity.
2. Treating every expression as a type requires a theory for equality such as whether `N + 1` equals `1 + N`.
3. Specializing code for every static string can multiply code size and package-interface work.
4. Validating a regex is insufficient if the compiler cannot materialize its compiled program as a runtime value.

## Problem

The design must serve five fixed use cases:

1. `Array[T, N]`, where `N` determines layout and type identity.
2. SIMD code, where a lane count determines available operations and lowering.
3. `Regex::must_compile(pattern)`, where a static pattern cannot fail at runtime.
4. `Url::must_parse(text)`, with the same static-input guarantee.
5. Structure metadata whose field handles can become compile-time constants without runtime initialization.

It must also define constant expressions, inference, package interfaces, evaluation failure, and materialization of compiled values.

## What hd Has Today

Generic parameters denote complete types, not values ([Generics](../../spec/lang/04-type-system.md#generics)).
Their declarations are explicit, while use sites infer arguments only when unambiguous.

Generic representation is intentionally unobservable. The compiler may share or specialize bodies, and package interfaces carry required generic bodies.

The language has no general constant-expression evaluator. `T::name()` is a narrow compiler-provided constant available only inside a template.

Facts do not supply compile-time evaluation. Their expressions run lazily at runtime when code first reads the fact or builds its handle.

`Regex::new` and `Url::parse` return `Result` today. Regex compilation builds lists and strings, so a static constructor must embed an immutable result or reconstruct it at runtime.

The SIMD probe found large scan gains, but SIMD remains parked until hd can express lane-dependent code. The measurements are in [Performance Roadmap section 2.4](PERFORMANCE_ROADMAP.md#24-what-the-simd-probe-settled).

## Survey

| Language or tool | Mechanism | Boundary relevant to hd |
| --- | --- | --- |
| [Rust](https://doc.rust-lang.org/reference/items/generics.html#const-generics) | Items may take typed const parameters. [Const contexts](https://doc.rust-lang.org/reference/const_eval.html) evaluate a specified subset, including calls of const functions. | Stable const parameters use scalar types; restrictions keep type equality and evaluation bounded. |
| [Zig](https://ziglang.org/documentation/master/#comptime) | A `comptime` parameter accepts any compile-time-known value. The same evaluation model drives generics and compile-time control flow. | One powerful mechanism is regular, but compile-time and runtime code are deeply interleaved. |
| [Mojo](https://mojolang.org/docs/manual/parameters/) | Parameters are compile-time values, may be inferred from static argument types, and specialize functions or structs. Parameterized `comptime` values can produce types. | This directly serves SIMD, but gives compile-time values broad language reach. |
| [Nim](https://nim-lang.org/docs/manual.html#statict) | `static[T]` parameters require constant expressions and specialize per distinct value. They work in function and generic type signatures. | Its own manual uses a precompiled-regex example, matching both hd use classes. |
| [D](https://dlang.org/spec/template.html#valueparameters) | Templates accept statically evaluated value parameters. [CTFE](https://dlang.org/spec/function.html#interpretation) runs ordinary functions when a context requires a compile-time value. | CTFE reuses function bodies, but compiler eligibility depends on executed operations. |
| [Scala 3](https://docs.scala-lang.org/sips/42.type.html) | Literal singleton types preserve exact values in the type system. [`inline` parameters](https://docs.scala-lang.org/scala3/reference/metaprogramming/inline.html) require constant expressions and enable compile-time reduction. | Type identity and call-time specialization are separate but interacting mechanisms. |
| [Haskell with GHC](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/exts/data_kinds.html) | `DataKinds` promotes data constructors. [Type-level literals](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/exts/type_literals.html) have natural, string, and character kinds with witness constraints. | Type-level computation is expressive, but runtime recovery needs witnesses such as `KnownNat`. |
| [TypeScript](https://www.typescriptlang.org/docs/handbook/2/everyday-types.html#literal-types) | String, number, and boolean literals can be singleton types, including as generic type arguments. | Literal types validate APIs, but do not choose native layout or precompute runtime objects. |
| [Go](https://go.dev/ref/spec#Type_parameter_declarations) | Generic declarations have type parameters only. Untyped constants influence type inference, but values are not generic arguments. | Refusing value generics keeps the model small and leaves fixed-size abstractions to dedicated types or generation. |

Three patterns emerge. Rust, Nim, D, Zig, and Mojo pass values directly; Haskell, Scala, and TypeScript reflect values through types.

The direct-value systems still restrict evaluation or effects. No surveyed system obtains safe compile-time execution merely by adding a parameter spelling.

Go is the useful negative case. It preserves simple generics by declining fixed-size value-indexed abstractions.

## Option: Typed Const Parameters And Const Functions

Add a const-kinded generic slot and a const-qualified ordinary parameter. Add explicit const functions whose bodies pass a restricted evaluator check.

```text
struct Array[T, N < const usize]:  # hypothetical syntax
    storage: intrinsic_array[T, N]  # hypothetical syntax

const fn compile_pattern(pattern: string) -> Regex:  # hypothetical syntax
    Regex::new(pattern).expect("invalid static regex")

fn must_compile(pattern: const string) -> Regex:  # hypothetical syntax
    compile_pattern(pattern)
```

`N` is part of `Array`'s type identity. A const value parameter is known at the call but need not appear in its result type.

Initial const types would be integers, `bool`, `char`, and `string`. Type positions would initially accept literals, const bindings, and standalone const parameters, not symbolic arithmetic.

Const functions would reject requirements, host calls, suspension, mutable references, pointer identity, and recursion without a compiler fuel limit. Failure while evaluation is required would be a compile-time diagnostic at the call.

Materialized results would contain values only. Strings, lists, structs, and enums would become frozen static data; mutable references and dynamic trait values would be excluded initially.

This adds language-tier parameter kinds, constant expressions, evaluation, type identity, and diagnostics. `Array` and SIMD types could then be ordinary stdlib APIs over low-level layout intrinsics.

Traits may take const arguments, and implementations would match exact evaluated values. Requirement rows remain unavailable during evaluation, and suspension is forbidden.

Inference stays at use sites. `N` may be inferred from an argument of type `Array[T, N]`, never from a runtime integer or from a declaration body.

Derivation may use const metadata after the compiler exposes handles as materializable values. RC needs frozen values with no reference count and no observable mutable identity.

Soundness requires deterministic evaluation, stable equality for const arguments, bounded resource use, and target-independent results where type identity depends on them.

## Option: Unified Comptime Evaluation

Follow Zig and Mojo: mark a parameter `comptime`, then run ordinary hd code whenever its inputs are known. Types and values share one compile-time evaluator.

```text
struct Array[T, comptime N: usize]:  # hypothetical syntax
    storage: intrinsic_array[T, N]  # hypothetical syntax

fn must_compile(comptime pattern: string) -> Regex:  # hypothetical syntax
    Regex::new(pattern).expect("invalid static regex")
```

There is no separate `const fn`. An ordinary function may run at compile time if every reached operation is eligible.

This adds language-tier compile-time parameters, evaluation staging, type identity, and diagnostics. It can eventually support type-valued computation as Mojo does, though that is not required initially.

Traits and derivation gain the same expressive compile-time values as functions. Requirement providers, suspension, and host operations must still be absent from a compile-time execution path.

Inference follows static types at use sites. A comptime parameter cannot be inferred from a runtime value merely because optimization later proves it constant.

RC materialization has the same frozen-data requirement as the first option. The evaluator must diagnose a forbidden operation only when execution reaches it, which makes agent-facing errors more path-dependent.

Soundness requires a precise staging boundary. A refactor that changes whether a helper reaches I/O can change a formerly valid compile-time call.

## Option: Promoted Literal Types

Promote literal values into types, following Haskell and Scala. Generic declarations remain type-parameterized, while witness traits recover a runtime value.

```text
struct Array[T, N < ConstUsize]:  # hypothetical syntax
    storage: intrinsic_array[T, N::value()]  # hypothetical syntax

type Block = Array[u8, 16]  # hypothetical syntax

fn must_compile[P < StaticString](pattern: P) -> Regex:  # hypothetical syntax
    Regex::new(P::value()).expect("invalid static regex")
```

Numeric and string literals inhabit singleton types. Compiler-provided traits such as `ConstUsize` and `StaticString` expose their values.

This adds language-tier literal types, promotion, kinds or sealed witness traits, type-level equality, and type-level operations. It avoids a second value-parameter namespace.

Traits can constrain literal types naturally. Requirement rows and suspension stay outside type-level computation, while derivation can carry literal metadata through witness types.

Use-site inference may preserve a literal type only when a parameter explicitly asks for a singleton bound. Ordinary literals still widen, preventing action at a distance.

RC is simple for the witness value, but producing a compiled `Regex` still needs macros, const evaluation, or a compiler hook. Literal types alone validate identity, not arbitrary parsing.

Soundness requires kind checking and terminating type-level reduction. The mechanism is much larger than hd's current complete-type-only generic model.

## Option: Purpose-Built Compiler Operations

Add no user-defined const parameters. Provide compiler-known fixed arrays, SIMD vectors, static regex construction, static URL construction, and constant structure handles.

```text
type Block = FixedArray[u8, 16]  # hypothetical syntax

fn route() -> Url:
    Url::must_parse("https://example.com/api")  # hypothetical syntax

fn matcher() -> Regex:
    Regex::must_compile("[a-z]+")  # hypothetical syntax
```

Each API has a dedicated intrinsic or checker. The compiler validates literals and embeds the relevant runtime representation.

This adds language-tier knowledge of each API but no general constant-expression or generic-value rules. It deliberately drops user-defined fixed-size types and user-defined static validators.

Traits, requirements, suspension, and derivation remain unchanged except for the one structure-handle intrinsic. Inference has no new work.

The compiler owns all materialization, so RC representation can be correct per built-in. Each new use case needs another compiler feature and duplicates validation logic from std.

Soundness is local to each intrinsic. Evolution is expensive because library APIs become compiler contracts by name.

This is the radical simplification. It solves the named examples with the smallest semantic surface, but conflicts with hd's preference for ordinary core-library code.

## Comparison

| Criterion | Typed const parameters | Unified comptime | Promoted literal types | Purpose-built operations |
| --- | --- | --- | --- | --- |
| Fixed arrays | Direct, value-indexed generic | Direct, value-indexed generic | Direct through literal types | Built-in array only |
| SIMD lanes | Direct and bounded | Direct and highly expressive | Direct through witness types | Built-in vector only |
| Static regex and URL | Explicit const function plus frozen result | Ordinary function at compile time | Needs another macro or evaluator | Dedicated compiler implementation |
| Const structure handles | Materialized const values | Materialized comptime values | Witness types or promoted metadata | Dedicated compiler implementation |
| Rules added | Const kind, expressions, evaluator, identity | Staging, evaluator, identity | Literal types, kinds, witnesses, reduction | One compiler contract per API |
| Soundness boundary | Checked const function body | Reached execution path | Type-level termination and equality | Each intrinsic implementation |
| Agent-writability | Explicit declaration and stable diagnostics | Compact, but staging failures are contextual | Most unfamiliar syntax and errors | Easy calls, no user extension |
| Human readability | Const intent appears in signatures | One keyword covers many meanings | Values disguised as types | Simple surface, hidden compiler coupling |
| Implementation cost | High once | Highest initial evaluator reach | Highest type-system cost | Low per first API, cumulative |
| Code-size risk | One instantiation per used const identity | Broad specialization pressure | One instance per literal type | Compiler-controlled per operation |
| Evolution | Can widen const types and evaluator deliberately | Tends toward general metaprogramming | Tends toward dependent typing | Every extension changes compiler and std together |
| Design-cost order | New syntax and semantics; std APIs remain ordinary | New syntax and broad semantics | New syntax and type-system semantics | Several compiler intrinsics |

By the repository's cost order, purpose-built operations have the preferred costliest mechanism: compiler intrinsics instead of syntax. They need at least four named compiler contracts.

The other three options all require syntax. Among them, typed const parameters restrict evaluation more than unified comptime, while promoted literals add the largest type-system mechanism.

## Recommendation

Choose **typed const parameters and explicit const functions**, in staged slices.

1. First add scalar const generic parameters with closed evaluated arguments and standalone parameters in type positions.
2. Then add const value parameters and explicit const functions, excluding requirements, suspension, references, and host effects.
3. Add frozen materialization only for immutable value graphs before promising `Regex::must_compile` or `Url::must_parse`.

This is less magical than unified comptime execution. It keeps declaration intent visible and gives diagnostics a stable function boundary.

The first slice supports arrays and SIMD without committing to compile-time heap objects. The second and third slices serve static validators when their representation is understood.

This gives up arbitrary ordinary-code evaluation and symbolic generic arithmetic. The next-best option is unified comptime if the owner values one metaprogramming mechanism above a narrow boundary.

The purpose-built option ranks better by mechanism under hd's design-cost order. It loses because the issue also asks for user-defined const generics and validators, not only four built-ins.

## Questions For The Owner

1. **Should constness be a generic-parameter kind and parameter modifier?**
   - **Recommended:** `[N < const usize]` and `pattern: const string`.
   - Alternative: `@const` decorators on parameters.
   - Alternative: one Zig-like `comptime` modifier.

```text
struct Array[T, N < const usize]:  # hypothetical syntax
    pass
```

2. **Which constant expressions should the first slice accept?**
   - **Recommended:** literals, named consts, scalar operators, and calls of explicit const functions.
   - Smaller: literals, named consts, and standalone const parameters only.
   - Larger: any ordinary function whose reached path is compile-time eligible.

```text
const fn lanes(bytes: usize) -> usize:  # hypothetical syntax
    bytes / 4
type Words = Array[u32, lanes(64)]  # hypothetical syntax
```

3. **May a const function produce heap-shaped immutable values?**
   - **Recommended:** yes, after scalar const generics, as frozen strings, lists, structs, and enums without identity.
   - Smaller: scalar results only; static regex and URL constructors wait.
   - Larger: materialize arbitrary graphs, including shared references.

```text
const fn static_regex(pattern: string) -> Regex:  # hypothetical syntax
    Regex::new(pattern).expect("invalid static regex")
```

4. **How much const arithmetic belongs in type identity initially?**
   - **Recommended:** evaluate closed arguments, but allow only standalone parameters inside generic type bodies.
   - Larger: normalize expressions such as `N + 1` symbolically.
   - Smallest: accept literal arguments only.

```text
fn split[N < const usize](block: Array[u8, N]) -> void:  # hypothetical syntax
    pass
```

5. **Should const values be inferred only from static argument types?**
   - **Recommended:** yes; infer `N` from `Array[T, N]`, never from a runtime integer value.
   - Alternative: require every const argument explicitly.

```text
fn width[N < const usize](items: Array[u8, N]) -> usize:  # hypothetical syntax
    N
```

## Sources

- [Rust Reference: generic parameters and const generics](https://doc.rust-lang.org/reference/items/generics.html#const-generics)
- [Rust Reference: constant evaluation](https://doc.rust-lang.org/reference/const_eval.html)
- [Zig Language Reference: comptime](https://ziglang.org/documentation/master/#comptime)
- [Mojo Manual: parameterization](https://mojolang.org/docs/manual/parameters/)
- [Nim Manual: `static[T]`](https://nim-lang.org/docs/manual.html#statict)
- [D Language Reference: template value parameters](https://dlang.org/spec/template.html#valueparameters)
- [D Language Reference: compile-time function execution](https://dlang.org/spec/function.html#interpretation)
- [Scala 3 Reference: inline parameters and constant expressions](https://docs.scala-lang.org/scala3/reference/metaprogramming/inline.html)
- [Scala SIP-23: literal-based singleton types](https://docs.scala-lang.org/sips/42.type.html)
- [GHC User Guide: datatype promotion](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/exts/data_kinds.html)
- [GHC User Guide: type-level literals](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/exts/type_literals.html)
- [TypeScript Handbook: literal types](https://www.typescriptlang.org/docs/handbook/2/everyday-types.html#literal-types)
- [Go Language Specification: type parameter declarations](https://go.dev/ref/spec#Type_parameter_declarations)

## Parse Log

Parsing checks syntax only; it does not type-check examples.

| Block | Result |
| --- | --- |
| Current runtime-length check | Parsed cleanly |
| Typed const parameters | Rejected with `syntax-error` on the marked const generic, const function, and const parameter syntax |
| Unified comptime | Rejected with `syntax-error` on the marked comptime generic and parameter syntax |
| Promoted literals | Rejected with `syntax-error` on the marked value operation and literal type argument |
| Purpose-built operations | Rejected with `syntax-error` on the marked literal type argument |
| Question 1 | Rejected with `syntax-error` on the marked const generic syntax |
| Question 2 | Rejected with `syntax-error` on the marked const function and computed type argument syntax |
| Question 3 | Rejected with `syntax-error` on the marked const function syntax |
| Question 4 | Rejected with `syntax-error` on the marked const generic syntax |
| Question 5 | Rejected with `syntax-error` on the marked const generic syntax |
