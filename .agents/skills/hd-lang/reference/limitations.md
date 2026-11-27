# What hd cannot do yet

hd is experimental. This page lists what the compiler or standard library
does not do today, what to write instead, and where the work is tracked. It
was checked against the installed compiler. When a row looks wrong, try the
code: the compiler is the authority, and a limit here may have been lifted.

Issue links point to
[HerringtonDarkholme/hd-lang](https://github.com/HerringtonDarkholme/hd-lang).

## Language and standard library

| You want | Today | Write instead | Tracked in |
| --- | --- | --- | --- |
| Keep a closure that arrived as a function parameter in a struct field, list or map | `local-reference-escapes`: the parameter may hold the caller's local references | an `enum` of actions and one function that matches it ([porting](../scenarios/porting.md#stored-callbacks-become-data)); or a generic field `F < Fn(i32) -> i32 & 'static` ([keeping a closure](references.md#keeping-a-closure)) | [#2409](https://github.com/HerringtonDarkholme/hd-lang/issues/2409), blocked by [#1436](https://github.com/HerringtonDarkholme/hd-lang/issues/1436) |
| Read a file or stdin as a stream, chunk by chunk | no `std.io`; `read_text!` and `read_bytes!` return the whole file | stdin: `read_line!` one line at a time, or `read_all!`; files: read whole and split | [#2411](https://github.com/HerringtonDarkholme/hd-lang/issues/2411): accepted, a `Read` trait and `BufReader` are planned |
| Hold invalid UTF-8 in a `string` | `string` is always valid UTF-8, by decision | keep the data as `List[u8]`; `string::from_utf8(bytes)` returns a `Result` and fails on bad bytes | [#2410](https://github.com/HerringtonDarkholme/hd-lang/issues/2410): `string::from_utf8_lossy` is planned, not in the compiler yet |
| Write raw bytes to the console | `Console` writes text only | `write_bytes!(Path("/dev/stdout"), bytes)` from `std.fs`, with `$ FsWrite` granted | [#2410](https://github.com/HerringtonDarkholme/hd-lang/issues/2410) |
| Profile a program | `hd run` has no profile option; release builds carry no symbols | time sections with `std.time` `monotonic` and `elapsed`; cap a run with `--time-limit` ([performance](../scenarios/performance.md#finding-a-hot-spot-without-a-profiler)) | [#2406](https://github.com/HerringtonDarkholme/hd-lang/issues/2406): `--profile` is planned |
| Take a slice that shares memory with its list | `xs.slice(a, b)` copies, so repeated slicing is quadratic | index into the list, or read `xs.view(a, b)` ([performance](../scenarios/performance.md#when-copies-happen)) | no issue: copy-on-write is the design |
| Share a list or map between two holders by assigning it | assignment copies | `box(list)`, a `*mut List[T]`, or lend `&mut` ([porting](../scenarios/porting.md#values-copy-sharing-is-explicit)) | no issue: value semantics are the design |

What does work, so you do not give up early: a named function or a closure
written in place fits a plain `fn(...)` field, including one that captures
values. The `local-reference-escapes` error is only for a closure that came
in as a parameter.

## Workflows, closures that travel, and hosts

These come from the
[Using hd guide](https://github.com/HerringtonDarkholme/hd-lang/blob/main/website/guide/USING_HD.md)
and [Use Scenarios](https://github.com/HerringtonDarkholme/hd-lang/blob/main/docs/use-scenarios.md).
Plugins and sandboxes work today; the rows below do not.

| You want | Today | Write instead | Tracked in |
| --- | --- | --- | --- |
| A durable workflow that resumes after a crash without repeating external calls | `!` functions give the pause points and replay rule; durable storage, retries and a resuming runner are not built | write the steps as `!` functions, and keep your own record of finished steps | the guide, "As a workflow"; [Use Scenarios](https://github.com/HerringtonDarkholme/hd-lang/blob/main/docs/use-scenarios.md#durable-workflows-and-recovery) |
| A serializable closure: `fn!() -> T + Wire` or `+ Send` as an RPC, a thread body or a worker script | specified, not built | send plain data (a struct or enum) and call a named function where it lands | [#2288](https://github.com/HerringtonDarkholme/hd-lang/issues/2288) |
| A host calling any exported hd function directly | the host runs the module's entry point, and the plugin calls back through `@host` traits | define the host API as an `@host` trait ([capabilities](../design/capabilities.md)) | the guide, "As a plugin": not designed yet |
| Load a host plugin as a native dynamic library | hd never loads native libraries; the host passes providers in by value | implement the generated Rust trait (Wasmtime) or a JavaScript provider module (Node) | the guide, "As a plugin": not designed yet |
| Derive a tool adapter for a function (an agent tool, a service endpoint, a job) | undecided; a decorator attaches a plain value that an adapter reads with `facts_of` | register each tool by hand | [#833](https://github.com/HerringtonDarkholme/hd-lang/issues/833), "parked tool adapters" |
| Deploy or synchronize code to a provider through generated adapters | the same registration model is planned for endpoints, jobs and workflows | register and deploy outside hd | [Use Scenarios](https://github.com/HerringtonDarkholme/hd-lang/blob/main/docs/use-scenarios.md) |

When you hit a limit not listed here, run the code, read the diagnostic,
and look in [explore](explore.md) for where to find the answer.
