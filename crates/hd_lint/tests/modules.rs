use hd_cache::MemoryStore;
use hd_diag::{Code, FixSafety};
use hd_driver::{
    Executor, Goal, Host, NoClock, Output, Packages, build_packages, build_packages_with_analysis,
};
use hd_lint::Policy;
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn checked(
    sources: &MemorySources,
    store: &MemoryStore,
    packages: &Packages<'_>,
    analyze: bool,
) -> Output {
    let host = Host {
        render_tir: &[],
        sources,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    if analyze {
        build_packages_with_analysis(
            &host,
            "app",
            packages,
            &Goal::Analyze,
            Some(&hd_lint::ANALYZER),
        )
    } else {
        build_packages(&host, "app", packages, &Goal::Analyze)
    }
}

fn sources() -> MemorySources {
    let mut sources = MemorySources::default();
    sources.insert(
        "src/main.hd",
        "use pkg.helper.label\npub fn main() -> void:\n    _ := label()\n",
    );
    sources.insert(
        "src/helper.hd",
        "# λ before the header\npub fn main() -> void: pass\npub fn label() -> string: \"kept\"\n",
    );
    sources.insert("src/unreached.hd", "pub fn main() -> void: pass\n");
    sources
}

#[test]
fn public_main_findings_use_the_lint_cache_without_poisoning_check_entries() {
    let sources = sources();
    let store = MemoryStore::default();
    let normal = checked(&sources, &store, &Packages::default(), false);
    assert!(!normal.diags.has_errors(), "{}", normal.render());
    assert!(normal.analysis_diags.is_empty());
    assert!(!normal.diags.code.contains(&Code::UnselectedMain));
    for warm in [false, true] {
        let out = checked(&sources, &store, &Packages::default(), true);
        assert!(!out.diags.has_errors(), "{}", out.render());
        assert_eq!(out.counters.modules_checked.is_empty(), warm);
        let findings = Policy::default().apply(&out.analysis_diags);
        assert_eq!(findings.len(), 2);
        let span = findings.primary[0];
        let source = &out.texts[span.file.idx()];
        assert_eq!(
            source[span.lo as usize..span.hi as usize].trim_end(),
            "pub fn main() -> void:"
        );
    }
    let following = checked(&sources, &store, &Packages::default(), false);
    assert!(following.counters.modules_checked.is_empty());
    assert!(following.analysis_diags.is_empty());
    assert!(!following.diags.code.contains(&Code::UnselectedMain));
}

#[test]
fn private_main_is_lint_only_and_library_private_mains_stay_quiet() {
    let mut sources = MemorySources::default();
    sources.insert(
        "src/main.hd",
        "use pkg.helper.value\nfn main() -> void:\n    _ := value()\n",
    );
    sources.insert(
        "src/helper.hd",
        "fn main() -> i32: +1\npub fn value() -> i32: main()\n",
    );
    let store = MemoryStore::default();
    let normal = checked(&sources, &store, &Packages::default(), false);
    assert!(!normal.diags.has_errors(), "{}", normal.render());
    assert!(!normal.diags.code.contains(&Code::PrivateMain));
    for warm in [false, true] {
        let out = checked(&sources, &store, &Packages::default(), true);
        assert!(!out.diags.has_errors(), "{}", out.render());
        assert_eq!(out.counters.modules_checked.is_empty(), warm);
        let findings = Policy::default().apply(&out.analysis_diags);
        assert_eq!(findings.code, [Code::PrivateMain]);
        assert_eq!(out.files[findings.primary[0].file.idx()], "src/main.hd");
        assert_eq!(findings.fixes[0].len, 1);
        let fix = findings.fixes[0].start as usize;
        assert_eq!(findings.fix_safety[fix], FixSafety::Exact);
        assert_eq!(
            findings.get_text(findings.fix_title[fix]),
            "make `main` public"
        );
        let edit = findings.fix_edits[fix].start as usize;
        assert_eq!(findings.edits[edit].span.lo, 21);
        assert_eq!(findings.get_text(findings.edits[edit].text), "pub ");
    }
}

#[test]
fn private_main_fix_is_a_suggestion_when_another_public_main_exists() {
    let mut sources = MemorySources::default();
    sources.insert("src/main.hd", "fn main() -> void:\n    pass\n");
    sources.insert("src/helper.hd", "pub fn main() -> void:\n    pass\n");
    let store = MemoryStore::default();
    let out = checked(
        &sources,
        &store,
        &Packages {
            entries: vec!["main".to_owned(), "helper".to_owned()],
            ..Packages::default()
        },
        true,
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    let findings = Policy::default().apply(&out.analysis_diags);
    assert_eq!(findings.code, [Code::PrivateMain]);
    let fix = findings.fixes[0].start as usize;
    assert_eq!(findings.fix_safety[fix], FixSafety::Suggestion);
}

#[test]
fn fetched_dependency_public_mains_never_become_root_advice() {
    let mut root = sources();
    root.insert(
        "src/main.hd",
        "use dep.util.value\npub fn main() -> void:\n    _ := value()\n",
    );
    let mut dependency = MemorySources::default();
    dependency.insert(
        "src/lib.hd",
        "pub fn main() -> void: pass\npub fn value() -> i32: +1\n",
    );
    let store = MemoryStore::default();
    let packages = Packages {
        requires: vec![("util".to_owned(), 1)],
        deps: vec![hd_driver::Dependency {
            name: "util".to_owned(),
            identity: "example/util@1".to_owned(),
            sources: &dependency,
            requires: vec![],
            fetched: true,
        }],
        ..Packages::default()
    };
    for _ in 0..2 {
        let out = checked(&root, &store, &packages, true);
        assert!(!out.diags.has_errors(), "{}", out.render());
        let findings = Policy::default().apply(&out.analysis_diags);
        assert_eq!(findings.len(), 2);
        assert!(
            findings
                .primary
                .iter()
                .all(|span| !out.files[span.file.idx()].starts_with('<'))
        );
    }
}

#[test]
fn reached_root_scope_and_named_entry_changes_are_reflected_on_warm_runs() {
    let sources = sources();
    let store = MemoryStore::default();
    let selected = Packages {
        check_roots: Some(vec!["src/main.hd".to_owned()]),
        ..Packages::default()
    };
    let out = checked(&sources, &store, &selected, true);
    assert!(!out.diags.has_errors(), "{}", out.render());
    let findings = Policy::default().apply(&out.analysis_diags);
    assert_eq!(findings.len(), 1);
    assert_eq!(out.files[findings.primary[0].file.idx()], "src/helper.hd");
    let named = checked(
        &sources,
        &store,
        &Packages {
            entries: vec!["main".to_owned(), "unreached".to_owned()],
            ..Packages::default()
        },
        true,
    );
    assert!(!named.diags.has_errors(), "{}", named.render());
    let findings = Policy::default().apply(&named.analysis_diags);
    assert_eq!(findings.len(), 1);
    assert_eq!(named.files[findings.primary[0].file.idx()], "src/helper.hd");
}
