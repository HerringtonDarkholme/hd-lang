# Execution Boundaries: Moving Code And Data

Status: proposal, 2026-11-02; domain: language (execution domains and
crossings); not accepted. Nothing here is in the specification.

- [1. Context And Goal](#1-context-and-goal)
- [2. Eight Programs That Cross](#2-eight-programs-that-cross)
- [3. The Model](#3-the-model)
- [4. Outcomes](#4-outcomes)
- [5. Type Checking](#5-type-checking)
- [6. The Process, End To End](#6-the-process-end-to-end)
- [7. Syntax](#7-syntax)
- [Appendix A. Prior Art](#appendix-a-prior-art)
- [Appendix B. Earlier Options And #166](#appendix-b-earlier-options-and-166)
- [Sources](#sources)
- [Parse Log](#parse-log)

## 1. Context And Goal

The goal is to move computation and data between **execution domains**: a
worker, a server, a job that runs later, a GPU, or a thread. Today an hd
program runs in one instance on one thread
([`module.instance.thread`](../../spec/lang/10-modules.md#r-module.instance.thread)).
Instances exchange only boundary-safe values, copied as trees
([`module.boundary.allowed`](../../spec/lang/10-modules.md#r-module.boundary.allowed),
[`module.boundary.tree`](../../spec/lang/10-modules.md#r-module.boundary.tree)).
Closures never cross
([`module.boundary.not-safe`](../../spec/lang/10-modules.md#r-module.boundary.not-safe)).

So each program below copies too much, cannot say what it needs, or cannot
be written. The design builds one model of a crossing first. The type
checks follow from the model, and syntax comes last.

**Principle: every execution domain runs the same code.** One
program is compiled for every domain it uses: the browser, the server, a
worker, a device, a job that runs later. A crossing never sends code or an
executable over the wire. It sends the identity of code the destination
already has, plus data. So a function or a closure is named by its
identity within that one program, and two domains never disagree about
what a name means. Domains that run different builds, such as a service on
another release or a job resumed by a newer deploy, are out of scope.

## 2. Eight Programs That Cross

Each sketch is hd as it is written today, with the crossing it needs in a
comment. Std names such as `Workers`, `Jobs` and `Gpu` are illustrations.

**1. Image editor: a thumbnail in a web worker.** The page sends a 50 MB
photo and the `thumbnail` code to a worker. A copy keeps the page
responsive but costs an encode and a copy. A transfer is fast, but the
page must not touch the photo until it comes back.

```text
struct Image:
    width: u32
    height: u32
    pixels: List[u8]    # 4096 x 3072 RGBA: 50 MB

fn thumbnail(photo: Image, side: u32) -> (Image, Image):
    small := shrink(photo, side)
    (photo, small)

pub fn on_export!(photo: Image) -> Result[Image, WorkerError] $ Workers:
    # Today: the 50 MB list is encoded, copied into the worker and decoded.
    # Wanted: hand the buffer over, and make any use of `photo` an error until it returns.
    let (back, small) = $.use(Workers).run!(fn() -> (Image, Image): thumbnail(photo, 256))?
    .Ok(small)
```

**2. Checkout: the code runs where its row is met.** Only the server has
`Db + Payments`, so `place_order` must run there. The cart is copied there
and the receipt copied back. When the reply times out, the browser cannot
tell whether the order committed.

```text
pub trait Db:
    fn insert_order!(&mut self, cart: Cart) -> Result[OrderId, DbError]

pub trait Payments:
    fn charge!(&mut self, card: CardToken, cents: i64) -> Result[ChargeId, PayError]

pub fn place_order!(cart: Cart) -> Result[Receipt, OrderError] $ Db + Payments:
    charge := $.use(Payments).charge!(cart.card, cart.total_cents)?
    order := $.use(Db).insert_order!(cart)?
    .Ok(Receipt { order, charge })

# Browser code. The browser binds neither Db nor Payments.
fn on_buy!(cart: Cart) -> Result[Receipt, OrderError] $ Server:
    place_order!(cart)  # error today: missing-requirement; wanted: run it on the server
```

**3. Sign-up email: computation moves in time.** A job queue sends the
welcome mail ten minutes later, across a server restart. The job runs on
the same build that stored it (section 1); draining or re-queuing jobs
across a deploy is the deployment's work, not the language's. The email
address may be captured; a connection or a `&mut` may not.

```text
fn send_welcome!(email: string, name: string) -> Result[void, MailError] $ Mailer:
    $.use(Mailer).send!(email, "Welcome, $name")

pub fn sign_up!(user: User, conn: &mut DbConn) -> Result[void, SignUpError] $ Jobs:
    save_user!(conn, user)?
    # Wanted: a stored job; today a closure is not boundary-safe.
    $.use(Jobs).after!(minutes(10), fn!() -> Result[void, MailError] $ Mailer: send_welcome!(user.email, user.name))?
    $.use(Jobs).after!(minutes(10), fn!() -> Result[void, DbError]: mark_sent!(conn, user.id))?  # wanted: an error, `conn` is a `&mut`
    .Ok(())
```

**4. Photo filter on a GPU: data changes representation.** The photo is
uploaded once, five kernels run on the device, and one image is read
back. On the device the photo is a texture, not a `List[u8]`. The host
holds a handle that means something only on that one device.

```text
pub trait Gpu:
    fn upload!(&mut self, image: Image) -> Result[Texture, GpuError]
    fn run!(&mut self, kernel: fn(&mut Texture) -> void, texture: &mut Texture) -> Result[void, GpuError]
    fn read_back!(&mut self, texture: Texture) -> Result[Image, GpuError]

pub fn retouch!(photo: Image) -> Result[Image, GpuError] $ Gpu:
    gpu := $.use(Gpu)
    let mut tex = gpu.upload!(photo)?    # copied once, rebuilt as a texture
    for kernel in [blur, sharpen, denoise, vignette, tone]:
        gpu.run!(kernel, &mut tex)?      # stays resident on the device
    # Today nothing rejects a kernel that calls println, or `tex` used on a second GPU.
    gpu.read_back!(tex)
```

**5. Parallel word count: borrow, do not copy.** Four threads read
quarters of one big list in place. The scope joins them before it ends,
so the borrow cannot outlive the list. A second pass splits one `&mut`
into two disjoint halves and writes both in parallel.

```text
pub fn word_count(lines: &List[string]) -> Map[string, usize]:
    q := lines.len() / 4
    # Wanted: four threads borrow `lines`; today an instance has no threads.
    counts := scope(fn(s: &mut Scope) -> List[Map[string, usize]]:
        tasks := [for i in 0..4 => s.spawn(fn() -> Map[string, usize]: count_words(lines, i * q, i * q + q))]
        [for t in tasks => t.join()]
    )
    merge_counts(counts)

pub fn normalize(lines: &mut List[string]) -> void:
    let (left, right) = lines.split_halves_mut()    # wanted: two disjoint &mut halves
    scope(fn(s: &mut Scope) -> void:
        s.spawn(fn() -> void: lowercase_all(left))
        lowercase_all(right)
    )
```

**6. Reporting dashboard: code moves to the data.** The browser needs the
200 sales that match a filter. Today it downloads a million rows and
filters locally. The filter code and its small `Filter` value should go
to the server where the rows live.

```text
struct Filter:
    region: string
    since_day: i32
    min_cents: i64

fn matches(f: Filter, sale: Sale) -> bool:
    sale.region == f.region && sale.day >= f.since_day && sale.cents >= f.min_cents

pub fn top_sales!(f: Filter) -> Result[List[Sale], ApiError] $ SalesApi:
    rows := $.use(SalesApi).all_sales!()?          # a million rows cross the network
    .Ok([for s in rows if matches(f, s) => s])     # to keep 200
```

**7. Todo app: a server action, as React's.** The server renders a row
and, with it, the code that renames that todo. The browser keeps the
action and calls it later, when the user submits. Today the server
exposes an endpoint and the page sends the id back by hand, so a page can
send any id, and the endpoint must re-check the user owns it.

```text
pub fn todo_row(todo: Todo) -> Html $ Render:
    # Today: the id rides in the page as plain data, and the client posts it back.
    # Wanted: a closure over `todo.id` that runs on the server; the browser holds
    # only a sealed reference to it and cannot read or change the captured id.
    row([text_input(value=todo.title, on_submit_url="/todos/rename?id=${todo.id}")])

pub fn rename_endpoint!(req: Request) -> Result[Response, TodoError] $ Db:
    id := todo_id(req.query("id"))?                # untrusted: any id can arrive
    title := req.form("title")?
    rename_todo!(id, title)?
    .Ok(ok())
```

**8. Photo scanner on a phone: heavy work off the UI thread.** A tap on
"Scan" runs face detection, which takes two seconds. Only the UI thread may
touch views, as on Android and iOS, so the tap handler must hand the work to
a background thread, keep the UI thread drawing a spinner, and apply the
result back on the UI thread. Both threads share one process's memory, but
the work outlives the handler that started it.

```text
struct Photo:
    width: u32
    height: u32
    pixels: List[u8]    # 12 MP: 48 MB

fn detect_faces(photo: Photo) -> List[Rect]:    # heavy; uses no capability
    scan_regions(photo, 24)

pub fn on_scan_tapped!(photo: Photo) -> void $ Ui:
    $.use(Ui).show_spinner()
    # Today: runs on the UI thread and freezes it for two seconds.
    # Wanted: run detect_faces on a background thread, give the UI thread back to its
    # event loop meanwhile, and resume here on the UI thread with the result.
    # A closure that calls $.use(Ui) must not run on the background thread.
    faces := detect_faces(photo)
    $.use(Ui).hide_spinner()
    $.use(Ui).draw_boxes(faces)
```

Together they cover space (1, 2, 4, 5, 6, 7, 8) and time (3, 7), the modes
copy, transfer and lend (1, 5, 8), a change of representation (4), an
unknown outcome (2), code identity within one program (3, 7), code sent to
data (6), code that stays home while a reference to it travels (7), and
work that outlives its caller in shared memory and returns to a thread
that alone holds a capability (8).

## 3. The Model

### Execution Domains

An execution domain has four properties:

| Property | Question it answers |
| --- | --- |
| Memory | Is memory shared with the source, or separate? |
| Capabilities | Which providers exist there? |
| Code | Which code runs there? Always the same program (section 1) |
| Lifetime | Can it outlive the source? Can contact be lost? |

Six kinds of domain:

| Kind | Memory | Capabilities | Code | Lifetime | Example |
| --- | --- | --- | --- | --- | --- |
| Scoped thread | shared | the source's | same build | ends before its scope | 5 |
| Worker | separate | its own host's | same build | may crash | 1 |
| Remote service | separate | the service's | same build | outlives; contact may be lost | 2, 6 |
| Device | separate | restricted operations, kernels only | device code | may reset | 4 |
| Later / resume | separate | the resuming host's | same build | outlives the source | 3 |
| Background thread | shared | its own: not the UI thread's | same build | outlives the call that started it; ends with the process | 8 |

A background thread shares memory as a scoped thread does, but outlives
the call that started it, as a worker does. So it cannot lend (no scope
ends the loan), a move needs no copy, and its capabilities exclude the UI
thread's `Ui`: a view update sent to it is a type error, where Android
throws `CalledFromWrongThreadException` at run time.

### A Crossing

A **crossing** sends code and an environment to a destination, and returns
a result.

- **Code** is already at the destination, because every domain runs the
  same program (section 1). The crossing names it by its identity in that
  program: a function or a closure for a domain of the same build, a
  method of a declared interface for another build (section 6). No code
  or executable crosses.
- **Environment**: each captured value crosses in one mode, below.
- **Requirement row**: code sent to a domain has its row checked against
  the destination's capabilities, not the source's. An interface method
  gets what it needs from its provider's fields instead (section 6).
- **Result**: crosses back by the same modes.

### Crossing Modes

Four modes for environment values:

| Mode | The destination gets | The source afterwards | Where |
| --- | --- | --- | --- |
| Copy | its own value | unchanged | all domains |
| Transfer | ownership | loses the value until it is handed back | separate memory |
| Lend | a borrow | frozen or excluded until the borrow ends | shared memory, scoped only |
| Handle | a handle; the data stays where it lives | a handle tied to that one domain instance | all domains |

The source note (Sources) named five modes. Three of them fold into these four:
**share** is lend in shared memory or a handle in separate memory,
**proxy** is a handle, and **reconstruct** is a copy rebuilt in the
destination's representation (example 4's upload).

| Domain | Copy | Transfer | Lend | Handle |
| --- | --- | --- | --- | --- |
| Scoped thread | yes | not needed: an ordinary move | yes | yes |
| Worker | yes | yes | no | yes |
| Remote service | yes | yes | no | yes |
| Device | yes, often rebuilt | yes | no | yes |
| Later / resume | yes | no (L4) | no | yes |
| Background thread | yes | an ordinary move | no: not scoped | yes |

### The Examples In The Model

| # | Source to destination | Code | Environment | Result |
| --- | --- | --- | --- | --- |
| 1 | page to worker | `thumbnail`, by name | `photo`: copy or transfer | thumbnail copied back; a transferred photo handed back |
| 2 | browser to remote service | `Orders.place_order`, a `Remote` method the server serves | `cart`: copy | receipt copied back, or an unknown outcome |
| 3 | now to later | a closure calling `send_welcome`, by its identity in the program | `email`, `name`: copy; `conn`: rejected | done or failed, with no caller waiting |
| 4 | host to device | five kernels, methods of a `Kernel` trait | `photo`: copy rebuilt as a texture; the host keeps a handle | one image copied back |
| 5 | thread to scoped threads | closures, same build | `lines`: shared lend; halves: exclusive lends | counts returned; threads joined before the scope ends |
| 6 | browser to remote service | `SalesApi.top_sales`, a `Remote` method; the filter runs where the rows are | `f`: copy; the rows never move | 200 rows copied back |
| 7 | server to browser, then back | the action's closure stays on the server; the browser holds a handle, `Action[string, void, TodoError]`, whose home is the server | `todo.id`: copy, sealed in the handle; `title`: copy, untrusted | rename done or failed, or an unknown outcome |
| 8 | UI thread to a background thread, then back | a closure calling `detect_faces`; its empty row fits the background thread | `photo`: copied (a snapshot of the copy-on-write buffer, no deep copy), or moved if `Photo` is non-copyable | faces copied back; the handler resumes on the UI thread |

### Lifecycle Rules

- **L1.** A borrow ends when the destination's access ends (quiescence),
  not when the result arrives.
- **L2.** An unknown outcome never returns transferred ownership.
- **L3.** A handle keeps its data alive until the last handle is dropped.
- **L4.** The later/resume domain allows only copy and handle.

### Affinity

Some work belongs near some data. Two kinds behave
differently:

| Kind | Meaning | Example | Who decides |
| --- | --- | --- | --- |
| Required | The work can run only where the data is: a handle is valid only in its home domain | five kernels on the GPU that holds the texture (4); a query on the shard that holds the rows (6) | the checker, from the handles the code uses |
| Preferred | The work may run anywhere but is cheaper near data already there | a worker pool reusing the worker that holds the photo (1); a replica with a warm cache | the scheduler at run time, as a cost hint |

A sketch of how each fits the model, for discussion:

- **Required affinity follows from handles.** Every handle has a home
  domain. A crossing may target the domain a handle lives in,
  instead of a named domain, as Chapel's `on x do` runs where `x` lives.
  The checker then requires every handle the code uses to share that home,
  so a kernel cannot take a texture from another GPU.
- **Preferred affinity is a scheduling hint, not a type.** A domain pool
  (`Workers`) may pick the member that already holds a value. It changes
  cost, never meaning, so the checker need not see it. Prior art keeps it
  in the runtime: Spark and Dask preferred locations, Ray's locality-aware
  scheduling, Orleans placement, NUMA and Kubernetes affinity rules.

## 4. Outcomes

A domain that can lose contact may leave a crossing with no answer. In
example 2 the charge may or may not have happened. There is no new outcome
type: a crossing returns an ordinary `Result`, and losing contact is a
case of std's `CrossError`:

- **`Unreachable`:** the call never left, so it did not run.
- **`Unknown`:** the call left and no answer came, including a timeout
  after it was sent, so it may have run.

A worker or a device reports `Unknown` only when it crashes or resets. A
scoped thread cannot lose contact and never fails.

The error type that carries a `CrossError` is always one the code
declares, never a widened or nested one:

- A domain's `run!` returns `Result[T, CrossError]`.
- Code that returns its own `Result[T, E]` crosses with `try_run!`, which
  needs `E < From[CrossError]` and returns `Result[T, E]`: one flat error.
- A `Remote` method's error type holds a `CrossError` case (section 6,
  A3), and so does an `Action`'s.

```text
# hypothetical syntax; orders: the Orders provider, a Remote trait
@error
pub enum OrderError:
    @error("card declined")
    Declined
    @error("lost contact: $cause")
    Cross(cause: CrossError)

match orders.place_order!(cart):     # Result[Receipt, OrderError]
    .Ok(receipt) => show(receipt)
    .Err(.Cross(.Unknown)) => check_order_status!(cart.id)
    .Err(error) => show_error(error)

match rename.call!(text):            # rename: Action[string, void, TodoError]
    .Ok(()) => pass
    .Err(.Cross(.Unknown)) => println("saving; refresh to check")
    .Err(error) => println("could not rename: $error")
```

L1 and L2 hold by construction, so the checker needs no rule for
`Unknown` (section 5, Outcomes And Cancellation).

| Prior art | What the caller sees | How "unknown" is handled |
| --- | --- | --- |
| gRPC | `DEADLINE_EXCEEDED`, one status among others | Folded into the error; the docs warn the operation may have completed |
| Temporal | An activity times out and is retried; the workflow sees one completion | The runtime retries; activities should be idempotent |
| Azure Durable Functions | Orchestrators replay from a history | Activities may run more than once; orchestrator code must be deterministic |
| Erlang monitors | `{'DOWN', Ref, process, Pid, noconnection}` | A distinct reason: the process may still exist |
| Swift distributed actors | Every remote call is implicitly `async throws` | Transport failures are errors, forced on remote calls only |
| Stripe idempotency keys | A retry with the same key returns the first result | The client resolves unknown by retrying safely |

hd follows gRPC, Erlang and Temporal: unknown is a case the caller
matches, not a separate type. Temporal, Durable Functions and Stripe then
turn unknown into "retry until known", which needs idempotent work; a
library can build that on the `Unknown` case.

## 5. Type Checking

The checker sees a crossing as a call with three inputs: a destination,
code, and an environment. It checks them at four constructs: the
crossing, each capture, each use of a handle, and the code's body. Each
check names what it enforces from section 3, the hd rules it reuses, and
what is new. A table at the end sums up.

The destination is always a value in code: a provider from
the source's row (`Workers`, `Background`, `Gpu`, or a service trait that
extends `Remote`), a scope, or a handle, whose home a crossing may target.
A test swaps a provider with `$.with`. The sketches assume
these std traits:

```text
# hypothetical syntax throughout this section
# Workers, Background and Scope are sketched with their bounds in Two Bounds, below.
trait Gpu:
    type Device                      # one type per device, as Gpu0
    fn upload!(&mut self, image: Image) -> Result[Texture[Self::Device], CrossError]
    fn read_back!(&mut self, texture: Texture[Self::Device]) -> Result[Image, CrossError]

pub trait Filters[D] < Kernel:       # device code (section 6, A1); the host calls a generated stub
    fn blur!(&mut self, texture: Texture[D], radius: f32) -> Result[void, CrossError]
```

### The Row Against The Destination

- **Checked at the crossing:** every key in the code's row is a capability
  of the destination. The source's row need not hold them (example 2).
- **Enforces:** the capabilities property; the row is checked at the
  destination, not the source (A Crossing).
- **Reuses:** a domain trait carries its capabilities as a row parameter,
  as `Fn[I, O, $R]` does ([`fn.trait.decl`](../../spec/lang/07-functions.md#r-fn.trait.decl)). Code whose row lists a key
  the destination lacks does not fit, `type-mismatch`
  ([`req.row.subsume.missing`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.subsume.missing)); a narrower row fits
  ([`req.row.subsume.closure`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.subsume.closure)). A closure captures no provider from
  `$.with`, so its row lists every key its body uses
  ([`fn.capture.no-providers`](../../spec/lang/07-functions.md#r-fn.capture.no-providers)). Two devices are two keys,
  `Gpu[Device = Gpu0]` and `Gpu[Device = Gpu1]`
  ([`req.key.binding.identity`](../../spec/lang/11-requirements-and-suspension.md#r-req.key.binding.identity)). A service's registered bindable set
  stands for its capabilities ([`module.register.row`](../../spec/lang/10-modules.md#r-module.register.row)).
- **New:** each domain trait declares its capabilities. A `Remote`
  service's implementation is checked against the target that serves it,
  not against its callers' rows
  ([`req.row.set.call`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.set.call)); section 6.

```text
# hypothetical syntax
workers.run!(fn() -> void $ Db: save(doc))?                # error: type-mismatch, Db is not a worker capability
workers.run!(fn() -> void $ Log: log_size(doc))?           # valid: the worker offers Log
background.run!(fn() -> void $ Ui: draw_boxes(faces))?      # error: type-mismatch, only the UI thread offers Ui
faces := background.run!(fn() -> List[Rect]: detect_faces(photo))?  # valid: an empty row fits; the handler suspends until it returns
```

### Two Bounds: Send And Wire

What may cross is stated where it is enforced: as a bound on the domain's
method. Two std marker traits, checked structurally as `Copy` is
([`ref.copy.marker`](https://github.com/HerringtonDarkholme/hd-lang/blob/9af95bd2f/spec/lang/13-values-and-references.md#r-ref.copy.marker)), cover every kind:

- **`Send`:** the value may move to another thread of the same memory.
- **`Sync`:** a `&T` may be shared across threads; `&T` is `Send` exactly
  when `T < Sync`.
- **`Wire`:** the value can be encoded as a self-contained tree, today's
  boundary-safe values ([`module.boundary.allowed`](../../spec/lang/10-modules.md#r-module.boundary.allowed)) given a name. A
  `Wire` value is a tree, so it is also `Send`; the reverse does not hold.

The structural rules:

| Type | `Send` | `Sync` | `Wire` |
| --- | --- | --- | --- |
| a number, `bool`, `char`, `string` | yes | yes | yes |
| a struct, enum, tuple, `List`, `Map`, `Set`, optional, `Result` | when every part is | when every part is | when every part is |
| a copy-on-write `List` or `Map` | sending takes its snapshot on the sending thread; counts are atomic | yes: a write copies | yes |
| `&T` | when `T < Sync` | when `T < Sync` | no |
| `&mut T` | when `T < Send` | when `T < Sync` | no |
| `*mut T`, a box | no: a read through `&T` may write through it | no | no |
| a trait value, a provider value | no | no | no |
| a handle | yes, within its home | no | no: it crosses as a handle |
| a closure | when its captures are `Send` and it writes none of them | when its captures are `Sync` | when its captures are `Wire`, for same-build code only |

A closure that writes a captured variable shares that cell with its scope
([`fn.capture.cell.writes`](../../spec/lang/07-functions.md#r-fn.capture.cell.writes)), so it is not `Send`. Two spawns doing
`total += n` are rejected instead of racing; each returns its count.

Each domain's method states its bound:

```text
# hypothetical std declarations
trait Background[$Cap]:              # shared memory, outlives the call; $Cap: what it offers
    fn run![T < Send, F < 'static & Send & Fn() -> T $Cap](&mut self, code: F) -> Result[T, CrossError]

trait Workers[$Cap]:                 # separate memory, same build
    fn run![T < Wire, F < 'static & Wire & Fn() -> T $Cap](&mut self, code: F) -> Result[T, CrossError]
    fn try_run![T < Wire, E < Wire & From[CrossError], F < 'static & Wire & Fn() -> Result[T, E] $Cap](&mut self, code: F) -> Result[T, E]

struct Scope['s]:                    # shared memory, joins before 's ends, so it may lend
    count: usize

impl['s] Scope['s]:
    fn spawn[T < Send, F < 's & Send & FnMut() -> T](&mut self, code: F) -> Task['s, T]
```

| Domain | Code and captures | Arguments and results |
| --- | --- | --- |
| Scoped thread | `Send`, may borrow for `'s` | `Send` |
| Background thread | `Send`, `'static` | `Send` |
| Worker, job | `Wire`, `'static` | `Wire` |
| Remote service, device | no code: interface methods only (A2) | `Wire`, or a handle whose home is that domain |

So a reader tells the thread rules from the client and server rules by
the bound on the method, and a failure is an ordinary
`unsatisfied-trait-bound`:

```text
# hypothetical syntax; edits: *mut Edits
background.run!(fn() -> Image: shrink(photo, edits))?  # error: unsatisfied-trait-bound, *mut Edits does not implement Send
workers.run!(fn() -> Image: shrink(photo, edits))?     # error: unsatisfied-trait-bound, *mut Edits does not implement Wire
background.run!(fn() -> Image: shrink(photo, 256))?    # valid: Image is Send; the buffer is shared, not copied
workers.run!(fn() -> Image: shrink(photo, 256))?       # valid: Image is Wire; it is encoded and copied
```

Rust states the same split with `Send` on `thread::spawn` and `Serialize`
on serde's encoders.

### Each Capture's Mode

- **Checked at each capture, argument and result:** the value meets the
  bound the domain's method states (`Send` or `Wire`, above), then crosses
  in the mode its type decides. L4 is the job domain's `Wire` bound plus
  its refusal of moves.
- **Reuses:** capture analysis finds the captures ([`fn.capture.locals`](../../spec/lang/07-functions.md#r-fn.capture.locals)).
  The move checker's copy test sorts them: a `&mut` and a type holding one
  are not copied ([`ref.move.mut`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.mut), [`ref.move.holds`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.holds)).
- **New:** the table as a rule. The mode follows from the value's type and
  the domain kind. A copyable value copies, a non-copyable one transfers, a
  `&` or `&mut` lends, and a handle type crosses as a handle. So the
  checker infers every mode, and code never writes one.

The four modes add these checks.

**Copy**, at each copied capture, argument and result. In separate
memory, the value must be `Wire`. In shared memory, a `Send` value copies,
as a capture does today ([`fn.capture.copy.value`](../../spec/lang/07-functions.md#r-fn.capture.copy.value)).

- **Enforces:** the destination gets its own value; the source is
  unchanged.
- **Reuses:** `Wire` names the boundary-safe values ([`module.boundary.allowed`](../../spec/lang/10-modules.md#r-module.boundary.allowed)): closures,
  trait values, handles and references fail
  ([`module.boundary.not-safe`](../../spec/lang/10-modules.md#r-module.boundary.not-safe)). Values cross as trees
  ([`module.boundary.tree`](../../spec/lang/10-modules.md#r-module.boundary.tree)), and private fields need `Serialize`
  ([`module.boundary.out`](../../spec/lang/10-modules.md#r-module.boundary.out), [`module.boundary.error`](../../spec/lang/10-modules.md#r-module.boundary.error)).
- **New:** the boundary check runs on a crossing's captures, not only on a
  registered signature. Code for separate memory is bounded `Fn`, so a
  closure that writes a capture is `not-writable`
  ([`fn.trait.read-only`](../../spec/lang/07-functions.md#r-fn.trait.read-only)). Today a captured variable shares a cell with
  its scope ([`fn.capture.cell.writes`](../../spec/lang/07-functions.md#r-fn.capture.cell.writes)); across memories the write would
  change only the destination's copy.

```text
# hypothetical syntax
workers.run!(fn() -> Image: shrink(photo, edits))?  # error: unsatisfied-trait-bound, *mut Edits does not implement Wire
workers.run!(fn() -> void: done += 1)?              # error: not-writable, the write would change only the worker's copy
workers.run!(fn() -> Image: shrink(photo, 256))?    # valid: Image is Wire, so it is copied
```

**Transfer**, at the capture and at each later use of the source binding.
The value's type is not copyable, and the crossing ends the source binding
as a move.

- **Enforces:** the destination gets ownership; the source loses the value
  until it is handed back. Transfer is for non-copyable types only
 .
- **Reuses:** the moves of M1 and M2 ([`ref.move.mut`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.mut),
  [`ref.move.holds`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.holds)). A later use is `use-of-moved-value`
  ([`ref.move.use`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.use)); assigning the returned value makes the binding
  usable again ([`ref.move.reassign`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.reassign)).
- **New:** a crossing's capture is a move site, one more row of the
  [`ref.move.sites`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.sites) table. A copyable type never transfers; it
  copies. In separate memory the value must also be `Wire`. A type
  declared `impl !Copy`
  ([#1141](https://github.com/HerringtonDarkholme/hd-lang/issues/1141)) whose fields are `Wire`, such as
  example 1's photo buffer, transfers into a worker: the source binding
  ends, and the buffer is handed over instead of copied.

```text
# hypothetical syntax
scope(fn(s: &mut Scope) -> void:
    s.spawn(fn() -> void: lowercase_all(left))  # left, a SpanMut, moves into the thread
    lowercase_all(left)                          # error: use-of-moved-value
    lowercase_all(right)                         # valid: right never moved
)
```

**Lend**, at the capture and at the scope call that joins. A `&` or
`&mut` capture goes only to a scoped shared-memory domain, and the loan
stays live until that domain's access ends (L1).

- **Enforces:** the source is frozen or excluded until the borrow ends,
  and the borrow ends at quiescence.
- **Reuses:** a closure given directly for a `fn` parameter may capture
  local references ([`fn.capture.local-parameter`](../../spec/lang/07-functions.md#r-fn.capture.local-parameter)); storing that
  parameter is an error ([`fn.capture.local-store`](../../spec/lang/07-functions.md#r-fn.capture.local-store)). The scope keeps its
  spawned closures under a lifetime bound, `F < FnMut() -> T & 's`
  ([`ref.generic.outlives`](../../spec/lang/13-values-and-references.md#r-ref.generic.outlives)). The loan lasts for the scope call
  ([`ref.excl.call`](../../spec/lang/13-values-and-references.md#r-ref.excl.call), [`ref.excl.live`](../../spec/lang/13-values-and-references.md#r-ref.excl.live)). While `&p` is lent the source
  does not write `p` ([`ref.excl.shared`](../../spec/lang/13-values-and-references.md#r-ref.excl.shared)); while `&mut p` is lent it does
  not use `p` ([`ref.excl.mut`](../../spec/lang/13-values-and-references.md#r-ref.excl.mut)). Disjoint fields lend separately
  ([`ref.excl.place`](../../spec/lang/13-values-and-references.md#r-ref.excl.place); through a call, [`ref.result.path.caller`](../../spec/lang/13-values-and-references.md#r-ref.result.path.caller)).
  Separate-memory code is bounded `'static`, so a local-reference capture
  there is `local-reference-escapes` ([`ref.generic.outlives.box`](../../spec/lang/13-values-and-references.md#r-ref.generic.outlives.box),
  [`ref.local.escape.capture-stored`](../../spec/lang/13-values-and-references.md#r-ref.local.escape.capture-stored)). A suspension that holds a local
  reference never reaches a host call ([`ref.dep.suspension.store`](../../spec/lang/13-values-and-references.md#r-ref.dep.suspension.store)). So
  a lending scope is an ordinary call that returns after the join, and
  nothing abandons it midway: L1 needs no new rule.
- **New:** the scope itself, which needs threads. The run-time loans that
  guard counted references are per instance
  ([`ref.excl.alias.conflict`](../../spec/lang/13-values-and-references.md#r-ref.excl.alias.conflict)) and would have to see other threads. A
  shared lend is not deep: a read through `&T` may write through a `*mut`
  field ([`ref.read.shallow`](../../spec/lang/13-values-and-references.md#r-ref.read.shallow)), and a captured variable is a shared cell
  ([`fn.capture.cell`](../../spec/lang/07-functions.md#r-fn.capture.cell)). Both race across threads; that is the missing
  `Sync`.

```text
# hypothetical syntax; lines: &mut List[string]
workers.run!(fn() -> usize: count_words(lines))?  # error: local-reference-escapes, a worker has separate memory
counts := scope(fn(s: &mut Scope) -> List[Map[string, usize]]:
    tasks := [for i in 0..4 => s.spawn(fn() -> Map[string, usize]: count_quarter(lines, i))]
    lines.push("")                  # error: conflicting-access, lines is lent until the scope joins
    [for t in tasks => t.join()]
)
lines.push("")                      # valid: the scope has joined
```

**Handle**, at each call that takes the handle. A handle's type names its
home domain, so using it in another domain is a type error, with no
run-time check.

- **Enforces:** the handle row of the mode table, and L3.
- **Reuses:** the device trait's associated type, bound in its key:
  `$.use(Gpu[Device = Gpu0])` has type `&dyn Gpu[Device = Gpu0]`
  ([`req.key.binding.value.reference-values`](../../spec/lang/11-requirements-and-suspension.md#r-req.key.binding.value.reference-values)). Its `upload!` returns a
  `Texture[Gpu0]`, and the kernels for Gpu0, `Filters[Gpu0]`, take only
  that, so a `Texture[Gpu0]` given to Gpu1's kernels is an ordinary
  `type-mismatch`. Handles are not
  `Wire` ([`module.boundary.not-safe`](../../spec/lang/10-modules.md#r-module.boundary.not-safe)), so a copy never takes one.
  Within a domain a handle keeps its data alive as a counted reference
  does ([`ref.box.lifetime`](../../spec/lang/13-values-and-references.md#r-ref.box.lifetime)). `using` or an `@auto_closing` constructor
  releases it at scope exit ([`flow.auto-closing.statement`](../../spec/lang/06-control-flow.md#r-flow.auto-closing.statement)).
- **New:** a std marker trait for handle
  types that the compiler knows, as `AutoClosing` is
  one ([`flow.auto-closing.marker`](../../spec/lang/06-control-flow.md#r-flow.auto-closing.marker)). The checker needs it to pick the
  handle mode and to find handles for affinity. Counting a handle across
  domains (L3) is run-time work. Domains are static: one picked at run
  time would need a fresh type.

```text
# hypothetical syntax
let tex = $.use(Gpu[Device = Gpu0]).upload!(photo)?  # tex: Texture[Gpu0]
$.use(Filters[Gpu1]).blur!(tex, 2.0)?                # error: type-mismatch, a Texture[Gpu0] for Gpu1's kernels
$.use(Filters[Gpu0]).blur!(tex, 2.0)?                # valid
```

### Handle Agreement And The Affinity Target

- **Checked at the crossing's target and each handle the code uses:** a
  crossing aimed at a handle runs in the domain the handle's type names,
  and every handle the code captures or takes names that same domain.
- **Enforces:** required affinity, as Chapel's `on x do`.
  Preferred affinity is a run-time hint to a pool such as `Workers`; the
  checker never sees it.
- **Reuses:** the home is a type argument, so "the same domain" is type
  equality. The target's capabilities come from its key, checked as in the
  row check; a row without that key is `missing-requirement`
  ([`req.use.missing`](../../spec/lang/11-requirements-and-suspension.md#r-req.use.missing)).
- **New:** a target read from a type: `at(rows)` with `rows: Table[D]`
  resolves to the provider of the domain `D` where the rows live. Then a
  walk compares every handle the code uses with `D`. This is example 6's
  "code goes to the data", for same-build code next to a handle.

```text
# hypothetical syntax
# rows: Table[Shard3], other: Table[Shard1], f: Filter
at(rows).run!(fn() -> List[Sale]: join_rows(rows, other, f))?  # error: other is a Table[Shard1], the target is Shard3
at(rows).run!(fn() -> List[Sale]: filter_rows(rows, f))?       # valid: runs on Shard3, where rows live
```

### The Later Domain (L4)

- **Checked at each capture of a job:** a job for later takes only copies
  and handles.
- **Enforces:** L4. The job's code is named within the program, so the
  build that stored it runs it (section 1).
- **Reuses:** the job domain's `Wire` bound rejects a reference, and the
  `'static` bound rejects a lend
  ([`ref.generic.outlives.box`](../../spec/lang/13-values-and-references.md#r-ref.generic.outlives.box)). The table's L4 row rejects a
  non-copyable value. The job's row is checked against the resuming host's
  capabilities, as in the row check.
- **New:** no check beyond the table row. A name means the same function
  everywhere within one compiled program
  ([`req.determinism.same-program`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.same-program)), which is all the principle
  needs; content hashes stay deferred
  ([`fn.capture.serializable-deferred`](../../spec/lang/07-functions.md#r-fn.capture.serializable-deferred)).

```text
# hypothetical syntax
jobs.after!(minutes(10), fn() -> Result[void, DbError] $ Db: mark_sent(conn, user.id))?  # error: unsatisfied-trait-bound, &mut DbConn does not implement Wire
jobs.after!(minutes(10), fn() -> Result[void, MailError] $ Mailer: send_welcome(user.email, user.name))?  # valid: two strings are copied
```

### The Body Against The Destination's Environment

- **Checked at the body of the crossing's code:** it runs with what the
  destination gives it: its parameters, its captures as they crossed, and
  the destination's providers for its row. It sees no provider and no
  memory of the source.
- **Enforces:** the capabilities and memory properties.
- **Reuses:** a body's available keys are its row plus lexical keys
  ([`req.row.entail.available`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.entail.available)). A closure's keys resolve at each call
  ([`req.with.dynamic`](../../spec/lang/11-requirements-and-suspension.md#r-req.with.dynamic)), so here they resolve at the destination. A
  provider value captured from `$.use` would fix the source's provider
  ([`req.with.lexical`](../../spec/lang/11-requirements-and-suspension.md#r-req.with.lexical), [`fn.capture.provider-value`](../../spec/lang/07-functions.md#r-fn.capture.provider-value)). It is a counted
  value, neither `Send` nor `Wire`, so the domain's bound rejects it.
- **New:** nothing for the row. A device body must be device code, a
  restricted subset with no allocation and no host calls. That subset is
  a new check, not specified here.

```text
# hypothetical syntax
log := $.use(Log)
workers.run!(fn() -> void: log.info("done"))?               # error: unsatisfied-trait-bound, the source's Log provider is not Wire
workers.run!(fn() -> void $ Log: $.use(Log).info("done"))?  # valid: the worker's own Log
```

### Top-Level Bindings

- **Checked at the crossing's code and every function it reaches:** the
  code reads no `let mut` top-level binding, directly or through a callee.
  It may read immutable top-level bindings.
- **Enforces:** the memory property. Module storage belongs to each
  program instance ([`module.init.program-instance`](../../spec/lang/10-modules.md#r-module.init.program-instance)), initialized once
  per instance ([`module.init.cycle.once`](../../spec/lang/10-modules.md#r-module.init.cycle.once)). In separate memory a
  mutable top-level read or write would touch the destination's own copy
  and leave the source's unchanged, the same silent divergence the `Fn`
  bound rules out for captures. In shared memory it would be a data race.
- **Reuses:** top-level initialization is requirement-free
  ([`module.init.entry.statements-before-main`](../../spec/lang/10-modules.md#r-module.init.entry.statements-before-main)), so no top-level
  binding holds a provider, a connection or a handle, and an immutable one
  has the same value in every domain. Imports and top-level functions are
  code, which every domain already has (section 1). A capability a callee
  needs is in its declared row, so the row check sees it transitively.
- **New:** a per-function summary, "reaches mutable top-level state",
  computed over the call graph as a callee's row is read. A crossing whose
  code has it is an error. Swift treats global mutable state the same
  way: it is not `Sendable`.

```text
# hypothetical syntax
LIMIT := +100
let mut hits: i64 = 0
fn top(rows: List[Row]) -> List[Row]: rows.take(usize(LIMIT))
fn record() -> void: hits += 1

workers.run!(fn() -> List[Row]: top(rows))?  # valid: LIMIT is immutable, the same in every domain
workers.run!(fn() -> void: record())?        # error: record writes the mutable top-level hits
```

### Outcomes And Cancellation

- **Checked:** nothing specific to unknown. A crossing returns
  `Result[T, E]`, and unknown is one case of `E`, handled with `match`
  (section 4). The checker treats `.Unknown` as any other error case, so
  section 4's `match` is the only sketch.
- **Why L1 needs no rule:** a lend goes only to a scoped shared-memory
  domain, which always joins and never reports unknown.
- **Why L2 needs no rule:** a transfer is a move, so the source binding is
  gone already ([`ref.move.use`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.use)). A transferred value comes back only
  inside `Ok`. A handle creation with an unknown outcome returns no
  handle.
- **Cancellation reuses:** abandoning a crossing aborts the host operation
  as that operation's rule says ([`req.abandon.host`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.host)) and unwinds once,
  synchronously ([`req.abandon.unwind`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.unwind)). A moved value stays moved, and
  abandoning closes no handle.

### What Is New

| Check | At | Reused | New |
| --- | --- | --- | --- |
| Row against the destination | crossing | row subsumption, bound keys, no captured providers, bindable sets | capabilities on domain traits; interface methods have no row (A5) |
| Bound per domain | each crossing | trait bounds, the structural `Copy` check | `Send`, `Sync` and `Wire` markers, stated on each domain's method |
| Mode per capture | capture | capture analysis; the copy test of M1 and M2 | modes inferred from types |
| Copy | capture, result | boundary-safe values, `Fn` bounds | `Wire` at captures; `Fn` for separate memory |
| Transfer | capture, later uses | M1 and M2 moves | a capture as a move site; no separate-memory case before `!Copy` types |
| Lend | capture, scope call | closure-parameter lifetimes, lifetime bounds, exclusivity, the suspension store rule | the scope; run-time loans across threads |
| Handle | handle use | bound keys, type equality | a handle marker; counts across domains |
| Affinity target | crossing | type equality, `missing-requirement` | a target read from a type; the handle walk |
| Later (L4) | capture, code | the `Wire` bound, the `'static` bound | no move into a job |
| Body | body | row resolution at each call | the device-code subset |
| Top-level bindings | crossing, reached functions | per-instance module storage, requirement-free init, declared rows | the mutable-state summary |
| Outcomes | none | `Result`, `match`, moves, `abandon` | none |

The gaps:

- **`Send`, `Sync` and `Wire` are bounds; threads are not built.** The
  bounds are fixed above. Atomic counts, so that sending a copy-on-write
  `List` is safe, come with the threads themselves.
- **No threads and no shared heap.** One instance runs on one thread
  ([`module.instance.thread`](../../spec/lang/10-modules.md#r-module.instance.thread)), and instances' heaps are disjoint
  ([`module.instance.disjoint`](../../spec/lang/10-modules.md#r-module.instance.disjoint)). Lend waits for both.
- **No device-code subset yet.** A `Kernel` implementation's body must be device code (no allocation, no host calls); that subset is not specified here.
- **The outcome type is not a gap.** Unknown is an error case (section 4).

## 6. The Process, End To End

Sections 3 to 5 define a crossing and check it. This section follows one
program through every stage: how its author writes code that runs
elsewhere, how a reader sees it, what the checker accepts, how the
compiler builds each target, and how the runtime runs it.

One split decides most of the answers: whether the destination runs the
same build as the caller.

| | Same build | Another build |
| --- | --- | --- |
| Domains | scoped thread, background thread, worker, later/job | remote service, device |
| Code that may cross | any function or closure | only the methods of a declared interface |
| Compiled | once | twice: the implementation for the destination, a stub for each caller |

### Writing It

**Same build: nothing to declare.** The author hands a function or a
closure to the domain's method, and its captures cross by their types:

```text
# hypothetical syntax; photo: Photo, background: the $ Background provider
let mut background = $.use(Background)
faces := background.run!(fn() -> List[Rect]: detect_faces(photo))?
```

**Another build: declare an interface, implement it there, install it.**
Code that another build runs lives in a trait that extends a target
marker: `Remote` for a service, `Kernel` for a device. Its methods are the
endpoints, the only code the other side can reach.

```text
# hypothetical syntax
# src/users.hd: the interface, shared by both sides
pub struct User:
    pub id: i64
    pub name: string

@error
pub enum UserError:
    @error("no user $id")
    Missing(id: i64)
    @error("lost contact: $cause")
    Cross(cause: CrossError)

pub trait Users < Remote:
    fn load_user!(&mut self, id: i64) -> Result[User, UserError]

# src/server.hd: the real provider; the only module that uses the driver
use dep.postgres.{Pool, PgError, connect}

struct PgUsers:
    pool: Pool

impl Users for PgUsers:
    fn load_user!(&mut self, id: i64) -> Result[User, UserError]:
        match self.pool.query_one!("select id, name from users where id = ?", id):
            .Some(row) => .Ok(User { id: row.i64(0), name: row.text(1) })
            .None => .Err(UserError.Missing(id))

pub fn server_main!() -> Result[void, PgError] $ Console + Net:
    pool := connect!("postgres://localhost/app")?
    serve!(PgUsers { pool })                 # exposes Users's methods

# src/browser.hd: the stub provider, then ordinary provider calls
pub fn browser_main!() -> void $ Dom + Net:
    $.with(Users=remote::[Users]("https://api.example.com")):
        on_open!(7)

pub fn on_open!(id: i64) -> void $ Dom + Users:
    let mut users = $.use(Users)
    match users.load_user!(id):
        .Ok(user) => show("Hi, ${user.name}")
        .Err(error) => show_error("$error")

tests:
    it("shows the user"):
        $.with(Users=FakeUsers {}, Dom=FakeDom {}):
            on_open!(7)
```

The author's rules:

- **A1.** Code for another build is a trait that extends a target marker,
  `Remote` or `Kernel`.
- **A2.** Its methods take and return `Wire` types, or handles whose home
  is that domain (an `Action` for a service, a `Texture[D]` for a device).
  They are not generic and take no closure, reference or trait value.
- **A3.** A `Remote` method's error type can hold a `CrossError`, as
  `UserError.Cross` does. No result type is widened implicitly.
- **A4.** The destination installs the real provider with `serve!`. Each
  calling target installs a generated stub: `remote::[Users](address)` for
  a service, `kernels::[Filters[Gpu0]](gpu)` for a device. A test installs
  a fake. All are ordinary providers.
- **A5.** An interface method has no row of its own. Its implementation
  gets what it needs from its provider's fields, such as `PgUsers.pool`,
  so the server's `main` decides what the service can reach.

### Reading It

A reader finds every crossing from three signals, all in today's syntax:

| Where | What it shows | Example |
| --- | --- | --- |
| The function's row | which domains the function talks to | `fn on_open!(id: i64) -> void $ Dom + Users` |
| The call | a bang method on a destination value | `users.load_user!(id)`, `background.run!(...)` |
| The types | the trait extends `Remote`; the error has a `CrossError` case | `.Err(UserError.Cross(cause))` |

A line crosses only when it is a bang method on a provider from the row,
on a scope, or on a handle. A bare function call never crosses. The one
thing the call site does not show is which provider is local:
`users.load_user!(id)` and `db.find_user!(id)` look alike, and the trait
tells them apart, as a gRPC stub and a local object do.

### Checking It

The checker looks at three places.

- **C1, the interface:** A2 and A3 at the trait's declaration. A `Kernel`
  implementation's body must be device code (section 5).
- **C2, each crossing call:** the code's row fits the destination's
  capabilities, the domain trait's row parameter (section 5, The Row
  Against The Destination). Each capture and argument meets the bound the
  domain's method states, `Send` or `Wire` (section 5, Two Bounds), and
  crosses in the mode its type decides (Each Capture's Mode). Code for separate
  memory is bounded `Fn`, so a write to a capture is `not-writable`. Code
  that reaches `let mut` top-level state does not cross (Top-Level
  Bindings).
- **C3, the code's body:** for same-build code, its `$.use` calls resolve
  at the destination, never to the caller's provider (The Body Against The
  Destination's Environment). An interface implementation has no row
  (A5), so C3 does not apply to it.

What cannot cross follows from the bounds (section 5, Two Bounds) plus
three rules that bounds do not cover. Each is a compile error at the
crossing that names the value and the rule:

| Rule | Where | Fails |
| --- | --- | --- |
| The domain method's bound | every crossing | a box or trait value anywhere; a reference outside a scope; a non-`Wire` value into separate memory |
| A handle goes only to its home | every crossing | `Texture[Gpu0]` given to `Gpu1` |
| No move into the job domain (L4) | jobs | a non-copyable value |
| No `let mut` top-level state | every crossing | code that reaches one |
| The row fits the destination | every crossing | code whose row the destination lacks |

### Building It

The deployment lists targets. Each is an entry module, a
runtime profile (what its host binds), and an output: Wasm for a browser
or a server, device code for a GPU.

- **B1.** Each target walks the use graph and the call graph from its
  entry and compiles only what it reaches. A module only the server
  reaches, such as the one that uses `dep.postgres`, is never parsed for
  the browser.
- **B2.** A `serve!(impl)` in a target compiles that implementation and a
  generated dispatcher: decode a method number and its arguments,
  validate them as untrusted input, call, encode the `Result`.
- **B3.** A `remote::[T](address)` or `kernels::[T](device)` in a target
  compiles a generated stub type: encode, send or launch, decode, and map
  a transport or device failure to `CrossError`. The stub holds no
  implementation code. A `Kernel` implementation compiles into the device
  target's output.
- **B4.** A same-build crossing compiles into the one Wasm module. Its
  code is named by its index in the module's function table, plus a
  captured environment.
- **B5.** Each boundary type gets a generated codec, the tree encoding of
  boundary values ([`module.boundary.tree`](../../spec/lang/10-modules.md#r-module.boundary.tree)). Both sides come from
  one compile, so method numbers and codecs agree. A build ID stamped on
  both lets the runtime refuse a peer from another build with a
  `CrossError`, which keeps different builds out of scope (section 1).

| | browser build | server build |
| --- | --- | --- |
| `users.hd` | the trait, no code | the trait and the dispatcher |
| `server.hd`, `dep.postgres` | not reached | yes |
| `remote::[Users]` stub | yes | only if the server calls another service |
| `on_open` | yes | not reached |

### Running It

| Domain | How a crossing runs | Failure and cancellation |
| --- | --- | --- |
| Scoped thread | a thread on the same instance's memory runs the closure; the scope joins before it returns | none: contact cannot be lost |
| Background thread | a task (code, environment) goes to a thread of the same instance; the caller suspends, and the reply wakes it on its own event loop, so a UI handler resumes on the UI thread | a crash is a `CrossError` |
| Worker | a second instance of the same module receives the encoded code, environment and arguments | a crash is `CrossError.Unknown`; cancelling posts an abort |
| Job | the jobs provider stores the code and environment; a host of the same build runs it later; no caller waits | its retry policy decides; L2 holds |
| Remote service | the stub sends the method number and arguments over HTTP or a WebSocket; the dispatcher validates them and calls the implementation inside the server's providers | a timeout or disconnect is a `CrossError`, `Unknown` when the call may have run; cancelling sends a cancel that the server's call sees at its next suspension |
| Device | upload, launch the kernel by number with its handles, read back | a reset is a `CrossError` |

The runtime needs four pieces that do not exist yet, each its own slice:
Wasm threads with shared memory and atomic counts for the two thread kinds
(the `Sync` design); spawning instances for workers; a
transport for services; and a jobs store.

**Prior art.** gRPC and tRPC declare a service and give callers a stub,
as `Remote` traits do. Swift's distributed actors mark the declaration
(`distributed func`) and the call (`try await`); here the row and the
bang call carry the call side. Rayon, Kotlin's `withContext` and Swift's
`Task.detached` take a closure for same-build work, as `run!` does.

## 7. Syntax

No keyword is new. A crossing is a bang method call on a value that stands
for the destination, so one shape covers every kind:

```text
faces := background.run!(fn() -> List[Rect]: detect_faces(photo))?  # hypothetical syntax
user := users.load_user!(id)                                         # users: the Users provider, a Remote trait
```

Neither line writes a mode: each value's type decides it.
Two spellings were considered and dropped. A keyword form, `on D: call`,
adds a keyword. Marking functions `@remote(D)` and requiring `$ D` in a
caller's row gave cross-target calls a second shape beside
`workers.run!(...)`.

## Appendix A. Prior Art

Each snippet is abridged from its source.

**Rust scoped threads.** Threads borrow from the caller; the scope joins
them before it returns. hd takes this as the lend mode and L1.

```rust
let (left, right) = words.split_at(words.len() / 2);
let total = std::thread::scope(|s| {
    let a = s.spawn(|| count(left));
    count(right) + a.join().unwrap()
});
```

**Swift `sending` (SE-0430).** A value whose region nothing else reaches
moves to another isolation domain, and a later use is an error. hd takes
this as transfer, checked as an M1 move.

```swift
func process(_ image: sending Image) async { /* ... */ }
let img = Image()
await process(img)
// a use of img here is an error: it was sent
```

**Chapel `on`.** An `on` clause moves the running task, and `on x` runs
where `x` lives. hd takes "code moves to the data" (example 6).

```chapel
on Locales[1] do writeln(here.id);
on A[i] do A[i] += 1;
```

**ML5.** Types name the world a value lives at, and `get` evaluates an
expression at another world. hd takes the domain-tagged handle.

```sml
from server get (lookup key)
```

**ScalaLoci.** Peers and their ties are types, a placed value is read
remotely with `asLocal`, and transmitters may change the received type.
hd takes reconstruct and handle; its coherence rules make ScalaLoci's
endpoint check unnecessary.

```scala
@peer type Server <: { type Tie <: Multiple[Client] }
val items: Items on Server = /* ... */
on[Client] { Signal { items.asLocal() map createUIEntry } }
```

**Spores.** A closure whose captures are listed and checked, so it can be
sent. hd takes checked captures, one mode each.

```scala
val s = spore {
  val h = helper
  (x: Int) => h.inc(x)
}
```

**Unison.** Code is identified by content hash, and `Value.map` moves a
function to a remote value. hd does not need it: every domain runs the same program (section 1).

```unison
Seq (Value.map go (Seq.unwrap s))
```

## Appendix B. Earlier Options And #166

The brainstorm compared four options. The model keeps a part of each.

| Option | Kept | Dropped |
| --- | --- | --- |
| A: named code, copied data | Named functions as code identity; the copy mode | Copy as the only mode |
| B: portable closures placed by rows (#166) | Content hash across builds; rows checked at the destination | Placement by configuration alone |
| C: typed crossing contracts (owner's note) | A mode per capture; a destination representation; handles tied to an instance | A user `Across` trait per type as the primary check |
| D: `Send`, `Sync`, scoped threads | The scoped-thread domain and lend | Markers now: they wait for threads |

The parked #166 study
([SERIALIZABLE_CLOSURE_DESIGN.md @ 28072b8a1](https://github.com/HerringtonDarkholme/hd-lang/blob/28072b8a1/docs/future-work/SERIALIZABLE_CLOSURE_DESIGN.md))
is the copy-mode slice of this model.

| #166 | This model |
| --- | --- |
| `@remote` functions and portable closure literals | Code identity: a name or a content hash |
| Captures must be boundary-safe | The copy mode's check |
| Placement by requirement rows and `[places]` | The row is checked against a domain's capabilities |
| `Opaque[T]` for server state | A handle |
| `RemoteError` with `OutcomeUnknown` | The `Unknown` case of the crossing error (section 4) |
| Workers, servers, services; GPUs deferred | All five domain kinds; lend and transfer added |

## Sources

- The owner's note, "Types and Transport Across Execution Boundaries" (gist, 2026-10-31), and the owner's review of PR #1139.
- #166 study: <https://github.com/HerringtonDarkholme/hd-lang/blob/28072b8a1/docs/future-work/SERIALIZABLE_CLOSURE_DESIGN.md>; [Parallelism And Locality](PARALLELISM_AND_LOCALITY.md).
- gRPC status codes, `DEADLINE_EXCEEDED`: <https://grpc.github.io/grpc/core/md_doc_statuscodes.html>
- Temporal, activity timeouts: <https://docs.temporal.io/encyclopedia/detecting-activity-failures>; idempotency: <https://docs.temporal.io/activity-definition>
- Azure Durable Functions, orchestrator code constraints: <https://learn.microsoft.com/en-us/azure/azure-functions/durable/durable-functions-code-constraints>
- Erlang, `monitor/2` and `noconnection`: <https://www.erlang.org/doc/apps/erts/erlang.html#monitor/2>
- Swift SE-0336, distributed actor isolation: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0336-distributed-actor-isolation.md>
- Stripe, idempotent requests: <https://docs.stripe.com/api/idempotent_requests>
- Rust, `thread::scope`: <https://doc.rust-lang.org/std/thread/fn.scope.html>
- Swift SE-0430, `sending`: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0430-transferring-parameters-and-results.md>
- Chapel, on clauses: <https://chapel-lang.org/docs/users-guide/locality/onClauses.html>
- Murphy, Crary, Harper, "Type-Safe Distributed Programming with ML5", TGC 2007: <https://kilthub.cmu.edu/articles/journal_contribution/Type-Safe_Distributed_Programming_with_ML5/6611192/1>
- ScalaLoci: <https://scala-loci.github.io/>; Weisenburger, Salvaneschi, Programming 2020: <https://scala-loci.github.io/publications/2020-programming/paper.pdf>
- Miller, Haller, Odersky, "Spores", ECOOP 2014: <https://doi.org/10.1007/978-3-662-44202-9_13>
- Unison, distributed datasets: <https://www.unison-lang.org/articles/distributed-datasets/>

## Parse Log

All 24 `text` blocks were parsed on 2026-11-02 with
`scripts/hd-conformance.sh parse`, which checks syntax only; nothing was
type-checked. Std names (`Workers`, `Background`, `Jobs`, `Gpu`, `Remote`,
`Kernel`, `serve!`, `remote::[T]`) are sketches.

| Block | Where | Result |
| --- | --- | --- |
| 1 | 2. Eight Programs That Cross | parses |
| 2 | 2. Eight Programs That Cross | parses |
| 3 | 2. Eight Programs That Cross | parses |
| 4 | 2. Eight Programs That Cross | parses |
| 5 | 2. Eight Programs That Cross | parses |
| 6 | 2. Eight Programs That Cross | parses |
| 7 | 2. Eight Programs That Cross | parses |
| 8 | 2. Eight Programs That Cross | parses |
| 9 | 4. Outcomes | parses; `# hypothetical syntax` |
| 10 | 5. Type Checking | parses; `# hypothetical syntax` |
| 11 | The Row Against The Destination | parses; `# hypothetical syntax` |
| 12 | Two Bounds: Send And Wire | parses |
| 13 | Two Bounds: Send And Wire | parses; `# hypothetical syntax` |
| 14 | Each Capture's Mode | parses; `# hypothetical syntax` |
| 15 | Each Capture's Mode | parses; `# hypothetical syntax` |
| 16 | Each Capture's Mode | parses; `# hypothetical syntax` |
| 17 | Each Capture's Mode | parses; `# hypothetical syntax` |
| 18 | Handle Agreement And The Affinity Target | parses; `# hypothetical syntax` |
| 19 | The Later Domain (L4) | parses; `# hypothetical syntax` |
| 20 | The Body Against The Destination's Environment | parses; `# hypothetical syntax` |
| 21 | Top-Level Bindings | parses; `# hypothetical syntax` |
| 22 | Writing It | parses; `# hypothetical syntax` |
| 23 | Writing It | parses; `# hypothetical syntax` |
| 24 | 7. Syntax | parses; `# hypothetical syntax` |
