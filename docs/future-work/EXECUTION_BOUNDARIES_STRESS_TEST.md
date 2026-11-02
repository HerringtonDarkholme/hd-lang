# Execution Boundaries: Stress Test

Status: design review, 2026-11-02; changes no decision, design record, spec
text, or compiler code. It tests the decisions of
[Execution Boundaries](EXECUTION_BOUNDARIES.md) (PR #1139): the same-code
principle ([section 1](EXECUTION_BOUNDARIES.md#1-context-and-goal)), the
domain properties, kinds, modes and lifecycle rules
([section 3](EXECUTION_BOUNDARIES.md#3-the-model)), the checks of
[section 5](EXECUTION_BOUNDARIES.md#5-type-checking), the process of
[section 6](EXECUTION_BOUNDARIES.md#6-the-process-end-to-end) (rules A1 to
A4, C1 to C3, B1 to B5 and its what-cannot-cross table), and questions 1
and 3 to 17 ([section 8](EXECUTION_BOUNDARIES.md#8-questions-for-the-owner)),
as of commit 742fb8d4f. Two spellings of a cross-target call are compared:
V1, the `@remote(D)` form the record dropped in section 7, and V2, the one
crossing shape it now uses. The spec text it leans on is
[Boundary-Safe Values](../../spec/lang/10-modules.md#boundary-safe-values),
[Registration](../../spec/lang/10-modules.md#registration),
[Union Types](../../spec/lang/04-type-system.md#r-types.union.syntax),
[Supertraits](../../spec/lang/09-traits.md#r-trait.super.bound),
[Error Conversion](../../spec/lang/05-expressions.md#r-expr.try.convert),
[Cancellation](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.bang)
and [Abandoning](../../spec/lang/11-requirements-and-suspension.md#abandoning),
and the stdlib tier's [Timeout](../../spec/std/task.md#timeout) and
[Mutable Spans](../../spec/std/collections.md#mutable-spans).

## Contents

- [Surface Being Tested](#surface-being-tested)
- [Method](#method)
- [Summary](#summary)
- [Cases](#cases)
- [Comparison With Other Languages](#comparison-with-other-languages)
- [Problems, Ranked](#problems-ranked)
- [Owner Question A: How The Compiler Compiles It](#owner-question-a-how-the-compiler-compiles-it)
- [Owner Question B: Can A Reader See A Remote Call?](#owner-question-b-can-a-reader-see-a-remote-call)
- [Owner Question C: Uniform, Simple, Orthogonal?](#owner-question-c-uniform-simple-orthogonal)
- [Sources](#sources)
- [Parse Log](#parse-log)

## Surface Being Tested

| ID | What the report assumes | From |
| --- | --- | --- |
| S1 | Every domain runs the same program; a crossing sends a code identity and data, never code. Different builds are out of scope. | section 1 (approved) |
| S2 | Four properties (memory, capabilities, code, lifetime) and five kinds, plus the background thread as a sixth. | section 3 (approved), question 16 (proposed) |
| S3 | Four modes, inferred from the value's type: copy, transfer, lend, handle; the domain-by-mode table. | section 3, question 7 |
| S4 | L1 to L4. | section 3 (approved) |
| S5 | Required affinity through `at(handle)`; preferred affinity is a run-time hint. | questions 6, 11 |
| S6 | A crossing's failure is a `CrossError` with an `Unknown` case. | question 1 |
| S7 | Workers, devices and background threads are provider values from the row; a handle carries its home in its type (`Texture[Gpu0]`). | questions 3, 4, 8, 9 |
| S8 | Transfer needs a non-copyable type ([#1141](https://github.com/HerringtonDarkholme/hd-lang/issues/1141)); separate-memory code is bounded `Fn`; `Sync` comes later. | questions 5, 10, 12 |
| S9 | One program, one build per target. | question 15 |
| S10 | Crossing code reaches no `let mut` top-level binding; immutable ones are allowed. | question 17 |
| S11 | The what-cannot-cross table: per domain kind, what a copyable value, a non-copyable value, a reference, a box, a closure, a handle and code do. | section 6, Checking It |
| S12 | C2 checks each crossing call; C3 resolves the body's `$.use` at the destination; B4 names same-build code by its function-table index; B5 stamps a build ID and refuses a peer from another build. | section 6 |

### Variants

The variants differ only in how a cross-target call is written. S1 to S12
hold in both; rules A1 to A4, B2 and B3 belong to V2.

| | V1: dropped in section 7 | V2: the record now (question 14) |
| --- | --- | --- |
| Cross-target call | `load_user!(id)` on an `@remote(Server)` function | `users.load_user!(id)` on a provider of a trait `Users < Remote` (A1) |
| Caller's row | `$ Server`, met by a stub the build provides | `$ Users`, an ordinary provider row |
| Endpoints | every `@remote(D)` function, in D's build | the methods of a `Remote` trait, exposed by `serve!(impl)` (A4, B2) |
| Client side | a stub compiled in place of the body | a generated provider, `$.with(Users=remote::[Users](url))` (A4, B3) |
| Failure type | widened: `Result[T, E \| CrossError]` | declared: `E` must hold a `CrossError`; nothing is widened (A3) |
| Signatures | boundary-safe parameters and results | the same, and not generic; no closure, reference or trait value (A2) |
| Devices | a domain value, `gpu.run!(blur, tex)` | another build: a trait that extends `Kernel` (A1) |
| Same-build domains | `workers.call!(...)` and kin | the same shape: `workers.run!(...)`, `s.spawn(...)`, `rename.call!(text)` on an `Action` |
| Tests | `$.with(Server=LocalServer {})` | a fake provider (A4) |

V2's surface, abridged from section 6:

```text
@error
pub enum UserError:
    @error("no user $id")
    Missing(id: i64)
    @error("lost contact: $cause")
    Cross(cause: CrossError)

pub trait Users < Remote:
    fn load_user!(&mut self, id: i64) -> Result[User, UserError]

pub fn server_main!() -> Result[void, PgError] $ Console + Net:
    pool := connect!("postgres://localhost/app")?
    serve!(PgUsers { pool })

pub fn browser_main!() -> void $ Dom + Net:
    $.with(Users=remote::[Users]("https://api.example.com")):
        on_open!(7)

pub fn on_open!(id: i64) -> void $ Dom + Users:
    let mut users = $.use(Users)
    match users.load_user!(id):
        .Ok(user) => show("Hi, ${user.name}")
        .Err(error) => show_error("$error")
```

Where the record leaves an API open, the examples assume:

- **X1 (question 13, recommended, not answered).** The deployment lists
  each target as an entry module, a runtime profile and an output kind. A
  target's capability set is what its host binds plus what its `main`
  installs with `$.with` and `serve!`. Under V1 each target also binds a
  stub provider for every other target's domain key.
- **X2.** Under V1, `Server`, `Api` and `Billing` are domain keys that
  `$.with` can swap; their trait methods are not specified.
- **X3.** Under V1, a test build compiles both an `@remote` body and its
  callers.
- **X4.** Std names (`Workers`, `Background`, `Jobs`, `Gpu`, `scope`, `at`,
  `Texture`, `Partition`, `Stream`, `Remote`, `remote`, `serve`) are
  sketches. Section 5 spells the error `CrossingError`; this report uses
  `CrossError`.

## Method

**Cases.** Eighteen shapes, each approximating a well-known library's
public API: 9 library cases, 4 operation cases and 5 hard cases. Library
code is abridged to a few lines; nothing is copied at length. Each case
shows V1; a V2 block follows where V2's shape differs.

**Verdicts.**

- *works*: the variant expresses the shape, and a user sees the same
  behavior as in the original.
- *friction*: it works with a workaround, or a user sees a different
  behavior.
- *breaks*: the variant cannot express it, or accepts code that gives a
  different answer (a race, a silent divergence).

**Counting rules.** Question B counts crossing call sites in this report's
blocks: one call expression that starts a crossing is one site; error
sketches are not counted. Question C counts one construct per name the
compiler must know, and one rule per table cell, list item or numbered
rule the record states.

**Limits.** Every `text` block is parsed with
`scripts/hd-conformance.sh parse`, which checks syntax only. Nothing is
type-checked, except five probes run with `hd check`, each named where it
is used. Behavior statements follow the record and the cited spec rules;
none is measured. Size estimates in question A are rough and unmeasured.

## Summary

| # | Case | Library approximated | V1 | V2 | Design element at fault |
| ---: | --- | --- | --- | --- | --- |
| 1 | Thumbnail in a web worker | Comlink, transferables | friction | friction | transfer waits for #1141; no progress channel; skew |
| 2 | Form action | Next.js `"use server"` | friction | friction | per-request session; bound actions; skew |
| 3 | Typed service | tRPC, gRPC | friction | friction | cross-release clients out of scope |
| 4 | Parallel count and sort | `std::thread::scope`, rayon | breaks | breaks | two threads write one captured cell |
| 5 | Heavy work off the UI thread | Kotlin `withContext`, Swift `Task.detached` | works | works | none |
| 6 | Delayed welcome mail | Sidekiq, Temporal | breaks | breaks | a job outlives its build |
| 7 | Image kernels | wgpu, CUDA | friction | breaks | no launch geometry; one type per device; V2: A2 bars the texture from a `Kernel` method |
| 8 | Data-local count | Ray, Spark, Dask | breaks | breaks | partitions on run-time nodes |
| 9 | Long-lived account process | Erlang, Akka | breaks | breaks | dynamic instances; one-way sends |
| 10 | Test swaps the server | MSW, Swift local actor system | friction | works | V1: local server semantics unstated |
| 11 | Error propagation | tonic, thiserror | friction | friction | V1: `?` fails on the union; V2: a dead `Cross` case |
| 12 | Two crossings in `all!` | `Promise.all`, Go errgroup | works | works | none |
| 13 | Cancel mid-crossing | Go `context`, gRPC deadline | friction | friction | a timeout is not `.Unknown` |
| 14 | A remote call inside a remote call | gRPC service chains | friction | friction | V1: domain of unmarked code; V2: capabilities move to fields |
| 15 | Remote function with a closure or handle | Next.js bound actions, Ray `ObjectRef` | breaks | breaks | handle row vs plain-data signatures |
| 16 | Generic remote function | Swift generic `distributed func` | friction | breaks | V2 forbids generic methods |
| 17 | Streaming results | gRPC streaming, tRPC subscriptions | breaks | breaks | one result per crossing |
| 18 | Background result updates UI state | Kotlin `StateFlow`, SwiftUI | friction | friction | the box is refused, but progress has no form |

Totals: V1 has 2 works, 10 friction, 6 breaks. V2 has 3 works, 7 friction,
8 breaks.

| Decision under test | Evidence | Status in this test |
| --- | --- | --- |
| Same-code principle (S1) | cases 1, 2, 3, 6 deploy a second build while the first still talks | strained: P1 |
| Properties and kinds (S2) | cases 8, 9 need run-time instances; case 8 needs trust, which no property states | strained: P3, P8 |
| Background thread (q16) | cases 5, 18 | holds for both once the table refuses boxes; needs P12; "a crash is a `CrossError`" meets instance poisoning: P12 |
| Modes and the table (S3) | cases 1, 4, 18 | modes hold; section 5 still says any copyable value copies in shared memory, which the section 6 table contradicts: P2, P19 |
| L1 to L4 (S4) | cases 4, 6, 13 | L1, L2 hold; L3 with L4 is unclear across restarts: P14 |
| Affinity (q6, q11) | cases 7, 8 | holds for one handle; a list of handles breaks: P3 |
| Outcomes (q1) | cases 11, 13, 14 | the case holds; timeouts and propagation strain it: P10, P11 |
| q3, q4 | cases 7, 8, 9 | static instances only: P3 |
| q5, q7 | case 1 | holds; the 50 MB copy is today's only option |
| q8 | cases 1, 14 | the row spelling of `Workers[$ Log]` is open: P19 |
| q9 | cases 7, 15 | holds |
| q10 | case 8 | holds, and catches Spark's silent counter bug |
| q12 | cases 4, 18 | the gap is wider than lends: cells and boxes in scoped threads: P2 |
| q14 | cases 2, 3, 10, 11, 14 to 17 | V1 had two shapes (P5, P6); V2 has one; both hide the crossing at the site: P7 |
| q15 | cases 1, 2, 3, 6 | holds inside one deploy: P1 |
| q17 | case 18 and a probe | an immutable binding to a box escapes it: P4 |
| A1 | cases 3, 7 | holds for services; a `Kernel` trait must take handles, which A2 bars: P8 |
| A2 | cases 7, 15, 16 | plain data holds for services; bars kernels' textures and generic endpoints: P8 |
| A3 | cases 11, 13 | `?` works; every server impl and fake carries a dead `Cross` case: P11; section 4 still widens: P19 |
| A4 | cases 2, 10, 14 | fakes work; a server impl gets no row of its own: P16 |
| C1 | cases 7, 16 | holds as stated; see A2 |
| C2 | cases 4, 8, 18 | holds for separate memory; scoped threads still take boxes and cells: P2 |
| C3 | cases 2, 14 | empty for services: an impl method's row equals the trait's, so its `$.use` finds nothing to resolve: P16 |
| B1 | the section 6 program | holds |
| B2, B3 | cases 2, 14 | hold; the stub's conversion into `E` is unstated: P18 |
| B4 | cases 1, 5 | holds |
| B5 | cases 1, 2, 3, 6 | the refusal is loud, but it fails every open page and every pending job at each deploy: P1 |
| What-cannot-cross table | cases 1, 4, 6, 15, 18 | closes the background-thread box (case 18); leaves scoped threads open (case 4); its service handle cell contradicts A2: P2, P8 |

The model holds where one program talks to itself inside one deploy, with
static domains: cases 5 and 12, and the plain paths of 1, 2, 3 and 7. It
fails in three families, under both variants. Real deploys keep old
clients and old jobs alive, so two builds talk; B5 now refuses them, which
turns every deploy into failed calls and lost jobs (P1). Actors,
partitions and multiple GPUs pick their domain instance at run time (P3).
Scoped threads still take boxes and captured cells, which race, and a
top-level box escapes question 17 (P2, P4). The variants differ on cases
7, 10, 11, 14 and 16 and on the questions below.

## Cases

### 1. Thumbnail In A Web Worker (Comlink, Transferables)

```js
// worker.js
Comlink.expose({ thumbnail: (px, w, h, side) => shrink(px, w, h, side) });
// main.js
const api = Comlink.wrap(new Worker(new URL("./worker.js", import.meta.url)));
const small = await api.thumbnail(Comlink.transfer(px, [px.buffer]), w, h, 256);
// px.byteLength === 0 now: the buffer moved
await api.scan(px2, Comlink.proxy((p) => bar.value = p));   // progress callback
```

```text
pub fn on_export!(photo: Image) -> Result[Image, CrossError] $ Workers:
    workers := $.use(Workers)
    small := workers.call!(fn() -> Image: shrink(photo, 256))?    # copied: 50 MB encoded and decoded
    .Ok(small)

pub fn on_export_live!(photo: Image, bar: *mut Progress) -> Result[Image, CrossError] $ Workers:
    workers := $.use(Workers)
    workers.call!(fn() -> Image: shrink_reporting(photo, fn(p: f64) -> void: bar.set(p)))  # error: a closure and a box are not boundary-safe

# Pixels is non-copyable, once #1141 lands
pub fn on_export_moved!(photo: Pixels) -> Result[Pixels, CrossError] $ Workers:
    workers := $.use(Workers)
    let (back, small) = workers.call!(fn() -> (Pixels, Pixels): thumbnail(photo, 256))?
    _ := photo.width    # error: use-of-moved-value; photo moved into the worker
    .Ok(small)
```

**Behavior.** The first form copies, as Comlink does without
`transfer`. The transfer form needs a non-copyable type, which waits for
#1141 (question 5). The progress form has no counterpart: Comlink's
`proxy` is a handle to a page closure, and section 5 rejects it.

**Breaks.** Comlink workers often keep a loaded model or cache between
calls. The only hd home for it is a handle owned by one worker, which
question 4's static types cannot name for a pool (P3). A top-level `box`
cache is accepted, and each worker silently gets its own (P4). A page
open across a deploy may spawn its worker from the new build's script
(P1).

**V2.** The same code, with `run!` for `call!`. **Verdict: V1 friction;
V2 friction.**

### 2. Form Action (Next.js "use server")

```tsx
// app/actions.ts
"use server";
export async function renameTodo(id: string, form: FormData) {
  const session = await auth();
  await db.todo.update({ where: { id, ownerId: session.userId }, data: { title: form.get("title") } });
}
// in a server component: the id is bound and sealed into the action
<form action={renameTodo.bind(null, todo.id)}>...</form>
```

```text
@remote(Server)    # hypothetical syntax
pub fn rename_todo!(id: i64, title: string) -> Result[void, TodoError] $ Db + Session:
    owner := $.use(Session).user_id()
    $.use(Db).rename!(owner, id, title)

pub fn on_rename!(id: i64, title: string) -> void $ Dom + Server:
    match rename_todo!(id, title):    # Result[void, TodoError | CrossError]
        .Ok(()) => refresh()
        .Err(error) => show_error("$error")
```

**Behavior.** The server build compiles the body and an endpoint; the
browser gets a stub, as Next.js compiles an action reference. Arguments
are untrusted, which matches Next.js's advice to re-check every action
([Next.js data security](https://nextjs.org/docs/app/guides/data-security)).

**Breaks.** `Session` is a per-request capability: its provider comes
from the request's cookie, and nothing binds a provider per crossing
(P15). The bound form is a server-made closure; Next.js encrypts its
captures with a per-build key. hd's version is the record's example 7
`Action` handle, which a remote result cannot carry (case 15, P8). Next.js
reports "Failed to find Server Action" when page and server come from
different builds
([Next.js](https://nextjs.org/docs/messages/failed-to-find-server-action)),
the skew of P1.

**V2.**

```text
@error
pub enum TodoError:
    @error("no todo $id")
    Missing(id: i64)
    @error("lost contact: $cause")
    Cross(@from cause: CrossError)

pub trait Todos < Remote:
    fn rename_todo!(&mut self, id: i64, title: string) -> Result[void, TodoError]

struct PgTodos:
    pool: Pool

impl Todos for PgTodos:
    fn rename_todo!(&mut self, id: i64, title: string) -> Result[void, TodoError]:
        owner := $.use(Session).user_id()    # error: missing-requirement; an impl method adds no row key
        self.pool.rename!(owner, id, title)

pub fn on_rename!(id: i64, title: string) -> void $ Dom + Todos:
    let mut todos = $.use(Todos)
    match todos.rename_todo!(id, title):
        .Ok(()) => refresh()
        .Err(error) => show_error("$error")
```

An impl method's row must equal the trait's: a probe of an impl method
that adds `$ Billing` reports `trait-method-signature: ... differs from
the trait's declaration: the requirement row`. So a V2 endpoint cannot
reach server capabilities through its row. They must be fields of the
impl, set before `serve!`, or keys in the shared trait that every caller
would then need (P16). C3 resolves a body's `$.use` at the destination,
but an endpoint's row is the trait's, so for a `Remote` method C3 has
nothing to resolve. A2 and A3 hold for `rename_todo`. **Verdict: V1
friction; V2 friction.**

### 3. Typed Service (tRPC, gRPC)

```ts
export const appRouter = router({
  userById: publicProcedure.input(z.number()).query(({ input }) => db.user.find(input)),
});
const user = await trpc.userById.query(1);
```

```proto
service Users { rpc GetUser(GetUserRequest) returns (User); }
```

```text
@error
pub enum UserError:
    @error("no user $id")
    Missing(id: i64)
    @error("a name may not be empty")
    EmptyName

@remote(Api)    # hypothetical syntax
pub fn user_by_id!(id: i64) -> Result[User, UserError] $ Db:
    $.use(Db).find_user!(id)

@remote(Api)    # hypothetical syntax
pub fn rename_user!(id: i64, name: string) -> Result[void, UserError] $ Db:
    if name.len() == 0:
        return .Err(UserError.EmptyName)
    $.use(Db).rename!(id, name)

pub fn profile!(id: i64) -> Result[User, UserError | CrossError] $ Api:
    user_by_id!(id)
```

**Behavior.** A tRPC router maps to a module of `@remote(Api)` functions.
The decoder checks shape, as tRPC's zod parser does; a refinement such as
"non-empty" stays in the body. Client types come from the same program,
as tRPC's inferred client types do.

**Breaks.** gRPC exists for clients on other releases and languages;
protobuf's field numbers keep old messages readable
([protobuf, updating a message](https://protobuf.dev/programming-guides/proto3/#updating)).
Section 1 puts that out of scope, and no rule says what an old client sees
(P1). gRPC sends the caller's deadline to the server, which cancels its
handler; neither variant says whether a crossing carries one (P10).

**V2.**

```text
pub trait Api < Remote:
    fn user_by_id!(&mut self, id: i64) -> Result[User, UserError]
    fn rename_user!(&mut self, id: i64, name: string) -> Result[void, UserError]
```

A `Remote` trait reads like a proto `service` or a tRPC router type: one
declaration lists every endpoint. `UserError` then needs a `Cross` case.
**Verdict: V1 friction; V2 friction.**

### 4. Parallel Count And Sort (std::thread::scope, rayon)

```rust
let total = AtomicUsize::new(0);
std::thread::scope(|s| {
    s.spawn(|| total.fetch_add(count(&lines[..500]), Relaxed));
    s.spawn(|| total.fetch_add(count(&lines[500..]), Relaxed));
});
let (l, r) = v.split_at_mut(mid);
rayon::join(|| l.sort(), || r.sort());
```

```text
use std.collections.SpanMut

pub fn total_words(lines: &List[string]) -> usize:
    let total = 0
    scope(fn(s: &mut Scope) -> void:
        s.spawn(fn() -> void: total += count_words(lines, 0, 500))
        s.spawn(fn() -> void: total += count_words(lines, 500, 1000))    # accepted: two threads write one cell
    )
    total

pub fn sort_halves(xs: &mut List[i64]) -> void:
    let (left, right) = xs.span_mut().split_at_mut(500)    # no such method: SpanMut has slice_mut(self)
    scope(fn(s: &mut Scope) -> void:
        s.spawn(fn() -> void: sort_span(left))
        sort_span(right)
    )
```

**Behavior.** Rust rejects a plain `usize` here: two closures cannot both
borrow `total` mutably, so the original uses an atomic. In hd a captured
variable is a shared cell
([`fn.capture.cell`](../../spec/lang/07-functions.md#r-fn.capture.cell)),
and the spawn bound is `FnMut`; C2's `Fn` bound covers separate memory
only. Section 6's table lets a scoped thread take a box (✓) and copy a
plain value. If the capture is the shared cell, the two writes race; if
it is a copy, both writes are lost and `total` stays 0. Either answer is
wrong, and nothing rejects the code.

**Breaks.** The record lists the shared cell under the missing `Sync`
(question 12), but as a lend gap. The cell is not a lend, so the lend rule
does not stop it, and the table's box cell for scoped threads is ✓ (P2). The sort needs two disjoint `SpanMut` halves;
`slice_mut` consumes its span, so std has no split (P19).

**V2.** The same code. **Verdict: V1 breaks; V2 breaks.**

### 5. Heavy Work Off The UI Thread (Kotlin, Swift)

```kotlin
fun onScan(photo: Photo) = viewModelScope.launch {
    spinner.value = true
    val faces = withContext(Dispatchers.Default) { detectFaces(photo) }
    boxes.value = faces; spinner.value = false
}
```

```swift
@MainActor func scan(_ photo: Photo) async {
    spinner = true
    let faces = await Task.detached { detectFaces(photo) }.value
    boxes = faces; spinner = false
}
```

```text
pub fn on_scan_tapped!(photo: Photo) -> Result[void, CrossError] $ Ui + Background:
    $.use(Ui).show_spinner()
    faces := $.use(Background).run!(fn() -> List[Rect]: detect_faces(photo))?
    $.use(Ui).hide_spinner()
    $.use(Ui).draw_boxes(faces)
    .Ok(())
```

**Behavior.** The handler suspends on `run!`, the UI thread's loop keeps
drawing, and the handler resumes there with the faces, as in Kotlin and
Swift. A closure with `$ Ui` in its row is a `type-mismatch`, earlier
than Android's `CalledFromWrongThreadException`.

**Differences.** Cancelling the handler (the screen closes) cannot stop
`detect_faces`, which has no suspension point; Kotlin's CPU loop also runs
on unless it checks `isActive`. Section 6's Running It says a background
crash is a `CrossError`. But a panic poisons its program instance
([`module.profile.host-result.status`](../../spec/lang/10-modules.md#r-module.profile.host-result.status)),
and the background thread shares the UI's instance, so the UI cannot go
on as if one call failed (P12). An uncaught Kotlin or Swift crash ends the
app.

**V2.** Section 6 writes this code, and C2 checks it. **Verdict: V1 works; V2 works.**

### 6. Delayed Welcome Mail (Sidekiq, Temporal)

```ruby
class WelcomeJob
  include Sidekiq::Job
  sidekiq_options retry: 5
  def perform(user_id) = UserMailer.welcome(User.find(user_id)).deliver_now
end
WelcomeJob.perform_in(10.minutes, user.id)
```

```go
func SignUp(ctx workflow.Context, u User) error {
    workflow.Sleep(ctx, 10*time.Minute)
    v := workflow.GetVersion(ctx, "welcome-v2", workflow.DefaultVersion, 1)
    return workflow.ExecuteActivity(ctx, SendWelcome, u.Email, v).Get(ctx, nil)
}
```

```text
fn send_welcome!(email: string, name: string) -> Result[void, MailError] $ Mailer:
    $.use(Mailer).send!(email, "Welcome, $name")

pub fn sign_up!(user: User, photo: Image) -> Result[void, SignUpError] $ Db + Jobs + Gpu[Device = Gpu0]:
    save_user!(user)?
    jobs := $.use(Jobs)
    jobs.after!(minutes(10), fn!() -> Result[void, MailError] $ Mailer: send_welcome!(user.email, user.name))?
    tex := $.use(Gpu[Device = Gpu0]).upload!(photo)?
    jobs.after!(days(30), fn!() -> Result[void, GpuError] $ Gpu[Device = Gpu0]: refresh!(tex))?    # accepted by L4: a handle
    .Ok(())
```

**Behavior.** The mail job copies two strings, as Sidekiq's advice to pass
simple arguments asks
([Sidekiq best practices](https://github.com/sidekiq/sidekiq/wiki/Best-Practices)).
A `&mut` capture is rejected, as the record shows.

**Breaks.** A job set for 30 days runs on whatever build is live then.
Section 1 asks the old build to drain its queue, which means keeping it
running for a month. Temporal ships `GetVersion` because this is routine
([Temporal versioning](https://docs.temporal.io/develop/go/versioning)).
B5 makes the new build refuse it, and Running It leaves the job to "its
retry policy", so a month-old job retries against a build that always
refuses it, or is dropped (P1). L4 and the table (Job: a handle ✓) admit
the texture handle, but its device may reset before day 30, and L3 keeps
its data alive meanwhile (P14). The job's `Err` has no caller to see it,
and no std item names the retry or dead-letter policy (P14).

**V2.** The same code. **Verdict: V1 breaks; V2 breaks.**

### 7. Image Kernels (wgpu, CUDA)

```cuda
__global__ void blur(uchar4* px, int w, int h) {
    int x = blockIdx.x * blockDim.x + threadIdx.x;  /* one invocation per pixel */
}
cudaMemcpy(d_px, px, n, cudaMemcpyHostToDevice);
blur<<<dim3(w / 16, h / 16), dim3(16, 16)>>>(d_px, w, h);
```

```text
struct Gpu0: pass

fn blur(tex: Texture[Gpu0]) -> void:    # device code: the subset is unspecified
    pass

fn blur_pixel(tex: Texture[Gpu0], x: u32, y: u32) -> void:    # what CUDA and WGSL write
    pass

pub fn retouch!(photo: Image) -> Result[Image, CrossError] $ Gpu[Device = Gpu0]:
    gpu := $.use(Gpu[Device = Gpu0])
    tex := gpu.upload!(photo)?
    gpu.run!(blur, tex)?
    gpu.dispatch!(blur_pixel, tex, (photo.width / 16, photo.height / 16))?    # no such form in the record
    gpu.read_back!(tex)
```

**Behavior.** One device works: the upload, the resident texture, the
read back, and the type error for a `Texture[Gpu0]` on `Gpu1`. A kernel
that calls `println` fails the empty row.

**Breaks.** Real kernels run once per invocation with an index and a
launch grid; the record's kernel takes the whole texture (P17). A
machine's GPU count is known at run time (`cudaGetDeviceCount`), but each
device needs its own type (P3). No backend emits device code; hd emits
only Wasm (P12).

**V2.** Section 6 makes a device another build: kernels are the methods
of a trait that extends `Kernel` (A1), and C1 checks them by A2.

```text
pub trait Filters < Kernel:
    fn blur!(&mut self, tex: &mut Texture[Gpu0]) -> Result[void, GpuError]    # rejected by A2: a reference to a handle
    fn tone!(&mut self, tex: Texture[Gpu0]) -> Result[void, GpuError]    # rejected by A2: a handle is not boundary-safe
```

Running It launches "the kernel by number with its handles", but A2 lets
a kernel method take only boundary-safe values, and a texture is a handle.
So no kernel can name the data it works on (P8). **Verdict: V1 friction;
V2 breaks.**

### 8. Data-Local Count (Ray, Spark, Dask)

```python
@ray.remote
def count_region(part, region): return sum(1 for s in part if s.region == region)
parts = [ray.put(load(i)) for i in range(64)]                 # ObjectRefs on many nodes
total = sum(ray.get([count_region.remote(p, "EU") for p in parts]))

seen = 0
rdd.foreach(lambda s: seen += 1)   # Spark: each executor bumps its own copy; seen stays 0
```

```text
struct Node: pass

fn count_region(part: Partition[Node], region: string) -> usize:
    part.count(fn(s: Sale) -> bool: s.region == region)

pub fn eu_sales!(parts: List[Partition[Node]]) -> Result[usize, CrossError] $ Cluster:
    let total = 0
    for part in parts:
        total += at(part).call!(fn() -> usize: count_region(part, "EU"))?    # every part has one type, so one home
    let seen = 0
    $.use(Cluster).call!(fn() -> void: seen += 1)?    # error: not-writable
    .Ok(total)
```

**Behavior.** The `seen` line is the bug Spark's guide warns about: the
closure updates a copy on each executor
([Spark, understanding closures](https://spark.apache.org/docs/latest/rdd-programming-guide.html#understanding-closures-)).
Question 10's `Fn` bound turns it into a compile error.

**Breaks.** The 64 partitions live on nodes the scheduler picks at run
time. A `List[Partition[Node]]` gives every element one home type, so
`at(part)` targets one node for all of them (P3). Ray's locality is also
preferred, not required: a task can pull an object to another node
([Ray scheduling](https://docs.ray.io/en/latest/ray-core/scheduling/index.html)).
A Spark driver sends arbitrary closures to trusted executors. Whether a
remote domain accepts any code identity is a trust question that no domain
property states (P8).

**V2.** The same code. **Verdict: V1 breaks; V2 breaks.**

### 9. Long-Lived Account Process (Erlang, Akka)

```erlang
loop(Balance) ->
    receive
        {deposit, N} -> loop(Balance + N);
        {balance, From} -> From ! {balance, Balance}, loop(Balance)
    end.
Pid = spawn(fun() -> loop(0) end), monitor(process, Pid), Pid ! {deposit, 10}.
```

```text
enum AccountMsg:
    Deposit(cents: i64)
    Balance(reply: Reply[i64])    # a reply handle: a sketch

fn account!(inbox: Inbox[AccountMsg]) -> void:
    let balance: i64 = 0
    while true:
        match inbox.next!():
            .Deposit(cents) => balance += cents
            .Balance(reply) => reply.send(balance)

pub fn open_account!() -> Result[void, CrossError] $ Processes:
    procs := $.use(Processes)
    pid := procs.spawn!(account)?    # its home is picked at run time
    pid.send!(AccountMsg.Deposit(1000))?    # one-way: there is no result to wait for
    .Ok(())
```

**Behavior.** The process's state is a local in its loop, so question 17
holds. Messages are copies, as Erlang's are.

**Breaks.** Each `spawn` makes a new domain instance at run time, which
question 4 leaves unplanned (P3). A crossing is call and return, while
`!` is one-way; `send!` above has no place in the model (P9). A monitor's
`DOWN` message reports a domain's death outside any crossing, but the
model reports loss only as one crossing's `Err`
([Erlang monitors](https://www.erlang.org/doc/apps/erts/erlang.html#monitor/2)).

**V2.** The same code. **Verdict: V1 breaks; V2 breaks.**

### 10. A Test Swaps The Server (MSW, Swift Local Actor System)

```swift
let system = LocalTestingDistributedActorSystem()
let account = Account(actorSystem: system)      // same API, no network
try await account.deposit(10)
```

```text
tests:
    struct FakeDb:
        users: List[User]

    impl Db for FakeDb:
        fn find_user!(&mut self, id: i64) -> Result[User, DbError]:
            match self.users.get(0):
                .Some(user) => .Ok(user)
                .None => .Err(DbError.Missing(id))

    it("opens a user through a local server"):
        db := box(FakeDb { users: [User { id: 1, name: "Ada" }] })
        $.with(Db=db):
            $.with(Server=box(LocalServer {})):    # LocalServer: a sketch
                match load_user!(1):
                    .Ok(user) => assert_equal(user.name, "Ada")
                    .Err(_) => panic("expected Ada")
```

**Behavior.** Section 6 says a test swaps `Server` with `$.with`. Its
sketch writes `$.with(Server=LocalServer {})`, but a provider is a box
today, so that line is a `type-mismatch` (P19).

**Open.** Three things are unstated (P13). Which build compiles a test
that holds both the stub and the body (X3)? Section 5 says the body sees
no source provider, so does `load_user`'s body see the test's `Db`? Does
`LocalServer` encode and decode, so a test catches a value that is not
boundary-safe? A canned-answer fake, as MSW gives, needs typed methods on
`Server`, which the record does not define.

**V2.**

```text
tests:
    struct FakeUsers: pass

    impl Users for FakeUsers:
        fn load_user!(&mut self, id: i64) -> Result[User, UserError]:
            .Ok(User { id: id, name: "Ada" })

    it("shows the user"):
        $.with(Users=box(FakeUsers {})):
            on_open!(7)
```

A fake is an ordinary implementation of the trait, and it answers per
method, as MSW answers per route. The real `PgUsers` can also be
installed directly, as A4 says. Section 6's own test writes
`$.with(Users=FakeUsers {}, Dom=FakeDom {})`, which needs `box(...)` too
(P19). **Verdict: V1 friction; V2 works.**

### 11. Error Propagation (tonic, thiserror)

```rust
match client.place_order(req).await {
    Ok(r) => show(r.into_inner()),
    Err(s) if s.code() == Code::DeadlineExceeded => check_status(id).await,  // may have committed
    Err(s) => show_error(s.message()),
}
```

```text
@error
pub enum CheckoutError:
    @error("order failed")
    Order(@from error: OrderError)
    @error("connection failed")
    Cross(@from error: CrossError)

pub fn checkout!(cart: Cart) -> Result[Receipt, CheckoutError] $ Server:
    receipt := place_order!(cart)?    # error: invalid-result-propagation
    .Ok(receipt)

pub fn checkout_mapped!(cart: Cart) -> Result[Receipt, CheckoutError] $ Server:
    match place_order!(cart):
        .Ok(receipt) => .Ok(receipt)
        .Err(._0(order)) => .Err(CheckoutError.Order(order))
        .Err(._1(cross)) => .Err(CheckoutError.Cross(cross))
```

**Probe.** With `hd check`, a `?` on `Result[i64, OrderError | CrossError]`
in a function returning `CheckoutError` reports `invalid-result-propagation:
CheckoutError does not implement From[OrderError | CrossError]`, although
both members have an `@from` variant. `"$error"` on the union checks, so
`Display` reaches through it. Writing the union inline inside
`Result[...]` reports `generic-kind-mismatch`, and an alias works. The
spec allows the inline form
([`types.union.application-precedence`](../../spec/lang/04-type-system.md#r-types.union.application-precedence)),
so that is compiler lag (P19).

**Behavior.** Under V1, every caller that wants `?` writes the four-line
match, once per remote function (P11). A remote body's error must be
boundary-safe, so an `@source` of the erased `Error` becomes an
`ErrorReport` first
([`module.boundary.erased-error`](../../spec/lang/10-modules.md#r-module.boundary.erased-error)).
The record names only `Unknown`; a remote panic, a decode failure and a
refused call have no stated case (P10).

**V2.**

```text
@error
pub enum CheckoutError:
    @error("user lookup failed")
    User(@from error: UserError)

pub fn checkout!(id: i64) -> Result[User, CheckoutError] $ Users:
    let mut users = $.use(Users)
    user := users.load_user!(id)?    # valid: one @from conversion
    .Ok(user)

fn describe(error: UserError) -> string:
    match error:
        .Missing(id) => "missing $id"
        .Cross(_) => "cannot happen on the server"    # the server and every fake still cover it
```

`?` works because the error is declared (A3). The cost is a `Cross` case
that the server's impl and every fake must carry but never produce (P11).
Section 6 writes `Cross(cause: CrossError)` without `@from`, so B3's stub
has no stated conversion to build it (P18). Section 4 still says a crossing
widens to `Result[T, E | CrossError]` and matches `.Err(.Unknown)` on
`orders.place_order!`, which A3 rules out (P19). **Verdict: V1 friction;
V2 friction.**

### 12. Two Crossings In all! (Promise.all, errgroup)

```ts
const [user, orders] = await Promise.all([trpc.user.query(id), trpc.orders.query(id)]);
```

```text
use std.task.all

pub fn on_open!(id: i64) -> void $ Dom + Server:
    let (user, orders) = all!(load_user(id), load_orders(id))
    match (user, orders):
        (.Ok(u), .Ok(os)) => show_profile(u, os)
        _ => show_error("could not load the profile")
```

**Behavior.** Each stub is a cold suspension, so `all!` runs both
crossings at once and returns two `Result`s. A failed crossing is a value,
so `all!` does not cancel its sibling; Go's errgroup cancels on the first
error, `Promise.all` does not. tRPC's batch link would send both in one
request; the record leaves batching to the transport.

**V2.** `all!(users.load_user(id), users.load_orders(id))` on one
provider. A probe with two `&mut self` bang methods on one provider inside
`all!` checks and runs. **Verdict: V1 works; V2 works.**

### 13. Cancel Mid-Crossing (Go context, gRPC deadline)

```go
ctx, cancel := context.WithTimeout(ctx, 2*time.Second)
defer cancel()
r, err := client.PlaceOrder(ctx, req)   // the server's ctx is cancelled too
```

```text
use std.task.{TimedOut, apply, timeout}
use std.time.{Clock, s}

@error
pub enum BuyError:
    @error("order failed")
    Order(@from error: OrderError)
    @error("connection failed")
    Cross(@from error: CrossError)
    @error("timed out")
    Late(@from error: TimedOut)

pub fn buy!(cart: Cart) -> Result[Receipt, BuyError] $ Server + Clock:
    apply!(timeout(s(2))):
        let result: Result[Receipt, BuyError] = match place_order!(cart):
            .Ok(receipt) => .Ok(receipt)
            .Err(._0(order)) => .Err(BuyError.Order(order))
            .Err(._1(cross)) => .Err(BuyError.Cross(cross))
        result

pub fn on_buy!(cart: Cart) -> void $ Dom + Server + Clock:
    match buy!(cart):
        .Ok(receipt) => show(receipt)
        .Err(.Cross(.Unknown)) => check_order_status!(cart.id)
        .Err(.Late(_)) => show_error("try again")    # also unknown: the card may be charged
        .Err(error) => show_error("$error")
```

**Behavior.** The timeout cancels the stub, which aborts the host
operation
([`req.cancel.external-abort`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.external-abort)).
The caller sees `TimedOut`, not `.Unknown`, though the server may have
charged the card. The obvious `.Unknown` arm misses it, and nothing warns
(P10).

**Now stated.** Section 6's Running It answers two points. Cancelling a
service call sends a cancel that the server's call sees at its next
suspension, as gRPC cancels the handler's context. Cancelling a worker
crossing posts an abort.

**Open.** The deadline itself does not travel, so the server cannot stop
early on its own. A value transferred into a cancelled crossing is lost
for good; L2 allows that, but Comlink users would not expect it.

**V2.** The same, with `.Order(.Cross(.Unknown))` for the transport case;
the timeout is still a separate `Late`. **Verdict: V1 friction; V2
friction.**

### 14. A Remote Call Inside A Remote Call

**V1, same service.** `user_title!` is shared code with no mark, so it
lands in both builds. `dashboard!` runs on the server and calls
`load_user!` directly.

```text
@remote(Server)    # hypothetical syntax
pub fn load_user!(id: i64) -> Result[User, DbError] $ Db:
    $.use(Db).find_user!(id)

pub fn user_title!(id: i64) -> Result[string, DbError | CrossError] $ Server:
    match load_user!(id):
        .Ok(user) => .Ok(greeting(user))
        .Err(error) => .Err(error)

@remote(Server)    # hypothetical syntax
pub fn dashboard!(id: i64) -> Result[string, DbError] $ Db:
    match load_user!(id):    # a normal call in the server build; does the row need $ Server?
        .Ok(user) => .Ok(greeting(user))
        .Err(error) => .Err(error)
```

Section 6 compiles a call by the build: a normal call in D's build, a stub
elsewhere. The checker types `load_user!(id)` once, though. If
`dashboard!` needs no `$ Server`, its call returns `Result[User,
DbError]`, while `user_title!`'s returns `DbError | CrossError`. Then a
shared function's type would depend on the build it lands in (P6).

**V1, two services.** The browser calls `Server`, which calls `Billing`.

```text
@remote(Billing)    # hypothetical syntax
pub fn charge!(card: CardToken, cents: i64) -> Result[ChargeId, PayError] $ Payments:
    $.use(Payments).charge!(card, cents)

@remote(Server)    # hypothetical syntax
pub fn place_order!(cart: Cart) -> Result[Receipt, PayError | CrossError] $ Db + Billing:
    charge := charge!(cart.card, cart.total_cents)?
    order := $.use(Db).insert_order!(cart)?
    .Ok(Receipt { order, charge })

pub fn on_buy!(cart: Cart) -> void $ Dom + Server:
    match place_order!(cart):    # Result[Receipt, (PayError | CrossError) | CrossError]
        .Ok(receipt) => show(receipt)
        .Err(._0(._1(.Unknown))) => show_error("the bank did not answer")
        .Err(._1(.Unknown)) => check_order_status!(cart.id)
        .Err(error) => show_error("$error")
```

Positional unions do not flatten
([`types.union.substitution`](../../spec/lang/04-type-system.md#r-types.union.substitution)),
so the two unknowns stay apart, but the match grows one level per hop.
`insert_order!`'s `DbError` must also join the union, or its `?` fails.
Under X1 the server target binds a `Billing` stub; the record does not say
a target's set includes other targets' keys (P6).

**V2.**

```text
pub trait Billing < Remote:
    fn charge!(&mut self, card: CardToken, cents: i64) -> Result[ChargeId, PayError]

pub trait Orders < Remote:
    fn place_order!(&mut self, cart: Cart) -> Result[Receipt, OrderError]

struct Shop:
    billing: *mut dyn Billing    # the stub is stored: the impl method cannot list $ Billing
    db: *mut dyn Db

impl Orders for Shop:
    fn place_order!(&mut self, cart: Cart) -> Result[Receipt, OrderError]:
        charge := self.billing.charge!(cart.card, cart.total_cents)?    # PayError into OrderError by @from
        order := self.db.insert_order!(cart)?
        .Ok(Receipt { order, charge })
```

A bare call never crosses, so shared code has one type in every build, and
P6 does not arise. The two unknowns become named cases,
`OrderError.Cross` and `OrderError.Pay(PayError.Cross)`. The price is that
server code holds its providers in fields, not rows, because an impl
method's row must match the trait's (P16). **Verdict: V1 friction; V2
friction.**

### 15. A Remote Function With A Closure Or A Handle

```python
ref = ray.put(big_table)               # a handle to an object in the store
result = ray.get(lookup.remote(ref, key))
```

```text
@remote(Server)    # hypothetical syntax
pub fn filter_sales!(keep: fn(Sale) -> bool) -> Result[List[Sale], ApiError] $ Sales:    # error: a closure is not boundary-safe
    rows := $.use(Sales).all!()?
    .Ok([for s in rows if keep(s) => s])

@remote(Server)    # hypothetical syntax
pub fn rename_action!(id: i64) -> Result[Action[Server, string, void], TodoError] $ Db:    # error: a handle is not boundary-safe
    .Ok(action(fn!(title: string) -> Result[void, TodoError] $ Db: rename_todo!(id, title)))
```

**Behavior.** Rejecting the closure is right for a browser caller: its
code identity is client data, so it could name any function in the
program. That is remote code execution, and section 6's signature rule
stops it.

**Breaks.** The mode table says a remote service takes handles, and the
record's example 7 needs a server-made `Action` the browser holds. Section
6 says parameters and results are plain data. The two rules disagree, and
neither covers a sealed handle, one the client may hold but not forge
(P8).

**V2.** V2 calls an `Action` as `rename.call!(text)`, but A2 still bars a
`Remote` method from returning one, and bars closures by name. Section 6's
table marks a handle to a service "its home only", as if a handle could
reach its home service, while A2 bars every handle. The same gap.
**Verdict: V1 breaks; V2 breaks.**

### 16. A Generic Remote Function (Swift Generic distributed func)

```swift
distributed func setting<T: Codable & Sendable>(_ key: String) async throws -> T
// the call records the generic substitution; the receiver resolves T at run time
```

```text
@remote(Server)    # hypothetical syntax
pub fn load_setting![T < Deserialize & Serialize](key: string) -> Result[T, SettingError] $ Settings:
    $.use(Settings).read!(key)?.decode::[T]()

pub fn theme!() -> Result[Theme, SettingError | CrossError] $ Server:
    load_setting!::[Theme]("theme")
```

**Behavior.** Swift sends the substitution and resolves `T` from run-time
metadata (SE-0344). hd monomorphizes, so the server build needs one
endpoint per `T` that any other build uses. One program makes that set
knowable, but section 6's walk from each target's own entry does not find
it (P12). The signature also needs a bound meaning "boundary-safe".
Structural values are boundary-safe without a derive, so no trait names
that set.

**V2.**

```text
pub trait Settings < Remote:
    fn load_setting![T < Deserialize & Serialize](&mut self, key: string) -> Result[T, SettingError]    # error under V2: a Remote method is not generic
    fn load_theme!(&mut self) -> Result[Theme, SettingError]    # the workaround: one method per type
```

A2 forbids it, so the shape cannot be written; one method per type is the
workaround, as gRPC has no generic RPCs either. **Verdict: V1 friction;
V2 breaks.**

### 17. Streaming Results (gRPC Server Streaming, tRPC Subscriptions)

```proto
rpc ListSales(Filter) returns (stream Sale);
```

```ts
for await (const token of streamText({ model, prompt }).textStream) out.append(token);
```

```text
@remote(Server)    # hypothetical syntax
pub fn list_sales!(f: Filter) -> Result[List[Sale], ApiError] $ Sales:    # all 200 rows arrive at the end
    $.use(Sales).query!(f)

@remote(Server)    # hypothetical syntax
pub fn stream_sales!(f: Filter) -> Result[Stream[Sale], ApiError] $ Sales:    # error: a stream is not boundary-safe
    $.use(Sales).cursor!(f)

pub fn show_sales!(f: Filter) -> void $ Dom + Server:
    match stream_sales!(f):
        .Ok(rows) =>
            for sale in rows:
                add_row(sale)    # what does contact lost after 120 rows give?
        .Err(error) => show_error("$error")
```

**Breaks.** A crossing returns one result. gRPC streams, tRPC
subscriptions, LLM token streams, Comlink progress callbacks and Kotlin
`Flow` all return many. A stream would be a handle that lives on the
server; the record has no form for it, no per-item outcome, and no
backpressure or early stop (P9).

**V2.** The owner lists streaming as not covered. **Verdict: V1 breaks;
V2 breaks.**

### 18. A Background Result Updates UI State (StateFlow, @Published)

```kotlin
withContext(Dispatchers.Default) {
    for ((i, r) in regions.withIndex()) {
        scan(r)
        withContext(Dispatchers.Main) { progress.value = i }   // hop back to the UI thread
    }
}
```

```text
struct ScanState:
    progress: f64
    faces: List[Rect]

pub fn on_scan_tapped!(photo: Photo, vm: *mut ScanState) -> Result[void, CrossError] $ Ui + Background:
    background := $.use(Background)
    faces := background.run!(fn() -> List[Rect]:
        vm.progress = 0.5    # error by section 6's table: a box does not cross to a background thread until Sync
        detect_faces(photo)
    )?
    vm.faces = faces    # back on the UI thread
    $.use(Ui).redraw()
    .Ok(())
```

**Behavior.** The final write runs on the UI thread, as the case asks.
The write inside the closure is the problem. Section 6's table refuses a
box on a background thread until `Sync`, as Rust refuses a non-`Send`
`Rc` and Swift 6 a non-`Sendable` class. Section 5 still says that in
shared memory "any copyable value copies", and a `*mut ScanState` is
copyable; the two sections disagree (P19).

**Friction.** The result path works by returning the value. The honest
form of progress, a hop back to the UI thread, has no domain value for
"the UI thread" inside background code (P9).

**V2.** The same code. **Verdict: V1 friction; V2 friction.**

## Comparison With Other Languages

| Concern | hd V1 | hd V2 | Swift | Rust | Kotlin | Next.js, tRPC, gRPC | Ray, Spark, Erlang |
| --- | --- | --- | --- | --- | --- | --- | --- |
| What may cross to a thread | copyable values; lends in a scope | same | `Sendable` values | `Send` types | anything | n/a | n/a |
| Global mutable state | `let mut` banned (q17) | same | global `var` not concurrency-safe | `static mut` is `unsafe` | allowed | per instance | per process |
| Remote call mark | `@remote(D)` and `$ D` | a `Remote` trait, a provider row | `distributed func`, `try await` | n/a | n/a | `"use server"`; `trpc.x.query`; a stub object | `@ray.remote` and `f.remote()`; `!` |
| Where the service is declared | each function | one trait | one actor type | n/a | n/a | tRPC router, proto `service` | a remote function or actor |
| Failure in the type | widened, `E \| CrossError` | declared in `E` | `throws`, any error | n/a | n/a | thrown or a status | a raised error |
| Test double | a local server provider | a trait impl | a local actor system | n/a | n/a | MSW, an in-process server | a local cluster |
| Other builds | out of scope | out of scope | the actor system's concern | n/a | n/a | skew protection; field numbers | Temporal `GetVersion` |
| Domain instances | static types (q4) | same | dynamic actor refs | n/a | dynamic dispatchers | dynamic connections | dynamic refs and pids |

Swift is the closest prior art for both variants. Its distributed actor is
V2's shape: one type declares the endpoints, and a provider-like actor
system chooses local or remote
([SE-0336](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0336-distributed-actor-isolation.md)).
Swift also checks the thread boundary with one property, `Sendable`, over
values, closures and globals alike
([SE-0302](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0302-concurrent-value-and-concurrent-closures.md),
[SE-0412](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0412-strict-concurrency-for-global-variables.md)).
hd splits that job across three rules (q10, q12, q17), and boxes fall
between them (P2, P4).

## Problems, Ranked

Rank weighs how many cases hit a problem and how silent the failure is.

| Rank | ID | Problem | Severity | Variants | Cases |
| ---: | --- | --- | --- | --- | --- |
| 1 | P1 | Two builds talk in every deploy; B5 refuses them, so open pages fail and pending jobs are lost | high | both | 1, 2, 3, 6 |
| 2 | P3 | Domain instances must be static types | high | both | 1, 7, 8, 9 |
| 3 | P2 | Scoped threads still take boxes and captured cells | high, silent | both | 4; q16 |
| 4 | P4 | An immutable top-level binding to a box escapes question 17 | high, silent | both | 1, 18; probe |
| 5 | P8 | A2 bars handles, which kernels, actions and streams need; trust is no domain property | medium | both; worse in V2 | 2, 7, 8, 15, 17 |
| 6 | P5 | Two shapes for one crossing concept | medium | V1 | 2, 3, 10 to 17 against 1, 4 to 9, 18 |
| 7 | P6 | An unmarked caller has no domain, so a call's type depends on the build | medium | V1 | 10, 14, 16 |
| 8 | P7 | A remote call looks local at its call site | medium | both | 2, 3, 10 to 17 |
| 9 | P9 | A crossing returns exactly one value | medium | both | 1, 9, 17, 18 |
| 10 | P10 | Unknown has several spellings, and `CrossError`'s other cases are unstated | medium, silent | both | 11, 13, 14 |
| 11 | P11 | Crossing failure in result types: `?` fails (V1), a dead case (V2) | medium | both | 11, 13, 14 |
| 12 | P16 | A V2 endpoint cannot use rows: an impl method's row must match the trait's | medium | V2 | 2, 14 |
| 13 | P12 | No compilation or runtime strategy for threads, kernels, or cross-build ids | medium | both | 4, 5, 7, 16, 18 |
| 14 | P13 | A local test server's semantics are unstated | low | V1 | 10 |
| 15 | P14 | Later jobs: handles across restarts, errors with no caller | low | both | 6 |
| 16 | P15 | Per-request capabilities on a server | low | both | 2, 3 |
| 17 | P17 | GPU kernels have no launch geometry or device subset | low | both | 7 |
| 18 | P18 | V2's open details: `serve!` with two `Remote` traits, the stub's `CrossError` conversion | low | V2 | 2, 11 |
| 19 | P19 | Editorial, internal contradictions and compiler lag | low | both | 4, 10, 11, 18 |

### P1. Two Builds Talk In Every Deploy

**Effect.** Section 1 puts different builds out of scope, but ordinary
deploys create them. A page open across a deploy calls the new server
(case 2) and may load a new worker script (case 1). A gRPC client on an
old release calls a new service (case 3). A job set for 30 days runs on a
later build (case 6). B5 now stamps a build ID on both sides and refuses
a peer from another build with a `CrossError`, so the failure is loud,
not a wrong decode. It is also routine: every deploy fails every open
page's next call, and every job queued before it. B5 names no
`CrossError` case for the refusal, so a page cannot tell "reload" from
"retry". Next.js fails the same way ("Failed to find Server Action"), and
Vercel's skew protection exists to route old clients to the old
deployment ([Vercel](https://vercel.com/docs/skew-protection)).

**Candidates.**

1. Should the refusal be its own `CrossError` case, so a page reloads and
   a job queue knows to re-queue for the old build or drop?
   ```text
   match users.load_user!(id):
       .Ok(user) => show(user.name)
       .Err(UserError.Cross(.OtherBuild)) => reload_page()    # OtherBuild: a case CrossError does not have yet
       .Err(error) => show_error("$error")
   ```
2. Should a crossing's identity hash the method's signature and boundary
   types, so an endpoint whose shape did not change keeps working across
   builds? Content hashes are deferred today
   ([`fn.capture.serializable-deferred`](../../spec/lang/07-functions.md#r-fn.capture.serializable-deferred)).
3. Should the later domain take only named entries with plain data, so a
   job survives any deploy that keeps the entry?

### P2. Scoped Threads Still Take Boxes And Captured Cells

**Effect.** Section 6's table now refuses a box, a trait value and a
provider value on a background thread until `Sync`, which closes case 18.
It still marks them ✓ for a scoped thread, and it does not mention a
captured variable, which is a cell. So case 4's two scoped threads add to
one counter: the writes race, or each lands in a copy and is lost. Either
way the answer is wrong and nothing rejects the code. C2's `Fn` bound
covers separate memory only. Question 12 lists the missing `Sync` for
lends only, and section 5 still says any copyable value copies in shared
memory (P19).

**Candidates.**

1. Until `Send` and `Sync` are designed, should a scoped thread take boxes
   and cells only as lends that the exclusivity rules check, so two spawns
   writing one variable are `conflicting-access`?
   ```text
   s.spawn(fn() -> void: total += n)    # would be an error: total is lent to another spawn
   ```
2. Should the table's background-thread row (✗ until `Sync`) apply to
   scoped threads too?
3. Should scoped threads bound their code `Fn`, as C2 bounds separate
   memory, so no spawn writes a capture?

### P3. Domain Instances Must Be Static Types

**Effect.** Question 4 names a handle's home in its type, and dynamic
instances are "not planned now". Four case families pick instances at run
time: a pool worker that keeps a model (case 1), a machine's GPUs (case
7), 64 partitions on scheduler-chosen nodes (case 8), and every spawned
process (case 9). The failure is loud: these programs cannot be written.
The new evidence is that every actor, cluster and multi-GPU library in the
cases needs it.

**Candidates.**

1. Should a fresh domain type be introduced per run-time instance, as
   Rust's generativity brands a value inside a closure?
   ```text
   procs.spawn!(account, fn[D](pid: Mailbox[AccountMsg, D]) -> void: use_it(pid))    # hypothetical syntax
   ```
2. Should a pool's handles carry a run-time home id, checked at use, with
   a `CrossError` case on mismatch? This reverses question 4's "no run-time
   check", so it needs the owner's explicit reopening.
3. Should a handle to immutable data be readable anywhere by fetching, as
   Ray's object refs are, so only mutable handles keep required affinity?

### P4. An Immutable Top-Level Binding To A Box Escapes Question 17

**Effect.** Probe: `CACHE := box([+1])` at top level, and a function that
calls `CACHE.push(+2)`, pass `hd check` and run. Question 17 allows
reading an immutable top-level binding, and `CACHE` is one. So a worker
crossing that reaches it mutates the worker's own copy, the divergence the
rule exists to stop. On a background thread it races. Comlink workers keep
module-level caches exactly this way (case 1).

**Candidates.**

1. Should the rule be "reaches no top-level binding whose type holds a
   `*mut` or a cell", not "no `let mut`"?
2. Should top-level initializers be barred from making boxes, so module
   storage is plain data?
3. Should question 17 merge with the `Fn` bound and the future `Send` into
   one check over everything a crossing reaches (question C, cut K4)?

### P5. Two Shapes For One Crossing Concept

**Effect.** Under V1, a same-build crossing is a method call on a domain
value: `workers.call!(...)`, `s.spawn(...)`, `gpu.run!(...)`. A
cross-target crossing is a bare `load_user!(id)` on an `@remote(Server)`
function, with `$ Server` in the caller's row. A reader learns two forms
for one idea, and the forms behave differently. One names its destination
at the site; the other hides it. One is swapped in tests by an ordinary
provider; the other by a server-shaped provider whose methods are unstated
(case 10). P6 and P7 follow from the split. Question 14 now answers this
with V2 (section 7), so the problem stands as evidence for that answer,
not as an open choice.

**Candidates.**

1. Should section 7's note on the dropped `@remote(D)` form cite cases 10
   and 14 as the reason, so the choice is not re-asked?

### P6. An Unmarked Caller Has No Domain

**Effect.** V1 compiled a call to an `@remote(D)` function as a normal call
in D's build and a stub elsewhere, with `$ D` needed "from another
domain". Unmarked shared code lands in several builds, so it has no one
domain. In case 14, either `dashboard!` needs `$ Server` on the server
too, or a function's error type changes with the build. Tests (case 10)
and generic endpoints (case 16) inherit the question. Under V2 a bare call
never crosses (section 6, Reading It), so the problem does not arise.

**Candidates.**

1. Should section 7 record this as a second reason V1 was dropped?

### P7. A Remote Call Looks Local At Its Call Site

**Effect.** Under V1, `load_user!(id)` reads as any bang call; the row's
`$ Server` says some call in the body crosses, not which. Under V2,
`users.load_user!(id)` reads as any provider call; only the trait's
`< Remote` says it may cross; section 6's Reading It says so itself:
`users.load_user!(id)` and `db.find_user!(id)` look alike. Question B
counts 13 such sites in the cases. At none of them can a reader see the
cost, and under V1 not the destination either.

**Candidates.**

1. Should a remote call carry a site mark, as Swift's `try await` does?
2. Should `hd` tooling (hover, lint) name the destination and the mode at
   each site, with no language change?
3. Should the provider name carry it by convention only, as tRPC's
   `trpc.` prefix does?

### P8. A2 Bars Handles; Trust Is No Property

**Effect.** A2 lets a method of another build take and return
boundary-safe types only, and a handle is not one. Three needs collide
with it. A `Kernel` method must name the texture it works on, and Running
It launches kernels "with its handles" (case 7). Example 7 needs a
server-made `Action` the browser holds (case 15). A stream would be a
handle (case 17). Section 6's table also marks a handle to a service "its
home only", which A2 forbids outright. Case 8 shows a second gap: a Spark
driver may send arbitrary closures to trusted executors, while a server
must never run a client-chosen code identity, and no domain property
records trust.

**Candidates.**

1. Should A2 admit handles whose home is the method's own domain, so a
   `Kernel` method takes `Texture[D]` on device D and a service returns
   its own handles?
   ```text
   pub trait Filters < Kernel:
       fn blur!(&mut self, tex: Texture[Gpu0]) -> Result[void, GpuError]    # would be valid on Gpu0
   ```
2. Should a remote result admit a sealed handle that the home domain made
   and authenticates, as Next.js encrypts bound action captures?
3. Should a fifth domain property, trust, decide whether the destination
   accepts arbitrary code identities (workers, cluster executors) or only
   declared endpoints (services)?

### P9. A Crossing Returns Exactly One Value

**Effect.** gRPC streaming, tRPC subscriptions, LLM tokens (case 17),
Comlink progress callbacks (case 1), Kotlin's hop back to the UI thread
(case 18) and Erlang's one-way sends (case 9) need more or fewer than one
result. The model is call and return, so each has no form.

**Candidates.**

1. Should std add a stream handle that crosses in the handle mode, with
   each item copied and a per-item `Result` that can end in `CrossError`?
2. Should a domain kind offer a channel handle for progress and one-way
   messages?
3. Should streaming stay library work over repeated crossings, as
   pagination does?

### P10. Unknown Has Several Spellings

**Effect.** In case 13 a client timeout arrives as `TimedOut`, though the
order's outcome is as unknown as `.Unknown`. The natural `.Unknown` arm
misses it, silently, in both variants. In case 14 two unknowns sit at two
depths. `CrossError`'s other cases (a remote panic, a decode failure, a
refused call, another build) are not named, so no match can be exhaustive
by intent. Section 6's Running It now sends a cancel to the server and
makes a stub's own timeout a `CrossError`, `Unknown` when the call may
have run. A caller's `std.task` timeout still yields `TimedOut`, and the
deadline does not travel.

**Candidates.**

1. Should `CrossError` implement `From[TimedOut]` as `.Unknown`, so a
   timed-out crossing reads as unknown?
2. Should a crossing take its deadline from the destination value and
   report expiry as `.Unknown`, sending the deadline on as gRPC does?
3. Should the record list `CrossError`'s cases now, each with what it says
   about the remote effect: did not run, may have run, or ran?

### P11. Crossing Failure In Result Types

**Effect.** Under V1, `?` on `Result[T, OrderError | CrossError]` into a
named error with an `@from` for each member is
`invalid-result-propagation` (probe, case 11). Every caller that wants `?`
writes a match. Under V2, `?` works, but the server impl and every fake
must carry a `Cross` case they never produce, and server-side matches must
cover it.

**Candidates.**

1. Under V1, should `?` convert a union error member by member when the
   target has a `From` for each member?
2. Under V2, should the server-side trait and the client-side error
   differ, with the stub's error type derived from the trait's?
3. Should the cost stay as it is in either variant, documented?

### P12. No Compilation Or Runtime Strategy For Threads, Kernels, Or Ids

**Effect.** Question A traces the stages. Running It lists the missing
runtime pieces, but three parts need a strategy the record does not
describe, in both variants. Shared-memory threads (cases 4, 5, 18) need
Wasm shared memory and atomic RC counts, and nobody has said whether
every program pays. A background-thread panic is "a `CrossError`" in
Running It, yet a panic poisons the one instance both threads share (case
5). GPU kernels (case 7) need a device backend; B1 names "device code" as
an output, but hd emits only Wasm. V1's generic endpoints (case 16) need
instances that another target asks for; B5's agreement on method numbers
covers V2.

**Candidates.**

1. Should a program that uses a shared-memory domain get an atomic-RC
   lowering profile, with other programs unchanged?
2. Should device kernels wait until a device backend is designed, with the
   device kind out of scope until then?
3. Should a background-thread panic poison the whole instance, as today,
   with "a crash is a `CrossError`" kept for workers, which have their own?

### P13. A Local Test Server's Semantics Are Unstated

**Effect.** Under V1, case 10 cannot say which build runs the test,
whether the body sees the test's `Db`, or whether arguments are encoded.
A canned-answer fake needs typed `Server` methods. V2's fakes are trait
impls, so the problem does not arise there.

**Candidates.**

1. Should a local server encode and decode, and give the body only the
   providers it was built with (`LocalServer { db }`)?
2. Should `$.with` swap one `@remote` function at a time, as MSW mocks one
   route?

### P14. Later Jobs: Handles Across Restarts, Errors With No Caller

**Effect.** L4 allows handles in a job (case 6), but a device or worker
handle may not survive a restart, and L3 keeps its data alive until the
job runs. A job's `Err` has no caller; retries and dead letters are
unstated.

**Candidates.**

1. Should the later domain take copies only, or handles marked durable?
2. Should `Jobs` own a failure policy (retry, dead letter) as a std item?

### P15. Per-Request Capabilities On A Server

**Effect.** `Session` in case 2 comes from the request. Under V1 the row
check needs it in the target's set, but nothing binds a provider per
crossing. Under V2 the impl method cannot list it at all (P16).

**Candidates.**

1. Should an endpoint's adapter bind request-scoped providers from the
   crossing's transport metadata?
2. Should a session stay an explicit parameter, checked in the body?

### P16. A V2 Endpoint Cannot Use Rows

**Effect.** Probe: an impl method that adds `$ Billing` to its trait's
empty row reports `trait-method-signature`. So a V2 server impl reaches
its database, its billing stub and its session through fields set before
`serve!` (cases 2, 14). The alternative, listing `$ Db` in the shared
`Remote` trait, would put server keys in every browser caller's row.
Elsewhere in hd, capabilities travel in rows and tests swap them with
`$.with`; here they would travel in struct fields. C3, which resolves a
body's `$.use` at the destination, then has nothing to check for a
service, and X1's "what its `main` installs with `$.with`" never reaches an
endpoint's body.

**Candidates.**

1. Should a `Remote` trait's implementation be allowed a row of its own,
   checked against the serving target's set, as V1 checks an `@remote`
   function?
2. Should `serve!` take the providers its impl needs and bind them per
   request, so the impl's methods keep an empty row?
3. Should server dependencies live in fields, documented as the V2 style?

### P17. GPU Kernels Have No Launch Geometry Or Device Subset

**Effect.** Case 7's kernel takes a whole texture; CUDA and WGSL kernels
run per invocation with an index and a grid. The device subset is a listed
gap.

**Candidates.**

1. Should a kernel type take an invocation index, with the grid at the
   dispatch call?
2. Should kernels wait with the device backend (P12)?

### P18. V2's Open Details

**Effect.** `serve!(impl)` exposes "the trait's methods", but one type may
implement two `Remote` traits, and then which endpoints are served is
unclear. Section 6's `Cross(cause: CrossError)` has no `@from`, and A3
says only that the error "can hold" one, so B3's stub has no stated way to
build the case (case 11).

**Candidates.**

1. Should `serve!` name the trait, `serve!::[Users](impl)`, so one call
   serves one trait?
2. Should a `Remote` method's error type be bounded `From[CrossError]`, so
   the stub converts with `From`?

### P19. Editorial, Internal Contradictions And Compiler Lag

- Section 4 still says a crossing's result widens to `Result[T, E |
  CrossError]`, and its sketches match `.Err(.Unknown)` and `._1(.Unknown)`
  directly. A3 says no result is widened, so those arms would be
  `.Err(OrderError.Cross(.Unknown))` and the like.
- Section 5's copy rule says any copyable value copies in shared memory;
  section 6's table refuses a box on a background thread.
- Section 6's Reading It says a crossing is "a bang method", but a scope's
  `s.spawn(...)` has no bang.
- Section 5 names the error `CrossingError`; sections 4 and 6 name it
  `CrossError`.
- Section 6's test `$.with(Users=FakeUsers {}, Dom=FakeDom {})` needs
  `box(...)`, as every provider does today.
- The row spelling of a domain key with capabilities, such as
  `$ Workers[$ Log]`, is not given.
- The checker reports `generic-kind-mismatch` for `Result[T, A | B]`,
  although the spec's union syntax allows it; an alias works.
- `SpanMut` has no `split_at_mut`; `slice_mut` consumes its span.

## Owner Question A: How The Compiler Compiles It

Today `hd_mono` collects instances from one entry, and `hd_wasm_linear`
emits one module. Both variants need one walk and one module per target.
They differ in what marks an endpoint and what generates a stub. Section
6's B1 to B5 now describe the V2 column; the trace below agrees with them
and adds what they leave open.

### The Section 6 Program, Side By Side

| | V1 | V2 (B1 to B5) |
| --- | --- | --- |
| browser module's entries | `on_open` and the handlers | `browser_main` (B1) |
| server module's entries | `main`, plus one endpoint per `@remote(Server)` function | `server_main`; `serve!` reaches the endpoints (B1, B2) |
| how `load_user` gets into the browser | the call site compiles to a stub | `remote::[Users]` instantiates a stub type (B3) |
| how it gets into the server | its body and an endpoint export | `PgUsers.load_user`, reached through `serve!`'s dispatcher (B2) |
| `PgUsers`, `dep.postgres` | server only | server only (B1) |
| who finds the endpoints | the per-target walk, from `@remote` marks | mono, from `serve![T]`'s instances |
| crossing ids | per `@remote` function, shared by both modules | method numbers, agreed by one compile (B5) |
| a peer from another build | unstated | refused with a `CrossError` by build ID (B5) |

**V1 stub and endpoint**, shown as Rust for brevity:

```rust
// browser module: generated in place of load_user's body
async fn load_user(id: i64) -> Result<User, Union2<DbError, CrossError>> {
    let args = encode_tree(&(id,));                       // module.boundary encoder
    match host::cross(SERVER, FN_LOAD_USER, args).await { // hd.cross.start / hd.cross.finish
        Reply::Ok(bytes)   => decode::<Result<User, DbError>>(bytes).map_err(Union2::_0),
        Reply::NotSent     => Err(Union2::_1(CrossError::/* unnamed: did not run */)),
        Reply::Lost        => Err(Union2::_1(CrossError::Unknown)),
        Reply::RemotePanic => Err(Union2::_1(CrossError::/* unnamed */)),
    }
}
// server module: exported as hd.endpoint.<id>
async fn endpoint_load_user(args: Bytes) -> Bytes {
    let (id,) = match decode_untrusted::<(i64,)>(args) { Ok(a) => a, Err(_) => return BAD_ARGS };
    encode_tree(&load_user(id).await)                     // the body runs under the server's Db
}
```

**V2 stub and dispatcher**:

```rust
// browser module: remote::[Users](url) builds this provider
impl Users for RemoteUsers {
    async fn load_user(&mut self, id: i64) -> Result<User, UserError> {
        match host::cross(&self.url, USERS_LOAD_USER, encode_tree(&(id,))).await {
            Reply::Ok(bytes) => decode::<Result<User, UserError>>(bytes),
            Reply::Lost      => Err(UserError::from(CrossError::Unknown)),  // needs From[CrossError]: P18
            _                => Err(UserError::from(CrossError::/* unnamed */)),
        }
    }
}
// server module: serve!(PgUsers { pool }) instantiates this dispatcher
async fn dispatch_users(imp: &mut PgUsers, id: u32, args: Bytes) -> Bytes {
    match id {
        USERS_LOAD_USER => { let (a,) = decode_untrusted::<(i64,)>(args)?; encode_tree(&imp.load_user(a).await) }
        _ => BAD_ID,
    }
}
```

In both, the stub's poll step lowers to a waiting host method, a
`.start`/`.finish` import pair, as other waiting host methods already do:

```wat
(call $hd.cross.start (i32.const 1) (i32.const 7) (local.get $args) (local.get $len))
(local.set $op)          ;; saved in the suspension frame
;; poll: (call $hd.cross.finish (local.get $op)) -> status, result pointer
;; cancel glue: (call $hd.cross.abort (local.get $op))
```

**Encoding and errors.** Arguments and results use the boundary tree
encoding
([`module.boundary.tree`](../../spec/lang/10-modules.md#r-module.boundary.tree)),
and the endpoint's decode is the untrusted side. V1 maps a transport
status into the union's `_1` case. V2 maps it through the declared error's
`From[CrossError]`, which A3 does not yet require (P18). Both need
`CrossError` cases the record has not named: "not sent", "remote panic",
"bad shape" and "other build" (P10, P1).

**Suspension and cancellation.** A `Suspend[T]` never crosses; it is not
boundary-safe. The stub is a suspension frame waiting on a host operation,
and the endpoint is a root suspension on the server's entry driver.
Cancelling the stub runs its cancel glue, which calls `hd.cross.abort`
([`req.cancel.external-abort`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.external-abort)).
Whether the server's adapter then cancels the endpoint's root is unstated
(P10).

**A strategy V2 needs that the record lacks.** `remote::[Users]` must
build an impl of an arbitrary trait from its method list. Today's
`Structure` generator walks types, not trait methods, so this is a new
compiler generator. Its signature, a function returning `*mut dyn T` for
a type parameter `T`, is not a plain hd declaration, which an intrinsic's
signature must be.

### A Worker Crossing On The Same Build (Case 1; Both Variants)

The worker runs the same module bytes in another instance, so no second
walk is needed. `workers.call!(closure)` monomorphizes per closure type.
B4 names the code by its index in the module's function table; since both
instances load the same bytes, the index agrees. The module exports one
dispatcher that decodes the environment for that index:

```rust
// both instances: the same module
fn dispatch(id: u32, env: Bytes) -> Bytes {
    match id {
        CLOSURE_17 => { let (photo,) = decode::<(Image,)>(env); encode_tree(&shrink(photo, 256)) }
        _ => BAD_ID,
    }
}
```

Captures encode as one tuple. A transfer, once #1141 lands, is a move,
plus a detach of the buffer where the host allows it (`postMessage`
transfer lists). Cancel calls `hd.cross.abort`; whether that terminates
the worker or posts a cancel message is a runtime choice to state.

### A Background Crossing (Cases 5 And 18; Both Variants)

The background thread is a second Wasm thread over the same shared
memory. That needs Wasm threads and atomics in the module, a shadow stack
and globals per thread, and atomic RC updates on every value that can be
shared. This is a new lowering profile in `hd_wasm_linear`, and the record
does not say which programs pay for it (P12). Nothing is encoded: a copy
is an RC increment, which is why P2's box copy races.

### A GPU Kernel (Case 7; Both Variants)

The kernel's TIR must become device code (WGSL or SPIR-V), embedded in
the build and named by id. Under V1, `gpu.run!(blur, tex)` lowers to a
waiting host method with that id and the texture's handle slot. Under V2,
B1 makes the device a target whose output is device code, the `Kernel`
impl is that target's code, and the host launches a method by number
(Running It). Either way hd has one backend, Wasm, so this is a new
backend plus C1's device-subset check (P12, P17). V2 adds a blocker: A2
bars the handle the launch passes (P8).

### Stages That Change

Sizes are rough per-component guesses in Rust lines, not measured.

| Stage | V1 | V2 |
| --- | --- | --- |
| `hd_project`: the targets table (question 13) | 300 | 300 |
| `hd_resolve` | `@remote(D)` in the folder interface: 300 | the `Remote` marker is an ordinary supertrait: 50 |
| `hd_check`, cross-target | `@remote` checks, the `$ D` call rule, the `E \| CrossError` rewrite, per-build typing (P6): 1,200 | `Remote` trait checks (plain data, not generic, error holds `CrossError`): 500 |
| `hd_check`, shared | modes at captures, a boundary-safe bound, the handle marker, `at`, the top-level summary: 1,100 | the same: 1,100 |
| `hd_driver`: the top-level summary task, one output per target | 600 | 600 |
| new pass: crossing ids, whole program | 400 | 300 |
| build ID stamp and check (B5) | 100 | 100 |
| `hd_mono` | a walk per target with `@remote` entries; generic instances another target asks for: 600 | a walk per target; `remote::[T]` and `serve!` instances: 400 |
| new generator | none | the stub impl and the dispatcher per `Remote` trait: 700 |
| codecs, if the host ABI codecs do not serve | 800 | 800 |
| `hd_wasm_linear`: stubs as waiting host methods, endpoint exports, the dispatcher | 900 | 600 |
| runtimes and hosts: worker transport, HTTP endpoints, a job store | 2,000 or more | 2,000 or more |
| shared-memory profile | not estimated: a design first | same |
| device backend | not estimated: a design first | same |
| total without threads and devices | about 8,300 | about 7,450 |

## Owner Question B: Can A Reader See A Remote Call?

At each crossing site in the cases, four questions: is it remote (R),
which destination (D), what it costs (C: what is copied, moved or lent),
and how it fails (F: does the site handle a crossing failure, not just
"some error").

| Case | Site under V1 | Site under V2 | R V1 | R V2 | D V1 | D V2 | C | F |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `workers.call!(...)`, two sites | `workers.run!(...)` | yes | yes | yes | yes | no | yes |
| 2 | `rename_todo!(id, title)` | `todos.rename_todo!(...)` | no | no | no | yes | no | no |
| 3 | `user_by_id!(id)` | `api.user_by_id!(id)` | no | no | no | yes | no | no |
| 4 | `s.spawn(...)`, three sites | same | yes | yes | yes | yes | no | yes |
| 5 | `$.use(Background).run!(...)` | same | yes | yes | yes | yes | no | yes |
| 6 | `jobs.after!(...)`, two sites | same | yes | yes | yes | yes | no | no |
| 7 | `gpu.upload!`, `run!`, `read_back!` | same | yes | yes | yes | yes | yes | yes |
| 8 | `at(part).call!(...)` | same | yes | yes | yes | yes | no | yes |
| 9 | `procs.spawn!`, `pid.send!` | same | yes | yes | no | no | no | yes |
| 10 | `load_user!(1)` | `users.load_user!(7)` in `on_open!` | no | no | no | yes | no | no |
| 11 | `place_order!(cart)`, two sites | `users.load_user!(id)` | no | no | no | yes | no | one of two |
| 12 | two stubs in `all!` | two provider calls in `all!` | no | no | no | yes | no | no |
| 13 | `place_order!(cart)` | `orders.place_order!(cart)` | no | no | no | yes | no | yes |
| 14 | four stub sites | four provider calls | no | no | no | yes | no | one of four |
| 16 | `load_setting!::[Theme]` | `settings.load_theme!()` | no | no | no | yes | no | no |
| 17 | `list_sales!`, `stream_sales!` | two provider calls | no | no | no | yes | no | no |
| 18 | `background.run!(...)` | same | yes | yes | yes | yes | no | yes |

There are 30 sites in each variant: 13 cross-target sites and 17
same-build sites.

| Hidden at the site | V1 | V2 |
| --- | ---: | ---: |
| remote (R), cross-target sites | 13 of 13 | 13 of 13 |
| destination (D), cross-target sites | 13 of 13 | 0 of 13 |
| cost (C), all sites | 27 of 30 | 27 of 30 |
| crossing failure (F), all sites | 13 of 30 | 13 of 30 |
| destination (D), same-build sites | 2 of 17 | 2 of 17 |

V2 names the destination at every cross-target site, through the
receiver. Neither variant shows that the call crosses or what it costs.
Cost is the weakest answer everywhere: the mode is inferred from the type
(question 7), so case 1's 50 MB copy and case 18's box copy look like a
small copy. Under V2 a reader cannot tell a remote call from a local
provider call, which is the owner's first listed weak point, and also the
reason a test can swap it.

| System | Remote visible | Destination visible | Cost visible | Failure visible |
| --- | --- | --- | --- | --- |
| hd V1 stubs | no; only the row's `$ Server` | no | no | in the type only |
| hd V2 provider calls | no; only the trait's `< Remote` | yes: the receiver | no | in the declared error |
| hd domain values (both) | yes | yes, except run-time instances | no | `?` or `match` |
| Swift distributed actors | yes: `try await` on every remote call | yes: the actor reference | no | yes: `try` |
| gRPC stubs | yes: the client object | yes: the connection | partly: an explicit request message | yes: a status |
| React `"use server"` | no: looks local | no | no | partly: a thrown error |
| tRPC | yes: `trpc.path.query` | yes: the router path | no | partly: a rejected promise |

V1's stub sites read like React's. V2's read like a Swift distributed
actor call without the `try await`, or a gRPC stub without the `ctx`.

**Section 6's three reading signals, tested.** Reading It says a reader
finds every crossing from the row, the call ("a bang method on a
destination value") and the types. In the case blocks, 13 local provider
calls have that same shape, such as `$.use(Db).find_user!(id)` and
`$.use(Sales).query!(f)`. So the call signal marks "may cross" at 30
crossing sites plus 13 local ones, and only the trait's `< Remote` or the
row key's trait sorts them. The signal also misses the scope's
`s.spawn(...)`, which has no bang (P19). The row signal holds: every V2
caller lists the service key, as `$ Users`.

## Owner Question C: Uniform, Simple, Orthogonal?

### Each Construct's One Job

| Construct | Its one job | V1 | V2 | Covered elsewhere, or interacts badly |
| --- | --- | --- | --- | --- |
| `@remote(D)` | compile a function as D's endpoint and a stub elsewhere | yes | no | a second way to cross (P5) |
| `$ D` caller rule | say a function talks to D | yes | no | the one row key no `$.use` reads (below) |
| implicit `E \| CrossError` | add the crossing's failure to the result | yes | no | `?` cannot see through it (P11) |
| `Remote` marker trait | mark a trait whose methods are endpoints | no | yes | an ordinary supertrait; impl rows must match (P16) |
| `Kernel` marker trait | mark a trait whose methods are device code | no | yes | A2 bars the handles kernels need (P8) |
| what-cannot-cross table | say per kind what crosses | yes | yes | restates the mode table, adds box and closure rows; disagrees with section 5 (P19) |
| `remote::[T](url)` | make a stub provider for a `Remote` trait | no | yes | a new generator over trait methods (question A) |
| `serve!(impl)` | expose an impl's `Remote` methods | no | yes | ambiguous for two traits (P18) |
| domain values (`Workers`, `Background`, `Jobs`, `Gpu`) | name a destination; carry its capabilities | yes | yes | the uniform form |
| `at(handle)` | target the domain a handle lives in | yes | yes | needs static homes (P3) |
| `CrossError` | carry the crossing's own failure | yes | yes | overlaps with `TimedOut` (P10) |
| the handle marker | tell the checker a type is a handle | yes | yes | parallel to `AutoClosing`; no conflict found |
| `scope` | join threads before borrowed data ends | yes | yes | cells race (P2) |
| six domain kinds | fix memory, capabilities and lifetime per destination | yes | yes | derivable from three properties (cut K1) |
| four modes | say how each value crosses | yes | yes | reuse copy, move and borrow; only the handle is new |
| L1 to L4 | lifecycle guarantees | yes | yes | L1 and L2 hold by construction; L4 is a table row |
| the top-level rule | no shared mutable module state | yes | yes | misses boxes (P4); overlaps with the `Fn` bound and `Send` |

The coordinator's two checks:

- **`@remote` versus `workers.call!`.** Under V1 they are two ways to
  cross. A worker crossing names its destination at the site; a service
  crossing names it at the declaration and hides it at the site. The split
  follows from compilation, since only services need a second build. It
  shows at every stub site (question B) and creates the per-build typing
  question (P6). V2 has one shape.
- **`$ D` versus provider rows.** `$ Workers` is a provider row: the body
  calls `$.use(Workers)`. Under V1, `$ Server` is a row key that no
  `$.use` in the body reads; the stub reads it implicitly. Under V2,
  `$ Users` is an ordinary provider row. V2 pays elsewhere: a server impl
  cannot use rows at all (P16).

V2's listed weak points, checked against the cases. The server impl's dead
`Cross` case is real (case 11, P11). The `serve!` ambiguity is real but
small (P18). `run!` takes a closure argument because `?` cannot follow a
trailing block; that costs nothing in the cases, and every block above
parses with the closure inside the call. Streaming is uncovered in both
variants (P9). The cases add two weak points the list lacks: an endpoint
gets no row of its own (P16), and A2 bars kernels' handles (P8).

### Construct And Rule Counts

Counting rules: a construct is a name the compiler must know (a mark, a
marker trait, a generator, a checker rule with its own name); a std item
written in plain hd is not one. A rule is a table cell, list item or
labeled rule the record states. A restatement counts once: C1 to C3
restate section 5's checks, and the what-cannot-cross table restates the
mode table except its box and closure rows. Rules that do not depend on
the spelling are shared and counted for both.

| Area | V1 rules | V2 rules |
| --- | ---: | ---: |
| principle | 1 | 1 |
| domain kinds (one row each) | 6 | 6 |
| domain-by-mode table (cells) | 24 | 24 |
| what-cannot-cross table: the box and closure rows (cells) | 10 | 10 |
| L1 to L4 | 4 | 4 |
| affinity | 2 | 2 |
| the `Unknown` case | 1 | 1 |
| section 5 checks (C1 to C3 included) | 11 | 11 |
| top-level rule | 1 | 1 |
| building: B1, B4, B5 | 3 | 3 |
| shared subtotal | 63 | 63 |
| cross-target spelling | 10: row against the target; plain-data signatures; endpoint and stub per build; the `$ D` rule; the widened error; the build walk with `@remote` entries; the three-row call table | 7: A1, A2's two parts (plain data; not generic, no closure), A3, A4, B2, B3 |
| **total rules** | **73** | **70** |
| **compiler-known constructs** | **9**: `@remote`, the `$ D` rule, the widened error, the handle marker, `at`'s target from a type, the kind-by-mode rule, the top-level summary, `CrossError`, `scope`'s thread loans | **10**: the `Remote` and `Kernel` markers, the `remote` generator, `serve!`'s dispatcher, the handle marker, `at`'s target from a type, the kind-by-mode rule, the top-level summary, `CrossError`, `scope`'s thread loans |

The counts are close. V2 trades V1's three call-site constructs for four
declaration-site ones (two markers, the generator, the dispatcher), and it
has three fewer rules, all in the cross-target spelling. The record now
states the mode rule twice, in section 3's table and section 6's, and
section 5 states a third version for shared memory (P19).

### Candidate Cuts

None of these is chosen; each is a question for the owner, and each
applies to either variant.

- **K1. Six kinds become three properties.** Each kind is a choice of
  shared or separate memory, scoped or not, and durable or not. Five
  rules replace the 34 cells of the two tables: lend needs shared memory
  and a scope; transfer is refused in a durable domain (L4); a copy into
  separate memory must be boundary-safe; a box needs `Sync`; a closure is
  the code, except into another build. Trust (P8) would be a fourth
  property.
- **K2. L1, L2 and L4 move to notes.** The record says L1 and L2 need no
  checker rule, and L4 is K1's durable rule. L3 stays as a runtime
  contract.
- **K3. One crossing shape.** This is V2 itself, or P5's second candidate
  for V1.
- **K4. One "crossable" check.** The `Fn` bound (q10), the top-level rule
  (q17) and the future `Send` answer one question: what the crossing's
  code reaches that is shared and mutable. One check over captures,
  reached top-level bindings and boxes would replace three, as Swift's
  `Sendable` does, and close P2 and P4.
- **K5. One unknown.** `CrossError` takes `TimedOut` as `.Unknown`, and
  section 5's `CrossingError` goes (P10, P19).

K1 turns 6 kind rows and 34 cells into 3 properties and 5 rules, and
absorbs section 5's mode and later checks (11 checks to 9). K2 drops 3 of
L1 to L4. K4 folds the top-level rule into the `Fn` bound's check, with
`Send` to come. K5 adds no rule and removes a second error name. The
shared subtotal goes from 63 to 25.

| After K1, K2, K4 and K5 | V1 | V2 |
| --- | ---: | ---: |
| total rules | 35 | 32 |

**Evidence, not a choice.** The cases favor V2 on uniformity (P5), on one
type per shared function (P6), on testing (case 10) and on `?` (case 11).
They favor V1 on generic endpoints (case 16), on server capabilities kept
in rows (P16), and on devices, which V1 left as a domain value and V2's A2
blocks (case 7). The four highest-ranked problems, P1, P3, P2 and P4, are
the same under both.

## Sources

- Next.js, data security and closures in Server Actions: <https://nextjs.org/docs/app/guides/data-security>
- Next.js, "Failed to find Server Action": <https://nextjs.org/docs/messages/failed-to-find-server-action>
- Vercel, Skew Protection: <https://vercel.com/docs/skew-protection>
- Comlink: <https://github.com/GoogleChromeLabs/comlink>; MDN, transferable objects: <https://developer.mozilla.org/en-US/docs/Web/API/Web_Workers_API/Transferable_objects>
- tRPC procedures: <https://trpc.io/docs/server/procedures>; subscriptions: <https://trpc.io/docs/server/subscriptions>
- gRPC core concepts (streaming, deadlines, cancellation): <https://grpc.io/docs/what-is-grpc/core-concepts/>
- Protocol Buffers, updating a message type: <https://protobuf.dev/programming-guides/proto3/#updating>
- Rust, `thread::scope`: <https://doc.rust-lang.org/std/thread/fn.scope.html>; `Send` and `Sync`: <https://doc.rust-lang.org/nomicon/send-and-sync.html>; rayon `join`: <https://docs.rs/rayon/latest/rayon/fn.join.html>
- Kotlin, coroutine context and dispatchers: <https://kotlinlang.org/docs/coroutine-context-and-dispatchers.html>
- Swift, `Task.detached`: <https://developer.apple.com/documentation/swift/task/detached(priority:operation:)-d24l>; SE-0302 `Sendable`: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0302-concurrent-value-and-concurrent-closures.md>; SE-0412 global variables: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0412-strict-concurrency-for-global-variables.md>
- Swift SE-0336, distributed actor isolation: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0336-distributed-actor-isolation.md>; SE-0344, distributed actor runtime (generic substitutions): <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0344-distributed-actor-runtime.md>
- Sidekiq, best practices: <https://github.com/sidekiq/sidekiq/wiki/Best-Practices>
- Temporal, versioning in Go: <https://docs.temporal.io/develop/go/versioning>
- CUDA C++ programming guide: <https://docs.nvidia.com/cuda/cuda-c-programming-guide/>; wgpu: <https://docs.rs/wgpu>
- Spark, understanding closures: <https://spark.apache.org/docs/latest/rdd-programming-guide.html#understanding-closures->
- Ray, objects: <https://docs.ray.io/en/latest/ray-core/objects.html>; scheduling: <https://docs.ray.io/en/latest/ray-core/scheduling/index.html>; Dask futures: <https://docs.dask.org/en/stable/futures.html>
- Erlang, `monitor/2`: <https://www.erlang.org/doc/apps/erts/erlang.html#monitor/2>; Akka typed actors: <https://doc.akka.io/docs/akka/current/typed/actors.html>

## Parse Log

All 31 `text` blocks were parsed on 2026-11-02 with
`scripts/hd-conformance.sh parse`, which checks syntax only; four blocks
sit inside list items and were parsed after removing their indent. Lines
that use unspecified syntax end in `# hypothetical syntax`; std names
(`Workers`, `Background`, `Jobs`, `Gpu`, `scope`, `at`, `Remote`,
`Kernel`, `remote`, `serve`) are sketches whose syntax is ordinary.

| Block | Where | Result |
| --- | --- | --- |
| 1 | Surface, V2's surface from section 6 | parse |
| 2 | Case 1 | parse |
| 3, 4 | Case 2, V1 and V2 | parse; V1's `@remote(Server)` lines `# hypothetical syntax` |
| 5, 6 | Case 3, V1 and V2 | parse; `@remote(Api)` lines `# hypothetical syntax` |
| 7 | Case 4 | parse |
| 8 | Case 5 | parse |
| 9 | Case 6 | parse |
| 10, 11 | Case 7, V1 and V2 | parse |
| 12 | Case 8 | parse |
| 13 | Case 9 | parse |
| 14, 15 | Case 10, V1 and V2 | parse |
| 16, 17 | Case 11, V1 and V2 | parse |
| 18 | Case 12 | parse |
| 19 | Case 13 | parse |
| 20 to 22 | Case 14: V1 same service, V1 two services, V2 | parse; `@remote` lines `# hypothetical syntax` |
| 23 | Case 15 | parse; `@remote` lines `# hypothetical syntax` |
| 24, 25 | Case 16, V1 and V2 | parse, after a fix: the first draft joined bounds with `+`; hd joins them with `&` |
| 26 | Case 17 | parse |
| 27 | Case 18 | parse |
| 28 | P1, candidate 1 | parse |
| 29 | P2, candidate 1 | parse |
| 30 | P3, candidate 1 | fails as expected at `fn[D]`: a generic closure is `# hypothetical syntax` |
| 31 | P8, candidate 1 | parse |

Five probes ran `hd check` (the worktree's dev build) on scratch files,
not on report blocks:

| Probe | Result | Used in |
| --- | --- | --- |
| `CACHE := box([+1])` at top level, mutated by a function | checks and runs, printing `2` | P4 |
| `?` on `Result[i64, Failure]`, `Failure = OrderError \| CrossError`, into an error with an `@from` for each | `invalid-result-propagation`; the union's `Display` checks | case 11, P11 |
| the same union written inline, `Result[i64, OrderError \| CrossError]` | `generic-kind-mismatch`, although the union syntax rules allow it | P19 (compiler lag) |
| two `&mut self` bang methods on one provider inside `all!` | checks and runs | case 12 |
| an impl method that adds `$ Billing` to its trait's empty row | `trait-method-signature` | case 2, P16 |

No block that a chapter specifies was rejected by the parser.
