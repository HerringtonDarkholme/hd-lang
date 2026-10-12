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

#[test]
fn an_example_runs_to_wasm() {
    let mut s = Session::new();
    let r = s.compile(
        [("src/main.hd", example("inventory.hd"))],
        "src/main.hd",
        Goal::Run,
    );
    assert!(errors(&r).is_empty(), "{:?}", errors(&r));
    let wasm = r.wasm.expect("wasm");
    assert_eq!(&wasm[..4], b"\0asm");
}

#[test]
fn a_package_example_runs_to_wasm() {
    let files = ["src/main.hd", "src/models/mod.hd", "src/models/user.hd"]
        .map(|p| (p, example(&format!("package/{p}"))));
    let r = Session::new().compile(files, "src/main.hd", Goal::Run);
    assert!(errors(&r).is_empty(), "{:?}", errors(&r));
    assert!(r.wasm.is_some());
}

#[test]
fn a_file_without_main_runs_as_a_script() {
    let r = Session::new().compile([("src/main.hd", "println(42)\n")], "src/main.hd", Goal::Run);
    assert!(errors(&r).is_empty(), "{:?}", errors(&r));
    assert!(r.wasm.is_some());
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
    assert!(r.wasm.is_none());
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
        .map(|c| (c.name.as_str(), c.kind.as_str()))
        .collect();
    assert_eq!(
        cases,
        [
            ("lowers and joins words", "it"),
            ("keeps one word", "it_each"),
            ("renders a title", "it"),
            ("an empty list has no first item", "it"),
        ]
    );
    assert!(r.wasm.is_some());
}

#[test]
fn a_body_edit_reuses_the_store() {
    let mut s = Session::new();
    let text = example("inventory.hd");
    let cold = s.compile([("src/main.hd", text.as_str())], "src/main.hd", Goal::Run);
    assert!(cold.wasm.is_some());
    let edited = text.replace("println(\"no sofa in stock\")", "println(\"sold out\")");
    assert_ne!(edited, text);
    let warm = s.compile([("src/main.hd", edited)], "src/main.hd", Goal::Run);
    assert!(warm.wasm.is_some());
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
