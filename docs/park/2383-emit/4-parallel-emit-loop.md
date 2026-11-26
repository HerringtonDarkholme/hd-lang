# Link: run the emission loop on the scheduler's threads (#2383)

Rule: #2383, the owner's "emit only" choice: `link: emit instances`
scales with jobs; the `.wasm` byte-identical at any `-j` and to main.

Needs 1-3 first. Then:
- Split what emission reads from what it writes: `emit::instance` takes
  `&Module` (or an `Arc`'d read-only part) and `&mut Unit`; today every
  emitter holds `&mut Module` (`Em::m`, 82 `m.func` calls and the census
  list), so this is a signature change through `emit/`, `task.rs`,
  `boundary.rs`, `drive.rs`, `rt::counted_of`, `rt::empty_buffer`.
- Each unit's view reads one base frozen before the loop
  (`Symbols::local` over `Symbols::freeze`d tables: lock-free reads),
  instead of the current base the serial merge keeps today. A unit then
  re-adds what an earlier unit added; the merge's interns and the
  first-wins rules of 1-3 make that a no-op, so the ids come out as the
  serial ones. Defines: a function an earlier unit defined (a
  `CellDrop`, a linear adapter) is dropped, not pushed to `inst_order`
  twice.
- Run `hd_sched::par_map(input.order, ...)` for the units, then the merge
  over `input.order` serially. Extend
  `release_ownership_bytes_match_at_one_and_eight_threads`' doc to say it
  covers emission.

Done check: `link: emit instances` at 8 jobs below its 1-job time on
bench/json-decode and the 1,700-line program (3 interleaved cold pairs,
`target/lane303/bench.sh`); 1-job wall not slower; bytes identical.
