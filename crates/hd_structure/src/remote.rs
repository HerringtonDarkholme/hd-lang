//! The generated parts of remote traits (`module.remote.stub`,
//! `module.remote.handle-call`): a `Stub` as `*mut dyn T`, each method of
//! the std `Stub` at a remote trait, and the dispatcher `serve[S]` of a
//! root or of a returned object's trait value. Each
//! is a few calls of `std.remote`'s plain hd, which frames, encodes,
//! decodes and refuses; only what names a trait's methods is generated.

use hd_base::{DefId, LocalId, StageResult};
use hd_resolve::known::REMOTE_MODULE as REMOTE;
use hd_tir::Body;
use hd_tir::ir::{
    Callee, ChoiceKind, Coercion, NONE, Place, PlaceRoot, PrimOp, Proj, Providers, ReadMode, Ref,
    Tag, TirSink, local_flags,
};
use hd_types::{ParamRef, Prim, Ty, TyData, TyList};

use super::{Cx, Gen, Handles, SYN, param, subst_sig};

/// A remote trait a root implements, at the root's trait arguments, with
/// the impl that answers it, none for a trait value's own dispatch, and
/// its interface number (`module.remote.interface`).
pub struct Served {
    pub trait_: DefId,
    pub args: TyList,
    pub impl_: Option<DefId>,
    pub interface: u64,
}

/// `stub_as[T]`'s body at the trait value `tv`: its `*mut Stub` argument
/// as `*mut dyn T`, which `Stub: T` answers (`module.remote.stub`).
pub fn stub_as_body(cx: &Cx<'_, '_>, item: DefId, tv: Ty) -> StageResult<Body> {
    let names = cx.names;
    let mut g = Gen::new(cx, tv, item, Handles::Supplied);
    let sig = subst_sig(names, cx.method_sig(item), &|p: ParamRef| {
        (p.owner == item).then_some(tv)
    });
    let [(held, boxed)] = g.params(&sig);
    let blk = g.b.open_block();
    let addr = g.b.open_block();
    let source = g.b.get(held, boxed, SYN);
    let value = g.b.coerce(Coercion::RefToTraitValue, NONE, source, tv, SYN);
    let addr = g.b.close_block(addr, Some(value), tv, SYN);
    let reference = g.b.emit(Tag::RefDyn, addr.0, NONE, sig.ret, SYN);
    let root = g.b.close_block(blk, Some(reference), sig.ret, SYN);
    g.finish(root).map(|(body, _)| body)
}

/// Method `method` of the remote trait `trait_` at the std `Stub` type
/// `stub`: one `stub_call!` with the interface of `trait_` itself, so a
/// supertrait's method names its own interface whatever trait the stub was
/// made for, the method's number, its declaration index, and its arguments
/// as one tuple (`module.remote.stub.send`). The trait's own parameters
/// stay, so the body serves every instance.
pub fn stub_method_body(
    cx: &Cx<'_, '_>,
    trait_: DefId,
    method: DefId,
    stub: Ty,
) -> StageResult<Body> {
    let names = cx.names;
    let pool = names.pool;
    let number = cx
        .trait_data(trait_)
        .methods
        .iter()
        .position(|&(_, m)| m == method)
        .unwrap_or(0);
    let sig = subst_sig(names, cx.method_sig(method), &|p: ParamRef| {
        (p.owner == trait_ && p.index == 0).then_some(stub)
    });
    let mut g = Gen::new(cx, stub, method, Handles::Supplied);
    let params: Vec<_> = (sig.params.iter())
        .map(|&(n, t)| (g.b.local(t, n, local_flags::PARAM, SYN), t))
        .collect();
    let (value, error) = public_parts(cx, sig.ret);
    let blk = g.b.open_block();
    let receiver = g.b.get(params[0].0, params[0].1, SYN);
    let fields: Vec<Ref> = (params[1..].iter())
        .map(|&(l, t)| g.b.get(l, t, SYN))
        .collect();
    let arg_tys: Vec<Ty> = params[1..].iter().map(|p| p.1).collect();
    let args_ty = tuple(pool, &arg_tys);
    let rec = g.b.refs_record(&fields);
    let args = g.b.emit(Tag::NewTuple, NONE, rec, args_ty, SYN);
    let k =
        g.b.const_value(Ty::prim(Prim::U64), u64::try_from(number).unwrap_or(0));
    let generics = cx.items.item(trait_).map_or(0, |i| i.generics.len());
    // ruled out: a trait declares fewer than 2^16 type parameters.
    let own: Vec<Ty> = (1..=u16::try_from(generics).unwrap_or(u16::MAX))
        .map(|i| param(names, trait_, i))
        .collect();
    let tv = pool.intern_ty(&TyData::TraitValue {
        def: trait_,
        args: pool.list(&own),
        bindings: vec![],
    });
    let interface = g.std_call("interface_of", &[tv], &[], Ty::prim(Prim::U64));
    let call = g.std_call(
        "stub_call",
        &[args_ty, value, error],
        &[receiver, interface, k, args],
        sig.ret,
    );
    g.b.body_mut().tags[call.0 as usize] = Tag::Await;
    let root = g.b.close_block(blk, Some(call), sig.ret, SYN);
    g.finish(root).map(|(body, _)| body)
}

/// `serve[S]`'s body at `root`, a root type or a returned object's trait
/// value, for a frame whose head `serve_open!` accepted: for each remote
/// trait in `served` a test of its interface, and for each of its methods
/// an arm that decodes the arguments, calls the implementation and encodes
/// its public result. Any other frame is `WrongHome`. Cost: one comparison
/// per served trait and one per method of the trait the frame names.
pub fn serve_body(cx: &Cx<'_, '_>, item: DefId, root: Ty, served: &[Served]) -> StageResult<Body> {
    let names = cx.names;
    let sig = subst_sig(names, cx.method_sig(item), &|p: ParamRef| {
        (p.owner == item).then_some(root)
    });
    let mut g = Gen::new(cx, root, item, Handles::Supplied);
    let [(s, s_ty), (head, head_ty)] = g.params(&sig);
    let bytes = sig.ret;
    let blk = g.b.open_block();
    let interface = g.read_field(head, 1, Ty::prim(Prim::U64));
    // Each test's `else` holds the next test, so the blocks open forward
    // and close inward-out.
    let mut tests = Vec::with_capacity(served.len());
    for one in served {
        let methods = g.serve_methods(one, (s, s_ty), (head, head_ty), root, bytes);
        let number = g.b.const_value(Ty::prim(Prim::U64), one.interface);
        let same =
            g.b.prim(PrimOp::Eq as u32, &[interface, number], Ty::BOOL, SYN);
        tests.push((same, methods, g.b.open_block()));
    }
    let mut tail = g.std_call("serve_wrong_home", &[], &[], bytes);
    for (same, methods, other) in tests.into_iter().rev() {
        let other = g.b.close_block(other, Some(tail), bytes, SYN);
        let rec = g.b.refs_record(&[methods, other]);
        tail = g.b.emit(Tag::If, same.0, rec, bytes, SYN);
    }
    let body = g.b.close_block(blk, Some(tail), bytes, SYN);
    g.finish(body).map(|(body, _)| body)
}

/// The tuple type of `elems`.
fn tuple(pool: &hd_types::InternPool, elems: &[Ty]) -> Ty {
    pool.intern_ty(&TyData::Tuple {
        elems: pool.list(elems),
        rest: None,
    })
}

/// The body that returns `value`: `build_id()`'s, the build ID which
/// std's framing reads (`module.remote.handle-call.build`), and
/// `interface_of[T]()`'s, `T`'s interface number (`module.remote.interface`).
pub fn u64_body(cx: &Cx<'_, '_>, item: DefId, value: u64) -> StageResult<Body> {
    let u64_ = Ty::prim(Prim::U64);
    let mut g = Gen::new(cx, u64_, item, Handles::Supplied);
    let blk = g.b.open_block();
    let id = g.b.const_value(u64_, value);
    let root = g.b.close_block(blk, Some(id), u64_, SYN);
    g.finish(root).map(|(body, _)| body)
}

/// `Result[V, E | CrossError]`'s `V` and `E` (`module.remote.r2.result`).
fn public_parts(cx: &Cx<'_, '_>, ret: Ty) -> (Ty, Ty) {
    let pool = cx.names.pool;
    let (_, args) = pool.adt_parts(ret);
    // ruled out: R1 and R2 (hd_check remote.rs) fix every remote method's
    // result to `Result[V, E | CrossError]` before collection runs.
    let [value, union] = *pool.list_items(args) else {
        return (Ty::POISON, Ty::POISON);
    };
    let error = match pool.get(union) {
        TyData::Union(members) => pool.list_items(members)[0],
        _ => Ty::POISON,
    };
    (value, error)
}

impl Gen<'_, '_> {
    /// A call of `std.remote`'s `name` at `targs`.
    fn std_call(&mut self, name: &str, targs: &[Ty], args: &[Ref], ret: Ty) -> Ref {
        let callee = Callee::Item {
            def: self.names.item(REMOTE, name),
            targs: self.names.pool.list(targs),
        };
        self.b.call(&callee, args, Providers::None, ret, SYN)
    }

    /// `Read [$l .k]`.
    fn read_field(&mut self, l: hd_base::LocalId, k: u32, ty: Ty) -> Ref {
        let word = self.b.place(&Place {
            root: PlaceRoot::Local(l),
            proj: vec![Proj::Field(k)],
        });
        self.b.emit(Tag::Read, word, ReadMode::Use as u32, ty, SYN)
    }

    /// The block that switches on the frame's method number over `one`'s
    /// methods, each arm one call of the root's implementation, or through
    /// the trait value `root` when `one` names no impl.
    fn serve_methods(
        &mut self,
        one: &Served,
        (s, s_ty): (LocalId, Ty),
        (head, head_ty): (LocalId, Ty),
        root: Ty,
        bytes: Ty,
    ) -> Ref {
        let names = self.names;
        let pool = names.pool;
        let trait_ = one.trait_;
        let targs = pool.list_items(one.args).to_vec();
        let methods = self.cx.trait_data(trait_).methods.clone();
        let blk = self.b.open_block();
        let number = self.read_field(head, 2, Ty::prim(Prim::U32));
        let mut arms = Vec::with_capacity(methods.len());
        for (k, &(_, method)) in methods.iter().enumerate() {
            let sig = subst_sig(names, self.cx.method_sig(method), &|p: ParamRef| {
                (p.owner == trait_).then(|| match p.index {
                    0 => root,
                    i => targs[usize::from(i) - 1],
                })
            });
            let arg_tys: Vec<Ty> = sig.params[1..].iter().map(|p| p.1).collect();
            let args_ty = tuple(pool, &arg_tys);
            let arm = self.b.open_block();
            let h = self.b.get(head, head_ty, SYN);
            let decoded = self.adt(names.known.option, &[args_ty]);
            let a = self.std_call("serve_args", &[args_ty], &[h], decoded);
            let some = self.b.open_block();
            let tuple = self.b.payload(a, 1, 0, args_ty, SYN);
            let decoded_args = self.b.local(args_ty, names.syms.intern("$args"), 0, SYN);
            self.b.set(decoded_args, tuple, SYN);
            let receiver = if one.impl_.is_some() {
                self.b.get(s, s_ty, SYN)
            } else {
                let place = self.b.place(&Place {
                    root: PlaceRoot::Local(s),
                    proj: vec![Proj::Deref],
                });
                self.b
                    .emit(Tag::Read, place, ReadMode::Use as u32, root, SYN)
            };
            let mut refs = vec![receiver];
            for (i, &t) in (0u32..).zip(&arg_tys) {
                refs.push(self.read_field(decoded_args, i, t));
            }
            let callee = Callee::TraitMethod {
                trait_,
                method,
                self_ty: root,
                targs: one.args,
                choice: one
                    .impl_
                    .map_or((ChoiceKind::TraitValue, trait_.raw()), |i| {
                        (ChoiceKind::Impl, i.raw())
                    }),
            };
            let result = self.b.call(&callee, &refs, Providers::None, sig.ret, SYN);
            self.b.body_mut().tags[result.0 as usize] = Tag::Await;
            let h = self.b.get(head, head_ty, SYN);
            let ran = self.std_call("serve_ran", &[sig.ret], &[h, result], bytes);
            let some = self.b.close_block(some, Some(ran), bytes, SYN);
            let none = self.b.open_block();
            let wrong = self.std_call("serve_wrong_home", &[], &[], bytes);
            let none = self.b.close_block(none, Some(wrong), bytes, SYN);
            let chosen = self.switch_tag(a, 1, some, none, bytes);
            let arm = self.b.close_block(arm, Some(chosen), bytes, SYN);
            // ruled out: a trait declares fewer than 2^32 methods.
            arms.push((
                u32::try_from(k).unwrap_or(u32::MAX),
                arm,
                self.b.open_block(),
            ));
        }
        let mut tail = self.std_call("serve_wrong_home", &[], &[], bytes);
        for (k, arm, other) in arms.into_iter().rev() {
            let other = self.b.close_block(other, Some(tail), bytes, SYN);
            tail = self.switch_int(number, k, arm, other, bytes);
        }
        self.b.close_block(blk, Some(tail), bytes, SYN)
    }
}
