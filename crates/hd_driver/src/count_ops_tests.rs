//! A real hot-loop count increase must fail the same comparator as CI.
use super::*;
use std::{
    io::Write as _,
    process::{Command, Stdio},
};

#[test]
fn an_extra_owner_in_a_hot_loop_is_a_gate_regression() {
    let text = include_str!("../tests/cases/counts/operation-counter-loop.hd");
    let store = crate::warm_std_store();
    let mut modules = Vec::new();
    for source in [
        text.replace("EXTRA := true", "EXTRA := false"),
        text.to_owned(),
    ] {
        let mut sources = MemorySources::default();
        sources.insert("main.hd", &source);
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(hd_sched::SerialOrder::Priority),
        };
        let out = build_packages(
            &host,
            "gate",
            &Packages {
                checked: true,
                build_profile: BuildProfile::Release,
                level: OptLevel::Optimized,
                verify_own: true,
                ..Packages::default()
            },
            &Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(!out.diags.has_errors(), "{}", out.render());
        modules.push(out.wasm.expect("counter test program"));
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = format!(
        r"
      import assert from 'node:assert/strict';
      import {{pathToFileURL}} from 'node:url';
      const root=pathToFileURL({:?}+'/');
      const {{createHost}}=await import(new URL('host/node.ts',root));
      const {{countEntry,compareCounts,recordCounts}}=await import(new URL('bench/counts/baseline.ts',root));
      const rows=[];
      for (const bytes of {modules:?}) {{
        const module=await WebAssembly.compile(new Uint8Array(bytes));
        const errors=[];
        const host=createHost({{out(){{}},err(x){{errors.push(x)}}}},{{module}});
        try{{const instance=await WebAssembly.instantiate(module,host.imports);
          const result=await host.run(instance,'hd.init','hd.poll');
          assert.equal(result.status,0,errors.join(''));
          rows.push(countEntry('loop',['hd.retained','hd.released','hd.uniques'].map(k=>instance.exports[k]())));
        }} finally{{host.close();}}
      }}
      assert.equal(BigInt(rows[1].retained)-BigInt(rows[0].retained),1000n,JSON.stringify(rows));
      assert.equal(BigInt(rows[1].released)-BigInt(rows[0].released),1000n,JSON.stringify(rows));
      assert.ok(compareCounts([rows[1]],recordCounts([rows[0]])).length>0);
    ",
        root.to_str().unwrap()
    );
    let mut child = Command::new("node")
        .arg("--input-type=module")
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
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
