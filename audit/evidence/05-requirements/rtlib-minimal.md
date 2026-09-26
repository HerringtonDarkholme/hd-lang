<!-- commit bd985d7; command: node --experimental-strip-types audit/scripts/arch/rtlib-inventory.ts audit/probes/arch/runtime-lib/minimal.hd; date: 2026-09-25T20:44:13.856Z -->

Source: `audit/probes/arch/runtime-lib/minimal.hd`

| item | emitted | kept after remove-unused-module-elements |
| --- | --- | --- |
| defined functions | 28 | 1 |
| imports | 1 | 0 |
| globals | 9 | 0 |
| type definitions (text) | 47 | 1 |
| exports | 1 | 1 |
| wasm bytes | 2458 | 37 (binaryen -O2: 37) |

Exports: main

Imports: hd.panic

Unused imports: hd.panic

Unused functions (27): hd.provider_get, hd.provider_concat, hd.vector_get, hd.vector_set, hd.vector_append, hd.add_i32, hd.sub_i32, hd.mul_i32, hd.pow_i32, hd.neg_i32, hd.shl_i32, hd.shr_i32, hd.string_len, hd.string_concat, hd.i32_to_string, hd.char_to_string, hd.string_starts_with, hd.string_compare, hd.suspension_poll, hd.suspension_cancel, hd.suspension_result, hd.suspension_drive, hd.map_key_equal, hd.map_insert, hd.map_get, hd.map_remove, fv0

Unused globals (9): e0v0, e0v1, e0v2, hd.runtime-root, hd.driver-active, hd.host-call-site, hd.panic-integer-overflow, hd.panic-invalid-shift, hd.panic-index-out-of-bounds

All defined functions: hd.provider_get, hd.provider_concat, hd.vector_get, hd.vector_set, hd.vector_append, hd.add_i32, hd.sub_i32, hd.mul_i32, hd.pow_i32, hd.neg_i32, hd.shl_i32, hd.shr_i32, hd.string_len, hd.string_concat, hd.i32_to_string, hd.char_to_string, hd.string_starts_with, hd.string_compare, hd.suspension_poll, hd.suspension_cancel, hd.suspension_result, hd.suspension_drive, hd.map_key_equal, hd.map_insert, hd.map_get, hd.map_remove, f0, fv0
