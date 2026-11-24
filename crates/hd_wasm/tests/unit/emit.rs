mod borrow;
mod control;
mod refs;
mod shape;
mod susp;
mod value;

mod frame_ref_tests {
    use hd_wasm::testing::emit::*;

    use hd_wasm::testing::layout::Enc;
    use hd_wasm::testing::module::{Func, Val};

    use hd_wasm::testing::frame_referents::{Guard, Plan, Referent};
    use hd_wasm::testing::layout::{Discr, Leaf};

    #[test]
    fn disjoint_variants_share_one_saved_owner_per_physical_value() {
        let discr = Discr {
            value: 0,
            leaf: Leaf {
                offset: 0,
                val: Val::I32,
                bytes: 4,
                signed: false,
            },
            enc: Enc::Direct,
        };
        let plan = Plan {
            guards: (0..1_000)
                .map(|variant| Guard {
                    parent: None,
                    discr,
                    variant,
                })
                .collect(),
            refs: (0..1_000)
                .map(|variant| Referent {
                    guard: Some(variant),
                    value: 1,
                })
                .collect(),
        };
        let mut f = Func::default();
        let saved = FrameRefs::new(0, plan, &mut f);
        assert_eq!(saved.plan.refs.len(), 1_000);
        assert_eq!(saved.held.len(), 1);
        assert_eq!(f.locals, [Val::I32]);
        assert_eq!(saved.conditional, [true]);
        assert!(saved.slots.iter().all(|&slot| slot == 0));
    }

    #[test]
    fn simultaneous_fields_keep_distinct_owners() {
        let plan = Plan {
            guards: vec![],
            refs: vec![
                Referent {
                    guard: None,
                    value: 0,
                },
                Referent {
                    guard: None,
                    value: 1,
                },
            ],
        };
        let mut f = Func::default();
        let saved = FrameRefs::new(0, plan, &mut f);
        assert_eq!(saved.held.len(), 2);
        assert_eq!(saved.slots, [0, 1]);
        assert_eq!(saved.conditional, [false, false]);
    }
}

use hd_base::Hash128;
use hd_wasm::testing::emit::*;
use hd_wasm::testing::module::{Global, Module, Sym, Val};
use hd_wasm::testing::op::Op as I;

/// A minimal handle body increments a counter on entry and returns it.
/// Execute the emitted cache twice: the body must have run only once.
#[test]
fn handle_once_cell_runs_its_body_once() {
    let cell = OnceCell {
        key: Hash128(616),
        results: vec![Val::I32],
        fact: None,
    };
    let counter = Global::Storage(Hash128(617), 0, Val::I32);
    let mut m = Module::default();
    let mut handle = m.func("handle", vec![], vec![Val::I32]);
    let cached = cell.load_cached(&mut handle);
    OnceCell::return_cached(&mut handle, &cached);
    handle.global_get(counter);
    handle.i32(1);
    handle.op(I::I32Add);
    let result = handle.local(Val::I32);
    handle.set(result);
    handle.get(result);
    handle.global_set(counter);
    cell.publish(&mut handle, &[result]);
    handle.get(result);
    let mut runs = m.func("runs", vec![], vec![Val::I32]);
    runs.global_get(counter);
    m.define(Sym::Inst(Hash128(616)), handle);
    m.export("handle", Sym::Inst(Hash128(616)));
    m.define(Sym::Inst(Hash128(617)), runs);
    m.export("runs", Sym::Inst(Hash128(617)));
    let wasm = m.finish().expect("link counter fixture");
    let mut child = hd_testkit::node::command()
        .args([
            "--input-type=module",
            "-e",
            r"
            const chunks = [];
            for await (const chunk of process.stdin) chunks.push(chunk);
            const {instance} = await WebAssembly.instantiate(Buffer.concat(chunks));
            const first = instance.exports.handle();
            const second = instance.exports.handle();
            const runs = instance.exports.runs();
            if (first !== 1 || second !== 1 || runs !== 1) {
                throw new Error(JSON.stringify({first, second, runs}));
            }
        ",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("start Node counter fixture");
    std::io::Write::write_all(&mut child.stdin.take().expect("fixture stdin"), &wasm)
        .expect("send counter fixture");
    let output = child.wait_with_output().expect("run counter fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
