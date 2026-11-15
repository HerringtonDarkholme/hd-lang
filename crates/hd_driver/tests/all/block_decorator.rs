//! The unused-derivation-fact warning (spec 14 `annot.fact.unused-block-decorator`):
//! a decorator before a derivation block is legal and ordinary checks stay quiet, whatever its
//! value. A decorator before a data declaration is a type-level fact and stays
//! clean, and a decorator that is already an error does not also warn.

use hd_cache::MemoryStore;
use hd_diag::{Code, Severity};
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_with(src: &MemorySources, store: &MemoryStore) -> Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

/// Each warning's code and the byte offset where its span starts.
fn warnings(out: &Output) -> Vec<(Code, u32)> {
    out.diags
        .content_order()
        .into_iter()
        .filter(|&i| out.diags.severity[i] == Severity::Warning)
        .map(|i| (out.diags.code[i], out.diags.primary[i].lo))
        .collect()
}

fn errors(out: &Output) -> Vec<Code> {
    out.diags
        .content_order()
        .into_iter()
        .filter(|&i| out.diags.severity[i] == Severity::Error)
        .map(|i| out.diags.code[i])
        .collect()
}

const STYLE: &str = "struct Style:
    prefix: string = \"\"

fn style(prefix: string = \"\") -> Style:
    Style { prefix: prefix }
";

#[test]
fn cached_rebuild_stays_quiet() {
    // Ordinary cold and warm checks do not retain lint-only advice.
    let src = format!(
        "use std.structure.Structure

{STYLE}
struct Loud:
    id: i64

@style(prefix=\"l_\")
impl Loud by Structure:
    id += [\"key\"]
"
    );
    let mut sources = MemorySources::default();
    sources.insert("main.hd", &src);
    let store = MemoryStore::default();
    for _ in 0..2 {
        let out = build_with(&sources, &store);
        assert!(errors(&out).is_empty(), "{}", out.render());
        assert!(warnings(&out).is_empty(), "{}", out.render());
    }
}
