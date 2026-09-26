# F-400: Strings that start with U+FEFF lose it at the host boundary
Severity: major
Area: correctness
Evidence: audit/evidence/04-runtime/bom-host-boundary.log; audit/evidence/04-runtime/host-values.tsv (row `str-bom.hd`: live run `assertion-failed`)
Effect: `join!("\u{FEFF}", "x").len()` under `--profile ready-text` returns 1, expected 2. `join!("a", "\u{FEFF}x")` returns 2, expected 3. A U+FEFF in the middle survives. The recorded history already holds the stripped argument (`utf8: ""`). A sidecar result edited to `efbbbf78` replays as length 1. `"\u{FEFF}A".lower().len()` returns 1, expected 2, while `"\u{FEFF}A".len()` returns 2. Host providers and the string bridge silently change user text. Spec 04 says strings are UTF-8 at every Wasm host boundary.
Recommendation: implementation change: create every boundary `TextDecoder` with `ignoreBOM: true`, including the replay decoder, and add a portable case with a leading U+FEFF argument.

Mechanism: `new TextDecoder("utf-8", { fatal: true })` strips a leading BOM by default (`node -e` check in the log). src/compiler.ts uses it for provider arguments, `decodeHostValue`, and `string_transform_output`. The U+FEFF half of F-355 (`trim`) may have this cause rather than JavaScript `trim`.
