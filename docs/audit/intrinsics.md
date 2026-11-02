# Intrinsics Sweep

Status: audit, 2026-11-02, at main 0cbec9c65 plus #1204; the inventory for
#1206. It changes no code. Every place the compiler gives a std name or a
type a meaning of its own is listed once, in seven kinds, with a verdict:

- **keep**: a real primitive. A machine operation, the memory model, the
  host boundary, the suspension runtime, or type information only the
  compiler has. hd cannot express it, and should not.
- **hd later**: hd can already express it, but the hd version is slower
  because of how hd memory code compiles (#180, #514, #1205). Replace it
  once the optimizer tickets from #1205's WAT diff land.
- **hd now**: hd can express it today at no known cost. Replace it.
- **review**: the compiler knows a std name it may not need; find out why
  before choosing.

Owner direction (2026-11-02): a std performance gap is std and optimizer
quality, fixed in hd and in how hd compiles, never with a new intrinsic.

## Summary

| Kind | Count | keep | hd later | hd now | review |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1. `@intrinsic` functions (`hd_host_abi::INTRINSICS`) | 95 keys | 66 | 15 | 0 | 14 |
| 2. Compiler-emitted runtime helpers (`hd_wasm_linear::rt::Helper`) | 146 | 110 | 34 | 2 | 0 |
| 3. Built-in types | 34 forms | 32 | 2 | 0 | 0 |
| 4. Built-in methods (spec ch. 10) | 17 | 6 | 11 | 0 | 0 |
| 5. Known std items (`hd_resolve::KnownItems`) | 102 | 76 | 9 | 0 | 17 |
| 6. Host capabilities (`hd_host_abi` tables) | 17 traits | 17 | 0 | 0 | 0 |
| 7. Compiler-interpreted annotations | 13 | 11 | 0 | 0 | 2 |

The two big "hd later" groups are the same code seen from two sides: the
`List`/`Map` built-ins (two types, eleven methods, five intrinsics and
25 runtime helpers) and the string kernels (ten intrinsics and nine
helpers).
Both wait on one thing: hd-level memory code compiling as well as the
compiler's hand-emitted helpers. #1205 measures it on `UniqMap`: the same
table design is 2.3x slower in hd.

## 1. `@intrinsic` Functions

94 declarations in lib/std name 90 keys; with the compiler-supplied
`panic`, `box`, `task_all`, `test_case` and `assert_equal` the table has
95. All lower inline (`Lowering::Compiler`) except the seven host
primitives in the last rows.

| Group | Keys | Verdict | Why |
| --- | --- | --- | --- |
| Memory (`std.rt.memory`, 21) | `mem_pages`, `mem_grow`, `mem_load_u8`..`u64`, `mem_store_u8`..`u64`, `mem_copy`, `mem_fill`, `ptr_size_of`, `ptr_read`, `ptr_write`, `ptr_offset`, `mem_buffer`, `buffer_item_ref`, `buffer_item_mut`, `mem_string_bytes`, `mem_new_string` | keep | the primitives hd collections are written over |
| String layout (6) | `bytes_len`, `bytes_at`, `string_from_bytes`, `string_to_bytes`, `char_scalar`, `char_from_scalar` | keep | they read and build the string representation itself |
| String kernels (10) | `bytes_slice`, `bytes_concat`, `bytes_index`, `bytes_index_pair`, `bytes_index_json`, `bytes_mismatch`, `utf8_valid_len`, `concat_all`, `bytes_append`, `format_int` | hd later | loops over bytes; the hd versions measured 1.14x to 1.34x slower (#514) |
| Machine operations (12) | `int_wrapping_add`, `_sub`, `_mul`, `int_count_ones`, `int_leading_zeros`, `int_trailing_zeros`, `int_min`, `int_max`, `f64_to_bits`, `f64_from_bits`, `f32_to_bits`, `f32_from_bits` | keep | one Wasm instruction or a per-width constant |
| Collections (5) | `list_truncate`, `list_extend`, `list_reserve`, `map_probe`, `map_steps` | hd later | `List`/`Map` internals; they go with the built-in types (kind 3) |
| Panics and boxes (3) | `panic`, `panic_message`, `box` | keep | the trap and the counted allocation |
| Type information (6) | `type_id_of`, `downcast_ref`, `downcast_mut`, `facts_of`, `ref_identity`, `descriptor_glue_mask` | keep | only the compiler knows types, descriptors and addresses |
| Suspension (10) | `block_on`, `task_all`, `task_all_frame`, `task_race_frame`, `task_cleanup_frame`, `task_pending_frame`, `task_abandon`, `task_wait_cell`, `task_wait_frame`, `task_wait_signal` | keep | the suspension runtime's frames |
| Closures (2) | `fn_call`, `fn_mut_call` | keep | the closure calling convention |
| Weak references (5) | `weak_new`, `weak_new_mut`, `weak_upgrade`, `weak_upgrade_mut`, `weak_weaken` | keep | reference-count internals |
| Hash seed (1) | `map_hash_seed` | keep | the per-run seed from the host |
| Debug, test and entry output (8) | `dbg`, `dbg_write`, `dbg_printer`, `repl_write`, `entry_write`, `test_case`, `assert_equal`, `snapshot_update` (host) | review | `dbg` and `dbg_printer` need source locations and type printers; why `assert_equal` and `test_case` cannot be hd over `Debug` and a location is the open question |
| Host formatting and case (6) | `format_f64`, `format_f32`, `format_f64_fixed`, `parse_f64`, `string_lower`, `string_upper` | review | every host must implement them, the Rust host included; hd versions (shortest float printing, Unicode case tables) would remove six host primitives, at a size and speed cost to measure |

## 2. Runtime Helpers

`hd_wasm_linear::rt::Helper` has 146 variants: functions the backend emits
into each module on demand.

| Group | Helpers | Verdict |
| --- | --- | --- |
| Allocation, counts, drop, cycles (30) | `Alloc`, `New`, `NewFixed`, `AllocClass`, `FreeClass`, `FreeClassLoaded`, `Retain`, `Release`, `ReleaseRef`, `ReleaseDrain`, `RetainKnown`, `ReleaseKnown`, `RetainAny`, `RetainFields`, `DropFields`, `Collect`, `CycleTrace`, `CycleRelease`, `CycleFree`, `Teardown`, `CheckedPoll`, `CollectPoll`, `WeakNoopDrop`, `WeakNew`, `WeakEntryDrop`, `WeakTypeInit`, `Opaque`, `Str`, `TypeIdOf`, `RuntimeType` | keep |
| References, views, loans (21) | `RefProject`, `RefFieldGet`, `RefFieldSet`, `RefPayload`, `RefPayloadGet`, `RefPayloadSet`, `RefIsView`, `RefViewsAbsent`, `RefViewDrop`, `RefAddress`, `RefView`, `RefGet`, `RefSet`, `LoanBegin`, `LoanCount`, `LoanCheck`, `EntryInit`, `EntryPoll`, `EntryReset`, `ListPinned`, `DynTrap` | keep |
| Suspension, wakers, cancellation, host calls (47) | `CanCancel`, `CancellationRoot`, `PollCheck`, `CancelCheck`, `CancelEnter`, `CancelLeave`, `WakeMark`, `WakeTake`, `WakeSlot`, `WakeRegister`, `WakeRemove`, `WakeSignal`, `WakerNew`, `WakerParent`, `WakerSlot`, `WakerLive`, `WakerDrop`, `WakeLink`, `WakeNotify`, `EntryCancel`, `EntryCall`, `ChildDrop`, `CleanupCold`, `CleanupPoll`, `CleanupCancel`, `PendingCold`, `PendingPoll`, `PendingCancel`, `WaitCell`, `WaitCold`, `WaitPoll`, `WaitSignal`, `Race`, `RaceDrop`, `All`, `OpenFrame`, `Frame`, `Drive`, `HostDrive`, `HostPoll`, `HostCancel`, `HostCold`, `HostCall`, `HostPrim`, `ForbidCheck`, `CtxFind`, `CtxNew` | keep |
| Host ABI encoding (7) | `Fit`, `LebPut`, `LebGet`, `Enc`, `Dec`, `HandleSlot`, `PackageHandleRelease` | keep |
| Panics (3) | `Panic`, `PanicStr`, `PanicWith` | keep |
| Collections (25) | `ListBuf`, `ListClone`, `ListUnique`, `ListCopy`, `ListGrow`, `ListTruncate`, `ListReserve`, `ListExtend`, `CopyElems`, `DropElems`, `CollCopy`, `ToBuf`, `MapTable`, `DropEntries`, `MapHash`, `MapFind`, `MapPut`, `MapGrow`, `MapUnique`, `MapClone`, `MapSlotNext`, `MapStep`, `DynSlot`, `DynRead`, `BytesAppend` | hd later (with kind 3) |
| String kernels (11) | `StrFromBytes`, `StrToBytes`, `ConcatAll`, `FmtInt`, `StrEq`, `StrCmp`, `BytesIndexJson`, `BytesIndex`, `BytesIndexPair`, `BytesMismatch`, `Utf8ValidLen` | hd later, except `StrFromBytes`/`StrToBytes` (keep: layout) |
| Float math (2) | `PowF64`, `RemF64` | hd now: Wasm has no `pow` or `fmod` instruction, so these are already ordinary code the backend writes; an hd version in `std.num` is the same code in the library |

## 3. Built-In Types

| Type | Verdict |
| --- | --- |
| integers (`i8`..`i64`, `u8`..`u64`, `usize`), `f32`, `f64`, `bool`, `char` | keep |
| `void`, `never` | keep |
| `string` | keep (its layout is the primitive the string kernels sit on) |
| tuples, unions, function types, closures | keep |
| `&T`, `&mut T`, `*T`, `*mut T`, `box(x)` | keep: the memory model |
| `T?` | keep |
| `Suspend[T]` | keep: the suspension runtime |
| `Any`, `AnyVal`, `AnyRef`, `'static`, `Copy` | keep: markers only the compiler can answer |
| `List[T]`, `Map[K, V]` | hd later: #180 and #1205; `UniqList`/`UniqMap` already show the design in hd |

`Result`, `Range`, `Buffer` and `Ordering` are std declarations the
compiler recognizes (kind 5), not compiler-supplied types.

## 4. Built-In Methods

spec/lang/10-modules.md, Built-In Methods.

| Receiver | Methods | Verdict |
| --- | --- | --- |
| `string` | `len`, `chars`, `char_indices`, `bytes`, `slice` | keep (string layout) |
| `Display` | `to_string` | keep (interpolation) |
| `List[T]` | `len`, `iter`, `push`, `pop`, `insert`, `remove_at`, `clear` | hd later, with `List` |
| `Map[K, V]` | `len`, `get`, `iter`, `remove` | hd later, with `Map` |

## 5. Known Std Items

`hd_resolve::KnownItems` names 102 std items the compiler gives meaning to.

| Group | Items | Verdict |
| --- | --- | --- |
| Operators | `add`, `sub`, `mul`, `div`, `rem`, `neg`, `not`, `bit_and`, `bit_or`, `bit_xor`, `shl`, `shr`, `eq`, `partial_ord`, `ord`, `ordering`, `index`, `index_set`, `apply`, `update` | keep: operator syntax lowers to them |
| Iteration | `iterator`, `iterable`, `range`, `range_from`, `range_to`, `range_full` | keep: `for` and range syntax |
| Markers and core types | `any`, `any_val`, `any_ref`, `static_bound`, `copy`, `tuple`, `union_type`, `result`, `default`, `num`, `integer`, `float`, `num_suffix`, `str_prefix`, `template` | keep: literal and type rules |
| Errors and resources | `error`, `from`, `disposable`, `async_disposable`, `auto_closing` | keep: `?`, `using`, `@error` |
| Closures | `fn_trait`, `fn_call`, `fn_mut_trait`, `fn_mut_call` | keep |
| Cells | `ref_cell`, `counted_cell` | keep: capture cells |
| Suspension | `suspend`, `block_on`, `race`, `all`, `all_list` | keep |
| Derivation and facts | `structure`, `walker`, `walker_data_variant`, `walker_data_member`, `field`, `field_get`, `variant_holds`, `describer`, `source`, `fills_defaults`, `skips`, `facts`, `facts_find`, `inspectable`, `annotate` | keep: the `Structure` walk that std derives over |
| Formatting | `display`, `debug`, `dbg` | keep: interpolation and `dbg` |
| Hashing | `hash`, `key_bucket_hash`, `key_equal`, `int_bucket_hash`, `string_bucket_hash` | hd later: the `Map` codegen picks a hash per key type by these names; with `Map` in hd they become ordinary calls |
| Testing | `check_equal`, `it_each`, `it_prop`, `it_prop_with`, `each_case`, `prop_case`, `prop_with_case`, `snapshot`, `test_runner`, `property_runner` | review: which of these need the compiler (source spans, test registration) and which are plain std |
| Output and serde | `println`, `json_encode`, `json_decode`, `serialize`, `deserialize`, `serializer_begin_data`, `serializer_end_data` | review: why the checker knows `println` and the driver knows `json_encode`; serde's begin/end hooks may belong to the `Structure` walk |
| Collections | `list`, `list_is_empty`, `map`, `buffer` | hd later, with kind 3 |
| Code-generation hints | `inline`, `cold` | keep |

## 6. Host Capabilities

17 host traits, the program's only way out of its instance: `Console`,
`ConsoleInput`, `Clock`, `FsRead`, `FsWrite`, `Random`, `Http`, `Net`,
`Process`, `Sys`, `Env`, `Args`, `TestRunner`, `PropertyRunner`, and the
handles `TcpStream`, `TcpListener`, `UdpSocket`. All keep. The runtime's
own imports (`hd:rt` `block`, `abort`, `stderr`, `stdout`, `panic`,
`fact`, `hash_seed`) keep too.

## 7. Compiler-Interpreted Annotations

| Annotation | Verdict |
| --- | --- |
| `@intrinsic`, `@host` | keep: they mark the rows above |
| `@derive` | keep: it selects std's `Structure` derivations, written in hd |
| `@inline`, `@cold`, `@cfg` | keep: code generation and configuration |
| `@annotate`, `@num_suffix`, `@str_prefix`, `@skips`, `@fills_defaults` | keep: facts and literal rules |
| `@error` | review: the compiler generates the error enum's `Display` and `From` impls; a `Structure`-based derivation in std could do the same |
| `@auto_closing` | review: one check on a function's result; could be a bound |

## Next

1. The #1205 WAT diff of `UniqMap.insert` against `rt:map_put`, which names
   the optimizer tickets every "hd later" row waits on.
2. `PowF64` and `RemF64` into `std.num` as hd (the "hd now" rows).
3. The twelve "review" items, one finding each, in #1206.
