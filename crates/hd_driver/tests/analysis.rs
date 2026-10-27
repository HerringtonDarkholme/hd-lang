#![cfg(feature = "analysis")]

use std::sync::Mutex;

use hd_cache::MemoryStore;
use hd_check::analysis::CheckedView;
use hd_diag::{Code, DiagBuf, Severity};
use hd_driver::analysis::{AnalysisSink, Event, Events};
use hd_driver::{
    Executor, Goal, Host, NoClock, Packages, build_packages, build_packages_with_analysis,
};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<String>>,
    interests: Events,
}

impl Recorder {
    fn new(interests: Events) -> Self {
        Self {
            events: Mutex::default(),
            interests,
        }
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}

impl AnalysisSink for Recorder {
    fn interests(&self) -> Events {
        self.interests
    }

    fn event(&self, event: &Event<'_>, view: &CheckedView<'_>, out: &mut DiagBuf) {
        let source = view.source();
        let shown = match event {
            Event::Ident(span) => format!("ident:{}", source.text_at(*span).unwrap()),
            Event::Decl(event) => format!(
                "decl:{}",
                event
                    .name
                    .and_then(|span| source.text_at(span))
                    .unwrap_or("impl")
            ),
            Event::Module(event) => {
                out.push(
                    Code::MixedScriptIdentifier,
                    Severity::Warning,
                    event.span,
                    "synthetic analysis finding",
                    None,
                );
                format!("module:{}", event.path)
            }
        };
        self.events.lock().unwrap().push(shown);
    }
}

fn run(store: &MemoryStore, source: &str, sink: Option<&dyn AnalysisSink>) -> hd_driver::Output {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build_packages_with_analysis(&host, "app", &Packages::default(), &Goal::Analyze, sink)
}

#[test]
fn verified_source_walk_is_ordered_and_repeats_after_a_check_hit() {
    let source =
        "pub fn alpha(value: i32) -> i32:\n    value\n\nfn beta() -> void:\n    _ := alpha(+1)\n";
    let store = MemoryStore::default();
    let expected = [
        "ident:alpha",
        "ident:value",
        "ident:i32",
        "ident:i32",
        "ident:value",
        "decl:alpha",
        "ident:beta",
        "ident:void",
        "ident:alpha",
        "decl:beta",
        "module:app.main",
    ];
    for _ in 0..2 {
        let sink = Recorder::new(Events::ALL);
        let out = run(&store, source, Some(&sink));
        assert!(!out.diags.has_errors(), "{}", out.render());
        assert_eq!(sink.take(), expected);
        assert_eq!(out.analysis_diags.len(), 1);
        assert_eq!(out.analysis_diags.code, [Code::MixedScriptIdentifier]);
    }
}

#[test]
fn interests_skip_unrequested_events_and_errors_discard_findings() {
    let store = MemoryStore::default();
    let sink = Recorder::new(Events::DECL.union(Events::MODULE));
    let out = run(&store, "pub fn main() -> void:\n    pass\n", Some(&sink));
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert_eq!(sink.take(), ["decl:main", "module:app.main"]);
    assert_eq!(out.analysis_diags.len(), 1);

    let failed = Recorder::new(Events::ALL);
    let out = run(
        &MemoryStore::default(),
        "pub fn main() -> void:\n    missing_name\n",
        Some(&failed),
    );
    assert!(out.diags.has_errors());
    assert!(failed.take().is_empty());
    assert!(out.analysis_diags.is_empty());
}

#[test]
fn ordinary_builds_do_not_run_analysis() {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", "pub fn main() -> void:\n    pass\n");
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build_packages(&host, "app", &Packages::default(), &Goal::Analyze);
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert!(out.analysis_diags.is_empty());
}

#[test]
fn an_empty_interest_set_keeps_warm_checks_warm() {
    let source = "fn value() -> i32:\n    +1\n";
    let store = MemoryStore::default();
    let cold = run(&store, source, None);
    assert!(!cold.diags.has_errors(), "{}", cold.render());
    assert!(!cold.counters.modules_checked.is_empty());

    let sink = Recorder::new(Events::default());
    let warm = run(&store, source, Some(&sink));
    assert!(!warm.diags.has_errors(), "{}", warm.render());
    assert!(sink.take().is_empty());
    assert!(warm.counters.modules_checked.is_empty());
    assert!(warm.counters.hit("check") > 0);
}

#[test]
fn module_findings_are_merged_in_source_table_order() {
    fn build(executor: Executor) -> Vec<u32> {
        let mut sources = MemorySources::default();
        sources.insert("b.hd", "fn b() -> void:\n    pass\n");
        sources.insert("a.hd", "fn a() -> void:\n    pass\n");
        let store = MemoryStore::default();
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor,
        };
        let sink = Recorder::new(Events::MODULE);
        let out = build_packages_with_analysis(
            &host,
            "app",
            &Packages::default(),
            &Goal::Analyze,
            Some(&sink),
        );
        assert!(!out.diags.has_errors(), "{}", out.render());
        out.analysis_diags
            .primary
            .iter()
            .map(|span| span.file.raw())
            .collect()
    }

    let serial = build(Executor::Serial(SerialOrder::Priority));
    let pool = build(Executor::Pool(4));
    assert_eq!(serial, pool);
    assert_eq!(serial.len(), 2);
    assert!(serial.is_sorted());
}
