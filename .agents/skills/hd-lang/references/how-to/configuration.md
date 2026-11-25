# How To: Load Configuration

**Scenario.** A service reads `config.json`, lets environment variables
override single settings, falls back to defaults when the file is absent,
and reports a bad value with its name.

## The program

```hd
use std.fs.{FsError, FsRead, FsWrite, MemoryFs, read_text, stat, write_text}
use std.host.{Env, HostError, ScriptedEnv, env, env_as}
use std.encoding.json.{JsonError, decode, encode_pretty}
use std.path.Path
use std.encoding.{Deserialize, Serialize}

@derive(Serialize, Deserialize, Eq, Debug)
struct Limits:
    retries: u32 = 3
    timeout_ms: u32 = 250

@derive(Serialize, Deserialize, Eq, Debug)
struct Config:
    name: string = "service"
    port: u16 = 8080
    limits: Limits = Limits {}
    tags: List[string] = []

@error
pub enum ConfigError:
    @error("cannot read the config file")
    Read(@from error: FsError)
    @error("the config file is not valid")
    Parse(@from error: JsonError)
    @error("bad environment value")
    Environment(@from error: HostError)

# The file is optional: without it, every field takes its default.
fn from_file!(path: Path) -> Result[Config, ConfigError] $ FsRead:
    if stat!(path)?.is_none():
        return .Ok(Config {})
    text := read_text!(path)?
    .Ok(decode::[Config](text)?)

# Environment variables override the file, one setting at a time.
fn with_environment(config: Config) -> Result[Config, ConfigError] $ Env:
    let mut result = config
    match env("APP_NAME"):
        .Some(name) => result.name = name
        .None => pass
    match env_as::[u16]("APP_PORT")?:
        .Some(port) => result.port = port
        .None => pass
    .Ok(result)

fn load!(path: Path) -> Result[Config, ConfigError] $ FsRead + Env:
    with_environment(from_file!(path)?)

fn save!(path: Path, config: Config) -> Result[void, ConfigError] $ FsWrite:
    write_text!(path, encode_pretty(config))?
    .Ok(())

pub fn main!() -> Result[void, ConfigError] $ Console + FsRead + Env:
    config := load!(Path("config.json"))?
    println("${config.name} on port ${config.port}, ${config.limits.retries} retries")
    .Ok(())

tests:
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

    it("reports a bad environment value"):
        $.with(FsRead=MemoryFs::new(), Env=box(ScriptedEnv::new({"APP_PORT": "http"}))):
            assert_equal(load!(Path("config.json")).is_err(), true)

    it("saves and loads the same value"):
        fs := MemoryFs::new()
        config := Config { name: "shop", tags: ["a"] }
        $.with(FsWrite=fs, FsRead=fs, Env=box(ScriptedEnv::new({}))):
            assert(save!(Path("out.json"), config).is_ok())
            assert_equal(load!(Path("out.json")).ok(), .Some(config))
```

## The pieces

- **Typed JSON**: `@derive(Serialize, Deserialize)` on each struct, then
  `decode::[Config](text)`. A field with a default may be missing from the
  file, an unknown field is ignored, and a missing field without a default
  is an error that names its path.
- **Defaults live on the struct.** With a default on every field,
  `Config {}` is the default configuration.
- **Environment**: `env(name)` gives `string?`. `env_as::[T](name)` parses
  the value and gives `Result[T?, HostError]`. Always write the type
  argument, as `env_as::[u16]("APP_PORT")`.
- **One error type**: an `@error` enum with a `@from` variant per source,
  so `?` converts each failure on the way up. It is `pub` because the
  public `main` returns it.
- **A missing file is not an error here**: `stat!(path)` gives `.None`
  when nothing is there.
- **Writing**: `encode_pretty(value)` for a file people read, `encode`
  for compact text.
- **In a package**, grant what `main` names in `hd.toml`, and narrow it:
  `FsRead = ["config.json"]`, `Env = ["APP_*"]`.
