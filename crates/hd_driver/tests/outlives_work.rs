//! Count proof projection work through the real solver and region checker.
use std::fmt::Write as _;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn source(width: usize, depth: usize) -> String {
    let mut text = String::from(
        "struct Wrapped[T]:\n    value: T\ntrait Seed0:\n    pass\nimpl[T < 'static] Seed0 for Wrapped[T]:\n    pass\n",
    );
    for d in 1..=depth {
        let _ = write!(
            text,
            "trait Seed{d}:\n    pass\nimpl[T < Seed{}] Seed{d} for Wrapped[T]:\n    pass\n",
            d - 1,
        );
    }
    for i in 0..width {
        let _ = write!(
            text,
            "trait Branch{i}:\n    pass\nimpl[T < Seed{depth}] Branch{i} for Wrapped[T]:\n    pass\n",
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
        text.push(']');
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

fn work(width: usize, depth: usize) -> u64 {
    let store = MemoryStore::default();
    let bootstrap = analyze(&store, "pub fn main() -> void: pass\n");
    assert!(!bootstrap.diags.has_errors(), "{}", bootstrap.render());
    let text = source(width, depth);
    let (out, work) = hd_check::work::count_scan_work(|| analyze(&store, &text));
    assert!(!out.diags.has_errors(), "{}", out.render());
    work
}

#[test]
fn shared_outlives_proofs_stay_linear_at_width_1000_and_depth_20() {
    let small = work(100, 1);
    let wide = work(1000, 1);
    let deep = work(1000, 20);
    eprintln!("outlives proof work: small={small}, wide={wide}, deep={deep}");
    assert!(small > 0);
    assert!(wide <= 12 * small, "{small} -> {wide}");
    assert!(deep <= 2 * wide, "{wide} -> {deep}");
}

fn header_work(width: usize, depth: usize) -> (u64, u64) {
    let mut ty = "&'static i32".to_owned();
    for _ in 0..depth {
        ty = format!("Wrapped[{ty}]");
    }
    let mut text = String::from("struct Wrapped[T]:\n    value: T\n");
    for i in 0..width {
        let _ = writeln!(
            text,
            "fn inspect{i}[T < 'static = {ty}](value: T) -> void: pass"
        );
    }
    text.push_str("pub fn main() -> void: pass\n");
    measure_header(&text)
}

fn measure_header(text: &str) -> (u64, u64) {
    let store = MemoryStore::default();
    let bootstrap = analyze(&store, "pub fn main() -> void: pass\n");
    assert!(!bootstrap.diags.has_errors(), "{}", bootstrap.render());
    let ((out, scanned), laid) = hd_check::work::count_layout_steps(|| {
        hd_check::work::count_scan_work(|| analyze(&store, text))
    });
    assert!(!out.diags.has_errors(), "{}", out.render());
    (scanned, laid)
}

#[test]
fn default_outlives_checks_share_layout_work_across_headers() {
    let small = header_work(100, 1);
    let wide = header_work(1000, 1);
    let deep = header_work(1000, 20);
    eprintln!("header outlives work (scan, layout): small={small:?}, wide={wide:?}, deep={deep:?}");
    for (small, wide, deep) in [(small.0, wide.0, deep.0), (small.1, wide.1, deep.1)] {
        assert!(small > 0);
        assert!(wide <= 12 * small, "{small} -> {wide}");
        assert!(deep <= 2 * wide, "{wide} -> {deep}");
    }
}

fn supertrait_work(width: usize, depth: usize) -> (u64, u64) {
    let mut ty = "&'static i32".to_owned();
    for _ in 0..depth {
        ty = format!("Wrapped[{ty}]");
    }
    let mut text = String::from(
        "struct Wrapped[T]:\n    value: T\ntrait Saved[A]:\n    pass\ntrait Child[A] < Saved[A]:\n    pass\n",
    );
    for i in 0..width {
        let _ = write!(
            text,
            "struct User{i}:\n    value: i32\nimpl[U < 'static] Saved[U] for User{i}:\n    pass\nimpl Child[{ty}] for User{i}:\n    pass\n",
        );
    }
    text.push_str("pub fn main() -> void: pass\n");
    measure_header(&text)
}

#[test]
fn inherited_outlives_checks_share_instantiation_work_across_headers() {
    let small = supertrait_work(100, 1);
    let wide = supertrait_work(1000, 1);
    let deep = supertrait_work(1000, 20);
    eprintln!(
        "supertrait outlives work (scan, layout): small={small:?}, wide={wide:?}, deep={deep:?}"
    );
    for (small, wide, deep) in [(small.0, wide.0, deep.0), (small.1, wide.1, deep.1)] {
        assert!(small > 0);
        assert!(wide <= 12 * small, "{small} -> {wide}");
        assert!(deep <= 2 * wide, "{wide} -> {deep}");
    }
}

fn associated_work(width: usize, depth: usize) -> (u64, u64) {
    let mut ty = "&'static i32".to_owned();
    for _ in 0..depth {
        ty = format!("Wrapped[{ty}]");
    }
    let mut text = String::from(
        "struct Wrapped[T]:\n    value: T\ntrait Saved:\n    pass\nimpl[T < 'static] Saved for Wrapped[T]:\n    pass\ntrait Shelf:\n    type Item < Saved\n",
    );
    for i in 0..width {
        let _ = write!(
            text,
            "struct User{i}:\n    value: i32\nimpl Shelf for User{i}:\n    type Item = {ty}\n",
        );
    }
    text.push_str("pub fn main() -> void: pass\n");
    measure_header(&text)
}

#[test]
fn associated_outlives_checks_share_assigned_type_work_across_headers() {
    let small = associated_work(100, 1);
    let wide = associated_work(1000, 1);
    let deep = associated_work(1000, 20);
    eprintln!(
        "associated outlives work (scan, layout): small={small:?}, wide={wide:?}, deep={deep:?}"
    );
    for (small, wide, deep) in [(small.0, wide.0, deep.0), (small.1, wide.1, deep.1)] {
        assert!(small > 0);
        assert!(wide <= 12 * small, "{small} -> {wide}");
        assert!(deep <= 2 * wide, "{wide} -> {deep}");
    }
}

fn delegation_work(width: usize, depth: usize) -> (u64, u64) {
    let mut ty = "&'static i32".to_owned();
    for _ in 0..depth {
        ty = format!("Wrapped[{ty}]");
    }
    let mut text = String::from(
        "struct Wrapped[T]:\n    value: T\ntrait Saved:\n    fn touch(self) -> void\nimpl[T < 'static] Saved for Wrapped[T]:\n    fn touch(self) -> void:\n        pass\n",
    );
    let _ = writeln!(text, "type Deep = {ty}");
    for i in 0..width {
        let _ = write!(
            text,
            "struct Holder{i}[T]:\n    Wrapped[T]\nimpl Saved for Holder{i}[Deep] by Wrapped\n",
        );
    }
    text.push_str("pub fn main() -> void: pass\n");
    measure_header(&text)
}

#[test]
fn delegated_outlives_checks_bound_generated_body_and_header_work() {
    let small = delegation_work(100, 1);
    let wide = delegation_work(1000, 1);
    let deep = delegation_work(1000, 20);
    eprintln!(
        "delegation outlives work (scan, layout): small={small:?}, wide={wide:?}, deep={deep:?}"
    );
    for (small, wide, deep) in [(small.0, wide.0, deep.0), (small.1, wide.1, deep.1)] {
        assert!(small > 0);
        assert!(wide <= 12 * small, "{small} -> {wide}");
        assert!(deep <= 2 * wide, "{wide} -> {deep}");
    }
}
