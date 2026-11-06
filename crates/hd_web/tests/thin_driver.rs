//! Run alone with `cargo test -p hd_web --test thin_driver -- --ignored`.
//! Workspace roots (and `hd_driver`'s testkit dev-dependency) unify the driver's
//! default features, so ordinary workspace runs cannot exercise this build.
use hd_base::Stage;
use hd_cache::MemoryStore;
use hd_driver::{
    Executor, Goal, Host, HostPackage, NoClock, Packages, build_packages, host_artifacts,
};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

#[test]
#[ignore = "run this target alone so other workspace roots do not enable driver defaults"]
fn a_thin_client_keeps_host_contracts_without_generating_glue() {
    let mut files = MemorySources::default();
    files.insert(
        "src/main.hd",
        "@host\npub trait Service:\n    fn read(&self) -> i32\npub fn main() -> void $ Console + Service:\n    println($.use(Service).read())\n",
    );
    let store = MemoryStore::default();
    let host = Host {
        sources: &files,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
        render_tir: &["app.*"],
    };
    let packages = Packages {
        host_packages: vec![HostPackage {
            package: 0,
            identity: "example.org/sdk@1".into(),
            versioned_source: Some("example.org/sdk@1.2.3".into()),
            declarations: Some(hd_project::HostDeclarations {
                providers: Some(vec![hd_project::HostProvider {
                    trait_path: "main.Service".into(),
                    path: Some("provider.mjs".into()),
                    line: 1,
                }]),
                line: 1,
            }),
            manifest: "hd.toml".into(),
        }],
        ..Packages::default()
    };
    let out = build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert!(out.host_artifacts.is_none());
    let wasm = out.wasm.as_ref().expect("host program builds");
    let artifacts = host_artifacts(wasm)
        .expect("valid host metadata and explicit glue")
        .expect("host contract remains in the module");
    let contract = &artifacts.contract;
    assert_eq!(contract.interfaces.len(), 1);
    assert_eq!(contract.interfaces[0].methods[0].name, "read");
    {
        assert!(out.tir_text.is_empty());
        assert!(out.own_text.is_empty());
    }
}

#[test]
#[ignore = "run this target alone so other workspace roots do not enable driver defaults"]
fn an_unavailable_fixture_reports_its_missing_feature() {
    let mut files = MemorySources::default();
    files.insert("src/main.hd", "fn value() -> i32: 7\n");
    let store = MemoryStore::default();
    let host = Host {
        sources: &files,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
        render_tir: &[],
    };
    let out = build_packages(
        &host,
        "app",
        &Packages::default(),
        &Goal::Fixture {
            module: "main".into(),
        },
    );
    assert!(out.wasm.is_none());
    let tally = &out.report.stages[&Stage::Collect];
    assert_eq!(tally.not_implemented, 1);
    assert!(tally.first_reason.as_ref().is_some_and(|reason| {
        reason.contains("fixture emission requires hd_driver's fixtures feature")
    }));
}

#[test]
#[ignore = "run this target alone so other workspace roots do not enable driver defaults"]
fn an_unavailable_checked_build_reports_its_missing_feature() {
    let mut files = MemorySources::default();
    files.insert("src/main.hd", "fn main() -> void:\n    println(\"hi\")\n");
    let store = MemoryStore::default();
    let host = Host {
        sources: &files,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
        render_tir: &[],
    };
    for packages in [
        Packages {
            checked: true,
            ..Packages::default()
        },
        Packages {
            verify_own: true,
            ..Packages::default()
        },
    ] {
        let out = build_packages(
            &host,
            "app",
            &packages,
            &Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(out.wasm.is_none());
        let tally = &out.report.stages[&Stage::Collect];
        assert!(tally.first_reason.as_ref().is_some_and(|reason| {
            reason.contains("checked builds require hd_driver's checked-builds feature")
        }));
    }
}
