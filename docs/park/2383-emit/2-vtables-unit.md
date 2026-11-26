# Link: vtables behind the per-instance unit (#2383)

Rule: #2383, emit each instance into its own unit and merge serially.

Today: `Module::vtable_full`, `vtable_reserve`, `ReservedVtable::fill`
and the caches `dyn_vtables` (`emit/dyn_value.rs:124-145`) and
`handle_vtables` (`boundary.rs:1337`) write `vtables`, `vtable_at` and
`reserved_vtables` during emission; the index goes into `Ins::VTable`
and `Edge::VTable` at once, and into other tables' `parents`. Lane 303's
`Module::remap_vtables` only renumbers the local symbol ids inside the
tables an instance added; their indices are still the module's.

Direction: unit-local vtable indices (tagged), the cache inserts queued
with the table they name. The merge replays the unit's tables in creation
order: a `vtable_full` deduplicates against `vtable_at` (after its slots
and parents are renumbered); a cache insert whose key the module already
holds (an earlier unit built it first) maps the unit's index to the
module's and drops the unit's tables built only for it, reserved ones
included. Then `Ins::VTable`/`Edge::VTable` are renumbered in the pass
that renumbers symbols.

Done check: as 1; plus a fixture where two instances convert the same
type to the same trait (dyn and host handle), so the second unit builds a
table the first already built, and the bytes stay identical at 8 jobs.
