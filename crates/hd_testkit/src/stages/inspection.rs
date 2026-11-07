use hd_driver::{Executor, Host, NoClock, Output, Packages, build_packages};
use hd_sched::SerialOrder;

pub(super) fn build(p: &super::Program, origins: bool, at: super::Build) -> Output {
    let mut sources = p.sources();
    if at.std {
        sources = hd_driver::std_sources();
        sources.insert(super::own::STD_CASE, &p.text);
    }
    let host = Host {
        sources: &sources,
        store: super::shared_store(),
        render_tir: &[],
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build_packages(
        &host,
        if at.std { "std" } else { "app" },
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
            checked: at.checked,
            std_leaf: at.std.then_some(super::own::STD_CASE),
            ..Packages::default()
        },
        &at.goal(),
    )
}
