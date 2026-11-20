//! `HD_WORK=FILE` (#1880): `hd` writes its work counters to FILE as one
//! JSON object; a serial run of the same input repeats every count, and a
//! report it cannot write leaves the command's status alone.

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
    hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
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
    assert_eq!(again, report);
}

/// Copies the tree at `from` to `to`.
fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).expect("dir");
    for entry in std::fs::read_dir(from).expect("read dir") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy");
        }
    }
}

/// #1969: the scale check runs each shape in a fresh directory with a
/// copy of one warm cache. Two such runs of one program count the same
/// work, allocations included: the cache store's own work, whose pack
/// names follow the clock and the process id, is not counted.
#[test]
fn two_runs_from_copies_of_one_warm_cache_count_the_same_work() {
    let warm = package("work-repeat-warm");
    assert!(check(&warm, &warm.join("work.json")).status.success());
    let parens = format!("{}1{}", "(".repeat(40), ")".repeat(40));
    let run = |name: &str| {
        let dir = package(name);
        std::fs::write(
            dir.join("src/main.hd"),
            format!("pub fn main() -> void $ Console:\n    println(\"${{{parens}}}\")\n"),
        )
        .expect("write");
        copy_tree(&warm.cache(), &dir.cache());
        let work = dir.join("work.json");
        let out = check(&dir, &work);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        std::fs::read_to_string(&work).expect("report")
    };
    // Without the fix one run in three or four differed: four runs.
    let first = run("work-repeat-0");
    for i in 1..4 {
        assert_eq!(run(&format!("work-repeat-{i}")), first, "run {i}");
    }
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
