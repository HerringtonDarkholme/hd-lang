//! What several test modules share: the warm std store, a one-file program
//! build and the package host's JavaScript runner.

use std::sync::OnceLock;

use hd_base::BuildProfile;
use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_project::MemorySources;

/// A test's store: empty, over a process-wide store where one build per goal has
/// already prepared std. A test whose subject is std's cold path uses its own
/// `MemoryStore`.
pub fn warm_std_store() -> hd_cache::OverlayStore<'static> {
    static SEED: OnceLock<MemoryStore> = OnceLock::new();
    hd_cache::OverlayStore::new(SEED.get_or_init(|| {
        let store = MemoryStore::default();
        let mut sources = hd_project::MemorySources::default();
        sources.insert("main.hd", "pub fn main() -> void: pass\n");
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(hd_sched::SerialOrder::Priority),
        };
        for goal in [
            Goal::Analyze,
            Goal::CheckTests,
            Goal::Program {
                entry: "main".into(),
            },
        ] {
            let out = build_packages(&host, "app", &Packages::default(), &goal);
            assert!(!out.diags.has_errors(), "{}", out.render());
        }
        store
    }))
}

#[cfg(feature = "host-artifacts")]
pub fn run_package_host_javascript(out: &Output, body: &str) {
    use std::io::Write as _;
    use std::process::Stdio;
    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/core.ts");
    let glue = &out.host_artifacts.as_ref().unwrap().javascript;
    let script = format!(
        "\
import assert from 'node:assert/strict';
import {{ pathToFileURL }} from 'node:url';
const coreURL = pathToFileURL({:?});
const {{ createCoreHost: createBaseHost }} = await import(coreURL);
const {{ createPackageRuntime, readPackageContract }} = await import(new URL('package-providers.ts', coreURL));
const createCoreHost = (sink, env, layer) =>
  createBaseHost(sink, {{ ...env, createPackageRuntime, readPackageContract }}, layer);
const glue = await import('data:text/javascript,' + encodeURIComponent({glue:?}));
const module = await WebAssembly.compile(new Uint8Array({:?}));
const output = [], errors = [];
const sink = {{ out: text => output.push(text), err: text => errors.push(text) }};
{body}
",
        core.to_str().unwrap(),
        out.wasm.as_ref().unwrap(),
    );
    let mut child = hd_testkit::node::command()
        .args(["--input-type=module"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Node for package host integration");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

/// Builds the program `text` as `app` into `store` at `level`.
pub fn build_at(
    store: &MemoryStore,
    text: &str,
    profile: BuildProfile,
    level: hd_base::OptLevel,
) -> hd_driver::Output {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", text);
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store,
        clock: &NoClock,
        executor: Executor::Serial(hd_sched::SerialOrder::Priority),
    };
    let packages = Packages {
        build_profile: profile,
        level,
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
    out
}
