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
    // The first run fills the cache. Past the settle window (`disk::SETTLE`,
    // 2 s) the second records every file's stamp, so the next two start
    // from the same cache and the same stamps, however slow the machine.
    assert!(check(&dir, &work).status.success());
    std::thread::sleep(std::time::Duration::from_millis(2200));
    assert!(check(&dir, &work).status.success());
    let first = check(&dir, &work);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let report = std::fs::read_to_string(&work).expect("report");
    for name in ["allocs", "alloc_bytes", "hashes", "item_body_checks"] {
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

/// `item_decodes` of a warm `hd check` after a body edit in the first of
/// `modules` library modules.
fn items_decoded_after_an_edit(modules: usize) -> u64 {
    let dir = package(&format!("work-item-decodes-{modules}"));
    let module = |k: usize, add: i32| {
        format!("pub fn f{k}(x: i32) -> i32:\n    x + {add}\n\npub fn g{k}() -> i32:\n    2\n")
    };
    for k in 0..modules {
        std::fs::write(dir.join(format!("src/m{k}.hd")), module(k, 1)).expect("write");
    }
    let work = dir.join("work.json");
    assert!(check(&dir, &work).status.success());
    std::fs::write(dir.join("src/m0.hd"), module(0, 2)).expect("write");
    let out = check(&dir, &work);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = std::fs::read_to_string(&work).expect("report");
    let count = report
        .split("\"item_decodes\":")
        .nth(1)
        .and_then(|rest| rest.split([',', '}']).next())
        .and_then(|n| n.trim().parse().ok());
    count.unwrap_or_else(|| panic!("no item_decodes: {report}"))
}

/// #1801 draft 64: the whole-program checks after the module checks
/// decoded every module's items on every warm run, twice. Now the
/// `@inline(.Always)` check reads only a module whose entry says it
/// declares one, and the entry capabilities only an entry module or one
/// with tests, so a warm edit decodes as many items at 16 modules as at 4.
#[test]
fn a_warm_edit_decodes_no_more_items_in_a_larger_package() {
    let (small, large) = (
        items_decoded_after_an_edit(4),
        items_decoded_after_an_edit(16),
    );
    assert_eq!(small, large, "4 modules: {small}; 16 modules: {large}");
}

/// A counter of a `hd check` run's work report.
fn counter(report: &str, name: &str) -> u64 {
    report
        .split(&format!("\"{name}\":"))
        .nth(1)
        .and_then(|rest| rest.split([',', '}']).next())
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or_else(|| panic!("no {name}: {report}"))
}

/// #1801 draft 44: a file saved with the same bytes less than 2 s before
/// a check had no stamp, so the run lost the whole-package fast path and
/// looked up every module's entry: about 75,000 map lookups more than a
/// settled run, at 4 modules as at 16. Its content stamp now keeps the
/// fast path: the run does a settled run's lookups plus the one file's
/// read and the stamp table's write.
#[test]
fn a_resave_within_the_settle_window_costs_one_file_over_a_settled_run() {
    let module = |k: usize| format!("pub fn f{k}(x: i32) -> i32:\n    x + 1\n");
    let dirs: Vec<Scratch> = [4, 16]
        .iter()
        .map(|&n| {
            let dir = package(&format!("work-resave-{n}"));
            for k in 0..n {
                std::fs::write(dir.join(format!("src/m{k}.hd")), module(k)).expect("write");
            }
            assert!(check(&dir, &dir.join("work.json")).status.success());
            dir
        })
        .collect();
    // Past the settle window (`disk::SETTLE`, 2 s), every file has a stamp.
    std::thread::sleep(std::time::Duration::from_millis(2200));
    for dir in &dirs {
        let work = dir.join("work.json");
        let hashes = || counter(&std::fs::read_to_string(&work).expect("report"), "hashes");
        assert!(check(dir, &work).status.success());
        assert!(check(dir, &work).status.success());
        let settled = hashes();
        std::fs::write(dir.join("src/m0.hd"), module(0)).expect("write");
        assert!(check(dir, &work).status.success());
        let resaved = hashes();
        assert!(
            resaved <= settled + 100,
            "settled {settled}, resaved {resaved}"
        );
    }
}
