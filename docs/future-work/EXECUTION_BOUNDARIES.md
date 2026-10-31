# Execution Boundaries: Moving Code And Data

Status: design exploration, 2026-10-31; nothing here is decided or in the
specification. It compares two designs for one open question. One is the
owner's new note, "Types and Transport Across Execution Boundaries",
shared as a gist. The other is the parked [#166](https://github.com/HerringtonDarkholme/hd-lang/issues/166)
study, [SERIALIZABLE_CLOSURE_DESIGN.md @ 28072b8a1](https://github.com/HerringtonDarkholme/hd-lang/blob/28072b8a1/docs/future-work/SERIALIZABLE_CLOSURE_DESIGN.md).
It also weighs a review of the owner's note against both. The spec
sections it relies on are [Boundary-Safe Values](../../spec/lang/10-modules.md#boundary-safe-values),
[Instances And Threads](../../spec/lang/10-modules.md#instances-and-threads),
[Captures](../../spec/lang/07-functions.md#captures),
[Writes While Borrowed](../../spec/lang/13-values-and-references.md#writes-while-borrowed),
[Dependent Results](../../spec/lang/13-values-and-references.md#dependent-results),
and [Abandoning](../../spec/lang/11-requirements-and-suspension.md#abandoning).

**The question.** How should hd move a computation and its data across an
execution boundary? The boundary may be another worker or thread, a
server, a GPU, or a later resume.

- [1. The Domain](#1-the-domain)
- [2. Problem, And What hd Has Today](#2-problem-and-what-hd-has-today)
- [3. Survey](#3-survey)
- [4. Option A: Named Code, Copied Data](#4-option-a-named-code-copied-data)
- [5. Option B: Portable Closures Placed By Rows](#5-option-b-portable-closures-placed-by-rows)
- [6. Option C: Typed Crossing Contracts](#6-option-c-typed-crossing-contracts)
- [7. Option D: Send, Sync, And Scoped Threads](#7-option-d-send-sync-and-scoped-threads)
- [8. Comparison](#8-comparison)
- [Recommendation](#recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Sources](#sources)
- [Parse Log](#parse-log)

## 1. The Domain

An **execution domain** is a context with its own rules for memory,
authority, available operations, or lifetime, in the owner's note's words.
Another Wasm instance, a thread, a server, a GPU and a process that resumes
tomorrow are each one. Code that runs in one domain and continues in
another must answer three questions about every value it uses.

| Question | Example of a wrong answer |
| --- | --- |
| Can the destination use this value at all? | A file handle or a provider has no meaning on another machine. |
| What does the destination get? | A copy, the same object, a handle back to the owner, or a new device buffer. |
| What may the source still do with it? | Write a buffer that a worker is still reading. |

### Closures Are The Hard Case

A closure is code plus an environment. **Closure conversion** turns it
into a named function whose first parameters are the captured values. hd
already does this for every closure.

```text
pub struct Loan:
    pub principal: i64
    pub rate_bp: i32

pub struct Model:
    pub weights: List[f64]

fn score(l: Loan, model: Model) -> f64:
    f64(l.principal) * model.weights[0] + f64(l.rate_bp) * model.weights[1]

fn scorer(model: Model) -> fn(Loan) -> f64:
    fn(l: Loan) -> f64: score(l, model)    # code plus the environment { model }

# What the compiler makes of it: a named function and one environment value.
fn scorer_lifted(model: Model, l: Loan) -> f64:
    score(l, model)
```

Running `scorer(model)` elsewhere needs two things. The destination must
have `scorer_lifted`, and the environment `{ model }` must reach it. Every
design in this record is a different answer to those two needs.

### Why The Naive Answer Fails

The naive answer serializes the whole closure, code and captures. Each
capture below breaks it in a different way:

```text
struct Counter:
    n: i64

fn hazards(model: Model, hits: *mut Counter) -> void $ Console:
    a := fn(l: Loan) -> f64: score(l, model)    # a copy of a value: safe to send
    b := fn(l: Loan) -> f64:                    # a shared object: a remote copy counts alone
        hits.n += 1
        score(l, model)
    c := fn(l: Loan) -> void: println("x")      # Console is in the row: the destination must bind it
```

- `a` is fine: `model` is a value, so a copy means the same thing anywhere.
- `b` changes meaning. Spark documents the same bug: a counter bumped on
  executors stays 0 at the driver.
- `c` captures nothing, yet needs a console where it runs. Captures do not
  describe effects.
- Shipping code bytes lets a client run code the server never vetted.
  React Server Functions send a reference to installed code instead.

The owner's note adds the cases where the destination's type differs. A
host image becomes a device buffer on a GPU, and a server object becomes a
remote handle. The body that runs there must type-check against the new
type.

## 2. Problem, And What hd Has Today

### What hd Has

| Area | Today | Spec |
| --- | --- | --- |
| Instances | One instance runs on one thread. Instances run in parallel only by exchanging boundary-safe values; heaps are disjoint. | [`module.instance.thread`](../../spec/lang/10-modules.md#r-module.instance.thread), [`module.instance.parallel`](../../spec/lang/10-modules.md#r-module.instance.parallel), [`module.instance.disjoint`](../../spec/lang/10-modules.md#r-module.instance.disjoint) |
| What crosses | Primitives, strings, tuples, lists, maps, structs and enums of those. Never closures, trait values or live handles. | [`module.boundary.allowed`](../../spec/lang/10-modules.md#r-module.boundary.allowed), [`module.boundary.not-safe`](../../spec/lang/10-modules.md#r-module.boundary.not-safe) |
| How it crosses | Tree semantics; a shared node is duplicated; a cycle is a boundary error. Private fields cross only through `Serialize`. | [`module.boundary.tree`](../../spec/lang/10-modules.md#r-module.boundary.tree), [`module.boundary.sharing`](../../spec/lang/10-modules.md#r-module.boundary.sharing), [`module.boundary.out`](../../spec/lang/10-modules.md#r-module.boundary.out) |
| Authority | Requirement rows are host bindings, never serialized. A registered function's row must be bindable by its host. | [`module.boundary.rows`](../../spec/lang/10-modules.md#r-module.boundary.rows), [`module.register.row`](../../spec/lang/10-modules.md#r-module.register.row) |
| Captures | Immutable bindings are copied; `let` variables are shared through a cell; providers are never captured. | [`fn.capture.copy.using-split`](../../spec/lang/07-functions.md#r-fn.capture.copy.using-split), [`fn.capture.cell`](../../spec/lang/07-functions.md#r-fn.capture.cell), [`fn.capture.no-providers`](../../spec/lang/07-functions.md#r-fn.capture.no-providers) |
| `Fn` and `FnMut` | A closure that changes a capture is only `FnMut`. | [`fn.trait.read-only`](../../spec/lang/07-functions.md#r-fn.trait.read-only) |
| Local references | `&T` and `&mut T` are lent to a call. A closure may capture one only as a direct `fn(...)` argument. | [`ref.local.escape.capture-stored`](../../spec/lang/13-values-and-references.md#r-ref.local.escape.capture-stored), [`fn.capture.local-parameter`](../../spec/lang/07-functions.md#r-fn.capture.local-parameter) |
| Suspensions that borrow | `Suspend[R, 'a]` may hold local references, but never reaches a box, provider, host call or top-level binding. | [`ref.dep.suspension.lifetime`](../../spec/lang/13-values-and-references.md#r-ref.dep.suspension.lifetime), [`ref.dep.suspension.store`](../../spec/lang/13-values-and-references.md#r-ref.dep.suspension.store) |
| Exclusivity | Compile-time exclusivity for written references; run-time loans panic `borrow-conflict` through a `*mut` alias. | [`ref.excl.mut`](../../spec/lang/13-values-and-references.md#r-ref.excl.mut), [`ref.excl.alias.conflict`](../../spec/lang/13-values-and-references.md#r-ref.excl.alias.conflict) |
| Result paths | A result may borrow one field, so the caller locks only that field. | [`ref.result.path.caller`](../../spec/lang/13-values-and-references.md#r-ref.result.path.caller) |
| Abandon | `abandon()` unwinds a suspension synchronously; a pending host operation aborts by its own rule; a run-time loan the suspension held ends (a #1134 fixture). | [`req.abandon.unwind`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.unwind), [`req.abandon.host`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.host) |
| Cleanup | `using` and `@auto_closing` dispose a handle at scope exit. | [Using Bindings](../../spec/lang/06-control-flow.md#using-bindings), [`flow.auto-closing.statement`](../../spec/lang/06-control-flow.md#r-flow.auto-closing.statement) |
| Code identity | Only "the same compiled program" is defined; serializable closures are deferred. | [`req.determinism.same-program`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.same-program), [`fn.capture.serializable-deferred`](../../spec/lang/07-functions.md#r-fn.capture.serializable-deferred) |
| Coherence | Implementations have no orphans and never overlap, so one type has one implementation of a trait. | [`trait.own.orphan`](../../spec/lang/09-traits.md#r-trait.own.orphan), [`trait.overlap.definition`](../../spec/lang/09-traits.md#r-trait.overlap.definition) |

In progress or not yet there: `&mut` becomes non-copyable with reborrows at
calls ([#1137](https://github.com/HerringtonDarkholme/hd-lang/issues/1137),
slice M1 of [#1130](https://github.com/HerringtonDarkholme/hd-lang/issues/1130)).
Every other type still copies, and `~Copyable` comes later
([#535](https://github.com/HerringtonDarkholme/hd-lang/issues/535)).
There are no threads in one heap and no atomics. There is no `Send` or
`Sync` yet; the owner wants both later. [Parallelism And Locality](PARALLELISM_AND_LOCALITY.md)
proposes workers as separate instances with copied messages and
`SharedBytes`, and parks shared heaps.

Three facts change the old study's picture:

1. A suspension that borrows cannot reach a host call. So nothing lent can
   be handed to another instance today; the review's "structured scope for
   borrows" is already the rule, at the strictest setting.
2. Abandoning ends a run-time loan because unwinding is synchronous and in
   one instance. A loan held by another thread would break that.
3. A plain value always copies. A "move" of a `List` across disjoint heaps
   is still an encode and a copy. Moves matter only for `&mut`, later for
   `~Copyable` types, and for handles such as `SharedBytes`.

### Use Cases

Every option is judged on the same six cases.

| # | Case | Boundary |
| --- | --- | --- |
| U1 | Score a million loans on a worker pool with one model. | instance to instance |
| U2 | Hand a 64 MB buffer to another worker without copying it. | instance to instance, shared memory |
| U3 | Two parallel children read the parent's list; the parent waits for both. | thread to thread, one heap |
| U4 | Browser code calls a server function; the server binds its own authority; a lost reply is visible. | client to server |
| U5 | Upload an image to a GPU once, run two steps there, read back one number. | host to device, resident data |
| U6 | A charge-then-mail run crashes and resumes without charging twice. | now to later |

### The Two Designs

The owner's note proposes a rule family `Across<Source, Destination, Mode, T>`
with an associated destination type `There` (also spelled `Cross … Received`).
A destination-typed form `at D capture { … } return r do body` checks the
body against the received types. A judgment Γ_A ⊢ c : A→B ⇒ Δ_B ▷ Γ_A′
says what the destination gets (Δ_B) and what the source keeps (Γ_A′).
Modes are copy, move (transfer), share, proxy and reconstruct.

The #166 study proposes named `@remote` functions and `remote!(f, args)`.
It adds portable closure literals with copied boundary-safe captures, and
placement by requirement rows. `Opaque[T]` carries state a server hands
out. Code never
travels and never names a place; the deployment configuration does.

**Where they agree.**

| Point | Owner's note | #166 |
| --- | --- | --- |
| Code is not uploaded | "a client does not gain permission to upload arbitrary executable code"; the compiler resolves registered code | "No code ever travels"; a portable closure is a content identity plus captures |
| Captures are checked one by one | Each capture names a mode | Each capture must be boundary-safe |
| The destination's operations limit the body | Σ_B and Allowed_B(ε) | The place's profile must bind the row |
| Authority is not carried | Authority is a separate constraint | No handle travels; the destination binds its own requirements |
| Failure differs from a local call | `Execution_A(σ)` reports success, failure, or an unknown outcome | `RemoteError` has `OutcomeUnknown` |
| Durable work replays | Replay-compatible code, controlled effects | Recorded host results, no stack serialized |

**Where they differ.**

| Point | Owner's note | #166 |
| --- | --- | --- |
| Modes | copy, move, share, proxy, reconstruct, chosen per capture | copy only |
| Destination type | May differ (`There`), as host tensor to device tensor | Always the same type |
| Code form | A second-class `at D … do` block that runs at once | A first-class `Portable[F]` value, stored and passed, plus named functions |
| Source rights | Γ_A′ records consumed values and loans | The source keeps everything; copies only |
| Who names the destination | Code: `at Worker`, `at GPU0` | The deployment configuration; code never names a place (owner, 2026-10-17) |
| Result | An execution handle with a return mode | `Result[T, RemoteError]` from `remote!` |
| Scope | Threads, GPUs, clusters and workflows equally | Browser, server, services and workers; GPUs deferred |
| Secrets | Notes that `Secret<T>` needs information-flow labels | `Opaque[T]`, trust per edge, `Caller` |

### Is It Core?

Partly. A copy-only design depends on nothing open. Move and share depend
on three open core decisions: `~Copyable` (#535), `Send` and `Sync`, and a
shared heap ([stage C](PARALLELISM_AND_LOCALITY.md#34-stage-c-structured-parallel-children)).
Device types depend on a GPU code subset, which #166 left to its own
study. The owner parked #166 on 2026-10-23 as too early, behind compiler
and stdlib work; this record does not change that order.

## 3. Survey

| System | What crosses, and how | How the destination finds code | What is checked statically |
| --- | --- | --- | --- |
| Rust | `thread::spawn` moves a closure; `thread::scope` lets threads borrow and joins them before returning | Same binary, same heap | `Send` for moving, `Sync` for sharing, `'static` unless scoped; both are unsafe traits |
| Swift | `Sendable` values cross isolation; `sending` transfers a value whose region nothing else reaches; distributed actor calls take serializable arguments | Same binary, or the actor system for distributed calls | `Sendable` conformance, region analysis, the actor system's serialization requirement |
| Go | Goroutines share one heap; a channel send copies the value, which may hold pointers | Same binary | Nothing; a race detector at run time |
| Pony | `iso` (unique), `val` (immutable) and `tag` (identity only) may be shared with other actors; `ref` may not | Same binary, actors | Reference capabilities on every reference |
| OxCaml | `portable` functions may run in parallel; `contended` values may not be written | Same binary, one heap | Modes, checked per function |
| JS workers | Structured clone copies; a transfer list moves `ArrayBuffer` and `MessagePort`; `SharedArrayBuffer` shares | The worker script's URL | Nothing static; clone fails at run time |
| Chapel | An `on` clause moves the running task to a locale; `on x` runs where `x` lives | Same program on every locale | Locality is separate from parallelism |
| ML5 | Types name the world a value lives at; resources stay at their world | The compiler emits code for every host | Modal type system rejects unsafe use of mobile resources |
| ScalaLoci | `Transmittable[B, I, R]` picks how a value is transmitted; the received type may differ, as an event stream | The compiler places each value per peer | Placement types, tie rules, and a coherence check between both ends |
| Spores 3, Cloud Haskell | A closure with checked captures; a static key for its code | A key valid within one binary | Capture restrictions; `static` forms |
| Unison | Code by content hash; the receiver fetches what it lacks | Content hash | Abilities handled at the destination |
| JAX, CUDA | A traced or kernel function runs on a device; host and device memory are separate | Compiled for the device | Only device operations; Python side effects run once at trace time |

**Takeaways.**

1. Systems that cross machines name installed code and send data: React
   Server Functions, ScalaLoci, Spores, Cloud Haskell. Only Unison ships
   code, and it does so by hash. hd's same-build rule fits this family.
2. A destination type that differs from the source type is established
   prior art in ScalaLoci and in device runtimes. ScalaLoci needed a
   coherence check because Scala implicits are not globally unique; hd's
   orphan and overlap rules already make one implementation per type.
3. Threads in one heap are a permission question, not a representation
   one. Rust, Swift, Pony and OxCaml each answer it with markers or modes
   on references. None of them serializes.

## 4. Option A: Named Code, Copied Data

**The idea.** Only named functions run elsewhere, and only boundary-safe
copies travel. No closure crosses, so no capture rule is needed: the
caller passes the environment as arguments. This is the radical
simplification. It is #166's `@remote` half without portable closures,
`Opaque`, queries or places.

```text
use std.worker.{WorkerError, Workers}

# A named function that another instance of this build may run.
@remote
pub fn score_chunk(loans: List[Loan], model: Model) -> List[f64]:
    [for l in loans => score(l, model)]

fn total_risk!(chunks: List[List[Loan]], model: Model) -> Result[f64, WorkerError] $ Workers:
    let total = 0.0
    for part in $.use(Workers).map!(score_chunk, chunks, model)?:
        total = total + part.sum()
    .Ok(total)
```

A closure in the function position is an error, with a placeholder code:

```text
fn by_closure!(chunks: List[List[Loan]], model: Model) -> Result[List[List[f64]], WorkerError] $ Workers:
    $.use(Workers).map!(fn(c: List[Loan]) -> List[f64]: score_chunk(c, model), chunks, model)  # error: remote-no-place
```

| Rule | Tier | Cost-order kind |
| --- | --- | --- |
| `@remote` marks a top-level function another instance may run; its signature is boundary-safe and has no type parameters. | language | intrinsic (the compiler knows the mark) |
| A call that may cross takes only an `@remote` function. | language | intrinsic |
| `Workers`, `map!`, `WorkerError` | stdlib | core library |
| The crossing itself | none new | reuses [`module.boundary.allowed`](../../spec/lang/10-modules.md#r-module.boundary.allowed) and the host codecs |

- **Soundness.** Nothing lent leaves the instance, so source rights never
  change. Abandoning a call aborts the host operation by its own rule.
- **Interaction.** Rows place the function, as for a registered entry.
  Derivation is untouched; `Serialize` decides private fields. The RC
  runtime is untouched, since heaps stay disjoint.
- **Gives up.** U2 needs `SharedBytes` from the parallelism record. U3,
  U5's residency, and first-class closures (a stored action, a pushed-down
  predicate) are out.

## 5. Option B: Portable Closures Placed By Rows

**The idea.** A closure literal becomes data: its content identity plus
copied, boundary-safe captures. The destination runs the lifted function
it already has, and binds the closure's row itself. This is the #166
design, read against tier 2.

```text
use std.remote.{Portable, RemoteError, portable}

pub trait Workers:
    fn map![A, B](self, items: List[A], job: Portable[fn(A) -> B]) -> Result[List[B], RemoteError]

fn total_risk!(loans: List[Loan], model: Model) -> Result[f64, RemoteError] $ Workers:
    scores := $.use(Workers).map!(loans, portable(fn(l: Loan) -> f64: score(l, model)))?
    .Ok(scores.sum())

fn counted!(loans: List[Loan], model: Model, hits: *mut Counter) -> Result[List[f64], RemoteError] $ Workers:
    $.use(Workers).map!(loans, portable(fn(l: Loan) -> f64: score(l, model) + f64(hits.n)))  # error: portable-capture-not-boundary-safe
```

| Rule | Tier | Cost-order kind |
| --- | --- | --- |
| `portable(fn …)` takes a closure literal and makes `Portable[F]` | language | intrinsic |
| Every capture is boundary-safe; no capture is assigned or mutated | language | intrinsic (a check on the literal) |
| A content identity hashes the lifted body and its callees, same build only | language | intrinsic |
| `@remote`, and `remote!(f, args)` returning `Result[T, RemoteError]` (the call form decided 2026-10-17) | language | intrinsic |
| `[places]` in `hd.toml`; a place binds the row | CLI | syntax (a manifest table) |
| `Opaque[T]`, `OpaqueStore`, `@query` | stdlib and language | core library plus intrinsic |

Tier 2 adds little work here. A captured `&T` is already
`local-reference-escapes` in a stored closure, and a `*mut` is not
boundary-safe. `FnMut` matches the "no mutated capture" rule: a portable
closure is always `Fn`.

- **Soundness.** As A: copies only, so no permission returns after
  failure. The new surface is the content identity, which must cover type
  layouts and callees.
- **Interaction.** Rows place code, providers are never captured
  ([`fn.capture.no-providers`](../../spec/lang/07-functions.md#r-fn.capture.no-providers)),
  and a portable call is an ordinary suspension, so `race!` and `abandon()`
  work unchanged. Derivation is untouched.
- **Gives up.** U2 and U3 as in A. U5: a GPU place takes portable kernels,
  but the device-buffer type and residency are deferred.

## 6. Option C: Typed Crossing Contracts

**The idea.** A trait per crossing gives each type a destination type and a
transport. A destination-typed block names its captures and their modes,
and its body type-checks against the destination types and the
destination's row. This is the owner's note; the review's main advice is
to make the environment judgment, not the trait, the primary check.

The owner's spelling, for U1 and U5:

```text
h := at Workers capture { model = copy(model), part = copy(chunk) } return copy do: score_all(part, model)  # hypothetical syntax
g := at Gpu0 capture { pixels = upload(image) } return readback do: reduce_sum(pixels)  # hypothetical syntax
```

The trait in hd's own syntax. The device type names its device, which is
the review's "destination instance" point:

```text
pub struct Gpu0: pass
pub struct Upload: pass

pub struct HostImage:
    pub pixels: List[f32]

pub struct DeviceImage[D]:
    handle: u64

pub trait Across[D, M]:
    type There
    fn cross!(self) -> Result[Self::There, CrossError]

impl Across[Gpu0, Upload] for HostImage:
    type There = DeviceImage[Gpu0]
    fn cross!(self) -> Result[DeviceImage[Gpu0], CrossError]:
        .Ok(DeviceImage { handle: upload_pixels!(self.pixels)? })
```

The capture judgment rejects `x`, and ordinary type checking rejects `y`
and `z`:

```text
x := at Worker capture { a = copy(p), b = share(p) } do: a.n + b.n  # hypothetical syntax
y := at Gpu1 capture { pixels = use_existing(on_gpu0) } do: reduce_sum(pixels)  # hypothetical syntax
z := at Gpu0 capture { pixels = upload(image) } do: host_scan(pixels)  # hypothetical syntax
```

`x` gives one object two meanings. `y` uses a `DeviceImage[Gpu0]` on
another device, a type error once `D` is in the type. `z` calls a host
function on a device type, a type error in the body.

| Rule | Tier | Cost-order kind |
| --- | --- | --- |
| `at D capture { … } return r do body` | language | syntax |
| The body captures nothing outside its capture clause | language | semantic rule (a new closure check) |
| The capture judgment: modes, Δ_B, Γ_A′ | language | semantic rule |
| Copy is derived for boundary-safe types; other crossings are written | language | intrinsic |
| Mode types (`Copy`, `Move`, `Share`, `Proxy`, `Upload`) and `Across` | language if the compiler names them, else stdlib | intrinsic or core library |
| Body checked against D's row and operations | language | reuses row checks per place |

- **Soundness.** A user `cross!` runs effects and can fail half-way
  through a capture clause. The review asks who owns what was prepared.
  hd's `using` could own each prepared handle. But an abandoned async
  dispose is cut short ([`flow.using.async.cancel.cut-short`](../../spec/lang/06-control-flow.md#r-flow.using.async.cancel.cut-short)),
  which leaks a resident remote resource. Move and share need
  `~Copyable` and `Send`/`Sync` first.
- **Interaction.** Traits and coherence carry the protocol choice; hd
  needs no ScalaLoci-style coherence check within one build. The handle
  could be a plain `mut Suspend[R]`, so `race!` and `abandon()` apply. A
  three-way outcome fits a positional union or an enum. Derivation would
  add a derived copy crossing per type.
- **Gives up.** First-class closures: `map_at(dest, f, items)` needs a
  separate kind of function value, which the review flags as open. The
  surface is the largest of the four.

## 7. Option D: Send, Sync, And Scoped Threads

**The idea.** Threads share one heap, so nothing is encoded. Marker traits
say what may move to another thread and what may be shared, and a scope
joins its threads before it returns. The tier 2 checker supplies the
source's rights: a `&` lent to children blocks writes, and a moved
`&mut` is unusable after. This is Rust's model, and the owner's planned
`Send` and `Sync`.

```text
use std.thread.{Scope, scope}

fn total_risk(loans: &List[Loan], model: &Model) -> f64:
    scope(fn(s: &mut Scope) -> f64:
        half := loans.len() / 2
        left := s.spawn(fn() -> f64: score_range(loans, model, 0, half))
        right := score_range(loans, model, half, loans.len())
        left.join() + right
    )

fn racy(hits: *mut Counter) -> void:
    scope(fn(s: &mut Scope) -> void:
        s.spawn(fn() -> void: hits.n += 1)  # error: not-send
        hits.n += 1
    )
```

| Rule | Tier | Cost-order kind |
| --- | --- | --- |
| `Send` and `Sync` marker traits, derived by structure | language | intrinsic |
| `spawn` needs its closure to be `Send`; a captured `&T` needs `T: Sync` | language | semantic rule |
| A `*mut T` is neither, unless its target is frozen or locked | language | semantic rule |
| `scope`, `Scope`, `spawn`, `join` | stdlib | core library |
| Atomic counts or a frozen region for shared objects | runtime | not a language rule; large runtime work |

- **Soundness.** Today one instance's runtime keeps its run-time
  loans, so loans would need to be per thread or atomic. A scope must be
  synchronous: if it could suspend, `abandon()` would have to wait for
  other threads, against [`req.abandon.unwind`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.unwind).
  With a synchronous scope, abandoning mid-scope panics by
  [`req.abandon.active`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.active).
- **Interaction.** Captured local references in a direct `fn(...)` argument
  are already legal ([`fn.capture.local-parameter`](../../spec/lang/07-functions.md#r-fn.capture.local-parameter)),
  so `spawn` reuses that rule. `let` cells shared by closures
  ([`fn.capture.cell`](../../spec/lang/07-functions.md#r-fn.capture.cell)) are not `Sync`.
  The RC runtime needs atomic counts or a frozen region; top-level `let`
  bindings become shared state.
- **Gives up.** Every boundary that is not one heap: U4, U5's device, U6.
  It is the thread half of the owner's note only, and waits for a shared
  heap on Wasm.

## 8. Comparison

| | A: Named code, copied data | B: Portable closures (#166) | C: Typed contracts (owner's note) | D: Send, Sync, scoped threads |
| --- | --- | --- | --- | --- |
| U1 fan-out | Yes, data passed as arguments | Yes, closure captures copied | Yes, captures copied by mode | Yes, no copy; needs a shared heap |
| U2 64 MB hand-off | With `SharedBytes` only | With `SharedBytes` only | A `move` or `share` mode | A moved value, no copy |
| U3 scoped parallel read | No | No | A `lend` mode, with the abandon problem | Yes |
| U4 server call | Yes, `@remote` | Yes, plus `Opaque` and `Caller` | Yes, `at Server` | No |
| U5 resident GPU data | No | Kernel only; buffer type deferred | Yes: `There`, device in the type | No |
| U6 resume later | Named entries replay | Named entries replay | Needs a durable domain's rules | No |
| Rules added, costliest kind | Intrinsic (`@remote`) | Intrinsic (`portable`), manifest syntax | Syntax (`at … do`) plus semantic rules | Semantic rules plus runtime change |
| Soundness risk | Lowest: copies only | Low: content identity must be complete | User transports, partial preparation, permission return | Data races if a marker is wrong; abandon across threads |
| Agent-writability | One mark, plain arguments | One wrapper on a literal | A capture clause per call; modes to choose | Rust-shaped, familiar errors |
| Human readability | Reads as a function call | Reads as a closure | Every crossing is visible | Familiar from Rust |
| Implementation cost | Small: a mark, host spawn, codecs exist | Medium-large: 31 slices planned | Large: new syntax, judgment, per-domain backends | Large: shared heap, atomic RC, markers |
| Evolution | B or C can grow from it | C's copy slice equals B's capture rule | Contains A and B as its copy mode | Composes with C's thread domain later |

**Review gaps, option by option.**

| Gap from the review | A | B (#166) | C (owner's note) | D |
| --- | --- | --- | --- | --- |
| G1 Destination-typed body vs shipping an existing closure | Closed: only named code | Closed: literals only, same build | Chooses destination-typed bodies; higher-order `map_at` open | Not needed: same code, same heap |
| G2 Environment judgment over per-type rules (aliasing between captures) | Closed: tree copies, aliasing duplicated by rule | Closed the same way | Open until the judgment is primary | Closed by the tier 2 exclusivity checker plus markers |
| G3 Destination instance identity | Not needed: nothing resident | Open: GPU deferred | Open; a device parameter in the type closes it | Not needed |
| G4 Permission return after failure or unknown outcome; `abandon()` with outstanding borrows | Closed: nothing lent | Closed: nothing lent | Open for move and lend | Closed only for a synchronous scope |
| G5 Transport effects and partial preparation | Closed: encode only | Closed: encode only | Open: user `cross!` has effects | Not needed: no transport |
| G6 Transport laws and endpoint coherence | Closed: same build, call fingerprint | Closed: same build, content identity | Coherence closed by hd's impl rules within one build; laws open | Not needed |
| G7 Result vs proxy meaning of `There` | Not needed | Partly: `ServerRefs` is a proxy for values | Open: one `There` carries both | Not needed |
| G8 Transportable vs allowed to disclose | Partly: `Serialize` on private fields; authorization in code | Partly: `Opaque`, trust per edge, `Caller` | Open: the note asks for information-flow labels | Not needed |
| Two-backend milestone with a changed type | Not possible | Not possible | Its intended test | Not possible |

## Recommendation

**Recommendation.** Build in this order. A comes first, when worker work
is scheduled, and B's portable closures next. C's typed `There` waits for
a resident backend, and D for the owner's `Send` and `Sync`.

- A covers U1, U4 and U6 with one mark and no new rule about captures.
  It follows the prior-art majority: name installed code, send data.
- B adds first-class closures on top of A without changing it. Its
  capture rule is C's copy mode, so nothing is thrown away if C follows.
- C's copy-only slice adds the capture clause and destination-typed body.
  Its value shows only with a second backend whose type differs (U5).
  The review names that the decisive experiment, and hd has no such
  backend today.
- Move, lend and share need `~Copyable` (#535), `Send`/`Sync` and a shared
  heap. Specifying them earlier would specify against unknown core rules.
- D is the best-known answer for threads, and the review's G2 and G4 land
  on hd's tier 2 checker there, not on a transport trait.

What this gives up: no zero-copy hand-off of values before a shared heap,
and no typed device buffers until a GPU study. The next best option is C
first, with copy mode only, as the owner's note's own first experiment.
It costs new syntax before any backend needs it.

## Questions For The Owner

1. **How does code reach the destination?** A worker or server must
   already have the code. Options: (a) named `@remote` functions only;
   (b) also portable closure literals, first-class values (#166); (c) a
   destination-typed `at D … do` block. Recommended: (a) first, then (b).

   ```text
   pool.map!(score_chunk, chunks, model)                                       # (a)
   pool.map!(loans, portable(fn(l: Loan) -> f64: score(l, model)))             # (b)
   ```

2. **Is the capture list written or inferred?** Writing it shows every
   crossing and its mode; inferring it keeps closures short. Options: (a)
   inferred, every capture copied (#166); (b) written, as the owner's
   `capture { … }`. Recommended: (a) while copy is the only mode; (b) once
   a second mode exists.

   ```text
   h := at Workers capture { model = copy(model) } do: score_all(chunk, model)  # hypothetical syntax
   ```

3. **May the destination see a different type?** A GPU sees a device
   buffer, not the host list. Options: (a) never: the same type both sides;
   (b) a `There` type per crossing, with the device named in the type.
   Recommended: (a) until a resident backend is scheduled.

   ```text
   impl Across[Gpu0, Upload] for HostImage:
       type There = DeviceImage[Gpu0]
       fn cross!(self) -> Result[DeviceImage[Gpu0], CrossError]: upload_image!(self)
   ```

4. **Does code name the destination?** The 2026-10-17 decision for #166 says
   code never names a place; the new note writes `at Worker`. Options: (a)
   the deployment configuration only; (b) a value in code, such as a
   `Workers` or `Gpu` provider; (c) a type in code, as `at Gpu0`.
   Recommended: (b) for workers and devices, keeping (a) for services.

   ```text
   scores := $.use(Workers).map!(score_chunk, chunks, model)
   ```

5. **When do move, lend and share modes come?** They decide what the
   source may still do after a crossing. Options: (a) after `~Copyable`
   and `Send`/`Sync` are decided; (b) now, for `&mut` and `SharedBytes`
   only. Recommended: (a); `SharedBytes` transfer stays a run-time close.

   ```text
   to.send!(chunk.take(n))  # SharedBytes: the sender's handle is closed at run time
   ```

6. **May another thread borrow from a suspension?** A borrowed capture in
   a suspending scope meets `abandon()`, which unwinds at once. Options:
   (a) only a synchronous scope may lend, as Rust's `thread::scope`; (b)
   `abandon()` waits until the other thread stops using the loan.
   Recommended: (a); it keeps `req.abandon.unwind` as it is.

   ```text
   total := scope(fn(s: &mut Scope) -> f64: s.spawn(fn() -> f64: sum(loans)).join())
   ```

## Sources

- The owner's note, "Types and Transport Across Execution Boundaries" (gist, 2026-10-31), and a ChatGPT review of it (reference material, not decisions).
- #166 study: <https://github.com/HerringtonDarkholme/hd-lang/blob/28072b8a1/docs/future-work/SERIALIZABLE_CLOSURE_DESIGN.md>
- Rust, `thread::spawn`: <https://doc.rust-lang.org/std/thread/fn.spawn.html>; `thread::scope`: <https://doc.rust-lang.org/std/thread/fn.scope.html>; `Send`: <https://doc.rust-lang.org/stable/std/marker/trait.Send.html>
- Swift SE-0302, `Sendable`: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0302-concurrent-value-and-concurrent-closures.md>
- Swift SE-0414, region-based isolation: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0414-region-based-isolation.md>; SE-0430, `sending`: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0430-transferring-parameters-and-results.md>
- Swift SE-0336, distributed actor isolation: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0336-distributed-actor-isolation.md>
- Go, Effective Go, sharing by communicating: <https://go.dev/doc/effective_go#sharing>; race detector: <https://go.dev/doc/articles/race_detector>
- Pony, reference capabilities: <https://tutorial.ponylang.io/reference-capabilities/reference-capabilities.html>
- OxCaml parallelism tutorial: <https://oxcaml.org/documentation/tutorials/intro-to-parallelism-part-1/>
- HTML structured data (clone and transfer): <https://html.spec.whatwg.org/multipage/structured-data.html>; MDN, transferable objects: <https://developer.mozilla.org/en-US/docs/Web/API/Web_Workers_API/Transferable_objects>
- Chapel, on clauses: <https://chapel-lang.org/docs/users-guide/locality/onClauses.html>
- Murphy, Crary, Harper, "Type-Safe Distributed Programming with ML5", TGC 2007: <https://kilthub.cmu.edu/articles/journal_contribution/Type-Safe_Distributed_Programming_with_ML5/6611192/1>
- Weisenburger, Salvaneschi, "Implementing a Language for Distributed Systems", Programming 2020, §6.3 transmitters, §6.4 coherence: <https://scala-loci.github.io/publications/2020-programming/paper.pdf>
- Weisenburger, Köhler, Salvaneschi, "Distributed System Development with ScalaLoci", OOPSLA 2018: <https://doi.org/10.1145/3276499>
- Spores 3: <https://github.com/phaller/spores3>; Miller, Haller, Odersky, "Spores", ECOOP 2014: <https://doi.org/10.1007/978-3-662-44202-9_13>
- GHC, static pointers: <https://downloads.haskell.org/ghc/latest/docs/users_guide/exts/static_pointers.html>
- Unison, distributed datasets with the `Remote` ability: <https://www.unison-lang.org/articles/distributed-datasets/>
- JAX, just-in-time compilation and side effects: <https://docs.jax.dev/en/latest/201/jit.html>; CUDA programming model: <https://docs.nvidia.com/cuda/cuda-programming-guide/01-introduction/programming-model.html>
- React Server Functions: <https://react.dev/reference/rsc/server-functions>
- Spark, closures: <https://spark.apache.org/docs/latest/rdd-programming-guide.html#understanding-closures>

## Parse Log

All 15 `text` blocks were parsed on 2026-10-31 with
`scripts/hd-conformance.sh parse`, which checks syntax only. Nothing was
type-checked, and the blocks use std names (`std.worker`, `std.remote`,
`std.thread`, `Across`) that this record only sketches. The indented
blocks under the questions were parsed after removing their indent.

| Block | Where | Result |
| --- | --- | --- |
| 1 | 1, closure conversion | parses |
| 2 | 1, hazards | parses |
| 3 | 4, option A | parses |
| 4 | 4, option A error case | parses |
| 5 | 5, option B | parses |
| 6 | 6, option C, the owner's spelling | fails as expected: every line is `# hypothetical syntax` (`at D capture … do`) |
| 7 | 6, option C, `Across` in hd | parses |
| 8 | 6, option C, rejected crossings | fails as expected: every line is `# hypothetical syntax` |
| 9 | 7, option D | parses |
| 10 | Question 1 | parses |
| 11 | Question 2 | fails as expected: `# hypothetical syntax` |
| 12 | Question 3 | parses |
| 13 | Question 4 | parses |
| 14 | Question 5 | parses |
| 15 | Question 6 | parses |

Each `# error:` comment names the diagnostic the option would give. The
codes `remote-no-place` and `portable-capture-not-boundary-safe` are #166's
placeholders, and `not-send` is a placeholder too.
