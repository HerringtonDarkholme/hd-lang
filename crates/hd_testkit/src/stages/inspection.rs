use std::cell::RefCell;
use std::rc::Rc;

use hd_driver::{Executor, Host, NoClock, Output, Packages, build_packages};
use hd_sched::SerialOrder;

/// What names a build: the program's sources and its [`super::Build`].
type Key = (super::Build, String, Vec<(String, String)>);

thread_local! {
    /// This thread's last build. A case's views all ask for the same one.
    static LAST: RefCell<Option<(Key, Rc<Output>)>> = const { RefCell::new(None) };
}

/// The build the compiled views read: with instruction origins and the text
/// of the case's own bodies, which add output and leave the module unchanged.
pub(super) fn build(p: &super::Program, at: super::Build) -> Rc<Output> {
    let key = (at, p.text.clone(), p.files.clone());
    if let Some(built) = LAST.with_borrow(|last| {
        last.as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, built)| Rc::clone(built))
    }) {
        return built;
    }
    let built = Rc::new(fresh(p, at));
    LAST.set(Some((key, Rc::clone(&built))));
    built
}

/// Drops the kept build. Each case's views are done with it by the next case.
pub(super) fn forget() {
    LAST.set(None);
}

fn fresh(p: &super::Program, at: super::Build) -> Output {
    let mut sources = p.sources();
    if at.std {
        sources = hd_driver::std_sources();
        sources.insert(super::own::STD_CASE, &p.text);
    }
    let (package, render_tir): (_, &[&str]) = if at.std {
        ("std", &["std/rt_tests.stage_case/*"])
    } else {
        ("app", &["app/*"])
    };
    let host = Host {
        sources: &sources,
        store: super::shared_store(),
        render_tir,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build_packages(
        &host,
        package,
        &Packages {
            instruction_origins: true,
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
