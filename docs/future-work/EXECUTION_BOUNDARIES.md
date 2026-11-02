# Execution Domains: Moving Code And Data

Status: proposal, 2026-11-02; domain: language (execution domains and
crossings) and std; not accepted. Nothing here is in the specification.

- [1. Context And Goal](#1-context-and-goal)
- [2. Eight Programs That Cross](#2-eight-programs-that-cross)
- [3. The Design](#3-the-design)
- [4. The Eight Programs In The Design](#4-the-eight-programs-in-the-design)
- [5. The Process, End To End](#5-the-process-end-to-end)
- [6. Cost](#6-cost)
- [Appendix A. Stress Test](#appendix-a-stress-test)
- [Appendix B. Prior Art](#appendix-b-prior-art)
- [Appendix C. Alternatives Considered](#appendix-c-alternatives-considered)
- [Sources](#sources)
- [Parse Log](#parse-log)

## 1. Context And Goal

The goal is to move computation and data between **execution domains**: a
worker, a server, a job that runs later, a GPU, or a thread. Today an hd
program runs in one instance on one thread
([`module.instance.thread`](../../spec/lang/10-modules.md#r-module.instance.thread)).
Instances exchange only boundary-safe values, copied as trees
([`module.boundary.allowed`](../../spec/lang/10-modules.md#r-module.boundary.allowed),
[`module.boundary.tree`](../../spec/lang/10-modules.md#r-module.boundary.tree)), and closures never cross
([`module.boundary.not-safe`](../../spec/lang/10-modules.md#r-module.boundary.not-safe)). So each program in
section 2 copies too much, cannot say what it needs, or cannot be written.

**Principle: every execution domain runs the same code.** One program is
compiled for every domain it uses. A crossing never sends code or an
executable; it sends the identity of code the destination already has,
plus data. A peer that runs another build, as after a deploy, refuses the
call with `CrossError.OtherBuild` before any code runs, so a page left
open learns it must reload, and a stored job fails clearly.

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
    # Wanted: move the photo, so any use of `photo` is an error until it returns.
    let (back, small) = $.use(Workers).run!(fn() -> (Image, Image): thumbnail(photo, 256))?
    .Ok(small)
```

**2. Checkout: the order is placed on the server.** Only the server has
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
the same build that stored it (section 1). The email address may be
captured; a connection or a `&mut` may not.

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

## 3. The Design

The design adds four parts and no syntax. Everything else is existing hd
(rows and providers, closures, moves, scopes) or std library.

| # | Part | Its one job |
| --- | --- | --- |
| 1 | `Send` and `Wire` | what may cross: into shared memory, or into separate memory |
| 2 | `Remote` and `Served[T]` | the only code another target runs, and references to the objects that serve it |
| 3 | `CrossError` | how a crossing fails |
| 4 | one build, one output per target | how each domain's code is made |

A **build** compiles the whole program once and has one build ID. A
**target** is one entry module of the build and its output: Wasm for a
browser, a server or a worker, device code for a GPU.

A **crossing** is a call that runs in another domain. There are two
kinds, and nothing else crosses:

- a method of a domain value that takes code: `workers.run!(closure)`,
  `jobs.after!(delay, closure)`, `s.spawn(closure)`;
- a method of a `Remote` trait called on a `Served[T]`:
  `users.load_user!(id)` when `users` is a stub.

The first kind is visible at the call. The second is visible at the
trait's declaration (`< Remote`) and at the call as a bang call whose
error has a `Cross` case. Whether one such call crosses is decided where
its `Served[T]` is installed, usually as a provider (section 5).

### Domains Are Values

An execution domain is a place code runs: a thread, a worker instance, a
server, a GPU, a run of a job queue. Code never names one by type. It
holds a **domain value**, which it gets from a provider in its row, so a
test swaps it with `$.with`. A program can hold any number of them: a
pool of workers, every GPU on the machine, a Redis node per shard.

| Property | Values | Decides |
| --- | --- | --- |
| Memory | shared with the caller, or separate | the bound: `Send` or `Wire` |
| Target | the caller's target, or another one | closures, or `Remote` methods only |
| Contact | cannot be lost, or can | the result: `T`, or `Result[T, CrossError]` |

The domain kinds are std library types built on the four parts:

| Std type | Memory | Target | Contact | Code it takes |
| --- | --- | --- | --- | --- |
| `Scope['s]` (scoped threads) | shared | same | cannot be lost; joins before `'s` ends | a closure, `Send`, may borrow for `'s` |
| `Background` | shared | same | cannot be lost | a closure, `Send`, `'static` |
| `Workers`, `Worker` | separate | same | can be lost | a closure, `Wire`, `'static` |
| `Jobs` | separate, later | same | nobody waits | a closure, `Wire`, `'static` |
| a server, from `http_transport(url)` | separate | another | can be lost | `Remote` methods |
| `Gpus`, `Gpu` | separate | another (device code) | can be lost | `Remote` methods |

Each domain type states the capabilities it offers as a row parameter,
`Workers[$ Log + Clock]`, and code whose row asks for more does not fit,
through ordinary row subsumption
([`req.row.subsume.missing`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.subsume.missing)).

A panic in code on a shared-memory thread is a panic of the program, as
today. So the two thread kinds return a plain `T`, and only domains that
can lose contact return a `CrossError`.

### What May Cross: Send And Wire

Two std marker traits, checked structurally as `Copy` is
([`ref.copy.marker`](https://github.com/HerringtonDarkholme/hd-lang/blob/9af95bd2f/spec/lang/13-values-and-references.md#r-ref.copy.marker)):

- **`Wire`:** a value encoded as a self-contained tree, sent into separate
  memory. This names today's boundary-safe values
  ([`module.boundary.allowed`](../../spec/lang/10-modules.md#r-module.boundary.allowed)), plus `Served[T]`.
- **`Send`:** a value moved to, or borrowed by, another thread of the same
  memory. Every `Wire` value is `Send`.

| Type | `Wire` | `Send` |
| --- | --- | --- |
| numbers, `bool`, `char`, `string` | yes | yes |
| struct, enum, tuple, `List`, `Map`, `Set`, optional, `Result` | when every part is | when every part is |
| `Served[T]` | yes, as its home and token | yes |
| `&T`, `&mut T` | no | when `T < Send` |
| `*mut T`, a box | no | no |
| a trait value, a provider value | no | no |
| a closure | when its captures are `Wire` | when its captures are `Send` and it writes none |

`&T` needs no separate marker. While `&p` is lent, its owner does not
write `p` ([`ref.excl.shared`](../../spec/lang/13-values-and-references.md#r-ref.excl.shared)), and the one way to write
through a `&`, a box field
([`ref.read.shallow`](../../spec/lang/13-values-and-references.md#r-ref.read.shallow)), is not `Send`.

Three rules complete it for closures, the code a domain of the caller's target runs:

- A closure that writes a captured variable is not `Send`. Captures share
  a cell with their scope
  ([`fn.capture.cell.writes`](../../spec/lang/07-functions.md#r-fn.capture.cell.writes)), so two spawns doing `total += n`
  would race. They are rejected; each returns its count instead.
- A closure is `Send` or `Wire` only when every top-level binding it
  reaches, directly or through a callee, is immutable and `Wire`. Module
  storage is per instance
  ([`module.init.program-instance`](../../spec/lang/10-modules.md#r-module.init.program-instance)), so a `let mut` counter or a
  top-level box would change only one domain's copy.
- A closure handed to a crossing takes each `impl !Copy` capture by move:
  the source binding ends, and a later use is `use-of-moved-value`
  ([`ref.move.use`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.use)), as Swift's `sending` does.

The mode is never written. A `Copy` value copies, an `impl !Copy` value
moves, a reference lends inside a `Scope`, and a `Served[T]` stays home.
Into separate memory a move still copies the bytes once, in bulk, because
two Wasm instances do not share memory; what it saves is the source's
copy and any field-by-field encoding. In shared memory a move copies
nothing, and a copy-on-write `List` shares its buffer, whose counts
become atomic with threads. Rust states the same split with `Send` on
`thread::spawn` and serde's `Serialize` on encoders.

### Code For Another Target: Remote And Served

A domain of another target runs only methods of interface traits that
extend the std marker `Remote`. Those methods are its endpoints, and
nothing else of the program is reachable from outside.

- **R1.** Every method is a bang method whose parameters and result are
  `Wire` and not closures. It is not generic and has no row; its
  implementation gets what it needs from its own fields. A trait may be
  generic, and the served type fixes its type arguments.
- **R2.** Its error type `E` satisfies `E < From[CrossError]`, so a lost
  contact arrives as one of the method's own errors. An `@error` enum gets
  that from one variant, `Cross(@from cause: CrossError)`
  ([`annot.error.generates`](../../spec/lang/14-annotations.md#r-annot.error.generates)).
- **R3.** A domain serves a root object through a `Home[S]`, for every
  `Remote` trait `S` implements (What Std Adds, below). A method may also
  return a new object as a `Served[U]`, which the home adds to its table
  and serves too.

A **`Served[T]`** is a reference to an object of the `Remote` trait `T`
that one domain serves, its **home**. It is a counted reference, so it
can be a provider as a box can. It implements `T` through an impl the
compiler generates for each `Remote` trait, `impl Users for
Served[Users]` (B3): each method call goes to the home and is a local
call there. The caller gets the first one
from `remote::[T](transport)`, the root object of a domain; a test
installs a fake instead. So a service stub, a GPU texture, a server
action, a stream cursor and a long-lived process are all `Served[T]`:

| Object | Its trait | Its home |
| --- | --- | --- |
| the users service | `Users` | a server |
| a texture | `Texture` | a GPU |
| a server action | `Call[A, T, E]` | the server that rendered it |
| a stream cursor | `Next[T]` | the server that opened it |
| an account process | `Account` | a worker |

Three rules keep served objects safe:

- **Home only.** Every use is a call to the home, so an object is never
  touched elsewhere. Device code cannot make crossings, so a kernel given
  a `Served[Texture]` whose home is another GPU is refused with
  `CrossError.WrongHome` before it runs.
- **Unforgeable.** A `Served[T]` that leaves its target carries an
  unguessable token bound to the session it was sent to. The home refuses
  any other token with `WrongHome`, so a page cannot call another user's
  action by editing an id.
- **Leased.** A `Served[T]` held across a contact-losing boundary is a
  lease that each call renews. The home drops the object after a timeout,
  and a later call is `WrongHome`, so a closed browser tab leaks nothing.

"Code goes to the data" is then library: `workers.at(rows)` returns the
worker that is `rows`'s home, so code sent there reaches `rows` by local
calls, as Chapel's `on x do` runs where `x` lives.

```text
# hypothetical syntax
pub trait Users < Remote:
    fn load_user!(&mut self, id: i64) -> Result[User, UserError]

pub trait Filters < Remote:                    # served on a GPU: device code
    fn blur!(&mut self, texture: Served[Texture], radius: f32) -> Result[void, GpuError]
```

A device target's entry builds a `Home` of its kernels' implementation
and answers each frame with `handle_call!`. That implementation must be
device code: no allocation and no host calls. That subset, and launch geometry, get their own design.

### How A Crossing Fails: CrossError

`CrossError` is a std enum with four cases:

| Case | Meaning | Did it run? |
| --- | --- | --- |
| `Unreachable` | the call never left | no |
| `Unknown` | it left and no answer came, including a timeout after sending, or the destination crashed | maybe |
| `OtherBuild` | the destination runs another build of the program, as after a deploy, and refused | no |
| `WrongHome` | the call named an object its destination does not serve: another GPU's texture, a forged token, an expired lease | no |

The error type that carries it is always one the code declares:

- A domain of the caller's target: its `run!` returns `Result[T, CrossError]` when
  contact can be lost. Code that returns its own `Result` is followed by
  `??`: `?` converts the `CrossError` through `From`
  ([`expr.try.convert.from`](../../spec/lang/05-expressions.md#r-expr.try.convert.from)).
- A `Remote` method's error type holds it through R2.

Cancelling a crossing, by abandoning the task that waits on it
([`req.abandon.host`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.host)), sends a cancel; the call's outcome is then
`Unknown`. A moved value never comes back through an error, so an unknown
outcome never returns ownership.

### One Build, One Output Per Target

Every domain runs the same program (section 1). The package's target
table lists each target's entry module, its `main`, and its runtime
profile, which is what its host binds (section 5 shows one). One build
compiles every target, and every output carries the same build ID.

- **B1.** Each target walks the use graph and the call graph from its
  entry and compiles only what it reaches. A module only the server
  reaches, such as one that uses a database driver, is never parsed for
  the browser.
- **B2.** For each `S` of a `Home[S]` a target builds, the compiler
  generates the body of `handle_call![S]`: check the build ID, find the
  object and check its token, decode the method number and arguments as
  untrusted input, call, encode the result.
- **B3.** For each `Remote` trait `T` whose `Served[T]` a target can
  hold, as from `remote::[T](transport)`, the compiler generates
  `impl T for Served[T]`: the stub. Each method encodes its arguments,
  sends them with the method's number, decodes the result, and maps a
  transport failure to `CrossError` through R2. A stub holds no
  implementation code.
- **B4.** A crossing to a domain of the caller's target compiles into
  the one module. Its code is
  named by the closure's lifted function, plus its encoded captures.
- **B5.** Every `Wire` type gets a generated codec, the tree encoding of
  boundary values ([`module.boundary.tree`](../../spec/lang/10-modules.md#r-module.boundary.tree)). The build ID travels
  with every crossing, so a peer of another build refuses with
  `OtherBuild` before any code runs. Within one build, every target
  numbers a trait's methods alike. A decoded `Served[U]` takes the
  transport its frame came on as its home.

### What Std Adds

Everything the design adds to std, written out. Serving over HTTP reuses
the server SDK's `serve!` and `HttpServer`; calling reuses `std.http`'s
`Http`. Two items are generated rather than written: the body of
`handle_call!` (B2) and the `impl T for Served[T]` of each `Remote`
trait (B3).

```text
# hypothetical syntax; std.remote
use std.http.{Body, Http, Request, Response, Url}

pub trait Remote:                            # a marker; the compiler checks R1 and R2 on each trait that extends it
    pass

pub enum CrossError:
    Unreachable
    Unknown
    OtherBuild
    WrongHome

pub trait Transport:                         # carries call frames to one home
    fn call!(&mut self, frame: List[u8]) -> Result[List[u8], CrossError]

pub struct Served[T]:                        # a reference to an object of T at its home; fields private
    transport: *mut dyn Transport
    object: u64                              # 0 is the home's root object
    token: List[u8]                          # empty within the home's own target

pub fn remote[T < Remote](transport: *mut dyn Transport) -> Served[T]:
    Served { transport, object: 0, token: [] }

pub struct Home[S]:                          # the objects one domain serves; fields private
    root: S
    objects: ServedTable                     # objects that methods returned as Served[U], with tokens and leases

pub fn home[S](root: S) -> *mut Home[S]:
    box(Home { root, objects: ServedTable::new() })

# B2 generates the body for each S: check the build ID, find the object and check its token,
# decode the method number and arguments as untrusted input, call, encode the result
@intrinsic("handle_call")
pub fn handle_call![S](home: &mut Home[S], frame: List[u8]) -> List[u8]:
    panic("intrinsic")

# HTTP, as plain hd over the items above
pub struct HttpTransport:
    url: Url
    http: *mut dyn Http

impl Transport for HttpTransport:
    fn call!(&mut self, frame: List[u8]) -> Result[List[u8], CrossError]:
        request := Request { method: .Post, url: self.url, body: Body::from_bytes(frame) }
        match self.http.send!(&request, 0):
            .Ok(response) => match response.bytes():
                .Ok(bytes) => .Ok(bytes)
                .Err(_) => .Err(.Unknown)
            .Err(_) => .Err(.Unknown)        # split by HttpError: a connection never opened is Unreachable

pub fn http_transport(url: Url) -> *mut dyn Transport $ Http:
    box(HttpTransport { url, http: $.use(Http) })

pub fn http_handler![S](request: Request, home: &mut Home[S]) -> Response:
    match request.bytes():
        .Ok(frame) => Response { body: Body::from_bytes(handle_call!(home, frame)) }
        .Err(_) => Response { status: .BadRequest }
```

`http_handler::[S]` has exactly the handler type the SDK's `serve!`
takes, `fn!(Request, &mut S) -> Response`, with `Home[S]` as its state.
A `Worker` and a `Gpu` implement `Transport` the same way, over their
own channels, and a worker or device answers frames by calling
`handle_call!` on its `Home`.

## 4. The Eight Programs In The Design

**1. Thumbnail in a worker.** Declared `impl !Copy`, with `Wire` fields,
an `Image` is taken by move: `photo` is unusable until the result returns
it, and its bytes cross once, in bulk.

```text
# hypothetical syntax; workers: the Workers provider
let mut workers = $.use(Workers)
let (back, small) = workers.run!(fn() -> (Image, Image): thumbnail(photo, 256))?
```

**2. Checkout on the server.** The server serves `Orders`; the browser's
`main` installs a stub, and the call is an ordinary provider call.

```text
# hypothetical syntax
@error
pub enum OrderError:
    @error("card declined")
    Declined
    @error("lost contact: $cause")
    Cross(@from cause: CrossError)

pub trait Orders < Remote:
    fn place_order!(&mut self, cart: Cart) -> Result[Receipt, OrderError]

pub fn on_buy!(cart: Cart) -> void $ Page + Orders:
    let mut orders = $.use(Orders)
    match orders.place_order!(cart):
        .Ok(receipt) => show(receipt)
        .Err(.Cross(.Unknown)) => check_order_status!(cart.id)
        .Err(error) => show_error("$error")
```

**3. Welcome mail in ten minutes.** The job's closure captures two strings,
which are `Wire`. A `&mut DbConn` would not be.

```text
# hypothetical syntax; jobs: the Jobs provider
let mut jobs = $.use(Jobs)
jobs.after!(minutes(10), fn!() -> Result[void, MailError] $ Mailer: send_welcome!(email, name))?
```

**4. GPU filters.** The device holds the texture; every kernel call is a
crossing to its home.

```text
# hypothetical syntax; gpus: the Gpus provider
let mut gpu = gpus.device(0)?
tex := gpu.upload!(photo)?                      # tex: Served[Texture], home: this GPU
let mut filters = remote::[Filters](gpu)
filters.blur!(tex, 2.0)?
image := gpu.read_back!(tex)?
```

**5. Parallel word count.** Scoped threads borrow `lines`; the scope joins
before it returns, so the loan ends there.

```text
# hypothetical syntax; lines: &List[string]
counts := scope(fn(s: &mut Scope) -> List[Map[string, usize]]:
    tasks := [for i in 0..4 => s.spawn(fn() -> Map[string, usize]: count_quarter(lines, i))]
    [for t in tasks => t.join()]
)
```

**6. Dashboard filter at the data.** The service method runs where the
rows are; only the filter and 200 rows cross.

```text
# hypothetical syntax
pub trait SalesApi < Remote:
    fn top_sales!(&mut self, filter: Filter) -> Result[List[Sale], ApiError]
```

**7. Server action.** The server renders a `Served[Call[A, T, E]]` whose
home is the server; the browser calls it later, and the captured id never
leaves. Its token is bound to the page's session.

```text
# hypothetical syntax; rename: Served[Call[string, void, TodoError]]
match rename.call!(text):
    .Ok(()) => pass
    .Err(.Cross(.Unknown)) => println("saving; refresh to check")
    .Err(error) => println("could not rename: $error")
```

**8. Scan off the UI thread.** The background thread shares memory, so
`photo` is shared, not copied. A closure that used `$ Ui` would not fit,
because `Background` does not offer it.

```text
# hypothetical syntax; background: the Background provider, ui: the Ui provider
ui.show_spinner()
faces := background.run!(fn() -> List[Rect]: detect_faces(photo))
ui.draw_boxes(faces)
```

## 5. The Process, End To End

One package with a browser executable and a server executable, from
`hd.toml` to one call at run time. The lines marked ◀ change what the
program means. Everything else is ordinary hd and existing std: the
server runs on the server SDK's `serve!`
([`std-server-sdk.serve-decl`](../../spec/std/server-sdk.md#r-std-server-sdk.serve-decl)),
and the browser calls through `std.http`'s `Http`. The only new names are
those of `std.remote` (section 3, What Std Adds). `pg` is an illustrative
Postgres driver package.

**The manifest.** Every table and key below exists today. An executable
is a target ([`cli.exe.table`](../../spec/cli/command-line.md#r-cli.exe.table)).
Its entry module's `main!` row says what its host binds, and the
`[capabilities]` table must list every host capability of every `main`
row
([`cli.cap.table.required`](../../spec/cli/command-line.md#r-cli.cap.table.required)).
The table is shared by both executables, but a host binds only the
traits its own `main` row names, so the browser never gets `Net`.

```toml
[package]
name = "shop"

[[executable]]
name = "web"
module = "browser"          # src/browser.hd; its main! row: $ Page + Http

[[executable]]
name = "api"
module = "server"           # src/server.hd; its main! row: $ Net + HttpServer

[dependencies]
pg = "github.com/acme/pg@1.2.0"                   # illustrative
server_sdk = "github.com/acme/server_sdk@0.1.0"   # the SDK of spec/std/server-sdk.md; path illustrative

[host]
"ui.Page" = "providers/page.ts"                   # the provider of src/ui.hd's @host trait Page

[capabilities]
"pkg.ui.Page" = true
Http = ["api.example.com"]
Net = ["db.internal:5432"]
"dep.server_sdk.server.HttpServer" = true
```

**Writing it.**

```text
# hypothetical syntax
# src/users.hd: a library module both executables use
pub struct User:
    id: i64
    name: string                                    # Wire: every field is

@error
pub enum UserError:
    @error("no user $id")
    Missing(id: i64)
    @error("lost contact: $cause")
    Cross(@from cause: CrossError)                  # ◀2

pub trait Users < Remote:                           # ◀1
    fn load_user!(&mut self, id: i64) -> Result[User, UserError]

# src/ui.hd: the page, a package host capability
@host
pub trait Page:
    fn show(&mut self, text: string) -> void

# src/server.hd: the api executable's entry; only it uses the driver
use dep.pg.{Pool, connect}
use dep.server_sdk.server.{HttpServer, ServerError, serve}
use pkg.users.{User, UserError, Users}
use std.net.{Net, NetError}
use std.remote.{home, http_handler}

struct PgUsers:
    pool: Pool                                      # Pool keeps the counted *mut dyn Net it connected with

impl Users for PgUsers:
    fn load_user!(&mut self, id: i64) -> Result[User, UserError]:
        match self.pool.query_one!("select id, name from users where id = ?", id):
            .Some(row) => .Ok(User { id: row.i64(0), name: row.text(1) })
            .None => .Err(UserError.Missing(id))

@error
enum ApiError:
    @error("database: $cause")
    Db(@from cause: NetError)
    @error("server: $cause")
    Server(@from cause: ServerError)

pub fn main!() -> Result[void, ApiError] $ Net + HttpServer:
    pool := connect!("db.internal:5432")?
    serve!(0, home(PgUsers { pool }), http_handler::[PgUsers])?    # ◀3
    .Ok(())

# src/browser.hd: the web executable's entry
use pkg.ui.Page
use pkg.users.{UserError, Users}
use std.http.{Http, Url}
use std.remote.{Served, http_transport, remote}

pub fn main!() -> void $ Page + Http:
    url := Url::parse("https://api.example.com/rpc").unwrap()
    let stub: Served[Users] = remote::[Users](http_transport(url))   # ◀4
    $.with(Users=stub):                                               # ◀5
        on_open!(7)

pub fn on_open!(id: i64) -> void $ Page + Users:
    let mut page = $.use(Page)
    let mut users = $.use(Users)                    # users: &mut dyn Users
    match users.load_user!(id):                     # ◀6
        .Ok(user) => page.show("Hi, ${user.name}")
        .Err(.Cross(.Unknown)) => page.show("no answer; try again")
        .Err(error) => page.show("$error")
```

| Line | What it means |
| --- | --- |
| ◀1 `pub trait Users < Remote` | `load_user` may be called from another target; R1 and R2 apply to it |
| ◀2 `Cross(@from cause: CrossError)` | a lost contact becomes a `UserError` (R2) |
| ◀3 `serve!(0, home(PgUsers { pool }), http_handler::[PgUsers])` | the SDK's request loop hands each request to `http_handler`, which calls `PgUsers` |
| ◀4 `remote::[Users](http_transport(url))` | a `Served[Users]` whose home is that URL: the stub |
| ◀5 `$.with(Users=stub)` | every `Users` call below goes to the server |
| ◀6 `users.load_user!(id)` | nothing new: a call on a `&mut dyn Users` |

`PgUsers` and the stub both reach their host through a field, not a row:
a counted provider value may be stored
([`req.use.value.flow`](../../spec/lang/11-requirements-and-suspension.md#r-req.use.value.flow)),
and a row records only lookups
([`req.use.value.row-meaning`](../../spec/lang/11-requirements-and-suspension.md#r-req.use.value.row-meaning)).
So R1's "no row" costs the implementation nothing.

**Reading it.** ◀5 is the line that turns ◀6 into a round trip. Above
◀5, the same call would be local. The call line alone cannot tell,
because `$.use(Users)` is a `&mut dyn Users`
([`req.use.value.ordinary-type.reference-values`](../../spec/lang/11-requirements-and-suspension.md#r-req.use.value.ordinary-type.reference-values)).
It shows three things: the method belongs to a `Remote` trait, so it may
cross; it is a bang call, so it may wait; and its error has a `Cross`
case, so contact may be lost. A gRPC stub passed as an interface reads
the same way.

**Checking it.**

| Line | Check | Breaking it |
| --- | --- | --- |
| `hd.toml` | each `main` row's host capabilities are in `[capabilities]` | drop `Http`: `missing-capability`, today's rule |
| ◀1 | R1: `load_user` is a bang method; `i64` and `Result[User, UserError]` are `Wire`; no generics, row or closure | `fn load_user(...)` without `!`: an error at ◀1 |
| ◀2 | R2: `UserError < From[CrossError]` | delete the `Cross` variant: an error at ◀1 |
| `impl Users for PgUsers` | the impl's row agrees with the trait's, which is empty ([`req.row.callable.impl-agrees`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.callable.impl-agrees)) | `$ Net` on the method: an error |
| ◀3 | `http_handler::[PgUsers]` has the handler type `serve!` takes, `fn!(Request, &mut Home[PgUsers]) -> Response`; `handle_call!` needs `PgUsers` to implement a `Remote` trait | `home(pool)`: an error, `Pool` implements no `Remote` trait |
| ◀4 | `Users < Remote` | `remote::[Page](...)`: an error |
| ◀5 | `Served[Users]` implements `Users`, through the impl B3 generates | none |
| ◀6 | nothing new | |

**Building it.** `hd build` builds both executables with one build ID.
Each walks from its own entry module (B1):

| | `web` (browser) | `api` (server) |
| --- | --- | --- |
| `users.hd` | `User`, `UserError`, `Users`, and their codecs (B5) | the same |
| `ui.hd` | `Page`, and the generated `web.host.mjs` glue | not reached |
| `server.hd`, `pg`, `server_sdk` | never parsed | compiled |
| `std.remote` | `remote`, `http_transport`, `HttpTransport` | `home`, `http_handler`, `handle_call!` |
| the stub `impl Users for Served[Users]` (B3) | generated | none |
| the body of `handle_call![PgUsers]` (B2) | none | generated |

Each executable as built, shown as the hd it amounts to. The source as
written is kept; the build adds only the parts below. Generated names
such as `wire_encode_User` are illustrations.

*`web`, as built:*

```text
# hypothetical syntax; added to the web executable
BUILD_ID := "3f9a"                                  # the same in both executables of this build

# B5: one codec per Wire type the executable sends or receives
fn wire_encode_User(out: &mut Bytes, user: &User) -> void:
    wire_encode_i64(out, user.id)
    wire_encode_string(out, user.name)

fn wire_decode_User(input: &mut Reader) -> Result[User, WireError]:
    id := wire_decode_i64(input)?
    name := wire_decode_string(input)?
    .Ok(User { id, name })

# B3: the stub, because main makes a Served[Users] (◀4)
impl Users for Served[Users]:
    fn load_user!(&mut self, id: i64) -> Result[User, UserError]:
        let frame: mut Bytes = Bytes::new()
        wire_encode_frame_head(frame, BUILD_ID, self.object, self.token, +0)   # method 0: load_user
        wire_encode_i64(frame, id)
        match self.transport.call!(frame.to_list()):                          # HttpTransport: one POST
            .Ok(reply) => match wire_decode_Result_User_UserError(Reader::new(reply)):
                .Ok(result) => result
                .Err(_) => .Err(UserError.from(CrossError.Unknown))
            .Err(cross) => .Err(UserError.from(cross))                       # compiles because of ◀2
```

*`api`, as built.* `main!` is unchanged: `serve!` is the SDK's, and
`http_handler` is plain std code. The build adds the codecs and the body
of `handle_call!` for `PgUsers`:

```text
# hypothetical syntax; added to the api executable
BUILD_ID := "3f9a"

# B5: the same codecs as web's

# B2: handle_call! for Home[PgUsers], because main serves home(PgUsers { ... }) (◀3)
fn handle_call_PgUsers!(home: &mut Home[PgUsers], frame: List[u8]) -> List[u8]:
    let input = Reader::new(frame)
    match wire_decode_frame_head(input):                  # untrusted input from here on
        .Err(_) => refuse(CrossError.WrongHome)
        .Ok(head) if head.build != BUILD_ID => refuse(CrossError.OtherBuild)
        .Ok(head) if head.object != 0 => home.call_served!(head, input)   # an object a method returned
        .Ok(head) => match head.method:
            0 => match wire_decode_i64(input):
                .Ok(id) => answer(wire_encode_Result_User_UserError(home.root.load_user!(id)))
                .Err(_) => refuse(CrossError.WrongHome)
            _ => refuse(CrossError.WrongHome)
```

`web` holds no `PgUsers`, no SQL, no driver and no `handle_call!`; `api`
holds no stub. Method 0 is `load_user` by its place in the trait; both
executables come from one build, so they number it alike.

*A closure to a domain of the same target (B4).* Program 1, the
thumbnail on a worker, as written:

```text
# hypothetical syntax; as written
pub fn make_thumb!(photo: Image) -> Result[Image, CrossError] $ Workers:
    let mut workers = $.use(Workers)
    let (back, small) = workers.run!(fn() -> (Image, Image): thumbnail(photo, 256))?
    keep(back)
    .Ok(small)
```

A worker runs the same Wasm module as the page. So the build lifts the
closure to a top-level function, gives it an index, and sends the index
and the encoded captures:

```text
# hypothetical syntax; as built, in the one module the page and every worker load
fn closure_0(photo: Image) -> (Image, Image): thumbnail(photo, 256)    # captures become parameters

pub fn make_thumb!(photo: Image) -> Result[Image, CrossError] $ Workers:
    let mut workers = $.use(Workers)
    let args: mut Bytes = Bytes::new()
    wire_encode_Image(args, photo)                  # photo moves: its pixels are copied once, in bulk
    reply := workers.call!(closure=+0, args=args)?  # with BUILD_ID
    let (back, small) = wire_decode_Image_Image(Reader::new(reply)).unwrap()
    keep(back)
    .Ok(small)

# the worker's side: every instance has the same table
fn run_closure(index: u32, args: Bytes) -> Bytes:
    match index:
        0 => wire_encode_Image_Image(closure_0(wire_decode_Image(Reader::new(args)).unwrap()))
        _ => panic("no closure $index in this build")
```

Here decoding is trusted: the bytes come from the same build, checked by
the build ID, so a failure is a bug and panics. A `Jobs` closure is built
the same way, and the store keeps the index, the captures and the build
ID.

**Running it.** One call:

1. `on_open!(7)` reaches ◀6; `users` is the stub.
2. The stub writes a frame `{build: 3f9a…, object: 0, method: 0, args: [7]}`,
   and its `HttpTransport` posts it to `https://api.example.com/rpc`
   through `Http`, which the `Http = ["api.example.com"]` grant allows.
3. On the server, the SDK's `serve!` accepts the request and calls
   `http_handler`, which reads the body and calls `handle_call!`. That
   checks the build ID, decodes `7`, and calls `PgUsers.load_user!(7)`,
   which queries the database through `Net`. It encodes
   `Ok(User { id: 7, name: "Ada" })`, and `http_handler` returns it as
   the response body.
4. The browser decodes it, and the match takes `.Ok`.

When it fails: with the network down, the transport returns
`Unreachable`, and ◀2 makes it `.Err(UserError.Cross(Unreachable))`.
After a deploy, the server refuses with `OtherBuild`. When the reply is
lost, the outcome is `Unknown`, and the second arm handles it.

Requests run concurrently. The SDK's workers share the one state, here
the `Home[PgUsers]`
([`std-server-sdk.serve-state`](../../spec/std/server-sdk.md#r-std-server-sdk.serve-state)),
so requests interleave at its suspension points (`query_one!` here). The
SDK already states that a mutable reference gives no exclusion across a
suspension. An object that must take one call at a time queues its calls
itself.

Every domain kind, in the same terms:

| Domain | A crossing | Failure and cancel |
| --- | --- | --- |
| `Scope` | a thread on the same memory runs the closure; the scope joins | none |
| `Background` | a task goes to a thread of the same memory; the caller suspends, and the answer wakes it on its own event loop | none |
| `Workers` | a second instance of the same module gets the encoded closure and captures | `Unknown` on a crash; cancel posts an abort |
| `Jobs` | the store keeps the encoded closure, captures and build ID; a host of the same build runs it later | the job's retry policy; `OtherBuild` after a deploy |
| server | the stub's transport sends the build ID, object, token, method number and arguments; `handle_call!` refuses another build or a token it did not issue, validates, calls | `Unknown` when the call may have run; cancel is seen at the server's next suspension |
| GPU | the stub launches the kernel with its `Served[Texture]` arguments | `Unknown` on a reset; `WrongHome` for another GPU's texture |

## 6. Cost

| Piece | Tier | New |
| --- | --- | --- |
| `Send`, `Wire`, `Remote` | language: markers the compiler checks | structural checks, as `Copy`'s |
| `std.remote` (`Served[T]`, `CrossError`, `Transport`, `Home`, `remote`, the HTTP adapters), the domain types, `scope` | stdlib; `handle_call!`'s body and each `impl T for Served[T]` are generated | stub and dispatcher generation per `Remote` trait; tokens and leases |
| targets | CLI and driver | none in `hd.toml`: an executable is a target; one walk per executable |
| threads with shared memory and atomic counts | runtime | Wasm threads; only `Scope` and `Background` need them |
| worker instances, a service transport, a jobs store, a device backend | runtime | one slice each |

The language part is three markers. The std part is `Served[T]`,
`CrossError`, the domain types and two generated intrinsics. Every domain
kind is ordinary std code over them, so a new kind (a Redis node, a
WebRTC peer) is a library, not a language change.

Not in this design, each its own later design: a server's per-request
context (the caller's identity, which a `Remote` method cannot take as a
row), the device-code subset and launch geometry, and supervision of
long-lived served objects.

## Appendix A. Stress Test

Eighteen cases, each the public shape of a well-known library or a
crossing operation, rewritten in this design. Shapes are approximations.
A case *works*, has *friction* (it works with a workaround or a visible
difference from the original), or *breaks* (it cannot be written, or
behaves differently). Blocks are parsed, not type-checked.

| # | Case (library) | Verdict | What the design does |
| --- | --- | --- | --- |
| 1 | Worker thumbnail (Comlink, transferables) | works | the `impl !Copy` image moves; its bytes cross once in bulk, where JavaScript hands a buffer over with no copy |
| 2 | Form action (Next.js "use server") | works | a `Served[Call[A, T, E]]`; the captured id stays on the server, and the token is bound to the page's session |
| 3 | Typed service (tRPC, gRPC) | friction | the implementation has no per-request context: it cannot see the caller |
| 4 | Scoped count and sort (`thread::scope`, rayon) | works | borrows in a `Scope`; a spawn that writes a capture is rejected |
| 5 | Off the UI thread (Kotlin, Swift) | works | `background.run!` returns `T`; code that uses `$ Ui` does not fit `Background`'s row |
| 6 | Delayed mail (Sidekiq, Temporal) | friction | a deploy fails the jobs queued before it with `OtherBuild`, so a daily deploy rules out a job a week out |
| 7 | GPU kernels (wgpu, CUDA) | friction | the device's `Home` serves the kernels; the device-code subset and launch geometry are left to their own design |
| 8 | Data-local count (Ray, Spark) | works | `workers.at(rows)` runs the code at `rows`'s home, where calls on `rows` are local |
| 9 | Long-lived process (Erlang, Akka) | works | an account is a `Served[Account]` returned by a `Remote` method; supervision is library work |
| 10 | A test swaps the server | works | stub, real implementation and fake are all providers of `Users` |
| 11 | Error propagation | works | `E < From[CrossError]` with one `@from` variant lets `?` carry a lost contact |
| 12 | Crossings in `join!` | works | two bang calls on two domain values join as any two calls do |
| 13 | Cancel mid-crossing | works | the cancel makes the outcome `Unknown`; a moved value never returns through an error |
| 14 | A remote call inside a remote call | works | the implementation holds the other stub as a field and maps its `CrossError` |
| 15 | Remote with a closure or a served object | works | a closure parameter is rejected by R1; a `Served[T]` parameter crosses to its home |
| 16 | Generic remote function | friction | a generic method is rejected; a generic trait served at one type works |
| 17 | Streaming results | friction | a stream is a `Served[Next[T]]` pulled one item per round trip; server push needs a batching cursor |
| 18 | A background result updates UI | works | the handler resumes on the UI thread; a closure that captured the UI state's box is not `Send` |

Totals: 13 works, 5 friction, 0 breaks. Two frictions are left to their
own design (cases 3 and 7). One is the price of refusing another build
(case 6). Two are costs of the rules on purpose: R1 bars generic methods
(case 16), and a served object answers one call per round trip (case 17).

Adversarial checks, each with the rule that stops it:

| Attempt | Stopped by |
| --- | --- |
| two spawns each run `total += n` | a closure that writes a capture is not `Send` |
| a closure reaches a `let mut` top-level counter, or a top-level box | reached top-level bindings must be immutable and `Wire` |
| a scoped thread gets a box | a box is not `Send` |
| a caller passes a closure to a `Remote` method, so another target runs code it picks | R1: parameters are not closures |
| a page edits a served object's id to reach another user's action | the token is unguessable and bound to the session: `WrongHome` |
| a browser closes without dropping a served object | the lease expires at its home |
| a kernel gets a texture of another GPU | `WrongHome` before it runs |
| a client of yesterday's build calls today's server | `OtherBuild` before any code runs |
| a closure captures a provider value | a provider value is neither `Send` nor `Wire` |
| a moved value is used after the crossing fails | the source binding ended: `use-of-moved-value` |

**Case 4: the rejected write.**

```text
# hypothetical syntax; lines: &List[string]
let total = +0
counts := scope(fn(s: &mut Scope) -> List[i32]:
    tasks := [for i in 0..4 => s.spawn(fn() -> i32: count_quarter(lines, i))]
    s.spawn(fn() -> i32: total += 1)         # error: unsatisfied-trait-bound, a closure that writes total is not Send
    [for t in tasks => t.join()]
)
```

**Case 9: an account process.** A worker serves a `Bank`, and each
account it opens is a served object of its own.

```text
# hypothetical syntax; an account process on a worker, as an Erlang process
pub trait Bank < Remote:
    fn open_account!(&mut self) -> Result[Served[Account], AccountError]

pub trait Account < Remote:
    fn deposit!(&mut self, cents: i64) -> Result[i64, AccountError]

struct Ledger:
    cents: i64

impl Account for Ledger:
    fn deposit!(&mut self, cents: i64) -> Result[i64, AccountError]:
        self.cents += cents
        .Ok(self.cents)

pub fn first_deposit!() -> Result[i64, AccountError] $ Workers:
    let mut workers = $.use(Workers)
    let mut bank = remote::[Bank](workers.pick())
    let mut account = bank.open_account!()?        # account: Served[Account], home: that worker
    account.deposit!(500)
```

**Cases 15 and 16: what R1 rejects.**

```text
# hypothetical syntax
pub trait Sorter < Remote:
    fn sort_by!(&mut self, rows: List[Row], key: fn(Row) -> i64) -> Result[List[Row], SortError]   # error: R1, a closure parameter

pub trait Thumbs < Remote:
    fn shrink!(&mut self, tex: Served[Texture], side: u32) -> Result[Served[Texture], GpuError]   # valid: crosses to its home

pub trait Store < Remote:
    fn get![T < Wire](&mut self, key: string) -> Result[T, StoreError]   # error: R1, a generic method

pub trait TypedStore[T < Wire] < Remote:
    fn get!(&mut self, key: string) -> Result[T, StoreError]             # valid: home(UserStore {}) fixes T = User
```

**Case 17: a stream pulled item by item.**

```text
# hypothetical syntax
pub trait Feed < Remote:
    fn open!(&mut self, topic: string) -> Result[Served[Next[Event]], FeedError]

pub fn watch!(topic: string) -> Result[void, FeedError] $ Feed + Console:
    let mut feed = $.use(Feed)
    let mut events = feed.open!(topic)?
    while true:
        match events.next!()?:
            .Some(event) => println("$event")
            .None => break
    .Ok(())
```

## Appendix B. Prior Art

Each snippet is abridged from its source.

**Rust `Send`, `Sync` and serde.** `thread::spawn` requires `Send`, and
serde's encoders require `Serialize`. hd takes the same split as `Send`
for shared memory and `Wire` for separate memory. It needs no `Sync`,
because it has no `Cell` or `RefCell`.

```rust
let total = std::thread::scope(|s| {
    let a = s.spawn(|| count(left));
    count(right) + a.join().unwrap()
});
```

**gRPC and tRPC.** A service is declared once, the server registers an
implementation, and a client calls a generated stub. hd takes this as a
`Remote` trait, a `Home` and `remote::[T]`.

**Swift distributed actors (SE-0336).** A `distributed func` is the
remote surface, and every call to it is `try await`. hd marks the surface
with `Remote` and the call with `!` and a declared error type.

**Swift `sending` (SE-0430).** A value nothing else reaches moves to
another isolation domain, and a later use is an error. hd takes this as
the move of an `impl !Copy` value.

**Cap'n Proto, Erlang, Orleans.** A capability, a pid and a grain
reference each name an object that one place serves; every call goes
there. hd takes this as `Served[T]`.

**Chapel `on`.** `on x do` runs where `x` lives. hd takes it as
`workers.at(rows)`, with the home a run-time value.

**ML5 and ScalaLoci.** Types name where a value lives. hd keeps the kind
in the type (`Texture`) and the instance at run time, because a program
holds any number of GPUs, workers or caches.

```scala
@peer type Server <: { type Tie <: Multiple[Client] }
val items: Items on Server = /* ... */
```

**Spores.** A closure whose captures are checked so it can be sent. hd
checks captures through the `Send` and `Wire` bounds.

**Unison.** Code is identified by content hash across builds. hd does
not need it: every domain runs the same program, and another build is
refused with `OtherBuild`.

## Appendix C. Alternatives Considered

| Alternative | Why it is not the design |
| --- | --- |
| `@remote(D)` on functions plus `$ D` in callers' rows | two shapes for one concept, beside `workers.run!(...)` |
| a keyword, `on D: call` | a new keyword for what a method call already says |
| domain instances in types, `Texture[Gpu0]` | too rigid: a program holds many GPUs, workers and caches, known only at run time |
| six domain kinds, a kind-by-mode table, lifecycle rules L1 to L4 | they follow from the bounds, moves, scopes and served objects |
| content hashes, so another build can run a call | out of scope; `OtherBuild` names the refusal |
| a separate `Kernel` marker for devices | a device is a domain of another target, so `Remote` covers it |
| results widened to `E \| CrossError` | an implicit type change; `E < From[CrossError]` keeps one declared error |
| a `Handle` marker beside `Remote`, and a separate `Action` type | a handle is a `Served[T]`: one kind of reference for services, textures, actions, streams and processes |
| `Sync` beside `Send` | hd writes through `&` only via a box, which is not `Send`, so `&T` is `Send` when `T` is |
| `try_run!` beside `run!` | `?` already converts a `CrossError` through `From`, so `run!(...)??` does it |
| `CrossError` grouped as `NotRun(reason)` and `Unknown` | the same four facts one pattern level deeper; flat cases read as gRPC status codes do |
| `Send` merged into `Wire` | a scoped thread borrows (`&T` is not `Wire`), and a background thread shares a buffer instead of copying it |
| domain types without a row parameter | the row is what keeps `$ Ui` off a background thread |
| `Background` merged into `Scope` | their lifetimes differ: `'static` against `'s` |
| `WrongHome` as a panic | a server refusing a forged token must answer the caller, not crash |
| a target answers calls to every provider it installs | a `Home` is the explicit list of what another target may call |

The parked #166 study
([SERIALIZABLE_CLOSURE_DESIGN.md @ 28072b8a1](https://github.com/HerringtonDarkholme/hd-lang/blob/28072b8a1/docs/future-work/SERIALIZABLE_CLOSURE_DESIGN.md))
maps onto this design:

| #166 | This design |
| --- | --- |
| `@remote` functions and portable closure literals | `Remote` traits; closures only within one target |
| captures must be boundary-safe | the `Wire` bound |
| placement by requirement rows and `[places]` | providers installed by each target's `main` |
| `Opaque[T]` for server state | a `Served[T]` |
| `RemoteError` with `OutcomeUnknown` | `CrossError.Unknown` |

## Sources

- "Types and Transport Across Execution Boundaries" (gist, 2026-10-31), and the review of PR #1139.
- #166 study: <https://github.com/HerringtonDarkholme/hd-lang/blob/28072b8a1/docs/future-work/SERIALIZABLE_CLOSURE_DESIGN.md>; [Parallelism And Locality](PARALLELISM_AND_LOCALITY.md).
- Rust, `Send` and `Sync`: <https://doc.rust-lang.org/std/marker/trait.Send.html>; `thread::scope`: <https://doc.rust-lang.org/std/thread/fn.scope.html>
- serde, `Serialize`: <https://docs.rs/serde/latest/serde/trait.Serialize.html>
- gRPC, services and stubs: <https://grpc.io/docs/what-is-grpc/core-concepts/>; status codes: <https://grpc.github.io/grpc/core/md_doc_statuscodes.html>
- tRPC: <https://trpc.io/docs/>
- Swift SE-0336, distributed actor isolation: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0336-distributed-actor-isolation.md>
- Swift SE-0430, `sending`: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0430-transferring-parameters-and-results.md>
- Chapel, on clauses: <https://chapel-lang.org/docs/users-guide/locality/onClauses.html>
- Murphy, Crary, Harper, "Type-Safe Distributed Programming with ML5", TGC 2007: <https://kilthub.cmu.edu/articles/journal_contribution/Type-Safe_Distributed_Programming_with_ML5/6611192/1>
- ScalaLoci: <https://scala-loci.github.io/>
- Miller, Haller, Odersky, "Spores", ECOOP 2014: <https://doi.org/10.1007/978-3-662-44202-9_13>
- Unison, distributed datasets: <https://www.unison-lang.org/articles/distributed-datasets/>
- Temporal, activity timeouts: <https://docs.temporal.io/encyclopedia/detecting-activity-failures>
- Erlang, `monitor/2`: <https://www.erlang.org/doc/apps/erts/erlang.html#monitor/2>

## Parse Log

All 27 `text` blocks were parsed on 2026-11-02 with
`scripts/hd-conformance.sh parse`, which checks syntax only; nothing was
type-checked. Std names (`Workers`, `Background`, `Jobs`, `Gpus`,
`scope`) are sketches; `std.remote` is the proposed surface.

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
| 9 | Code For Another Target: Remote And Served | parses; `# hypothetical syntax` |
| 10 | What Std Adds | parses after two fixes of mine: an empty trait body is `pass` on its own line, and an intrinsic is written `@intrinsic` with a `panic` body, as `std.task` does |
| 11 | 4. The Eight Programs In The Design | parses; `# hypothetical syntax` |
| 12 | 4. The Eight Programs In The Design | parses; `# hypothetical syntax` |
| 13 | 4. The Eight Programs In The Design | parses; `# hypothetical syntax` |
| 14 | 4. The Eight Programs In The Design | parses; `# hypothetical syntax` |
| 15 | 4. The Eight Programs In The Design | parses; `# hypothetical syntax` |
| 16 | 4. The Eight Programs In The Design | parses; `# hypothetical syntax` |
| 17 | 4. The Eight Programs In The Design | parses; `# hypothetical syntax` |
| 18 | 4. The Eight Programs In The Design | parses; `# hypothetical syntax` |
| 19 | 5. The Process, End To End | parses; `# hypothetical syntax` |
| 20 | 5. The Process, End To End | parses; `# hypothetical syntax` |
| 21 | 5. The Process, End To End | parses; `# hypothetical syntax` |
| 22 | 5. The Process, End To End | parses; `# hypothetical syntax` |
| 23 | 5. The Process, End To End | parses; `# hypothetical syntax` |
| 24 | Appendix A. Stress Test | parses; `# hypothetical syntax` |
| 25 | Appendix A. Stress Test | parses; `# hypothetical syntax` |
| 26 | Appendix A. Stress Test | parses; `# hypothetical syntax` |
| 27 | Appendix A. Stress Test | parses; `# hypothetical syntax` |
