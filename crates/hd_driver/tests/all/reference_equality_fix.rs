//! `==` and `!=` between two references are `reference-equality`, with two
//! fix-its: `is`, which compares places, and `*`, which compares values
//! (spec/lang/13-values-and-references.md ref.identity.no-equality,
//! ref.identity.no-equality.fix). Each fix-it checks clean.

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

/// Applies fix `k` of every diagnostic that has one, last edit first.
fn apply_fix(main: &str, diags: &DiagBuf, k: u32) -> String {
    let mut edits = Vec::new();
    for fixes in &diags.fixes {
        if fixes.len <= k {
            continue;
        }
        let range = diags.fix_edits[(fixes.start + k) as usize];
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

#[test]
fn each_fix_compares_places_or_values_and_checks_clean() {
    let cases = [
        ("a == b", ["a is b", "*a == *b"]),
        ("a != b", ["!(a is b)", "*a != *b"]),
        (
            "a.inner == b.inner",
            ["a.inner is b.inner", "*a.inner == *b.inner"],
        ),
    ];
    for (expr, fixed) in cases {
        let src = format!(
            "struct Pair:\n    inner: *i32\n\nfn same(a: Pair, b: Pair) -> bool:\n    {expr}\n\npub fn main() -> void:\n    ()\n"
        )
        .replace(
            "fn same(a: Pair, b: Pair)",
            if expr.contains("inner") {
                "fn same(a: Pair, b: Pair)"
            } else {
                "fn same(a: &i32, b: &i32)"
            },
        );
        let d = diags(&src);
        assert!(d.code.contains(&Code::ReferenceEquality), "{expr}: {src}");
        for (k, want) in fixed.iter().enumerate() {
            let out = apply_fix(&src, &d, u32::try_from(k).expect("fix"));
            assert!(out.contains(want), "{expr}, fix {k}: {out}");
            assert_eq!(codes(&out), Vec::<Code>::new(), "{expr}, fix {k}: {out}");
        }
    }
}
