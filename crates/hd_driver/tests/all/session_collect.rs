//! Session collection (repl-plan.md §2.2, job C2): the driver builds one
//! more module of a session. Collection stops at the instances earlier
//! modules export, which the module imports under their session keys
//! (`hd_wasm::func_key`), and leaves out the inits they ran.

use std::collections::BTreeMap;

use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, Session, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const IN1: &str = "\
start := +20

pub fn bump(x: i32) -> i32:
    x + start

seed := bump(+1)
";

const IN2: &str = "\
use pkg.in1.{bump}

pub fn twice(x: i32) -> i32:
    bump(bump(x))

println(twice(1))
";

/// Builds the script module `entry` over `files`, as one more module of
/// `session` when it is given.
fn built(files: &[(&str, &str)], entry: &str, session: Option<&Session>) -> Output {
    let mut src = MemorySources::default();
    for (path, text) in files {
        src.insert(path, text);
    }
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        session,
        ..Packages::default()
    };
    let out = build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: entry.into(),
        },
    );
    assert!(out.diags.is_empty(), "{entry}: {}", out.render());
    out
}

/// What a module holds: its imports as `module/name`, its exports by name
/// with their function indices, and its functions by debug name.
#[derive(Debug, Default)]
struct Shape {
    imports: Vec<String>,
    exports: BTreeMap<String, u32>,
    names: BTreeMap<String, u32>,
}

/// Validates `wasm` and reads its shape.
fn shape(wasm: &[u8]) -> Shape {
    wasmparser::Validator::new()
        .validate_all(wasm)
        .expect("the module validates");
    let mut out = Shape::default();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::ImportSection(r) => {
                for i in r.into_imports() {
                    let i = i.expect("import");
                    out.imports.push(format!("{}/{}", i.module, i.name));
                }
            }
            wasmparser::Payload::ExportSection(r) => {
                for e in r {
                    let e = e.expect("export");
                    out.exports.insert(e.name.to_owned(), e.index);
                }
            }
            wasmparser::Payload::CustomSection(c) => {
                if let wasmparser::KnownCustom::Name(r) = c.as_known() {
                    for n in r {
                        if let wasmparser::Name::Function(m) = n.expect("name") {
                            for f in m {
                                let f = f.expect("function name");
                                out.names.insert(f.name.to_owned(), f.index);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The session key a module exports its function `name` under.
fn key_of(s: &Shape, name: &str) -> String {
    let Some((_, &index)) = s.names.iter().find(|(n, _)| n.starts_with(name)) else {
        panic!("no function named {name}: {:?}", s.names);
    };
    let Some((key, _)) = s
        .exports
        .iter()
        .find(|(n, i)| **i == index && n.starts_with("inst."))
    else {
        panic!("{name} is not exported: {:?}", s.exports);
    };
    key.clone()
}

#[test]
fn a_session_module_imports_the_function_an_earlier_module_defined() {
    let mut session = Session::start();
    let first = built(&[("in1.hd", IN1)], "in1", Some(&session));
    let added = first.session_added.clone().expect("what input 1 adds");
    let s1 = shape(first.wasm.as_deref().expect("input 1's module"));
    let bump = key_of(&s1, "app/in1/bump#");
    let init = key_of(&s1, "app/in1/init.");
    assert!(added.inits_run.len() == 1, "{:?}", added.inits_run);
    session.add(added);

    let both = [("in1.hd", IN1), ("in2.hd", IN2)];
    let second = built(&both, "in2", Some(&session));
    let s2 = shape(second.wasm.as_deref().expect("input 2's module"));
    // Input 2 emits no instance of `bump`: it imports input 1's.
    assert!(
        !s2.names.keys().any(|n| n.starts_with("app/in1/")),
        "{:?}",
        s2.names
    );
    assert!(
        s2.imports
            .contains(&format!("{}/{bump}", hd_wasm::SESSION_MODULE)),
        "{:?}",
        s2.imports
    );
    assert!(!s2.exports.contains_key(&bump), "{:?}", s2.exports);
    // Input 1's init ran: input 2 neither runs nor imports it.
    assert!(
        !s2.imports.iter().any(|i| i.ends_with(&init)),
        "{:?}",
        s2.imports
    );
    let added = second.session_added.expect("what input 2 adds");
    assert!(added.inits_run.len() == 1, "{:?}", added.inits_run);
    assert!(!added.externs.first);
    // It exports what it defines, `twice` among them.
    let twice = key_of(&s2, "app/in2/twice#");
    assert!(s2.exports.contains_key(&twice));
    // The memory and the runtime globals are input 1's.
    assert!(
        s2.imports.contains(&format!(
            "{}/{}",
            hd_wasm::SESSION_MODULE,
            hd_wasm::MEMORY_KEY
        )),
        "{:?}",
        s2.imports
    );

    // Without a session, the same sources build a closed program.
    let program = built(&both, "in2", None);
    assert!(program.session_added.is_none());
    let p = shape(program.wasm.as_deref().expect("program"));
    assert!(p.names.keys().any(|n| n.starts_with("app/in1/bump#")));
    assert!(!p.imports.iter().any(|i| i.starts_with("hd.s/")));
}
