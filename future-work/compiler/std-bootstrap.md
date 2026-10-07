# New Compiler: Standard-Library Bootstrap Inventory

| Field | Value |
| --- | --- |
| Status | Research inventory only; nothing in this file is accepted behavior beyond the cited spec and design records. |
| Scope | What M3 needs to make `lib/std` interfaces, bodies and the prelude available to programs. |
| Sources | [`lib/std`](../../lib/std), [Prelude](../../spec/lang/10-modules.md#prelude), [Standard Library Primitives](../../spec/std/README.md#standard-library-primitives), [Intrinsic Methods](../../spec/lang/09-traits.md#intrinsic-methods), [`hd_host_abi`](../../compiler/crates/hd_host_abi/src/lib.rs) |
| Inventory date | 2026-10-07, at M1 `07c74892` |

## Prelude

| Origin | Names every ordinary module sees | Source status | Bootstrap consequence |
| --- | --- | --- | --- |
| `std.core` | `never`, `bool`, `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `usize`, `f32`, `f64`, `char`, `string`, `void`, `List`, `Map`, `Any`, `AnyVal`, `AnyRef`, `Option`, `Result`, `panic` | No `lib/std/core.hd`; the compiler supplies the module. `Option` and `Result` also have ordinary methods in `option.hd` and `result.hd`. | Seed this interface before parsing any fixed prelude use. |
| `std.format` | `Display`, `Debug`, `debug`, `dbg` | Declared in `format.hd`; `dbg` has an intrinsic body. | Load after compiler-owned core types and before modules that import `DebugWriter`. |
| `std.cmp` | `Eq`, `PartialOrd`, `Ord`, `Ordering` | Declared in `cmp.hd`; primitive implementations use intrinsic methods. | Register numeric-family and primitive comparison heads while building its interface. |
| `std.hash` | `Hash`, `Hasher` | Declared in `hash.hd`; `char_scalar` is a private primitive. | Its `Structure` templates require `std.structure`. |
| `std.iter` | `Iterator`, `Iterable` | Declared in `iter.hd`. | Built-in string, list and map methods refer to these types. |
| `std.console` | `Console`, `ConsoleError`, `println` | Declared in `console.hd`; `println` is ordinary hd over `Console` and `block_on`. | The generic declaration supersedes `PRELUDE_IMPORTS`' temporary `println(i32)` shim. |
| `std.task` | `Suspend`, `Poll`, `PollContext`, `Waker` | No declarations for these four in `task.hd`; the compiler supplies them. | Seed the protocol interface before checking `console.hd`, `task.hd`, or any suspending signature. |
| `std.testing` | `it`, test code only | `it` is compiler-provided; `testing.hd` declares the remaining testing API. | Add this fixed use only to test overlays and test modules. |

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
| `std` (`lib/std`) | 34 modules: `annotation`, `cli`, `cmp`, `collections`, `console`, `convert`, `digest`, `encoding`, `error`, `format`, `fs`, `function`, `hash`, `host`, `http`, `inspect`, `iter`, `json`, `num`, `ops`, `option`, `path`, `prelude`, `process`, `random`, `regex`, `resource`, `result`, `serde`, `structure`, `task`, `testing`, `text`, `time` | `std.testing`, through `std.testing`'s re-export of `std.testing.arbitrary` | Member of `std -> std.testing -> std` |
| `std.prelude` (`lib/std/prelude`) | `testing` | `std`, through `use std.testing.it` | Acyclic leaf into the root folder |
| `std.testing` (`lib/std/testing`) | `arbitrary` | `std`, through `std.annotation.annotate` and `std.testing.Choices` | Member of `std -> std.testing -> std` |

| Cycle | Edge evidence | M3 effect |
| --- | --- | --- |
| `std -> std.testing -> std` | `testing.hd` has `pub use std.testing.arbitrary.{With, with}`; `testing/arbitrary.hd` uses `std.annotation.annotate` and `std.testing.Choices`. | The designed folder graph rejects it as `folder-cycle`; std interfaces cannot meet build-order slice 2's no-diagnostic exit until this source layout or the folder-cycle rule changes. |

## M3 Dependency Order

| Order | Piece | Depends on | What it unblocks |
| --- | --- | --- | --- |
| 1 | Resolve the `std`/`std.testing` folder cycle | Owner choice about source layout or the folder-cycle rule | Any successful `FolderIface` order for the current std tree. |
| 2 | Seed compiler-owned `std.core` and the `std.task` protocol (`Suspend`, `Poll`, `PollContext`, `Waker`, `block_on`, `all!`) | Core type and suspension descriptors in the toolchain | Parsing and resolving every prelude import and most std signatures. |
| 3 | Build `std.annotation`, `std.structure`, qualified fact markers, and template heads | Step 2; interface construction and stable compiler-known paths | `std.ops`, `std.testing.arbitrary`, all `by Structure` templates, and fact-bearing literal functions. |
| 4 | Register every primitive function and intrinsic-method signature above | Steps 2 and 3; one intrinsic ID table consumed by checking and emission | Body checking of `cmp`, `ops`, `text`, `format`, `num`, `collections`, `task`, and `annotation`. |
| 5 | Complete capability `TABLE` rows and define the host-primitive import registry | Steps 2 and 4; codecs for structured boundary types | `fs`, `random`, `http`, `process`, Unicode case mapping, and float formatting/parsing. |
| 6 | Check derive instances, facts and ordinary std bodies, then emit their reachable instances | Steps 3 through 5 | A complete std pack rather than header-only interfaces. |
| 7 | Install fixed prelude uses from the pack and retire the `println_i32` shim | Step 6; pack hash in the toolchain key | Ordinary programs see exactly the specified prelude and reach std implementations. |
