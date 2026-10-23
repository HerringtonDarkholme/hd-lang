//! The inliner's body copy (`docs/compiler/tir-v2.md`, "The inliner"): a
//! `Call` of a small body becomes a copy of that body, run before the
//! ownership passes so they see the combined body. The copy's locals,
//! labels, constants, strings and records move past the caller's; its
//! parameters are bound with `Store`s of the arguments; each `Return` is a
//! `Break` to its root block, which a `Scope` with no suites owns in the
//! call's place. The caller substitutes the call's type arguments first
//! ([`crate::ir::map_body_types`]); choosing what to inline is the
//! driver's.

use hd_base::{DefId, NodeIdx};
use hd_types::{Ty, TyList};

use crate::ir::{Body, BodyKind, Callee, ChoiceKind, INLINED, NONE, Op, Tag, local_flags, place};
use crate::own::splice::{permute_columns, renumber_through};

/// The callee of call `i` when it may inline: a `Call` with no providers
/// and no omitted argument.
#[must_use]
pub fn plain_call(b: &Body, i: usize) -> Option<Callee> {
    if b.tags[i] != Tag::Call {
        return None;
    }
    let r = b.record(b.data[i][1]);
    let (args, prov) = r.split_at(r.len().checked_sub(3)?);
    let default = args
        .iter()
        .any(|&a| b.tags.get(a as usize) == Some(&Tag::DefaultCall));
    if prov[0] != 0 || default {
        return None;
    }
    Callee::from_words(b.record(b.data[i][0]))
}

/// Chooses each `Bound` trait call of `k` again as an `Impl` call of the
/// impl `pick` names for its trait, receiver type and type arguments, as
/// substitution may have made them concrete.
pub fn rechoose(k: &mut Body, pick: &dyn Fn(DefId, Ty, TyList) -> Option<DefId>) {
    for i in 0..k.len() {
        let at = k.data[i][0] as usize;
        if k.tags[i] == Tag::Call
            && let Some(Callee::TraitMethod {
                trait_,
                self_ty,
                targs,
                choice: (ChoiceKind::Bound, _),
                ..
            }) = Callee::from_words(k.record(k.data[i][0]))
            && let Some(d) = pick(trait_, self_ty, targs)
        {
            // `[len, 1, trait, method, self_ty, targs, kind, value]`.
            k.extra[at + 6] = ChoiceKind::Impl as u32;
            k.extra[at + 7] = d.raw();
        }
    }
}

/// Whether `c` may be copied: a function of one sub-body, its root block
/// last, of at most `limit` instructions, with no suspension, `defer`,
/// closure, provider or ownership statement, and no call with providers.
#[must_use]
pub fn inlinable(c: &Body, limit: usize) -> bool {
    let n = c.len();
    let refused = |i: usize| match c.tags[i] {
        Tag::Call => c.record(c.data[i][1]).iter().rev().nth(2) != Some(&0),
        t => matches!(
            t,
            Tag::Await
                | Tag::AwaitValue
                | Tag::AwaitAll
                | Tag::AwaitRace
                | Tag::Hook
                | Tag::Defer
                | Tag::Closure
                | Tag::ProviderGet
                | Tag::ContextFor
                | Tag::ContextNew
                | Tag::With
                | Tag::CallDyn
                | Tag::CallHost
                | Tag::SwitchInt
                | Tag::SwitchChar
                | Tag::SwitchStr
                | Tag::Hole
                | Tag::Poison
                | Tag::Drop
                | Tag::IsUnique
                | Tag::Pin
                | Tag::FlagSet
                | Tag::FlagClear
        ),
    };
    c.kind == BodyKind::Fn
        && (1..=limit).contains(&n)
        && c.sub_root == [u32_of(n - 1)]
        && c.susp.is_empty()
        && c.cap_local.is_empty()
        && c.label.is_empty()
        && !(0..n).any(refused)
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).expect("TIR column")
}

/// Replaces `Call` `c` of `b` by `k`, a copy of the callee whose types
/// are already the call's. Instructions keep evaluation order: the copy
/// stands where the call stood, the `Scope` that holds it last.
///
/// # Errors
/// A callee whose parameters do not match the call's arguments, a call in
/// no block, or a caller the renumbering refuses (a `CallDyn`, a call's
/// providers of an unknown kind); `b` is then unchanged.
pub fn inline_call(b: &mut Body, c: usize, mut k: Body) -> Result<(), String> {
    let mut params = Vec::new();
    for l in 0..k.local_flags.len() {
        if k.local_flags[l] & local_flags::PARAM != 0 {
            params.push(u32_of(l));
        }
    }
    let r = b.record(b.data[c][1]);
    let args = r[..r.len().saturating_sub(3)].to_vec();
    let outer = (0..b.len())
        .find(|&i| b.tags[i] == Tag::Block && b.record(b.data[i][0]).contains(&u32_of(c)));
    let refused = |i: usize| {
        let kind = || b.record(b.data[i][1]).iter().rev().nth(2).copied();
        b.tags[i] == Tag::CallDyn
            || (matches!(b.tags[i], Tag::Call | Tag::Await) && kind().is_some_and(|p| p > 2))
    };
    let (Some(outer), false, false) = (
        outer,
        args.len() != params.len() || k.is_empty(),
        (0..b.len()).any(refused),
    ) else {
        return Err("a call this copy cannot replace".into());
    };
    let (m, np) = (k.len(), params.len());
    let base = u32_of(c);
    // The copy, numbered as it will stand: the `Store`s, the callee's
    // instructions but its root, the final `Break`, the root, the `Scope`.
    let root_at = base + u32_of(np + m);
    let mut new_of = Vec::with_capacity(m);
    for j in 0..m {
        new_of.push(if j + 1 == m {
            root_at
        } else {
            base + u32_of(np + j)
        });
    }
    renumber_through(&mut k, new_of, u32_of(b.consts.len()))?;
    let shift = u32_of(np + m + 1);
    let mut moved = Vec::with_capacity(b.len());
    for i in 0..b.len() {
        moved.push(u32_of(i) + if i >= c { shift } else { 0 });
    }
    let out = b;
    renumber_through(out, moved, 0)?;
    let (xbase, lbase) = (u32_of(out.extra.len()), u32_of(out.local_ty.len()));
    let label = u32_of(out.label_inst.len() + k.label_inst.len());
    relocate(&mut k, xbase, lbase, u32_of(out.label_inst.len()), label);
    let syn = out.syn[c];
    let ty = out.ty[c];
    out.extra.append(&mut k.extra);
    // The bindings stand right before the `Scope` in the call's block, so
    // an argument is consumed in the block that made it; the block also
    // lists the root, which the `Scope` runs.
    let scope = root_at + 1;
    let mut list = Vec::new();
    for &x in out.record(out.data[outer][0]) {
        if x == scope {
            for p in 0..np {
                list.push(base + u32_of(p));
            }
            list.push(root_at);
        }
        list.push(x);
    }
    out.data[outer][0] = push_record(out, &list);
    // The root's members: the callee's, then the final `Break`.
    let [members, tail] = k.data[m - 1];
    let mut list = out.record(members).to_vec();
    list.push(root_at - 1);
    let rec = push_record(out, &list);
    let empty = push_record(out, &[]);
    // The new instructions go at the columns' end, then into place by one
    // permutation, as a pass's rebuild does.
    if out.anchors.is_empty() {
        out.anchors = (0..u32_of(out.len())).collect();
    }
    // The copy's own instructions name the callee's as their anchors
    // ([`Body::inlined`]), so a panic in the copy reports the callee's
    // frame, then the call's; one without a position keeps none, as in
    // the callee, and so reports its enclosing one.
    let (n0, anchor) = (out.len(), out.anchors[c]);
    let first = out.inlined.last().map_or(0, |e| e[0] + e[1]);
    out.inlined.push([first, u32_of(m), k.item.raw(), anchor]);
    let mut put = |tag: Tag, data: [u32; 2], ty: Ty, at: Option<usize>| {
        out.tags.push(tag);
        out.data.push(data);
        out.ty.push(ty);
        let own = at.map(|j| (k.syn[j], INLINED | (first + u32_of(j))));
        out.syn.push(match own {
            Some((NodeIdx::NONE, _)) => NodeIdx::NONE,
            _ => syn,
        });
        out.anchors.push(own.map_or(anchor, |o| o.1));
    };
    for (&p, &a) in params.iter().zip(&args) {
        put(
            Tag::Store,
            [(p + lbase) | place::BARE_LOCAL, a],
            Ty::VOID,
            None,
        );
    }
    for j in 0..m - 1 {
        put(k.tags[j], k.data[j], k.ty[j], Some(j));
    }
    put(Tag::Break, [label, tail], Ty::NEVER, None);
    // The root takes the call's type, which its `Break`s carry.
    put(Tag::Block, [rec, NONE], ty, Some(m - 1));
    put(Tag::Scope, [root_at, empty], ty, None);
    let mut order = Vec::with_capacity(out.len());
    for i in (0..c).chain(n0..out.len()).chain(c + 1..n0) {
        order.push(u32_of(i));
    }
    permute_columns(out, &order);
    out.label_inst.extend_from_slice(&k.label_inst);
    out.label_inst.push(root_at);
    let sbase = out.strings.len() as u64;
    for l in 0..k.local_ty.len() {
        out.local_ty.push(k.local_ty[l]);
        out.local_name.push(k.local_name[l]);
        out.local_syn.push(syn);
        out.local_flags.push(k.local_flags[l] & !local_flags::PARAM);
    }
    for &(t, bits) in &k.consts {
        out.consts
            .push((t, if t == Ty::STRING { bits + sbase } else { bits }));
    }
    out.strings.append(&mut k.strings);
    // The copy's fallback rows, sorted, go where its instructions stand.
    let rest = out
        .fallback
        .split_off(out.fallback.partition_point(|r| r[0] < base));
    for &[i, l] in &k.fallback {
        out.fallback.push([i, if l == 0 { 0 } else { l + lbase }]);
    }
    out.fallback.extend_from_slice(&rest);
    Ok(())
}

/// Moves the copy's record words `xbase` on, its locals `lbase` on and
/// its labels `lab` on, and turns each `Return` into a `Break` to `label`.
fn relocate(k: &mut Body, xbase: u32, lbase: u32, lab: u32, label: u32) {
    let mut places = Vec::new();
    for i in 0..k.len() {
        let (oa, ob) = k.tags[i].operands();
        for (x, op) in [oa, ob].into_iter().enumerate() {
            let w = &mut k.data[i][x];
            match op {
                Op::Values | Op::Blocks | Op::Record if *w != NONE => *w += xbase,
                Op::Place if *w & place::BARE_LOCAL != 0 => *w += lbase,
                Op::Place => {
                    places.push(*w as usize);
                    *w += xbase;
                }
                Op::Label => *w += lab,
                // `ItemRef`'s first word is a record too.
                Op::Meta if k.tags[i] == Tag::ItemRef => *w += xbase,
                _ => {}
            }
        }
        if k.tags[i] == Tag::Return {
            k.tags[i] = Tag::Break;
            k.data[i] = [label, k.data[i][0]];
        }
    }
    places.sort_unstable();
    places.dedup();
    for at in places {
        if k.extra[at + 1] == place::ROOT_LOCAL {
            k.extra[at + 2] += lbase;
        }
    }
}

fn push_record(b: &mut Body, words: &[u32]) -> u32 {
    let at = u32_of(b.extra.len());
    b.extra.push(u32_of(words.len()));
    b.extra.extend_from_slice(words);
    at
}

#[cfg(test)]
mod tests {
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::{Ty, TyList};

    use super::{inlinable, inline_call, plain_call, rechoose, u32_of};
    use crate::ir::{
        Body, BodyKind, Callee, ChoiceKind, Providers, Ref, Tag, TirBuilder, TirSink, local_flags,
        map_body_types, verify, verify_owned,
    };
    use crate::own::tests::{Cx, show};
    use crate::own::{OwnScratch, run};

    const N: NodeIdx = NodeIdx::from_raw(1);
    /// The callee's type parameter, which the call substitutes by `string`.
    const T: Ty = Ty(1_000_001);

    /// `fn f(x: T) -> T { loop { if true { return x } break }; "s" }`:
    /// a loop, a `return` inside it, a constant and a string.
    fn callee() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(3), BodyKind::Fn);
        let blk = b.open_block();
        let x = b.local(T, Symbol::from_raw(0), local_flags::PARAM, N);
        let lp = b.open_loop();
        let body = b.open_block();
        let yes = b.const_value(Ty::BOOL, 1);
        let then = b.open_block();
        let v = b.get(x, T, N);
        let ret = b.emit(Tag::Return, v.0, crate::ir::NONE, Ty::NEVER, N);
        let then = b.close_block(then, Some(ret), Ty::NEVER, N);
        let other = b.open_block();
        let other = b.close_block(other, None, Ty::VOID, N);
        let arms = b.refs_record(&[then, other]);
        b.emit(Tag::If, yes.0, arms, Ty::VOID, N);
        b.emit(Tag::Break, lp.0.raw(), crate::ir::NONE, Ty::NEVER, N);
        let body = b.close_block(body, None, Ty::VOID, N);
        b.close_loop(lp, body, Ty::VOID, N);
        let s = b.const_str("s");
        let root = b.close_block(blk, Some(s), T, N);
        let mut c = b.finish(root, &[]).expect("callee verifies");
        map_body_types(&mut c, &|t| if t == T { Ty::STRING } else { t }, &|l| l);
        c
    }

    /// `g() { 9(3(7())) }`, with call 3 inlined.
    fn caller() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let blk = b.open_block();
        let s = b.const_str("caller");
        let item = |d| Callee::Item {
            def: DefId::from_raw(d),
            targs: TyList::EMPTY,
        };
        let a = b.call(&item(7), &[s], Providers::None, Ty::STRING, N);
        let r = b.call(&item(3), &[a], Providers::None, Ty::STRING, N);
        b.call(&item(9), &[r], Providers::None, Ty::VOID, N);
        let root = b.close_block(blk, None, Ty::VOID, N);
        b.finish(root, &[]).expect("caller verifies")
    }

    /// The copy binds the parameter with a `Store` in the call's block,
    /// turns the `return` into a `Break` to its root block, which a `Scope`
    /// owns in the call's place, and keeps the caller's constants apart
    /// from its own. The ownership passes then move the argument into the
    /// parameter, move the parameter out at the `return`, and drop it after
    /// the `Scope` on the path that breaks the loop.
    #[test]
    fn a_call_becomes_its_callee_bound_and_breaking_to_its_block() {
        let mut b = caller();
        let c = callee();
        assert!(inlinable(&c, 9) && !inlinable(&c, 8));
        let item = Callee::Item {
            def: DefId::from_raw(3),
            targs: TyList::EMPTY,
        };
        assert_eq!(plain_call(&b, 1), Some(item));
        inline_call(&mut b, 1, c).expect("inlines");
        // Anchors are the driver's, which the checker's TIR has none of.
        let mut v = b.clone();
        v.anchors.clear();
        assert_eq!(verify(&v), []);
        // The binding, the `Break` and the `Scope` stand for the call; the
        // callee's instructions for themselves, called at anchor 1.
        let i = |j| crate::ir::INLINED | j;
        assert_eq!(
            b.anchors,
            [
                0,
                1,
                i(0),
                i(1),
                i(2),
                i(3),
                i(4),
                i(5),
                i(6),
                i(7),
                1,
                i(8),
                1,
                2,
                3
            ]
        );
        assert_eq!(b.inlined_frame(i(4)), Some((DefId::from_raw(3), 4, 1)));
        assert_eq!(b.inlined_frame(1), None);
        let k = b.consts.len() - 1;
        let s = b.consts[k];
        assert_eq!((s.0, s.1, &*b.strings[1]), (Ty::STRING, 1, "s"));
        let root = b.label_inst[1];
        assert_eq!(
            (b.tags[12], root, b.data[root as usize][1]),
            (Tag::Scope, 11, crate::ir::NONE)
        );
        assert_eq!(
            (b.tags[10], b.data[10]),
            (Tag::Break, [1, Ref::konst(u32_of(k)).0])
        );
        run(&mut b, &Cx, &mut OwnScratch::default()).expect("passes");
        assert_eq!(verify_owned(&b, &Cx), []);
        assert_eq!(
            show(&b),
            "%0 Call\n%1 Store [$0] %0\n%2 Read move [$0]\n%3 Break %2\n%4 Block [%2 %3] -> %3\n\
             %5 Block []\n%6 If\n%7 Break\n%8 Block [%4 %5 %6 %7]\n%9 Loop\n%10 Break\n\
             %11 Block [%8 %9 %10]\n%12 Scope\n%13 Drop [$0]\n%14 Call %12\n\
             %15 Block [%0 %1 %11 %12 %13 %14]\n"
        );
    }

    /// A `Bound` call at a receiver substitution made concrete is chosen
    /// again as the impl `pick` names; one `pick` refuses stays a `Bound`.
    #[test]
    fn a_concrete_bound_call_is_chosen_again_as_an_impl() {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let blk = b.open_block();
        let at = |self_ty| Callee::TraitMethod {
            trait_: DefId::from_raw(5),
            method: DefId::from_raw(6),
            self_ty,
            targs: TyList::EMPTY,
            choice: (ChoiceKind::Bound, 0),
        };
        let s = b.const_str("s");
        b.call(&at(Ty::STRING), &[s], Providers::None, Ty::VOID, N);
        b.call(&at(T), &[s], Providers::None, Ty::VOID, N);
        let root = b.close_block(blk, None, Ty::VOID, N);
        let mut b = b.finish(root, &[]).expect("verifies");
        rechoose(&mut b, &|_, t, _| {
            (t == Ty::STRING).then(|| DefId::from_raw(7))
        });
        let choice = |i| match plain_call(&b, i) {
            Some(Callee::TraitMethod { choice, .. }) => choice,
            c => panic!("{c:?}"),
        };
        assert_eq!(choice(0), (ChoiceKind::Impl, 7));
        assert_eq!(choice(1), (ChoiceKind::Bound, 0));
        assert_eq!(verify(&b), []);
    }
}
