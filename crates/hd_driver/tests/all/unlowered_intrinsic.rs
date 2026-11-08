//! Valid homogeneous frames preserve owned results. (A malformed std
//! intrinsic declaration is the checker's to reject: #1524.)

use hd_driver::{Executor, Goal, Host, NoClock, std_sources};
use hd_sched::SerialOrder;

#[test]
fn homogeneous_all_frame_keeps_typed_and_counted_results() {
    let mut sources = std_sources();
    sources.insert(
        "flat.hd",
        r#"@intrinsic("task_all_frame")
pub fn frame[T](tasks: List[Suspend[T]]) -> Suspend[List[T]]:
    panic("intrinsic")

@intrinsic("task_pending_frame")
pub fn pending[T]() -> Suspend[T]:
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

fn discard![T](task: Suspend[List[T]]) -> usize:
    values := task!()
    values.len()

pub fn main!() -> void $ Console:
    let strings: List[Suspend[List[string]]] = [ready(["one", "two"]), ready(["three"])]
    let strings_frame: Suspend[List[List[string]]] = frame(strings)
    values := strings_frame!()
    check(values.len() == 2 && values[0][0] == "one" && values[0][1] == "two" && values[1][0] == "three")
    let variants: List[Suspend[Item]] = [ready(Item.Text("text")), ready(Item.Count(-7i64))]
    let variants_frame: Suspend[List[Item]] = frame(variants)
    variants_out := variants_frame!()
    match variants_out[0]:
        .Text(text) => check(text == "text")
        .Count(_) => panic("expected text")
    match variants_out[1]:
        .Count(count) => check(count == -7i64)
        .Text(_) => panic("expected count")
    left := box(Cell { value: "left" })
    right := box(Cell { value: "right" })
    let references: List[Suspend[*mut Cell]] = [ready(left), ready(right)]
    let reference_frame: Suspend[List[*mut Cell]] = frame(references)
    shared := reference_frame!()
    check(shared[0] is left)
    check(shared[1] is right)
    shared[0].value = "changed"
    check(left.value == "changed")
    let partial_values: List[Suspend[List[string]]] = [ready(["owned", "result"]), pending()]
    let partial_frame: Suspend[List[List[string]]] = frame(partial_values)
    _ := race!(discard(partial_frame), ready(0usize))
    let partial_refs: List[Suspend[*mut Cell]] = [ready(left), pending()]
    let partial_reference_frame: Suspend[List[*mut Cell]] = frame(partial_refs)
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
            .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
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
