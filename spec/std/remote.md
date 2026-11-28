# Remote

Status: standard library specification draft.

This chapter defines the plain hd parts of `std.remote`: the homes a
domain serves from, served functions, the HTTP adapters, and workers. They are
ordinary std code over the language items of
[Remote Built-Ins](../lang/10-modules.md#remote-built-ins).

## Homes

A home answers the calls that reach one domain. Its **request factory**
builds a root and a session for each incoming call record:

```text
use std.http.Request
use std.random.Random
use std.remote.{CrossError, Home, Refused, Remote, Rooted, Session, per_request}
use std.time.Clock

trait Greeter < Remote:
    fn greet!(&mut self, name: string) -> Result[string, string | CrossError]

struct Polite:
    caller: string

impl Greeter for Polite:
    fn greet!(&mut self, name: string) -> Result[string, string]:
        .Ok("hello $name, from ${self.caller}")

fn serving() -> *mut Home[Polite, Request] $ Random + Clock:
    per_request(fn!(request: &Request) -> Result[Rooted[Polite], Refused]:
        match request.cookie("session"):
            .Some(id) => .Ok(Rooted { root: Polite { caller: id }, session: Session { id } })
            .None => .Err(Refused { reason: "no session" })
    )
```

1. r[std-remote.home.per-request] `per_request(make)` returns a new `*mut Home[S, C]` whose request factory is `make`. `make` is a `fn!(&C) -> Result[Rooted[S], Refused]` closure.
2. r[std-remote.home.per-request.static] The home outlives the call that made it, so `make` captures no local reference. `per_request` takes it for a type parameter bounded by `Fn(&C) -> Suspend[Result[Rooted[S], Refused]] & 'static`. Error: `local-reference-escapes`.
3. r[std-remote.home.fixed] `home(root, session)` returns a new `*mut Home[S, C]` whose factory accepts every call record with a copy of `root` and `session`.
4. r[std-remote.home.fixed.copy] Each call runs on its own copy of `root`. State that must outlive one call sits behind a box that `root` holds.
5. r[std-remote.home.providers] `per_request` and `home` have the row `$ Random + Clock`, and the home keeps both providers.
6. r[std-remote.home.token] Each object that a method returns gets a 64-bit token drawn from the kept `Random`, by [`module.remote.session`](../lang/10-modules.md#r-module.remote.session).
7. r[std-remote.home.lease] An object's lease ends 10 minutes after its last call ends, by the kept `Clock`'s monotonic time, as [`module.remote.lease`](../lang/10-modules.md#r-module.remote.lease) requires.

> **Why.** A server builds a root per request, so the root can hold the
> caller its session proves, and two interleaved requests never share one.
> A worker has one peer, so one root and session serve every call. Java
> RMI's default lease is also 10 minutes.

## Served Functions

A **served function** is a closure that a home serves as a remote object:

1. r[std-remote.action.call] `std.remote` declares the remote trait `pub trait Call[A < Wire, T < Wire, E < Wire] < Remote` with one method, `fn call!(&mut self, arg: A) -> Result[T, E | CrossError]`.
2. r[std-remote.action.closure] `action(code)` returns a `*mut dyn Call[A, T, E]` whose `call!` runs `code` with its argument and returns `code`'s result.
3. r[std-remote.action.static] `code` captures no local reference, since the object outlives the call. Its captures need not be `Wire`. Error: `local-reference-escapes`.

## HTTP

`http_transport` and `http_handler` carry frames over `std.http`: a
frame is the body of a `POST`, and the reply is the body of its response.

```text
use std.http.{Http}
use std.url.{Url}
use std.remote.{CrossError, Remote, http_transport, remote}

trait Greeter < Remote:
    fn greet!(&mut self, name: string) -> Result[string, string | CrossError]

fn greet!(url: Url) -> string $ Http:
    let mut greeter = remote::[Greeter](http_transport(url))
    match greeter.greet!("Ada"):
        .Ok(text) => text
        .Err(._0(error)) => error
        .Err(._1(cross)) => "not sent: $cross"
```

1. r[std-remote.http.transport] `http_transport(url)` returns a `*mut dyn Transport` that keeps the current `Http` provider. Its row is `$ Http`.
2. r[std-remote.http.transport.post] Each `call!` sends one `POST` to `url` whose body is the frame, through the kept provider, following no redirect.
3. r[std-remote.http.transport.reply] A response with a success status gives its body as the reply.
4. r[std-remote.http.transport.forbidden] A `403 Forbidden` response is `CrossError.WrongHome`: the home refused the caller.
5. r[std-remote.http.transport.unreachable] An `HttpError` from before the request left is `CrossError.Unreachable`: `NotGranted`, `InvalidUrl`, `Dns`, `Connect`, `Tls`, `InvalidHeader` and `UnsupportedSetting`.
6. r[std-remote.http.transport.unknown] Every other `HttpError`, another failure status and a body that cannot be read are `CrossError.Unknown`.
7. r[std-remote.http.handler] `http_handler!(request, home)` answers one request for `home`, with the handler type `fn!(Request, &mut Home[S, Request]) -> Response`.
8. r[std-remote.http.handler.refused] It first runs the home's factory on `request`. A refusal is a `403 Forbidden` response, and no frame is read.
9. r[std-remote.http.handler.body] A request body that cannot be read is a `400 Bad Request` response.
10. r[std-remote.http.handler.reply] Otherwise it passes the body to `handle_call!` with the factory's `Rooted`, and answers `200 OK` with the encoded `Reply` as the body.

> **Why.** A failure before sending means the call did not run, so the
> caller may retry it. After sending, the call may have run, which
> `Unknown` says.

## Workers

A **worker** is another instance of the program's own module, in its own
memory. A crossing to a worker sends the identity of a closure the worker
already has and its captures, copied or moved, by
[`fn.crossing.lift`](../lang/07-functions.md#r-fn.crossing.lift):

```text
use std.remote.{CrossError, Workers}

struct Image:
    width: u32
    pixels: List[u8]

impl !Copy for Image

fn widths!(workers: *mut dyn Workers, photo: Image) -> Result[u32, CrossError]:
    workers.run!(fn() -> u32: photo.width * 2)   # photo moves to the worker
```

1. r[std-remote.workers.trait] `std.remote` declares the host capability trait `@host pub trait Workers` with one method, `fn send!(&mut self, frame: List[u8]) -> Result[List[u8], CrossError]`, which carries one crossing's frame to a worker and returns its encoded `Reply`.
2. r[std-remote.workers.run] `workers.run!(code)`, an inherent method of `dyn Workers` taking `code: fn() -> T + Wire` for `T < ?Copy & Wire`, sends `code` with the build ID and returns its result: `.Ok` with the value the worker's run of `code` returned.
3. r[std-remote.workers.run.cross] A failure of `send!` is its `CrossError`; a refusal is the refused `CrossError`; a reply or a result that does not decode is `CrossError.Unknown`.
4. r[std-remote.workers.answer] `worker_answer(frame)` answers one frame in the current instance with an encoded `Reply`: another build is `Refused(OtherBuild)`, a frame that names no closure of this build or whose captures do not decode is `Refused(WrongHome)`, and otherwise the closure runs and its encoded result is `Ran`.
5. r[std-remote.workers.loopback] `LoopbackWorkers` is a `Workers` whose `send!` answers with `worker_answer` in the current instance, for tests.
6. r[std-remote.workers.host] The host's `Workers` sends each frame to a worker of its own, which it starts at the first send. A worker whose instance ended, or a host without workers, makes that send `CrossError.Unreachable`; the next send starts another worker.
7. r[std-remote.workers.port] `std.remote` declares `@host pub trait WorkerPort` with `fn next(&mut self) -> List[u8]?`, which waits for the next frame sent to this instance, `.None` once none is left, and `fn reply(&mut self, frame: List[u8]) -> void`, which answers it. In an instance that is no worker, `next` returns `.None`.
8. r[std-remote.workers.serve] `worker_serve()`, with the row `$ WorkerPort`, replies to each frame that `next` gives with `worker_answer` of it, and returns at `.None`. A program that sends a crossing to a worker exports it as `hd.worker`, which a worker runs after `hd.init`.

> **Why.** A worker runs the same build, so a closure crosses as a
> number and its captures, never as code, and a deploy is one refusal.
> Values are copied between memories, never shared, so no count is
> atomic.

See also: [Remote Traits](../lang/10-modules.md#remote-traits),
[Crossing Errors](../lang/10-modules.md#crossing-errors),
[Http](http.md).
