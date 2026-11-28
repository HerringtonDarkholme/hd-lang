# Packages

A program larger than one file is a package: an `hd.toml` beside a `src/`
folder.

## Create one

```sh
hd new orders --app     # an application
hd new money --lib      # a library
```

```text
orders/hd.toml          the manifest
orders/src/main.hd      pub fn main() -> void $ Console: ...
orders/tests/orders.hd  an integration test that runs the program

money/hd.toml
money/src/lib.hd        the library root: its `pub` items are the interface
money/tests/money.hd
```

Run commands from inside the package folder: `hd run`, `hd test`,
`hd check --tests`, `hd fmt`.

## `hd.toml`

```toml
[package]
name = "orders"

[capabilities]          # what the program's `main` may use
Console = true
Args = true
Clock = true
Random = true

[dev-capabilities]      # extra grants for tests and tasks only
Process = true

[dependencies]
money = { path = "../money" }
```

Grants, scopes, `--cap` and refusals are in [capabilities](../design/capabilities.md).

## Where code goes

| Path | Is |
| --- | --- |
| `src/main.hd` | the program's entry point; `main` must be here, not in another module |
| `src/lib.hd` | a library's root module, imported as `pkg` (or `dep.NAME` from outside) |
| `src/NAME.hd` | the module `pkg.NAME` |
| `src/DIR/mod.hd` | the module `pkg.DIR`; needed for a folder of modules |
| `tests/NAME.hd` | an integration test ([testing](../design/testing.md#integration-tests)) |
| `tasks/NAME.hd` | a script run with `hd run NAME`, with the package's code in reach |
| `build/` | build output; do not edit or commit |

Modules and `use` are covered in
[language](../language.md).

## Several programs in one package

Each `[[executable]]` names an entry module. `hd run app` and `hd run dev`
then run each one.

```toml
[[executable]]
name = "app"
module = "main"         # src/main.hd

[[executable]]
name = "dev"
module = "dev"          # src/dev.hd
```

## Dependencies

```sh
hd add ../money             # a local package
hd add acme/json            # the newest release of a GitHub repository
hd add acme/json@2.1        # the newest 2.1 release
hd add --dev acme/testkit   # for tests only
hd update                   # move every dependency to its newest compatible release
hd remove json
hd fetch                    # download what the cache lacks
```

`hd add` writes `[dependencies]` and `hd.sum`, the recorded hashes; commit
both. Code then imports by the key in the manifest:

```text
use dep.money.{of_cents, show}
```

A library publishes a version with a git tag, `v2.3.0`. The manifest does
not state its own version.

## Workspaces

Several packages in one repository share a root `hd.toml` that lists them
and declares no package of its own:

```toml
[workspace]
members = ["money", "shop"]
```

At the root, `hd test` covers every member, and `-p NAME` picks one:
`hd run -p shop`.
