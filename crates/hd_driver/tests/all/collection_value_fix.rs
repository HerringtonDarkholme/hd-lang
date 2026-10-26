//! The errors of collection values (spec/lang/04-type-system.md,
//! "Collection Values") each carry a fix-it: a change to a list that is
//! no writable place makes its binding writable (`types.coll.writable.call`,
//! `types.coll.writable.store`). Each fixed program checks clean.

use hd_base::SliceSort as _;
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
    build(&host, "app", &Goal::Analyze).diags
}

/// The codes of `main`'s diagnostics, unused bindings aside.
fn codes(d: &DiagBuf) -> Vec<Code> {
    d.code
        .iter()
        .copied()
        .filter(|c| *c != Code::UnusedLocalBinding)
        .collect()
}

/// Applies the first fix of every diagnostic that has one, last edit first.
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
    edits.sort_stable_by_key(|e| std::cmp::Reverse((e.0, e.1)));
    let mut out = main.to_owned();
    for (lo, hi, text) in edits {
        out.replace_range(lo..hi, &text);
    }
    out
}

const PRELUDE: &str = "\
struct Point:
    x: i32
    y: i32

";

#[test]
fn each_fix_makes_the_program_check_clean() {
    let cases = [
        (
            "fn f() -> usize:\n    xs := [+1]\n    xs.push(2)\n    xs.len()\n",
            Code::NotWritable,
            "let mut xs = [+1]",
        ),
        (
            "fn f() -> i32:\n    let xs = [+1]\n    xs[0] = 2\n    xs[0]\n",
            Code::NotWritable,
            "let mut xs = [+1]",
        ),
        (
            "fn f(xs: List[i32]) -> usize:\n    xs.push(2)\n    xs.len()\n",
            Code::NotWritable,
            "fn f(mut xs: List[i32])",
        ),
    ];
    for (body, code, fixed) in cases {
        let src = format!("{PRELUDE}{body}");
        let d = diags(&src);
        assert_eq!(codes(&d), vec![code], "{src}");
        let out = apply_fixes(&src, &d);
        assert!(out.contains(fixed), "{body}: {out}");
        assert_eq!(codes(&diags(&out)), Vec::<Code>::new(), "{body}: {out}");
    }
}
