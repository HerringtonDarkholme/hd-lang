# Remote

Status: standard library specification draft.

This chapter defines `std.remote`, the std items of remote calls, which
`lib/std` writes in ordinary hd over the language tier:

- the error enum `RemoteError`, why a remote call failed on its way;
- the requirement trait `Caller` and the enum `Principal` it returns;
- the host capability trait `Remote`.

[Places and Remote Calls](../lang/15-places-and-remote-calls.md) defines
places and edges, and its status note lists the remote-call forms that are
not specified yet.

## Remote Errors

`RemoteError` says why a remote call failed between the caller and the
callee's home. It never holds the called function's own error:

```text
pub enum RemoteError:
    Unreachable
    OutcomeUnknown
    Incompatible
    Rejected(message: string)
    Panicked(category: string)
```

1. r[std-remote.error.decl] `std.remote` declares the enum `RemoteError` as above. Code imports it, as in `use std.remote.RemoteError`.
2. r[std-remote.error.unreachable] `Unreachable` means that the call never started.
3. r[std-remote.error.outcome-unknown] `OutcomeUnknown` means that the call started and no result arrived, so the callee may or may not have run.
4. r[std-remote.error.incompatible] `Incompatible` means that the receiving place has no function or closure with the call's identity.
5. r[std-remote.error.rejected] `Rejected(message)` means that the receiving place refused the call's arguments, and `message` says why.
6. r[std-remote.error.panicked] `Panicked(category)` means that the remote run panicked, and `category` is the panic's category.
7. r[std-remote.error.traits] `RemoteError` implements `Eq`, `Debug`, `Display`, and `std.error.Error`.
8. r[std-remote.error.text] The `Display` text of each variant is the text in the table below, with the payload for `MESSAGE` or `CATEGORY`.

| Variant | `Display` text |
| --- | --- |
| `Unreachable` | `the remote call never started` |
| `OutcomeUnknown` | `the remote call started, and no result arrived` |
| `Incompatible` | `the receiver has no function or closure with this identity` |
| `Rejected(message)` | `the receiver rejected the arguments: MESSAGE` |
| `Panicked(category)` | `the remote run panicked: CATEGORY` |

> **Why.** The failure belongs to the call, not to the function, as gRPC's
> status codes and Java RMI's `RemoteException` sit beside a method's own
> results. So a function's signature stays the same wherever it runs.

See also: [Error](error.md#standard-error-types).

## Callers

`Caller` names the user or service whose remote call the current run
serves:

```text
pub enum Principal:
    User(name: string)
    Service(name: string)
    Anonymous

pub trait Caller:
    fn principal(self) -> Principal
```

1. r[std-remote.principal.decl] `std.remote` declares the enum `Principal` as above. Code imports it, as in `use std.remote.Principal`.
2. r[std-remote.principal.user] `User(name)` is a signed-in user, named by `name`.
3. r[std-remote.principal.service] `Service(name)` is a service or another place, named by `name`.
4. r[std-remote.principal.anonymous] `Anonymous` is a caller that no one has identified.
5. r[std-remote.principal.traits] `Principal` implements `Eq` and `Debug`.
6. r[std-remote.caller.decl] `std.remote` declares the requirement trait `Caller` as above. Code imports it, as in `use std.remote.Caller`.
7. r[std-remote.caller.plain] `principal` is a plain call, not a bang call.
8. r[std-remote.caller.not-host] `Caller` is not a host capability trait. So an entry point whose row holds it is an error, by [`module.entry.row.host`](../lang/10-modules.md#r-module.entry.row.host). Error: `nonhost-entry-requirement`.

```text
use std.remote.Caller

fn greet() -> string $ Caller:
    match $.use(Caller).principal():
        .User(name) => "hello, ${name}"
        .Service(name) => "service ${name}"
        .Anonymous => "please sign in"
```

```text
use std.remote.Caller

pub fn main() -> void $ Caller:  # error: nonhost-entry-requirement
    pass
```

> **Why.** One enum covers the three kinds of caller, so a function
> matches on it in one place. Authorization stays ordinary code in the
> function, as a server action or a gRPC handler checks its caller.

## The Remote Capability

`Remote` is the capability a function needs to make remote calls.

1. r[std-remote.remote.decl] `std.remote` declares the host capability trait `Remote`. Code imports it, as in `use std.remote.Remote`.
2. r[std-remote.remote.method] `Remote`'s method serves the implementation of remote calls. This chapter does not specify it.

> **Note.** Until remote calls are specified, no runtime profile binds
> `Remote`. An entry point whose row holds it is then an error, by
> [`module.entry.row.host`](../lang/10-modules.md#r-module.entry.row.host).

See also: [Places and Remote Calls](../lang/15-places-and-remote-calls.md).
