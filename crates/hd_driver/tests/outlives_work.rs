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

fn measured(text: &str) -> (u64, u64, u64) {
    let store = MemoryStore::default();
    let bootstrap = analyze(&store, "pub fn main() -> void: pass\n");
    assert!(!bootstrap.diags.has_errors(), "{}", bootstrap.render());
    let (((out, work), layouts), slots) = hd_check::work::count_slot_counts(|| {
        hd_check::work::count_layout_steps(|| {
            hd_check::work::count_scan_work(|| analyze(&store, text))
        })
    });
    assert!(!out.diags.has_errors(), "{}", out.render());
    (work, layouts, slots)
}

fn work(width: usize, depth: usize, constants: bool) -> (u64, u64, u64) {
    measured(&source(width, depth, constants))
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

fn head_source(width: usize, depth: usize) -> String {
    let mut text = String::from(
        "struct Wrapped[T]:\n    value: T\nstruct Pair[A, B]:\n    left: A\n    right: B\ntrait Seed0:\n    pass\nimpl[T] Seed0 for Pair[T, &'static i32]:\n    pass\n",
    );
    for d in 1..=depth {
        let _ = write!(
            text,
            "trait Seed{d}:\n    pass\nimpl[T < Seed{}] Seed{d} for Wrapped[T]:\n    pass\n",
            d - 1
        );
    }
    for i in 0..width {
        let _ = write!(
            text,
            "trait Branch{i}:\n    pass\nimpl[T < Seed{depth}] Branch{i} for Wrapped[T]:\n    pass\n"
        );
    }
    text.push_str("fn require[T < ");
    for i in 0..width {
        if i != 0 {
            text.push_str(" & ");
        }
        let _ = write!(text, "Branch{i}");
    }
    text.push_str("](value: T) -> void: pass\nfn accepted['a](value: ");
    for _ in 0..=depth {
        text.push_str("Wrapped[");
    }
    text.push_str("Pair[(");
    for i in 0..width {
        if i != 0 {
            text.push_str(", ");
        }
        text.push_str("&'a i32");
    }
    text.push_str("), &'static i32]");
    for _ in 0..=depth {
        text.push(']');
    }
    text.push_str(
        ") -> void:\n    require(value)\n    require(value)\npub fn main() -> void: pass\n",
    );
    text
}

#[test]
fn selective_impl_heads_share_projection_and_warm_answers() {
    let small = measured(&head_source(100, 1));
    let wide = measured(&head_source(1000, 1));
    let deep = measured(&head_source(1000, 20));
    eprintln!("selective head proof work: small={small:?}, wide={wide:?}, deep={deep:?}");
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

fn argument_head_source(width: usize, depth: usize) -> String {
    let mut text = String::from(
        "struct Wrapped[T]:\n    value: T\nstruct Holder:\n    value: i32\ntype Deep[T] = ",
    );
    for _ in 0..depth {
        text.push_str("Wrapped[");
    }
    text.push('T');
    for _ in 0..depth {
        text.push(']');
    }
    text.push('\n');
    for i in 0..width {
        let _ = write!(
            text,
            "trait Branch{i}[A]:\n    pass\nimpl Branch{i}[Deep[&'static i32]] for Holder:\n    pass\n"
        );
    }
    text.push_str("fn require['a, T < ");
    for i in 0..width {
        if i != 0 {
            text.push_str(" & ");
        }
        let _ = write!(text, "Branch{i}[Deep[&'a i32]]");
    }
    text.push_str("](value: T, marker: Deep[&'a i32]) -> void: pass\nfn accepted(value: Holder, marker: Deep[&'static i32]) -> void:\n    require(value, marker)\n    require(value, marker)\npub fn main() -> void: pass\n");
    text
}

#[test]
fn written_argument_heads_reuse_correspondence_across_bounds() {
    let small = measured(&argument_head_source(100, 1));
    let wide = measured(&argument_head_source(1000, 1));
    let deep_small = measured(&argument_head_source(100, 20));
    let deep_wide = measured(&argument_head_source(1000, 20));
    eprintln!(
        "argument head proof work: small={small:?}, wide={wide:?}, deep_small={deep_small:?}, deep_wide={deep_wide:?}"
    );
    for (small, wide, deep_small, deep_wide) in [
        (small.0, wide.0, deep_small.0, deep_wide.0),
        (small.1, wide.1, deep_small.1, deep_wide.1),
        (small.2, wide.2, deep_small.2, deep_wide.2),
    ] {
        assert!(small > 0);
        assert!(wide <= 12 * small, "{small} -> {wide}");
        assert!(deep_wide <= 12 * deep_small, "{deep_small} -> {deep_wide}");
        // Widening must not multiply the extra work introduced by depth.
        let narrow_delta = deep_small.saturating_sub(small);
        let wide_delta = deep_wide.saturating_sub(wide);
        assert!(
            wide_delta <= 2 * narrow_delta + 1,
            "depth delta {narrow_delta} -> {wide_delta}"
        );
    }
}

fn conversion_source(width: usize, depth: usize) -> String {
    let mut text = String::from(
        "struct Wrapped[T]:\n    value: T\ntrait Seed0:\n    pass\nimpl[T < 'static] Seed0 for Wrapped[T]:\n    pass\n",
    );
    for d in 1..=depth {
        let _ = write!(
            text,
            "trait Seed{d}:\n    pass\nimpl[T < Seed{}] Seed{d} for Wrapped[T]:\n    pass\n",
            d - 1
        );
    }
    let _ = write!(
        text,
        "trait Saved:\n    fn number(&self) -> i32\nimpl[T < Seed{depth}] Saved for Wrapped[T]:\n    fn number(&self) -> i32: +7\nfn accepted(value: &"
    );
    for _ in 0..depth + 2 {
        text.push_str("Wrapped[");
    }
    text.push_str("&'static i32");
    for _ in 0..depth + 2 {
        text.push(']');
    }
    text.push_str(") -> void:\n");
    for i in 0..width {
        let _ = writeln!(text, "    let view{i}: &dyn Saved = value");
    }
    text.push_str("    pass\npub fn main() -> void: pass\n");
    text
}

#[test]
fn repeated_conversions_share_proof_projection() {
    let small = measured(&conversion_source(100, 1));
    let wide = measured(&conversion_source(1000, 1));
    let deep_small = measured(&conversion_source(100, 20));
    let deep_wide = measured(&conversion_source(1000, 20));
    eprintln!(
        "conversion work: small={small:?}, wide={wide:?}, deep_small={deep_small:?}, deep_wide={deep_wide:?}"
    );
    for (small, wide, deep_small, deep_wide) in [
        (small.0, wide.0, deep_small.0, deep_wide.0),
        (small.1, wide.1, deep_small.1, deep_wide.1),
        (small.2, wide.2, deep_small.2, deep_wide.2),
    ] {
        assert!(small > 0);
        assert!(wide <= 12 * small, "{small} -> {wide}");
        assert!(deep_wide <= 12 * deep_small, "{deep_small} -> {deep_wide}");
        let narrow_delta = deep_small.saturating_sub(small);
        let wide_delta = deep_wide.saturating_sub(wide);
        assert!(
            wide_delta <= 2 * narrow_delta + 1,
            "depth delta {narrow_delta} -> {wide_delta}"
        );
    }
}
