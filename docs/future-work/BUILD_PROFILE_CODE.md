# Build-Profile-Dependent Code

Status: design exploration, 2026-10-28; nothing here is decided or in the
specification.

This record explores [issue #965](https://github.com/HerringtonDarkholme/hd-lang/issues/965).
It asks whether source may depend on the debug, test, or release build profile.
The immediate use case is the proposed development-only `require` check from
[PR #958](https://github.com/HerringtonDarkholme/hd-lang/pull/958).

It reviews [Integer Arithmetic](../../spec/lang/04-type-system.md#integer-arithmetic),
[Package Manifest](../../spec/lang/10-modules.md#package-manifest),
[Release Builds And Dependencies](../../spec/lang/10-modules.md#release-builds-and-dependencies),
[Requirement Rows](../../spec/lang/11-requirements-and-suspension.md#requirement-rows), and
[Build Profiles](../../spec/cli/command-line.md#build-profiles).

## Contents

- [The Domain](#the-domain)
- [Problem](#problem)
- [What hd Has Today](#what-hd-has-today)
- [Survey](#survey)
- [Option: Source Exclusion](#option-source-exclusion)
- [Option: A Read-Only Profile Constant](#option-a-read-only-profile-constant)
- [Option: A Development-Only Combinator](#option-a-development-only-combinator)
- [Option: Compiler-Known Functions Only](#option-compiler-known-functions-only)
- [Comparison](#comparison)
- [Recommendation](#recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Sources](#sources)
- [Parse Log](#parse-log)

## The Domain

A **build profile** changes deliberate language behavior. Hd has three: debug,
test, and release. A **pipeline** only changes how an implementation generates
code. `hd test --release` therefore uses the test profile with the optimized
pipeline; it does not acquire release-profile behavior.

There are two importantly different ways to make code profile-dependent:

1. **Source exclusion** removes an inactive region before semantic checking.
   The region may refer to missing names or contain invalid types because the
   compiler never checks it in that build.
2. **Checked specialization** checks one source program and then proves a
   profile test constant. The optimizer removes the untaken runtime branch, but
   names, types, and requirement rows remain valid in every profile.

This distinction decides whether a package still has one interface per
version. If a public declaration or inferred requirement row can disappear,
debug and release builds can expose different interfaces. If all branches are
checked and rows include every branch, generated code may differ while the
interface stays fixed.

Requirement rows make the choice visible. This development-only diagnostic
uses `Console`:

```text
fn report_cache_size(size: usize) -> void $ Console:
    if is_development_build():  # hypothetical compiler-known constant
        println("cache entries: $size")
```

A stable-interface design keeps `$ Console` in every profile, even when release
code generation deletes the branch. A profile-dependent-row design removes
`Console` in release, but then the function has two types and a caller cannot
compile against one package interface.

The motivating `require` case is narrower. The proposed call takes a lazy
condition so release code can remove both the call and condition evaluation:

```text
fn debit(balance: i64, amount: i64) -> i64:
    require(fn() -> bool: amount >= 0)  # hypothetical std function
    balance - amount
```

Erasure is safe only under the purity rule settled with `require`. An empty
requirement row and a non-suspending `fn` exclude providers and suspension, but
do not by themselves prove the absence of every local mutation, panic, or
allocation. A general profile mechanism must not silently claim to solve that
separate contract-purity question.

## Problem

The design must handle five fixed cases:

1. `require` evaluates its pure lazy condition in debug and test, and emits no
   condition or message code in release.
2. A development-only diagnostic may need `$ Console`.
3. An inactive branch may contain a name or type error.
4. A package must keep one public interface, including requirement rows, in all
   profiles.
5. `hd test --release` must behave like test, not release, because its profile
   remains test.

The decision is not only syntax. A broad mechanism can create three behavior
variants of every function, make failures reproduce only under one command,
and turn profile selection into a hidden input to library code. A narrow
mechanism may serve `require` without granting that power to every package.

## What hd Has Today

`module.manifest.no-features` says that manifests have no optional features and
source has no conditional compilation. Its reason is one build and one
interface per package version.

The compiler already knows the profile in three narrow places:

- integer overflow and invalid shifts panic in debug and test, but wrap or mask
  in release;
- `dbg` in the user's own code is accepted in debug and test, but is an error in
  release;
- the CLI keeps the test profile under `hd test --release`, changing only the
  code-generation pipeline.

None is an ordinary source query. A library cannot ask for the current profile,
and user code cannot mark a block as development-only.

Requirement rows are part of function types. A private function's omitted row
is the least row containing every requirement its body uses; a public function
writes its complete row. The existing rules do not make row inference depend
on reachability or optimization.

The accepted direction in [issue #875](https://github.com/HerringtonDarkholme/hd-lang/issues/875)
is typed const parameters followed by explicit `const fn`, then frozen values.
That work is not scheduled. A profile constant could eventually reuse its
constant-evaluation vocabulary, but does not need to make profiles type
parameters or expose general compile-time evaluation now.

## Survey

| Language or tool | Profile mechanism | Is inactive code checked? | Lesson for hd |
| --- | --- | --- | --- |
| [Rust](https://doc.rust-lang.org/reference/conditional-compilation.html) | `#[cfg]` removes attributed source; `cfg!` produces a boolean; `debug_assert!` follows `debug_assertions`. | `#[cfg]` source is removed. Every branch around `cfg!` must be valid. `debug_assert!` expansion is always type-checked. | Rust exposes both dangerous source exclusion and a checked, erasable assertion. Hd can take the second without taking the first. |
| [Go](https://go.dev/cmd/go/#hdr-Build_constraints) | A `//go:build` expression selects whole files from target and user tags. There is no special debug profile tag. | An excluded file is not part of that package build, so that build does not type-check it. | File selection supports ports and generated variants, but directly permits different package declarations. It conflicts with hd's one-interface goal. |
| [Swift](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/statements/#Conditional-Compilation-Block) | `#if DEBUG` uses a named active compilation condition; `assert` is a separate development check. | Inactive `#if` statements are parsed but are not part of the compiled program; Swift does not promise semantic checking for them. | Parsing both sides catches malformed structure, not stale names or types. Named flags also grow beyond profiles. |
| [C](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n3220.pdf) | With `NDEBUG`, `<assert.h>` defines `assert(...)` as `((void)0)`. General `#if` is also available. | The preprocessor removes the assertion argument before C semantic checking. | This gives zero release cost, but permits stale assertions and side effects that vanish. `require` needs a stronger purity and checking story. |
| [Zig](https://ziglang.org/documentation/master/#Compile-Variables) | The compiler-provided `builtin.optimize` value is an ordinary compile-time constant used in `if`. | Zig's reference says a compile-time-known `if` skips analysis of the untaken branch. | A profile constant does not automatically mean both sides are checked. Hd would need to state a stricter rule deliberately. |
| [Kotlin](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin/assert.html) | There is no build-profile source exclusion. `assert` is an inline library API; JVM assertions are enabled at runtime and Native assertions at compilation. | The ordinary call and boolean argument are checked. The lazy message runs only on failure. | A library call keeps source valid but cannot generally avoid evaluating an eager condition, which is why `require` needs a lazy condition. |
| [Java](https://docs.oracle.com/javase/specs/jls/se9/html/jls-14.html#jls-14.21) | Assertions are enabled or disabled at runtime. A `static final boolean` may guard an ordinary `if`, and the compiler may omit its false body. | A constant-false `if` body is still a valid, checked statement; it is specially considered reachable. | This is the closest precedent for checked specialization, though Java warns that separately compiled clients can retain an old inlined flag. Hd's profile is compiler-owned, not package state. |

Three patterns emerge.

1. Rust uniquely offers all three useful comparison points: source removal,
   checked boolean specialization, and a checked debug assertion.
2. Go, Swift, C, and Zig allow inactive source to escape semantic checking.
   This makes platform adaptation expressive but lets variants drift.
3. Java demonstrates that an ordinary constant branch can remain checked and
   then disappear. Kotlin demonstrates the limit of an ordinary eager function
   argument.

The Zig detail corrects the premise in issue #965: current Zig does **not** keep
the untaken branch semantically analyzed when the `if` condition is known at
compile time. Hd may still choose that behavior; it would be an hd safeguard,
not behavior inherited from Zig.

## Option: Source Exclusion

Add a conditional-compilation directive or decorator whose false branch is
removed before name and type checking:

```text
#if profile != .Release:  # hypothetical syntax
fn report_cache_size(size: usize) -> void $ Console:
    println("cache entries: $size")
#endif  # hypothetical syntax
```

This is the Rust `#[cfg]`, Go build-tag, and Swift `#if` model. It can omit a
statement, declaration, module, or import. An inactive body can use an API that
does not exist in the active build.

That strength is the problem. Allowing declaration-level exclusion means a
package version has several interfaces. Restricting exclusion to function
bodies preserves declaration names, but inferred private rows can still differ
unless row checking examines removed code. Examining it would no longer be
source exclusion.

`require` can be compiled away, but so can arbitrary I/O, mutation, and return
paths. The mechanism provides no purity guarantee. It also creates pressure for
target, dependency, and feature predicates after the first profile predicate,
reversing `module.manifest.no-features`.

Traits, implementations, derivation, and generic bodies can all drift when a
declaration disappears. Separate checking of every profile catches only the
configured variants and multiplies package, editor, and CI work.

Soundness is conventional if each selected program is checked independently.
The risks are stale variants, interface multiplicity, and behavior hidden from
the source seen by one build.

Design-cost kinds: one syntax mechanism, kind 1; semantic source exclusion,
kind 2; compiler profile predicates, kind 3.

## Option: A Read-Only Profile Constant

Expose a compiler-known constant and use ordinary `if`. Hd deliberately checks
every branch before profile-aware code generation removes one:

```text
fn report_cache_size(size: usize) -> void $ Console:
    if build_profile() != .Release:  # hypothetical compiler-known constant
        println("cache entries: $size")
```

`BuildProfile` is a closed enum with `.Debug`, `.Test`, and `.Release`. User code
cannot define flags or alter its value. `build_profile()` is constant during
one compilation, but its `if` follows hd's ordinary name, type, and requirement
checking rules in every branch. This differs intentionally from Zig.

The release optimizer removes the `println` branch. The function still has
`$ Console` in every profile, because requirement rows summarize all checked
source, not emitted instructions. This keeps one interface. A release entry
point must still make the provider available even if whole-program generation
later proves that no call reaches it.

`require` can be implemented as an ordinary std function only after explicit
`const fn` can expose the profile within std and the compiler can erase its lazy
condition closure. Before then, `build_profile()` is itself a narrow intrinsic.
Either form still needs the separate purity rule for the condition.

This option allows `else`, so packages may implement three different ordinary
algorithms, not only add checks. All variants remain type-correct, but tests
must still exercise their behavior. Test-profile semantics remain distinct
under `hd test --release`.

Traits and generic signatures do not vary. Derivation sees one declaration
graph. RC sees only the selected branch after optimization. Const work in #875
could later provide the vocabulary, but profile values must not become generic
arguments or public type identity.

Soundness requires the compiler to prevent profile reads from becoming runtime
state and to check every branch before folding. Cross-package compilation must
specialize bodies with the consumer's selected profile without changing their
published signatures.

Design-cost kinds: one compiler-known constant or intrinsic, kind 3; one
semantic rule that its ordinary branches are checked before folding, kind 2.

## Option: A Development-Only Combinator

Add one compiler-backed std combinator. It accepts a callback in debug and test
but erases the whole callback in release:

```text
fn report_cache_size(size: usize) -> void $ Console:
    dev_only(fn() -> void $ Console: println("cache entries: $size"))
```

Conceptually its polymorphic signature preserves the callback row:

```text
fn dev_only[R](body: fn() -> void $ R) -> void $ R:  # hypothetical intrinsic body
    body()
```

The callback is always parsed, resolved, and type-checked. Its row flows through
`dev_only`, so the enclosing function needs `$ Console` in every profile. The
release compiler removes callback construction and the call only after checking.

The return type is `void` and there is no `else`, which prevents profile choice
from selecting a value or a whole replacement algorithm. It nevertheless
permits arbitrary development-only mutation, I/O, allocation, and panics. A
programmer reading the call sees the erasure boundary, but widespread calls can
still create profile-dependent behavior and reproduce-only-in-debug failures.

`require` could be implemented using `dev_only`, but a general effectful callback
does not prove that a contract predicate is safe to erase. `require` still needs
its own empty-row, non-suspending, and purity constraints.

Traits and derivation see one declaration graph. A row parameter preserves
requirements without profile-specific function types. Local `&T` and `&mut T`
cannot escape into the callback under current capture rules, so callers use
ordinary captured values or explicitly counted references when needed.

Soundness is straightforward because the callback result is unused. The main
risk is semantic: local state changes inside it must be understood to disappear
in release.

Design-cost kinds: one compiler intrinsic, kind 3, behind one core-library API,
kind 4. Erasing its callback is a semantic exception, kind 2.

## Option: Compiler-Known Functions Only

Add no user-level profile query, block, decorator, or combinator. The compiler
defines release behavior separately for a short closed list of functions such
as `require`, as it already does for overflow and `dbg`:

```text
fn debit(balance: i64, amount: i64) -> i64:
    require(fn() -> bool: amount >= 0)  # hypothetical compiler-known std function
    balance - amount
```

Every call, condition, and lazy message is name-resolved and type-checked in
all profiles. Debug and test evaluate the check. Release erases the call and
closures after the compiler verifies the contract-specific purity rule. The
release artifact can therefore be byte-identical to one whose source omitted
the `require` call.

`require` callbacks have empty requirement rows, so a check cannot hide
`$ Console`. The profile mechanism introduces no row question for users. A
future compiler-known function with a nonempty row would keep that row in every
profile unless a separate design explicitly changed the one-interface rule.

The compiler contract can be attached to the std function's lang-item identity,
not to arbitrary user names. Users cannot define their own development-only
logging, choose another release algorithm, or inspect the profile.

Traits, generics, derivation, RC, and package interfaces remain unchanged. The
cost is cumulative: every new dev-only API needs an explicit compiler decision.
That friction is useful while the only demonstrated case is `require`.

Soundness is local to each compiler-known function. For `require`, the hard part
is the purity and capture rule already under review in PR #958, not a new general
conditional-compilation system.

This is the radical simplification.

Design-cost kinds: one compiler intrinsic or lang-item behavior per accepted
function, kind 3; the ordinary std surface remains a core-library API, kind 4.

## Comparison

| Criterion | Source exclusion | Profile constant | `dev_only` combinator | Compiler-known functions |
| --- | --- | --- | --- | --- |
| `require` disappears in release | Yes | Yes, with intrinsic or future const support | Yes, but purity remains separate | Yes, directly |
| Development logging | Arbitrary | Arbitrary | Arbitrary callback | Not available |
| Inactive names and types checked | No | Yes | Yes | Yes |
| Requirement row for a dev-only `$ Console` use | May differ by profile | `$ Console` in all profiles | `$ Console` in all profiles | User cannot write the case |
| One public interface per package version | No without severe placement limits | Yes | Yes | Yes |
| Can select different release algorithm | Yes | Yes | No result or `else`, but may mutate | No |
| Test under `hd test --release` | Selected as test source | Constant is `.Test` | Callback runs | Check runs |
| Rules added | Directives, predicates, placement, exclusion | Closed enum value and checked folding | Row-polymorphic intrinsic and erasure | One rule per named function |
| Soundness boundary | Each selected program separately | All branches plus constant folding | Checked callback; effects intentionally vanish | Per-function purity contract |
| Agent writability | Familiar but easy to leave variants stale | Ordinary `if`; profile use can spread | Erasure boundary is explicit | Simplest; no general mechanism to misuse |
| Human readability | Hidden source can change interface | Branch is local and explicit | Intent is explicit at the call | Only named APIs have special behavior |
| Implementation cost | Highest surface and tooling multiplication | Moderate compiler and cross-package work | Moderate generic-row and lowering work | Lowest for the first function |
| Evolution risk | Expands toward target flags and features | Expands toward profile-driven algorithms | Expands toward effectful debug blocks | Revisit only after another concrete use |
| Design-cost order | Syntax (1), semantics (2), intrinsic (3) | Semantics (2), intrinsic (3) | Semantics (2), intrinsic (3), std (4) | Intrinsic (3), std (4) |

By the repository's design-cost order, all general options need a semantic
exception or syntax. Compiler-known functions keep the costliest mechanism at
an intrinsic and add no general syntax. The combinator is the next narrowest,
but it deliberately admits arbitrary disappearing effects.

## Recommendation

Choose **compiler-known functions only** for the demonstrated `require` case,
and add no user-level profile mechanism now.

This preserves semantic checking, requirement rows, and one interface in every
profile. It also keeps the open purity question exactly where it belongs: on
the function whose arguments disappear. The existing compiler already treats
overflow and `dbg` by name and profile, so this extends a narrow pattern instead
of creating conditional-compilation grammar.

This gives up user-defined development logging and profile-selected algorithms.
No issue has supplied a concrete need for them, and `$ Console` shows that the
authority does not disappear merely because optimized code does.

The next-best option is the **read-only profile constant** if a second concrete
use cannot fit a compiler-known function. It should be a closed compiler value,
not a user flag; both branches must be checked; all branch requirements must
remain in the row; and it must never change a declaration or public type. This
keeps the useful Java/Rust `cfg!` property without taking Rust `#[cfg]` sprawl.

Do not adopt source exclusion. It contradicts the reason hd has no features and
would make every package, editor, test plan, and published interface reason
about multiple source programs.

## Questions For The Owner

1. **Should the first `require` implementation add no user-level profile mechanism?**
   - **Recommended:** yes; make only `require` compiler-known and close this
     exploration until a second concrete use appears.
   - Alternative: add the general `dev_only` combinator with `require`.
   - Alternative: add the read-only profile constant immediately.

```text
fn debit(balance: i64, amount: i64) -> i64:
    require(fn() -> bool: amount >= 0)  # hypothetical std function
    balance - amount
```

2. **If a user-level mechanism is later needed, should every inactive branch
   remain name-resolved and type-checked?**
   - **Recommended:** yes; removal happens only after semantic checking.
   - Alternative: source exclusion, accepting stale inactive code.

```text
fn report_cache_size(size: usize) -> void $ Console:
    if build_profile() != .Release:  # hypothetical compiler-known constant
        println("cache entries: $size")
```

3. **Should requirements used only by development code stay in every profile's
   function row?**
   - **Recommended:** yes; `$ Console` remains part of the one public interface.
   - Alternative: infer a row per profile, abandoning one interface per version.

```text
fn report_cache_size(size: usize) -> void $ Console:
    dev_only(fn() -> void $ Console: println("cache entries: $size"))
```

4. **Should a future profile value expose all three profiles or only the safe
   predicate needed for erasable checks?**
   - **Recommended:** expose only `is_development_build()` at first, so code
     cannot distinguish debug from test or attach a release `else` algorithm.
   - Alternative: expose the closed `BuildProfile` enum.

```text
fn checked_only() -> void:
    if is_development_build():  # hypothetical compiler-known constant
        require(fn() -> bool: expensive_invariant())
```

## Sources

- [Rust Reference: conditional compilation](https://doc.rust-lang.org/reference/conditional-compilation.html)
- [Rust by Example: `cfg` compared with `cfg!`](https://doc.rust-lang.org/stable/rust-by-example/attribute/cfg.html)
- [Rust standard library: `debug_assert!`](https://doc.rust-lang.org/std/macro.debug_assert.html)
- [Go command: build constraints](https://go.dev/cmd/go/#hdr-Build_constraints)
- [Swift Reference: conditional compilation blocks](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/statements/#Conditional-Compilation-Block)
- [Swift PackageDescription: define a compilation condition](https://developer.apple.com/documentation/packagedescription/swiftsetting/define%28_%3A_%3A%29)
- [C working draft N3220: `<assert.h>` and `NDEBUG`](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n3220.pdf)
- [Zig Language Reference: compile variables and `comptime`](https://ziglang.org/documentation/master/#Compile-Variables)
- [Kotlin standard library: `assert`](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin/assert.html)
- [Java Language Specification: reachability and conditional compilation](https://docs.oracle.com/javase/specs/jls/se9/html/jls-14.html#jls-14.21)
- [Java assertions guide](https://docs.oracle.com/javase/8/docs/technotes/guides/language/assert.html)

## Parse Log

Parsing checks syntax only; it does not type-check hypothetical names or APIs.

| Block | Result |
| --- | --- |
| Domain: development diagnostic | Parsed cleanly; the hypothetical name is not type-checked |
| Domain: lazy `require` | Parsed cleanly; the hypothetical name is not type-checked |
| Source exclusion | Parsed cleanly; current hd treats the marked `#if` and `#endif` lines as comments, not directives |
| Profile constant | Parsed cleanly; the hypothetical name is not type-checked |
| Development-only combinator call | Parsed cleanly; the hypothetical name is not type-checked |
| Development-only combinator signature | Parsed cleanly; the intrinsic behavior is not type-checked |
| Compiler-known functions | Parsed cleanly; the hypothetical name is not type-checked |
| Question 1 | Parsed cleanly; the hypothetical name is not type-checked |
| Question 2 | Parsed cleanly; the hypothetical name is not type-checked |
| Question 3 | Parsed cleanly; the hypothetical name is not type-checked |
| Question 4 | Parsed cleanly; the hypothetical names are not type-checked |
