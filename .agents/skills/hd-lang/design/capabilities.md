# Capabilities: What A Program May Reach

The row of `main` says which host capabilities a program uses
([effects](effects.md#the-host-capabilities)). `hd.toml` says which it may
have. This page covers grants, scopes, flags, refusals, untrusted code and
plugins.

## When

| You want to | Write |
| --- | --- |
| let it read one folder only | `FsRead = ["data/"]` |
| forbid a capability, or allow some variables | `Process = false`, `Env = ["APP_*"]` |
| give tests or tasks more than the program has | `[dev-capabilities]` |
| change a grant for one run | `hd run --cap Http=localhost:8080` |
| let a host supply an API | a `@host` trait ([plugins](#plugins)) |

A single file outside a package needs no table.

## `[capabilities]`

Every host capability in `main`'s row must be listed:

```text
error: src/main.hd:5:24: missing-capability: `main` requires `FsRead` in `$ Console + FsRead`, but `[capabilities]` in hd.toml does not list it
```

`hd check --fix` adds the line with `true`; narrow it. The names are the host capabilities of [effects](effects.md#the-host-capabilities);
any other name is an error.

A value is `true` (no limit), `false` ([refusal](#refusal)), or a list.

## Scopes

A list grants only what its entries cover. It is an error on a capability
with no scope entries: `Console`, `ConsoleInput`, `Clock`, `Random`,
`Args`, `Signals` and `Workers` take only `true` or `false`.

| Key | An entry names |
| --- | --- |
| `FsRead`, `FsWrite` | a file, or a directory with everything under it |
| `Http` | a host, with an optional `:port`; `*.example.com` matches subdomains |
| `Net` | `host:port`, or a host for every port |
| `Env` | a variable, or a prefix ending in `*` |
| `Process` | a program name, as written in the call |
| `Sys` | a method of `Sys`: `os`, `arch`, `hostname`, `cpu_count` |

- Table paths are relative to the package folder, flag paths to the
  working directory.
- `..` and symbolic links are resolved first, so no path escapes a granted
  folder.
- An `FsWrite` entry grants no read.
- `Http` checks each redirect target as a new request.

A call outside the list returns an error and the program goes on.
`FsRead` and `FsWrite` return `.Err(FsError.NotGranted(path))`. `Http`,
`Net`, `Process` and `Sys` return their own error type's `NotGranted`. A
refusal by the grant is never `PermissionDenied`, which is the operating
system's. `Env` returns `.None` from `env(name)`, and `hd` prints one line
to standard error per name: `hd: env PATH is set but not granted; run with
--cap Env=PATH`.

### Example

<!-- file: scoped/hd.toml -->
```toml
[package]
name = "scoped"

[capabilities]
Console = true
FsRead = ["data/"]
```

<!-- file: scoped/src/main.hd -->
```hd
use std.fs.{FsRead, read_text}
use std.path.Path

fn show!(path: string) -> void $ Console + FsRead:
    match read_text!(Path(path)):
        .Ok(text) => println("$path: ${text.trim()}")
        .Err(.NotGranted(_)) => println("$path: not granted")
        .Err(_) => println("$path: unreadable")

pub fn main!() -> void $ Console + FsRead:
    show!("data/a.txt")
    show!("secrets/key.txt")
    show!("data/../secrets/key.txt")
```

With `data/a.txt` present, `hd run` prints:

```text
data/a.txt: hello
secrets/key.txt: not granted
data/../secrets/key.txt: not granted
```

## `--cap`

`--cap NAME=VALUE` sets one grant for one run: `true`, `false`, or entries
separated by commas. `hd FILE`, `hd run` and `hd test` accept it any number
of times, before `--`.

```sh
hd run --cap FsRead=out/                    # out/ alone
hd run --cap Env=HOME --cap Env=PATH        # flags add
```

A flag never replaces the manifest line that `main` needs. `hd` never asks
at run time: a call outside its grant fails at once.

## Precedence

The first rule that applies decides:

| Rule | When | Grant |
| --- | --- | --- |
| deny | the table or any `--cap` flag says `false` | refusal at start |
| flags | a `--cap` flag names it | the flags' entries, replacing the table's |
| table | the table names it | the table's value |
| default | nothing names it | no limit |

So `--cap Process=true` cannot undo `Process = false`, and `--cap
FsRead=out/` replaces `["data/"]`.

## Refusal

A capability set to `false` stops the program before its first line runs,
with status 101:

```text
error: denied-capability: the program needs FsRead, which `FsRead = false` in hd.toml denies, so it does not start
```


## `[dev-capabilities]`

Tests and tasks may need more than the shipped program, such as writing
to `out/`. List those under `[dev-capabilities]`, with the same keys.

- Tasks and integration tests get both tables together. `true` in either
  gives no limit, `false` in either denies, and scopes are joined.
- `main` uses `[capabilities]` alone, and a dependency's dev grants never
  count. A capability in neither table is `missing-capability`, and
  `hd check --fix` adds it to `[dev-capabilities]`.
- Unit tests get no host capabilities. They install fakes
  ([testing](testing.md)).

## Running code you did not write

Code an agent or a stranger wrote reaches only what its row names, and
`hd.toml` can grant less. Put it in a package and write `[capabilities]`
with scoped entries. `hd check` reports what it needs but lacks as
`missing-capability`, before it runs. Use `false` for what it must never
touch.

## Plugins

A host application can supply an API to hd code. Declare it as a trait
marked `@host`. The plugin calls it like any trait, and the host
implements it. The
[hd-lang guide](https://github.com/HerringtonDarkholme/hd-lang/blob/main/website/guide/USING_HD.md)
has the details.

```text
# hd.toml
[host]
"ui.Surface" = "providers/ui.mjs"
[capabilities]
Console = true
"pkg.ui.Surface" = true

# src/ui.hd
@host
pub trait Surface:
    fn render(&mut self, line: string) -> void

# src/main.hd
use pkg.ui.Surface
pub fn main() -> void $ Surface:
    $.use(Surface).render("hello")

# providers/ui.mjs
export default {
    render(line) { console.log("host drew: " + line); },
};
```

On Node, `hd run` prints `host drew: hello`.

- `[host]` maps each `@host` trait to a provider file that default-exports
  a plain object of methods.
- The manifest must also grant it, as `"pkg.MODULE.Trait"`, `true` or
  `false`.
- On Wasmtime, `hd build --engine wasmtime` also writes `main.host.rs`,
  one Rust trait per host interface, for the embedding application to
  implement. `hd` itself registers none, so it refuses such a module on
  Wasmtime. Not run here.
