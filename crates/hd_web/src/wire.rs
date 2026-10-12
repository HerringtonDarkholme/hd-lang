//! The wire format of the wasm exports (playground-wasm.md §3.1), as plain
//! functions so native tests cover it.
//!
//! A request is bytes: the file count, then each file's path and text, then
//! the entry path, then one goal byte (0 run, 1 check, 2 test). Counts and
//! string lengths are unsigned LEB128 (at most 32 bits); strings are UTF-8.
//! Nothing may follow the goal byte.
//!
//! A response is JSON text, written by hand (rule 2.2.4: no serde), plus the
//! programs' bytes when there are any. A compiled request answers
//! `{"diagnostics":[...]}`, with `"programs":[...]` and `"tests":[...]`
//! added for the test goal; a malformed one answers `{"error":"..."}`. The
//! test goal's programs (the unit-test program, then each integration test
//! file's) follow one another in the bytes, each as long as its `size`.

use std::fmt::Write as _;

use hd_driver::diagnostics::{Diag, quote};

use crate::{CompileResult, Goal, Session};

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
    pub goal: Goal,
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

/// Decodes `request` and compiles it in `session`.
pub fn respond(session: &mut Session, request: &[u8]) -> Response {
    match decode_request(request) {
        Ok(req) => {
            let result = session.compile(req.files, &req.entry, req.goal);
            let status = if result.diagnostics.iter().any(Diag::is_error) {
                STATUS_ERRORS
            } else {
                STATUS_OK
            };
            Response {
                status,
                json: encode_result(&result, req.goal),
                wasm: result
                    .programs
                    .iter()
                    .filter_map(|p| p.wasm.as_deref())
                    .flatten()
                    .copied()
                    .collect(),
            }
        }
        Err(WireError(reason)) => Response {
            status: STATUS_MALFORMED,
            json: format!("{{\"error\":{}}}", quote(reason)),
            wasm: Vec::new(),
        },
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
    let goal = match r.byte()? {
        0 => Goal::Run,
        1 => Goal::Check,
        2 => Goal::Test,
        _ => return Err(WireError("unknown goal byte")),
    };
    if r.at != bytes.len() {
        return Err(WireError("bytes after the goal byte"));
    }
    Ok(Request { files, entry, goal })
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
    let mut s = String::from("{\"diagnostics\":[");
    for (n, d) in result.diagnostics.iter().enumerate() {
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
                goal: Goal::Test,
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
            Err(WireError("bytes after the goal byte"))
        );
        let mut goal = good.clone();
        *goal.last_mut().unwrap() = 3;
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

    #[test]
    fn a_malformed_request_answers_an_error() {
        let r = respond(&mut Session::new(), &[1]);
        assert_eq!(r.status, STATUS_MALFORMED);
        assert_eq!(r.json, "{\"error\":\"request ends early\"}");
        assert!(r.wasm.is_empty());
    }
}
