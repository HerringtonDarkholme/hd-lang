#![cfg(feature = "analysis")]

use std::sync::Mutex;

use hd_base::{Hash128, Span};
use hd_cache::{CacheStore, EntryKind, LAYOUT_VERSION, MemoryStore, encode_entry};
use hd_diag::{Code, DiagBuf, FixSafety, Severity};
use hd_driver::analysis::{AnalysisSink, Module, Requests};
use hd_driver::{
    Dependency, Executor, Goal, Host, NoClock, Packages, build_packages,
    build_packages_with_analysis,
};
use hd_project::MemorySources;
use hd_resolve::Src;
use hd_sched::SerialOrder;
use hd_syntax::Decl;

/// Records each hook call, and reports each public declaration's name with
/// a label and a fix, so cached findings carry every part.
struct Recorder {
    events: Mutex<Vec<String>>,
    requests: Requests,
}

impl Recorder {
    fn new(requests: Requests) -> Self {
        Self {
            events: Mutex::default(),
            requests,
        }
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}

impl AnalysisSink for Recorder {
    fn requests(&self) -> Requests {
        self.requests
    }

    fn diagnostic_code(&self, name: &str) -> Option<Code> {
        (name == Code::MixedScriptIdentifier.as_str()).then_some(Code::MixedScriptIdentifier)
    }

    fn module(&self, module: &Module<'_>, out: &mut DiagBuf) {
        let src = module.src;
        for node in src.root().descendants() {
            let Some(decl) = Decl::cast(node) else {
                continue;
            };
            if src.is_pub(node)
                && let Some(name) = decl.name(&src.parse.tokens)
            {
                let span = src.token_span(name);
                out.push(
                    Code::MixedScriptIdentifier,
                    Severity::Warning,
                    span,
                    "synthetic analysis finding",
                    None,
                );
                out.add_label(src.span(node), "the public declaration");
                out.add_fix("rename it", FixSafety::Exact, &[(span, "entry")]);
            }
        }
        self.events
            .lock()
            .unwrap()
            .push(format!("module:{}", module.path));
    }

    fn fetched_debug(&self, package: &str, span: Span, src: Src<'_>, out: &mut DiagBuf) {
        let reference = &src.text[span.lo as usize..span.hi as usize];
        assert!(reference.starts_with("dbg"), "{reference}");
        out.push(
            Code::DbgInDependency,
            Severity::Warning,
            span,
            "synthetic fetched debug finding",
            None,
        );
        self.events
            .lock()
            .unwrap()
            .push(format!("fetched-debug:{package}:{reference}"));
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

/// Shows what each checked view answers.
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
    fn requests(&self) -> Requests {
        Requests::CHECKED
    }

    fn diagnostic_code(&self, _: &str) -> Option<Code> {
        None
    }

    fn module(&self, module: &Module<'_>, _: &mut DiagBuf) {
        let src = module.src;
        let text = |node| {
            let span = src.span(src.parse.tree.node(node));
            &src.text[span.lo as usize..span.hi as usize]
        };
        let facts = module.facts.expect("the recorder requests checked facts");
        let mut events = self.events.lock().unwrap();
        for comparison in &facts.comparisons {
            events.push(format!(
                "comparison:{}:{:?}:{:?}",
                text(comparison.node),
                comparison.left,
                comparison.right
            ));
        }
        for binding in &facts.bindings {
            events.push(format!(
                "binding:{}:{}:{}",
                text(binding.node),
                binding.read,
                binding.mutability.as_ref().is_some_and(|fact| {
                    fact.primitive && !fact.address_taken && text(fact.pattern).starts_with("mut ")
                })
            ));
        }
        for &statement in &facts.unreachable {
            events.push(format!("unreachable:{}", text(statement)));
        }
        for write in &facts.parameter_writes {
            events.push(format!(
                "parameter-write:{}:{}",
                text(write.parameter),
                text(write.write)
            ));
        }
        for pattern in &facts.patterns {
            let bindings: Vec<_> = pattern.subs.iter().map(|sub| text(sub.unwrap())).collect();
            events.push(format!(
                "pattern:{}:{:?}:{bindings:?}",
                pattern.variant, pattern.fields
            ));
        }
        for call in &facts.discarded {
            events.push(format!(
                "discard:{}:{}:{}:{}",
                text(call.node),
                call.selected.is_some() && call.prelude_debug,
                call.standard_debug,
                call.standard_dbg
            ));
        }
    }

    fn fetched_debug(&self, _: &str, _: Span, _: Src<'_>, _: &mut DiagBuf) {
        unreachable!("ruled out: the recorder requests checked facts only");
    }
}

#[test]
fn fetched_debug_event_reuses_dependency_check_entries_without_body_rechecks() {
    let mut root = MemorySources::default();
    root.insert(
        "src/main.hd",
        "use dep.money.{cents}\npub fn main() -> void:\n    _ := cents(+1)\n",
    );
    let mut money = MemorySources::default();
    money.insert(
        "src/lib.hd",
        "pub fn cents(n: i32) -> i32:\n    dbg(n)\n    n * 100\n",
    );
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &root,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        requires: vec![("money".to_owned(), 1)],
        deps: vec![Dependency {
            name: "money".to_owned(),
            identity: "/w/money".to_owned(),
            sources: &money,
            requires: Vec::new(),
            fetched: true,
        }],
        ..Packages::default()
    };
    let goal = Goal::Analyze;

    let checked = build_packages(&host, "app", &packages, &goal);
    assert!(checked.diags.is_empty(), "{}", checked.render());
    assert!(!checked.counters.modules_checked.is_empty());

    for _ in 0..2 {
        let sink = Recorder::new(Requests::FETCHED_DEBUG);
        let lint = build_packages_with_analysis(&host, "app", &packages, &goal, Some(&sink));
        assert!(lint.diags.is_empty(), "{}", lint.render());
        let events = sink.take();
        assert_eq!(events.len(), 1);
        assert!(events[0].starts_with("fetched-debug:money:dbg("));
        assert_eq!(lint.analysis_diags.code, [Code::DbgInDependency]);
        assert!(lint.counters.modules_checked.is_empty());
    }
}

#[test]
fn checked_views_expose_completed_types_bindings_flow_patterns_and_callees() {
    let source = "enum Pair:\n    Values(left: i32, right: i32)\nfn inspect(value: Pair, remaining: usize) -> bool:\n    let mut count = +1\n    _ := count\n    match value:\n        .Values(right, left) =>\n            debug(+1)\n            remaining < 0\nfn stop() -> i32:\n    return +1\n    +2\n";
    let store = MemoryStore::default();
    let checked = run(&store, source, None);
    assert!(!checked.diags.has_errors(), "{}", checked.render());
    assert_eq!(checked.counters.modules_checked, ["app.main"]);

    let sink = TypedRecorder::default();
    let out = run(&store, source, Some(&sink));
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert_eq!(out.counters.miss("lint"), 1);
    assert_eq!(out.counters.modules_checked, ["app.main"]);
    let events = sink.take();
    assert!(
        events
            .iter()
            .any(|event| event == "comparison:remaining < 0:Some(Usize):Some(Usize)"),
        "{events:#?}"
    );
    assert!(
        events
            .iter()
            .any(|event| event == "binding:mut count:true:true"),
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
            .any(|event| event == "discard:debug(+1):true:true:true"),
        "{events:#?}"
    );
    assert!(
        events.iter().any(|event| event == "unreachable:+2"),
        "{events:#?}"
    );

    let warm = TypedRecorder::default();
    let out = run(&store, source, Some(&warm));
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert!(warm.take().is_empty());
    assert_eq!(out.counters.hit("lint"), 1);
    assert!(out.counters.modules_checked.is_empty());
}

#[test]
fn verified_modules_reach_the_hook_then_load_the_lint_cache() {
    let source =
        "pub fn alpha(value: i32) -> i32:\n    value\n\nfn beta() -> void:\n    _ := alpha(+1)\n";
    let store = MemoryStore::default();
    let expected = ["module:app.main"];
    let cold_sink = Recorder::new(Requests::MODULE);
    let cold = run(&store, source, Some(&cold_sink));
    assert!(!cold.diags.has_errors(), "{}", cold.render());
    assert_eq!(cold_sink.take(), expected);
    assert_eq!(cold.analysis_diags.code, [Code::MixedScriptIdentifier]);
    assert_eq!(cold.counters.miss("lint"), 1);

    let warm_sink = Recorder::new(Requests::MODULE);
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
    let cold_sink = Recorder::new(Requests::MODULE);
    let cold = run(&store, source, Some(&cold_sink));
    assert!(!cold.diags.has_errors(), "{}", cold.render());
    assert_eq!(cold.analysis_diags.len(), 1);

    let prefix = "# inserted comment\n\n";
    let shifted = format!("{prefix}{source}");
    let warm_sink = Recorder::new(Requests::MODULE);
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
    let cold_sink = Recorder::new(Requests::MODULE);
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

    let sink = Recorder::new(Requests::MODULE);
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
    let cold_sink = Recorder::new(Requests::MODULE);
    let cold = build(&store, "    pass", &cold_sink);
    assert!(!cold.diags.has_errors(), "{}", cold.render());
    assert_eq!(cold.analysis_diags.len(), 2);

    let warm_sink = Recorder::new(Requests::MODULE);
    let warm = build(&store, "    pass", &warm_sink);
    assert!(!warm.diags.has_errors(), "{}", warm.render());
    assert!(warm_sink.take().is_empty());
    assert_eq!(warm.counters.hit("lint"), 2);

    let changed_sink = Recorder::new(Requests::MODULE);
    let changed = build(&store, "    _ := +1", &changed_sink);
    assert!(!changed.diags.has_errors(), "{}", changed.render());
    assert_eq!(changed_sink.take(), ["module:app.b"]);
    assert_eq!(changed.counters.hit("lint"), 1);
    assert_eq!(changed.counters.miss("lint"), 1);
    assert_eq!(changed.counters.modules_checked, ["app.b"]);
}

#[test]
fn errors_discard_findings() {
    let store = MemoryStore::default();
    let sink = Recorder::new(Requests::MODULE);
    let out = run(&store, "pub fn main() -> void:\n    pass\n", Some(&sink));
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert_eq!(sink.take(), ["module:app.main"]);
    assert_eq!(out.analysis_diags.len(), 1);

    let failed = Recorder::new(Requests::MODULE);
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
fn no_requests_keep_warm_checks_warm() {
    let source = "fn value() -> i32:\n    +1\n";
    let store = MemoryStore::default();
    let cold = run(&store, source, None);
    assert!(!cold.diags.has_errors(), "{}", cold.render());
    assert!(!cold.counters.modules_checked.is_empty());

    let sink = Recorder::new(Requests::NONE);
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
        let sink = Recorder::new(Requests::MODULE);
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
