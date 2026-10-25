# Pipe-style measurements (#822)

These are ordinary hd desugarings, not implementations of proposed pipe syntax.
See [the report](../../../docs/future-work/PIPE_STYLES.md) for semantics, source pins, interpretation, and limitations.
`common.hd` provides identical eager operations for six generated packages.

- `direct`: subject-first calls and an explicit stateless element callback; models Elixir, Hack, and placeholder's pipe slot.
- `curried`: configuration factories returning captured unary functions; models F#.
- `partial`: the direct callback saved to a local; models the literal lambda expansion of an outside-pipe partial.

Numeric/string inputs each have one million values.
Every execution verifies filtered/mapped counts, joined byte length, checksum, exit status, and empty stderr against independent Python computation.
No runtime, compiler, or std file is edited.

## Reproduce

Use an hd dev CLI built with Wasmtime support. The recorded CLI is source
`16fd4789bf3dda1ada8cab1710c8f5ff5fc21880`; see `results/job.json` for its exact hash
and the shared-target provenance correction. The report branch's std and backend
executable sources match that source. Do not overwrite another agent's compiler;
copy a built CLI to an owned scratch path first.

From the repository root, with your frozen compiler at `/path/to/hd`:

```sh
python3 bench/study/pipe-styles/run.py --hd /path/to/hd --work .cache-pipe-study --runs 21
python3 bench/study/pipe-styles/allocations.py --hd /path/to/hd --work .cache-pipe-study
python3 bench/study/pipe-styles/census.py > .cache-pipe-study/std-census.json
python3 bench/study/pipe-styles/parse.py --parser target/debug/examples/parse_check --work .cache-pipe-study/parse
```

`run.py --quick` restricts execution to V8; the default includes V8 and Wasmtime,
alternating engines and rotating forms. `--prepare-only` checks/builds the twelve
artifacts without timing. The program build uses `hd build --release`, not a
Cargo release compiler. Timings are fresh-process wall and child CPU, including
startup/compilation and common checksum work. They are not warmed-instance throughput.

`allocations.py` wraps scratch Wasm exports, leaving original code/data intact,
and returns each byte of the allocator's existing 32-bit GIVEN counter in four
separate processes. It refuses an unknown counter layout. Counts include
freelist reuse and class fast paths; they are guest allocation requests, not
live bytes or native allocations. Original stdout must remain exact. An
instrumented module's nonzero process status encodes a counter byte and must
not be interpreted as normal program success or used for timing.

`census.py` inventories explicitly spelled public function declarations and
balanced parameter lists. It includes inherent methods but not implicit-public
trait methods or generated implementations; it does not guess semantic subjects.

The `probes/` files retest focused inference/default/suspension controls from
#818. Some deliberately fail. Expected checks and both-engine outputs are
recorded in `results/probes.json` and `results/additional-probes.json`; do not
run this whole directory assuming every file should check.

`results/` preserves all 252 raw timings, medians/dispersion, twelve artifact
identities and size counts, 24 allocation observations, emitted application
function names, the complete std census, and parse/type-control receipts.
Absolute local artifact paths are omitted; scripts write fresh scratch outputs.
The measured timings were collected with other agents' heavy checks held.
