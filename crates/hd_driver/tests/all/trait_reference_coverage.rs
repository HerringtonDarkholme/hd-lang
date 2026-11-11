//! Exact weak-expiry oracles for cold fat-reference temporaries.
use hd_cache::MemoryStore;
use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

#[test]
fn cold_trait_temporaries_keep_exact_owners() -> Result<(), Box<dyn std::error::Error>> {
    // A wide type argument keeps this on the generic fallback body,
    // where local trait widening must adopt the copied payload's count.
    // Only type metadata grows; no wide value is allocated or passed.
    let big = format!("type Big = ({})", "i32, ".repeat(5000));
    let wide_generic =
        include_str!("wide_generic_trait_widen.hd").replace("type Big = (i32,)", &big);
    let cases = [
        (
            "cold-trait-temporary",
            include_str!(
                "../../../hd_wasm/tests/cases/references/a-cold-local-trait-reference-owns-its-fresh-temporary.hd"
            ),
        ),
        ("wide-generic-trait-widen", wide_generic.as_str()),
    ];
    for (name, source) in cases {
        let mut sources = MemorySources::default();
        sources.insert("main.hd", source);
        let store = MemoryStore::default();
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        for level in [OptLevel::Dev, OptLevel::Optimized] {
            for checked in [false, true] {
                let out = build_packages(
                    &host,
                    "app",
                    &Packages {
                        build_profile: BuildProfile::Release,
                        level,
                        checked,
                        verify_own: true,
                        ..Packages::default()
                    },
                    &Goal::Program {
                        entry: "main".into(),
                    },
                );
                assert!(!out.diags.has_errors(), "{name}: {}", out.render());
                let wasm = out.wasm.ok_or("no emitted module")?;
                let path = hd_testkit::Scratch::unmade(
                    env!("CARGO_TARGET_TMPDIR"),
                    "trait-reference-owner.wasm",
                );
                std::fs::write(&path, wasm)?;
                // cost: two fixed cases times four pipeline/check modes.
                let ran = std::process::Command::new("node")
                    .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
                    .arg(&path)
                    .output()?;
                assert!(
                    ran.status.success(),
                    "{name} {level:?} checked={checked}: {}",
                    String::from_utf8_lossy(&ran.stderr)
                );
                assert_eq!(
                    String::from_utf8(ran.stdout)?.trim(),
                    "ok",
                    "{name} {level:?} checked={checked}"
                );
            }
        }
    }
    Ok(())
}
