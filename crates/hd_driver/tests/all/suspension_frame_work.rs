//! #1898: frame-resident values cost accesses and slots, not live values
//! multiplied by suspension points. Exercise actual full compiler builds.

use std::fmt::Write as _;

use hd_driver::{
    EngineTarget, Executor, Goal, Host, NoClock, OptLevel, Output, Packages, build_packages,
};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn program(points: usize, all_live: bool, depth: usize, pending: bool) -> String {
    let mut text =
        String::from("use std.time.{Clock, ms, sleep}\n\nfn step!(x: i32) -> i32 $ Clock:\n");
    if pending {
        text.push_str("    sleep!(ms(1))\n");
    }
    text.push_str("    x + 1\n\nfn run!() -> i32 $ Clock:\n    let total = +0\n");
    let mut indent = String::from("    ");
    for _ in 0..depth {
        let _ = writeln!(text, "{indent}for _ in 0..1:");
        indent.push_str("    ");
    }
    for i in 0..points {
        if all_live {
            let _ = writeln!(text, "{indent}v{i} := step!(+{i})");
        } else {
            let _ = writeln!(text, "{indent}total = step!(total)");
        }
    }
    if all_live {
        for i in 0..points {
            let _ = writeln!(text, "{indent}total += v{i}");
        }
    }
    text.push_str("    total\n\npub fn main!() -> void $ Console + Clock:\n    println(run!())\n");
    text
}

/// [`program`]'s values with no suspension, under loops whose bound is
/// the run's argument, so the optimizer keeps every loop.
fn looped_program(width: usize, depth: usize) -> String {
    let mut text = String::from(
        "use std.time.{Clock}\n\nfn step!(x: i32) -> i32 $ Clock:\n    x + 1\n\n\
         fn run!(n: i32) -> i32 $ Clock:\n    let total = +0\n",
    );
    let mut indent = String::from("    ");
    for _ in 0..depth {
        let _ = writeln!(text, "{indent}for _ in 0..n:");
        indent.push_str("    ");
    }
    for i in 0..width {
        let _ = writeln!(text, "{indent}v{i} := step!(+{i})");
    }
    for i in 0..width {
        let _ = writeln!(text, "{indent}total += v{i}");
    }
    text.push_str("    total\n\npub fn main!() -> void $ Console + Clock:\n    println(run!(1))\n");
    text
}

fn compile(source: &str, target: EngineTarget, checked: bool, level: OptLevel) -> Output {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        target,
        checked,
        level,
        ..Packages::default()
    };
    let output = build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!output.diags.has_errors(), "{}", output.render());
    assert!(output.wasm.is_some(), "missing module");
    output
}

fn on_worker_stack(run: impl FnOnce() + Send + 'static) {
    let worker = std::thread::Builder::new()
        .stack_size(hd_sched::pool::WORKER_STACK)
        .spawn(run);
    assert!(worker.is_ok());
    if let Ok(worker) = worker {
        assert!(worker.join().is_ok());
    }
}

#[test]
fn suspension_module_size_is_linear_even_when_all_results_stay_live() {
    on_worker_stack(|| {
        for all_live in [false, true] {
            let bytes = |points| {
                let out = compile(
                    &program(points, all_live, 0, true),
                    EngineTarget::V8,
                    false,
                    OptLevel::Dev,
                );
                let bytes = out.wasm.as_deref().map_or(0, <[u8]>::len);
                assert!(bytes > 0);
                eprintln!("suspension size all_live={all_live}, points={points}: {bytes}");
                bytes
            };
            let (base, small, large) = (bytes(0), bytes(100), bytes(200));
            assert!(small > base);
            assert!(
                large - base <= 3 * (small - base),
                "{base}, {small}, {large}"
            );
        }
    });
}

/// The `optimizer` phase is liveness work: the resume entry's query and
/// dev's local sharing. The programs build in dev, so the optimized
/// link's rule passes do not hide the query's depth factor (#2401); the
/// loops nest 2 to 20 deep.
#[test]
fn resident_spill_work_is_linear_in_width_and_depth_together() {
    on_worker_stack(|| {
        let work = |width, depth| {
            let ((out, work), optimizer) = hd_wasm::work::count_liveness(|| {
                hd_wasm::suspension_work::count(|| {
                    let source = program(width, true, depth, true);
                    compile(&source, EngineTarget::V8, false, OptLevel::Dev)
                })
            });
            assert!(out.wasm.is_some());
            eprintln!(
                "resident spill work width={width}, depth={depth}: {work:?}, optimizer={optimizer}"
            );
            (work, optimizer)
        };
        let (small, small_optimizer) = work(100, 2);
        let (wide, wide_optimizer) = work(1000, 2);
        for depth in [5, 10, 20] {
            let (nested, nested_optimizer) = work(1000, depth);
            for (phase, a, b, c) in [
                (
                    "instructions",
                    small.instructions,
                    wide.instructions,
                    nested.instructions,
                ),
                ("accesses", small.accesses, wide.accesses, nested.accesses),
                ("slots", small.slots, wide.slots, nested.slots),
                ("points", small.points, wide.points, nested.points),
                ("dispatch", small.dispatch, wide.dispatch, nested.dispatch),
                (
                    "optimizer",
                    small_optimizer,
                    wide_optimizer,
                    nested_optimizer,
                ),
            ] {
                assert!(a > 0 && b <= 14 * a, "{phase}: {a}, {b}");
                // Growing both dimensions catches a per-point walk of every
                // ancestor, not just the per-point scan of every live value.
                assert!(c <= 14 * a, "{phase}: {a}, {c}");
            }
        }
    });
}

/// #2453: local sharing's liveness (dev) and rule 13's dead accesses
/// (optimized) are linear in width and depth together: a loop-carried
/// local read at an outer header is live in the inner loops' blocks,
/// which the worklist fixed point found one nesting level per round.
#[test]
fn local_liveness_work_has_no_loop_depth_factor() {
    on_worker_stack(|| {
        let rows = [OptLevel::Dev, OptLevel::Optimized].map(|level| {
            [2, 5, 10, 20].map(|depth| {
                let (out, work) = hd_wasm::work::count_liveness(|| {
                    compile(&looped_program(1000, depth), EngineTarget::V8, false, level)
                });
                assert!(out.wasm.is_some());
                eprintln!("liveness {level:?}, depth={depth}: {work}");
                work
            })
        });
        for row in rows {
            assert!(
                row.iter().all(|&deep| deep * 10 <= row[0] * 13),
                "liveness at 2, 5, 10 and 20 loops (dev, optimized): {rows:?}"
            );
        }
    });
}

#[cfg(feature = "wasmtime")]
#[test]
fn one_thousand_real_awaits_execute_on_wasmtime() {
    on_worker_stack(|| {
        for checked in [false, true] {
            let out = compile(
                &program(1000, false, 0, true),
                EngineTarget::Wasmtime,
                checked,
                OptLevel::Dev,
            );
            if let Some(wasm) = out.wasm {
                let setup = hd_run::HostSetup {
                    grants: hd_run::Grants::default(),
                    limits: hd_run::Limits::default(),
                    args: Vec::new(),
                    program: String::new(),
                };
                let config = hd_run_wasmtime::EngineConfig::pipeline(hd_driver::OptLevel::Dev);
                let ran = hd_run_wasmtime::run_captured(config, &wasm, &setup);
                assert!(ran.is_ok(), "{ran:?}");
                if let Ok(ran) = ran {
                    assert_eq!(ran.status, 0, "{}", String::from_utf8_lossy(&ran.stderr));
                    assert_eq!(ran.stdout, b"1000\n");
                }
            }
        }
    });
}
