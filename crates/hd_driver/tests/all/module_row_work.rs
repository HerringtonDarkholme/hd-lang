//! #1530: omitted-row function values form a worklist, not a native
//! body-check stack (`req.row.omitted.cycle`). Count actual checks and
//! maximum active depth; elapsed time cannot prove either bound.

use std::fmt::Write as _;

use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The issue's `gen.py chain N`, copied here without an external script.
fn ring(n: usize) -> String {
    let mut out = String::new();
    for i in 0..n {
        let _ = writeln!(out, "fn h{i}(k: i32) -> void:");
        if i == n - 1 {
            let _ = writeln!(out, "    println(\"h{i} $k\")");
        }
        let next = (i + 1) % n;
        let _ = write!(
            out,
            "    if k > 0:\n        next := h{next}\n        next(k - 1)\n\n"
        );
    }
    out.push_str("pub fn main() -> void $ Console:\n    h0(3)\n");
    out
}

#[test]
fn row_signature_rings_have_linear_body_work_and_constant_active_depth() {
    for n in [100, 400, 1000, 2000] {
        let mut src = MemorySources::default();
        src.insert("main.hd", &ring(n));
        let store = crate::warm_store();
        let host = Host {
            render_tir: &[],
            sources: &src,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        let (out, counts) =
            hd_check::work::count_item_body_checks(|| build(&host, "app", &Goal::Analyze));
        assert!(out.diags.is_empty(), "ring {n}: {}", out.render());
        let counts: Vec<_> = counts
            .iter()
            .filter(|c| c.owner.starts_with("app/"))
            .collect();
        assert_eq!(
            counts.len(),
            n + 1,
            "ring {n}: one record per source function"
        );
        for c in &counts {
            assert!(
                c.checks <= 2,
                "ring {n}: {} checked {} times",
                c.owner,
                c.checks
            );
            assert!(
                c.max_active_depth <= 1,
                "ring {n}: {} at depth {}",
                c.owner,
                c.max_active_depth
            );
        }
        assert_eq!(
            counts.iter().map(|c| c.checks).sum::<usize>(),
            2 * n + 1,
            "ring {n}"
        );
    }
}

#[test]
fn a_default_sees_the_completed_row_of_a_function_value() {
    let source = "\
fn start() -> void:
    next := finish
    next()

fn finish() -> void:
    println(\"done\")

struct Holder:
    callback: fn() -> void = start
";
    for (source, expected) in [
        (source.to_owned(), vec![hd_diag::Code::TypeMismatch]),
        (
            source.replace(
                "callback: fn() -> void =",
                "callback: fn() -> void $ Console =",
            ),
            vec![],
        ),
    ] {
        let mut src = MemorySources::default();
        src.insert("main.hd", &source);
        let store = crate::warm_store();
        let host = Host {
            render_tir: &[],
            sources: &src,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        let out = build(&host, "app", &Goal::Analyze);
        assert_eq!(out.diags.code, expected, "{}", out.render());
    }
}

/// #1526: settling argument goals must not retry the caller body or grow
/// its native body-check stack with the number/nesting of value uses.
#[test]
fn delayed_function_value_arguments_have_bounded_body_work() {
    for (width, depth) in [(100, 1), (1000, 1), (1, 20)] {
        let mut source = String::from(
            "use std.function.Fn\n\nfn invoke[F, T, R, $ E](call: fn(&F, (T,)) -> R $ E, f: F, value: T) -> R $ E:\n    call(&f, (value,))\n\nfn logged(value: i32) -> i32 $ Console:\n    println(value)\n    value + 1\n\n",
        );
        for i in 0..width {
            let _ = writeln!(source, "fn worker{i}() -> i32:");
            let expression = format!(
                "{}+0{}",
                "invoke(Fn::call, logged, ".repeat(depth),
                ")".repeat(depth),
            );
            let _ = writeln!(source, "    {expression}\n");
        }
        source.push_str("pub fn main() -> void $ Console:\n    println(worker0())\n");
        let mut src = MemorySources::default();
        src.insert("main.hd", &source);
        let store = crate::warm_store();
        let host = Host {
            render_tir: &[],
            sources: &src,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        let (out, counts) =
            hd_check::work::count_item_body_checks(|| build(&host, "app", &Goal::Analyze));
        assert!(
            out.diags.is_empty(),
            "width {width}, depth {depth}: {}",
            out.render()
        );
        let counts: Vec<_> = counts
            .iter()
            .filter(|c| c.owner.starts_with("app/"))
            .collect();
        assert_eq!(counts.len(), width + 3);
        for c in counts {
            assert_eq!(c.checks, 1, "width {width}, depth {depth}: {}", c.owner);
            assert!(
                c.max_active_depth <= 1,
                "{}: {}",
                c.owner,
                c.max_active_depth
            );
        }
    }
}
