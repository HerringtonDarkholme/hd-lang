use std::sync::Arc;

use hd_base::{DefId, FileId, PathId, SliceSort as _, Span};
use hd_cache::MemoryStore;
use hd_diag::{Code, Severity};
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_lint::{DERIVATION_DRIFT, Level, Policy, UNUSED_DERIVATION, analyze_derivations};
use hd_project::MemorySources;
use hd_resolve::facts::{BlockDecorator, DerivationBlock, DerivationFacts, Derived};
use hd_sched::SerialOrder;

fn span(file: u32, lo: u32, hi: u32) -> Span {
    Span {
        file: FileId::from_raw(file),
        lo,
        hi,
    }
}

fn block(file: u32, lo: u32, trait_: u32, name: &str, member: &str) -> DerivationBlock {
    DerivationBlock {
        folder: 0,
        target: DefId::from_raw(1),
        package: PathId::from_raw(2),
        trait_: DefId::from_raw(trait_),
        trait_name: name.to_owned(),
        target_name: "Session".to_owned(),
        lines: vec![(member.to_owned(), false, vec!["pass".to_owned()])],
        span: span(file, lo, lo + 1),
    }
}

#[test]
fn drift_uses_the_first_nominal_block_before_the_derive_fallback() {
    let first = block(0, 1, 3, "Print", "cache");
    let second = block(0, 2, 4, "Encode", "id");
    let third = block(0, 3, 5, "Inspect", "cache");
    let mut facts = DerivationFacts {
        blocks: vec![second.clone(), first.clone(), third, second],
        derived: vec![
            Derived {
                folder: 0,
                target: first.target,
                package: first.package,
                trait_: DefId::from_raw(6),
                name: "Zed".to_owned(),
            },
            Derived {
                folder: 0,
                target: first.target,
                package: first.package,
                trait_: DefId::from_raw(7),
                name: "Alpha".to_owned(),
            },
        ],
        ..DerivationFacts::default()
    };
    let findings = analyze_derivations(&facts, &[], Policy::default());
    assert_eq!(findings.code, [Code::DerivationLineDrift; 3]);
    let messages: Vec<_> = findings
        .message
        .iter()
        .map(|m| findings.get_text(*m))
        .collect();
    assert_eq!(
        messages,
        [
            "`Print` configures member lines for `Session` that its `@derive(Alpha)` does not",
            "`Encode` configures different member lines for `Session` than `Print` does",
            "`Inspect` configures member lines for `Session` that its `@derive(Alpha)` does not",
        ]
    );
    facts.blocks[1].lines.clear();
    // Empty lines suppress only the derive fallback, not differing-block advice.
    let findings = analyze_derivations(&facts, &[], Policy::default());
    assert_eq!(findings.len(), 2);
}

#[test]
fn block_comparisons_keep_module_target_and_package_boundaries() {
    let first = block(0, 1, 3, "Print", "cache");
    let mut other_file = block(1, 2, 4, "Encode", "id");
    let mut other_target = block(0, 3, 5, "Inspect", "id");
    other_target.target = DefId::from_raw(11);
    let mut other_package = block(0, 4, 6, "Visit", "id");
    other_package.package = PathId::from_raw(12);
    let facts = DerivationFacts {
        blocks: vec![first, other_file.clone(), other_target, other_package],
        ..DerivationFacts::default()
    };
    assert!(analyze_derivations(&facts, &[], Policy::default()).is_empty());
    other_file.span.file = FileId::from_raw(0);
    let mut facts = facts;
    facts.blocks.push(other_file);
    assert_eq!(analyze_derivations(&facts, &[], Policy::default()).len(), 1);
}

#[test]
fn exact_utf8_offsets_comments_and_crlf_are_preserved_in_fact_moves() {
    for newline in ["\n", "\r\n"] {
        let source = format!(
            "# café{newline}@\"雪\"  # note{newline}impl T by Structure:{newline}    pass{newline}"
        );
        let start = source.find('@').unwrap();
        let value = start + 1;
        let end = value + "\"雪\"".len();
        let first = source.find("pass").unwrap();
        let fact = BlockDecorator {
            span: span(
                0,
                u32::try_from(start).unwrap(),
                u32::try_from(end).unwrap(),
            ),
            value: Some(span(
                0,
                u32::try_from(value).unwrap(),
                u32::try_from(end).unwrap(),
            )),
            first: Some(span(
                0,
                u32::try_from(first).unwrap(),
                u32::try_from(first + 4).unwrap(),
            )),
            marker: false,
            valid: true,
        };
        let facts = DerivationFacts {
            decorators: vec![fact.clone(), fact],
            ..DerivationFacts::default()
        };
        let findings =
            analyze_derivations(&facts, &[Arc::from(source.as_str())], Policy::default());
        assert_eq!(findings.code, [Code::UnusedDerivationFact]);
        assert_eq!(findings.primary[0].lo as usize, start);
        assert_eq!(findings.edits.len(), 2);
        assert_eq!(findings.edits[0].span.lo as usize, start);
        assert_eq!(
            findings.edits[0].span.hi as usize,
            source.find("impl").unwrap()
        );
        assert_eq!(
            findings.get_text(findings.edits[1].text),
            format!("    Self += [\"雪\"]  # note{newline}")
        );
        assert_eq!(findings.edits[1].span.lo as usize, first - 4);
    }
}

#[test]
fn independent_policy_keeps_errors_and_consumed_markers_out_of_advice() {
    let mut facts = DerivationFacts {
        blocks: vec![
            block(0, 1, 3, "Print", "cache"),
            block(0, 2, 4, "Encode", "id"),
        ],
        ..DerivationFacts::default()
    };
    let occurrence = BlockDecorator {
        span: span(0, 0, 1),
        value: None,
        first: None,
        marker: false,
        valid: true,
    };
    facts.decorators.push(occurrence.clone());
    facts.decorators.push(BlockDecorator {
        marker: true,
        ..occurrence.clone()
    });
    facts.decorators.push(BlockDecorator {
        valid: false,
        ..occurrence
    });
    for (allow, deny, expected) in [
        (
            UNUSED_DERIVATION,
            DERIVATION_DRIFT,
            Code::DerivationLineDrift,
        ),
        (
            DERIVATION_DRIFT,
            UNUSED_DERIVATION,
            Code::UnusedDerivationFact,
        ),
    ] {
        let mut policy = Policy::default();
        policy.set(allow, Level::Allow).unwrap();
        policy.set(deny, Level::Deny).unwrap();
        let findings = analyze_derivations(&facts, &[], policy);
        assert_eq!(findings.code, [expected]);
        assert_eq!(findings.severity, [Severity::Error]);
    }
}

#[test]
fn warm_interface_and_check_hits_do_not_lose_or_cache_derivation_findings() {
    let source =
        include_str!("../../../spec/conformance/typing/warnings/derivation-block-decorator.hd");
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = MemoryStore::default();
    for executor in [Executor::Serial(SerialOrder::Priority), Executor::Pool(4)] {
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor,
        };
        for collect in [false, true, true, false] {
            let out = build_packages(
                &host,
                "app",
                &Packages {
                    collect_source_facts: collect,
                    ..Packages::default()
                },
                &Goal::Analyze,
            );
            assert!(out.diags.is_empty(), "{}", out.render());
            let findings =
                analyze_derivations(&out.derivation_facts, &out.texts, Policy::default());
            if collect {
                assert_eq!(findings.code, [Code::UnusedDerivationFact]);
            } else {
                assert!(out.derivation_facts.decorators.is_empty());
                assert!(out.derivation_facts.blocks.is_empty());
                assert!(out.derivation_facts.derived.is_empty());
                assert!(findings.is_empty());
            }
        }
        let out = build_packages(&host, "app", &Packages::default(), &Goal::Analyze);
        assert!(
            out.counters.modules_checked.is_empty(),
            "following ordinary check stays warm"
        );
    }
}

#[test]
fn selected_file_closure_collects_root_modules_but_not_dependencies_or_unreached_files() {
    let annotated =
        include_str!("../../../spec/conformance/typing/warnings/derivation-block-decorator-std.hd");
    let annotated = format!("{annotated}\npub fn value() -> i32:\n    +1\n");
    let mut sources = MemorySources::default();
    sources.insert("src/main.hd", "use pkg.helper.value\nuse dep.remote.value as remote_value\nfn root() -> i32:\n    value() + remote_value()\n");
    sources.insert("src/helper.hd", &annotated);
    sources.insert("src/unreached.hd", &annotated);
    let mut remote = MemorySources::default();
    remote.insert("src/lib.hd", &annotated);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    for selected in [true, false, true] {
        let out = build_packages(
            &host,
            "app",
            &Packages {
                collect_source_facts: true,
                check_roots: selected.then(|| vec!["src/main.hd".to_owned()]),
                requires: vec![("remote".to_owned(), 1)],
                deps: vec![hd_driver::Dependency {
                    name: "remote".to_owned(),
                    identity: "local-remote".to_owned(),
                    sources: &remote,
                    requires: vec![],
                    fetched: false,
                }],
                ..Packages::default()
            },
            &Goal::Analyze,
        );
        assert!(out.diags.is_empty(), "{}", out.render());
        let findings = analyze_derivations(&out.derivation_facts, &out.texts, Policy::default());
        let mut files: Vec<_> = findings
            .primary
            .iter()
            .map(|s| out.files[s.file.idx()].as_str())
            .collect();
        files.sort_stable();
        assert_eq!(
            files,
            if selected {
                vec!["src/helper.hd"]
            } else {
                vec!["src/helper.hd", "src/unreached.hd"]
            }
        );
    }
}

#[test]
fn invalid_derivation_decorators_keep_the_same_error_prerequisites() {
    let source = "use std.structure.Structure\nstruct Item:\n    id: i64\n@derive(Structure)\nimpl Item by Structure:\n    id += [\"key\"]\n";
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let mut expected = None;
    for collect in [false, true, true, false] {
        let out = build_packages(
            &host,
            "app",
            &Packages {
                collect_source_facts: collect,
                ..Packages::default()
            },
            &Goal::Analyze,
        );
        assert!(out.diags.has_errors(), "{}", out.render());
        assert!(out.diags.severity.iter().all(|s| *s == Severity::Error));
        let rendered = out.diags.render_json();
        if let Some(expected) = &expected {
            assert_eq!(&rendered, expected);
        } else {
            expected = Some(rendered);
        }
        assert!(
            analyze_derivations(&out.derivation_facts, &out.texts, Policy::default()).is_empty()
        );
    }
}

#[test]
fn cached_folder_facts_include_sibling_derive_identities() {
    let original = include_str!(
        "../../../spec/conformance/typing/warnings/derivation-line-drift-beside-derive.hd"
    );
    let mut sources = MemorySources::default();
    sources.insert("src/lib.hd", original);
    sources.insert(
        "src/helper.hd",
        "@derive(Debug)\npub struct Other:\n    pub value: i64\n",
    );
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    for collect in [false, true, true] {
        let out = build_packages(
            &host,
            "app",
            &Packages {
                collect_source_facts: collect,
                check_roots: Some(vec!["src/lib.hd".to_owned()]),
                ..Packages::default()
            },
            &Goal::Analyze,
        );
        assert!(out.diags.is_empty(), "{}", out.render());
        assert!(
            !out.counters
                .modules_checked
                .iter()
                .any(|name| name.ends_with(".helper"))
        );
        let findings = analyze_derivations(&out.derivation_facts, &out.texts, Policy::default());
        if collect {
            assert_eq!(findings.code, [Code::DerivationLineDrift]);
            assert_eq!(
                findings.get_text(findings.message[0]),
                "`Probe` configures member lines for `Session` that its `@derive(Echo)` does not"
            );
            let target = out.derivation_facts.blocks[0].target;
            assert!(
                out.derivation_facts
                    .derived
                    .iter()
                    .any(|derived| { derived.target != target && derived.name == "Debug" }),
                "the selected module's frozen folder retains its sibling's nominal derive identity"
            );
        } else {
            assert!(findings.is_empty());
        }
    }
}

#[test]
fn derive_fallback_uses_only_the_blocks_own_folder_snapshot() {
    let block = block(0, 1, 3, "Print", "cache");
    let mut facts = DerivationFacts {
        derived: vec![Derived {
            folder: 99,
            target: block.target,
            package: block.package,
            trait_: DefId::from_raw(4),
            name: "Encode".to_owned(),
        }],
        blocks: vec![block],
        ..DerivationFacts::default()
    };
    assert!(analyze_derivations(&facts, &[], Policy::default()).is_empty());
    facts.derived[0].folder = 0;
    assert_eq!(
        analyze_derivations(&facts, &[], Policy::default()).code,
        [Code::DerivationLineDrift]
    );
}

#[test]
fn integrated_module_and_derivation_facts_stay_independent_on_warm_checks() {
    let annotated =
        include_str!("../../../spec/conformance/typing/warnings/derivation-block-decorator.hd");
    let source = format!("{annotated}\npub fn main() -> void: pass\n");
    let mut sources = MemorySources::default();
    sources.insert("src/helper.hd", &source);
    for executor in [Executor::Serial(SerialOrder::Priority), Executor::Pool(4)] {
        let store = MemoryStore::default();
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor,
        };
        for (collect_source_facts, collect_main_sites) in [
            (false, false),
            (false, true),
            (true, false),
            (true, true),
            (true, true),
            (false, true),
            (false, false),
        ] {
            let out = build_packages(
                &host,
                "app",
                &Packages {
                    collect_source_facts,
                    collect_main_sites,
                    ..Packages::default()
                },
                &Goal::Analyze,
            );
            assert!(out.diags.is_empty(), "{}", out.render());
            let modules = hd_lint::analyze_modules(&out.main_sites, Policy::default());
            let derivations =
                analyze_derivations(&out.derivation_facts, &out.texts, Policy::default());
            assert_eq!(
                modules.code,
                vec![Code::UnselectedMain; usize::from(collect_main_sites)]
            );
            assert_eq!(
                derivations.code,
                vec![Code::UnusedDerivationFact; usize::from(collect_source_facts)]
            );
            if !collect_source_facts && collect_main_sites {
                assert!(
                    out.counters.modules_checked.is_empty(),
                    "main sites retain true Check hits"
                );
                assert!(out.derivation_facts.decorators.is_empty());
            }
            if collect_source_facts && collect_main_sites {
                let mut policy = Policy::default();
                policy.set(hd_lint::UNSELECTED_MAIN, Level::Allow).unwrap();
                policy.set(UNUSED_DERIVATION, Level::Deny).unwrap();
                assert!(hd_lint::analyze_modules(&out.main_sites, policy).is_empty());
                assert_eq!(
                    analyze_derivations(&out.derivation_facts, &out.texts, policy).severity,
                    [Severity::Error]
                );
            }
        }
    }
}
