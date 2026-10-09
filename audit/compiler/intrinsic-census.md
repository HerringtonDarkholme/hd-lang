# Census Of Unimplemented Runtime Intrinsics (R11)

Method: the 18 R2 §E programs that compile and panic with
`explicit-panic: intrinsic`. Each was traced to its stub: small
repro probes through `hd run` in a scratch package (cheap: ~0.1 s
each), else the std source chain (`pop` → `list_truncate`,
`trim` → `bytes_*`, `parse` → `host_parse_f64` — all one-hop calls
to `panic("intrinsic")` bodies). Reference: the emitter's
`call_intrinsic` (`hd_wasm/src/emit.rs`) handles exactly 7 keys
(`block_on`, `bytes_len`, `char_scalar`, `entry_write`, `panic`,
`panic_message`, `task_race_frame`); every key below misses it.
`panic`/`panic_with` is the panic mechanism itself, not a gap, and
the bare `@intrinsic` operator/cmp methods lower through `Prim` —
neither appears here. Read only.

| Intrinsic key(s) | std function | Cases that hit it | What it must do (spec rule) | Task group |
| --- | --- | --- | --- | --- |
| `host_lower`, `string_upper` | `host_lower`, `host_upper` (text.hd) | empty-string-operations (`.lower()`), multibyte-scalar-strings (non-ASCII `.lower()` ×2), replace-empty-old (`.lower()` on U+FEFF), string-more-methods (`.upper()`), string-trim-and-lower (`.lower()`) | Full Unicode case mappings (std-text case rules; `"straße".upper()` → `"STRASSE"`, `"İ".lower()` → `"i̇"`) | Unicode case tables (host) |
| `parse_f64` | `host_parse_f64` (num.hd:515) | num-parse-f64-round-trip, num-parse-f64-specials, num-parse-f64-values | Correctly rounded decimal→f64, ties to even, with sign/range/specials (std-num.parse-f64.*) | Decimal parsing (host or Wasm) |
| `format_f64` (, `format_f32`) | `f64_text` (format.hd:482) | display-tuples (`2.5` element), signed-zero-and-infinity-through-generics (both tests: `-0.0`, infinities), generic-data-let-pattern (`1.5`), generic-data-match-pattern, generic-data-pattern-in-generic-function (`2.5`), interpolation-display-order (`1.5`) | Shortest round-trip float text incl. NaN/inf/`-0.0` (float Display; "the host writes a float's text") | Float formatting (host) |
| `format_f64_fixed` | `host_format_f64_fixed` (num.hd:522) | num-to-fixed (all 5 tests) | Correctly rounded fixed-point text, ties to even, 0–100 digits, NaN/inf/`-0.0` spellings (fixed-point primitive) | Fixed-point formatting (host or Wasm) |
| `dbg`, `dbg_text`, `dbg_write` | `dbg` (format.hd:623) | dbg-prints-void (panics on entry) | Print each value's debug text with source position; return `void` (module.dbg.*) | Debug-printing body (checker swaps it in; emitter gives it) |
| `list_truncate` | `list_truncate` (collections.hd:804) | list-pop (`pop`), list-insert-remove-clear (`remove_at`, `clear`) | Shrink to first `len` elements, bounds pre-checked by callers | Length truncation lowering |
| `string_from_bytes`, `char_from_scalar` (, `bytes_concat`, `string_upper` pathway) | text.hd stubs | No failing case in this census reaches them directly (byte readers/writers around them all work: `len`/`starts_with`/`split`/`replace`/`trim`/`chars` verified live) | UTF-8 validation views (string primitives) | Same byte-primitive task when reached |

Notes for grouping (one compiler task per group):

- The string byte primitives (`bytes_len`, `bytes_at`, `bytes_slice`,
  `bytes_concat`, `bytes_match`/`byte_find`/`scalar_at` over them) all
  work — verified live (`trim`, `split`, `replace`, `starts_with`,
  `contains`, `chars`, multibyte `len`). Do not queue byte-primitive
  work from these cases; the string group is case conversion only.
- `char_scalar` is implemented (emitter key; multibyte char Display
  verified live). Not a gap.
- `f64` interpolation of a literal is a separate compile-time
  `unsupported` (`to_string at f64`); the runtime census above covers
  values that reach `f64_text` through generic/template dispatch.
- `task_race_frame` / combinator frames and `facts_of` have stubs but
  no failing case in this census (suspension cases fail as Wasm
  traps instead — see R12).
