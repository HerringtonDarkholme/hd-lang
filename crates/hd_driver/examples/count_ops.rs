//! Compile the named programs for the checked operation-count gate.
//! ```sh
//! cargo run -p hd_driver --example count_ops -- target/count-ops/modules bench/study/closures.hd
//! ```
use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, OptLevel, Packages};
use hd_project::MemorySources;

fn main() {
    let mut arguments = std::env::args().skip(1);
    let directory = std::path::PathBuf::from(arguments.next().expect("output directory"));
    std::fs::create_dir_all(&directory).expect("create count modules directory");
    let store = hd_cache::MemoryStore::default();
    for (index, program) in arguments.enumerate() {
        let mut sources = MemorySources::default();
        sources.insert(
            "main.hd",
            &std::fs::read_to_string(&program).expect("benchmark source"),
        );
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(hd_sched::SerialOrder::Priority),
        };
        let out = hd_driver::build_packages(
            &host,
            "counts",
            &Packages {
                checked: true,
                level: OptLevel::Optimized,
                build_profile: BuildProfile::Release,
                verify_own: true,
                ..Packages::default()
            },
            &Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(!out.diags.has_errors(), "{program}: {}", out.render());
        let path = directory.join(format!("{index}.wasm"));
        std::fs::write(&path, out.wasm.expect("checked benchmark module"))
            .expect("write count module");
        println!("{program}\t{}", path.display());
    }
}
