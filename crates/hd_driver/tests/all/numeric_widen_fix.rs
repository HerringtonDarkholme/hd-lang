//! A value of a narrower numeric type where a wider one of its family is
//! expected is `type-mismatch` with a fix-it that writes the conversion
//! around the value, as `i64(small)`
//! (spec/lang/04-type-system.md types.num.no-implicit.wider,
//! types.num.no-implicit.fix). Narrowing and mixed families offer none.

use hd_diag::{Code, DiagBuf};
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn diags(main: &str) -> DiagBuf {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
    .diags
}

fn codes(main: &str) -> Vec<Code> {
    diags(main)
        .code
        .iter()
        .copied()
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

/// Applies the first fix of every diagnostic, last edit first.
fn apply_fixes(main: &str, diags: &DiagBuf) -> String {
    let mut edits = Vec::new();
    for fixes in &diags.fixes {
        if fixes.len == 0 {
            continue;
        }
        let range = diags.fix_edits[fixes.start as usize];
        for e in &diags.edits[range.start as usize..(range.start + range.len) as usize] {
            edits.push((
                e.span.lo as usize,
                e.span.hi as usize,
                diags.get_text(e.text).to_owned(),
            ));
        }
    }
    edits.sort_by_key(|e| std::cmp::Reverse((e.0, e.1)));
    let mut out = main.to_owned();
    for (lo, hi, text) in edits {
        out.replace_range(lo..hi, &text);
    }
    out
}

fn fixed_source(main: &str) -> String {
    let d = diags(main);
    assert!(d.code.contains(&Code::TypeMismatch), "{main}");
    apply_fixes(main, &d)
}

const PRELUDE: &str = "data Box:\n    n: i64\n\nfn take(x: i64) -> i64: x\n\n";

#[test]
fn fix_in_each_position_checks_clean() {
    let bodies = [
        (
            "let in a binding",
            "fn f(small: i16) -> i64:\n    let wide: i64 = small\n    wide\n",
            "i64(small)",
        ),
        (
            "an argument",
            "fn f(small: i16) -> i64:\n    take(small)\n",
            "take(i64(small))",
        ),
        (
            "a return",
            "fn f(small: i16) -> i64:\n    return small\n",
            "return i64(small)",
        ),
        (
            "a field",
            "fn f(small: i16) -> Box:\n    Box { n: small }\n",
            "Box { n: i64(small) }",
        ),
        (
            "a float",
            "fn f(x: f32) -> f64:\n    let wide: f64 = x\n    wide\n",
            "f64(x)",
        ),
        (
            "a sum",
            "fn f(small: i16, big: i64) -> i64:\n    big + small\n",
            "big + i64(small)",
        ),
    ];
    for (what, body, expect) in bodies {
        let src = format!("{PRELUDE}{body}\npub fn main() -> void:\n    ()\n");
        let fixed = fixed_source(&src);
        assert!(fixed.contains(expect), "{what}: {fixed}");
        assert_eq!(codes(&fixed), Vec::<Code>::new(), "{what}: {fixed}");
    }
}

#[test]
fn narrowing_and_mixed_families_offer_no_conversion_fix() {
    let cases = [
        (
            "fn f(big: i64) -> i16:\n    let n: i16 = big\n    n\n",
            Code::ImplicitNarrowing,
        ),
        (
            "fn f(small: u16) -> i64:\n    let n: i64 = small\n    n\n",
            Code::TypeMismatch,
        ),
        (
            "fn f(x: u32) -> usize:\n    let n: usize = x\n    n\n",
            Code::TypeMismatch,
        ),
        (
            "fn f(x: i32) -> f64:\n    let n: f64 = x\n    n\n",
            Code::TypeMismatch,
        ),
    ];
    for (body, code) in cases {
        let src = format!("{body}\npub fn main() -> void:\n    ()\n");
        let d = diags(&src);
        let i = d
            .code
            .iter()
            .position(|c| *c == code)
            .expect("the diagnostic");
        assert_eq!(d.fixes[i].len, 0, "{src}");
    }
}
