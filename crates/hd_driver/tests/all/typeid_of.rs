//! `TypeId::of::[T]()` (trait.typeid.of) and the qualified associated
//! calls with explicit type arguments it needs (`Type::f::[T]()`,
//! fn.generic.explicit.*, fn.ref.lookup): the owner resolves first, then the
//! member, then the written type arguments. Checked through the driver, then
//! built and run on V8 (`host/run.ts`), as the conformance runner does.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn run_named(package: &str, packages: &Packages<'_>, main: &str, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build_packages(&host, package, packages, goal)
}

/// The standard output of a program that must build and succeed.
fn output_in(name: &str, package: &str, packages: &Packages<'_>, main: &str) -> String {
    let out = run_named(
        package,
        packages,
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("typeid-of-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout).expect("UTF-8")
}

const NAMES: &str = "\
use std.inspect.TypeId

struct User:
    name: string

pub fn main() -> void $ Console:
    println(\"${TypeId::of::[User]()}\")
    println(\"${TypeId::of::[User?]()}\")
    println(\"${TypeId::of::[List[User]]()}\")
    println(\"${TypeId::of::[(User, i32)]()}\")
    println(\"${TypeId::of::[Map[string, User?]]()}\")
";

#[test]
fn a_package_prints_its_name_with_underscores() {
    assert_eq!(
        output_in("package", "acme-shop", &Packages::default(), NAMES),
        "acme_shop.main.User\nacme_shop.main.User?\nList[acme_shop.main.User]\n\
         (acme_shop.main.User, i32)\nMap[string, acme_shop.main.User?]\n"
    );
}

/// A single-file program prints its file's stem, then the name
/// (trait.typeid.name.single-file).
#[test]
fn a_single_file_program_prints_its_stem() {
    let packages = Packages {
        single_file: true,
        ..Packages::default()
    };
    assert_eq!(
        output_in("single-file", "app", &packages, NAMES),
        "main.User\nmain.User?\nList[main.User]\n(main.User, i32)\n\
         Map[string, main.User?]\n"
    );
}
