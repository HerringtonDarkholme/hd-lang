# Validation And Checked Readers

Status: proposal, 2026-10-26; domain: standard library API; not accepted.

## Context

Readers establish types; applications also need ranges, lengths and related-field checks.
JSON uses [Deserialize](../../spec/std/json.md#typed-json), command lines use [Cli](../../spec/std/cli.md), and environment input uses [Parse or Deserialize](../../spec/std/host.md).
The proposal adds one validation concern, configured in its own derivation block.
The [signature package](validation/src/validation.hd) and [examples](validation/src/examples.hd) show the complete proposed API.

## Illustrative Problems

These fragments use the types in the example package.

```text
config := decode::[ServerConfig](text)?
```

A `u32` port of zero decodes successfully; TLS may have no certificate.
The caller must repeat semantic checks after each config load.

```text
options := Options::parse_args()?
```

`--port 0` parses as an integer, yet cannot be used as the application's listening port.
A second check must also preserve the CLI's help and exit behavior.

```text
port := env_as::[u32]("APP_PORT")?
```

The text `"0"` parses, and an absent variable returns `None`.
Its validation error should name `APP_PORT`, while JSON and CLI errors need their own names.

## Proposed Solution

Each boundary reads and validates through one checked call:

```text
config := decode_checked::[ServerConfig](text)?
options := parse_args_checked::[Options]()?
port := env_as_checked::[Port, string]("APP_PORT")?
```

The same typed rule can be reused in independent validation views:

```text
impl Validate for ServerConfig by Structure:
    port = [port_rules()]
    certificate = [rules([optional(text_length(1, 4096))])]
    fn record(self, context: *mut ValidationContext) -> void:
        if self.tls && self.certificate.is_none():
            context.reject(Problem { code: "tls-certificate", message: "TLS needs a certificate" })
```

The package also shows a signup request with nested address lists, typed role choices,
password confirmation, conditional invitation codes, and a plain username-check function.
CLI options have their own reader view; both reuse the port rule.

| Public types | Purpose |
| --- | --- |
| `Validate`, `Rule[F]`, `Rules[F]` | Independent derivation and composable typed field checks |
| `ValidationContext`, `Problem` | Plain custom checks and completed-record rejection |
| `Mode` | First error or bounded collection of semantic errors |
| `PathStep`, `Violation`, `ValidationError` | Member/index identity, issues, truncation and invalid rule definitions |
| `Location`, `CheckedReadError[E]` | Reader spelling alongside structural errors; original read errors remain typed |
| `TextLengthUnit` | Explicit byte or Unicode-scalar length |

Full signatures, defaults and capability rows are in [validation.hd](validation/src/validation.hd).
The JSON helper needs `Deserialize & Validate`; CLI needs `Cli & Validate`.
Scalar environment input keeps `Parse & Validate`; record environment input keeps `Deserialize & Validate`.

The recommended implementation decodes once, then runs `Validate` on the completed value.
This adds a semantic traversal and checks defaults and handwritten decoders uniformly.
It deviates from the recorded phrase “validates while reading”; it requires approval and is not fused validation.
Existing unchecked reader entry points retain their behavior.

Rules cover inclusive ranges, text/list lengths, regex patterns, typed one-of, optional values,
nested records, list elements and plain requirement-free custom functions.
Validation observes values without normalizing them or granting permanent validity after later mutation.
Standard problems carry a code and message without retaining rejected values.

The proposal defaults to All with at most 64 retained issues and explicit truncation.
Order is declaration, rule-list and list-element order, then completed-record checks.
First mode or the cap stops later checks, including the record check.
Read failures occur before semantic validation; All collects semantic issues from a successfully decoded value.
Invalid options fail before reading; malformed rule definitions fail when their configured visit occurs.

Structural paths retain declaration positions, variants and list indices.
Readers attach names such as `$.listenPort`, `--port` and `APP_PORT` to those identities.
An opaque handwritten decoder falls back to semantic names when it exposes no member correspondence.
Record checks initially report at the record root; a field-targeting projection API is outside this proposal.

## Cost

[Bounded reference measurements](https://github.com/HerringtonDarkholme/hd-lang/issues/846#issuecomment-6406520802)
pin their compiler and source; they measure a model rather than production validation overhead.

| Component | Measured reference or remaining cost |
| --- | --- |
| Context, paths and report | 28 matched model code lines |
| Typed rules and list adapters | 23 matched model code lines |
| Validate traversal and record callback | 25 matched model code lines |
| Checked-call composition | 8 matched model code lines |
| Full constructors and reader alias capture | Unbuilt; their remaining implementation cost is unmeasured |
| Emitted Wasm | Control 123,453B /326functions /100,826code bytes; model 143,901B /398functions /116,174code bytes |
| Runtime | Five-launch medians: Node 55.4/60.4ms, Wasmtime 41.0/46.3ms, control/model; includes startup |
| Compile time | Full-library increment unmeasured; the current signature package checks with zero diagnostics |
| Web compiler | Recorded pre-feature baseline 1,972,480B gzip against the 2,000,000B cap; implementation increment unmeasured |

The handwritten control only counts failures; it does not implement equivalent diagnostic paths.
Alias capture, complete regex/rule constructors and reader wrappers need separate implementation measurements.
Each implementation slice stops for review if it exceeds 1.5 times its measured component budget.
The web compiler must stay below its gzip cap.

## Open Questions

1. Approve decode-then-Validate inside one entry point, replacing “validates while reading” without changing reader protocols?
2. Default to All with a positive 64-issue cap, explicit truncation and declaration/rule/element order?
3. Let completed-record checks report at the record root, with no field-projection helper initially?
4. Use semantic-name fallback when a handwritten decoder supplies no reader-name correspondence?

## Appendix A: Alternatives Considered

Cooperative checked construction could validate fields before the parent finishes.
It needs typed construction hooks, a custom-reader contract and completion checks for defaults.
General reader hooks would affect more construction paths and may require extra Inspectable evidence.
The completed-value recommendation keeps each existing reader protocol and its custom implementations.

## Appendix B: Prior Art

[Rust validator](https://docs.rs/validator/latest/validator/) combines typed range/length/custom checks with a separate validation derive.
[Garde](https://docs.rs/garde/latest/garde/) adds nested/optional checks, container traversal and explicit length units.
Those common cases appear in the signup and config examples.
[Go validator](https://pkg.go.dev/github.com/go-playground/validator/v10) supplies field tags, cross-field checks and struct-level callbacks.
The proposed typed rules and completed-record callback cover those checks without string-tag rules.

[Zod](https://zod.dev/api) supplies composable checks and refinements.
[Pydantic](https://pydantic.dev/docs/validation/latest/concepts/validators/) distinguishes field checks from completed-model checks.
The examples use completed-parent password and TLS checks; asynchronous checks and coercion are outside this proposal.
Extra email/URL convenience rules can use the plain custom-check atom.

## Appendix C: Stress Test

The checked examples cover signup fields and nested addresses, JSON config, CLI options,
scalar/record environment input, conditional presence and user-written checks.
All public types, functions and context methods are used outside imports.
Validate's protocol is represented by derivations and record overrides.
Type-checking stubs establishes API compatibility, without executing their proposed behavior.

The [bounded model](https://github.com/HerringtonDarkholme/hd-lang/issues/846#issuecomment-6406520802)
passed seven tests on each engine, covering independent names, nested indices, caps, defaults and custom decoding.
General reader alias capture remains unproved.
The example separates CLI options from JSON/environment config to respect today's same-package derivation-line warning.

## Appendix D: Implementation And Checks

The package's functions are unimplemented stubs.
Its sole template exception panics before an unreachable `walk` call, which the compiler requires to recognize a derivation template.

Implementation follows owner review: first the Validate atom, facts and errors;
then checked JSON and name capture; then CLI/environment readers and CLI help/exit preservation.
Each slice ships its stdlib rules, focused conformance cases and measured size on both engines.

From `docs/future-work/validation`, run `hd check --all --format json` and `hd fmt --check`.
