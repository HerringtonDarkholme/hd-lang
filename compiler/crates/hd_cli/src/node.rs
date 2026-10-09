//! Node as an `hd_run::Engine` (build-order.md §22: until wasmtime is an
//! approved dependency, `hd run` and `hd test` run on V8 through Node).
//! The JS host (`compiler/host/*.mjs`) is embedded, so the binary is
//! relocatable.

use std::fmt::Write as _;
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::Poll;

use hd_cache::CacheStore;
use hd_run::{Engine, Grants, HostSetup, Instance, Limit, LoadError, Outcome, StartError};

use crate::report::quote;

/// The JS host: the import object and the entry driver.
const CORE: &str = include_str!("../../../host/core.mjs");
/// `hd run`: one instance, `hd.init` then `hd.poll`.
const RUN: &str = include_str!("../../../host/run.mjs");
/// `hd test`: one fresh instance per case.
const TEST: &str = include_str!("../../../host/test.mjs");

pub struct NodeEngine;

pub struct NodeModule {
    wasm: Vec<u8>,
}

struct NodeInstance {
    wasm: Vec<u8>,
    /// The program's arguments, after the module's path and its
    /// configuration on Node's command line.
    args: Vec<String>,
    /// What its `Args.program` returns.
    program: String,
    /// Its capability grant, which the host's providers check
    /// (`cli.host.default-profile.granted`).
    grants: Grants,
    done: Option<Outcome>,
}

impl Engine for NodeEngine {
    type Module = NodeModule;
    fn load(&self, wasm: &[u8], _cache: &dyn CacheStore) -> Result<NodeModule, LoadError> {
        if wasm.get(..4) != Some(b"\0asm") {
            return Err(LoadError::NotHd("not a Wasm module".into()));
        }
        Ok(NodeModule {
            wasm: wasm.to_vec(),
        })
    }
    fn instantiate(
        &self,
        m: &NodeModule,
        host: &HostSetup,
    ) -> Result<Box<dyn Instance>, StartError> {
        Ok(Box::new(NodeInstance {
            wasm: m.wasm.clone(),
            args: host.args.clone(),
            program: host.program.clone(),
            grants: host.grants.clone(),
            done: None,
        }))
    }
}

/// A scratch directory with the host scripts and the module, removed on
/// drop. Each is unique, so parallel test workers do not share one.
struct Scratch(PathBuf);

impl Scratch {
    fn new(wasm: &[u8]) -> Result<Self, String> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("hd-run-{}-{n}", std::process::id()));
        let s = Scratch(dir);
        std::fs::create_dir_all(&s.0).map_err(|e| format!("{}: {e}", s.0.display()))?;
        for (name, text) in [("core.mjs", CORE), ("run.mjs", RUN), ("test.mjs", TEST)] {
            std::fs::write(s.0.join(name), text).map_err(|e| format!("{}: {e}", s.0.display()))?;
        }
        std::fs::write(s.0.join("main.wasm"), wasm)
            .map_err(|e| format!("{}: {e}", s.0.display()))?;
        Ok(s)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl NodeInstance {
    /// Runs the whole program in one Node process, with our standard
    /// streams, so it reads our input (`ConsoleInput`) and writes as it
    /// goes. Its configuration holds its `Args.program` and its grant.
    fn run(&self) -> Outcome {
        let dir = match Scratch::new(&self.wasm) {
            Ok(d) => d,
            Err(e) => return Outcome::Internal(e),
        };
        let config = dir.0.join("config.json");
        let text = run_config(&self.program, &self.grants);
        if let Err(e) = std::fs::write(&config, text) {
            return Outcome::Internal(format!("{}: {e}", config.display()));
        }
        let _ = std::io::stdout().flush();
        let status = Command::new("node")
            .arg(dir.0.join("run.mjs"))
            .arg(dir.0.join("main.wasm"))
            .arg(&config)
            .args(&self.args)
            .status();
        match status {
            Ok(s) if s.success() => Outcome::Exit(0),
            Ok(s) => Outcome::Exit(u8::try_from(s.code().unwrap_or(1).clamp(1, 255)).unwrap_or(1)),
            Err(e) => Outcome::Internal(format!("node: {e}")),
        }
    }
}

impl Instance for NodeInstance {
    fn init(&mut self) -> Outcome {
        let o = self.run();
        let r = o.clone();
        self.done = Some(o);
        r
    }
    fn poll(&mut self) -> Poll<Outcome> {
        Poll::Ready(self.done.clone().unwrap_or(Outcome::Exit(0)))
    }
    fn wake(&mut self, _: &[u32]) {}
    fn call_test(&mut self, _: u32) -> Poll<Outcome> {
        Poll::Ready(Outcome::Internal(
            "a test case runs through `run_cases` on the Node engine".into(),
        ))
    }
    fn completed(&mut self) -> Vec<u32> {
        Vec::new()
    }
}

/// One finished case, or one row of an `it_each` case, as the test host
/// reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaseRun {
    pub test: u32,
    pub status: i64,
    pub trapped: bool,
    pub stdout: String,
    pub stderr: String,
    pub us: u64,
    /// An `it_each` row: its index and the table's row count
    /// (`std-testing.it-each.name`). A table with no rows is one result
    /// with `rows` 0 and no outcome.
    pub row: Option<(u32, u32)>,
    /// A failed property's shrunk input, as `Debug` shows it
    /// (`std-testing.prop.report`).
    pub input: Option<String>,
}

impl CaseRun {
    /// Whether this is the case's last result: a case's only one, or its
    /// table's last row.
    #[must_use]
    pub fn last(&self) -> bool {
        self.row.is_none_or(|(row, rows)| row + 1 >= rows)
    }
}

/// One case as the test host runs it: its `hd.test.i` and `hd.init.j`
/// exports, its own temporary directory (`cli.test.env.temp-dir`), which
/// the host makes on the case's first `temp_dir` call, the base seed of a
/// property test case (`cli.test.seed`), and the case's grant.
pub struct HostCase {
    pub test: u32,
    pub init: u32,
    /// The registration function: `it`, `it_each`, `it_prop` or
    /// `it_prop_with`. Only an `it_each` case runs rows.
    pub kind: String,
    pub temp_dir: PathBuf,
    pub seed: Option<i64>,
    pub grants: Grants,
    pub snapshot: Snapshot,
}

/// Where a case's snapshot files are (`std-testing.snapshot-file.path`):
/// `<root>/<folder>/<slug>-<n>.snap`, and whether this is an update run,
/// which records them (`std-testing.snapshot-file.missing`).
pub struct Snapshot {
    pub root: PathBuf,
    /// `__snapshots__/<module>`, as a message shows it.
    pub folder: String,
    /// `__regressions__/<module>`, where a property case keeps its file
    /// `<slug>` (`std-testing.prop.regression-file`).
    pub regressions: String,
    pub slug: String,
    pub update: bool,
}

/// What the cases of one test program share (Test Environments): an
/// integration test's or a doc test's working directory, the package
/// directory (`cli.test.env.cwd`), and the path that `Args.program`
/// returns (`cli.test.env.args.program`). Every case's program arguments
/// are empty (`cli.test.env.args`) and its standard input is closed
/// (`cli.test.env.stdin`).
pub struct TestEnv<'a> {
    pub cwd: Option<&'a Path>,
    pub program: &'a str,
    /// The programs its `Process` provider starts by name, an integration
    /// test's or a doc test's (`cli.test.process`).
    pub programs: Option<&'a Programs>,
}

/// The test host's configuration: the program's shared environment, then
/// each case's.
fn host_config(env: &TestEnv<'_>, cases: &[HostCase]) -> String {
    let programs = env.programs.map_or("null", |p| p.json.as_str());
    let mut out = format!(
        "{{\"program\":{},\"args\":[],\"programs\":{programs},\"cases\":[",
        quote(env.program)
    );
    for (i, c) in cases.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let seed = c.seed.map_or_else(|| "null".to_owned(), |s| s.to_string());
        let s = &c.snapshot;
        // A property case's regression file (`std-testing.prop.regression-file`).
        let regression = if c.seed.is_some() {
            let shown = format!("{}/{}", s.regressions, s.slug);
            format!(
                "{{\"file\":{},\"shown\":{}}}",
                quote(&s.root.join(&shown).to_string_lossy()),
                quote(&shown)
            )
        } else {
            "null".to_owned()
        };
        let _ = write!(
            out,
            "{{\"test\":{},\"init\":{},\"kind\":{},\"tempDir\":{},\"seed\":{seed},\"grants\":{},\"snapshot\":{{\"dir\":{},\"shown\":{},\"slug\":{},\"update\":{}}},\"regression\":{regression}}}",
            c.test,
            c.init,
            quote(&c.kind),
            quote(&c.temp_dir.to_string_lossy()),
            grants_json(&c.grants),
            quote(&s.root.join(&s.folder).to_string_lossy()),
            quote(&s.folder),
            quote(&s.slug),
            s.update
        );
    }
    out.push_str("]}");
    out
}

/// The package's executables and tasks as the test runner's `Process`
/// provider starts them (`cli.test.process`, `.process.tasks`): each built
/// module, with the run host and a configuration of its `Args.program` and
/// grant, in a scratch directory that lives as long as this value.
pub struct Programs {
    _dir: Scratch,
    json: String,
}

impl Programs {
    /// `list` holds each program's name, module and grant; each runs with
    /// `cwd`, the package directory, as its working directory
    /// (`cli.test.process.cwd`).
    pub fn new(list: &[(String, Vec<u8>, Grants)], cwd: &Path) -> Result<Programs, String> {
        let dir = Scratch::new(&[])?;
        let mut json = String::from("{");
        for (i, (name, wasm, grants)) in list.iter().enumerate() {
            let module = dir.0.join(format!("program-{i}.wasm"));
            let config = dir.0.join(format!("program-{i}.json"));
            std::fs::write(&module, wasm).map_err(|e| format!("{}: {e}", module.display()))?;
            std::fs::write(&config, run_config(name, grants))
                .map_err(|e| format!("{}: {e}", config.display()))?;
            if i > 0 {
                json.push(',');
            }
            let _ = write!(
                json,
                "{}:{{\"host\":{},\"wasm\":{},\"config\":{},\"cwd\":{}}}",
                quote(name),
                quote(&dir.0.join("run.mjs").to_string_lossy()),
                quote(&module.to_string_lossy()),
                quote(&config.to_string_lossy()),
                quote(&cwd.to_string_lossy())
            );
        }
        json.push('}');
        Ok(Programs { _dir: dir, json })
    }
}

/// The run host's configuration (`run.mjs`): the program's `Args.program`
/// and its grant.
fn run_config(program: &str, grants: &Grants) -> String {
    format!(
        "{{\"program\":{},\"grants\":{}}}",
        quote(program),
        grants_json(grants)
    )
}

/// A grant as the JS host reads it: each trait with a limit, as `false`
/// for a total deny or the list of its entries; a trait it leaves out has
/// no limit (`cli.cap.order.default`).
fn grants_json(g: &Grants) -> String {
    let mut out = String::from("{");
    for (i, (key, limit)) in g.limits.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&quote(key));
        out.push(':');
        match limit {
            Limit::Deny => out.push_str("false"),
            Limit::Entries(list) => {
                let items: Vec<String> = list.iter().map(|e| quote(e)).collect();
                let _ = write!(out, "[{}]", items.join(","));
            }
        }
    }
    out.push('}');
    out
}

/// Runs `cases` of a test program in one Node process, each in a fresh
/// instance, and hands each result to `done` as it ends.
pub fn run_cases(
    wasm: &[u8],
    env: &TestEnv<'_>,
    cases: &[HostCase],
    done: &mut dyn FnMut(CaseRun),
) -> Result<(), String> {
    let dir = Scratch::new(wasm)?;
    let config = dir.0.join("config.json");
    std::fs::write(&config, host_config(env, cases))
        .map_err(|e| format!("{}: {e}", config.display()))?;
    let mut command = Command::new("node");
    command
        .arg(dir.0.join("test.mjs"))
        .arg(dir.0.join("main.wasm"))
        .arg(&config);
    if let Some(cwd) = env.cwd {
        command.current_dir(cwd);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("node: {e}"))?;
    let stdout = child.stdout.take().ok_or("node: no standard output")?;
    let mut seen = 0;
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|e| format!("node: {e}"))?;
        let r = parse_case(&line).ok_or_else(|| format!("node: unreadable result `{line}`"))?;
        seen += usize::from(r.last());
        done(r);
    }
    let out = child.wait_with_output().map_err(|e| format!("node: {e}"))?;
    if seen != cases.len() {
        return Err(format!(
            "node: the test host stopped after {seen} of {} cases: {}",
            cases.len(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// Reads one flat JSON object of the test host.
fn parse_case(line: &str) -> Option<CaseRun> {
    let mut r = CaseRun::default();
    let mut s = line.trim().strip_prefix('{')?.strip_suffix('}')?;
    while !s.is_empty() {
        let (key, rest) = json_str(s.trim_start())?;
        let rest = rest.trim_start().strip_prefix(':')?.trim_start();
        let rest = if rest.starts_with('"') {
            let (v, rest) = json_str(rest)?;
            match key.as_str() {
                "stdout" => r.stdout = v,
                "stderr" => r.stderr = v,
                "input" => r.input = Some(v),
                _ => {}
            }
            rest
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            let v = rest[..end].trim();
            match key.as_str() {
                "test" => r.test = v.parse().ok()?,
                "status" => r.status = v.parse().ok()?,
                "trapped" => r.trapped = v == "true",
                "us" => r.us = v.parse().ok()?,
                "row" => r.row = Some((v.parse().ok()?, r.row.map_or(0, |x| x.1))),
                "rows" => r.row = Some((r.row.map_or(0, |x| x.0), v.parse().ok()?)),
                _ => {}
            }
            &rest[end..]
        };
        s = rest
            .trim_start()
            .strip_prefix(',')
            .unwrap_or(rest.trim_start());
    }
    Some(r)
}

/// A JSON string at the start of `s`, decoded, and the rest after it.
fn json_str(s: &str) -> Option<(String, &str)> {
    let body = s.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = body.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((out, &body[i + 1..])),
            '\\' => {
                let (_, e) = chars.next()?;
                match e {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'u' => {
                        let mut code = 0u32;
                        for _ in 0..4 {
                            code = code * 16 + chars.next()?.1.to_digit(16)?;
                        }
                        // A surrogate pair is two escapes.
                        if (0xD800..0xDC00).contains(&code) {
                            let rest: String = chars.by_ref().take(6).map(|x| x.1).collect();
                            let low = u32::from_str_radix(rest.strip_prefix("\\u")?, 16).ok()?;
                            let hi = code.checked_sub(0xD800)?;
                            let lo = low.checked_sub(0xDC00).filter(|l| *l < 0x400)?;
                            code = 0x10000 + (hi << 10) + lo;
                        }
                        out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                    }
                    other => out.push(other),
                }
            }
            c => out.push(c),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use hd_run::{Grants, Limit};

    use super::{HostCase, Snapshot, TestEnv, host_config, parse_case};

    #[test]
    fn writes_the_host_config() {
        let cases = [HostCase {
            test: 2,
            init: 1,
            kind: "it_prop".into(),
            temp_dir: PathBuf::from("/tmp/hd-test-1-0/4"),
            seed: Some(9),
            grants: Grants {
                limits: vec![
                    ("Console".into(), Limit::Deny),
                    ("FsWrite".into(), Limit::Entries(vec!["/tmp/a\"b".into()])),
                ],
            },
            snapshot: Snapshot {
                root: PathBuf::from("/pkg"),
                folder: "__snapshots__/tests.report".into(),
                regressions: "__regressions__/tests.report".into(),
                slug: "sums-prices".into(),
                update: false,
            },
        }];
        let env = TestEnv {
            cwd: Some(Path::new("/pkg")),
            program: "tests/report.hd",
            programs: None,
        };
        assert_eq!(
            host_config(&env, &cases),
            r#"{"program":"tests/report.hd","args":[],"programs":null,"cases":[{"test":2,"init":1,"kind":"it_prop","tempDir":"/tmp/hd-test-1-0/4","seed":9,"grants":{"Console":false,"FsWrite":["/tmp/a\"b"]},"snapshot":{"dir":"/pkg/__snapshots__/tests.report","shown":"__snapshots__/tests.report","slug":"sums-prices","update":false},"regression":{"file":"/pkg/__regressions__/tests.report/sums-prices","shown":"__regressions__/tests.report/sums-prices"}}]}"#
        );
    }

    #[test]
    fn reads_a_case_line() {
        let r = parse_case(
            r#"{"test":2,"status":3,"trapped":true,"stdout":"a\"b\n","stderr":"xé😀","us":15}"#,
        )
        .expect("parses");
        assert_eq!(r.test, 2);
        assert_eq!(r.status, 3);
        assert!(r.trapped);
        assert_eq!(r.stdout, "a\"b\n");
        assert_eq!(r.stderr, "x\u{e9}\u{1f600}");
        assert_eq!(r.us, 15);
        assert!(r.last());
        let row = parse_case(
            r#"{"test":2,"row":1,"rows":3,"status":0,"trapped":false,"stdout":"","stderr":"","us":1}"#,
        )
        .expect("parses");
        assert_eq!(row.row, Some((1, 3)));
        assert!(!row.last());
        let failed = parse_case(
            r#"{"test":4,"status":3,"trapped":true,"stdout":"","stderr":"","us":1,"input":"Point { x: 50 }"}"#,
        )
        .expect("parses");
        assert_eq!(failed.input.as_deref(), Some("Point { x: 50 }"));
        let empty = parse_case(r#"{"test":2,"row":0,"rows":0}"#).expect("parses");
        assert!(empty.last());
    }
}
