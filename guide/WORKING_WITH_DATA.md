# Working With Data

This page covers five everyday jobs that the
[Language Tour](LANGUAGE_TOUR.md) does not: reading JSON into typed
config, tests that live in the documentation, concurrent work with a
timeout, file reads and writes, and pulling fields out of log lines. Each
program below runs as written; the tests pass with `hd test FILE` (or in
package mode, `hd test`).
# Working With Data

This page covers five everyday jobs that the
[Language Tour](LANGUAGE_TOUR.md) does not: reading JSON into typed
config, tests that live in the documentation, concurrent work with a
timeout, file reads and writes, and pulling fields out of log lines. Each
program below runs as written; the tests pass with `hd test FILE` (or in
package mode, `hd test`).
## Documentation That Runs As Tests

A code block in a doc comment that no one runs drifts out of date. A
fenced `hd` block inside a `##` documentation comment is a doc test: `hd
test` runs it like any other test, so the example in the docs is the
example that compiles today. The rules are in
[Doc Tests](../spec/lang/10-modules.md#doc-tests):

```hd
## Money helpers.
##
## The example runs as a test:
## ```hd
## use pkg.total
## use std.testing.assert_equal
##
## assert_equal(total([+300, +200]), +500, reason="sum")
## ```
pub fn total(prices: List[i32]) -> i32:
    let sum = +0
    for price in prices:
        sum = sum + price
    sum
```

A doc test is its own program: it imports what it uses, from the package
under `pkg` or from `std`. Run it with `hd test` in the package.
