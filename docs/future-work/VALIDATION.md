# Validation And Checked Readers

Status: proposal, 2026-10-26; domain: standard library API; not accepted.

Add a separate `Validate` derivation with concrete typed rules, automatic nested checks and ordinary custom functions.
One `.validated()` combinator follows JSON, HTTP, CLI or environment input.
Errors already contain their display path, code and message; Display prints one line per issue.
The [signature package](validation/src/validation.hd) and [usage examples](validation/src/examples.hd) are the API under review.

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

## Open Questions

1. Use one post-reader `validated` combinator instead of checked-reader variants or fused validation? **Recommend yes:** it also covers HTTP and existing values.
2. Default to All with a positive 64-issue cap and unvisited-check truncation? **Recommend yes:** First remains explicit.
3. Keep completed-record errors at the record root initially? **Recommend yes:** defer a field-projection API.
4. Add reader-name resolver hooks with semantic fallback for opaque readers? **Recommend yes:** preserve each reader's naming policy.

## Illustrative Problems

| Boundary | Today | Proposed |
| --- | --- | --- |
| Config JSON | `decode[ServerConfig]` accepts port zero or TLS without a certificate | `decode[ServerConfig](text)?.validated()` checks both |
| HTTP signup | Typed JSON still needs nested constraints and a hand-built error response | `req.json[Signup]()?.validated()`; 422 JSON issues |
| CLI | `--port 0` parses as u32; help and parse errors already have behavior | Validate after parsing, keeping existing help/error handling |
| Environment | Scalar parsing and record decoding lack semantic checks | The same value combinator; no caller-named parse error type |

<a id="proposed-solution"></a>

## How It Works

Each rule kind has a different fact type: `Length`, `Pattern`, `Check[F]`, and so on.
There is no `Rules` wrapper or erased list of rules.
Two patterns on one field remain a `duplicate-fact`; combine the regex or use a custom check.
The derive dispatches known fact kinds in source order.

Nested Validate records and list/map elements recurse automatically; no `nested()` is needed.
Optionals skip None, decoded defaults are checked, and enums visit only the active payload.
The example uses `Email(@email() address: string)` because existing derivation lines name variants, not payload fields.

| Public API | Purpose |
| --- | --- |
| `Validate.validated`, `Mode` | One boundary; First or bounded All |
| Concrete rule facts, `Rule[F]` | Typed checks; callback/message and optional/element composition protocol |
| `HasLength` | String scalar length or collection size; byte length is explicit |
| `ValidationContext`, `Problem` | Borrowed custom checks; `reject(code, message)` |
| `PathStep`, `Violation` | Names, indices, map keys and ready-to-display errors |
| `ValidationError` | Typed definition errors, issue lists, truncation and line-oriented Display |

The HTTP example returns 400 for decoding, 422 for validation and 500 for invalid rule definitions.
`for_json()`, `for_cli()` and `for_env()` put reader names directly on each issue.
Their metadata resolvers are unimplemented; no location-matching loop is left to the caller.
Opaque readers fall back to semantic names, and record checks initially report at the root.

All retains at most 64 issues; a positive cap or First stops starting later checks.
A record callback never starts after stopping; an already-running callback completes normally.
Truncation means checks remained unvisited, not that another failure was proved.
Validation observes values without coercion, transforms or permanent validity after mutation.

Contexts, receivers and check arguments use non-escaping `&T`/`&mut T`.
Stored callbacks own their rule data, never a borrowed input or context.
Existing reader behavior is unchanged until an application calls validation.

## Cost

These are **estimates for the revised proposal**, not measurements of implemented validation.

Compiler work still needed: trait-default `.validated()` reports `unsupported: stage Body: a default argument of this callee is not supported`.
Non-generic `Length`/`Pattern` remove `.message` inference; generic wrappers still need typed helpers (`cannot-infer-type`: `a type argument here has no solution; write it explicitly or give an expected type`).
Inline `env_as(...)?` inside the custom-error example reports `a type argument named only in bounds has no solution; write it explicitly`.
An expected `Result[Port?, HostError]` binding resolves it without naming the parse error type; direct HostError-returning controls also pass.

| Component | Estimated ordinary hd code and cost |
| --- | --- |
| Context, paths, report and Display | 50–90 lines; at most the cap's issue records plus paths/name strings |
| Concrete facts and typed callbacks | 80–140 lines; owned callback captures, no retained input values |
| Built-in formats and collections | 100–200 lines; reuse regex/URL parsing, hashing for uniqueness |
| Derive, automatic traversal and callbacks | 50–90 lines; one additional semantic traversal |
| Reader-name resolvers | 80–160 lines; new std hooks, no compiler intrinsic proposed |
| Value combinator | 10–20 lines; zero duplicated readers or generic read-error enum |
| Compile time | Additional monomorphizations; full increment unmeasured |
| Wasm and runtime | Revised feature unmeasured; historical reference below is not its budget |
| Web compiler | Recorded baseline 1,972,480B gzip / 2,000,000B cap; implementation increment unmeasured |

The previous [bounded model](https://github.com/HerringtonDarkholme/hd-lang/issues/846#issuecomment-6406520802) measured control/model at 123,453/143,901B, 326/398 functions, and 100,826/116,174 code-section bytes.
Five-launch startup-inclusive medians were Node 55.4/60.4ms and Wasmtime 41.0/46.3ms; the control omitted equivalent diagnostic paths.
Those sources stay in scratch, and the broader revision needs fresh implementation measurements.

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
Positive controls supply explicit mode/cap only to isolate the known default-method bug.
Stubs were never executed: “checks” below proves typing, not runtime validation.

| Case | What happened | Verdict / response |
| --- | --- | --- |
| Real Pydantic translation | Explicit-argument control checks | Shape works; runtime unproved |
| u8 field bounds without type arguments | Checks | Inference works at direct facts |
| Concrete Length `.message` with Pattern | Checks | Nongeneric facts remove this inference friction |
| Email fact on a u32 field | `type-mismatch` | Required typed rejection |
| Two Pattern facts | `duplicate-fact` | Required rejection; combine or use check |
| Store or capture a borrowed context | Both reject `local-reference-escapes` | Keep contexts local |
| `.validated()` defaults | `unsupported: stage Body` | Breaks today; compiler bug assigned to orchestrator |
| Generic non-empty optional/element rule | `cannot-infer-type` | Friction; checked typed policy helper |
| Inline env_as plus custom-error propagation | Bounds-only `cannot-infer-type` | Typed whole-Result binding checks; direct-return controls also check |
| Mismatched passwords | Input/call shape checks only | Semantic rejection remains unproved |
| Invalid regex `[` | Fact/call shape checks only | Definition-error behavior remains unproved |

The exposed failures are default-method lowering, wrapper inference and the narrow env-call inference case.
The target short calls remain visible; no compiler/spec change or named-list bypass was made.
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
| Integer, one-sided and inclusive bounds | Static integer types; `at_least`, `at_most`, `range` |
| Non-empty, length, regex, enum choice | `non_empty`, `length`/`list_length`/`byte_length`, `pattern`, `one_of` |
| Email, URL, UUID | Built-in facts; syntactic checks only, no network capability |
| Nested objects/arrays, optional/nullable, defaults | Recursive Validate/container implementations, `T?`, existing reader defaults |
| Unique items and map rules | `unique`, `keys`, `values`; ordinary Eq/Hash |
| Custom checks/messages | `check(fn(&T))`, `reject(code, message)`, per-fact `.message` |
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

**Recommendation:** the value combinator, the smallest ordinary-library addition.
Distinct facts preserve today's duplicate rule, like `rename` and `env_name`.
Optional/each wrappers erase owned callbacks into concrete typed facts, not an untyped registry.

<a id="context"></a>

## Appendix D: Today In hd

[JSON](../../spec/std/json.md#typed-json), [CLI](../../spec/std/cli.md) and [environment readers](../../spec/std/host.md) establish types.
[Approved HTTP messages](http/src/http.hd) supply typed body readers; their implementation is #847.
None currently supplies this validation library; “while reading” was the earlier proposal, not implemented behavior.
The revised combinator deliberately proposes a second traversal instead of changing reader protocols.
The signature example depends on the approved HTTP package until #847 migrates it to std.

<a id="appendix-d-implementation-and-checks"></a>

## Appendix E: Implementation And Checks

The sole template exception panics before an unreachable `walk`, as approved in PR review.
All non-escaping references are local borrows; an implementation must keep traversal state as owned data rather than store those borrows.
First implement concrete facts, traversal and errors; then reader-name resolvers and the HTTP/CLI/env examples.
Each slice ships stdlib rules, focused conformance on both engines, size measurements and checks for incorrect fact types and recursion.
Full builtin matching policies, including email and URL syntax, require review before implementation.

From `docs/future-work/validation`, run `hd check --all --format json` and `hd fmt --check`.
