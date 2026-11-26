# Native FFI: hd Calls C

Status: proposal, 2026-11-26; domain: language, runtime and host, CLI;
not accepted. This is the design for #2388, and its first user is
`std.terminal` (#2382). Nothing in it is accepted behavior. Under review:
the owner's decision of 2026-11-26 (hd calls C directly, option D),
[Host Boundary](../../spec/lang/10-modules.md#host-boundary) and its FFI
note, [Native Package Providers](../../spec/cli/command-line.md#native-package-providers),
and [Capability Grants](../../spec/cli/command-line.md#capability-grants).

## The Proposal

- A std module declares a C function with `@foreign("LIBRARY")` on an
  ordinary function declaration, the form `@intrinsic` already uses. The
  signature holds C-ABI types only. There is no grammar change.
- A foreign call is one backend-neutral TIR primitive. The Wasm backend
  lowers it to an import from `hd:ffi/LIBRARY`; an LLVM backend would
  lower it to a direct call.
- A foreign function named with `!` blocks. It runs on a host thread, and
  the calling task suspends while the instance runs other tasks.
- Per-OS layouts stay in a small flat C library. Std's first one,
  `hd_term`, has seven functions and is linked into `hd`. Node and the
  web supply the same symbols in JavaScript.
- Phase 1 is std only. Packages come in phase 2, under a root grant that
  names each library, as Deno's `--allow-ffi=PATH` does.

The decided `Terminal` (#2382), written in hd over `hd_term` in place of
new Rust and TypeScript host code:

```text
# lib/std/terminal.hd, proposed
@foreign("hd_term")
fn hd_term_read(term: i32, into: &mut List[u8], cap: usize) -> i64:   # never blocks
    panic("foreign")

@foreign("hd_term")
fn hd_term_wait!(term: i32) -> i32:   # `!`: runs on a host thread
    panic("foreign")

struct NativeTerminal:   # private: the default profile binds it for `$ Terminal`
    term: i32

impl Terminal for NativeTerminal:
    fn read!(&mut self) -> Result[List[u8], ConsoleError]:
        while true:
            let mut bytes: List[u8] = []
            bytes.resize(4096, 0)
            cap := bytes.len()
            n := hd_term_read(self.term, &mut bytes, cap)   # C writes into the list, for this call only
            if n > 0:
                bytes.truncate(usize(n))
                return .Ok(bytes)
            if n < 0:
                return .Err(.Other(i32(-n)))
            ready := hd_term_wait!(self.term)   # suspends; reads nothing, so a cancel loses no byte
            if ready < 0:
                return .Err(.Other(-ready))
        .Ok([])
```

## Open Questions

One line each; options are in [Questions For The Owner](#questions-for-the-owner).

1. Declare with `@foreign("lib")`, not an `extern` block? **Recommend yes.**
2. Std only in phase 1, packages in phase 2? **Recommend yes.**
3. A `!` foreign function blocks on a host thread, scalars only? **Recommend yes.**
4. A cancelled blocking call runs on and its result is dropped? **Recommend yes.**
5. `hd_term` linked into `hd`, and published as a dylib for embedders? **Recommend yes.**
6. `Console.is_terminal` and `size` as host methods over `hd_term`? **Recommend yes.**
7. Phase 2 consent as an `Ffi` key in `[capabilities]`? **Recommend yes.**
8. Package FFI refused on Node, which picks Wasmtime? **Recommend yes, for now.**
9. Raw mode restored at process exit only, not at instance close? **Recommend yes, for now.**

## Problems It Solves

| Program | Today | Proposed |
| --- | --- | --- |
| `cat notes.md \| pager`: keys come from `/dev/tty`, not stdin | no API; #2382 planned six host methods in Rust and TypeScript | `hd_term_open` picks stdin or `/dev/tty`; the rest is std hd code |
| `race!(term.read!(), sleep!(16ms))` in a game loop | no API | `read!` suspends in `hd_term_wait!`; the timer cancels a wait that read nothing |
| a panic while the terminal is raw | the shell stays raw | `hd_term_enable_raw` registers an `atexit` restore; a panic exits `hd` through `exit` |
| a package that needs `sqlite3` | a native provider needs a custom Rust embedder ([`native-cli`](../../spec/cli/command-line.md#r-cli.host.package.native-cli)) | phase 2: `@foreign("sqlite3")`, a manifest path, and the app's `Ffi` grant |

## How It Works

**Declaring.** A foreign function is a module-level function after
`@foreign("LIB")`, with body `panic("foreign")`, the shape of std's
`@intrinsic` declarations. Its name is the C symbol. It is never `pub`,
so callers go through its module's wrapper; in phase 1 only std sees the
name `foreign`.

| hd type in the signature | C type | Wasm import type |
| --- | --- | --- |
| `bool`; `i8` to `u32`; `i64`, `u64`; `f32`, `f64` | the C type of the same width | `i32`; `i32`; `i64`; `f32`, `f64` |
| `usize` | `size_t` | `i32`; a result out of range panics `host-contract` |
| `&T`, `&mut T`, T a number type | `const T *`, `T *` | `i32`, the address |
| `&List[T]`, `&mut List[T]`, T a number type | `const T *`, `T *` to element 0; the length is its own `usize` | `i32` |
| `string` | `const uint8_t *` to its UTF-8 bytes, no NUL | `i32` |
| `u64` holding a host pointer C returned (opaque handle) | `void *` | `i64` |

Structs, enums, optionals, closures and variadic functions are out; a
per-OS struct is the library's job. Callbacks are out of scope, since C
would re-enter the instance inside a call. A pointer is valid for the
call only: each reference argument is already a local loan that ends at
the return ([pointer loans](../compiler/ownership.md#pointer-loans-model-a)),
and the library contract forbids keeping it, as cgo's rules do.

**Calling.** A call is a TIR `CallForeign` (a: the symbol, b: the
arguments), which ownership treats as any call. The Wasm backend emits
one import per reachable symbol, `hd:ffi/hd_term` `hd_term_read`, and
lists each with its C signature in an `hd.foreign` custom section. A
blocking call lowers to the existing `.start` and `.finish` host pair.

**Binding on Wasmtime.** Startup reads `hd.foreign` once, beside the
native-provider preflight. Std's libraries are built into `hd`: a static
table maps each symbol to its C signature and a typed binder
(`Linker::func_wrap`). A missing symbol or a differing signature refuses
the run before the guest starts (`foreign-unavailable`).

A binder adds the memory's base to each pointer argument and calls C,
with no codec and no copy. The base cannot move during a synchronous
call: a shared memory's never changes, and only the calling instance
grows its own. A `.cwasm` image is unchanged, since foreign imports bind
at instantiation. Phase 2 opens a package library with `dlopen` and
builds one trampoline per signature with Cranelift, as Deno does.

**Blocking calls.** A `!` foreign function runs on the `hd-io` blocking
pool, as `Http.send!` does, and its completion wakes the reactor. It
takes scalars only, because the instance keeps running and could grow
memory or drop a buffer's frame. A running C call cannot be stopped: a
cancel drops its result, as with Deno's `nonblocking` and Go's cgo.
`hd_term_wait` wakes any earlier waiter, so `race!` leaves at most one
blocked thread.

**Node and the web.** For each std library, the JavaScript host carries
a module with the same symbols; its `!` symbols return a Promise. On
Node, `hd_term` uses `setRawMode`, a `tty.ReadStream` on `/dev/tty` and
the `readable` event, and restores the mode on `exit`; no toolchain is
needed. On the web, `hd_term_open` returns `-ENOTTY`, so `is_terminal`
is false, `size` is `.None`, and the rest return `Unsupported` (#2382).
Node 26's `node:ffi` is experimental and cannot block, so phase 2 refuses
package libraries on Node, and `hd` with no `--engine` picks Wasmtime.

**Authority.** There is no `unsafe` keyword: the private declaration is
the unsafe part, and its module's `pub` API is the safe wrapper. A std
library needs no grant of its own. `Terminal = true` in `[capabilities]`
grants the trait, and the default profile binds it to std's private
`NativeTerminal`, so only code with `$ Terminal` reaches `hd_term`. That
binding of a std-written provider is new and needs approval. In phase 2,
`Ffi = ["dep.sqlite_hd.sqlite3"]` consents to a package library, which
has the process's authority, as Deno warns.

**The Terminal.** `hd_term` has seven flat C functions; a terminal
number hides `/dev/tty` or Windows' `CONIN$`. `hd run` and a `.cwasm`
bind its imports from the table in `hd`; `hd install` adds no file;
`hd test` swaps in `ScriptedTerminal`. `Console.is_terminal` and `size`
stay host methods; Wasmtime's calls `hd_term` from Rust, so the platform
code exists once. The C API, commands and fake are in
[Appendix E](#the-terminal-end-to-end).

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

## Cost

Line counts are estimates from the nearest built component, not
measurements.

| Component | Lines | Per-run cost |
| --- | --- | --- |
| checker: `@foreign`, std-only, type table, `!` rule | +250 Rust | O(foreign declarations) |
| TIR `CallForeign`, ownership, verifier | +150 Rust | O(calls), as any call |
| Wasm lowering: imports, `hd.foreign`, blocking pair | +200 Rust | O(reachable symbols) |
| profile binds a std-written provider | +150 Rust | O(row traits) |
| Wasmtime binder: preflight, table, typed binders | +300 Rust | O(reachable symbols), at startup |
| `hd_term`: termios, `/dev/tty`, `poll`, `ioctl`; Windows console | +450 Rust | none until called |
| `std.terminal`: trait, `NativeTerminal`, `ScriptedTerminal` | +180 hd | none |
| Node and web `hd_term` modules | +180 TypeScript | none until called |
| spec, conformance and CLI cases | +150 | none |
| #2382's six host methods on two hosts, not written | about -700 avoided | none |
| phase 2: `dlopen`, Cranelift trampolines, manifest, `Ffi` | +700 Rust | a `dlopen` per library, a compile per signature |

- **Per synchronous call:** one Wasmtime typed host call, one addition
  per pointer argument, the C call. Nothing is encoded or copied.
- **Per blocking call:** the same, plus a pool-thread hop and a reactor
  wake, as `Http.send!` pays today.
- **Startup:** a table lookup per reachable symbol; phase 1 opens no
  library. Nothing scales with heap, program size or stored state.
- **Size:** about 20 bytes of Wasm per reachable import; the web
  compiler grows by the checker rows, an estimated 3 KB gzip of 2 MB.
- **Removed:** nothing. Package host traits and native providers stay.

Build order: checker and TIR, Wasm lowering, `hd_term` and its binder,
`std.terminal` and the profile binding, then Node and web
([phases](#phases)).

## Appendix A. Stress Test

| # | Case | What happens | What the design does |
| --- | --- | --- | --- |
| 1 | crossterm's `enable_raw_mode` (`terminal/sys/unix.rs:108`): save termios, make raw, restore later | needs `struct termios`, whose size and fields differ between Linux and macOS (libc crate) | the save stays in `hd_term_enable_raw`; hd never sees the struct |
| 2 | Go `x/term` `makeRaw` (`term_unix.go`, v0.48.0) clears eight flag groups per OS | per-OS constants (`TCGETS` against `TIOCGETA`) | same: a flat call; the constants live in `hd_term` |
| 3 | Windows: no `/dev/tty`, no termios | a termios API would not port | the terminal number hides `CONIN$`; the seven functions stay |
| 4 | `race!(term.read!(), sleep!(16ms))` at 60 frames a second | each frame cancels a wait; a naive library leaves one blocked thread per frame until a key comes | **found**: `hd_term_wait` wakes the earlier waiter, so at most one thread waits |
| 5 | a blocking `read(fd, buf, n)` declared as `fn read!(fd: i32, buf: &mut List[u8], n: usize)` | the frame could be dropped by a cancel while C writes into `buf` | rejected: `invalid-foreign-function`; split into a blocking wait and a non-blocking read |
| 6 | `getaddrinfo`-shaped calls: blocking, with output buffers, no readiness call | [`module.ffi.blocking.scalars`](#foreign-functions) forbids buffers on blocking calls | **found**: the library returns a handle and a later synchronous call copies out; a copy-in-copy-out extension waits for a real case |
| 7 | `sqlite3_open(const char *, sqlite3 **)` in phase 2 | needs a NUL and an out-pointer to a host pointer | `c_string(text)` copies with a NUL; the out-pointer is `&mut u64`, right only on 64-bit hosts. **Found**: a 32-bit host needs a pointer-sized handle type |
| 8 | `qsort` or `sqlite3_exec` with a callback | C would call hd during a call | rejected at the declaration: a function type is not in the table |
| 9 | C keeps the `buf` pointer and writes later | writes into a freed hd block | not checkable, as in every FFI; phase 1 limits it to std's audited library |
| 10 | C writes past `cap` | corrupts the instance's heap | not checkable; FFI ends the Wasm sandbox for the process, as Deno's docs say |
| 11 | a worker thread grows the shared memory during a call | the base could move | a shared memory's base never changes (Wasmtime `SharedMemory::data`) |
| 12 | the instance grows its own memory during a call | impossible: it is inside the call | blocking calls take no pointer, so they need no stable base |
| 13 | panic while raw | shell left raw | `atexit` restores it; `hd` exits through `exit` |
| 14 | `SIGKILL` while raw | no handler runs | left raw, as with crossterm; `reset` fixes it |
| 15 | an embedder whose instance traps but whose process lives | `atexit` never runs | **found**: the mode stays raw; the embedder calls `hd_term_disable_raw` (question 9) |
| 16 | two worker instances each call `read!` | one terminal, two readers split the keystrokes | following #2382's one-reader rule, a second `enable_raw` returns `-EBUSY` |
| 17 | `hd_term_read` returns a `size_t` above `u32` | does not fit `usize` | a `host-contract` panic |
| 18 | std declares a signature that differs from `hd_term`'s table | wrong ABI at the call | refused at startup; phase 2's `dlopen` cannot check, which is why phase 1 is std only |
| 19 | a test that never touches a terminal | `ScriptedTerminal` replaces the trait | no foreign call runs, so the test runs on the web too |

Case 1 rewritten: crossterm's saved-termios logic moves, nearly line for
line, into `hd_term`'s Unix side, and hd sees one integer.

```rust
// hd_term, Unix: the termios layout stays in the libc crate
static SAVED: Mutex<Option<libc::termios>> = Mutex::new(None);

#[no_mangle]
pub extern "C" fn hd_term_enable_raw(term: i32) -> i32 {
    let fd = fd_of(term);
    let mut t = unsafe { std::mem::zeroed::<libc::termios>() };
    if unsafe { libc::tcgetattr(fd, &mut t) } != 0 { return -errno(); }
    let mut saved = SAVED.lock().unwrap();
    if saved.is_none() { *saved = Some(t); unsafe { libc::atexit(restore) }; }
    unsafe { libc::cfmakeraw(&mut t) };
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &t) } != 0 { return -errno(); }
    0
}
```

## Appendix B. Prior Art, With Code

**Deno** ([FFI](https://docs.deno.com/runtime/fundamentals/ffi/), v2.9.8):

```ts
const lib = Deno.dlopen("libexample.so", {
  add: { parameters: ["i32", "i32"], result: "i32" },
  slow: { parameters: ["u64"], result: "u64", nonblocking: true },
});
lib.symbols.add(5, 3);              // 8
await lib.symbols.slow(1n);         // runs on a blocking thread, returns a Promise
```

Run with `--allow-ffi[=PATH...]` ([permissions](https://docs.deno.com/runtime/reference/permissions/)).
Deno says loading a library equals `--allow-all`. A per-signature
Cranelift trampoline is the fast path, with libffi as fallback
([dlfcn.rs](https://github.com/denoland/deno/blob/v2.9.8/ext/ffi/dlfcn.rs)).
`nonblocking` runs `ffi_call` in `spawn_blocking`
([call.rs](https://github.com/denoland/deno/blob/v2.9.8/ext/ffi/call.rs)),
with no cancellation. hd takes the declaration list, `nonblocking` as
`!`, the path grant and Cranelift. It rejects structs by value.

**Bun** ([bun:ffi](https://bun.sh/docs/runtime/ffi), v1.4.5):

```ts
import { dlopen, FFIType, suffix } from "bun:ffi";
const lib = dlopen(`libadd.${suffix}`, {
  add: { args: [FFIType.i32, FFIType.i32], returns: FFIType.i32 },
});
lib.symbols.add(1, 2);
```

Experimental; JavaScriptCore now makes the calls, and TinyCC remains only
for `cc()`. "Async functions are not supported." Bun's `buffer_length`
passes a view's length with its pointer. hd passes the length explicitly
instead, because an hd list cannot be resized during the call.

**Python ctypes** ([docs](https://docs.python.org/3/library/ctypes.html), 3.15):

```py
from ctypes import CDLL, c_char_p, c_char
libc = CDLL("libc.so.6")
libc.strchr.restype = c_char_p
libc.strchr.argtypes = [c_char_p, c_char]
libc.strchr(b"abcdef", b"d")        # b'def'
```

ctypes releases the GIL around each call. It rebuilds the libffi
`ffi_cif` on every call
([callproc.c](https://github.com/python/cpython/blob/v3.15.2/Modules/_ctypes/callproc.c)).
hd takes typed declarations and rejects per-call type preparation.

**Rust** ([external blocks](https://doc.rust-lang.org/reference/items/external-blocks.html), 2024 edition):

```rust
unsafe extern "C" {
    fn tcgetattr(fd: c_int, t: *mut termios) -> c_int;  // unsafe to call
    safe fn sqrt(x: f64) -> f64;                          // callable without `unsafe`
}
```

Binding is at link time, with no `dlopen`. The `libc` crate defines
`termios` per target (Linux `c_cc: [cc_t; 32]`, macOS `[cc_t; 20]`;
[docs.rs](https://docs.rs/libc/latest/libc/struct.termios.html)). hd
takes "the declaration is the unsafe part" without the keyword, and
leaves per-target structs to the C library.

**Go** ([cgo](https://pkg.go.dev/cmd/cgo), go1.27):

```go
// static void myprint(char* s) { printf("%s\n", s); }
import "C"
func main() {
	cs := C.CString("hello")   // a malloc'd copy with a NUL
	C.myprint(cs)
	C.free(unsafe.Pointer(cs))
}
```

"C code may not keep a copy of a Go pointer after the call returns." A
goroutine in a C call holds its OS thread, and the scheduler hands its
slot to another thread
([cgocall.go](https://github.com/golang/go/blob/go1.27.4/src/runtime/cgocall.go)).
Without cgo, `x/sys/unix` makes raw syscalls on Linux but calls libc on
macOS. hd takes the pointer rule and the blocking-thread model.

**Swift** ([wrapping a C library](https://www.swift.org/documentation/articles/wrapping-c-cpp-library-in-swift.html)):

```swift
// sqlite3.modulemap: module CSQLite { header "sqlite3.h" }
import CSQLite
guard sqlite3_open_v2(filename, &db, flags, nil) == SQLITE_OK else { return nil }
```

**Zig** ([0.15.2 docs](https://ziglang.org/documentation/0.15.2/)):

```zig
extern "c" fn atan2(a: f64, b: f64) f64;
pub fn main() void { _ = atan2(1.0, 2.0); }
```

Both import C declarations through Clang, so struct layouts come from
the target's headers. Zig removed `@cImport` in 0.17 for a build step
([notes](https://ziglang.org/download/0.17.0/release-notes.html)). hd
rejects header import: it would bring per-OS layouts into hd.

**Wasmtime** ([Func](https://docs.rs/wasmtime/51.0.0/wasmtime/struct.Func.html); hd pins 49.0.2):

```rust
linker.func_wrap("hd:ffi/hd_term", "hd_term_open", || -> i32 {
    unsafe { hd_term_open() }
})?;
```

`Func::wrap` is "both safer and faster" than `func_new`. Memory growth
"may change the base pointer", while a shared memory's base "does not
change". hd uses typed binders and reads the base per call.

**libffi** ([manual](https://github.com/libffi/libffi/blob/v3.8.2/doc/libffi.texi), 3.8.2):

```c
ffi_prep_cif(&cif, FFI_DEFAULT_ABI, 1, &ffi_type_sint, args);  /* once */
ffi_call(&cif, (void (*)(void))puts, &rc, values);             /* per call */
```

`ffi_call` reclassifies every argument on each call (`ffi64.c`); 3.7
added reusable call plans. hd keeps it as phase 2's fallback: it adds a
C library to the build, while Cranelift is already linked.

**Node 26** ([node:ffi](https://nodejs.org/docs/latest-v26.x/api/ffi.html)):

```js
import { dlopen, suffix } from "node:ffi";
using h = dlopen(`./mylib.${suffix}`, {
  add_i32: { arguments: ["int32", "int32"], return: "int32" },
});
h.functions.add_i32(20, 22);
```

Added in v26.1.0, experimental, with a boolean `--allow-ffi`. It has no
blocking-call option. `process.dlopen` loads Node addons only. hd uses a
JavaScript `hd_term` on Node and defers `node:ffi` to phase 2.

**WASI.** [`terminal.wit`](https://github.com/WebAssembly/wasi-cli/blob/v0.2.8/wit/terminal.wit)
(0.2.8) has no raw mode, size or input method, so it cannot serve #2382.

## Appendix C. Alternatives Considered

- **An `extern` block.** New grammar, the costliest change in the
  [Design Cost Order](../../AGENTS.md#design-cost-order). `@foreign`
  parses today and reuses the `@intrinsic` shape.
- **A package host trait for `Terminal`.** Rejected by the owner.
- **Host intrinsics per terminal operation.** #2382's plan: six methods
  each in Rust and TypeScript, about 700 lines, repeated for every OS
  facility.
- **Declaring `termios` in hd per OS.** Rejected by the owner.
- **libffi for every call.** One more C library in the build, and
  argument classification on each call. Typed binders cost less for
  built-in libraries.
- **Copy-in-copy-out buffers for blocking calls.** Correct under cancel,
  but no phase 1 case needs it (stress case 6).
- **A declared cancel function per blocking symbol.** Deno, Go and tokio
  have none; a wakeable library is enough for `hd_term`.
- **`hd_term` as a dylib beside `hd`.** `cargo install` builds no
  dylib, so the user would fetch one per platform.

## Appendix D. Today In hd

- **Package host traits** (built, #51): an SDK's `@host` trait maps in
  `[host]` to a JavaScript provider; the root `[capabilities]` consents
  ([design](../compiler/package-host-capabilities.md)). Kept.
- **Native providers** (built, #2077 to #2083): `hd build --engine
  wasmtime` writes `NAME.host.rs`; a custom embedder registers providers
  (`crates/hd_run_wasmtime/src/native.rs`). Kept.
- **Std intrinsics**: `@intrinsic("name")` on a function with body
  `panic("intrinsic")`, std only. `thread_spawn` already lowers to an
  `hd:rt` import. `@foreign` copies this shape.
- **Raw memory**: `std.rt.memory` has `Ptr[T]`, `Buffer[T]` and
  `string_bytes`, std only
  ([Typed Memory](../../spec/lang/10-modules.md#typed-memory)).
- **Blocking work**: `Http.send!` and DNS run in `spawn_blocking` on
  `hd-io`; the reactor delivers each `Done`
  (`crates/hd_run_wasmtime/src/runtime/reactor.rs`).
- **Memory**: Wasmtime 49.0.2 with `shared_memory(true)`; a program that
  starts threads imports the shared heap `hd:rt` `heap`.
- **Host calls in TIR**: `CallHost` appears only in std provider bodies
  that `hd_wasm` generates (`crates/hd_tir/DESIGN.md` §8, check 9).
  `CallForeign` is a real TIR call, so any backend lowers it.

Rules this design revises, quoted:

| Rule | Today | Proposed |
| --- | --- | --- |
| [Host Boundary note](../../spec/lang/10-modules.md#host-boundary) | "hd has no `extern` declaration and no FFI capability. A Wasm module can call only its imports, so a custom host trait that an embedder binds is hd's foreign function interface" | "A Wasm module calls only its imports. A foreign function is one, which the host resolves to its C symbol; a custom host trait that an embedder binds is the other interface." |
| [`cli.host.package.native-no-discovery`](../../spec/cli/command-line.md#r-cli.host.package.native-no-discovery) | "An implementation never discovers or loads a dynamic library for a package host trait." | Phase 1: unchanged, since std libraries are built in. Phase 2: "An implementation loads a dynamic library only for a foreign library that a package declares and the root `Ffi` grant names." |
| [`cli.host.package.native-cli`](../../spec/cli/command-line.md#r-cli.host.package.native-cli) | "The `hd` command registers no native provider, so on Wasmtime it refuses every module requiring a package host trait" | Unchanged: it concerns package host traits. Foreign functions are not providers. |
| [`cli.host.package.native-cli-node`](../../spec/cli/command-line.md#r-cli.host.package.native-cli-node) | "Without an `--engine` flag, `hd run` and `hd FILE.wasm` run a module requiring a package host trait on Node" | Unchanged. Phase 2 adds the mirror: a module needing a package library runs on Wasmtime; needing both refuses. |

## Appendix E. Details

### Proposed Spec Rules

Language tier, a new subsection of Host Boundary in
`spec/lang/10-modules.md`:

#### Foreign Functions

A **foreign function** is a function of a C library that hd code
declares and calls:

```text
@foreign("hd_term")
fn hd_term_isatty(stream: i32) -> i32:
    panic("foreign")
```

1. r[module.ffi.decl] `@foreign("LIBRARY")` before a module-level function declares a foreign function of LIBRARY, whose C symbol is the function's name.
2. r[module.ffi.body] Its body is `panic("foreign")`, and a call never runs it.
3. r[module.ffi.std-only] Only the standard library sees the name `foreign`. In any other package, `@foreign` names no binding. Error: `unknown-name`.
4. r[module.ffi.private] A foreign function is not `pub`. Error: `invalid-foreign-function`.
5. r[module.ffi.shape] A foreign function has no type parameter, no requirement row and no default argument. Error: `invalid-foreign-function`.
6. r[module.ffi.types] Each parameter type and the result type is a row of the foreign type table. Error: `invalid-foreign-function`.
7. r[module.ffi.call] A call runs the C function with the arguments the table converts, and returns its converted result.
8. r[module.ffi.reference] A reference argument passes the address of its referent, or of a list's first element. The address is valid until the call returns.
9. r[module.ffi.blocking] A foreign function whose name ends in `!` is **blocking**: a call suspends its task while the C function runs on another thread.
10. r[module.ffi.blocking.scalars] A blocking foreign function has no reference or `string` parameter. Error: `invalid-foreign-function`.
11. r[module.ffi.blocking.cancel] Cancelling a blocking call does not stop the C function. Its result is dropped when it returns.
12. r[module.ffi.usize-result] A `usize` result outside the type's range stops the call. Panic: `host-contract`.

```text
@foreign("hd_term")
pub fn hd_term_open() -> i32:  # error: invalid-foreign-function
    panic("foreign")

@foreign("hd_term")
fn hd_term_read!(term: i32, into: &mut List[u8], cap: usize) -> i64:  # error: invalid-foreign-function
    panic("foreign")
```

> **Why.** A wrapper module is the safe API, so the declaration stays
> private. A blocking call outlives its frame when cancelled, so it may
> hold no pointer into the instance.

Language tier, Runtime Profiles:

8. r[module.profile.std-provider] A profile may bind a standard trait to a provider that the standard library writes in hd.

CLI tier, `spec/cli/command-line.md`, a new Foreign Libraries section:

1. r[cli.ffi.preflight] Before guest initialization, the host resolves every foreign function the module reaches.
2. r[cli.ffi.unavailable] A missing library or symbol, or a signature that differs from the library's, refuses the run and names both. Error: `foreign-unavailable`.
3. r[cli.ffi.std] `hd` supplies each library the standard library declares, on every engine, and opens no file for it.
4. r[cli.ffi.javascript] On a JavaScript host, a std library's symbols are JavaScript functions with the same results.
5. r[cli.ffi.needs] A std library's imports count as needs of the std trait whose provider declares them, for [Total Deny](../../spec/cli/command-line.md#total-deny).
6. r[cli.host.terminal] The default profile binds `Terminal` (`std.terminal`) to the standard library's provider, and `Terminal` is a boolean capability key.

The `std-terminal.*` rules themselves are #2382's.

### The Terminal, End To End

`hd_term`'s flat API. A terminal number hides the OS handle, so Windows
(`CONIN$`, `SetConsoleMode`) has the same seven functions:

```c
int32_t hd_term_isatty(int32_t stream);              /* 0 stdin, 1 stdout, 2 stderr: 1 or 0 */
int32_t hd_term_size(int32_t stream, uint16_t *cols, uint16_t *rows);  /* 0, or -errno */
int32_t hd_term_open(void);              /* stdin if a terminal, else /dev/tty: a number >= 0, or -errno */
int32_t hd_term_enable_raw(int32_t term);   /* saves the mode once, registers atexit restore */
int32_t hd_term_disable_raw(int32_t term);  /* restores the saved mode: 0, or -errno */
int32_t hd_term_wait(int32_t term);         /* blocks: 1 ready, 0 woken by a later wait, -errno */
int64_t hd_term_read(int32_t term, uint8_t *buf, size_t cap);  /* never blocks: n, 0 none, -errno */
```

`std.terminal` declares five of them; the hosts' `Console` uses
`hd_term_isatty` and `hd_term_size`.

| Command | What happens |
| --- | --- |
| `hd run` | Wasmtime binds the five `hd:ffi/hd_term` imports from the built-in table; nothing is opened from disk |
| `hd build --release` | the `.cwasm` holds the same imports; no companion file |
| `hd install` | the same artifact and launcher; `hd_term` is inside `hd`, so nothing else is installed |
| `hd run --engine node` | the JavaScript `hd_term` |
| `hd test` | `$.with(Terminal=ScriptedTerminal::new(chunks))`; no foreign call runs |

The fake, for tests (checked with `hd check --tests` against stub
declarations):

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

- **Grammar and formatting:** none new; `@foreign` is a decorator line.
- **Typing:** the type table only; use-site inference unchanged.
- **Ownership:** reference arguments are ordinary call loans.
- **Inliner:** a `CallForeign` is opaque; its wrapper inlines as usual.
- **Soundness:** C is trusted; std only in phase 1 (stress cases 9, 10).
- **Diagnostics:** `invalid-foreign-function`, `foreign-unavailable`.
- **Pipeline:** after checking, before ownership; no new pass.
- **Determinism:** `hd.foreign` lists symbols in first-reach order.
- **Host parity:** a conformance case per `hd_term` symbol on Node and
  Wasmtime under a pseudo-terminal; the web returns `-ENOTTY`.
- **Suspension:** blocking calls reuse `.start`/`.finish` and the reactor.
- **Failure and cleanup:** `atexit` restore; Node's exit hook.
- **Grants:** `Terminal = true`; phase 2 `Ffi` scope entries.
- **Rollback:** delete `std.terminal` and the decorator; nothing else
  depends on them.

### Phases

| # | Step | Test |
| --- | --- | --- |
| 1 | checker: `@foreign`, std-only name, type table, `!` rule | diagnostics stage cases for each error |
| 2 | TIR `CallForeign` and verifier | Rust test: a foreign call's loans end at the call |
| 3 | Wasm lowering: imports and `hd.foreign` | `functions`-view stage case lists the imports |
| 4 | `hd_term` crate, Unix | Rust test under a pseudo-terminal: raw, read, restore |
| 5 | Wasmtime binder and preflight | refusal case for a missing symbol; a read through the binder |
| 6 | blocking calls on `hd-io` | Rust test: 1000 cancelled waits leave one blocked thread |
| 7 | `std.terminal` and the profile binding | `ScriptedTerminal` conformance cases |
| 8 | Node and web `hd_term` | `--host` cases for `is_terminal` and `size` under a pipe |
| 9 | `hd_term` on Windows | the step 4 test on Windows CI |
| 10 | example pager in `examples/` | run by hand, as #2382's done check says |

### Parse Log

Each `text` block was parsed with `scripts/hd-conformance.sh parse`,
which checks syntax only, and run through `hd check --tests`. `hd check`
reports only names that do not exist yet: `foreign`, `std.terminal`,
`Terminal` and `ConsoleError` in a block without imports. The
`NativeTerminal` and `ScriptedTerminal` logic was also checked in one
file against stub declarations without `@foreign`: no diagnostics.

| Block | Parser | `hd check` |
| --- | --- | --- |
| The Proposal, `std.terminal` excerpt | parses | `unknown-name` (`foreign`) lines 2 and 6; `unknown-trait` line 13 |
| How It Works, a program using `Terminal` | parses | `unknown-module` (`std.terminal`) line 1 |
| Foreign Functions, definition | parses | `unknown-name` line 1 |
| Foreign Functions, error examples | parses | `unknown-name` lines 1 and 5 |
| The Terminal, `ScriptedTerminal` | parses | `unknown-trait` (`Terminal`, `ConsoleError`) lines 8, 14, 15, 22 |
| Question 1 example | parses | `unknown-name` line 2 |

## Questions For The Owner

1. **Declaration form.** Effect: whether FFI adds grammar.
   (a) `@foreign("hd_term")` on a function with body `panic("foreign")`;
   (b) an `extern "hd_term":` block of body-less declarations.
   **Recommendation: (a)**, a compiler intrinsic with no syntax change.
2. **Who declares.** Effect: whether a package can load C in phase 1.
   (a) std only, as `@intrinsic`; (b) packages too, under a grant.
   **Recommendation: (a)** until a package needs it.
3. **Blocking calls.** Effect: whether `read!` can wait without stalling.
   (a) a `!` foreign function runs on a host thread and takes scalars
   only; (b) a `nonblocking` decorator argument.
   **Recommendation: (a)**, reusing `!`.
4. **Cancelling a blocking call.** Effect: what `race!` does to C.
   (a) C runs to completion and the result is dropped; (b) a declared
   cancel symbol the host calls. **Recommendation: (a)**, as Deno and Go.
5. **Shipping `hd_term`.** Effect: the user's toolchain needs.
   (a) linked into `hd`, and also published as a dylib for embedders;
   (b) a dylib in release archives; (c) built on first use.
   **Recommendation: (a)**: `cargo install` already builds it.
6. **Console's queries.** Effect: whether two host methods are added.
   (a) the hosts implement `is_terminal` and `size`, Wasmtime through
   `hd_term`; (b) the profile's `Console` becomes std code.
   **Recommendation: (a)**: two read-only methods, no device state.
7. **Phase 2 grant.** Effect: how an app consents to a package library.
   (a) `Ffi = ["dep.sqlite_hd.sqlite3"]` in `[capabilities]`;
   (b) a separate `[ffi]` table. **Recommendation: (a)**, one grant table.
8. **Package FFI on Node.** Effect: whether such a module runs on Node.
   (a) refuse, and pick Wasmtime with no `--engine`; (b) use the
   experimental `node:ffi`. **Recommendation: (a)** until it is stable
   and can block.
9. **Raw mode under an embedder.** Effect: a trapped instance in a living
   process leaves the terminal raw. (a) restore at process exit only;
   (b) the host calls a library close hook at instance close.
   **Recommendation: (a)** for now; `hd` always exits.

```text
# question 1, option (a)
@foreign("hd_term")
fn hd_term_open() -> i32:
    panic("foreign")
```
