//! Development fixture emission has no executable entry. Real Wasm bodies,
//! including uncalled declarations and closures, survive link pruning.

use hd_driver::{Executor, Goal, Host, NoClock, OptLevel, Output, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use std::collections::BTreeMap;
use std::fmt::Write as _;

fn built(text: &str, level: OptLevel, goal: &Goal) -> Output {
    built_packages(
        text,
        &Packages {
            level,
            ..Packages::default()
        },
        goal,
    )
}

fn built_packages(text: &str, packages: &Packages<'_>, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", text);
    let store = crate::warm_store();
    let host = Host {
        sources: &src,
        store: &store,
        render_tir: &[],
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build_packages(&host, "app", packages, goal)
}

fn fixture(text: &str, level: OptLevel) -> Output {
    let out = built(
        text,
        level,
        &Goal::Fixture {
            module: "main".into(),
        },
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert!(out.wasm.is_some(), "{}", out.render());
    out
}

/// Names are tied to actual decoded code bodies; a stray name section entry
/// alone cannot satisfy an emission regression. Validate all signatures too.
fn bodies(out: &Output) -> BTreeMap<String, Vec<String>> {
    let wasm = out.wasm.as_deref().expect("wasm");
    wasmparser::Validator::new()
        .validate_all(wasm)
        .expect("valid Wasm");
    let mut names = BTreeMap::new();
    let mut code = Vec::new();
    let mut imports = 0;
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::ImportSection(r) => {
                for i in r.into_imports() {
                    if matches!(i.expect("import").ty, wasmparser::TypeRef::Func(_)) {
                        imports += 1;
                    }
                }
            }
            wasmparser::Payload::CustomSection(c) => {
                if let wasmparser::KnownCustom::Name(r) = c.as_known() {
                    for n in r {
                        if let wasmparser::Name::Function(m) = n.expect("name") {
                            for f in m {
                                let f = f.expect("function name");
                                names.insert(f.index, f.name.to_owned());
                            }
                        }
                    }
                }
            }
            wasmparser::Payload::CodeSectionEntry(body) => {
                code.push(
                    body.get_operators_reader()
                        .expect("operators")
                        .into_iter()
                        .map(|op| format!("{:?}", op.expect("operator")))
                        .collect(),
                );
            }
            wasmparser::Payload::ExportSection(r) => {
                for e in r {
                    let e = e.expect("export");
                    assert_ne!(
                        e.kind,
                        wasmparser::ExternalKind::Func,
                        "fixture exported {}",
                        e.name
                    );
                }
            }
            _ => {}
        }
    }
    code.into_iter()
        .enumerate()
        .map(|(i, code)| {
            let index = u32::try_from(i).expect("function index") + imports;
            (
                names.remove(&index).expect("defined function has a name"),
                code,
            )
        })
        .collect()
}

#[test]
fn uncalled_functions_and_methods_have_real_code_without_an_entry() {
    let text = "\
pub struct Point:
    x: i32

impl Point:
    fn next(self) -> i32: self.x + 1

fn f(x: i32) -> i32: x + 7

pub fn main(x: i32) -> Point: Point { x: x }
";
    for level in [OptLevel::Dev, OptLevel::Optimized] {
        let out = fixture(text, level);
        let bodies = bodies(&out);
        assert!(
            bodies["app/main/f#0"]
                .iter()
                .any(|op| matches!(op.as_str(), "I32Add" | "I64Add")),
            "{bodies:?}"
        );
        for part in ["next#0", "main#0"] {
            assert!(
                bodies.keys().any(|n| n.ends_with(part)),
                "{part}: {bodies:?}"
            );
        }
        assert!(out.fixture_notes.is_empty(), "{:?}", out.fixture_notes);
    }
}

#[test]
fn unused_closures_and_named_generic_instances_survive_folding() {
    let text = "\
fn identity[T](value: T, n: i32) -> T:
    if n > 0: identity(value, n - 1) else: value
fn unused[T](value: T) -> T: value

fn f(x: i32) -> i32:
    add := fn(y: i32) -> i32: x + y
    fn local[T](value: T) -> T: value
    if false:
        identity(local(+3), 0)
    x
";
    for level in [OptLevel::Dev, OptLevel::Optimized] {
        let out = fixture(text, level);
        let bodies = bodies(&out);
        for part in ["app/main/f#1", "app/main/f#2", "app/main/identity#0"] {
            assert!(bodies.contains_key(part), "{part}: {bodies:?}");
        }
        assert_eq!(
            out.fixture_notes,
            ["skipped app/main/unused: no concrete instantiation named by the case"]
        );
    }
}

#[test]
fn empty_and_uninstantiated_generic_cases_emit_with_sorted_notes() {
    for text in [
        "",
        "struct Point:\n    x: i32\n",
        "fn z[T](x: T) -> T: x\nfn a[T](x: T) -> T: x\n",
    ] {
        let out = fixture(text, OptLevel::Dev);
        let _ = bodies(&out);
        let warm = fixture(text, OptLevel::Dev);
        assert_eq!(warm.fixture_notes, out.fixture_notes, "warm fixture notes");
        assert_eq!(warm.wasm, out.wasm, "warm fixture emission");
        if text.starts_with("fn z") {
            assert_eq!(
                out.fixture_notes,
                [
                    "skipped app/main/a: no concrete instantiation named by the case",
                    "skipped app/main/z: no concrete instantiation named by the case",
                ]
            );
        } else {
            assert!(out.fixture_notes.is_empty());
        }
    }
}

#[test]
fn declared_capabilities_are_parameters_without_host_boundary_codecs() {
    let out = fixture(
        "\
struct Token:
    value: i32
trait Service:
    fn query(self, value: Token) -> Token
fn get(value: Token) -> Token $ Service:
    $.use(Service).query(value)
",
        OptLevel::Dev,
    );
    assert!(bodies(&out).contains_key("app/main/get#0"));
}

#[test]
fn wide_signatures_lower_internally_without_export_limits() {
    let mut text = String::from("struct Wide:\n");
    for i in 0..40 {
        writeln!(text, "    f{i}: string").expect("write declaration");
    }
    text.push_str(
        "\nfn first(value: Wide) -> string: value.f0\nfn identity(value: Wide) -> Wide: value\n",
    );
    let out = fixture(&text, OptLevel::Dev);
    let bodies = bodies(&out);
    assert!(bodies.contains_key("app/main/first#0"));
    assert!(bodies.contains_key("app/main/identity#0"));
}

#[test]
fn fixture_cache_cannot_change_executable_entry_validation() {
    let text = "pub fn main(x: i32) -> i32: x\n";
    let _ = fixture(text, OptLevel::Dev);
    let program = built(
        text,
        OptLevel::Dev,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(
        program.render().contains("entry-point-parameters"),
        "{}",
        program.render()
    );
    assert!(program.wasm.is_none());
    let _ = fixture(text, OptLevel::Dev);
}

#[test]
fn uncalled_suspending_functions_keep_every_generated_part() {
    let out = fixture("fn ready!() -> i32: 42\n", OptLevel::Optimized);
    let bodies = bodies(&out);
    for name in [
        "app/main/ready#0",
        "app/main/ready#0.poll",
        "app/main/ready#0.cancel",
    ] {
        assert!(bodies.contains_key(name), "{name}: {bodies:?}");
    }
}

#[test]
fn generic_impl_methods_and_local_functions_are_not_rooted_without_types() {
    let out = fixture(
        "\
struct Box[T]:
    value: T
impl[T] Box[T]:
    fn read(self) -> T: self.value
fn wrapper(x: i32) -> i32:
    fn unused[T](value: T) -> T: value
    x
",
        OptLevel::Dev,
    );
    let bodies = bodies(&out);
    assert!(bodies.contains_key("app/main/wrapper#0"));
    assert_eq!(out.fixture_notes.len(), 2, "{:?}", out.fixture_notes);
    assert!(
        out.fixture_notes
            .iter()
            .any(|n| n.contains("Box") && n.contains(".read:")),
        "{:?}",
        out.fixture_notes
    );
    assert!(
        out.fixture_notes.iter().any(|n| n.contains("wrapper#1")),
        "{:?}",
        out.fixture_notes
    );
}

#[test]
fn uncalled_fixture_bodies_keep_origins_and_notes_on_warm_builds() {
    let source = "fn f(x: i32) -> i32: x + 7\nfn unused[T](x: T) -> T: x\n";
    let goal = Goal::Fixture {
        module: "main".into(),
    };
    for level in [OptLevel::Dev, OptLevel::Optimized] {
        let plain = built(source, level, &goal);
        assert!(!plain.diags.has_errors(), "{}", plain.render());
        for _ in 0..2 {
            let mapped = built_packages(
                source,
                &Packages {
                    level,
                    instruction_origins: true,
                    ..Packages::default()
                },
                &goal,
            );
            assert!(!mapped.diags.has_errors(), "{}", mapped.render());
            assert_eq!(
                mapped.wasm, plain.wasm,
                "mapping must preserve fixture bytes"
            );
            assert_eq!(mapped.fixture_notes, plain.fixture_notes);
            assert_eq!(mapped.fixture_notes.len(), 1);
            let f = mapped
                .instruction_origins
                .iter()
                .find(|f| f.name == "app/main/f#0")
                .expect("uncalled fixture function has an origin map");
            assert_eq!(f.item.as_deref(), Some("app/main/f"));
            assert!(!f.instructions.is_empty());
            assert!(f.instructions.iter().any(|i| i.origin.is_some()));
        }
    }
}
