# Spec Terminology Inventory (S11, 2026-10-08)

Owner direction: don't invent unnecessary new words. Source: `pnpm run
spec glossary --json` (186 terms: bold first uses plus the hand-written
glossaries in `spec/README.md` and `spec/std/README.md`), plus a
repeated-noun sweep of `spec/lang/`, `spec/std/`, `spec/cli/`,
`spec/README.md` and `spec/STYLE.md` for non-glossary terms of art
(`fuel`, `slot`, `witness`, `realm`, `ledger`, `drift` and
kin: all benign — ordinary words, standard concepts, or example
identifiers).

Verdict: **keep 186, replace 0, owner 0.** The closest calls are
explained per row: `ghost entry` is verification vocabulary (Dafny
ghost state), not spookiness; `draw budget` is QuickCheck vocabulary;
`law partners`, `take part`, `fits`, `coherence slot` and
`template` are plain words. The one glossary-hygiene note (not a term
verdict): `same compiled program` and `the same compiled program`
are duplicate entries for one rule and should merge when that section is
next touched.

| Term | Defined in | Meaning in plain words | Verdict | Replacement or note |
| --- | --- | --- | --- | --- |
| active | lang/11-requirements-and-suspension.md#r-req.bang.active | A driver context while its executor is evaluating or polling it on the current program-instance call stack. | keep | standard PL or domain vocabulary — ordinary word for the currently executing context |
| available | lang/03-names-and-scopes.md#r-names.visible.trait | A trait method is available in a module when its trait is available to dot-call lookup there. | keep | ordinary English |
| bare step | lang/05-expressions.md#r-expr.pipe.step-kinds | A pipe step that is a name or path without `_`. | keep | ordinary English |
| base seed | cli/command-line.md#r-cli.test.seed | `hd test` derives a property test case's **base seed**, the `seed` of its first case, from the test case's name. | keep | standard tooling or process vocabulary |
| base tag | cli/command-line.md#r-cli.dep.pseudo.base | A pseudo-version's **base tag** is the tag its form names, by the pseudo-version table, with the package's tag prefix. | keep | ordinary English |
| bound method reference | lang/07-functions.md#r-fn.ref.bound | `value::name`, where `value` names a value, as a function value. | keep | standard PL or domain vocabulary |
| bound requirement key | lang/11-requirements-and-suspension.md#bound-requirement-keys | A requirement key that binds associated types, such as `Store[Item = User]`; its provider value has the trait value type `dyn Store[Item = User]`. | keep | standard PL or domain vocabulary |
| bound-only parameter | lang/04-type-system.md#r-types.generic.infer.bound | A type parameter of a call that no parameter type names but a bound of another type parameter does; inference solves it from that bound. | keep | ordinary English |
| build directory | cli/command-line.md#r-cli.build.directory | The directory `build` in a package directory, where `hd` writes its build and cache output, such as `build/debug/NAME.wasm`. | keep | ordinary English |
| build profiles | lang/04-type-system.md#integer-arithmetic | A program is built under one of three **build profiles**: debug, release, or test. | keep | standard tooling or process vocabulary |
| cache directory | cli/command-line.md#cache | The one directory per user where `hd` keeps every fetched dependency version, read-only. | keep | standard tooling or process vocabulary |
| call place | lang/05-expressions.md#callable-values | A call `v()` whose callee's type implements `Update`, so `v() = x` and `v() op= x` store through it. | keep | standard PL or domain vocabulary — Rust vocabulary for a callable storage place |
| callable value | lang/05-expressions.md#callable-values | A value whose type implements `Apply`, read by calling it with no arguments, as in `count()`. | keep | standard PL or domain vocabulary — Python vocabulary for a value used as a function |
| capability grant | cli/command-line.md#capability-grants | What a program's host capability traits may touch at run time: for each trait, no limit, a total deny, or a list of scope entries. | keep | standard tooling or process vocabulary |
| coherence slot | lang/14-annotations.md#terminology | One `(trait, concrete target)` pair over the resolved package graph. | keep | standard PL or domain vocabulary — ordinary `slot`: one table position per trait-target pair |
| collect target | std/iter.md#collect-targets | The collection that `collect` builds, named by the expected type. | keep | ordinary English — ordinary words for what `collect` builds |
| compatibility line | lang/10-modules.md#r-module.version.line | The versions of a package that must stay compatible: one major number, or `0.MINOR` below 1.0. | keep | ordinary English |
| compiled entries | cli/command-line.md#r-cli.cache.obj | `hd` stores its **compiled entries** in the directory `obj` of the cache directory. | keep | ordinary English |
| compound assignment | lang/05-expressions.md#compound-assignment | A statement `place op= value`, such as `total += x`, that combines an operator with a store. | keep | standard PL or domain vocabulary |
| conflict | lang/03-names-and-scopes.md#r-names.conflict.definition | Two or more members with one name at the smallest depth where that name occurs, including one member reached through two paths. | keep | ordinary English |
| copy-update literal | lang/08-data-and-enums.md#copy-update-literals | A data literal with one leading spread, which builds a new value from an existing one. | keep | ordinary English |
| data type | lang/08-data-and-enums.md#r-data.kind.data | A nominal product type with reference semantics. | keep | standard PL or domain vocabulary |
| Debug builders | std/format.md#debug-builders | The `DebugWriter` methods that describe a value as a struct, tuple, list, or map. | keep | ordinary English |
| declarations | lang/02-grammar.md#declarations | A declaration is an optionally public named declaration or an implementation: | keep | ordinary English |
| default executable | cli/command-line.md#r-cli.exe.default-main | With no `[[executable]]` table in `hd.toml`, `src/main.hd` is the package's **default executable**, as Cargo's `src/main.rs` is. | keep | standard tooling or process vocabulary |
| default profile | cli/command-line.md#host-capabilities | `hd FILE`, `hd run`, a task, and the REPL use the **default profile**, which binds these traits: | keep | standard tooling or process vocabulary |
| default type | lang/04-type-system.md#r-types.literal.local.default | The type a literal class takes when it meets no type: `i32` for integers when a member is a signed literal, `usize` otherwise, and `f64` for floating-point literals. | keep | ordinary English |
| dependency requirement | lang/10-modules.md#dependency-requirements | A manifest entry `PATH@VERSION` that maps a dependency key to a host path and a minimum version. | keep | ordinary English |
| depth | lang/03-names-and-scopes.md#r-names.part.depth | The number of embedded fields on a part's path. | keep | ordinary English |
| derivation block | lang/14-annotations.md#derivation-blocks | An `impl Trait for X by Structure:` that applies a trait's template to one type, with optional member lines. | keep | ordinary English |
| derived variance | lang/04-type-system.md#r-types.variance.target.derived | The variance an inherent `impl` parameter has in the implementation's target type, by which its methods are checked. | keep | standard PL or domain vocabulary |
| dev dependency | lang/10-modules.md#r-module.test.dev-dependency | A dependency that the manifest declares in `[dev-dependencies]`, which test code and tasks may use and a dependent never sees. | keep | standard PL or domain vocabulary |
| doc test | lang/10-modules.md#r-module.test.doc.block | A fenced `hd` block in a documentation comment of a module under the source root, compiled as its own program with one test case. | keep | standard PL or domain vocabulary |
| draw budget | std/testing.md#r-std-testing.budget | The per-case limit on draws from `Choices`; once it is spent, every draw returns its simplest value. | keep | standard tooling or process vocabulary — property-testing vocabulary: a random draw is QuickCheck usage |
| driver context | lang/11-requirements-and-suspension.md#r-req.bang.driver-contexts | Where a bang call is valid: a suspending function or closure body, or the host executor driving `main!`. | keep | standard PL or domain vocabulary — ordinary words for the executing context |
| dynamic provider | lang/11-requirements-and-suspension.md#lexical-and-dynamic-providers | A provider a closure gets at each call, because its row keeps the key. | keep | standard PL or domain vocabulary |
| embedded field | lang/08-data-and-enums.md#data-embedding | A bare type-name member of a data declaration, which embeds another data type. | keep | ordinary English |
| entry module | lang/10-modules.md#r-module.init.entry-module.selected | The module that a program starts from: a module that the toolchain selects in a package, or the file of a single-file program. | keep | standard PL or domain vocabulary — ordinary words; also Wasm `module` usage |
| enum | lang/08-data-and-enums.md#r-data.kind.enum | A nominal sum type. | keep | standard PL or domain vocabulary |
| error derivation | lang/14-annotations.md#error-derivation | Implementing `Display`, `Error`, and `From` for an error type from its `@error` lines. | keep | standard PL or domain vocabulary |
| error type | lang/14-annotations.md#r-annot.error.type | An enum with a bare `@error` line, or a data type with an `@error("...")` or `@error(transparent)` line. | keep | ordinary English |
| executable | cli/command-line.md#r-cli.exe.table | A program a package ships, declared by an `[[executable]]` table of `hd.toml`, or `src/main.hd` when the manifest declares none. See `cli.exe.table` and `cli.exe.default-main`. | keep | ordinary English |
| executable entry point | lang/10-modules.md#r-module.entry.definition | A public top-level function named `main` or `main!` with no parameters. | keep | standard PL or domain vocabulary |
| exhausted | lang/06-control-flow.md#r-flow.for.iterator-exhausted | An iterator whose `next` has returned `.None`. | keep | ordinary English |
| exhausted iterator | lang/06-control-flow.md#r-flow.for.iterator-exhausted | An iterator whose `next` has returned `.None`. What a later `next` returns is unspecified. | keep | ordinary English |
| fact | lang/14-annotations.md#facts | An ordinary value attached to a type, member, or variant for derivations to read. | keep | standard PL or domain vocabulary — logic-programming vocabulary (Datalog) for an asserted proposition |
| field lookup | lang/03-names-and-scopes.md#field-lookup | The steps that resolve `x.name` to one field from a module. | keep | ordinary English |
| field shorthand | lang/02-grammar.md#r-grammar.primary.field-shorthand | A `data_field_item` that is a bare `identifier` is **field shorthand**: `x` means `x: x`, as in `Point { x, y }`. | keep | ordinary English |
| fields | lang/03-names-and-scopes.md#r-names.member.fields | Its **fields**, including embedded fields named by their embedded type name, are found by field lookup. | keep | ordinary English |
| fingerprint | cli/command-line.md#r-cli.test.fingerprint | A test program's **fingerprint** changes whenever a change could change its outcome: a source file it reads, a dependency, a manifest setting, the `hd` version, or a test option such as `--seed`. | keep | standard PL or domain vocabulary — content-addressing vocabulary for a content hash |
| fits | lang/09-traits.md#r-trait.resolve.fits | A candidate implementation fits a call when the call's arguments check against its method's parameter types. | keep | ordinary English — ordinary verb: a candidate the arguments satisfy |
| fixed elements | lang/04-type-system.md#r-types.tuple.rest.form | The elements of a tuple type other than its rest element. | keep | ordinary English |
| flag | std/cli.md#r-std-cli.cli.flag | A command-line argument that `std.cli` reads as set or not set; it takes no value. | keep | standard PL or domain vocabulary |
| folder | lang/10-modules.md#r-module.folder.holder | The directory that holds a source file, or for a file `x.hd` with child modules, the directory `x/` that holds them; nested directories are separate folders. | keep | ordinary English |
| folder graph | lang/10-modules.md#r-module.cycle.folder-edge | A package's folders, with an edge where a file in one folder uses a module in another. It must be acyclic. | keep | ordinary English |
| generic field | lang/04-type-system.md#r-types.path.field.generic | A field whose declared type is a generic parameter; reading it yields the substituted type unchanged. | keep | ordinary English |
| handle | lang/14-annotations.md#handles | A compiler-generated constant naming one member (`Field[S, F]`) or variant (`Variant[S]`) of a derivation's target. | keep | ordinary English — ordinary word for an opaque reference |
| hidden item | lang/10-modules.md#r-module.interface.hidden-item | An item in a package interface that only the code of an instantiated template may name. | keep | ordinary English |
| hides | lang/03-names-and-scopes.md#r-names.hide.depth | A member hides every member with the same name at a greater depth, in the same namespace. | keep | ordinary English |
| infinite loop | lang/06-control-flow.md#r-flow.while.infinite | A `while` loop whose condition is the literal `true`. It completes normally only through a `break` that targets it. | keep | ordinary English |
| inherent associated function | lang/09-traits.md#inherent-members | A member of an inherent implementation without a `self` parameter, called through the type, as in `User::guest()`. | keep | standard PL or domain vocabulary |
| inherent method | lang/09-traits.md#inherent-members | A member of an inherent implementation whose first parameter is `self` or `mut self`, called with dot syntax. | keep | standard PL or domain vocabulary |
| initialization group | lang/10-modules.md#r-module.init.group | A strongly connected component of the use graph: one module, or modules that use each other in a loop, initialized together. | keep | ordinary English |
| inspectable types | lang/09-traits.md#inspectable-types | The types for which the compiler supplies `Inspectable`: primitives, module-level declarations, collections and tuples of inspectable types, and matching dynamic values. | keep | standard PL or domain vocabulary |
| integration test module | lang/10-modules.md#r-module.test.integration | A module under the package's test root, which sees the package as a dependent does. | keep | standard PL or domain vocabulary |
| integration test program | lang/10-modules.md#r-module.test.integration.program | A file directly under the test root, compiled as its own program. | keep | standard PL or domain vocabulary |
| intrinsic method | lang/09-traits.md#intrinsic-methods | An implementation method written after `@intrinsic`, with no body, whose body the compiler supplies. Only the standard library declares one, for a primitive operation such as integer addition. | keep | standard PL or domain vocabulary |
| irrefutable | lang/06-control-flow.md#r-flow.let.irrefutable | A pattern that alone covers its initializer's type, so a `let` with it needs no `else`. Any other pattern is refutable. | keep | standard PL or domain vocabulary |
| iterable | lang/06-control-flow.md#r-flow.for.accepts | A value whose type implements `Iterable[T]`, such as a `List`, a `Map`, or a range `a..b`. An `Iterator` is not iterable, though `for` takes a mutable one directly. | keep | standard PL or domain vocabulary |
| iterator | lang/06-control-flow.md#r-flow.for.iterator-type | A value of the prelude type `Iterator[T]`. It stores one traversal's progress and is single-pass: a second traversal calls `iter()` on the source again. | keep | standard PL or domain vocabulary |
| iterator adapters | std/iter.md#iterator-adapters | Methods of the prelude `Iterator[T]` that wrap an iterator in a new one, or drain it. | keep | standard PL or domain vocabulary |
| known implementation | lang/03-names-and-scopes.md#r-names.member.known-impl | An implementation in the program's dependency graph whose target matches a type; a local one counts only where its methods are available. | keep | ordinary English |
| law partners | lang/09-traits.md#law-partners | Comparison and hash traits whose laws relate them, such as `Hash` and `Eq`. | keep | ordinary English — plain words: traits whose laws relate them |
| let-else | lang/02-grammar.md#let-else-statements | A `let` statement with a refutable pattern and an `else` block, which runs when the pattern does not match and must diverge. | keep | name the language itself spells — name the language spells (Rust names it too) |
| lexical provider | lang/11-requirements-and-suspension.md#lexical-and-dynamic-providers | A provider a closure fixes where it is written, by capturing the value of `$.use`. | keep | standard PL or domain vocabulary |
| literal class | lang/04-type-system.md#r-types.literal.local.class | Unsuffixed literals with no expected type that meet one another; they take one width. | keep | standard PL or domain vocabulary |
| literal function | lang/05-expressions.md#literal-suffixes | A function marked `@num_suffix` or `@str_prefix`, which a suffixed literal or prefixed string calls. | keep | standard PL or domain vocabulary |
| literal suffix | lang/01-lexical-structure.md#literal-suffixes | A name written directly after a numeric literal's digits, which names a suffix function. | keep | standard PL or domain vocabulary |
| local type names | lang/03-names-and-scopes.md#r-names.category.local-type | The name category of data types, enums, traits, aliases, and newtypes declared inside an executable suite. | keep | ordinary English |
| manifest hash | cli/command-line.md#r-cli.sum.manifest-hash | The tree hash of a tree that holds only a dependency version's `hd.toml`. | keep | standard tooling or process vocabulary |
| manifest line | cli/command-line.md#r-cli.sum.manifest-line | An `hd.sum` line that records the manifest hash of a version that selection reads. | keep | standard tooling or process vocabulary |
| member line | lang/14-annotations.md#member-lines | A line of a derivation block that edits one member's facts or omits it, or a line of a trait-less derivation block that edits its metadata. | keep | ordinary English |
| member metadata | lang/14-annotations.md#terminology | The ordered list of values attached to a data field, an enum variant, or a parameter. | keep | ordinary English |
| member names | lang/03-names-and-scopes.md#r-names.category.member | The name category of data fields, embedded fields, methods, enum variants, and tuple fields, within the namespace of their owning type. | keep | ordinary English |
| method lookup | lang/03-names-and-scopes.md#method-lookup | The steps that resolve `x.name(args)` to an own inherent method or a candidate. | keep | ordinary English |
| method reference | lang/07-functions.md#method-references | A method or associated function named as a function value, written `Owner::name` or `value::name` without arguments. | keep | ordinary English |
| methods | lang/03-names-and-scopes.md#r-names.member.methods | Its **methods** are found by method lookup. | keep | ordinary English |
| minimal version selection | lang/10-modules.md#version-selection | Choosing, for each host path and compatibility line, the largest minimum that any reached manifest states. | keep | ordinary English |
| module names | lang/03-names-and-scopes.md#r-names.category.module | The name category of top-level types, traits, functions, and names introduced by use declarations. | keep | ordinary English |
| mutable access | lang/04-type-system.md#mutation-checks | What an expression has when its type is `mut U`, its type is a parameter bounded by `mut Trait` or `mut Any`, or it is `self` in a `mut self` method. | keep | ordinary English |
| mutable edge | lang/04-type-system.md#r-types.path.field.mutable-edge | A direct field declared `field: mut U`; a readonly container removes its `mut`. | keep | ordinary English |
| mutable edges | lang/08-data-and-enums.md#mutable-edges | What a data type has when it, or a type it embeds at any depth, declares a direct `field: mut U`. | keep | ordinary English |
| mutable requirement trait | lang/11-requirements-and-suspension.md#r-req.mut.trait | A trait that declares or inherits a `mut self` method; its providers always have mutable access. | keep | ordinary English |
| non-reassignable | lang/04-type-system.md#r-types.view.non-reassignable | A binding whose name cannot be rebound. | keep | ordinary English |
| operator trait | lang/05-expressions.md#operator-traits | A `std.ops` trait, such as `Add[Rhs = Self]`, whose implementation gives a type one operator. | keep | standard PL or domain vocabulary |
| option | std/cli.md#r-std-cli.cli.option | A command-line argument that `std.cli` reads with one value. | keep | standard PL or domain vocabulary |
| package mode | cli/command-line.md#r-cli.mode.package.nearest | How a command works when the nearest `hd.toml` at or above its start directory declares a package. | keep | ordinary English |
| pad bits | std/encoding.md#r-std-encoding.base64.encode.pad-bits | The low bits of the last base64 symbol that encode no byte; they are zero. | keep | ordinary English |
| padding | std/encoding.md#r-std-encoding.base64.decode.padding | The one or two `=` characters that end a base64 text. | keep | ordinary English |
| part | lang/08-data-and-enums.md#parts-and-copies | The value an embedded field holds: the outer value's own copy of a value of the embedded type. | keep | ordinary English |
| partially denied | cli/command-line.md#partial-deny | A trait whose grant is a list of scope entries; a call outside them returns a `NotGranted` error. | keep | standard tooling or process vocabulary |
| path requirement | lang/10-modules.md#r-module.path-dep.form | A manifest value `{ path = "DIR" }` through which a package depends on the local package in `DIR`. | keep | ordinary English |
| pattern | lang/14-annotations.md#r-annot.typed-fact.pattern | That type argument is the fact type's **pattern**, a type over the fact type's own type parameters, such as `T`, `List[T]`, or `fn(T) -> R`. | keep | standard PL or domain vocabulary |
| pipe expression | lang/05-expressions.md#pipe-expressions | `value \\|> step`, which passes a value to a step. | keep | ordinary English |
| pipeline | cli/command-line.md#pipelines | A **pipeline** is the way an implementation generates code for a build. | keep | standard PL or domain vocabulary — ordinary word for staged execution |
| place expression | lang/05-expressions.md#r-expr.category.place | An expression that identifies a storage location, which may be read or, when permissions allow, assigned. | keep | standard PL or domain vocabulary — Rust vocabulary for an addressable expression |
| positional | std/cli.md#r-std-cli.cli.positional | A command-line argument that is not a flag or an option; `std.cli` reads it by position. | keep | standard PL or domain vocabulary |
| positional spread | lang/05-expressions.md#positional-spreads | An argument `x...` that passes the value `x` in place of separate arguments: a tuple fills the callee's remaining inputs, and a vararg takes a value of its own type. | keep | ordinary English |
| prefix function | lang/05-expressions.md#r-expr.literal-fn.marker | A literal function marked `@str_prefix`, which a prefixed string calls. | keep | standard PL or domain vocabulary |
| prefixed string | lang/01-lexical-structure.md#r-lex.prefix.form | An identifier followed directly by `"` or `"""`, as in `sql"..."`. | keep | ordinary English |
| prelude | lang/10-modules.md#prelude | The implicit scope of public standard-library names that every module has. | keep | standard PL or domain vocabulary — Rust vocabulary for implicitly imported names |
| primitive types | lang/04-type-system.md#primitive-types | `bool`, the integer types `i8` to `i64` and `u8` to `u64`, `f32`, `f64`, `char`, and `string`. | keep | standard PL or domain vocabulary |
| program instance | lang/10-modules.md#r-module.init.program-instance | One instantiated Wasm module graph with its module storage, provider bindings, and execution state. | keep | standard PL or domain vocabulary — WebAssembly vocabulary for a running module |
| promoted candidate | lang/03-names-and-scopes.md#r-names.method-lookup.promoted-candidate | Among the promoted inherent methods that take part, the one with the called name at the smallest depth. | keep | ordinary English |
| promoted member | lang/03-names-and-scopes.md#r-names.promote.member | A `pub` field or `pub` inherent method of a part's type, reached from the outer type through the part's path. | keep | ordinary English |
| property test | std/testing.md#property-tests | A test case whose body runs on inputs drawn from a `Choices` source. | keep | standard PL or domain vocabulary |
| pseudo-version | lang/10-modules.md#r-module.version.pseudo | A version that names one untagged commit by a base version, its time, and its hash. | keep | standard PL or domain vocabulary — Go vocabulary for a version synthesized from a commit |
| range pattern | lang/02-grammar.md#r-grammar.pattern.range | A **range pattern** takes one of five forms: `a..=b`, `a..b`, `a..`, `..=b`, or `..b`. | keep | standard PL or domain vocabulary |
| readonly edge | lang/04-type-system.md#r-types.path.field.readonly-edge | A field declared `field: U` with a composite `U`, which gives readonly access through any container. | keep | ordinary English |
| readonly view | lang/04-type-system.md#r-types.view.term | The `T` access to a composite value; it does not imply deep immutability. | keep | ordinary English |
| refutable | lang/06-control-flow.md#r-flow.let.irrefutable | A pattern that may fail to match its initializer, such as `.Some(v)`; a `let` with one needs an `else` block. | keep | standard PL or domain vocabulary |
| requirement row | lang/11-requirements-and-suspension.md#r-req.row.definition | The normalized unordered set of requirement keys on a callable signature. | keep | ordinary English |
| requirement-free | lang/07-functions.md#r-fn.default.requirement-free | A default expression that uses no provider and does not suspend. | keep | ordinary English |
| rest element | lang/04-type-system.md#rest-elements | A last tuple element `List[T]...`, which stands for any number of trailing `T` values. | keep | ordinary English |
| rest member | lang/14-annotations.md#r-annot.tuple.rest | The one `List[T]` member that a rest tuple's `Structure` has for its rest element. | keep | ordinary English |
| root file | lang/10-modules.md#r-module.relative.root-file | `src/lib.hd`, `src/main.hd`, or an integration test program, whose relative lookup starts at its root and which has no `super`. | keep | ordinary English |
| row alias | lang/11-requirements-and-suspension.md#row-aliases | A transparent alias that names a set of requirement keys. | keep | ordinary English |
| row parameter | lang/11-requirements-and-suspension.md#r-req.row.parameter | A generic parameter whose values are requirement rows. | keep | ordinary English |
| row slot | lang/11-requirements-and-suspension.md#r-req.row.slot | A place in a type that takes a requirement row, written after `$`: `$.Context[...]`, the row argument of `Fn`, and a type argument for a row parameter. | keep | ordinary English |
| rule ID | STYLE.md#rule-ids | A stable dotted name for one normative rule. | keep | ordinary English |
| runtime identity | lang/09-traits.md#r-trait.identity.definition | Two types share it when they are the same declaration applied to type arguments with the same runtime identity. | keep | standard PL or domain vocabulary |
| runtime profile | lang/10-modules.md#r-module.profile.definition | A named compile-time set of host capability traits, their boundary adapters, and runtime choices such as panic exit statuses. | keep | ordinary English |
| safe | cli/command-line.md#r-cli.fix.safe | A fix-it is **safe** when applying it adds no syntax error and no new diagnostic. | keep | standard tooling or process vocabulary |
| same compiled program | lang/11-requirements-and-suspension.md#r-req.determinism.same-program | Two builds whose compiler outputs are byte-identical. | keep | ordinary English — duplicate of `the same compiled program` below; glossary hygiene note, not a term verdict |
| scalar boundary | lang/04-type-system.md#r-types.string.boundary | A byte offset of a string, from `0` to its length, that does not fall inside a scalar value's encoding. | keep | ordinary English |
| script | lang/10-modules.md#r-module.init.script | An entry module with no `main`, whose top-level executable statements are the entry behavior. | keep | name the language itself spells |
| sealed trait | lang/09-traits.md#sealed-traits | A standard trait whose implementations only the compiler and the standard library supply. | keep | standard PL or domain vocabulary — Scala vocabulary for a closed trait family |
| self reference | lang/14-annotations.md#self-references | A member's or variant's `self_ref`: whether its type needs the type being derived (`.Required`), only refers to it (`.Optional`), or neither (`.Absent`), computed from its type alone. | keep | ordinary English |
| serialization opt-in | lang/14-annotations.md#serialization | A type's implementation of `std.serde.Serialize` or `std.serde.Deserialize`, one permission for every format to write or build its values, private members included. | keep | owner-blessed wording from S10 — owner-blessed in S10: say what it means where a noun is unavoidable |
| shape | lang/04-type-system.md#shapes-and-generic-code | In generic code, the machine representation a value occupies. | keep | standard PL or domain vocabulary — PL vocabulary for a value's machine representation |
| shared task module | cli/command-line.md#r-cli.task.shared | A module in a subdirectory of `tasks`, such as `tasks/shared/zip.hd`, is a **shared task module**. | keep | ordinary English |
| shared test module | lang/10-modules.md#r-module.test.integration.shared | A module in a subdirectory of the test root, which every integration test program may use. | keep | ordinary English |
| signed literal | lang/04-type-system.md#r-types.literal.local.signed | An integer literal written directly after unary `-` or `+`, as in `-1` or `+5`. | keep | ordinary English |
| single-file program | lang/10-modules.md#single-file-programs | One source file compiled with no package, which may use only `std`. | keep | standard PL or domain vocabulary |
| spread pattern | lang/06-control-flow.md#spread-patterns | A last tuple-pattern element, a name or `_` followed by `...`, that matches a rest element's list, as in `let (a, xs...) = t`. | keep | standard PL or domain vocabulary |
| substitution step | lang/05-expressions.md#r-expr.pipe.step-kinds | A pipe step that contains `_`. | keep | ordinary English |
| suffix function | lang/05-expressions.md#r-expr.literal-fn.marker | A literal function marked `@num_suffix`, which a suffixed literal calls. | keep | standard PL or domain vocabulary |
| suffixed literal | lang/05-expressions.md#literal-suffixes | A numeric literal with a literal suffix, such as `250ms`, which calls the suffix function, as `ms(250)`. | keep | standard PL or domain vocabulary |
| summary | cli/command-line.md#r-cli.doc.summary | An item's **summary** is the first sentence of its documentation, or empty when it has none. | keep | standard tooling or process vocabulary |
| take part | lang/03-names-and-scopes.md#r-names.take-part.definition | The members of a type that lookup considers: its own fields and inherent methods, whatever their visibility, and its promoted members. | keep | ordinary English — ordinary English: members participating in lookup |
| target parameter | lang/14-annotations.md#r-annot.typed-fact.pattern.trivial | That parameter is the fact type's **target parameter**, and it is inferred as the target's whole type. | keep | ordinary English |
| task | cli/command-line.md#tasks | A development program of a package, a file `tasks/NAME.hd` that `hd run NAME` runs and the package never ships. | keep | standard PL or domain vocabulary — ordinary word for a unit of scheduled work |
| template | lang/14-annotations.md#templates | A trait's one derived implementation, written `impl[T] Trait for T by Structure:` in the trait's module. | keep | standard PL or domain vocabulary — ordinary word for a code outline stamped per member |
| template helper | lang/10-modules.md#r-module.package.template-helper | A private item that a template body names; it follows the signature rules of a public declaration. | keep | standard PL or domain vocabulary — ordinary words |
| test case | lang/10-modules.md#test-cases | One test, registered by a call of the prelude function `it` in test position, by one row of an `it_each` call, or by an `it_prop` or `it_prop_with` call. | keep | standard PL or domain vocabulary |
| test code | lang/10-modules.md#r-module.test.code | A package's `tests:` blocks, test modules, integration test modules, and doc tests, compiled only by a test build. | keep | name the language itself spells |
| test grant | cli/command-line.md#r-cli.test.env.grant | The capability grant of an integration test case or a doc test, built from `[test.capabilities]`, the flags of `hd test`, and fixed file system entries. | keep | standard tooling or process vocabulary |
| test module | lang/10-modules.md#test-modules | A module whose file name ends in `_test.hd`. | keep | standard PL or domain vocabulary |
| test position | lang/10-modules.md#r-module.testing.test-position | The top level of a `tests:` block, a test module, or an integration test module, where test-case calls go. | keep | name the language itself spells |
| test program | cli/command-line.md#r-cli.test.program | A **test program** is a program that `hd test` builds to run test cases: one per module with unit test cases or doc tests, and one per integration test module. | keep | standard tooling or process vocabulary |
| test registration function | lang/10-modules.md#r-module.testing.position-statements | `it`, `it_each`, `it_prop`, or `it_prop_with`; only a direct call of one may stand in test position. | keep | standard tooling or process vocabulary |
| the same compiled program | lang/11-requirements-and-suspension.md#r-req.determinism.same-program | Two builds are **the same compiled program** when the compiler's outputs for them are byte-identical. | keep | ordinary English — duplicate of `same compiled program` above; glossary hygiene note, not a term verdict |
| totally denied | cli/command-line.md#total-deny | A trait whose grant is `false`; a module that imports it never starts. | keep | standard tooling or process vocabulary |
| trait candidates | lang/03-names-and-scopes.md#r-names.method-lookup.trait-candidates | The trait methods of the receiver's type with the called name whose trait is available at the call. | keep | standard PL or domain vocabulary |
| trait methods | lang/03-names-and-scopes.md#r-names.member.trait-methods | The methods of every trait that a known implementation implements for a type. | keep | standard PL or domain vocabulary |
| trait-less derivation block | lang/14-annotations.md#trait-less-derivation-blocks | An `impl X by Structure:` without a trait, whose member lines write shared metadata of `X` for every derivation. | keep | ordinary English |
| tree hash | cli/command-line.md#r-cli.sum.hash | The `h1:` hash of a dependency version's files, which `hd.sum` records. | keep | standard tooling or process vocabulary |
| tree line | cli/command-line.md#r-cli.sum.line | An `hd.sum` line that records the tree hash of a selected version. | keep | standard tooling or process vocabulary |
| trivial pattern | lang/14-annotations.md#r-annot.typed-fact.pattern.trivial | A pattern that is one of those parameters alone, as `T` in `@annotate::T`, is a **trivial pattern**. | keep | standard PL or domain vocabulary |
| tuple template | lang/14-annotations.md#tuple-templates | A trait's derivation for every tuple type, written `impl[T < Tuple] Trait for T by Structure:` in the trait's module. | keep | standard PL or domain vocabulary |
| type forms | lang/04-type-system.md#type-forms | The kinds of type that hd-lang has, such as primitive types, tuples, optional types, and function types. | keep | ordinary English |
| type-argument default | lang/04-type-system.md#type-argument-defaults | A type written with `=` after a generic parameter's bound, used when a use site leaves the parameter unsolved or a written type omits it. | keep | ordinary English |
| type-argument marker | lang/02-grammar.md#r-grammar.expr.type-arguments.marker | The `::` before an explicit type-argument list in an expression, as in `first::string`. | keep | ordinary English |
| typed derivation | lang/14-annotations.md#typed-derivation | Implementing a trait for a data type or enum from its members through the trait's template. | keep | standard PL or domain vocabulary |
| typed fact type | lang/14-annotations.md#r-annot.typed-fact.declare | A fact type whose `@annotate` decorator writes a type argument, a pattern such as `T` in `@annotate::T` or `fn(T) -> R`. The pattern's parameters are inferred from a target's type as a call's are, and a value on that target checks against the fact type at them. | keep | ordinary English |
| unbound method reference | lang/07-functions.md#r-fn.ref.unbound | `Owner::name` without an argument clause, where `Owner` names a type, a trait, or a type parameter. | keep | standard PL or domain vocabulary |
| unit pattern | lang/02-grammar.md#r-grammar.pattern.unit | `()` is the **unit pattern**. | keep | standard PL or domain vocabulary |
| unit test case | lang/10-modules.md#r-module.testing.unit-row.anywhere | A test case in a `tests:` block or a test module, wherever its file lies, whose body gets `TestRunner` alone from the runner. | keep | ordinary English |
| untyped fact type | lang/14-annotations.md#r-annot.typed-fact.untyped | A fact type whose `annotate` type argument is the default `Any`, as in `@annotate(.Field)`, so its values stay unchecked. | keep | ordinary English |
| value expression | lang/05-expressions.md#r-expr.category.value | An expression that produces a value. | keep | standard PL or domain vocabulary |
| value names | lang/03-names-and-scopes.md#r-names.category.value | The name category of top-level executable bindings, parameters, local bindings, local named functions, loop bindings, pattern bindings, and captured values. | keep | standard PL or domain vocabulary |
| vararg | lang/07-functions.md#r-fn.vararg.form | A final parameter written `name...: T`, which collects the call's remaining positional arguments into `T`. | keep | standard PL or domain vocabulary — C vocabulary for variadic arguments |
| visible | lang/03-names-and-scopes.md#r-names.visible.field-method | A field or inherent method is visible from a module that declares it, and from every module when it is `pub`. | keep | ordinary English |
| workspace | lang/10-modules.md#workspaces | A set of packages that one committed workspace manifest lists, selected as one graph. | keep | standard PL or domain vocabulary — package-manager vocabulary (Cargo) for a package set |
| workspace mode | cli/command-line.md#workspace-mode | How a command works at a workspace root, where the nearest `hd.toml` lists members and declares no package. | keep | ordinary English |
| written-out size | std/regex.md#r-std-regex.time.size | The **written-out size** of a pattern is given by the table below, where s is the size of the repeated item. | keep | ordinary English |
