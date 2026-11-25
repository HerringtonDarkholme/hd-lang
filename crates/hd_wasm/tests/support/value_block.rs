use hd_base::NodeIdx;
use hd_tir::ir::{Body, NONE, Phase, PrimOp, ReadMode, Ref, Tag, place};
use hd_types::{Prim, Ty};

fn inst(b: &mut Body, tag: Tag, a: u32, w: u32, ty: Ty) -> u32 {
    let i = u32::try_from(b.len()).unwrap();
    b.tags.push(tag);
    b.data.push([a, w]);
    b.ty.push(ty);
    b.syn.push(NodeIdx::NONE);
    i
}

fn block(b: &mut Body, items: &[u32], tail: u32, ty: Ty) -> u32 {
    let list = b.push_record(items);
    inst(b, Tag::Block, list, tail, ty)
}

/// The landing arm breaks either to the value join (5) or to its own
/// void landing. The other value flows through a constant-tail Scope.
/// An extra wrapper models the value destination of an inlined body.
fn transform(path: &str, original: &Body) -> Option<Body> {
    let name = path.rsplit('/').next()?;
    let (nested, fallback) = match name {
        "landing_plain_zero" => (false, 0),
        "landing_plain_nonzero" => (false, 7),
        "landing_nested_zero" => (true, 0),
        "landing_nested_nonzero" => (true, 7),
        _ => return None,
    };
    let mut b = Body::new(original.item, original.kind);
    b.phase = Phase::Lowered;
    b.local_ty.clone_from(&original.local_ty);
    b.local_name.clone_from(&original.local_name);
    b.local_syn.clone_from(&original.local_syn);
    b.local_flags.clone_from(&original.local_flags);
    b.sub_params.clone_from(&original.sub_params);
    b.sub_parent.clone_from(&original.sub_parent);
    b.sub_flags.clone_from(&original.sub_flags);
    b.consts = vec![
        (Ty::prim(Prim::Usize), 0),
        (Ty::prim(Prim::Usize), 5),
        (Ty::prim(Prim::Usize), fallback),
    ];
    let zero = Ref::konst(0).0;
    let five = Ref::konst(1).0;
    let fallback = Ref::konst(2).0;
    let n = inst(
        &mut b,
        Tag::Read,
        place::BARE_LOCAL,
        ReadMode::Copy as u32,
        Ty::prim(Prim::Usize),
    );
    let operands = b.push_record(&[n, zero]);
    let cond = inst(&mut b, Tag::Prim, PrimOp::Eq as u32, operands, Ty::BOOL);
    let take = inst(&mut b, Tag::Break, 0, five, Ty::NEVER);
    let yes = block(&mut b, &[take], NONE, Ty::VOID);
    let skip = inst(&mut b, Tag::Break, 1, NONE, Ty::NEVER);
    let no = block(&mut b, &[skip], NONE, Ty::VOID);
    let arms = b.push_record(&[yes, no]);
    let branch = inst(&mut b, Tag::If, cond, arms, Ty::VOID);
    let landing = block(&mut b, &[n, cond, yes, no, branch], NONE, Ty::VOID);
    let suites = b.push_record(&[]);
    let first = inst(&mut b, Tag::Scope, landing, suites, Ty::VOID);
    let last = block(&mut b, &[], fallback, Ty::prim(Prim::Usize));
    let tail = inst(&mut b, Tag::Scope, last, suites, Ty::prim(Prim::Usize));
    let join = block(
        &mut b,
        &[landing, first, last, tail],
        tail,
        Ty::prim(Prim::Usize),
    );
    b.label_inst = vec![join, landing];
    let scope = inst(&mut b, Tag::Scope, join, suites, Ty::prim(Prim::Usize));
    let result = if nested {
        let exit = inst(&mut b, Tag::Break, 2, scope, Ty::NEVER);
        let wrapper = block(&mut b, &[join, scope, exit], NONE, Ty::prim(Prim::Usize));
        b.label_inst.push(wrapper);
        let nested_value = inst(&mut b, Tag::Scope, wrapper, suites, Ty::prim(Prim::Usize));
        block(
            &mut b,
            &[wrapper, nested_value],
            nested_value,
            Ty::prim(Prim::Usize),
        )
    } else {
        block(&mut b, &[join, scope], scope, Ty::prim(Prim::Usize))
    };
    b.sub_root.push(result);
    let errors = hd_tir::ir::verify(&b);
    assert!(errors.is_empty(), "{name}: {errors:?}");
    Some(b)
}

pub fn view(
    name: &str,
    build: hd_testkit::stages::Build,
    p: &hd_testkit::stages::Program,
) -> Option<String> {
    if p.text
        .contains("# Hand-built emitter value-block fixture (#2373).")
    {
        hd_wasm::testing::emit::with_body_transform(transform, || {
            hd_testkit::stages::view(name, build, p)
        })
    } else {
        hd_testkit::stages::view(name, build, p)
    }
}
