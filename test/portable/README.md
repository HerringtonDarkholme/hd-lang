# Portable Test Selection

`cases.tsv` selects the conformance fixtures and phases currently supported by
the MVP. Each `.hd` source owns its expected diagnostic, warning, panic, or
result. The runner cross-checks that source-owned expectation against the
normative entry in `spec/conformance/cases.tsv`.

Run the suite against the repository compiler:

```sh
pnpm run test:portable
```

This runs the repository compiler in-process. The conformance runner loads
[`../hd-adapter.ts`](../hd-adapter.ts) through its `--adapter` option
([Adapters](../../spec/tools/README.md#adapters)). The adapter passes each
command line to `main` in `src/cli.ts` on a pool of worker threads, and
captures the exit status, stdout, and stderr that a spawned `hd` would
report. A case that runs past the 10-second limit has its worker terminated
and replaced. The `test/fixtures` cases use the same adapter.

Run it against another implementation that provides compatible `parse`,
`check`, and `test` commands; it is spawned once per command:

```sh
HD_TEST_COMMAND="other-hd" node --experimental-strip-types test/run-portable.ts
node --experimental-strip-types test/run-portable.ts --compiler "node --experimental-strip-types bin/hd.js"
```

The commands use exit status for success, rejection, and runtime panic.
Diagnostics must include their stable code followed by `:`. The `test` command
runs every test case of a fixture's `tests:` block. The prototype's `hd test`
never runs `main`; the runner runs an entry as `IMPL FILE`.

Select one tier of the specification with `--tier language`, `--tier std`,
or `--tier cli` ([Tiers](../../spec/conformance/README.md#tiers)). A case
whose `specification` column in `spec/conformance/cases.tsv` cites a `std/`
path is stdlib tier, a [CLI case](../../spec/conformance/README.md#cli-cases)
is CLI tier, and every other case is language tier. Like `--phase`,
`--tier` runs only conformance cases, not `test/fixtures`. Without it, every
tier runs. The summary line counts passes per tier.

`cases.tsv` lists a CLI case as `cli/NAME`, with the phase column `cli`. A
CLI case the prototype fails stays out of `cases.tsv` and has a row in
`KNOWN_FAILURES.tsv`, as a language fixture does.

```sh
pnpm run test:portable --tier language
```

Independent cases run concurrently. Set `HD_TEST_JOBS` or pass `--jobs` to
change the default of min(8, CPUs) workers, or compiler processes when
spawning.
