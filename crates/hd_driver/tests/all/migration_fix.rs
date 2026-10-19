//! The migration edits of `hd fix --object-model`
//! (spec/cli/command-line.md cli.fix.object-model.coerce,
//! cli.fix.object-model.coerce.cancel, cli.fix.object-model.coerce.closure,
//! cli.fix.object-model.equality): where a value meets a reference to its
//! type, or two references meet at `==`, the diagnostic carries a fix-it of
//! safety `Migration` that the reports leave out. Each edit checks clean.

use hd_diag::{Code, DiagBuf, FixSafety};
use hd_driver::diagnostics::{from_output, from_output_migration};
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn output(main: &str) -> Output {
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
}

/// The codes of the errors a build of `main` reports.
fn errors(main: &str) -> Vec<Code> {
    let d = output(main).diags;
    (0..d.len())
        .filter(|&i| d.severity[i] == hd_diag::Severity::Error && d.code[i] != Code::Unsupported)
        .map(|i| d.code[i])
        .collect()
}

/// Applies the migration edit of every diagnostic that has one, last edit
/// first.
fn migrate(main: &str, diags: &DiagBuf) -> String {
    let mut edits = Vec::new();
    for fixes in &diags.fixes {
        let Some(f) = (fixes.start..fixes.start + fixes.len)
            .find(|&f| diags.fix_safety[f as usize] == FixSafety::Migration)
        else {
            continue;
        };
        for e in &diags.edits[diags.fix_edits[f as usize].range()] {
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

const PRELUDE: &str = "@derive(Eq)\nstruct Point:\n    x: i32\n\nfn fill(xs: &mut List[i32]) -> void:\n    xs.push(1)\n\nfn len(xs: &List[i32]) -> usize:\n    xs.len()\n\nfn copy(p: Point) -> i32:\n    p.x\n\n";

#[test]
fn each_migration_edit_checks_clean() {
    let cases = [
        ("    let mut xs = [+1]\n    fill(xs)\n", "fill(&mut xs)"),
        ("    fill([+1, 2])\n", "fill(&mut [+1, 2])"),
        ("    xs := [+1]\n    n := len(xs)\n", "len(&xs)"),
        ("    p := &Point { x: 1 }\n    n := copy(p)\n", "copy(*p)"),
        (
            "    n := copy(&mut Point { x: 1 })\n",
            "copy(Point { x: 1 })",
        ),
        (
            "    a := &Point { x: 1 }\n    b := &Point { x: 1 }\n    same := a == b\n",
            "*a == *b",
        ),
        (
            "    a := &Point { x: 1 }\n    same := a != &mut Point { x: 2 }\n",
            "*a != Point { x: 2 }",
        ),
        (
            "    ps := [&mut Point { x: 1 }]\n    n := ps.filter(fn(p: &Point) -> bool: p.x > 0).len()\n",
            "fn(p: &mut Point)",
        ),
    ];
    for (body, want) in cases {
        let src = format!("{PRELUDE}pub fn main() -> void:\n{body}");
        let out = output(&src);
        assert!(!out.diags.is_empty(), "{body}: no diagnostic");
        let fixed = migrate(&src, &out.diags);
        assert!(fixed.contains(want), "{body}: {fixed}");
        assert_eq!(errors(&fixed), Vec::<Code>::new(), "{body}: {fixed}");
    }
}

#[test]
fn reports_leave_migration_edits_out() {
    let src = format!("{PRELUDE}pub fn main() -> void:\n    fill([+1])\n");
    let out = output(&src);
    let shown = from_output(&out, None);
    assert!(
        shown.iter().all(|d| d
            .fixes
            .iter()
            .all(|f| !f.migration && f.message != "write `&mut`")),
        "{shown:?}"
    );
    let all = from_output_migration(&out, None);
    assert!(
        all.iter().any(|d| d.fixes.iter().any(|f| f.migration)),
        "{all:?}"
    );
}
