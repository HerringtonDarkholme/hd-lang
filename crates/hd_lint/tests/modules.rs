use hd_base::{FileId, Span};
use hd_cache::MemoryStore;
use hd_diag::{Code, Severity};
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_lint::{Level, Policy, UNSELECTED_MAIN, analyze_modules};
use hd_project::{MainSite, MemorySources, Role};
use hd_sched::SerialOrder;

const MESSAGE: &str = "this public `main` is an ordinary function, since no executable names its module; name it in an `[[executable]]` table of `hd.toml` to run it";

#[test]
fn module_roles_and_executable_membership_select_advice_with_exact_spans() {
    let span = Span {
        file: FileId::from_raw(7),
        lo: 11,
        hi: 31,
    };
    for role in [Role::Lib, Role::Exe, Role::Test, Role::Task] {
        for source_root in [false, true] {
            for entry in [false, true] {
                let findings = analyze_modules(
                    &[MainSite {
                        span,
                        role,
                        entry,
                        source_root,
                    }],
                    Policy::default(),
                );
                let expected = source_root && role == Role::Lib && !entry;
                assert_eq!(findings.len(), usize::from(expected));
                if expected {
                    assert_eq!(findings.code, [Code::UnselectedMain]);
                    assert_eq!(findings.primary, [span]);
                    assert_eq!(findings.get_text(findings.message[0]), MESSAGE);
                    assert_eq!(findings.fixes[0].len, 0);
                }
            }
        }
    }
}

#[test]
fn allow_warn_and_deny_are_independent_of_other_advice() {
    let site = MainSite {
        span: Span {
            file: FileId::from_raw(0),
            lo: 0,
            hi: 20,
        },
        role: Role::Lib,
        entry: false,
        source_root: true,
    };
    for (level, expected) in [
        (Level::Allow, None),
        (Level::Warn, Some(Severity::Warning)),
        (Level::Deny, Some(Severity::Error)),
    ] {
        let mut policy = Policy::default();
        policy.set(UNSELECTED_MAIN, level).unwrap();
        policy.set(hd_lint::UNUSED_BINDING, Level::Deny).unwrap();
        let findings = analyze_modules(&[site], policy);
        assert_eq!(findings.severity.first().copied(), expected);
    }
}

fn checked(sources: &MemorySources, store: &MemoryStore, packages: &Packages<'_>) -> Output {
    build_packages(
        &Host {
            render_tir: &[],
            sources,
            store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        },
        "app",
        packages,
        &Goal::Analyze,
    )
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
fn public_main_facts_do_not_require_checked_body_reruns_or_poison_warm_checks() {
    let sources = sources();
    let store = MemoryStore::default();
    let normal = checked(&sources, &store, &Packages::default());
    assert!(!normal.diags.has_errors(), "{}", normal.render());
    assert!(normal.main_sites.is_empty());
    assert!(!normal.diags.code.contains(&Code::UnselectedMain));
    for _ in 0..2 {
        let out = checked(
            &sources,
            &store,
            &Packages {
                collect_main_sites: true,
                ..Packages::default()
            },
        );
        assert!(!out.diags.has_errors(), "{}", out.render());
        assert!(
            out.counters.modules_checked.is_empty(),
            "module facts must survive true Check hits"
        );
        let findings = analyze_modules(&out.main_sites, Policy::default());
        assert_eq!(findings.len(), 2);
        let span = findings.primary[0];
        let source = &out.texts[span.file.idx()];
        assert_eq!(
            source[span.lo as usize..span.hi as usize].trim_end(),
            "pub fn main() -> void:"
        );
    }
    let following = checked(&sources, &store, &Packages::default());
    assert!(following.counters.modules_checked.is_empty());
    assert!(following.main_sites.is_empty());
    assert!(!following.diags.code.contains(&Code::UnselectedMain));
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
        collect_main_sites: true,
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
        let out = checked(&root, &store, &packages);
        assert!(!out.diags.has_errors(), "{}", out.render());
        assert!(
            out.main_sites
                .iter()
                .all(|site| !out.files[site.span.file.idx()].starts_with('<'))
        );
        assert_eq!(analyze_modules(&out.main_sites, Policy::default()).len(), 2);
    }
}

#[test]
fn reached_root_scope_and_named_entry_changes_are_reflected_on_warm_runs() {
    let sources = sources();
    let store = MemoryStore::default();
    let selected = Packages {
        collect_main_sites: true,
        check_roots: Some(vec!["src/main.hd".to_owned()]),
        ..Packages::default()
    };
    let out = checked(&sources, &store, &selected);
    assert!(!out.diags.has_errors(), "{}", out.render());
    let findings = analyze_modules(&out.main_sites, Policy::default());
    assert_eq!(findings.len(), 1);
    assert_eq!(out.files[findings.primary[0].file.idx()], "src/helper.hd");
    let named = checked(
        &sources,
        &store,
        &Packages {
            collect_main_sites: true,
            entries: vec!["main".to_owned(), "unreached".to_owned()],
            ..Packages::default()
        },
    );
    assert!(!named.diags.has_errors(), "{}", named.render());
    let findings = analyze_modules(&named.main_sites, Policy::default());
    assert_eq!(findings.len(), 1);
    assert_eq!(named.files[findings.primary[0].file.idx()], "src/helper.hd");
}
