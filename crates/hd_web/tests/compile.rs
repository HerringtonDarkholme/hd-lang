//! The playground's compile API over real playground examples.

use hd_web::{CompileResult, Goal, Session};

const EXAMPLES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../website/playground/examples"
);

fn example(path: &str) -> String {
    std::fs::read_to_string(format!("{EXAMPLES}/{path}")).expect("example")
}

fn errors(r: &CompileResult) -> Vec<String> {
    r.diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(hd_driver::diagnostics::Diag::text)
        .collect()
}

/// The bytes of a compile's one program.
fn wasm(r: &CompileResult) -> &[u8] {
    match r.programs.as_slice() {
        [p] => p.wasm.as_deref().expect("wasm"),
        _ => panic!("{} programs", r.programs.len()),
    }
}

#[test]
fn an_example_runs_to_wasm() {
    let mut s = Session::new();
    let r = s.compile(
        [("src/main.hd", example("inventory.hd"))],
        "src/main.hd",
        Goal::Run,
    );
    assert!(errors(&r).is_empty(), "{:?}", errors(&r));
    let wasm = wasm(&r);
    assert_eq!(&wasm[..4], b"\0asm");
}

#[test]
fn a_package_example_runs_to_wasm() {
    let files = ["src/main.hd", "src/models/mod.hd", "src/models/user.hd"]
        .map(|p| (p, example(&format!("package/{p}"))));
    let r = Session::new().compile(files, "src/main.hd", Goal::Run);
    assert!(errors(&r).is_empty(), "{:?}", errors(&r));
    wasm(&r);
}

#[test]
fn a_file_without_main_runs_as_a_script() {
    let r = Session::new().compile([("src/main.hd", "println(42)\n")], "src/main.hd", Goal::Run);
    assert!(errors(&r).is_empty(), "{:?}", errors(&r));
    wasm(&r);
}

/// `hd check --format json crates/hd_web/tests/broken.hd` reports these
/// codes, lines and columns.
#[test]
fn a_broken_file_reports_what_hd_check_reports() {
    let text = include_str!("broken.hd");
    let r = Session::new().compile([("broken.hd", text)], "broken.hd", Goal::Check);
    let got: Vec<_> = r
        .diagnostics
        .iter()
        .map(|d| {
            (
                d.code.map(hd_diag::Code::as_str),
                d.file.as_deref(),
                d.line,
                d.column,
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (Some("type-mismatch"), Some("broken.hd"), Some(6), Some(5)),
            (Some("unknown-name"), Some("broken.hd"), Some(9), Some(13)),
        ]
    );
    // The end of `totl`, for the editor's underline.
    let unknown = &r.diagnostics[1];
    assert_eq!((unknown.end_line, unknown.end_column), (Some(9), Some(17)));
    assert!(r.programs.is_empty());
}

#[test]
fn a_tests_block_lists_its_cases() {
    let r = Session::new().compile(
        [("src/main.hd", example("tests.hd"))],
        "src/main.hd",
        Goal::Test,
    );
    assert!(errors(&r).is_empty(), "{:?}", errors(&r));
    let cases: Vec<_> = r
        .tests
        .iter()
        .map(|(p, c)| (*p, c.name.as_str(), c.kind.as_str()))
        .collect();
    assert_eq!(
        cases,
        [
            (0, "lowers and joins words", "it"),
            (0, "keeps one word", "it_each"),
            (0, "renders a title", "it"),
            (0, "an empty list has no first item", "it"),
        ]
    );
    wasm(&r);
}

/// Each file directly under `tests/` is a program of its own, as `hd test`
/// builds it (`module.test.integration.program`); the unit-test program
/// leaves them out, and the cases come in file and line order.
#[test]
fn each_integration_test_file_is_its_own_program() {
    let lib = "pub fn add(a: i32, b: i32) -> i32:\n    a + b\n\ntests:\n    it(\"adds\"):\n        pass\n";
    let it = "use pkg.{add}\nuse std.testing.assert_equal\n\nit(\"sums\"):\n    assert_equal(add(1, 2), 3, reason=\"sum\")\n";
    let r = Session::new().compile(
        [("src/lib.hd", lib), ("tests/b.hd", it), ("tests/a.hd", it)],
        "src/lib.hd",
        Goal::Test,
    );
    assert!(errors(&r).is_empty(), "{:?}", errors(&r));
    let programs: Vec<_> = r.programs.iter().map(|p| p.file.as_deref()).collect();
    assert_eq!(programs, [None, Some("tests/a.hd"), Some("tests/b.hd")]);
    assert!(r.programs.iter().all(|p| p.wasm.is_some()));
    let cases: Vec<_> = r
        .tests
        .iter()
        .map(|(p, c)| (*p, c.file.as_str(), c.name.as_str()))
        .collect();
    assert_eq!(
        cases,
        [
            (0, "src/lib.hd", "adds"),
            (1, "tests/a.hd", "sums"),
            (2, "tests/b.hd", "sums"),
        ]
    );
}

#[test]
fn a_body_edit_reuses_the_store() {
    let mut s = Session::new();
    let text = example("inventory.hd");
    let cold = s.compile([("src/main.hd", text.as_str())], "src/main.hd", Goal::Run);
    wasm(&cold);
    let edited = text.replace("println(\"no sofa in stock\")", "println(\"sold out\")");
    assert_ne!(edited, text);
    let warm = s.compile([("src/main.hd", edited)], "src/main.hd", Goal::Run);
    wasm(&warm);
    assert!(
        cold.counters.modules_checked.len() > 1,
        "std is checked cold"
    );
    assert_eq!(warm.counters.modules_checked, ["playground.main"]);
    assert!(
        warm.counters.hit("check") >= 1,
        "std's check entries reused"
    );
}
