# Code Generation With Compile-Time Reflection

Status: design exploration, 2026-10-01; nothing here is decided or in the
specification. It changes no spec text, prototype code, or library code.

The owner asked: "we have a research stuff about compile time codegen and
compile time reflection proposal. please queue an agent to do the research
and design. to see if it can simplify the language". The owner's earlier
framing, from batch 36: "something between golang and macro".

Under review: the deferred direction in
[One Compile-Time Intrinsic](archive/COMPTIME_UNIFICATION.md#deferred-code-generation-plus-compile-time-reflection),
its [O1 and O2 rejections](archive/COMPTIME_UNIFICATION.md#owner-decisions),
and the [idea noted for later](OPEN_ISSUES.md#ideas-noted-for-later). The
spec areas it would touch are
[Typed Derivation](../spec/14-annotations.md#typed-derivation),
[Member-Typed Facts](../spec/14-annotations.md#member-typed-facts),
[Error Derivation](../spec/14-annotations.md#error-derivation),
[Trait Delegation](../spec/09-traits.md#trait-delegation),
[Derived Implementations](../spec/09-traits.md#derived-implementations),
[Typing `all!`](../spec/11-requirements-and-suspension.md#typing-all),
[Literal Suffixes](../spec/05-expressions.md#literal-suffixes), and
[Derived Arbitrary](../spec/std/testing.md#derived-arbitrary).

## Contents

- [Summary](#summary)
- [Problem And What hd Has Today](#problem-and-what-hd-has-today)
- [Survey](#survey)
- [The Design: Typed Generators](#the-design-typed-generators)
- [What Generators Could Replace](#what-generators-could-replace)
- [Before And After](#before-and-after)
- [Compile-Time Cost](#compile-time-cost)
- [Error Messages For A Weak Model](#error-messages-for-a-weak-model)
- [Fit With hd's Decisions](#fit-with-hds-decisions)
- [Scopes Compared](#scopes-compared)
- [Risks](#risks)
- [Recommendation](#recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Sources](#sources)
- [Parse Log](#parse-log)

## Summary

| Finding | Detail |
| --- | --- |
| Generators beside templates do not simplify | Adding generators while `Structure` templates stay adds about 35 language rules and a second derivation mechanism. |
| Generators instead of templates do simplify | Replacing templates, walkers, and `@error` removes about 171 language rules and adds about 35: a net of about **-136** language rules, **-83** over both tiers. |
| No new syntax | Generators attach with `@derive`, read facts, and are named by a decorator on the trait. The added rules are intrinsic and semantic kinds. |
| Four features stay | `by`, `all!`, literal markers, and typed facts are not worth replacing. Each needs a call site, a trait name as a value, or an attach-time check that generators cannot give. |
| Cost | Hand-written code of the kind a generator emits checks about 9 times faster, and is about 9 times smaller, than the prototype's template path. Generator run time is not measured. |
| Main risk | Error quality depends on generators tagging each emitted line with the member it came from. An untagged error reads like a C++ template error. |

**Recommendation** (labeled, see [Recommendation](#recommendation)): take
generators as the direction only if they replace templates, walkers, and
`@error`. Gate it on a stress test of the std derives and a Haiku probe of
mocked diagnostics. Keep `by`, `all!`, literal markers, and typed facts.

## Problem And What hd Has Today

hd writes code for the user in several places. Typed derivation does it
through `Structure` templates, walkers, describers, and sources. `@error`
is its own intrinsic. `by` writes forwarding methods. `all!` has typing
rules instead of a signature.

Rule counts below are numbered `r[...]` items from `pnpm run spec counts
--by topic`, on 2026-10-01 after spec pass 41. The language tier has 3,634
rules and the stdlib tier 208.

| Area | Rule topics | Rules | Tier | Design Cost Order kind |
| --- | --- | ---: | --- | --- |
| `Structure`, its module, `name()` | `annot.structure` | 22 | language | 3 intrinsic |
| Templates and tuple templates | `annot.template` | 19 | language | 2 semantic |
| Generated `walk`, `describe`, `build` | `annot.walk`, `.describe`, `.build` | 18 | language | 3 intrinsic |
| Handles, walkers, describers, sources | `annot.handle`, `.walker` | 24 | language | 2 semantic, 3 intrinsic |
| Derived bounds, limits | `annot.bound`, `.limit` | 7 | language | 2 semantic |
| Tuple `Structure` | `annot.tuple` | 8 | language | 3 intrinsic |
| Self references | `annot.self-ref` | 13 | language | 2 semantic |
| Members and variants | `annot.member`, `.variant` | 8 | language | 3 intrinsic |
| Opt-in, blocks, member lines, omit, trait-less blocks | `annot.derive`, `.block`, `.line`, `.omit`, `.traitless` | 60 | language | 2 semantic |
| Facts | `annot.fact` | 19 | language | 2 semantic |
| Typed facts | `annot.typed-fact` | 15 | language | 2 semantic, 3 intrinsic |
| Decorators, metadata, target kinds | `annot.decorator`, `.metadata`, `.target` | 35 | language | 2 semantic |
| `@error` | `annot.error` | 54 | language | 3 intrinsic |
| Comparison derives, law partners, newtypes | `trait.derive` | 30 | language | 2 semantic |
| `by` delegation | `trait.by` | 13 | language | 1 syntax, 2 semantic |
| `all!` typing | `req.combinator.all-*` | 4 | language | 3 intrinsic |
| Literal functions | `expr.literal-fn`, `.suffix`, `.prefix` | 12 | language | 3 intrinsic |
| Derive meanings in std | `std-cmp.derive`, `std-hash.derive`, `std-format.debug`, `std-testing.arbitrary` | 62 | stdlib | 4 core library |

The prototype already lowers templates and `@error` by writing hd source
and checking it again. Its `Source_` class in
[`src/checker/generated-source.ts`](../src/checker/generated-source.ts)
keeps a span per generated line and placeholders for user expressions.
The archived measurement showed that this path costs about 18 times the
check time of an intrinsic derive
([Compiler Performance](archive/COMPTIME_UNIFICATION.md#a-measurement-of-hds-prototype)).

Three facts matter below:

1. **hd already needs a compile-time evaluator.** A fact expression is
   evaluated once at compile time and may call any requirement-free
   function ([`annot.fact.eval`](../spec/14-annotations.md#r-annot.fact.eval)).
   Package interfaces record fact values
   ([`module.interface.fact-values`](../spec/10-modules.md#r-module.interface.fact-values)).
2. **The walker protocol exists because closures are monomorphic.** A
   per-member callback must be a trait with a generic method,
   `member[F]`, with its obligation and `generic-member-call` rules.
3. **Template bodies are already checked per opt-in**
   ([`annot.template.checked`](../spec/14-annotations.md#r-annot.template.checked)),
   and package interfaces carry them
   ([`annot.limit.interfaces`](../spec/14-annotations.md#r-annot.limit.interfaces)).

## Survey

| Tool | Runs | Input | Output | Sandbox | Errors and IDE |
| --- | --- | --- | --- | --- | --- |
| Go `go generate` | an external program, run by hand, "never run automatically by go build" ([cmd/go][go-cmd]) | anything; often `go/types` | checked-in `.go` files marked by `^// Code generated .* DO NOT EDIT\.$` ([cmd/go][go-cmd]) | none | `//line` directives make the compiler "report positions in the original input to the generator" ([cmd/compile][go-line]) |
| Rust proc macros | inside the compiler, from a separate `proc-macro` crate that its own crate cannot use ([reference][rust-proc]) | token streams, no types | tokens, checked after expansion | none: they "have the same resources that the compiler has" ([reference][rust-proc]) | `compile_error!` or a panic; IDEs run an expansion server ([rust-analyzer][ra-macros]) |
| Swift macros | a separate plugin process ([SE-0382][swift-expr], [SE-0389][swift-attached]) | syntax only: type-check results are "not provided to the macro" ([SE-0382][swift-expr]) | syntax, "type-checked using the original macro result type" ([SE-0382][swift-expr]) | "a sandbox like SwiftPM plugins, preventing file system and network access" ([SE-0382][swift-expr]) | building SwiftSyntax took about 20 s in debug and 4 min in release, before prebuilts ([forum][swift-macro-time], [prebuilts][swift-prebuilts]) |
| Scala 3 `inline` and quotes | inside the compiler | typed `Expr[T]` and `quotes.reflect` | typed trees: "If a quote is well typed, then the generated code is well typed" ([macros][scala-macros]) | none stated | inlining stops at 32 nested inlines by default ([options][scala-options]) |
| Zig `comptime` | the compiler's interpreter ([docs][zig-comptime]) | types as values, `@typeInfo` | unrolled code; `@Type` cannot create declarations ([0.15 notes][zig-015]) | the compiler's interpreter | a 1,000 backward-branch default quota ([quota][zig-quota]); ZLS cannot resolve complex comptime ([ZLS][zls]) |
| C# incremental generators | inside the compiler | the semantic model plus extra files ([overview][cs-overview]) | added source: "Explicitly additive only ... may not modify existing user code" ([design][cs-design]) | determinism expected | cached per pipeline step; attribute-driven lookup is "usually 99x more efficient" ([incremental][cs-incremental]) |
| Kotlin KSP | a compiler plugin | declarations, types, annotations; processors "can't examine expressions or statements, and they can't modify the source code" ([KSP][ksp]) | new files | none stated | isolating outputs "depend only on their specified sources" ([incremental][ksp-incremental]) |
| OCaml ppx | before type checking | the untyped Parsetree: "takes a Parsetree and returns a possibly modified Parsetree" ([ocaml.org][ppx]) | Parsetree | none | derivers attach with `[@@deriving name]` ([ocaml.org][ppx]) |

Takeaways:

1. **Typed declaration data without a syntax tree exists.** C# and KSP
   give generators a typed model of declarations and annotations, and
   take new source back. That is the "between Go and macros" point.
2. **The middle-ground tools are additive only.** C# and KSP cannot
   modify user code, and KSP cannot read function bodies. Those limits
   make caching and IDE support tractable.
3. **Caching needs a narrow, comparable input.** C# compares each
   pipeline step's output; KSP tracks which sources each output depends
   on. hd's equivalent is the declaration plus its fact values, which
   the package interface already records.
4. **Error positions need explicit mapping.** Go uses `//line`
   directives; Swift checks the expansion against a contextual type.
   Without either, errors land in generated code.
5. **Separate macro builds dominate compile cost.** Rust and Swift pay
   to build the macro itself. An in-compiler interpreter, as in Zig, pays
   per evaluation instead.

## The Design: Typed Generators

A **generator** is an ordinary hd function. It receives a read-only
description of one declaration and writes hd source. The compiler runs it
during the build and checks its output like hand-written code.

Every name in `std.gen` below is hypothetical. The blocks parse because
they use existing syntax only; their meaning is the proposal.

### The Reflection Data

The compiler supplies these values. They hold declaration data and fact
values only, never type-checking results:

```text
pub data Span: pass

pub data TypeRef:
    pub text: string
    pub head: string
    pub args: List[TypeRef]
    pub is_param: bool

pub data FactInfo:
    pub value: Any
    pub ty: TypeRef
    pub text: string
    pub span: Span

pub data FieldInfo:
    pub name: string
    pub ty: TypeRef
    pub facts: List[FactInfo]
    pub embedded: bool
    pub positional: bool
    pub has_default: bool
    pub doc: string?
    pub span: Span

pub data VariantInfo:
    pub name: string
    pub index: i32
    pub fields: List[FieldInfo]
    pub facts: List[FactInfo]
    pub span: Span

pub enum DeclKind:
    Data(fields: List[FieldInfo])
    Enum(variants: List[VariantInfo])
    Newtype(base: TypeRef)
    Tuple(arity: i32, rest: TypeRef?)

pub data Decl:
    pub name: string
    pub params: List[string]
    pub kind: DeclKind
    pub facts: List[FactInfo]
    pub derives: List[string]
    pub span: Span
```

| Part | Holds | Why |
| --- | --- | --- |
| `TypeRef.text` | the type as written, resolvable in the target's module | output can name the type without a syntax tree |
| `TypeRef.head`, `args`, `is_param` | the type's structure | a library can compute derived bounds and `self_ref` in plain hd |
| `FactInfo.value` | the fact's compile-time value | the generator reads settings such as a rename |
| `FactInfo.text` | the decorator's argument expression as written | output can re-evaluate a fact that holds a function, such as `arbitrary.with(gen)` |
| `Span` | an opaque position: the opt-in, a member, a variant, or a fact | error mapping |
| `derives` | the trait names in the target's `@derive` list | a generator can check law partners |

A derivation block's member lines edit `facts` for that block, and an
omitted member `f = pass` is absent from `fields`. So member lines and
trait-less blocks keep their meaning unchanged.

### Writing A Generator

A generator writes lines into a `Code` value, and tags a line with the
span it came from:

```text
pub data GenError:
    pub message: string
    pub at: Span
    pub hint: string?

pub data Code: pass

impl Code:
    pub fn line(mut self, text: string) -> void: pass
    pub fn line_at(mut self, at: Span, text: string, hint: string? = .None) -> void: pass
    pub fn uses(mut self, path: string) -> void: pass
    pub fn fresh(mut self, base: string) -> string: pass

pub data Generator:
    pub run: fn(Decl, mut Code) -> Result[void, GenError]
    pub tuples: bool

pub fn generator(run: fn(Decl, mut Code) -> Result[void, GenError], tuples: bool = false) -> Generator:
    Generator { run: run, tuples: tuples }
```

`Debug`'s generator for data types, in `std.format`:

```text
use std.gen.{Decl, Code, GenError, generator}

fn derive_debug(d: Decl, out: mut Code) -> Result[void, GenError]:
    out.uses("std.format.DebugWriter")
    out.line("impl${bounds_for(d, "Debug")} Debug for ${self_type(d)}:")
    out.line("    fn debug(self, out: mut DebugWriter) -> void:")
    match d.kind:
        .Data(fields) =>
            out.line("        let mut s = out.debug_struct(\"${d.name}\")")
            for f in fields:
                out.line_at(f.span, "        s = s.field(\"${f.name}\", self.${f.name})", hint="add @derive(Debug) to ${f.ty.head}")
            out.line("        s.finish()")
        _ => return derive_debug_other(d, out)
    .Ok()

@generator(derive_debug)
pub trait Debug:
    fn debug(self, out: mut DebugWriter) -> void
```

The enum, newtype, and tuple cases, in `derive_debug_other`, are elided.
`bounds_for` and `self_type` are plain `std.gen` helpers over `Decl`.
`bounds_for(d, "Debug")` writes `[T < Debug]` for each parameter used in
a member, which is today's
[`annot.bound.params`](../spec/14-annotations.md#r-annot.bound.params) as
library code.

### Attaching A Generator

| Question | Answer in this design |
| --- | --- |
| How does a trait name its generator? | `@generator(f)` before the trait, in the trait's module, as a template must be ([`annot.template.module`](../spec/14-annotations.md#r-annot.template.module)). |
| How does a type opt in? | `@derive(X)`, unchanged. It runs `X`'s generator once, with that derivation's facts. |
| Configuration | facts, derivation blocks, and member lines, unchanged |
| Tuples | `@generator(f, tuples=true)` also runs `f` once per tuple size that the program uses with `X`, with `DeclKind.Tuple`. Its output is one generic impl per size. |
| Generators with no trait | Not attached in source. External-schema generators (protobuf, SQL) stay Go-style tools; see [Build Output](#build-output). |

`@derive(X)` stays the only opt-in. A generator for a trait is found the
way a template is found today, so a reader still finds it beside the
trait.

### How Generators Run

| Property | Rule | How hd gets it |
| --- | --- | --- |
| No IO | a generator's function type has the empty requirement row | `generator(run: fn(Decl, mut Code) -> ...)` has no `$`, so a function with a row does not fit |
| No suspension | no `!` in the function type, and no `block_on` | the existing rule for fact expressions, [`annot.fact.no-block-on`](../spec/14-annotations.md#r-annot.fact.no-block-on) |
| Deterministic | same `Decl`, same output | follows from the empty row: no clock, no random source, no host |
| Bounded | a step and memory budget per run; over it is `generator-budget` at the opt-in | Zig's branch quota is the model |
| Evaluator | the compile-time evaluator that facts already need | [`annot.fact.eval`](../spec/14-annotations.md#r-annot.fact.eval) |

The empty row is the sandbox. Swift needs a separate process for this;
hd's requirement rows state it in the type.

### Build Output

**Derive generators run during the build.** Their output is never
checked in. It is a virtual part of the target's module, cached on disk,
and shown by `hd expand` and the LSP. Stale output cannot occur: the
cache key is the input.

**External-schema generators stay tooling, Go-style.** A program such as
`protoc-gen-hd` writes ordinary `.hd` files that are checked in. It needs
no language rule. A first line `# generated by <tool> from <input hash>;
do not edit` lets `hd gen --check` report stale files in CI, as
`go generate` plus `git diff --exit-code` does.

| Model | Pros | Cons |
| --- | --- | --- |
| In the build, for `@derive` | always fresh; no file noise for `Eq` on every type; the user side is unchanged | needs `hd expand` and LSP support to be seen |
| Checked in, for schemas | reviewable; no build-time cost; works without the generator | stale files; one run per schema change |

Checking in derive output would put about 6 lines per member per derived
trait into source files. For agents that read files, that costs context
on every read.

### Output Rules

1. The output may declare only implementations whose target is the
   derivation's target, and private functions. It must implement `X`.
2. It is checked as if written at the end of the target's module, so it
   reads private members, as [`annot.structure.private`](../spec/14-annotations.md#r-annot.structure.private)
   allows today.
3. Its `uses` lines open a nested scope. A generated local that would
   shadow a name in a fact's `text` must come from `fresh`.
4. A method written in a derivation block replaces the generated method
   of that name, as [`annot.block.methods`](../spec/14-annotations.md#r-annot.block.methods)
   says for templates.
5. Output never contains `@derive`, so generation does not recurse.

### Caching And Incremental Builds

| Key part | Changes when |
| --- | --- |
| the generator's identity: its package version and its body's hash | the generator's package changes |
| the serialized `Decl` | the target's declaration, a fact value, or a member line changes |

A changed function body that leaves fact values equal leaves the key
equal. That is the same boundary as
[`module.interface.determined-facts`](../spec/10-modules.md#r-module.interface.determined-facts).
The output is compiled in the target's package, and its impls enter that
package's interface. So a downstream package needs neither the generator
nor a template body.

Tuple generators are the exception. Their output for a size is built
where that size is used, as reified code is
([`types.generic.specialized`](../spec/04-type-system.md#r-types.generic.specialized)),
and cached per (generator, size).

### Errors

| Source | Reported at | Code |
| --- | --- | --- |
| The generator returns `GenError` | the `at` span it chose, with its `hint` | `generator-failed` |
| A tagged generated line fails to check | the tagged span: a member, variant, or fact | the ordinary code, such as `unsatisfied-trait-bound` |
| An untagged generated line fails | the `@derive` line, naming the generator's package as the likely fault | the ordinary code |
| A panic, or the budget is exceeded | the `@derive` line | `generator-failed`, `generator-budget` |

Every report also quotes the generated line as a note. The user never
gets a position inside a virtual file only.

### LSP

| Feature | Behavior |
| --- | --- |
| Hover on `@derive(Debug)` | shows the generated code |
| Go to definition on `p.debug(out)` | opens a read-only virtual document for the output |
| Diagnostics | as in [Errors](#errors), at user lines |
| Keystrokes | a body edit leaves every `Decl` equal, so no generator runs; a declaration edit reruns only that target's generators |

The C# lesson applies: its first generator API ran on every keystroke,
and the incremental API caches each step
([cookbook][cs-incremental]). Here the key is narrow from the start.

### Rules The Design Adds

| Rule | Kind | Tier | Count |
| --- | --- | --- | ---: |
| `std.gen.generator` is recognized by name; one per trait; in the trait's module; its function type | 3 intrinsic | language | 4 |
| `Decl`, `DeclKind`, `FieldInfo`, `VariantInfo`, `TypeRef`, `FactInfo`, `Span`, `Code`, `GenError`: what the compiler supplies | 3 intrinsic | language | 12 |
| `@derive(X)` runs `X`'s generator once per derivation, with its facts | 2 semantic | language | 2 |
| Output rules 1-5 | 2 semantic | language | 6 |
| Empty row, no `block_on`, determinism, budget | 2 semantic | language | 4 |
| Error mapping | 2 semantic | language | 4 |
| Tuple generators: one generic impl per size used | 2 semantic | language | 3 |
| **Total** | no syntax | | **35** |

## What Generators Could Replace

Each row counts numbered rules. "Moved" means a rule leaves the language
tier and reappears in `spec/std/`.

| Feature | Replace? | Language removed | Language added | Stdlib | Why |
| --- | --- | ---: | ---: | ---: | --- |
| `Structure`, templates, walk/describe/build, handles, walkers, bounds, limits | yes | 83 | 32 of the 35 above | +6 helpers | a generator emits direct code, so no traversal protocol is needed |
| Self references | yes, moved | 13 | 0 | +13 | `self_ref` is a function of `TypeRef`, computable in plain hd |
| Members and variants | reworded | 0 | 0 | 0 | they become the `FieldInfo` and `VariantInfo` rules |
| Omitted members | partly | 2 | 0 | 0 | the default check moves into generated code |
| `@error` | yes, moved | 54 | 0 | +35 | `@derive(Error)` with `std.error` facts; `Error` gets a generator |
| `annot.derive.error-trait` | yes | 1 | 0 | 0 | `Error` becomes derivable |
| Comparison derives, newtypes | partly | 3 | 0 | 0 | field-missing errors become ordinary type errors at the member |
| Derived Arbitrary, Debug, Eq, Hash | reworded | 0 | 0 | -1 | the std rules say "generator" for "template"; `with` needs no typed read |
| Tuple derives at every size | yes | 15 | 3 of the 35 above | 0 | per-size generic impls replace the tuple `Structure` and its obligation rule |
| Typed-fact checks | no | 0 | 0 | 0 | see below |
| `by` delegation | no | 0 | 0 | 0 | see below |
| `all!` | no | 0 | 0 | 0 | see below |
| Literal markers | no | 0 | 0 | 0 | see below |
| **Total** | | **171** | **35** | **+53** | |

The tuple rows are the 7 tuple-template rules of `annot.template` and
the 8 of `annot.tuple`. Language net: about **-136**. Both tiers: about
**-83**. Reworded rows are not counted. Every count is an estimate until
a spec pass writes the rules.

Diagnostics: `structure-outside-template`, `marker-template`,
`generic-member-call`, `member-not-derivable`, and `invalid-error-marker`
go. `generator-failed` and `generator-budget` come in.

### What It Cannot Replace

| Feature | Why a generator does not fit |
| --- | --- |
| `by` delegation | It works for any trait, so the trait must be named in a fact. A fact holds a value, and a trait is not one. The options are a string, `@delegate("Describe")`, or a new rule that lets a trait be a decorator type argument. Each costs as much as `by`'s 13 rules and reads worse. |
| `all!` | Its result type comes from each call's arguments. A generator sees declarations, not call sites. Per-size functions `all2!` to `all8!` give up the one name, and the bodies stay intrinsic ([`req.combinator.intrinsic`](../spec/11-requirements-and-suspension.md#r-req.combinator.intrinsic)). |
| Literal markers | `12px` is use-site sugar for `px(12)`. A declaration-level generator never sees the literal. |
| Typed-fact checks | They check a fact where it is written, whether or not any generator reads it. Literal markers rely on them. A generator's output checks only the facts it uses. |
| Facts, decorators, derivation blocks, member lines | They are the generator's input, so they stay. |
| Law partners | They relate one type's derived and hand-written impls. A generator could check its own list, but not a hand-written partner elsewhere. |
| Runtime reflection: `Inspectable`, `shape[T]()` | Run-time data, out of scope. `Decl` and the shape types might later merge. |

## Before And After

Library code is shown once. A line whose syntax no chapter specifies ends
in `# hypothetical syntax`; none was needed. Parsing checks syntax only.

### A Derive

The user side is unchanged:

```text
@derive(Debug)
data Point:
    x: i64
    y: i64
```

Today `Debug` derives through a template and a walker. A user template
such as `Encode` looks like this, from the conformance suite:

```text
use std.structure.{Structure, Field, Variant, Walker}

data Encoder:
    out: string

impl[S] Walker[S] for Encoder:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        .Ok()

    fn member[F < Encode](mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.out = self.out + h.info.name + "=" + value.encode() + ";"
        .Ok()

impl[T] Encode for T by Structure:
    fn encode(self) -> string:
        let mut w = Encoder { out: "" }
        _ := Structure::walk(self, w)
        w.out
```

With a generator, `Encode` is one function and one decorator:

```text
use std.gen.{Decl, Code, GenError, generator}

fn derive_encode(d: Decl, out: mut Code) -> Result[void, GenError]:
    out.line("impl${bounds_for(d, "Encode")} Encode for ${self_type(d)}:")
    out.line("    fn encode(self) -> string:")
    out.line("        let mut text = \"\"")
    for f in fields_of(d):
        out.line_at(f.span, "        text = text + \"${f.name}=\" + self.${f.name}.encode() + \";\"")
    out.line("        text")
    .Ok()

@generator(derive_encode)
trait Encode:
    fn encode(self) -> string
```

The output for `Point`, shown by `hd expand`, is ordinary hd:

```text
use std.format.DebugWriter

impl Debug for Point:
    fn debug(self, out: mut DebugWriter) -> void:
        let mut s = out.debug_struct("Point")
        s = s.field("x", self.x)
        s = s.field("y", self.y)
        s.finish()
```

### `by` Delegation

Today, one line:

```text
impl Describe for Service by Logger
```

With a delegate generator, the trait must be named as a string, because
a fact cannot hold a trait:

```text
@delegate("Describe", via="Logger")  # hypothetical: a std generator with no trait
data Service:
    Logger
    port: i32
```

Both produce the same output, which is today's expansion:

```text
impl Describe for Service:
    fn describe(self) -> string:
        Describe::describe(self.Logger)

    fn headline(self) -> string:
        Describe::headline(self.Logger)
```

The string loses rename support and early checking, and `@delegate`
fits no trait's generator. So `by` stays.

### `@error`

Today:

```text
@error
enum LoadError:
    @error("cannot read $path")
    Read(path: string, @source error: FsError)
    @error("bad config")
    Yaml(@from error: YamlError)
    @error(transparent)
    Fs(@from error: FsError)
```

With a generator, `@error`, `@source`, `@from`, and `@transparent` are
ordinary `std.error` facts. The message uses `{path}`, because a fact is
evaluated at compile time, where `$path` names nothing:

```text
use std.error.{Error, error, from, source, transparent}

@derive(Error)
enum LoadError:
    @error("cannot read {path}")
    Read(path: string, @source error: FsError)
    @error("bad config")
    Yaml(@from error: YamlError)
    @transparent
    Fs(@from error: FsError)
```

The generated code:

```text
impl Display for LoadError:
    fn to_string(self) -> string:
        match self:
            .Read(path, _) => "cannot read ${path}"
            .Yaml(_) => "bad config"
            .Fs(error) => error.to_string()

impl Error for LoadError:
    fn cause(self) -> Error?:
        match self:
            .Read(_, error) => error
            .Yaml(error) => error
            .Fs(error) => error.cause()

impl From[YamlError] for LoadError:
    fn from(value: YamlError) -> LoadError:
        LoadError.Yaml(value)

impl From[FsError] for LoadError:
    fn from(value: FsError) -> LoadError:
        LoadError.Fs(value)
```

The cost is a second placeholder spelling, `{name}`, used only in error
messages. `$` keeps meaning interpolation everywhere else.

### A Tuple Trait

Today a tuple template covers every size through the tuple's
`Structure`:

```text
use std.function.Tuple
use std.structure.Structure

impl[T < Tuple] Encode for T by Structure:
    fn encode(self) -> string:
        let mut w = Encoder { out: "" }
        _ := Structure::walk(self, w)
        w.out
```

With a generator, the trait opts tuples in, and each size used gets one
generic impl:

```text
@generator(derive_encode, tuples=true)
trait Encode:
    fn encode(self) -> string
```

Output for size 2:

```text
impl[A < Encode, B < Encode] Encode for (A, B):
    fn encode(self) -> string:
        let mut text = ""
        text = text + "_0=" + self._0.encode() + ";"
        text = text + "_1=" + self._1.encode() + ";"
        text
```

Whether `(i32, Label)` implements `Encode` now follows the ordinary
bound rule. The special rule
[`annot.template.tuple.implements`](../spec/14-annotations.md#r-annot.template.tuple.implements)
goes.

### `all!`

Today:

```text
use std.task.all

fn page!(id: i64) -> string:
    let (user, orders) = all!(load_user(id), load_orders(id))
    "${user.name}: ${orders.len()}"
```

A generator could only emit one function per size, each with its own
name and an intrinsic body:

```text
pub fn all2![A, B](a: mut Suspend[A], b: mut Suspend[B]) -> (A, B):
    pass

fn page!(id: i64) -> string:
    let (user, orders) = all2!(load_user(id), load_orders(id))
    "${user.name}: ${orders.len()}"
```

That trades 4 typing rules for a numbered family of names and a size
limit. So `all!` stays.

## Compile-Time Cost

The archived record measured `hd check` and `hd build --wat` on generated
programs: an intrinsic `Eq` cost about 0.03 ms and 0.28 KB per member,
and a user template about 0.57 ms and 3.0 KB
([measurement](archive/COMPTIME_UNIFICATION.md#a-measurement-of-hds-prototype)).

This record repeated it with the code a generator would emit, written by
hand. One run each, on 2026-10-01, on one local machine, with 100 data
types of 40 `i64` fields; times include about 0.4 s of start-up. A second
run was slower overall but kept the same order.

| Program | `check` ms | `build` ms | WAT bytes | Extra `check` per member | Extra WAT per member |
| --- | ---: | ---: | ---: | ---: | ---: |
| no derive | 449 | 628 | 167,171 | | |
| `@derive(Eq)`, intrinsic in the prototype | 574 | 915 | 1,474,665 | 0.031 ms | 327 B |
| hand-written `Eq`, as a generator would emit | 498 | 838 | 963,665 | 0.012 ms | 199 B |
| `@derive(Debug)`, intrinsic in the prototype | 760 | 1,151 | 1,451,725 | 0.078 ms | 321 B |
| hand-written `Debug`, as a generator would emit | 805 | 1,172 | 1,602,625 | 0.089 ms | 359 B |
| `@derive(Encode)` through a user template and walker | 7,853 | 12,133 | 12,456,651 | 1.85 ms | 3,072 B |
| hand-written `Encode`, as a generator would emit | 1,253 | 1,807 | 1,488,553 | 0.20 ms | 330 B |

| Path | Compared with the template path |
| --- | --- |
| generated direct code | about 9 times less check time and 9 times less WAT per member |
| intrinsic derive | about the same as generated direct code |

What the table does not include:

- **Generator run time.** The prototype has no evaluator for function
  calls at compile time, so it was not measured. A generator does a few
  string operations per member. In an interpreter that is likely
  microseconds per member, below the check cost, but this is an estimate.
- **Caching.** With a cache hit, the run is skipped. Only declaration or
  fact edits rerun a generator.
- **Separate compilation.** Today a package interface carries template
  bodies, and each downstream use may specialize them. Derive output is
  compiled once, in the target's package.

The template path is slow because of the prototype's lowering, not the
spec's design, as the archived record says. A fixed template lowering
could close part of the gap. Generated direct code needs no such fix.

Other languages, from the archived record and the survey: Rust and Swift
pay to build each macro crate or plugin; Scala inlines per use site; Zig
interprets per instantiation. Generators here pay one interpreted run per
target and trait, cached.

## Error Messages For A Weak Model

A weak model needs three things: the error on a line it wrote, the name
of what is wrong, and a fix it can type. Today's derivation errors give
all three: `member-not-derivable` names the member at the opt-in.

Proposed reports for the common cases:

```text
data Label:
    text: string

@derive(Debug)
data Tag:
    label: Label  # error: unsatisfied-trait-bound (hint: add @derive(Debug) to Label)

@derive(Error)
enum ParseError:
    @error("bad token {token}")  # error: generator-failed (no member named token; members: text)
    BadToken(text: string)
```

| Case | Today | With generators | Fix the model must find |
| --- | --- | --- | --- |
| A member lacks the trait | `member-not-derivable` at the `@derive`, naming the member | `unsatisfied-trait-bound` at the member, with the generator's hint and the generated line as a note | derive or implement the trait on the member's type |
| Unknown name in an error message | `unknown-name` at the `@error` line | `generator-failed` at the fact, listing the members | fix the placeholder |
| Wrong `arbitrary.with` generator type | `type-mismatch` at the decorator, by typed facts | unchanged: typed facts stay | change the generator |
| A bug in a third-party generator | not possible: templates are checked once | the ordinary code at the `@derive` line, naming the generator's package as the likely fault | not the user's code; report or replace the generator |
| A hand-written impl beside `@derive` | `overlapping-impl` | `overlapping-impl` | remove one |

The new risk is the fourth row. A generator that tags no lines gives
C++-style errors at the opt-in. The first two rows can be better than
today, because each generator writes its own hint.

**A Haiku probe for this design.** The owner plans a Haiku probe of error
messages. These six programs would test the design before any spec pass,
with mocked compiler output:

| Probe | Program | Pass when Haiku |
| --- | --- | --- |
| P1 | `@derive(Debug)` on a type with a member lacking `Debug` | adds `@derive(Debug)` to the member's type |
| P2 | `@error("bad {tokn}")` | fixes the placeholder |
| P3 | a `$path` placeholder written from habit | switches to `{path}` |
| P4 | an untagged generator bug | does not edit the user's type, and says the generator is at fault |
| P5 | the same error shown with and without the generated-line note | picks the right fix in fewer tries with the note |
| P6 | a hand-written `impl Debug` beside `@derive(Debug)` | removes one of them |

## Fit With hd's Decisions

| Decision | Fit |
| --- | --- |
| No higher-kinded types | Holds. Reflection is values, not types. No function returns a type. |
| No packs | Holds. A tuple generator loops over a size inside the generator; users write no pack syntax. Output is a fixed-size generic impl. |
| Written public signatures ([`module.package.annotated`](../spec/10-modules.md#r-module.package.annotated)) | Holds. Output may add only impls and private functions. An impl's bounds are written in its output, not inferred, and the interface records them. |
| Monomorphic closures | Better than today. The walker trait exists because a per-member callback needs a generic method. A generator handles type descriptions, so it needs none. |
| Implementation-neutral spec | Holds. The spec states the input values, the output text, determinism, and error positions. The evaluator and the cache are implementation choices. |
| Two-tier spec | The mechanism is language tier: the compiler knows `std.gen` by name. Each generator, `Debug` to `Error`, passes the tier test and lives in `spec/std/`. `@error` moves from the language tier to the stdlib tier. |
| One block per concern | Holds. Derivation blocks and member lines are unchanged. |
| No action at a distance | Weaker than checked-in code. `@derive` already hides code, and `hd expand` and hover show it. |
| Design G over Design B (2026-09-26) | This reopens Design B's family. New evidence: template bodies are already checked per opt-in; facts already need an evaluator; and generated direct code measured about 9 times cheaper than templates. |
| O1 and O2 rejected in batch 36 | Generators avoid O2's syntax-tree API, sigil, and macro engine. They avoid O1's new syntax and per-target unrolling. They meet the owner's deferred direction. |

## Scopes Compared

| | G0 status quo | G1 schemas only | G2 generators replace templates and `@error` | G3 generators beside templates |
| --- | --- | --- | --- | --- |
| Language rules, net | 0 | 0 | about -136 | about +35 |
| Both tiers, net | 0 | 0 | about -83 | about +35 |
| Costliest change | none | none (tooling) | 3 intrinsic | 3 intrinsic |
| Derivation mechanisms | 1 (templates) | 1 | 1 (generators) | 2 |
| `@error` | intrinsic | intrinsic | stdlib | stdlib or intrinsic |
| `by`, `all!`, literal markers | unchanged | unchanged | unchanged | unchanged |
| Library code checked | once, plus each opt-in | once | each opt-in, in output | both |
| Error site | the member, at the opt-in | the member | the tagged member, or the opt-in | both |
| Prototype TypeScript | as today | as today | about -2,300 lines of derivation; plus an evaluator, reflection, and code tagging, about 1,500 to 2,500 | added only |
| Compile cost per member, prototype | templates 0.6 to 1.9 ms; intrinsic derives 0.03 to 0.08 ms | as today | about 0.01 to 0.2 ms, plus generator runs | both |

## Risks

| Risk | Effect | Mitigation |
| --- | --- | --- |
| Untagged generator output | Errors appear at `@derive`, in code the user never wrote | Output rule: an untagged error names the generator's package; std generators tag every member line |
| The reflection data becomes a stable API | Every change to `Decl` can break generators | Keep it declaration data only, no syntax tree, as KSP does |
| A full compile-time evaluator | More compiler work than fact constants | The spec already needs it for fact expressions that call functions |
| Two placeholder spellings | `{path}` in error messages, `$path` elsewhere | One question for the owner; the probe P3 tests it |
| Generator run time | Not measured | The cache hides it after the first build; measure in the spike |
| Code is hidden | A reader sees `@derive`, not the methods | `hd expand`, hover, and go to definition |
| Hygiene | A generated local can capture a name in a fact's `text` | `fresh` names; output rule 3 |
| Reverses batch 36 "templates stay" | The deferred note kept templates as the mechanism | Only G2 simplifies; G3 adds rules. The owner decides. |

## Recommendation

**Recommendation: G2, generators that replace templates, walkers, and
`@error`, gated on two checks before any spec pass.** Net: about **-136
language rules and -83 over both tiers**, with no new syntax.

- **Only replacement simplifies.** Generators beside templates add about
  35 rules and a second mechanism. G2 removes the walker protocol, the
  largest part of typed derivation.
- **The costly parts already exist.** Facts already need a compile-time
  evaluator, and template bodies are already checked per opt-in.
- **It is faster.** Generated direct code measured about 9 times cheaper
  per member than the prototype's template path.
- **`@error` moves to the stdlib tier.** That is 54 language rules
  replaced by about 35 stdlib rules over existing facts.
- **Keep the four that do not fit.** `by`, `all!`, literal markers, and
  typed facts stay as they are.
- **Gate 1: a stress test.** Write the std generators for `Debug`, `Eq`,
  `Ord`, `Hash`, `Default`, `Arbitrary`, and `Error`, and one JSON codec,
  against `std.gen`. Confirm the reflection data suffices and recount.
- **Gate 2: the Haiku probe** P1-P6 on mocked diagnostics.

What it gives up: library code checked once, independent of targets;
code visible at the derive site without a tool; and one placeholder
spelling. The next best option is G0, the status quo. G1, schemas only,
needs no language rule and could be done as tooling at any time.

## Questions For The Owner

Q1 and Q2 stand alone. Q3 is the core question; Q4 to Q6 wait for it.

### Q1. `by` Stays A Language Form

A generator can write delegation, but must name the trait as a string.

- **A.** Keep `by` as it is. *Recommended.*
- **B.** Replace it with a std delegate generator named by a string.

```text
impl Describe for Service by Logger
```

### Q2. `all!` Stays An Intrinsic

A generator sees declarations, not calls, so it could only emit `all2!`
to `all8!`.

- **A.** Keep `all!` and its four typing rules. *Recommended.*
- **B.** Replace it with per-size generated functions.

```text
fn page!(id: i64) -> (User, List[i64]):
    all!(load_user(id), load_orders(id))
```

### Q3. Generators Replace Templates And `@error`

Derivation would read declaration data and emit hd, instead of walking
values through `Structure`. The user side, `@derive` and facts, stays.

- **A.** Yes, as G2, after the stress test and the Haiku probe.
  *Recommended.*
- **B.** No. Keep templates; allow schema generators as tooling only.
- **C.** No change.

```text
@generator(derive_debug)
pub trait Debug:
    fn debug(self, out: mut DebugWriter) -> void
```

### Q4. Where Derive Output Lives

Waits on Q3.

- **A.** In the build, cached, shown by `hd expand` and the LSP.
  *Recommended:* no stale files, no file noise.
- **B.** Checked in beside the source, Go-style.

```text
@derive(Debug, Eq, Hash)
data Point:
    x: i64
    y: i64
```

### Q5. Error Message Placeholders

Waits on Q3. A fact is evaluated at compile time, so `$path` in
`@error("...")` would name nothing.

- **A.** `@derive(Error)` with `{path}` placeholders. *Recommended.*
- **B.** Keep `$path`, with a rule that an `error` fact's string is not
  interpolated.
- **C.** Keep the `@error` intrinsic, even under Q3 A.

```text
@derive(Error)
enum FsError:
    @error("not found: {path}")
    NotFound(path: string)
```

### Q6. How A Trait Names Its Generator

Waits on Q3.

- **A.** A `@generator(f)` fact before the trait, in the trait's module.
  *Recommended:* no syntax, and the generator sits beside its trait.
- **B.** A decorator on the generator function naming the trait as a
  string.

```text
@generator(derive_encode, tuples=true)
trait Encode:
    fn encode(self) -> string
```

## Sources

[go-cmd]: https://pkg.go.dev/cmd/go#hdr-Generate_Go_files_by_processing_source
[go-blog]: https://go.dev/blog/generate
[go-line]: https://pkg.go.dev/cmd/compile#hdr-Compiler_Directives
[rust-proc]: https://doc.rust-lang.org/reference/procedural-macros.html
[ra-macros]: https://rust-analyzer.github.io/blog/2021/11/21/ides-and-macros.html
[swift-expr]: https://github.com/swiftlang/swift-evolution/blob/main/proposals/0382-expression-macros.md
[swift-attached]: https://github.com/swiftlang/swift-evolution/blob/main/proposals/0389-attached-macros.md
[swift-macro-time]: https://forums.swift.org/t/swift-macros-build-time-overhead-concerns/70443
[swift-prebuilts]: https://forums.swift.org/t/preview-swift-syntax-prebuilts-for-macros/80202
[scala-macros]: https://docs.scala-lang.org/scala3/reference/metaprogramming/macros.html
[scala-options]: https://docs.scala-lang.org/scala3/guides/migration/options-new.html
[zig-comptime]: https://ziglang.org/documentation/master/#comptime
[zig-015]: https://ziglang.org/download/0.15.1/release-notes.html
[zig-quota]: https://ziggit.dev/t/what-is-the-eval-branch-quota/7852
[zls]: https://github.com/zigtools/zls/blob/master/README.md
[cs-overview]: https://learn.microsoft.com/en-us/dotnet/csharp/roslyn-sdk/
[cs-design]: https://github.com/dotnet/roslyn/blob/main/docs/features/source-generators.md
[cs-incremental]: https://github.com/dotnet/roslyn/blob/main/docs/features/incremental-generators.md
[ksp]: https://kotlinlang.org/docs/ksp-overview.html
[ksp-incremental]: https://kotlinlang.org/docs/ksp-incremental.html
[ppx]: https://ocaml.org/docs/metaprogramming

| Topic | Source |
| --- | --- |
| Go generate, generated-file marker, build never runs it | [cmd/go][go-cmd], [Go blog][go-blog] |
| Go `//line` directives | [cmd/compile][go-line] |
| Rust proc macros: crate type, resources, token streams | [reference][rust-proc] |
| Rust macros in the IDE | [rust-analyzer][ra-macros] |
| Swift macros: sandbox, syntax-only input, checked output | [SE-0382][swift-expr], [SE-0389][swift-attached] |
| Swift macro build cost | [overhead thread][swift-macro-time], [prebuilts][swift-prebuilts] |
| Scala 3 quotes and the inline cap | [macros][scala-macros], [options][scala-options] |
| Zig comptime, `@Type`, quota, ZLS | [reference][zig-comptime], [0.15.1 notes][zig-015], [Ziggit][zig-quota], [ZLS][zls] |
| C# generators: additive only, caching, attribute lookup | [overview][cs-overview], [design][cs-design], [incremental][cs-incremental] |
| Kotlin KSP: limits, isolating outputs | [overview][ksp], [incremental][ksp-incremental] |
| OCaml ppx and derivers | [ocaml.org][ppx] |
| hd's earlier measurement, O1, O2, and the deferred direction | [One Compile-Time Intrinsic](archive/COMPTIME_UNIFICATION.md) |
| The prototype's generated-source builder | [`src/checker/generated-source.ts`](../src/checker/generated-source.ts) |

## Parse Log

Every `text` block was parsed with `parseSource` from
[spec/reference-parser/parser.ts](../spec/reference-parser/parser.ts) on
2026-10-01. Parsing checks syntax only; no block is claimed to
type-check. Every `std.gen` name and the meaning of `@generator`,
`@delegate`, `@derive(Error)`, and `{name}` placeholders are hypothetical.
Names such as `bounds_for`, `load_user`, and `Encode` rely on
declarations elsewhere.

| Block | Section | Result |
| ---: | --- | --- |
| 1 | The reflection data | parses |
| 2 | `Code` and `generator` | parses |
| 3 | `Debug`'s generator | parses |
| 4 | A derive: user side | parses |
| 5 | A derive: today's template and walker | parses |
| 6 | A derive: `Encode` as a generator | parses |
| 7 | A derive: generated output | parses |
| 8 | `by`: today | parses |
| 9 | `by`: a delegate generator | parses; its meaning is hypothetical |
| 10 | `by`: generated output | parses |
| 11 | `@error`: today | parses |
| 12 | `@error`: with a generator | parses; its meaning is hypothetical |
| 13 | `@error`: generated output | parses |
| 14 | Tuple trait: today | parses |
| 15 | Tuple trait: with a generator | parses |
| 16 | Tuple trait: output for size 2 | parses |
| 17 | `all!`: today | parses |
| 18 | `all!`: per-size functions | parses |
| 19 | Error messages | parses; the errors are proposed checker results |
| 20 | Q1 | parses |
| 21 | Q2 | parses |
| 22 | Q3 | parses |
| 23 | Q4 | parses |
| 24 | Q5 | parses |
| 25 | Q6 | parses |
