//! The wire format of the wasm exports (`crates/hd_web/DESIGN.md` §3.1), as plain
//! functions so native tests cover it.
//!
//! A request is bytes: the file count, then each file's path and text, then
//! the entry path, then one goal byte, then the goal's own fields. Counts,
//! numbers and string lengths are unsigned LEB128 (at most 32 bits);
//! strings are UTF-8. Nothing may follow the goal's fields.
//!
//! | Goal byte | Goal | Files and entry | Fields after the goal byte |
//! | --- | --- | --- | --- |
//! | 0 | run | the playground's files; the file Run builds | none |
//! | 1 | check | the same | none |
//! | 2 | test | the same | none |
//! | 3 | legacy run alias | the same | none |
//! | 4 | repl start | the files the session acts in (its package); entry empty | none |
//! | 5 | repl input | none; entry empty | the input's text (a string), then optionally its number |
//! | 6 | repl outcome | none; entry empty | the module's number, then one outcome byte |
//! | 7 | repl rebuild input | none; entry empty | the input's text, then its number; builds a whole linked base |
//!
//! WAT requests compile as Run. The worker prints those bytes with the
//! separate `hd_wat` module, loaded only when the WAT view requests it.
//!
//! A response is JSON text, written by hand (rule 2.2.4: no serde), plus the
//! programs' bytes when there are any. A compiled request answers
//! `{"diagnostics":[...]}`, with `"programs":[...]` and `"tests":[...]`
//! added for the test goal; a malformed one answers `{"error":"..."}`. The
//! test goal's programs (the unit-test program, then each integration test
//! file's) follow one another in the bytes, each as long as its `size`.
//!
//! **REPL goals** drive the session's REPL session
//! ([`Session::repl_start`]), one input per request:
//!
//! - Repl start drops any earlier REPL session and starts a new one over
//!   the files; it builds nothing and answers input 0 with no bytes. Repl
//!   input builds the next input, `repl#N`, as a base or an extension
//!   importing the completed inputs' initialized storage. It runs from
//!   exports `hd.init.N` and `hd.poll.N`. N is the number the request
//!   gives, which must be greater than every input's before it, or else
//!   one more than the last input's; a host that rebuilds a session
//!   builds its kept inputs again under their old numbers with repl
//!   rebuild input, producing a whole linked base for historical replay.
//!   Both input goals answer
//!   `{"diagnostics":[...],"input":N}` and the module in the bytes (output
//!   kind 1), or no bytes and status 1 when it does not build; such an
//!   input is dropped at once.
//! - The host runs an ordinary input against initialized storage, or
//!   replays history into a rebuilt base (host/core.ts `createSession`),
//!   then reports
//!   how input N ended with repl outcome: outcome byte 0 when it ended
//!   with status 0, so it joins the session; 1 when it did not run or was
//!   stopped or cancelled, so it is dropped; 2 when it panicked or ended
//!   with another status, so it stays for replays but later inputs do not
//!   see what it declares. The answer is
//!   `{"diagnostics":[],"input":N,"outcome":"joined"|"dropped"|"failed"}`,
//!   no bytes.
//! - The session builds nothing more until the waiting input's outcome
//!   arrives. A REPL request that does not fit the session's state (no
//!   session, an input still waiting, an outcome for another input)
//!   answers status 2 and `{"error":"..."}`, as a malformed request does.

use std::fmt::Write as _;

use hd_driver::diagnostics::{Diag, quote};

use crate::{CompileResult, Goal, ReplBuild, ReplError, ReplOutcome, Session};

/// `hd_compile`'s status: compiled, and no diagnostic is an error.
pub const STATUS_OK: u32 = 0;
/// Compiled, and some diagnostic is an error.
pub const STATUS_ERRORS: u32 = 1;
/// The request did not decode; the JSON holds the reason.
pub const STATUS_MALFORMED: u32 = 2;

/// One decoded request.
#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    /// Playground paths and their texts, in request order.
    pub files: Vec<(String, String)>,
    pub entry: String,
    pub action: Action,
}

/// What a request asks for, by its goal byte.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Goal bytes 0 to 3; byte 3 remains an alias for Run.
    Compile { goal: Goal },
    /// Goal byte 4: start a REPL session over the request's files.
    ReplStart,
    /// Goal byte 5: build the REPL session's next input, numbered
    /// `number`, or one after the last module.
    ReplInput { text: String, number: Option<u32> },
    /// Goal byte 6: how input `input` ended.
    ReplOutcome { input: u32, outcome: ReplOutcome },
    /// Goal byte 7: a whole linked base for a recorded input's replay.
    ReplRebuildInput { text: String, number: u32 },
}

/// Why a request did not decode.
#[derive(Debug, PartialEq, Eq)]
pub struct WireError(pub &'static str);

/// What one request produced: the status, the JSON text and the program.
pub struct Response {
    pub status: u32,
    pub json: String,
    /// The programs one after another; empty when the compile produced
    /// none.
    pub wasm: Vec<u8>,
}

/// Decodes `request` and runs it in `session`.
pub fn respond(session: &mut Session, request: &[u8]) -> Response {
    let req = match decode_request(request) {
        Ok(req) => req,
        Err(WireError(reason)) => return refused(reason),
    };
    match req.action {
        Action::Compile { goal } => compile(session, req.files, &req.entry, goal),
        Action::ReplStart => repl_build(session.repl_start(req.files)),
        Action::ReplInput { text, number: None } => repl_build(session.repl_input(&text)),
        Action::ReplInput {
            text,
            number: Some(number),
        } => repl_build(session.repl_input_as(number, &text)),
        Action::ReplRebuildInput { text, number } => {
            repl_build(session.repl_rebuild_input_as(number, &text))
        }
        Action::ReplOutcome { input, outcome } => match session.repl_outcome(input, outcome) {
            Ok(()) => Response {
                status: STATUS_OK,
                json: format!(
                    "{{\"diagnostics\":[],\"input\":{input},\"outcome\":\"{}\"}}",
                    match outcome {
                        ReplOutcome::Joined => "joined",
                        ReplOutcome::Dropped => "dropped",
                        ReplOutcome::Failed => "failed",
                    }
                ),
                wasm: Vec::new(),
            },
            Err(ReplError(reason)) => refused(&reason),
        },
    }
}

/// A request that did not decode or does not fit the session's state.
fn refused(reason: &str) -> Response {
    Response {
        status: STATUS_MALFORMED,
        json: format!("{{\"error\":{}}}", quote(reason)),
        wasm: Vec::new(),
    }
}

/// Compiles goal bytes 0 to 3.
fn compile(
    session: &mut Session,
    files: Vec<(String, String)>,
    entry: &str,
    goal: Goal,
) -> Response {
    let result = session.compile(files, entry, goal);
    let wasm: Vec<u8> = result
        .programs
        .iter()
        .filter_map(|p| p.wasm.as_deref())
        .flatten()
        .copied()
        .collect();
    Response {
        status: status(&result.diagnostics),
        json: encode_result(&result, goal),
        wasm,
    }
}

fn status(diagnostics: &[Diag]) -> u32 {
    if diagnostics.iter().any(Diag::is_error) {
        STATUS_ERRORS
    } else {
        STATUS_OK
    }
}

/// Answers a REPL start or input: the JSON, and the module's bytes.
fn repl_build(result: Result<ReplBuild, ReplError>) -> Response {
    match result {
        Ok(build) => Response {
            status: status(&build.diagnostics),
            json: encode_repl(&build),
            wasm: build.wasm.unwrap_or_default(),
        },
        Err(ReplError(reason)) => refused(&reason),
    }
}

/// Decodes a request; any malformed input is an error, never a panic.
pub fn decode_request(bytes: &[u8]) -> Result<Request, WireError> {
    let mut r = Reader { bytes, at: 0 };
    let count = r.leb()?;
    let mut files = Vec::new();
    for _ in 0..count {
        let path = r.string()?;
        let text = r.string()?;
        files.push((path, text));
    }
    let entry = r.string()?;
    let byte = r.byte()?;
    let action = match byte {
        0 | 3 => Action::Compile { goal: Goal::Run },
        1 => Action::Compile { goal: Goal::Check },
        2 => Action::Compile { goal: Goal::Test },
        4 => Action::ReplStart,
        5 => {
            let text = r.string()?;
            let number = if r.at == bytes.len() {
                None
            } else {
                Some(
                    u32::try_from(r.leb()?)
                        .map_err(|_| WireError("number does not fit 32 bits"))?,
                )
            };
            Action::ReplInput { text, number }
        }
        7 => {
            let text = r.string()?;
            let number =
                u32::try_from(r.leb()?).map_err(|_| WireError("number does not fit 32 bits"))?;
            Action::ReplRebuildInput { text, number }
        }
        6 => {
            let input =
                u32::try_from(r.leb()?).map_err(|_| WireError("number does not fit 32 bits"))?;
            let outcome = match r.byte()? {
                0 => ReplOutcome::Joined,
                1 => ReplOutcome::Dropped,
                2 => ReplOutcome::Failed,
                _ => return Err(WireError("unknown outcome byte")),
            };
            Action::ReplOutcome { input, outcome }
        }
        _ => return Err(WireError("unknown goal byte")),
    };
    if r.at != bytes.len() {
        return Err(WireError("bytes after the request's last field"));
    }
    if byte >= 4 && !entry.is_empty() {
        return Err(WireError("a REPL request has no entry"));
    }
    if byte >= 5 && !files.is_empty() {
        return Err(WireError("a REPL input or outcome carries no files"));
    }
    Ok(Request {
        files,
        entry,
        action,
    })
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, WireError> {
        let b = *self
            .bytes
            .get(self.at)
            .ok_or(WireError("request ends early"))?;
        self.at += 1;
        Ok(b)
    }

    /// An unsigned LEB128 number of at most 32 bits.
    fn leb(&mut self) -> Result<usize, WireError> {
        let mut value = 0u32;
        for shift in [0, 7, 14, 21, 28] {
            let b = self.byte()?;
            // The fifth byte carries the top 4 bits and ends the number.
            if shift == 28 && b > 0x0f {
                return Err(WireError("number does not fit 32 bits"));
            }
            value |= u32::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                break;
            }
        }
        usize::try_from(value).map_err(|_| WireError("number does not fit 32 bits"))
    }

    fn string(&mut self) -> Result<String, WireError> {
        let len = self.leb()?;
        let end = self
            .at
            .checked_add(len)
            .ok_or(WireError("request ends early"))?;
        let raw = self
            .bytes
            .get(self.at..end)
            .ok_or(WireError("request ends early"))?;
        let text = std::str::from_utf8(raw).map_err(|_| WireError("string is not UTF-8"))?;
        self.at = end;
        Ok(text.to_owned())
    }
}

/// The JSON text of a compile: its diagnostics, and for the test goal its
/// programs (`file`, the integration test file or null for the unit-test
/// program, and `size`, its byte count) and its cases (`program`, the index
/// of the case's program; `seed`, a property case's base seed
/// (`cli.test.seed`), else null; `run`, `[test, init]`, the case's
/// `hd.test.i` and `hd.init.j` export numbers, or null for a case that
/// cannot run).
#[must_use]
pub fn encode_result(result: &CompileResult, goal: Goal) -> String {
    let mut s = String::from("{\"diagnostics\":");
    push_diagnostics(&mut s, &result.diagnostics);
    if goal == Goal::Test {
        s.push_str(",\"programs\":[");
        for (n, p) in result.programs.iter().enumerate() {
            if n > 0 {
                s.push(',');
            }
            let _ = write!(
                s,
                "{{\"file\":{},\"size\":{}}}",
                opt_str(p.file.as_deref()),
                p.wasm.as_ref().map_or(0, Vec::len)
            );
        }
        s.push_str("],\"tests\":[");
        for (n, (program, t)) in result.tests.iter().enumerate() {
            if n > 0 {
                s.push(',');
            }
            let run = t
                .run
                .map_or_else(|| "null".to_owned(), |(i, j)| format!("[{i},{j}]"));
            let _ = write!(
                s,
                "{{\"program\":{program},\"name\":{},\"file\":{},\"line\":{},\"kind\":{},\"ignore\":{},\"expectPanic\":{},\"unsupported\":{},\"seed\":{},\"run\":{run}}}",
                quote(&t.name),
                quote(&t.file),
                t.line,
                quote(&t.kind),
                opt_str(t.ignore.as_deref()),
                opt_str(t.expect_panic.as_deref()),
                opt_str(t.unsupported.as_deref()),
                t.base_seed(None)
                    .map_or_else(|| "null".to_owned(), |n| n.to_string()),
            );
        }
        s.push(']');
    }
    s.push('}');
    s
}

/// The JSON text of a REPL start or input: its diagnostics and the
/// input's number (`input`).
#[must_use]
pub fn encode_repl(build: &ReplBuild) -> String {
    let mut s = String::from("{\"diagnostics\":");
    push_diagnostics(&mut s, &build.diagnostics);
    let _ = write!(s, ",\"input\":{}}}", build.input);
    s
}

/// Appends `diagnostics` as a JSON array.
fn push_diagnostics(s: &mut String, diagnostics: &[Diag]) {
    s.push('[');
    for (n, d) in diagnostics.iter().enumerate() {
        if n > 0 {
            s.push(',');
        }
        let _ = write!(
            s,
            "{{\"path\":{},\"code\":{},\"severity\":\"{}\",\"message\":{},\"line\":{},\"column\":{},\"endLine\":{},\"endColumn\":{}}}",
            opt_str(d.file.as_deref()),
            opt_str(d.code.map(hd_diag::Code::as_str)),
            d.severity_word(),
            quote(&d.message),
            opt_num(d.line),
            opt_num(d.column),
            opt_num(d.end_line),
            opt_num(d.end_column),
        );
    }
    s.push(']');
}

fn opt_str(s: Option<&str>) -> String {
    s.map_or_else(|| "null".to_owned(), quote)
}

fn opt_num(n: Option<usize>) -> String {
    n.map_or_else(|| "null".to_owned(), |n| n.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leb(out: &mut Vec<u8>, mut n: usize) {
        loop {
            let low = u8::try_from(n & 0x7f).unwrap();
            n >>= 7;
            if n == 0 {
                out.push(low);
                return;
            }
            out.push(low | 0x80);
        }
    }

    fn string(out: &mut Vec<u8>, s: &str) {
        leb(out, s.len());
        out.extend_from_slice(s.as_bytes());
    }

    fn request(files: &[(&str, &str)], entry: &str, goal: u8) -> Vec<u8> {
        let mut out = Vec::new();
        leb(&mut out, files.len());
        for (path, text) in files {
            string(&mut out, path);
            string(&mut out, text);
        }
        string(&mut out, entry);
        out.push(goal);
        out
    }

    #[test]
    fn a_request_decodes() {
        let long = "x".repeat(200); // a two-byte length
        let bytes = request(
            &[("src/main.hd", "println(\"λ\")\n"), ("src/a.hd", &long)],
            "src/main.hd",
            2,
        );
        let req = decode_request(&bytes).unwrap();
        assert_eq!(
            req,
            Request {
                files: vec![
                    ("src/main.hd".into(), "println(\"λ\")\n".into()),
                    ("src/a.hd".into(), long),
                ],
                entry: "src/main.hd".into(),
                action: Action::Compile { goal: Goal::Test },
            }
        );
    }

    #[test]
    fn a_malformed_request_is_an_error() {
        let good = request(&[("a.hd", "x")], "a.hd", 0);
        // Every truncation fails, and so does a trailing byte.
        for end in 0..good.len() {
            assert!(decode_request(&good[..end]).is_err(), "{end}");
        }
        let mut long = good.clone();
        long.push(0);
        assert_eq!(
            decode_request(&long),
            Err(WireError("bytes after the request's last field"))
        );
        let mut goal = good.clone();
        *goal.last_mut().unwrap() = 255;
        assert_eq!(decode_request(&goal), Err(WireError("unknown goal byte")));
        // A count past 32 bits, a length past the end, and bad UTF-8.
        assert!(decode_request(&[0xff, 0xff, 0xff, 0xff, 0x7f]).is_err());
        assert!(decode_request(&[1, 0xff, 0xff, 0xff, 0xff, 0x0f]).is_err());
        assert_eq!(
            decode_request(&[1, 1, 0xff, 0, 0, 0]),
            Err(WireError("string is not UTF-8"))
        );
    }

    #[test]
    fn a_compile_answers_json_and_wasm() {
        let mut session = Session::new();
        let bytes = request(
            &[(
                "src/main.hd",
                "pub fn main() -> void $ Console:\n    println(42)\n",
            )],
            "src/main.hd",
            0,
        );
        let r = respond(&mut session, &bytes);
        assert_eq!(
            (r.status, r.json.as_str()),
            (STATUS_OK, "{\"diagnostics\":[]}")
        );
        assert_eq!(&r.wasm[..4], b"\0asm");
    }

    #[test]
    fn the_legacy_wat_goal_builds_the_same_program() {
        let mut session = Session::new();
        let files = [(
            "src/main.hd",
            "pub fn main() -> void $ Console:\n    println(42)\n",
        )];
        let r = respond(&mut session, &request(&files, "src/main.hd", 3));
        assert_eq!(
            (r.status, r.json.as_str()),
            (STATUS_OK, "{\"diagnostics\":[]}")
        );
        assert_eq!(&r.wasm[..4], b"\0asm");
        let run = respond(&mut session, &request(&files, "src/main.hd", 0));
        assert_eq!(r.wasm, run.wasm);
        // A broken file has no program to print.
        let bad = respond(
            &mut session,
            &request(&[("main.hd", "x := \"a\\b\" + 1\n")], "main.hd", 3),
        );
        assert_eq!(bad.status, STATUS_ERRORS);
        assert!(bad.wasm.is_empty());
    }

    #[test]
    fn diagnostics_and_tests_encode() {
        let mut session = Session::new();
        let bad = request(&[("main.hd", "x := \"a\\b\" + 1\n")], "main.hd", 1);
        let r = respond(&mut session, &bad);
        assert_eq!(r.status, STATUS_ERRORS);
        assert!(r.wasm.is_empty());
        assert!(
            r.json
                .starts_with("{\"diagnostics\":[{\"path\":\"main.hd\",\"code\":\""),
            "{}",
            r.json
        );
        assert!(r.json.contains("\"severity\":\"error\""), "{}", r.json);
        assert!(
            r.json.contains("\"endLine\":1,\"endColumn\":"),
            "{}",
            r.json
        );
        assert!(!r.json.contains("\"tests\""), "{}", r.json);

        let tests = "fn add(a: i32, b: i32) -> i32:\n    a + b\n\ntests:\n    use std.testing.assert_equal\n\n    it(\"adds\"):\n        assert_equal(add(1, 2), 3, reason=\"sum\")\n";
        let r = respond(&mut session, &request(&[("main.hd", tests)], "main.hd", 2));
        assert_eq!(r.status, STATUS_OK, "{}", r.json);
        let size = r.wasm.len();
        assert!(
            r.json.starts_with(&format!(
                "{{\"diagnostics\":[],\"programs\":[{{\"file\":null,\"size\":{size}}}],\"tests\":[{{\"program\":0,\"name\":\"adds\",\"file\":\"main.hd\",\"line\":7,\"kind\":\"it\",\"ignore\":null,\"expectPanic\":null,\"unsupported\":null,\"seed\":null,\"run\":["
            )),
            "{}",
            r.json
        );
        assert_eq!(&r.wasm[..4], b"\0asm");
    }

    #[test]
    fn each_test_program_has_its_size_and_its_bytes() {
        let lib = "pub fn add(a: i32, b: i32) -> i32:\n    a + b\n\ntests:\n    it(\"adds\"):\n        pass\n";
        let it = "use pkg.{add}\nuse std.testing.assert_equal\n\nit(\"sums\"):\n    assert_equal(add(1, 2), 3, reason=\"sum\")\n";
        let files = [("src/lib.hd", lib), ("tests/sums.hd", it)];
        let r = respond(&mut Session::new(), &request(&files, "src/lib.hd", 2));
        assert_eq!(r.status, STATUS_OK, "{}", r.json);
        let sizes: Vec<usize> = r
            .json
            .split("\"size\":")
            .skip(1)
            .map(|rest| rest[..rest.find('}').unwrap()].parse().unwrap())
            .collect();
        assert_eq!(sizes.len(), 2, "{}", r.json);
        assert!(
            r.json.contains("{\"file\":\"tests/sums.hd\",\"size\":"),
            "{}",
            r.json
        );
        assert_eq!(sizes.iter().sum::<usize>(), r.wasm.len());
        assert_eq!(&r.wasm[sizes[0]..sizes[0] + 4], b"\0asm");
    }

    /// A REPL input's request: no files, no entry, goal 5, the text.
    fn repl_input(text: &str) -> Vec<u8> {
        let mut out = request(&[], "", 5);
        string(&mut out, text);
        out
    }

    /// A REPL outcome's request: goal 6, the module, the outcome byte.
    fn repl_outcome(input: usize, outcome: u8) -> Vec<u8> {
        let mut out = request(&[], "", 6);
        leb(&mut out, input);
        out.push(outcome);
        out
    }

    #[test]
    fn repl_requests_decode() {
        let start = request(&[("src/lib.hd", "x := 1\n")], "", 4);
        assert_eq!(decode_request(&start).unwrap().action, Action::ReplStart);
        assert_eq!(
            decode_request(&repl_input("1 + 2\n")).unwrap().action,
            Action::ReplInput {
                text: "1 + 2\n".into(),
                number: None
            }
        );
        let mut numbered = repl_input("1 + 2\n");
        leb(&mut numbered, 300);
        assert_eq!(
            decode_request(&numbered).unwrap().action,
            Action::ReplInput {
                text: "1 + 2\n".into(),
                number: Some(300)
            }
        );
        assert_eq!(
            decode_request(&repl_outcome(300, 1)).unwrap().action,
            Action::ReplOutcome {
                input: 300,
                outcome: ReplOutcome::Dropped
            }
        );
        let mut rebuilt = request(&[], "", 7);
        string(&mut rebuilt, "count = count + 1\n");
        leb(&mut rebuilt, 300);
        assert_eq!(
            decode_request(&rebuilt).unwrap().action,
            Action::ReplRebuildInput {
                text: "count = count + 1\n".into(),
                number: 300
            }
        );
        let bad = |bytes: &[u8]| decode_request(bytes).unwrap_err().0;
        numbered.push(0);
        assert_eq!(bad(&numbered), "bytes after the request's last field");
        assert_eq!(
            decode_request(&repl_outcome(1, 2)).unwrap().action,
            Action::ReplOutcome {
                input: 1,
                outcome: ReplOutcome::Failed
            }
        );
        assert_eq!(bad(&repl_outcome(1, 3)), "unknown outcome byte");
        assert_eq!(
            bad(&request(&[], "src/main.hd", 4)),
            "a REPL request has no entry"
        );
        let mut with_file = request(&[("a.hd", "x")], "", 5);
        string(&mut with_file, "1\n");
        assert_eq!(bad(&with_file), "a REPL input or outcome carries no files");
        let mut trailing = repl_outcome(1, 0);
        trailing.push(0);
        assert_eq!(bad(&trailing), "bytes after the request's last field");
        let good = repl_outcome(1, 0);
        for end in 0..good.len() {
            assert!(decode_request(&good[..end]).is_err(), "{end}");
        }
    }

    #[test]
    fn a_repl_session_answers_modules_and_outcomes() {
        let mut session = Session::new();
        let refused = respond(&mut session, &repl_input("1\n"));
        assert_eq!(refused.status, STATUS_MALFORMED);
        assert_eq!(
            refused.json,
            "{\"error\":\"no REPL session: start one first\"}"
        );

        let start = respond(&mut session, &request(&[], "", 4));
        assert_eq!(
            (start.status, start.json.as_str()),
            (STATUS_OK, "{\"diagnostics\":[],\"input\":0}")
        );
        assert!(start.wasm.is_empty());

        let input = respond(&mut session, &repl_input("x := 20\n"));
        assert_eq!(input.status, STATUS_OK, "{}", input.json);
        assert_eq!(input.json, "{\"diagnostics\":[],\"input\":1}");
        assert_eq!(&input.wasm[..4], b"\0asm");
        let waiting = respond(&mut session, &repl_input("1\n"));
        assert_eq!(waiting.json, "{\"error\":\"input 1 has no outcome yet\"}");
        let dropped = respond(&mut session, &repl_outcome(1, 1));
        assert_eq!(
            (dropped.status, dropped.json.as_str()),
            (
                STATUS_OK,
                "{\"diagnostics\":[],\"input\":1,\"outcome\":\"dropped\"}"
            )
        );
        assert!(dropped.wasm.is_empty());
        let unknown = respond(&mut session, &repl_input("x\n"));
        assert_eq!(unknown.status, STATUS_ERRORS);
        assert!(
            unknown
                .json
                .starts_with("{\"diagnostics\":[{\"path\":\"repl#2\",\"code\":\"unknown-name\""),
            "{}",
            unknown.json
        );
        assert!(unknown.json.ends_with("\"input\":2}"));
        assert!(unknown.wasm.is_empty());

        // A numbered input keeps its number, which only goes up.
        let mut numbered = repl_input("y\n");
        leb(&mut numbered, 7);
        let seven = respond(&mut session, &numbered);
        assert!(
            seven
                .json
                .starts_with("{\"diagnostics\":[{\"path\":\"repl#7\""),
            "{}",
            seven.json
        );
        let after = respond(&mut session, &repl_input("1\n"));
        assert!(after.json.ends_with("\"input\":8}"), "{}", after.json);
        respond(&mut session, &repl_outcome(8, 0));
        let mut again = repl_input("1\n");
        leb(&mut again, 8);
        assert_eq!(
            respond(&mut session, &again).json,
            "{\"error\":\"input 8 does not come after input 8\"}"
        );
    }

    #[test]
    fn a_malformed_request_answers_an_error() {
        let r = respond(&mut Session::new(), &[1]);
        assert_eq!(r.status, STATUS_MALFORMED);
        assert_eq!(r.json, "{\"error\":\"request ends early\"}");
        assert!(r.wasm.is_empty());
    }
}
