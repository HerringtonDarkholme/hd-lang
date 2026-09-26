<!-- commit bd985d7; command: node --experimental-strip-types audit/scripts/arch/rtlib-imports.ts; date: 2026-09-25T20:44:40.539Z -->

Modules compiled: 303. Host-provider imports are normalized as host_T_M_* (T = trait index, M = method index).

| import | signature | modules importing it | example |
| --- | --- | --- | --- |
| hd.panic | `(param i32)` | 303 | spec/conformance/parse/valid/doc-comments.hd |
| hd.trace | `(param i32 i32)` | 88 | spec/conformance/parse/valid/test-block.hd |
| hd.pending | `(param i32 i32) (result i32)` | 88 | spec/conformance/parse/valid/test-block.hd |
| hd.console_byte | `(param externref i32)` | 8 | spec/conformance/parse/valid/layout.hd |
| hd.host_T_M_begin | `(param externref i32) (result externref)` | 6 | spec/conformance/runtime/valid/cancellation-runs-defer.hd |
| hd.host_T_M_poll | `(param externref) (result i32)` | 6 | spec/conformance/runtime/valid/cancellation-runs-defer.hd |
| hd.host_T_M_cancel | `(param externref)` | 6 | spec/conformance/runtime/valid/cancellation-runs-defer.hd |
| hd.string_transform_begin | `(param i32)` | 3 | spec/conformance/typing/valid/collections.hd |
| hd.string_transform_input | `(param i32)` | 3 | spec/conformance/typing/valid/collections.hd |
| hd.string_transform_output | `(param i32) (result i32)` | 3 | spec/conformance/typing/valid/collections.hd |
| hd.host_T_M_result | `(param externref) (result i32)` | 2 | test/fixtures/suspension/33-host-provider-scalar-arguments-and-results.hd |
| hd.format_f64 | `(param f64 i32) (result i32)` | 1 | spec/conformance/runtime/valid/float-display.hd |
| hd.pow_f64 | `(param f64 f64) (result f64)` | 1 | test/fixtures/compiler/06-floating-power-uses-the-host-ieee-pow-primitive.hd |
| hd.host_T_M_argument_byte | `(param externref i32 i32 i32)` | 1 | test/fixtures/suspension/36-host-provider-strings-use-utf8-boundary.hd |
| hd.host_T_M_result_length | `(param externref) (result i32)` | 1 | test/fixtures/suspension/36-host-provider-strings-use-utf8-boundary.hd |
| hd.host_T_M_result_byte | `(param externref i32) (result i32)` | 1 | test/fixtures/suspension/36-host-provider-strings-use-utf8-boundary.hd |
