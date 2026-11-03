# Standard-Library Bootstrap

Status: describes the code at 3209d690; the code and the spec win.

How compiler-supplied names and `lib/std` reach programs, and what the std
pack would hold. `hd_driver::std_sources` embeds `lib/std` at build time and
adds one virtual module, `rt/entry/mod.hd` (`hd_driver::RT_SOURCE`). Every run checks
std from these sources, with the cache in front.

`hd_stdpack::build_pack` runs the driver over std and refuses until every
stage through `ModuleFinish` passes. Planned, not built: the pack writer
(std's folder interface blobs, by folder, embedded in `hd_driver` in place
of source-backed std interfaces).

## Prelude

`lib/std/prelude.hd` is the prelude as fixed `use` lines;
`lib/std/prelude/testing.hd` adds `it` to test code only.

| Origin | Names every ordinary module sees | Source |
| --- | --- | --- |
| `std.core` | `never`, `bool`, `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `usize`, `f32`, `f64`, `char`, `string`, `void`, `List`, `Map`, `Any`, `AnyVal`, `AnyRef`, `Option`, `Result`, `panic` | `hd_resolve::seed`, in `core/mod.hd`; `Option` and `Result` also have methods in `option.hd` and `result.hd` |
| `std.format` | `Display`, `Debug`, `debug`, `dbg` | `format.hd`; `dbg` has an intrinsic body |
| `std.cmp` | `Eq`, `PartialOrd`, `Ord`, `Ordering` | `cmp.hd`; `Eq` is declared in `core/mod.hd` and re-exported; primitive impls use intrinsic methods |
| `std.hash` | `Hash`, `Hasher` | declared in `core/mod.hd`, re-exported by `hash.hd` |
| `std.iter` | `Iterator`, `Iterable` | `iter.hd` |
| `std.console` | `Console`, `ConsoleError`, `println` | `console.hd`; `println` is hd over `Console` and `block_on` |
| `std.task` | `Suspend`, `Poll`, `PollContext`, `Waker` | `hd_resolve::seed` |
| `std.testing` | `it`, test code only | `hd_resolve::seed` |

### Compiler-Supplied Names Outside The Prelude

`hd_resolve::seed::SEEDED` lists the modules with seeded items. Each is an
ordinary interface item of its module; a source declaration of the same name
wins.

| Module | Supplied names | Why source alone is not enough |
| --- | --- | --- |
| `std.task` | `all` | `join!` has no writable signature; the emitter lowers it through the `task_all` key |
| `std.function` | `Fn`, `SuspendFn` | function-type syntax lowers to these constructors; `Tuple` stays in `function.hd` |
| `std.structure` | `Structure` and its members | derivation templates receive this sealed view from the compiler |
| `std.testing` | `it`, `assert_equal` | intrinsic bodies (`test_case`, `assert_equal`) |
| `std.rt.entry` | `entry_status`, `entry_status_error`, `entry_status_dyn`, `entry_status_dyn_ref`, `test_status_error`, `test_status_dyn`, `test_status_dyn_ref`, `entry_write` | hd functions in the virtual `rt/entry/mod.hd` report entry and test results over the `entry_write` intrinsic |

| Fixed, non-exporting prelude use | Why the compiler needs it |
| --- | --- |
| `std.convert.From` | postfix `?` converts through it without a user import |
| `std.ops.{Add, Sub, Mul, Div, Rem, Neg, Not, BitAnd, BitOr, BitXor, Shl, Shr, Index, IndexSet, Apply, Update, Range, RangeFrom, RangeTo, RangeFull}` | operators, indexing, callable values and ranges resolve these |
| `std.text`, `std.option`, `std.result`, `std.collections`, `std.num` | inherent methods on built-in types live here |

## Primitive Functions

`hd_host_abi::INTRINSICS` lists every `@intrinsic("key")` std may name and
its lowering: `Compiler` (code the emitter writes, crates/hd_mono/DESIGN.md
§12.2) or a host primitive import of module `hd:prim`. A test checks that
every key in `lib/std` has a row. Each key is declared once. A byte, text
or panic primitive that several std modules call is a `pub` function of the
runtime module `std.rt.bytes` (`rt/bytes.hd`), which only std may use
([`module.rt.bytes`](../../spec/lang/10-modules.md#r-module.rt.bytes)); the
others are private to the one module that calls them.

| Key | Declaration in `lib/std` | Lowering |
| --- | --- | --- |
| `bytes_len`, `bytes_at`, `bytes_slice` | `rt/bytes.hd`: `(text: string) -> usize`, `(text, index: usize) -> u8`, `(text, start, end: usize) -> string` | compiler |
| `string_from_bytes` | `(bytes: List[u8]) -> string`, in `rt/bytes.hd`; it owns its argument and takes over an unshared buffer | compiler |
| `bytes_index_json` | `(text: string, from: usize) -> usize`, in `rt/bytes.hd`; the body there is the reference, and the linear backend links the SIMD128 helper `rt:bytes_index_json` (#428) | compiler |
| `bytes_index` | `(text: string, from: usize, byte: u8) -> usize`, in `rt/bytes.hd`; the body there is the reference, and the linear backend links the SIMD128 helper `rt:bytes_index` (#428) | compiler |
| `bytes_mismatch` | `(left: string, right: string) -> usize`, in `rt/bytes.hd`; the body there is the reference, and the linear backend links the SIMD128 helper `rt:bytes_mismatch` (#428) | compiler |
| `bytes_index_pair` | `(text: string, from: usize, first: u8, last: u8, gap: usize) -> usize`, in `rt/bytes.hd`; the body there is the reference, and the linear backend links the SIMD128 helper `rt:bytes_index_pair` (#428) | compiler |
| `utf8_valid_len` | `(bytes: List[u8]) -> usize`, in `text.hd`; the body there is the reference, and the linear backend links the SIMD128 helper `rt:utf8_valid_len` (#428) | compiler |
| `string_to_bytes` | the intrinsic method `string.to_utf8(self) -> List[u8]`, in `core/mod.hd`, which `Hasher.write_str` calls | compiler |
| `char_scalar` | `(value: char) -> u32`, in `rt/bytes.hd` | compiler |
| `char_from_scalar` | `(point: u32) -> char?`, in `rt/bytes.hd` | compiler |
| `format_int` | `[N < Integer](value: N) -> string`, in `format.hd`; the runtime's `IntToStr` helper | compiler |
| `list_truncate` | `[T](items: &mut List[T], len: usize) -> void`, in `collections.hd` | compiler |
| `list_extend` | `[T](items: &mut List[T], other: List[T]) -> void`, in `collections.hd`; `other` is lent | compiler |
| `list_reserve` | `[T](items: &mut List[T], count: usize) -> void`, in `text.hd`, under `StringBuilder.reserve`; the runtime's one reserve routine, which `list_extend` and `bytes_append` call too | compiler |
| `bytes_append` | `(bytes: &mut List[u8], text: string) -> void`, in `rt/bytes.hd`, under `StringBuilder.push`; `text` is lent | compiler |
| `panic` | `panic_with(category: string, message: string) -> void`, in `rt/bytes.hd`; `rt/alloc.hd` and `rt/checked/mod.hd` keep private copies that return `never` | compiler |
| `facts_of` | `[F](f: F) -> Facts`, in `annotation.hd` | compiler |
| `downcast_ref`, `downcast_mut` | inherent methods of `dyn Inspectable` in `inspect.hd` and `dyn Error` in `error.hd` | compiler |
| `type_id_of` | `TypeId.of[T < Inspectable]() -> TypeId`, in `inspect.hd` | compiler |
| `task_cleanup_frame`, `task_abandon`, `task_pending_frame`, `task_race_frame`, `task_all_frame`, `task_wait_cell`, `task_wait_frame`, `task_wait_signal` | `task.hd` | compiler |
| `dbg`, `dbg_printer`, `dbg_write`, `repl_write` | `format.hd` | compiler |
| `panic_message`, `task_all`, `test_case`, `assert_equal`, `entry_write` | seeded items and `rt/entry/mod.hd` | compiler |
| `mem_pages`, `mem_grow`, `mem_load_u8` to `mem_load_u64`, `mem_store_u8` to `mem_store_u64`, `mem_copy`, `mem_fill` | `rt/memory.hd`, the `pub` functions of the runtime module `std.rt.memory` (spec 10 `module.rt.memory.*`) | compiler; only std can use these runtime memory intrinsics |
| `ptr_size_of`, `ptr_read`, `ptr_write`, `ptr_offset` | `rt/memory.hd`: `size_of[T]` and the methods of `Ptr[T]` in `std.rt.memory` (spec 10 `module.rt.ptr.*`); the checker makes `read` and `write` a `Read` and a `Store` of the pointer's target | compiler; only std can use this typed memory |
| `mem_buffer`, `mem_string_bytes`, `mem_new_string` | `rt/memory.hd`: `Buffer.new`, `string_bytes` and `new_string` in `std.rt.memory` (spec 10 `module.rt.buffer.*`, `module.rt.string.*`) | compiler; only std can use these |
| `format_f64`, `format_f32` | `format.hd` | host primitive |
| `string_lower`, `string_upper` | `rt/bytes.hd`: `host_lower`, `host_upper` | host primitive |
| `parse_f64`, `format_f64_fixed` | `num.hd` | host primitive |

## Intrinsic And Built-In Methods

| Member | Where | Lowering |
| --- | --- | --- |
| `add`, `sub`, `mul`, `div`, `rem`, `neg`, `bit_and`, `bit_or`, `bit_xor`, `not`, `shl`, `shr` | `@intrinsic` methods of the numeric impls in `ops.hd`; `shl` and `shr` have one impl per unsigned count type | TIR `Prim` |
| `eq`, `partial_cmp`, `cmp` | `@intrinsic` methods of the number, `char` and `bool` impls in `cmp.hd` | TIR `Prim` |
| `Index.index` on `List`, `Map`, `string`; `IndexSet.index_set` on `List`, `Map` | `ops.hd`; each body is the built-in indexing (`self[key]`) | built-in indexing, with its panics |
| `List.len`, `List.push`, `List.iter`, `Map.len`, `Map.get`, `Map.remove`, `Map.iter` | no declaration; `hd_check` `builtin_method` | `IntrinsicOp` |

Every other `List` mutator is hd over `push`, `list_truncate` and
`list_extend`; string
methods and `Display.to_string` are hd over the `bytes_*` primitives and
ordinary impls. Earlier designs made these compiler-supplied; keeping them
in hd shrinks the compiler to a few kernels and lets a program that never
calls a method compile none of its code.

## Host Capabilities

`hd_host_abi::TABLE` mirrors std's capability traits (`Console`,
`ConsoleInput`, `Clock`, `FsRead`, `FsWrite`, `Random`, `Http`, `Net`,
`Process`, `Sys`, `Env`, `Args`) with every host method; `RUNNERS` holds
`TestRunner` and `PropertyRunner`, and `HANDLES` the `std.net` handle
traits. A test checks the table against std. It is not an intrinsic
registry.

## Annotations And Derivation

| Syntax used by `lib/std` | Declarations or targets |
| --- | --- |
| `@annotate::[F](.Fn)` | `std.ops.NumSuffix[F]`, `std.ops.StrPrefix[F]` |
| `@annotate(.Variant)` | `std.ops.DefaultVariant` |
| `@annotate::[F](.Field)` | `std.testing.arbitrary.With[F]` |
| `@annotate(.Data, .Enum)` | `std.annotation.Annotate` itself (the spec's self-bootstrap rule) |
| `@str_prefix` | `std.text.r` |
| `@num_suffix` | `std.time.ms`, `s`, `min`, `h` |

No std declaration uses `@derive`, `@error`, `@from` or `@source`: std
types write their `Debug` builder calls and error impls by hand, so std
needs no self-derivation.

| `by Structure` template | Location | Instance family |
| --- | --- | --- |
| `Arbitrary` | `testing.hd` | struct and enum values |
| `Hash` | `hash.hd` | struct and enum values; tuples |
| `Default` | `ops.hd` | tuples; struct and enum values |
| `Eq`, `PartialOrd`, `Ord` | `cmp.hd` | struct and enum values; tuples |
| `Serialize`, `Deserialize` | `serde.hd` | struct and enum values |
| `Debug` | `format.hd` | struct and enum values; tuples |
| `Display` | `format.hd` | tuples |

## Folder Graph

| Folder | Modules | Outgoing folder edges |
| --- | --- | --- |
| `std.core` (`lib/std/core`) | `std.core` from `core/mod.hd`: the seeded names, and `Copy`, `Eq`, `Hash`, `Hasher` | none |
| `std` (`lib/std`) | the root files other than `prelude.hd` and `testing.hd` | `std.core`, `std.rt` (`std.rt.bytes`) |
| `std.rt` (`lib/std/rt`) | `std.rt.bytes`, `std.rt.memory`, `std.rt.alloc`, `std.rt.cycle`, `std.rt.weak` | `std.core` |
| `std.rt.entry` (virtual `rt/entry/mod.hd`) | `std.rt.entry` | `std`, `std.core` |
| `std.rt.checked` (`lib/std/rt/checked`) | `std.rt.checked` | `std`, `std.rt`, `std.core` |
| `std.rt.checked_report` (`lib/std/rt/checked_report`) | `std.rt.checked_report` | `std.rt.checked`, `std.rt`, `std.core` |
| `std.testing` (`lib/std/testing`) | `std.testing` from `testing.hd`, plus `std.testing.arbitrary` | `std` |
| `std.prelude` (`lib/std/prelude`) | `std.prelude` from `prelude.hd`, plus `std.prelude.testing` | `std`; `std.testing`, through the test-only re-export of `it` |

The graph is acyclic. Under
[`module.folder.parent-file`](../../spec/lang/10-modules.md#r-module.folder.parent-file),
`testing.hd` and `testing/arbitrary.hd` share folder `std.testing`, so the
re-export between them adds no edge.

`std.core` and `std.rt` are std's leaf layer
([`module.core.leaf`](../../spec/lang/10-modules.md#r-module.core.leaf),
[`module.rt.leaf`](../../spec/lang/10-modules.md#r-module.rt.leaf)).
`std.core` declares the traits that `List` and `Map` name, which `std.ops`,
`std.cmp` and `std.hash` re-export. A module of either folder sees only
`std.core`'s prelude row, so neither reaches the root folder. The entry
glue (`hd_driver::RT_SOURCE`, module `std.rt.entry`) and the checked
runtime use `std.format`, `std.process`, `std.error` and `std.json`, so
each is a folder of its own under `rt/`, above the root. Every other
prelude origin is in folder `std`, so the fixed uses give every other
folder an edge to `std`.
