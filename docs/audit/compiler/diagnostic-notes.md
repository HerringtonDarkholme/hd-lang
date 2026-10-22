# Deferred Compiler Diagnostics

| Date | Case | Current behavior | Desired diagnostic |
| --- | --- | --- | --- |
| 2026-10-22 | Nested record field in `@derive(Cli)` (#777) | Parsing returns `CliError.InvalidValue` naming the field and explaining that subcommands are an enum. | Reject the record-typed field at the derive with the same field name and guidance; an explicit flattening fact needs a separate decision. |
