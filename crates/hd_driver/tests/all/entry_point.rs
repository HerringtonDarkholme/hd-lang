//! `module.entry.*`: the entry point of an entry module is validated when
//! the module is checked. A public `main` or `main!` takes no parameters
//! and no generic parameters (`entry-point-parameters`), every key of its
//! requirement row is a host capability trait of the runtime profile
//! (`nonhost-entry-requirement`), and its result implements
//! `std.process.Termination` (`unsatisfied-trait-bound`). Each problem is
//! one error at the name, the offending key or the result type, with no
//! error after it. A function that is not an entry point owes nothing.

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn check(files: &[(&str, &str)], goal: &Goal, packages: &Packages<'_>) -> Output {
    let mut src = MemorySources::default();
    for (name, text) in files {
        src.insert(name, text);
    }
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build_packages(&host, "app", packages, goal)
}

fn analyze(files: &[(&str, &str)]) -> Output {
    check(files, &Goal::Analyze, &Packages::default())
}

/// The `(code, line, column)` of each error, in source order.
fn errors(out: &Output) -> Vec<(Code, usize, usize)> {
    out.diags
        .content_order()
        .into_iter()
        .filter(|i| out.diags.severity[*i] == hd_diag::Severity::Error)
        .map(|i| {
            let (_, line, column) = out.locate(out.diags.primary[i]);
            (out.diags.code[i], line, column)
        })
        .collect()
}

// module.entry.parameters

// module.entry.row.host

#[test]
fn a_host_trait_the_profile_leaves_out_is_an_error() {
    let packages = Packages {
        profile: Some(vec!["std.console.Console".to_owned()]),
        ..Packages::default()
    };
    let text = "use std.time.Clock\n\npub fn main() -> void $ Console + Clock:\n    pass\n";
    let out = check(&[("main.hd", text)], &Goal::Analyze, &packages);
    assert_eq!(errors(&out), [(Code::NonhostEntryRequirement, 3, 35)]);
    let text = "pub fn main() -> void $ Console:\n    pass\n";
    let out = check(&[("main.hd", text)], &Goal::Analyze, &packages);
    assert!(!out.diags.has_errors(), "{}", out.render());
}

// module.entry.result-termination

// The shapes that are entry points.

// Where the rules do not apply.

#[test]
fn a_library_module_may_have_any_main() {
    let files = [
        ("src/lib.hd", "pub fn main(n: i32) -> i32:\n    n\n"),
        ("src/util.hd", "pub fn main[T](x: T) -> T:\n    x\n"),
    ];
    let out = analyze(&files);
    assert!(!out.diags.has_errors(), "{}", out.render());
}

#[test]
fn a_test_module_may_have_any_main() {
    let files = [
        ("src/lib.hd", "pub fn one() -> i32:\n    1\n"),
        ("src/lib_test.hd", "pub fn main(n: i32) -> i32:\n    n\n"),
    ];
    let out = check(&files, &Goal::CheckTests, &Packages::default());
    assert!(!out.diags.has_errors(), "{}", out.render());
}

// Packages.

#[test]
fn the_main_file_of_a_package_is_an_entry_module() {
    let files = [
        ("src/lib.hd", "pub fn one() -> i32:\n    1\n"),
        ("src/main.hd", "pub fn main(n: i32) -> void:\n    pass\n"),
    ];
    let out = analyze(&files);
    assert_eq!(errors(&out), [(Code::EntryPointParameters, 1, 8)]);
    let (file, _, _) = out.locate(out.diags.primary[0]);
    assert!(file.ends_with("src/main.hd"), "{file}");
}

#[test]
fn an_executable_module_is_an_entry_module() {
    let files = [
        ("src/lib.hd", "pub fn one() -> i32:\n    1\n"),
        ("src/tool.hd", "pub fn main() -> i32:\n    1\n"),
    ];
    let named = Packages {
        entries: vec!["tool".to_owned()],
        ..Packages::default()
    };
    let out = check(&files, &Goal::Analyze, &named);
    assert_eq!(errors(&out), [(Code::UnsatisfiedTraitBound, 1, 18)]);
    // Unnamed, the module is a library module and `main` an ordinary function.
    let out = analyze(&files);
    assert!(!out.diags.has_errors(), "{}", out.render());
}

// Goals and caches.

#[test]
fn a_test_run_reports_the_entry_too() {
    let out = check(
        &[(
            "main.hd",
            "pub fn main(n: i32) -> void:\n    pass\n\ntests:\n    it(\"runs\"):\n        pass\n",
        )],
        &Goal::Tests {
            module: None,
            filter: None,
        },
        &Packages::default(),
    );
    assert_eq!(errors(&out), [(Code::EntryPointParameters, 1, 8)]);
}

#[test]
fn a_warm_cache_reports_the_same_errors() {
    let store = MemoryStore::default();
    let mut src = MemorySources::default();
    src.insert("main.hd", "pub fn main() -> i32:\n    7\n");
    let run = |store: &MemoryStore| {
        let host = Host {
            render_tir: &[],
            sources: &src,
            store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        build_packages(&host, "app", &Packages::default(), &Goal::Analyze)
    };
    let cold = errors(&run(&store));
    let warm = errors(&run(&store));
    assert_eq!(cold, [(Code::UnsatisfiedTraitBound, 1, 18)]);
    assert_eq!(warm, cold);
}

#[test]
fn naming_an_executable_changes_the_cached_check() {
    let store = MemoryStore::default();
    let mut src = MemorySources::default();
    src.insert("src/lib.hd", "pub fn one() -> i32:\n    1\n");
    src.insert("src/tool.hd", "pub fn main() -> i32:\n    1\n");
    let run = |entries: Vec<String>| {
        let host = Host {
            render_tir: &[],
            sources: &src,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        let packages = Packages {
            entries,
            ..Packages::default()
        };
        build_packages(&host, "app", &packages, &Goal::Analyze)
    };
    assert!(errors(&run(Vec::new())).is_empty());
    assert_eq!(errors(&run(vec!["tool".to_owned()])).len(), 1);
    assert!(errors(&run(Vec::new())).is_empty());
}
