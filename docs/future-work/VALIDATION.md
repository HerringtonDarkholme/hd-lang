# Validation And Checked Readers

Status: design proposal, 2026-10-25; domain: standard library API and typed readers; not accepted.

This note addresses [#846](https://github.com/HerringtonDarkholme/hd-lang/issues/846).
It follows [STDLIB](STDLIB.md#decided-not-yet-in-std), its separate Validate derivation, and reader-specific member paths.
The [review direction](https://github.com/HerringtonDarkholme/hd-lang/issues/846#issuecomment-6404976797)
requests a post-decode recommendation, preserving each reader's existing trait.
That direction changes the recommendation, not the accepted specification.
In particular, this design does **not** claim fused validation or reinterpret the recorded words “validates while reading.”

## Contents

- [Problem, users, use cases and today](#problem-users-use-cases-and-today)
- [State of the art](#state-of-the-art)
- [Semantics](#semantics)
- [Options](#options)
- [Architecture](#architecture)
- [Rule facts](#rule-facts)
- [Checked readers](#checked-readers)
- [Extension points](#extension-points)
- [Cost](#cost)
- [Stress test](#stress-test)
- [Phasing](#phasing)
- [Recommendation and approvals](#recommendation-and-approvals)
- [Sources and parse log](#sources-and-parse-log)

## Problem, users, use cases and today

Parsing a value does not establish its application's requirements.
A u32 port can decode successfully while being outside 1 through 65535.
A missing certificate can be a valid Option yet violate the completed configuration's TLS requirement.
Users need one checked input operation and a reusable validation atom for values constructed without a reader.

The owner requires Validate configuration in its own derivation block.
Document formats retain Deserialize, command lines retain Cli, and scalar environment input retains Parse.
Validation must not replace their conversion, naming, default, unknown-input or error behavior.
The additional record environment reader retains Deserialize.

| Fixed use case | End-to-end operation | Result the caller needs |
| --- | --- | --- |
| Signup request | Decode a request with names, age, addresses and password confirmation; validate; construct a reply | Typed request or stable member/index issues; no retained password values |
| Service configuration | Read port, TLS and certificate through JSON, CLI or environment; validate defaults too | The same port rule at $.port, --port or APP_PORT |
| Existing value | Construct or mutate a value, then call the validation atom | Validation independent of any wire format |
| Nested batch | Validate address/backend lists with invalid elements at several indices | First/all policy, bounded retained issues, independent path snapshots |
| Custom decoder | Use a handwritten Deserialize or Parse implementation, then check the resulting domain value | No new callbacks or reflection bounds on that implementation |
| Related fields | Read all fields before comparing confirmation, intervals or conditional requirements | Typed completed-parent checks, with no fabricated partial values |

Current std has no Validate trait, validation facts or checked validation entry points.
JSON already has an indexed text decoder; it does not need an intermediate Json tree for this feature.
Cli uses declaration and read methods over its own parser.
env_as parses one named value; env_decode reads a record through Deserialize.
See [JSON](../../spec/std/json.md#typed-json), [CLI](../../spec/std/cli.md),
[environment input](../../spec/std/host.md), and [serde](../../spec/std/serde.md).

The bounded model uses the real current Structure and Deserialize implementations.
Its configuration has an omitted leading validation member and a Deserialize-only listenPort rename.
The validation path still identifies the original port position.
This tests independence of concerns without changing std or the compiler.

## State of the art

The comparison separates parsing, semantic checks, aggregation, defaults and cross-field execution.
An API returning one Result does not establish that parsing and validation share a traversal.

### Rust validator

Rust validator derives typed validation separately from Serde and validates completed values.
Its generated code calls field validators and nested validators, then schema callbacks.
Errors form a field/record/index tree; Option absence and required checks are separate.
Schema checks normally skip after field failures, an explicit policy hd need not copy.

Custom functions are typed calls rather than registered string tags.
Some generated errors retain the rejected value, so that error representation is unsuitable as an accidental default for secrets.
hd borrows separate derivations and typed functions, while proposing value-free standard problems.
[API](https://docs.rs/validator/0.21.0/validator/),
[derive implementation](https://github.com/Keats/validator/blob/68f2e33d236f579ae7bf42c82cf2ca7986f176f6/validator_derive/src/lib.rs).

### Rust garde

Garde also checks completed typed values.
Its dive operation delegates nested validation; inner applies rules to container elements.
Conditional and custom functions cover parent-dependent requirements.
Reports store multiple path/error pairs, with separate key/index path components.

Length modes distinguish bytes, scalar values, UTF-16 units and graphemes; several helpers have optional dependency costs.
Its deserializable Unvalidated wrapper still validates afterward.
That is a useful comparison for the selected post-decode route, not evidence of fused Serde execution.
[API](https://docs.rs/garde/0.23.0/garde/),
[validation wrappers](https://github.com/jprochazk/garde/blob/c67cda4573fee55865811eefc7fb7f22a692336c/garde/src/validate.rs).

### Go validator

Go's validator caches reflection metadata and parsed string tags per type in a reusable validator instance.
Custom field and record functions extend its registry, and invalid rule definitions can panic.
Traversal continues across fields, but ordinary AND rules stop at the first failure within one field.
Container and record callbacks add further aggregation boundaries.

Alternative field-name registration helps presentation but does not provide one simultaneous JSON/CLI/environment name map.
hd instead starts from typed Structure facts and reader-owned aliases.
No global registry, reflection-tag parser or runtime plugin loading is needed.
[Package](https://pkg.go.dev/github.com/go-playground/validator/v10),
[traversal](https://github.com/go-playground/validator/blob/a5843c6fbb596822bb41c4f12bdd5fec6420dc58/validator.go).

### Zod

Zod composes schema objects with checks, refinements and explicit coercions.
Its ordinary refinements can accumulate issues, while abort and dependency conditions change which later checks run.
Async checks use separate async parsing.
The selected hd scope contains neither asynchronous validation nor normalization/coercion through validation callbacks.

Object refinements can attach issues to paths; tree formatting preserves nested structure that flattening loses.
Zod's optional compiled path can fall back to ordinary parsing, potentially repeating refinements or transformations on invalid input.
hd should not acquire that repeated-callback behavior through an optimization.
[API](https://zod.dev/api), [errors](https://zod.dev/error-formatting), [compilation](https://zod.dev/compile).

### Pydantic

Pydantic builds a core schema and delegates execution to pydantic-core.
Field, before, after and wrap validators provide different access and transformation behavior.
Field validators observing earlier siblings depend on declaration order; model validators address completed-parent relationships.
Defaults require an explicit validation setting, making them an important comparison case.

Direct JSON validation avoids an unnecessary Python object tree, while reusable adapters avoid rebuilding schema machinery.
Its FailFast setting applies to sequences, rather than promising one universal first-error policy.
hd borrows explicit phase and aggregation contracts, not a runtime schema interpreter or transformation framework.
[Validators](https://pydantic.dev/docs/validation/latest/concepts/validators/),
[defaults](https://pydantic.dev/docs/validation/latest/concepts/fields/),
[performance](https://pydantic.dev/docs/validation/latest/concepts/performance/).

## Semantics

### Types, facts and derivation views

Validate is an ordinary stdlib concern, with no compiler-known name or new syntax.
Its own Structure view supplies validation facts.
Deserialize, Cli and environment naming remain independent, following
[per-derivation Structure](../../spec/lang/14-annotations.md#r-annot.structure.per-derivation)
and [block-local configuration](../../spec/lang/14-annotations.md#r-annot.block.local).

The selected field walker uses the existing typed
[Field.fact lookup](../../spec/lang/14-annotations.md#r-annot.handle.fact.key).
It does not erase a field value or require generic F to implement Inspectable.
This distinction matters: a plain Facts.find on a generic fact type has different evidence requirements.

A compiled negative probe shows that T<Deserialize alone cannot be returned as dyn Inspectable.
The compiler reports `T does not implement Inspectable`.
Adding the explicit bound succeeds; an ordinary generic wrapper does not supply it.
Those constraints belong to the compared hook design, not the selected reader signatures.

This intentionally rejected program parses, then reports `T does not implement Inspectable`:

```text
use std.inspect.Inspectable
use std.serde.Deserialize

fn erase[T < Deserialize](value: *T) -> *dyn Inspectable:
    value
```

### Values, mutation and callbacks

Validation observes ordinary typed values.
Standard rules do not normalize the value or grant permanent validity.
References and counted mutable referents keep their existing
[shallow access](../../spec/lang/13-values-and-references.md#r-ref.read.shallow)
and [copying](../../spec/lang/13-values-and-references.md#r-ref.identity.copies) behavior.
Another alias can invalidate a previously checked value later.

User callbacks retain ordinary shallow-reference and captured-state mutation behavior.
Validation neither rolls back those effects nor upgrades a checked value into an immutable proof.

The proposed user check is a plain requirement-free function, not a bang function.
It may inspect any typed input, including one that fails another rule.
A callback receives no proof that division is safe or that sibling checks passed.
Panics propagate normally; validation does not catch a panic and relabel it as a user-input problem.

Captured owned values and counted references follow
[ordinary closure rules](../../spec/lang/07-functions.md#r-fn.capture.copy.value).
A callback must not capture a local reference into a stored fact.
The executable model uses one counted report because a traversal object cannot retain a local report reference.
That allocation is explicit cost, not a new lifetime rule.

### Defaults, absence and related fields

The selected route validates the value produced by the reader, including declared defaults and generated skipped defaults.
It does not care which Source callback constructed them.
This follows the existing [default/skip behavior](../../spec/std/serde.md#derived-implementations)
instead of trying to intercept every construction path.

None, empty text, an empty list and an absent input with a default are different final values.
Required-if is a completed-record check over those values.
Input-presence validation is not implied: after a default is applied, Validate cannot know whether the caller supplied it.
A separate reader-presence feature would require another decision.

Field-local checks run in declared order, followed by the record check.
Nested rules run at their configured field, with list elements in index order.
In All mode, completed-record checks may run after semantic field failures; they must handle any typed parent.
In First mode, a prior failure stops later checks, including the record check.

### Parsing and aggregation

The proposal collects semantic violations only after successful decoding.
With valid options, malformed input or a failed conversion returns the existing reader error first.
There is no partially valid T and no fabricated zero or default used to collect more errors.
Thus “All” means all visited semantic checks up to the cap, not every possible conversion and validation error together.

The proposed defaults are All mode and at most 64 retained violations.
The caller can request First mode or a different positive cap.
Stable order is declaration, rule-list and element order, followed by record checks.
Truncation is explicit when another issue exceeds the cap; invalid options or rule definitions produce a definition error.

## Options

### Post-decode validation inside one checked entry point

Decode T through the existing reader trait, then run Validate exactly once before returning Ok(T).
This handles handwritten decoders, defaults and skipped fields uniformly.
It adds one semantic traversal and cannot reject a semantic error before decoding finishes.
It is the smallest complete mechanism satisfying the requested checked-call and path surface.

This is the requested recommendation after the integration review.
It deliberately deviates from the original “validates while reading” wording.
That deviation and its reason require owner approval through this note.
It is not presented as fused or streaming validation.

### Cooperative checked construction

Add an explicit checked method or protocol alongside ordinary Deserialize/Cli reading.
Typed field callbacks validate after construction, then completed-record callbacks run once.
This can avoid a second traversal for participating derived readers.
Custom readers need a checked method or an atomic fallback.

Cross-derivation plans require an exact field identity and typed callback bridge.
Existing declaration positions survive omission, but compact vector offsets and display names are not identities.
Erased callbacks need explicit Inspectable evidence; TypeId lacks Hash/Ord, so a TypeId map is not an existing free facility.
Generated omitted defaults still need unseen-field completion handling.

### General reader hooks

Extend reader construction paths with enter/accept/leave hooks and a validation context.
This integrates checks with standard derived reading and container adapters.
It also changes ordinary reader/template code, adds generic bounds or another typed bridge, and cannot force handwritten implementations to cooperate.
It has the broadest compatibility and exactly-once audit burden.

| Mechanism | Signup/config/custom readers | Defaults and omitted fields | Costliest change | Main cost |
| --- | --- | --- | --- | --- |
| Post-decode | Existing reader unchanged; one checked Result | Uniform completed value | Ordinary library addition | Additional semantic traversal |
| Separate checked construction | Participating code can check earlier | Completion tracking needed | New library protocol | Bounds, plans and custom implementation contract |
| General hooks | Standard paths only until manual code cooperates | Multiple bypass paths to reconcile | Reader protocol changes | More call sites and compatibility obligations |

No option needs new language syntax or an intrinsic in the proposed scope.
The selected route introduces fewer mechanisms under the repository's design-cost order.

## Architecture

### Data structures and complete signatures

Proposed module: std.validation, implemented in ordinary hd.
The model names below are design shapes, not accepted APIs.

| Value | Fields or variants |
| --- | --- |
| MemberPath | Owned List[PathStep] |
| PathStep | Field(variant index, original position, declared name), Index(usize), Key(string) |
| Problem | Machine code and message; no automatically retained input value |
| Violation | path:MemberPath, problem:Problem |
| ValidationError | Invalid(issues:List[Violation], truncated:bool), Definition(path:MemberPath, reason:string) |
| Mode | First, All |
| Rule[F] | Typed callback plus rule-definition status and descriptive code |
| Rules[F] | A field fact containing List[Rule[F]] |
| ValidationContext | Mutable path stack, issue list, mode, cap and stop/truncation state |
| CheckedReadError[E] | Read(E), Validation(cause:ValidationError, locations:List[Location]) |
| Location | path:MemberPath, display:string; owned reader presentation, separate from semantic identity |

The proposed atom is `validate[T < Validate](value: T, mode: Mode = .All, max_errors: usize = 64) -> Result[void, ValidationError]`.
Validate supplies ordinary traversal and a default no-op record check.
The record method can be overridden in the same Validate derivation block and call plain typed functions.
All proposed validation entry points have empty requirement rows and do not suspend.
ValidationContext.reject(problem) records an issue at the current path; is_stopped() lets manual implementations honor stopping.
The context enforces its cap even if a manual implementation attempts to report another issue.

The proposed check signature is `fn(value: F) -> Result[void, Problem]`.
A typed field fact stores a list of these rule values.
Grouping them into one Rules[F] fact avoids duplicate concrete fact types in a derivation list.
It reuses existing list composition rather than adding annotation grammar.
The [checked signature catalog](validation/api-catalog.hd) spells out each bound, return type, capability row and default argument.
Its bodies are explicit unimplemented stubs, not a validation implementation.

The following excerpt is implemented by the bounded model; it is not an import from std.validation.

```text
impl Validate for Config by Structure:
    ignored = pass
    port = [rules([port_rule()])]
    fn record(self, report: *mut Report) -> void:
        rule := tls_rule()
        (rule.apply)(self, report)
```

Record checks attach to the current record path by default.
A required certificate can therefore be a record-level certificate-required problem.
Targeting a different leaf needs an explicit typed path helper; the design does not infer field identity from arbitrary callback code.
No string-based dependency registry or field-projection intrinsic is proposed.

### Data flow and core algorithm

1. Read T with its existing reader trait and existing parse-error behavior.
2. Allocate one validation context and traverse only the configured Validate view.
3. Apply typed field rules, extending the path for nested records and indices.
4. Run the completed-record method according to First/All stopping.
5. Map semantic paths through the active reader's naming context.
6. Return T only when no validation or definition error exists.

```rust
fn checked_read<T>(input, reader, options) -> Result<T, CheckedReadError> {
    options.check_definition()
        .map_err(|e| Validation(e, reader.root_locations()))?;
    let (value, names) = reader.read_with_existing_trait(input)?;
    match validate(&value, options) {
        Ok(()) => Ok(value),
        Err(error) => {
            let locations = names.locations(&error);
            Err(Validation(error, locations))
        }
    }
}
fn validate<T>(value, options) -> Result<(), ValidationError> {
    options.check_definition()?;
    let context = Context::new(options);
    value.validate_into(&context);
    context.finish()
}
fn member<F>(handle, value, context) {
    if context.stopped() { return; }
    context.push_field(handle.original_position, handle.declared_name);
    if let Some(rules) = handle.typed_fact::<Rules<F>>() {
        for rule in rules.items {
            rule.check_definition(context)?;
            if context.stopped() { break; }
            rule.apply(value, context);
        }
    }
    context.pop();
}
fn each<F>(values, rule, context) {
    for (index, value) in values.enumerate() {
        if context.stopped() { break; }
        context.push_index(index);
        rule.apply(value, context);
        context.pop();
    }
}
```

This algorithm does not add a reader method named read_with_existing_trait.
That pseudocode represents a format-owned checked helper around today's read call and its name information.
Ordinary unchecked entry points retain their behavior.
Any shared metadata change must be measured; an internal naming implementation is not automatically zero-cost for unchecked readers.
Invalid checked-call options are rejected before reading; rule definitions are checked when their configured validation visit occurs.

### Naming, caches and incremental behavior

Paths store declaration positions and variant/index distinctions before formatting.
Reader aliases do not change that identity.
The current generator assigns positions before omitting members, which the model tests using a leading omitted member.
Each checked operation scopes its path/name mapping to its target type, avoiding a global TypeId registry.

JSON must use Deserialize's rename and rename_all facts, CLI its own option metadata, and environment its actual EnvName/prefix rules.
Format-owned checked helpers can record names through existing declaration/member callbacks without changing those callback signatures.
This alias-table integration is unimplemented; its allocations and nested/default cases are a separate measured phase.
No second decode is allowed merely to discover names, since custom decoding may have observable effects.

A handwritten decoder may provide no member correspondence for its atomic input.
The fallback is the declared semantic path within that input, or an explicit application mapper.
It cannot claim a byte location or alias the reader never supplied.
Skipped fields with no input binding likewise use their declared semantic names.

The checked error owns rendered Location values alongside the unchanged semantic ValidationError.
The temporary reader/name table can then be released without losing aliases or retaining borrowed cursors.
Direct validate errors have no reader presentation until a caller supplies one.
Read(E) preserves the existing reader error, which may already contain input-dependent text.

Facts and handles retain their existing lazy evaluation behavior.
The design adds no global schema cache and does not rerun rule construction for every list element deliberately.
Whether the compiler hoists a particular fact access must be measured, not assumed.
Custom checks execute once per configured visit; nested, field and record checks are distinct visits.

## Rule facts

The fact spelling below is proposed; the checked model implements the shared typed-function mechanism only.
Atomic helpers return Rule[F] values that compose inside one Rules[F] fact.
They neither normalize input nor convert every numeric type to f64.

| Atom | Proposed behavior | Bound or unresolved detail |
| --- | --- | --- |
| range(low, high) | Inclusive typed comparison | PartialOrd; NaN fails; invalid bounds are a definition error |
| length_by(min, max, measure) | Typed custom measurement function | Core helper; no new generic length trait |
| text_length(min, max, unit) | Explicit bytes or Unicode scalar count; default Scalars | Grapheme tables excluded initially |
| list_length(min, max) | Collection element count | Uses List.len without traversing elements |
| pattern(source) | Reuse std.regex search semantics, compiled when the fact is evaluated | Anchors remain explicit in the source; invalid pattern becomes a definition error |
| one_of(values) | Typed Eq membership | Preserve integer precision; large sets need a measured lookup choice |
| check(function) | Plain typed application predicate or Result | Empty row; no automatic input retention |
| nested() | Delegate to the child's Validate | F<Validate; absent optional behavior explicit through a container helper |
| each(rule) | Apply a scalar/nested rule at each index | Preserves index paths; respects First/cap |
| required-if | Completed-record ordinary function over flags/options | Checks final values, not original input presence |

Option rules distinguish the outer optional check from checks of a present inner value.
None does not fabricate a child value to validate.
An empty string can still fail a configured nonempty/length rule.
An absent default is validated as the value that construction produced.

Rule definitions are data until their configured visit.
An invalid bound or pattern reports Definition rather than panicking through lazy fact evaluation.
Panicking user fact expressions remain ordinary fact-evaluation failures; the library cannot make arbitrary user code total.
No speculative network/email/database validator is included.

## Checked readers

### JSON decode

Proposed `decode_checked[T < Deserialize & Validate](text: string, mode: Mode = .All, max_errors: usize = 64) -> Result[T, CheckedReadError[JsonError]]`.
It uses today's indexed text decoder, then validates the completed T.
JsonError retains its existing syntax/type path and is distinct from a semantic ValidationError.
No Json tree or new deserializer bound is introduced.

JSON aliases require the format-owned name table described above.
The model demonstrates one listenPort mapping explicitly; it does not prove general alias capture.
Arbitrary custom decoder locations cannot be reconstructed from a value alone.
This limitation must be visible in documentation and tested before implementation ships.

### Options::parse_args

The proposed CLI helper is `parse_args_checked[T < Cli & Validate](mode: Mode = .All, max_errors: usize = 64) -> Result[T, CheckedReadError[CliError]] $ Args`.
It preserves Cli.declare/read, help, subcommands, required arguments and unknown-option behavior.
The checked spelling is the free `parse_args_checked::[Options]()` helper around Options::parse_args.
Adding an optional-Validate method to Cli would need another constraint mechanism or checked trait; neither is needed here.
Options remains an application type, not a compiler-recognized name.

A list-taking `parse_checked[T < Cli & Validate](arguments: List[string], ...)` remains requirement-free for tests and embedding.
CLI validation errors render through declared long/positional/subcommand bindings.
Parent errors use an options/command location rather than pretending to identify one leaf.
Existing exit/help behavior remains a CLI concern, outside pure Validate.
In particular, Read(CliError.Help) must still use CliError.exit's stdout and status0 behavior.
The proposed std.cli.exit_checked(error) adapter delegates Read errors to that existing method and renders semantic issues to stderr with status2.
Its complete signature is `exit_checked(error: CheckedReadError[CliError]) -> ExitCode $ Console`.
Returning the wrapper through generic Result termination must not silently replace those CLI-specific rules.

### env_as and record environment input

Current env_as converts through Parse and returns HostError, including Display text from parse failures.
Proposed `env_as_checked[T < Parse[Error = E] & Validate, E < Display](name: string, ...) -> Result[T?, CheckedReadError[HostError]] $ Env` preserves that contract.
An unset variable remains Ok(None); a set value is parsed once and validated once.
Its root location is the actual requested name.

Proposed `env_decode_checked[T < Deserialize & Validate](...) -> Result[T, CheckedReadError[HostError]] $ Env` handles records.
It preserves current flat environment decoding, EnvName and uppercase/default rules.
It does not depend on the undecided implementation of EnvConfig or invent nested environment spelling.
Adding nested/list validation does not make every nested type readable by today's environment format.

## Extension points

Applications compose typed Rule values and write ordinary check functions or a manual Validate implementation.
They can override the record method inside their own Validate block.
No runtime registration, string-dispatched plugin or compiler change is needed for those extensions.
Foreign types use a local mirror or an explicitly written wrapper under existing coherence rules.
Manual implementations can report at the current path; standard each/nested composition supplies container prefixes.
Arbitrary field-targeted cross-field projection is intentionally outside the initial helper set.

The standard error stores codes and messages without automatically storing rejected values.
A custom check can deliberately include sensitive text; that is ordinary application code, not a universal redaction guarantee.
Error renderers preserve structural identity while choosing human text or JSON output.
Localization can map codes externally without changing traversal.

Async or capability-using business validation remains ordinary application work after typed input.
It is not hidden inside requirement-free Structure callbacks.
Validation is observational, not a normalization, migration or authorization framework.

## Cost

The feature replaces zero existing public validation code.
Existing sources are reuse references, not deletion credits or an additive implementation estimate.
Line counts exclude blank and whole-line comments and record exact source hashes in the prototype receipts.

| Component | Existing measured reference | Replaced code | New work requiring its own budget |
| --- | --- | ---: | --- |
| Derivation | Eq template/walker20code lines; Facts lookup9; Member metadata8 | 0 | Validate traversal, typed rule collection and record callback |
| JSON integration | Indexed reader357; decode entry7 | 0 | Checked wrapper, alias capture and error mapping |
| CLI integration | Cli/template199; scalar/container readers102 | 0 | Checked result wrapper and option/subcommand names |
| Environment | env_as7; env_decode/reader98 | 0 | Scalar/record checked wrappers and EnvName mapping |
| Cross-field/reference helpers | Rust validator schema emitter41Rust lines | 0 | Typed callbacks, root/path targeting and definitions |
| Range/length/custom atoms | Rust emitters44/38/34Rust lines | 0 | hd implementations with selected units and error payloads |

The bounded hd model supplies matched component references, not a complete std implementation budget.
Its predicate, nested/list traversal, path/error accumulation and own-derivation behavior are executed evidence.
Pattern compilation, all public rule constructors, generic reader alias capture and the checked public signatures remain unbuilt.
Their estimates cannot be inferred by summing unrelated Rust emitter sizes.

The published model contains267 physical lines and220 code lines.
Its 84-line mechanism subset is separated from 76 example/rule-configuration lines, 52 test lines, 4 main-workflow lines and 4 imports.
These exact counts include annotations and declarations, excluding blank and whole-line comment lines.

| Matched model component | Code lines | 1.5× review threshold | Exclusions |
| --- | ---: | ---: | --- |
| Paths, issues and report | 28 | 42 | Full Problem/Definition family and reader rendering |
| Typed rule composition | 23 | 34 | Complete standard atom constructors and definition checking |
| Validate template and walker | 25 | 37 | Public API polish and generic container adapters |
| Atom entry and nested delegation | 8 | 12 | Format wrappers and alias capture |

The before control is 24 physical lines and 19 code lines.
Both programs decode the same JSON configuration and print 0 on Node and Wasmtime.
The control only counts handwritten failures; it does not implement equivalent structured error paths.

| Published sample | Wasm bytes | Defined functions | Code-section payload bytes |
| --- | ---: | ---: | ---: |
| Handwritten count-only control | 123,453 | 326 | 100,826 |
| Derived validation model | 143,901 | 398 | 116,174 |

The model sample adds 20,448 bytes, 72 defined functions and 15,348 code-section bytes.
This is a whole-program footprint comparison, not isolated production-library overhead or a matched diagnostic implementation.
The existing JSON reader is present in both.
Single published hd-build process observations were151.3ms for the control and170.8ms for the model.
They include compiler startup and the recorded cache setup, not an isolated per-stage compile-time estimate.

Five alternating before/model launches per engine, after one preflight each, produced these end-to-end medians:

| Engine | Handwritten control | Model |
| --- | ---: | ---: |
| Node | 55.4 ms | 60.4 ms |
| Wasmtime | 41.0 ms | 46.3 ms |

These measure the pinned CLI launching the compiled sample, including host/engine startup, JSON input and output.
They are not hot validation throughput or isolated traversal cost, and the controls have different diagnostic machinery.
Invalid-input throughput, peak memory and per-rule allocation counts remain unmeasured.

One counted report plus a path stack and bounded issue list is the model's explicit state.
Successful field traversal need not format diagnostic strings; each retained failure copies a path snapshot.
Ignoring user-check cost, traversal scales with visited fields/rules/elements; retained path space scales with issues times depth.
Nested mutation/copying and actual emitted allocations still need measurement before claiming constant allocation counts.

The selected route adds a complete semantic walk after reading.
That is its deliberate cost versus cooperative construction.
No runtime speedup or reduced parse allocation claim is made.
The alternative also needs callback plans, erased-type evidence, default completion state and compatibility work.

Each implementation slice stops for review above 1.5 times its measured component reference.
Missing integration references must be measured before assigning those slices a numeric allowance.
Compile time, valid/invalid runtime, Wasm bytes/function/code-section counts and web-compiler gzip are separate measurements.
The web compiler's2,000,000-byte gzip cap remains unchanged; no new std/compiler payload is embedded by this design-only job.
The fresh source-baseline build is 5,901,057 raw bytes and 1,972,480 gzip bytes, leaving 27,520 gzip bytes below that cap.
Its web-profile build took 14.02 seconds and its smoke test passed.
These are baseline observations, not the future validation implementation's gzip or compile-time cost.

## Stress test

The fixed surface is separate Validate configuration, typed checks and post-decode checked entry points.
The cases adapt official Rust/Go validation examples and realistic request/config workflows.
Works means an actual model assertion; proposal-only cells are not runtime evidence.

| Case | Source analogue | Result / limitation |
| --- | --- | --- |
| V1 separate names | Rust validator Serde rename | Model passes listenPort decoding with independent Validate facts |
| V2 leading omitted field | Structure per-derivation omission | Model retains port's original position1 |
| V3 first/all | Go field versus record aggregation | Model distinguishes one field error from field plus record errors |
| V4 nested addresses/backends | Go User/Address dive; garde inner | Model retains separate list index snapshots |
| V5 invalid default | Pydantic default-validation choice | Model validates an absent port's default after construction |
| V6 handwritten leaf | Serde custom deserialize | Model reads a number into Opaque unchanged, then validates it |
| V7 late certificate/confirmation | Go record validator; Zod refinements | Typed completed-parent check; no declaration-order dependency |
| V8 invalid scalar conversion | CLI numeric parser | Existing conversion error wins; no invalid typed root is manufactured |
| V9 three input spellings | Go tag-name adaptation | Model demonstrates explicit aliases; generic capture remains future |
| V10 error storm | Pydantic sequence FailFast tradeoff | Model caps retained issues and reports truncation |
| V11 captured mutation | Ordinary hd counted references | A successful check is not permanent validity; no Valid[T] promise |
| V12 divide-by-zero callback | User refinement failure | Callback must guard any typed input; panic is not a validation issue |
| V13 skipped default | Existing serde omission | Post-decode route sees completed values; hooks need extra tracking |
| V14 generic erasure | Current Inspectable evidence | Negative probe rejects Deserialize-only erasure and generic-wrapper workaround |
| V15 wrong field rule | Typed annotation applicability | A string predicate on u32 port rejects with type-mismatch before execution |

The important failures are useful constraints on the proposal.
Fused hooks cannot be a transparent wrapper over today's traits, and value-only validation cannot recover arbitrary wire aliases or original presence.
The selected route resolves default/custom-reader timing by accepting a second traversal, while pricing name mapping explicitly.
It does not erase those limitations through a convenience function name.

## Phasing

All production phases wait for owner approval of this note.
Each slice targets about an hour and must include stdlib rules and focused conformance cases for its behavior.

| Phase | Independently green result | Required focused proof |
| --- | --- | --- |
| 0 | Final error, callback, naming and post-decode policy approved | Exact public signatures and default/custom-reader examples |
| 1 | Validate atom, Rules and record method | Own facts, omission, first/all/cap, nested indices and exactly-once callbacks |
| 2a | Typed range/length/one-of/custom atoms | Wrong-field facts rejected; definitions never become input errors |
| 2b | Regex pattern and remaining container helpers | Existing regex semantics, invalid definitions and empty/optional containers |
| 3 | JSON checked entry point and alias mapper | Rename/default/skip, enum/list paths and handwritten decoder fallback |
| 4 | CLI checked parse/parse_args and exit adapter | Help stdout/status0, usage stderr/status2, semantic locations and Args behavior |
| 5 | Scalar and record environment checked entry points | Unset, parse failure, EnvName/defaults and no unrelated EnvConfig dependency |

If a slice lacks a measured integration budget, its first action is a bounded reference proof rather than an unpriced implementation.
No phase may call a post-decode walk fused or change another reader's policy to simplify the tests.
Spec language changes are unnecessary under the selected design; new ordinary APIs belong in spec/std.

## Recommendation and approvals

**Recommendation:** add the ordinary Validate atom and post-decode checked entry points, preserving existing reader traits and custom implementations.
Use one own-derivation field-rules fact, plain typed checks, and an overridable completed-record method.
Keep semantic paths independent from reader presentation, with explicit fallback where no wire correspondence exists.

The tradeoff is one additional semantic traversal and later rejection of semantic failures.
It is not fused validation and deliberately differs from the earlier “validates while reading” wording.
The owner requested that recommendation for review in6404976797; it still needs approval as the feature's final contract.

The next-best mechanism is a separate cooperative checked-construction protocol.
It should be revisited only with measured evidence that the extra traversal matters enough to pay its bound, default and manual-reader costs.
General reader hooks are the most invasive option and are not the recommended first implementation.

Approval must settle the exact names/error shapes, rule units, First/All/cap policy, record-callback behavior and name fallback.
It must also confirm the explicit post-decode deviation and record-level required-if attachment.
No new public std API, rule ID, compiler mechanism or production implementation is accepted by this document alone.

## Sources and parse log

Existing rules reviewed: [Structure](../../spec/lang/14-annotations.md#the-stdstructure-module),
[derivation blocks](../../spec/lang/14-annotations.md#derivation-blocks),
[facts](../../spec/lang/14-annotations.md#facts), [closures](../../spec/lang/07-functions.md#closures),
[serde](../../spec/std/serde.md), [JSON](../../spec/std/json.md), [CLI](../../spec/std/cli.md),
[environment](../../spec/std/host.md), and [coherence](../../spec/lang/09-traits.md).
The stdlib bar and owner decisions remain [STDLIB](STDLIB.md) and the linked issue replies.

The bounded model and evidence distinguish the selected post-decode path from discarded hook experiments.
Expected negative generic-evidence probes are not positive API examples.
Parsing proves syntax only; signature-only declarations do not prove behavior.
Publication checks, exact source/compiler identities, line counts and measured artifact sizes are recorded with the accompanying materials.

The [published materials](validation/README.md) pin source 51d48ea128329748ae01ba53e84e76def5295b1d
and native dev compiler 7421c58a0cb5f81b5d58190cee4b5c2385b887d979fbfd3b20d12d6b72d75afa, with Wasmtime enabled.
Every run uses scratch HD_CACHE inside the isolated worktree.

| Material | Parse/type/format | Runtime evidence |
| --- | --- | --- |
| Typed-evidence rejection example | Parse pass; intentionally rejects without Inspectable | Not a runnable positive example |
| Config Validate excerpt | Parse pass; included in the checked model | Exercised by field/record/default controls |
| Before package | Check0errors/warnings; format pass | Both engines print0 |
| Model package | Check0errors/warnings; format pass |7tests per engine pass; both mains print0 |
| Signature catalog | Check0errors/warnings; format pass | Not run: explicit shape-only stubs |

The discarded hook/record-fact experiment failed two completed-record controls and is not published as working evidence.
The selected model uses an explicit typed record method and passes those behaviors through its ordinary post-decode traversal.
The intentional wrong-field probe reports `in argument: expected fn(u32) -> bool, found fn(string) -> bool`.
The audit log records the actual writing/command mistakes once, separately from intentional negative bound probes.

No full local suite or accepted spec change is part of this design job.
