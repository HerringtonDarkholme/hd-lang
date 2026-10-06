# `hd doc` Redesign (Task D2)

Status: design proposal, 2026-10-06. Nothing in it is accepted behavior.
Every decision below waits for the owner; the proposed spec text is a
draft and is not applied to `spec/`.

Under review:
- [Documentation](../spec/cli/command-line.md#documentation), the current
  `hd doc` section, rules `cli.doc.*`;
- [Planned: GitHub Pages Workflow](../spec/cli/command-line.md#planned-github-pages-workflow),
  rules `cli.new.pages*`;
- [Machine Output](../spec/cli/command-line.md#machine-output), rule
  `cli.json.kind`;
- [Doc Tests](../spec/lang/10-modules.md#doc-tests) and
  [Documentation Comments](../spec/lang/01-lexical-structure.md#documentation-comments),
  which this design keeps unchanged;
- the `answer-size` metric of
  [Goal Metrics](NEW_COMPILER_ARCHITECTURE.md#goal-metrics-proposal-2026-10-06-awaiting-the-owners-edits).

## Summary

`hd doc` gets two modes. A **query** prints a compact **interface view** to
standard output: the source with bodies removed, plus a few computed lines.
`hd doc --html` writes a static site into `build/doc` for people.

| | Query (agents, terminal) | Site (people) |
| --- | --- | --- |
| Command | `hd doc [QUERY]`, `--format json`, `--all` | `hd doc --html [--open]` |
| Output | standard output only | `build/doc/`, with `llms.txt` and `.md` pages |
| Needs | declarations that parse and resolve | a clean `hd check` |
| Shows | doc comments, decorators, signatures with rows, an errors line, impl heads, doc test names | the same, plus search, cross-links, source pages, rendered examples |

The interface view reuses hd's own syntax: `##` lines are the docs and the
signature is copied as written. An agent already reads that form, so it
costs no new format and few tokens. A full function item is about 280
bytes; a module outline costs about 75 bytes per item.

Other recommendations:
- Fold the unspecified `hd def` into `hd doc`, since every item view starts
  with its file and line.
- Keep `hd explain` apart, and leave a REPL `:doc` to the implementation.
- Make `missing-doc` an opt-in warning of `hd doc --lint`.
- Keep `broken-doc-link` as it is.
- Unblock `hd new --pages`: its workflow runs `hd doc --html` and deploys
  `build/doc`.
- Do not replace the website's std pages, which are the normative stdlib
  tier. Give `lib/std` `##` summaries instead. Later, publish a generated
  std API index beside the spec.

The draft retires 27 `cli.*` rule IDs, keeps 18, and adds 69. The ten
known failures map to sixteen CLI cases, fourteen of them new or rewritten.

## Research

Sources are the tools' own documentation, fetched 2026-10-06.

| Tool | Terminal / agent view | Machine format | Site for people | Examples | Private items | Lint |
| --- | --- | --- | --- | --- | --- | --- |
| Rust: `cargo doc`, docs.rs | none built in; agents read the HTML or the source | rustdoc JSON, `--output-format json`, **nightly only**; docs.rs serves it since 2025-05-23, zstd-compressed, with a `format_version` | `target/doc`, cumulative; search, `[src]` pages, intra-doc links; `--open`, `--no-deps` | doc tests, run by `cargo test`, shown as code blocks | `--document-private-items`, on by default for binaries | `broken_intra_doc_links` and `private_intra_doc_links` warn; `missing_docs` is allowed by default |
| Go: `go doc`, pkg.go.dev | `go doc [pkg.]Sym[.Method]`: the declaration, then its doc comment, indented; `-short` gives one line per symbol, `-all` the whole package, `-src` the body, `-u` unexported | none from `go doc`; `go/doc` extracts docs "from a Go AST" | pkg.go.dev: docs, source links, versions, licenses, search; `go doc -http` serves HTML locally | `ExampleFoo` / `ExampleBar_Qux` functions, checked against an `// Output:` comment | `-u` | none built in; vet and linters outside |
| Deno: `deno doc` | `deno doc file.ts` prints signatures and JSDoc; `--filter` takes a dotted symbol path | `--json`, the input of the deno doc website | `--html`, default `./docs/`, `--name`, `--output`; client-side search; `--symbol-redirect-map` for external links | JSDoc `@example` | `--private` | `--lint`: missing JSDoc, missing explicit return or property types, and exported types that reference private ones; problems exit non-zero |
| Elixir: ExDoc, IEx `h/1` | `h(Enum.map)` prints the spec and the doc; `t/1` types, `b/1` callbacks | — | HTML with search, source links, auto-links into dependencies on hexdocs.pm; v0.40 also writes **Markdown, `llms.txt`**, and EPUB | doctests (`iex>` lines) shown in the doc | `@doc false` hides an item | warns on undefined references; `skip_undefined_reference_warnings_on` silences them |
| Gleam | — | `package-interface.json` beside the HTML, or `gleam export package-interface`: every public type, alias, function and constant, with types and docs | `gleam docs build [--open]`, published by `gleam docs publish` to HexDocs | — | public items only | — |
| `llms.txt` (llmstxt.org) | — | Markdown: an H1 name, a blockquote summary, then H2 sections that list links with notes | — | — | — | — |

What hd takes from each:
- **Go:** the query grammar (`pkg.Sym.Member`), a terse terminal view,
  and an `-all` switch. Go answers from the syntax tree, so a type error in
  a body does not stop a lookup.
- **Gleam:** an interface file is the machine format. hd's
  [package interface](../spec/lang/10-modules.md#package-interfaces)
  already holds signatures, rows, and fact values.
- **Deno:** one command with `--json`, `--html`, and `--lint`.
- **Rust:** `missing_docs` off by default and broken links as warnings.
  Also a cautionary tale: rustdoc JSON has been unstable since its 2020
  tracking issue, so hd should spec its record fields from the start.
- **ExDoc:** Markdown and `llms.txt` next to the HTML, for agents that
  browse a published site.

Sources:
[cargo doc](https://doc.rust-lang.org/cargo/commands/cargo-doc.html),
[rustdoc unstable features](https://doc.rust-lang.org/rustdoc/unstable-features.html),
[rustdoc lints](https://doc.rust-lang.org/rustdoc/lints.html),
[docs.rs rustdoc JSON](https://docs.rs/about/rustdoc-json),
[go command: doc](https://pkg.go.dev/cmd/go#hdr-Show_documentation_for_package_or_symbol),
[go/doc](https://pkg.go.dev/go/doc),
[Go examples](https://go.dev/blog/examples),
[deno doc](https://docs.deno.com/runtime/reference/cli/doc/),
[ExDoc](https://ex-doc.hexdocs.pm/readme.html),
[IEx helpers](https://iex.hexdocs.pm/IEx.Helpers.html),
[Gleam commands](https://gleam.run/documentation/command-line-reference/),
[Gleam package interface](https://gleam-package-interface.hexdocs.pm/),
[llms.txt](https://llmstxt.org/).

## The Example Package

Every example below uses this module. It type-checks with the prototype
(`hd check`), and its doc test passes (`hd test`, 1 passed).

```text
## Placing and storing a shop's orders.

use std.time.{Clock, Timestamp, now}

## Where orders are kept.
pub trait Db:
    fn insert!(mut self, order: Order) -> Result[void, DbError]
    fn find!(self, id: i64) -> Result[Order?, DbError]

## Why the store failed.
@error
pub enum DbError:
    @error("order $id exists")
    Duplicate(id: i64)
    @error("store offline")
    Offline

## Why an order was not placed.
@error
pub enum OrderError:
    @error("cart is empty")
    EmptyCart
    @error(transparent)
    Store(@from error: DbError)

## One placed order.
@derive(Eq)
pub data Order:
    pub id: i64
    pub total_cents: i64
    pub placed: Timestamp

## The total of a cart's prices, in cents.
##
## ```hd
## use pkg.orders.{total}
## use std.testing.assert_equal
##
## assert_equal(total([250, 100]), 350, reason="sums the prices")
## ```
pub fn total(prices: List[i64]) -> i64:
    let sum: i64 = 0
    for price in prices:
        sum = sum + price
    sum

## Places an order for a cart, stamped with the current time.
## An empty cart is [`OrderError.EmptyCart`].
pub fn place!(id: i64, prices: List[i64]) -> Result[Order, OrderError] $ Db + Clock:
    if prices.len() == 0:
        return .Err(.EmptyCart)
    order := Order { id: id, total_cents: total(prices), placed: now() }
    let mut db = $.use(Db)
    db.insert!(order)?
    .Ok(order)
```

It is `src/orders.hd` of package `shop`. Package `shop` also has the
`text` module of the existing `doc-name` fixture, and a dependency `json`.

## Agent And Terminal Mode

### What A Query Prints

One item, in full:

```sh
$ hd doc orders.place
# pkg.orders.place  src/orders.hd:49
## Places an order for a cart, stamped with the current time.
## An empty cart is [`OrderError.EmptyCart`].
pub fn place!(id: i64, prices: List[i64]) -> Result[Order, OrderError] $ Db + Clock
# errors: OrderError = EmptyCart | Store(error)
```

The doc test stays in place, as written, and its test name follows:

```sh
$ hd doc orders.total
# pkg.orders.total  src/orders.hd:41
## The total of a cart's prices, in cents.
##
## ```hd
## use pkg.orders.{total}
## use std.testing.assert_equal
##
## assert_equal(total([250, 100]), 350, reason="sums the prices")
## ```
pub fn total(prices: List[i64]) -> i64
# examples: doc orders.total[0]
```

A type shows its decorators, its members as written, and every
implementation head, marked by where it came from:

```sh
$ hd doc orders.OrderError
# pkg.orders.OrderError  src/orders.hd:20
## Why an order was not placed.
@error
pub enum OrderError:
    @error("cart is empty")
    EmptyCart
    @error(transparent)
    Store(@from error: DbError)
impl Display for OrderError  # @error
impl Error for OrderError  # @error
impl From[DbError] for OrderError  # @error
```

A module shows its documentation, then one summary line and one head line
per item:

```sh
$ hd doc orders
# pkg.orders  src/orders.hd
## Placing and storing a shop's orders.

## Where orders are kept.
pub trait Db
## Why the store failed.
pub enum DbError
## Why an order was not placed.
pub enum OrderError
## One placed order.
pub data Order
## The total of a cart's prices, in cents.
pub fn total(prices: List[i64]) -> i64
## Places an order for a cart, stamped with the current time.
pub fn place!(id: i64, prices: List[i64]) -> Result[Order, OrderError] $ Db + Clock
```

The package view lists the root module's items, the other modules, and the
dependencies. It is the entry point an agent starts from:

```sh
$ hd doc
# shop  hd.toml
# modules:
pkg.orders  ## Placing and storing a shop's orders.
pkg.text  ## Text helpers for URLs and titles.
# dependencies:
dep.json  ## Strict RFC 8259 JSON values and typed decoding.
```

A std item and a std module work the same way, inside a package or outside
one. These assume that `lib/std` gains `##` summaries
([D10](#d10-the-std-reference)); today it has none:

```sh
$ hd doc std.json.decode
# std.json.decode  std/json.hd:624
## `parse`, then `from_json` of the parsed value (std-json.decode).
pub fn decode[T < Deserialize](text: string) -> Result[T, JsonError]
# errors: JsonError = UnexpectedEnd(position) | UnexpectedCharacter(position) | InvalidEscape(position) | LoneSurrogate(position) | ControlCharacter(position) | NumberOutOfRange(position) | NestingTooDeep(position) | WrongType(path, expected) | MissingField(path) | UnknownVariant(path, name)
$ hd doc std.time
# std.time  std/time.hd
## Durations, the clock capability, timestamps, and a manual clock for tests.

## A whole number of milliseconds.
pub data Duration
## A duration of that many milliseconds.
@num_suffix
pub fn ms(count: i64) -> Duration
## A duration of that many seconds.
@num_suffix
pub fn s(count: i64) -> Duration
## The host capability that reads time and waits.
pub trait Clock
## The current time of the covering `Clock` provider.
pub fn now() -> Timestamp $ Clock
## Waits `duration` on the covering `Clock` provider.
pub fn sleep!(duration: Duration) -> void $ Clock
```

The `std.time` listing is cut short here; the full module has 13 items.
The signatures and `@num_suffix` are copied from `lib/std/time.hd`; the
`decode` signature and the ten `JsonError` variants are from
[std/json.md](../spec/std/json.md#typed-json).

### JSON Records

`--format json` prints the same content as JSON lines, one record per
module or item, then the usual summary:

```json
{"kind":"item","path":"pkg.orders.place","item":"function","file":"src/orders.hd","line":49,"signature":"pub fn place!(id: i64, prices: List[i64]) -> Result[Order, OrderError] $ Db + Clock","summary":"Places an order for a cart, stamped with the current time.","doc":"Places an order for a cart, stamped with the current time.\nAn empty cart is [`OrderError.EmptyCart`].","decorators":[],"requirements":["Db","Clock"],"errors":{"type":"OrderError","variants":["EmptyCart","Store(error)"]},"impls":[],"examples":[]}
{"kind":"summary","errors":0,"warnings":0,"passed":0,"failed":0,"ignored":0,"status":0}
```

### Token Budgets

Bytes are what the `answer-size` metric measures. Tokens are estimated as
bytes ÷ 4; code often tokenizes denser. The measured column is the drafts
above.

| Query | Fixture | Measured | Budget |
| --- | --- | ---: | ---: |
| item, a function with a row and an errors line | `orders.place` | 277 B (≈ 70 tok) | ≤ 400 B |
| item, a function with one doc test | `orders.total` | 297 B (≈ 75 tok) | ≤ 400 B |
| item, an error enum with three impl heads | `orders.OrderError` | 318 B (≈ 80 tok) | ≤ 450 B |
| item, a std function whose error has ten variants | `std.json.decode` | 471 B (≈ 120 tok) | ≤ 600 B |
| module, six items | `orders` | 466 B (≈ 115 tok) | ≤ 80 B per item + 60 B |
| module, 13 std items | `std.time` | ≈ 1,060 B (≈ 265 tok) | ≤ 80 B per item + 60 B |
| package, two modules and one dependency | `shop` | 176 B (≈ 45 tok) | ≤ 60 B per line + 30 B |
| one JSON item record | `orders.place` | 603 B | ≤ 2.5 × the text view |

The budgets are for the metric script, not the spec. The format is
normative, so the size follows from the content. A regression then means
the format grew or a computed line got longer.

For comparison, on `orders.place`:

| Format | Bytes | Has location and errors line |
| --- | ---: | --- |
| interface view (recommended) | 277 | yes |
| interface view without those two lines | 193 | no |
| Markdown section, today's `cli.doc.item` | 207 | no |
| Go-style indented text | 226 | no |
| today's prototype output | ≈ 200 | location only |

## Human Mode

`hd doc --html` writes one static site for the package and every package
it depends on, except dev dependencies:

```sh
build/doc/index.html              # the documented packages
build/doc/llms.txt                # the index for agents
build/doc/llms-full.txt           # every page's Markdown in one file
build/doc/shop/index.html         # package shop: root module, modules, dependencies
build/doc/shop/index.md
build/doc/shop/orders.html        # module pkg.orders
build/doc/shop/orders.md
build/doc/json/index.html         # the dependency json
```

Each module page shows:
- the module's documentation, then an index of its items with summaries;
- per item, the signature with each type linked, decorators, the Markdown
  doc, the errors with each variant's `@error` message, and impl heads;
- each doc test as an "Example `doc orders.total[0]`" block, and a
  compile-fail test labelled "Does not compile (CODE)";
- a source link per item, to a source page with line anchors.

The site also has an offline search over item paths and summaries. Links
into `std` go to the toolchain version's std reference on the website. The
look follows the website's stylesheet; the spec leaves styling free.

## What Gets Documented

| Shown | How | Mode |
| --- | --- | --- |
| public items and their public members | as written, by `cli.doc.items` | all |
| `##` doc comments | as written, `##` kept in the view, rendered as Markdown in HTML | all |
| decorators: typed facts such as `@num_suffix` or `@route("/users")`, `@derive`, `@error`, member metadata | as written, unevaluated | all |
| trait-less derivation blocks | as written after the type, since their facts enter the interface | item, HTML |
| derived impls | one impl head per line, `# derived` | item, HTML |
| impls that `@error` generates | one impl head per line, `# @error` | item, HTML |
| hand-written impls, in any module | one impl head per line | item, HTML |
| requirement rows | inside the signature, as written | all |
| inferred rows of private functions | `# inferred: $ Clock`, with `--private` | item |
| error types | `# errors:` line for a `Result[T, E]` result | item, HTML |
| doc tests | in place inside the doc, plus `# examples:` names | item, HTML |
| private items and members | with `--private`, for the package only | all |
| `pub use` re-exports | the `pub use` line, linked to the declaring module | all |

Left out:

| Left out | Why |
| --- | --- |
| function bodies | not API; the header's file and line lead to them |
| ordinary `#` comments | not documentation by `lex.doc.form`; `lib/std` uses them today, so it needs `##` |
| test code: `tests:` blocks, test modules, integration tests | not part of any API |
| tasks, and `src/main.hd` without `--private` | not library modules |
| dev dependencies, in the site | test-only; a query can still read them |
| private fields | replaced by `# private fields omitted`, as today |
| fact values after evaluation | the written expression is shorter and deterministic |
| method lists of trait impls | the trait documents its methods |
| warnings, in a query | they cost tokens on every lookup; `--html` and `--lint` report them |

## Decisions

Each decision lists options and a **Recommendation**. The tier column says
who decides.

| # | Decision | Tier | Decides |
| --- | --- | --- | --- |
| D1 | command shape | CLI | owner |
| D2 | interface view format | CLI | owner |
| D3 | the errors line | CLI | owner |
| D4 | queries on code with type errors | CLI | owner |
| D5 | JSON records | CLI | owner |
| D6 | site layout and flags | CLI | owner |
| D7 | Markdown and `llms.txt` in the site | CLI | owner |
| D8 | doc lint | CLI, plus a new warning code | owner |
| D9 | `hd def`, `hd explain`, REPL `:doc` | CLI | owner |
| D10 | the std reference | stdlib docs and website | owner (site structure); the `##` text is a stdlib call |
| D11 | query details: bare names, prelude types | CLI | agent recommendation; owner may rubber-stamp |
| D12 | styling, search, source pages, colors | implementation | implementation |

### D1. Command Shape

| Option | `hd doc` | Writes the site |
| --- | --- | --- |
| A. today's spec | writes HTML and Markdown into `build/doc` | bare `hd doc`; `hd doc NAME` prints |
| B. query first (Go, IEx) | prints the package view | `hd doc --html` |
| C. two commands | prints | a new `hd site` or `hd doc build` |

**Recommendation: B.** Lookups far outnumber site builds, and an agent's
bare `hd doc` should answer, not write files. Deno's `--html` shows one
command can carry both. C adds a command for a rare action.

### D2. Interface View Format

| Option | Shape | Cost on `orders.place` |
| --- | --- | ---: |
| A. Markdown sections (today) | `## \`place\``, a fenced `hd` signature, the doc | 207 B |
| B. Go style | signature, then the doc indented by four spaces | 226 B |
| C. interface view | `##` doc lines, decorators, the signature as written, then `#` lines that `hd doc` computes | 193 B, 277 B with location and errors |

**Recommendation: C, with its text normative.** It is hd syntax an agent
already reads, and a doc test looks exactly as in source. A normative text
lets CLI cases assert `stdout:` lines and keeps `answer-size` stable. An
implementation may still color it when standard output is a terminal.

### D3. The Errors Line

hd has no checked exception list, so "the errors a function returns" means
the `E` of a `Result[T, E]` result. The question is how much of `E` to
inline.

| Option | `orders.place` shows | `std.json.decode` adds |
| --- | --- | ---: |
| A. type only | `# errors: OrderError` | ≈ 25 B |
| B. variants in pattern form | `# errors: OrderError = EmptyCart \| Store(error)` | ≈ 300 B |
| C. variants with `@error` messages | B plus each message | ≈ 600 B |

**Recommendation: B.** An agent that handles the error writes a `match`,
and pattern form is what an arm needs. That saves a second lookup per
error type. Messages stay in the type's own view and in HTML. A data error
type, or a generic `E`, shows the type only.

### D4. Queries On Code With Type Errors

Agents look things up mid-edit, when the package often fails `hd check`.

| Option | A body with a type error |
| --- | --- |
| A. a query checks the package first (today's `cli.doc.check`) | the query prints nothing and exits 101 |
| B. a query needs only the declarations it prints | the query answers |

**Recommendation: B for queries, A for `--html`.** Go answers from the
syntax tree for the same reason. It also suits the new compiler: a
package interface depends on declarations and fact values only
([`module.interface.determined-facts`](../spec/lang/10-modules.md#r-module.interface.determined-facts)),
so a query can come from cached interfaces. A published site should only
come from code that checks.

### D5. JSON Records

| Option | Shape |
| --- | --- |
| A. one JSON document (the prototype's `{query, symbols, suggestions}`) | a new shape, unlike every other command |
| B. JSON lines with new kinds `module` and `item`, then the summary | the shape of `hd check` and `hd test` |

**Recommendation: B, with fields fixed in the spec.** rustdoc JSON shows
the cost of an unstable format: tools pin `format_version`. In a module or
package view, item records keep the same keys, with `null` or `[]` for
what the text view leaves out.

### D6. Site Layout And Flags

| Question | Options | Recommendation |
| --- | --- | --- |
| output directory | `build/doc` fixed; or `--out DIR` (today) | **fixed**, as `cargo doc` and `gleam docs build` do; the Pages workflow uploads `build/doc` |
| stale files | cumulative (Cargo); or replace | **replace**: the directory holds only this run's output, so a Pages upload has no stale pages |
| dependencies | linked as plain code (today); or documented in the site | **documented**, one directory per package; dev dependencies left out |
| `std` | bundled; or linked to the website | **linked** to the std reference of the toolchain's version |
| `--open` | keep; or drop | **keep**, as Cargo and Gleam do |
| workspace root | needs `-p` (today); or documents every member | **every member** into the root's `build/doc`, `-p` narrows it |
| root module page | `pkg.html` (today); or the package page | **package page** `NAME/index.html`; a module `index` moves to `NAME/index/index.html` |

### D7. Markdown And `llms.txt` In The Site

| Option | Agent files in `build/doc` |
| --- | --- |
| A. today: Markdown section pages, `llms.txt`, `llms-full.txt` | a second format beside the view |
| B. none | a browsing agent must read HTML |
| C. each `.md` page is the module's `--all` interface view in one `hd` fence, plus `llms.txt` and `llms-full.txt` | one format everywhere |

**Recommendation: C.** ExDoc ships Markdown and `llms.txt` for the same
reason: an agent browsing a published site pays less for Markdown than for
HTML. One renderer serves the terminal, JSON, and the `.md` pages.

### D8. Doc Lint

| Option | `missing-doc` on an undocumented `pub` item |
| --- | --- |
| A. off; opt in with `hd doc --lint` (Rust, Deno) | warns only when asked |
| B. a warning in every `hd doc --html` | each site build lists them |
| C. a warning in `hd check` | every agent edit loop pays for it |

**Recommendation: A**, with `missing-doc` as a new warning code in the
[Diagnostics](../spec/README.md#diagnostics) table. It covers public items
and the root module, not fields or variants, which Rust's `missing_docs`
covers but which would flood early packages. `broken-doc-link` stays as
today and is reported by `--html` and `--lint`. Warnings keep exit status 0;
CI reads the summary's `warnings` count from `--format json`.

### D9. `hd def`, `hd explain`, And The REPL

`hd def NAME` and `hd explain CODE` exist in the prototype and in
[the guide](../guide/COMMANDS.md#debug-and-explore), but in no spec rule.
The prototype REPL has `:type` and `:source`, also unspecified.

| Question | Options | Recommendation |
| --- | --- | --- |
| `hd def` | keep; or fold into `hd doc` | **fold**: every item view starts with `FILE:LINE`, so `hd def` repeats a subset |
| `hd explain CODE` | fold in as `hd doc CODE`; or keep | **keep apart**: it reads the spec, not a package; spec it in its own task |
| REPL `:doc NAME` | spec it; or leave it | **leave it to the implementation**: REPL commands are unspecified, and CLI cases do not cover the REPL |

### D10. The Std Reference

The website's std pages are the stdlib-tier chapters `spec/std/*.md`,
rendered by `website/src/pages.ts`. `lib/std` has no `##` comment; its
notes are `#` comments that cite rule IDs.

| Option | Website | Source of truth |
| --- | --- | --- |
| A. status quo | spec chapters only; `hd doc std.*` has no summaries | spec |
| B. generate the std pages from `hd doc --html` of `lib/std`, replacing the spec pages | one implementation's library becomes the reference | `lib/std` |
| C. keep the spec pages; give `lib/std` `##` summaries that end with their rule ID; later publish a generated std API index beside the spec | spec stays normative; the index links each item to its rule | spec, with `lib/std` checked against it |

**Recommendation: C.** B would make one implementation normative and lose
the rule IDs and error examples. That conflicts with implementation-neutral
spec tooling. The `##` summaries are a stdlib-implementation task: one line
per public item, copied from the spec table's meaning column. A script
can then check that each `stdlib-items.tsv` item has a documented
declaration. The generated index waits for the new compiler's `--html`.

### D11. Query Details

These follow from D1 to D4; an agent can settle them.

- **Query form.** A query is a use path: `pkg.orders.place`,
  `dep.json.parse`, `std.time.now`. A path without `pkg`, `dep`, or `std`
  is read as `pkg.` plus the path, matching doc test names such as
  `doc text.slugify`.
- **Bare names.** One identifier that names no root item names the item of
  that name in any module, when exactly one module declares it. Two or more
  is an error that lists them.
- **Prelude and primitive types.** `string.trim`, `List.map`, and
  `println` resolve to the std declarations, so agents need not know that
  `impl string` lives in `std.text`.
- **Dependencies.** `dep.KEY` alone prints that package's view; dev
  dependencies are queryable, since test code uses them.
- **Outside a package.** `std` and prelude queries work; others fail.
- **Missing names.** An error, exit 101; suggestions in the message are
  implementation freedom.

### D12. Implementation Freedom

| Area | Left free |
| --- | --- |
| styling | the website's stylesheet is recommended; dark mode follows the browser |
| search | an offline index, such as a `.js` file loaded by a script tag, so it works from `file://` |
| source pages | rendering, highlighting, and their file names |
| terminal | ANSI color when standard output is a terminal |
| std and dependency file paths in a header line | a stable path the implementation chooses |
| package directory names on a name clash | any distinct names |
| speed | queries should read cached package interfaces, not re-check bodies |

## How It Fits With The Rest

| Piece | Fit |
| --- | --- |
| doc tests | unchanged: `hd test` runs them; the view shows them in place with their `doc module.item[i]` names; HTML renders them as examples |
| `hd test --filter` | the `# examples:` names are its filter values |
| `hd def` | folded into item views (D9) |
| `hd explain` | separate (D9) |
| REPL | `:doc` left to the implementation (D9) |
| `hd new --pages` | its workflow runs `hd doc --html` and uploads `build/doc`; the "not available yet" note goes |
| website std pages | kept; `lib/std` gains `##` summaries; a generated index later (D10) |
| doc lint | `missing-doc` opt-in, `broken-doc-link` unchanged (D8) |
| `answer-size` | budgets in [Token Budgets](#token-budgets) |
| a spec slip | the current [Links](../spec/cli/command-line.md#links) example writes `///`; hd doc comments are `##`; the draft fixes it |

## Proposed Spec Text

A draft for `spec/cli/command-line.md`, ready to apply once the owner
decides D1 to D9. It replaces everything from `## Documentation` up to
`## Machine Output`, and edits four places elsewhere, listed after it.
Existing headings keep their anchors; new headings are added. Rule IDs that
change meaning are retired and replaced, by
[STYLE Stability](../spec/STYLE.md#stability).

The draft is one fenced block, so its links stay unchecked until it is
applied. Its inner fences are written `~~~`; turn each into three
backticks when applying.

```markdown
## Documentation

`hd doc` answers questions about a package's API on standard output, and
writes its documentation site. It builds both from the declarations and
their `##` [documentation comments](../lang/01-lexical-structure.md#documentation-comments):

~~~sh
hd doc                    # the package: root items, modules, dependencies
hd doc orders             # module orders: its doc and an outline of its items
hd doc orders.place       # one item in full
hd doc std.json.decode    # a standard library item
hd doc dep.json.parse     # an item of the dependency json
hd doc --html             # writes the site into build/doc
hd doc --lint             # reports undocumented public items
~~~

### Scope

1. r[cli.doc.mode.query] `hd doc` without `--html` or `--lint` is a **doc query**. It prints to standard output and writes no file.
2. r[cli.doc.mode.html] `hd doc --html` writes the documentation site, by [Output](#output). It prints only diagnostics.
3. r[cli.doc.mode.lint] `hd doc --lint` reports the warnings of [Doc Lint](#doc-lint), and writes no file.
4. r[cli.doc.mode.exclusive] `--html`, `--lint`, and a QUERY exclude each other, and `--all` works only in a doc query. Any other combination is a usage error.
5. r[cli.doc.markdown] The text of a documentation comment is Markdown, as CommonMark defines it.
6. r[cli.doc.items] `hd doc` documents each `pub` item of every module under the source root, with the `pub` members of each, as the table below lists. Items are functions, data types, enums, traits, and type aliases.
7. r[cli.doc.private] `--private` also documents the private items and members. It applies to the package only, in every mode.
8. r[cli.doc.reexport] A `pub use` of a module is listed under that module, with a link to the page of the module that declares the item. It copies no signature and no doc, since the item keeps its [identity](../lang/10-modules.md#r-module.pub-use.identity).
9. r[cli.doc.scope.written] An item's documentation is its documentation comment, its decorators, and its signature, each as written in source.
10. r[cli.doc.scope.omitted] `hd doc` shows none of the parts in the second table below.

| Item | The signature holds |
| --- | --- |
| function | the declaration through its result type and requirement row, with no `:` and no body |
| data type | the declaration head and its `pub` fields; the line `# private fields omitted` stands for the private ones |
| enum | the declaration head and its variants with their payloads |
| trait | the declaration head and the signature of each method |
| type alias | the whole declaration |
| `pub` method of an inherent `impl` | the method's signature, under its type |

| Not shown | Note |
| --- | --- |
| function bodies | an item's [header](#r-cli.doc.view.header) names its file and line |
| ordinary `#` comments | only `##` comments document |
| test code, tasks, and dev dependencies | a doc query may still read a dev dependency, by [`cli.doc.query.dependency`](#r-cli.doc.query.dependency) |
| the values of facts | a decorator shows as written, unevaluated |

> **Why.** Source text is what an agent would read anyway, so the
> documentation copies it rather than restating it.

### Looking Up One Item

~~~sh
hd doc orders.place       # pkg.orders.place
hd doc place              # the one item named place, in any module
hd doc string.trim        # a std method of a primitive type
hd doc dep.json           # the package view of dependency json
~~~

1. r[cli.doc.query.form] A QUERY is a use path, as a `use` declaration writes it, such as `pkg.orders.place`, `dep.json.parse`, or `std.time.now`.
2. r[cli.doc.query.relative] A QUERY that starts with neither `pkg`, `dep`, nor `std` is read as `pkg.` followed by the QUERY, so `orders.place` is `pkg.orders.place`.
3. r[cli.doc.query.member] A path may continue past an item to one member, as in `orders.Order.id` or `time.Timestamp.date`.
4. r[cli.doc.query.bare] A QUERY of one identifier that names no item of the root module names the item of that name in any module of the package, when exactly one module declares one.
5. r[cli.doc.query.ambiguous] When several modules declare such an item, the QUERY is an error whose message lists their paths.
6. r[cli.doc.query.prelude] A QUERY whose first segment is a prelude name or a primitive type names a `std` declaration. So `string.trim` finds the `trim` of std's `impl string`.
7. r[cli.doc.query.dependency] A QUERY that starts with `dep.` and a dependency key names that dependency's `pub` modules and items. Keys of `[dependencies]` and `[dev-dependencies]` both work.
8. r[cli.doc.query.missing] A QUERY that names no documented module, item, or member is an error.
9. r[cli.doc.query.none] `hd doc` without a QUERY prints the [package view](#r-cli.doc.view.package).
10. r[cli.doc.query.outside] Outside any package, a QUERY that starts with `std`, a prelude name, or a primitive type works. Any other doc query is an error there.
11. r[cli.doc.query.workspace] In workspace mode, a doc query needs one member selected by `-p NAME`, as [Selecting Members](#selecting-members) defines. Without `-p` and without a QUERY, `hd doc` lists the members, one line each.
12. r[cli.doc.query.declarations] A doc query needs only the declarations it prints, and the names their signatures use. An error in a function body, or in a declaration it does not print, does not stop it.
13. r[cli.doc.query.no-warnings] A doc query reports errors only, never warnings.

> **Why.** Agents look names up while code is half edited, so a body that
> does not check yet must not hide the docs. Go's `go doc` reads the
> syntax tree for the same reason. A warning would cost tokens on every
> lookup.

### Interface View

A doc query prints the **interface view**: the source with the bodies
removed, plus lines that start with `# ` and that `hd doc` computes.

~~~sh
$ hd doc orders.place
# pkg.orders.place  src/orders.hd:49
## Places an order for a cart, stamped with the current time.
## An empty cart is [`OrderError.EmptyCart`].
pub fn place!(id: i64, prices: List[i64]) -> Result[Order, OrderError] $ Db + Clock
# errors: OrderError = EmptyCart | Store(error)
~~~

1. r[cli.doc.view.header] A view starts with a header line: `# `, the full use path, two spaces, and the source location. The location is `FILE` for a module and `FILE:LINE` for an item or member.
2. r[cli.doc.view.header.file] In the package, `FILE` is the path relative to the package directory. For `std` and a dependency, `FILE` is a stable path that the implementation chooses.
3. r[cli.doc.view.doc] The documentation comment follows, each line as `## ` and the line's text, or `##` for an empty line.
4. r[cli.doc.view.decorators] Each decorator line of the declaration follows, as written.
5. r[cli.doc.view.signature] The signature follows, as written and by the table of [Scope](#scope). A member keeps its own documentation comment and decorators above it.
6. r[cli.doc.view.methods] A data type or enum with `pub` inherent methods shows them after its members, under one `impl NAME:` line, each signature indented by four spaces.
7. r[cli.doc.view.impls] A data type, enum, or newtype lists each implementation head for it, one per line, from its package and every package the query reads.
8. r[cli.doc.view.impls.marks] An implementation that `@derive` or a derivation block creates ends with `  # derived`. One that `@error` generates ends with `  # @error`.
9. r[cli.doc.view.errors] A function or method whose result type is `Result[T, E]` gets the line `# errors: E` after its signature.
10. r[cli.doc.view.errors.variants] When `E` is an enum, that line goes on with ` = ` and its variants in declaration order, joined by ` | `.
11. r[cli.doc.view.errors.payload] A variant with a payload shows its payload members in parentheses, each by its name, or by its type when it has none.
12. r[cli.doc.view.inferred] With `--private`, a private function whose requirement row is [inferred](../lang/11-requirements-and-suspension.md#r-req.row.omitted.inferred-private) and not empty gets the line `# inferred: $ ` and that row.
13. r[cli.doc.view.examples] An item with doc tests ends with the line `# examples: ` and their names, as [`cli.test.doc.name`](#r-cli.test.doc.name) forms them, joined by `, `.
14. r[cli.doc.view.module] A module's view holds its header, its documentation, and an empty line. Then, for each documented item, comes its summary as one `## ` line when it has one, its decorator lines, and its signature's first line, without a final `:`.
15. r[cli.doc.view.package] The **package view** holds the header `# NAME  hd.toml`, then the root module's documentation and items as a module's view holds them. A `# modules:` line and a `# dependencies:` line follow, each only when its list is not empty.
16. r[cli.doc.view.package.lists] Under each of those two lines comes one line per module or dependency: its use path, two spaces, `## `, and its summary.
17. r[cli.doc.view.dependency] A QUERY of `dep.` and a key alone prints that dependency's package view, without its `# dependencies:` part.
18. r[cli.doc.view.all] With `--all`, a module's or package's view shows each item as a doc query of that item would, separated by empty lines.
19. r[cli.doc.view.order] Items appear in source order, modules in path order with `pkg` first, and dependencies in key order.
20. r[cli.doc.view.exact] When standard output is not a terminal, a doc query prints exactly the text these rules define. On a terminal, an implementation may add color.

~~~sh
$ hd doc orders
# pkg.orders  src/orders.hd
## Placing and storing a shop's orders.

## One placed order.
pub data Order
## Places an order for a cart, stamped with the current time.
pub fn place!(id: i64, prices: List[i64]) -> Result[Order, OrderError] $ Db + Clock
~~~

> **Why.** The view is hd syntax that an agent already reads, and a doc
> test looks as it does in source. An exact text keeps the answer small
> and lets a test check it. An error line in pattern form saves a second
> lookup before writing a `match`.

### Doc Records

~~~sh
$ hd doc --format json orders.place
{"kind":"item","path":"pkg.orders.place","item":"function","file":"src/orders.hd","line":49,"signature":"pub fn place!(id: i64, prices: List[i64]) -> Result[Order, OrderError] $ Db + Clock","summary":"Places an order for a cart, stamped with the current time.","doc":"Places an order for a cart, stamped with the current time.\nAn empty cart is [`OrderError.EmptyCart`].","decorators":[],"requirements":["Db","Clock"],"errors":{"type":"OrderError","variants":["EmptyCart","Store(error)"]},"impls":[],"examples":[]}
{"kind":"summary","errors":0,"warnings":0,"passed":0,"failed":0,"ignored":0,"status":0}
~~~

1. r[cli.doc.record.lines] With `--format json`, a doc query writes JSON lines to standard output: one record for each module and item that its view shows, in the same order, then the summary object.
2. r[cli.doc.record.module] A module record has `kind` `"module"`, and the fields `path`, `file`, `summary`, and `doc`.
3. r[cli.doc.record.item] An item record has `kind` `"item"`, and the fields of the table below.
4. r[cli.doc.record.shape] Every item record has every field. A field that the text view leaves out holds `null`, or `[]` for a list.
5. r[cli.doc.record.modes] With `--format json`, `--html` and `--lint` write diagnostics and the summary, as `hd check` does.

| Field | Holds |
| --- | --- |
| `path` | the full use path, as in the header |
| `item` | `"function"`, `"data"`, `"enum"`, `"trait"`, `"alias"`, `"newtype"`, `"method"`, `"field"`, or `"variant"` |
| `file`, `line` | the source location, as in the header |
| `signature` | the signature text, as the view prints it |
| `summary`, `doc` | the summary, and the whole documentation text |
| `decorators` | each decorator line, as written |
| `requirements` | the keys of the requirement row, in source order |
| `errors` | `null`, or an object with `type` and `variants`; `variants` is `null` unless the type is an enum |
| `impls` | one object per implementation head, with `head` and `origin`: `"written"`, `"derived"`, or `"error"` |
| `examples` | one object per doc test, with `name`, `code`, and `compile_fail`, the expected code or `null` |

### Output

~~~sh
hd doc --html             # build/doc, for the package and its dependencies
hd doc --html --open      # also opens build/doc/index.html
hd doc --html --private   # the package's private items too
~~~

1. r[cli.doc.site.dir] `hd doc --html` writes into the directory `doc` of the [build directory](#r-cli.build.directory), as `build/doc`. No flag changes it.
2. r[cli.doc.site.replace] After a run, the directory holds only what that run wrote.
3. r[cli.doc.site.check] `hd doc --html` checks the package as `hd check` does. When that reports an error, it writes no file.
4. r[cli.doc.site.packages] The site documents the package and each package it depends on, except through dev dependencies. Each package's files lie in a directory named after the package.
5. r[cli.doc.site.workspace] In workspace mode, `hd doc --html` documents every member into the workspace root's `build/doc`. With `-p NAME`, it documents only the selected members.
6. r[cli.doc.open] `--open` opens the entry page `index.html` of the output with the platform's default program, after the files are written.
7. r[cli.doc.site.files] The output holds the files of the table below. A module's HTML and Markdown pages sit side by side.
8. r[cli.doc.site.index-module] A top-level module named `index` has its pages at `NAME/index/index.html` and `NAME/index/index.md`, so the package page keeps its names.
9. r[cli.doc.site.main] An application's `src/main.hd` has pages only with `--private`, as the module `main`: `NAME/main.html` and `NAME/main.md`.
10. r[cli.doc.relative] A link between the output's files is a relative path, so the output works from any base path.

| Files | Hold |
| --- | --- |
| `index.html` | the entry page: each documented package, with its root module's summary |
| `NAME/index.html`, `NAME/index.md` | package `NAME`: its package view |
| `NAME/PATH.html`, `NAME/PATH.md` | one pair for each other module; `PATH` is the module path with each `.` a `/`, so module `shop.cart` is `NAME/shop/cart.md` |
| `llms.txt` | the index for agents, by [`cli.doc.llms`](#r-cli.doc.llms) |
| `llms-full.txt` | every Markdown page in one file, by [`cli.doc.site.llms-full`](#r-cli.doc.site.llms-full) |

~~~sh
build/doc/index.html          # the entry page
build/doc/shop/index.md       # package shop, beside index.html
build/doc/shop/orders.md      # module pkg.orders, beside orders.html
build/doc/json/index.html     # the dependency json
build/doc/llms.txt            # the index for agents
~~~

> **Why.** One fixed directory is what a Pages workflow uploads, as
> `cargo doc` writes `target/doc`. Replacing it leaves no page of a deleted
> module behind.

### Pages

1. r[cli.doc.site.markdown] A Markdown page holds the `--all` interface view of its module or package, in one fenced block whose info string is `hd`. Its fence is longer than any run of backticks inside.
2. r[cli.doc.html] A module's HTML page holds everything its Markdown page holds, under the same anchors. Its styling and navigation are not specified.
3. r[cli.doc.summary] An item's **summary** is the first sentence of its documentation, or empty when it has none.
4. r[cli.doc.anchor] An item's anchor is its item name, as [`cli.test.doc.name`](#r-cli.test.doc.name) forms `<item>`, such as `slugify` or `Slug.new`.
5. r[cli.doc.concise] A page holds no function body and no navigation text.
6. r[cli.doc.site.example] On an HTML page, each [doc test](../lang/10-modules.md#doc-tests) appears in place, as written, in an `hd` block. The word `Example` and the doc test's name in code, as [`cli.test.doc.name`](#r-cli.test.doc.name) forms it, come before the block.
7. r[cli.doc.site.example.compile-fail] On an HTML page, a compile-fail doc test is labelled `Does not compile (CODE)`, with its error code, and its block keeps the `# error: CODE` line.
8. r[cli.doc.site.types] On an HTML page, each type and trait name in a signature links to its item.
9. r[cli.doc.site.errors] An item's errors show the error type linked, and each variant with the message its `@error` line gives.
10. r[cli.doc.site.source] Each item links to a page of the site that shows its source file, at the item's line.
11. r[cli.doc.site.search] The site has a search over item paths and summaries that works from the output directory alone, without a server.

### Pages For Agents

1. r[cli.doc.llms] `llms.txt` starts with a heading of the package's name. Then it holds a blockquote with the summary of the root module's documentation, and a `## Modules` list.
2. r[cli.doc.site.llms-list] The list has one entry for each module of each documented package: a link to its `.md` page, a colon, and the module's summary. The package page `NAME/index.md` comes first for each package.
3. r[cli.doc.site.llms-full] `llms-full.txt` holds every Markdown page, package by package in the order of `llms.txt`.

~~~sh
# build/doc/llms.txt:
# # shop
#
# > Tools for the shop's catalog.
#
# ## Modules
# - [shop](shop/index.md): Tools for the shop's catalog.
# - [pkg.orders](shop/orders.md): Placing and storing a shop's orders.
~~~

### Links

1. r[cli.doc.link.form] A documentation comment links to an item with the Markdown shortcut `` [`NAME`] ``, as in `` [`Slug.new`] ``. A Markdown link with a target is ordinary Markdown and is not checked.
2. r[cli.doc.link.scope] NAME resolves in the module scope of the documented item: its own declarations and the names its `use` declarations bind. A dotted NAME names a member or a path through modules.
3. r[cli.doc.link.resolved] A resolved link becomes a link to the target's anchor, on the target's page.
4. r[cli.doc.link.std] A NAME that resolves to a `std` item links to that item in the standard library reference of the toolchain's version.
5. r[cli.doc.link.dependency] A NAME that resolves into a dependency links to the item's page in the dependency's directory of the site.
6. r[cli.doc.link.broken] A NAME that resolves to no item is a warning, as is one that resolves to a private item the run does not document. The text shows as plain code. Warning: `broken-doc-link`.

~~~sh
# in the doc comments of src/text.hd:
# ## Reads what [`slugify`] wrote.      # a link to #slugify on this page
# ## See [`std.json.parse`].            # a link to the standard library reference
# ## See [`sluggify`].                  # warning: broken-doc-link
~~~

### Doc Lint

~~~sh
hd doc --lint                 # warning: missing-doc, for each undocumented pub item
hd doc --lint --format json   # the same warnings as JSON lines, and the summary
~~~

1. r[cli.doc.lint.missing] `hd doc --lint` warns on each `pub` item without a documentation comment, and on a root module without module documentation. Warning: `missing-doc`.
2. r[cli.doc.lint.members] A field, a variant, or a method without a documentation comment gets no warning.
3. r[cli.doc.lint.links] `hd doc --lint` also reports each `broken-doc-link` warning.
4. r[cli.doc.lint.status] `hd doc --lint` exits with status 0 when it reports only warnings, by [`cli.exit.success`](#r-cli.exit.success).

> **Why.** Undocumented items warn only when asked, as Rust's
> `missing_docs` and Deno's `--lint` do. A broken link warns, so a rename
> never blocks a build.

See also: [Doc Tests](../lang/10-modules.md#doc-tests),
[Documentation Comments](../lang/01-lexical-structure.md#documentation-comments),
[Creating A Package](#creating-a-package).
```

Edits outside that section:

| Place | Edit |
| --- | --- |
| chapter intro bullet | "`hd doc`, which answers queries about a package's API and writes its documentation site;" |
| [Commands](../spec/cli/command-line.md#commands) table | `hd doc [--private] [--all] [QUERY]`, `hd doc --html [--private] [--open]`, `hd doc --lint`: answers a [doc query](#documentation), or writes or lints the documentation |
| [Machine Output](../spec/cli/command-line.md#machine-output) | retire `cli.json.kind`; add `r[cli.json.kinds] Each object's kind field is "diagnostic", "test", "module", "item", or "summary".` |
| [Planned: GitHub Pages Workflow](../spec/cli/command-line.md#planned-github-pages-workflow) | delete the "not available yet" note; retire `cli.new.pages.workflow`; add `r[cli.new.pages.deploy] That workflow runs hd doc --html and deploys build/doc to GitHub Pages.`; in the YAML, `run: hd doc --html` and `path: build/doc` |
| [Diagnostics](../spec/README.md#diagnostics) | add the warning `missing-doc` |

The heading "Planned: GitHub Pages Workflow" keeps its name, since anchors
never change. Its word "Planned" then reads oddly; the owner may approve a
one-off rename with a link fix.

### Rule Accounting

| Status | Rule IDs |
| --- | --- |
| kept, same meaning (18) | `cli.doc.markdown`, `cli.doc.items`, `cli.doc.private`, `cli.doc.reexport`, `cli.doc.open`, `cli.doc.relative`, `cli.doc.html`, `cli.doc.summary`, `cli.doc.anchor`, `cli.doc.concise`, `cli.doc.llms`, `cli.doc.link.form`, `cli.doc.link.scope`, `cli.doc.link.resolved`, `cli.doc.link.broken`, `cli.new.pages`, `cli.new.pages.ask`, `cli.new.pages.default` |
| retired (27) | `cli.doc.command`, `cli.doc.package`, `cli.doc.workspace`, `cli.doc.check`, `cli.doc.main`, `cli.doc.dir`, `cli.doc.files`, `cli.doc.root-page`, `cli.doc.index-module`, `cli.doc.page`, `cli.doc.item`, `cli.doc.signature`, `cli.doc.impls`, `cli.doc.example`, `cli.doc.example.compile-fail`, `cli.doc.llms.modules`, `cli.doc.llms-full`, `cli.doc.link.external`, `cli.doc.name`, `cli.doc.name.form`, `cli.doc.name.module`, `cli.doc.name.dependency`, `cli.doc.name.std`, `cli.doc.name.std.output`, `cli.doc.name.missing`, `cli.json.kind`, `cli.new.pages.workflow` |
| added (69) | `cli.doc.mode.*` (4), `cli.doc.scope.*` (2), `cli.doc.query.*` (13), `cli.doc.view.*` (20), `cli.doc.record.*` (5), `cli.doc.site.*` (17), `cli.doc.link.std`, `cli.doc.link.dependency`, `cli.doc.lint.*` (4), `cli.json.kinds`, `cli.new.pages.deploy` |

Where an old rule's intent survives with a new meaning, its successor is:

| Retired | Successor |
| --- | --- |
| `cli.doc.check` | `cli.doc.query.declarations` for a query, `cli.doc.site.check` for the site |
| `cli.doc.name.*` | `cli.doc.query.*` |
| `cli.doc.dir`, `cli.doc.files`, `cli.doc.index-module`, `cli.doc.main` | `cli.doc.site.dir`, `cli.doc.site.files`, `cli.doc.site.index-module`, `cli.doc.site.main` |
| `cli.doc.signature`, `cli.doc.impls` | `cli.doc.view.signature`, `cli.doc.view.impls.marks` |
| `cli.doc.example`, `cli.doc.example.compile-fail` | `cli.doc.site.example`, `cli.doc.site.example.compile-fail` (HTML pages only) |
| `cli.doc.llms.modules`, `cli.doc.llms-full` | `cli.doc.site.llms-list`, `cli.doc.site.llms-full` |
| `cli.doc.link.external` | `cli.doc.link.std`, `cli.doc.link.dependency` |
| `cli.json.kind` | `cli.json.kinds` |
| `cli.new.pages.workflow` | `cli.new.pages.deploy` |

Every added ID was checked against the history of `spec/cli/command-line.md`
and `spec/README.md`; none appears there. The counts are by hand; the
spec pass takes them from `pnpm run spec counts`.

## Conformance Cases

The ten known failures are eight `CLI-DOC` rows and two `CLI-PAGES-HIDDEN`
rows in `test/portable/KNOWN_FAILURES.tsv`. Each maps to new or rewritten
CLI cases. "Asserts" uses the `expect.txt` line kinds of
[CLI Cases](../spec/conformance/README.md#cli-cases).

| Known failure | Replaced by | Asserts | Rules |
| --- | --- | --- | --- |
| `doc-out` | `doc-html` | `hd doc --html`: `file:` `build/doc/index.html`, `build/doc/shop/index.html`, `build/doc/shop/index.md`, `build/doc/shop/text.html`, `build/doc/shop/text.md`, `build/doc/llms.txt`, `build/doc/llms-full.txt`; empty stdout | `cli.doc.mode.html`, `cli.doc.site.dir`, `cli.doc.site.files`, `cli.doc.llms`, `cli.doc.site.llms-full` |
| `doc-out` (stale files) | `doc-html-replace` | a second `--html` run after deleting `src/text.hd`: `no-file: build/doc/shop/text.html` | `cli.doc.site.replace` |
| `doc-name` | `doc-query-item` | `hd doc text.slugify`: exact `stdout:` lines of the view; `hd doc slugify`: the same lines; `hd doc text.missing`: `exit: 101`; `no-file: build/doc` | `cli.doc.mode.query`, `cli.doc.query.form`, `cli.doc.query.relative`, `cli.doc.query.bare`, `cli.doc.query.missing`, `cli.doc.view.header`, `cli.doc.view.doc`, `cli.doc.view.signature`, `cli.doc.view.exact` |
| `doc-name` (module) | `doc-query-module` | `hd doc text` and bare `hd doc`: exact `stdout:` lines | `cli.doc.view.module`, `cli.doc.view.package`, `cli.doc.view.package.lists`, `cli.doc.view.order`, `cli.doc.query.none` |
| `doc-name` (std) | `doc-query-std` | `hd doc --format json std.json.decode`: `stdout-json:` an item with the spec'd `signature` and the ten `JsonError` variants; `hd doc std.json.missing`: `exit: 101` | `cli.doc.query.prelude`, `cli.doc.view.errors`, `cli.doc.view.errors.variants`, `cli.doc.record.item`, `cli.doc.record.shape` |
| `doc-private` | `doc-query-private` | `hd doc text.helper`: `exit: 101`; `hd doc --private text.helper`: exact `stdout:` lines | `cli.doc.private`, `cli.doc.items`, `cli.doc.query.missing` |
| `doc-main-page` | `doc-main-page` (rewritten) | `--html`: `no-file: build/doc/shop/main.html`; `--html --private`: `file:` it and `main.md` | `cli.doc.site.main`, `cli.doc.private` |
| `doc-broken-link` | `doc-broken-link` (rewritten) | `hd doc --html --format json`: `stdout-json:` a `broken-doc-link` warning, then a summary with `status` 0; the page is written | `cli.doc.link.broken`, `cli.doc.record.modes`, `cli.exit.success` |
| `doc-index-module` | `doc-index-module` (rewritten) | `file:` `build/doc/shop/index.html` and `build/doc/shop/index/index.html` | `cli.doc.site.index-module`, `cli.doc.site.files` |
| `doc-outside-package` | `doc-outside-package` (rewritten) | `hd doc --format json`: `exit: 101`, a diagnostic and a summary; `hd doc std.time.now`: exit 0 | `cli.doc.query.outside`, `cli.exit.hd-failure` |
| `doc-check-error` | `doc-check-error` (rewritten) | `hd doc --html --format json`: `exit: 101`, a `type-mismatch` diagnostic, `no-file: build/doc/index.html`; then `hd doc text.slugify`: exit 0 with its view | `cli.doc.site.check`, `cli.doc.query.declarations` |
| — (new) | `doc-query-json` | `hd doc --format json text`: `stdout-json:` one module record, one item record, a summary | `cli.doc.record.lines`, `cli.doc.record.module`, `cli.json.kinds` |
| — (new) | `doc-lint` | `hd doc --lint --format json`: a `missing-doc` warning for the undocumented `pub` function only; status 0; `no-file: build/doc` | `cli.doc.mode.lint`, `cli.doc.lint.missing`, `cli.doc.lint.members`, `cli.doc.lint.status` |
| — (new) | `doc-dependency` | with a path dependency `money`: `hd doc dep.money` exact lines; `hd doc --html`: `file: build/doc/money/index.html` | `cli.doc.query.dependency`, `cli.doc.view.dependency`, `cli.doc.site.packages` |
| `new-pages` | `new-pages` (unchanged) | passes once `--pages` ships | `cli.new.pages`, `cli.new.pages.deploy` |
| `new-pages-existing` | `new-pages-existing` (unchanged) | passes once `--pages` ships | `cli.new.pages`, `cli.new.existing` |

Exact `stdout:` lines are possible because the interface view is
normative (D2). A case whose output comes from `std` docs asserts only
JSON fields the stdlib tier specifies, since `##` text in `lib/std` is the
implementation's own. The frozen prototype would fail every new case, so
each gets a `CLI-DOC` known-failure row until the new compiler passes it.

## Questions For The Owner

Each is one decision; the recommendation is the first answer.

1. **D1, command shape.** Should bare `hd doc` print the package view, with
   `hd doc --html` writing the site? Or keep bare `hd doc` writing pages?
   `hd doc` → `# shop  hd.toml` and a module list.
2. **D2, view format.** Should a doc query print the normative interface
   view (`##` docs, signature as written, computed `#` lines)? Or Markdown
   sections, or Go-style text? The view is 193 B on `orders.place`.
3. **D3, errors line.** Should the errors line list an enum's variants in
   pattern form, or name the type only? `# errors: OrderError = EmptyCart | Store(error)`.
4. **D4, broken code.** Should a doc query answer while a body fails to
   check, with `--html` still needing a clean check?
5. **D5, JSON.** Should `--format json` write JSON lines with new kinds
   `module` and `item`, and the fixed field table?
6. **D6, site layout.** Accept `build/doc` with no `--out`, replace-on-run,
   one directory per package with dependencies documented, `std` linked,
   and `--html` at a workspace root documenting every member?
7. **D7, agent files.** Should each `.md` page be the module's `--all`
   interface view in one `hd` fence, beside `llms.txt` and `llms-full.txt`?
8. **D8, lint.** Add the warning code `missing-doc`, reported only by
   `hd doc --lint`, for `pub` items and the root module, not members?
9. **D9, neighbours.** Fold `hd def` into `hd doc`, keep `hd explain` as a
   separate command to spec later, and leave a REPL `:doc` unspecified?
10. **D10, std reference.** Keep `spec/std` as the website's std pages, add
    `##` summaries to `lib/std`, and later publish a generated std API index
    beside it?

## Parse Log

| Block | Result |
| --- | --- |
| The Example Package, `src/orders.hd` | parses (`hd debug parse`); `hd check` reports `shop: ok`; `hd test` reports `src/orders.hd: 1 passed` |

The `sh`, `json`, and `markdown` blocks are output and spec text, not hd
programs, and were not parsed. Each signature in them was compared with
its source:

| Item | Checked against |
| --- | --- |
| `pkg.orders.*` | the example module above |
| `std.json.decode`, the ten `JsonError` variants and their payload names | `lib/std/json.hd` lines 234-254 and 624; [std/json.md](../spec/std/json.md#typed-json) |
| `std.time` items, `@num_suffix`, `now`, `sleep!` | `lib/std/time.hd` lines 125-155 |
| `string.trim` | `impl string:` in `lib/std/text.hd` |
| `text.slugify`, `text.helper` | the `doc-name` fixture |
| `money` path dependency | the `dep-path-local` fixture |
