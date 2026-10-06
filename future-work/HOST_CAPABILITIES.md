# Host Capabilities And Permissions (Task N1)

> **Applied to the spec, task N2 (2026-10-06).** The approved decisions
> are in [Host Capabilities](../spec/cli/command-line.md#host-capabilities),
> [Capability Grants](../spec/cli/command-line.md#capability-grants),
> [Test Environments](../spec/cli/command-line.md#test-environments),
> [Processes](../spec/lang/10-modules.md#processes),
> [Host Boundary](../spec/lang/10-modules.md#host-boundary),
> [Http](../spec/std/http.md), [Net](../spec/std/net.md) and
> [Sys](../spec/std/sys.md). The prototype follows in task N3; the gap is
> the CAPS tag in `test/portable/KNOWN_FAILURES.tsv`. Open points are in
> [Open Issues](OPEN_ISSUES.md#runtime-library-abi-and-tooling-work).

> **Owner feedback, 2026-10-06. It overrides the recommendations below
> where they differ.**
>
> 1. Two levels of control: yes. Requirement rows are static; a run-time
>    grant scopes them.
> 2. Grants are written in `hd.toml`, and CLI flags override them.
> 3. **The default is everything granted.** A program gets every capability
>    unless `hd.toml` has a grant table or a CLI flag restricts it. This
>    reverses H3, deny by default.
> 4. **Every capability is gated,** including Console, ConsoleInput, Clock,
>    Random and Args. This reverses "ungated, as in Deno".
> 5. **Two tiers of deny,** with details proposed below.
> 6. **The playground grants `Http` to its own origin only,** as the browser's
>    same-origin rule does.
> 7. **Naming:** "permissions" is rejected. The name is open; see below.
>
> **Orchestrator's proposals on 0 and 5, awaiting the owner's OK:**
>
> - **Name:** a `[capabilities]` table keyed by the requirement-row trait
>   names (`FsRead = ["data/"]`, `Http = ["api.example.com"]`,
>   `Console = true`, `Process = false`), with matching flags
>   `--cap Http=api.example.com` and `--cap Process=false`. This drops
>   Deno's read/net/run category names.
> - **Total deny** (`Process = false`) is a **startup error**. The owner
>   noted that `hd run` also runs prebuilt Wasm, so it cannot be a compile
>   error. Before `main` runs, the host compares the module's needs with
>   the grants, and refuses to start when a needed capability is totally
>   denied. The refusal names the capability and the setting that denied
>   it. The needs come from the module's Wasm import list
>   (`WebAssembly.Module.imports()`): every host-capability call is an
>   import, and dead code is already removed, so no custom section is
>   needed. `hd check` may report the same problem early, as a diagnostic;
>   the normative rule is the startup refusal.
> - **Partial deny** (a scoped list) returns `NotGranted` through the call's
>   `Result`, never a panic. The resource is known only at run time, and
>   the program can recover.
>
> **Owner, 2026-10-06: the name is `capabilities`.** The grant table is
> `[capabilities]` in `hd.toml`, keyed by the requirement-row trait names
> (`FsRead`, `FsWrite`, `Http`, `Net`, `Env`, `Process`, `Sys`, `Console`,
> …), and the flags are `--cap Name=…`.
>
> **Owner, 2026-10-06: keep sockets.** A `Net` capability (TCP, UDP, DNS),
> scoped by `host:port`, stays in this design beside `Http`, and is no
> longer deferred. The prototype sessions still start with the HTTP
> client. The other suggestions (never prompt, `NotGranted` per error
> enum, ungranted `Env.get` returns `.None` with a notice, no `ffi`, park
> runtime loading, Process and Http in the default profile) are approved.


Status: design proposal, 2026-10-06. Nothing in it is accepted behavior.
Every decision below waits for the owner or the orchestrator, as each one
says. The spec outline is a draft and is not applied to `spec/`.

Under review:
- [Requirements and Suspension](../spec/lang/11-requirements-and-suspension.md),
  above all [Provider Access](../spec/lang/11-requirements-and-suspension.md#provider-access)
  and [Runtime Boundary](../spec/lang/11-requirements-and-suspension.md#runtime-boundary);
- [Runtime Profiles](../spec/lang/10-modules.md#runtime-profiles),
  [Processes](../spec/lang/10-modules.md#processes),
  [Registration](../spec/lang/10-modules.md#registration),
  [Boundary-Safe Values](../spec/lang/10-modules.md#boundary-safe-values) and
  [Host Boundary](../spec/lang/10-modules.md#host-boundary);
- [Host Capabilities](../spec/cli/command-line.md#host-capabilities) and
  [Test Environments](../spec/cli/command-line.md#test-environments) in the
  CLI tier;
- [Fs](../spec/std/fs.md), [Host](../spec/std/host.md) (which holds `Env`),
  [Process](../spec/std/process.md) and
  [Unit Test Providers](../spec/std/testing.md#unit-test-providers);
- the parked host-config sketch in
  [Open Issues](OPEN_ISSUES.md#runtime-library-abi-and-tooling-work), which
  this record would replace;
- the goal metrics of
  [New Compiler: Architecture Notes](NEW_COMPILER_ARCHITECTURE.md#goal-metrics-proposal-2026-10-06-awaiting-the-owners-edits).

## Owner's Words (2026-10-06)

- "queue up a work to allow net capability. you now cannot even send http
  request."
- "not just http. follow something like
  https://docs.deno.com/runtime/reference/permissions/ and design your
  stdlib accordingly."
- "probably we don't need ffi, since host cap already covers it. check it
  in N1."
- "dynamic import is interesting, does that apply to hd-lang? or wasm lang
  in general? if it is hard to design, we can park import part." This
  means loading code at run time, not lazy imports at build time.
- Prototype work "depends on the work amount. 3 one hr session at most".

## Summary

hd keeps two levels of authority:

| Level | Says | Checked | Written in |
| --- | --- | --- | --- |
| **Requirement** (exists) | which host traits a function may use: `$ FsRead + Http` | at compile time, by the row rules | the source |
| **Grant** (new) | which resources those traits may touch: paths, hosts, variable names, programs | at run time, by the host provider | `hd.toml` `[permissions]` and `--allow-*` / `--deny-*` flags |

The row stays the static summary that a reader and the compiler check. The
grant is the runner's consent, given outside the program. It follows Deno's
categories and scope syntax, with three changes:
- **Deny by default, never prompt.** Agents can't answer a prompt, and a
  prompt would be a hidden input to an otherwise deterministic run.
- **A denial is an error value.** Each error enum gets a `NotGranted`
  variant, apart from the OS's `PermissionDenied`, as Deno 2 split
  `NotCapable` from `PermissionDenied`.
- **The traits are the categories.** `read` gates `FsRead`, `write` gates
  `FsWrite`, `net` gates `Http`, `run` gates `Process`, `env` gates `Env`
  and `sys` gates `Sys`. `Console`, `ConsoleInput`, `Args`, `Clock` and
  `Random` stay ungated, as in Deno.

Other findings:
- **FFI: the owner's hypothesis holds.** A Wasm module reaches nothing
  outside itself but its imports. So a host trait that an embedder binds
  already is hd's foreign function interface, and hd needs no `ffi`
  permission and no `extern` syntax. One gap remains: a trait can't yet
  return an opaque handle to a native object.
- **Runtime code loading applies to every Wasm language, but only as a
  host capability.** No Wasm instruction loads code, so the host must
  instantiate it. Six hard parts block it, so the recommendation is to
  park it (the [parked text](#parked-open-issues-text) is below).
- **Stdlib:**
  - `std.http` comes first: one `Http` trait with `send!`, a `get!`
    helper, `ScriptedHttp` for tests, and browser `fetch` in the
    playground.
  - `Process` exists already; it joins the default profile behind `run`.
  - `std.sys` is four gated reads, and comes last.
  - Sockets and an HTTP server wait.
- **Prototype:** HTTP with the `net` grant fits one session. The full
  grant model and its migration take two more. Everything else goes to the
  new compiler.

## Research

Sources were fetched on 2026-10-06; links are under [Sources](#sources).

### Deno Permissions

| Category | Flag (short) | Scope syntax | Gates |
| --- | --- | --- | --- |
| read | `--allow-read` (`-R`) | comma-separated paths; a directory covers everything below it | file system reads |
| write | `--allow-write` (`-W`) | paths, as for read | file system writes |
| net | `--allow-net` (`-N`) | `host`, `host:port`, `*.example.com`, an IP with a port, `[ipv6]` | HTTP, TCP and UDP sockets, listening |
| env | `--allow-env` (`-E`) | variable names; a `PREFIX_*` suffix wildcard | reading and setting variables; `--ignore-env` reads as `undefined` instead of failing |
| sys | `--allow-sys` (`-S`) | API names: `hostname`, `osRelease`, `cpus`, `networkInterfaces`, `systemMemoryInfo`, `uid`, `gid`, ... | OS information |
| run | `--allow-run` | program names | subprocesses |
| ffi | `--allow-ffi` | library paths | loading native libraries |
| import | `--allow-import` | hosts; a default list (`jsr.io`, `deno.land`, `esm.sh`, ...) | remote imports computed at run time |

| Topic | Deno's rule |
| --- | --- |
| Default | "no access to read or write arbitrary files ..., to make network requests ..., to access environment variables, or to spawn subprocesses." |
| Precedence | "`--deny-*` flags override their `--allow-*` counterparts." |
| Allow all | `-A` / `--allow-all` "disables the security sandbox entirely." |
| Prompting | asks on a terminal when no flag decides; `--no-prompt` turns that off; no prompt without a TTY |
| Denial error | Deno 2 raises `Deno.errors.NotCapable` for a missing flag and keeps `PermissionDenied` for the OS's refusal, "to make it easier to discriminate between OS-level errors and Deno errors" |
| Escapes | a subprocess "run[s] independently from the permissions granted to the parent"; a native library "can issue system calls directly" |
| Config | `deno.json` `"permissions"` holds named sets of the same keys (`true`, a list, or `{allow, deny, ignore}`); `-P` selects `"default"`, `-P=NAME` another |
| Tests | the `test`, `bench` and `compile` sections hold a set or name one, and then `deno test` must get `-P` or a flag; `Deno.test` also takes a per-test `permissions` option |
| Static imports | "loaded by the runtime without consulting the permission system"; a computed `import()` needs `--allow-read` or `--allow-import` |
| Runtime API | `Deno.permissions.query`, `request` and `revoke` |

### WASI And Wasm Hosts

| Source | Model |
| --- | --- |
| `wasi:filesystem` | no ambient authority: every call takes a directory descriptor and a path that is "sandboxed to be resolved within that directory"; preopens give the first descriptors |
| wasmtime `WasiCtxBuilder` | `preopened_dir(host, guest, DirPerms, FilePerms)`, `env`, `inherit_env`, `allow_tcp`, `allow_udp`, `allow_ip_name_lookup`, `inherit_network`, and `socket_addr_check`, a callback "called for each socket address that is used" |
| `wasi:sockets` | deny by default: modules "can not open sockets by themselves without a network capability handle"; how a runtime grants access is host policy, which the proposal leaves open |
| `wasi:http` 0.3 (WASI 0.3, released 2026-06-11) | one `handler` interface, `handle: async func(request: request) -> result<response, error-code>`, imported to send and exported to serve; the `service` world replaces 0.2's `proxy`, and `middleware` adds forwarding; `error-code` has an `HTTP-request-denied` case |
| wasmtime | Wasmtime 46 is to ship WASI 0.3 and component async on by default; a `Store` limits memory (`limiter`), fuel and epoch deadlines |
| Wasm security model | "Each WebAssembly module executes within a sandboxed environment ..." and "can't escape the sandbox without going through appropriate APIs" |
| Component model | "A component interacts with a runtime or other components only by calling its imports and having its exports called"; a component "may not export a memory" |

### Runtime Loading And Plugins

| System | Loads | Interface check | Data crossing | Limits |
| --- | --- | --- | --- | --- |
| JS `WebAssembly.instantiate` | bytes or a compiled `Module` | each import must be in `importObject`, else `LinkError`; exports are untyped to the caller | numbers and references; GC references pass if the types canonicalize alike | none per instance; a Worker can be terminated |
| wasmtime `Linker` + `Store` | modules and components | link-time import types; WIT types for components | the canonical ABI copies values | `limiter`, fuel, epochs |
| Component model | components | WIT worlds | lift and lower, shared-nothing | host policy |
| Extism | `.wasm` by path, URL or bytes, with an optional `sha256` | host function signatures | bytes, often JSON | `memory.max_pages`, `allowed_hosts`, `allowed_paths`, `timeout_ms` |
| Deno `import()` | JS modules | none (dynamic) | same heap | none: same permissions as the importer |
| Wasm GC | struct and array types | iso-recursive types; "two subtypes are equivalent if their structure is equivalent" | references across modules in one engine | none |

### hd Today

| Item | Where | State |
| --- | --- | --- |
| Rows name what a function may use | [Requirement Rows](../spec/lang/11-requirements-and-suspension.md#requirement-rows) | specified |
| Entry rows hold only the profile's host traits | [`module.entry.row.host`](../spec/lang/10-modules.md#r-module.entry.row.host) | specified |
| A profile is a toolchain-defined trait set; a manifest can't define one | [`module.profile.toolchain-names`](../spec/lang/10-modules.md#r-module.profile.toolchain-names) | specified |
| An embedder may bind application traits | [`module.register.application`](../spec/lang/10-modules.md#r-module.register.application) | specified |
| Default profile: `Console`, `ConsoleInput`, `Args`, `Env`, `Clock`, `Random`, `FsRead`, `FsWrite`, ungated | [Host Capabilities](../spec/cli/command-line.md#host-capabilities) | specified, implemented |
| "the row already states what the program needs, so no flag or manifest table repeats it ... in hd the row is that permission" | same section, Why callout | **this record reverses it** |
| `Process` with `NotFound`, `PermissionDenied`, `Other`; bound only by `hd test` | [Processes](../spec/lang/10-modules.md#processes) | specified, implemented |
| Unit tests get `TestRunner` alone; integration and doc tests get the default profile and `Process` | [Test Environments](../spec/cli/command-line.md#test-environments) | specified |
| Deterministic providers: `MemoryFs`, `MapEnv`, `ScriptedProcess`, ... | [Unit Test Providers](../spec/std/testing.md#unit-test-providers) | specified |
| Cancelling must abort a known HTTP request | [`req.cancel.external-abort`](../spec/lang/11-requirements-and-suspension.md#r-req.cancel.external-abort) | specified, nothing to abort yet |
| Host wait maps to the component-model async ABI | [`req.host-wait.leaf`](../spec/lang/11-requirements-and-suspension.md#r-req.host-wait.leaf) | specified |
| Prototype bridge: one generic import per method; structured results cross as node trees; strings one byte per call (F-558); host answers synchronously, and a pending answer busy-polls (F-555) | [src/README.md](../src/README.md), `src/commands/default-profile.ts` | implemented |
| Archived `Http.send!` sketch with `ScriptedHttp` | STDLIB draft, 2026-09-26 (git `b37c8fbc`); [STDLIB_PLAN HTTP Client](STDLIB_PLAN.md#http-client) | "stands" |

## The Two-Level Model

### Requirement And Grant

The row says which traits a function may use. The grant says which
resources the host's provider for each trait may touch. A dependency can't
widen either: it adds keys to rows that its caller must accept, and it
never sees the grant.

```text
use std.fs.{FsError, FsRead, read_text}
use std.path.Path

pub fn main!() -> Result[void, FsError] $ FsRead + Console:
    text := read_text!(Path("data/orders.csv"))?
    println(text)
    .Ok(())
```

```sh
hd run                                # error: read access to data/orders.csv is not granted
hd run --allow-read=data              # prints the file
hd run --allow-read=. --deny-read=data/secrets
```

The row `$ FsRead` lets the compiler reject a helper that reads files from
a function that promised not to. The grant `data` keeps the program, and
every dependency it calls, inside one directory.

> **Why two levels.** A row can't name a path or a host: those are run-time
> values. A grant can't replace the row: it doesn't say which function
> reaches the file system. Deno has only the grant, and WASI only the
> capability handle. hd has both because the row already exists.

### Where A Grant Comes From

```toml
# hd.toml
[permissions]
read = ["data/", "config.toml"]
write = ["out/"]
net = ["api.github.com", "localhost:8080"]
env = ["APP_*", "HOME"]
run = ["git"]

[permissions.deny]
read = ["data/secrets/"]

[test.permissions]
net = ["localhost"]
```

| Program | Its grant |
| --- | --- |
| an executable or task under `hd run`, or `hd FILE` inside a package | `[permissions]`, then the command's flags |
| `hd FILE` outside a package, and the REPL | the command's flags only |
| an integration test case or a doc test | the **test grant**: read of the package directory, write of its own [`temp_dir`](../spec/std/testing.md#temporary-directories), then `[test.permissions]`, then the flags of `hd test` |
| a program that `hd_run!` starts | as `hd run NAME` gets it: `[permissions]`, with no flags |
| a unit test case | no host capability, unchanged |
| a built `.wasm` under another host | that host's policy; `hd build` embeds no grant |

1. **Effective grant.** For each category, a resource is granted when an
   allow entry from any source covers it and no deny entry from any source
   does. So deny wins, whatever its source.
2. **Flags.** Each category has `--allow-NAME[=LIST]` and
   `--deny-NAME[=LIST]`, with comma-separated entries, as in Deno. A flag
   with no list covers the whole category.
3. **No short forms and no `--allow-all`** (decision [H5](#h5-no---allow-all-owner)).
4. **Relative entries.** A manifest path is relative to the package
   directory. A flag path is relative to the command's working directory,
   as Deno resolves its flags.

> **Why `hd.toml` and flags.** The manifest holds what a program always
> needs, reviewed in a diff. A flag widens or narrows one run, such as an
> agent's sandbox or a CI job. Deno has the same pair.

> **Why one package table, not Deno's named sets.** A named set needs a
> selector flag, `-P=NAME`, on every command. One table plus flags covers
> every case found so far. Per-program tables can come later, keyed by
> executable or task name.

### Deny By Default

Every program of the table above starts with an empty grant. `hd run` on a
program whose row names `FsRead` reads nothing until a grant names a path.

> **Why.** The default profile binds the whole file system and the whole
> environment today. An agent that runs a dependency's task, or a script
> it just wrote, then hands that code its user's home directory. Deny by
> default makes the blast radius what the manifest says. Deno and WASI
> both start from nothing.

> **Cost.** Four examples, about six conformance fixtures and a few test
> files use `FsRead`, `FsWrite` or `Env` under `hd run` today (counted by
> grep, 2026-10-06). Each needs a `[permissions]` entry or a flag.
> Integration tests keep working, since the test grant covers the package
> directory and their own temporary directory.

### No Prompting

`hd` never asks. A request outside the grant fails at once, with an error
value.

> **Why.** Agents can't answer a prompt, and a run that waits for one
> burns wall time (Arena pillar 1). A prompt's answer would also be an
> input outside [`req.determinism.depends`](../spec/lang/11-requirements-and-suspension.md#r-req.determinism.depends),
> so the same command could behave two ways. Deno itself never prompts
> without a terminal.

### A Denial Is An Error Value

A provider that refuses a resource returns an ordinary error. It never
panics, so code that probes an optional file keeps working.

| Trait | Denial | Display text (draft) |
| --- | --- | --- |
| `FsRead`, `FsWrite` | `FsError.NotGranted(path)` | `access to data/x.csv is not granted; run with --allow-read or --allow-write, or add it to [permissions] in hd.toml` |
| `Http` | `HttpError.NotGranted(host)` | `net access to api.github.com:443 is not granted; run with --allow-net=api.github.com, or add it to [permissions] net in hd.toml` |
| `Process` | `ProcessError.NotGranted` | `starting this program is not granted; run with --allow-run=NAME, or add it to [permissions] run in hd.toml` |
| `Sys` | `SysError.NotGranted(name)` | `sys access to hostname is not granted; run with --allow-sys=hostname` |
| `Env` | see [H7](#h7-a-variable-outside-the-env-grant-owner) | |

> **Why its own variant.** `PermissionDenied` means the OS refused, and the
> fix is outside hd. `NotGranted` means hd refused, and the fix is one
> flag. Deno 2 split `NotCapable` from `PermissionDenied` for this reason.

### Scopes

| Category | Entry | Matches |
| --- | --- | --- |
| `read`, `write` | a path | that file, or everything under that directory. The path is resolved with `..` and symbolic links first, so neither escapes a granted directory, as WASI's sandboxed resolution guarantees. |
| `net` | `host`, `host:port`, `*.example.com`, an IPv4 address, `[IPv6]`, each with an optional port | the URL's host as written, and its port. No port in the entry means any port. Each redirect hop is checked again. |
| `env` | a name, or `PREFIX_*` | that variable, or every variable that starts with `PREFIX_` |
| `run` | a program name or an absolute path | the `program` argument of `run!` as written, before the host looks it up on `PATH` |
| `sys` | `hostname`, `cpu_count`, `os`, `arch` | that method of `Sys` |

`write` does not imply `read`, as in Deno.

> **Note.** `run` grants more than it says. A started program runs with
> the OS authority of `hd`, not with hd's grant, as Deno warns of its own
> subprocesses. The manual should say so beside the flag.

## Deno Permissions Mapped To hd

| Deno | hd trait | Exists? | Scope | Recommendation |
| --- | --- | --- | --- | --- |
| read | `FsRead` | yes | paths | gate it; add `FsError.NotGranted` |
| write | `FsWrite` | yes | paths | gate it, as for read |
| net | `Http` (client) | **new**, `std.http` | hosts and ports | first; see [`std.http`](#stdhttp) |
| net | HTTP server | new | listen address | later, as a registered boundary: the `wasi:http` handler export |
| net | TCP, UDP | new | hosts and ports | later: a socket is a live handle, which waits for the resource design |
| net | DNS lookup | new | host names | later, with sockets; `Http` resolves names itself |
| env | `Env` | yes | names, `PREFIX_*` | gate it; the denial form is [H7](#h7-a-variable-outside-the-env-grant-owner) |
| sys | `Sys` | **new**, `std.sys` | method names | last, and small |
| run | `Process` | yes, language tier | program names | add it to the default profile behind `run` |
| ffi | none | no | none | no permission: see [FFI](#ffi) |
| import | none | no | none | parked: see [Runtime Code Loading](#runtime-code-loading) |
| (ungated) | `Console`, `ConsoleInput`, `Args`, `Clock`, `Random` | yes | none | stay ungated; Deno gates none of them |

## FFI

**The hypothesis holds.** A Wasm module calls nothing outside itself but its
imports:
- The Wasm security model: modules "can't escape the sandbox without going
  through appropriate APIs".
- The component model: a component interacts "only by calling its imports
  and having its exports called".
- `WebAssembly.instantiate` fails with a `LinkError` when the
  `importObject` lacks an import, so a module can't name a host function
  it wasn't given.
- hd routes every import through a trait:
  [`module.host.requirements`](../spec/lang/10-modules.md#r-module.host.requirements)
  says "Every host facility is injected through an ordinary requirement
  trait".

So Deno's `ffi` permission has no hd counterpart. Deno needs it because
`Deno.dlopen` loads native code into a process that the sandbox can't
inspect. An hd program has no `dlopen`: the module's import list is fixed
when the host is built.

**What an embedder does to expose a native library**, for example SQLite:

1. Declare the interface as an hd trait, in a package the program depends
   on:

   ```text
   pub enum DbError:
       Closed
       Query(message: string)

   pub trait Database:
       fn query!(self, sql: string, args: List[string]) -> Result[List[List[string]], DbError]
   ```

2. Implement the trait on the host side, in Rust or JavaScript, by calling
   the native library. In the component model, this is a WIT interface that
   the host implements.
3. List the trait in the registration contract
   ([`module.register.application`](../spec/lang/10-modules.md#r-module.register.application)),
   so an entry point's row may name it.
4. The program calls it like any provider. A test binds a fake with
   `$.with`, as for every host trait.

| Gap | Effect | Status |
| --- | --- | --- |
| No opaque handle type | A native object (a connection, a prepared statement) can't cross as a value: live handles are not boundary-safe ([`module.boundary.not-safe`](../spec/lang/10-modules.md#r-module.boundary.not-safe)). The embedder must keep a table and hand out integer ids, as `Database` above hides one connection. | waits for the resource design; component-model `resource` types are the target |
| Copying | Each call copies its boundary values, so bulk data, such as an image, costs a copy per call. | measured by `host-call-overhead` |
| The `hd` CLI is not an embedder | `hd run` can't load a user's host code. A native tool is reachable only through `run` (a subprocess) or `net` (a local service). | intended: an app that needs native code ships its own host |
| Wasm libraries from other languages | A C library compiled to a Wasm component would link at build time through WIT. hd would import it as a trait too. | waits for the component ABI ([`module.host.abi`](../spec/lang/10-modules.md#r-module.host.abi)) |

**Recommendation:** no `ffi` permission, no `extern` declaration, and one
spec Note that a custom host trait is hd's FFI. The handle gap goes to
OPEN_ISSUES under the resource design.

## Runtime Code Loading

**Does it apply to Wasm languages?** Yes, but only through the host. Wasm has
no instruction that compiles or links code. A module that wants a plugin
calls a host import, and the host runs `WebAssembly.instantiate`, a
wasmtime `Linker`, or a component instantiation. In hd that import is a
host trait, so loading is a capability like any other.

### A Sketch

```text
use std.load.{Limits, LoadError, Loader}  # hypothetical: no such module
use std.path.Path

trait Transform:
    fn apply(self, input: string) -> string

data Quiet: pass

impl Console for Quiet:
    fn write_line!(mut self, text: string) -> Result[void, ConsoleError]: .Ok(())

fn run_plugin!(path: Path, input: string) -> Result[string, LoadError] $ Loader:
    limits := Limits { memory_bytes: 67108864, millis: 2000 }
    providers := $.context(Console=Quiet {})
    plugin := $.use(Loader).load!::[Transform](path, limits, providers)?  # hypothetical: a trait as a type argument
    .Ok(plugin.apply(input))
```

| Part | Sketch |
| --- | --- |
| Interface | an hd trait, checked at load against the plugin's exports. A mismatch is `LoadError.Interface(message)`. |
| Source | a built `.wasm` artifact only, never source: loading source would ship the compiler in every runtime. Optionally pinned by `sha256`, as in Extism. |
| Capabilities | passed explicitly as a `$.Context`, which binds the plugin's entry row. The plugin gets no grant of its own beyond what those providers allow. |
| Data | serde-style: every argument and result is a [boundary-safe value](../spec/lang/10-modules.md#boundary-safe-values), copied. |
| Permission | `load`, scoped by paths or hosts, as Deno's `import`. |
| Isolation | a memory limit and a time limit. A plugin panic or limit is `LoadError.Panicked` or `LoadError.Limit`, and never poisons the loader. |

### Hard Parts

| # | Problem | Why it is hard |
| --- | --- | --- |
| 1 | **Typing the interface across builds** | Two separately compiled programs must agree on a trait's methods and value layouts. That needs a stable type description in the artifact: the component ABI, which is still open ([`module.host.abi`](../spec/lang/10-modules.md#r-module.host.abi)). |
| 2 | **A trait as a type argument** | `load!::[Transform]` returns a value of a trait chosen by the caller, with a check at run time. The compiler must reify the trait's interface: a new intrinsic. |
| 3 | **Providers that call back** | The plugin calls its `Console` provider, which lives in the loader. That is a call from one instance into another, carrying a live handle that is not boundary-safe. The component model's `own` and `borrow` handles solve it; hd has neither. |
| 4 | **Suspension across instances** | A plugin method that suspends needs its waker and cancellation to cross instances: the component-model async ABI, two drivers, and two poison scopes. |
| 5 | **Memory limits under Wasm GC** | hd's values live on the engine's GC heap. V8 has no per-instance GC heap limit, so a browser or Node host isolates a plugin only in its own Worker. wasmtime limits a `Store`, but cross-store references are impossible. Either way the plugin is a separate instance. |
| 6 | **Copy cost and identity** | The heaps are separate, so every call copies its values, and sharing is lost ([`module.boundary.sharing`](../spec/lang/10-modules.md#r-module.boundary.sharing)). Wasm GC could pass references between modules of one engine, but two hd builds may lay out the same type differently. |

**Recommendation: park it.** Parts 1, 3 and 4 wait for the component ABI,
and part 2 needs an intrinsic. Two things cover most plugin needs today:
- `run`: a plugin as a separate program, started with `Process`;
- `net`: a plugin as a local service, called with `Http`.

The reverse direction already exists: a host embeds hd through
[Registration](../spec/lang/10-modules.md#registration).

## Stdlib Modules

Each module stays the size of what Go and Deno ship. Every trait gets a
deterministic provider for unit tests, as
[Unit Test Providers](../spec/std/testing.md#unit-test-providers) requires.

### `std.http`

A client first, as one function from a request to a response: Deno's
`fetch`, Go's `Client.Do`, and `wasi:http`'s `handle`.

```text
use std.time.Duration

pub enum Method:
    Get
    Head
    Post
    Put
    Patch
    Delete
    Options
    Other(name: string)

pub data Request:
    pub method: Method = Method.Get
    pub url: string
    pub headers: List[(string, string)] = []
    pub body: List[u8] = []
    pub timeout: Duration? = .None

pub data Response:
    pub status: u16
    pub headers: List[(string, string)]
    pub body: List[u8]

pub enum HttpError:
    NotGranted(host: string)
    InvalidUrl(url: string)
    Dns(host: string)
    Connect(message: string)
    Tls(message: string)
    Timeout
    TooManyRedirects(url: string)
    Other(message: string)

pub trait Http:
    fn send!(mut self, request: Request) -> Result[Response, HttpError]

pub fn send!(request: Request) -> Result[Response, HttpError] $ Http:
    $.use(Http).send!(request)

pub fn get!(url: string) -> Result[Response, HttpError] $ Http:
    send!(Request { url: url })

impl Response:
    pub fn text(self) -> string:
        pass

    pub fn header(self, name: string) -> string?:
        pass
```

| Item | Behavior |
| --- | --- |
| `send!` | sends `request`, follows up to 10 redirects (Go's limit), and completes with the whole response |
| Status | any status, 404 and 500 included, is `.Ok`, as with `fetch`, Go and [`module.process.nonzero-ok`](../spec/lang/10-modules.md#r-module.process.nonzero-ok) |
| Headers | a list of pairs, so a repeated header such as `Set-Cookie` keeps every value, in order |
| `header(name)` | the first value whose name matches, ignoring ASCII case, as Go's `Header.Get` |
| `text()` | the body decoded as UTF-8, with U+FFFD for each invalid sequence, as `fetch`'s `text()` and [`cli.test.process.decode`](../spec/cli/command-line.md#r-cli.test.process.decode) do |
| `timeout` | `.None` means the provider's default; elapsed time is `.Err(HttpError.Timeout)` |
| Cancellation | cancelling the suspension aborts the request, by [`req.cancel.external-abort`](../spec/lang/11-requirements-and-suspension.md#r-req.cancel.external-abort) |
| `mut self` | so a provider may record what it sent, as `Process` and `Console` do |
| Errors | `HttpError` implements `Eq`, `Debug`, `Display` and `Error`; its cases are a subset of `wasi:http`'s `error-code` |
| Bodies | whole bodies, no streams; streaming waits for the resource design |

```text
use std.http.{Http, HttpError, get}

fn latest_release!(repo: string) -> Result[string, HttpError] $ Http:
    response := get!("https://api.github.com/repos/${repo}/releases/latest")?
    if response.status != 200:
        return .Err(.Other("status ${response.status}"))
    .Ok(response.text())
```

**Test provider: `ScriptedHttp`.** This is the brief's `FakeHttp`, named
after `ScriptedProcess` and the archived sketch.

```text
use std.http.{Http, Response, ScriptedHttp, get}
use std.testing.assert_equal

fn status!(url: string) -> u16 $ Http:
    match get!(url):
        .Ok(response) => response.status
        .Err(_) => 0

tests:
    it("reads the scripted status"):
        let mut http = ScriptedHttp::new({"https://example.com/": Response { status: 204, headers: [], body: [] }})
        $.with(Http=http):
            assert_equal(status!("https://example.com/"), 204, reason="scripted")
        assert_equal(http.sent().len(), 1, reason="one request")
```

| Item | Behavior |
| --- | --- |
| `ScriptedHttp::new(responses: Map[string, Response]) -> mut ScriptedHttp` | answers each URL in `responses`, by its exact text, whatever the method |
| Unknown URL | `.Err(HttpError.Connect("ScriptedHttp has no response for URL"))`, which names the URL a test forgot |
| `sent(self) -> List[Request]` | every request received, in order, for assertions |
| Host | it opens no connection |

**Playground.** The provider calls the browser's `fetch` (in the prototype,
a synchronous `XMLHttpRequest`, which a dedicated worker still allows):
- The browser enforces CORS. A refused request can't be told apart from a
  network failure, so both are
  `HttpError.Connect("the browser refused the request: a network error or CORS")`.
- The browser drops forbidden headers, such as `Cookie` and `Host`.
- The playground grants `net` to every host, since the browser is already
  the sandbox. It grants nothing else.

**Later in `std.http`:**
- **A server**, as a [registered boundary](../spec/lang/10-modules.md#registration)
  that exports the `wasi:http` handler, as `Deno.serve` takes a handler.
  Listening needs `net` on its address, as in `--allow-net=0.0.0.0:8000`.
- **Sockets and DNS**, in `std.net`.

### `std.process`

`Process` and `run!` stay as the language tier defines them
([Processes](../spec/lang/10-modules.md#processes)). Three changes:

| Change | Tier |
| --- | --- |
| The default profile binds `Process` for `hd run`, `hd FILE` and the REPL, gated by `run`. | CLI |
| `ProcessError` gains `NotGranted`, with the Display text `starting this program is not granted`. | language (`ProcessError` is declared there) |
| The started program inherits the whole host environment, as Deno and Go do by default. The `env` grant limits what hd code reads, not what a child sees. | CLI |

```text
pub enum ProcessError:
    NotFound
    PermissionDenied
    NotGranted
    Other(message: string)
```

The test runner's `Process` provider keeps its scope: the package's
executables and tasks ([`cli.test.process`](../spec/cli/command-line.md#r-cli.test.process)).
`ScriptedProcess` stays the test fake. A working directory and per-child
variables, as Go's `Cmd.Dir` and `Cmd.Env`, wait for a real need.

**Playground:** `Process` is not bound, so an entry row that names it is
`nonhost-entry-requirement`, as today.

### `std.sys`

Go-sized: `runtime.GOOS`, `runtime.GOARCH`, `runtime.NumCPU` and
`os.Hostname`. Deno gates each `sys` call by name, so each method returns a
`Result`.

```text
pub enum SysError:
    NotGranted(name: string)
    Unsupported(name: string)

pub trait Sys:
    fn os(self) -> Result[string, SysError]
    fn arch(self) -> Result[string, SysError]
    fn hostname(self) -> Result[string, SysError]
    fn cpu_count(self) -> Result[u32, SysError]
```

| Item | Behavior |
| --- | --- |
| `os` | `linux`, `macos`, `windows`, or another lowercase name, as Go's `GOOS` |
| `arch` | `x86_64`, `aarch64`, or another, of the host machine |
| Plain calls | each returns a value the host holds, as `Env` reads do ([`std-host.plain-reads`](../spec/std/host.md#r-std-host.plain-reads)) |
| `Unsupported` | a host that can't answer, such as a browser for `hostname` |
| Test provider | `MapSys::new(values: Map[string, string])`, after `MapEnv`; a missing name is `Unsupported` |
| Playground | not bound, as for `Process` |

**Recommendation:** ship `std.sys` last. No program in the repo needs it
yet, so its shape should wait for one that does.

### `std.fs` Changes

| Change | Rule it touches |
| --- | --- |
| `FsError` gains `NotGranted(path: Path)`. Display: `access to PATH is not granted`, plus the flag hint. | [`std-fs.error.decl`](../spec/std/fs.md#r-std-fs.error.decl) |
| The default profile's providers check each path against `read` or `write` after resolving it. | [Host Capabilities](../spec/cli/command-line.md#host-capabilities) |
| `rename!` needs `write` for both paths. `list_dir!` and `stat!` need `read`. | new CLI rules |
| `MemoryFs` doesn't change: a fake has no grant. | none |

**Playground:** neither trait is bound today, and none would be.

### `std.host` Changes (`Env`)

| Change | Rule it touches |
| --- | --- |
| The provider's `get(name)` checks the `env` grant. A name outside it reads as [H7](#h7-a-variable-outside-the-env-grant-owner) decides. | [`std-host.env.get`](../spec/std/host.md#r-std-host.env.get) |
| `names()` returns only the granted names that are set. | [`std-host.env.names`](../spec/std/host.md#r-std-host.env.names) |
| `MapEnv` doesn't change. | none |

`Args` stays ungated: the runner chose the arguments, as in Deno.

## Boundary ABI

Grants are checked in the host's provider, never in hd code: hd code is
the party being limited.

| Trait | Prototype (frozen, Node and browser) | Official runtime (Component Model) |
| --- | --- | --- |
| `FsRead`, `FsWrite` | the existing per-method imports; `default-profile.ts` resolves the path and checks the grant before `node:fs` | `wasi:filesystem`: each `read` or `write` entry becomes a preopen with read-only or read-write `DirPerms`; the descriptor sandbox does the check |
| `Env` | the existing import; the answer filters by the grant | `wasi:cli/environment`: the host passes only granted variables |
| `Http` | a new provider: `Request` crosses out and `Response` back as node trees, as `ProcessOutput` does; a synchronous fetch in a Worker, waited on with `Atomics.wait`; each hop's host checked, using `redirect: "manual"` | `wasi:http` `handler.handle`, imported; async through [`host_wait!`](../spec/lang/11-requirements-and-suspension.md#r-req.host-wait.leaf); the host's outgoing-request hook checks the grant and returns `HTTP-request-denied` |
| `Process` | `spawnSync` with the grant check, as `processes.ts` does for tests | no WASI interface: an hd host extension, such as `hd:host/process` |
| `Sys` | the existing per-method imports | no WASI interface: an `hd:host/sys` extension |
| sockets (later) | none | `wasi:sockets`, checked by wasmtime's `socket_addr_check` |

> **Note.** A `NotGranted` value crosses as an ordinary enum case. It needs
> no new boundary rule.

## Arena Check

| Metric | Pillar | Effect |
| --- | --- | --- |
| `host-call-overhead` | 3 | Each gated call adds a check. A path check costs one path resolution; a host check scans a short list. Budget: the check costs less than the call it guards. Add an HTTP case: one `send!` to a local server, measured per crossing. |
| `integration-test-perf` | 3 | The fixed test grant needs no setup, so per-test setup doesn't change. |
| `mistakes`, `answer-size` | 1 | Add corpus cases for a missing grant. The `NotGranted` text must name the flag, so one rerun fixes it. Keep each denial at most 60 tokens. |
| `determinism` | 1 | No prompting keeps a command's output a function of its inputs and flags. |
| `dead-code` | 3 | `std.http` must not reach the binary of a program that doesn't use it. |
| `concurrency` | 2 | Not measured, but helped: scoped `write` grants keep N agents' runs and tests from writing into each other's files. |

No compiler metric changes. Grants are runtime, so `edit-latency`,
`cold-check` and `resources` stay as they are.

## Decisions

Each decision gives options and a labeled recommendation. **Owner** marks a
language-tier or CLI-tier choice, or one that reverses a recorded
rationale. **Orchestrator** marks a stdlib or implementation call.

### H1. Two Levels: Rows Plus Run-Time Grants (Owner)

| Option | Effect |
| --- | --- |
| A. Rows only (today) | `hd run` hands a program the whole file system, network and environment for each trait its row names. |
| B. Rows plus grants | Rows stay; a grant scopes each trait's resources at run time. |
| C. Grants only | Drop host traits from rows. This breaks fakes, which are the testing model. |

**Recommendation: B.** It reverses the Why callout of
[Host Capabilities](../spec/cli/command-line.md#host-capabilities), "in hd
the row is that permission". New evidence: a row can't name a path or a
host, so a dependency inside the row can reach every file.

### H2. Grant Sources (Owner)

| Option | Effect |
| --- | --- |
| A. Flags only | Every run repeats its flags; nothing reviewable in the repo. |
| B. `[permissions]` in `hd.toml` plus flags | Lasting grants in a diff, one-off changes on the command line. |
| C. Deno's named sets plus `-P=NAME` | B, plus a selector on every command. |
| D. An annotation on `main` | The program grants itself, so it limits dependencies but never its own author. |

**Recommendation: B.** C is a second mechanism with no current use. D fails
the point of a grant, which is the runner's consent.

### H3. Deny By Default (Owner)

| Option | Effect |
| --- | --- |
| A. Deny by default for every program `hd` runs | About a dozen repo files need a grant. |
| B. Deny for single files and tasks only; executables keep today's full access | Two rules for one command. |
| C. Allow by default, `--deny-*` to narrow | Today's blast radius stays. |

**Recommendation: A**, with the fixed test grant so integration tests keep
working.

### H4. No Prompting (Owner)

**Recommendation:** never prompt. A refused request fails at once with a
`NotGranted` value. Alternative: prompt on a terminal, as Deno does. That
makes behavior depend on whether a TTY is attached.

### H5. No `--allow-all` (Owner)

| Option | Effect |
| --- | --- |
| A. No `-A`; an unscoped `--allow-net` still covers one category | An agent can't silence every check with one flag. |
| B. Deno's `-A` | Agents will reach for it, and the grant then means nothing. |

**Recommendation: A.**

### H6. A Denial Variant (Owner, Because `ProcessError` Is Language Tier)

| Option | Effect |
| --- | --- |
| A. `NotGranted` in each error enum | The fix (a flag) differs from an OS refusal's (file modes, `sudo`). |
| B. Reuse `PermissionDenied` | No new variant; an agent can't tell the two fixes apart. |
| C. A panic | Code can't probe an optional resource. |

**Recommendation: A**, as Deno 2's `NotCapable`.

### H7. A Variable Outside The `env` Grant (Owner)

`Env.get` returns `string?`, so it has no error channel.

| Option | Effect |
| --- | --- |
| A. `.None`, silently | As Deno's `--ignore-env`; an agent may hunt for a variable that is set. |
| B. `.None`, and `hd` writes one notice to standard error per name that is set on the host but not granted | The agent sees the fix; a program's standard error gains a line. |
| C. Change `get` to return `Result[string?, EnvError]` | Every caller changes, for one rare case. |

**Recommendation: B.** The notice names the flag, as in
`hd: env API_KEY is set but not granted; run with --allow-env=API_KEY`.

### H8. FFI Through Host Traits (Owner)

**Recommendation:** confirm the hypothesis. No `ffi` permission, no
`extern`; a Note in [Host Boundary](../spec/lang/10-modules.md#host-boundary)
says a custom host trait is hd's FFI. The opaque-handle gap goes to the
resource design in OPEN_ISSUES.

### H9. Park Runtime Code Loading (Owner)

**Recommendation:** park it, with the text under
[Parked Open Issues Text](#parked-open-issues-text). Plugins use `run` or
`net` until the component ABI exists.

### H10. `Process` In The Default Profile (Owner)

**Recommendation:** bind `Process` for `hd run`, `hd FILE` and the REPL,
behind the `run` grant. It removes the CLI note "`Process` and an HTTP
client are not in the default profile". `Http` joins it behind `net`.

### S1. Module Name: `std.http` (Orchestrator)

`std.http` holds the client, and later the server. `std.net` later holds
sockets and DNS. Go splits `net` and `net/http` the same way. The brief's
`std.net` for HTTP would mix a handle-free client with handle-based
sockets. **Recommendation:** `std.http`.

### S2. `Http` Shape (Orchestrator)

One trait method `send!`, plus a `get!` helper; data `Request` and
`Response`; header pairs; whole bodies; any status is `.Ok`; up to 10
redirects. **Recommendation:** as in [`std.http`](#stdhttp). A `post!`
helper waits for evidence that `send!` is too long to write.

### S3. `ScriptedHttp` (Orchestrator)

Keyed by exact URL text, records `sent()`, and an unknown URL is a
`Connect` error that names it. **Recommendation:** as above, not a 404,
which would hide a missing route behind a valid status.

### S4. `std.sys` Last (Orchestrator)

**Recommendation:** four gated methods, after `std.http` and `Process`.

### S5. Path Resolution (Orchestrator)

Resolve `..` and symbolic links before the check, so nothing escapes a
granted directory. The Node prototype uses `realpath`, which has a race
between check and use. The official runtime's preopens don't.
**Recommendation:** document the race as a prototype limit.

### S6. Synchronous HTTP In The Prototype (Orchestrator)

| Option | Cost |
| --- | --- |
| A. A Worker running `fetch`, waited on with `Atomics.wait` | about 1 ms per call; about 80 lines |
| B. `spawnSync` of a Node child running `fetch` | about 50 ms per call; about 30 lines |
| C. A truly pending provider, after fixing F-555 | the wake-driven host entry; days of work |

**Recommendation: A**, with B as the fallback.

### S7. Playground Grants (Orchestrator)

**Recommendation:** `net` to every host through browser `fetch`; nothing
else. The browser's CORS rule is the real policy there.

## Spec Draft Outline

Only after the owner decides. Each line names its tier.

| Chapter | Change |
| --- | --- |
| [CLI: Host Capabilities](../spec/cli/command-line.md#host-capabilities) | Replace the Why callout. Add `Process` and `Http` to the default profile's table. Remove the "not in the default profile" Note. Add a subsection **Permissions** with the rules below. |
| [CLI: Test Environments](../spec/cli/command-line.md#test-environments) | Add a Grant column: the test grant for integration and doc tests, none for unit tests. |
| [CLI: REPL](../spec/cli/command-line.md#repl) | The REPL takes the permission flags. |
| [Modules: Processes](../spec/lang/10-modules.md#processes) | `ProcessError.NotGranted`, and a rule for when `run!` returns it. |
| [Modules: Host Boundary](../spec/lang/10-modules.md#host-boundary) | A Note: a provider may refuse a resource by its runtime grant, with an ordinary error. A Note: a custom host trait is the FFI. |
| [std/fs.md](../spec/std/fs.md) | `FsError.NotGranted` and its Display. |
| [std/host.md](../spec/std/host.md) | `Env` under a grant, by H7. |
| [std/process.md](../spec/std/process.md) | The Display text of `NotGranted`. |
| new `std/http.md` | `std.http` as above, `ScriptedHttp`, playground behavior. |
| [std/testing.md](../spec/std/testing.md#unit-test-providers) | Add `Http` and `ScriptedHttp` to the providers table. |
| new `std/sys.md` | later, with `std.sys` |
| conformance | CLI cases for each flag, deny precedence, the test grant, and each `NotGranted` value |

Draft rules for the CLI **Permissions** subsection (IDs proposed):

1. `cli.perm.categories`: The permission categories are `read`, `write`, `net`, `env`, `run` and `sys`. They gate `FsRead`, `FsWrite`, `Http`, `Env`, `Process` and `Sys`.
2. `cli.perm.ungated`: `Console`, `ConsoleInput`, `Args`, `Clock` and `Random` take no permission.
3. `cli.perm.default-deny`: A program that `hd` runs starts with an empty grant in every category.
4. `cli.perm.flags`: `--allow-NAME=LIST` and `--deny-NAME=LIST` add comma-separated entries for category `NAME`; without `=LIST`, the flag covers the whole category.
5. `cli.perm.no-all`: There is no flag that covers every category.
6. `cli.perm.manifest`: The `[permissions]` table of `hd.toml` holds a list per category, and `[permissions.deny]` holds deny lists.
7. `cli.perm.manifest.unknown`: An unknown category key in either table is an error. Error: `unknown-permission`.
8. `cli.perm.sources`: `hd run`, and `hd FILE` inside a package, grant the manifest's entries and then the command's flags.
9. `cli.perm.outside-package`: `hd FILE` outside a package, and the REPL, grant only the command's flags.
10. `cli.perm.deny-wins`: A resource is granted when an allow entry from any source covers it and no deny entry from any source covers it.
11. `cli.perm.relative`: A manifest path is relative to the package directory, and a flag path to the working directory.
12. `cli.perm.path`: A `read` or `write` entry covers its file, or every path under its directory, after `..` and symbolic links are resolved.
13. `cli.perm.write-not-read`: A `write` entry grants no read.
14. `cli.perm.net`: A `net` entry covers a URL whose host matches it, as a name, a `*.` subdomain pattern, or an address, and whose port matches when the entry gives one.
15. `cli.perm.net.redirect`: Each redirect target is checked as a new request.
16. `cli.perm.env`: An `env` entry covers its variable name, or with a trailing `*`, every name that starts with the text before it.
17. `cli.perm.run`: A `run` entry covers a `run!` call whose `program` argument equals it.
18. `cli.perm.run.escape`: A started program runs with the operating-system authority of `hd`; no grant limits it. (Note.)
19. `cli.perm.sys`: A `sys` entry covers the `Sys` method of that name.
20. `cli.perm.no-prompt`: `hd` never asks for a permission; a request outside the grant fails at once.
21. `cli.perm.denial`: A refused operation returns its trait's `NotGranted` error, and never panics.
22. `cli.perm.denial.text`: The Display text of a `NotGranted` value names the flag and the manifest key that would grant it.
23. `cli.perm.test-grant`: An integration test case or doc test gets read of the package directory, write of its own temporary directory, `[test.permissions]`, and the flags of `hd test`.
24. `cli.perm.test.no-package`: The `[permissions]` table does not apply to test cases. A program that `hd_run!` starts gets it, as `hd run NAME` does.
25. `cli.perm.build`: `hd build` writes no grant into its output.

Rule accounting: 25 new `cli.perm.*` rules. Two rules change
(`cli.host.default-profile` gains `Process` and `Http`;
`module.process.permission-denied` (since retired) narrows to the OS refusal). One new
language rule is `module.process.not-granted`. One Why callout is removed.
The stdlib tier gains `std/http.md`, about 30 rules.

## Parked Open Issues Text

To replace the "host extensions after the default profile" bullet of
[Runtime, Library, ABI, And Tooling Work](OPEN_ISSUES.md#runtime-library-abi-and-tooling-work)
once H1-H10 are decided, and to add:

> - **Runtime code loading (parked, 2026-10-06).** A host trait `Loader`
>   would load a built `.wasm` plugin at run time. It would check the
>   plugin's exports against an hd interface trait, bind its row from an
>   explicit `$.Context`, copy every value as boundary-safe data, and run
>   it under memory and time limits, behind a `load` permission. It waits
>   on:
>   - the component ABI, so two builds agree on a trait's types;
>   - an intrinsic that reifies a trait for a run-time check;
>   - cross-instance provider handles (`own` and `borrow`);
>   - the component-model async ABI for suspending plugin calls.
>
>   Until then a plugin is a program started through `Process` or a
>   service called through `Http`. Design:
>   [Host Capabilities](HOST_CAPABILITIES.md#runtime-code-loading).
> - **Opaque host handles (with the resource design).** A host trait can't
>   return a native object, such as a database connection, because live
>   handles are not boundary-safe. Custom host traits are hd's FFI
>   ([Host Capabilities](HOST_CAPABILITIES.md#ffi)), so this limits every
>   embedder. Target: component-model `resource` types.
> - **Sockets, DNS and an HTTP server.** `std.net` with TCP, UDP and
>   lookup waits for the resource design, since a socket is a live handle.
>   The HTTP server is a registered boundary that exports the `wasi:http`
>   handler.

## Prototype Plan

At most three one-hour sessions, in this order. Each session needs its
spec pass first, since the spec leads the prototype. Each one
reports the tiny-program footprint before and after: WAT size, function
count and build time.

| Session | Work | Done when |
| --- | --- | --- |
| 1. HTTP client | `lib/std/http.hd` (types, `Http`, `send!`, `get!`, `text`, `header`, `ScriptedHttp`); a Node provider in `src/commands/` that runs `fetch` in a Worker, waits with `Atomics.wait`, and checks a `--allow-net` list on each hop; `Http` in the default profile's trait list | a script with `--allow-net=example.com` fetches; without it, `NotGranted`; `ScriptedHttp` unit tests pass with `--changed` |
| 2. Grants | `[permissions]`, `[permissions.deny]` and `[test.permissions]` in `src/manifest.ts`; the `--allow-*` and `--deny-*` flags; deny-wins matching; path checks in `default-profile.ts`; `Env` filtering and H7; `FsError.NotGranted`; grants for the files that need them | the deny-by-default migration passes the scoped checks; KNOWN_FAILURES rows for anything left |
| 3. Process and playground | `Process` in the default profile behind `run`; `ProcessError.NotGranted`; the playground's sync-XHR `Http` provider and its CORS error text | `hd run --allow-run=git` runs git; the playground example fetches a CORS-enabled URL |

Everything else goes to the new compiler: `std.sys`, sockets, the HTTP
server, streaming bodies, the `wasi:http` binding and runtime code loading.

**Risks:**
- Session 2 touches every host provider. If the owner wants the full grant
  model designed with the new compiler's CLI, as the parked sketch said,
  then stop after session 1. Keep `--allow-net` as the prototype's only
  grant, and add the rest as KNOWN_FAILURES rows.
- `Atomics.wait` blocks the main thread. That is fine for `hd run`, which
  already blocks on every host call (F-555), but the REPL must run its
  program off the main thread, as it does today.

## Questions For The Owner

One idea each; the effect first, then the candidates, then a short example.

1. **H1, two levels.** Today `hd run` gives a program the whole disk for
   `$ FsRead`. Keep rows and add run-time grants (recommended), keep rows
   only, or grants only?
   `hd run --allow-read=data`
2. **H2, where grants live.** A grant must be written somewhere reviewable.
   `[permissions]` in `hd.toml` plus flags (recommended), flags only, or
   Deno's named sets?
   `[permissions] read = ["data/"]`
3. **H3, deny by default.** About a dozen repo files would need a grant.
   Deny by default for every program (recommended), for scripts and tasks
   only, or allow by default?
4. **H4, prompting.** A prompt stalls an agent. Never prompt (recommended),
   or prompt on a terminal like Deno?
5. **H5, `--allow-all`.** One flag would silence every check. No `-A`
   (recommended), or Deno's `-A`?
6. **H6, denial value.** An agent must tell "hd refused" from "the OS
   refused". A `NotGranted` variant in each error enum (recommended), reuse
   `PermissionDenied`, or a panic?
   `.Err(FsError.NotGranted(Path("data/x.csv")))`
7. **H7, ungranted variable.** `env("API_KEY")` returns `.None` for a set
   variable. Add a stderr notice that names the flag (recommended), stay
   silent, or make `get` return a `Result`?
8. **H8, FFI.** Confirmed: a Wasm module can call only its imports. Drop
   `ffi` and record that a custom host trait is the FFI (recommended)?
9. **H9, runtime loading.** It needs the component ABI and a new intrinsic.
   Park it (recommended), or design it now?
10. **H10, `Process` in the default profile.** Scripts can't start git
    today. Bind `Process` behind `run` (recommended), or keep it test-only?
    `hd run --allow-run=git`

## Parse Log

Every `text` block above was parsed with `hd debug parse`, which checks
syntax only; nothing here type-checks.

| Block | Result |
| --- | --- |
| 1, a program that reads a file | parse |
| 2, `DbError` and `Database` | parse |
| 3, the `Loader` sketch | parse; `std.load` and a trait type argument are hypothetical, not syntax |
| 4, the `std.http` declarations | parse; method bodies are `pass` |
| 5, `latest_release!` | parse |
| 6, the `ScriptedHttp` test | parse |
| 7, `ProcessError` with `NotGranted` | parse |
| 8, `SysError` and `Sys` | parse |

## Sources

- Deno: [Permissions reference](https://docs.deno.com/runtime/reference/permissions/);
  [Security and permissions](https://docs.deno.com/runtime/fundamentals/security/);
  [`deno.json` permissions](https://docs.deno.com/runtime/reference/deno_json/);
  [Modules: dynamic import](https://docs.deno.com/runtime/fundamentals/modules/);
  [Deno 2.0 RC, `NotCapable`](https://deno.com/blog/v2.0-release-candidate).
- WASI: [wasi-filesystem](https://github.com/WebAssembly/wasi-filesystem);
  [wasi-sockets](https://github.com/WebAssembly/wasi-sockets);
  [wasi-http](https://github.com/WebAssembly/wasi-http) and its
  [0.3 handler](https://github.com/WebAssembly/wasi-http/blob/main/wit-0.3.0-draft/handler.wit);
  [WASI 0.3 launch](https://bytecodealliance.org/articles/WASI-0.3).
- wasmtime: [`WasiCtxBuilder`](https://docs.wasmtime.dev/api/wasmtime_wasi/struct.WasiCtxBuilder.html);
  [`Store`](https://docs.wasmtime.dev/api/wasmtime/struct.Store.html).
- Wasm: [security](https://webassembly.org/docs/security/);
  [`WebAssembly.instantiate`](https://developer.mozilla.org/en-US/docs/WebAssembly/Reference/JavaScript_interface/instantiate_static);
  [GC MVP](https://github.com/WebAssembly/gc/blob/main/proposals/gc/MVP.md);
  [Why the component model](https://component-model.bytecodealliance.org/design/why-component-model.html).
- Extism: [host functions](https://extism.org/docs/concepts/host-functions);
  [manifest](https://extism.org/docs/concepts/manifest);
  [Rust `Manifest`, `timeout_ms`](https://docs.rs/extism/latest/extism/struct.Manifest.html).
