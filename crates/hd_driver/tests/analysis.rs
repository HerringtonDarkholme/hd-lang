#![cfg(feature = "analysis")]

use std::sync::Mutex;

use hd_base::Hash128;
use hd_cache::{CacheStore, EntryKind, LAYOUT_VERSION, MemoryStore, encode_entry};
use hd_check::analysis::CheckedView;
use hd_diag::{Code, DiagBuf, FixSafety, Severity};
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

    fn diagnostic_code(&self, name: &str) -> Option<Code> {
        (name == Code::MixedScriptIdentifier.as_str()).then_some(Code::MixedScriptIdentifier)
    }

    fn event(&self, event: &Event<'_>, view: &CheckedView<'_>, out: &mut DiagBuf) {
        let source = view.source();
        let shown = match event {
            Event::Ident(span) => format!("ident:{}", source.text_at(*span).unwrap()),
            Event::Decl(event) => {
                let name = event
                    .name
                    .and_then(|span| source.text_at(span))
                    .unwrap_or("impl");
                if event.public
                    && let Some(span) = event.name
                {
                    out.push(
                        Code::MixedScriptIdentifier,
                        Severity::Warning,
                        span,
                        "synthetic analysis finding",
                        None,
                    );
                    out.add_label(event.span, "the public declaration");
                    out.add_fix("rename it", FixSafety::Exact, &[(span, "entry")]);
                }
                format!("decl:{name}")
            }
            Event::Module(event) => {
                format!("module:{}", event.path)
            }
            Event::Comparison(_) => "comparison".to_owned(),
            Event::Binding(_) => "binding".to_owned(),
            Event::Statement(_) => "statement".to_owned(),
            Event::Pattern(_) => "pattern".to_owned(),
            Event::Discard(_) => "discard".to_owned(),
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

#[derive(Default)]
struct TypedRecorder {
    events: Mutex<Vec<String>>,
}

impl TypedRecorder {
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}

impl AnalysisSink for TypedRecorder {
    fn interests(&self) -> Events {
        Events::CHECKED
    }

    fn diagnostic_code(&self, _: &str) -> Option<Code> {
        None
    }

    fn event(&self, event: &Event<'_>, view: &CheckedView<'_>, _: &mut DiagBuf) {
        let shown = match event {
            Event::Comparison(event) => {
                let left = view.expression(event.left).unwrap();
                let right = view.expression(event.right).unwrap();
                let primitive = view.adjusted_type(event.left).unwrap().primitive();
                format!(
                    "comparison:{:?}:{primitive:?}:{}:{}",
                    event.operator,
                    view.source().text_at(left.span()).unwrap(),
                    right.is_unsuffixed_zero()
                )
            }
            Event::Binding(source) => {
                let binding = view.binding(*source).unwrap();
                let mutability = binding.mutability();
                format!(
                    "binding:{}:{}:{}",
                    binding.name(),
                    binding.is_read(),
                    mutability.is_some_and(|fact| {
                        fact.is_primitive()
                            && !fact.annotation_is_writable()
                            && !fact.mutable_address_taken()
                            && fact.token().is_some()
                    })
                )
            }
            Event::Statement(source) => {
                let statement = view.statement(*source).unwrap();
                format!("statement:{}", statement.is_reachable())
            }
            Event::Pattern(source) => {
                let pattern = view.pattern(*source).unwrap();
                let bindings: Vec<_> = pattern
                    .bindings()
                    .iter()
                    .map(|binding| binding.name.as_str())
                    .collect();
                format!(
                    "pattern:{}:{:?}:{bindings:?}",
                    pattern.variant(),
                    pattern.fields()
                )
            }
            Event::Discard(source) => {
                let call = view.call(*source).unwrap();
                let selected = view.selected_declaration(*source);
                format!(
                    "discard:{}:{}:{}:{}:{}",
                    call.positional(),
                    call.named(),
                    call.has_spread(),
                    call.after_spread(),
                    selected.is_some_and(hd_check::analysis::DeclRef::is_prelude_debug)
                )
            }
            Event::Ident(_) | Event::Decl(_) | Event::Module(_) => unreachable!(),
        };
        self.events.lock().unwrap().push(shown);
    }
}

#[test]
fn checked_views_expose_completed_types_bindings_flow_patterns_and_callees() {
    let source = "enum Pair:\n    Values(left: i32, right: i32)\nfn inspect(value: Pair, remaining: usize) -> bool:\n    let mut count = +1\n    _ := count\n    match value:\n        .Values(right, left) =>\n            debug(+1)\n            remaining < 0\n";
    let store = MemoryStore::default();
    let checked = run(&store, source, None);
    assert!(!checked.diags.has_errors(), "{}", checked.render());
    assert_eq!(checked.counters.modules_checked, ["app.main"]);

    let sink = TypedRecorder::default();
    let out = run(&store, source, Some(&sink));
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert_eq!(out.counters.miss("lint"), 1);
    assert_eq!(out.counters.modules_checked, ["app.main"]);
    assert!(out.source_facts.comparisons.is_empty());
    assert!(out.source_facts.bindings.is_empty());
    assert!(out.source_facts.patterns.is_empty());
    assert!(out.source_facts.statements.is_empty());
    assert!(out.source_facts.discarded.is_empty());

    let events = sink.take();
    assert!(
        events
            .iter()
            .any(|event| event == "comparison:Lt:Some(Usize):remaining:true"),
        "{events:#?}"
    );
    assert!(
        events
            .iter()
            .any(|event| event == "binding:count:true:true"),
        "{events:#?}"
    );
    assert!(
        events
            .iter()
            .any(|event| { event == "pattern:Values:[\"left\", \"right\"]:[\"right\", \"left\"]" }),
        "{events:#?}"
    );
    assert!(
        events
            .iter()
            .any(|event| event == "discard:1:0:false:0:true"),
        "{events:#?}"
    );
    assert!(events.iter().any(|event| event == "statement:true"));

    let warm = TypedRecorder::default();
    let out = run(&store, source, Some(&warm));
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert!(warm.take().is_empty());
    assert_eq!(out.counters.hit("lint"), 1);
    assert!(out.counters.modules_checked.is_empty());
}

#[test]
fn verified_source_walk_is_ordered_then_loads_the_lint_cache() {
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
    let cold_sink = Recorder::new(Events::SOURCE);
    let cold = run(&store, source, Some(&cold_sink));
    assert!(!cold.diags.has_errors(), "{}", cold.render());
    assert_eq!(cold_sink.take(), expected);
    assert_eq!(cold.analysis_diags.code, [Code::MixedScriptIdentifier]);
    assert_eq!(cold.counters.miss("lint"), 1);

    let warm_sink = Recorder::new(Events::SOURCE);
    let warm = run(&store, source, Some(&warm_sink));
    assert!(!warm.diags.has_errors(), "{}", warm.render());
    assert!(warm_sink.take().is_empty());
    assert!(warm.counters.modules_checked.is_empty());
    assert_eq!(warm.counters.hit("lint"), 1);
    assert_eq!(warm.analysis_diags.code, cold.analysis_diags.code);
    assert_eq!(warm.analysis_diags.primary, cold.analysis_diags.primary);
    assert_eq!(
        warm.analysis_diags.label_span,
        cold.analysis_diags.label_span
    );
    assert_eq!(warm.analysis_diags.edits, cold.analysis_diags.edits);
    assert_eq!(warm.analysis_diags.text, cold.analysis_diags.text);
}

#[test]
fn cached_finding_spans_follow_comment_only_line_shifts() {
    let store = MemoryStore::default();
    let source = "pub fn main() -> void:\n    pass\n";
    let cold_sink = Recorder::new(Events::SOURCE);
    let cold = run(&store, source, Some(&cold_sink));
    assert!(!cold.diags.has_errors(), "{}", cold.render());
    assert_eq!(cold.analysis_diags.len(), 1);

    let prefix = "# inserted comment\n\n";
    let shifted = format!("{prefix}{source}");
    let warm_sink = Recorder::new(Events::SOURCE);
    let warm = run(&store, &shifted, Some(&warm_sink));
    assert!(!warm.diags.has_errors(), "{}", warm.render());
    assert!(warm_sink.take().is_empty());
    assert!(warm.counters.modules_checked.is_empty());
    assert_eq!(warm.counters.hit("lint"), 1);
    let delta = u32::try_from(prefix.len()).unwrap();
    assert_eq!(
        warm.analysis_diags.primary[0].lo,
        cold.analysis_diags.primary[0].lo + delta
    );
    assert_eq!(
        warm.analysis_diags.primary[0].hi,
        cold.analysis_diags.primary[0].hi + delta
    );
    assert_eq!(
        warm.analysis_diags.label_span[0].lo,
        cold.analysis_diags.label_span[0].lo + delta
    );
    assert_eq!(
        warm.analysis_diags.edits[0].span.lo,
        cold.analysis_diags.edits[0].span.lo + delta
    );
    let span = warm.analysis_diags.primary[0];
    assert_eq!(
        &warm.texts[span.file.idx()][span.lo as usize..span.hi as usize],
        "main"
    );
}

#[test]
fn lint_entries_from_another_toolchain_are_cache_misses() {
    let source = "pub fn main() -> void:\n    pass\n";
    let store = MemoryStore::default();
    let cold_sink = Recorder::new(Events::SOURCE);
    let cold = run(&store, source, Some(&cold_sink));
    assert!(!cold.diags.has_errors(), "{}", cold.render());
    let key = cold.counters.check_keys["app.main"];
    let foreign = encode_entry(
        EntryKind::Lint,
        LAYOUT_VERSION,
        key,
        Hash128(1),
        0,
        &[(0, 0, &[])],
    );
    store.put(EntryKind::Lint, &key, &foreign);

    let sink = Recorder::new(Events::SOURCE);
    let out = run(&store, source, Some(&sink));
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert!(!sink.take().is_empty());
    assert_eq!(out.counters.miss("lint"), 1);
    assert_eq!(out.counters.modules_checked, ["app.main"]);
}

#[test]
fn changing_one_module_misses_only_its_lint_entry() {
    fn build(store: &MemoryStore, b_body: &str, sink: &Recorder) -> hd_driver::Output {
        let mut sources = MemorySources::default();
        sources.insert("a.hd", "pub fn a() -> void:\n    pass\n");
        sources.insert("b.hd", &format!("pub fn b() -> void:\n{b_body}\n"));
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        build_packages_with_analysis(
            &host,
            "app",
            &Packages::default(),
            &Goal::Analyze,
            Some(sink),
        )
    }

    let store = MemoryStore::default();
    let cold_sink = Recorder::new(Events::SOURCE);
    let cold = build(&store, "    pass", &cold_sink);
    assert!(!cold.diags.has_errors(), "{}", cold.render());
    assert_eq!(cold.analysis_diags.len(), 2);

    let warm_sink = Recorder::new(Events::SOURCE);
    let warm = build(&store, "    pass", &warm_sink);
    assert!(!warm.diags.has_errors(), "{}", warm.render());
    assert!(warm_sink.take().is_empty());
    assert_eq!(warm.counters.hit("lint"), 2);

    let changed_sink = Recorder::new(Events::SOURCE);
    let changed = build(&store, "    _ := +1", &changed_sink);
    assert!(!changed.diags.has_errors(), "{}", changed.render());
    assert_eq!(
        changed_sink.take(),
        ["ident:b", "ident:void", "decl:b", "module:app.b"]
    );
    assert_eq!(changed.counters.hit("lint"), 1);
    assert_eq!(changed.counters.miss("lint"), 1);
    assert_eq!(changed.counters.modules_checked, ["app.b"]);
}

#[test]
fn interests_skip_unrequested_events_and_errors_discard_findings() {
    let store = MemoryStore::default();
    let sink = Recorder::new(Events::DECL.union(Events::MODULE));
    let out = run(&store, "pub fn main() -> void:\n    pass\n", Some(&sink));
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert_eq!(sink.take(), ["decl:main", "module:app.main"]);
    assert_eq!(out.analysis_diags.len(), 1);

    let failed = Recorder::new(Events::SOURCE);
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
    assert_eq!(out.counters.hit("lint") + out.counters.miss("lint"), 0);
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
    assert_eq!(warm.counters.hit("lint") + warm.counters.miss("lint"), 0);
}

#[test]
fn module_findings_are_merged_in_source_table_order() {
    fn build(executor: Executor) -> Vec<u32> {
        let mut sources = MemorySources::default();
        sources.insert("b.hd", "pub fn b() -> void:\n    pass\n");
        sources.insert("a.hd", "pub fn a() -> void:\n    pass\n");
        let store = MemoryStore::default();
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor,
        };
        let sink = Recorder::new(Events::DECL.union(Events::MODULE));
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
