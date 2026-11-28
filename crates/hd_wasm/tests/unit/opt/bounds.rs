mod tests {
    use hd_wasm::testing::opt::bounds::*;

    use hd_wasm::testing::module::{Func, Ins};
    use hd_wasm::testing::op::Op as I;
    use wasm_encoder::BlockType::Empty;

    use hd_base::Hash128;
    use hd_wasm::testing::module::{Module, Sym, Val};
    use std::io::Write;

    fn scan(last_offset: i32, escape: bool, table: bool) -> Func {
        let mut f = Module::default().func("scan", vec![Val::I32; 4], vec![Val::I32; 2]);
        f.locals = vec![Val::I32; 2];
        f.op(I::I32Const(-1));
        f.op(I::LocalSet(4));
        if escape {
            f.op(I::Block(Empty));
        }
        f.op(I::Block(Empty));
        f.op(I::Loop(Empty));
        for op in [
            I::LocalGet(0),
            I::I32Const(4),
            I::I32Add,
            I::LocalGet(1),
            I::I32LeU,
            I::I32Eqz,
            I::BrIf(1),
        ] {
            f.op(op);
        }
        for offset in [0, 1, 2, last_offset] {
            f.op(I::LocalGet(0));
            let index = if offset == 0 {
                0
            } else {
                f.op(I::I32Const(offset));
                f.op(I::I32Add);
                f.op(I::LocalTee(5));
                5
            };
            for op in [
                I::LocalGet(2),
                I::I32GeU,
                I::If(Empty),
                I::Unreachable,
                I::End,
                I::LocalGet(index),
                I::LocalGet(3),
                I::I32Eq,
                I::If(Empty),
                I::LocalGet(index),
                I::LocalSet(4),
            ] {
                f.op(op);
            }
            let target = if escape { 3 } else { 2 };
            if table {
                f.op(I::I32Const(0));
                f.br_table(vec![target], target);
            } else {
                f.op(I::Br(target));
            }
            f.op(I::End);
        }
        for op in [
            I::LocalGet(0),
            I::I32Const(4),
            I::I32Add,
            I::LocalSet(0),
            I::Br(0),
            I::End,
            I::End,
        ] {
            f.op(op);
        }
        if escape {
            for op in [I::I32Const(99), I::LocalSet(4), I::End] {
                f.op(op);
            }
        }
        f.op(I::LocalGet(4));
        f.op(I::LocalGet(0));
        f.origins = Some(vec![Some(7); f.body.len()]);
        f
    }

    fn wasm(f: Func) -> Vec<u8> {
        let mut module = Module::default();
        // These generated bodies have no symbolic calls or globals.
        let mut local = module.func(f.name, f.params, f.results);
        local.locals = f.locals;
        local.body = f.body;
        local.labels = f.labels;
        local.origins = f.origins;
        module.define(Sym::Inst(Hash128(1760)), local);
        module.export("scan", Sym::Inst(Hash128(1760)));
        let bytes = module.finish().expect("range test module");
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&bytes)
            .expect("valid versioned control flow");
        bytes
    }

    fn compare(base: Func) {
        let mut head = base.clone();
        assert!(run(&mut head));
        assert_eq!(head.origins.as_ref().map(Vec::len), Some(head.body.len()));
        let base = wasm(base);
        let head = wasm(head);
        let script = format!(
            r"
const a = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({base:?}))).exports.scan;
const b = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({head:?}))).exports.scan;
const call = (f, args) => {{try {{ return f(...args); }} catch(e) {{ if(e instanceof WebAssembly.RuntimeError) return 'trap'; throw e; }}}};
if(JSON.stringify(call(a,[0,8,1,0]))!=='[0,0]') throw Error('early exit must precede the invalid later index');
if(call(a,[0,8,1,-1])!=='trap' || call(a,[-3,5,5,0])!=='trap') throw Error('reached invalid indices must trap');
const cases = [
 [0,8,8,3], [0,8,8,-1], [4,12,12,7],
 [0,8,1,0], [0,8,1,-1], [2,3,0,-1],
 [-3,5,5,0], [-3,-1,-1,-3], [0,4,4,-1], [4,4,0,-1]
];
for (const args of cases) {{const x=call(a,args), y=call(b,args); if(JSON.stringify(x)!==JSON.stringify(y)) throw Error(JSON.stringify({{args,x,y}}));}}
console.log('ok');
"
        );
        // cost: one Node process per comparison of two fixed-size test modules.
        let mut child = hd_testkit::node::command()
            .arg("-")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // cost: the process makes ten bounded scan comparisons, independent of repository size.
            .spawn()
            .expect("node");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(script.as_bytes())
            .expect("script");
        let result = child.wait_with_output().expect("range comparisons");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(result.stdout, b"ok\n");
    }

    #[test]
    fn versions_preserve_early_exits_zero_trips_and_wraparound() {
        compare(scan(3, false, false));
        compare(scan(4, false, false));
    }

    #[test]
    fn versions_adjust_branches_and_branch_tables_past_the_region() {
        compare(scan(3, true, false));
        compare(scan(3, true, true));
    }

    #[test]
    fn changing_lengths_bounds_and_unproved_counter_updates_keep_checks() {
        for local in [1, 2, 0] {
            let mut f = scan(3, false, false);
            let end = f
                .body
                .iter()
                .rposition(|x| matches!(x, Ins::Op(I::Br(0))))
                .expect("backedge");
            f.body.splice(
                end..end,
                [Ins::Op(I::I32Const(0)), Ins::Op(I::LocalSet(local))],
            );
            f.origins = None;
            assert!(!run(&mut f));
        }
    }

    #[test]
    fn version_work_and_output_are_linear_at_width_1000_depth_20() {
        for (width, depth) in [(1usize, 1usize), (1000, 1), (1, 20), (1000, 20)] {
            let one = scan(3, false, false);
            let region = &one.body[2..one.body.len() - 2];
            let mut f = one.clone();
            f.results.clear();
            f.body = [I::Block(Empty), I::Loop(Empty)]
                .into_iter()
                .map(Ins::Op)
                .cycle()
                .take(2 * depth)
                .collect();
            for _ in 0..width {
                f.body.extend_from_slice(region);
            }
            f.body.extend(vec![Ins::Op(I::End); 2 * depth]);
            f.origins = Some(vec![Some(7); f.body.len()]);
            let n = f.body.len();
            let mut work = 0;
            assert!(rewrite(&mut f, &mut || work += 1));
            assert!(work <= 8 * n, "{work} > 8*{n}");
            assert!(f.body.len() <= 3 * n);
            assert_eq!(
                f.body
                    .iter()
                    .filter(|i| matches!(i, Ins::Op(I::Loop(_))))
                    .count(),
                2 * width + depth
            );
            assert_eq!(f.origins.as_ref().map(Vec::len), Some(f.body.len()));
        }
    }
}
