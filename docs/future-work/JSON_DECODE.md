# JSON Decode: One Pass With A Key Loop

Status: proposal, 2026-11-17; domain: std.json and std.serde reading; not
accepted. It follows the owner's rule (a) on #1809 (2026-11-17):
`deserialize` may run before the end of the text is validated. Under
review: [std-json.decode.direct](../../spec/std/json.md#r-std-json.decode.direct),
[std-json.decode.parse-error](../../spec/std/json.md#r-std-json.decode.parse-error),
[std-serde.derive.deserialize.data-and-enum-forms](../../spec/std/serde.md#r-std-serde.derive.deserialize.data-and-enum-forms)
and the [Reading table](../../spec/std/serde.md#reading).

## The Proposal

`decode` reads the text once, in order, with no index and no fallback.
A derived `Deserialize` takes a record's keys in whatever order the text
has them: the reader names each key's member (serde's `__Field` match),
the walk fills that member's slot, an unknown key's value is skipped, and
a member whose key never came is read as absent. Only a failed read
validates the whole text, so a syntax error still wins over a typed error.

```hd
@derive(Deserialize)
struct Order:
    id: i64
    note: string?
    lines: List[Line]

# Every one of these decodes, in one pass over the text:
decode::[Order]("{\"id\":1,\"note\":null,\"lines\":[]}")   # declaration order
decode::[Order]("{\"lines\":[],\"id\":1}")                 # any order, `note` absent
decode::[Order]("{\"v\":2,\"id\":1,\"lines\":[]}")         # unknown key skipped
```

A format opts in with one method, `keyed() -> true`, and then implements
`next_field` and `absent_field`. Every other format keeps today's pull by
name through `member(m)`. The signatures and a hand-written example are in
[json-decode/](json-decode/src/json_decode.hd).

## Open Questions

1. Adopt the key loop (V3) as `decode`'s only reader and delete the indexed
   `TextReader`? **Recommendation:** yes; it is faster than today in 26
   of 27 cells, and its Wasm grows 7 to 10% where V2's grows 18 to 41%.
2. Name match: a per-type wire-name table, made once per type per decode
   and keyed by a compiler-assigned id on `Members` (needs approval: one
   `usize` per member directory)? **Recommendation:** yes. A scan is O(F)
   per out-of-order key: 3.1x today's time on a shuffled 64-field struct,
   where the table is 19% faster than today (Appendix A).
3. A variant object with a second key (`{"A":{},"A":{}}`) becomes a
   `WrongType` at the variant, as in serde_json, instead of duplicate-last?
   **Recommendation:** yes; one pass cannot reread the first payload.
4. A repeated record key keeps duplicate-last (today), not serde's
   `DuplicateField`? **Recommendation:** keep duplicate-last; it matches
   `parse` and `from_json`, and costs nothing in the key loop.
5. Add `skip()` to `Deserializer` (serde's `IgnoredAny`), so a hand-written
   key loop can drop an unknown key's value without building a `Json`?
   **Recommendation:** yes.
6. With two typed errors in one document, report the first in text order
   (serde_json), not the first in declaration order? **Recommendation:**
   yes; a one-pass reader sees them in text order, and two conformance
   fixtures change (Appendix A).

## Problems It Solves

| Program | Today (two passes) | Lane 160's patch (V2) | Proposal (V3) |
| --- | --- | --- | --- |
| GitHub-like issues, 96 KB, keys shuffled | validate and index, then read | in-order pass fails at the first out-of-order key, then today's reader again | one pass; 504 µs today, 510 V2, 461 V3 (-9%) |
| Same document with unknown keys (237 KB) | indexes every skipped value | falls back | skips each value once; 863 µs today, 898 V2, 781 V3 (-10%) |
| A program with 20 derived types | one reader per type | two readers per type: 495,750 B (+41%) | one reader per type: 387,774 B (+10%) |
| `{"Pair":[1]}` into a tuple variant | `WrongType("$.Pair", "array of 2 items")` | same, after two passes | same, from the one pass |

## How It Works

- **Reader.** `StreamReader` holds the `Parser` cursor and a stack of
  frames in reused parallel lists (kind, first-slot flag, item count, path
  step, variant name). It checks every byte it passes with `parse`'s
  routines, so a value it returns is a value `parse` accepts.
- **Key loop.** `next_field(members, facts, guess)` reads the next key
  and returns its member's index. `guess` is the member after the last one
  named, checked in the text first (`Parser.key_is`, no copy), so
  declaration order costs one comparison per key. On a miss, a type of up
  to 8 members is scanned by name length; a larger one looks the key up in
  its type's wire-name table, made once per type per decode. An unknown
  key's value is validated and skipped.
- **Slots.** The derived `Source` marks each named member in a `u64` (a
  list past 64 members). After the last key it offers each unseen
  unskipped member once with `absent_field`, so `T?` reads `.None`, a
  declared default applies, and anything else is `MissingField` at the
  member's path. A repeated key reads again and its last value wins.
- **Errors.** A failed or unfinished read validates the whole text once.
  A syntax error found there is returned; otherwise the reader's typed
  error is, with the same path today's reader reports.
- **Hand-written code.** A format (`Deserializer`) writes nothing new:
  `keyed()` defaults to `false`, and derived code then pulls by name as
  today. A hand-written `Deserialize` for a record stops calling
  `begin_data` and `member(m)`, which the JSON reader would only honour for
  keys in declaration order. It writes serde's loop instead: `begin_map`,
  `next_key`, one slot per member, `skip()` for an unknown key
  ([example](json-decode/src/examples.hd)). No std type does this today.
- **Values no read consumed.** A hand-written `deserialize` that only
  peeks leaves its value due; the next slot or close skips it, as today's
  indexed reader does.

## Cost

Per document: O(text bytes) for the read, plus O(F) per object for the
slot bits and the absent walk (F = members of its type). The name match is
O(key bytes) per key with the per-type table (question 2), made once per
type per decode. The experiment's length-filtered scan is O(F) per
out-of-order key, so O(F²) per object; the table removes that above 8
members. No step scans all fields per key.

| Component | Change |
| --- | --- |
| Code removed | `TextReader` and `TextReadFrame` (about 760 lines of lib/std/json.hd); `validate`'s index writing (about 80 lines) |
| Code added | `StreamReader` with the name table (about 520 lines); keyed `DeserializeSource` (about 100 lines in lib/std/serde.hd) |
| Decode time | V3 is faster than today in 26 of 27 cells (-1% to -22%; +3% on reversed orders); V2 is slower than today on 17 of 18 cells whose keys are not in declaration order |
| Wasm size, decode program | 78,679 B today; V2 93,037 (+18%); V3 73,168 (-7%) with the scan, 83,945 (+7%) with the name table |
| Wasm size, 20 derived types | 351,537 B today; V2 495,750 (+41%); V3 377,461 (+7%) with the scan, 387,774 (+10%) with the name table |
| `hd build --release`, 20 types | 1.07 s today, 2.10 s V2, 1.06 s V3 (median of 5, fresh cache each; times fall on about 1.06 s steps) |
| Web compiler gzip | std text only; no compiler change except question 2 |
| Error path | one extra validation of the whole text, only for a failed decode |

Method: release builds (`hd build --release`), Node's default V8 tier
(Node 24.19), each program self-timed with `std.time.monotonic` in
batches of at least 150 ms; variants run alternately, 2 rounds of 7
batches, median of the 14. Corpus: [Appendix A](#a-stress-test).

## A. Stress Test

The corpus is generated in hd, linearly, from a fixed seed (no literal
documents). Shapes: the serde-throughput orders; a GitHub-API-like issue
list (nested `User`, a list of `Label`, optional `assignee`, `closed_at`,
`body`); a `package.json`-like list (nested `Person` and `Repository`,
three `Map[string, string]`). Orderings per object: declaration order,
shuffled, reversed, an unknown key with a nested value after every second
field, and optional fields dropped.

Median microseconds per decode, and MB/s, by variant (V1 today, V2 lane
160's patch, V3 this proposal):

| Shape | Order | Bytes | V1 µs (MB/s) | V2 µs (MB/s) | V3 µs (MB/s) | V3 vs V1 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| package | declaration | 1,207 | 7.7 (158) | 6.5 (187) | 6.2 (193) | -19% |
| package | shuffled | 1,207 | 8.2 (148) | 8.7 (139) | 6.9 (175) | -16% |
| package | declaration | 96,566 | 516 (187) | 460 (210) | 420 (230) | -19% |
| package | shuffled | 96,566 | 605 (159) | 621 (155) | 517 (187) | -15% |
| package | reversed | 96,566 | 541 (178) | 558 (173) | 485 (199) | -10% |
| package | unknown keys | 193,906 | 922 (210) | 961 (202) | 721 (269) | -22% |
| package | optional missing | 72,826 | 469 (155) | 484 (150) | 389 (187) | -17% |
| package | declaration | 10,120,181 | 52,875 (191) | 46,750 (216) | 43,375 (233) | -18% |
| package | shuffled | 10,120,181 | 63,000 (161) | 64,750 (156) | 51,500 (197) | -18% |
| order | declaration | 1,105 | 8.9 (124) | 7.3 (151) | 8.0 (138) | -10% |
| order | shuffled | 1,105 | 9.5 (116) | 9.9 (112) | 9.3 (118) | -2% |
| order | declaration | 103,026 | 705 (146) | 609 (169) | 674 (153) | -4% |
| order | shuffled | 103,026 | 834 (124) | 869 (119) | 824 (125) | -1% |
| order | reversed | 103,026 | 732 (141) | 758 (136) | 754 (137) | +3% |
| order | unknown keys | 313,626 | 1,238 (253) | 1,258 (249) | 1,234 (254) | -0% |
| order | optional missing | 95,976 | 688 (140) | 715 (134) | 682 (141) | -1% |
| order | declaration | 10,751,951 | 70,375 (153) | 62,000 (173) | 67,500 (159) | -4% |
| order | shuffled | 10,751,951 | 83,750 (128) | 86,250 (125) | 82,500 (130) | -1% |
| issue | declaration | 784 | 3.9 (199) | 2.9 (270) | 3.2 (241) | -17% |
| issue | shuffled | 784 | 4.0 (194) | 4.3 (181) | 3.9 (204) | -5% |
| issue | declaration | 96,074 | 418 (230) | 314 (306) | 333 (289) | -20% |
| issue | shuffled | 96,074 | 504 (191) | 510 (188) | 461 (208) | -9% |
| issue | reversed | 96,074 | 428 (225) | 432 (223) | 418 (230) | -2% |
| issue | unknown keys | 237,194 | 863 (275) | 898 (264) | 781 (304) | -10% |
| issue | optional missing | 55,877 | 313 (178) | 315 (177) | 272 (205) | -13% |
| issue | declaration | 9,893,877 | 41,250 (240) | 31,500 (314) | 33,438 (296) | -19% |
| issue | shuffled | 9,893,877 | 51,875 (191) | 51,625 (192) | 45,000 (220) | -13% |

V3 in this table matches names by trying the expected member, then
scanning by length (the issue and package types have 14 and 12 fields). A
64-field struct (102 KB, 100 objects) shows why the
proposal needs the name table (question 2):

| Key order | Today (µs) | V3, scan (µs) | V3, name table (µs) |
| --- | ---: | ---: | ---: |
| declaration | 856 | 582 | 561 |
| shuffled | 955 | 2,930 (3.1x) | 770 (-19%) |
| reversed | 916 | 2,859 (3.1x) | 764 (-17%) |

The name-table build is also faster than today on the issue shape (14 fields):
341, 438 and 404 µs against 415, 507 and 424 for declaration, shuffled
and reversed order. It is used only above 8 members; at 8 or fewer the
scan is cheaper than hashing the key.

Serde-throughput metric on the shuffled orders (about 100 KB): Node
`JSON.parse` 222 MB/s (Node 24.19; 241 on 26.13). hd today 124 MB/s
(0.56x), V2 119 (0.53x), V3 125 (0.56x). On this shape the reader, not the
pass structure, is the cost.

Edge cases, A/B against today's reader over every `std/json` and
`std/serde` conformance fixture, with the name-table build: 92 of 97
give identical output. One differs only in std line numbers in a panic
trace. Three are the rule changes below (rule (a), questions 3 and 6), and
one holds the enum forms the experiment did not build
(`json-enum-tagging.hd`). Three V3 bugs the A/B found were fixed during the
experiment: a key after pretty-printed whitespace (two fixtures) and
`Result`'s variant payload.

| Case | Today | V3 |
| --- | --- | --- |
| `{"Empty":{},"Empty":{}}` | `.Ok(Choice.Empty)` | `WrongType("$", "variant")` (question 3) |
| `{"count":null,"name":false}`, `name` declared first | `WrongType("$.name", "string")` | `WrongType("$.count", "u8")` (question 6) |
| `decode::[Unread]("{bad")`, a reader that panics | not called | called, then the parse error (rule (a)) |
| `"User"` for `User(name: string)` | `MissingField("$.User.name")` | same |
| `{"id":6,"id":7,...}` | 7 | 7 |
| `[true,{"x":[1,2]},[false,null]]` into a peek-only reader | 3 items | 3 items |
| internal, adjacent or untagged enums | read | not built in the experiment (see E) |

## B. Prior Art, With Code

**serde** ([serde.rs/deserialize-struct](https://serde.rs/deserialize-struct.html)):
the derive generates a `__Field` enum and a `visit_map` loop with one
`Option` slot per field.

```rust
while let Some(key) = map.next_key()? {
    match key {
        Field::Secs => { if secs.is_some() { return Err(de::Error::duplicate_field("secs")); }
                         secs = Some(map.next_value()?); }
        Field::Nanos => { /* same */ }
    }
}
let secs = secs.ok_or_else(|| de::Error::missing_field("secs"))?;
```

`__Field` is a `match` on `&str`, which rustc lowers to length and byte
comparisons. hd takes the slot loop and the any-order read. It keeps
duplicate-last instead of `duplicate_field` (question 4), and matches
names with a per-type table because the template is library code, not a
generated `match`.

**serde_json** reads one pass and lets `Deserialize` run before the end
is seen ([`from_str`](https://docs.rs/serde_json/latest/serde_json/fn.from_str.html));
trailing characters are an error after the value. That is rule (a).

**simd-json and sonic-rs** ([simd-json](https://github.com/simd-lite/simd-json),
[simdjson paper](https://arxiv.org/abs/1902.08318),
[sonic-rs](https://github.com/cloudwego/sonic-rs)): stage 1 finds every
structural character with SIMD into an index; stage 2 walks it.

```rust
let v: Value = simd_json::to_borrowed_value(&mut bytes)?;   // stage 1 + 2
let o: Order = sonic_rs::from_str(text)?;                   // serde, SIMD skipping
```

Today's hd decode is the two-stage shape without SIMD: the index costs a
pass that the scalar Wasm reader does not win back (V1 is slowest in
almost every cell). sonic-rs keeps serde's one-pass visitor and spends
SIMD on skipping, which hd could add to `next_field`'s skip later.

**Go encoding/json** ([Unmarshal](https://pkg.go.dev/encoding/json#Unmarshal)):
`checkValid` scans the whole text first, then decodes; field names match
"preferring an exact match but also accepting a case-insensitive match".

```go
type Order struct { ID int64 `json:"id"` }
json.Unmarshal([]byte(`{"ID": 1}`), &o)   // sets o.ID: case-folded match
```

hd does not fold case. Folding makes `{"id":1,"ID":2}` ambiguous, and
two parsers that disagree on it is a known injection route
([Bishop Fox, JSON
interoperability](https://bishopfox.com/blog/json-interoperability-vulnerabilities)).
Go's own v2 proposal makes matching case-sensitive by default
([golang/go#63397](https://github.com/golang/go/discussions/63397)).

## C. Alternatives Considered

- **V2, lane 160's in-order pass with the two-pass fallback.** Fast only
  when keys are in declaration order; on shuffled or reversed text it is
  as slow as today or slower. It compiles every `deserialize` for two
  readers (+18 to +41% Wasm).
- **V4, validate without an index, then the key loop.** Not built: V3
  returns the spec's errors without it (Appendix A). Lane 160 measured
  the validating pass alone costing more than one pass saves.
- **The name table found by the address of the first member's name.**
  This is the experiment's stand-in for question 2: no compiler change,
  but each object checks all F name addresses once, and it relies on
  member names being static data.
- **A generated `match` on the key, as serde's `__Field`.** The derive
  is a library template, so it would need the compiler to generate the
  match per type; the table gets the same cost without that.
- **`Deserialize::pure()` gating the one-pass path (#1809 option c).** The
  owner chose rule (a), so no gate is needed.

## D. Today In hd

`decode` runs `Parser.validate`, which checks the text and writes a
structural index, then `TextReader` serves `member(m)` by finding the key
among the object's index entries
([lib/std/json.hd](../../lib/std/json.hd)). Derived `Deserialize` asks for
members in declaration order
([std-serde.derive.deserialize.data-and-enum-forms](../../spec/std/serde.md#r-std-serde.derive.deserialize.data-and-enum-forms)).
The `Source` protocol already lets a source name members in any order and
passes the previous value
([annot.build.next-option](../../spec/lang/14-annotations.md#r-annot.build.next-option)),
so the key loop needs no compiler change.

## E. Details

Proposed spec text, not applied:

- `std-json.decode.direct`: "`decode` | ... | the `T` that `T::deserialize`
  reads from the text in one pass".
- `std-json.decode.parse-error`: "When `parse` fails, `decode` returns its
  parse error. `T::deserialize` may have run on the text before the error."
- `std-serde.read.keyed` (new): "`keyed()` is `true` for a reader that names
  a record's members itself through `next_field`; the default is `false`."
- `std-serde.read.next-field` (new): "`next_field(members, facts, guess)` is
  the index of the member that the record's next key names, or `.None`
  after its last key. A key that names no unskipped member is skipped with
  its value."
- `std-serde.read.absent-field` (new): "After `absent_field(m, facts)`,
  `is_absent` and `is_null` are true and any other read is the format's
  missing-member error."
- `std-serde.read.skip` (new): "`skip()` reads one value of any kind and
  drops it."
- `std-serde.derive.deserialize.data-and-enum-forms`: "... reads every
  unskipped built member: in the order `next_field` names them when the
  reader is keyed, then each unnamed one through `absent_field`; otherwise
  in declaration order through `member(m)`."
- `std-json.decode.first-error` (new): "When the text holds several typed
  errors, `decode` returns the first one in text order."
- `std-json.derive.variant-object-one-key` (new): "An externally tagged
  variant object has one key; a second key is `WrongType` at the object,
  expecting `variant`."

Phases, about an hour each, each with its fixture:

1. Keyed `Source` and the `Deserializer` hooks; the `Feed` fixture
   (`serde-derive-read-order.hd`) keeps declaration order, a new keyed feed
   reads any order.
2. `StreamReader` for structs and externally tagged enums; delete
   `TextReader`; rewrite `json-decode-streaming.hd`'s `Unread` case to
   rule (a).
3. Internal, adjacent and untagged enums on the stream reader. Internal
   tags need the tag before the payload: buffer the object's entries until
   the tag key, as serde's `Content` does, only when the tag is not first.
4. The per-type name table (question 2), with a counted test at F = 16 and
   F = 160 members, shuffled, asserting key comparisons grow linearly.
5. `skip()` on `Deserializer`, `JsonReader` and the stream reader.
