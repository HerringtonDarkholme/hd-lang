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

/// The errors of a single-file program with this text.
#[track_caller]
fn single(text: &str) -> Vec<(Code, usize, usize)> {
    let out = analyze(&[("main.hd", text)]);
    errors(&out)
}

#[track_caller]
fn accepts(text: &str) {
    let out = analyze(&[("main.hd", text)]);
    assert!(!out.diags.has_errors(), "{text}\n{}", out.render());
}

/// The host capability traits that the prelude leaves out.
const HOST_USES: &str = "use std.console.ConsoleInput\nuse std.time.Clock\nuse std.fs.{FsRead, FsWrite}\nuse std.random.Random\nuse std.http.Http\nuse std.net.Net\nuse std.process.Process\nuse std.sys.Sys\nuse std.host.{Env, Args}\n\n";

const DATABASE: &str = "pub trait Database:\n    fn query(self) -> i32\n\n";
const HIDDEN: &str = "pub struct HiddenError: pass\n\n";

// module.entry.parameters

#[test]
fn a_parameter_is_an_error_at_the_name() {
    assert_eq!(
        single("pub fn main(args: List[string]) -> void:\n    pass\n"),
        [(Code::EntryPointParameters, 1, 8)]
    );
}

#[test]
fn a_generic_parameter_is_an_error_at_the_name() {
    assert_eq!(
        single("pub fn main[T]() -> void:\n    pass\n"),
        [(Code::EntryPointParameters, 1, 8)]
    );
}

#[test]
fn a_row_parameter_is_a_generic_parameter() {
    assert_eq!(
        single("pub fn main[$R]() -> void $ R:\n    pass\n"),
        [(Code::EntryPointParameters, 1, 8)]
    );
}

#[test]
fn a_suspending_entry_with_parameters_is_an_error() {
    assert_eq!(
        single("pub fn main!(code: i32) -> void:\n    pass\n"),
        [(Code::EntryPointParameters, 1, 8)]
    );
}

#[test]
fn a_parameter_is_the_only_error() {
    // The result and the row are not checked for a function that is no
    // entry point shape at all.
    let text = format!("{DATABASE}pub fn main(n: i32) -> i32 $ Database:\n    n\n");
    assert_eq!(single(&text), [(Code::EntryPointParameters, 4, 8)]);
}

#[test]
fn a_parameter_with_a_default_is_still_a_parameter() {
    assert_eq!(
        single("pub fn main(verbose: bool = false) -> void:\n    pass\n"),
        [(Code::EntryPointParameters, 1, 8)]
    );
}

// module.entry.row.host

#[test]
fn a_key_that_is_no_host_trait_is_an_error_at_the_key() {
    let text = format!("{DATABASE}pub fn main() -> void $ Database:\n    pass\n");
    assert_eq!(single(&text), [(Code::NonhostEntryRequirement, 4, 25)]);
}

#[test]
fn a_suspending_entry_row_is_checked_too() {
    let text = format!("{DATABASE}pub fn main!() -> void $ Database:\n    pass\n");
    assert_eq!(single(&text), [(Code::NonhostEntryRequirement, 4, 26)]);
}

#[test]
fn only_the_non_host_keys_are_errors() {
    let text = format!(
        "use std.time.Clock\n\n{DATABASE}pub fn main() -> void $ Console + Database + Clock:\n    pass\n"
    );
    assert_eq!(single(&text), [(Code::NonhostEntryRequirement, 6, 35)]);
}

#[test]
fn each_non_host_key_is_its_own_error() {
    let text = format!(
        "{DATABASE}pub trait Cache:\n    fn size(self) -> i32\n\npub fn main() -> void $ Database + Cache:\n    pass\n"
    );
    let found = single(&text);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().all(|e| e.0 == Code::NonhostEntryRequirement));
    assert!(found.iter().all(|e| e.1 == 7));
}

#[test]
fn the_host_traits_of_the_default_profile_are_allowed() {
    accepts(&format!(
        "{HOST_USES}pub fn main() -> void $ Console + ConsoleInput + Clock + FsRead + FsWrite + Random:\n    pass\n"
    ));
    accepts(&format!(
        "{HOST_USES}pub fn main!() -> void $ Console + Http + Net + Process + Sys + Env + Args:\n    pass\n"
    ));
}

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

#[test]
fn an_i32_result_is_an_error_at_the_result_type() {
    assert_eq!(
        single("pub fn main() -> i32:\n    7\n"),
        [(Code::UnsatisfiedTraitBound, 1, 18)]
    );
}

#[test]
fn a_string_result_is_an_error() {
    assert_eq!(
        single("pub fn main() -> string:\n    \"x\"\n"),
        [(Code::UnsatisfiedTraitBound, 1, 18)]
    );
}

#[test]
fn a_result_whose_error_is_not_error_is_an_error() {
    let text = format!(
        "{HIDDEN}pub fn main() -> Result[void, HiddenError]:\n    .Err(HiddenError {{}})\n"
    );
    assert_eq!(single(&text), [(Code::UnsatisfiedTraitBound, 3, 18)]);
}

#[test]
fn a_result_whose_value_is_not_termination_is_an_error() {
    assert_eq!(
        single("pub fn main() -> Result[string, string]:\n    .Ok(\"x\")\n"),
        [(Code::UnsatisfiedTraitBound, 1, 18)]
    );
    assert_eq!(
        single("pub fn main!() -> Result[i32, string]:\n    .Ok(1)\n"),
        [(Code::UnsatisfiedTraitBound, 1, 19)]
    );
}

#[test]
fn the_result_and_the_row_are_each_one_error() {
    let text = format!("{DATABASE}pub fn main() -> i32 $ Database:\n    7\n");
    let found = single(&text);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.contains(&(Code::NonhostEntryRequirement, 4, 24)));
    assert!(found.contains(&(Code::UnsatisfiedTraitBound, 4, 18)));
}

#[test]
fn the_result_of_a_valid_row_is_still_checked() {
    assert_eq!(
        single("pub fn main() -> i32 $ Console:\n    println(1)\n    7\n"),
        [(Code::UnsatisfiedTraitBound, 1, 18)]
    );
}

// The shapes that are entry points.

#[test]
fn void_is_an_entry_result() {
    accepts("pub fn main() -> void:\n    pass\n");
    accepts("pub fn main() -> void $ Console:\n    println(42)\n");
    accepts("pub fn main!() -> void $ Console:\n    println(42)\n");
}

#[test]
fn exit_code_is_an_entry_result() {
    accepts("use std.process.ExitCode\n\npub fn main() -> ExitCode:\n    ExitCode(7)\n");
}

#[test]
fn a_result_with_a_string_error_is_an_entry_result() {
    accepts("pub fn main() -> Result[void, string]:\n    .Err(\"oops\")\n");
    accepts(
        "use std.process.ExitCode\n\npub fn main() -> Result[ExitCode, string]:\n    .Ok(ExitCode(3))\n",
    );
    accepts("pub fn main!() -> Result[void, string]:\n    .Ok(())\n");
}

#[test]
fn a_result_of_a_result_is_an_entry_result() {
    accepts("pub fn main() -> Result[Result[void, string], string]:\n    .Ok(.Ok(()))\n");
}

#[test]
fn a_user_error_with_error_is_an_entry_error() {
    accepts(
        "use std.error.Error\n\npub struct Oops:\n    code: i32\n\nimpl Display for Oops:\n    fn to_string(self) -> string:\n        \"oops\"\n\nimpl Error for Oops\n\npub fn main() -> Result[void, Oops]:\n    .Err(Oops { code: 1 })\n",
    );
}

#[test]
fn a_user_error_with_only_display_is_not_an_entry_error() {
    assert_eq!(
        single(
            "pub struct Oops:\n    code: i32\n\nimpl Display for Oops:\n    fn to_string(self) -> string:\n        \"oops\"\n\npub fn main() -> Result[void, Oops]:\n    .Err(Oops { code: 1 })\n"
        ),
        [(Code::UnsatisfiedTraitBound, 8, 18)]
    );
}

#[test]
fn the_erased_error_satisfies_display() {
    accepts("use std.error.Error\n\npub fn main() -> Result[void, dyn Error]:\n    .Ok(())\n");
}

// Where the rules do not apply.

#[test]
fn a_private_main_is_no_entry_point() {
    let out = analyze(&[("main.hd", "fn main(n: i32) -> i32:\n    n\n")]);
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert!(
        out.diags.code.contains(&Code::PrivateMain),
        "{}",
        out.render()
    );
}

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
fn a_public_function_with_another_name_is_no_entry_point() {
    accepts("pub fn start(n: i32) -> i32:\n    n\n\npub fn main() -> void:\n    pass\n");
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

#[test]
fn a_named_single_file_is_an_entry_module() {
    let out = analyze(&[("tool.hd", "pub fn main(n: i32) -> void:\n    pass\n")]);
    assert_eq!(errors(&out), [(Code::EntryPointParameters, 1, 8)]);
}

// Goals and caches.

#[test]
fn building_a_bad_entry_stops_at_the_check() {
    for text in [
        "pub fn main(n: i32) -> void:\n    pass\n",
        "pub fn main() -> i32:\n    7\n",
        "pub trait Database:\n    fn query(self) -> i32\n\npub fn main() -> void $ Database:\n    pass\n",
    ] {
        let out = check(
            &[("main.hd", text)],
            &Goal::Program {
                entry: "main".into(),
            },
            &Packages::default(),
        );
        assert!(out.diags.has_errors(), "{text}");
        assert!(out.wasm.is_none(), "{text}");
        assert!(!out.render().contains("unsupported"), "{}", out.render());
        assert_eq!(errors(&out).len(), 1, "{}", out.render());
    }
}

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
