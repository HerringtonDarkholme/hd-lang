# Intrinsics Sweep

Status: audit, 2026-11-02, at main 0cbec9c65 plus #1204; the inventory for
#1206. It changes no code. Every place the compiler gives a std name or a
type a meaning of its own is listed once, in seven kinds, with a verdict:

- **keep**: a real primitive. A machine operation, the memory model, the
  host boundary, the suspension runtime, or type information only the
  compiler has. hd cannot express it, and should not.
- **hd later**: hd can already express it, but the hd version is slower
  because of how hd memory code compiles (#180, #514). #1205's gate is
  met: `UniqMap` runs its benchmark within 1.06x of `Map` since #1711.
  `List` and `Map` stay compiler built-ins (owner, 2026-11-12), so their
  rows are keep; only the string kernels remain hd later.
- **hd now**: hd can express it today at no known cost. Replace it.
- **review**: the compiler knows a std name it may not need; find out why
  before choosing.

Owner direction (2026-11-02): a std performance gap is std and optimizer
quality, fixed in hd and in how hd compiles, never with a new intrinsic.

Implementation update for #1324: `bytes_append` has no intrinsic key or
backend helper; builders append with an hd memory copy over unique room.
`format_int` stays one allocation-free writer intrinsic, with its string
wrapper in hd.

Update, 2026-11-13 (main `a45fdfc17`): the counts and rows below are
recounted against the tree. Weak references and their intrinsics and
helpers were removed (#1731), `unique_mut` and `make_unique` were deleted
(#1732), and reference views went with #1448. The verdicts are the
2026-11-02 audit's, except where an owner decision since is cited.

## Summary

| Kind                                                        |     Count | keep | hd later | hd now | review |
| ----------------------------------------------------------- | --------: | ---: | -------: | -----: | -----: |
| 1. `@intrinsic` functions (`hd_resolve::intrinsics`)        |   88 keys |   75 |        7 |      0 |      6 |
| 2. Compiler-emitted runtime helpers (`hd_wasm::rt::Helper`) |       130 |  122 |        8 |      0 |      0 |
| 3. Built-in types                                           |  34 forms |   34 |        0 |      0 |      0 |
| 4. Built-in methods (spec ch. 10)                           |        17 |   17 |        0 |      0 |      0 |
| 5. Known std items (`hd_resolve::KnownItems`)               |       103 |   94 |        0 |      0 |      9 |
| 6. Host capabilities (`hd_host_abi` tables)                 | 17 traits |   17 |        0 |      0 |      0 |
| 7. Compiler-interpreted annotations                         |        13 |   13 |        0 |      0 |      0 |

The `List`/`Map` built-ins (two types, eleven methods, five intrinsics,
23 runtime helpers and four known items) are keep since the owner closed
#1219 and #1220 on 2026-11-12. The one "hd later" group left is the string
kernels (seven intrinsics and eight helpers).
The original gate was hd-level memory code compiling as well as the
compiler's hand-emitted helpers. #1205 closed on 2026-11-12: at
`71aa383a6`, `UniqMap` measured 51 ms against `Map`'s 48 ms (1.06x,
15 interleaved V8 release runs), meeting its 1.1x target. This is the
[recorded result](https://github.com/HerringtonDarkholme/hd-lang/issues/1205#issuecomment-6814901613),
not a new measurement of every kernel. `List` and `Map` stay built-in
(owner, 2026-11-12); the inventory below remains the dated snapshot.

## 1. `@intrinsic` Functions

95 `@intrinsic("key")` declarations in lib/std name 83 keys; 45 more
`@intrinsic` annotations with no key mark operator, comparison and float
bodies. `hd_resolve::intrinsics::INTRINSICS` has 84 rows: the 83 keys and
`entry_write`, which the seeded `rt/entry/mod.hd` declares. Seeding gives
four more items an intrinsic body with no row: `panic_message`, `box`,
`task_all` and `test_case`. That makes the 88 keys below. All lower
inline except the seven host primitives of `hd_host_abi::PRIMITIVES`
(the host formatting row and `snapshot_update`).

| Group                            | Keys                                                                                                                                                                                                           | Verdict  | Why                                                                                                                                                                                      |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Memory (`std.rt.memory`, 20)     | `mem_pages`, `mem_grow`, `mem_load_u8`..`u64`, `mem_store_u8`..`u64`, `mem_copy`, `mem_fill`, `ptr_size_of`, `ptr_read`, `ptr_write`, `ptr_ref`, `ptr_mut`, `ptr_offset`, `mem_string_bytes`, `mem_new_string` | keep     | the primitives hd collections are written over                                                                                                                                           |
| String layout (6)                | `bytes_len`, `bytes_at`, `string_from_bytes`, `string_to_bytes`, `char_scalar`, `char_from_scalar`                                                                                                             | keep     | they read and build the string representation itself                                                                                                                                     |
| String kernels (7)               | `bytes_slice`, `bytes_index`, `bytes_index_pair`, `bytes_index_set`, `bytes_mismatch`, `utf8_valid_len`, `format_int`                                                                                          | hd later | loops over bytes; the hd versions measured 1.14x to 1.34x slower (#514)                                                                                                                  |
| Machine operations (12)          | `int_wrapping_add`, `_sub`, `_mul`, `int_count_ones`, `int_leading_zeros`, `int_trailing_zeros`, `int_min`, `int_max`, `f64_to_bits`, `f64_from_bits`, `f32_to_bits`, `f32_from_bits`                          | keep     | one Wasm instruction or a per-width constant                                                                                                                                             |
| Collections (5)                  | `list_truncate`, `list_extend`, `list_reserve`, `map_probe`, `map_steps`                                                                                                                                       | keep     | `List`/`Map` internals; the built-in types stay (owner, 2026-11-12)                                                                                                                         |
| Panics and boxes (3)             | `panic`, `panic_message`, `box`                                                                                                                                                                                | keep     | the trap and the counted allocation                                                                                                                                                      |
| Type information (6)             | `type_id_of`, `downcast_ref`, `downcast_mut`, `facts_of`, `ref_identity`, `descriptor_glue_mask`                                                                                                               | keep     | only the compiler knows types, descriptors and addresses                                                                                                                                 |
| Suspension (11)                  | `block_on`, `task_all`, `task_all_frame`, `task_race_frame`, `task_cleanup_frame`, `task_pending_frame`, `task_abandon`, `task_wait_cell`, `task_wait_frame`, `task_wait_signal`, `task_idle_hook`             | keep     | the suspension runtime's frames                                                                                                                                                          |
| Closures (2)                     | `fn_call`, `fn_mut_call`                                                                                                                                                                                       | keep     | the closure calling convention                                                                                                                                                           |
| Hash seed (1)                    | `map_hash_seed`                                                                                                                                                                                                | keep     | a fresh map seed mixed from the recorded host run seed and a per-run counter                                                                                                             |
| Debug, test and entry output (9) | `debug_texts`, `dbg_write`, `dbg_printer`, `repl_write`, `entry_write`, `test_case`, `snapshot_update` (host), `caller_location`, `caller_args`                                                                   | keep     | `dbg` and `assert_equal` are ordinary `@track_caller` hd (#1210); `debug_texts` and `dbg_printer` select a printer by static type; the caller pair reads the call site; the rest are output and test registration |
| Host formatting and case (6)     | `format_f64`, `format_f32`, `format_f64_fixed`, `parse_f64`, `string_lower`, `string_upper`                                                                                                                    | review   | every host must implement them, the Rust host included; hd versions (shortest float printing, Unicode case tables) would remove six host primitives, at a size and speed cost to measure |

## 2. Runtime Helpers

`hd_wasm::rt::Helper` has 130 variants: functions the backend emits
into each module on demand.

| Group                                             | Helpers                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | Verdict                                                                                                                                                            |
| ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Allocation, counts, drop, cycles (32) | `Alloc`, `New`, `NewFixed`, `AllocClass`, `FreeClass`, `FreeClassLoaded`, `FreeFrame`, `Retain`, `Release`, `ReleaseRef`, `ReleaseDrain`, `RetainKnown`, `ReleaseKnown`, `RetainAny`, `RetainFields`, `DropFields`, `DropValue`, `TakeBox`, `Collect`, `CycleTrace`, `CycleRelease`, `CycleFree`, `Teardown`, `CheckedPoll`, `CollectPoll`, `Opaque`, `Str`, `TypeIdOf`, `TypeInfo`, `RuntimeType`, `MemoryCopy`, `MemoryFill` | keep |
| Loans and entry (6) | `LoanShare`, `LoanUnshare`, `EntryInit`, `EntryPoll`, `EntryReset`, `DynTrap` | keep |
| Suspension, wakers, cancellation, host calls (49) | `CanCancel`, `CancellationRoot`, `PollCheck`, `CancelCheck`, `CancelEnter`, `CancelLeave`, `WakeMark`, `WakeTake`, `WakeSlot`, `WakeRegister`, `WakeRemove`, `WakeSignal`, `WakerNew`, `WakerParent`, `WakerSlot`, `WakerLive`, `WakerDrop`, `WakeLink`, `WakeNotify`, `EntryCancel`, `EntryCall`, `ChildDrop`, `CleanupCold`, `CleanupPoll`, `CleanupCancel`, `PendingCold`, `PendingPoll`, `PendingCancel`, `WaitCell`, `WaitCold`, `WaitPoll`, `WaitSignal`, `Race`, `RaceDrop`, `All`, `Frame`, `Drive`, `HostDrive`, `HostPoll`, `HostCancel`, `HostCold`, `HostCall`, `HostPrim`, `ForbidCheck`, `CtxFind`, `CtxNew`, `IdleBlock`, `IdleHook`, `IdleWake` | keep |
| Host ABI encoding (7) | `Fit`, `LebPut`, `LebGet`, `Enc`, `Dec`, `HandleSlot`, `PackageHandleRelease` | keep |
| Panics (3) | `Panic`, `PanicStr`, `PanicWith` | keep |
| Collections (23) | `ListBuf`, `ListUnique`, `ListCopy`, `ListGrow`, `ListTruncate`, `ListReserve`, `ListExtend`, `CopyElems`, `DropElems`, `CollCopy`, `ToBuf`, `MapTable`, `DropEntries`, `MapHash`, `MapFind`, `MapPut`, `MapGrow`, `MapUnique`, `MapSlotNext`, `MapStep`, `MapSeed`, `DynSlot`, `DynRead` | keep (with kind 3) |
| String kernels (10) | `StrFromBytes`, `StrToBytes`, `StrAppend`, `FmtInt`, `StrEq`, `BytesIndexSet`, `BytesIndex`, `BytesIndexPair`, `BytesMismatch`, `Utf8ValidLen` | hd later, except `StrFromBytes`/`StrToBytes` (keep: layout) |

The float math helpers `PowF64` and `RemF64` are gone; so are the
reference-view helpers (#1448) and the weak-reference helpers (#1731).

## 3. Built-In Types

| Type                                                                       | Verdict                                                                      |
| -------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| integers (`i8`..`i64`, `u8`..`u64`, `usize`), `f32`, `f64`, `bool`, `char` | keep                                                                         |
| `void`, `never`                                                            | keep                                                                         |
| `string`                                                                   | keep (its layout is the primitive the string kernels sit on)                 |
| tuples, unions, function types, closures                                   | keep                                                                         |
| `&T`, `&mut T`, `*T`, `*mut T`, `box(x)`                                   | keep: the memory model                                                       |
| `T?`                                                                       | keep                                                                         |
| `Suspend[T]`                                                               | keep: the suspension runtime                                                 |
| `Any`, `AnyVal`, `AnyRef`, `'static`, `Copy`                               | keep: markers only the compiler can answer                                   |
| `List[T]`, `Map[K, V]`                                                     | keep (owner, 2026-11-12: `List` and `Map` stay built-in; #1219 and #1220 closed) |

`Result`, `Range`, `Buffer` and `Ordering` are std declarations the
compiler recognizes (kind 5), not compiler-supplied types.

## 4. Built-In Methods

spec/lang/10-modules.md, Built-In Methods.

| Receiver    | Methods                                                      | Verdict               |
| ----------- | ------------------------------------------------------------ | --------------------- |
| `string`    | `len`, `chars`, `char_indices`, `bytes`, `slice`             | keep (string layout)  |
| `Display`   | `to_string`                                                  | keep (interpolation)  |
| `List[T]`   | `len`, `iter`, `push`, `pop`, `insert`, `remove_at`, `clear` | keep, with `List`     |
| `Map[K, V]` | `len`, `get`, `iter`, `remove`                               | keep, with `Map`      |

## 5. Known Std Items

`hd_resolve::KnownItems` names 103 std items the compiler gives meaning to.

| Group | Items | Verdict |
| --- | --- | --- |
| Operators | `add`, `sub`, `mul`, `div`, `rem`, `neg`, `not`, `bit_and`, `bit_or`, `bit_xor`, `shl`, `shr`, `eq`, `partial_ord`, `ord`, `ordering`, `index`, `index_set`, `index_mut`, `apply`, `update` | keep: operator syntax lowers to them |
| Iteration | `iterator`, `iterable`, `range`, `range_from`, `range_to`, `range_full` | keep: `for` and range syntax |
| Markers and core types | `any`, `any_val`, `any_ref`, `static_bound`, `copy`, `tuple`, `union_type`, `result`, `option`, `default`, `num`, `integer`, `float`, `num_suffix`, `str_prefix`, `template`, `ptr`, `function_module` | keep: literal and type rules |
| Errors and resources | `error`, `from`, `disposable`, `async_disposable`, `auto_closing`, `drop_trait`, `drop_method` | keep: `?`, `using`, `@error`, and the `std.rt` `Drop` hook that release glue calls |
| Closures | `fn_trait`, `fn_call`, `fn_mut_trait`, `fn_mut_call` | keep |
| Suspension | `suspend`, `block_on`, `race`, `all_list` | keep |
| Derivation and facts | `structure`, `walker`, `walker_data_variant`, `walker_data_member`, `field`, `field_get`, `variant_holds`, `describer`, `source`, `fills_defaults`, `skips`, `facts`, `facts_find`, `inspectable`, `annotate` | keep: the `Structure` walk that std derives over |
| Call sites | `site_location`, `site_args`, `site_function_text` | keep: the call-site records of `@track_caller` |
| Formatting | `display`, `debug`, `dbg` | keep: interpolation and `dbg` |
| Hashing | `hash`, `map_key_hash`, `key_equal`, `map_integer_hash`, `map_string_hash` | keep: `Map` codegen selects ordinary `std.hash` foldhash functions through these known-item slots; `UniqMap` calls the same functions directly |
| Testing | `testing_module`, `it_each`, `it_prop`, `it_prop_with`, `each_case`, `prop_case`, `prop_with_case`, `test_runner`, `property_runner` | review: which of these need the compiler (source spans, test registration) and which are plain std |
| Serde | `serialize`, `deserialize` | keep: std.json picks its fast path in hd with `Serialize.regular()` (#1445) |
| Collections | `list`, `list_is_empty`, `map`, `buffer` | keep, with kind 3 |
| Code-generation hints | `inline`, `cold` | keep |

`println`, `check_equal`, `serializer_begin_data`, `serializer_end_data`
and the capture-cell items `ref_cell` and `counted_cell` are no longer
known items.

## 6. Host Capabilities

17 host traits, the program's only way out of its instance: `Console`,
`ConsoleInput`, `Clock`, `FsRead`, `FsWrite`, `Random`, `Http`, `Net`,
`Process`, `Sys`, `Env`, `Args`, `TestRunner`, `PropertyRunner`, and the
handles `TcpStream`, `TcpListener`, `UdpSocket`. All keep. The runtime's
own imports (`hd:rt` `block`, `abort`, `stderr`, `stdout`, `panic`,
`fact`, `hash_seed`) keep too.

## 7. Compiler-Interpreted Annotations

| Annotation                                                             | Verdict                                                                                                                                        |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `@intrinsic`, `@host`                                                  | keep: they mark the rows above                                                                                                                 |
| `@derive`                                                              | keep: it selects std's `Structure` derivations, written in hd                                                                                  |
| `@inline`, `@cold`, `@cfg`                                             | keep: code generation and configuration                                                                                                        |
| `@annotate`, `@num_suffix`, `@str_prefix`, `@skips`, `@fills_defaults` | keep: facts and literal rules                                                                                                                  |
| `@error`                                                               | keep (owner, 2026-11-02): a deliberate compiler-generated form, as `thiserror` is a macro                                                      |
| `@auto_closing`                                                        | keep (owner, 2026-11-02): it marks the function whose statement calls register their result; the result type's side is the `AutoClosing` trait |

## Next

`List` and `Map` stay compiler built-ins (owner, 2026-11-12): #1206,
#1219, #1220, #1223 and #1350 are closed, and their rows above are keep.
String-kernel pricing remains separate (#514). Review rows report findings
on their issue, and the owner decides each one.
