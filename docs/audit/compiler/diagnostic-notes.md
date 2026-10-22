# Deferred Compiler Diagnostics

| Date | Case | Current behavior | Desired diagnostic |
| --- | --- | --- | --- |
| 2026-10-22 | Nested record field in `@derive(Cli)` (#777) | Parsing returns `CliError.InvalidValue` naming the field and explaining that subcommands are an enum. | Reject the record-typed field at the derive with the same field name and guidance; an explicit flattening fact needs a separate decision. |
| 2026-10-22 | Multiple command fields at one CLI level (#779) | Parsing returns `InvalidValue` naming both fields; one parser can select only one command. | Reject the two command-typed fields at the derive with both field names. |
| 2026-10-22 | Repeated command fields, including wrapped lists (#779) | Parsing returns `InvalidValue` naming the field; repeated commands lack a child-argument delimiter. | Reject the repeated subcommand field at the derive, including a list inside a wrapper. |
| 2026-10-22 | Non-record root passed to `env_decode[T < Deserialize]` (#498) | Reading returns `HostError.Other("env_decode needs a struct with named fields")`; the public helper needs no `Inspectable` bound. | Reject a root without named struct fields at the call, with the same guidance; retain the ordinary `Deserialize` bound. |
