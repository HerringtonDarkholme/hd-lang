# Remote

Status: standard library specification draft.

This chapter defines the plain hd parts of `std.remote`: the homes a
domain serves from, served functions, and the HTTP adapters. They are
ordinary std code over the language items of
[Remote Built-Ins](../lang/10-modules.md#remote-built-ins).

## Homes

A home answers the calls that reach one domain. Its **request factory**
builds a root and a session for each incoming call record:

```text
use std.http.Request
use std.remote.{CrossError, Home, Refused, Remote, Rooted, Session, per_request}

trait Greeter < Remote:
    fn greet!(&mut self, name: string) -> Result[string, string | CrossError]

struct Polite:
    caller: string

impl Greeter for Polite:
    fn greet!(&mut self, name: string) -> Result[string, string]:
        .Ok("hello $name, from ${self.caller}")

fn serving() -> *mut Home[Polite, Request]:
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

> **Why.** A server builds a root per request, so the root can hold the
> caller its session proves, and two interleaved requests never share one.
> A worker has one peer, so one root and session serve every call.

## Served Functions

A **served function** is a closure that a home serves as a remote object:

1. r[std-remote.action.call] `std.remote` declares the remote trait `pub trait Call[A < Wire, T < Wire, E < Wire] < Remote` with one method, `fn call!(&mut self, arg: A) -> Result[T, E | CrossError]`.
2. r[std-remote.action.closure] `action(code)` returns a `*mut dyn Call[A, T, E]` whose `call!` runs `code` with its argument and returns `code`'s result.
3. r[std-remote.action.static] `code` captures no local reference, since the object outlives the call. Its captures need not be `Wire`. Error: `local-reference-escapes`.

## HTTP

`http_transport` and `http_handler` carry frames over `std.http`: a
frame is the body of a `POST`, and the reply is the body of its response.

```text
use std.http.{Http, Url}
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

See also: [Remote Traits](../lang/10-modules.md#remote-traits),
[Crossing Errors](../lang/10-modules.md#crossing-errors),
[Http](http.md).
