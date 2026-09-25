# Portable Test Selection

`cases.tsv` selects the conformance fixtures and phases currently supported by
the MVP. Each `.hd` source owns its expected diagnostic, warning, panic, or
result. The runner cross-checks that source-owned expectation against the
normative entry in `spec/conformance/cases.tsv`.

Run the suite against the repository compiler:

```sh
npm run test:portable
```

Run it against another implementation that provides compatible `parse`,
`check`, and `test` commands:

```sh
HD_TEST_COMMAND="other-hd" node --experimental-strip-types test/run-portable.ts
```

The commands use exit status for success, rejection, and runtime panic.
Diagnostics must include their stable code followed by `:`. The `test` command
runs `main` and every named hd-lang test block in a fixture.

Independent cases run concurrently. Set `HD_TEST_JOBS` or pass `--jobs` to
change the default of up to eight compiler processes.
