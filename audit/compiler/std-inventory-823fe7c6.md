# L1: Std Inventory, Spec vs `lib/std` (`823fe7c6`)

Compared all 29 `spec/std/*.md` chapters (declared items from
`declares` rules and `use std.*` examples, methods via `Type::method`
references) against `lib/std/*.hd` (+ `testing/arbitrary.hd`,
`prelude/testing.hd`) and the seeded items in
`compiler/crates/hd_resolve/src/seed.rs` (std.core/task/function/
inspect/structure/testing, incl. the harness `it`/`assert_equal`).
Every lib extra was cross-checked against the whole `spec/` tree.
Method: scripts in /tmp (not committed) plus targeted reads; `::`
attribution and bang-less use spellings were verified by hand, not by
regex alone.

## Missing modules (2): no lib file at all

- **`std.net`** — spec declares `Datagram`, `Net` (+ `bind_udp!`,
  `connect!`, `listen!`, `lookup!`), `NetError`, `TcpListener`
  (+ `accept!`, `close`), `TcpStream` (+ `close`, `read!`, `write!`),
  `UdpSocket` (+ `close`, `receive!`, `send_to!`). Traits + error enum
  are plain hd (a fixture already implements `Net` by hand, below);
  a live socket provider needs host runtime support
  (intrinsic/host-bound, like the `Http`/`Console` providers).
  Fixtures using it: 1 (`runtime/valid/net-own-provider.hd`, ~15
  `std-net.*` rules, via a hand-written `NoNetwork` provider).
- **`std.sys`** — spec declares `Sys` (+ `os`, `arch`, `hostname`,
  `cpu_count`), `SysError`, `MapSys`. `MapSys` is plain hd (a
  deterministic map-backed provider); live `Sys` reads are
  host-bound. Fixtures using it: 1 (`runtime/valid/map-sys.hd`,
  ~13 `std-sys.*` rules).

## Missing items in present modules (4)

- **`std.task`: `Backoff` + `retry_with!`** (rules
  `std-task.backoff.decl-usize`, `std-task.retry-with.decl`). `Backoff`
  is plain data + `Eq`; `retry_with!` is a loop over `Clock.sleep!`,
  plain hd like the existing `retry!`. Fixtures: 1
  (`runtime/valid/retry-with-backoff.hd`).
- **`std.random`: `Rng::from_seed`** — referenced by
  `std-random.rng.from-random` (`rng()` "returns `Rng::from_seed` of
  that draw"); `rng()` and `Rng::new` exist, `from_seed` does not.
  Plain hd constructor. Fixtures: 0.
- **`std.ops`: free `default() -> DefaultVariant`** (rule
  `std-ops.default.derive.marker`, the `@default` fact constructor).
  Only the `Default::default` trait method exists in lib. Probably
  plain hd (the consuming derive template is compiler-side).
  Fixtures: 0.

## Signature differences: none

Every top-level `fn` present in both spec and lib matches after
whitespace normalization (the one textual hit, `format::debug`, is
example-code trailing-colon formatting). The only shape difference is
the missing free `default()` above (method exists, free function does
not).

## Extras in std: all spec-mentioned

No lib item was found unmentioned by the spec. The large groups resolve
as: language-tier items kept by name (`Eq`, `Ord`, `Hash`, `Display`,
`println`, operator traits, `Iterable`/`Iterator`, …), prelude names,
or items declared in their own chapter's prose/rules/tables
(`BufferConsole`, `MapArgs`, `parse_usize`, `num_suffix` machinery,
`Backoff`-adjacent `race!` via lang 11, etc.). Lib-only support
modules (`annotation`, `convert`, `function`, `inspect`, `resource`,
`structure`, `prelude/`, `testing/`) are all named from lang chapters.
Seeded items (`it`, `assert_equal`, core/task/function/inspect/
structure) complement, not duplicate, the lib files.

## Limits

- Name-level inventory: method bodies, generic bounds, default values
  and requirement rows were not pairwise diffed.
- `use`-spellings without `!` (`read_line`, `get`, `sleep`) resolve to
  the banged lib items; verified by hand.
- `fs` `main!` is a user-declared entry convention (spec prose), not a
  missing std item.
