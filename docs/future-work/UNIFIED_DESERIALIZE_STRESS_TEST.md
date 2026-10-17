# Unified Deserialize Stress Test

Status: design review, 2026-10-17; changes no decision, design record,
spec text, or compiler code. It tests the owner's question of 2026-10-17
(#350): can `@derive(Deserialize)` with per-concern facts and a separate
`Validate` derive serve every input source by default? It weighs that
against the redesign's separate `Cli` and `EnvConfig` derives
([std.cli](STDLIB_REDESIGN.md#stdcli),
[Typed Environment Config](STDLIB_REDESIGN.md#typed-environment-config)).
The protocol under test is
[The `std.serde` Module](../../spec/lang/14-annotations.md#the-stdserde-module),
[Serde](../../spec/std/serde.md), and
[The `std.structure` Module](../../spec/lang/14-annotations.md#the-stdstructure-module).
Nothing here is accepted behavior.

**Decided by the owner, 2026-10-17.** Not fully unified: a hybrid. `Deserialize` (and `Serialize`) serve the document formats (JSON, YAML, TOML, HTTP payloads, SQL rows, binary formats); the command line and environment keep their own derives, `Cli` and `EnvConfig`; `Validate` is its own derive. Each concern is configured in its own derivation block, `impl Cli for T by Structure: field = [facts]` (UD8). UD1 to UD5 are therefore moot. Answers to the rest:
- UD6: a shared `rename` / `rename_all` in the `Deserialize`/`Serialize` block, read by every document format (a format may override in its own block); `Cli` and `EnvConfig` never read it.
- UD7: an untagged fact; the derived template buffers the value once and tries each variant in order.
- UD9: checked readers: an entry that requires `T < Deserialize & Validate` and validates while reading, so validation cannot be forgotten on that path.
- UD10: a std helper (`read_parsed`) for the three-line `impl Deserialize` of a `Parse` leaf.
- UD11: `ValidationError` carries a member path that each reader maps to its own name (`$.port`, `--port`, `APP_PORT`).
- UD12: `flatten` and `rest` facts.
- UD13: a bytes kind in `Serializer`/`Deserializer`; JSON writes base64.
- UD14: scalar-form facts in a format's block (e.g. unix seconds), newtypes otherwise.

## Contents

- [Verdict Summary](#verdict-summary)
- [Owner Questions, Ranked](#owner-questions-ranked)
- [Surface Being Tested](#surface-being-tested)
- [Method](#method)
- [Cases](#cases)
- [Prior Art](#prior-art)
- [Parse Log](#parse-log)

## Verdict Summary

Each case is split into feature rows, and each row gets one verdict.
**Works**: by default, or with a per-concern fact that one reader reads.
**With additions**: needs something the proposal does not name, such as a
protocol call, a std reader, or a hand-written implementation.
**Breaks**: no form keeps the derived, typed path without changing a
recorded rule, or the default is silently wrong.

| Area | Cases | Rows | Works | With additions | Breaks |
| --- | --- | --- | --- | --- | --- |
| Command lines | C1-C7 | 40 | 26 | 14 | 0 |
| Environment | E1-E2 | 15 | 5 | 10 | 0 |
| One type, three sources | L1-L2 | 14 | 4 | 5 | 5 |
| HTTP payloads | H1-H5 | 24 | 17 | 5 | 2 |
| Config files | T1, Y1, Y2 | 17 | 11 | 6 | 0 |
| Database rows | D1-D2 | 11 | 11 | 0 | 0 |
| Binary format | B1 | 7 | 5 | 2 | 0 |
| Validation | V1 | 9 | 6 | 2 | 1 |
| **Total** | **23** | **137** | **85** | **44** | **8** |

By case, taking each case's worst row: **breaks** in L1, L2, H3, H4, and
V1; **works** in H1, D1, and D2; **with additions** in the other fifteen.
The ranked table below names the cases each problem hits.

**Conclusion.** The model holds for formats that hold a whole document
before the type reads it: JSON, YAML, TOML, query strings, forms,
headers, SQL rows, and buffered protobuf. Each is a plain `Deserializer`
whose per-format needs are facts that only it reads; every database row
passes. The command line is where it strains: the protocol hands the
reader one member at a time, with no kind, default, or nested shape (UD1).

The proposal's first enabler names the right need but the wrong place.
`Members.infos` reaches the template's own `Source`, never the format's
`Deserializer`, and `Member` carries no type. Nested data, maps, and
layering in text sources come next (UD4, UD5). Validation as its own derive
works, cross-field rules included, through a derivation block's method.

## Owner Questions, Ranked

Ranked by how many cases a problem hits and how silent it is. Each gives
the effect, candidates, a labeled recommendation, and an example: a block
of its own, or the case block that shows the problem.

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | UD1 | The reader never sees the schema before values | breaks the CLI | C1-C3, C7, E1, L2, H5 |
| 2 | UD2 | Shape errors move from compile time to run time | silent until run | C1, C3, C5, L2 |
| 3 | UD3 | Every member is readable from every source | silent, security | H3, L1 |
| 4 | UD4 | Nested data, maps, and collisions in text sources | silent collisions | C2, E1, E2, L1, L2 |
| 5 | UD5 | Layering needs absence at every depth | high | C3, E1, L1 |
| 6 | UD6 | Names repeat across document formats | medium | H1, T1, Y2, L2 |
| 7 | UD7 | Untagged unions cannot be derived | medium | H2, T1, Y1, Y2 |
| 8 | UD8 | Where per-concern facts live | principle | every case with three concerns |
| 9 | UD9 | A forgotten `.validate()` is silent | silent | V1 |
| 10 | UD10 | A `Parse` type is not a `Deserialize` leaf | medium | C4, C5, C6 |
| 11 | UD11 | Validation errors lose the source's name | medium | V1, E1, L1 |
| 12 | UD12 | No flatten and no catch-all member | medium | H5, B1 |
| 13 | UD13 | No bytes in the data model | medium | H4, B1 |
| 14 | UD14 | A field cannot choose its type's encoding | low | H2 |

### UD1. The reader never sees the schema before values

**Effect.** A CLI reader cannot split `-r eu billing`, print help, reject
`--colour`, bundle `-iv`, or take a global flag after a subcommand without
every member's kind first (C1-C3, C7). The template calls `begin_variant`,
then `member(m)` one at a time, and `Member` has no type, kind, or
default. An env reader must then snapshot `Env.names()`, losing the
redesign's least-authority read (E1).

1. Should `Deserialize` gain `fn schema() -> Shape`, derived through
   `T::describe` and cached per type? It answers the open
   [Schemas](OPEN_ISSUES.md#serialization-formats) question too.
2. Should the template pass one level of member shapes after
   `begin_variant`, through a `members(shapes)` call?
3. Should the command line keep `@derive(Cli)`, with every other source
   on `Deserialize`?

**Recommendation.** Candidate 1. Candidate 2 cannot list a subcommand's
options in top-level help. Defaults in help need `F < Serialize` in the
describer, and the cache waits on the spec's undecided derived-function
cache.

```text
pub enum Shape:
    Flag
    Value(type_name: string)
    Choices(names: List[string])
    List(item: Shape)
    Map(value: Shape)
    Optional(inner: Shape)
    Record(members: List[(Member, Shape, string?)])
    Commands(variants: List[(VariantInfo, Shape)])
```

### UD2. Shape errors move from compile time to run time

**Effect.** `cli.parse::[T]` accepts any `T < Deserialize`, so a
`List[Pool]` field or `@cli.short` on a record compiles and fails at run
time (L2). The redesign rejects both at `@derive`, through
`member-not-derivable` and the typed fact `Short[F < Cli]`. A fact that
names a field by string, as `@cli.conflicts_with("verbose")`, is checked
by nothing (C1, C3, C5).

1. Should the reader check the schema on its first call, with a std test
   helper `cli.check::[T]()`?
2. Should each text source add a shape trait that readers also require?

**Recommendation.** Candidate 1, and only with UD1 candidate 1. Candidate
2 is a separate derive under another name, as UD1 candidate 3 is.

```text
tests:
    it("the command line has a valid shape"):
        cli.check::[Cargo]().expect("valid")
```

### UD3. Every member is readable from every source

**Effect.** By default `database_password` becomes `--database-password`,
which other users see in `ps` and shell history keeps (L1). A stored type
reused as a request body lets the body set `role`, the mass-assignment hole
of GitHub's 2012 Rails incident (H3).

1. Default on, with an opt-out fact per source, as `@cli.skip`?
2. Default on for env and documents, with flags only by a cli fact?
3. A neutral sensitivity fact that the CLI refuses and help hides?

**Recommendation.** Candidate 1, with guidance that a request body gets
its own input type. Candidate 3 waits for the parked
[secret values](OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets).
Example: `database_password` in the L1 block.

### UD4. Nested data, maps, and collisions in text sources

**Effect.** The proposal gives CLI and env no rule for records or maps, so
`pool: Pool` has no variable name (E1, L1). With the common `_` join,
`pool.max` and a member `pool_max` both read `APP_POOL_MAX`, and the first
match wins silently (L2). Kubernetes' `FOO=""` into `string?` needs a rule
too (E2).

1. Join with `_` for env and `-` for flags, as Go's envconfig does, and
   report collisions when the schema is built?
2. Join with `__` and `.`, as pydantic-settings and kotlinx's Properties
   format do, so no collision can happen?
3. Refuse nested data in text sources unless a prefix fact names it?

**Recommendation.** Candidate 1 with the collision check, which needs
UD1; 12-factor apps already name `POOL_MAX`. Maps read `k=v` per flag and
`k=v,k2=v2` per variable; an empty variable is `.None`. Example: `pool`
in the E1 block beside a member `pool_max: u32`.

### UD5. Layering needs absence at every depth

**Effect.** Flag over env over file over default must know, per member at
every depth, which layer holds a value (C3, E1, L1). Alone, each reader
fails on a required member that another layer would supply. Figment's CLI
recipe merges `Serialized::defaults(Config::parse())` first, so clap's
filled-in defaults sit below the file.

1. Should std ship `layered::[T](readers)`, a `Deserializer` that asks each
   reader in turn at every depth, with `is_absent()` asked for every member?
2. Should programs merge whole values with `.or()`, as the redesign does?
3. Should readers build dynamic trees that std merges, as figment does?

**Recommendation.** Candidate 1. It keeps absence exact with no copy.

```text
fn load_config!(path: Path) -> Result[AppConfig, ConfigError] $ Args + Env + FsRead:
    layered::[AppConfig]([cli.source(), env.source(), json.source(read_text!(path)?)])
```

### UD6. Names repeat across document formats

**Effect.** JSON, YAML, TOML, and MessagePack are keyed by name, and each
needs its own rename. A Kubernetes object repeats its camelCase fact for
JSON and YAML (Y2), and GitHub's `+1` key repeats for a YAML fixture (H1).
Yet Cargo writes `rust-version` in TOML and `rust_version` in
`cargo metadata` JSON (T1).

1. Per-format names only, as proposed?
2. Neutral `@serde.rename` and `rename_all` that document formats read and
   may override, and text sources never read, as the
   [Serialization Formats](OPEN_ISSUES.md#serialization-formats) plan has?
3. A neutral rename that every source reads, as pydantic's `alias`?

**Recommendation.** Candidate 2. pydantic-settings documents that an
alias applies "in every source", which hides values a lower-priority
source keeps under the field's own name.
Example: the camelCase fact in the Y2 block.

### UD7. Untagged unions cannot be derived

**Effect.** Stripe's expandable `customer`, Cargo's string-or-table
dependency, Kubernetes' int-or-string port, and an Actions `on:` pick a
variant by the value's shape (H2, T1, Y1, Y2). Each needs a hand-written
`Deserialize` with `peek`, since `begin_variant` returns an index before
any member is read. Internally tagged enums work: the reader finds the tag.

1. Keep hand-written implementations, with a std helper per pattern?
2. Should `@serde.untagged` make the derived template buffer the value and
   try each variant, as serde's `Content` does, at the cost of one copy?
3. Per-format facts? A reader cannot try variants, so this needs 2 anyway.

**Recommendation.** Candidate 2; six rows in four cases need it. Text
sources cannot use it, since their `peek` is always `Text`.
Example: `Expandable` in the H2 block.

### UD8. Where per-concern facts live

**Effect.** One field may carry `@cli.short`, `@env.name`, `@json.rename`,
and `@range` (E1, L1). The owner keeps configuration per concern, and the
redesign gave `Cli` and `EnvConfig` a block each. The unified model has one
`Deserialize` block and at most one block with no trait
([`annot.no-trait.unique`](../../spec/lang/14-annotations.md#r-annot.no-trait.unique)).

1. Facts at the declaration, namespaced by module, accepting the stack?
2. One block with no trait per concern, relaxing `annot.no-trait.unique`?
3. Separate derives per concern, each with its own block?

**Recommendation.** Candidate 1. Each module reads only its own facts, so
the concerns stay apart; only their placement is shared. Example: `port`
in the E1 block, with an env fact and a validation fact.

### UD9. A forgotten `.validate()` is silent

**Effect.** `cli.parse::[Deploy]()?` without `.validate()` compiles and
runs with `port: 0` (V1). pydantic validates while it builds, so a model
value is always valid.

1. Keep the explicit call, as proposed, and rely on tests?
2. A checked entry, `cli.parse_valid::[T < Deserialize & Validate]()`?
3. Single-field rules in constrained newtypes, such as a `Port` whose
   `Parse` refuses `0`, with `Validate` kept for cross-field rules?

**Recommendation.** Candidate 3, the redesign's principle 8: newtypes make
invalid states unrepresentable.
Example: `age` in the V1 block, whose `@range` a newtype would replace.

### UD10. A `Parse` type is not a `Deserialize` leaf

**Effect.** curl's `Name: value` header and docker's `src:dst` mount have
`Parse` (C4, C6), but a blanket implementation would overlap every derived
one. So each writes `impl Deserialize` that reads text and parses, and JSON
reads it as text too. kubectl's `-o jsonpath=...` is a data enum, which
the default reads as subcommands (C5).

1. A std helper, `read_parsed::[T, R](input, name)`, for a three-line impl?
2. A std `TextLeaf` derive that `Deserialize` defers to?
3. A cli fact, `@cli.value`, that reads a data enum through `Parse`?

**Recommendation.** Candidate 1; one text form across formats is usually
wanted, as `"30s"`.

```text
impl Deserialize for Header:
    fn deserialize[R < Deserializer](input: mut R) -> Result[Header, R::Error]:
        read_parsed::[Header, R](input, "Header")
```

### UD11. Validation errors lose the source's name

**Effect.** `validate` runs on the built value, so its error says `port`,
never `--port`, `APP_PORT`, `$.port`, or which layer supplied it (V1, E1,
L1).

1. A member path in `ValidationError` that each reader maps to its own
   name, as figment's `Metadata` maps a key path to a native key?
2. Readers that check validation facts while they read?
3. Field paths only, as pydantic's error `loc`?

**Recommendation.** Candidate 1. Candidate 2 puts `Validate` into every
reader.
Example: the `@range` error of the E1 block.

### UD12. No flatten and no catch-all member

**Effect.** problem+json keeps extension members beside its standard ones,
and JSON writes an embedded `Problem` as a nested key (H5). A catch-all
needs every other member's name first, and protobuf keeps unknown fields to
write them back (B1).

1. Neutral `@serde.flatten` and `@serde.rest` facts, given UD1's members?
2. Flatten every embedded member, changing
   [`std-json.derive.to-json.embedded`](../../spec/std/json.md#r-std-json.derive.to-json.embedded)?
3. Hand-written implementations only?

**Recommendation.** Candidate 1. Row readers flatten records anyway.
Example: `extensions` and the embedded `Problem` in the H5 block.

### UD13. No bytes in the data model

**Effect.** A multipart file, a protobuf `bytes` field, a SQL `BYTEA`, and
a MessagePack `bin` have no kind (H4, B1). `List[u8]` costs one call per
byte, and JSON writes an array of numbers.

1. Add `bytes` to `Serializer`, `Deserializer`, and `ValueKind`, with JSON
   writing base64, as serde's `serialize_bytes`?
2. Keep `List[u8]`, special-cased by readers?
3. Per-format leaf types, such as a multipart `FilePart`?

**Recommendation.** Candidate 1, in the language chapter, which lists the
trait declarations
([`annot.serde.module`](../../spec/lang/14-annotations.md#r-annot.serde.module)).
Example: `bytes` in the H4 block.

### UD14. A field cannot choose its type's encoding

**Effect.** Stripe's `created` is unix seconds, but `Timestamp` reads RFC
3339 text (H2). A field's facts reach the reader, never the field type's
`deserialize`.

1. A newtype per encoding, as `type UnixSeconds(Timestamp)`?
2. Reader-level facts for scalar forms, as the open plan's
   `@json(repr=...)`?

**Recommendation.** Candidate 2 for scalar forms, candidate 1 otherwise.
Example: `created` in the H2 block.

## Surface Being Tested

The examples read the proposal this way where it leaves an API open:

- Readers take any `T < Deserialize`: `cli.parse::[T]()` with `$ Args`,
  `env.load::[T]()` with `$ Env`, `json.decode::[T](text)`, and package
  readers such as `toml.decode`, `query.decode`, and `db.query_as`.
- Names default to kebab flags, upper snake variables, and verbatim keys.
  Every `bool` flag also takes a `--no-` form; env splits a list on commas.
- A fact `module.name(...)` is read by that module's reader only.
  Validation facts belong to `Validate`; any reader may read them for help.
- Enablers: the reader gets every member and its kind before values, and
  text sources convert a leaf's text to the scalar asked for.
- A `Deserializer` runs inside pure `build`, so it does no I/O
  ([`annot.structure.pure`](../../spec/lang/14-annotations.md#r-annot.structure.pure)).
- `Validate` declares `fn check(self) -> Result[void, ValidationError]`
  with a default body, which a derivation block may write
  ([`annot.block.methods`](../../spec/lang/14-annotations.md#r-annot.block.methods)).

| Need | Today | Proposal |
| --- | --- | --- |
| Member list before values | only the template's `Source` sees `Members.infos` | the reader sees it |
| Member kind | absent; `Member` has no type | added |
| Member default | `Field.has_default()` and `default()`, inside templates only | not stated |
| Doc comments | `Member.doc`, `VariantInfo.doc` | used for help |

## Method

Cases approximate the public surface of real tools and APIs; they are not
copies. Each row gets one verdict, and a case's verdict is its worst
row's. Blocks show a subset of members, and rows may cover members a block
leaves out. Every `text` block is checked with
`scripts/hd-conformance.sh parse`, which checks syntax only. Every fact is
hypothetical API, and each line that attaches one ends in
`# hypothetical syntax`; the decorator syntax itself is specified.

## Cases

### C1. ripgrep

Surface: `rg [-i] [--no-hidden] [-u...] [-g GLOB]... [--color WHEN] [-e PAT]... [PAT] [PATH...]`.

```text
## Search files for a regular expression.
@derive(Deserialize)
data Rg:
    ## Search without case.
    @cli.short('i')  # hypothetical syntax
    ignore_case: bool
    @cli.short('u')  # hypothetical syntax
    @cli.count  # hypothetical syntax
    unrestricted: u8 = 0
    regexp: List[string]
    @cli.positional  # hypothetical syntax
    @cli.required_unless("regexp")  # hypothetical syntax
    pattern: string?
    @cli.positional  # hypothetical syntax
    path: List[Path]
```

| Feature | Verdict | How |
| --- | --- | --- |
| `-iu` bundling | works | the enabler's kinds say both take no value |
| `--no-hidden` | works | the `--no-` rule |
| `-uuu` counted | works | `@cli.count`; the reader answers `uint()` with the count |
| `-g` repeated | works | `List[T]` default |
| `--color` choices | works | a payload-free enum |
| PAT unless `-e` | with additions | the fact names a field by string, unchecked (UD2) |
| help shows `[default: auto]` | with additions | `Member` holds no default (UD1) |

### C2. git commit and clone

Surface: `git [-C path] [-c k=v]... commit [-m msg]... [--] [pathspec...]`
and `git clone [--recurse-submodules[=spec]] repo`.

```text
@derive(Deserialize)
data Git:
    config: Map[string, string]
    command: GitCommand

@derive(Deserialize)
enum GitCommand:
    ## Record changes to the repository.
    Commit(CommitArgs)
    Clone(CloneArgs)

@derive(Deserialize)
data CloneArgs:
    @cli.missing_value(".")  # hypothetical syntax
    recurse_submodules: string?
    @cli.positional  # hypothetical syntax
    repository: string
```

| Feature | Verdict | How |
| --- | --- | --- |
| subcommands | works | data-enum default; a record payload holds the command's options |
| `-C` before the subcommand | works | reader default |
| `-c name=value` | with additions | `Map` has no kind or flag form (UD4) |
| `--recurse-submodules[=spec]` | works | `@cli.missing_value` |
| `--` then pathspec | works | reader default plus a fact |
| `git commit -h` | with additions | the subcommand's shape is needed before reading (UD1) |

A variant payload member cannot declare a default (the parser rejects it),
so a subcommand with defaulted options needs a record payload, as in clap.

### C3. cargo build

Surface: `cargo [-v...] [-q] [-Z flag]... build [--features f,g]... [--no-default-features]`, with `b` for `build`.

```text
@derive(Deserialize)
data Cargo:
    @cli.count  # hypothetical syntax
    @cli.global  # hypothetical syntax
    verbose: u8 = 0
    @cli.conflicts_with("verbose")  # hypothetical syntax
    quiet: bool
    command: CargoCommand

@derive(Deserialize)
enum CargoCommand:
    @cli.alias("b")  # hypothetical syntax
    Build(BuildArgs)

@derive(Deserialize)
data BuildArgs:
    @cli.split(',')  # hypothetical syntax
    features: List[string]
    no_default_features: bool
```

| Feature | Verdict | How |
| --- | --- | --- |
| `cargo build -v`, a global after the subcommand | with additions | the subcommand reader needs the parent's shape (UD1) |
| `-q` conflicts with `-v` | with additions | the fact names a field by string (UD2) |
| `-Z` hidden | works | fact |
| `b` alias | works | variant fact |
| `--features a,b --features c` | works | `@cli.split` |
| `--no-default-features` | with additions | a member `default_features: bool` would claim the same flag; only a schema check finds it (UD2) |
| `CARGO_BUILD_JOBS` and `.cargo/config.toml` | with additions | layering (UD5) |

### C4. curl

Surface: `curl [-sSL] [-H "Name: value"]... [-d data]... [-o file|-] URL...`.

```text
@derive(Deserialize)
data Curl:
    @cli.short('H')  # hypothetical syntax
    header: List[Header]
    @cli.short('d')  # hypothetical syntax
    `data`: List[string]
    @cli.short('o')  # hypothetical syntax
    output: string?
    @cli.positional  # hypothetical syntax
    url: List[string]
```

| Feature | Verdict | How |
| --- | --- | --- |
| `-sSL` bundling | works | member kinds |
| `-H` into a `Parse` type | with additions | a hand-written `Deserialize` (UD10) |
| `--data`, a reserved word | works | raw identifier `` `data` `` |
| `-o -` | works | reader default: a lone `-` is a value |
| `-d @file` | works | the program reads the file after parsing |

### C5. kubectl get

Surface: `kubectl [-n ns] [--context c] get TYPE [NAME...] [-o json|yaml|jsonpath=T] [-A]`.

```text
@derive(Deserialize)
data GetArgs:
    @cli.short('o')  # hypothetical syntax
    output: OutputFormat?
    @cli.conflicts_with("namespace")  # hypothetical syntax
    all_namespaces: bool
    resource: List[string]

enum OutputFormat:
    Json
    Jsonpath(template: string)
```

| Feature | Verdict | How |
| --- | --- | --- |
| `-n`, `--context` on every subcommand | works | `@cli.global` |
| `-o jsonpath=...`, a data enum as a value | with additions | the default reads subcommands; needs `Parse` (UD10) |
| `-A` conflicts with the parent's `-n` | with additions | a string names another level's field (UD2) |
| `TYPE/NAME` and `TYPE NAME` | works | the program reads the strings |
| `-l` selector | works | a string, as kubectl keeps it |

### C6. docker run

Surface: `docker run [-it] [--rm] [-e K=V|-e K]... [-v src:dst]... IMAGE [COMMAND] [ARG...]`.

```text
@derive(Deserialize)
data Run:
    @cli.short('e')  # hypothetical syntax
    env: List[EnvPair]
    @cli.short('v')  # hypothetical syntax
    volume: List[Mount]
    @cli.positional  # hypothetical syntax
    image: string
    @cli.trailing  # hypothetical syntax
    command: List[string]
```

| Feature | Verdict | How |
| --- | --- | --- |
| `-it` | works | member kinds |
| `-e K=V`, `-v a:b`, `-p 1:2` into `Parse` types | with additions | UD10 |
| `-e K` copies the host's variable | works | `EnvPair.value: string?`; the program reads `Env` after parsing |
| options after IMAGE belong to COMMAND | works | `@cli.trailing` |

### C7. Help, version, hidden options, aliases

These rows reuse the C1-C3 types.

| Feature | Verdict | How |
| --- | --- | --- |
| `##` doc comments as help | works | `Member.doc` and `VariantInfo.doc` exist today |
| `-h` lists every option of its level | with additions | UD1 |
| `--version` text | with additions | a type-level fact holds it; hd has no build-time version constant |
| hidden options | works | fact |
| field and variant aliases | works | fact |
| help names each option's variable | works | the CLI reader reads `env.*` facts, as pydantic-settings can |

### E1. A 12-factor service

```text
@derive(Deserialize, Validate)
@env.prefix("ORDERS_")  # hypothetical syntax
data Settings:
    database_url: string
    @env.name("PORT")  # hypothetical syntax
    @range(1, 65535)  # hypothetical syntax
    port: u16 = 8080
    pool: Pool = Pool {}
    allowed_origins: List[string]
    request_timeout: Duration = 30s
    sentry_dsn: string?
```

| Feature | Verdict | How |
| --- | --- | --- |
| required `ORDERS_DATABASE_URL` | works | default; the error names the variable |
| `PORT` without the prefix | works | fact |
| `ORDERS_ALLOWED_ORIGINS=a,b` | works | comma default |
| `ORDERS_REQUEST_TIMEOUT=30s` | with additions | `Duration` reads milliseconds today; the redesign's text form is pending |
| unset `sentry_dsn` | works | `T?` default |
| `ORDERS_POOL_MAX` | with additions | no nested rule (UD4) |
| only `ORDERS_POOL_MAX` set | with additions | `is_absent` for `pool` must scan the prefix (UD4) |
| read only declared names | with additions | without a schema the reader snapshots `Env.names()` (UD1) |
| `DATABASE_URL_FILE` secrets | with additions | a reader does no I/O; a file layer goes first (UD5) |
| a range error names `PORT` | with additions | UD11 |

### E2. Kubernetes env injection

The pod injects `PRIMARY_DB_URL` and `REPLICA_DB_URL` from secrets,
`POD_NAME` from the downward API, and `LABELS="team=pay,tier=backend"`.

```text
@derive(Deserialize)
data PodSettings:
    @env.prefix("PRIMARY_DB_")  # hypothetical syntax
    primary: Database
    @env.prefix("REPLICA_DB_")  # hypothetical syntax
    replica: Database?
    pod_name: string
    labels: Map[string, string]
```

| Feature | Verdict | How |
| --- | --- | --- |
| one type under two prefixes | with additions | a prefix fact, once records have a rule (UD4) |
| `replica` absent with no `REPLICA_DB_*` | with additions | UD4 |
| `LABELS=k=v,k2=v2` | with additions | no map rule (UD4) |
| `FOO=""` into `string?` | with additions | no empty-value rule (UD4) |
| `$(VAR)` expansion | works | the kubelet expands it first |

### L1. One type from a JSON file, the environment, and flags

```text
@derive(Deserialize)
@json.rename_all("camelCase")  # hypothetical syntax
@env.prefix("APP_")  # hypothetical syntax
data AppConfig:
    listen_addr: string = "0.0.0.0:8080"
    pool: Pool = Pool {}
    @cli.name("label")  # hypothetical syntax
    labels: Map[string, string]
    database_password: string
    upstream: Upstream

@derive(Deserialize)
enum Upstream:
    Direct(url: string)
    Proxy(url: string, token: string)
```

| Feature | Verdict | How |
| --- | --- | --- |
| `listenAddr`, `APP_LISTEN_ADDR`, `--listen-addr` | works | each reader's default, plus one fact |
| `--label k=v` for `labels` | works | fact |
| `database_password` exposed as a flag | breaks | UD3 |
| flag over env over file over default | with additions | UD5 |
| file sets `pool.maxSize`, env sets `APP_POOL_MIN` | with additions | UD5 |
| `--pool.max-size` or `--pool-max-size` | with additions | UD4 |
| `labels` from env | with additions | UD4 |
| `upstream`, a data enum field, from env or flags | breaks | the CLI reads a data enum as subcommands; no text form exists |
| the error names `APP_POOL_MIN`, not `pool.min` | with additions | UD11 |
| absence kept through layers | works | `is_absent` exists, unlike clap's filled-in defaults |

### L2. Failure modes

These rows reuse the E1 and L1 types.

| Feature | Verdict | How |
| --- | --- | --- |
| `cli.parse` of a type with `List[Pool]` | breaks | compiles, then fails at run time (UD2) |
| `@cli.short` on a member of type `Pool` | breaks | a typed fact can only bound `F < Deserialize` (UD2) |
| `pool_max` and `pool.max` both `APP_POOL_MAX` | breaks | silent first match (UD4) |
| one fact, two meanings | works | each module reads its own facts; a shared rename brings the problem back (UD6) |

A type that cannot be a command line at all, such as records in lists,
is the first row: nothing rejects it before it runs. A schema built on
every call costs one describe pass, O(members), unless a per-type cache
holds it (UD1).

### H1. GitHub REST: an issue

```text
@derive(Deserialize)
data Issue:
    number: u64
    body: string?
    state: IssueState
    closed_at: Timestamp?
    pull_request: PullRequestRef?
    @json.rename("+1")  # hypothetical syntax
    plus_one: u32

@derive(Serialize)
data CreateIssue:
    title: string
    @json.skip_none  # hypothetical syntax
    milestone: u64?
```

| Feature | Verdict | How |
| --- | --- | --- |
| snake keys | works | verbatim default |
| unknown keys | works | ignored ([`std-json.derive.from-json.unknown`](../../spec/std/json.md#r-std-json.derive.from-json.unknown)) |
| `null` or a missing key into `T?` | works | `is_null`, `is_absent` |
| `"open"` variants | works | a per-format `rename_all` fact |
| the `+1` key (nested under `reactions` in the real API) | works | per-format fact, repeated per format (UD6) |
| `milestone: null` clears the milestone | works | `skip_none`; without it the write is silently wrong |

### H2. Stripe: a charge

```json
{"id": "ch_1", "object": "charge", "created": 1679090539, "customer": "cus_9", "status": "succeeded",
 "metadata": {"order_id": "6735"}, "payment_method_details": {"type": "card", "card": {"brand": "visa"}}}
```

```text
@derive(Deserialize)
data Charge:
    @json.repr("unix_seconds")  # hypothetical syntax
    created: Timestamp
    customer: Expandable[Customer]?

enum Expandable[T]:
    Id(string)
    Object(T)

impl[T < Deserialize] Deserialize for Expandable[T]:
    fn deserialize[R < Deserializer](input: mut R) -> Result[Expandable[T], R::Error]:
        match input.peek()?:
            .Text => .Ok(Expandable.Id(input.text("Expandable")?))
            _ => .Ok(Expandable.Object(T::deserialize(input)?))

@derive(Deserialize)
@json.tag("type")  # hypothetical syntax
enum PaymentDetails:
    Card(card: Card)
```

| Feature | Verdict | How |
| --- | --- | --- |
| `object: "charge"` | works | an unknown key, ignored |
| `created` in unix seconds | with additions | UD14 |
| expandable `customer` | with additions | hand-written with `peek` (UD7) |
| a status added after the client shipped | works | `@json.other`: the reader returns that variant's index |
| `{"type": "card", "card": {...}}` | works | internal tag; the payload member `card` reads its key from the same object |

### H3. Query strings, forms, and headers

```text
## GET /search?q=rust&page=2&tag=web&tag=cli
@derive(Deserialize)
data Search:
    page: u32 = 1
    @query.name("tag")  # hypothetical syntax
    tags: List[string]

@derive(Deserialize)
data WebhookHeaders:
    @header.name("X-GitHub-Event")  # hypothetical syntax
    event: string
    content_type: string
```

| Feature | Verdict | How |
| --- | --- | --- |
| `tag=web&tag=cli` into a list | works | the reader collects repeated keys; `serde_urlencoded` cannot, so axum-extra uses `serde_html_form` |
| a form checkbox: `on`, or absent | works | the form reader's `bool` rule |
| Stripe-style `metadata[order_id]=6735` | works | the package's bracket convention behind `begin_map` |
| `Content-Type`, any case | works | header reader default: kebab, without case |
| custom header names | works | fact |
| a stored `User` reused as the PATCH body sets `role` | breaks | UD3 |

### H4. Multipart upload with a file part

```text
@derive(Deserialize)
data AvatarUpload:
    caption: string = ""
    avatar: FilePart

@derive(Deserialize)
data FilePart:
    filename: string?
    bytes: List[u8]
```

| Feature | Verdict | How |
| --- | --- | --- |
| text parts | works | a text source, like a form |
| the file's bytes | with additions | `List[u8]` is one call per byte (UD13) |
| a large file, streamed | breaks | `build` is pure and whole; axum's `Multipart` streams outside serde too |

### H5. problem+json (RFC 9457)

```json
{"type": "https://example.com/probs/out-of-credit", "title": "Not enough credit.", "status": 403, "balance": 30}
```

```text
@derive(Serialize, Deserialize)
data Problem:
    `type`: string = "about:blank"
    status: u16?
    @json.rest  # hypothetical syntax
    extensions: Map[string, Json]

@derive(Serialize, Deserialize)
data OutOfCredit:
    Problem
    balance: i64
```

| Feature | Verdict | How |
| --- | --- | --- |
| the `type` key | works | raw identifier |
| `OutOfCredit` embeds `Problem` | with additions | JSON nests an embedded member (UD12) |
| `extensions` keeps the rest | with additions | needs every other member's name first (UD12, UD1) |
| `Json` as a dynamic member | works | `Json` reads itself through `peek` |

### T1. Cargo.toml

```toml
[package]
version.workspace = true
rust-version = "1.75"
[dependencies]
tokio = { version = "1", features = ["full"] }
[[bin]]
name = "demo-cli"
```

```text
@derive(Deserialize)
data Manifest:
    package: Package
    dependencies: Map[string, Dependency] = {}
    @toml.rename("bin")  # hypothetical syntax
    bins: List[Target] = []

@derive(Deserialize)
@toml.rename_all("kebab-case")  # hypothetical syntax
data Package:
    version: Inheritable[string]
    rust_version: string?
```

| Feature | Verdict | How |
| --- | --- | --- |
| dotted keys | works | the TOML reader presents nested tables |
| `[[bin]]` arrays of tables | works | `List[T]` default |
| kebab keys | works | per-format fact, rightly: `cargo metadata` JSON is snake case (UD6) |
| `serde = "1"` or a table | with additions | hand-written (UD7) |
| `{ workspace = true }` for `Inheritable` | with additions | hand-written (UD7) |
| an offset datetime | works | the reader hands RFC 3339 text to `Timestamp`; a local date needs its own type |

### Y1. A GitHub Actions workflow

```yaml
on: {push: {branches: [main]}}
jobs:
  test:
    runs-on: ubuntu-latest
    if: github.event_name == 'push'
    steps: [{uses: actions/checkout@v4, with: {fetch-depth: 0}}, {run: cargo test}]
```

```text
@derive(Deserialize)
data Workflow:
    on: Triggers

@derive(Deserialize)
@yaml.rename_all("kebab-case")  # hypothetical syntax
data Job:
    runs_on: string
    `if`: string?
    steps: List[Step]

enum Step:
    Action(uses: string, with: Map[string, string]?)
    Command(run: string)
```

| Feature | Verdict | How |
| --- | --- | --- |
| the `on` key | works | not reserved in hd; the reader must use YAML 1.2, where `on` is not `true` |
| `runs-on` | works | fact |
| `if` | works | raw identifier |
| `on:` as a name, a list, or a map | with additions | hand-written (UD7) |
| steps told apart by `uses` or `run` | with additions | UD7 |
| `fetch-depth: 0` into a string | works | the YAML reader returns the scalar's text for `text()` |

### Y2. Kubernetes manifests

A file holds a `Deployment` and a `Service`, split by `---`, each with
`apiVersion`, `kind`, `metadata`, and `spec`; a `targetPort` is `80` or
`http`.

```text
@derive(Deserialize)
@yaml.tag("kind")  # hypothetical syntax
@yaml.rename_all_members("camelCase")  # hypothetical syntax
enum Resource:
    Deployment(api_version: string, metadata: ObjectMeta, spec: DeploymentSpec)
    Service(api_version: string, metadata: ObjectMeta, spec: ServiceSpec)
```

| Feature | Verdict | How |
| --- | --- | --- |
| documents split by `---` | works | `yaml.decode_all::[Resource](text)` |
| `kind` picks the variant | works | internal tag; the reader finds the key |
| camelCase members, PascalCase variants | works | two facts, as serde's `rename_all` and `rename_all_fields` |
| `targetPort` int-or-string | with additions | hand-written (UD7) |
| the same `ObjectMeta` from `kubectl -o json` | with additions | the camelCase fact repeats per format (UD6) |

### D1. A SQL row (sqlx `FromRow`)

The sqlx original puts `#[sqlx(rename)]`, `Option<T>`, `#[sqlx(json)]`,
and `#[sqlx(flatten)]` on one row type.

```text
@derive(Deserialize)
data OrderRow:
    @sql.column("customer_email")  # hypothetical syntax
    email: string
    note: string?
    status: OrderStatus
    total: Decimal
    @sql.json  # hypothetical syntax
    items: List[Item]
    @sql.prefix("customer_")  # hypothetical syntax
    customer: Customer

@derive(Deserialize)
enum OrderStatus:
    @sql.value(1)  # hypothetical syntax
    Pending
```

| Feature | Verdict | How |
| --- | --- | --- |
| column rename | works | fact |
| `NULL` into `T?` | works | `is_null` |
| a join flattened into one row | works | a row is flat, so a nested record reads the same row; a prefix fact splits clashing names |
| an enum stored as text | works | the row reader matches the text in `begin_variant` |
| an enum stored as an int | works | explicit `@sql.value`; `VariantInfo.index` moves when variants are reordered |
| a JSON column | works | the row reader hands the column to a JSON reader |
| `NUMERIC` and `TIMESTAMPTZ` | works | the driver renders text; `Decimal` and `Timestamp` read text |
| per-row cost | works | members arrive in a fixed order, so the reader caches column indexes after the first row |

### D2. INSERT through `Serialize`

```text
@derive(Serialize)
data NewOrder:
    @sql.generated  # hypothetical syntax
    id: i64 = 0
    customer_email: string

fn save(db: Db, order: NewOrder) -> Result[i64, DbError]:
    db.insert("orders", order)
```

| Feature | Verdict | How |
| --- | --- | --- |
| column list and parameters | works | `member(m)` names the column, then the value follows |
| a generated `id` left out | works | `@sql.generated`, which JSON ignores |
| a JSON column | works | a nested JSON writer |

### B1. Protobuf

The message has `string query = 1`, `repeated string tags = 4`,
`oneof filter { string author = 5; int64 since = 6; }`, and an enum field.

```text
@derive(Serialize, Deserialize)
data SearchRequest:
    @proto.field(1)  # hypothetical syntax
    query: string = ""
    filter: Filter?
    @proto.unknown  # hypothetical syntax
    unknown_fields: List[u8] = []

@derive(Serialize, Deserialize)
enum Filter:
    @proto.field(5)  # hypothetical syntax
    Author(string)
    @proto.field(6)  # hypothetical syntax
    Since(i64)
```

| Feature | Verdict | How |
| --- | --- | --- |
| field numbers | works | facts; the reader buffers fields by number, since members are asked in declaration order |
| absent scalars are zero | works | declared defaults |
| `oneof` | works | the reader picks the variant whose number appears |
| an enum number this build does not know | works | an `@proto.other` variant with an `i32` payload |
| `bytes` fields | with additions | UD13 |
| unknown fields kept for writing back | with additions | UD12, UD13 |
| MessagePack arrays by position | works | `Member.position` |

### V1. Validation

```text
@derive(Deserialize, Validate)
data Signup:
    @pattern("^[^@]+@[^@]+$")  # hypothetical syntax
    email: string
    @range(13, 130)  # hypothetical syntax
    age: u8
    @check(is_slug)  # hypothetical syntax
    handle: string

data PortRange:
    low: u16
    high: u16

impl Validate for PortRange by Structure:
    fn check(self) -> Result[void, ValidationError]:
        if self.low < self.high: .Ok(()) else: .Err(ValidationError::new(["low"], "must be below high"))
```

| Feature | Verdict | How |
| --- | --- | --- |
| range, non-empty, length | works | typed facts such as `Range[F < Integer]` check where they attach |
| a regex | works | a fact is evaluated once per program; a bad pattern panics at first read |
| a custom validator | works | a typed fact holding `fn(T) -> bool` |
| every error, not the first | works | `validate` walks every member |
| `low < high` | works | the block writes `check`, by `annot.block.methods` |
| ripgrep's PAT-or-`-e` as a rule | works | `check` on `Rg` |
| a nested record checked with its parent | with additions | every member type needs `Validate`, std leaves included; a foreign type is omitted with `= pass` |
| the error names `--age`, `APP_AGE`, or `$.age` | with additions | UD11 |
| a forgotten `.validate()` | breaks | UD9 |

## Prior Art

| Library | One model? | Command line | Names per source | Lesson |
| --- | --- | --- | --- | --- |
| [pydantic-settings](https://docs.pydantic.dev/latest/concepts/pydantic_settings/) | yes: JSON, env, CLI | `CliSettingsSource`, `CliPositionalArg`, `CliSubCommand` | one `alias` for all; `env_prefix`, `env_nested_delimiter` | one model can serve a CLI; an alias hides lower-priority values; env reads complex values as JSON, so `NUMBERS=1,2,3` fails |
| [figment](https://docs.rs/figment/latest/figment/) | serde for files and env | clap, merged through `Serialized` | serde renames, `Env::prefixed`, `split` | `Metadata` maps a key path to a native key for errors (UD11) |
| [config-rs](https://docs.rs/config/latest/config/) | serde for files and env | none | `Environment::with_prefix`, `separator` | layering by merging dynamic values (UD5 candidate 3) |
| [envy](https://github.com/softprops/envy) | serde for env | none | serde's renames, shared with JSON | env and JSON share one name table (UD6) |
| [clap derive](https://docs.rs/clap/latest/clap/_derive/index.html) | no: `Parser` beside serde | its own derive over the `Command` builder | `short`, `long`, `env`, `global`, groups | the parser needs the whole schema first; figment pairs it with serde |
| [docopt.rs](https://github.com/docopt/docopt.rs) | serde for arguments | the schema comes from the usage text | field prefixes `flag_`, `arg_` | serde alone carried no schema; unmaintained, it points to clap |
| [Go struct tags](https://pkg.go.dev/reflect#StructTag), [envconfig](https://github.com/kelseyhightower/envconfig) | one struct | the `flag` package, by hand | a tag key per source: `json:`, `env:`, `db:` | per-source names on one type (UD8 candidate 1); maps as `k:v,k2:v2` |
| [cobra](https://github.com/spf13/cobra), [viper](https://github.com/spf13/viper) | one merged map, then `mapstructure` | cobra flags bound to viper keys | `mapstructure` tags | precedence: explicit set, flag, env, config, key/value store, default |
| [kotlinx.serialization](https://github.com/Kotlin/kotlinx.serialization/blob/master/docs/formats.md) | yes: JSON, CBOR, protobuf, Properties | none; clikt is separate | `@SerialName` shared; `@ProtoNumber`, `@CborLabel` per format | per-format facts on one model (B1); Properties flattens to dotted keys (UD4) |
| [sqlx `FromRow`](https://docs.rs/sqlx/latest/sqlx/trait.FromRow.html), diesel `Queryable` | no: their own derives | none | `rename`, `rename_all`, `flatten`, `json` | separate because decoding is typed per database; the facts map one to one (D1) |

## Parse Log

Every `text` block was checked with `scripts/hd-conformance.sh parse` on
2026-10-17: 25 blocks, all parse. Each hypothetical fact line is
marked, and no block uses syntax that a chapter leaves unspecified.

| Blocks | Sections | Result |
| --- | --- | --- |
| 1-4 | UD1, UD2, UD5, UD10 | parses |
| 5-10 | C1, C2, C3, C4, C5, C6 | parses |
| 11-12 | E1, E2 | parses |
| 13 | L1 | parses |
| 14-18 | H1, H2, H3, H4, H5 | parses |
| 19 | T1 | parses |
| 20-21 | Y1, Y2 | parses |
| 22-23 | D1, D2 | parses |
| 24 | B1 | parses |
| 25 | V1 | parses |

The parser rejected one draft of Y1: a default on a variant payload
member, `with: Map[string, string] = {}`, is `syntax-error: only shared
enum constructor parameters declare defaults`. The report now uses `T?`
there, and C2 notes the effect on subcommands. No block was rejected for
syntax that a chapter specifies.
