mod tests {
    use hd_wasm::testing::module::{Func, Ins};
    use hd_wasm::testing::op::Op as I;
    use hd_wasm::testing::opt::empty_loops::*;

    use hd_wasm::testing::module::Val;
    use wasm_encoder::BlockType::Empty;

    fn finite(step: i32) -> Vec<Ins> {
        vec![
            I::Block(Empty),
            I::Loop(Empty),
            I::LocalGet(1),
            I::LocalGet(0),
            I::I32LtU,
            I::I32Eqz,
            I::BrIf(1),
            I::LocalGet(1),
            I::I32Const(step),
            I::I32Add,
            I::LocalSet(1),
            I::Br(0),
            I::End,
            I::End,
        ]
        .into_iter()
        .map(Ins::Op)
        .collect()
    }

    fn function(body: Vec<Ins>, locals: usize) -> Func {
        let mut f =
            hd_wasm::testing::module::Module::default().func("empty", vec![Val::I32], vec![]);
        f.locals = vec![Val::I32; locals];
        f.body = body;
        f.origins = Some(vec![Some(7); f.body.len()]);
        f
    }

    #[test]
    fn deletes_only_finite_unobserved_effect_free_loops() {
        let mut f = function(finite(1), 1);
        assert!(run(&mut f));
        assert_eq!(f.body.len(), 2);
        assert_eq!(f.origins, Some(vec![Some(7); 2]));
        for body in [
            finite(2),
            {
                let mut body = finite(1);
                body.push(Ins::Op(I::LocalGet(1)));
                body.push(Ins::Op(I::Drop));
                body
            },
            {
                let mut body = finite(1);
                body.insert(8, Ins::Op(I::Unreachable));
                body
            },
            {
                let mut body = finite(1);
                body[4] = Ins::Op(I::I32LeU);
                body
            },
            {
                let mut body = finite(1);
                body[3] = Ins::Op(I::LocalGet(1));
                body
            },
        ] {
            let mut f = function(body, 1);
            assert!(!run(&mut f));
        }
    }

    #[test]
    fn signed_and_wide_strict_bounds_are_finite_but_calls_stay() {
        for comparison in [I::I32LtS, I::I64LtU, I::I64LtS] {
            let wide = !matches!(comparison, I::I32LtS);
            let mut body = finite(1);
            body[4] = Ins::Op(comparison);
            if wide {
                body[8] = Ins::Op(I::I64Const(1));
                body[9] = Ins::Op(I::I64Add);
            }
            let mut f = function(body, 1);
            if wide {
                f.params[0] = Val::I64;
                f.locals[0] = Val::I64;
            }
            assert!(run(&mut f));
        }
        let mut body = finite(1);
        body.insert(7, Ins::Call(hd_wasm::testing::module::SymId(0), None));
        let mut f = function(body, 1);
        assert!(!run(&mut f));
    }

    #[test]
    fn work_is_linear_at_width_1000_and_depth_20() {
        for width in [1, 1000] {
            let mut body = vec![Ins::Op(I::Block(Empty)); 20];
            for k in 0..width {
                let mut loop_body = finite(1);
                for ins in &mut loop_body {
                    if let Ins::Op(I::LocalGet(local) | I::LocalSet(local)) = ins
                        && *local == 1
                    {
                        *local += k;
                    }
                }
                body.extend(loop_body);
            }
            body.extend(vec![Ins::Op(I::End); 20]);
            let mut f = function(body, width as usize);
            let n = f.body.len();
            let mut visits = 0;
            assert!(rewrite(&mut f, &mut || visits += 1));
            assert!(visits <= 4 * n, "{visits} visits for {n} instructions");
        }
    }
}
