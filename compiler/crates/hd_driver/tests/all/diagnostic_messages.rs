//! The driver reports a syntax diagnostic with the message the lexer or
//! parser gave it, and an import the module does not give with the module
//! and the name, never the code twice.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

/// The `code: message` pairs of a run's errors.
fn errors(out: &Output) -> Vec<String> {
    (0..out.diags.len())
        .filter(|&i| out.diags.severity[i] == hd_diag::Severity::Error)
        .map(|i| {
            format!(
                "{}: {}",
                out.diags.code[i].as_str(),
                out.diags.get_text(out.diags.message[i])
            )
        })
        .collect()
}

#[test]
fn a_syntax_error_says_what_was_expected() {
    let out = analyze("fn main():\n    if a and b:\n        pass\n");
    assert!(
        errors(&out).contains(&"syntax-error: expected ':', found 'and'".to_owned()),
        "{:?}",
        errors(&out)
    );
}

#[test]
fn a_rule_diagnostic_says_the_rule() {
    let out = analyze("fn main():\n    x := \"Ada\";\n");
    assert!(
        errors(&out).contains(
            &"reserved-semicolon: semicolon is reserved and cannot separate statements".to_owned()
        ),
        "{:?}",
        errors(&out)
    );
}

#[test]
fn an_unknown_import_names_the_module_and_the_name() {
    let out = analyze("use std.cmp.{NoSuch}\n\nfn main():\n    pass\n");
    assert!(
        errors(&out).contains(&"unknown-import: module 'std.cmp' declares no 'NoSuch'".to_owned()),
        "{:?}",
        errors(&out)
    );
}
