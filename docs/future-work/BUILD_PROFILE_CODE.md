# Build-Profile-Dependent Code

Status: direction accepted by the owner, 2026-10-28; domain: language feature.
Building waits for the owner's go-ahead.

This record applies the decision in
[issue #965](https://github.com/HerringtonDarkholme/hd-lang/issues/965).
It reviews [Package Manifest](../../spec/lang/10-modules.md#package-manifest),
[Requirement Rows](../../spec/lang/11-requirements-and-suspension.md#requirement-rows),
and [Build Profiles](../../spec/cli/command-line.md#build-profiles).

## The Direction

`@cfg(.Debug)` and `@cfg(.Release)` select between two declarations with one
name. `.Debug` means debug and test profiles. `.Release` means release.

```text
@cfg(.Debug)
fn require(condition: fn() -> bool) -> void:
    if !condition():
        panic("requirement failed")

@cfg(.Release)
fn require(condition: fn() -> bool) -> void:
    pass
```

Both declarations are present for semantic checking. The active profile picks
one body for code generation. The optimizer then removes dead calls and values.

There is no profile predicate, profile enum, or user-defined build flag.
Packages keep one interface per version.

## Rules

1. `.Debug` covers debug and test builds, including `hd test --release`, which changes the pipeline, not the profile. `.Release` covers release builds.
2. The variants of one name have identical signatures, including generics and requirement rows.
3. A configured name has exactly one `.Debug` and one `.Release` declaration. A missing or duplicate variant is an error at the declaration.
4. A name without `@cfg` has one ordinary declaration.
5. Both bodies are name-resolved and type-checked in every profile.
6. No expression can inspect the active profile: there is no predicate and no `BuildProfile` value.
7. Packages define no extra flags, so `module.manifest.no-features` stays true.
8. `require` needs no special capture rule. Its closure type already forbids requirements and suspension, and captured mutation is accepted.
9. A release WAT stage test proves the zero cost: a call to an empty `.Release` variant, with a capturing closure argument, emits no code.

## Problems It Solves

### Ordinary development checks

`require` stays ordinary std code. Debug and test evaluate the condition;
release selects the empty declaration.

```text
fn debit(balance: i64, amount: i64) -> i64:
    require(fn() -> bool: amount >= 0)
    balance - amount
```

The release optimizer removes the call and closure. Both bodies and the call
remain semantically checked in every profile.

### One interface per package version

Variants cannot change a requirement row or generic parameter list.
Development diagnostics therefore cannot silently add an authority only in
one profile.

```text
@cfg(.Debug)
fn report_cache_size(size: usize) -> void $ Console:
    println("cache entries: $size")

@cfg(.Release)
fn report_cache_size(size: usize) -> void $ Console:
    pass
```

The release body does not use `Console`, but the shared signature still names
it. Callers see the same function type in every build.

### Checked inactive code

A stale release implementation fails during an ordinary debug check. Teams do
not need a separate profile matrix merely to find missing names or bad types.

```text
@cfg(.Debug)
fn load_limit() -> usize:
    100

@cfg(.Release)
fn load_limit() -> usize:
    configured_limit()
```

`configured_limit` must resolve and return `usize` in every profile.

## How It Works

The language tier gains one compiler-known prefix decorator, `cfg`. It accepts
only `.Debug` or `.Release` on a declaration with a body.

For each declared name, the compiler applies these rules:

1. No `@cfg` means one ordinary declaration.
2. Any `@cfg` means exactly two declarations, one for each accepted argument.
3. The two signatures are identical after resolution, including generic
   parameters and requirement rows.
4. Both bodies are resolved and checked in every profile.
5. Debug and test select `.Debug`; release selects `.Release`.
6. Selection changes the body used by later compilation stages, not the
   declaration's type.
7. Ordinary optimization removes the selected body's dead calls and values.

The duplicate and missing-pair checks happen at declarations. They do not wait
for a call site, so unused configured functions are checked too.

The compiler does not expose the selected profile as a value. It also does not
infer a profile-specific function type, requirement row, trait implementation,
or derived shape.

The tier-2 object model in
[#163](https://github.com/HerringtonDarkholme/hd-lang/issues/163) may later give
closures read-only captures. If it does, `require` can be revisited there;
`@cfg` adds no capture exception now.

## Cost

| Component | Direction |
| --- | --- |
| Grammar | No new grammar; reuse prefix-decorator syntax |
| Resolver | Pair declarations by name and compare their complete signatures |
| Checker | Check both bodies; report missing and duplicate variants at declarations |
| Lowering | Select one checked body from the closed profile |
| Optimizer | Existing dead-code removal must erase an empty release call and closure |
| Public interfaces | Unchanged across profiles |
| Requirement rows | Identical across variants |
| User configuration | None; no feature flags or profile values |
| Spec | Language-tier decorator, pairing, checking, and selection rules |
| Conformance | Valid pair; missing pair; duplicate; mismatched signatures; both bodies checked |
| Stage tests | Release WAT contains neither the empty call nor its captured closure |
| Wasm size | The release gate requires zero code for the motivating empty variant |

The costliest change is a semantic rule for declaration variants, design-cost
kind 2. Compiler recognition is kind 3. Existing decorator grammar avoids a
new syntax mechanism.

## Appendix A: Stress Test

### Requirements remain stable

The `report_cache_size` example keeps `$ Console` in release. This costs an
authority at the interface even when no emitted instruction uses it, but it
preserves one interface.

Allowing the row to differ would make callers and trait conformance depend on
the profile. The identical-signature rule rejects that alternative.

### Generic variants cannot drift

```text
@cfg(.Debug)
fn choose[T](value: T, fallback: T) -> T:
    value

@cfg(.Release)
fn choose[T](value: T, fallback: T) -> T:
    fallback
```

Both bodies use the same `T` and signature. A release variant that adds a bound
or changes a parameter type is rejected at the declarations.

### An inactive body can still fail

```text
@cfg(.Debug)
fn parse_limit(text: string) -> Result[usize, ParseIntError]:
    Ok(100)

@cfg(.Release)
fn parse_limit(text: string) -> Result[usize, ParseIntError]:
    text.parse()
```

The release body must resolve and type-check during a debug build. This catches
drift earlier than source-exclusion systems.

### Captured mutation is deliberate

```text
fn checked_increment(value: i64) -> i64:
    let next = value
    require(fn() -> bool:
        next += 1
        next > value
    )
    next
```

The closure type forbids requirements and suspension, not mutation. Release may
erase this mutation. The owner accepts that behavior; no purity rule is added.

### Test stays development-like

`hd test --release` selects `.Debug`, because it remains a test-profile build.
Only its compilation pipeline is release-like. This keeps development checks
active in optimized test runs.

The stress test found two costs. Stable requirement rows may retain an unused
authority, and captured mutations may disappear in release. Both are explicit
consequences of the accepted direction.

## Appendix B: Prior Art

### Rust

```rust
#[cfg(debug_assertions)]
fn check() { debug_assert!(valid()); }
```

Rust `#[cfg]` removes attributed source, while `debug_assert!` keeps a checked
assertion form. Hd rejects general source exclusion and pairs complete checked
declarations instead. See the
[Rust Reference](https://doc.rust-lang.org/reference/conditional-compilation.html).

### Go

```go
//go:build linux

package platform
func Open() error { return openLinux() }
```

Go build constraints select whole files, which may expose different package
declarations. Hd keeps one signature across both bodies. See
[Go build constraints](https://go.dev/cmd/go/#hdr-Build_constraints).

### Swift

```swift
#if DEBUG
checkInvariant()
#endif
```

Swift active compilation conditions can omit source. Hd's inactive declaration
is still resolved and checked. See
[Swift conditional
compilation](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/statements/#Conditional-Compilation-Block).

### C

```c
#ifndef NDEBUG
assert(valid());
#endif
```

C preprocessing removes source before semantic checking. Hd keeps the release
body checked and limits the choices to two compiler profiles. See
[C working draft N3220](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n3220.pdf).

### Zig

```zig
if (builtin.mode == .Debug) {
    std.debug.assert(valid());
}
```

Zig skips semantic analysis of an untaken compile-time branch. Hd deliberately
checks both declarations. See the
[Zig language reference](https://ziglang.org/documentation/master/#Compile-Variables).

### Kotlin And Java

```kotlin
assert(valid())
```

```java
assert valid();
```

Neither language offers source-profile declaration variants. Their assertions
show a narrow alternative, but hd needs ordinary std `require` code whose body
differs by profile. See
[Kotlin `assert`](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin/assert.html)
and the
[Java assertions guide](https://docs.oracle.com/javase/8/docs/technotes/guides/language/assert.html).

## Appendix C: Alternatives Considered

**A profile predicate or enum.** An ordinary `if` would let any expression
select different algorithms. It would also preserve every branch requirement.
The owner rejected both forms; profiles are not values.

**Source exclusion.** Rust `#[cfg]`, Go tags, and Swift `#if` permit unchecked
inactive source and different package interfaces. This conflicts with one
interface per package version.

**A `dev_only` combinator.** A callback makes the erasure boundary visible but
allows arbitrary disappearing effects. Paired declarations cover `require`
without adding another std mechanism.

**Compiler-known functions only.** This was the exploration's original
recommendation. The owner instead chose one general declaration mechanism,
while keeping expressions profile-independent.

## Appendix D: Today In hd

`module.manifest.no-features` says manifests have no optional features and
source has no conditional compilation. The accepted direction keeps user flags
out, but adds two compiler-selected declaration bodies.

The compiler already makes overflow, invalid shifts, and `dbg` depend on the
profile. Libraries cannot express that behavior in ordinary declarations.

Requirement rows are part of function types. Private inference and public
annotations do not depend on optimization or reachability.

## Appendix E: Implementation Checklist

- Grammar and formatting: reuse prefix decorators; format one line above each declaration.
- Typing: compare complete signatures, then check both bodies.
- Patterns and derives: no change; both see one selected declaration type.
- Closures: no new capture rule; release optimization must erase empty calls.
- Modules: configured pairs keep one exported name and signature.
- Ownership: both bodies obey ordinary ownership checks.
- Inlining: body selection precedes ordinary optimization.
- Diagnostics: missing, duplicate, and mismatched pairs point at declarations.
- Spec: language-tier rules; no stdlib or CLI profile value.
- Conformance: valid and invalid declaration-pair fixtures.
- Stage test: release WAT proves the empty variant and capture are absent.
- Rollback: remove the decorator rules and keep `require` out of std.

## Parse Log

Parsing checks syntax only. The accepted decorator is not implemented yet, so
the parser checks the declaration and body shapes, not `cfg` semantics.

| Block | Expected result |
| --- | --- |
| `require` declarations | Parse; decorator semantics wait for implementation |
| `debit` | Parse |
| `report_cache_size` | Parse; names are not type-checked by this check |
| stale release body | Parse; the checker must later reject bad names or types |
| generic variants | Parse |
| inactive body | Parse; type checking is a later implementation gate |
| captured mutation | Parse; capture semantics are checked later |
