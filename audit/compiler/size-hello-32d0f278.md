# M4b Hello-World Size Breakdown

Status: measurement report, 2026-10-07, at `32d0f278`; no design change
is accepted by this report. It measures the M4b linker against
[codegen §12.8](../../future-work/compiler/codegen.md#128-size-versus-speed-policy)
and [wasm-layout §15.6](../../future-work/compiler/wasm-layout.md#156-the-2-kb-tiny-program).

## Reproduction

```sh
cd compiler
cargo run --release -p hd_cli -- build samples/hello -o /tmp/hello.wasm
cd ..
node compiler/bench/wasm-size.mjs /tmp/hello.wasm
```

The input is `println(42)`. The resulting module is **4,775 bytes** and
has SHA-256 `a86be6b002a272bdc220958a603b5d2c4ea8b6d494a5b76d2a104a88f69d0507`.
It prints `42` followed by a newline through V8.

The script is a deliberately small parser for the linker's current Wasm.
It reads section framing, function names, code bodies, padded direct-call
and `ref.func` relocations, exports and data segments. It is not a general
Wasm instruction disassembler.

## Sections

Encoded bytes include each section id and length field. The eight-byte
Wasm header is listed separately.

| Section | Bytes | Module share |
| --- | ---: | ---: |
| header | 8 | 0.2% |
| type | 119 | 2.5% |
| import | 93 | 1.9% |
| function | 33 | 0.7% |
| memory | 5 | 0.1% |
| global | 105 | 2.2% |
| export | 17 | 0.4% |
| element | 8 | 0.2% |
| data count | 3 | 0.1% |
| code | 3,222 | 67.5% |
| data | 161 | 3.4% |
| standard `name` custom section | 1,001 | 21.0% |
| **total** | **4,775** | **100.0%** |

The code section contains 3,183 function-body bytes and 39 bytes of
vector and body-length framing. The standard `name` section alone is
half of the 2 KB target.

## Every Function

Sizes are body payload bytes; imports have no body. “Panic path” means
the function is statically reachable although the successful hello run
does not execute it.

| Index | Function | Bytes | Why reachable |
| ---: | --- | ---: | --- |
| 0 | `hd:Console/write_line.finish` | 0 | `main` → `println` → Console host suspension poll |
| 1 | `hd:Console/write_line.start` | 0 | `main` → `println` → Console host suspension poll |
| 2 | `hd:rt/block` | 0 | `main` → `println` → reduced `block_on` Pending path |
| 3 | `hd:rt/stderr` | 0 | panic stubs reachable from formatting and `println` error paths |
| 4 | literal getter `""` | 25 | `signed_text`'s empty-text branch |
| 5 | literal getter newline | 25 | `println`/`PanicStr` newline path |
| 6 | literal getter `"-"` | 25 | `signed_text` negative-number branch |
| 7 | literal getter `"0"` | 25 | `digit_text` |
| 8 | literal getter `"1"` | 25 | `digit_text` |
| 9 | literal getter `"2"` | 25 | `digit_text` |
| 10 | literal getter `"3"` | 25 | `digit_text` |
| 11 | literal getter `"4"` | 25 | `digit_text` |
| 12 | literal getter `"5"` | 25 | `digit_text` |
| 13 | literal getter `"6"` | 25 | `digit_text` |
| 14 | literal getter `"7"` | 25 | `digit_text` |
| 15 | literal getter `"8"` | 25 | `digit_text` |
| 16 | literal getter `"9"` | 25 | `digit_text` |
| 17 | literal getter `"panic: "` | 25 | `println` → `PanicStr` error path |
| 18 | arithmetic-overflow message getter | 25 | `signed_text` checked-arithmetic panic path |
| 19 | division-by-zero message getter | 26 | signed/unsigned formatting division panic path |
| 20 | `println` Console-error message getter | 26 | `println` `.Err(ConsoleError)` path |
| 21 | `rt:StrToBuf` | 88 | Console host poll and every string panic stub |
| 22 | arithmetic-overflow panic stub | 28 | `signed_text` checked arithmetic |
| 23 | division-by-zero panic stub | 28 | signed and unsigned formatting division checks |
| 24 | `rt:PanicStr` | 68 | `println`'s Console-error path |
| 25 | Console `HostCold` | 21 | entry-created Console provider vtable |
| 26 | Console `HostPoll` | 144 | provider vtable → `write_line.start`/`.finish` |
| 27 | entry wrapper | 29 | exported `main`; installs the Console provider and calls hd `main` |
| 28 | `Display for i32.to_string` | 59 | `println(42)` resolves `Display[i32]` |
| 29 | `hello.main` | 14 | entry root |
| 30 | `std.console.println` | 464 | `hello.main` calls it |
| 31 | `std.format.signed_text` | 797 | `Display[i32].to_string` calls it |
| 32 | `std.format.digit_text` | 391 | signed and unsigned formatting convert each digit through it |
| 33 | `std.format.unsigned_text` | 625 | `signed_text` delegates magnitude formatting to it |

## Every Data Segment

| Segment | Mode | Payload bytes | Why reachable |
| ---: | --- | ---: | --- |
| 0 | passive | 154 | Literal getters for newline, `-`, digits, panic prefixes, arithmetic/division messages and the `println` Console-error message read this pooled segment. |

The payload is:

```text
\n-0123456789panic: panic: arithmetic-overflow: integer overflow\npanic: division-by-zero: division by zero\nprintln: write_line! returned .Err(ConsoleError)
```

## Five Biggest Contributors

| Rank | Contributor | Bytes | Classification | Why it is large / owning design |
| ---: | --- | ---: | --- | --- |
| 1 | standard `name` section | 1,001 | **implementation slip** | A release `hd build` emits dev names. The design requires compact `hd.names` and omits `name` in release ([wasm-layout §15.5](../../future-work/compiler/wasm-layout.md#155-panic-sites-and-backtraces)). |
| 2 | `std.format.signed_text` | 797 | **implementation slip** | The unoptimized generic body retains sign, boundary and panic branches for the constant `42`; specialization and dead-branch removal belong to [codegen §12.6](../../future-work/compiler/codegen.md#126-tiers-and-optimizations). |
| 3 | `std.format.unsigned_text` | 625 | **implementation slip** | The helper remains a separate full body with all division and overflow paths; trivial inlining, folding and scalar replacement have not run (§12.6). |
| 4 | `std.console.println` | 464 | **design issue** | The required std path carries provider dispatch, a cold host suspension, synchronous `block_on` and its failure path even for hello ([suspension §14.9](../../future-work/compiler/suspension.md#149-block_on)). |
| 5 | `std.format.digit_text` | 391 | **implementation slip** | Ten branches reach ten 25-byte literal getters. The design's short-literal constants and folding are not implemented ([wasm-layout §15.4](../../future-work/compiler/wasm-layout.md#154-globals-and-module-initialization)). |

The first four items total 2,887 bytes before their helper and literal
dependencies. The immediate priority is the release-name mistake, then
the missing optimization and short-literal paths; no tuning was performed.
