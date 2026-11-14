//! `HD_WORK=FILE` (#1880): `hd` writes its work counters to FILE as one
//! JSON object; a serial run of the same input repeats the compiler's
//! counts, and a report it cannot write leaves the command's status alone.

use std::process::Command;

use hd_testkit::Scratch;

const MAIN: &str = "pub fn main() -> void $ Console:\n    println(\"ok\")\n";

fn package(name: &str) -> Scratch {
    let dir = Scratch::new(env!("CARGO_TARGET_TMPDIR"), name);
    std::fs::write(
        dir.join("hd.toml"),
        "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = true\n",
    )
    .expect("write");
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(dir.join("src/main.hd"), MAIN).expect("write");
    dir
}

fn check(dir: &Scratch, work: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", dir.cache())
        .env("HD_JOBS", "1")
        .env("HD_WORK", work)
        .current_dir(dir)
        .arg("check")
        .output()
        .expect("run hd")
}

#[test]
fn a_serial_run_reports_every_counter_and_repeats_its_counts() {
    let dir = package("work-report");
    let work = dir.join("work.json");
    // The first run fills the cache; the next two start from the same one.
    assert!(check(&dir, &work).status.success());
    let first = check(&dir, &work);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let report = std::fs::read_to_string(&work).expect("report");
    for name in [
        "allocs",
        "alloc_bytes",
        "hashes",
        "item_body_checks",
        "walk_plans",
    ] {
        assert!(report.contains(&format!("\"{name}\":")), "{name}: {report}");
    }
    assert!(!report.contains("\"allocs\":0,"), "{report}");
    assert!(check(&dir, &work).status.success());
    let again = std::fs::read_to_string(&work).expect("report");
    // Allocations vary by a few with file-system names; the rest repeat.
    let compiler = |text: &str| -> Vec<String> {
        text.trim_matches(|c| c == '{' || c == '}' || c == '\n')
            .split(',')
            .filter(|field| !field.starts_with("\"alloc"))
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(compiler(&again), compiler(&report));
    assert!(compiler(&report).len() > 10, "{report}");
}

#[test]
fn a_report_that_cannot_be_written_keeps_the_status() {
    let dir = package("work-report-unwritable");
    let out = check(&dir, &dir.join("missing/work.json"));
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("HD_WORK: cannot write"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
