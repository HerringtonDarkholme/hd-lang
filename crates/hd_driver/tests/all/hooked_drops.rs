//! Programs whose values hold a `std.rt.Drop` hook that counts its runs,
//! moved and left in every way (`tests/all/hooked_drops/`, the cross
//! product of value kinds, moves and ways out): each builds as a checked
//! std program in dev and in release, ends with no count left (the exit
//! audit) and prints its `# stdout:` line, which counts each hook once.
//! Only std may implement `Drop` (`module.rt.std-only`), so these are not
//! stage cases.
//!
//! Cost: two builds and two Node runs per file.

use hd_base::SliceSort as _;
use hd_cache::MemoryStore;
use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_sched::SerialOrder;

fn run(name: &str, source: &str, release: bool) -> String {
    let mut sources = hd_driver::std_sources();
    sources.insert("main.hd", source);
    let store = MemoryStore::default();
    let host = Host {
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
        render_tir: &[],
    };
    let mut packages = Packages {
        checked: true,
        verify_own: true,
        std_leaf: Some("main.hd"),
        ..Packages::default()
    };
    if release {
        packages.build_profile = BuildProfile::Release;
        packages.level = OptLevel::Optimized;
    }
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let out = build_packages(&host, "std", &packages, &goal);
    assert!(!out.diags.has_errors(), "{name}: {}", out.render());
    // ruled out: a Program build without errors emits its Wasm.
    let wasm = out.wasm.expect("a built program has Wasm");
    let ran = super::release_profile_run::run_wasm(name, &wasm);
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8_lossy(&ran.stdout).trim_end().to_owned()
}

#[test]
fn each_hook_runs_once_however_its_value_leaves() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/all/hooked_drops");
    let mut files: Vec<_> = (std::fs::read_dir(&dir).expect("the cases").flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "hd"))
        .collect();
    files.sort_stable();
    assert!(!files.is_empty());
    for path in files {
        let name = path
            .file_stem()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        let source = std::fs::read_to_string(&path).expect("read");
        let want = (source.lines())
            .find_map(|l| l.strip_prefix("# stdout: "))
            .unwrap_or_else(|| panic!("{name}: no `# stdout:` line"));
        for release in [false, true] {
            let mode = if release { "release" } else { "dev" };
            assert_eq!(run(&name, &source, release), want, "{name} ({mode})");
        }
    }
}
