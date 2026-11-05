//! Checked count audit, one run per named program (no timings or full suite).
//! ```sh
//! HD_CACHE=target/count-audit cargo run -p hd_driver --features checked-report \
//! --example count_audit -- bench/study/closures.hd
//! ```
#[cfg(feature = "checked-report")]
fn main() {
    use hd_base::SliceSort as _;
    use hd_driver::{Executor, Goal, Host, NoClock, Packages};
    use hd_project::MemorySources;
    use std::{
        io::Write as _,
        process::{Command, Stdio},
    };
    let mut programs: Vec<_> = std::env::args().skip(1).collect();
    if programs.is_empty() {
        for directory in ["bench/study", "bench/runtime/progs"] {
            programs.extend(
                std::fs::read_dir(directory)
                    .expect("bench directory")
                    .map(|e| e.unwrap().path())
                    .filter(|p| p.extension().is_some_and(|e| e == "hd"))
                    .map(|p| p.to_string_lossy().into_owned()),
            );
        }
        programs.sort_stable();
    }
    let store = hd_cache::MemoryStore::default();
    for program in programs {
        hd_driver::copy_report_label(&program);
        let mut sources = MemorySources::default();
        sources.insert(
            "main.hd",
            &std::fs::read_to_string(&program).expect("program source"),
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
            "audit",
            &Packages {
                checked: true,
                ..Packages::default()
            },
            &Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(!out.diags.has_errors(), "{program}: {}", out.render());
        let wasm = out.wasm.expect("checked program");
        let node = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/node.ts");
        let script = format!(
            r"
            import {{pathToFileURL}} from 'node:url';
            const {{createHost}}=await import(pathToFileURL({:?}));
            const module=await WebAssembly.compile(new Uint8Array({wasm:?}));
            const errors=[];
            const host=createHost({{out(){{}},err(x){{errors.push(x)}}}},{{module,program:{program:?}, countAudit(sites){{console.log(JSON.stringify({{program:{program:?},sites}}));}} }});
            try {{ const instance=await WebAssembly.instantiate(module,host.imports);
                const result=await host.run(instance,'hd.init','hd.poll');
                if(result.status!==0)throw new Error(errors.join(''));
            }} finally {{ host.close(); }}
        ",
            node.to_str().unwrap()
        );
        let mut child = Command::new("node")
            .arg("--input-type=module")
            .stdin(Stdio::piped())
            .spawn()
            .expect("Node runtime");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        assert!(
            child.wait().expect("count audit").success(),
            "audit failed: {program}"
        );
    }
}
#[cfg(not(feature = "checked-report"))]
fn main() {
    panic!("count_audit requires --features checked-report");
}
