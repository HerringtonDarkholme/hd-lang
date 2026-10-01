# Portable Test Selection

`cases.tsv` selects the conformance fixtures and phases currently supported by
the MVP. Each `.hd` source owns its expected diagnostic, warning, panic, or
result. The runner cross-checks that source-owned expectation against the
normative entry in `spec/conformance/cases.tsv`.

Run the suite against the repository compiler:

```sh
pnpm run test:portable
```

Run it against another implementation that provides compatible `parse`,
`check`, and `test` commands:

```sh
HD_TEST_COMMAND="other-hd" node --experimental-strip-types test/run-portable.ts
```

The commands use exit status for success, rejection, and runtime panic.
Diagnostics must include their stable code followed by `:`. The `test` command
runs `main` and every test case of a fixture's `tests:` block.

Select one tier of the specification with `--tier language` or `--tier std`
([Tiers](../../spec/conformance/README.md#tiers)). A case whose
`specification` column in `spec/conformance/cases.tsv` cites a `std/` path
is stdlib tier, and every other case is language tier. Like `--phase`,
`--tier` runs only conformance cases, not `test/fixtures`. Without it, both
tiers run. The summary line counts passes per tier.

```sh
pnpm run test:portable --tier language
```

Independent cases run concurrently. Set `HD_TEST_JOBS` or pass `--jobs` to
change the default of up to eight compiler processes.
