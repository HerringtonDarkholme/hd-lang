//! The CLI tier of the specification conformance suite
//! (`spec/conformance/README.md`, "CLI Cases"): each case copies its
//! directory to a fresh temp directory, runs the `run:` lines of
//! `expect.txt` against the `hd` binary, and judges exit status, standard
//! streams and files.
//!
//! A case that needs a host service the sandbox lacks is unsupported, not
//! failed. Cases recorded as passing in the "CLI Conformance" section of
//! `compiler/CONFORMANCE.md` may not regress. Set `HD_UPDATE_CONFORMANCE=1`
//! to rewrite that section. `HD_CONFORMANCE_ONLY=a,b` runs the cases whose
//! name contains one of the parts and prints each verdict.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const HEADING: &str = "\n## CLI Conformance\n";
const PASS_START: &str = "<!-- cli-pass-list-start -->";
const PASS_END: &str = "<!-- cli-pass-list-end -->";
const LIMIT: Duration = Duration::from_secs(10);
const WORKERS: usize = 4;

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
            "file" => step.files.push((true, value.to_owned())),
            "no-file" => step.files.push((false, value.to_owned())),
            _ => return Err(format!("unknown expect.txt line: {line}")),
        }
    }
    Ok(steps)
}

// -------------------------------------------------------------- running

fn suite_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../spec/conformance")
}

fn report_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../CONFORMANCE.md")
}

fn work_root() -> PathBuf {
    std::env::temp_dir().join(format!("hd-cli-conformance-{}", std::process::id()))
}

fn copy_case(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_case(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

fn run_hd(dir: &Path, cache: &Path, words: &[String]) -> std::io::Result<Ran> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hd"))
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
        code: status.code(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
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
    if ran.timed_out {
        return Err("timed out after 10 seconds".to_owned());
    }
    let Some(code) = ran.code else {
        return Err("ended with a signal".to_owned());
    };
    let want = i32::from(step.exit.unwrap_or(0));
    if code != want {
        return Err(format!(
            "exit: want {want}, got {code}; stderr {:?}",
            ran.stderr
        ));
    }
    judge_stream(
        "stdout",
        &ran.stdout,
        &step.stdout,
        step.stdout_json.as_ref(),
    )?;
    judge_stream(
        "stderr",
        &ran.stderr,
        &step.stderr,
        step.stderr_json.as_ref(),
    )?;
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

fn run_case(name: &str, root: &Path) -> Verdict {
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
    if needs_git && git_missing() {
        return Verdict::Unsupported("needs git on PATH".to_owned());
    }
    let dir = root.join(name);
    let cache = root.join(format!("cache-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cache);
    if let Err(e) = copy_case(&source, &dir) {
        return Verdict::Fail(format!("copy case: {e}"));
    }
    // `expect.txt` is not part of the case input.
    let _ = std::fs::remove_file(dir.join("expect.txt"));
    let mut verdict = Verdict::Pass;
    for step in &steps {
        let label = format!("hd {}", step.words.join(" "));
        let result = run_hd(&dir, &cache, &step.words)
            .map_err(|e| format!("spawn failed: {e}"))
            .and_then(|ran| judge_step(&dir, step, &ran));
        if let Err(e) = result {
            verdict = Verdict::Fail(format!("`{label}`: {e}"));
            break;
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&cache);
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

fn run_all(names: &[String]) -> Vec<Verdict> {
    let root = work_root();
    std::fs::create_dir_all(&root).expect("work directory");
    let next = AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<Verdict>>> =
        names.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(name) = names.get(index) else { break };
                    let verdict = run_case(name, &root);
                    *results[index].lock().expect("results lock") = Some(verdict);
                }
            });
        }
    });
    let _ = std::fs::remove_dir_all(&root);
    results
        .into_iter()
        .map(|slot| slot.into_inner().expect("results lock").expect("verdict"))
        .collect()
}

// --------------------------------------------------------------- report

fn render_section(names: &[String], results: &[Verdict]) -> String {
    let mut pass = Vec::new();
    let mut fail = Vec::new();
    let mut unsupported = Vec::new();
    for (name, verdict) in names.iter().zip(results) {
        match verdict {
            Verdict::Pass => pass.push(name.as_str()),
            Verdict::Fail(why) => fail.push((name.as_str(), why.as_str())),
            Verdict::Unsupported(why) => unsupported.push((name.as_str(), why.as_str())),
        }
    }
    pass.sort_unstable();
    let mut out = String::from(HEADING);
    out.push_str(
        "\nThe CLI tier (`spec/conformance/cli-cases.tsv`) runs in\n`hd_cli/tests/cli_conformance.rs` against the `hd` binary.\n\n",
    );
    writeln!(
        out,
        "| Pass | Fail | Unsupported | Total |\n| ---: | ---: | ---: | ---: |\n| {} | {} | {} | {} |",
        pass.len(),
        fail.len(),
        unsupported.len(),
        names.len()
    )
    .expect("write report");
    if !unsupported.is_empty() {
        out.push_str("\nUnsupported:\n\n");
        for (name, why) in &unsupported {
            writeln!(out, "- `cli/{name}`: {why}").expect("write report");
        }
    }
    out.push_str(
        "\n`HD_UPDATE_CONFORMANCE=1` replaces this list with every CLI case that passes.\n\n",
    );
    out.push_str(PASS_START);
    out.push_str("\n```text\n");
    for name in pass {
        writeln!(out, "cli/{name}").expect("write report");
    }
    out.push_str("```\n");
    out.push_str(PASS_END);
    out.push('\n');
    out
}

fn pass_list(report: &str) -> BTreeSet<String> {
    let Some((_, after)) = report.split_once(PASS_START) else {
        return BTreeSet::new();
    };
    let Some((body, _)) = after.split_once(PASS_END) else {
        return BTreeSet::new();
    };
    body.lines()
        .filter(|line| !line.is_empty() && !line.starts_with("```"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn cli_conformance_does_not_regress() {
    let all = case_names();
    if let Ok(only) = std::env::var("HD_CONFORMANCE_ONLY") {
        let parts: Vec<&str> = only.split(',').filter(|p| !p.is_empty()).collect();
        let names: Vec<String> = all
            .into_iter()
            .filter(|name| parts.iter().any(|part| name.contains(part)))
            .collect();
        let results = run_all(&names);
        for (name, verdict) in names.iter().zip(&results) {
            println!("cli/{name}: {verdict:?}");
        }
        return;
    }
    let results = run_all(&all);
    if std::env::var_os("HD_UPDATE_CONFORMANCE").is_some() {
        let old = std::fs::read_to_string(report_path()).expect("compiler/CONFORMANCE.md");
        let head = old
            .split_once(HEADING)
            .map_or(old.as_str(), |(head, _)| head);
        let mut report = head.to_owned();
        report.push_str(&render_section(&all, &results));
        std::fs::write(report_path(), report).expect("write report");
        return;
    }
    let report = std::fs::read_to_string(report_path())
        .expect("compiler/CONFORMANCE.md; run with HD_UPDATE_CONFORMANCE=1");
    let baseline = pass_list(&report);
    assert!(
        !baseline.is_empty(),
        "the checked-in CLI pass list is empty"
    );
    let regressed: Vec<String> = baseline
        .iter()
        .filter(|path| {
            let verdict = path
                .strip_prefix("cli/")
                .and_then(|name| all.iter().position(|n| n == name))
                .map(|index| &results[index]);
            verdict != Some(&Verdict::Pass)
        })
        .map(|path| {
            let why = path
                .strip_prefix("cli/")
                .and_then(|name| all.iter().position(|n| n == name))
                .map_or_else(
                    || "no such case".to_owned(),
                    |i| format!("{:?}", results[i]),
                );
            format!("{path}: {why}")
        })
        .collect();
    assert!(
        regressed.is_empty(),
        "previously passing CLI cases regressed:\n{}",
        regressed.join("\n")
    );
}
