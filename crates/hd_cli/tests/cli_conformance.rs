//! The CLI tier of the specification conformance suite
//! (`spec/conformance/README.md`, "CLI Cases"): each case copies its
//! directory to a fresh temp directory, runs the `run:` lines of
//! `expect.txt` against the `hd` binary, and judges exit status, standard
//! streams and files.
//!
//! A case that needs a host service the sandbox lacks is unsupported.
//! Every case must pass, except a case listed as `cli/NAME` in the root
//! `KNOWN_FAILURES.tsv` (`hd_testkit::known_failures`) for the engine; a
//! listed case that passes fails the test too, and unsupported counts as
//! not passing. A full run prints the counts and failure buckets.
//! `HD_CONFORMANCE_ONLY=a,b` runs the cases whose name contains one of the
//! parts and prints each verdict.
//! `HD_CONFORMANCE_ENGINE=wasmtime-only` selects Wasmtime for program commands
//! without overriding explicit engine flags. `wasmtime` compares fresh complete
//! Node/Wasmtime fixture runs at identical paths, judging by Wasmtime. Program
//! and test commands both select the engine. Comparison excludes setup
//! commands and explicit-engine steps.
//! Both Wasmtime modes need `hd` built with `--features wasmtime`; without it
//! they are refused.

//! `HD_TEST_TIMINGS=1` logs per-case or per-command wall times for profiling.

use std::collections::BTreeSet;
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use hd_testkit::known_failures::{self, Outcome};

const LIMIT: Duration = Duration::from_mins(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EngineMode {
    Default,
    NodeBaseline,
    WasmtimeOnly,
    Compare,
}

impl EngineMode {
    fn parse(value: Option<&str>) -> Result<Self, String> {
        let mode = match value {
            None | Some("" | "node") => Self::Default,
            Some("wasmtime-only") => Self::WasmtimeOnly,
            Some("wasmtime") => Self::Compare,
            Some(value) => {
                return Err(format!(
                    "unknown HD_CONFORMANCE_ENGINE={value:?}; use node, wasmtime or wasmtime-only"
                ));
            }
        };
        if matches!(mode, Self::WasmtimeOnly | Self::Compare) && !cfg!(feature = "wasmtime") {
            return Err(format!(
                "HD_CONFORMANCE_ENGINE={} needs an hd with the wasmtime engine; rerun with --features wasmtime",
                value.unwrap_or_default()
            ));
        }
        Ok(mode)
    }

    fn forced_engine(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::NodeBaseline => Some("node"),
            Self::WasmtimeOnly | Self::Compare => Some("wasmtime"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeRoute {
    None,
    Wasmtime,
    NodeProgram,
    OtherProgram,
    NodeTest,
    WasmtimeTest,
    OtherTest,
}

/// Follow main's dispatch: the command word comes first. `run` covers named
/// package executables, workspace members and package tasks.
fn runtime_command(words: &[String]) -> Option<(usize, bool)> {
    let before = &words[..words.iter().position(|w| w == "--").unwrap_or(words.len())];
    let index = 0;
    match before.get(index)?.as_str() {
        "test" => Some((index, true)),
        "run" | "--repl" => Some((index, false)),
        "help" | "--help" | "new" | "clean" | "add" | "update" | "remove" | "fetch" | "fmt"
        | "build" | "check" => None,
        _ if before.iter().any(|w| {
            Path::new(w)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("hd") || e.eq_ignore_ascii_case("wasm"))
        }) =>
        {
            Some((index, false))
        }
        _ => None,
    }
}

fn explicit_engine(words: &[String]) -> Option<&str> {
    let before = &words[..words.iter().position(|w| w == "--").unwrap_or(words.len())];
    before
        .iter()
        .enumerate()
        .filter_map(|(i, word)| {
            if word == "--engine" {
                Some(before.get(i + 1).map_or("", String::as_str))
            } else {
                word.strip_prefix("--engine=")
            }
        })
        .next_back()
}

fn select_engine(
    words: &[String],
    mode: EngineMode,
    fallback: &str,
) -> (Vec<String>, RuntimeRoute) {
    let mut selected = words.to_vec();
    let Some((command, test)) = runtime_command(words) else {
        return (selected, RuntimeRoute::None);
    };
    let explicit = explicit_engine(words);
    let engine = if let Some(engine) = explicit {
        engine
    } else if let Some(engine) = mode.forced_engine() {
        // Keep `run` as the command; standalone files allow leading options.
        let at = if matches!(words[command].as_str(), "run" | "test" | "--repl") {
            command + 1
        } else {
            0
        };
        selected.splice(at..at, ["--engine".to_owned(), engine.to_owned()]);
        engine
    } else if words[command] == "--repl" {
        // A session names its engine in every mode, so a mode that leaves
        // program commands alone still judges the session on one engine.
        let engine = if fallback.is_empty() {
            "node"
        } else {
            fallback
        };
        let at = command + 1;
        selected.splice(at..at, ["--engine".to_owned(), engine.to_owned()]);
        engine
    } else {
        fallback
    };
    let route = match (engine, test) {
        ("wasmtime", false) => RuntimeRoute::Wasmtime,
        ("wasmtime", true) => RuntimeRoute::WasmtimeTest,
        ("", false) if explicit.is_some() => RuntimeRoute::OtherProgram,
        ("", true) if explicit.is_some() => RuntimeRoute::OtherTest,
        ("" | "node", false) => RuntimeRoute::NodeProgram,
        ("" | "node", true) => RuntimeRoute::NodeTest,
        (_, false) => RuntimeRoute::OtherProgram,
        (_, true) => RuntimeRoute::OtherTest,
    };
    (selected, route)
}

#[derive(Default)]
struct RuntimeStats {
    wasmtime: AtomicUsize,
    node: AtomicUsize,
    other: AtomicUsize,
    tests: AtomicUsize,
    wasmtime_tests: AtomicUsize,
    node_time: std::sync::Mutex<Duration>,
    wasmtime_time: std::sync::Mutex<Duration>,
    node_paths: std::sync::Mutex<BTreeSet<String>>,
    test_paths: std::sync::Mutex<BTreeSet<String>>,
}

impl RuntimeStats {
    fn record_time(&self, route: RuntimeRoute, elapsed: Duration) {
        let total = match route {
            RuntimeRoute::Wasmtime | RuntimeRoute::WasmtimeTest => &self.wasmtime_time,
            RuntimeRoute::NodeProgram | RuntimeRoute::NodeTest => &self.node_time,
            _ => return,
        };
        *total.lock().expect("runtime command time") += elapsed;
    }

    fn record(&self, name: &str, route: RuntimeRoute) {
        let counter = match route {
            RuntimeRoute::None => return,
            RuntimeRoute::Wasmtime => &self.wasmtime,
            RuntimeRoute::OtherProgram | RuntimeRoute::OtherTest => &self.other,
            RuntimeRoute::WasmtimeTest => &self.wasmtime_tests,
            RuntimeRoute::NodeProgram => {
                self.node_paths
                    .lock()
                    .expect("Node paths")
                    .insert(name.to_owned());
                &self.node
            }
            RuntimeRoute::NodeTest => {
                self.test_paths
                    .lock()
                    .expect("test paths")
                    .insert(name.to_owned());
                &self.tests
            }
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn print(&self, mode: EngineMode, results: &[Verdict], elapsed: Duration) {
        let passes = results.iter().filter(|v| **v == Verdict::Pass).count();
        let unsupported = results
            .iter()
            .filter(|v| matches!(v, Verdict::Unsupported(_)))
            .count();
        println!(
            "CLI tier {mode:?}: {} cases, {passes} pass, {} fail, {unsupported} unsupported; wall {elapsed:.3?}",
            results.len(),
            results.len() - passes - unsupported,
        );
        println!(
            "CLI runtime-command steps attempted: Wasmtime programs {}, Node programs {}, other/invalid engine {}, Wasmtime test commands {}, Node test commands {} (not a count of guest executions)",
            self.wasmtime.load(Ordering::Relaxed),
            self.node.load(Ordering::Relaxed),
            self.other.load(Ordering::Relaxed),
            self.wasmtime_tests.load(Ordering::Relaxed),
            self.tests.load(Ordering::Relaxed),
        );
        if matches!(mode, EngineMode::WasmtimeOnly | EngineMode::Compare) {
            for (label, paths) in [
                ("remaining Node program", &self.node_paths),
                ("remaining Node test", &self.test_paths),
            ] {
                let paths = paths.lock().expect("runtime paths");
                println!("CLI {label} paths: {}", paths.len());
                for path in paths.iter() {
                    println!("  cli/{path}");
                }
            }
        }
    }
}

/// Worker count, like the TS runner (`spec/tools/run-conformance.ts`):
/// `HD_TEST_JOBS` if set to a positive integer, else
/// `min(8, available_parallelism())`.
fn jobs() -> usize {
    if let Ok(value) = std::env::var("HD_TEST_JOBS")
        && let Ok(n) = value.trim().parse::<usize>()
        && n >= 1
    {
        return n;
    }
    std::thread::available_parallelism().map_or(8, |n| n.get().min(8))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Verdict {
    Pass,
    Fail(String),
    Unsupported(String),
}

// ---------------------------------------------------------------- JSON

#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl JsonParser<'_> {
    fn parse(text: &str) -> Result<Json, String> {
        let mut parser = JsonParser {
            bytes: text.as_bytes(),
            at: 0,
        };
        let value = parser.value()?;
        parser.space();
        if parser.at != parser.bytes.len() {
            return Err("trailing text after JSON value".to_owned());
        }
        Ok(value)
    }

    fn space(&mut self) {
        while matches!(self.bytes.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Result<(), String> {
        self.space();
        if self.bytes.get(self.at) == Some(&byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!("expected `{}` at byte {}", byte as char, self.at))
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(format!("bad JSON at byte {}", self.at))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.space();
        match self.bytes.get(self.at) {
            Some(b'{') => {
                self.at += 1;
                let mut fields = Vec::new();
                self.space();
                if self.bytes.get(self.at) == Some(&b'}') {
                    self.at += 1;
                    return Ok(Json::Obj(fields));
                }
                loop {
                    self.space();
                    let key = self.string()?;
                    self.eat(b':')?;
                    fields.push((key, self.value()?));
                    self.space();
                    match self.bytes.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {
                            self.at += 1;
                            return Ok(Json::Obj(fields));
                        }
                        _ => return Err(format!("bad object at byte {}", self.at)),
                    }
                }
            }
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                self.space();
                if self.bytes.get(self.at) == Some(&b']') {
                    self.at += 1;
                    return Ok(Json::Arr(items));
                }
                loop {
                    items.push(self.value()?);
                    self.space();
                    match self.bytes.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(Json::Arr(items));
                        }
                        _ => return Err(format!("bad array at byte {}", self.at)),
                    }
                }
            }
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'n') => self.word("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => {
                let start = self.at;
                while matches!(
                    self.bytes.get(self.at),
                    Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                ) {
                    self.at += 1;
                }
                let text =
                    std::str::from_utf8(&self.bytes[start..self.at]).map_err(|e| e.to_string())?;
                text.parse::<f64>()
                    .map(Json::Num)
                    .map_err(|_| format!("bad number `{text}`"))
            }
            _ => Err(format!("bad JSON at byte {}", self.at)),
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .ok_or("short \\u escape")?;
        let text = std::str::from_utf8(digits).map_err(|e| e.to_string())?;
        self.at += 4;
        u32::from_str_radix(text, 16).map_err(|_| "bad \\u escape".to_owned())
    }

    fn string(&mut self) -> Result<String, String> {
        if self.bytes.get(self.at) != Some(&b'"') {
            return Err(format!("expected string at byte {}", self.at));
        }
        self.at += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(&byte) = self.bytes.get(self.at) else {
                return Err("unterminated string".to_owned());
            };
            self.at += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let Some(&escape) = self.bytes.get(self.at) else {
                        return Err("unterminated escape".to_owned());
                    };
                    self.at += 1;
                    let ch = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let mut unit = self.hex4()?;
                            if (0xD800..0xDC00).contains(&unit)
                                && self.bytes[self.at..].starts_with(b"\\u")
                            {
                                self.at += 2;
                                let low = self.hex4()?;
                                unit =
                                    0x10000 + ((unit - 0xD800) << 10) + (low.wrapping_sub(0xDC00));
                            }
                            char::from_u32(unit).ok_or("bad \\u scalar")?
                        }
                        _ => return Err("bad string escape".to_owned()),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(byte),
            }
        }
        String::from_utf8(out).map_err(|e| e.to_string())
    }
}

/// The README's matching rules: `expected` matches `actual`.
fn json_matches(expected: &Json, actual: &Json) -> bool {
    match (expected, actual) {
        (Json::Null, _) => true,
        (Json::Obj(fields), Json::Obj(have)) => fields.iter().all(|(key, want)| {
            have.iter()
                .find(|(name, _)| name == key)
                .is_some_and(|(_, value)| json_matches(want, value))
        }),
        (Json::Arr(want), Json::Arr(have)) => {
            want.len() == have.len() && want.iter().zip(have).all(|(w, h)| json_matches(w, h))
        }
        (want, have) => want == have && !matches!(want, Json::Obj(_) | Json::Arr(_)),
    }
}

fn judge_json(stream: &str, expected: &str) -> Result<(), String> {
    let Json::Arr(want) =
        JsonParser::parse(expected).map_err(|e| format!("bad expectation: {e}"))?
    else {
        return Err("expectation is not an array".to_owned());
    };
    let mut lines: Vec<&str> = stream.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let mut have = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let value = JsonParser::parse(line)
            .map_err(|e| format!("line {} is not JSON ({e}): {line}", index + 1))?;
        if !matches!(value, Json::Obj(_)) {
            return Err(format!("line {} is not a JSON object", index + 1));
        }
        have.push(value);
    }
    if want.len() != have.len() {
        return Err(format!(
            "expected {} JSON lines, got {}: {stream:?}",
            want.len(),
            have.len()
        ));
    }
    for (index, (w, h)) in want.iter().zip(&have).enumerate() {
        if !json_matches(w, h) {
            return Err(format!(
                "JSON line {} does not match: {}",
                index + 1,
                lines[index]
            ));
        }
    }
    Ok(())
}

// ------------------------------------------------------------ expect.txt

#[derive(Default)]
struct Step {
    words: Vec<String>,
    exit: Option<u8>,
    stdout: Vec<String>,
    stdout_json: Option<String>,
    stderr: Vec<String>,
    stderr_json: Option<String>,
    /// `transcript: PATH`: standard output is the text of this file.
    transcript: Option<String>,
    files: Vec<(bool, String)>,
}

/// Decodes the escapes of the README's "Standard Output": `\\`, `\t`,
/// `\u{H}`.
fn unescape(text: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('t') => out.push('\t'),
            Some('u') => {
                if chars.next() != Some('{') {
                    return Err(format!("bad escape in `{text}`"));
                }
                let mut digits = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(d) if d.is_ascii_hexdigit() && digits.len() < 6 => digits.push(d),
                        _ => return Err(format!("bad \\u escape in `{text}`")),
                    }
                }
                let scalar = u32::from_str_radix(&digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or_else(|| format!("bad \\u scalar in `{text}`"))?;
                out.push(scalar);
            }
            _ => return Err(format!("bad escape in `{text}`")),
        }
    }
    Ok(out)
}

fn parse_expect(text: &str) -> Result<Vec<Step>, String> {
    let mut steps: Vec<Step> = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((kind, value)) = line
            .split_once(": ")
            .or_else(|| line.strip_suffix(':').map(|kind| (kind, "")))
        else {
            return Err(format!("bad expect.txt line: {line}"));
        };
        if kind == "run" {
            let mut words = value.split(' ').filter(|w| !w.is_empty());
            if words.next() != Some("hd") {
                return Err(format!("run line does not start with hd: {line}"));
            }
            steps.push(Step {
                words: words.map(str::to_owned).collect(),
                ..Step::default()
            });
            continue;
        }
        let Some(step) = steps.last_mut() else {
            return Err(format!("line before the first run: {line}"));
        };
        match kind {
            "exit" => {
                step.exit = Some(value.parse().map_err(|_| format!("bad exit: {line}"))?);
            }
            "stdout" => step.stdout.push(unescape(value)?),
            "stderr" => step.stderr.push(unescape(value)?),
            "stdout-json" => step.stdout_json = Some(value.to_owned()),
            "stderr-json" => step.stderr_json = Some(value.to_owned()),
            "transcript" => {
                if step.transcript.replace(value.to_owned()).is_some() {
                    return Err(format!("transcript given twice: {line}"));
                }
            }
            "file" => step.files.push((true, value.to_owned())),
            "no-file" => step.files.push((false, value.to_owned())),
            _ => return Err(format!("unknown expect.txt line: {line}")),
        }
    }
    Ok(steps)
}

// -------------------------------------------------------------- running

fn suite_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/conformance")
}

fn work_root() -> PathBuf {
    std::env::temp_dir().join(format!("hd-cli-conformance-{}", std::process::id()))
}

/// The directory of a case's local remotes, which is no part of its input
/// (README, "Local Remotes").
const REMOTES: &str = "remotes";

fn copy_case(from: &Path, to: &Path) -> std::io::Result<()> {
    copy_tree(from, to, true)
}

fn copy_tree(from: &Path, to: &Path, top: bool) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if top && entry.file_name() == REMOTES {
            continue;
        }
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target, false)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Every `.fi` file under `dir`, as its path below `dir` without `.fi`.
fn streams(dir: &Path, below: &str, out: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = if below.is_empty() {
            name.clone()
        } else {
            format!("{below}/{name}")
        };
        if entry.file_type()?.is_dir() {
            streams(&entry.path(), &path, out)?;
        } else if let Some(repository) = path.strip_suffix(".fi") {
            out.push((repository.to_owned(), entry.path()));
        }
    }
    Ok(())
}

/// A git command of the runner itself, with no global or system
/// configuration.
fn runner_git(args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .args(args)
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("LC_ALL", "C")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}

/// Turns each `remotes/REPOSITORY.fi` of the case at `source` into a bare
/// repository `into/REPOSITORY`, by `git fast-import`, whose `HEAD` names
/// `refs/heads/main`.
fn build_remotes(source: &Path, into: &Path) -> Result<(), String> {
    let dir = source.join(REMOTES);
    if !dir.is_dir() {
        return Ok(());
    }
    let mut found = Vec::new();
    streams(&dir, "", &mut found).map_err(|e| format!("remotes: {e}"))?;
    for (repository, stream) in found {
        let repo = into.join(&repository);
        let repo_arg = repo.to_string_lossy().into_owned();
        let init = runner_git(&[
            "init",
            "--bare",
            "--quiet",
            "--initial-branch=main",
            "--object-format=sha1",
            &repo_arg,
        ])
        .output()
        .map_err(|e| format!("git init: {e}"))?;
        if !init.status.success() {
            return Err(format!(
                "git init {repository}: {}",
                String::from_utf8_lossy(&init.stderr)
            ));
        }
        let input = std::fs::File::open(&stream).map_err(|e| format!("{repository}.fi: {e}"))?;
        let imported = runner_git(&[&format!("--git-dir={repo_arg}"), "fast-import", "--quiet"])
            .stdin(input)
            .output()
            .map_err(|e| format!("git fast-import: {e}"))?;
        if !imported.status.success() {
            return Err(format!(
                "git fast-import {repository}.fi: {}",
                String::from_utf8_lossy(&imported.stderr)
            ));
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct Ran {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
}

/// Points `command`'s git at no global configuration and sends every
/// remote URL `hd` tries (HTTPS and SSH) to the case's local remotes in
/// `remotes`, a directory that may not exist, so a test never reaches the
/// network: a repository the case has no stream for fails at once. Git
/// reads the rewrite from the environment, so no file is left behind.
fn offline_git(command: &mut Command, remotes: &Path) {
    let mut base = remotes.to_string_lossy().replace('\\', "/");
    if !base.starts_with('/') {
        base.insert(0, '/');
    }
    let local = format!("file://{base}/");
    command
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_COUNT", "3");
    for (i, from) in ["https://", "ssh://git@", "git@"].into_iter().enumerate() {
        command
            .env(
                format!("GIT_CONFIG_KEY_{i}"),
                format!("url.{local}.insteadOf"),
            )
            .env(format!("GIT_CONFIG_VALUE_{i}"), from);
    }
}

fn run_hd(dir: &Path, cache: &Path, remotes: &Path, words: &[String]) -> std::io::Result<Ran> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_hd"));
    offline_git(&mut command, remotes);
    let mut child = command
        .args(words)
        .current_dir(dir)
        .env("HD_CACHE", cache)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut out = child.stdout.take().expect("piped stdout");
    let mut err = child.stderr.take().expect("piped stderr");
    let out_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = out.read_to_end(&mut bytes);
        bytes
    });
    let err_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = err.read_to_end(&mut bytes);
        bytes
    });
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > LIMIT {
            timed_out = true;
            child.kill()?;
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    Ok(Ran {
        status,
        stdout,
        stderr,
        timed_out,
    })
}

fn judge_stream(
    name: &str,
    actual: &str,
    lines: &[String],
    json: Option<&String>,
) -> Result<(), String> {
    if let Some(json) = json {
        return judge_json(actual, json).map_err(|e| format!("{name}-json: {e}"));
    }
    if lines.is_empty() {
        return Ok(());
    }
    let mut want = String::new();
    for line in lines {
        want.push_str(line);
        want.push('\n');
    }
    if actual == want {
        Ok(())
    } else {
        Err(format!("{name}: want {want:?}, got {actual:?}"))
    }
}

fn judge_step(dir: &Path, step: &Step, ran: &Ran) -> Result<(), String> {
    // Fixture judging keeps its existing lossy-text policy; comparison uses
    // the original bytes so distinct invalid UTF-8 cannot become false matches.
    let stdout = String::from_utf8_lossy(&ran.stdout);
    let stderr = String::from_utf8_lossy(&ran.stderr);
    if ran.timed_out {
        return Err("timed out after 60 seconds".to_owned());
    }
    let Some(code) = ran.status.code() else {
        return Err("ended with a signal".to_owned());
    };
    let want = i32::from(step.exit.unwrap_or(0));
    if code != want {
        return Err(format!("exit: want {want}, got {code}; stderr {stderr:?}"));
    }
    judge_stream("stdout", &stdout, &step.stdout, step.stdout_json.as_ref())?;
    if let Some(path) = &step.transcript {
        let want = std::fs::read_to_string(dir.join(path))
            .map_err(|e| format!("transcript {path}: {e}"))?;
        // Line ends are LF on both sides: a Windows checkout of the
        // transcript has CRLF.
        let want = want.replace("\r\n", "\n");
        let got = stdout.replace("\r\n", "\n");
        if want != got {
            return Err(format!("transcript {path}: want {want:?}, got {got:?}"));
        }
    }
    judge_stream("stderr", &stderr, &step.stderr, step.stderr_json.as_ref())?;
    for (exists, path) in &step.files {
        let present = dir.join(path).symlink_metadata().is_ok();
        if present != *exists {
            return Err(format!(
                "{}: {path}",
                if *exists {
                    "file missing"
                } else {
                    "no-file present"
                }
            ));
        }
    }
    Ok(())
}

fn git_missing() -> bool {
    Command::new("git")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_err()
}

struct Observation {
    step: usize,
    test: bool,
    explicit: bool,
    output: Ran,
}

#[derive(Default)]
struct ComparisonCounts {
    matched: AtomicUsize,
    different: AtomicUsize,
    missing: AtomicUsize,
    excluded: AtomicUsize,
}

#[derive(Default)]
struct ComparisonStats {
    programs: ComparisonCounts,
    tests: ComparisonCounts,
}

impl ComparisonStats {
    fn compare(&self, name: &str, node: &[Observation], wasmtime: &[Observation]) {
        let indices = node
            .iter()
            .chain(wasmtime)
            .map(|o| o.step)
            .collect::<BTreeSet<_>>();
        for step in indices {
            let node = node.iter().find(|o| o.step == step);
            let wasmtime = wasmtime.iter().find(|o| o.step == step);
            let counts = if node
                .into_iter()
                .chain(wasmtime)
                .any(|observation| observation.test)
            {
                &self.tests
            } else {
                &self.programs
            };
            let counter = if node.into_iter().chain(wasmtime).any(|o| o.explicit) {
                &counts.excluded
            } else if let (Some(node), Some(wasmtime)) = (node, wasmtime) {
                if node.output == wasmtime.output {
                    &counts.matched
                } else {
                    println!(
                        "CLI engine difference cli/{name} step {}: Node {:?}; Wasmtime {:?}",
                        step + 1,
                        node.output,
                        wasmtime.output,
                    );
                    &counts.different
                }
            } else {
                println!(
                    "CLI engine comparison missing cli/{name} step {}: Node {}, Wasmtime {}",
                    step + 1,
                    node.is_some(),
                    wasmtime.is_some(),
                );
                &counts.missing
            };
            counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn print(&self) {
        for (label, counts) in [("program", &self.programs), ("test", &self.tests)] {
            println!(
                "CLI Node/Wasmtime {label}-command comparison: {} raw status/stdout/stderr matches, {} differences, {} missing pairs, {} explicit-engine exclusions (not a count of guest executions)",
                counts.matched.load(Ordering::Relaxed),
                counts.different.load(Ordering::Relaxed),
                counts.missing.load(Ordering::Relaxed),
                counts.excluded.load(Ordering::Relaxed),
            );
        }
    }
}

fn run_case(
    name: &str,
    root: &Path,
    mode: EngineMode,
    fallback: &str,
    stats: &RuntimeStats,
    mut observations: Option<&mut Vec<Observation>>,
) -> Verdict {
    let source = suite_root().join("cli").join(name);
    let text = match std::fs::read_to_string(source.join("expect.txt")) {
        Ok(text) => text,
        Err(e) => return Verdict::Fail(format!("expect.txt: {e}")),
    };
    let steps = match parse_expect(&text) {
        Ok(steps) => steps,
        Err(e) => return Verdict::Fail(e),
    };
    let needs_git = steps.iter().flat_map(|s| s.files.iter()).any(|(_, path)| {
        matches!(
            Path::new(path).file_name().and_then(|n| n.to_str()),
            Some(".git" | ".gitignore")
        )
    });
    let needs_git = needs_git || source.join(REMOTES).is_dir();
    if needs_git && git_missing() {
        return Verdict::Unsupported("needs git on PATH".to_owned());
    }
    let dir = root.join(name);
    let cache = root.join(format!("cache-{name}"));
    let remotes = root.join(format!("remotes-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cache);
    let _ = std::fs::remove_dir_all(&remotes);
    // Only compiled, content-addressed objects are shared. Fixture files,
    // fetched dependencies and execution state remain private to the case.
    // Cache commands and raw engine comparisons retain fresh object stores.
    #[cfg(unix)]
    if observations.is_none()
        && !steps
            .iter()
            .any(|step| step.words.first().is_some_and(|word| word == "cache"))
    {
        let objects = root.join("shared-objects");
        let setup = std::fs::create_dir_all(&objects)
            .and_then(|()| std::fs::create_dir_all(&cache))
            .and_then(|()| std::os::unix::fs::symlink(&objects, cache.join("obj")));
        if let Err(error) = setup {
            return Verdict::Fail(format!("shared object store: {error}"));
        }
    }
    if let Err(e) = copy_case(&source, &dir) {
        return Verdict::Fail(format!("copy case: {e}"));
    }
    if let Err(e) = build_remotes(&source, &remotes) {
        let _ = std::fs::remove_dir_all(&remotes);
        return Verdict::Fail(e);
    }
    // `expect.txt` is not part of the case input.
    let _ = std::fs::remove_file(dir.join("expect.txt"));
    let timings = std::env::var_os("HD_TEST_TIMINGS").is_some();
    let mut verdict = Verdict::Pass;
    for (index, step) in steps.iter().enumerate() {
        let label = format!("hd {}", step.words.join(" "));
        let (words, route) = select_engine(&step.words, mode, fallback);
        stats.record(name, route);
        let start = Instant::now();
        let ran = run_hd(&dir, &cache, &remotes, &words);
        stats.record_time(route, start.elapsed());
        if timings {
            eprintln!(
                "cli-step\t{name}\t{index}\t{:.6}",
                start.elapsed().as_secs_f64()
            );
        }
        let result = ran
            .map_err(|e| format!("spawn failed: {e}"))
            .and_then(|ran| {
                let result = judge_step(&dir, step, &ran);
                if route != RuntimeRoute::None
                    && let Some(observations) = observations.as_mut()
                {
                    observations.push(Observation {
                        step: index,
                        test: matches!(
                            route,
                            RuntimeRoute::WasmtimeTest
                                | RuntimeRoute::NodeTest
                                | RuntimeRoute::OtherTest
                        ),
                        explicit: explicit_engine(&step.words).is_some(),
                        output: ran,
                    });
                }
                result
            });
        if let Err(e) = result {
            verdict = Verdict::Fail(format!("`{label}`: {e}"));
            break;
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cache);
    let _ = hd_deps::cache::remove_tree(&remotes);
    verdict
}

fn case_names() -> Vec<String> {
    let text = std::fs::read_to_string(suite_root().join("cli-cases.tsv")).expect("cli-cases.tsv");
    text.lines()
        .skip(1)
        .map(|line| {
            line.split_once('\t')
                .unwrap_or_else(|| panic!("bad cli-cases.tsv row: {line}"))
                .0
                .to_owned()
        })
        .collect()
}

fn run_all(names: &[String], mode: EngineMode) -> Vec<Verdict> {
    let start = Instant::now();
    let stats = RuntimeStats::default();
    let node_stats = RuntimeStats::default();
    let comparison = ComparisonStats::default();
    let fallback = std::env::var("HD_ENGINE").unwrap_or_default();
    let root = work_root();
    std::fs::create_dir_all(&root).expect("work directory");
    let count = jobs().min(names.len().max(1));
    let next = AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<Verdict>>> =
        names.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..count {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(name) = names.get(index) else { break };
                    let verdict = if mode == EngineMode::Compare {
                        let mut node = Vec::new();
                        let mut wasmtime = Vec::new();
                        // run_case deletes and recopies BOTH fixture and cache.
                        // Sequential runs use exactly the same paths, including
                        // setup/mutation commands, never the other engine's state.
                        let _ = run_case(
                            name,
                            &root,
                            EngineMode::NodeBaseline,
                            &fallback,
                            &node_stats,
                            Some(&mut node),
                        );
                        let verdict = run_case(
                            name,
                            &root,
                            EngineMode::WasmtimeOnly,
                            &fallback,
                            &stats,
                            Some(&mut wasmtime),
                        );
                        comparison.compare(name, &node, &wasmtime);
                        verdict
                    } else {
                        run_case(name, &root, mode, &fallback, &stats, None)
                    };
                    *results[index].lock().expect("results lock") = Some(verdict);
                }
            });
        }
    });
    let _ = hd_deps::cache::remove_tree(&root);
    let results = results
        .into_iter()
        .map(|slot| slot.into_inner().expect("results lock").expect("verdict"))
        .collect::<Vec<_>>();
    stats.print(mode, &results, start.elapsed());
    if mode == EngineMode::Compare {
        comparison.print();
        println!(
            "CLI comparison aggregate runtime-command wall (including compilation and startup): Node {:.3?}, Wasmtime {:.3?}",
            *node_stats.node_time.lock().expect("Node command time"),
            *stats.wasmtime_time.lock().expect("Wasmtime command time"),
        );
    }
    results
}

#[test]
fn cli_conformance_passes_except_known_failures() {
    let selected = match std::env::var("HD_CONFORMANCE_ENGINE") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            panic!(
                "HD_CONFORMANCE_ENGINE must be valid UTF-8: use node, wasmtime or wasmtime-only"
            );
        }
    };
    let mode = EngineMode::parse(selected.as_deref()).unwrap_or_else(|error| panic!("{error}"));
    let all = case_names();
    if let Ok(only) = std::env::var("HD_CONFORMANCE_ONLY") {
        let parts: Vec<&str> = only.split(',').filter(|p| !p.is_empty()).collect();
        let names: Vec<String> = all
            .into_iter()
            .filter(|name| parts.iter().any(|part| name.contains(part)))
            .collect();
        let results = run_all(&names, mode);
        for (name, verdict) in names.iter().zip(&results) {
            println!("cli/{name}: {verdict:?}");
        }
        return;
    }
    let results = run_all(&all, mode);
    let engine = match mode.forced_engine() {
        Some(engine) => engine.to_owned(),
        None => std::env::var("HD_ENGINE")
            .ok()
            .filter(|engine| !engine.is_empty())
            .unwrap_or_else(|| "node".to_owned()),
    };
    let paths: Vec<String> = all.iter().map(|name| format!("cli/{name}")).collect();
    let outcomes: Vec<Outcome<'_>> = paths
        .iter()
        .zip(&results)
        .map(|(path, verdict)| Outcome {
            path,
            bucket: match verdict {
                Verdict::Pass => None,
                Verdict::Fail(why) => Some(format!("fail: {why}")),
                Verdict::Unsupported(why) => Some(format!("unsupported: {why}")),
            },
        })
        .collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let known = known_failures::load(&root);
    let (summary, errors) = known_failures::judge(
        "CLI",
        &engine,
        None,
        &known,
        |path| path.starts_with("cli/"),
        &outcomes,
    );
    // Straight to stdout, past the test harness's capture, so a passing run
    // shows the counts too.
    std::io::stdout()
        .write_all(summary.as_bytes())
        .expect("print the summary");
    assert!(
        errors.is_empty(),
        "CLI conformance cases break the KNOWN_FAILURES.tsv rule:\n{}",
        errors.join("\n")
    );
}

#[test]
fn engine_mode_parser_rejects_unknown_modes() {
    assert_eq!(EngineMode::parse(None).unwrap(), EngineMode::Default);
    assert_eq!(
        EngineMode::parse(Some("node")).unwrap(),
        EngineMode::Default
    );
    for (name, mode) in [
        ("wasmtime-only", EngineMode::WasmtimeOnly),
        ("wasmtime", EngineMode::Compare),
    ] {
        let parsed = EngineMode::parse(Some(name));
        if cfg!(feature = "wasmtime") {
            assert_eq!(parsed.unwrap(), mode);
        } else {
            assert!(parsed.unwrap_err().contains("--features wasmtime"));
        }
    }
    assert!(
        EngineMode::parse(Some("wasmtiem"))
            .unwrap_err()
            .contains("wasmtime-only")
    );
}

#[test]
fn runtime_timing_counts_programs_and_tests_but_not_setup_or_other_engines() {
    let stats = RuntimeStats::default();
    for (route, millis) in [
        (RuntimeRoute::NodeProgram, 2),
        (RuntimeRoute::NodeTest, 3),
        (RuntimeRoute::Wasmtime, 7),
        (RuntimeRoute::WasmtimeTest, 11),
        (RuntimeRoute::None, 100),
        (RuntimeRoute::OtherProgram, 100),
        (RuntimeRoute::OtherTest, 100),
    ] {
        stats.record_time(route, Duration::from_millis(millis));
    }
    assert_eq!(*stats.node_time.lock().unwrap(), Duration::from_millis(5));
    assert_eq!(
        *stats.wasmtime_time.lock().unwrap(),
        Duration::from_millis(18)
    );
}

#[test]
fn engine_selection_covers_files_packages_tasks_and_preserves_argument_boundary() {
    let words = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
    for before in [
        words(&["main.hd"]),
        words(&["build/debug/app.wasm", "--", "--engine", "node"]),
        words(&["--format", "json", "src/main.hd"]),
        words(&["run"]),
        words(&["run", "named-executable", "--", "arg"]),
        words(&["run", "task-name", "-p", "member"]),
    ] {
        let (selected, route) = select_engine(&before, EngineMode::WasmtimeOnly, "node");
        assert_eq!(route, RuntimeRoute::Wasmtime);
        let index = selected.iter().position(|w| w == "--engine").unwrap();
        assert_eq!(selected[index + 1], "wasmtime");
        let mut restored = selected;
        restored.drain(index..index + 2);
        assert_eq!(restored, before);
    }
    for before in [
        words(&["run", "--engine", "node"]),
        words(&["main.hd", "--engine=node", "--", "--engine=wasmtime"]),
    ] {
        let (selected, route) = select_engine(&before, EngineMode::WasmtimeOnly, "wasmtime");
        assert_eq!(selected, before);
        assert_eq!(route, RuntimeRoute::NodeProgram);
    }
    for before in [
        words(&["test", "main.hd"]),
        words(&["test", "-p", "member"]),
    ] {
        let (selected, route) = select_engine(&before, EngineMode::WasmtimeOnly, "node");
        assert_eq!(route, RuntimeRoute::WasmtimeTest);
        let index = selected.iter().position(|word| word == "--engine").unwrap();
        assert_eq!(selected[index + 1], "wasmtime");
        let mut restored = selected;
        restored.drain(index..index + 2);
        assert_eq!(restored, before);
    }
    let explicit_test = words(&["test", "main.hd", "--engine=node"]);
    assert_eq!(
        select_engine(&explicit_test, EngineMode::WasmtimeOnly, "wasmtime"),
        (explicit_test, RuntimeRoute::NodeTest)
    );
}

#[test]
fn engine_selection_leaves_nonruntime_commands_and_default_mode_unchanged() {
    for words in [
        vec!["check", "main.hd"],
        vec!["build", "main.hd"],
        vec!["fmt", "src/main.hd"],
        vec!["help", "run"],
        vec!["run", "--engine=custom"],
    ] {
        let words = words.into_iter().map(str::to_owned).collect::<Vec<_>>();
        let (selected, _) = select_engine(&words, EngineMode::WasmtimeOnly, "node");
        assert_eq!(selected, words);
    }
    let words = vec![
        "run".to_owned(),
        "--".to_owned(),
        "--engine=node".to_owned(),
    ];
    let (selected, route) = select_engine(&words, EngineMode::Default, "node");
    assert_eq!(selected, words);
    assert_eq!(route, RuntimeRoute::NodeProgram);
    assert_eq!(
        select_engine(&words, EngineMode::Default, "wasmtime").1,
        RuntimeRoute::Wasmtime
    );
    let test = vec!["test".to_owned(), "--format".to_owned(), "json".to_owned()];
    assert_eq!(
        select_engine(&test, EngineMode::WasmtimeOnly, "wasmtime").1,
        RuntimeRoute::WasmtimeTest
    );
}

#[cfg(any(unix, windows))]
#[test]
fn engine_comparison_uses_raw_bytes_and_counts_exclusions_and_missing_pairs() {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt as _;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt as _;

    let observation = |step, explicit, stdout| Observation {
        step,
        test: false,
        explicit,
        output: Ran {
            status: std::process::ExitStatus::from_raw(0),
            stdout,
            stderr: Vec::new(),
            timed_out: false,
        },
    };
    let node = vec![
        observation(0, false, b"same".to_vec()),
        observation(1, false, vec![0xff]),
        observation(2, false, Vec::new()),
        observation(3, true, b"explicit Node".to_vec()),
    ];
    let wasmtime = vec![
        observation(0, false, b"same".to_vec()),
        observation(1, false, vec![0xfe]),
        observation(3, true, b"explicit Wasmtime".to_vec()),
    ];
    let stats = ComparisonStats::default();
    stats.compare("synthetic", &node, &wasmtime);
    assert_eq!(stats.programs.matched.load(Ordering::Relaxed), 1);
    assert_eq!(stats.programs.different.load(Ordering::Relaxed), 1);
    assert_eq!(stats.programs.missing.load(Ordering::Relaxed), 1);
    assert_eq!(stats.programs.excluded.load(Ordering::Relaxed), 1);
    let test = |step, explicit, stdout| Observation {
        test: true,
        ..observation(step, explicit, stdout)
    };
    stats.compare(
        "test-synthetic",
        &[
            test(0, false, b"test result: ok\n".to_vec()),
            test(1, true, Vec::new()),
        ],
        &[
            test(0, false, b"test result: ok\n".to_vec()),
            test(1, true, Vec::new()),
        ],
    );
    assert_eq!(stats.tests.matched.load(Ordering::Relaxed), 1);
    assert_eq!(stats.tests.excluded.load(Ordering::Relaxed), 1);
    assert_eq!(stats.programs.matched.load(Ordering::Relaxed), 1);
    let words = vec![
        "run".to_owned(),
        "task".to_owned(),
        "--".to_owned(),
        "--engine=custom".to_owned(),
    ];
    let (selected, route) = select_engine(&words, EngineMode::NodeBaseline, "wasmtime");
    assert_eq!(&selected[..3], ["run", "--engine", "node"]);
    assert_eq!(&selected[3..], &words[1..]);
    assert_eq!(route, RuntimeRoute::NodeProgram);
}
