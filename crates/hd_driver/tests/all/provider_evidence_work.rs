//! Checked supplier evidence is indexed once per whole key and run, not
//! per call, lexical nesting level, closure or named function body.
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use std::fmt::Write as _;

fn source(width: usize, depth: usize, graph: usize, bodies: bool) -> String {
    let mut out = String::from("trait Base:\n    fn base(&self) -> i32\n");
    for i in 0..graph {
        writeln!(out, "trait Side{i}:\n    pass").unwrap();
    }
    out.push_str("trait Named < Base");
    for i in 0..graph {
        write!(out, " & Side{i}").unwrap();
    }
    out.push_str(":\n    fn name(&self) -> i32\nstruct Source:\n    n: i32\nimpl Base for Source:\n    fn base(&self) -> i32: self.n\n");
    for i in 0..graph {
        writeln!(out, "impl Side{i} for Source:\n    pass").unwrap();
    }
    out.push_str("impl Named for Source:\n    fn name(&self) -> i32: self.n\nfn needs() -> i32 $ Base:\n    $.use(Base).base()\n");
    if bodies {
        for i in 0..width {
            writeln!(out, "fn worker_{i}() -> i32 $ Named:\n    needs()").unwrap();
        }
    } else {
        out.push_str("fn run() -> i32 $ Named:\n    let total = +0\n");
        let mut indent = "    ".to_owned();
        for _ in 0..depth {
            writeln!(out, "{indent}$.with(Named=box(Source {{ n: 1 }})):").unwrap();
            indent.push_str("    ");
        }
        for _ in 0..width {
            writeln!(out, "{indent}total += needs()").unwrap();
        }
        out.push_str("    total\n");
    }
    out.push_str("pub fn main() -> void $ Console:\n    $.with(Named=box(Source { n: 1 })):\n");
    if bodies {
        out.push_str("        let total = +0\n");
        for i in 0..width {
            writeln!(out, "        total += worker_{i}()").unwrap();
        }
        out.push_str("        println(total)\n");
    } else {
        out.push_str("        println(run())\n");
    }
    out
}

fn work(width: usize, depth: usize, graph: usize, bodies: bool, emit: bool) -> u64 {
    measure(&source(width, depth, graph, bodies), emit)
}

fn measure(text: &str, emit: bool) -> u64 {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", text);
    let store = crate::warm_store();
    // Keep lazy std checking outside the measurement at every size. Measuring
    // a cold first sample would make the tenfold-width assertion vacuous.
    let mut bootstrap_sources = MemorySources::default();
    bootstrap_sources.insert(
        "main.hd",
        "pub fn main() -> void $ Console:\n    println(+0)\n",
    );
    let bootstrap_host = Host {
        render_tir: &[],
        sources: &bootstrap_sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let bootstrap = build(
        &bootstrap_host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!bootstrap.diags.has_errors(), "{}", bootstrap.render());
    assert!(bootstrap.wasm.is_some());
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let goal = if emit {
        Goal::Program {
            entry: "main".into(),
        }
    } else {
        Goal::Analyze
    };
    let (out, work) = hd_check::work::count_scan_work(|| build(&host, "app", &goal));
    assert!(!out.diags.has_errors(), "{}", out.render());
    if emit {
        assert!(out.wasm.is_some(), "no Wasm: {}", out.render());
    }
    work
}

#[test]
fn projected_calls_stay_linear_at_width_1000_and_depth_20() {
    let (small, wide, deep) = (
        work(100, 2, 2, false, true),
        work(1000, 2, 2, false, true),
        work(1000, 20, 2, false, true),
    );
    eprintln!("provider evidence: small={small}, wide={wide}, deep={deep}");
    assert!(small > 0);
    assert!(wide <= 12 * small, "{small} -> {wide}");
    assert!(deep <= 2 * wide, "{wide} -> {deep}");
}

#[test]
fn bodies_share_the_whole_supplier_graph_index() {
    let (small, large) = (
        work(100, 0, 100, true, false),
        work(1000, 0, 1000, true, false),
    );
    eprintln!("shared supplier graph: {small} -> {large}");
    assert!(small > 0);
    assert!(large <= 12 * small, "{small} -> {large}");
}

fn competing_scopes(n: usize) -> String {
    let mut source = String::from("trait Base:\n    fn base(&self) -> i32\n");
    for i in 0..n {
        writeln!(source, "trait Supplier_{i} < Base:\n    pass").unwrap();
    }
    source.push_str("trait Other:\n    fn other(&self) -> i32\ntrait Unrelated < Other:\n    pass\nstruct Noise: pass\nimpl Other for Noise:\n    fn other(&self) -> i32: +0\nimpl Unrelated for Noise:\n    pass\nfn needs() -> i32 $ Base:\n    $.use(Base).base()\nfn run() -> i32 $ ");
    for i in 0..n {
        if i > 0 {
            source.push_str(" + ");
        }
        write!(source, "Supplier_{i}").unwrap();
    }
    source.push_str(":\n    let total = +0\n");
    for _ in 0..n {
        source.push_str("    $.with(Unrelated=box(Noise {})):\n        total += needs()\n");
    }
    source.push_str("    total\npub fn main() -> void:\n    pass\n");
    source
}

#[test]
fn unrelated_scopes_do_not_rescan_competing_suppliers() {
    let (small, large) = (
        measure(&competing_scopes(100), false),
        measure(&competing_scopes(1000), false),
    );
    eprintln!("competing suppliers/scopes: {small} -> {large}");
    assert!(small > 0 && large <= 12 * small, "{small} -> {large}");
}

fn constrained_competing_scopes(n: usize) -> String {
    let mut source = String::from(
        "trait Base:\n    type Item\n    fn base(&self) -> i32\ntrait Other:\n    fn other(&self) -> i32\ntrait Unrelated < Other:\n    pass\nstruct Noise: pass\nimpl Base for Noise:\n    type Item = i32\n    fn base(&self) -> i32: +0\nimpl Other for Noise:\n    fn other(&self) -> i32: +0\nimpl Unrelated for Noise:\n    pass\n",
    );
    for i in 0..n {
        writeln!(
            source,
            "trait Bad_{i} < Base[Item = i32]:\n    pass\nimpl Bad_{i} for Noise:\n    pass"
        )
        .unwrap();
    }
    source.push_str("trait Good < Base[Item = string]:\n    pass\nfn needs() -> i32 $ Base[Item = string]:\n    $.use(Base[Item = string]).base()\nfn run() -> i32 $ Good:\n    $.with(");
    for i in 0..n {
        if i > 0 {
            source.push_str(", ");
        }
        write!(source, "Bad_{i}=box(Noise {{}})").unwrap();
    }
    source.push_str("):\n        let total = +0\n");
    for _ in 0..n {
        source.push_str("        $.with(Unrelated=box(Noise {})):\n            total += needs()\n");
    }
    source.push_str("        total\npub fn main() -> void:\n    pass\n");
    source
}

#[test]
fn binding_constraints_do_not_rescan_competing_suppliers() {
    let (small, large) = (
        measure(&constrained_competing_scopes(100), false),
        measure(&constrained_competing_scopes(1000), false),
    );
    eprintln!("binding-constrained suppliers/scopes: {small} -> {large}");
    assert!(small > 0 && large <= 12 * small, "{small} -> {large}");
}
