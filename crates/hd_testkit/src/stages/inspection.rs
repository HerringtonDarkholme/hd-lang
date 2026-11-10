use std::cell::RefCell;
use std::rc::Rc;

use hd_driver::{Executor, Host, NoClock, Output, Packages, build_packages};
use hd_sched::SerialOrder;

/// What names a build: the program's sources and its [`super::Build`].
type Key = (
    super::Build,
    String,
    Vec<(String, String)>,
    Option<Vec<String>>,
    Vec<String>,
    Option<u32>,
);

thread_local! {
    /// This thread's last build. A case's views all ask for the same one.
    static LAST: RefCell<Option<(Key, Rc<Output>)>> = const { RefCell::new(None) };
}

/// The build the compiled views read: with instruction origins and the text
/// of the case's own bodies, which add output and leave the module unchanged.
pub(super) fn build(p: &super::Program, at: super::Build) -> Rc<Output> {
    let key = (
        at,
        p.text.clone(),
        p.files.clone(),
        p.profile.clone(),
        p.hosts.clone(),
        p.repl,
    );
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
    // The root package's manifest `[host]`, each trait bound to one
    // provider file, as `hd build` reads it (`module.host.package.marker`).
    let host_packages = if p.hosts.is_empty() {
        Vec::new()
    } else {
        let manifest: String = std::iter::once("[host]\n".to_owned())
            .chain(
                p.hosts
                    .iter()
                    .map(|t| format!("\"{t}\" = \"provider.ts\"\n")),
            )
            .collect();
        vec![hd_driver::HostPackage {
            package: 0,
            identity: package.to_owned(),
            versioned_source: None,
            declarations: hd_project::parse_manifest(&manifest)
                .expect("a stage case's `@host` traits make a manifest")
                .host,
            manifest: "hd.toml".to_owned(),
        }]
    };
    let run = |session: Option<&hd_driver::Session>, goal: &hd_driver::Goal| {
        build_packages(
            &host,
            package,
            &Packages {
                host_packages: host_packages.clone(),
                session,
                inputs_from: p.repl.map(|_| 1),
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
                profile: p.profile.clone(),
                ..Packages::default()
            },
            goal,
        )
    };
    let Some(last) = p.repl else {
        return run(None, &at.goal());
    };
    let mut session = hd_driver::Session::linked();
    let mut input = 1;
    loop {
        let mut built = run(Some(&session), &hd_driver::Goal::ReplInput { input });
        if input == last || built.wasm.is_none() {
            return built;
        }
        if let Some(added) = built.session_added.take() {
            session.add(added);
        }
        input += 1;
    }
}
