# Using hd

hd compiles to a WebAssembly module, and the module reaches the world only
through the capabilities its signatures name. That makes it a good fit for
code you embed, code you don't trust, and code that waits.

This page covers four uses. Plugins and sandboxes work today. Resumable
workflows and serializable closures are **work in progress**: the parts that
run are shown, and the rest is marked as planned.

## As a plugin: your application runs hd code

A host application loads an hd module, supplies the capabilities it needs,
and runs it. The host stays in control of what the plugin can reach.

**How the plugin and the host meet.** A package declares the host API as an
ordinary trait marked `@host`, and the plugin calls it like any other trait.
The host implements that trait as a *provider*:

- **On Node,** the provider is a JavaScript module the package names in its
  `hd.toml`. Generated glue handles the Wasm ABI, so the provider exports
  plain methods.
- **On Wasmtime,** `hd build --engine wasmtime` writes the module and a
  generated Rust companion beside it:

  ```sh
  hd build --engine wasmtime
  # main.wasm       the plugin
  # main.host.rs    one Rust trait per host interface, with its codecs
  ```

  Your application implements those Rust traits and registers them when it
  instantiates the module, through `hd_run_wasmtime::native`.

**What the host controls.**

- **Two keys.** A provider runs only when the host registered it *and* the
  application's manifest consents to its trait. Registering a provider grants
  the plugin nothing by itself.
- **Checked before it starts.** The host's registrations are checked against
  the module's contract (identity, method schemas, suspension, codecs)
  before the module links. A mismatch refuses to start and names the trait.
- **No dynamic loading.** hd never discovers or loads a native library for a
  host trait. The host passes its providers in by value.
- **Waiting calls.** A host method can answer at once or return a pending
  call. Aborting a call or closing the instance runs its abort hook at most
  once, and a late answer is ignored.

The [server SDK](../../packages/server-sdk/README.md) is a complete example:
the host owns the HTTP listener and hands each request to the hd
application, which answers through `std.http` values.

**Planned.** Today the host runs the module's entry point and the plugin
calls back through host traits. A host calling arbitrary exported hd
functions directly, and loading plugins as native dynamic libraries, are
not designed yet.

The rules are in [Native Package Providers](../../spec/cli/command-line.md#native-package-providers)
and [package host traits](../../spec/lang/10-modules.md#package-host-traits).

## As a sandbox: run code you didn't write

Every effect a function can have is in its signature, after `$`: printing is
`Console`, reading files is `FsRead`, the network is `Net`. Code an agent
generates can't reach anything its signature doesn't name, and you can see
what it asks for before you run it.

This program reads a file:

```hd
use std.fs.{FsRead, read_text}
use std.path.Path

fn count_lines!(path: string) -> usize $ FsRead:
    match read_text!(Path(path)):
        .Ok(text) => text.lines().len()
        .Err(_) => 0

pub fn main!() -> void $ Console + FsRead:
    println("${count_lines!("hd.toml")} lines")
```

Its package grants only `Console`:

```toml
[package]
name = "sandboxed"

[capabilities]
Console = true
```

So `hd run` refuses before any code runs:

```text
error: src/main.hd:9:24: missing-capability: `main` requires `FsRead` in `$ Console + FsRead`, but `[capabilities]` in hd.toml does not list it
    fix: list the missing capabilities in hd.toml
```

Adding `FsRead = true` grants it, and the program prints its count.

What keeps the sandbox closed:

- **Capabilities are explicit.** A grant names one of `Console`,
  `ConsoleInput`, `Args`, `Env`, `Clock`, `Random`, `FsRead`, `FsWrite`,
  `Process`, `Http`, `Net` or `Sys`. Anything else is a manifest error.
- **Refusal comes first.** A missing capability stops the run before the
  module starts, not halfway through.
- **WebAssembly isolation.** The module has its own linear memory, and it
  reaches the host only through the capabilities it was granted. On Wasmtime
  it runs in process; on Node it runs on V8.
- **Fakes for tests.** Each standard capability has a fake, so a test can fix
  the clock, the random source or the files a program sees.

## As a workflow (work in progress): steps that can resume

A function that waits ends in `!`, and so does every call that waits. Each
`!` call is a point where the computation can pause and continue later:

```hd
use std.time.{Clock, ms, sleep}

fn fetch_order!(id: i32) -> string $ Clock:
    sleep!(1ms)
    "order ${id}"

fn charge!(order: string) -> string $ Clock:
    sleep!(1ms)
    "charged ${order}"

pub fn main!() -> void $ Clock + Console:
    order := fetch_order!(42)
    println(charge!(order))
```

It prints `charged order 42`.

**Works today.** Suspension compiles to a state machine. Every pause point is
visible in the source, every effect is a capability, and a run given the same
inputs in the same order reproduces the same instance
([`req.determinism.replay`](../../spec/lang/11-requirements-and-suspension.md)).

**Work in progress.** A durable workflow saves its progress at those pause
points. After a crash or a deploy it resumes without repeating the external
calls it already made, and shows which work it replayed. The language
supplies the pause points and the replay rule. Durable storage, retries, and
the runner that resumes a workflow are runtime and library work, and are
not built yet. The requirements are in
[Use Scenarios](../../docs/use-scenarios.md#durable-workflows-and-recovery).

## Serializable closures (work in progress)

A serializable closure is code you can send somewhere else and run there,
with the values it captured. Three uses motivate it:

- **As RPC.** Send a function to the service that holds the data, rather than
  pulling the data back to the caller.
- **As a thread.** Run a closure on another thread, with the type checker
  making sure what it captured is safe to move.
- **As a worker script.** Ship a closure to a worker, such as a browser Web
  Worker or an edge function, as its program.

**Status.** Not built. The specification defers it: today a closure can't be
saved or restored ([`fn.capture.serializable-deferred`](../../spec/lang/07-functions.md)).
The design options are in [PR #1139](https://github.com/HerringtonDarkholme/hd-lang/pull/1139):
named code with copied data, portable closures placed by requirement rows,
typed crossing contracts, or `Send`-style threads. They wait on the owner's
choice, so this page shows no syntax for them yet.
