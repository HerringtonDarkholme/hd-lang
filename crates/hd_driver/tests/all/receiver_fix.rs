//! A `&mut self` call on a binding that is not writable, and a field write
//! through `self` in a struct's `self` method, carry a fix-it that makes the
//! binding writable (spec/lang/07-functions.md fn.method.auto-address.fix,
//! fn.method.self-struct-write). So does a `&mut self` call through a
//! read-only trait value reference (spec/lang/09-traits.md
//! trait.dyn.ref.member.mut). Each fix-it checks clean.

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

const POINT: &str = "struct Point:\n    x: i32\n    y: i32\n\nimpl Point:\n    fn shift(&mut self, dx: i32) -> void:\n        self.x += dx\n\n";

#[test]
fn each_fix_makes_the_receiver_writable_and_checks_clean() {
    let cases = [
        (
            "fn demo() -> i32:\n    p := Point { x: 0, y: 0 }\n    p.shift(1)\n    p.x\n",
            Code::NotWritable,
            "let mut p = Point",
        ),
        (
            "fn demo() -> i32:\n    let p = Point { x: 0, y: 0 }\n    p.shift(1)\n    p.x\n",
            Code::NotWritable,
            "let mut p = Point",
        ),
        (
            "fn demo(p: Point) -> i32:\n    p.shift(1)\n    p.x\n",
            Code::NotWritable,
            "fn demo(mut p: Point)",
        ),
        (
            "impl Point:\n    fn nudge(self) -> void:\n        self.shift(1)\n",
            Code::NotWritable,
            "fn nudge(&mut self)",
        ),
        (
            "impl Point:\n    fn reset(self) -> void:\n        self.x = 0\n",
            Code::NonReassignableParameterBinding,
            "fn reset(&mut self)",
        ),
    ];
    for (body, code, want) in cases {
        let src = format!("{POINT}{body}\npub fn main() -> void:\n    ()\n");
        let d = diags(&src);
        assert!(d.code.contains(&code), "{body}: {:?}", d.code);
        let out = apply_fix(&src, &d);
        assert!(out.contains(want), "{body}: {out}");
        assert_eq!(codes(&out), Vec::<Code>::new(), "{body}: {out}");
    }
}

const COUNTER: &str = "trait Counter:\n    fn bump(&mut self) -> void\n\nstruct Tally:\n    n: i32\n\nimpl Counter for Tally:\n    fn bump(&mut self) -> void:\n        self.n += 1\n\n";

#[test]
fn a_mut_method_through_a_shared_trait_value_reference_fixes_its_type() {
    let cases = [
        (
            "fn poke(counter: &dyn Counter) -> void:\n    counter.bump()\n",
            "fn poke(counter: &mut dyn Counter)",
        ),
        (
            "fn demo() -> void:\n    let mut tally = Tally { n: 0 }\n    let counter: &dyn Counter = &mut tally\n    counter.bump()\n",
            "let counter: &mut dyn Counter = &mut tally",
        ),
    ];
    for (body, want) in cases {
        let src = format!("{COUNTER}{body}\npub fn main() -> void:\n    ()\n");
        let d = diags(&src);
        assert!(d.code.contains(&Code::NotWritable), "{body}: {:?}", d.code);
        let out = apply_fix(&src, &d);
        assert!(out.contains(want), "{body}: {out}");
        assert_eq!(codes(&out), Vec::<Code>::new(), "{body}: {out}");
    }
}
