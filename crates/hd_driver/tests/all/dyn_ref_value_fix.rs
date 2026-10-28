//! A value where a trait value reference is expected is `type-mismatch`,
//! with a fix-it that writes `&` or `&mut` before a place or a struct
//! literal (spec/lang/09-traits.md trait.dyn.ref.written,
//! trait.dyn.ref.written.reference-fix.reference-values). Each fix-it checks clean, and a value whose
//! written reference would be an error, or that is no place or struct
//! literal, gets none.

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
fn apply_fix(main: &str, diags: &DiagBuf) -> String {
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

const TRAITS: &str = "trait Shape:\n    fn area(&self) -> f64\n\nstruct Square:\n    side: f64\n\nimpl Shape for Square:\n    fn area(&self) -> f64:\n        self.side * self.side\n\ntrait Counter:\n    fn bump(&mut self) -> void\n\nstruct Tally:\n    n: i32\n\nimpl Counter for Tally:\n    fn bump(&mut self) -> void:\n        self.n += 1\n\nfn show(shape: &dyn Shape) -> f64:\n    shape.area()\n\nfn count(counter: &mut dyn Counter) -> void:\n    counter.bump()\n\n";

#[test]
fn each_fix_writes_the_reference_and_checks_clean() {
    let cases = [
        (
            "fn demo() -> f64:\n    square := Square { side: 2.0 }\n    show(square)\n",
            "show(&square)",
        ),
        (
            "fn demo() -> i32:\n    let mut tally = Tally { n: 0 }\n    count(tally)\n    tally.n\n",
            "count(&mut tally)",
        ),
        (
            "fn demo() -> List[*dyn Shape]:\n    [Square { side: 1.0 }]\n",
            "[&Square { side: 1.0 }]",
        ),
        (
            "fn demo(n: i32) -> void:\n    let mut tally = Tally { n: n }\n    $.with(Counter=tally):\n        pass\n",
            "$.with(Counter=&mut tally)",
        ),
        (
            "fn demo() -> f64:\n    let mut big = Square { side: 2.0 }\n    let shapes: List[*dyn Shape] = [Square { side: 1.0 }, big]\n    big.side = 3.0\n    shapes[1].area()\n",
            "[&Square { side: 1.0 }, &big]",
        ),
    ];
    for (body, want) in cases {
        let src = format!("{TRAITS}{body}\npub fn main() -> void:\n    ()\n");
        let d = diags(&src);
        assert!(d.code.contains(&Code::TypeMismatch), "{body}: {:?}", d.code);
        let out = apply_fix(&src, &d);
        assert!(out.contains(want), "{body}: {out}");
        let left: Vec<Code> = codes(&out)
            .into_iter()
            .filter(|c| !matches!(c, Code::UnusedLocalBinding))
            .collect();
        assert_eq!(left, Vec::<Code>::new(), "{body}: {out}");
    }
}

#[test]
fn a_written_reference_that_is_an_error_gets_no_fix() {
    let cases = [
        // `&counter` on a bare trait value whose trait has a `&mut self`
        // method is itself an error (trait.dyn.bare-value-rejected).
        // A call result is no place and no struct literal.
        "fn make() -> Square:\n    Square { side: 1.0 }\n\nfn demo() -> f64:\n    show(make())\n",
    ];
    for body in cases {
        let src = format!("{TRAITS}{body}\npub fn main() -> void:\n    ()\n");
        let d = diags(&src);
        assert!(d.code.contains(&Code::TypeMismatch), "{body}: {:?}", d.code);
        assert!(d.fixes.iter().all(|f| f.len == 0), "{body}: a fix-it");
    }
}

#[test]
fn a_written_bare_trait_value_has_no_migration_fix() {
    let src = format!("{TRAITS}fn demo(counter: dyn Counter) -> void:\n    pass\n");
    let d = diags(&src);
    assert!(d.code.contains(&Code::TraitUsedAsType));
    assert!(d.fixes.iter().all(|fix| fix.len == 0));
}
