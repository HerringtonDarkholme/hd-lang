# New Compiler: Standard-Library Bootstrap Inventory

| Field | Value |
| --- | --- |
| Status | Research inventory only; nothing in this file is accepted behavior beyond the cited spec and design records. |
| Scope | What M3 implemented to make `lib/std` interfaces and the prelude available to programs, plus the body and pack work still ahead. |
| Sources | [`lib/std`](../../lib/std), [Prelude](../../spec/lang/10-modules.md#prelude), [Standard Library Primitives](../../spec/std/README.md#standard-library-primitives), [Intrinsic Methods](../../spec/lang/09-traits.md#intrinsic-methods), [`hd_host_abi`](../../compiler/crates/hd_host_abi/src/lib.rs) |
| Inventory date | 2026-10-07, after M3 `20ea6342` |

## Prelude

| Origin | Names every ordinary module sees | Source status | Bootstrap consequence |
| --- | --- | --- | --- |
| `std.core` | `never`, `bool`, `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `usize`, `f32`, `f64`, `char`, `string`, `void`, `List`, `Map`, `Any`, `AnyVal`, `AnyRef`, `Option`, `Result`, `panic` | No `lib/std/core.hd`; `hd_driver` inserts a virtual `core.hd`, and `hd_resolve::seed` supplies its declarations. `Option` and `Result` also have ordinary methods in `option.hd` and `result.hd`. | Discover the virtual module with the ordinary std sources, then seed its interface before resolving fixed prelude uses. |
| `std.format` | `Display`, `Debug`, `debug`, `dbg` | Declared in `format.hd`; `dbg` has an intrinsic body. | Load after compiler-owned core types and before modules that import `DebugWriter`. |
| `std.cmp` | `Eq`, `PartialOrd`, `Ord`, `Ordering` | Declared in `cmp.hd`; primitive implementations use intrinsic methods. | Register numeric-family and primitive comparison heads while building its interface. |
| `std.hash` | `Hash`, `Hasher` | Declared in `hash.hd`; `char_scalar` is a private primitive. | Its `Structure` templates require `std.structure`. |
| `std.iter` | `Iterator`, `Iterable` | Declared in `iter.hd`. | Built-in string, list and map methods refer to these types. |
| `std.console` | `Console`, `ConsoleError`, `println` | Declared in `console.hd`; `println` is ordinary hd over `Console` and `block_on`. | The generic declaration supersedes `PRELUDE_IMPORTS`' temporary `println(i32)` shim. |
| `std.task` | `Suspend`, `Poll`, `PollContext`, `Waker` | No declarations for these four in `task.hd`; the compiler supplies them. | Seed the protocol interface before checking `console.hd`, `task.hd`, or any suspending signature. |
| `std.testing` | `it`, test code only | `it` is compiler-provided; `testing.hd` declares the remaining testing API. | Add this fixed use only to test overlays and test modules. |

### Compiler-Supplied Names Outside The Prelude

| Module | Supplied names | Why source alone is insufficient |
| --- | --- | --- |
| `std.function` | `Fn`, `SuspendFn` | Function-type syntax lowers to these compiler-known constructors. `Tuple` remains an ordinary declaration in `function.hd`. |
| `std.inspect` | `downcast_val` | Runtime downcasting needs an intrinsic body, while `Inspectable` and `TypeId` remain ordinary declarations. |
| `std.structure` | `Structure` and its compiler-known members | Derivation templates receive this sealed structural view from the compiler. The supporting protocols remain ordinary declarations. |

**M3 gap 1.** These seeded declarations are ordinary interface items of
their named modules. A source declaration with the same stable path wins,
and only `std.core` needs a virtual source module for discovery.

| Fixed but non-exporting prelude use | Why the compiler needs it |
| --- | --- |
| `std.convert.From` | Postfix `?` resolves the standard conversion without a user import. |
| `std.ops.{Add, Sub, Mul, Div, Rem, Neg, Not, BitAnd, BitOr, BitXor, Shl, Shr, Index, IndexSet, Apply, Update, Range, RangeFrom, RangeTo, RangeFull}` | Operator, indexing, callable-value and range lowering resolve these qualified declarations. |
| `std.text`, `std.option`, `std.result`, `std.collections`, `std.num` | Built-in inherent methods and ordinary implementations live in these modules. |

## Primitive Functions

| Intrinsic key | Declaration signature | `lib/std` declarations | Design owner | In `hd_host_abi::TABLE` |
| --- | --- | --- | --- | --- |
| `bytes_len` | `(text: string) -> usize` | `text.hd` | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives), [Wasm values §15.2](wasm-layout.md#152-values) | No |
| `bytes_at` | `(text: string, index: usize) -> u8` | `text.hd` | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives), [Wasm values §15.2](wasm-layout.md#152-values) | No |
| `bytes_slice` | `(text: string, start: usize, end: usize) -> string` | `text.hd` | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives), [Wasm values §15.2](wasm-layout.md#152-values) | No |
| `bytes_concat` | `(left: string, right: string) -> string` | `text.hd` | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `string_from_bytes` | `(bytes: List[u8]) -> string` | `text.hd`, `format.hd` | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives), [Wasm values §15.2](wasm-layout.md#152-values) | No |
| `char_scalar` | `(value: char) -> u32` | `text.hd`, `format.hd`, `hash.hd` | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives), [Wasm values §15.2](wasm-layout.md#152-values) | No |
| `char_from_scalar` | `(point: u32) -> char?` | `text.hd` | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives), [Wasm values §15.2](wasm-layout.md#152-values) | No |
| `list_truncate` | `[T](items: mut List[T], len: usize) -> void` | `collections.hd` | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `panic` | `(category: string, message: string) -> void` | `collections.hd`, `ops.hd`, `testing.hd`, `text.hd` | [Panic lowering §12.2](codegen.md#122-lowering-rules), [Panics §16.2](runtime-and-host.md#162-panics-and-exit-codes) | No |
| `facts_of` | `[F](f: F) -> dyn Any` | `annotation.hd` | [Checker §4.13.9](checking-and-tir.md#4139-derive-instances-templates-facts-and-test-overlays), [Codegen §12.3](codegen.md#123-facts-defaults-derives-and-tests) | No |
| `task_race_frame` | `[T](tasks: List[mut Suspend[T]]) -> mut Suspend[T]` | `task.hd` | [Suspension §14.5](suspension.md#145-all-and-race), [Cancellation §14.6](suspension.md#146-cancellation-and-defer) | No |
| `task_all_frame` | `[T](tasks: List[mut Suspend[T]]) -> mut Suspend[List[T]]` | `task.hd` | [Suspension §14.5](suspension.md#145-all-and-race), [Cancellation §14.6](suspension.md#146-cancellation-and-defer) | No |
| `format_f64` | `(value: f64) -> string` | `format.hd` | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives); host primitive | No |
| `format_f32` | `(value: f32) -> string` | `format.hd` | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives); host primitive | No |
| `string_lower` | `(text: string) -> string` | `text.hd` | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives); host primitive | No |
| `string_upper` | `(text: string) -> string` | `text.hd` | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives); host primitive | No |
| `parse_f64` | `(text: string) -> f64` | `num.hd` | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives); host primitive | No |
| `format_f64_fixed` | `(value: f64, digits: usize) -> string` | `num.hd` | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives); host primitive | No |
| `dbg` | `[Args < Tuple](values...: Args) -> void` | `format.hd` | [TIR `Intrinsic`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `dbg_text` | `[T](value: T, width: usize) -> string` | `format.hd` | [TIR `Intrinsic`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `dbg_write` | `(line: string) -> void` | `format.hd` | [TIR `Intrinsic`](checking-and-tir.md#instruction-catalog), [Runtime imports §16.4](runtime-and-host.md#164-metadata-and-the-import-list) | No |

## Intrinsic And Equivalent Methods

| Compiler-supplied member | Signature family written or required by std | Declared in | Design owner | In `hd_host_abi::TABLE` |
| --- | --- | --- | --- | --- |
| `Add.add` | `[N < Num](self: N, rhs: N) -> N` | `ops.hd` | [TIR `Prim`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `Sub.sub` | `[N < Num](self: N, rhs: N) -> N` | `ops.hd` | [TIR `Prim`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `Mul.mul` | `[N < Num](self: N, rhs: N) -> N` | `ops.hd` | [TIR `Prim`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `Div.div` | `[N < Num](self: N, rhs: N) -> N` | `ops.hd` | [TIR `Prim`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `Rem.rem` | `[N < Num](self: N, rhs: N) -> N` | `ops.hd` | [TIR `Prim`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `Neg.neg` | `(self: T) -> T`, signed integers and floats | `ops.hd` | [TIR `Prim`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `BitAnd.bit_and` | `[N < Integer](self: N, rhs: N) -> N` | `ops.hd` | [TIR `Prim`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `BitOr.bit_or` | `[N < Integer](self: N, rhs: N) -> N` | `ops.hd` | [TIR `Prim`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `BitXor.bit_xor` | `[N < Integer](self: N, rhs: N) -> N` | `ops.hd` | [TIR `Prim`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `Not.not` | `[N < Integer](self: N) -> N` | `ops.hd` | [TIR `Prim`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `Shl.shl` | `[N < Integer, C unsigned](self: N, rhs: C) -> N` | `ops.hd`, one impl per unsigned count type | [TIR `Prim`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `Shr.shr` | `[N < Integer, C unsigned](self: N, rhs: C) -> N` | `ops.hd`, one impl per unsigned count type | [TIR `Prim`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `Eq.eq` | `(self: T, other: T) -> bool`, numbers, `char`, `bool` | `cmp.hd` | [TIR `Prim`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `PartialOrd.partial_cmp` | `(self: T, other: T) -> Ordering?`, numbers and `char` | `cmp.hd` | [TIR `Prim`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `Ord.cmp` | `(self: T, other: T) -> Ordering`, integers and `char` | `cmp.hd` | [TIR `Prim`](checking-and-tir.md#instruction-catalog), [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| Built-in `Index.index` | `List[T] × usize -> T`; `Map[K,V] × K -> V`; `string × usize -> u8`; string/list range families | `ops.hd`, ordinary-looking recursive bodies whose behavior is intrinsic | [Lowering §12.2](codegen.md#122-lowering-rules), [Panics §16.2](runtime-and-host.md#162-panics-and-exit-codes) | No |
| Built-in `IndexSet.index_set` | `mut List[T] × usize × T -> void`; `mut Map[K,V] × K × V -> void` | `ops.hd`, ordinary-looking recursive bodies whose behavior is intrinsic | [Lowering §12.2](codegen.md#122-lowering-rules), [Panics §16.2](runtime-and-host.md#162-panics-and-exit-codes) | No |
| Built-in string methods | `len`, `chars`, `char_indices`, `bytes`, `slice` | Compiler-supplied surface used by `text.hd` | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives), [Wasm values §15.2](wasm-layout.md#152-values) | No |
| Built-in `List` methods | `len`, `iter`, `push`, `pop`, `insert`, `remove_at`, `clear` | Compiler-supplied surface used by `collections.hd` | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives), [Wasm values §15.2](wasm-layout.md#152-values) | No |
| Built-in `Map` methods | `len`, `get`, `remove` | Compiler-supplied surface used throughout std | [Runtime §16.1](runtime-and-host.md#161-where-each-piece-lives), [Wasm values §15.2](wasm-layout.md#152-values) | No |
| Built-in `Display.to_string` | `(self: T < Display) -> string` | Compiler-supplied method over `std.format.Display` | [Lowering §12.2](codegen.md#122-lowering-rules) | No |
| `std.core.panic` | `(message: string) -> never` | Compiler-supplied `std.core`; distinct from the private two-argument `panic` primitive | [Panic lowering §12.2](codegen.md#122-lowering-rules), [Panics §16.2](runtime-and-host.md#162-panics-and-exit-codes) | No |
| `std.task.block_on` | `[T](s: mut Suspend[T]) -> T` | Compiler-supplied name used by `console.hd` | [Suspension §14.9](suspension.md#149-blockon) | No |
| `std.task.all!` | `(mut Suspend[X_1], ..., mut Suspend[X_n]) -> (X_1, ..., X_n)`; no written signature | Compiler-supplied name used by `task.hd` | [Suspension §14.5](suspension.md#145-all-and-race) | No |
| `std.task.race!` | `[T](tasks...: List[mut Suspend[T]]) -> T` | `task.hd`, backed by `task_race_frame` | [Suspension §14.5](suspension.md#145-all-and-race) | No |

## ABI Coverage

| `hd_host_abi` surface | Current contents | Consequence for std bootstrap |
| --- | --- | --- |
| `TABLE` | Capability traits only: `Console`, `ConsoleInput`, `Clock`, `FsRead`, `FsWrite`, `Random`, `Http`, `Process`, `Env`, `Args`. | It is not an intrinsic registry; every primitive and intrinsic-method row above is absent. |
| `PRELUDE_IMPORTS` | One temporary `println: (i32) -> void` import, `hd.println_i32`. | It does not match `std.console.println[T < Display](T) -> void $ Console`; remove the shim from the program path once std reaches programs. |
| `RUNTIME_IMPORTS` | `hd:rt.block`, `hd:rt.abort`, `hd:rt.stderr`. | Covers driver/runtime operations, not std primitive names. |

| Capability trait | Methods in `lib/std` | Methods currently in `TABLE` | Coverage |
| --- | --- | --- | --- |
| `Console` | `write_line!`; ordinary default `write_error_line!` | `write_line` | Host-required method complete |
| `ConsoleInput` | `read_line!` | `read_line` | Complete |
| `Clock` | `now`, `monotonic`, `sleep!` | `now`, `monotonic`, `sleep` | Complete, with scalar codecs for the three newtypes |
| `FsRead` | `read_bytes!`, `read_text!`, `list_dir!`, `stat!` | all four | Complete |
| `FsWrite` | `write_bytes!`, `write_text!`, `append_text!`, `create_dir_all!`, `remove!`, `rename!` | none | Missing six host methods |
| `Random` | `next_u64`, `fill` | none | Missing two host methods |
| `Http` | `send!` | none | Missing one host method |
| `Process` | `run!` | none | Missing one host method |
| `Env` | `get`, `names` | `get`, `names` | Complete |
| `Args` | `program`, `list` | `program`, `list` | Complete |

## Annotations And Derivation

| Syntax used by `lib/std` | Declarations or targets | Design owner | Bootstrap work |
| --- | --- | --- | --- |
| `@annotate::[F](.Fn)` | `std.ops.NumSuffix[F]`, `std.ops.StrPrefix[F]` | [Checker §4.13.9](checking-and-tir.md#4139-derive-instances-templates-facts-and-test-overlays), [Codegen §12.3](codegen.md#123-facts-defaults-derives-and-tests) | Build typed fact heads and validate the target-type pattern while constructing interfaces. |
| `@annotate(.Variant)` | `std.ops.DefaultVariant` | Same | Bootstrap `std.annotation.Annotate` first, then attach this target limit. |
| `@annotate::[F](.Field)` | `std.testing.arbitrary.With[F]` | Same | Needs the child folder's interface and typed member-fact checking. |
| `@annotate(.Data, .Enum)` | `std.annotation.Annotate` itself | Same | Self-bootstrap rule from the language spec; no prior ordinary `annotate` value can supply it. |
| `@str_prefix` | `std.text.r(Template[dyn Display]) -> string` | [Resolution §4.9](resolution-and-interfaces.md#49-name-resolution), [Expression checking §2.2](type-checking.md#22-expression-forms) | Resolve the qualified `std.ops.str_prefix` marker and validate the function signature. |
| `@num_suffix` | `std.time.ms`, `s`, `min`, `h`, each `(i64) -> Duration` | [Resolution §4.9](resolution-and-interfaces.md#49-name-resolution), [Expression checking §2.2](type-checking.md#22-expression-forms) | Resolve the qualified `std.ops.num_suffix` marker and validate each function signature. |
| `@derive(...)` | No std declaration uses it; public std types hand-write their `Debug` builder calls. | [Checker §4.13.9](checking-and-tir.md#4139-derive-instances-templates-facts-and-test-overlays) | No std self-derive is required to bootstrap the pack. |
| `@error`, `@from`, `@source` | No std declaration uses them; std error types write ordinary implementations. | [Solver §3.10](trait-solver.md#310-derives-delegation-and-error) | No error-derivation bootstrap is required for std itself. |

| `by Structure` template | Location | Instance family | Design owner |
| --- | --- | --- | --- |
| `Arbitrary` | `testing.hd` | data and enum values | [Solver §3.10](trait-solver.md#310-derives-delegation-and-error), [Checker §4.13.9](checking-and-tir.md#4139-derive-instances-templates-facts-and-test-overlays) |
| `Hash` | `hash.hd` | data and enum values; tuples | Same |
| `Default` | `ops.hd` | tuples; data and enum values | Same |
| `Eq`, `PartialOrd`, `Ord` | `cmp.hd` | data and enum values; tuples | Same |
| `Serialize`, `Deserialize` | `serde.hd` | data and enum values | Same |
| `Debug` | `format.hd` | data and enum values; tuples | Same |
| `Display` | `format.hd` | tuples | Same |

## Folder Graph

| Folder | Modules | Outgoing folder edges | Cycle status |
| --- | --- | --- | --- |
| `std` (`lib/std`) | 33 modules: the virtual `core`, plus the 32 root files other than `prelude.hd` and `testing.hd` | none outside itself | Acyclic root |
| `std.testing` (`lib/std/testing`) | `std.testing` from `testing.hd`, plus `std.testing.arbitrary` | `std`, through ordinary imports and the fixed prelude uses | Acyclic; the parent and child modules share this folder |
| `std.prelude` (`lib/std/prelude`) | `std.prelude` from `prelude.hd`, plus `std.prelude.testing` | `std`, through the fixed prelude origins; `std.testing`, through the test-only re-export | Acyclic leaf after both dependencies |

**M3 gap 1.** The former inventory's `std -> std.testing -> std` cycle was
wrong. Under [`module.folder.parent-file`](../../spec/lang/10-modules.md#r-module.folder.parent-file),
`testing.hd` and `testing/arbitrary.hd` share folder `std.testing`, so the
re-export between them creates no folder edge.

**M3 gap 3.** Every ordinary prelude origin is a module in folder `std`,
so the fixed uses intentionally give every other folder a dependency on
`std`. Folder `std.prelude` also depends on `std.testing` because its child
test module re-exports `it`; that extra edge is intended and remains
acyclic.

## M3 Dependency Order

| Order | Piece | Depends on | What it unblocks |
| --- | --- | --- | --- |
| 1 | Discover the virtual `std.core` module and seed all compiler-supplied declarations listed above | Core type, function, suspension, inspection and structure descriptors in the toolchain | Resolving every fixed prelude use and compiler-known std path. |
| 2 | Build folder interfaces in order: `std`, `std.testing`, `std.prelude` | Step 1 and the parent-file folder rule | The slice-2 no-diagnostic exit and canonical interface blobs. |
| 3 | Install fixed prelude uses from those interfaces | Step 2 | Ordinary programs see exactly the specified prelude and std types reach their signatures. |
| 4 | Build typed facts, derived heads and template bodies | Step 3; stable compiler-known paths | `by Structure`, annotations and fact-bearing literal functions. |
| 5 | Register every primitive function and intrinsic-method signature above | Steps 3 and 4; one intrinsic ID table consumed by checking and emission | Body checking of `cmp`, `ops`, `text`, `format`, `num`, `collections`, `task`, and `annotation`. |
| 6 | Complete capability `TABLE` rows and define the host-primitive import registry | Step 5; codecs for structured boundary types | `fs`, `random`, `http`, `process`, Unicode case mapping, and float formatting/parsing. |
| 7 | Check ordinary std bodies and emit their reachable instances into the std pack | Steps 4 through 6 | A reusable std pack rather than source-backed header interfaces. |
