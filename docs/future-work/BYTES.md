# List Views For Binary Data

Status: proposal, revised 2026-10-29; domain: standard library collections and binary data; not accepted.

This proposal addresses [#839](https://github.com/HerringtonDarkholme/hd-lang/issues/839).
It extends the existing snapshot [`ListView[T]`](../../spec/std/collections.md#views) instead of
adding a byte-only type. The [checked package](bytes/) contains the proposed signatures and examples.

## Proposal

Give `ListView[T]` zero-copy range indexing, content equality when `T < Eq`, and explicit
`compact()`. Binary std APIs then use `ListView[u8]` for immutable buffers. `List[u8]` remains the
mutable and host-wire form.

```text
use std.collections.ListView
use std.digest.sha256

body := received.view(0, received.len())
part := body[header_end..part_end]
digest := sha256(&part)
saved := part.compact()
```

A sub-view resolves its range against the current view. `compact` copies only when unused elements
of the backing list would otherwise remain alive.

## Open questions

1. **Add sub-views, bounded `Eq`, and `compact`? Recommendation: yes.** They complete the generic
   snapshot type that already has the required three-word representation.
2. **Add `List.view_all()`? Recommendation: no initially.** `items.view(0, items.len())` is explicit
   at the uncommon mutable-to-readonly boundary; another method would only abbreviate it.
3. **Migrate binary APIs in stages? Recommendation: yes.** Start with digest and encoding, then move
   `Body`, multipart, and decompression after their accepted designs settle.

## Problems it solves

| Program | Today | Proposed |
| --- | --- | --- |
| Multipart parsing | Each returned payload range copies bytes | Parts are sub-views of one body |
| Chunked SHA-256 | Each input chunk is a copied `List[u8]` | `update` borrows a `ListView[u8]` |
| Binary outputs | Read-only data still exposes a growable list | APIs return an immutable view |
| Long-lived tiny part | A view keeps a large body alive | `compact` makes the release copy explicit |

## How it works

### Collection surface

`ListView[T]` already stores a copy-on-write `List[T]` snapshot plus `start` and `end`. The proposal
adds no field or runtime mechanism.

| Item | Purpose |
| --- | --- |
| `view[start..end]` | Return a sub-view with both bounds relative to `view` |
| `view[start..]`, `view[..end]` | Return the corresponding zero-copy suffix or prefix |
| `Eq for ListView[T] where T < Eq` | Compare selected elements, independent of storage and offsets |
| `compact()` | Reuse a full-buffer view or copy a partial view into right-sized storage |

Ranges use the existing `Index` and range types. Invalid and reversed bounds keep the existing
`index-out-of-bounds` panic. Empty ranges are valid. Existing `len`, element indexing, iteration, and
`to_list` remain unchanged. Full signatures are in [`bytes/src/list_view.hd`](bytes/src/list_view.hd).

At an API boundary, a list becomes a full view with `items.view(0, items.len())`. That expression
copies no element and makes the snapshot boundary visible, so the first version adds no helper.

### Binary std migration

| Area | Signatures that switch |
| --- | --- |
| Digest | `sha256`, `Sha256.hmac`, and `Sha256.update` take `&ListView[u8]`; `Digest.bytes` is a view |
| Encoding | `hex_encode` and `Base64.encode` take `&ListView[u8]`; byte decoders return a view |
| HTTP | `Body` stores a view; byte constructors accept a view; read-only byte access returns a view |
| Multipart (`std.http`) | File input takes a view; decoded part bodies become sub-views of the message body |
| Decompression proposal #949 | Decoders take `&ListView[u8]` and return a compact full view |

Host DTOs remain `List[u8]` because the ABI owns an exchange-buffer copy. `string.to_utf8` and its
inverse stay list-based to avoid a language-tier change. `Hasher.write` waits because neither measured
path needs that migration.

## Measurement and cost

The ceiling probes built release Wasm at `0c71c705f` and ran fresh Node processes. Each cell is the
median of 15 interleaved base/view runs after two discarded runs; stdout matched. Pre-run load was
11.93/21.34/19.12.

| Path | Removed copy work | Default V8 | TurboFan only | Liftoff only |
| --- | ---: | ---: | ---: | ---: |
| 256 KiB multipart, 64 × 4 KiB parts, 600 passes | 38,400 lists; 157.3 MB | 268 → 100 ms (−62.8%) | 280 → 105 ms (−62.4%) | 671 → 218 ms (−67.5%) |
| 64 KiB SHA-256, 64 × 1 KiB chunks, 400 passes | 25,600 lists; 26.2 MB | 332 → 295 ms (−11.0%) | 375 → 324 ms (−13.4%) | 933 → 870 ms (−6.8%) |

| Component | Cost |
| --- | --- |
| Representation | Unchanged: one list handle and two `usize` offsets, 12 bytes on wasm32 |
| Sub-view | Bounds checks and one shared handle retain; no element copy |
| Equality | Linear in the shorter selected run, with a length check first |
| Compact | Constant time for a full view; linear in selected elements for a partial view |
| Probe Wasm | Multipart 5,714 → 5,569 bytes; digest 12,877 → 13,052 bytes |
| Compile and web size | Design only; measure cold compile and the 2 MB gzip cap during implementation |

The probes used `&List[u8]` plus bounds. They are ceiling hacks for the same copy removal, not an
implementation of `ListView` sub-ranges.

## Appendix A: Stress test

### Multipart upload

Sixty-four returned parts sub-view one message snapshot. A retained file part calls `compact` before
the request is released. This exposes the adverse case: a one-byte view can otherwise retain a
multi-megabyte upload.

### Chunked digest

Each SHA-256 update borrows one sub-view. The same call accepts a full view without another overload.
Hexadecimal and Base64 encoders have the same shape.

### Snapshot and range edges

- A source-list write after `view` does not change the view; existing copy-on-write provides this.
- A nested range uses the outer view's length and offsets, never the backing list's coordinates.
- Empty prefix, suffix, and middle ranges are valid; reversed or out-of-range bounds panic first.
- Equality compares contents across different buffers and offsets and short-circuits unequal lengths.
- `compact` of a full view returns the same snapshot; a partial view gets a right-sized list snapshot.
- `to_list` remains the explicit mutable or host-wire conversion.
- A borrowed `&ListView[u8]` cannot escape; an owned multipart view can outlive its parsing call.

The checked examples cover all three range forms, compaction, iteration, indexing, equality, and
host conversion.

## Appendix B: Prior art

### Rust slices

```rust
fn checksum(bytes: &[u8]) -> u64 {
    bytes[2..6].iter().map(|byte| u64::from(*byte)).sum()
}
```

Rust ranges produce sub-slices and slice equality compares elements. A slice is borrowed, while hd's
`ListView` is an owned snapshot that may escape.
[Rust slice documentation](https://doc.rust-lang.org/std/primitive.slice.html).

### Swift `ArraySlice`

```swift
let packet = Array(0..<1024)
let payload = packet[16..<packet.endIndex]
let saved = Array(payload)
```

Swift slices share array storage and preserve indices; converting to `Array` releases retained
storage. Hd uses relative indices and names the release operation `compact`.
[Swift `ArraySlice`](https://developer.apple.com/documentation/swift/arrayslice).

### Go slices

```go
body := []byte("headerpayload")
payload := body[6:]
payload[0] = 'P' // also changes body
```

Go sub-slices share mutable storage. Hd takes the compact range operation but keeps snapshot
semantics, avoiding action at a distance. [Go slice expressions](https://go.dev/ref/spec#Slice_expressions).

### Rust `bytes::Bytes`

```rust
let body = bytes::Bytes::from_static(b"headerpayload");
let payload = body.slice(6..);
```

Rust's networking type is cheaply cloned and sliced. Hd no longer needs a separate byte type because
generic `ListView[T]` has that representation and behavior.
[Rust `bytes::Bytes`](https://docs.rs/bytes/latest/bytes/struct.Bytes.html).

## Appendix C: Alternatives considered

| Alternative | Rejected because |
| --- | --- |
| New `std.bytes.Bytes` | `ListView[u8]` now has the same snapshot representation without a second view concept |
| Keep copied `List[u8]` ranges | The probes leave 6.8–67.5% available from range-copy removal |
| Pass `&List[u8]`, start, end | Local references cannot be stored or returned, and loose bounds lose the invariant |
| Add `List.view_all()` now | It abbreviates one explicit, zero-copy boundary expression but adds no behavior |
| Add a primitive byte slice | Ordinary std storage clears the bar without syntax, intrinsics, or runtime layout |
| Use segmented buffers | The motivating codecs and host exchange still need contiguous bytes |

## Appendix D: Today in hd

[`ListView[T]`](../../spec/std/collections.md#views) is already a copied snapshot of a list value. It
shares storage, ignores later source-list writes, supports length, element indexing, iteration, and
`to_list`, and keeps the full backing list alive. It has no sub-view, equality, or conditional
compaction operation today.

`std.http.Body`, `std.digest`, and `std.encoding` expose `List[u8]`. The shipped `std.http`
multipart parser copies parsed payloads, and the #949 decompression proposal uses lists at its
public boundary. This proposal changes those binary surfaces and drops no list or text API.

## Appendix E: Details and phases

- **Tier:** ordinary stdlib additions in `spec/std/collections.md` and affected std chapters.
- **Ownership:** views remain copy-on-write snapshots; sub-views retain storage, and `compact` may copy.
- **Panic:** range failures keep `index-out-of-bounds`; no new panic category or diagnostic is needed.
- **Host ABI:** unchanged; host request and response DTOs keep owned lists.
- **Footprint:** implementation measures hello identity, each migrated API, and web-compiler gzip.
- **Tests:** nested ranges, bounds, empty views, COW snapshots, cross-buffer equality, and compaction.
- **Phase 1:** add range indexing, bounded equality, compact, spec rules, and conformance fixtures.
- **Phase 2:** migrate digest and encoding, rerun vectors, probes, compile time, and size measurements.
- **Phase 3:** migrate `Body`, multipart, and decompression, then run host parity and read-once cases.
- **Rollback:** remove the three additions and restore copied binary boundaries; existing views remain.

## Parse log

The proposal's one `text` block parses with `scripts/hd-conformance.sh parse` on 2026-10-29. Parsing
checks syntax only; `hd check`, `hd lint`, and `hd fmt --check` validate the package.
