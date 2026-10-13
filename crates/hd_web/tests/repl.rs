//! The playground's REPL sessions (repl-plan.md job W3): one module per
//! input, linked by the JS host, which reports each module's outcome.

use std::collections::BTreeSet;

use hd_web::{ReplBuild, ReplError, Session};

fn errors(b: &ReplBuild) -> Vec<String> {
    b.diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(hd_driver::diagnostics::Diag::text)
        .collect()
}

/// The names a module imports from the session (`hd.s`).
fn session_imports(wasm: &[u8]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let Ok(wasmparser::Payload::ImportSection(imports)) = payload {
            for import in imports.into_imports().flatten() {
                if import.module == hd_wasm::SESSION_MODULE {
                    names.insert(import.name.to_owned());
                }
            }
        }
    }
    names
}

/// A session whose prelude joined, and the keys it exports.
fn started(files: &[(&str, &str)]) -> (Session, BTreeSet<String>) {
    let mut s = Session::new();
    let prelude = s.repl_start(files.iter().copied()).expect("start");
    assert!(errors(&prelude).is_empty(), "{:?}", errors(&prelude));
    assert_eq!(prelude.input, 0);
    let wasm = prelude.wasm.as_deref().expect("prelude");
    assert_eq!(&wasm[..4], b"\0asm");
    s.repl_outcome(0, true).expect("outcome");
    (s, prelude.session_keys.into_iter().collect())
}

/// Builds `text` as the next input, which must build, and reports that it
/// ran with status 0.
fn joined(s: &mut Session, text: &str, keys: &mut BTreeSet<String>) -> ReplBuild {
    let b = s.repl_input(text).expect("input");
    assert!(errors(&b).is_empty(), "{text}: {:?}", errors(&b));
    let wasm = b.wasm.as_deref().expect("module");
    // Every name it imports from the session, an earlier module exports.
    let missing: Vec<_> = session_imports(wasm).difference(keys).cloned().collect();
    assert!(missing.is_empty(), "{text}: imports {missing:?}");
    s.repl_outcome(b.input, true).expect("outcome");
    keys.extend(b.session_keys.iter().cloned());
    b
}

/// The spec's Redefinition example (`cli.repl.redeclare.binding`): a
/// binding, functions that read it, calls, and a redeclared name.
#[test]
fn a_session_returns_one_module_per_input() {
    let (mut s, mut keys) = started(&[]);
    assert!(keys.contains(hd_wasm::MEMORY_KEY), "{keys:?}");
    assert!(keys.iter().any(|k| k.starts_with("rt.")), "{keys:?}");

    let rate = joined(&mut s, "rate := +3\n", &mut keys);
    assert_eq!(rate.input, 1);
    assert!(
        rate.session_keys.iter().any(|k| k.starts_with("bind.")),
        "{:?}",
        rate.session_keys
    );
    assert!(rate.session_keys.iter().all(|k| hd_wasm::is_session_key(k)));
    joined(&mut s, "fn price(n: i32) -> i32: n * rate\n", &mut keys);
    joined(&mut s, "fn total() -> i32: price(+2)\n", &mut keys);
    let call = joined(&mut s, "total()\n", &mut keys);
    assert_eq!(call.input, 4);
    assert!(
        call.session_keys.iter().any(|k| k.starts_with("inst.")),
        "{:?}",
        call.session_keys
    );
    // The next call imports `total` and `price` instead of linking them.
    let again = joined(&mut s, "total() + 1\n", &mut keys);
    let imports = session_imports(again.wasm.as_deref().unwrap());
    assert!(
        call.session_keys
            .iter()
            .filter(|k| k.starts_with("inst."))
            .any(|k| imports.contains(k)),
        "{imports:?}"
    );

    let redeclared = s
        .repl_input("fn price(n: i32) -> i32: n * rate + 1\n")
        .expect("input");
    assert!(redeclared.wasm.is_none());
    let codes: Vec<_> = redeclared
        .diagnostics
        .iter()
        .map(|d| (d.code.map(hd_diag::Code::as_str), d.file.as_deref()))
        .collect();
    assert_eq!(codes, [(Some("duplicate-binding"), Some("repl#6"))]);
    // A `let` binding is assigned, not declared again.
    joined(&mut s, "let count = +1\n", &mut keys);
    joined(&mut s, "count = count + 1\n", &mut keys);
}

/// An input that panicked, and one that did not build, add nothing
/// (`cli.repl.panic.binding`): their names stay free.
#[test]
fn a_failed_input_does_not_join() {
    let (mut s, mut keys) = started(&[]);
    let panicked = s.repl_input("x := 1\n").expect("input");
    assert!(panicked.wasm.is_some(), "{:?}", errors(&panicked));
    s.repl_outcome(panicked.input, false).expect("outcome");
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
    let x = joined(&mut s, "x := 2\n", &mut keys);
    assert_eq!(x.input, 3);

    let broken = s.repl_input("y := missing\n").expect("input");
    assert!(broken.wasm.is_none() && broken.session_keys.is_empty());
    joined(&mut s, "y := x\n", &mut keys);
}

/// The session acts as code inside `src/lib.hd` (`cli.repl.package.lib`).
#[test]
fn a_session_sees_the_package_library() {
    let lib = "fn secret() -> i32: +7\n";
    let main = "pub fn main() -> void $ Console:\n    println(1)\n";
    let (mut s, mut keys) = started(&[("src/lib.hd", lib), ("src/main.hd", main)]);
    joined(&mut s, "n := secret() * 2\n", &mut keys);
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
    let (mut s, mut keys) = started(&[
        ("src/lib.hd", broken),
        ("src/main.hd", main),
        ("src/junk.hd", junk),
    ]);
    joined(&mut s, "1 + 2\n", &mut keys);
    joined(&mut s, "n := +3\n", &mut keys);
    let named = s.repl_input("n + secret()\n").expect("input");
    assert!(named.wasm.is_none());
    let found = errors(&named);
    assert!(
        !found.is_empty() && found.iter().all(|e| e.starts_with("error: src/lib.hd:")),
        "{found:?}"
    );
    let fixed = "fn secret() -> i32: +7\n";
    let (mut s, mut keys) = started(&[
        ("src/lib.hd", fixed),
        ("src/main.hd", main),
        ("src/junk.hd", junk),
    ]);
    joined(&mut s, "n := +3\n", &mut keys);
    joined(&mut s, "n + secret()\n", &mut keys);
}

#[test]
fn a_request_out_of_turn_is_refused() {
    let mut s = Session::new();
    assert!(s.repl_input("1\n").is_err());
    assert!(s.repl_outcome(0, true).is_err());
    let prelude = s.repl_start(Vec::<(&str, &str)>::new()).expect("start");
    assert_eq!(
        s.repl_input("1\n").err(),
        Some(ReplError("module 0 has no outcome yet".into()))
    );
    assert!(s.repl_outcome(1, true).is_err());
    s.repl_outcome(prelude.input, true).expect("outcome");
    // An outcome is taken once.
    assert!(s.repl_outcome(0, true).is_err());
    assert!(s.repl_start([("repl#1", "x := 1\n")]).is_err());
    // A prelude that did not run ends the session.
    let prelude = s.repl_start(Vec::<(&str, &str)>::new()).expect("start");
    s.repl_outcome(prelude.input, false).expect("outcome");
    assert!(s.repl_input("1\n").is_err());
}
