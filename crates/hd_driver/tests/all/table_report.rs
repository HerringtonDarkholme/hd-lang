//! The function-table report (#1536): for each bench program, each study
//! program, each std runtime test module and the largest conformance
//! programs, the function-table slots and descriptors one link uses in dev
//! and in release, against the glue field's limit
//! (`crates/hd_wasm_linear/DESIGN.md` §23.4). Run it by hand:
//!
//! ```sh
//! cargo test -p hd_driver --test all table_report -- --ignored --nocapture
//! ```
//!
//! It prints one row per build, then the maximum and the distribution.

use std::path::{Path, PathBuf};

use hd_base::SliceSort as _;
use hd_cache::MemoryStore;
use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// How many of the largest single-file runtime conformance programs join
/// the report.
const LARGEST_CONFORMANCE: usize = 25;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| d.filter_map(|e| Some(e.ok()?.path())).collect())
        .unwrap_or_default();
    out.retain(|p| p.extension().is_some_and(|e| e == "hd"));
    out.sort_stable();
    out
}

/// The programs, each a label, its package, its sources, its goal and, for
/// a std test module, its path as a std leaf.
fn programs() -> Vec<(String, &'static str, MemorySources, Goal, Option<String>)> {
    let root = root();
    let main = || Goal::Program {
        entry: "main".into(),
    };
    let mut out = Vec::new();
    let single = |label: String, path: &Path, out: &mut Vec<_>| {
        let mut sources = MemorySources::default();
        sources.insert("main.hd", &std::fs::read_to_string(path).expect("program"));
        out.push((label, "app", sources, main(), None));
    };
    let tree = |label: String, dir: &Path, out: &mut Vec<_>| {
        let mut sources = MemorySources::default();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d).expect("package").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "hd") {
                    let relative = path.strip_prefix(dir).expect("inside").to_string_lossy();
                    sources.insert(&relative, &std::fs::read_to_string(&path).expect("module"));
                }
            }
        }
        out.push((label, "app", sources, main(), None));
    };
    for dir in ["bench/study", "bench/micro", "bench/runtime/progs"] {
        for path in files(&root.join(dir)) {
            let name = path.file_name().expect("name").to_string_lossy();
            single(format!("{dir}/{name}"), &path, &mut out);
        }
    }
    for case in ["tokenizer", "math", "records", "traits"] {
        tree(
            format!("bench/{case}"),
            &root.join(format!("bench/{case}")),
            &mut out,
        );
    }
    // The generated 10,000-line package, when `node bench/generated/generate.ts
    // target/table-report/generated` made it.
    let generated = root.join("target/table-report/generated");
    if generated.join("main.hd").exists() {
        tree("bench/generated".to_owned(), &generated, &mut out);
    }
    let mut conformance: Vec<(u64, PathBuf)> = files(&root.join("spec/conformance/runtime/valid"))
        .into_iter()
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.len(), p)))
        .collect();
    conformance.sort_stable_by(|a, b| b.0.cmp(&a.0));
    // Every weak-linked program: its table has the smaller limit.
    let weak = |p: &Path| {
        std::fs::read_to_string(p).is_ok_and(|t| t.contains("std.weak") || t.contains("Weak["))
    };
    let (weak, rest): (Vec<_>, Vec<_>) = conformance.into_iter().partition(|(_, p)| weak(p));
    for (_, path) in weak
        .into_iter()
        .chain(rest.into_iter().take(LARGEST_CONFORMANCE))
    {
        let name = path.file_name().expect("name").to_string_lossy();
        single(format!("conformance/{name}"), &path, &mut out);
    }
    for path in files(&root.join("crates/hd_driver/tests/std_rt")) {
        let name = path
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        // The `count_pair_*` files are inputs of the count audit, not
        // test modules.
        if !name.ends_with("_tests.hd") {
            continue;
        }
        let leaf = format!("rt_tests/{name}");
        let mut sources = hd_driver::std_sources();
        sources.insert(&leaf, &std::fs::read_to_string(&path).expect("module"));
        let goal = Goal::Tests {
            module: Some(hd_project::module_path("std", &leaf)),
            filter: None,
        };
        out.push((format!("std/{leaf}"), "std", sources, goal, Some(leaf)));
    }
    out
}

/// The function table's size in a module's table section.
fn table_section(wasm: &[u8]) -> u32 {
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let wasmparser::Payload::TableSection(r) = payload.expect("payload")
            && let Some(t) = r.into_iter().next()
        {
            return u32::try_from(t.expect("table").ty.initial).expect("table size");
        }
    }
    0
}

#[test]
#[ignore = "a report, run by hand (#1536)"]
fn table_report() {
    let store = MemoryStore::default();
    let mut rows = Vec::new();
    println!("program\tprofile\tslots\tdescriptors\tlimit\tshare");
    for (label, package, sources, goal, leaf) in programs() {
        for (profile, level, name) in [
            (BuildProfile::Checked, OptLevel::Dev, "dev"),
            (BuildProfile::Release, OptLevel::Optimized, "release"),
        ] {
            let host = Host {
                render_tir: &[],
                sources: &sources,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            };
            let packages = Packages {
                build_profile: profile,
                level,
                std_leaf: leaf.as_deref(),
                ..Packages::default()
            };
            let out = build_packages(&host, package, &packages, &goal);
            let Some(wasm) = out.wasm.as_deref() else {
                println!(
                    "{label}\t{name}\tno module: {}",
                    out.render().lines().next().unwrap_or("")
                );
                continue;
            };
            let used = hd_wasm_linear::last_table_use();
            // A link this thread did not run would leave an older report.
            assert_eq!(used.slots, table_section(wasm), "{label} {name}");
            let limit = hd_wasm_linear::table_limit(used.weak);
            let share = f64::from(used.slots) * 100.0 / f64::from(limit);
            println!(
                "{label}\t{name}\t{}\t{}\t{limit}\t{share:.2}%",
                used.slots, used.descriptors
            );
            rows.push((used.slots, used.descriptors, share, label.clone(), name));
        }
    }
    rows.sort_stable_by(|a, b| a.2.total_cmp(&b.2));
    let at = |percent: usize| {
        let i = (rows.len() - 1) * percent / 100;
        (rows[i].0, rows[i].1)
    };
    println!("builds\t{}", rows.len());
    for (percent, name) in [(50, "median"), (90, "p90"), (99, "p99")] {
        let (slots, descriptors) = at(percent);
        println!("{name}\tslots {slots}\tdescriptors {descriptors}");
    }
    let max = rows.last().expect("a build");
    println!(
        "max\tslots {}\tdescriptors {}\t{:.2}% of its limit\t{} {}",
        max.0, max.1, max.2, max.3, max.4
    );
    let most = rows.iter().map(|r| r.1).max().unwrap_or(0);
    println!("max descriptors\t{most}");
}
