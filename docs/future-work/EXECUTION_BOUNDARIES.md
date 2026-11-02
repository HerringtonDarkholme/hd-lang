# Execution Boundaries: Moving Code And Data

Status: proposal, 2026-10-31; domain: language (execution domains and
crossings); not accepted. Nothing here is in the specification. It revises
the [PR #1139](https://github.com/HerringtonDarkholme/hd-lang/pull/1139)
brainstorm after the owner's review: examples first, then the model, then
type checking, then syntax. The parts the owner approved are marked
**approved**; everything else is a proposal.

- [1. Context And Goal](#1-context-and-goal)
- [2. Eight Programs That Cross](#2-eight-programs-that-cross)
- [3. The Model](#3-the-model)
- [4. Outcomes](#4-outcomes-approved-question-1)
- [5. Type Checking](#5-type-checking)
- [6. Code Generation Per Domain](#6-code-generation-per-domain)
- [7. Syntax](#7-syntax)
- [8. Questions For The Owner](#8-questions-for-the-owner)
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

**Principle (approved): every execution domain runs the same code.** One
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
    let (back, small) = $.use(Workers).call!(thumbnail, photo, 256)?
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

An execution domain has four properties (**approved**):

| Property | Question it answers |
| --- | --- |
| Memory | Is memory shared with the source, or separate? |
| Capabilities | Which providers exist there? |
| Code | Which code runs there? Always the same program (section 1) |
| Lifetime | Can it outlive the source? Can contact be lost? |

Five kinds of domain (**approved**), and a sixth (proposed, question 16):

| Kind | Memory | Capabilities | Code | Lifetime | Example |
| --- | --- | --- | --- | --- | --- |
| Scoped thread | shared | the source's | same build | ends before its scope | 5 |
| Worker | separate | its own host's | same build | may crash | 1 |
| Remote service | separate | the service's | same build | outlives; contact may be lost | 2, 6 |
| Device | separate | restricted operations, kernels only | device code | may reset | 4 |
| Later / resume | separate | the resuming host's | same build | outlives the source | 3 |
| Background thread | shared | its own: not the UI thread's | same build | outlives the call that started it; ends with the process | 8 |

### A Crossing

A **crossing** sends code and an environment to a destination, and returns
a result.

- **Code** is already at the destination, because every domain runs the
  same program (section 1). The crossing names it: a function, or a
  closure's lifted function, by its identity in that program. No code or
  executable crosses.
- **Environment**: each captured value crosses in one mode, below.
- **Requirement row**: the code's row is checked against the
  destination's capabilities, not the source's. That is example 2.
- **Result**: crosses back by the same modes.

### Crossing Modes

Four modes for environment values (**approved**):

| Mode | The destination gets | The source afterwards | Where |
| --- | --- | --- | --- |
| Copy | its own value | unchanged | all domains |
| Transfer | ownership | loses the value until it is handed back | separate memory |
| Lend | a borrow | frozen or excluded until the borrow ends | shared memory, scoped only |
| Handle | a handle; the data stays where it lives | a handle tied to that one domain instance | all domains |

The owner's note named five modes. Three of them fold into these four:
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
| 2 | browser to remote service | `place_order`, by name; its row is met only there | `cart`: copy | receipt copied back, or an unknown outcome |
| 3 | now to later | `send_welcome`, by its identity in the program | `email`, `name`: copy; `conn`: rejected | done or failed, with no caller waiting |
| 4 | host to device | five kernels | `photo`: copy rebuilt as a texture; the host keeps a handle | one image copied back |
| 5 | thread to scoped threads | closures, same build | `lines`: shared lend; halves: exclusive lends | counts returned; threads joined before the scope ends |
| 6 | browser to remote service | `matches`, by name | `f`: copy; the rows never move | 200 rows copied back |
| 7 | server to browser, then back | the action's closure stays on the server; the browser holds a handle, `Action[Server, string, T]` | `todo.id`: copy, sealed in the handle; `title`: copy, untrusted | rename done or failed, or an unknown outcome |
| 8 | UI thread to a background thread, then back | `detect_faces`, by name; its empty row fits the background thread | `photo`: copied (a snapshot of the copy-on-write buffer, no deep copy), or moved if `Photo` is non-copyable | faces copied back; the handler resumes on the UI thread |

### Lifecycle Rules (**approved**)

- **L1.** A borrow ends when the destination's access ends (quiescence),
  not when the result arrives.
- **L2.** An unknown outcome never returns transferred ownership.
- **L3.** A handle keeps its data alive until the last handle is dropped.
- **L4.** The later/resume domain allows only copy and handle.

### Affinity (**approved**: question 6)

Some work belongs near some data (owner, 2026-10-31). Two kinds behave
differently:

| Kind | Meaning | Example | Who decides |
| --- | --- | --- | --- |
| Required | The work can run only where the data is: a handle is valid only in its home domain | five kernels on the GPU that holds the texture (4); a query on the shard that holds the rows (6) | the checker, from the handles the code uses |
| Preferred | The work may run anywhere but is cheaper near data already there | a worker pool reusing the worker that holds the photo (1); a replica with a warm cache | the scheduler at run time, as a cost hint |

A sketch of how each fits the model, for discussion:

- **Required affinity follows from handles.** Every handle has a home
  domain (question 4). A crossing may target the domain a handle lives in,
  instead of a named domain, as Chapel's `on x do` runs where `x` lives.
  The checker then requires every handle the code uses to share that home,
  so a kernel cannot take a texture from another GPU.
- **Preferred affinity is a scheduling hint, not a type.** A domain pool
  (`Workers`) may pick the member that already holds a value. It changes
  cost, never meaning, so the checker need not see it. Prior art keeps it
  in the runtime: Spark and Dask preferred locations, Ray's locality-aware
  scheduling, Orleans placement, NUMA and Kubernetes affinity rules.

## 4. Outcomes (**approved**: question 1)

A domain that can lose contact may leave a crossing with no answer. In
example 2 the charge may or may not have happened. The owner decided
(2026-10-31): there is no new outcome type. A crossing returns an ordinary
`Result[T, E]`, and unknown is one case of its error type: a std crossing
error with an `Unknown` case beside the domain's own errors. The caller
handles it with an ordinary `match`. A worker or device reports unknown
only on a crash or reset, and a scoped thread never does.

```text
match place_order!(cart):  # hypothetical syntax: place_order is @remote(Server) (section 6)
    .Ok(receipt) => show(receipt)
    .Err(.Unknown) => check_order_status!(cart.id)
    .Err(error) => show_error(error)
```

The domain's errors and the crossing's errors join with a union type
([#804](https://github.com/HerringtonDarkholme/hd-lang/issues/804)), never
a nested `Result`. Code whose own result is `Result[T, E]` crosses as
`Result[T, E | CrossError]`, so example 7's action matches one flat
error:

```text
match rename.call!(text):  # Result[void, TodoError | CrossError]; hypothetical syntax
    .Ok(()) => pass
    .Err(._1(.Unknown)) => println("saving; refresh to check")  # case _1 is the CrossError
    .Err(error) => println("could not rename: $error")          # either side, through Display
```

L1 and L2 hold by construction, so the checker needs no rule for unknown
(section 5, Outcomes And Cancellation).

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

The destination comes from code or from the deployment (question 3).
Workers and devices are provider values from the source's row, so a test
swaps them with `$.with`. A remote service is named by the deployment
configuration. A crossing may also target a handle's home, read from the
handle's type (question 6). The sketches assume these std traits:

```text
# hypothetical syntax throughout this section
trait Workers[$Cap]:    # $Cap: the capabilities a worker offers
    fn call![T, F < Fn() -> T $Cap](&mut self, code: F) -> Result[T, CrossingError]

trait Gpu:
    type Device         # one type per device, as Gpu0
    fn upload!(&mut self, image: Image) -> Result[Texture[Self::Device], CrossingError]
    fn run!(&mut self, kernel: fn(Texture[Self::Device]) -> void, texture: Texture[Self::Device]) -> Result[void, CrossingError]
```

### The Row Against The Destination (**approved**: question 8)

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
- **New:** each domain trait declares its capabilities. An
  `@remote(D)` function's row is checked against target D's set at its
  declaration, not against its callers' rows
  ([`req.row.set.call`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.set.call)); section 6.

```text
# hypothetical syntax
workers.call!(fn() -> void $ Db: save(doc))?                # error: type-mismatch, Db is not a worker capability
gpu.run!(fn(t: Texture[Gpu0]) -> void: println("x"), tex)?  # error: type-mismatch, a kernel's row is empty
workers.call!(fn() -> void $ Log: log_size(doc))?           # valid: the worker offers Log
background.run!(fn() -> void $ Ui: draw_boxes(faces))?      # error: type-mismatch, only the UI thread offers Ui
faces := background.run!(fn() -> List[Rect]: detect_faces(photo))?  # valid: an empty row fits; the handler suspends until it returns
```

### Each Capture's Mode

- **Checked at each capture, argument and result:** the value crosses in a
  mode the domain kind allows. This is the domain-by-mode table, with L4
  as its last row.
- **Reuses:** capture analysis finds the captures ([`fn.capture.locals`](../../spec/lang/07-functions.md#r-fn.capture.locals)).
  The move checker's copy test sorts them: a `&mut` and a type holding one
  are not copied ([`ref.move.mut`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.mut), [`ref.move.holds`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.holds)).
- **New:** the table as a rule. The mode follows from the value's type and
  the domain kind. A copyable value copies, a non-copyable one transfers, a
  `&` or `&mut` lends, and a handle type crosses as a handle. So the
  checker infers every mode, and code never writes one (question 7).

The four modes add these checks.

**Copy**, at each copied capture, argument and result. In separate
memory, the value must be boundary-safe. In shared memory, any copyable
value copies, as a capture does today ([`fn.capture.copy.value`](../../spec/lang/07-functions.md#r-fn.capture.copy.value)).

- **Enforces:** the destination gets its own value; the source is
  unchanged.
- **Reuses:** boundary-safe values ([`module.boundary.allowed`](../../spec/lang/10-modules.md#r-module.boundary.allowed)): closures,
  trait values, handles and references fail
  ([`module.boundary.not-safe`](../../spec/lang/10-modules.md#r-module.boundary.not-safe)). Values cross as trees
  ([`module.boundary.tree`](../../spec/lang/10-modules.md#r-module.boundary.tree)), and private fields need `Serialize`
  ([`module.boundary.out`](../../spec/lang/10-modules.md#r-module.boundary.out), [`module.boundary.error`](../../spec/lang/10-modules.md#r-module.boundary.error)).
- **New:** the boundary check runs on a crossing's captures, not only on a
  registered signature. Code for separate memory is bounded `Fn`
  (**approved**: question 10), so a
  closure that writes a capture is `not-writable`
  ([`fn.trait.read-only`](../../spec/lang/07-functions.md#r-fn.trait.read-only)). Today a captured variable shares a cell with
  its scope ([`fn.capture.cell.writes`](../../spec/lang/07-functions.md#r-fn.capture.cell.writes)); across memories the write would
  change only the destination's copy.

```text
# hypothetical syntax
workers.call!(fn() -> Image: shrink(photo, edits))?  # error: edits is a *mut Edits, not boundary-safe
workers.call!(fn() -> void: done += 1)?              # error: not-writable, the write would change only the worker's copy
workers.call!(fn() -> Image: shrink(photo, 256))?    # valid: Image is boundary-safe, so it is copied
```

**Transfer**, at the capture and at each later use of the source binding.
The value's type is not copyable, and the crossing ends the source binding
as a move.

- **Enforces:** the destination gets ownership; the source loses the value
  until it is handed back. Transfer is for non-copyable types only
  (question 5).
- **Reuses:** the moves of M1 and M2 ([`ref.move.mut`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.mut),
  [`ref.move.holds`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.holds)). A later use is `use-of-moved-value`
  ([`ref.move.use`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.use)); assigning the returned value makes the binding
  usable again ([`ref.move.reassign`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.reassign)).
- **New:** a crossing's capture is a move site, one more row of the
  [`ref.move.sites`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.sites) table. A copyable type never transfers; it
  copies. In separate memory the value must also hold no
  reference into the source's memory. Every non-copyable type today holds
  a `&mut`, so transfer into a worker, a service or a device has no valid
  case before `!Copy` types
  ([#1141](https://github.com/HerringtonDarkholme/hd-lang/issues/1141)).
  Until then transfer is the scoped thread's ordinary move.

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
workers.call!(fn() -> usize: count_words(lines))?  # error: local-reference-escapes, a worker has separate memory
counts := scope(fn(s: &mut Scope) -> List[Map[string, usize]]:
    tasks := [for i in 0..4 => s.spawn(fn() -> Map[string, usize]: count_quarter(lines, i))]
    lines.push("")                  # error: conflicting-access, lines is lent until the scope joins
    [for t in tasks => t.join()]
)
lines.push("")                      # valid: the scope has joined
```

**Handle**, at each call that takes the handle. A handle's type names its
home domain, so using it in another domain is a type error, with no
run-time check (question 4).

- **Enforces:** the handle row of the mode table, and L3.
- **Reuses:** the device trait's associated type, bound in its key:
  `$.use(Gpu[Device = Gpu0])` has type `&dyn Gpu[Device = Gpu0]`
  ([`req.key.binding.value.reference-values`](../../spec/lang/11-requirements-and-suspension.md#r-req.key.binding.value.reference-values)). Its `upload!` returns a
  `Texture[Gpu0]`, and its `run!` takes only that, so a `Texture[Gpu0]`
  given to Gpu1 is an ordinary `type-mismatch`. Handles are not
  boundary-safe ([`module.boundary.not-safe`](../../spec/lang/10-modules.md#r-module.boundary.not-safe)), so a copy never takes one.
  Within a domain a handle keeps its data alive as a counted reference
  does ([`ref.box.lifetime`](../../spec/lang/13-values-and-references.md#r-ref.box.lifetime)). `using` or an `@auto_closing` constructor
  releases it at scope exit ([`flow.auto-closing.statement`](../../spec/lang/06-control-flow.md#r-flow.auto-closing.statement)).
- **New** (**approved**: question 9): a std marker trait for handle
  types that the compiler knows, as `AutoClosing` is
  one ([`flow.auto-closing.marker`](../../spec/lang/06-control-flow.md#r-flow.auto-closing.marker)). The checker needs it to pick the
  handle mode and to find handles for affinity. Counting a handle across
  domains (L3) is run-time work. Domains are static: one picked at run
  time would need a fresh type (question 4).

```text
# hypothetical syntax
let tex = $.use(Gpu[Device = Gpu0]).upload!(photo)?  # tex: Texture[Gpu0]
$.use(Gpu[Device = Gpu1]).run!(blur, tex)?           # error: type-mismatch, a Texture[Gpu0] on Gpu1
$.use(Gpu[Device = Gpu0]).run!(blur, tex)?           # valid
```

### Handle Agreement And The Affinity Target

- **Checked at the crossing's target and each handle the code uses:** a
  crossing aimed at a handle runs in the domain the handle's type names,
  and every handle the code captures or takes names that same domain.
- **Enforces:** required affinity (question 6), as Chapel's `on x do`.
  Preferred affinity is a run-time hint to a pool such as `Workers`; the
  checker never sees it.
- **Reuses:** the home is a type argument, so "the same domain" is type
  equality. The target's capabilities come from its key, checked as in the
  row check; a row without that key is `missing-requirement`
  ([`req.use.missing`](../../spec/lang/11-requirements-and-suspension.md#r-req.use.missing)).
- **New** (**approved**: question 11): a target read from a type: `at(tex)` with `tex: Texture[D]`
  resolves to the provider for `Gpu[Device = D]`. Then a walk compares
  every handle in the code with `D`.

```text
# hypothetical syntax
at(tex).call!(fn() -> void: blend(tex, mask))?  # error: mask is a Texture[Gpu1], the target is Gpu0
at(tex).call!(fn() -> void: blur(tex))?         # valid: runs on Gpu0, where tex lives
```

### The Later Domain (L4)

- **Checked at each capture of a job:** a job for later takes only copies
  and handles.
- **Enforces:** L4. The job's code is named within the program, so the
  build that stored it runs it (section 1).
- **Reuses:** the copy check rejects a reference, which is not
  boundary-safe, and the `'static` bound rejects a lend
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
jobs.after!(minutes(10), fn() -> Result[void, DbError] $ Db: mark_sent(conn, user.id))?  # error: conn is a &mut; later takes only copies and handles
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
  value, not boundary-safe, so the copy check rejects it.
- **New:** nothing for the row. A device body must be device code, a
  restricted subset with no allocation and no host calls. That subset is
  a new check, not specified here.

```text
# hypothetical syntax
log := $.use(Log)
workers.call!(fn() -> void: log.info("done"))?               # error: the source's Log provider is not boundary-safe
workers.call!(fn() -> void $ Log: $.use(Log).info("done"))?  # valid: the worker's own Log
```

### Top-Level Bindings (**approved**: question 17)

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

workers.call!(fn() -> List[Row]: top(rows))?  # valid: LIMIT is immutable, the same in every domain
workers.call!(fn() -> void: record())?        # error: record writes the mutable top-level hits
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
| Row against the destination | crossing | row subsumption, bound keys, no captured providers, bindable sets | capabilities on domain traits; a service's set from the configuration |
| Mode per capture | capture | capture analysis; the copy test of M1 and M2 | the kind-by-mode rule; modes inferred from types |
| Copy | capture, result | boundary-safe values, `Fn` bounds | the boundary check at captures; `Fn` for separate memory |
| Transfer | capture, later uses | M1 and M2 moves | a capture as a move site; no separate-memory case before `!Copy` types |
| Lend | capture, scope call | closure-parameter lifetimes, lifetime bounds, exclusivity, the suspension store rule | the scope; run-time loans across threads |
| Handle | handle use | bound keys, type equality | a handle marker; counts across domains |
| Affinity target | crossing | type equality, `missing-requirement` | a target read from a type; the handle walk |
| Later (L4) | capture, code | the copy check, the `'static` bound | none |
| Body | body | row resolution at each call, the copy check | the device-code subset |
| Top-level bindings | crossing, reached functions | per-instance module storage, requirement-free init, declared rows | the mutable-state summary |
| Outcomes | none | `Result`, `match`, moves, `abandon` | none |

The gaps:

- **No `Send` or `Sync`.** A shallow `&` and a shared cell race across
  threads. `Sync` gets its own design before threads land; until then
  lend reaches only scoped threads (question 12). Three rules are already
  fixed for it:
  - Sending a value takes its snapshot on the sending thread, so a
    copy-on-write `List` or `Map` sends safely once its counts are atomic,
    as Swift's arrays are `Sendable`.
  - A `&T` is `Send` only when `T: Sync`. A thread that took the snapshot
    through a `&` would load the buffer while the owner's `push` may
    reallocate it.
  - Anything holding a `*mut` or a box is not `Sync`: a snapshot is
    shallow, and a read through `&T` may write through a box field.
- **No threads and no shared heap.** One instance runs on one thread
  ([`module.instance.thread`](../../spec/lang/10-modules.md#r-module.instance.thread)), and instances' heaps are disjoint
  ([`module.instance.disjoint`](../../spec/lang/10-modules.md#r-module.instance.disjoint)). Lend waits for both.
- **No owned non-copyable type.** Transfer into separate memory waits for
  `!Copy` types ([#1141](https://github.com/HerringtonDarkholme/hd-lang/issues/1141)).
- **No device-code subset.** A kernel's body is checked only by its row.
- **The outcome type is not a gap.** Unknown is an error case (section 4).

## 6. Code Generation Per Domain (**approved**: question 14)

A client and server program is compiled into one build per target: the
browser's bundle and the server's module. A function that runs on one
target and is called from another is compiled differently in each: an
endpoint at home, a stub elsewhere, with its arguments decoded from the
wire as untrusted data. So that function is marked where it is declared,
and its callers are marked in their rows. Nothing is placed by inference.

**The declaration says where it runs.** `@remote(Server)` makes a function
an entry point of the `Server` domain:

- its row is checked against what the server target offers;
- its parameters and result must be boundary-safe (copyable plain data),
  and its arguments are untrusted input;
- the server build compiles its body and an endpoint; every other build
  compiles a stub.

**The caller says it talks to that domain.** Calling an `@remote(Server)`
function from another domain needs `$ Server` in the caller's row, so the
dependency shows in every signature up the chain, and a test swaps it with
`$.with(Server=LocalServer {})`. The call is an ordinary bang call whose
error type gains `CrossError` (section 4).

A whole minimal program, one package with two targets:

```text
# hd.toml, hypothetical targets table (question 13)
# [targets.server]  entry = "server",  binds = ["Console", "Net"]
# [targets.browser] entry = "browser", binds = ["Dom"]

# src/shared.hd: no row, so it can land in both builds
pub struct User:
    pub id: i64
    pub name: string

pub fn greeting(user: User) -> string:
    "Hi, ${user.name}"

# src/users.hd: an interface and one remote function; it uses no driver
@error
pub enum DbError:
    @error("no user $id")
    Missing(id: i64)

pub trait Db:
    fn find_user!(&mut self, id: i64) -> Result[User, DbError]

@remote(Server)
pub fn load_user!(id: i64) -> Result[User, DbError] $ Db:
    let mut db = $.use(Db)
    db.find_user!(id)

# src/server.hd: the only module that uses the driver
use dep.postgres.{Pool, PgError, connect}

struct PgUsers:
    pool: Pool

impl Db for PgUsers:
    fn find_user!(&mut self, id: i64) -> Result[User, DbError]:
        match self.pool.query_one!("select id, name from users where id = ?", id):
            .Some(row) => .Ok(User { id: row.i64(0), name: row.text(1) })
            .None => .Err(DbError.Missing(id))

pub fn main!() -> Result[void, PgError] $ Console + Net:
    pool := connect!("postgres://localhost/app")?
    $.with(Db=PgUsers { pool }):
        serve!()    # answers the endpoints of this build's @remote(Server) functions
    .Ok(())

# src/browser.hd
pub fn on_open!(id: i64) -> void $ Dom + Server:
    match load_user!(id):              # a stub here: Result[User, DbError | CrossError]
        .Ok(user) => show(greeting(user))
        .Err(error) => show_error("$error")
```

**How each build is made.** Each target walks the use graph and the call
graph from its entry. The `@remote(D)` functions are extra entries of D's
build.

| Call | Compiled as |
| --- | --- |
| to an `@remote(D)` function, in D's build | a normal call |
| to an `@remote(D)` function, in another build | a stub; the caller's row must list `$ D` |
| to any unmarked function | a normal call under today's row rule |

A row the caller does not meet stays `missing-requirement`
([`req.use.missing`](../../spec/lang/11-requirements-and-suspension.md#r-req.use.missing)), so a forgotten `$ Db` never becomes a network
call. The two builds come from one compiled program, so a stub and its
endpoint name the same function
([`req.determinism.same-program`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.same-program)).

| | browser build | server build |
| --- | --- | --- |
| modules reached | `browser`, `shared`, `users` | `server`, `users`, `shared`, `dep.postgres` |
| `load_user` | a stub | its body and an endpoint |
| `PgUsers`, `connect`, `dep.postgres` | not reached | yes |
| `greeting` | yes | not reached |

**What stays off the client.** `dep.postgres` is used only by
`server.hd`, which the browser entry never reaches, so the driver is not
even parsed for the browser. A function that touches the database lists
`$ Db` and is either `@remote` (a stub on the client) or unreachable from
it. The one gap is a secret written as a literal in shared code, which
both builds would copy; a rule or lint that secrets come from `Env`
closes it.

**Where no mark is needed.** Workers, threads and devices run the same
build as their caller, so nothing compiles differently there. Their
crossing is already visible where it happens, as a call on a domain value:
`workers.call!(...)`, `background.run!(...)`.

**Prior art.** Swift's distributed actors mark both sides: a
`distributed func` and `try await` at each call. gRPC and tRPC declare the
procedure as a service and give callers a stub. React marks the
declaration with `"use server"`, but its call sites look local. Links and
Electric Clojure place code by inference, which hides the network.

## 7. Syntax

Every spelling here is hypothetical and needs approval. Two alternatives:

```text
small := $.use(Workers).call!(thumbnail, photo, 256)?            # hypothetical syntax
small := at workers capture { photo } do: thumbnail(photo, 256)  # hypothetical syntax
```

The first is a call on a domain value. The second is the owner's note's
block, which names every capture. Neither writes a mode: each value's type
decides it (question 7).

## 8. Questions For The Owner

1. **Is "unknown" a first-class outcome?** Answered (owner, 2026-10-31):
   no new type. A crossing returns `Result[T, E]`, and unknown is an
   `Unknown` case of a std crossing error, matched like any other error
   (section 4).
2. **Do L1 to L4 hold?** Answered: yes, all four (owner, 2026-10-31).
3. **Does code name the destination?** Answered (owner, 2026-10-31): a
   domain value in code for workers and devices, a provider such as `Gpu`
   or `Workers` from the requirement row (swappable with `$.with` in
   tests). A service is named at the function's declaration with
   `@remote(D)` (question 14, owner, 2026-11-02).
4. **How does a handle name its domain instance?** Answered (owner,
   2026-10-31): in the type, as `Texture[Gpu0]`; using a handle in another
   domain is a type error, with no run-time check. Dynamic instances (a
   worker picked at run time) would need a fresh type per domain value, as
   Rust's generativity does; not planned now.
5. **When does transfer apply to plain values?** Answered (owner,
   2026-10-31): transfer is for non-copyable types only. Today those all
   hold a `&mut` into the source's memory, so transfer to separate memory
   has no valid case yet; the owner moved non-copyable types forward (their own
   design) so a user type such as a photo buffer can opt out of copying and
   transfer (example 1).
6. **How is affinity expressed?** Answered (owner, 2026-10-31): required
   affinity, (a): a crossing may target a handle's home domain, read from
   its type, and the checker requires every handle the code uses to share
   it, as Chapel's `on x do`. Preferred affinity is a run-time hint to a
   domain pool (`Workers`), outside the checker.

7. **Is a crossing mode ever written?** Answered (owner, 2026-10-31): no.
   The type decides: a copyable value copies, a non-copyable one
   transfers, a reference lends, and a handle stays home. There is no
   `transfer(x)`; to move a value instead of copying it, give its type
   `impl !Copy`. Swift and Rust have no transfer keyword either.
8. **How does a destination state its capabilities?** Answered (owner,
   2026-10-31): as a row parameter on the domain trait, such as
   `Workers[$ Clock + Console]`, checked by row subsumption, so each pool
   offers its own set and tests swap it with `$.with`. Not a fixed set per
   domain kind.
9. **How does the checker know a type is a handle?** Answered (owner,
   2026-10-31): a std marker trait the compiler knows, implemented by each
   handle type (`impl Handle for Texture[D]`), as `AutoClosing` is for
   `using`. A domain type argument alone does not make a type a handle.
10. **May code for separate memory write a capture?** Answered (owner,
    2026-10-31): no. It is bounded `Fn`, so a write is `not-writable`; the
    code returns the new value instead, as Swift's `@Sendable` closures do.
11. **May a crossing target a handle's home?** Answered (owner,
    2026-10-31): yes, `at(tex)` reads the domain from the handle's type and
    every other handle the code uses must share it (Chapel's `on x do`).
12. **What stops a lend racing?** Answered (owner, 2026-10-31): design
    `Sync` before threads; until then lend is for scoped threads only and
    the race is a listed gap. Its three fixed rules (value sends take the
    snapshot, `&T` is `Send` only when `T: Sync`, `*mut` and box holders
    are not `Sync`) are in section 5.

13. **Where do target capability sets live?** In the deployment
    configuration, as named targets with their runtime profiles (one
    `hd.toml` table per target), or in code as a provider value per target.
    The checker needs them to check an `@remote` function's row and to
    pick each build's entries.
14. **How is a crossing between targets marked?** Answered (owner,
    2026-11-02): on both sides. The declaration says `@remote(D)`, which
    decides how the function compiles (an endpoint in D's build, a stub
    elsewhere) and checks its signature as an untrusted boundary. A caller
    in another domain lists `$ D` in its row. The call stays a bang call
    whose error gains `CrossError`. Nothing is placed by inference
    (section 6).
15. **One program, or separate packages per side?** Answered (owner,
    2026-11-02) by the principle in section 1: every domain runs the same
    code, so one program is compiled into one build per target, and a
    crossing sends a code identity and data, never code.

16. **Is a background thread a sixth kind of domain?** Recommended: yes.
    It shares memory, as a scoped thread does, but outlives the call that
    started it, as a worker does, so lend is refused (no scope ends it) and a
    move needs no copy. Its capabilities exclude the UI thread's `Ui`, so a
    view update sent to it is a `type-mismatch` at compile time, where
    Android throws `CalledFromWrongThreadException` at run time. The call
    that dispatches is a bang call: the UI thread's event loop keeps running
    while the handler waits, as Swift's `@MainActor` with `Task.detached`
    and Kotlin's `withContext(Dispatchers.Default)` do. It needs threads and
    `Sync` (question 12): a copied `List` shares its buffer, so its counts
    must be atomic.

17. **May crossing code use top-level bindings?** Answered (owner,
    2026-11-02): it may read immutable top-level bindings. Code that
    reaches a `let mut` top-level binding, directly or through a callee,
    cannot cross: the crossing is an error (section 5, Top-Level
    Bindings).

```text
let tex: Texture[Gpu0] = gpu0.upload!(photo)?  # question 4 (a)
```

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
| B: portable closures placed by rows (#166) | Content hash across builds; rows checked at the destination | Placement by configuration alone (question 3) |
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

All 21 `text` blocks were parsed on 2026-10-31 and 2026-11-02 with
`scripts/hd-conformance.sh parse`, which checks syntax only; nothing was
type-checked. Std names (`Workers`, `Jobs`, `Gpu`, `scope`) are sketches.

| Block | Where | Result |
| --- | --- | --- |
| 1 to 6 | Examples 1 to 6 | parse |
| 7 | Section 4, the `match` on a crossing's result | parses; the placement and the `.Unknown` case are hypothetical |
| 8 | Section 5, the assumed `Workers` and `Gpu` traits | parses; `# hypothetical syntax` |
| 9 to 16 | Section 5, one invalid and one valid sketch per check | parse; every block is `# hypothetical syntax` |
| 17 | Section 6, the whole minimal program with `@remote(Server)` | parses (2026-11-02); the targets table is a comment, and the four modules share one block |
| 18 | Section 7, syntax | fails as expected at `at workers`: both lines `# hypothetical syntax` |
| 19 | Question 4 | parses |
| 21 | Section 5, top-level bindings | parses; `# hypothetical syntax` |
| 20 | Example 8 | parses (2026-11-02); the row-check sketch with its two background-thread lines parses too |
