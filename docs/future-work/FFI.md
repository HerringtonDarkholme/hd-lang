# Native FFI: C Providers For Host Traits

Status: accepted by the owner, 2026-11-27; phase steps 1, 2, 2b and 3
built (the `{ c, js }` manifest form, the [C value table](../../spec/lang/10-modules.md#c-value-table)
check, the C lowering with its table section, the JavaScript glue for a
C-bound trait's `js` provider, and the [header](../../spec/cli/command-line.md#c-providers)
`hd build` writes), the rest not yet (P3);
domain: runtime and host, CLI. Design for #2388, after the owner's
[decisions](#decisions).

**Changed 2026-11-27 (owner): the terminal is not std.** Std ships only
`is_terminal` and `Command.status!` (#2382), neither of which needs FFI.
Raw mode, size, resize and key input become a terminal *package* that
ships `hd_term` under `native/<target-triple>/`, so the package path
below is the real one. Read this doc's `std.terminal`, "linked into
`hd`" and default-profile rows as that package's, bound by the package's
own `[host]` table and granted by its user. It revises
[Package Host Traits](../../spec/lang/10-modules.md#package-host-traits)
and [Native Package Providers](../../spec/cli/command-line.md#native-package-providers).

## The Proposal

- C becomes a provider kind for any package's `@host` trait, beside the
  JavaScript file. The contract, root consent, preflight and handle
  registry are reused.
- A C-bound trait's methods take C-ABI types only. The library exports
  one entry symbol per trait that returns a table of function pointers in
  method order, checked by ABI version and contract hash.
- A `!` method gets a token, returns at once, and is completed later from
  any thread through `hd_complete_*`. An abort slot runs on cancel.
- A package ships its libraries prebuilt in `native/<target-triple>/`.
  Std's `hd_term` is linked into `hd`, so users need no toolchain.
- `Terminal` is std hd code over the public, flat, C-bound `TermHost`.
  Node and the web load a JavaScript provider of the same trait.

```text
# lib/std/terminal.hd, proposed
@host
pub trait TermHost:   # public, as WireHttp is; bound to C: C-ABI types only
    fn enable_raw(&mut self) -> i32                # 0, or -errno
    fn disable_raw(&mut self) -> i32
    fn read!(&mut self) -> Result[List[u8], i32]   # hd_complete_bytes or hd_complete_error

struct NativeTerminal:   # private; the default profile binds it for `$ Terminal`
    host: *mut dyn TermHost

fn status(code: i32) -> Result[void, ConsoleError]:
    if code < 0: .Err(.Other(-code)) else: .Ok(())

impl Terminal for NativeTerminal:
    fn enable_raw_mode!(&mut self) -> Result[void, ConsoleError]:
        status(self.host.enable_raw())

    fn disable_raw_mode!(&mut self) -> Result[void, ConsoleError]:
        status(self.host.disable_raw())

    fn read!(&mut self) -> Result[List[u8], ConsoleError]:
        match self.host.read!():
            .Ok(bytes) => .Ok(bytes)
            .Err(code) => .Err(.Other(code))
```

## Open Questions

None. The owner's answers are under [Decisions](#decisions).

## Problems It Solves

| Program | Today | Proposed |
| --- | --- | --- |
| `cat notes.md \| pager`: keys come from `/dev/tty` | no API; #2382 planned new Rust and TypeScript host code | `hd_term` opens stdin or `/dev/tty`; the rest is std hd code |
| `race!(term.read!(), sleep!(16ms))` | no API | `read!` waits on a completion; a cancel runs `read_abort` and loses no byte |
| a trap while the terminal is raw | the shell stays raw | `destroy` runs at instance close, trap or not, and restores the mode |
| a package wrapping `sqlite3` | needs a custom Rust embedder ([`native-cli`](../../spec/cli/command-line.md#r-cli.host.package.native-cli)) | a flat C shim in `native/<triple>/` and the app's root consent |

## How It Works

**Declaring.** The manifest decides the provider kind; the trait source is
an ordinary `@host` trait. One entry may name both kinds:

```toml
[host]
"terminal.TermHost" = { c = "hd_term", js = "providers/term.mjs" }
```

When `[host]` names `c`, `hd check` holds the trait's methods to this
table; anything else is `boundary-not-safe`.

| hd type | C type | Wasm import type |
| --- | --- | --- |
| `bool`, integers, `f32`, `f64` | the C type of the same width (`usize` is `size_t`) | `i32`, `i64`, `f32`, `f64` |
| `&T`, `&mut T`, T a number type | `const T *`, `T *` | `i32`, the address |
| `&List[T]`, `&mut List[T]`, T a number type | `const T *`, `T *`, then a `size_t` length | `i32`, `i32` |
| `string` | `const uint8_t *` to its UTF-8 bytes, then a `size_t` length, no NUL | `i32`, `i32` |
| a returned host handle | `void *`, the library's handle object | `i32`, a registry handle |
| `!` result: a number, `List[u8]`, or `Result[` one of them `, i32]` | completed through the token | the `.finish` result |

A pointer lives for the call, as any local loan
([pointer loans](../compiler/ownership.md#pointer-loans-model-a)); a `!`
method takes scalars, because the instance runs on while C works.

**The C side.** `hd build` in the declaring package writes
`native/include/<package>.h` from the trait, byte for byte the same each
time. Its table holds
`abi_version`, the 16-byte `contract_hash`, `create`, `destroy`, then the
methods in source order, each `!` method followed by its abort slot. The
token carries the host's completion table, so `hd_complete_*` are inline
functions of the header, and a library links against nothing in `hd`.
`TermHost`'s header is in [Appendix E](#the-c-header).

**Wasm.** A C-bound trait's methods are package host imports under the
same contract, and a `!` method is the `.start` and `.finish` pair. New:
each argument is an import parameter, with no exchange buffer, and a
pointer argument passes its address and length. A plain package method
encodes every argument in the buffer instead. The module records each
C-bound trait's slots and hash in an `hd.c` section beside the
contract, which JavaScript hosts do not read.

**Binding on Wasmtime.** Before the guest starts, the host opens each
package library from `native/<triple>/`, looks up its entry symbol,
checks version and hash, and calls `create` per reachable trait. Any
miss refuses the run, naming package, triple and trait. Std's linked
`hd_term` takes the same path without a file.

Per call, the binder adds the memory's base to each pointer and calls the
slot with `self`, with no codec. Std's binders are typed
(`Linker::func_wrap`); a package's signatures get one Cranelift
trampoline each per run, as Deno's do. The base cannot move mid-call: a
shared memory's never does, and only the calling instance grows its own.

**Completions.** As for a native provider's pending call:
`hd_complete_*` may run on any thread and wakes the reactor, the first
completion wins, and a later one is ignored. A cancel before the guest
takes the result runs the abort slot at most once, even after C
completed, and its `completed` flag says the result was discarded. A token lives until completed or aborted. Bytes
are copied at once, then into a new `List[u8]` on the instance's thread.

**Engines.** Wasmtime binds `c`; Node and the web load `js`, with
pointers as typed-array views. With no `--engine`, `hd` picks Wasmtime
when every required trait has `c`, else Node. A `c`-only trait is refused
on Node and the web at startup.

**The Terminal.** `hd_term` keeps the saved termios, opens `/dev/tty`
when stdin is not a terminal, and reads on one thread of its own. The
default profile binds `$ Terminal` to `NativeTerminal` over `$ TermHost`,
as `Http` is bound over `WireHttp`, under `Terminal = true`. Commands and
the fake are in [Appendix E](#the-terminal-end-to-end).

## Cost

Line counts are estimates from the nearest built component, not
measurements.

| Component | Lines | Per-run cost |
| --- | --- | --- |
| manifest `{ c, js }`, engine choice, refusals | +120 Rust | O(host entries) |
| checker: C type table; header writer | +300 Rust | O(methods of C-bound traits) |
| Wasm: pointer arguments, slots and hash in the contract | +180 Rust | O(reachable methods) |
| Wasmtime: open, entry, checks, `create`/`destroy`, tokens, aborts, trampolines | +600 Rust | O(libraries + reachable methods), at startup |
| JavaScript glue: typed-array views for pointers | +100 TypeScript | none |
| `hd_term`: termios, `/dev/tty`, reader thread; Windows console | +450 Rust | none until created |
| `std.terminal` and its profile row | +150 hd, +10 Rust | none |
| Node and web `TermHost` providers; `Console` queries | +200 TypeScript, +40 Rust | none |
| spec, conformance and CLI cases | +180 | none |
| #2382's Terminal host methods on two hosts, not written | about -450 avoided | none |

- **Per call:** one Wasmtime host call, one addition per pointer, one
  indirect call; a `!` call adds a reactor wake and two copies of bytes.
- **Per run:** one `dlopen` and one entry lookup per package library, a
  `create` per reachable trait, a trampoline per distinct signature.
  Nothing grows with heap, program size or stored state.
- **Removed:** nothing; the web compiler grows about 3 KB gzip. Build
  order is in [Appendix E](#phases).

## Appendix A. Stress Test

| # | Case | What happens | What the design does |
| --- | --- | --- | --- |
| 1 | crossterm `enable_raw_mode` (`terminal/sys/unix.rs:108`) | needs `struct termios`, laid out differently on Linux and macOS (libc crate) | stays inside `hd_term`'s `enable_raw`; hd sees an `i32` |
| 2 | Go `x/term` `makeRaw` (`term_unix.go`, v0.48.0) | per-OS ioctl constants (`TCGETS`, `TIOCGETA`) | same: they live in `hd_term` |
| 3 | Windows: no `/dev/tty`, no termios | a termios API would not port | `CONIN$` and `SetConsoleMode` behind the same table |
| 4 | `race!(term.read!(), sleep!(16ms))` at 60 frames a second | 60 cancels a second | each cancel runs `read_abort`; one reader thread serves every token, so no thread piles up |
| 5 | abort and completion race, see below | bytes could be lost | **found**: lossless because abort also runs after an accepted completion (decision 13) |
| 6 | an instance traps while raw | `atexit` would not run in an embedder | `destroy` runs at close, trap or not, and restores the mode |
| 7 | `SIGKILL` while raw | nothing runs | left raw, as with crossterm |
| 8 | a library completes a token after `read_abort` returned | use after free of the token | forbidden: a token's life ends at completion or abort; a concurrent completion is ignored |
| 9 | a library built for an older trait | wrong slots | the contract hash differs: refused at startup, naming both hashes |
| 10 | `native/` lacks `riscv64gc-unknown-linux-gnu` | no library | refused at startup, naming the package and the triple |
| 11 | `fn read!(&mut self, into: &mut List[u8]) -> i64` in a C-bound trait | a cancel would free `into` while C writes | `boundary-not-safe`: `!` methods take scalars |
| 12 | `fn put(&mut self, row: Row)` with a struct, in a C-bound trait | no C layout | `boundary-not-safe` |
| 13 | C keeps a pointer, or writes past a length | corrupts the instance's heap | not checkable, as in every FFI; C has the process's authority, as Deno warns |
| 14 | a worker grows the shared memory during a call | the base could move | a shared memory's base never changes (Wasmtime `SharedMemory::data`) |
| 15 | two worker instances each `create` a `TermHost` | one terminal, two readers | `hd_term` keeps one reader: a second `enable_raw` returns `-EBUSY` |
| 16 | a user names `$ TermHost` and implements it | `TermHost` is public | harmless: a user's provider is hd code, and only the profile binds the C one, under `Terminal = true` |
| 17 | `sqlite3_open(const char *, sqlite3 **)` | needs a NUL and a host pointer | **found**: every C API needs a flat shim; the shim adds the NUL and keeps `sqlite3 *` as `self` or a returned handle |
| 18 | `sqlite3_exec` with a callback | C would call into hd | not expressible: no function types in a C-bound trait |
| 19 | a `c`-only trait under `hd run --engine node` | no provider | refused at startup, not mid-run |
| 20 | an app needing a `c`-only and a `js`-only trait | no engine has both | **found**: refused everywhere, naming both traits |
| 21 | a test of the pager | must not touch a terminal | `$.with(Terminal=ScriptedTerminal::new(chunks))`; no C runs |

Case 5, the abort and completion race. `hd_term`'s reader thread reads
into its own buffer and completes a token under the same lock that
`read_abort` takes:

| Order | Without decision 13 | With it |
| --- | --- | --- |
| abort, then the reader sees the token aborted | bytes stay buffered for the next `read!` | same |
| completion accepted, then cancel before hd takes it | the host drops the bytes: lost | abort runs; `hd_term` puts its copy back at the front |
| completion races abort on two threads | the lock orders them into one of the rows above | same |

`hd_term` keeps a copy of its last completed bytes until the next `read`
begins, which proves the earlier result was taken or aborted.

Case 1 rewritten: crossterm's saved-termios logic moves into
`hd_term`'s `enable_raw` slot.

```rust
// hd_term, Unix: the termios layout stays in the libc crate
extern "C" fn enable_raw(this: *mut c_void) -> i32 {
    let term = unsafe { &mut *this.cast::<Term>() };
    let fd = match term.fd() { Ok(fd) => fd, Err(e) => return -e };
    let mut t = unsafe { std::mem::zeroed::<libc::termios>() };
    if unsafe { libc::tcgetattr(fd, &mut t) } != 0 { return -errno(); }
    term.saved.get_or_insert(t);          // destroy restores it
    unsafe { libc::cfmakeraw(&mut t) };
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &t) } != 0 { return -errno(); }
    0
}
```

## Appendix B. Prior Art, With Code

**Node-API** ([n-api](https://nodejs.org/docs/latest-v26.x/api/n-api.html), Node 26):

```c
NAPI_MODULE_INIT(/* napi_env env, napi_value exports */) {
  return create_addon(env);   /* one entry point per addon */
}
```

`NAPI_VERSION` bakes the API version into the addon. A thread-safe
function's callback "will be called on the main thread". hd takes the
one entry point, the version check and completion on the instance's
thread. Binaries built with `node-addon-api` "depend on the symbols of the
Node-API C-based functions exported by Node.js"; hd's token carries the
table instead.

**Python** ([extending](https://docs.python.org/3/extending/extending.html), 3.15):

```c
static struct PyModuleDef spam_module = {
    .m_base = PyModuleDef_HEAD_INIT, .m_name = "spam", .m_slots = spam_module_slots,
};
PyMODINIT_FUNC PyInit_spam(void) { return PyModuleDef_Init(&spam_module); }
```

All symbols but the init function "should be declared `static`". hd's
`hd_term_TermHost` is the same shape: one exported function returning a
static table.

**SQLite extensions** ([loadext](https://www.sqlite.org/loadext.html)):

```c
int sqlite3_extension_init(sqlite3 *db, char **pzErrMsg,
                           const sqlite3_api_routines *pApi) {
  SQLITE_EXTENSION_INIT2(pApi);
  return SQLITE_OK;
}
```

The entry point is found by name, `sqlite3_X_init` from the file name,
and receives `pApi`. hd names its entry from the library and the trait.

**Deno** ([FFI](https://docs.deno.com/runtime/fundamentals/ffi/), v2.9.8):

```ts
const lib = Deno.dlopen("libexample.so", {
  add: { parameters: ["i32", "i32"], result: "i32" },
  slow: { parameters: ["u64"], result: "u64", nonblocking: true },
});
lib.symbols.add(5, 3);
await lib.symbols.slow(1n);   // runs on a blocking thread
```

Deno builds a per-signature Cranelift trampoline, with libffi as the
fallback ([dlfcn.rs](https://github.com/denoland/deno/blob/v2.9.8/ext/ffi/dlfcn.rs)).
`nonblocking` runs in `spawn_blocking` with no cancellation
([call.rs](https://github.com/denoland/deno/blob/v2.9.8/ext/ffi/call.rs)).
It says loading a library equals `--allow-all`. hd takes the trampoline;
completion callbacks replace the thread pool, so a cancel reaches C.

**Bun** ([bun:ffi](https://bun.sh/docs/runtime/ffi), v1.4.5):

```ts
import { dlopen, FFIType, suffix } from "bun:ffi";
const lib = dlopen(`libadd.${suffix}`, {
  add: { args: [FFIType.i32, FFIType.i32], returns: FFIType.i32 },
});
```

Experimental; "Async functions are not supported". `buffer_length` passes
a view's length beside its pointer, as hd does for lists and strings.

**Python ctypes** ([docs](https://docs.python.org/3/library/ctypes.html)):

```py
libc = CDLL("libc.so.6")
libc.strchr.restype = c_char_p
libc.strchr.argtypes = [c_char_p, c_char]
```

It rebuilds the libffi `ffi_cif` on every call
([callproc.c](https://github.com/python/cpython/blob/v3.15.2/Modules/_ctypes/callproc.c)).
hd builds each call path once per run.

**Rust and Go.** Rust's `unsafe extern "C" { fn tcgetattr(...); }` binds
at link time, and the `libc` crate defines `termios` per target
([docs.rs](https://docs.rs/libc/latest/libc/struct.termios.html)). Go's cgo
rule: "C code may not keep a copy of a Go pointer after the call
returns" ([cgo](https://pkg.go.dev/cmd/cgo)). hd takes the pointer rule
and leaves per-target structs to C.

```go
// static void myprint(char* s) { printf("%s\n", s); }
import "C"
func main() { cs := C.CString("hi"); C.myprint(cs); C.free(unsafe.Pointer(cs)) }
```

**Wasmtime** ([Func](https://docs.rs/wasmtime/51.0.0/wasmtime/struct.Func.html); hd pins 49.0.2):

```rust
linker.func_wrap(module, "enable_raw", move |caller: Caller<'_, Host>| -> i32 {
    unsafe { (table.enable_raw)(self_of(&caller)) }
})?;
```

`Func::wrap` is "both safer and faster" than `func_new`; memory growth
"may change the base pointer", but a shared memory's "does not change".

**libffi** ([manual](https://github.com/libffi/libffi/blob/v3.8.2/doc/libffi.texi), 3.8.2):

```c
ffi_prep_cif(&cif, FFI_DEFAULT_ABI, 1, &ffi_type_sint, args);  /* once */
ffi_call(&cif, (void (*)(void))puts, &rc, values);             /* per call */
```

`ffi_call` reclassifies each argument per call (`ffi64.c`). It stays the
fallback if a Cranelift trampoline cannot be built.

**Node 26** ([node:ffi](https://nodejs.org/docs/latest-v26.x/api/ffi.html)):

```js
import { dlopen, suffix } from "node:ffi";
using h = dlopen(`./mylib.${suffix}`, { add_i32: { arguments: ["int32", "int32"], return: "int32" } });
```

Experimental since v26.1.0, with no blocking-call option. It works in the
pinned 26.13.1 (`dlopen(null)` called `getpid`, with an
`ExperimentalWarning`). hd keeps JavaScript providers on Node.

**WASI.** [`terminal.wit`](https://github.com/WebAssembly/wasi-cli/blob/v0.2.8/wit/terminal.wit)
0.2.8 has no raw mode, size or input method.

## Appendix C. Alternatives Considered

- **`@foreign` functions** (this PR's first draft). Std declared C
  functions with `@foreign("lib")`, each a TIR `CallForeign` and an
  `hd:ffi` import, std only, with blocking calls on a thread pool.
  Rejected by the owner: C is called through a host trait, as a
  JavaScript provider is.
- **An `extern` block.** New grammar, the costliest kind of change.
- **One `dlsym` per method.** Rejected for one entry symbol, which
  checks the whole table's version and hash once.
- **Blocking calls on a thread pool.** A running C call cannot be
  cancelled, and each cancelled wait holds a thread.
- **Restore at `atexit` only.** Misses an instance that traps in a living
  process; `destroy` covers both.
- **Hiding `TermHost`.** Rejected by the owner: it is public, as
  `WireHttp` is.
- **A Rust native provider.** Built, but it needs a custom embedder; it
  stays for that case.
- **Declaring `termios` in hd per OS.** Rejected by the owner.

## Appendix D. Today In hd

- **Package host traits** (built, #51): an SDK's `@host` trait maps in
  `[host]` to a JavaScript file; the root `[capabilities]` consents
  ([design](../compiler/package-host-capabilities.md)).
- **Native providers** (built, #2077 to #2083): `NAME.host.rs`, a custom
  embedder's registry, pending calls, abort at most once, late completion
  ignored, and the handle registry (`crates/hd_run_wasmtime/src/native.rs`).
- **Import lowering**: a package method's arguments all go through the
  exchange buffer (`rt::profile_tree`, `push_args` in
  `crates/hd_wasm/src/boundary.rs`); a std ABI row may pass scalars as
  import parameters.
- **The contract** lists reachable methods only, in source order
  (`hd_host_abi::schema::Interface`).
- **A provider bridge**: `Http` is std code over the public `@host`
  `WireHttp`, and grants name `Http` (`PROVIDER_BRIDGES` in
  `crates/hd_host_abi/src/lib.rs`; [`std-http.wire.host`](../../spec/std/http.md#r-std-http.wire.host)).
- **Dependencies**: `hd add` and `hd fetch` record each fetched tree's
  hash, which covers a package's `native/` folder.

Rules this design revises, quoted:

| Rule | Today | Proposed |
| --- | --- | --- |
| [Host Boundary note](../../spec/lang/10-modules.md#host-boundary) | "hd has no `extern` declaration and no FFI capability. A Wasm module can call only its imports, so a custom host trait that an embedder binds is hd's foreign function interface" | "hd has no `extern` declaration. A host trait is hd's foreign function interface: its provider is JavaScript, C that a package ships, or Rust that an embedder registers." |
| [`native-no-discovery`](../../spec/cli/command-line.md#r-cli.host.package.native-no-discovery) | "An implementation never discovers or loads a dynamic library for a package host trait." | "An implementation loads a dynamic library only for a C provider that a package's `[host]` table names, from that package's `native/<target-triple>/` folder." |
| [`native-cli`](../../spec/cli/command-line.md#r-cli.host.package.native-cli) | "The `hd` command registers no native provider, so on Wasmtime it refuses every module requiring a package host trait" | "On Wasmtime, `hd` binds C providers, and refuses a required trait that has neither a C provider nor an embedder's registration." |
| [`native-cli-node`](../../spec/cli/command-line.md#r-cli.host.package.native-cli-node) | "Without an `--engine` flag, `hd run` and `hd FILE.wasm` run a module requiring a package host trait on Node" | "Without an `--engine` flag, they run on Wasmtime when every required package host trait has a C provider, and else on Node." |

## Appendix E. Details

### Proposed Spec Rules

Language tier, Package Host Traits and Runtime Profiles in
`spec/lang/10-modules.md`:

1. r[module.host.package.c-types] A package host trait bound to a C provider declares only methods whose parameter and result types are rows of the C value table. Error: `boundary-not-safe`.
2. r[module.host.package.c-references] In such a trait, a local reference to a number or to a list of numbers is boundary-safe. The C function may use it only until the call returns.
3. r[module.host.package.c-suspend] A suspending method of such a trait takes only numbers and `bool`. Its result is a number, `List[u8]`, or a `Result` of one of them with an `i32` error. Error: `boundary-not-safe`.
4. r[module.host.package.c-no-supertraits] Such a trait declares no supertraits: its C function table holds its own methods, in source order. Error: `boundary-not-safe` (decision 14).
5. r[module.host.package.c-parity] A C provider and a JavaScript provider of the same trait observe the same hd values.
6. r[module.profile.bridge] A profile may bind a public std trait to std code over a std host trait. Grants name the public trait.

```text
@host
pub trait Disk:   # bound to C by its package's [host] table
    fn read!(&mut self, into: &mut List[u8]) -> i64   # error: boundary-not-safe
    fn put(&mut self, row: Row) -> i32                 # error: boundary-not-safe
```

> **Why.** A suspending call outlives its frame when it is cancelled, so
> it holds no pointer into the instance. C has no layout for a struct, so
> a C shim flattens it.

CLI tier, Package Host Declarations and a new C Providers section:

1. r[cli.host.declaration.kinds] A `[host]` value is a provider-file string, or a table with a `c` library name, a `js` provider file, or both. Error: `invalid-manifest`.
2. r[cli.host.c.file] `c = "NAME"` names the file `libNAME.so`, `libNAME.dylib` or `NAME.dll` in the package's `native/<target-triple>/` folder.
3. r[cli.host.c.target-missing] A missing file for the running target refuses the run before the guest starts, naming the package and the triple. Error: `native-provider-missing`.
4. r[cli.host.c.entry] The library exports one function per trait, named `NAME_TRAIT`, that returns the trait's table.
5. r[cli.host.c.check] A table whose `abi_version` or `contract_hash` differs from the module's contract refuses the run, naming the trait.
6. r[cli.host.c.create] The host calls `create` once for each reachable C-bound trait before the guest starts, and `destroy` once when the instance closes, whether or not it trapped.
7. r[cli.host.c.thread] The host calls every slot on the instance's thread. A library may call `hd_complete_*` on any thread.
8. r[cli.host.c.complete] The first completion of a token decides the call's result, and the host ignores later ones.
9. r[cli.host.c.abort] Cancelling a call before the guest takes its result runs its abort slot at most once, also after the library completed it; `completed` is then true.
10. r[cli.host.c.token] A library uses a token until it completes it or the token's abort slot returns.
11. r[cli.host.c.bytes] `hd_complete_bytes` copies the bytes before it returns, and the guest receives them as a new `List[u8]`.
12. r[cli.host.c.engine] A trait with only a C provider is refused on JavaScript hosts before the guest starts. Error: `native-provider-missing`.
13. r[cli.host.terminal] The default profile binds `Terminal` to std's provider over `TermHost`, and `Terminal` is a boolean capability key.
14. r[cli.host.c.header] `hd build` in a package declaring a C-bound trait writes `native/include/<package>.h`, and writing it twice gives the same bytes.

The four revised rules are in [Appendix D](#appendix-d-today-in-hd).
The `std-terminal.*` rules are #2382's.

### The C Header

`TermHost`'s table and entry symbol:

```c
typedef struct hd_TermHost {
    uint32_t abi_version;                         /* HD_HOST_ABI_VERSION */
    uint8_t contract_hash[16];                    /* this trait's signatures */
    void *(*create)(void);                        /* at instance start */
    void (*destroy)(void *self);                  /* at instance close, trap or not */
    int32_t (*enable_raw)(void *self);
    int32_t (*disable_raw)(void *self);
    void (*read)(void *self, hd_token token);     /* returns at once */
    void (*read_abort)(void *self, hd_token token,
                       bool completed);       /* true: a completed result was discarded */
} hd_TermHost;

const hd_TermHost *hd_term_TermHost(void);        /* the one entry symbol */
```

`hd_complete_i32`, `hd_complete_i64`, `hd_complete_f64`,
`hd_complete_bytes(t, ptr, len)` and `hd_complete_error(t, errno)` are
inline functions that call through the token's table.

As built (step 3), the header also has these parts. The fixture
`spec/conformance/cli/host-c-header/expected.h` shows one whole header.

| Part | As built |
| --- | --- |
| token | `typedef struct hd_call *hd_token`, whose first field points to the host's `hd_completions` table |
| a `!` method's slot | the token comes second, after `self` and before the arguments, as in the abort slot |
| a list or string argument | the pointer, then `size_t NAME_len` |
| contract hash | `#define HD_CONTRACT_HASH_LIBRARY_TRAIT { ... }`, for the table's initializer |
| shared part | guarded by `HD_HOST_ABI_VERSION`, so two packages' headers can be included together |
| names | a C keyword or a table field name takes a trailing `_`; non-ASCII names are written as universal character names |

### The Terminal, End To End

| Command | What happens |
| --- | --- |
| `hd run` | Wasmtime takes `hd_term_TermHost` from `hd` itself, checks it, calls `create` |
| `hd build --release` | the `.cwasm` holds the `TermHost` imports; no file beside it |
| `hd install` | the same artifact and launcher; a package's `native/<triple>/` file is installed with it |
| `hd run --engine node` | the built-in JavaScript `TermHost` |
| on the web | `TermHost`'s JavaScript provider returns `-ENOTTY`, so `read!` and the modes return an error; `is_terminal` is false and `size` is `.None` |
| `hd test` | `ScriptedTerminal` replaces `Terminal`; no C runs |

A program using it:

```text
use std.terminal.Terminal

pub fn main!() -> void $ Console + Terminal:
    term := $.use(Terminal)
    _ := term.enable_raw_mode!()
    match term.read!():
        .Ok(bytes) => println("read ${bytes.len()} bytes")
        .Err(error) => println("error: $error")
    _ := term.disable_raw_mode!()
```

The fake:

```text
pub struct ScriptedTerminal:
    chunks: List[List[u8]]

impl ScriptedTerminal:
    pub fn new(chunks: List[List[u8]]) -> *mut ScriptedTerminal:
        box(ScriptedTerminal { chunks: chunks })

impl Terminal for ScriptedTerminal:
    fn read!(&mut self) -> Result[List[u8], ConsoleError]:
        if self.chunks.len() == 0:
            return .Err(.Closed)
        .Ok(self.chunks.remove(0))

fn first_key!() -> u8? $ Terminal:
    match $.use(Terminal).read!():
        .Ok(bytes) => bytes.first()
        .Err(_) => .None

tests:
    it("q quits"):
        fake := ScriptedTerminal::new([[113]])
        $.with(Terminal=fake):
            assert_equal(first_key!(), .Some(113))
```

### Domain Checklists

- **Grammar:** none new; the manifest gains a table form.
- **Typing:** the C value table, checked against the manifest, rechecked
  when `[host]` changes ([`map-recheck`](../../spec/cli/command-line.md#r-cli.host.declaration.map-recheck)).
- **Ownership:** pointer arguments are ordinary call loans.
- **Host parity:** each `TermHost` method on Wasmtime and Node under a
  pseudo-terminal; the web returns `-ENOTTY`.
- **Suspension:** the existing `.start`/`.finish` pair and reactor.
- **Failure and cleanup:** `destroy` at close; a trap still closes.
- **Grants:** root consent per trait, as today; `Terminal = true`.
- **Security:** a C library has the process's authority; the fetched
  tree's hash pins its binaries.
- **Rollback:** remove the `c` kind; JavaScript and Rust providers stay.

### Phases

| # | Step | Test |
| --- | --- | --- |
| 1 | manifest `{ c, js }` and the C value table check | diagnostics stage cases for each error |
| 2 | Wasm: pointer arguments, slot and hash in the contract | `functions`-view stage case; contract snapshot |
| 3 | header writer | a generated header compiles in a C test |
| 4 | Wasmtime: open, entry, checks, `create`, `destroy` | refusal cases: missing triple, symbol, hash |
| 5 | tokens, completions, aborts | Rust test: 1000 cancels, each abort once, no late delivery |
| 6 | Cranelift trampolines for package libraries | a test library with every C type |
| 7 | `hd_term` on Unix | Rust test under a pseudo-terminal: raw, read, abort, restore |
| 8 | `std.terminal` and its profile row | `ScriptedTerminal` conformance cases |
| 9 | Node and web providers; `Console` queries | `--host` cases under a pipe |
| 10 | `hd_term` on Windows; the example pager | Windows CI; run by hand |

### Parse Log

Each `text` block was parsed with `scripts/hd-conformance.sh parse`,
which checks syntax only, and run through `hd check --tests`. The
diagnostics are only for names `std.terminal` does not have yet, and for
the `[host]` entry a single file cannot carry. The
`NativeTerminal` and `ScriptedTerminal` logic was also checked in one file
with `TermHost`, `Terminal` and `ConsoleError` declared or imported: no
diagnostics.

| Block | Parser | `hd check` |
| --- | --- | --- |
| The Proposal, `TermHost` and `NativeTerminal` | parses | `invalid-manifest` line 2 (no `[host]` entry in a single file); `unknown-trait` (`Terminal`) line 14 |
| Proposed Spec Rules, error example | parses | `invalid-manifest` line 1 (no `[host]` entry); `unknown-type` (`Row`) line 4 |
| The Terminal, a program using it | parses | `unknown-module` (`std.terminal`) line 1 |
| The Terminal, `ScriptedTerminal` | parses | `unknown-trait` (`Terminal`) lines 8, 14, 15, 22 |

## Decisions

Owner, 2026-11-26, on the first draft's questions:

1. **Declaration form:** "C func is called through host trait", as a
   JavaScript provider is. No `@foreign`, no `extern`, no std-only phase
   and no `Ffi` key.
2. **Binding:** one entry symbol per trait returns a table of function
   pointers in method order, with `abi_version` and `contract_hash`; a
   mismatch is refused at startup.
3. **Types:** C-ABI types only in a C-bound trait; std's public
   `Terminal` is hd code over a flat C-bound `TermHost`.
4. **Suspension:** completion callbacks with an `hd_token`, completed
   from any thread, with an abort slot; not a thread pool.
5. **Completion payload:** `!` arguments are scalars; a completion is a
   scalar, bytes copied into a new `List[u8]`, or an errno.
6. **Lifecycle:** `create()` at instance start for a reachable trait,
   `destroy(self)` once at close, trap or not; the terminal's `destroy`
   restores raw mode.
7. **Engines:** one entry may name `c` and `js`; with no `--engine`,
   Wasmtime when every required trait has `c`, else Node; `c`-only is
   refused on Node and the web.
8. **Shipping:** prebuilt libraries in `native/<target-triple>/`; a
   missing target is refused at startup; std's `hd_term` is linked into
   `hd`.
9. **Terminal row:** the default profile binds `$ Terminal` to std's
   `NativeTerminal` over `$ TermHost`; `Terminal = true` covers both.
10. **Console:** `is_terminal` and `size` stay host methods; Wasmtime's
    call the linked `hd_term` code.
11. **`TermHost` visibility:** "just make TermHost public". It is a
    public `@host` trait in std, so `module.host.package.marker` is
    unchanged and nothing hides it.

Owner, 2026-11-26, on the rewrite's questions:

12. **C header:** `hd build` in the declaring package writes
    `native/include/<package>.h`; a changed trait changes the contract
    hash, so a stale library is refused at startup.
13. **Abort after completion:** a cancel runs the abort slot whenever the
    guest did not take the result, even after C completed, with
    `completed` true; `hd_term` returns those bytes on the next `read!`.

Owner, 2026-11-27, on phase step 1's questions:

14. **Supertraits:** a C-bound trait declares no supertraits, since its
    table holds its own methods in source order; `hd check` refuses one
    with `boundary-not-safe` (built in step 1).
15. **Node glue:** from step 2, a C-bound trait's imports take C
    arguments, so Node and the web refuse it at startup, `{ c, js }` as a
    C-only trait, with `native-provider-missing`. The generic JavaScript
    glue (typed-array views for pointers, scalar parameters) follows as
    step 2b and lifts the refusal for `{ c, js }` (built:
    [`module.host.package.c-js-views`](../../spec/lang/10-modules.md#r-module.host.package.c-js-views)).

A third answer, on returned handles, takes effect in step 4 and is
recorded there.
