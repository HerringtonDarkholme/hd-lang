//! A focused runtime test, independent of the aggregate std runner.
use super::*;
use std::io::Write as _;
use std::process::{Command, Stdio};

#[test]
fn pending_counts_stop_at_observation_boundaries() {
    let mut sources = std_sources();
    sources.insert(
        "rt_count_pair_tests/mod.hd",
        include_str!("../tests/std_rt/count_pair_detector.hd"),
    );
    let store = hd_cache::MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(hd_sched::SerialOrder::Priority),
    };
    let out = build_packages(
        &host,
        "std",
        &Packages::default(),
        &Goal::Program {
            entry: "rt_count_pair_tests".into(),
        },
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    let wasm = out.wasm.expect("detector program builds");
    run_detector(&wasm, "");
}

#[cfg(feature = "checked-report")]
#[test]
fn emitted_pairs_have_source_lines_and_observation_barriers() {
    let mut sources = std_sources();
    sources.insert(
        "rt_count_pair_tests/mod.hd",
        include_str!("../tests/std_rt/count_pair_emission.hd"),
    );
    let store = hd_cache::MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(hd_sched::SerialOrder::Priority),
    };
    let out = build_packages(
        &host,
        "std",
        &Packages {
            checked: true,
            ..Packages::default()
        },
        &Goal::Program {
            entry: "rt_count_pair_tests".into(),
        },
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    run_detector(
        &out.wasm.expect("checked program builds"),
        r"
        const report=await import(new URL('./checked-report.ts',pathToFileURL(CORE)));
        const rows=report.countPairRows(module,instance);
        assert.ok(rows, 'metadata exists');
        assert.deepEqual(rows.filter(r=>r.function.includes('/redundant')), [{
          function:'std/rt_count_pair_tests/redundant', file:'rt_count_pair_tests/mod.hd', line:11, reason:'source call', count:'3'
        }]);
        assert.equal(rows.some(r=>r.function.includes('/needed_')),false,JSON.stringify(rows));
        assert.equal(snapshots[0].find(r=>r.function.endsWith('/redundant')).count,'3');
        await host.run(instance,'hd.init','hd.poll');
        assert.deepEqual(snapshots.at(-1),[], 'a repeated snapshot does not double count');
    ",
    );
}

fn run_detector(wasm: &[u8], check: &str) {
    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/core.ts");
    let script = format!(
        "import assert from 'node:assert/strict';\n\
         import {{pathToFileURL}} from 'node:url';\n\
         const CORE={:?};\n\
         const {{createCoreHost}}=await import(pathToFileURL(CORE));\n\
         const module=await WebAssembly.compile(new Uint8Array({wasm:?}));\n\
         const errors=[]; const snapshots=[];\n\
         const host=createCoreHost({{out(){{}},err(x){{errors.push(x)}}}},{{module,countAudit(rows){{snapshots.push(rows)}}}});\n\
         try{{const instance=await WebAssembly.instantiate(module,host.imports);\n\
         const result=await host.run(instance,'hd.init','hd.poll');\n\
         assert.equal(result.status,0,errors.join(''));{check}}}finally{{host.close();}}\n",
        core.to_str().expect("core path"),
    );
    let mut child = Command::new("node")
        .args(["--input-type=module"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Node runtime");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let result = child.wait_with_output().expect("run detector");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
