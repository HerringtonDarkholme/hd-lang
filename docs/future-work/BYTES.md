# Immutable Bytes

Status: design proposal, 2026-10-28; domain: standard library binary data; not accepted.

This proposal addresses [#839](https://github.com/HerringtonDarkholme/hd-lang/issues/839).
It adds `std.bytes.Bytes`, an immutable value snapshot with zero-copy slices.
The [checked package](bytes/) contains every proposed signature and realistic usage.

## Proposal

`Bytes` stores a shared `List[u8]` value plus a start and end offset.
Creating it from a list shares the list's copy-on-write buffer.
Slicing changes only the offsets; `compact` is the explicit copy that releases unused backing bytes.
Read-only calls take `&Bytes`, while `to_list` is the explicit mutable conversion.

```text
use std.bytes.Bytes
use std.digest.sha256

body := Bytes::from_list(received)
part := body[header_end..part_end]
digest := sha256(&part)
saved := part.compact()
```

## Open questions

1. Approve the three-word `std.bytes.Bytes` value over `List[u8]`? **Recommended: yes**, because it needs no compiler or runtime mechanism.
2. Approve zero-copy slices plus explicit `compact` for the tiny-slice-retains-large-buffer case? **Recommended: yes**, matching Rust `bytes::Bytes` while making retention visible.
3. Switch the measured HTTP, compression, digest, and encoding APIs together? **Recommended: yes**, but leave host wire DTOs, `string.to_utf8`, and `Hasher.write` unchanged in the first pass.

## Problems it solves

| Today | With this proposal |
| --- | --- |
| A `List[u8]` range creates and fills a new list | A `Bytes` range is a shared view with two new offsets |
| Multipart parsing copies every returned part | Each `MultipartPart.body` views the received body; callers compact only retained parts |
| Chunked SHA-256 callers copy every input chunk | `Sha256.update(&Bytes)` reads each chunk in place |
| Binary APIs expose a growable list even when they never mutate it | Inputs borrow `&Bytes`; outputs return immutable `Bytes` |
| A tiny view can retain a large message accidentally | `view.compact()` makes the retention-ending copy explicit |

## How it works

### Representation and behavior

`Bytes` has private `storage: List[u8]`, `start: usize`, and `end: usize` fields.
The storage value is a snapshot: a later write through another list value follows existing list copy-on-write rules.
Copies of `Bytes` share storage, and nested slices add checked offsets without copying bytes.

Indexing and slicing use the existing `Index` and range types.
An invalid byte index or range has the existing `index-out-of-bounds` panic.
`Eq` compares selected bytes, not the storage identity or offsets.
Iteration yields the selected bytes in order through `BytesIter`.

`to_list` returns an independent mutable list value of the selected bytes.
A full-range conversion may share its copy-on-write buffer until mutation; a partial view copies its range.
`compact` returns the same view when it already covers its buffer, otherwise it copies into a right-sized buffer.

### Public surface

| Item | Purpose |
| --- | --- |
| `Bytes::from_list` | snapshot a mutable list without copying its full buffer |
| `len`, `is_empty`, `get`, byte indexing | inspect a view through ordinary read operations |
| range indexing | return a zero-copy `Bytes` subview |
| `to_list` | cross an API boundary that needs a mutable list |
| `compact` | copy a retained subview into a right-sized backing buffer |
| `Iterable[u8]`, `Eq` | ordinary byte traversal and content equality |

Full signatures are in [`bytes/src/bytes.hd`](bytes/src/bytes.hd).
The existing `std.text.Bytes` string cursor keeps its name; module qualification separates it from `std.bytes.Bytes`, as Rust separates `str::Bytes` and `bytes::Bytes`.

### Standard-library migration

| Area | Signatures that switch |
| --- | --- |
| HTTP | `Body` stores `Bytes`; `from_bytes`, `bytes`, and request/response byte setters use `Bytes` |
| Multipart proposal #948 | `Multipart.file` takes `Bytes`; decoded part bodies are zero-copy views of the message body |
| Compression proposal #949 | `gzip_decode` and `zlib_decode` take `&Bytes` and return `Bytes` |
| Digest | `sha256`, `Sha256.hmac`, and `Sha256.update` take `&Bytes`; `Digest.bytes` is `Bytes` |
| Encoding | `hex_encode` and `Base64.encode` take `&Bytes`; decoders return `Bytes` |

Host DTOs remain `List[u8]` because the ABI already copies into an exchange buffer.
`string.to_utf8` and the `string_from_bytes` intrinsic remain list-based to avoid a language-tier change.
`Hasher.write` waits because changing every custom hasher was not part of either measured path.

## Measurement and cost

The ceiling probes built release Wasm on `0c71c705f` and ran fresh Node processes.
Each cell is the median of 15 interleaved base/view runs after two discarded runs; stdout matched every time.
The machine load before the run was 11.93/21.34/19.12.

| Path | Removed copy work | Default V8 | TurboFan only | Liftoff only |
| --- | ---: | ---: | ---: | ---: |
| 256 KiB multipart body, 64 × 4 KiB parts, 600 passes | 38,400 lists; 157.3 MB | 268 → 100 ms (−62.8%) | 280 → 105 ms (−62.4%) | 671 → 218 ms (−67.5%) |
| 64 KiB SHA-256, 64 × 1 KiB chunks, 400 passes | 25,600 lists; 26.2 MB | 332 → 295 ms (−11.0%) | 375 → 324 ms (−13.4%) | 933 → 870 ms (−6.8%) |

| Component | Cost |
| --- | --- |
| Value layout | one counted list handle and two `usize` offsets; 12 bytes on wasm32 |
| Slice | bounds checks plus one shared handle retain; no byte copy or new byte buffer |
| Compact/list conversion | linear in selected bytes when the view does not cover its buffer |
| Probe Wasm | multipart 5,714 → 5,569 bytes; digest 12,877 → 13,052 bytes |
| Compile time | design only; measure cold base/head compile during implementation |
| Web compiler | design only; implementation must remain below the 2 MB gzip cap |
| Unused program | expected 0 linked bytes; verify byte identity during implementation |

The probes are ceiling hacks, not the proposed implementation.
They replace copied slices with `&List[u8]` plus bounds, the same work a `Bytes` view removes.

## Appendix A: Stress test

The multipart case translates #948's buffered parser.
Sixty-four returned parts share one message buffer, and a long-lived file part calls `compact` before the request is released.
This found the main adverse case: a 1-byte view can otherwise retain a multi-megabyte upload.

The digest case translates chunked SHA-256.
Each update borrows a subview, so a caller can hash the whole value or ranges without allocating chunk lists.
The same shape covers hexadecimal and Base64 encoders.

Further edge cases shape the proposal:

- Mutating the source `List[u8]` after `from_list` must not change `Bytes`; existing list copy-on-write provides the snapshot.
- A slice of a slice resolves offsets against the selected view, not the original list.
- Empty ranges are valid; reversed or out-of-range bounds panic before constructing a view.
- `Bytes::from_list([])` is the single empty representation exposed by the API.
- Equality compares contents across different buffers and different offsets.
- `compact` of an already compact value allocates nothing.
- A host send converts explicitly to `List[u8]`; the host owns the boundary copy and no `Bytes` view crosses it.
- A borrowed `&Bytes` cannot escape, while an owned `Bytes` part can outlive the parsing call.

The checked examples cover multipart slicing, a borrowed checksum, iteration, indexing, equality, compaction, and host conversion.

## Appendix B: Prior art

### Rust `bytes::Bytes`

```rust
use bytes::Bytes;

let body = Bytes::from_static(b"headerpayload");
let payload = body.slice(6..);
assert_eq!(payload, b"payload"[..]);
```

Rust's networking-oriented `Bytes` is cheaply cloneable and sliceable, with shared backing memory.
hd takes that owned immutable shape, but uses ordinary `List` copy-on-write instead of dynamic storage implementations.
[Rust `bytes::Bytes`](https://docs.rs/bytes/latest/bytes/struct.Bytes.html).

### Rust borrowed slices

```rust
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().map(|byte| u64::from(*byte)).sum()
}
```

Rust represents a slice as a pointer and length, and distinguishes shared `&[T]` from mutable `&mut [T]`.
hd follows the shared-call convention with `&Bytes`; owned `Bytes` covers values that escape a call.
[Rust slice documentation](https://doc.rust-lang.org/std/primitive.slice.html).

### Go slices

```go
body := []byte("headerpayload")
payload := body[6:]
payload[0] = 'P' // also changes body
```

Go slicing returns a view of the same underlying array, including mutable aliasing.
hd takes its compact offset view but rejects mutable aliases: `Bytes` is immutable, and mutable conversion is explicit.
[Go slice expressions](https://go.dev/ref/spec#Slice_expressions).

## Appendix C: Alternatives considered

**Keep `List[u8]`.** This has no new type, but the two ceiling probes show 6.8–67.5% available wins from removing range copies.

**Use `ListView[u8]`.** It refers to a mutable list place, sees same-length element writes, and panics after a length change.
Binary message values need a stable snapshot that can outlive the source place.

**Pass `&List[u8]`, start, and end everywhere.** This is the ceiling hack and is ideal inside one call.
It cannot be stored or returned, and three loose parameters lose the selected-range invariant.

**Add a primitive byte-slice type or intrinsic.** That could reduce the handle below three words.
The ordinary-stdlib representation already clears the bar, so compiler knowledge has no justification.

**Use a segmented buffer.** It avoids some concatenation but makes indexing, codecs, and host exchange more complex.
The motivating paths all need contiguous bytes.

## Appendix D: Today in hd

[`List[u8]`](../compiler/lowering-catalog.md#bytes-listu8) stores bytes inline in a counted growable buffer.
Language [list slicing](../../spec/lang/05-expressions.md#slicing) creates a new list and copies selected elements.
[`ListView[T]`](../../spec/std/collections.md#views) is a live view of a mutable list place, not a snapshot.

`std.http.Body`, `std.digest`, and `std.encoding` expose `List[u8]` throughout.
The #948 multipart proposal copies each parsed payload into a new `Body`.
The #949 decompression proposal takes and returns `List[u8]`.
This proposal changes those binary surfaces and drops no text or list API.

## Appendix E: Implementation phases

1. Add `std.bytes.Bytes`, range indexing, iteration, equality, and focused conformance for snapshot, bounds, nested slices, and compaction.
2. Migrate digest and encoding; repeat the two ceiling probes with the real type and test exact known vectors.
3. Migrate HTTP `Body` and the accepted multipart/compression APIs; test read-once state and zero-copy part offsets.
4. Measure all program Wasm sizes, hello byte identity, cold JSON compile time, and web-compiler gzip before review.

The type is stdlib-tier ordinary hd.
It adds no syntax, intrinsic, lang item, capability, host ABI, suspension rule, or compiler cache entry.
If accepted, behavior belongs in `spec/std/bytes.md`, with stdlib-tier fixtures and updates to the affected std chapters.

## Parse log

`scripts/hd-conformance.sh parse` parses the proposal's `text` block.
This validates syntax only; `hd check`, `hd lint`, and `hd fmt --check` validate the package.
