# Link: static data behind the per-instance unit (#2383)

Rule: #2383, emit each instance into its own unit and merge serially; the
`.wasm` byte-identical at any `-j` and to main.

Today (after lane 303): symbols, globals, signatures, defines and count
shim requests go through `Module::emit_local`'s local view and `Unit`.
Static data does not: `Module::static_bytes`, `static_record`,
`static_slot` and `label_static` write `data`, `data_at`, `record_at`,
`data_pointers`, `data_chunks` and `static_labels` straight from the
emission, and the address they return goes into the body at once.

What carries a static address out of an instance's emission:
- `Ins::Static(at)` (`Func::static_addr`), `Func::data_refs`,
  `Func::statics`;
- a plain `i32.const`: `emit/refs.rs:336` pushes `rt::empty_buffer`'s
  address with `f.u32` (make it `static_addr`);
- arithmetic on an address before it is pushed: `+ HEADER`
  (`emit/list.rs:274`, `rt::empty_buffer`), `base + header`
  (`emit/closure.rs:335`);
- the bytes of a `static_record` (its `pointers` words), and
  `Module::stored` / `Address::Static` (`emit/closure.rs` `static_cell`
  -> `note_stored`).

Direction: the unit's own segment, addresses tagged as `Symbols` tags
local ids (bit 31; the static region is far below 2 GiB), each entry with
its alignment. The merge appends the unit's entries in creation order,
deduplicating `static_bytes`/`static_record` against `data_at`/`record_at`
(a record's pointer words relocated first), and relocates every tagged
address by entry (a sorted entry list; an address inside an entry keeps
its offset). Cost: one binary search per static address in a body; one
pass per function, already done for symbols.

Done check: byte-identical release wasm (bench/json-decode and the 1,700
line json program, 1 and 8 jobs, `target/lane303/bytes.sh`); the release
unit test; WAT baselines.
