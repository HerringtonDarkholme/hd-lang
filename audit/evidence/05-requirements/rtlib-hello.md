<!-- commit bd985d7; command: node --experimental-strip-types audit/scripts/arch/rtlib-inventory.ts audit/probes/arch/runtime-lib/hello.hd; date: 2026-09-25T20:44:14.513Z -->

Source: `audit/probes/arch/runtime-lib/hello.hd`

| item | emitted | kept after remove-unused-module-elements |
| --- | --- | --- |
| defined functions | 29 | 2 |
| imports | 2 | 1 |
| globals | 9 | 0 |
| type definitions (text) | 49 | 31 |
| exports | 1 | 1 |
| wasm bytes | 2585 | 370 (binaryen -O2: 348) |

Exports: main

Imports: hd.panic, hd.console_byte

Unused imports: hd.panic

Unused functions (27): hd.provider_get, hd.provider_concat, hd.vector_get, hd.vector_set, hd.vector_append, hd.add_i32, hd.sub_i32, hd.mul_i32, hd.pow_i32, hd.neg_i32, hd.shl_i32, hd.shr_i32, hd.string_len, hd.string_concat, hd.i32_to_string, hd.char_to_string, hd.string_starts_with, hd.string_compare, hd.suspension_poll, hd.suspension_cancel, hd.suspension_result, hd.suspension_drive, hd.map_key_equal, hd.map_insert, hd.map_get, hd.map_remove, fv0

Unused globals (9): e0v0, e0v1, e0v2, hd.runtime-root, hd.driver-active, hd.host-call-site, hd.panic-integer-overflow, hd.panic-invalid-shift, hd.panic-index-out-of-bounds

All defined functions: hd.provider_get, hd.provider_concat, hd.vector_get, hd.vector_set, hd.vector_append, hd.add_i32, hd.sub_i32, hd.mul_i32, hd.pow_i32, hd.neg_i32, hd.shl_i32, hd.shr_i32, hd.string_len, hd.string_concat, hd.i32_to_string, hd.char_to_string, hd.string_starts_with, hd.string_compare, hd.suspension_poll, hd.suspension_cancel, hd.suspension_result, hd.suspension_drive, hd.map_key_equal, hd.map_insert, hd.map_get, hd.map_remove, hd.console_print, f0, fv0
