//! A program that starts threads imports a shared heap, fills its static
//! data once from a start function and exports `hd.thread`; any other
//! program keeps its own unshared heap and active data
//! (`module.rt.thread.heap`).

use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_sched::SerialOrder;

const THREADS: &str = "use std.rt.alloc.alloc
use std.rt.memory.{atomic_add_u32, atomic_load_u32, store_u32}
use std.thread.{Scope, scope}

COUNTER := alloc(16)

fn add_one() -> void:
    _ := atomic_add_u32(COUNTER, 1)

pub fn main() -> void $ Console:
    store_u32(COUNTER, 0)
    scope(fn(s: &mut Scope) -> void: s.spawn(fn() -> void: add_one()).join())
    println(\"joined ${atomic_load_u32(COUNTER)}\")
";

/// `source` as std's `rt_tests.stage_case`, built at `level`.
fn wasm(source: &str, level: OptLevel) -> Vec<u8> {
    let mut sources = hd_driver::std_sources();
    sources.insert("rt_tests/stage_case.hd", source);
    let store = crate::warm_store();
    let host = Host {
        sources: &sources,
        store: &store,
        render_tir: &[],
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build_packages(
        &host,
        "std",
        &Packages {
            level,
            build_profile: if level == OptLevel::Dev {
                BuildProfile::Checked
            } else {
                BuildProfile::Release
            },
            std_leaf: Some("rt_tests/stage_case.hd"),
            ..Packages::default()
        },
        &Goal::Program {
            entry: "rt_tests.stage_case".into(),
        },
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    out.wasm.expect("wasm")
}

/// What the module's heap is: (imported shared, defined memories, start
/// function, passive segments, active segments, exports `hd.thread`).
fn heap(wasm: &[u8]) -> (bool, u32, bool, usize, usize, bool) {
    let (mut shared, mut defined, mut start, mut passive, mut active, mut thread) =
        (false, 0, false, 0, 0, false);
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::ImportSection(s) => {
                for i in s.into_imports() {
                    if let wasmparser::TypeRef::Memory(m) = i.expect("import").ty {
                        shared |= m.shared && m.maximum == Some(1 << 16) && m.initial == 1;
                    }
                }
            }
            wasmparser::Payload::MemorySection(s) => defined = s.count(),
            wasmparser::Payload::StartSection { .. } => start = true,
            wasmparser::Payload::DataSection(s) => {
                for d in s {
                    match d.expect("data").kind {
                        wasmparser::DataKind::Passive => passive += 1,
                        wasmparser::DataKind::Active { .. } => active += 1,
                    }
                }
            }
            wasmparser::Payload::ExportSection(s) => {
                thread = s
                    .into_iter()
                    .any(|e| e.expect("export").name == "hd.thread");
            }
            _ => {}
        }
    }
    (shared, defined, start, passive, active, thread)
}

#[test]
fn a_program_that_starts_threads_imports_a_shared_heap_it_fills_once() {
    for level in [OptLevel::Dev, OptLevel::Optimized] {
        let (shared, defined, start, passive, active, thread) = heap(&wasm(THREADS, level));
        // The exchange buffer stays each instance's own.
        assert!(shared && defined == 1 && start && thread, "{level:?}");
        assert!(passive > 0 && active == 0, "{level:?}");
    }
}

#[test]
fn a_program_that_never_starts_one_keeps_its_own_heap() {
    let single = THREADS.replace(
        "    scope(fn(s: &mut Scope) -> void: s.spawn(fn() -> void: add_one()).join())\n",
        "    add_one()\n",
    );
    assert_ne!(single, THREADS);
    let (shared, defined, start, passive, active, thread) =
        heap(&wasm(&single, OptLevel::Optimized));
    assert!(!shared && defined == 2 && !start && !thread);
    assert!(passive == 0 && active > 0);
}

/// The main thread starts one on Node, which adds to a word of the shared
/// heap through the program's own binding before `join` returns.
#[test]
fn a_thread_runs_its_work_on_node_before_join_returns() -> Result<(), String> {
    let path = hd_testkit::Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), "shared-heap.wasm");
    std::fs::write(&path, wasm(THREADS, OptLevel::Dev)).map_err(|e| e.to_string())?;
    let ran = hd_testkit::node::command()
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
        .arg(&path)
        .output()
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&ran.stdout);
    assert_eq!(
        stdout,
        "joined 1\n",
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    Ok(())
}
