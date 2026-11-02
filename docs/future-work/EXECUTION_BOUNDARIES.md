# Execution Domains: Moving Code And Data

Status: proposal, 2026-11-02; domain: language (execution domains and
crossings) and std; not accepted. Nothing here is in the specification.
The round-1 review is [EXECUTION_BOUNDARIES_STRESS_TEST.md](EXECUTION_BOUNDARIES_STRESS_TEST.md).

- [1. Context And Goal](#1-context-and-goal)
- [2. Eight Programs That Cross](#2-eight-programs-that-cross)
- [3. The Design](#3-the-design)
- [4. The Eight Programs In The Design](#4-the-eight-programs-in-the-design)
- [5. The Process, End To End](#5-the-process-end-to-end)
- [6. Cost](#6-cost)
- [Appendix A. Prior Art](#appendix-a-prior-art)
- [Appendix B. Alternatives Considered](#appendix-b-alternatives-considered)
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

## 3. The Design

The language adds five things and no syntax. Everything else is existing
hd (rows and providers, closures, moves, scopes) or std library.

| # | Part | Its one job |
| --- | --- | --- |
| 1 | `Send`, `Sync`, `Wire` | what may cross: into shared memory, or into separate memory |
| 2 | `Handle` | what stays where it is, and goes only home |
| 3 | `Remote` | the only code that runs in another build |
| 4 | `CrossError` | how a crossing fails |
| 5 | one build per target | how each domain's output is made |

A **crossing** is a bang method call on a value that stands for the
destination: a domain value, a handle, or a stub of a `Remote` interface.
Nothing else crosses. A bare function call never does.

### Domains Are Values

An execution domain is a place code runs: a thread, a worker instance, a
server, a GPU, a run of a job queue. Code never names one by type. It
holds a **domain value**, which it gets from a provider in its row, so a
test swaps it with `$.with`. A program can hold any number of them: a
pool of workers, every GPU on the machine, a Redis node per shard.

| Property | Values | Decides |
| --- | --- | --- |
| Memory | shared with the caller, or separate | the bound: `Send` or `Wire` |
| Build | the caller's build, or another one | closures, or `Remote` methods only |
| Contact | cannot be lost, or can | the result: `T`, or `Result[T, CrossError]` |

The domain kinds are std library types, built on the five parts:

| Std type | Memory | Build | Contact | Code it takes |
| --- | --- | --- | --- | --- |
| `Scope['s]` (scoped threads) | shared | same | cannot be lost; joins before `'s` ends | a closure, `Send`, may borrow for `'s` |
| `Background` | shared | same | cannot be lost | a closure, `Send`, `'static` |
| `Workers`, `Worker` | separate | same | can be lost | a closure, `Wire`, `'static` |
| `Jobs` | separate, later | same | nobody waits | a closure, `Wire`, `'static` |
| a server address | separate | another | can be lost | `Remote` methods |
| `Gpus`, `Gpu` | separate | another (device code) | can be lost | `Remote` methods |

Each domain type also states the capabilities it offers as a row
parameter, `Workers[$ Log + Clock]`, and code whose row asks for more does
not fit, through ordinary row subsumption
([`req.row.subsume.missing`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.subsume.missing)).

A panic in code on a shared-memory thread is a panic of the program, as
today. So the two thread kinds return a plain `T`, and only domains that
can lose contact return a `CrossError`.

### What May Cross: Send, Sync, Wire

Three std marker traits, checked structurally as `Copy` is
([`ref.copy.marker`](https://github.com/HerringtonDarkholme/hd-lang/blob/9af95bd2f/spec/lang/13-values-and-references.md#r-ref.copy.marker)):

- **`Wire`:** a value encoded as a self-contained tree, sent into separate
  memory. This names today's boundary-safe values
  ([`module.boundary.allowed`](../../spec/lang/10-modules.md#r-module.boundary.allowed)), plus handles (below).
- **`Send`:** a value moved to another thread of the same memory. Every
  `Wire` value is `Send`.
- **`Sync`:** `&T` may be shared across threads, so `&T` is `Send` exactly
  when `T < Sync`.

| Type | `Wire` | `Send` | `Sync` |
| --- | --- | --- | --- |
| numbers, `bool`, `char`, `string` | yes | yes | yes |
| struct, enum, tuple, `List`, `Map`, `Set`, optional, `Result` | when every part is | when every part is | when every part is |
| a handle | yes, as its home and id | yes | no |
| `&T` | no | when `T < Sync` | when `T < Sync` |
| `&mut T` | no | when `T < Send` | when `T < Sync` |
| `*mut T`, a box | no | no | no |
| a trait value, a provider value | no | no | no |
| a closure | when its captures are `Wire` | when its captures are `Send` and it writes none | when its captures are `Sync` |

Two rules complete it for closures, the code a same-build domain runs:

- A closure that writes a captured variable is not `Send`. Captures share
  a cell with their scope
  ([`fn.capture.cell.writes`](../../spec/lang/07-functions.md#r-fn.capture.cell.writes)), so two spawns doing `total += n`
  would race. They are rejected; each returns its count instead.
- A closure is `Send` or `Wire` only when every top-level binding it
  reaches, directly or through a callee, is immutable and `Wire`. Module
  storage is per instance
  ([`module.init.program-instance`](../../spec/lang/10-modules.md#r-module.init.program-instance)), so a `let mut` counter or a
  top-level box would change only one domain's copy.

A copy-on-write `List` is `Send`: sending it shares the buffer, and a
later write by either side copies. With threads, buffer counts become
atomic. Rust states the same split with `Send` on `thread::spawn` and
serde's `Serialize` on encoders.

Mode is never written. The value's type decides it: a `Copy` value copies,
an `impl !Copy` value moves and its source binding ends
([`ref.move.use`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.use)), a reference lends inside a `Scope`, and a
handle stays home.

### What Stays: Handles

A **handle** refers to a resource that lives in one domain instance: a GPU
texture, a worker's cache, a server action. Its type implements the std
marker `Handle` and names the kind (`Texture`, `Action[string, void,
TodoError]`). The instance it lives in, its **home**, is a run-time value
recorded when the handle is made.

- A handle may be held anywhere and crosses as its home and id.
- A crossing may send a handle only to its home. The runtime checks this
  when the call leaves, before any code runs, and refuses a mismatch with
  `CrossError.WrongHome`. A handle in a result comes back to the caller,
  who holds it.
- Every operation on a handle is a crossing to its home, so code away
  from home can pass a handle on but never touch the resource.
- The home keeps the resource alive until the last holder drops its
  handle, as a counted reference keeps a box alive
  ([`ref.box.lifetime`](../../spec/lang/13-values-and-references.md#r-ref.box.lifetime)).

A server action is a std handle, `Action[A, T, E]`: a closure that its
home serves as an object of a std `Remote` trait. Its captures stay home,
so they need no bound, and the caller never sees them.

"Code goes to the data" is then library: `workers.at(rows)` returns the
worker that is `rows`'s home, so a crossing aimed there passes the home
check by construction, as Chapel's `on x do` runs where `x` lives.

### Code For Another Build: Remote

A domain of another build runs only methods of an interface trait that
extends the std marker `Remote`. Those methods are its endpoints, and
nothing else of the program is reachable from outside.

- **R1.** Every method is a bang method whose parameters and result are
  `Wire`. It is not generic and has no row; its implementation gets what
  it needs from its own fields.
- **R2.** Its error type `E` satisfies `E < From[CrossError]`, so a lost
  contact arrives as one of the method's own errors. An `@error` enum gets
  that from one variant, `Cross(@from cause: CrossError)`
  ([`annot.error.generates`](../../spec/lang/14-annotations.md#r-annot.error.generates)).
- **R3.** The destination installs the implementation with `serve!(impl)`.
  A caller installs a generated stub, `remote::[T](domain)`, for any domain
  value: a server address, a GPU, a worker. A test installs a fake. All
  three are ordinary providers of `T`.

```text
# hypothetical syntax
pub trait Users < Remote:
    fn load_user!(&mut self, id: i64) -> Result[User, UserError]

pub trait Filters < Remote:                    # served on a GPU: device code
    fn blur!(&mut self, texture: Texture, radius: f32) -> Result[void, GpuError]
```

A `Remote` implementation that a device target compiles must be device
code: no allocation and no host calls. That subset gets its own design.

### How A Crossing Fails: CrossError

`CrossError` is a std enum with four cases:

| Case | Meaning | Did it run? |
| --- | --- | --- |
| `Unreachable` | the call never left | no |
| `Unknown` | it left and no answer came, including a timeout after sending, or the destination crashed | maybe |
| `OtherBuild` | the destination runs another build, as after a deploy, and refused | no |
| `WrongHome` | the call carried a handle whose home is not the destination, and was refused | no |

The error type that carries it is always one the code declares:

- A same-build domain's `run!` returns `Result[T, CrossError]` when
  contact can be lost; `try_run!` takes code that returns `Result[T, E]`
  with `E < From[CrossError]` and gives a flat `Result[T, E]`.
- A `Remote` method's error type holds it through R2.

Cancelling a crossing, by abandoning the task that waits on it
([`req.abandon.host`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.host)), sends a cancel; the call's outcome is then
`Unknown`. A moved value never comes back through an error, so an unknown
outcome never returns ownership.

### One Build Per Target

Every domain runs the same program (section 1). The deployment lists its
targets, each an entry module, a runtime profile (what its host binds)
and an output: Wasm for a browser, a server or a worker, device code for a
GPU.

- **B1.** Each target walks the use graph and the call graph from its
  entry and compiles only what it reaches. A module only the server
  reaches, such as one that uses a database driver, is never parsed for
  the browser.
- **B2.** `serve!(impl)` in a target compiles the implementation and a
  generated dispatcher: decode a method number and arguments, validate
  them as untrusted input, call, encode the result.
- **B3.** `remote::[T](domain)` in a target compiles a generated stub:
  encode, send, decode, and map a transport failure to `CrossError`. It
  holds no implementation code.
- **B4.** A same-build crossing compiles into the one module. Its code is
  named by the closure's lifted function, plus its encoded captures.
- **B5.** Every `Wire` type gets a generated codec, the tree encoding of
  boundary values ([`module.boundary.tree`](../../spec/lang/10-modules.md#r-module.boundary.tree)). A build ID travels
  with every crossing, so a peer of another build refuses with
  `OtherBuild` before any code runs.

## 4. The Eight Programs In The Design

**1. Thumbnail in a worker.** `Photo` is `impl !Copy` with `Wire` fields,
so it moves: the 50 MB buffer is handed over, and `photo` is unusable
until the result returns it.

```text
# hypothetical syntax; workers: the Workers provider
let mut workers = $.use(Workers)
let (back, small) = workers.run!(fn() -> (Photo, Photo): thumbnail(photo, 256))?
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

pub fn on_buy!(cart: Cart) -> void $ Dom + Orders:
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
jobs.after!(minutes(10), fn() -> Result[void, MailError] $ Mailer: send_welcome!(email, name))?
```

**4. GPU filters.** The device holds the texture; every kernel call is a
crossing to its home.

```text
# hypothetical syntax; gpus: the Gpus provider
let mut gpu = gpus.device(0)?
tex := gpu.upload!(photo)?                      # tex: Texture, home: this GPU
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

**7. Server action.** The server returns a handle whose home is the
server; the browser calls it later, and the captured id never leaves.

```text
# hypothetical syntax; rename: Action[string, void, TodoError]
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

**Writing it.** Same-build work needs no declaration: hand a closure to
the domain's method. Work in another build is a `Remote` interface,
implemented and served there, and called through a stub here.

```text
# hypothetical syntax
# src/users.hd: shared by both sides
pub trait Users < Remote:
    fn load_user!(&mut self, id: i64) -> Result[User, UserError]

# src/server.hd: the only module that uses the driver
struct PgUsers:
    pool: Pool

impl Users for PgUsers:
    fn load_user!(&mut self, id: i64) -> Result[User, UserError]:
        match self.pool.query_one!("select id, name from users where id = ?", id):
            .Some(row) => .Ok(User { id: row.i64(0), name: row.text(1) })
            .None => .Err(UserError.Missing(id))

pub fn server_main!() -> Result[void, PgError] $ Console + Net:
    pool := connect!("postgres://localhost/app")?
    serve!(PgUsers { pool })

# src/browser.hd
pub fn browser_main!() -> void $ Dom + Net:
    $.with(Users=remote::[Users](server("https://api.example.com"))):
        on_open!(7)

pub fn on_open!(id: i64) -> void $ Dom + Users:
    let mut users = $.use(Users)
    match users.load_user!(id):
        .Ok(user) => show("Hi, ${user.name}")
        .Err(error) => show_error("$error")
```

**Reading it.** A line crosses only when it is a bang method on a domain
value or on a provider of a `Remote` trait. The function's row names
every domain it uses (`$ Users`), and the error type shows contact can be
lost. A remote provider call and a local one look alike at the call site;
the trait tells them apart, as a gRPC stub and a local object do.

**Checking it.** Three places:

| Where | What | Error |
| --- | --- | --- |
| a `Remote` trait | R1 and R2 | the method and the rule |
| each crossing call | the domain method's bound on the code and every value; the code's row against the domain's row parameter | `unsatisfied-trait-bound`, `type-mismatch` |
| same-build code's body | `$.use` resolves at the destination, never to a captured provider | `unsatisfied-trait-bound`: a provider value is not `Wire` |

The handle home check is the one run-time check: `WrongHome`, before the
call runs.

**Building it.** B1 to B5. For the program above:

| | browser build | server build |
| --- | --- | --- |
| `users.hd` | the trait | the trait and the dispatcher |
| `server.hd` and the driver | not reached | yes |
| the `remote::[Users]` stub | yes | no |
| `on_open` | yes | not reached |

**Running it.**

| Domain | A crossing | Failure and cancel |
| --- | --- | --- |
| `Scope` | a thread on the same memory runs the closure; the scope joins | none |
| `Background` | a task goes to a thread of the same memory; the caller suspends, and the answer wakes it on its own event loop | none |
| `Workers` | a second instance of the same module gets the encoded closure and captures | `Unknown` on a crash; cancel posts an abort |
| `Jobs` | the store keeps the encoded closure, captures and build ID; a host of the same build runs it later | the job's retry policy; `OtherBuild` after a deploy |
| server | the stub sends the build ID, method number and arguments; the dispatcher refuses another build, validates, calls | `Unknown` when the call may have run; cancel is seen at the server's next suspension |
| GPU | the stub launches the kernel with its handles | `Unknown` on a reset |

## 6. Cost

| Piece | Tier | New |
| --- | --- | --- |
| `Send`, `Sync`, `Wire`, `Handle`, `Remote` | language: markers the compiler checks | structural checks, as `Copy`'s |
| `CrossError`, the domain types, `serve!`, `remote::[T]`, `scope` | stdlib, with `serve!` and `remote::[T]` as generated intrinsics | stub and dispatcher generation per `Remote` trait |
| per-target builds | CLI and driver | a target table; one walk per target |
| threads with shared memory and atomic counts | runtime | Wasm threads; only `Scope` and `Background` need them |
| worker instances, a service transport, a jobs store, a device backend | runtime | one slice each |

The language part is five markers and two generated intrinsics. Every
domain kind is ordinary std code over them, so a new kind (a Redis node,
a WebRTC peer) is a library, not a language change.

## Appendix A. Prior Art

Each snippet is abridged from its source.

**Rust `Send`, `Sync` and serde.** `thread::spawn` requires `Send`, and
serde's encoders require `Serialize`. hd takes the same split as `Send`
for shared memory and `Wire` for separate memory.

```rust
let total = std::thread::scope(|s| {
    let a = s.spawn(|| count(left));
    count(right) + a.join().unwrap()
});
```

**gRPC and tRPC.** A service is declared once, the server registers an
implementation, and a client calls a generated stub. hd takes this as a
`Remote` trait, `serve!` and `remote::[T]`.

**Swift distributed actors (SE-0336).** A `distributed func` is the
remote surface, and every call to it is `try await`. hd marks the surface
with `Remote` and the call with `!` and a declared error type.

**Swift `sending` (SE-0430).** A value nothing else reaches moves to
another isolation domain, and a later use is an error. hd takes this as
the move of an `impl !Copy` value.

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

## Appendix B. Alternatives Considered

| Alternative | Why it is not the design |
| --- | --- |
| `@remote(D)` on functions plus `$ D` in callers' rows | two shapes for one concept, beside `workers.run!(...)` |
| a keyword, `on D: call` | a new keyword for what a method call already says |
| domain instances in types, `Texture[Gpu0]` | too rigid: a program holds many GPUs, workers and caches, known only at run time |
| six domain kinds, a kind-by-mode table, lifecycle rules L1 to L4 | they follow from the bounds, moves, scopes and handles |
| content hashes, so another build can run a call | out of scope; `OtherBuild` names the refusal |
| a separate `Kernel` marker for devices | a device is a domain of another build, so `Remote` covers it |
| results widened to `E \| CrossError` | an implicit type change; `E < From[CrossError]` keeps one declared error |

The parked #166 study
([SERIALIZABLE_CLOSURE_DESIGN.md @ 28072b8a1](https://github.com/HerringtonDarkholme/hd-lang/blob/28072b8a1/docs/future-work/SERIALIZABLE_CLOSURE_DESIGN.md))
maps onto this design:

| #166 | This design |
| --- | --- |
| `@remote` functions and portable closure literals | `Remote` traits; closures only within one build |
| captures must be boundary-safe | the `Wire` bound |
| placement by requirement rows and `[places]` | providers installed by each target's `main` |
| `Opaque[T]` for server state | a handle |
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

All 18 `text` blocks were parsed on 2026-11-02 with
`scripts/hd-conformance.sh parse`, which checks syntax only; nothing was
type-checked. Std names (`Workers`, `Background`, `Jobs`, `Gpus`,
`Remote`, `serve!`, `remote::[T]`, `scope`) are sketches.

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
| 9 | Code For Another Build: Remote | parses; `# hypothetical syntax` |
| 10 | 4. The Eight Programs In The Design | parses; `# hypothetical syntax` |
| 11 | 4. The Eight Programs In The Design | parses; `# hypothetical syntax` |
| 12 | 4. The Eight Programs In The Design | parses; `# hypothetical syntax` |
| 13 | 4. The Eight Programs In The Design | parses; `# hypothetical syntax` |
| 14 | 4. The Eight Programs In The Design | parses; `# hypothetical syntax` |
| 15 | 4. The Eight Programs In The Design | parses; `# hypothetical syntax` |
| 16 | 4. The Eight Programs In The Design | parses; `# hypothetical syntax` |
| 17 | 4. The Eight Programs In The Design | parses; `# hypothetical syntax` |
| 18 | 5. The Process, End To End | parses; `# hypothetical syntax` |
