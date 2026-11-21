//! Count proof projection work through the real solver and region checker.
use std::fmt::Write as _;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn source(width: usize, depth: usize, constants: bool) -> String {
    let extra = if constants { ", N < const usize" } else { "" };
    let target = if constants {
        "Wrapped[T, N]"
    } else {
        "Wrapped[T]"
    };
    let mut text = format!(
        "struct Wrapped[T{extra}]:\n    value: T\ntrait Seed0:\n    pass\nimpl[T < 'static{extra}] Seed0 for {target}:\n    pass\n"
    );
    for d in 1..=depth {
        let _ = write!(
            text,
            "trait Seed{d}:\n    pass\nimpl[T < Seed{}{extra}] Seed{d} for {target}:\n    pass\n",
            d - 1,
        );
    }
    for i in 0..width {
        let _ = write!(
            text,
            "trait Branch{i}:\n    pass\nimpl[T < Seed{depth}{extra}] Branch{i} for {target}:\n    pass\n",
        );
    }
    text.push_str("fn require[T < ");
    for i in 0..width {
        if i != 0 {
            text.push_str(" & ");
        }
        let _ = write!(text, "Branch{i}");
    }
    text.push_str("](value: T) -> void: pass\nfn accepted(value: ");
    for _ in 0..depth + 2 {
        text.push_str("Wrapped[");
    }
    text.push('(');
    for i in 0..width {
        if i != 0 {
            text.push_str(", ");
        }
        text.push_str("&'static i32");
    }
    text.push(')');
    for _ in 0..depth + 2 {
        text.push_str(if constants { ", 0]" } else { "]" });
    }
    text.push_str(") -> void: require(value)\npub fn main() -> void: pass\n");
    text
}

fn analyze(store: &MemoryStore, text: &str) -> hd_driver::Output {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", text);
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

fn work(width: usize, depth: usize, constants: bool) -> (u64, u64, u64) {
    let store = MemoryStore::default();
    let bootstrap = analyze(&store, "pub fn main() -> void: pass\n");
    assert!(!bootstrap.diags.has_errors(), "{}", bootstrap.render());
    let text = source(width, depth, constants);
    let (((out, work), layouts), slots) = hd_check::work::count_slot_counts(|| {
        hd_check::work::count_layout_steps(|| {
            hd_check::work::count_scan_work(|| analyze(&store, &text))
        })
    });
    assert!(!out.diags.has_errors(), "{}", out.render());
    (work, layouts, slots)
}

#[test]
fn shared_outlives_proofs_stay_linear_at_width_1000_and_depth_20() {
    for constants in [false, true] {
        let small = work(100, 1, constants);
        let wide = work(1000, 1, constants);
        let deep = work(1000, 20, constants);
        eprintln!(
            "outlives proof work (constants={constants}): small={small:?}, wide={wide:?}, deep={deep:?}"
        );
        for (small, wide, deep) in [
            (small.0, wide.0, deep.0),
            (small.1, wide.1, deep.1),
            (small.2, wide.2, deep.2),
        ] {
            assert!(small > 0);
            assert!(wide <= 12 * small, "{small} -> {wide}");
            assert!(deep <= 2 * wide, "{wide} -> {deep}");
        }
    }
}
