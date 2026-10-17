//! A suspending body's frame holds only the locals a resume reads
//! (`crates/hd_wasm/DESIGN.md` §14.2, item 1): a value dead before every
//! wait gets no field, so the frame does not keep it alive.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// `main!` with `before` ahead of a wait that `after` follows.
fn program(before: &str, after: &str) -> String {
    format!(
        "use std.time.{{Clock, Duration, sleep}}\n\n\
         pub fn main!() -> void $ Console + Clock:\n\
         {before}    sleep!(Duration::milliseconds(1))\n\
         {after}    println(\"done\")\n"
    )
}

/// The number of fields of `main!`'s frame: the struct its body casts its
/// frame parameter to.
fn frame_fields(main: &str) -> usize {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
        render_tir: &[],
    };
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let out = build(&host, "app", &goal);
    let Some(wasm) = &out.wasm else {
        panic!("{}", out.render())
    };
    let mut structs = Vec::new();
    let mut casts = Vec::new();
    let mut names = std::collections::BTreeMap::new();
    for p in wasmparser::Parser::new(0).parse_all(wasm) {
        match p.expect("valid module") {
            wasmparser::Payload::TypeSection(r) => {
                for g in r {
                    for t in g.expect("rec group").into_types() {
                        structs.push(match t.composite_type.inner {
                            wasmparser::CompositeInnerType::Struct(s) => Some(s.fields.len()),
                            _ => None,
                        });
                    }
                }
            }
            wasmparser::Payload::CodeSectionEntry(body) => {
                let mut ops = body.get_operators_reader().expect("operators");
                let mut first = None;
                while !ops.eof() && first.is_none() {
                    if let wasmparser::Operator::RefCastNullable {
                        hty: wasmparser::HeapType::Concrete(t),
                    } = ops.read().expect("op")
                    {
                        first = t.as_module_index();
                    }
                }
                casts.push(first);
            }
            wasmparser::Payload::CustomSection(c) if c.name() == "name" => {
                let r = wasmparser::NameSectionReader::new(wasmparser::BinaryReader::new(
                    c.data(),
                    c.data_offset(),
                ));
                for n in r {
                    if let Ok(wasmparser::Name::Function(m)) = n {
                        for x in m {
                            let x = x.expect("name");
                            names.insert(x.index as usize, x.name.to_owned());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let imports = names.len() - casts.len();
    let (i, _) = names
        .iter()
        .find(|(_, n)| n.starts_with("app/main/main") && n.ends_with("$Body"))
        .unwrap_or_else(|| panic!("no body part in {names:?}"));
    let t = casts[i - imports].expect("the body casts its frame") as usize;
    structs[t].expect("the frame is a struct")
}

#[test]
fn a_list_dead_before_the_wait_takes_no_frame_field() {
    let plain = frame_fields(&program("", ""));
    // Built and read before the wait, never after.
    let dead = frame_fields(&program(
        "    let xs = [1, 2, 3, 4, 5, 6, 7, 8]\n    println(xs.len())\n",
        "",
    ));
    assert_eq!(dead, plain, "a dead list adds no field");
    // Read after the wait, it is saved.
    let live = frame_fields(&program(
        "    let xs = [1, 2, 3, 4, 5, 6, 7, 8]\n",
        "    println(xs.len())\n",
    ));
    assert!(live > plain, "{live} fields, {plain} without the list");
}
