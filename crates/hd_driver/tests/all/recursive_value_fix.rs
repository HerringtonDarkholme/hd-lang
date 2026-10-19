//! A struct or enum that contains itself by value is `recursive-value-type`,
//! with a fix-it that writes `&` before each field that holds it
//! (spec/lang/08-data-and-enums.md data.struct.no-recursion.fix,
//! data.enum.no-recursion.fix). Each fixed program checks clean.

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
    edits.sort_by_key(|e| std::cmp::Reverse((e.0, e.1)));
    let mut out = main.to_owned();
    for (lo, hi, text) in edits {
        out.replace_range(lo..hi, &text);
    }
    out
}

const MAIN: &str = "\npub fn main() -> void:\n    ()\n";

#[test]
fn each_fix_writes_a_reference_and_checks_clean() {
    let cases = [
        (
            "enum Tree:\n    Leaf(value: i64)\n    Node(left: Tree, right: Tree)\n",
            "    Node(left: &Tree, right: &Tree)\n",
        ),
        (
            "enum Chain:\n    End\n    Link(next: Chain?)\n",
            "    Link(next: &Chain?)\n",
        ),
        (
            "enum Pairing:\n    Single(value: i32)\n    Both(pair: (i32, Pairing))\n",
            "    Both(pair: &(i32, Pairing))\n",
        ),
        (
            "enum Branch:\n    Leaf\n    Fork(forest: Forest)\n\nstruct Forest:\n    branch: Branch\n",
            "    Fork(forest: &Forest)\n",
        ),
        (
            "struct Chain:\n    value: i32\n    next: Chain?\n",
            "    next: &Chain?\n",
        ),
        (
            "enum Expr(parent: Expr?):\n    Num(n: i64)\n",
            "enum Expr(parent: &Expr?):\n",
        ),
    ];
    for (decl, fixed) in cases {
        let src = format!("{decl}{MAIN}");
        let d = diags(&src);
        let found: Vec<Code> = d
            .code
            .iter()
            .copied()
            .filter(|c| *c != Code::Unsupported)
            .collect();
        assert_eq!(found, vec![Code::RecursiveValueType], "{src}");
        let out = apply_fixes(&src, &d);
        assert!(out.contains(fixed), "{decl}: {out}");
        assert_eq!(codes(&out), Vec::<Code>::new(), "{decl}: {out}");
    }
}

#[test]
fn recursion_through_a_reference_or_a_list_is_accepted() {
    for decl in [
        "enum Expr:\n    Num(n: i64)\n    Add(left: &Expr, right: &Expr)\n",
        "enum Tree:\n    Leaf\n    Node(children: List[Tree])\n",
        "struct Cell:\n    value: i64\n    next: &Cell?\n",
        "enum Link:\n    End\n    More(next: &mut Link)\n",
    ] {
        let src = format!("{decl}{MAIN}");
        assert_eq!(codes(&src), Vec::<Code>::new(), "{src}");
    }
}
