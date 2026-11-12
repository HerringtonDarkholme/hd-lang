//! Native function-value transfers use the ordinary owned Drop path.

#[test]
fn native_callback_inputs_transfer_hooks_and_conditional_fields_once() -> Result<(), String> {
    use hd_driver::{
        BuildProfile, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages,
    };
    use hd_sched::SerialOrder;
    let source = include_str!("../cases/counts/function-value-native-argument-transfer.hd");
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
    for (level, checked) in [
        (OptLevel::Dev, false),
        (OptLevel::Dev, true),
        (OptLevel::Optimized, false),
        (OptLevel::Optimized, true),
    ] {
        let out = build_packages(
            &host,
            "std",
            &Packages {
                checked,
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
        if out.diags.has_errors() {
            return Err(out.render());
        }
        let path = hd_testkit::Scratch::unmade(
            env!("CARGO_TARGET_TMPDIR"),
            &format!("function-value-native-drop-{level:?}-{checked}.wasm"),
        );
        std::fs::write(&path, out.wasm.ok_or("no native Drop wasm")?).map_err(|e| e.to_string())?;
        // Fixed cost: one fixture module and one Node process per mode
        // (four modes total), with no source-sized subprocess fan-out.
        let ran = std::process::Command::new("node")
            .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
            .arg(&path)
            .output()
            .map_err(|e| e.to_string())?;
        if !ran.status.success() {
            return Err(format!(
                "{level:?}, checked={checked}: {}",
                String::from_utf8_lossy(&ran.stderr)
            ));
        }
    }
    Ok(())
}
