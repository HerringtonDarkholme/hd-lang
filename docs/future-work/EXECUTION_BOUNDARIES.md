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
so the borrow cannot outlive the list. A second pass lowercases two
halves in parallel: each half goes to its thread and comes back as the
thread's result, so no thread writes through a shared `&mut`.

```text
pub fn word_count(lines: &List[string]) -> Map[string, usize]:
    q := lines.len() / 4
    # Wanted: four threads borrow `lines`; today an instance has no threads.
    counts := scope(fn(s: &mut Scope) -> List[Map[string, usize]]:
        tasks := [for i in 0..4 => s.spawn(fn() -> Map[string, usize]: count_words(lines, i * q, i * q + q))]
        [for t in tasks => t.join()]
    )
    merge_counts(counts)

pub fn normalize(lines: List[string]) -> List[string]:
    let (left, right) = lines.split_halves()         # left, right: List[string]
    scope(fn(s: &mut Scope) -> List[string]:
        task := s.spawn(fn() -> List[string]: lowercase_all(left))   # wanted: left goes in by value
        done := lowercase_all(right)
        [task.join()..., done...]
    )
```

**6. Reporting dashboard: code moves to the data.** The browser needs the
200 sales that match a filter. Today it downloads a million rows and
filters locally. Only the small `Filter` should cross: the server, where
the rows live, should run the filter and return the 200 rows.

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
unknown outcome (2), code identity within one program (3, 7), a call that runs where the data lives (6), code that stays home while a reference to it travels (7), and
work that outlives its caller in shared memory and returns to a thread
that alone holds a capability (8).

## 3. The Design

The design adds four parts and no syntax. Everything else is existing hd
(rows and providers, closures, moves, scopes, unions, counted trait
references) or std library.

| # | Part | Its one job |
| --- | --- | --- |
| 1 | `Send` and `Wire` | what may cross: into shared memory, or into separate memory |
| 2 | `Remote` | the only code another target runs; a `*mut dyn T` of a `Remote` trait is a reference to an object one domain serves |
| 3 | `CrossError` | how a crossing fails, kept apart from the application's errors |
| 4 | one build, one output per target | how each domain's code is made |

A **build** compiles the whole program once and has one build ID. A
**target** is one executable of the package and its output: Wasm for a
browser, a server or a worker, device code for a GPU.

A **crossing** is a call that runs in another domain. There are two
kinds, and nothing else crosses:

- a method of a domain value that takes code: `workers.run!(closure)`,
  `jobs.after!(delay, closure)`, `s.spawn(closure)`;
- a method of a `Remote` trait called on a `*mut dyn T` whose object lives
  in another domain: `users.load_user!(id)` when `users` is a stub.

The first kind is visible at the call. The second is visible at the
trait's declaration (`< Remote`), in the method's error type, which ends
in `| CrossError`, and at the call as a bang call. Whether one such call
crosses is decided where its reference is installed, usually as a
provider (section 5).

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
| Contact | cannot be lost, or can | the result: `T`, or a result with `CrossError` |

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
  ([`module.boundary.allowed`](../../spec/lang/10-modules.md#r-module.boundary.allowed)),
  plus a `*mut dyn T` of a `Remote` trait `T`.
- **`Send`:** a value copied, moved or lent to another thread of the same
  memory. The two are checked separately: a value can be one and not
  the other, as the table shows.

| Type | `Wire` | `Send` |
| --- | --- | --- |
| numbers, `bool`, `char`, `string` | yes | yes |
| struct, enum, tuple, union, `List`, `Map`, `Set`, optional, `Result` | when every part is | when every part is |
| `*mut dyn T` with `T < Remote` | yes, as its home, interface, object number and token | no: a stub holds its own mutable transport |
| `&T` | no | when `T < Send`; its place is frozen until the scope joins |
| `&mut T` | no | no |
| `*mut T`, a box, any other trait value reference | no | no |
| a provider value | no | no |
| a closure written at the crossing | when its captures are | when its captures are |
| any other value of function type | no | no |

A thread that changes a value never shares it: it gets the value by copy
or move and returns the result, so `&mut T` never crosses. A `&T` lent to
a thread freezes its place until the scope joins: neither its owner nor
an element access writes it. That is stronger than today's loan of an
element, which allows writes to the element while the reference lives
([`ref.elem.access.live-length`](../../spec/lang/13-values-and-references.md#r-ref.elem.access.live-length)).
The one write through a `&`, a box field
([`ref.read.shallow`](../../spec/lang/13-values-and-references.md#r-ref.read.shallow)),
needs a box, which is not `Send`.

A remote reference is a box too. A stub calls through a transport that
holds mutable state, such as an `*mut dyn Http`, so two threads must not
share one. A background thread that calls a server builds its own stub
from a `Wire` URL, with `Http` in its domain's row:
`background.run!(fn!() -> ... $ Http: ...)`.

Four rules say what happens to a closure's captures at a crossing:

- **No cell crosses.** Today a captured `let` variable shares a cell with
  its scope
  ([`fn.capture.cell`](../../spec/lang/07-functions.md#r-fn.capture.cell)).
  A closure handed to a crossing takes each capture as it is at that
  moment instead: a `Copy` value is copied, an `impl !Copy` value moves
  and its source binding ends, so a later use is `use-of-moved-value`
  ([`ref.move.use`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.use)),
  as Swift's `sending` does, and a `&T` is lent for the scope. A parent's
  later write never reaches the child.
- **No capture is written.** A crossing closure that assigns a captured
  variable is an error, since it would change only its own copy. Each
  child returns its part instead. Swift rejects the same mutation in
  concurrently executing code.
- **Only a closure written at the crossing crosses.** The compiler sees
  its captures there and checks them. A closure value stored elsewhere,
  in a variable, a field or a list, has a type `fn(A) -> R` that hides
  its captures, and copying it keeps its cells
  ([`fn.capture.copies`](../../spec/lang/07-functions.md#r-fn.capture.copies)).
  So any value of function type that is not the crossing's own literal is
  neither `Send` nor `Wire`, and a struct, list or `&T` holding one does
  not cross. A top-level function is called from the literal instead.
  This leaves out helpers that hand a callback to a thread, such as
  `parallel_map(items, f)` or a `logged_run(f)` wrapper: their `f` is a
  function value. The first version supports direct task submission and
  parallel algorithms that std writes as built-ins over literals.
- **Reached top-level bindings are immutable, and `Send` for a thread.**
  A closure is `Send` only when every top-level binding it reaches,
  directly or through a callee, is immutable and `Send`, so a crossing
  literal that calls a helper that uses a top-level stub is rejected. It
  is `Wire` only when every such binding is immutable: module storage is
  per instance
  ([`module.init.program-instance`](../../spec/lang/10-modules.md#r-module.init.program-instance)),
  so another memory has its own copy, and a `let mut` counter would
  change only one domain's.

The mode is never written. A `Copy` value copies, an `impl !Copy` value
moves, a `&T` is lent inside a `Scope`, and a remote reference stays with
its home. Into separate memory a move still copies the bytes once, in
bulk, because two Wasm instances do not share memory; what it saves is
the source's copy and any field-by-field encoding. In shared memory a
move copies nothing, and a copy-on-write `List` shares its buffer, whose
counts become atomic with threads. Rust states the same split with `Send`
on `thread::spawn`, its `move` closures, and serde's `Serialize`.

### Code For Another Target: Remote

A domain of another target runs only methods of interface traits that
extend the std marker `Remote`. Those methods are its endpoints, and
nothing else of the program is reachable from outside.

- **R1.** Every method is a bang method. Its parameters, its success type
  and its error type are `Wire`, so no value of function type appears at
  any depth and no caller picks code another target runs. It is not generic and has no row; its
  implementation gets what it needs from its own fields. A trait may be
  generic, and the served type fixes its type arguments.
- **R2.** The trait declares the result callers see,
  `Result[V, E | CrossError]`. An implementation declares
  `Result[V, E]`, without `CrossError`, and a local call wraps its error
  as `._0`. So case `_1` comes only from a stub and always describes that
  one call, on every path: through the network, on the home itself, and
  in a test. An implementation that calls another service must match the
  other call's `._1` into one of its own `E` cases, because its error type
  has no other place for it.

  | Entry | Result | Who calls it |
  | --- | --- | --- |
  | the raw entry: the implementation's body | `Result[V, E]` | only the generated dispatcher |
  | the public entry: the trait's method | `Result[V, E \| CrossError]`, the body's error wrapped as `._0` | every call: on a concrete value, through a generic bound, through `&dyn` or `*mut dyn`, and a method value |
  | the stub's method | the same public result, adding this call's own failure as `._1` | every call on a stub |
- **R3.** A domain serves its root objects through a home (What Std Adds,
  below). A method may also return a new object as a `*mut dyn U` with
  `U < Remote`; the home adds it to its table and serves it too.

A `*mut dyn T` of a `Remote` trait `T` is a reference to an object that
one domain serves, its **home**. On its home it is the object itself.
Anywhere else it is a stub: a value of a type the compiler generates for
`T` (B3), whose methods send each call to the home, where it is a local
call. `remote::[T](transport)` returns the root object at the far end of
a transport as such a reference; a test installs a fake instead. So a
service, a GPU texture, a server action, a stream cursor and a
long-lived process are all `*mut dyn T`:

| Object | Its trait | Its home |
| --- | --- | --- |
| the users service | `Users` | a server |
| a texture | `Texture` | a GPU |
| a server action | `Call[A, T, E]` | the server that rendered it |
| a stream cursor | `Next[T, E]` | the server that opened it |
| an account process | `Account` | a worker |

Away from its home, a remote reference is a lease bound to one session,
as an ordinary Cap'n Proto capability lives only as long as its
connection:

- **Home only.** Every use is a call to the home, so an object is never
  touched elsewhere. Device code cannot make crossings, so a kernel given
  a texture whose home is another GPU is refused with
  `CrossError.WrongHome` before it runs.
- **Bound to a session.** A reference that leaves its target carries an
  unguessable token bound to the session it was sent to. Every call
  arrives with its session, verified by the home's request factory, and
  the home refuses a token from any other session with `WrongHome`. So a page cannot call
  another user's action by editing an id, and a reference forwarded to
  another holder fails at its first call.
- **Leased.** Each call renews the lease, and a call in progress holds it
  until the call ends. The home drops the object after a timeout, and a
  later call is `WrongHome`. So a closed browser tab leaks nothing, and a
  job that stored a reference fails loudly. What must last or travel is
  stored as plain data, such as an `account_id: i64`, and looked up again
  through the service, which gives the new holder its own reference.

Only a domain that has a home can send its own objects; a browser that
serves no calls refuses to send one, with `WrongHome`. "Code goes to the
data" is library: `workers.at(rows)` returns the worker that is `rows`'s
home, so code sent there reaches `rows` by local calls, as Chapel's
`on x do` runs where `x` lives.

```text
# hypothetical syntax
pub trait Users < Remote:
    fn load_user!(&mut self, id: i64) -> Result[User, UserError | CrossError]

pub trait Filters < Remote:                    # served on a GPU: device code
    fn blur!(&mut self, texture: *mut dyn Texture, radius: f32) -> Result[void, GpuError | CrossError]
```

A device target's entry builds a home of its kernels' implementation and
answers each frame with `handle_call!`. That implementation must be
device code: no allocation and no host calls. That subset, and launch
geometry, get their own design.

### How A Crossing Fails: CrossError

`CrossError` is a std enum with four cases:

| Case | Meaning | Did this call run? |
| --- | --- | --- |
| `Unreachable` | the call never left | no |
| `Unknown` | it left and no answer came, including a timeout after sending, a crash at the destination, or a reply that does not decode | maybe |
| `OtherBuild` | the destination runs another build of the program, as after a deploy, and refused | no |
| `WrongHome` | the call named an object its destination does not serve: another GPU's texture, a token of another session, an expired lease | no |

A `CrossError` always describes one invocation, the one it is returned
from:

- A domain of the caller's target: its `run!` returns `Result[T,
  CrossError]` when contact can be lost. Code that returns its own
  `Result[V, E]` gives `Result[Result[V, E], CrossError]`, so the outer
  layer is the crossing and the inner one the code's. `??` flattens it
  when `E < From[CrossError]`
  ([`expr.try.convert.from`](../../spec/lang/05-expressions.md#r-expr.try.convert.from)).
- A `Remote` method: case `_1` of its `E | CrossError`, filled only by a
  stub (R2). A failure of another call the implementation made reaches
  its caller only as one of its own `E` cases, which its author writes.

Cancelling a crossing, by abandoning the task that waits on it
([`req.abandon.host`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.host)), sends a cancel; the call's outcome is then
`Unknown`. A moved value never comes back through an error, so an unknown
outcome never returns ownership.

### One Build, One Output Per Target

Every domain runs the same program (section 1). A target is an
executable of the package
([`cli.exe.table`](../../spec/cli/command-line.md#r-cli.exe.table)): its
`main` row says what its host binds, and the `[capabilities]` table
grants it (section 5 shows one). One build compiles every target, and
every output carries the same build ID.

- **B1.** Each target walks the use graph and the call graph from its
  entry and compiles only what it reaches. A module only the server
  reaches, such as one that uses a database driver, is never parsed for
  the browser.
- **B2.** For each root type `S` a target serves, the compiler generates
  the body of `handle_call![S]`: check the build ID; find the object, and
  check its token and lease against the call's session; check that the
  object implements the frame's interface; decode the method number and
  arguments as untrusted input; call the raw entry; and encode the answer
  as a `Reply`: `Ran` with the encoded `Result[V, E]`, or `Refused` with
  the `CrossError` of a failed check.
- **B3.** For each `Remote` trait `T` whose objects a target can hold away
  from their home, the compiler generates a stub type and its `impl T`.
  Each method encodes its arguments, sends them with the interface's
  identity and the method's number, decodes the `Reply` and then its
  payload, and returns a `Refused` reply or a transport failure as
  `._1(cross)`. A
  stub holds no implementation code.
- **B4.** A crossing to a domain of the caller's target compiles into the
  one module. Its code is named by the closure's lifted function, plus
  its captures, copied or moved at the crossing.
- **B5.** Every `Wire` type gets a generated codec, the tree encoding of
  boundary values ([`module.boundary.tree`](../../spec/lang/10-modules.md#r-module.boundary.tree)).
  The build ID travels with every crossing, so a peer of another build
  refuses with `OtherBuild` before any code runs. Every frame names its
  interface: the trait's path and type arguments, numbered once per
  build. Within one build, every target numbers each trait's methods
  alike, and a frame for another interface than the object serves, as
  from a stub pointed at the wrong service, is `WrongHome`. Decoding is checked on every
  crossing: a matching build ID shows that the two sides agree on the
  format, not that the bytes arrived intact, so a frame that fails to
  decode is `Unknown` at the caller, never a panic. A decoded remote
  reference takes the transport its frame came on as its home's.

### Homes, Roots, Sessions And Refusals

Five words recur in the std items below. Each names one step of how a
call from another domain reaches an object.

- **Home.** The table one domain keeps of the objects it serves. A server
  has one home, and so does each worker and each GPU. A call arrives at a
  home, which finds the object the call names and runs the method there.
  Std writes it as `Home[S, C]`.
- **Root.** The object a caller can reach with no reference in hand: the
  first object of a home, number 0. `remote::[Users](transport)` returns
  a stub of the root at the far end of that transport. Every other object
  a caller holds was returned by a method, starting from the root. For
  the api server, the root is a `PgUsers`.
- **Session.** Who a call comes from, as the home has verified it, such
  as one login of one user. The home learns it from the call itself,
  never from the frame's bytes: over HTTP, from the session cookie, which
  the server's own code checks against its database. Each object a method
  returns is recorded with the session it was returned to, and a later
  call to that object from another session is refused. That is why a
  page cannot call another user's server action by editing an object
  number.
- **Rooted.** What the home builds for each incoming call: a root and
  the session of its caller, `Rooted { root, session }`. The program
  supplies the function that builds it, the **request factory**. A
  server builds a new root per request, `per_request(...)`, so the root
  can hold the verified caller (`PgUsers.caller`) and two interleaved
  requests never share one. A worker, which has one peer, uses one fixed
  root and session, `home(root, session)`.
- **Refusal.** A call turned away before any method runs. It happens at
  two steps, and both reach the caller as a `CrossError` that says only
  that the call did not run:
  - the factory refuses the caller as a whole, as when the cookie is
    missing. It returns `Refused { reason }`, whose reason is for the
    server's own log; the caller sees `WrongHome`. Over HTTP it is a 403.
  - the dispatcher refuses one call after the factory accepted the
    caller: another build, another interface, an object of another
    session, an expired lease. It answers `Reply.Refused(cross)`, which
    carries the `CrossError` itself (`OtherBuild` or `WrongHome`).

  A `Reply` is the envelope of every answer, so a caller tells a refusal
  apart from an application result: `Ran` carries the method's encoded
  `Result[V, E]`, and only then is there a result to decode.

The api server's call, in order: the request arrives; the factory checks
the cookie and builds `Rooted { root: PgUsers { pool, caller }, session }`,
or refuses; the dispatcher checks the build ID, the interface, and the
object's session and lease, or refuses; then `load_user` runs on the root
and its answer goes back as `Reply.Ran`.

### What Std Adds

Everything the design adds to std, written out. Serving over HTTP reuses
the server SDK's `serve!` and `HttpServer`; calling reuses `std.http`'s
`Http`. Two items are built-ins, declared here and lowered by the
compiler: `remote`, which takes a trait as its type argument as
`$.use(Users)` takes a requirement key, and `handle_call!`, whose body B2
generates. The stub types of B3 have no declaration at all.

```text
# hypothetical syntax; std.remote
use std.http.{Body, Http, Request, Response, Url}

pub trait Remote:                            # a marker; the compiler checks R1 and R2 on each trait that extends it
    pass

@error
pub enum CrossError:
    @error("unreachable")
    Unreachable
    @error("no answer")
    Unknown
    @error("another build")
    OtherBuild
    @error("not served here")
    WrongHome

pub trait Transport:                         # carries call frames to one home
    fn call!(&mut self, frame: List[u8]) -> Result[List[u8], CrossError]

# the root object at the far end of transport, as a stub of T (B3)
@intrinsic("remote")
pub fn remote[T < Remote](transport: *mut dyn Transport) -> *mut dyn T:
    panic("intrinsic")

pub struct Refused:                          # why a home turned a caller away before any call
    pub reason: string

pub struct Session:                          # who a call comes from, as the home verified it
    pub id: string

pub struct Rooted[S]:                        # what a request factory returns
    pub root: S
    pub session: Session

pub struct Home[S, C]:                       # what one domain serves; fields private
    make: fn!(&C) -> Result[Rooted[S], Refused]   # a root and a session for each incoming call record C
    objects: ServedTable                     # objects that methods returned, each with its session, token and lease

pub fn per_request[S, C](make: fn!(&C) -> Result[Rooted[S], Refused]) -> *mut Home[S, C]:
    box(Home { make, objects: ServedTable::new() })

pub fn home[S, C](root: S, session: Session) -> *mut Home[S, C]:   # one root and one peer, as for a worker
    per_request(fn!(call: &C) -> Result[Rooted[S], Refused]: .Ok(Rooted { root, session }))

pub enum Reply:                              # every answer a home sends, before any application payload
    Ran(payload: List[u8])                   # the call ran: an encoded Result[V, E]
    Refused(error: CrossError)               # it did not run: another build, interface, session or lease

# B2 generates the body for each S; session comes from the factory, never from the frame;
# the bytes it returns are an encoded Reply
@intrinsic("handle_call")
pub fn handle_call![S, C](home: &mut Home[S, C], rooted: &mut Rooted[S], frame: List[u8]) -> List[u8]:
    panic("intrinsic")

# HTTP, as plain hd over the items above
pub struct HttpTransport:
    url: Url
    http: *mut dyn Http

impl Transport for HttpTransport:
    fn call!(&mut self, frame: List[u8]) -> Result[List[u8], CrossError]:
        request := Request { method: .Post, url: self.url, body: Body::from_bytes(frame) }
        match self.http.send!(&request, 0):
            .Ok(response) if response.status == .Forbidden => .Err(.WrongHome)   # the home refused the caller
            .Ok(response) => match response.bytes():
                .Ok(bytes) => .Ok(bytes)
                .Err(_) => .Err(.Unknown)
            .Err(_) => .Err(.Unknown)        # split by HttpError: a connection never opened is Unreachable

pub fn http_transport(url: Url) -> *mut dyn Transport $ Http:
    box(HttpTransport { url, http: $.use(Http) })

pub fn http_handler![S](request: Request, home: &mut Home[S, Request]) -> Response:
    make := home.make
    let mut rooted = match make!(&request):
        .Ok(rooted) => rooted
        .Err(_) => return Response { status: .Forbidden }
    match request.bytes():
        .Ok(frame) => Response { body: Body::from_bytes(handle_call!(home, &mut rooted, frame)) }
        .Err(_) => Response { status: .BadRequest }
```

`http_handler::[S]` has exactly the handler type the SDK's `serve!`
takes, `fn!(Request, &mut Home[S, Request]) -> Response`. A root built
per request sees the request it serves, so it can carry the caller's
verified identity in a field, and two interleaved requests never share
it. The factory also returns the call's `Session`. `handle_call!`
records it on every object a method returns, and checks it on every
later call to that object, so the session binding holds for returned
objects, not only for the root. A `Worker` and a `Gpu` implement `Transport` the same way over their
own channels, and a worker or device answers frames with `handle_call!`
on its home.

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

pub trait Orders < Remote:
    fn place_order!(&mut self, cart: Cart) -> Result[Receipt, OrderError | CrossError]

pub fn on_buy!(cart: Cart) -> void $ Page + Orders:
    let mut orders = $.use(Orders)
    match orders.place_order!(cart):
        .Ok(receipt) => show(receipt)
        .Err(._0(error)) => show_error("$error")
        .Err(._1(.Unknown)) => check_order_status!(cart.id)       # this call may have run
        .Err(._1(_)) => retry!()                                  # this call did not run
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
tex := gpu.upload!(photo)?                      # tex: *mut dyn Texture, home: this GPU
let mut filters = remote::[Filters](gpu)
filters.blur!(tex, 2.0)?
image := gpu.read_back!(tex)?
```

**5. Parallel word count.** Scoped threads borrow `lines`, which stays frozen; the scope joins
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

**7. Server action.** The server renders a `*mut dyn Call[A, T, E]` whose
home is the server; the browser calls it later, and the captured id never
leaves. Its token is bound to the page's session.

```text
# hypothetical syntax; rename: *mut dyn Call[string, void, TodoError]
match rename.call!(text):
    .Ok(()) => pass
    .Err(._0(error)) => println("could not rename: $error")
    .Err(._1(.Unknown)) => println("saving; refresh to check")
    .Err(._1(_)) => println("not saved; try again")
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
    @error("user $id is not yours")
    Forbidden(id: i64)

pub trait Users < Remote:                           # ◀1
    fn load_user!(&mut self, id: i64) -> Result[User, UserError | CrossError]   # ◀2

# src/ui.hd: the page, a package host capability
@host
pub trait Page:
    fn show(&mut self, text: string) -> void

# src/server.hd: the api executable's entry; only it uses the driver
use dep.pg.{Pool, connect}
use dep.server_sdk.server.{HttpServer, ServerError, serve}
use pkg.users.{User, UserError, Users}
use std.http.Request
use std.net.{Net, NetError}
use std.remote.{Refused, Rooted, Session, http_handler, per_request}

struct PgUsers:
    pool: *mut Pool                                 # shared by every request; keeps the counted Net it connected with
    caller: i64                                     # this request's caller, verified when the root was built

impl Users for PgUsers:
    fn load_user!(&mut self, id: i64) -> Result[User, UserError]:              # R2: no CrossError here
        if self.caller != id:
            return .Err(UserError.Forbidden(id))
        match self.pool.query_one!("select id, name from users where id = ?", id):
            .Some(row) => .Ok(User { id: row.i64(0), name: row.text(1) })
            .None => .Err(UserError.Missing(id))

fn verify_session!(pool: *mut Pool, cookie: string?) -> Result[(i64, Session), Refused]:
    match cookie:
        .Some(token) => match pool.query_one!("select user_id, id from sessions where token = ?", token):
            .Some(row) => .Ok((row.i64(0), Session { id: row.text(1) }))   # one id per login: phone and desktop differ
            .None => .Err(Refused { reason: "unknown session" })
        .None => .Err(Refused { reason: "no session" })

@error
enum ApiError:
    @error("database: $cause")
    Db(@from cause: NetError)
    @error("server: $cause")
    Server(@from cause: ServerError)

pub fn main!() -> Result[void, ApiError] $ Net + HttpServer:
    pool := box(connect!("db.internal:5432")?)
    root := per_request(fn!(request: &Request) -> Result[Rooted[PgUsers], Refused]:     # ◀3
        let (caller, session) = verify_session!(pool, request.cookie("session"))?
        .Ok(Rooted { root: PgUsers { pool, caller }, session })
    )
    serve!(0, root, http_handler::[PgUsers])?
    .Ok(())

# src/browser.hd: the web executable's entry
use pkg.ui.Page
use pkg.users.{UserError, Users}
use std.http.{Http, Url}
use std.remote.{http_transport, remote}

pub fn main!() -> void $ Page + Http:
    url := Url::parse("https://api.example.com/rpc").unwrap()
    stub := remote::[Users](http_transport(url))    # ◀4 stub: *mut dyn Users
    $.with(Users=stub):                             # ◀5
        on_open!(7)

pub fn on_open!(id: i64) -> void $ Page + Users:
    let mut page = $.use(Page)
    let mut users = $.use(Users)                    # users: &mut dyn Users
    match users.load_user!(id):                     # ◀6
        .Ok(user) => page.show("Hi, ${user.name}")
        .Err(._0(error)) => page.show("$error")
        .Err(._1(.Unknown)) => page.show("no answer; try again")
        .Err(._1(cross)) => page.show("not sent: $cross")
```

| Line | What it means |
| --- | --- |
| ◀1 `pub trait Users < Remote` | `load_user` may be called from another target; R1 and R2 apply to it |
| ◀2 `UserError \| CrossError` | what callers see; case `_1` is this call's own failure, filled only by a stub, while `PgUsers` declares `Result[User, UserError]` (R2) |
| ◀3 `per_request(fn!(request: &Request) ...)` | each request gets its own `PgUsers`, holding the caller its session cookie proves, and the verified `Session` that guards every object it returns |
| ◀4 `remote::[Users](http_transport(url))` | a `*mut dyn Users` whose home is that URL: the stub |
| ◀5 `$.with(Users=stub)` | every `Users` call below goes to the server |
| ◀6 `users.load_user!(id)` | nothing new: a call on a `&mut dyn Users` |

`PgUsers` and the stub both reach their host through a field, not a row:
a counted provider value may be stored
([`req.use.value.flow`](../../spec/lang/11-requirements-and-suspension.md#r-req.use.value.flow)),
and a row records only lookups
([`req.use.value.row-meaning`](../../spec/lang/11-requirements-and-suspension.md#r-req.use.value.row-meaning)).
Who the caller is comes from the root built for the request, not from a
row or an argument, so R1's "no row" does not keep it from the
implementation.

**Reading it.** ◀5 is the line that turns ◀6 into a round trip. Above
◀5, the same call would be local. The call line alone cannot tell,
because `$.use(Users)` is a `&mut dyn Users`
([`req.use.value.ordinary-type.reference-values`](../../spec/lang/11-requirements-and-suspension.md#r-req.use.value.ordinary-type.reference-values)).
It shows three things: the method belongs to a `Remote` trait, so it may
cross; it is a bang call, so it may wait; and its error type ends in
`| CrossError`, so contact may be lost. A gRPC stub passed as an
interface reads the same way.

**Checking it.**

| Line | Check | Breaking it |
| --- | --- | --- |
| `hd.toml` | each `main` row's host capabilities are in `[capabilities]` | drop `Http`: `missing-capability`, today's rule |
| ◀1 | R1: `load_user` is a bang method; `i64`, `User` and `UserError` are `Wire`, so no function value is inside; no generics, no row | `fn load_user(...)` without `!`: an error at ◀1 |
| ◀2 | R2: the trait's result is `Result[V, E \| CrossError]`, and the implementation's is `Result[V, E]` | the trait without `\| CrossError`, or `PgUsers` with it: an error |
| `impl Users for PgUsers` | the impl's row agrees with the trait's, which is empty ([`req.row.callable.impl-agrees`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.callable.impl-agrees)) | `$ Net` on the method: an error |
| ◀3 | `http_handler::[PgUsers]` has the handler type `serve!` takes, `fn!(Request, &mut Home[PgUsers, Request]) -> Response`; `handle_call!` needs `PgUsers` to implement a `Remote` trait | a factory rooting a `Pool`: an error, `Pool` implements no `Remote` trait |
| ◀4 | `Users < Remote` | `remote::[Page](...)`: an error |
| ◀5 | the stub implements `Users`, through the impl B3 generates | none |
| ◀6 | nothing new | |

**Building it.** `hd build` builds both executables with one build ID.
Each walks from its own entry module (B1):

| | `web` (browser) | `api` (server) |
| --- | --- | --- |
| `users.hd` | `User`, `UserError`, `Users`, and their codecs (B5) | the same |
| `ui.hd` | `Page`, and the generated `web.host.mjs` glue | not reached |
| `server.hd`, `pg`, `server_sdk` | never parsed | compiled |
| `std.remote` | `remote`, `http_transport`, `HttpTransport` | `per_request`, `http_handler`, `handle_call!` |
| the stub type for `Users` and its `impl Users` (B3) | generated | none |
| the body of `handle_call!` for `PgUsers` (B2) | none | generated |

Each executable as built, shown as the hd it amounts to. The source as
written is kept; the build adds only the parts below. Generated names
such as `UsersStub` and `wire_encode_User` are illustrations.

*`web`, as built:*

```text
# hypothetical syntax; added to the web executable
BUILD_ID := "3f9a"                                  # the same in both executables of this build
INTERFACE_USERS := +3                               # pkg.users.Users, numbered once per build

# B5: one codec per Wire type the executable sends or receives
fn wire_encode_User(out: &mut Bytes, user: &User) -> void:
    wire_encode_i64(out, user.id)
    wire_encode_string(out, user.name)

fn wire_decode_User(input: &mut Reader) -> Result[User, WireError]:
    id := wire_decode_i64(input)?
    name := wire_decode_string(input)?
    .Ok(User { id, name })

# B3: the stub type for Users, because main calls remote::[Users] (◀4)
struct UsersStub:
    transport: *mut dyn Transport
    object: u64                                     # 0: the home's root object
    token: List[u8]

impl Users for UsersStub:
    fn load_user!(&mut self, id: i64) -> Result[User, UserError | CrossError]:
        let frame: mut Bytes = Bytes::new()
        wire_encode_frame_head(frame, BUILD_ID, INTERFACE_USERS, self.object, self.token, +0)   # method 0: load_user
        wire_encode_i64(frame, id)
        match self.transport.call!(frame.to_list()):                          # HttpTransport: one POST
            .Ok(bytes) => match wire_decode_Reply(Reader::new(bytes)):
                .Ok(.Refused(cross)) => .Err(._1(cross))                      # OtherBuild, WrongHome: did not run
                .Ok(.Ran(payload)) => match wire_decode_Result_User_UserError(Reader::new(payload)):
                    .Ok(.Ok(user)) => .Ok(user)
                    .Ok(.Err(error)) => .Err(._0(error))                      # the implementation's own error
                    .Err(_) => .Err(._1(CrossError.Unknown))                  # B5: a bad reply is Unknown
                .Err(_) => .Err(._1(CrossError.Unknown))
            .Err(cross) => .Err(._1(cross))                                   # ◀2: this call's own failure

# ◀4 lowers to: box(UsersStub { transport: http_transport(url), object: 0, token: [] })
```

*`api`, as built.* `main!` is unchanged: `serve!` is the SDK's, and
`per_request` and `http_handler` are plain std code. The build adds the
codecs and the body of `handle_call!` for `PgUsers`:

```text
# hypothetical syntax; added to the api executable
BUILD_ID := "3f9a"
INTERFACE_USERS := +3

# B5: the same codecs as web's

# B2: handle_call! for PgUsers, because main serves a Home[PgUsers, Request] (◀3)
fn handle_call_PgUsers!(home: &mut Home[PgUsers, Request], rooted: &mut Rooted[PgUsers], frame: List[u8]) -> List[u8]:
    let input = Reader::new(frame)
    match wire_decode_frame_head(input):                  # untrusted input from here on
        .Err(_) => refuse(CrossError.WrongHome)                   # refuse(e): an encoded Reply.Refused(e)
        .Ok(head) if head.build != BUILD_ID => refuse(CrossError.OtherBuild)
        .Ok(head) if head.object != 0 => home.call_served!(head, rooted.session, input)   # checks interface, session, token, lease
        .Ok(head) if head.interface != INTERFACE_USERS => refuse(CrossError.WrongHome)    # PgUsers serves Users only
        .Ok(head) => match head.method:
            0 => match wire_decode_i64(input):
                .Ok(id) => ran(wire_encode_Result_User_UserError(PgUsers_load_user_raw!(&mut rooted.root, id)))   # the raw entry
                .Err(_) => refuse(CrossError.WrongHome)
            _ => refuse(CrossError.WrongHome)
```

`web` holds no `PgUsers`, no SQL, no driver and no `handle_call!`; `api`
holds no stub. Interface 3 is `pkg.users.Users` and method 0 is
`load_user`; both executables come from one build, so they number them
alike. A stub of another interface, pointed at this server by mistake,
gets `WrongHome` before anything runs.

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
and the captures, copied or moved at the crossing:

```text
# hypothetical syntax; as built, in the one module the page and every worker load
fn closure_0(photo: Image) -> (Image, Image): thumbnail(photo, 256)    # captures become parameters

pub fn make_thumb!(photo: Image) -> Result[Image, CrossError] $ Workers:
    let mut workers = $.use(Workers)
    let args: mut Bytes = Bytes::new()
    wire_encode_Image(args, photo)                  # photo moves: its pixels are copied once, in bulk
    reply := workers.call!(closure=+0, args=args)?  # with BUILD_ID
    let (back, small) = match wire_decode_Image_Image(Reader::new(reply)):
        .Ok(pair) => pair
        .Err(_) => return .Err(CrossError.Unknown)  # B5: a bad frame is Unknown, never a panic
    keep(back)
    .Ok(small)

# the worker's side: every instance has the same table
fn run_closure(index: u32, args: List[u8]) -> Result[List[u8], CrossError]:
    match index:
        0 => match wire_decode_Image(Reader::new(args)):
            .Ok(photo) => .Ok(wire_encode_Image_Image(closure_0(photo)))
            .Err(_) => .Err(CrossError.Unknown)
        _ => .Err(CrossError.OtherBuild)
```

A `Jobs` closure is built the same way, and the store keeps the index,
the captures and the build ID. A truncated job record still carries the
right build ID, so it fails its decode and the job ends with `Unknown`.

**Running it.** One call:

1. `on_open!(7)` reaches ◀6; `users` is the stub.
2. The stub writes a frame `{build: 3f9a…, interface: 3, object: 0, method: 0, args: [7]}`,
   and its `HttpTransport` posts it to `https://api.example.com/rpc`
   through `Http`, with the page's session cookie, as the
   `Http = ["api.example.com"]` grant allows.
3. On the server, the SDK's `serve!` accepts the request and calls
   `http_handler`. That runs the ◀3 factory, which verifies the session
   cookie, finds caller 7 and that login's session, and builds
   `PgUsers { pool, caller: 7 }`. Then
   `handle_call!` checks the build ID, decodes `7`, and calls
   `load_user!(7)`, which queries the database through `Net`. It encodes
   `Ok(User { id: 7, name: "Ada" })` as the response body.
4. The browser decodes it, and the match takes `.Ok`. Had `load_user!`
   returned `Err(Missing(7))`, the stub would return `._0(Missing(7))`,
   exactly what a local call to `PgUsers` returns.

When it fails: with the network down, the transport returns
`Unreachable`, and the stub returns `.Err(._1(.Unreachable))`. After a
deploy, the server refuses with `OtherBuild`. When the reply is lost,
the outcome is `._1(.Unknown)`. A page without a valid session gets
`Forbidden` from `http_handler`, which the transport reports as
`._1(.WrongHome)`: the call did not run.

Requests run concurrently. The SDK's workers share the one state, here
the `Home[PgUsers, Request]`
([`std-server-sdk.serve-state`](../../spec/std/server-sdk.md#r-std-server-sdk.serve-state)),
and requests interleave at suspension points such as `query_one!`. Each
has its own `PgUsers`, so one request's `caller` never leaks into
another; only what sits behind `pool` is shared.

Every domain kind, in the same terms:

| Domain | A crossing | Failure and cancel |
| --- | --- | --- |
| `Scope` | a thread on the same memory runs the closure; the scope joins | none |
| `Background` | a task goes to a thread of the same memory; the caller suspends, and the answer wakes it on its own event loop | none |
| `Workers` | a second instance of the same module gets the lifted function's index and the captures | `Unknown` on a crash or a bad frame; cancel posts an abort |
| `Jobs` | the store keeps the index, the captures and the build ID; a host of the same build runs it later | the job's retry policy; `OtherBuild` after a deploy |
| server | the stub's transport sends the build ID, object, token, method number and arguments; `handle_call!` refuses another build, another session's token or an expired lease, validates, calls | `Unknown` when the call may have run; cancel is seen at the server's next suspension |
| GPU | the stub launches the kernel with its texture references | `Unknown` on a reset; `WrongHome` for another GPU's texture |

## 6. Cost

Every rule the compiler must know is language tier, even where its
declaration sits in std.

| Piece | Tier | New |
| --- | --- | --- |
| `Send`, `Wire`, `Remote` | language: markers | structural checks, as `Copy`'s |
| captures at a crossing: copied, moved or lent; never a shared cell, never written; only the crossing's own literal crosses | language: a capture mode for closures passed to crossing methods | a check at each crossing call |
| a `&T` lent to a thread freezes its place | language: a loan rule stronger than [`ref.elem.access.live-length`](../../spec/lang/13-values-and-references.md#r-ref.elem.access.live-length) | the loan checker treats the whole scope as one write-excluding loan |
| R1 and R2 on `Remote` traits | language: checks over existing types and unions, and an implementation's error type narrower than its trait's | a local call wraps the implementation's error as `._0` |
| a `*mut dyn T` of a `Remote` trait is `Wire`, not `Send` | language: a typing rule | one stub type and `impl T` generated per trait (B3) |
| `remote::[T]`, which takes a trait argument, and `handle_call!` | language: built-ins declared in `std.remote` | lowering in the compiler; the dispatcher body per root type (B2) |
| codecs for `Wire` types, the build ID, interface numbers | compiler and runtime | generated per type and per trait (B5) |
| `std.remote`'s plain parts (`CrossError`, `Transport`, `Home`, `per_request`, `home`, the HTTP adapters), the domain types, `scope` | stdlib, written in hd | none |
| targets | CLI and driver | none in `hd.toml`: an executable is a target; one walk per executable |
| threads with shared memory and atomic counts | runtime | Wasm threads; only `Scope` and `Background` need them |
| worker instances, a jobs store, a device backend | runtime | one slice each |

The language part is three markers, four rules (capture mode, frozen
loans, R1 and R2, remote references as `Wire`), two built-ins, and the
generated stubs, dispatchers and codecs. Every domain kind is std code
over them, so a new kind (a Redis node, a WebRTC peer) is a `Transport`
and a home, not a language change.

Not in this design, each its own later design: the device-code subset,
launch geometry and resource binding, and supervision of long-lived
served objects.

## Appendix A. Stress Test

Eighteen cases, each the public shape of a well-known library or a
crossing operation, rewritten in this design. Shapes are approximations.
A case *works*, has *friction* (it works with a workaround or a visible
difference from the original), or *breaks* (it cannot be written, or
behaves differently). Verdicts come from reading the rules: the blocks
are parsed, not type-checked or run.

| # | Case (library) | Verdict | What the design does |
| --- | --- | --- | --- |
| 1 | Worker thumbnail (Comlink, transferables) | friction | the `impl !Copy` image moves, but its pixels are copied once, in bulk, between two Wasm memories, where JavaScript hands a buffer over with no copy |
| 2 | Form action (Next.js "use server") | works | a `*mut dyn Call[A, T, E]`; the captured id stays on the server, and the token is bound to the page's session |
| 3 | Typed service (tRPC, gRPC) | works | `per_request` builds the root with the caller its session proves, as tRPC's `createContext` |
| 4 | Scoped count and sort (`thread::scope`, rayon) | friction | direct spawns work: `lines` is lent, frozen until the join, and each half goes in by value and comes back; but a rayon-style `par_map(items, f)` cannot hand its `f` to a thread |
| 5 | Off the UI thread (Kotlin, Swift) | works | `background.run!` returns `T`; code that uses `$ Ui` does not fit `Background`'s row; a background call to a server builds its own stub, since a stub is not `Send` |
| 6 | Delayed mail (Sidekiq, Temporal) | friction | a deploy fails the jobs queued before it with `OtherBuild`, so a daily deploy rules out a job a week out |
| 7 | GPU kernels (wgpu, CUDA) | friction | host code uploads, calls and reads back; until one complete kernel, with its device code and resource binding, is written, the model is not shown to fit devices |
| 8 | Data-local count (Ray, Spark) | works | within one target: `workers.at(rows)` runs a closure at `rows`'s home |
| 9 | Long-lived process (Erlang, Akka) | works | an account is a `*mut dyn Account` returned by a `Remote` method; supervision is library work |
| 10 | A test swaps the server | works | stub, real implementation and fake are all providers of `Users` |
| 11 | Error propagation | works | `E \| CrossError`: a call's own failure stays its own, and another call's failure needs a case its author writes |
| 12 | Crossings in `join!` | works | two bang calls on two domain values join as any two calls do |
| 13 | Cancel mid-crossing | works | the cancel makes the outcome `Unknown`; a moved value never returns through an error |
| 14 | A remote call inside a remote call | works | the implementation holds the other stub as a field and must match its `._1` into one of its own `E` cases, since its own error type has no `_1` |
| 15 | Remote with a closure or a remote reference | works | a function value at any depth, in parameters, result or error, is rejected by R1; a `*mut dyn Texture` crosses to its home |
| 16 | Generic remote function | friction | a generic method is rejected; a generic trait served at one type works |
| 17 | Streaming results | friction | a stream is a `*mut dyn Next[T, E]` pulled one item per round trip; server push needs a batching cursor |
| 18 | A background result updates UI | works | the handler resumes on the UI thread; a closure that captured the UI state's box is not `Send` |

Totals: 12 works, 6 friction, 0 breaks. One friction is a copy two Wasm
memories cannot avoid (case 1), one is the price of refusing another
build (case 6), and one is the literal-only rule, which leaves
callback-taking parallel helpers out of the first version (case 4). Case 7 waits for the device design. Two are costs of the
rules on purpose: R1 bars generic methods (case 16), and a remote object
answers one call per round trip (case 17).

What the cases show is typed calls between targets, plus closures within
one target. They do not show shipping code a client picks to another
target: a new kind of query on the server is a new `Remote` method
(program 6).

Adversarial checks, each with the rule that stops it:

| Attempt | Stopped by |
| --- | --- |
| a child reads a `let` variable while the parent writes it | no cell crosses: the child has its own copy |
| two spawns each run `total += n` | a crossing closure may not write a capture |
| the parent writes `xs[0]` while a thread reads `&xs[0]` | a `&T` lent to a thread freezes its place until the join |
| a `&mut` half of a list goes to a thread | `&mut T` is not `Send`; the half goes by value and returns |
| a closure reaches a `let mut` top-level counter | reached top-level bindings must be immutable |
| a crossing literal calls a helper that uses a top-level stub | for a thread, reached top-level bindings must be `Send`, and a stub is not |
| a dispatcher refusal arrives in an HTTP 200 body | the `Reply` envelope: `Refused(OtherBuild)` decodes to `._1(.OtherBuild)`, not to an application result |
| one user's phone presents a token issued to their desktop | sessions are per login, not per user: `WrongHome` |
| a scoped thread gets a box | a box is not `Send` |
| a caller passes a closure, or a struct holding one, to a `Remote` method | R1: no closure at any depth |
| an implementation passes Inventory's `._1(OtherBuild)` on with `?` | a type error: its error type is its own `E`, which has no `_1` case |
| a test calls the real implementation and a page calls its stub | both see the same `._0` errors; only the stub adds `._1` |
| a `Notifications` stub pointed at the Accounts server | the frame names its interface: `WrongHome` |
| one object's token presented under two sessions | the object's recorded session is checked: `WrongHome` |
| two threads share one stub | a remote reference is not `Send` |
| a closure captures another closure, or a struct or `&T` holding one | only the crossing's own literal crosses: a function value is not `Send` |
| an error payload holds a closure or a connection | R1: the error type is `Wire` too |
| two interleaved requests read each other's caller | a root per request |
| a page edits an object's id to reach another user's action | the token is unguessable and bound to the session: `WrongHome` |
| a holder forwards a remote reference to another session | `WrongHome` at its first call |
| a job stores a remote reference and runs after the lease ends | `WrongHome`; it should store the id and look the object up again |
| a browser closes without dropping a remote reference | the lease expires at its home |
| a kernel gets a texture of another GPU | `WrongHome` before it runs |
| a client of yesterday's build calls today's server | `OtherBuild` before any code runs |
| a job record is truncated but keeps its build ID | its decode fails: `Unknown`, never a panic |
| a closure captures a provider value | a provider value is neither `Send` nor `Wire` |
| a moved value is used after the crossing fails | the source binding ended: `use-of-moved-value` |

**Case 4: what a crossing closure may not do.**

```text
# hypothetical syntax; players: mut List[Player]
let total = +0
first := &players[0]                           # first: &Player
scope(fn(s: &mut Scope) -> void:
    t := s.spawn(fn() -> i32: first.score)     # first is lent until the join
    s.spawn(fn() -> void: total += 1)          # error: a crossing closure may not write a capture
    read := fn() -> i32: total
    s.spawn(fn() -> i32: read())               # error: read is a function value, not the crossing's literal
    players[0].score = 9                       # error: players is lent to a thread until the scope joins
    t.join()
)
```

**Case 9: an account process.** A worker serves a `Bank`, and each
account it opens is a remote object of its own.

```text
# hypothetical syntax; an account process on a worker, as an Erlang process
pub trait Bank < Remote:
    fn open_account!(&mut self) -> Result[*mut dyn Account, AccountError | CrossError]

pub trait Account < Remote:
    fn deposit!(&mut self, cents: i64) -> Result[i64, AccountError | CrossError]

struct Ledger:
    cents: i64

impl Account for Ledger:
    fn deposit!(&mut self, cents: i64) -> Result[i64, AccountError]:
        self.cents += cents
        .Ok(self.cents)

pub fn first_deposit!() -> Result[i64, AccountError | CrossError] $ Workers:
    let mut workers = $.use(Workers)
    let mut bank = remote::[Bank](workers.pick())
    let mut account = bank.open_account!()?        # account: *mut dyn Account, home: that worker
    account.deposit!(500)
```

**Cases 14, 15 and 16: what R1 and R2 reject, and what R2 requires.**

```text
# hypothetical syntax
struct SortJob:
    rows: List[Row]
    key: fn(Row) -> i64

pub trait Sorter < Remote:
    fn sort_by!(&mut self, rows: List[Row], key: fn(Row) -> i64) -> Result[List[Row], SortError | CrossError]   # error: R1, a closure parameter
    fn sort!(&mut self, job: SortJob) -> Result[List[Row], SortError | CrossError]                          # error: R1, a closure inside SortJob

pub trait Thumbs < Remote:
    fn shrink!(&mut self, tex: *mut dyn Texture, side: u32) -> Result[*mut dyn Texture, GpuError | CrossError]   # valid

pub trait Store < Remote:
    fn get![T < Wire](&mut self, key: string) -> Result[T, StoreError | CrossError]   # error: R1, a generic method

pub trait TypedStore[T < Wire] < Remote:
    fn get!(&mut self, key: string) -> Result[T, StoreError | CrossError]             # valid: home(UserStore {}) fixes T = User

impl Orders for ShopOrders:
    fn place_order!(&mut self, cart: Cart) -> Result[Receipt, OrderError]:   # R2: no CrossError here
        charge := charge_card!(cart)?                                      # a local function returning OrderError
        match self.inventory.reserve!(cart.items):                         # Result[void, InventoryError | CrossError]
            .Ok(()) => .Ok(Receipt { charge })
            .Err(._0(error)) => .Err(OrderError.OutOfStock(error))
            .Err(._1(cross)) => .Err(OrderError.InventoryDown(cross))      # charged, then Inventory failed: the author says so
```

**Case 17: a stream pulled item by item.**

```text
# hypothetical syntax
pub trait Feed < Remote:
    fn open!(&mut self, topic: string) -> Result[*mut dyn Next[Event, FeedError], FeedError | CrossError]

pub fn watch!(topic: string) -> Result[void, FeedError | CrossError] $ Feed + Console:
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

**Rust `Send`, `Sync` and serde.** `thread::spawn` requires `Send`, its
`move` closures take captures by value, and serde's encoders require
`Serialize`. hd takes the same split as `Send`
for shared memory and `Wire` for separate memory, and copies or moves
captures at a crossing as a `move` closure does. It needs no `Sync`,
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
with `Remote`, and the call with `!` and an error type ending in
`| CrossError`.

**Swift `sending` (SE-0430).** A value nothing else reaches moves to
another isolation domain, and a later use is an error. hd takes this as
the move of an `impl !Copy` value.

**Cap'n Proto, Erlang, Orleans.** A capability, a pid and a grain
reference each name an object that one place serves; every call goes
there. hd takes this as a `*mut dyn T` of a `Remote` trait. It also takes
Cap'n Proto's rule that an ordinary capability lives only as long as its
connection, and a durable one is saved explicitly; hd saves it as an id.

**Chapel `on`.** `on x do` runs where `x` lives. hd takes it as
`workers.at(rows)`, with the home a run-time value.

**ML5 and ScalaLoci.** Types name where a value lives. hd keeps the kind
in the type (`*mut dyn Texture`) and the instance at run time, because a program
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
| `E < From[CrossError]`, with a `Cross` variant in each error enum | `?` merges another call's failure into this call's, so a caller cannot tell which call failed |
| `Result[Result[V, E], CrossError]` for remote calls | a stub could not implement its trait, so `$.with(Users=stub)` and test fakes would not work |
| a `Downstream(cause)` case that the codec adds to every decoded `CrossError` | it rewrites values; the union keeps a call's own failures apart without that |
| the dispatcher rewriting an implementation's `._1` into `Unknown` | a local call bypasses the dispatcher, so a fake and a stub would disagree |
| remote references `Send`, with each call routed to the thread that owns the transport | a thread blocked in a scope's join could not serve the routed call; a stub per thread needs no new mechanism |
| recursive snapshots of the cells of captured closures | a deep copy of environments that the type `fn(A) -> R` does not show |
| function values `Send` when their captures are | the type `fn(A) -> R` does not record captures; a later design may add a capture bound to function types, which would admit `parallel_map(items, f)` |
| a method number unique in the whole build, with no interface in the frame | a root that serves two traits, and a generic trait served at two types, still need the interface to say which |
| a `Handle` marker beside `Remote`, and a separate `Action` type | a handle is a `*mut dyn T` of a `Remote` trait: one kind of reference for services, textures, actions, streams and processes |
| a `Served[T]` std type | a trait cannot be a type argument ([`trait.dyn.keyword.positions`](../../spec/lang/09-traits.md#r-trait.dyn.keyword.positions)), and `*mut dyn T` already names a counted reference to a trait object |
| an impl-only `$ Caller` row | a special case in [`req.row.callable.impl-agrees`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.callable.impl-agrees), and an in-process call would need a `Caller` provider |
| bearer remote references, or explicit `delegate!` and `save!` | a leaked bearer token is leaked authority; storing an id and looking it up again needs no new API |
| capture cells shared across threads, with every captured `let` not `Send` | it rejects ordinary closures; copying captures at the crossing removes the race instead |
| a `&mut T` lent to a scoped thread | two holders of one place across threads; a thread gets a value and returns it instead |
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
| `Opaque[T]` for server state | a `*mut dyn T` of a `Remote` trait |
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
| 9 | Code For Another Target: Remote | parses; `# hypothetical syntax` |
| 10 | What Std Adds | parses after two fixes of mine: an empty trait body is `pass` on its own line, and a built-in is written `@intrinsic` with a `panic` body, as `std.task` does |
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
