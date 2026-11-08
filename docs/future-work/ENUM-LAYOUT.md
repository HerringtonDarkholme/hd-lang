# Enum Layout

Proposal, 2026-11-08; domain: compiler internals (`hd_wasm_linear`); owner
decided its five questions on 2026-11-08 (below). Issue #1422. Owner direction on record: "how rust is doing this,
do no worse" and "i32?? should be 8, not 12. this is general".

## The Proposal

hd lays an enum out the way rustc's `layout_of_enum` does, on wasm32:

1. Each variant's payload is one struct at one offset (Rust's `univariant`).
2. The tag is the smallest integer that fits the discriminants, widened to the
   payloads' first-field alignment.
3. When the largest variant has a niche (bytes with invalid values), the other
   variants are stored as niche values and there is no tag.
4. A value in Wasm locals is one value per 4- or 8-byte word of that layout,
   so a copy never branches on the variant.

```text
enum A:
    Maybe(i32?)
    Must(i32)

fn read(a: A) -> i32:
    match a:
        .Maybe(.Some(x)) => x
        .Maybe(.None) => 1
        .Must(y) => y
```

Today `A` is 12 bytes and 3 Wasm values. With the proposal it is 8 bytes and
2 values, and the whole match is one `br_table` on word 0. This is what rustc
emits for the same type.

## Decisions (owner, 2026-11-08)

1. **Field reordering: yes.** Struct and payload fields are ordered the way
   `univariant` does (largest alignment first, the largest niche at an edge).
2. **Address niche: only 0**, Rust's rule, on every target. hd's unused
   addresses below `STATIC_BASE` are not a niche.
3. **Niche values: always extend the valid range upward from its end.** No
   prefer-zero, no -1, no `bool` exception. Sizes match rustc; some niche
   values differ (`char?`'s `None` is `0x110000`; rustc 1.97 uses -1).
4. **Locals: one Wasm value per 4- or 8-byte word**, with a bit container
   where variants disagree on a word's type. No shadow stack.
5. **Order: payload as a struct (with reordering), then delete the views
   (#1448), then niches.** The one-test and `br_table` match lowering rides
   with each part.

## The Gap

Sizes in bytes / alignment, wasm32. Rust is `size_of`/`align_of` from
rustc 1.97.1 `--target wasm32-unknown-unknown -O`, read back in Node. hd
today is `# @dump: layout` on main at c506d5a21. "Proposed" applies the
algorithm below, with the decisions above. Values
are Wasm values per local: Rust's ABI (S scalar, SP scalar pair, M one
address) and hd's leaves today and cells proposed. hd `string` is compared with
`Rc<str>`, `List[T]` with `Rc<Vec<T>>`, and `*T` with `&T`.

| Type | Rust | hd today | Proposed | Values R / today / prop. |
| --- | --- | --- | --- | --- |
| `i32?` / `i32??` / `i32???` | 8/4, 8/4, 8/4 | 8, **12**, **16** | 8, 8, 8 | SP / 2,3,4 / 2 |
| `i64?` / `i64??` | 16/8, 16/8 | 16, 16 | 16, 16 | SP / 2,3 / 2 |
| `bool?` / `bool??` / `bool???` | 1/1 each | **8, 12, 16** | 1 | S / 2,3,4 / 1 |
| `char?` / `char??` / `char???` | 4/4 each | **8, 12, 16** | 4 | S / 2,3,4 / 1 |
| `string?` / `??` / `???` | 8, 12, 12 | **12, 16, 20** | 8, 12, 12 | SP,M / 3,4,5 / 2,3,3 |
| `List[i32]?` / `??` / `???` | 4, 8, 8 | 8, 12, 16 | 4, 8, 8 | S,SP / 2,3,4 / 1,2,2 |
| `(*i32)?` / `??` / `???` | 4, 8, 8 | 4, 8, 12 | 4, 8, 8 | S,SP / 1,2,3 / 1,2,2 |
| `A` (above) | 8/4 | **12** | 8 | M / 3 / 2 |
| `Shape` (4 variants, 3 with data) | 24/8 | **32** | 24 | M / 6 / 4 |
| `Shape?` | 24/8 | **40** | 24 | M / 7 / 4 |
| `Pair: L(bool, i32); M(i32)` | 8/4 | 12 | 8 | M / 3 / 2 |
| `Tail: V(i32, bool); W` (niche not first) | 8/4 | 12 | 8 | M / 3 / 2 |
| `Tail?` | 8/4 | 16 | 8 | M / 4 / 2 |
| `Late?`, `Late { a: i32, b: char }` | 8/4 | 12 | 8 | M / 3 / 2 |
| `Mixed { a: i32, b: f64, c: bool }` | 16/8 | 24 | 16 | M / 3 / 3 |
| `Mixed?` | 16/8 | 24 | 16 | M / 4 / 3 |
| `HasMixed: M(Mixed); N(i32); O` | 16/8 | 24 | 16 | M / 4 / 3 |
| `Result[string, LoadErr]` | 12/4 | **16** | 12 | M / 4 / 3 |
| `Result[i32, LoadErr]` | 12/4 | 16 | 12 | M / 4 / 3 |
| `Big` (300 fieldless) / `Big?` | 2/2, 2/2 | 2, **8** | 2, 2 | S / 1,2 / 1 |
| `Color` (3 fieldless) / `Color?` | 1/1, 1/1 | 1, **8** | 1, 1 | S / 1,2 / 1 |
| `WithNever: A(i32); B(never)` | 4/4 | 8 | 4 | S / 2 / 1 |
| `WithNever2: A(i32); B(i64, never); C` | 16/8 | 16 | 16 | M / 4 / 3 |
| `Tree: Leaf(i32); Node(*Tree, *Tree)` | 8/4 | 12 | 8 | SP / 3 / 2 |
| `Cons: Nil; Cons(i32, *Cons)` | 8/4 | 12 | 8 | SP / 3 / 2 |
| `Either[i32, bool]` / `[i64, i32]` | 8/4, 16/8 | 8, 16 | 8, 16 | SP,M / 3,3 / 2,2 |
| `Either[string, i32]` / `[bool, bool]` | 8/4, 2/1 | 12, 8 | 8, 2 | SP,S / 3,3 / 2,1 |
| `Holder { flag: bool, v: i32?, w: i64 }` | 24/8 | 24 | 24 | M / 4 / 4 |
| `Outer { x: A?, y: u8 }` / `Outer?` | 12/4, 12/4 | **20, 24** | 12, 12 | M / 4,5 / 3 |
| std `Json` / `Json?` | 16/8, 16/8 | **32, 32** | 16, 16 | M / 5 / 2 |
| std `Cookie` / `Cookie?` | 72/8, 72/8 | 80, 80 | 72 | M |

A `List` element's stride is the size, so a `List[A]` holds 8 bytes per
element instead of 12. An enum as a struct field (`Holder`, `Outer`) and an
enum in an enum in a struct (`Outer?`) follow the same rules.

Code differs as much as size:

| Operation | Rust (wasm32 `-O`) | hd today | Proposed |
| --- | --- | --- | --- |
| Discriminant read | 1 load or 1 value, plus a compare for a niche | value 0 | 1 value, plus a compare for a niche |
| Construction | stores of the tag and the fields used | every value; unused slots get 0 | the words the variant uses |
| Two-variant match | `select` or one `br_if` | `if` chain plus `unreachable` arms | one `br_if` |
| n-variant match | one `br_table` | `if` chain, one compare per arm | one `br_table` |
| Nested match (`i32??`) | one `br_table` on word 0 | two `if` chains | one `br_table` on word 0 |
| Match through `&` | loads only the arm's fields | loads every value first | loads only the arm's fields |
| `&`/`&mut` into payload | address + offset | address, or a view (`RefPayload`) | address + offset, always |
| Drop | `SwitchInt` on the discriminant | `if v == k` per variant | one switch on the discriminant |

## The Algorithm

### (a) Rust's, from rustc 1.97.1

`layout.rs` is `compiler/rustc_abi/src/layout.rs`. Line numbers are at tag
`1.97.1`.

```pseudo
layout_of_struct_or_enum(variants):                     # layout.rs:340
  present = variants not absent                         # absent = uninhabited AND all fields 1-ZST (:88)
  if no present: return never_type                       # size 0, uninhabited
  if not enum or one present: return univariant(that variant's fields)
  return layout_of_enum(variants)                       # :573

univariant(fields, kind):                                # :265, :1091
  order = fields sorted by                               # :1198
    (alignment group descending, largest niche first, niche nearest the start)
    # kind = Prefixed (behind a tag): ascending alignment, largest niche last (:1233)
  offset = prefix size aligned to prefix align (Prefixed) else 0
  for f in order: offset = align_up(offset, f.align); place f; offset += f.size
  largest_niche = the field niche with most available values, moved to its offset
  if the niche is not at an edge: retry with NicheBias::End, keep the one
    whose niche is nearer an edge (:288)
  size = align_up(offset, max align)

layout_of_enum(variants):
  # 1. niche-filling candidate                          # :591
  vl = [univariant(v, AlwaysSized) for v in variants]
  L = index of the largest vl[i].size                   # :626 (last on ties)
  needs = [i != L and not absent(i)]
  niche_variants = first(needs) ..= last(needs)          # :634
  count = niche_variants.len()
  niche = vl[L].largest_niche or give up
  niche_start, new_valid = niche.reserve(count) or give up      # :642
  size = align_up(vl[L].size, max align)
  for i != L:                                           # :647
    if vl[i].size <= niche.offset: fits before the niche
    else: o = align_up(niche.offset + niche.size, vl[i].align)
          if o + vl[i].size > size: give up
          shift every field of i by o
  abi = Scalar/ScalarPair of vl[L] if same size, same align and every
        other variant zero-sized, else Memory          # :694
  encoding = Niche{untagged: L, niche_variants, niche_start}   # :717

  # 2. tagged candidate
  discr = sorted discriminants of inhabited variants    # :751
  (min, max) = the range around the largest gap         # :771
  min_ity = smallest int fitting min..max, unsigned preferred, >= I8
            # rustc_middle/src/ty/layout.rs:77 discr_range_of_repr
  vl = [univariant(v, Prefixed(min_ity.size, min_ity.align)) for v in variants]
  start_align = min over variants of its first non-1-ZST field's align
  ity = int of size start_align if bigger than min_ity  # :891 widen the tag
  move fields at offset min_ity.size to ity.size
  size = align_up(max vl.size, max align)
  tag = ity at offset 0, valid min..=max
  abi = Scalar if size == tag size;
        ScalarPair(tag, prim) if every variant has at most one scalar field,
        all at one offset with one primitive;            # :937
        else Memory

  # 3. pick                                             # :1070
  smaller size wins; on a tie the larger available niche; else tagged.

Niche.reserve(count):                                    # lib.rs:2028
  v = valid range (wrapping), max = 2^bits - 1
  if count > available: none
  if count == 1 and v != 0..=1:   take the free value nearest zero:
     next_up = |sext(v.end+1)|, next_down = |sext(v.start-1)|
     down <= up ? (start = v.start-1, v.start = start)
                : (start = v.end+1,   v.end   = v.end+1)
  else: extend v.end by count (or v.start, avoiding a pass over zero)
  return start, the widened range
```

### (b) Proposed for hd

Input: a concrete type. Output: `Layout { size, align, niche, enc, fields,
cells }`. It is computed once per type in the link-wide layout cache
(`layout::memoized`), and every use site reads it.

```pseudo
struct Niche  { offset, bytes, valid: WrappingRange }    # values outside `valid` are free
enum   Enc    { Single(v) | Direct{tag_bytes} | Niche{offset, bytes, untagged, first, last, start} }
struct Cell   { offset, val: i32|i64|f32|f64, bytes }    # one Wasm value
struct Layout { size, align, niche: Niche?, enc, fields: [[offset]] per variant, cells: [Cell],
                uninhabited }

scalar(t):                                # size = align = bytes
  bool                  -> 1, niche valid 0..=1
  char                  -> 4, niche valid 0..=0x10FFFF
  i8..u64, f32, f64     -> no niche
  never                 -> size 0, uninhabited
  address (*T, *mut T, boxed cell, List/Map buffer)  -> 4, valid 1..=2^32-1   (decision 2: null only)
  string                -> struct(address, u32 len)
  trait value, fn value -> struct(address, address); niche on the vtable address

struct_layout(fields) = univariant(fields, AlwaysSized)  # decision 1
layout(enum) = layout_of_enum(variants)                  # (a), steps 1 to 3, except reserve:

reserve(niche, count):                                   # decision 3: one rule
  v = niche.valid (wrapping), bits = 8 * niche.bytes
  if count > 2^bits - (v.end - v.start + 1) (wrapping): none
  start = v.end + 1 (wrapping); v.end = v.end + count (wrapping)
  return start, v
cells(L) = words(all leaves of every variant at their absolute offsets)

words(leaves):
  o = 0
  while o < size:
    if some leaf of 8 bytes starts at o: w = 8 else w = 4
    ls = leaves overlapping [o, o + w)
    if ls is empty: o += w; continue                    # padding has no value
    if all ls are one full-width type at o: cell = (o, that type, w)
    else: cell = (o, w == 8 ? i64 : i32, w)              # bit container
    o += w

leaf read from a cell: none, or i32.wrap / shr_u / and / f64.reinterpret
leaf write into a cell: or / shl, or a reinterpret back
```

### Discriminant read, write and match

```pseudo
read_discr(v):                     # v: the enum's cells, or its address
  Single(k):  k
  Direct:     tag = cell 0 (or load tag_bytes at 0)
  Niche{offset, bytes, untagged, first, last, start}:
    t   = the cell at offset (or a load)
    rel = (t - start) mod 2^(8*bytes)
    if last == first: rel == 0 ? first : untagged      # one compare
    else:             rel <= last - first ? first + rel : untagged

write_discr(place, k):
  Direct:                         store k at 0
  Niche and k == untagged:        nothing (the payload's own bytes say it)
  Niche:                          store (k - first + start) at offset
  k uninhabited:                  unreachable

match lowering:
  decision tree from the checker: binary SwitchTag [case then else]
  group the tests that read one cell (an enum and the enums nested in its
    untagged payload share the niche cell)
  map each arm to the raw cell values that select it
  values dense (span <= 2 * arms): one br_table on (cell - min), default the rest
  else: a compare chain, the most frequent arm last
  through &: load the cell, then each arm loads only its own fields
```

### Walkthrough: `i32??`

```pseudo
i32:   size 4, align 4, no niche
i32?:  variants None [], Some [i32]
       niche-filling: Some has no niche -> none
       tagged: discr 0..=1 -> min_ity u8; Some's i32 at align_up(1, 4) = 4
               start_align = 4 -> tag widens to u32
       => size 8, align 4, Direct, tag u32 @0 valid 0..=1, Some.0 @4
          niche {@0, 4 bytes, valid 0..=1}, cells [i32 @0, i32 @4]
i32??: variants None [], Some [i32?]
       L = Some (8 bytes), needs = {None}, count 1
       reserve(1) on 0..=1: extend end: start = 2, valid 0..=2
       None is zero-sized: fits
       => size 8, Niche{@0, 4 bytes, untagged Some, first = last = None, start 2}
          cells [i32 @0, i32 @4]   (today: 12 bytes, 3 values)
read:  t = cell0; t == 2 -> None; else inner: t == 0 -> None, 1 -> Some(cell1)
match: one br_table on cell0: [0 -> Some(None), 1 -> Some(Some x), 2 -> None]
write None: cell0 = 2. Some(Some(x)): cell0 = 1, cell1 = x.
```

### Walkthrough: `enum A: Maybe(i32?); Must(i32)`

```pseudo
vl: Maybe = univariant([i32?]) = 8 bytes, niche {@0, valid 0..=1}
    Must  = univariant([i32])  = 4 bytes, no niche
L = Maybe, needs = {Must}, count 1, reserve(1) on 0..=1 -> start 2, valid 0..=2
Must: 4 <= niche.offset 0? no; o = align_up(0 + 4, 4) = 4; 4 + 4 <= 8: fits at 4
tagged: Maybe(i32?) behind a widened u32 tag = 12 bytes; niche 8 < 12 -> niche
=> size 8, align 4, Niche{@0, 4 bytes, untagged Maybe, first = last = Must, start 2}
   Maybe.0 @0 (its own tag @0, x @4); Must.0 @4
   cells [i32 @0, i32 @4]   (today: 12 bytes, 3 values)
read:   cell0 == 2 ? Must : Maybe
match:  one br_table on cell0: [0 -> Maybe(None), 1 -> Maybe(Some x), 2 -> Must y]
write:  Must(y): cell0 = 2, cell1 = y.  Maybe(Some(x)): cell0 = 1, cell1 = x.
```

## Side by Side

The Rust column is `--emit=asm` for wasm32. The hd column is the stage
baseline on main. "Proposed" is written by hand from the rules above.

`twice(v: i32??) -> i32`, a nested match:

```wat
;; Rust: 2 params                ;; hd today: 3 params, 2 if-chains
(param i32 i32) (result i32)     (param $v.0 i32) (param $v.1 i32) (param $v.2 i32)
block block block                local.get $v.0  i32.const 1  i32.eq
  local.get 0                    if  local.get $v.1  i32.const 1  i32.eq
  br_table {1, 0, 2, 0}            if br 2 else local.get $v.1 i32.eqz br_if 3 unreachable end
end local.get 1 return           else local.get $v.0 i32.eqz br_if 3 unreachable end
end i32.const 1 ...              ... (39 lines)
;; Proposed: Rust's, with 2 params
local.get $v.0   br_table $some_none $some_some $none
```

`read_a(a: A)` and `make_a(x)`:

```wat
;; Rust: a by address; make_a writes through a return pointer
local.get 0  i32.load 0  br_table {1, 0, 0, 0}   ;; then i32.load 4
;; hd today: 3 params, 3 results; read_a is two if-chains
i32.const 0  i32.const 1  local.get $x            ;; make_a
;; Proposed: 2 params, 2 results
i32.const 1  local.get $x                         ;; make_a
local.get $a.0  br_table $maybe_none $maybe_some $must
```

`area(s: &Shape)`, an n-variant match through a reference:

```wat
;; Rust: one tag load, one br_table, only the arm's loads
local.get 0  i32.load 0  br_table {0, 1, 2, 3, 0}
.LBB4_2: local.get 0 f64.load 8  local.get 0 f64.load 16  f64.mul
;; hd today: loads all 6 leaves (offsets 0, 4, 8, 12, 16, 24) first, then
;; compares the tag with 0, 1, 2, 3 in nested ifs
;; Proposed: Rust's code; Rect's fields at 8 and 16
```

`drop_res(r: Result[string, LoadErr])`:

```wat
;; Rust: Err's msg address at 0 is the niche; 0 means Ok, whose Rc is at 4
local.get 0  i32.load 0  local.tee 1  br_if 0  local.get 0  i32.load 4 ...
;; hd today: 4 params into rt:release, which tests the tag against 0 and 1
;; Proposed: 3 params; cell0 == 0 ? release(cell1) : release(cell0)
```

## How It Lands

| Today (`crates/hd_wasm_linear/src`) | Proposed |
| --- | --- |
| `layout.rs` `Mem { size, align, leaves }` | adds `niche`, `enc`, per-variant field offsets, `cells` |
| `enum_mem_by`: tag, then slots grouped by value kind (`SLOT_ORDER`) | `layout_of_enum` from (a); slots and `Positions` are deleted |
| `record`: fields in declaration order | `univariant` (decision 1) |
| `nullable` and `Walk::Nullable` | deleted: `(*T)?` is the niche case with `start` 0 |
| `emit/value.rs` `tag()` = value 0; `variant()` puts `v` in value 0, zeros elsewhere | `read_discr` / `write_discr`; words the variant does not use stay undefined |
| `emit.rs` `SwitchTag`: `tag == case` per test | grouped tests, one `br_table` per cell |
| `rt.rs` `walk_code` `Walk::Switch(0, arms)`: `if tag == v` per arm | one switch on `read_discr` |
| `emit/refs.rs` `Helper::RefPayload` (view) when leaves are not at one offset | always `ref_projection` at the payload offset |
| `boundary.rs` `LEnum.vals[0]` is the variant index; `is_tags` host bytes | codecs call `read_discr`/`write_discr`; the host format stays the variant index |

**Deleted**, after rule 1 (#1448): `Helper::RefPayload`, `RefPayloadGet`,
`RefPayloadSet`, `RefIsView`, `RefViewsAbsent`, `RefViewDrop`, `Walk::View`
and its branches in `emit/count.rs`, `emit.rs` and `task.rs`, the link's view
fold, and `Reason::View`. The issue estimates 650 to 800 lines. Done check:
`rg 'RefPayload|ref_is_view|view_get' crates` finds nothing.

**Places that assume zero is `None` or variant 0**, each moving to
`write_discr` or a decoded read:

- `emit/value.rs` `variant()` (the tag is value 0, other values zero) and `tag()`.
- `map.rs` `MapStep` and `emit/iter.rs` `opt_of`: `OptVal::Tag` writes 1
  and `None` is all-zero locals.
- `layout.rs` `nullable`, `switch_walk` (`Walk::Switch(0, …)`), `Walk::Known`
  ("null or a payload start").
- `boundary.rs` `LEnum` (value 0 is the index) and `is_tags` (the host reads tag
  bytes).
- `emit/closure.rs` `zero_src` and zeroed frames (`emit/susp.rs`): zero
  words are still a valid value of any type, but for a niche type not the first
  variant. No code may read such a value before it is written.

**Wasm locals.** A local or parameter takes one value per cell. In every
row of the gap table, cells are no more than today's leaves.

**Cost.** Layout is computed once per type in the link-wide cache, in
O(fields log fields) per variant. `univariant` runs at most twice per variant,
with start and end bias. No use site computes a layout. `emit/value.rs`
`flat` already caches per emitter and becomes a read of the shared entry.
A warm rebuild that changes no type does no layout work. Changing k types
costs k layouts.

| Component | Effect |
| --- | --- |
| Code removed | views, about 650 to 800 lines (#1448); slot grouping; `nullable` |
| Code added | `univariant`, `layout_of_enum`, `reserve`, cells: about 500 lines |
| Compile time | flat: one layout per type, as today |
| Runtime | fewer locals and loads; one `br_table` per match |
| Wasm size | -39 to -42% where views were made today (issue measure); smaller matches |
| Web compiler gzip | net negative: more lines are deleted than added |

**Where hd stays worse than Rust.** LLVM folds some matches into
arithmetic (`char???` becomes an `abs`), and hd's emitter does not. A large
enum in hd is a value in several locals where Rust passes one address. That is
more parameters, but no memory traffic.

## Appendix A. Stress Test

| Case | What happens |
| --- | --- |
| std `Json` | `Number` holds `NumberRepr`, whose tag (u64-widened, 0..=2) is the niche. `Text`, `Array` and `Object` fit after it at 8. Size is 32 today, 16 proposed, the same as Rust. |
| std `Cookie?` | The niche is in `path: string?` or `secure: bool`, so `Cookie?` costs 0 bytes, as in Rust. |
| `&mut` into `A.Maybe`'s `i32?` | A write stores 0 or 1 at word 0. Both are valid, so the outer discriminant still reads `Maybe`. This is sound only because every write through `&mut T` writes a valid `T`. A raw `Buffer` write into a typed slot must not break that. |
| `WithNever2: B(i64, never)` | `B` is uninhabited but not 1-ZST, so it is not absent. It keeps its space, as in Rust (16). `write_discr(B)` is `unreachable`. |
| `bool` nested 255 deep | The 254 free values of the byte run out at depth 255, and level 256 gets a tag. Rust does the same. |
| `f64?` | There is no niche, so a tag widens to 8 bytes: 16 bytes, as in Rust. |
| Zeroed static or frame of `i32???` | Zero words decode as `Some(Some(Some(0)))`, not `None`. That is valid, and no code reads it before a write. A test asserts that a zeroed frame is never read. |
| Generic `Either[T, never]` | `Right` is absent, so the type is a single variant: the size of `T`. |
| `List[A]` push and load | The stride is 8, not 12. A copy is 2 words, with no branch (Rust: one `i64.load`/`i64.store`). |

## Appendix B. Prior Art

**Rust** (rustc 1.97.1). The algorithm is (a) above. The measured examples:

```rust
// wasm32: Option<Option<i32>> = 8, niche_start 2; Option<char> None = 0xFFFF_FFFF
// (hd, decision 3: char? None = 0x110000; same size)
#[rustc_dump_layout(debug)] type T2 = Option<Option<i32>>;
enum A { Maybe(Option<i32>), Must(i32) } // 8: Niche { untagged_variant: 0, niche_variants: 1..=1, niche_start: 2 }
fn twice(v: Option<Option<i32>>) -> i32 { match v { Some(Some(x)) => x, Some(None) => 1, None => 0 } }
```

- **Tag integer.** `discr_range_of_repr` picks the smallest unsigned (else
  signed) integer at least `I8`. `layout_of_enum` widens it to the smallest
  first-field alignment (`Integer::for_align(start_align)`, layout.rs:891).
  This is why `Option<i32>`'s tag is a `u32`.
- **Field order.** `univariant_biased` sorts by alignment group, then niche
  size (layout.rs:1198). Behind a tag (`Prefixed`) it sorts ascending, so small
  fields pack after the tag (`Pair`: tag at 0, bool at 1, i32 at 4).
- **Niche nesting.** Each niche-filled layout exports its remaining range as
  its own `largest_niche`. `Option<Option<Option<bool>>>` uses 2, 255 and 254
  in one byte (measured).
- **Read.** `codegen_get_discr` (`rustc_codegen_ssa/src/mir/operand.rs:416`)
  computes `relative = tag - niche_start`, then one equality when
  `relative_max == 0`, else an unsigned `<=` (operand.rs:496, 513, 614).
- **Write.** `codegen_set_discr` / `codegen_tag_value` (place.rs:252, 462)
  store `(i - niche_variants.start) + niche_start`, masked to the niche width.
  The untagged variant stores nothing. An uninhabited variant aborts.
- **Match.** MIR `SwitchInt` on `Rvalue::Discriminant`
  (`rustc_mir_build/src/builder/matches/test.rs:80`). LLVM turns it into a
  `br_table`, merging nested enums whose niche is the same word (`twice`).
- **Drop glue.** `open_drop_for_multivariant` and `adt_switch_block`
  (`rustc_mir_transform/src/elaborate_drop.rs:834, 940`) switch on the
  discriminant and drop the active variant's fields.
- **ABI.** `Scalar` when the enum is its tag, `ScalarPair` when every variant
  has at most one scalar at one offset, else `Memory`, passed by address.
- **Uninhabited.** An absent variant takes no discriminant. A present
  uninhabited one keeps its space (layout.rs:88).

Sources: [rustc_abi layout.rs](https://github.com/rust-lang/rust/blob/1.97.1/compiler/rustc_abi/src/layout.rs),
[Reference: Type layout](https://doc.rust-lang.org/reference/type-layout.html#the-rust-representation)
(the Rust representation is unspecified), [UCG: enums](https://rust-lang.github.io/unsafe-code-guidelines/layout/enums.html),
[rustc dev guide: drop elaboration](https://rustc-dev-guide.rust-lang.org/mir/drop-elaboration.html).
hd takes all of it.

**Swift** puts a single-payload enum's no-data cases in the payload's
"extra inhabitants", the invalid bit patterns, in ascending order. The ones
left over become the enum's own extra inhabitants, so niches nest as in Rust.

```swift
enum CharOrSectionMarker {   // i32: Paragraph is 0x0020_0000,
  case Paragraph             // a value no UnicodeScalar has
  case Char(UnicodeScalar)
}
```

hd takes the nesting. Addresses keep Rust's null-only niche (decision 2). Source: [TypeLayout.rst](https://github.com/swiftlang/swift/blob/main/docs/ABI/TypeLayout.rst).

**Zig** guarantees `?*T` is pointer-sized (null). Other optionals get a tag.

```zig
const p: ?*u32 = null; // @sizeOf(?*u32) == @sizeOf(*u32)
```

hd already does this (`nullable`). The proposal generalizes it. Source:
[Zig reference: Optional Pointers](https://ziglang.org/documentation/master/#Optional-Pointers).

## Appendix C. Alternatives Considered

- **Keep slots by kind and add niches only.** The slots still scatter a
  payload's leaves, so `&` into a payload still needs views, and #1448
  cannot finish.
- **`nullable`-style special cases per shape** (`i32??`, `bool?`). The
  owner said "this is general". Every new shape would need code.
- **Rust's ABI (memory beyond two scalars).** hd has no stack slots for
  values today. Adding them is a separate design. Cells keep today's model.
- **Tag at the end.** It saves nothing over the widened tag, and niche
  nesting gets harder.

## Appendix D. Today in hd

`layout.rs` `enum_mem_by`: the tag is 1, 2 or 4 bytes at 0. Then come shared
slots: the `k`th `i32` value of any variant is in `i32` slot `k`, then `f32`,
`i64` and `f64` (`SLOT_ORDER`). Each slot is 4 or 8 bytes. A variant's fields
are `Positions` (value indices), not an offset. `nullable` is the one niche:
`(*T)?` is an address. A `bool` payload takes a 4-byte slot, so `bool?` is
8 bytes. A struct is `record` in declaration order. Matches are binary
`SwitchTag` tests (`emit.rs`). Drops are `Walk::Switch(0, …)` (`rt.rs`
`walk_code`). `RefPayload` builds a view when a payload field's leaves do not
follow each other at one offset (`emit/refs.rs`). The probes were a
`layout` stage case with one field per gap-table row and two `wat` cases
(`twice`, `read_a`, `area`, `code_of`, `drop_res`). They are kept in scratch,
not in the repo. The
spec leaves layout open (`types.option.repr`, `data.unsupported.layout`), so
no spec change is needed.

## Appendix E. Details

- **Pipeline.** Layout stays in `hd_wasm_linear`, read through `layout::mem`.
  The checker and TIR do not change.
- **Determinism.** Ties break as in Rust: the last largest variant, and the
  first or last niche by bias.
- **Rollback.** Each part below is one commit with its stage baselines.
- **Parse log.** The one hd block (`enum A` and `read`) parses with `scripts/hd-conformance.sh parse`, and `hd check` passes on it. The `pseudo`, `rust`, `wat`, `swift` and `zig` blocks are not hd.

Implementation order, about 30 minutes each, each with its test:

1. `Niche` on `Mem` for scalars and addresses. Layout dump prints it. Test: layout baseline.
2. Payload as a struct at one offset, with `univariant` field reordering, the tagged path with tag widening. Test: layout rows `Pair`, `Shape`, `Tail`, `Mixed`, `Cookie`.
3. Cells and leaf read/write ops in `payload()`/`variant()`. Test: wat of `area`, conformance enum cases.
4. `RefPayload` always physical; delete views (#1448). Test: `rg` done check, debug-source-text size.
5. Niche-filling path and `reserve`. Test: `i32??`, `A`, `bool???`, `char???` rows.
6. `read_discr` in `SwitchTag`, `tag()`, walks. Test: wat of `twice`, `read_a`.
7. `write_discr` in `variant()`, `MapStep`, `opt_of`. Test: map iteration conformance.
8. Delete `nullable` and `Walk::Nullable`. Test: `(*T)?` rows unchanged.
9. Boundary codecs through `read_discr`/`write_discr`. Test: host enum round-trip cases.
10. Absent uninhabited variants. Test: `WithNever` row.
11. One `br_table` per cell in match lowering. Test: wat of `twice`, `area`.
12. Arm-local loads for a match through `&`. Test: wat of `area`, `width`.

