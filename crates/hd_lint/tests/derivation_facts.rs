use hd_base::SliceSort as _;
use hd_cache::MemoryStore;
use hd_diag::{Code, Severity};
use hd_driver::{
    Executor, Goal, Host, NoClock, Packages, build_packages, build_packages_with_analysis,
};
use hd_lint::{ANALYZER, Level, Policy, UNUSED_DERIVATION};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

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
        for analyze in [false, true, true, false] {
            let out = if analyze {
                build_packages_with_analysis(
                    &host,
                    "app",
                    &Packages::default(),
                    &Goal::Analyze,
                    Some(&ANALYZER),
                )
            } else {
                build_packages(&host, "app", &Packages::default(), &Goal::Analyze)
            };
            assert!(out.diags.is_empty(), "{}", out.render());
            let findings = Policy::default().apply(&out.analysis_diags);
            if analyze {
                assert_eq!(findings.code, [Code::UnusedDerivationFact]);
            } else {
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
        let out = build_packages_with_analysis(
            &host,
            "app",
            &Packages {
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
            Some(&ANALYZER),
        );
        assert!(out.diags.is_empty(), "{}", out.render());
        let findings = Policy::default().apply(&out.analysis_diags);
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
    for analyze in [false, true, true, false] {
        let out = if analyze {
            build_packages_with_analysis(
                &host,
                "app",
                &Packages::default(),
                &Goal::Analyze,
                Some(&ANALYZER),
            )
        } else {
            build_packages(&host, "app", &Packages::default(), &Goal::Analyze)
        };
        assert!(out.diags.has_errors(), "{}", out.render());
        assert!(out.diags.severity.iter().all(|s| *s == Severity::Error));
        let rendered = out.diags.render_json();
        if let Some(expected) = &expected {
            assert_eq!(&rendered, expected);
        } else {
            expected = Some(rendered);
        }
        assert!(out.analysis_diags.is_empty());
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
    for analyze in [false, true, true] {
        let packages = Packages {
            check_roots: Some(vec!["src/lib.hd".to_owned()]),
            ..Packages::default()
        };
        let out = if analyze {
            build_packages_with_analysis(&host, "app", &packages, &Goal::Analyze, Some(&ANALYZER))
        } else {
            build_packages(&host, "app", &packages, &Goal::Analyze)
        };
        assert!(out.diags.is_empty(), "{}", out.render());
        assert!(
            !out.counters
                .modules_checked
                .iter()
                .any(|name| name.ends_with(".helper"))
        );
        let findings = Policy::default().apply(&out.analysis_diags);
        if analyze {
            assert_eq!(findings.code, [Code::DerivationLineDrift]);
            assert_eq!(
                findings.get_text(findings.message[0]),
                "`Probe` configures member lines for `Session` that its `@derive(Echo)` does not"
            );
        } else {
            assert!(findings.is_empty());
        }
    }
}

#[test]
fn registered_module_and_derivation_findings_share_warm_checks() {
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
        for analyze in [false, true, true, false] {
            let out = if analyze {
                build_packages_with_analysis(
                    &host,
                    "app",
                    &Packages::default(),
                    &Goal::Analyze,
                    Some(&ANALYZER),
                )
            } else {
                build_packages(&host, "app", &Packages::default(), &Goal::Analyze)
            };
            assert!(out.diags.is_empty(), "{}", out.render());
            let findings = Policy::default().apply(&out.analysis_diags);
            if analyze {
                assert_eq!(
                    findings.code,
                    [Code::UnselectedMain, Code::UnusedDerivationFact]
                );
                let mut policy = Policy::default();
                policy.set(hd_lint::UNSELECTED_MAIN, Level::Allow).unwrap();
                policy.set(UNUSED_DERIVATION, Level::Deny).unwrap();
                let selected = policy.apply(&out.analysis_diags);
                assert_eq!(selected.code, [Code::UnusedDerivationFact]);
                assert_eq!(selected.severity, [Severity::Error]);
            } else {
                assert!(findings.is_empty());
            }
        }
    }
}
