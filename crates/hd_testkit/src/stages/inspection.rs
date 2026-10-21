use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_sched::SerialOrder;

pub(super) fn build(p: &super::Program, origins: bool, at: super::Build) -> Output {
    let mut sources = hd_project::MemorySources::default();
    sources.insert("main.hd", &p.text);
    let store = hd_cache::MemoryStore::default();
    let host = Host {
        sources: &sources,
        store: &store,
        render_tir: &[],
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build_packages(
        &host,
        "app",
        &Packages {
            instruction_origins: origins,
            target: at.target,
            level: if at.release {
                hd_driver::OptLevel::Optimized
            } else {
                hd_driver::OptLevel::Dev
            },
            build_profile: if at.release {
                hd_driver::BuildProfile::Release
            } else {
                hd_driver::BuildProfile::Checked
            },
            ..Packages::default()
        },
        &Goal::Fixture {
            module: "main".to_owned(),
        },
    )
}
