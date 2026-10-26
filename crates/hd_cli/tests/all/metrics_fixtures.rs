//! The goal-metrics fixtures (`bench/metrics/fixtures/`) check clean with
//! this `hd`, and each mistake program reports its expected code, so a
//! language change that breaks one fails here rather than in the next
//! daily metrics run (`bench/metrics/check-fixtures.ts`, #831).
use std::path::Path;
use std::process::Command;

#[test]
fn metrics_fixtures_check_with_this_hd() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("node")
        .arg("--experimental-strip-types")
        .arg(repo.join("bench/metrics/check-fixtures.ts"))
        .arg("--hd")
        .arg(env!("CARGO_BIN_EXE_hd"))
        .current_dir(&repo)
        .env_remove("HD_ENGINE")
        .output()
        .expect("node runs bench/metrics/check-fixtures.ts");
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "metrics fixtures fail with this hd; update bench/metrics/fixtures/:\n{report}"
    );
}
