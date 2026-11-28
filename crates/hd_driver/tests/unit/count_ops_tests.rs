//! A real hot-loop count increase must fail the same comparator as CI.
use hd_driver::*;
use hd_project::MemorySources;
use std::{io::Write as _, process::Stdio};

#[test]
fn an_extra_owner_in_a_hot_loop_is_a_gate_regression() {
    let text = include_str!("../cases/counts/operation-counter-loop.hd");
    let store = crate::support::warm_std_store();
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
      const {{countEntry,compareCounts}}=await import(new URL('bench/counts/baseline.ts',root));
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
      assert.equal(compareCounts([rows[1]],[rows[0]]).increases.length,1);
    ",
        root.to_str().unwrap()
    );
    let mut child = hd_testkit::node::command()
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

/// The checked report is the build's choice (#2308): a checked build that
/// does not ask for it, the count budgets' build, emits the same module
/// whether or not the compiler has the `checked-report` feature, and the
/// report decides nothing in a plain build. One program, built both ways
/// at each setting, compared.
#[test]
fn the_checked_report_changes_only_a_build_that_asks_for_it() {
    let text = include_str!("../../../../bench/runtime/progs/closures.hd");
    let store = crate::support::warm_std_store();
    let mut sources = MemorySources::default();
    sources.insert("main.hd", text);
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(hd_sched::SerialOrder::Priority),
    };
    let build = |checked: bool, report: bool| {
        let out = build_packages(
            &host,
            "counts",
            &Packages {
                checked,
                report,
                build_profile: BuildProfile::Release,
                level: OptLevel::Optimized,
                ..Packages::default()
            },
            &Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(!out.diags.has_errors(), "{}", out.render());
        out.wasm.expect("a module")
    };
    assert_eq!(build(false, true), build(false, false), "a plain build");
    let (asked, not) = (build(true, true), build(true, false));
    let names = |wasm: &[u8]| {
        let mut out = Vec::new();
        for payload in wasmparser::Parser::new(0).parse_all(wasm) {
            match payload.expect("a valid module") {
                wasmparser::Payload::CustomSection(c) => out.push(c.name().to_owned()),
                wasmparser::Payload::ExportSection(s) => {
                    out.extend(s.into_iter().map(|e| e.expect("an export").name.to_owned()));
                }
                _ => {}
            }
        }
        out
    };
    let report = |wasm: &[u8]| {
        names(wasm)
            .into_iter()
            .any(|n| n == "hd.count.anchors" || n == "hd.cycles.pointer")
    };
    assert!(!report(&not), "{:?}", names(&not));
    assert_eq!(report(&asked), cfg!(feature = "checked-report"));
    if !cfg!(feature = "checked-report") {
        assert_eq!(asked, not);
    }
}
