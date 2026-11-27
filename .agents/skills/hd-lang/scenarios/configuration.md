# Load Configuration

**Scenario.** A service reads `config.json`, lets environment variables
override single settings, falls back to defaults when the file is absent,
and reports every bad value with its name.

**Derive, don't hand-parse.** `@derive(Deserialize)` reads the file,
`env_decode` reads the environment into a struct, and `@derive(Validate)`
checks the rules written on the fields. Don't read keys or variables one
by one.

## The program

```hd
use std.encoding.{Deserialize, Serialize}
use std.encoding.json.{JsonError, decode, encode, encode_pretty}
use std.fs.{FsError, FsRead, FsWrite, MemoryFs, read_text, stat, write_text}
use std.host.{Env, HostError, ScriptedEnv, env_decode}
use std.path.Path
use std.testing.Arbitrary
use std.validate.{Validate, ValidationError, length, range}

@derive(Serialize, Deserialize, Validate, Arbitrary, Eq, Debug)
struct Limits:
    @range(1, 10)
    retries: u32 = 3
    timeout_ms: u32 = 250

@derive(Serialize, Deserialize, Validate, Arbitrary, Eq, Debug)
struct Config:
    @length(1, 40)
    name: string = "service"
    @range(1024, 65535)
    port: u16 = 8080
    limits: Limits = Limits {}
    tags: List[string] = []

# The settings the environment may override: an unset variable is `.None`.
@derive(Deserialize)
struct Overrides:
    name: string?
    port: u16?

@error
pub enum ConfigError:
    @error("cannot read the config file")
    Read(@from error: FsError)
    @error("the config file is not valid JSON: $error")
    Parse(@from error: JsonError)
    @error("$error")
    Environment(@from error: HostError)
    @error("$error")
    Invalid(@from error: ValidationError)

# The file is optional: without it, every field takes its default.
fn from_file!(path: Path) -> Result[Config, ConfigError] $ FsRead:
    if stat!(path)?.is_none():
        return .Ok(Config {})
    .Ok(decode::[Config](read_text!(path)?)?)

# `APP_NAME` and `APP_PORT` override the file.
fn with_environment(config: Config) -> Result[Config, ConfigError] $ Env:
    overrides := env_decode::[Overrides](prefix="APP_")?
    .Ok(Config {
        ...config,
        name: overrides.name.unwrap_or(config.name),
        port: overrides.port.unwrap_or(config.port),
    })

fn load!(path: Path) -> Result[Config, ConfigError] $ FsRead + Env:
    config := with_environment(from_file!(path)?)?
    .Ok(config.validated()?)                    # the rules on the fields

fn save!(path: Path, config: Config) -> Result[void, ConfigError] $ FsWrite:
    write_text!(path, encode_pretty(config))?
    .Ok(())

pub fn main!() -> Result[void, ConfigError] $ Console + FsRead + Env:
    config := load!(Path("config.json"))?
    println("${config.name} on port ${config.port}, ${config.limits.retries} retries")
    .Ok(())

tests:
    use std.testing.it_prop

    it("uses defaults when the file is absent"):
        $.with(FsRead=MemoryFs::new(), Env=box(ScriptedEnv::new({}))):
            assert_equal(load!(Path("config.json")).ok(), .Some(Config {}))

    it("reads the file, then lets the environment override it"):
        fs := MemoryFs::new()
        $.with(FsWrite=fs):
            _ := write_text!(Path("config.json"), "{\"name\": \"shop\", \"limits\": {\"retries\": 5}}")
        $.with(FsRead=fs, Env=box(ScriptedEnv::new({"APP_PORT": "9000"}))):
            config := load!(Path("config.json")).ok().expect("a valid config")
            assert_equal(config.name, "shop")
            assert_equal(config.port, 9000)
            assert_equal(config.limits, Limits { retries: 5, timeout_ms: 250 })

    it("names a bad environment value"):
        $.with(FsRead=MemoryFs::new(), Env=box(ScriptedEnv::new({"APP_PORT": "http"}))):
            error := load!(Path("config.json")).err().expect("an error")
            assert_equal("$error", "invalid value for APP_PORT: invalid digit at position 0")

    it("names every field that breaks a rule"):
        fs := MemoryFs::new()
        $.with(FsWrite=fs):
            _ := write_text!(Path("config.json"), "{\"name\": \"\", \"port\": 80, \"limits\": {\"retries\": 0}}")
        $.with(FsRead=fs, Env=box(ScriptedEnv::new({}))):
            error := load!(Path("config.json")).err().expect("an error")
            assert_equal("$error", "$.name: has the wrong length\n$.port: must be within the allowed range\n$.limits.retries: must be within the allowed range")

    it("saves and loads the same value"):
        fs := MemoryFs::new()
        config := Config { name: "shop", tags: ["a"] }
        $.with(FsWrite=fs, FsRead=fs, Env=box(ScriptedEnv::new({}))):
            assert(save!(Path("out.json"), config).is_ok())
            assert_equal(load!(Path("out.json")).ok(), .Some(config))

    # Any value, not one example: what is encoded decodes to the same value.
    it_prop("every config survives encode and decode", prop=fn!(config: Config):
        assert_equal(decode::[Config](encode(config)).ok(), .Some(config))
    )
```

## The pieces

- **Typed JSON**: `@derive(Serialize, Deserialize)` on each struct, then
  `decode::[Config](text)`. A field with a default may be missing from the
  file, an unknown field is ignored, and a missing field without a default
  is an error that names its path. `@rename("listenPort")` on a field, or
  `@rename_all(.CamelCase)` on the struct, changes the JSON key (import
  `rename`, `rename_all` from `std.encoding`).
- **Defaults live on the struct.** With a default on every field,
  `Config {}` is the default configuration.
- **Environment**: `env_decode::[T](prefix="APP_")` reads each field of
  `T` from `APP_` plus its name in uppercase (`port` reads `APP_PORT`). A
  field that is `T?` or has a default may be unset; a list splits on
  commas; a bad value is `HostError.InvalidValue`, naming the variable.
  Decode the whole config this way when the environment is its only
  source. For one variable, `env_as::[u16]("PORT")` gives
  `Result[u16?, HostError]`, and `env("NAME")` the raw `string?`.
- **Rules on the fields**: `@derive(Validate)` and rule facts from
  `std.validate` on the fields: `@range(low, high)`, `@at_least(n)`,
  `@max(n)`, `@length(min, max)`, `@pattern(regex)`, `@email()`,
  `@url()`, `@one_of([...])`, `@unique()`, `@check(fn)`. Nested structs are
  checked too. `value.validated()` returns the value, or a
  `ValidationError` listing every broken rule with its path. Validate
  after reading, whatever the source: JSON, environment or `@derive(Cli)`.
- **One error type**: an `@error` enum with a `@from` variant per source
  ([errors](../design/errors.md)). It is `pub` because the public `main`
  returns it.
- **A missing file is not an error here**: `stat!(path)` gives `.None`
  when nothing is there.
- **Writing**: `encode_pretty(value)` for a file people read, `encode`
  for compact text.
- **In a package**, narrow the grants: `FsRead = ["config.json"]`,
  `Env = ["APP_*"]` ([capabilities](../design/capabilities.md)).
- **Round trip**: the property test checks every `Config`, not one
  example ([testing](../design/testing.md)).
