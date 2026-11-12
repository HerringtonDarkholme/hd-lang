//! #1376: Copy inputs at the common callable ABI need one body census and
//! one argument index per call, including a generic owning body specialized
//! to a borrowing concrete type. Width and alias depth must not multiply it.

use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use std::fmt::Write as _;

fn source(width: usize, depth: usize) -> String {
    let inputs = vec!["T"; width].join(", ");
    let concrete = vec!["string"; width].join(", ");
    let mut s = format!(
        "fn make[T < ?Copy]() -> fn(fn({inputs}) -> usize, {inputs}) -> usize:\n    fn(callback: fn({inputs}) -> usize"
    );
    for k in 0..width {
        let _ = write!(s, ", p{k}: T");
    }
    s.push_str(") -> usize:\n");
    for k in 0..width {
        for d in 0..depth {
            let from = if d == 0 {
                format!("p{k}")
            } else {
                format!("a{k}_{}", d - 1)
            };
            let _ = writeln!(s, "        a{k}_{d} := {from}");
        }
    }
    s.push_str("        callback(");
    for k in 0..width {
        if k != 0 {
            s.push_str(", ");
        }
        if depth == 0 {
            let _ = write!(s, "p{k}");
        } else {
            let _ = write!(s, "a{k}_{}", depth - 1);
        }
    }
    let _ = write!(
        s,
        ")\n\nfn concrete() -> fn(fn({concrete}) -> usize, {concrete}) -> usize:\n    make::[string]()\n"
    );
    s
}

fn work(width: usize, depth: usize) -> u64 {
    measure(&source(width, depth))
}

fn measure(source: &str) -> u64 {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let (out, work) = hd_wasm::work::count_loan_queries(|| {
        build(
            &host,
            "app",
            &Goal::Fixture {
                module: "main".into(),
            },
        )
    });
    assert!(!out.diags.has_errors(), "{}", out.render());
    let valid = out.wasm.as_deref().map(|bytes| {
        wasmparser::Validator::new()
            .validate_all(bytes)
            .map(|_| ())
            .map_err(|error| error.to_string())
    });
    assert_eq!(valid, Some(Ok(())), "{}", out.render());
    work
}

#[test]
fn wide_function_value_loans_visit_actual_uses_once_at_depth_twenty() {
    let shallow = (work(100, 1), work(1000, 1));
    let deep = (work(100, 20), work(1000, 20));
    for (depth, (small, large)) in [(1, shallow), (20, deep)] {
        eprintln!("function-value loan work: width100 {small}, width1000 {large}, depth{depth}");
        assert!(small > 0, "the generated program did not query loan facts");
        assert!(large <= small * 11, "width100 {small}, width1000 {large}");
    }
    assert!(
        deep.0 <= shallow.0 * 20,
        "width100: {shallow:?} -> {deep:?}"
    );
    assert!(
        deep.1 <= shallow.1 * 20,
        "width1000: {shallow:?} -> {deep:?}"
    );
}

fn captured_source(width: usize) -> String {
    let mut s = String::from("fn concrete(text: string) -> usize:\n    let total: usize = 0\n");
    for k in 0..width {
        let _ = writeln!(s, "    callback{k} := fn() -> usize: text.len()");
        let _ = writeln!(s, "    total += callback{k}()");
    }
    s.push_str("    total\n");
    s
}

#[test]
fn sibling_captures_share_the_census_and_visit_only_their_subbody() {
    let small = measure(&captured_source(25));
    let large = measure(&captured_source(250));
    eprintln!("sibling capture loan work: width25 {small}, width250 {large}");
    assert!(small > 0, "the generated program did not query loan facts");
    assert!(large <= small * 11, "width25 {small}, width250 {large}");
}
