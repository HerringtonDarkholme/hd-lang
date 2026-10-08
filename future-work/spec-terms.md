# Spec Terminology Inventory (S11b, 2026-10-08)

Owner direction: don't invent unnecessary new words. Redo of the S11
table under a stricter test: keep only with evidence (a named language
or source and the page where the term means the same thing, a keyword
hd spells, or plain English used in its everyday sense with the sense
stated). Source: `pnpm run spec glossary --json` (186 terms), plus a
repeated-noun sweep of `spec/lang/`, `spec/std/`, `spec/cli/`,
`spec/README.md` and `spec/STYLE.md` (`fuel`, `slot`,
`witness`, `realm`, `ledger`, `drift` and kin: all benign).

## Owner rows

None: every hd-only concept below keeps an established or plain word.
(`serialization opt-in` stays per the S10 owner decision.)

## Verdicts

**keep 186, replace 1, owner 0.** The 186 glossary rows all keep; the one replacement is `ghost
entry` → `stale entry` (added as a manual row below: it is not bold in
the chapter, so `spec glossary` misses it). Everyday
"ghost" means a haunting, which says nothing about an unreachable map
entry, and the verification sense (Dafny ghost state, erased at
runtime) means something related but different. Stale is the cache
literature's word for present-but-no-longer-valid. Glossary hygiene
note (not a term verdict): `same compiled program` and `the same
compiled program` duplicate one rule and should merge when that
section is next touched (S12's lane).

| Term | Defined in | Meaning in plain words | Verdict | Evidence / replacement |
| --- | --- | --- | --- | --- |
| active | lang/11-requirements-and-suspension.md#r-req.bang.active | A driver context while its executor is evaluating or polling it on the current program-instance call stack. | keep | P: currently operating |
| available | lang/03-names-and-scopes.md#r-names.visible.trait | A trait method is available in a module when its trait is available to dot-call lookup there. | keep | P: at hand for use |
| bare step | lang/05-expressions.md#r-expr.pipe.step-kinds | A pipe step that is a name or path without `_`. | keep | P: unadorned stage of a pipeline |
| base seed | cli/command-line.md#r-cli.test.seed | `hd test` derives a property test case's **base seed**, the `seed` of its first case, from the test case's name. | keep | P: the starting seed a run derives from |
| base tag | cli/command-line.md#r-cli.dep.pseudo.base | A pseudo-version's **base tag** is the tag its form names, by the pseudo-version table, with the package's tag prefix. | keep | P: the tag a form names |
| bound method reference | lang/07-functions.md#r-fn.ref.bound | `value::name`, where `value` names a value, as a function value. | keep | Python docs, bound methods: a function tied to its receiver |
| bound requirement key | lang/11-requirements-and-suspension.md#bound-requirement-keys | A requirement key that binds associated types, such as `Store[Item = User]`; its provider value has the trait value type `dyn Store[Item = User]`. | keep | P: a key carrying its bindings |
| bound-only parameter | lang/04-type-system.md#r-types.generic.infer.bound | A type parameter of a call that no parameter type names but a bound of another type parameter does; inference solves it from that bound. | keep | P: named only in a bound |
| build directory | cli/command-line.md#r-cli.build.directory | The directory `build` in a package directory, where `hd` writes its build and cache output, such as `build/debug/NAME.wasm`. | keep | P: the directory a build writes to |
| build profiles | lang/04-type-system.md#integer-arithmetic | A program is built under one of three **build profiles**: debug, release, or test. | keep | Cargo book, Profiles: named build configurations |
| cache directory | cli/command-line.md#cache | The one directory per user where `hd` keeps every fetched dependency version, read-only. | keep | P: the directory a cache lives in |
| call place | lang/05-expressions.md#callable-values | A call `v()` whose callee's type implements `Update`, so `v() = x` and `v() op= x` store through it. | keep | R: Place Expressions — an addressable location |
| callable value | lang/05-expressions.md#callable-values | A value whose type implements `Apply`, read by calling it with no arguments, as in `count()`. | keep | Python docs, callable objects: a value used as a function |
| capability grant | cli/command-line.md#capability-grants | What a program's host capability traits may touch at run time: for each trait, no limit, a total deny, or a list of scope entries. | keep | object-capability literature (Dennis–Van Horn): what a capability permits |
| coherence slot | lang/14-annotations.md#terminology | One `(trait, concrete target)` pair over the resolved package graph. | keep | P: a position in a table |
| collect target | std/iter.md#collect-targets | The collection that `collect` builds, named by the expected type. | keep | P: what an operation builds toward |
| compatibility line | lang/10-modules.md#r-module.version.line | The versions of a package that must stay compatible: one major number, or `0.MINOR` below 1.0. | keep | P: a line of mutually compatible versions |
| compiled entries | cli/command-line.md#r-cli.cache.obj | `hd` stores its **compiled entries** in the directory `obj` of the cache directory. | keep | P: entries already compiled |
| compound assignment | lang/05-expressions.md#compound-assignment | A statement `place op= value`, such as `total += x`, that combines an operator with a store. | keep | C standard, assignment operators: `op=` forms |
| conflict | lang/03-names-and-scopes.md#r-names.conflict.definition | Two or more members with one name at the smallest depth where that name occurs, including one member reached through two paths. | keep | P: two claims that cannot both stand |
| copy-update literal | lang/08-data-and-enums.md#copy-update-literals | A data literal with one leading spread, which builds a new value from an existing one. | keep | P: a literal copying a value with updates |
| data type | lang/08-data-and-enums.md#r-data.kind.data | A nominal product type with reference semantics. | keep | Haskell report, algebraic data types |
| Debug builders | std/format.md#debug-builders | The `DebugWriter` methods that describe a value as a struct, tuple, list, or map. | keep | GoF Builder pattern: piecemeal construction |
| declarations | lang/02-grammar.md#declarations | A declaration is an optionally public named declaration or an implementation: | keep | P: named program entities |
| default executable | cli/command-line.md#r-cli.exe.default-main | With no `[[executable]]` table in `hd.toml`, `src/main.hd` is the package's **default executable**, as Cargo's `src/main.rs` is. | keep | P: the executable used when none is named |
| default profile | cli/command-line.md#host-capabilities | `hd FILE`, `hd run`, a task, and the REPL use the **default profile**, which binds these traits: | keep | P: the profile used when none is named |
| default type | lang/04-type-system.md#r-types.literal.local.default | The type a literal class takes when it meets no type: `i32` for integers when a member is a signed literal, `usize` otherwise, and `f64` for floating-point literals. | keep | P: the type used when none is pinned down |
| dependency requirement | lang/10-modules.md#dependency-requirements | A manifest entry `PATH@VERSION` that maps a dependency key to a host path and a minimum version. | keep | Cargo book, Dependencies: a named dependency need |
| depth | lang/03-names-and-scopes.md#r-names.part.depth | The number of embedded fields on a part's path. | keep | P: how deep nesting goes |
| derivation block | lang/14-annotations.md#derivation-blocks | An `impl Trait for X by Structure:` that applies a trait's template to one type, with optional member lines. | keep | R: derive macros, plus P: a block of declarations |
| derived variance | lang/04-type-system.md#r-types.variance.target.derived | The variance an inherent `impl` parameter has in the implementation's target type, by which its methods are checked. | keep | TAPL §19 (subtyping): variance of constructors |
| dev dependency | lang/10-modules.md#r-module.test.dev-dependency | A dependency that the manifest declares in `[dev-dependencies]`, which test code and tasks may use and a dependent never sees. | keep | Cargo book, dev-dependencies |
| doc test | lang/10-modules.md#r-module.test.doc.block | A fenced `hd` block in a documentation comment of a module under the source root, compiled as its own program with one test case. | keep | rustdoc book: tests in documentation |
| draw budget | std/testing.md#r-std-testing.budget | The per-case limit on draws from `Choices`; once it is spent, every draw returns its simplest value. | keep | H docs, data.draw(): values drawn from strategies; budget is P: an allowance |
| driver context | lang/11-requirements-and-suspension.md#r-req.bang.driver-contexts | Where a bang call is valid: a suspending function or closure body, or the host executor driving `main!`. | keep | P: the context doing the driving |
| dynamic provider | lang/11-requirements-and-suspension.md#lexical-and-dynamic-providers | A provider a closure gets at each call, because its row keeps the key. | keep | Guice docs, Providers: values supplying a capability; dynamic is P |
| embedded field | lang/08-data-and-enums.md#data-embedding | A bare type-name member of a data declaration, which embeds another data type. | keep | P: a field embedded in another type |
| entry module | lang/10-modules.md#r-module.init.entry-module.selected | The module that a program starts from: a module that the toolchain selects in a package, or the file of a single-file program. | keep | P: the module a run enters through |
| enum | lang/08-data-and-enums.md#r-data.kind.enum | A nominal sum type. | keep | Haskell report / Rust reference: enumerated variants |
| error derivation | lang/14-annotations.md#error-derivation | Implementing `Display`, `Error`, and `From` for an error type from its `@error` lines. | keep | P: deriving an error implementation |
| error type | lang/14-annotations.md#r-annot.error.type | An enum with a bare `@error` line, or a data type with an `@error("...")` or `@error(transparent)` line. | keep | P: the type of an error value |
| executable | cli/command-line.md#r-cli.exe.table | A program a package ships, declared by an `[[executable]]` table of `hd.toml`, or `src/main.hd` when the manifest declares none. See `cli.exe.table` and `cli.exe.default-main`. | keep | P: a file that can be executed |
| executable entry point | lang/10-modules.md#r-module.entry.definition | A public top-level function named `main` or `main!` with no parameters. | keep | P: where execution enters (C `main`) |
| exhausted | lang/06-control-flow.md#r-flow.for.iterator-exhausted | An iterator whose `next` has returned `.None`. | keep | P: emptied of items |
| exhausted iterator | lang/06-control-flow.md#r-flow.for.iterator-exhausted | An iterator whose `next` has returned `.None`. What a later `next` returns is unspecified. | keep | P: an iterator with no items left |
| fact | lang/14-annotations.md#facts | An ordinary value attached to a type, member, or variant for derivations to read. | keep | Datalog literature: an asserted proposition (vs a rule) |
| field lookup | lang/03-names-and-scopes.md#field-lookup | The steps that resolve `x.name` to one field from a module. | keep | P: finding a field by name |
| field shorthand | lang/02-grammar.md#r-grammar.primary.field-shorthand | A `data_field_item` that is a bare `identifier` is **field shorthand**: `x` means `x: x`, as in `Point { x, y }`. | keep | P: abbreviated field syntax |
| fields | lang/03-names-and-scopes.md#r-names.member.fields | Its **fields**, including embedded fields named by their embedded type name, are found by field lookup. | keep | P: named members holding data |
| fingerprint | cli/command-line.md#r-cli.test.fingerprint | A test program's **fingerprint** changes whenever a change could change its outcome: a source file it reads, a dependency, a manifest setting, the `hd` version, or a test option such as `--seed`. | keep | Git docs, object hashes; OpenSSH key fingerprints: a content hash |
| fits | lang/09-traits.md#r-trait.resolve.fits | A candidate implementation fits a call when the call's arguments check against its method's parameter types. | keep | P: be the right shape for |
| fixed elements | lang/04-type-system.md#r-types.tuple.rest.form | The elements of a tuple type other than its rest element. | keep | P: elements fixed in place |
| flag | std/cli.md#r-std-cli.cli.flag | A command-line argument that `std.cli` reads as set or not set; it takes no value. | keep | POSIX Utility Conventions: command-line flags |
| folder | lang/10-modules.md#r-module.folder.holder | The directory that holds a source file, or for a file `x.hd` with child modules, the directory `x/` that holds them; nested directories are separate folders. | keep | P: a directory of sources |
| folder graph | lang/10-modules.md#r-module.cycle.folder-edge | A package's folders, with an edge where a file in one folder uses a module in another. It must be acyclic. | keep | P: folders as graph nodes |
| generic field | lang/04-type-system.md#r-types.path.field.generic | A field whose declared type is a generic parameter; reading it yields the substituted type unchanged. | keep | P: a field of generic type |
| handle | lang/14-annotations.md#handles | A compiler-generated constant naming one member (`Field[S, F]`) or variant (`Variant[S]`) of a derivation's target. | keep | POSIX file handles: an opaque reference |
| hidden item | lang/10-modules.md#r-module.interface.hidden-item | An item in a package interface that only the code of an instantiated template may name. | keep | P: an item kept out of view |
| hides | lang/03-names-and-scopes.md#r-names.hide.depth | A member hides every member with the same name at a greater depth, in the same namespace. | keep | P: keeps out of view |
| infinite loop | lang/06-control-flow.md#r-flow.while.infinite | A `while` loop whose condition is the literal `true`. It completes normally only through a `break` that targets it. | keep | P: a loop that never ends |
| inherent associated function | lang/09-traits.md#inherent-members | A member of an inherent implementation without a `self` parameter, called through the type, as in `User::guest()`. | keep | R: inherent implementations |
| inherent method | lang/09-traits.md#inherent-members | A member of an inherent implementation whose first parameter is `self` or `mut self`, called with dot syntax. | keep | R: inherent implementations |
| initialization group | lang/10-modules.md#r-module.init.group | A strongly connected component of the use graph: one module, or modules that use each other in a loop, initialized together. | keep | P: units initialized together |
| inspectable types | lang/09-traits.md#inspectable-types | The types for which the compiler supplies `Inspectable`: primitives, module-level declarations, collections and tuples of inspectable types, and matching dynamic values. | keep | P: types that can be inspected |
| integration test module | lang/10-modules.md#r-module.test.integration | A module under the package's test root, which sees the package as a dependent does. | keep | Cargo book, integration tests |
| integration test program | lang/10-modules.md#r-module.test.integration.program | A file directly under the test root, compiled as its own program. | keep | Cargo book, integration tests |
| intrinsic method | lang/09-traits.md#intrinsic-methods | An implementation method written after `@intrinsic`, with no body, whose body the compiler supplies. Only the standard library declares one, for a primitive operation such as integer addition. | keep | R: intrinsics — compiler-known operations |
| irrefutable | lang/06-control-flow.md#r-flow.let.irrefutable | A pattern that alone covers its initializer's type, so a `let` with it needs no `else`. Any other pattern is refutable. | keep | R reference, Refutability: a pattern that cannot fail |
| iterable | lang/06-control-flow.md#r-flow.for.accepts | A value whose type implements `Iterable[T]`, such as a `List`, a `Map`, or a range `a..b`. An `Iterator` is not iterable, though `for` takes a mutable one directly. | keep | GoF Iterator pattern; R Iterator trait |
| iterator | lang/06-control-flow.md#r-flow.for.iterator-type | A value of the prelude type `Iterator[T]`. It stores one traversal's progress and is single-pass: a second traversal calls `iter()` on the source again. | keep | GoF Iterator pattern; R Iterator trait |
| iterator adapters | std/iter.md#iterator-adapters | Methods of the prelude `Iterator[T]` that wrap an iterator in a new one, or drain it. | keep | R Iterator trait: adapter methods |
| known implementation | lang/03-names-and-scopes.md#r-names.member.known-impl | An implementation in the program's dependency graph whose target matches a type; a local one counts only where its methods are available. | keep | P: an implementation already known |
| law partners | lang/09-traits.md#law-partners | Comparison and hash traits whose laws relate them, such as `Hash` and `Eq`. | keep | P: a pair engaged together (traits whose laws relate them) |
| let-else | lang/02-grammar.md#let-else-statements | A `let` statement with a refutable pattern and an `else` block, which runs when the pattern does not match and must diverge. | keep | R, let-else statements (RFC 3137) |
| lexical provider | lang/11-requirements-and-suspension.md#lexical-and-dynamic-providers | A provider a closure fixes where it is written, by capturing the value of `$.use`. | keep | Guice docs, Providers; lexical is PL scoping vocabulary |
| literal class | lang/04-type-system.md#r-types.literal.local.class | Unsuffixed literals with no expected type that meet one another; they take one width. | keep | P: a class of literal spellings |
| literal function | lang/05-expressions.md#literal-suffixes | A function marked `@num_suffix` or `@str_prefix`, which a suffixed literal or prefixed string calls. | keep | P: a function named by a literal |
| literal suffix | lang/01-lexical-structure.md#literal-suffixes | A name written directly after a numeric literal's digits, which names a suffix function. | keep | P: letters appended to a literal |
| local type names | lang/03-names-and-scopes.md#r-names.category.local-type | The name category of data types, enums, traits, aliases, and newtypes declared inside an executable suite. | keep | P: type names visible locally |
| manifest hash | cli/command-line.md#r-cli.sum.manifest-hash | The tree hash of a tree that holds only a dependency version's `hd.toml`. | keep | P: the hash recorded in a manifest |
| manifest line | cli/command-line.md#r-cli.sum.manifest-line | An `hd.sum` line that records the manifest hash of a version that selection reads. | keep | P: one line of a manifest |
| member line | lang/14-annotations.md#member-lines | A line of a derivation block that edits one member's facts or omits it, or a line of a trait-less derivation block that edits its metadata. | keep | P: one line configuring a member |
| member metadata | lang/14-annotations.md#terminology | The ordered list of values attached to a data field, an enum variant, or a parameter. | keep | P: data about a member |
| member names | lang/03-names-and-scopes.md#r-names.category.member | The name category of data fields, embedded fields, methods, enum variants, and tuple fields, within the namespace of their owning type. | keep | P: the names of members |
| method lookup | lang/03-names-and-scopes.md#method-lookup | The steps that resolve `x.name(args)` to an own inherent method or a candidate. | keep | P: finding a method by name |
| method reference | lang/07-functions.md#method-references | A method or associated function named as a function value, written `Owner::name` or `value::name` without arguments. | keep | Java Language Spec §15.13: `Type::name` references |
| methods | lang/03-names-and-scopes.md#r-names.member.methods | Its **methods** are found by method lookup. | keep | P: functions of a type |
| minimal version selection | lang/10-modules.md#version-selection | Choosing, for each host path and compatibility line, the largest minimum that any reached manifest states. | keep | P: choosing the smallest acceptable versions |
| module names | lang/03-names-and-scopes.md#r-names.category.module | The name category of top-level types, traits, functions, and names introduced by use declarations. | keep | P: the names of modules |
| mutable access | lang/04-type-system.md#mutation-checks | What an expression has when its type is `mut U`, its type is a parameter bounded by `mut Trait` or `mut Any`, or it is `self` in a `mut self` method. | keep | P: access permitting mutation |
| mutable edge | lang/04-type-system.md#r-types.path.field.mutable-edge | A direct field declared `field: mut U`; a readonly container removes its `mut`. | keep | graph theory: a connection marked mutable |
| mutable edges | lang/08-data-and-enums.md#mutable-edges | What a data type has when it, or a type it embeds at any depth, declares a direct `field: mut U`. | keep | graph theory: connections marked mutable |
| mutable requirement trait | lang/11-requirements-and-suspension.md#r-req.mut.trait | A trait that declares or inherits a `mut self` method; its providers always have mutable access. | keep | P: a requirement trait needing mutable access |
| non-reassignable | lang/04-type-system.md#r-types.view.non-reassignable | A binding whose name cannot be rebound. | keep | P: not permitted to reassign |
| operator trait | lang/05-expressions.md#operator-traits | A `std.ops` trait, such as `Add[Rhs = Self]`, whose implementation gives a type one operator. | keep | P: a trait for operator syntax |
| option | std/cli.md#r-std-cli.cli.option | A command-line argument that `std.cli` reads with one value. | keep | POSIX Utility Conventions: command options |
| package mode | cli/command-line.md#r-cli.mode.package.nearest | How a command works when the nearest `hd.toml` at or above its start directory declares a package. | keep | P: operating on a package |
| pad bits | std/encoding.md#r-std-encoding.base64.encode.pad-bits | The low bits of the last base64 symbol that encode no byte; they are zero. | keep | RFC 4648 (base64): padding bits |
| padding | std/encoding.md#r-std-encoding.base64.decode.padding | The one or two `=` characters that end a base64 text. | keep | RFC 4648 (base64): `=` padding |
| part | lang/08-data-and-enums.md#parts-and-copies | The value an embedded field holds: the outer value's own copy of a value of the embedded type. | keep | P: a piece of a whole |
| partially denied | cli/command-line.md#partial-deny | A trait whose grant is a list of scope entries; a call outside them returns a `NotGranted` error. | keep | P: refused in part |
| path requirement | lang/10-modules.md#r-module.path-dep.form | A manifest value `{ path = "DIR" }` through which a package depends on the local package in `DIR`. | keep | Cargo book, path dependencies |
| pattern | lang/14-annotations.md#r-annot.typed-fact.pattern | That type argument is the fact type's **pattern**, a type over the fact type's own type parameters, such as `T`, `List[T]`, or `fn(T) -> R`. | keep | Haskell report, pattern matching |
| pipe expression | lang/05-expressions.md#pipe-expressions | `value \\|> step`, which passes a value to a step. | keep | Unix pipes; F# pipeline operator: stages joined by \|> |
| pipeline | cli/command-line.md#pipelines | A **pipeline** is the way an implementation generates code for a build. | keep | P: stages in sequence (Unix pipes, CI pipelines) |
| place expression | lang/05-expressions.md#r-expr.category.place | An expression that identifies a storage location, which may be read or, when permissions allow, assigned. | keep | R: Place Expressions |
| positional | std/cli.md#r-std-cli.cli.positional | A command-line argument that is not a flag or an option; `std.cli` reads it by position. | keep | POSIX Utility Conventions: positional operands |
| positional spread | lang/05-expressions.md#positional-spreads | An argument `x...` that passes the value `x` in place of separate arguments: a tuple fills the callee's remaining inputs, and a vararg takes a value of its own type. | keep | P: spreading positional arguments |
| prefix function | lang/05-expressions.md#r-expr.literal-fn.marker | A literal function marked `@str_prefix`, which a prefixed string calls. | keep | P: a function named by a prefix |
| prefixed string | lang/01-lexical-structure.md#r-lex.prefix.form | An identifier followed directly by `"` or `"""`, as in `sql"..."`. | keep | P: a string with a prefix |
| prelude | lang/10-modules.md#prelude | The implicit scope of public standard-library names that every module has. | keep | R: the Prelude chapter — implicitly imported names |
| primitive types | lang/04-type-system.md#primitive-types | `bool`, the integer types `i8` to `i64` and `u8` to `u64`, `f32`, `f64`, `char`, and `string`. | keep | TAPL §11: base types |
| program instance | lang/10-modules.md#r-module.init.program-instance | One instantiated Wasm module graph with its module storage, provider bindings, and execution state. | keep | WebAssembly spec: an instantiated module |
| promoted candidate | lang/03-names-and-scopes.md#r-names.method-lookup.promoted-candidate | Among the promoted inherent methods that take part, the one with the called name at the smallest depth. | keep | P: a candidate raised in rank |
| promoted member | lang/03-names-and-scopes.md#r-names.promote.member | A `pub` field or `pub` inherent method of a part's type, reached from the outer type through the part's path. | keep | P: a member raised in rank |
| property test | std/testing.md#property-tests | A test case whose body runs on inputs drawn from a `Choices` source. | keep | Q: properties checked over random inputs |
| pseudo-version | lang/10-modules.md#r-module.version.pseudo | A version that names one untagged commit by a base version, its time, and its hash. | keep | Go Modules Reference, Pseudo-versions |
| range pattern | lang/02-grammar.md#r-grammar.pattern.range | A **range pattern** takes one of five forms: `a..=b`, `a..b`, `a..`, `..=b`, or `..b`. | keep | R reference, Range patterns |
| readonly edge | lang/04-type-system.md#r-types.path.field.readonly-edge | A field declared `field: U` with a composite `U`, which gives readonly access through any container. | keep | P: a connection permitting only reads |
| readonly view | lang/04-type-system.md#r-types.view.term | The `T` access to a composite value; it does not imply deep immutability. | keep | P: a read-only perspective on a value |
| refutable | lang/06-control-flow.md#r-flow.let.irrefutable | A pattern that may fail to match its initializer, such as `.Some(v)`; a `let` with one needs an `else` block. | keep | R reference, Refutability: a pattern that can fail |
| requirement row | lang/11-requirements-and-suspension.md#r-req.row.definition | The normalized unordered set of requirement keys on a callable signature. | keep | Rémy 1989 (Quelques objets...), row polymorphism |
| requirement-free | lang/07-functions.md#r-fn.default.requirement-free | A default expression that uses no provider and does not suspend. | keep | P: needing no requirements |
| rest element | lang/04-type-system.md#rest-elements | A last tuple element `List[T]...`, which stands for any number of trailing `T` values. | keep | ECMAScript, rest elements: leftovers collected |
| rest member | lang/14-annotations.md#r-annot.tuple.rest | The one `List[T]` member that a rest tuple's `Structure` has for its rest element. | keep | ECMAScript, rest elements |
| root file | lang/10-modules.md#r-module.relative.root-file | `src/lib.hd`, `src/main.hd`, or an integration test program, whose relative lookup starts at its root and which has no `super`. | keep | P: the file a tree grows from |
| row alias | lang/11-requirements-and-suspension.md#row-aliases | A transparent alias that names a set of requirement keys. | keep | Rémy 1989, row variables |
| row parameter | lang/11-requirements-and-suspension.md#r-req.row.parameter | A generic parameter whose values are requirement rows. | keep | Rémy 1989, row variables |
| row slot | lang/11-requirements-and-suspension.md#r-req.row.slot | A place in a type that takes a requirement row, written after `$`: `$.Context[...]`, the row argument of `Fn`, and a type argument for a row parameter. | keep | Rémy 1989, rows; slot is P: a position |
| rule ID | STYLE.md#rule-ids | A stable dotted name for one normative rule. | keep | P: an identifier for a rule |
| runtime identity | lang/09-traits.md#r-trait.identity.definition | Two types share it when they are the same declaration applied to type arguments with the same runtime identity. | keep | C++ RTTI (`typeid`): a type's runtime identity |
| runtime profile | lang/10-modules.md#r-module.profile.definition | A named compile-time set of host capability traits, their boundary adapters, and runtime choices such as panic exit statuses. | keep | P: a named set of run options |
| safe | cli/command-line.md#r-cli.fix.safe | A fix-it is **safe** when applying it adds no syntax error and no new diagnostic. | keep | P: free from harm |
| same compiled program | lang/11-requirements-and-suspension.md#r-req.determinism.same-program | Two builds whose compiler outputs are byte-identical. | keep | P: the identical build artifact (dup note below) |
| scalar boundary | lang/04-type-system.md#r-types.string.boundary | A byte offset of a string, from `0` to its length, that does not fall inside a scalar value's encoding. | keep | Unicode Standard, scalar values: a boundary between them |
| script | lang/10-modules.md#r-module.init.script | An entry module with no `main`, whose top-level executable statements are the entry behavior. | keep | P: a program run as written (scripting) |
| sealed trait | lang/09-traits.md#sealed-traits | A standard trait whose implementations only the compiler and the standard library supply. | keep | Scala book, sealed classes: a closed family |
| self reference | lang/14-annotations.md#self-references | A member's or variant's `self_ref`: whether its type needs the type being derived (`.Required`), only refers to it (`.Optional`), or neither (`.Absent`), computed from its type alone. | keep | P: referring to itself |
| serialization opt-in | lang/14-annotations.md#serialization | A type's implementation of `std.serde.Serialize` or `std.serde.Deserialize`, one permission for every format to write or build its values, private members included. | keep | owner-blessed in S10: say what it means |
| shape | lang/04-type-system.md#shapes-and-generic-code | In generic code, the machine representation a value occupies. | keep | V8 blog, object shapes: a value's memory layout |
| shared task module | cli/command-line.md#r-cli.task.shared | A module in a subdirectory of `tasks`, such as `tasks/shared/zip.hd`, is a **shared task module**. | keep | P: a task module used in common |
| shared test module | lang/10-modules.md#r-module.test.integration.shared | A module in a subdirectory of the test root, which every integration test program may use. | keep | P: a test module used in common |
| signed literal | lang/04-type-system.md#r-types.literal.local.signed | An integer literal written directly after unary `-` or `+`, as in `-1` or `+5`. | keep | P: a literal with a sign |
| single-file program | lang/10-modules.md#single-file-programs | One source file compiled with no package, which may use only `std`. | keep | P: a program of one file |
| spread pattern | lang/06-control-flow.md#spread-patterns | A last tuple-pattern element, a name or `_` followed by `...`, that matches a rest element's list, as in `let (a, xs...) = t`. | keep | ECMAScript, spread elements |
| substitution step | lang/05-expressions.md#r-expr.pipe.step-kinds | A pipe step that contains `_`. | keep | P: a pipeline stage doing substitution |
| suffix function | lang/05-expressions.md#r-expr.literal-fn.marker | A literal function marked `@num_suffix`, which a suffixed literal calls. | keep | P: a function named by a suffix |
| suffixed literal | lang/05-expressions.md#literal-suffixes | A numeric literal with a literal suffix, such as `250ms`, which calls the suffix function, as `ms(250)`. | keep | P: a literal with a suffix |
| summary | cli/command-line.md#r-cli.doc.summary | An item's **summary** is the first sentence of its documentation, or empty when it has none. | keep | P: a brief account |
| take part | lang/03-names-and-scopes.md#r-names.take-part.definition | The members of a type that lookup considers: its own fields and inherent methods, whatever their visibility, and its promoted members. | keep | P: participate in |
| target parameter | lang/14-annotations.md#r-annot.typed-fact.pattern.trivial | That parameter is the fact type's **target parameter**, and it is inferred as the target's whole type. | keep | P: the parameter aimed at |
| task | cli/command-line.md#tasks | A development program of a package, a file `tasks/NAME.hd` that `hd run NAME` runs and the package never ships. | keep | P: a unit of scheduled work |
| template | lang/14-annotations.md#templates | A trait's one derived implementation, written `impl[T] Trait for T by Structure:` in the trait's module. | keep | C++ standard, templates: an outline stamped per use |
| template helper | lang/10-modules.md#r-module.package.template-helper | A private item that a template body names; it follows the signature rules of a public declaration. | keep | templates (C++ standard) plus P: a helper |
| test case | lang/10-modules.md#test-cases | One test, registered by a call of the prelude function `it` in test position, by one row of an `it_each` call, or by an `it_prop` or `it_prop_with` call. | keep | P: one checked example |
| test code | lang/10-modules.md#r-module.test.code | A package's `tests:` blocks, test modules, integration test modules, and doc tests, compiled only by a test build. | keep | P: code that tests |
| test grant | cli/command-line.md#r-cli.test.env.grant | The capability grant of an integration test case or a doc test, built from `[test.capabilities]`, the flags of `hd test`, and fixed file system entries. | keep | P: the scope granted to tests |
| test module | lang/10-modules.md#test-modules | A module whose file name ends in `_test.hd`. | keep | P: a module of tests |
| test position | lang/10-modules.md#r-module.testing.test-position | The top level of a `tests:` block, a test module, or an integration test module, where test-case calls go. | keep | P: where test cases stand |
| test program | cli/command-line.md#r-cli.test.program | A **test program** is a program that `hd test` builds to run test cases: one per module with unit test cases or doc tests, and one per integration test module. | keep | P: a program running tests |
| test registration function | lang/10-modules.md#r-module.testing.position-statements | `it`, `it_each`, `it_prop`, or `it_prop_with`; only a direct call of one may stand in test position. | keep | P: a function registering a test |
| the same compiled program | lang/11-requirements-and-suspension.md#r-req.determinism.same-program | Two builds are **the same compiled program** when the compiler's outputs for them are byte-identical. | keep | P: the identical build artifact (dup note below) |
| totally denied | cli/command-line.md#total-deny | A trait whose grant is `false`; a module that imports it never starts. | keep | P: refused entirely |
| trait candidates | lang/03-names-and-scopes.md#r-names.method-lookup.trait-candidates | The trait methods of the receiver's type with the called name whose trait is available at the call. | keep | P: candidate traits |
| trait methods | lang/03-names-and-scopes.md#r-names.member.trait-methods | The methods of every trait that a known implementation implements for a type. | keep | P: a trait's functions |
| trait-less derivation block | lang/14-annotations.md#trait-less-derivation-blocks | An `impl X by Structure:` without a trait, whose member lines write shared metadata of `X` for every derivation. | keep | P: a derivation block with no trait |
| tree hash | cli/command-line.md#r-cli.sum.hash | The `h1:` hash of a dependency version's files, which `hd.sum` records. | keep | Git docs, object hashes: a tree's hash |
| tree line | cli/command-line.md#r-cli.sum.line | An `hd.sum` line that records the tree hash of a selected version. | keep | P: one line about a tree |
| trivial pattern | lang/14-annotations.md#r-annot.typed-fact.pattern.trivial | A pattern that is one of those parameters alone, as `T` in `@annotate::T`, is a **trivial pattern**. | keep | P: a pattern doing nothing |
| tuple template | lang/14-annotations.md#tuple-templates | A trait's derivation for every tuple type, written `impl[T < Tuple] Trait for T by Structure:` in the trait's module. | keep | templates (C++) for tuples |
| type forms | lang/04-type-system.md#type-forms | The kinds of type that hd-lang has, such as primitive types, tuples, optional types, and function types. | keep | P: the shapes types take |
| type-argument default | lang/04-type-system.md#type-argument-defaults | A type written with `=` after a generic parameter's bound, used when a use site leaves the parameter unsolved or a written type omits it. | keep | P: a default for a type argument |
| type-argument marker | lang/02-grammar.md#r-grammar.expr.type-arguments.marker | The `::` before an explicit type-argument list in an expression, as in `first::string`. | keep | P: a marker for type arguments |
| typed derivation | lang/14-annotations.md#typed-derivation | Implementing a trait for a data type or enum from its members through the trait's template. | keep | P: deriving with type information |
| typed fact type | lang/14-annotations.md#r-annot.typed-fact.declare | A fact type whose `@annotate` decorator writes a type argument, a pattern such as `T` in `@annotate::T` or `fn(T) -> R`. The pattern's parameters are inferred from a target's type as a call's are, and a value on that target checks against the fact type at them. | keep | P: the type of a typed fact |
| unbound method reference | lang/07-functions.md#r-fn.ref.unbound | `Owner::name` without an argument clause, where `Owner` names a type, a trait, or a type parameter. | keep | Python docs, unbound methods |
| unit pattern | lang/02-grammar.md#r-grammar.pattern.unit | `()` is the **unit pattern**. | keep | Haskell report, unit pattern `()` |
| unit test case | lang/10-modules.md#r-module.testing.unit-row.anywhere | A test case in a `tests:` block or a test module, wherever its file lies, whose body gets `TestRunner` alone from the runner. | keep | P: a test of one unit |
| untyped fact type | lang/14-annotations.md#r-annot.typed-fact.untyped | A fact type whose `annotate` type argument is the default `Any`, as in `@annotate(.Field)`, so its values stay unchecked. | keep | P: the type of an untyped fact |
| value expression | lang/05-expressions.md#r-expr.category.value | An expression that produces a value. | keep | P: an expression yielding a value |
| value names | lang/03-names-and-scopes.md#r-names.category.value | The name category of top-level executable bindings, parameters, local bindings, local named functions, loop bindings, pattern bindings, and captured values. | keep | P: the names of values |
| vararg | lang/07-functions.md#r-fn.vararg.form | A final parameter written `name...: T`, which collects the call's remaining positional arguments into `T`. | keep | C standard, `<stdarg.h>`; Java varargs |
| visible | lang/03-names-and-scopes.md#r-names.visible.field-method | A field or inherent method is visible from a module that declares it, and from every module when it is `pub`. | keep | P: able to be seen (in scope) |
| workspace | lang/10-modules.md#workspaces | A set of packages that one committed workspace manifest lists, selected as one graph. | keep | Cargo book, workspaces |
| workspace mode | cli/command-line.md#workspace-mode | How a command works at a workspace root, where the nearest `hd.toml` lists members and declares no package. | keep | Cargo book, workspaces |
| written-out size | std/regex.md#r-std-regex.time.size | The **written-out size** of a pattern is given by the table below, where s is the size of the repeated item. | keep | P: the size as written |
| ghost entry | lang/04-type-system.md#r-types.map.stale | An entry visible during iteration yet unreachable by lookup after its key mutates. Not bold in the chapter, so `spec glossary` misses it; classified here. | replace | stale entry — cache literature's word for present-but-no-longer-valid |
