//! Node as an `hd_run::Engine` (build-order.md §22: until wasmtime is an
//! approved dependency, `hd run` and `hd test` run on V8 through Node).
//! The JS host (`compiler/host/*.mjs`) is embedded, so the binary is
//! relocatable.

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::Poll;

use hd_cache::CacheStore;
use hd_run::{Engine, HostSetup, Instance, LoadError, Outcome, StartError};

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
        _host: &HostSetup,
    ) -> Result<Box<dyn Instance>, StartError> {
        Ok(Box::new(NodeInstance {
            wasm: m.wasm.clone(),
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
    /// Runs the whole program in one Node process; output goes to ours.
    fn run(&self) -> Outcome {
        let dir = match Scratch::new(&self.wasm) {
            Ok(d) => d,
            Err(e) => return Outcome::Internal(e),
        };
        let out = Command::new("node")
            .arg(dir.0.join("run.mjs"))
            .arg(dir.0.join("main.wasm"))
            .output();
        match out {
            Ok(o) => {
                let _ = std::io::stdout().write_all(&o.stdout);
                if o.status.success() {
                    Outcome::Exit(0)
                } else {
                    let _ = std::io::stderr().write_all(&o.stderr);
                    Outcome::Exit(
                        u8::try_from(o.status.code().unwrap_or(1).clamp(1, 255)).unwrap_or(1),
                    )
                }
            }
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

/// One finished case as the test host reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaseRun {
    pub test: u32,
    pub status: i64,
    pub trapped: bool,
    pub stdout: String,
    pub stderr: String,
    pub us: u64,
}

/// Runs `cases` (`(test, init)` export indices) of a test program in one
/// Node process, each in a fresh instance, and hands each result to `done`
/// as it ends.
pub fn run_cases(
    wasm: &[u8],
    cases: &[(u32, u32)],
    done: &mut dyn FnMut(CaseRun),
) -> Result<(), String> {
    let dir = Scratch::new(wasm)?;
    let list: Vec<String> = cases.iter().map(|(t, i)| format!("{t}:{i}")).collect();
    let mut child = Command::new("node")
        .arg(dir.0.join("test.mjs"))
        .arg(dir.0.join("main.wasm"))
        .arg(list.join(","))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("node: {e}"))?;
    let stdout = child.stdout.take().ok_or("node: no standard output")?;
    let mut seen = 0;
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|e| format!("node: {e}"))?;
        let r = parse_case(&line).ok_or_else(|| format!("node: unreadable result `{line}`"))?;
        seen += 1;
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
    use super::parse_case;

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
    }
}
