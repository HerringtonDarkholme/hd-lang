//! Count actual regex construction in the guest without production counters.
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_sched::SerialOrder;

#[test]
fn a_pattern_applied_ten_thousand_times_compiles_once() -> Result<(), String> {
    let regex = include_str!("../../../../lib/std/regex.hd");
    let site = "    pub fn new(pattern: string) -> Result[Regex, RegexError]:\n";
    assert_eq!(regex.matches(site).count(), 1, "one constructor entry");
    let instrumented = regex
        .replacen(
            "impl Regex:",
            "let pattern_compile_count: usize = 0\n\nimpl Regex:",
            1,
        )
        .replace(site, &format!("{site}        pattern_compile_count += 1\n"))
        + "\npub fn pattern_compiles() -> usize: pattern_compile_count\n";
    let mut sources = hd_driver::std_sources();
    sources.insert("regex.hd", &instrumented);
    sources.insert(
        "rt_tests/validate_pattern.hd",
        include_str!("validate_pattern.hd"),
    );
    let store = crate::warm_store();
    let host = Host {
        sources: &sources,
        store: &store,
        render_tir: &[],
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    for level in [hd_driver::OptLevel::Dev, hd_driver::OptLevel::Optimized] {
        let out = build_packages(
            &host,
            "std",
            &Packages {
                level,
                build_profile: hd_driver::BuildProfile::Checked,
                std_leaf: Some("rt_tests/validate_pattern.hd"),
                ..Packages::default()
            },
            &Goal::Program {
                entry: "rt_tests.validate_pattern".into(),
            },
        );
        if out.diags.has_errors() {
            return Err(out.render());
        }
        let path =
            hd_testkit::Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), "validate-pattern.wasm");
        std::fs::write(&path, out.wasm.ok_or("no validator module")?).map_err(|e| e.to_string())?;
        // cost: one Node process for each of two fixed pipelines; guest counters measure the work.
        let ran = std::process::Command::new("node")
            .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
            .arg(&path)
            .output()
            .map_err(|e| e.to_string())?;
        assert!(
            ran.status.success(),
            "{}{}",
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr)
        );
        assert_eq!(ran.stdout, b"pattern compiled once\n");
    }
    Ok(())
}
