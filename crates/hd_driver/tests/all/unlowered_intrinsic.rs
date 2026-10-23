//! Malformed frame calls fail at build time rather than compiling their
//! placeholder bodies; valid homogeneous frames preserve owned results.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build, std_sources};
use hd_sched::SerialOrder;

#[test]
fn a_frame_intrinsic_without_its_task_list_fails_at_build_time() {
    let mut sources = std_sources();
    sources.insert(
        "bad.hd",
        r#"@intrinsic("task_all_frame")
pub fn unlowered() -> void:
    panic("intrinsic")
"#,
    );
    sources.insert(
        "main.hd",
        r"use std.bad.unlowered

pub fn main() -> void:
    unlowered()
",
    );
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(
        &host,
        "std",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    let rendered = out.render();
    assert!(out.wasm.is_none(), "the program compiled to Wasm");
    assert!(out.diags.code.contains(&Code::Unsupported), "{rendered}");
    assert!(
        rendered.contains("the intrinsic `task_all_frame`"),
        "{rendered}"
    );
}

#[test]
fn homogeneous_all_frame_keeps_typed_and_counted_results() {
    let mut sources = std_sources();
    sources.insert(
        "flat.hd",
        r#"@intrinsic("task_all_frame")
pub fn frame[T](tasks: List[mut Suspend[T]]) -> mut Suspend[List[T]]:
    panic("intrinsic")

@intrinsic("task_pending_frame")
pub fn pending[T]() -> mut Suspend[T]:
    panic("intrinsic")
"#,
    );
    sources.insert(
        "main.hd",
        r#"use std.flat.{frame, pending}
use std.task.race
enum Item:
    Text(string)
    Count(i64)

struct Cell:
    value: string

fn ready![T](value: T) -> T:
    value

fn check(ok: bool) -> void:
    if !ok:
        panic("typed all-frame result differs")

fn discard![T](task: mut Suspend[List[T]]) -> usize:
    values := task!()
    values.len()

pub fn main!() -> void $ Console:
    let strings: List[mut Suspend[List[string]]] = [ready(["one", "two"]), ready(["three"])]
    let strings_frame: mut Suspend[List[List[string]]] = frame(strings)
    values := strings_frame!()
    check(values.len() == 2 && values[0][0] == "one" && values[0][1] == "two" && values[1][0] == "three")
    let variants: List[mut Suspend[Item]] = [ready(Item.Text("text")), ready(Item.Count(-7i64))]
    let variants_frame: mut Suspend[List[Item]] = frame(variants)
    variants_out := variants_frame!()
    match variants_out[0]:
        .Text(text) => check(text == "text")
        .Count(_) => panic("expected text")
    match variants_out[1]:
        .Count(count) => check(count == -7i64)
        .Text(_) => panic("expected count")
    left := &mut Cell { value: "left" }
    right := &mut Cell { value: "right" }
    let references: List[mut Suspend[&mut Cell]] = [ready(left), ready(right)]
    let reference_frame: mut Suspend[List[&mut Cell]] = frame(references)
    shared := reference_frame!()
    check(shared[0] is left)
    check(shared[1] is right)
    shared[0].value = "changed"
    check(left.value == "changed")
    let partial_values: List[mut Suspend[List[string]]] = [ready(["owned", "result"]), pending()]
    let partial_frame: mut Suspend[List[List[string]]] = frame(partial_values)
    _ := race!(discard(partial_frame), ready(0usize))
    let partial_refs: List[mut Suspend[&mut Cell]] = [ready(left), pending()]
    let partial_reference_frame: mut Suspend[List[&mut Cell]] = frame(partial_refs)
    _ := race!(discard(partial_reference_frame), ready(0usize))
    check(left.value == "changed")
    println("typed flat results")
"#,
    );
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    for checked in [false, true] {
        let out = hd_driver::build_packages(
            &host,
            "std",
            &hd_driver::Packages {
                checked,
                ..hd_driver::Packages::default()
            },
            &Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(out.diags.is_empty(), "{}", out.render());
        let wasm = out.wasm.expect("typed flat-frame Wasm");
        let path = hd_testkit::Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), "flat-all-typed.wasm");
        std::fs::write(&path, &wasm).expect("write Wasm");
        let ran = std::process::Command::new("node")
            .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
            .arg(&path)
            .output()
            .expect("Node");
        let _ = std::fs::remove_file(&path);
        assert!(
            ran.status.success(),
            "checked={checked}: {}",
            String::from_utf8_lossy(&ran.stderr)
        );
        assert_eq!(
            String::from_utf8(ran.stdout).expect("UTF-8"),
            "typed flat results\n"
        );
        #[cfg(feature = "wasmtime")]
        {
            let setup = hd_run::HostSetup {
                providers: Vec::new(),
                grants: hd_run::Grants::default(),
                limits: hd_run::Limits::default(),
                args: Vec::new(),
                program: String::new(),
            };
            let config = hd_run_wasmtime::EngineConfig::pipeline(hd_driver::OptLevel::Dev);
            let (ran, _) =
                hd_run_wasmtime::run_captured_reading(config, &wasm, &setup, "hd.all_polls")
                    .expect("native typed flat-frame run");
            assert_eq!(
                ran.status,
                0,
                "checked={checked}: {}",
                String::from_utf8_lossy(&ran.stderr)
            );
            assert_eq!(ran.stdout, b"typed flat results\n");
        }
    }
}
