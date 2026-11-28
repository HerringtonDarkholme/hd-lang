//! Valid homogeneous frames of the `task_all_frame` intrinsic preserve owned
//! results, in dev and checked builds. A declaration without the
//! intrinsic's shape (`trait.impl.intrinsic.shape`) is the stage case
//! `tests/cases/diagnostics/a-frame-intrinsic-without-its-task-list-is-an-invalid-intrinsic`.

use hd_driver::{Executor, Goal, Host, NoClock, std_sources};
use hd_sched::SerialOrder;

#[test]
fn homogeneous_all_frame_keeps_typed_and_counted_results() {
    let mut sources = std_sources();
    sources.insert(
        "flat.hd",
        include_str!("programs/unlowered_intrinsic/flat.hd"),
    );
    sources.insert(
        "main.hd",
        include_str!("programs/unlowered_intrinsic/main.hd"),
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
        let ran = hd_testkit::node::command()
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
