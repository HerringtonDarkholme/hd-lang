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
    // The first run fills the cache. Each compared run follows a save of
    // the same text, so both read it as a just-saved file (within
    // `disk::SETTLE`) and start from the same cache, however slow the
    // machine. A settled file cannot be made by setting its times back:
    // that moves its change time to now.
    let save = || std::fs::write(dir.join("src/main.hd"), MAIN).expect("write");
    assert!(check(&dir, &work).status.success());
    save();
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
    save();
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

/// The counter `name` of a warm `hd check` after a body edit in the first
/// of `modules` library modules.
fn after_an_edit(modules: usize, name: &str) -> u64 {
    let dir = package(&format!("work-{name}-{modules}"));
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
    counter(&std::fs::read_to_string(&work).expect("report"), name)
}

/// #1801 draft 64: the whole-program checks after the module checks
/// decoded every module's items on every warm run, twice. Now the
/// `@inline(.Always)` check reads only a module whose entry says it
/// declares one, and the entry capabilities only an entry module or one
/// with tests, so a warm edit decodes as many items at 16 modules as at 4.
#[test]
fn a_warm_edit_decodes_no_more_items_in_a_larger_package() {
    let (small, large) = (
        after_an_edit(4, "item_decodes"),
        after_an_edit(16, "item_decodes"),
    );
    assert_eq!(small, large, "4 modules: {small}; 16 modules: {large}");
}

/// #1801 option 2: a warm edit looked up every module's `check` entry, read
/// it whole, and looked up its `parse` entry, though one module changed. A
/// module whose key and text match its row in the last-run record now takes
/// the row, so a warm edit looks up as many entries at 16 modules as at 4.
#[test]
fn a_warm_edit_looks_up_no_more_entries_in_a_larger_package() {
    let (small, large) = (
        after_an_edit(4, "entry_lookups"),
        after_an_edit(16, "entry_lookups"),
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

/// `hd doc NAME` in `dir`, writing its work counters to `work`.
fn doc(dir: &Scratch, work: &std::path::Path, name: &str) -> std::process::Output {
    hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", dir.cache())
        .env("HD_JOBS", "1")
        .env("HD_WORK", work)
        .current_dir(dir)
        .args(["doc", name])
        .output()
        .expect("run hd")
}

/// #1801 draft 13: `hd doc std.NAME` built the whole package before it
/// answered from std's text. It now checks the package as `hd check`
/// does (`cli.doc.check`), so in a warm package it replays the stored
/// report: its map lookups exceed a warm `hd check`'s by std's page
/// alone, at 4 modules as at 16.
#[test]
fn a_std_name_in_a_warm_package_costs_a_warm_check_and_the_page() {
    for n in [4, 16] {
        let dir = package(&format!("work-doc-std-{n}"));
        for k in 0..n {
            let text = format!("pub fn f{k}(x: i32) -> i32:\n    x + 1\n");
            std::fs::write(dir.join(format!("src/m{k}.hd")), text).expect("write");
        }
        let work = dir.join("work.json");
        assert!(check(&dir, &work).status.success());
        assert!(check(&dir, &work).status.success());
        let checked = counter(&std::fs::read_to_string(&work).expect("report"), "hashes");
        let out = doc(&dir, &work, "std.collections.List");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8_lossy(&out.stdout).contains("List"));
        let documented = counter(&std::fs::read_to_string(&work).expect("report"), "hashes");
        assert!(
            documented < checked + 1000,
            "{n} modules: check {checked}, doc {documented}"
        );
    }
}

/// #1801 draft 76: every run listed a path dependency's `tests` and
/// `tasks` too, a `stat` and a row each, though a dependent reaches only
/// its library. A warm check of the dependent now allocates as much with
/// 300 test files in the dependency as with none.
#[test]
fn a_dependency_tests_are_not_listed() {
    let dir = package("work-dep-tests");
    let dep = dir.join("dep");
    std::fs::create_dir_all(dep.join("src")).expect("dep src");
    std::fs::create_dir_all(dep.join("tests")).expect("dep tests");
    std::fs::write(dep.join("hd.toml"), "[package]\nname = \"dep\"\n").expect("write");
    let lib = "pub fn one() -> i32: 1\n";
    let manifest = "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = true\n\n\
                    [dependencies]\ndep = { path = \"dep\" }\n";
    std::fs::write(dir.join("hd.toml"), manifest).expect("write");
    let work = dir.join("work.json");
    // Each run follows a save of every listed file with the same text, so
    // both phases see them just saved (within `disk::SETTLE`), whatever the
    // load: a phase that found them settled made 6 fewer allocations.
    let save = || {
        std::fs::write(dir.join("src/main.hd"), MAIN).expect("write");
        std::fs::write(dep.join("src/lib.hd"), lib).expect("write");
    };
    let allocs = || {
        save();
        let out = check(&dir, &work);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        save();
        let out = check(&dir, &work);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        counter(&std::fs::read_to_string(&work).expect("report"), "allocs")
    };
    let none = allocs();
    for k in 0..300 {
        let test = format!("use dep.dep.{{one}}\n\nfn t{k}() -> i32: one()\n");
        std::fs::write(dep.join(format!("tests/t{k}.hd")), test).expect("write");
    }
    let many = allocs();
    assert_eq!(none, many, "allocations with 300 dependency tests");
}

/// #1801: a plain `hd check` discovered the package's modules three times
/// on an edit (for the left-out test files' layout errors, for the stored
/// report's key, for the build) and twice with no change. The layout
/// errors now discover only the roots that hold a left-out file, and the
/// build takes the table the key discovered, so a run discovers each
/// module once: 12 more files at 16 modules than at 4, edited or not.
#[test]
fn a_check_discovers_each_module_once() {
    let discovered = |n: usize| -> [u64; 2] {
        let dir = package(&format!("work-discover-{n}"));
        std::fs::create_dir_all(dir.join("tests")).expect("tests");
        let smoke = "it(\"runs\"):\n    assert(true)\n";
        std::fs::write(dir.join("tests/smoke.hd"), smoke).expect("write");
        let module = |k: usize, add: usize| format!("pub fn f{k}(x: i32) -> i32:\n    x + {add}\n");
        for k in 0..n {
            std::fs::write(dir.join(format!("src/m{k}.hd")), module(k, 1)).expect("write");
        }
        let work = dir.join("work.json");
        let run = || {
            let out = check(&dir, &work);
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            counter(
                &std::fs::read_to_string(&work).expect("report"),
                "discovered_files",
            )
        };
        run();
        std::fs::write(dir.join("src/m0.hd"), module(0, 2)).expect("write");
        [run(), run()]
    };
    let (small, large) = (discovered(4), discovered(16));
    assert_eq!(
        large[0] - small[0],
        12,
        "edited: 4 modules {small:?}, 16 {large:?}"
    );
    assert_eq!(
        large[1] - small[1],
        12,
        "unchanged: 4 modules {small:?}, 16 {large:?}"
    );
}

/// #1802 draft 31: `HD_TRACE=FILE` writes Chrome trace-event JSON with one
/// complete event per task, named by its module or folder, and per named
/// serial phase of the run.
#[test]
fn a_trace_names_each_task_and_the_serial_phases() {
    let dir = package("work-trace");
    let trace = dir.join("trace.json");
    let out = hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", dir.cache())
        .env("HD_TRACE", &trace)
        .current_dir(&dir)
        .arg("check")
        .output()
        .expect("run hd");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&trace).expect("trace")).expect("json");
    let events = json["traceEvents"].as_array().expect("events");
    let named = |cat: &str, name: &str| {
        events
            .iter()
            .any(|e| e["ph"] == "X" && e["cat"] == cat && e["name"] == name && e["dur"].is_u64())
    };
    assert!(named("task", "ModulePrep shop.main"), "{json}");
    for phase in [
        "load package",
        "result key",
        "tasks",
        "records",
        "publish",
        "settle",
    ] {
        assert!(named("phase", phase), "{phase}: {json}");
    }
}

/// #1802: a cold run reads, hashes and skims its files on the pool before
/// the task graph starts. Each file's result is its own and taken in file
/// order, so a cold check prints the same at `--jobs 1` as at `--jobs 8`:
/// here 24 modules, some with errors, and a folder cycle.
#[test]
fn a_cold_check_prints_the_same_at_one_job_and_at_eight() {
    let dir = package("work-cold-jobs");
    for k in 0..24 {
        let body = if k % 5 == 0 { "true" } else { "x + 1" };
        let text = format!("pub fn f{k}(x: i32) -> i32:\n    {body}\n");
        std::fs::write(dir.join(format!("src/m{k}.hd")), text).expect("write");
    }
    for (folder, other) in [("a", "b"), ("b", "a")] {
        std::fs::create_dir_all(dir.join(format!("src/{folder}"))).expect("dir");
        let text = format!("use pkg.{other}.x.{{g}}\n\npub fn g() -> i32:\n    1\n");
        std::fs::write(dir.join(format!("src/{folder}/x.hd")), text).expect("write");
    }
    let run = |jobs: &str| {
        let cache = dir.join(format!("cache-{jobs}"));
        let out = hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
            .env("HD_CACHE", &cache)
            .current_dir(&dir)
            .args(["check", "--jobs", jobs])
            .output()
            .expect("run hd");
        let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        (out.status.code(), text(&out.stdout), text(&out.stderr))
    };
    let one = run("1");
    assert_ne!(one.0, Some(0), "{one:?}");
    assert_eq!(run("8"), one);
}
