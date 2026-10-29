# Validation And Checked Readers

Status: implemented in [`std.validate`](../../spec/std/validate.md#validation); domain: standard library API.

Add a separate `Validate` derivation with concrete typed rules, automatic nested checks and ordinary custom functions.
One `.validated()` combinator follows JSON, HTTP, CLI or environment input.
Errors keep structural field identity, code and message; each reader renders its own names.
The API is built in `lib/std/validate.hd` and specified in `spec/std/`; the reviewed signature package is in git history at c6d7ffe6a, `docs/future-work/validation/`.

```text
struct Signup:
    username: string
    email: string
    password: string
    confirmation: string

impl Validate for Signup by Structure:
    username = [length(3, 40), pattern("^[a-z][a-z0-9_]*$")]
    email = [email()]
    fn record(&self, context: &mut ValidationContext) -> void:
        if self.password != self.confirmation:
            context.reject("confirmation", "passwords must match")

signup := req.json::[Signup]()?.validated()?
```

## Boundaries Of The Design

1. One post-reader `validated` combinator, not checked-reader variants or validation fused into decoding. It also covers HTTP and existing values.
2. A positive issue cap of 64 by default, with unvisited checks truncated; `max_errors=1` is the fail-fast form.
3. Completed-record errors report at the record root; a field-projection API waits for a need.
4. Readers name paths through resolver hooks, with source names as the fallback for opaque readers, so each reader keeps its own naming policy.

## Illustrative Problems

| Boundary | Today | Proposed |
| --- | --- | --- |
| Config JSON | `decode[ServerConfig]` accepts port zero or TLS without a certificate | `decode[ServerConfig](text)?.validated()` checks both |
| HTTP signup | Typed JSON still needs nested constraints and a hand-built error response | `req.json[Signup]()?.validated()`; 422 JSON issues |
| CLI | `--port 0` parses as u32; help and parse errors already have behavior | Validate after parsing, keeping existing help/error handling |
| Environment | Scalar parsing and record decoding lack semantic checks | The same value combinator; no caller-named parse error type |

<a id="proposed-solution"></a>

## How It Works

Each rule kind has a different phantom-typed fact type: `Length[F]`, `Pattern[F]`, `Check[F]`, and so on.
There is no `Rules` wrapper or erased list of rules.
Two patterns on one field remain a `duplicate-fact`; combine the regex or use a custom check.
The derive dispatches known fact kinds in their declared built-in order, with `Check[F]` last.

Nested Validate records and list/map elements recurse automatically; no `nested()` is needed.
Optionals skip None, decoded defaults are checked, and enums visit only the active payload.
An optional member takes the same rule builders as its payload type; a rule checks `.Some` and skips `.None`.
Element rules come from element types that implement `Validate`, or from a `Check[F]` on the collection.
The example uses `Email(@email() address: string)` because existing derivation lines name variants, not payload fields.

| Public API | Purpose |
| --- | --- |
| `Validate.validated(max_errors=64)` | One boundary; one positive issue limit |
| Concrete data facts, `Rule[F]` | One static check per kind; only `Check[F]` stores a user callback |
| Builder `message=` arguments | A same-type user-facing message for one fact |
| `HasLength` | String scalar length or collection size; byte length is explicit |
| `ValidationContext`, `Problem` | Borrowed custom checks; `reject(code, message)` |
| `PathStep`, `Violation` | Field identity, indices, map keys and reader-side `display_with` |
| `ValidationError` | Typed definition errors, issue lists, truncation and line-oriented Display |

The HTTP example returns 400 for decoding, 422 for validation and 500 for invalid rule definitions.
JSON, CLI, environment, form and query readers render each `PathStep.Field` through their own metadata.
Validation does not hard-code a closed reader list; opaque readers fall back to source names.
Record checks initially report at the root.

Validation retains at most 64 issues by default; any positive cap stops starting later checks.
A record callback never starts after stopping; an already-running callback completes normally.
Truncation means checks remained unvisited, not that another failure was proved.
Validation observes values without coercion, transforms or permanent validity after mutation.

Contexts, receivers and check arguments use non-escaping `&T`/`&mut T`.
Built-in facts contain data only; `Check[F]` owns its user callback and never a borrowed input or context.
Existing reader behavior is unchanged until an application calls validation.

## Cost

The dispatch representations were measured for PR head `395e511e6` in release Wasm on `a121c3ffd` with Node v24.19.0 before the API was implemented.
Each fresh process ran a tier-specific fixed batch sized to about 300 ms of hand-written work.
Fifteen runs were rotated and interleaved per variant at the default, `--no-liftoff` and `--liftoff-only` tiers.
The in-Wasm timer covered validation only; every path produced the same issue count and checksum.
Ratios are median paired throughput ratios, so higher is better.

| Variant | Default validations / 300 ms | Ratio | `--no-liftoff` validations / 300 ms | Ratio | `--liftoff-only` validations / 300 ms | Ratio |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Hand-written checks | 5,182,724 | 1.000× | 6,437,086 | 1.000× | 2,101,911 | 1.000× |
| Distinct data facts, static calls | 5,032,258 | 0.969× | 5,062,500 | 0.786× | 1,696,658 | 0.802× |
| One dynamic composite per field | 2,765,957 | 0.536× | 2,963,415 | 0.463× | 1,120,543 | 0.530× |
| Dynamic `Rules[F]` list | 2,063,492 | 0.398× | 2,044,164 | 0.318× | 706,638 | 0.335× |

The end-to-end pair freshly decoded the same four `Signup` documents on every validation.
Both arms allocated the same paths, problems and violations; the derived arm added `Structure::walk`, metadata lookup and static rule dispatch.
The dispatch probes use separate prefix, contains and excludes fact types so all four representations execute identical leaf checks.

| Variant | Default values / 300 ms | Ratio | `--no-liftoff` values / 300 ms | Ratio | `--liftoff-only` values / 300 ms | Ratio |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| JSON decode + hand-written checks | 305,085 | 1.000× | 252,632 | 1.000× | 94,839 | 1.000× |
| JSON decode + derived distinct facts | 204,545 | 0.663× | 174,545 | 0.690× | 65,479 | 0.692× |

Static facts meet the 1.5× validation-only bar at every tier; both dynamic representations miss it clearly.
Decode does not erase derivation cost: the real derived path retains 66–69% of hand-written end-to-end throughput.
The measured Wasm sizes were 12,562/13,526/15,066/17,892B for hand-written/static/composite/list validation and 56,221/62,581B for hand-written/derived JSON.

| Component | Estimated ordinary hd code and cost |
| --- | --- |
| Context, paths, report and Display | 50–90 lines; at most the cap's issue records plus paths/name strings |
| Concrete facts and typed callbacks | 80–140 lines; data-only built-ins, one owned callback kind, no retained input values |
| Built-in formats and collections | 100–200 lines; reuse regex/URL parsing, hashing for uniqueness |
| Derive, automatic traversal and callbacks | 50–90 lines; one additional semantic traversal |
| Reader-name resolvers | 80–160 lines; new std hooks, no compiler intrinsic proposed |
| Value combinator | 10–20 lines; zero duplicated readers or generic read-error enum |
| Compile time | Additional monomorphizations; full increment unmeasured |
| Runtime | Static calls retain 0.79–0.97× validation throughput; real decode plus derived traversal retains 0.66–0.69×; both dynamic paths are rejected |
| Web compiler | Baseline 1,722,109B gzip at `0c71c705f` (#952) / 2,000,000B cap; implementation increment unmeasured |

The previous [bounded model](https://github.com/HerringtonDarkholme/hd-lang/issues/846#issuecomment-6406520802) measured control/model at 123,453/143,901B, 326/398 functions, and 100,826/116,174 code-section bytes.
Five-launch startup-inclusive medians were Node 55.4/60.4ms and Wasmtime 41.0/46.3ms; the control omitted equivalent diagnostic paths.
Those sources stay in scratch; the implementation issue records its final compile-time, size and runtime measurements.

<a id="appendix-c-stress-test"></a>

## Appendix A: Stress Test

The surface tested is the concrete-fact revision of PR #849: local borrows, value validation and typed errors.
The real-library case translates [Pydantic's documented UserModel](https://docs.pydantic.dev/latest/concepts/validators/#model-validators):

```python
class UserModel(BaseModel):
    username: str
    password: str
    password_repeat: str
    @model_validator(mode="after")
    def check_passwords_match(self):
        if self.password != self.password_repeat: raise ValueError("Passwords do not match")
        return self
user = UserModel.model_validate_json(text)
```

Its literal hd translation is `Passwords`/`pydantic_model` in the checked examples:

```text
impl Validate for Passwords by Structure:
    fn record(&self, context: &mut ValidationContext) -> void:
        if self.password != self.password_repeat:
            context.reject("password-match", "passwords must match")
```

Seventeen focused compilation probes were run; the related controls are grouped below.
Stubs were never executed: “checks” below proves typing, not runtime validation.

| Case | What happened | Verdict / response |
| --- | --- | --- |
| Real Pydantic translation | Checks with defaulted `.validated()` | Shape works; runtime unproved |
| u8 field bounds without type arguments | Checks | Inference works at direct facts |
| Phantom `Length[F]` with `message=` and `Pattern[F]` | Checks | Expected field types infer the phantom target |
| Email fact on a u32 field | `type-mismatch` | Required typed rejection |
| Two Pattern facts | `duplicate-fact` | Required rejection; combine or use check |
| Store or capture a borrowed context | Both reject `local-reference-escapes` | Keep contexts local |
| `.validated()` defaults | Checks after f82d56b49 | Fixed and covered by the compiler fixture |
| Optional payload rule | Checks through the field's expected fact type | `.Some` is checked and `.None` is skipped |
| Mismatched passwords | Input/call shape checks only | Semantic rejection remains unproved |
| Invalid regex `[` | Fact/call shape checks only | Definition-error behavior remains unproved |

The historical model's seven tests per engine do not prove this broader revision, automatic recursion or reader aliases.

## Appendix B: Prior Art

These comparison fragments show the same signup/password and config/TLS checks; other signup fields are omitted here.
They were reviewed against the linked documentation, not executed in this job.

[Zod declarations](https://zod.dev/api) compose schemas and recursively validate nested objects/arrays:

```ts
const Signup = z.object({
  username: z.string().min(3).max(40), email: z.email(),
  password: z.string().min(12), confirmation: z.string()
}).refine(v => v.password === v.confirmation, { message: "passwords must match" });
const Config = z.object({
  port: z.number().int().min(1).max(65535).default(8080),
  tls: z.boolean().default(false), certificate: z.string().nonempty().optional()
}).refine(v => !v.tls || !!v.certificate, { message: "TLS needs a certificate" });
const result = Signup.safeParse(input); // Config.parse(JSON.parse(text)) for config
```

`.superRefine` can report multiple issues; [issues](https://zod.dev/error-formatting) carry `path`, `code` and `message`.
Zod 4 offers `flattenError`/`treeifyError`; the older `.flatten()`/`.format()` serve the same presentation concern.

[ArkType declarations and calls](https://arktype.io/docs/intro/your-first-type) return data or errors with a summary:

```ts
const Signup = type({
  username: "3 <= string <= 40", email: "string.email",
  password: "string >= 12", confirmation: "string"
}).narrow(v => v.password === v.confirmation);
const Config = type({
  port: "1 <= number.integer <= 65535 = 8080",
  tls: "boolean = false", "certificate?": "string > 0"
}).narrow(v => !v.tls || !!v.certificate);
const result = Signup(input); // Config(JSON.parse(text)) for config
if (result instanceof type.errors) console.error(result.summary);
```

ArkType's [constraints](https://arktype.io/docs/intro/adding-constraints), [narrowing](https://arktype.io/docs/expressions#narrow) and [built-ins](https://arktype.io/docs/primitives) cover custom rejection, email, URL, UUID and integer strings.

[Pydantic fields](https://docs.pydantic.dev/latest/concepts/fields/) and [model validators](https://docs.pydantic.dev/latest/concepts/validators/) separate field and completed-model checks:

```python
class Signup(BaseModel):
    username: str = Field(min_length=3, max_length=40)
    email: EmailStr
    password: str = Field(min_length=12)
    confirmation: str
    @model_validator(mode="after")
    def passwords_match(self):
        if self.password != self.confirmation: raise ValueError("passwords must match")
        return self
class Config(BaseModel):
    port: int = Field(default=8080, ge=1, le=65535, validate_default=True)
    tls: bool = False
    certificate: str | None = Field(default=None, min_length=1)
    @model_validator(mode="after")
    def needs_certificate(self):
        if self.tls and not self.certificate: raise ValueError("TLS needs a certificate")
        return self
config = Config.model_validate_json(text)
```

Pydantic's [`errors()`](https://docs.pydantic.dev/latest/errors/errors/) uses `loc`, `type` and `msg`; hd exposes structural paths plus display names and typed problems.

| Common capability | hd mapping |
| --- | --- |
| Integer, one-sided and inclusive bounds | Static integer types; `min`, `max`, `range` |
| Non-empty, length, regex, enum choice | `min_length(1)`, generic `length`/`byte_length`, `pattern`, list-valued `one_of` |
| Email, URL, UUID | Built-in facts; syntactic checks only, no network capability |
| Nested objects/arrays, optional/nullable, defaults | Recursive Validate/container implementations, `T?`, existing reader defaults |
| Unique items and element rules | `unique`; element `Validate` implementations or a collection `check` |
| Custom checks/messages | `check(fn(&T))`, `reject(code, message)`, builder `message=` arguments |
| Starts-with/contains | Plain string methods inside `check`, shown by the username policy |
| Coercion/transforms | None; parsing and application transformations remain explicit |

[Rust validator](https://docs.rs/validator/latest/validator/) keeps validation separate from deserialization:

```rust
#[derive(validator::Validate)]
struct Config {
    #[validate(range(min = 1, max = 65535))]
    port: u32,
}
config.validate()?;
```

hd keeps that separation and uses ordinary typed facts instead of attribute argument syntax.
[Garde](https://docs.rs/garde/latest/garde/) also puts checks beside fields:

```rust
#[derive(garde::Validate)]
struct Signup {
    #[garde(length(min = 3, max = 40))]
    username: String,
}
signup.validate()?;
```

hd takes typed custom checks and explicit length semantics, but proposes automatic nested traversal instead of `dive`.
[Go validator](https://pkg.go.dev/github.com/go-playground/validator/v10) is a secondary reference for record callbacks; hd does not adopt string-tag rules.

<a id="appendix-a-alternatives-considered"></a>

## Appendix C: Alternatives Considered

| Boundary | Benefit | Additional cost |
| --- | --- | --- |
| Six checked-reader variants | One named read-and-validate operation | Six wrappers and generic read-error composition; HTTP needs another |
| `reader()?.validated()` | Works after any reader and on existing values | Extra traversal and name resolvers |
| Cooperative validation during construction | Can reject before parent construction finishes | Build hooks, defaults/completion and custom-reader contracts |
| Dynamic `Rules[F]` list | One extensible erased collection | Retains only 0.32–0.40× hand-written throughput and adds 42% Wasm in the ceiling |
| One dynamic composite per field | One indirect call per field | Retains only 0.46–0.54× throughput and adds 20% Wasm in the ceiling |

**Recommendation:** the value combinator, the smallest ordinary-library addition.
Distinct facts preserve today's duplicate rule: a second fact of the same rule kind is rejected.
Optional fields use the payload rule builders directly. Element rules use the element's `Validate`
implementation or a collection `Check[F]`; there are no optional, element or map wrappers.

<a id="context"></a>

## Appendix D: Today In hd

[JSON](../../spec/std/json.md#typed-json), [CLI](../../spec/std/cli.md) and [environment readers](../../spec/std/host.md) establish types.
[HTTP](../../spec/std/http.md) supplies typed body readers through `std.http`.
None currently supplies this validation library; “while reading” was the earlier proposal, not implemented behavior.
The revised combinator deliberately proposes a second traversal instead of changing reader protocols.
The signature example uses the implemented `std.http` request and response types.

<a id="appendix-d-implementation-and-checks"></a>

## Appendix E: Implementation And Checks

The sole template exception panics before an unreachable `walk`, as approved in PR review.
All non-escaping references are local borrows; an implementation must keep traversal state as owned data rather than store those borrows.
First implement concrete facts, traversal and errors; then reader-name resolvers and the HTTP/CLI/env examples.
Each slice ships stdlib rules, focused conformance on both engines, size measurements and checks for incorrect fact types and recursion.
Email follows the WHATWG HTML valid-address grammar. URL accepts the absolute HTTP(S) URLs that `std.http.Url.parse` accepts.

From `docs/future-work/validation`, run `hd check --all --format json` and `hd fmt --check`.
