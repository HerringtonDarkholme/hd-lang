//! The playground's REPL sessions: each input builds the session program,
//! every kept input and the new one in one module, which the JS host runs
//! after replaying the kept inputs; the host reports how the input ended.

use hd_web::{ReplBuild, ReplError, ReplOutcome, Session};

fn errors(b: &ReplBuild) -> Vec<String> {
    b.diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(hd_driver::diagnostics::Diag::text)
        .collect()
}

/// The names of a module's exports.
fn exports(wasm: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut at = 8;
    // The export section (id 7), read by hand: a count, then each name,
    // its kind byte and its index.
    let leb = |bytes: &[u8], at: &mut usize| {
        let (mut value, mut shift) = (0usize, 0);
        loop {
            let b = bytes[*at];
            *at += 1;
            value |= usize::from(b & 0x7f) << shift;
            shift += 7;
            if b < 0x80 {
                return value;
            }
        }
    };
    while at < wasm.len() {
        let id = wasm[at];
        at += 1;
        let size = leb(wasm, &mut at);
        let end = at + size;
        if id == 7 {
            let count = leb(wasm, &mut at);
            for _ in 0..count {
                let len = leb(wasm, &mut at);
                names.push(String::from_utf8_lossy(&wasm[at..at + len]).into_owned());
                at += len + 1;
                leb(wasm, &mut at);
            }
        }
        at = end;
    }
    names
}

/// A started session.
fn started(files: &[(&str, &str)]) -> Session {
    let mut s = Session::new();
    let start = s.repl_start(files.iter().copied()).expect("start");
    assert!(errors(&start).is_empty(), "{:?}", errors(&start));
    assert_eq!((start.input, start.wasm.is_none()), (0, true));
    s
}

/// Builds `text` as the next input, which must build, and reports that it
/// ran with status 0.
fn joined(s: &mut Session, text: &str) -> ReplBuild {
    let b = s.repl_input(text).expect("input");
    assert!(errors(&b).is_empty(), "{text}: {:?}", errors(&b));
    assert!(b.wasm.is_some(), "{text}");
    s.repl_outcome(b.input, ReplOutcome::Joined)
        .expect("outcome");
    b
}

/// The spec's Redefinition example (`cli.repl.redeclare.binding`): a
/// binding, functions that read it, calls, and a redeclared name. Each
/// input's module holds every input so far, each with its entry exports.
#[test]
fn a_session_returns_one_program_per_input() {
    let mut s = started(&[]);
    let rate = joined(&mut s, "rate := +3\n");
    assert_eq!(rate.input, 1);
    joined(&mut s, "fn price(n: i32) -> i32: n * rate\n");
    joined(&mut s, "fn total() -> i32: price(+2)\n");
    let call = joined(&mut s, "total()\n");
    assert_eq!(call.input, 4);
    let names = exports(call.wasm.as_deref().expect("a module"));
    for n in 1..=4 {
        assert!(names.contains(&format!("hd.init.{n}")), "{names:?}");
        assert!(names.contains(&format!("hd.poll.{n}")), "{names:?}");
    }
    assert!(names.iter().any(|n| n == "hd.reset"), "{names:?}");

    let redeclared = s
        .repl_input("fn price(n: i32) -> i32: n * rate + 1\n")
        .expect("input");
    assert!(redeclared.wasm.is_none());
    let codes: Vec<_> = redeclared
        .diagnostics
        .iter()
        .map(|d| (d.code.map(hd_diag::Code::as_str), d.file.as_deref()))
        .collect();
    assert_eq!(codes, [(Some("duplicate-binding"), Some("repl#5"))]);
    // A `let` binding is assigned, not declared again.
    joined(&mut s, "let count = +1\n");
    joined(&mut s, "count = count + 1\n");
}

/// An input that panicked stays in the program but adds nothing
/// (`cli.repl.panic.binding`); one that did not run, or did not build,
/// is dropped. Their names stay free.
#[test]
fn a_failed_input_does_not_join() {
    let mut s = started(&[]);
    let panicked = s.repl_input("x := 1\n").expect("input");
    assert!(panicked.wasm.is_some(), "{:?}", errors(&panicked));
    s.repl_outcome(panicked.input, ReplOutcome::Failed)
        .expect("outcome");
    let unknown = s.repl_input("x + 1\n").expect("input");
    assert!(unknown.wasm.is_none());
    assert!(
        unknown
            .diagnostics
            .iter()
            .any(|d| d.code.map(hd_diag::Code::as_str) == Some("unknown-name")),
        "{:?}",
        errors(&unknown)
    );
    let x = joined(&mut s, "x := 2\n");
    assert_eq!(x.input, 3);
    // The failed input stays in the program, for replays.
    let names = exports(x.wasm.as_deref().expect("a module"));
    assert!(names.iter().any(|n| n == "hd.init.1"), "{names:?}");

    let stopped = s.repl_input("z := 3\n").expect("input");
    s.repl_outcome(stopped.input, ReplOutcome::Dropped)
        .expect("outcome");
    let broken = s.repl_input("y := missing\n").expect("input");
    assert!(broken.wasm.is_none());
    let y = joined(&mut s, "y := x\nz := 4\n");
    let names = exports(y.wasm.as_deref().expect("a module"));
    assert!(!names.iter().any(|n| n == "hd.init.4"), "{names:?}");
}

/// The session acts as code inside `src/lib.hd` (`cli.repl.package.lib`).
#[test]
fn a_session_sees_the_package_library() {
    let lib = "fn secret() -> i32: +7\n";
    let main = "pub fn main() -> void $ Console:\n    println(1)\n";
    let mut s = started(&[("src/lib.hd", lib), ("src/main.hd", main)]);
    joined(&mut s, "n := secret() * 2\n");
}

/// A package file that does not check stops only the inputs that reach
/// it: `src/main.hd` and a module nothing uses never, and `src/lib.hd`
/// only from the first input that names one of its names, with the
/// library's error. A session started over the fixed library runs it.
#[test]
fn a_broken_package_file_stops_only_the_inputs_that_reach_it() {
    let main = "pub fn main() -> void $ Console:\n    println(missing)\n";
    let junk = "fn (\n";
    let broken = "fn secret() -> i32: \"seven\"\n";
    let mut s = started(&[
        ("src/lib.hd", broken),
        ("src/main.hd", main),
        ("src/junk.hd", junk),
    ]);
    joined(&mut s, "1 + 2\n");
    joined(&mut s, "n := +3\n");
    let named = s.repl_input("n + secret()\n").expect("input");
    assert!(named.wasm.is_none());
    let found = errors(&named);
    assert!(
        !found.is_empty() && found.iter().all(|e| e.starts_with("error: src/lib.hd:")),
        "{found:?}"
    );
    let fixed = "fn secret() -> i32: +7\n";
    let mut s = started(&[
        ("src/lib.hd", fixed),
        ("src/main.hd", main),
        ("src/junk.hd", junk),
    ]);
    joined(&mut s, "n := +3\n");
    joined(&mut s, "n + secret()\n");
}

#[test]
fn a_request_out_of_turn_is_refused() {
    let mut s = Session::new();
    assert!(s.repl_input("1\n").is_err());
    assert!(s.repl_outcome(0, ReplOutcome::Joined).is_err());
    s.repl_start(Vec::<(&str, &str)>::new()).expect("start");
    let one = s.repl_input("1\n").expect("input");
    assert_eq!(
        s.repl_input("2\n").err(),
        Some(ReplError("input 1 has no outcome yet".into()))
    );
    assert!(s.repl_outcome(2, ReplOutcome::Joined).is_err());
    s.repl_outcome(one.input, ReplOutcome::Joined)
        .expect("outcome");
    // An outcome is taken once.
    assert!(s.repl_outcome(1, ReplOutcome::Joined).is_err());
    assert!(s.repl_start([("repl#1", "x := 1\n")]).is_err());
}
