//! The inliner's body copy (`docs/compiler/tir-v2.md`, "The inliner"): a
//! `Call` of a small body becomes a copy of that body, run before the
//! ownership passes so they see the combined body. The copy's locals,
//! labels, constants, strings and records move past the caller's; its
//! parameters are bound as the call passes them ([`Bind`]); each `Return` is a
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

/// How a copy binds a parameter to its call's argument, by the call's
/// pass mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bind {
    /// A `Store` of the argument into the parameter's local, which the
    /// passes take as the call's argument ([`Body::bound`]): it copies a
    /// header only where the call would.
    Store,
    /// The same for a list or map receiver, which a call passes as is.
    Receiver,
    /// An in-place parameter, whose argument is a `Read`: the copy's places
    /// rooted at the parameter are rooted at the place that read reads, so
    /// a change writes back through it, and the read is left unused.
    Place,
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).expect("TIR column")
}

/// A call to copy ([`inline_calls`]): the caller's `Call` `call`, the
/// callee's copy `copy` with the call's types, and how each parameter
/// binds.
pub struct Site {
    pub call: usize,
    pub copy: Body,
    pub binds: Vec<Bind>,
}

/// A site that passed its checks: its parameters a `Store` binds (local,
/// argument, and for a bound one whether it is a receiver), those an
/// argument's place stands for (local, place word), the call's block,
/// and where the copy will start.
struct Ready {
    c: usize,
    k: Body,
    params: Vec<(u32, usize, Option<bool>)>,
    at: Vec<(u32, u32)>,
    outer: usize,
    base: u32,
}

/// Replaces each site's `Call` of `b` by its copy, all in one rebuild:
/// the caller is renumbered once and its columns permuted once, however
/// many calls are copied. `sites` come in descending call order, which is
/// the order the copies' locals, constants, labels and records are
/// appended in; the result says which sites were copied. The copy stands
/// where its call stood, keeping evaluation order, the `Scope` that holds
/// it last.
///
/// A site is skipped, the rest still copied, when its callee's parameters
/// do not match the call's arguments or binds, a [`Bind::Place`] argument
/// is no `Read`, or the call is in no block. A caller the renumbering
/// refuses (a `CallDyn`, a call's providers of an unknown kind) copies
/// none and is unchanged.
///
/// # Errors
/// Only if the renumbering refuses what that check let through.
pub fn inline_calls(b: &mut Body, sites: Vec<Site>) -> Result<Vec<bool>, String> {
    let mut copied = vec![false; sites.len()];
    let refused = |i: usize| {
        let kind = || b.record(b.data[i][1]).iter().rev().nth(2).copied();
        b.tags[i] == Tag::CallDyn
            || (matches!(b.tags[i], Tag::Call | Tag::Await) && kind().is_some_and(|p| p > 2))
    };
    if sites.is_empty() || (0..b.len()).any(refused) {
        return Ok(copied);
    }
    // Each instruction's block, the first that lists it.
    let mut outer = vec![NONE; b.len()];
    for i in (0..b.len()).filter(|&i| b.tags[i] == Tag::Block) {
        for &x in b.record(b.data[i][0]) {
            if let Some(o) = outer.get_mut(x as usize).filter(|o| **o == NONE) {
                *o = u32_of(i);
            }
        }
    }
    let mut ready = Vec::new();
    for (s, site) in sites.into_iter().enumerate() {
        if let Some(r) = prepare(b, site, &outer) {
            copied[s] = true;
            ready.push(r);
        }
    }
    // Each caller instruction moves past every copy at or before it, so a
    // call's number becomes its `Scope`'s; each copy starts where its call
    // moves to, less its own rows.
    let n = b.len();
    let (mut fin, mut shift, mut s) = (Vec::with_capacity(n), 0, ready.len());
    for i in 0..n {
        while s > 0 && ready[s - 1].c == i {
            s -= 1;
            ready[s].base = u32_of(i + shift);
            shift += ready[s].params.len() + ready[s].k.len() + 1;
        }
        fin.push(u32_of(i + shift));
    }
    renumber_through(b, fin, 0)?;
    if b.anchors.is_empty() {
        b.anchors = (0..u32_of(n)).collect();
    }
    // The copies' rows go at the columns' end, then into place by one
    // permutation, as a pass's rebuild does.
    let mut spans = Vec::with_capacity(ready.len());
    let rows = b.fallback.len();
    for r in ready {
        let (c, start) = (r.c, b.len());
        copy_in(b, r)?;
        spans.push((c, start..b.len()));
    }
    let mut order = Vec::with_capacity(b.len());
    for i in 0..n {
        match spans.last() {
            Some((c, span)) if *c == i => {
                order.extend(span.clone().map(u32_of));
                spans.pop();
            }
            _ => order.push(u32_of(i)),
        }
    }
    permute_columns(b, &order);
    if b.fallback.len() > rows {
        b.fallback.sort_by_key(|r| r[0]);
    }
    Ok(copied)
}

/// Checks `site` against the caller, unchanged yet, and sorts its
/// parameters by how they bind; `None` skips it.
fn prepare(b: &Body, site: Site, outer: &[u32]) -> Option<Ready> {
    let Site {
        call: c,
        copy: k,
        binds,
    } = site;
    let r = b.record(b.data[c][1]);
    let args = &r[..r.len().saturating_sub(3)];
    let (mut params, mut at) = (Vec::new(), Vec::new());
    for l in 0..k.local_flags.len() {
        if k.local_flags[l] & local_flags::PARAM == 0 {
            continue;
        }
        let j = params.len() + at.len();
        let (&bind, &a) = (binds.get(j)?, args.get(j)?);
        // A parameter the callee stores whole binds as any local does.
        let word = u32_of(l) | place::BARE_LOCAL;
        let stored = (0..k.len()).any(|i| k.tags[i] == Tag::Store && k.data[i][0] == word);
        if bind != Bind::Place {
            params.push((u32_of(l), j, (!stored).then_some(bind == Bind::Receiver)));
        } else if b.tags.get(a as usize) == Some(&Tag::Read) {
            at.push((u32_of(l), b.data[a as usize][0]));
        } else {
            return None;
        }
    }
    let outer = *outer.get(c).filter(|&&o| o != NONE)?;
    (args.len() == params.len() + at.len() && !k.is_empty()).then_some(Ready {
        c,
        k,
        params,
        at,
        outer: outer as usize,
        base: 0,
    })
}

/// Appends site `r`'s rows to the renumbered caller, numbered as they will
/// stand once permuted: the `Store`s, the callee's instructions but its
/// root, the final `Break`, the root, the `Scope`.
fn copy_in(out: &mut Body, r: Ready) -> Result<(), String> {
    let Ready {
        c,
        mut k,
        params,
        at,
        outer,
        base,
    } = r;
    let (m, np) = (k.len(), params.len());
    let root_at = base + u32_of(np + m);
    let mut new_of = Vec::with_capacity(m);
    for j in 0..m {
        new_of.push(if j + 1 == m {
            root_at
        } else {
            base + u32_of(np + j)
        });
    }
    renumber_through(&mut k, new_of, u32_of(out.consts.len()))?;
    let (xbase, lbase) = (u32_of(out.extra.len()), u32_of(out.local_ty.len()));
    let label = u32_of(out.label_inst.len() + k.label_inst.len());
    relocate(&mut k, xbase, lbase, u32_of(out.label_inst.len()), label);
    let syn = out.syn[c];
    let ty = out.ty[c];
    out.extra.append(&mut k.extra);
    for j in (0..m).filter(|_| !at.is_empty()) {
        let (oa, ob) = k.tags[j].operands();
        for (x, op) in [oa, ob].into_iter().enumerate() {
            if op == Op::Place {
                k.data[j][x] = rooted(out, k.data[j][x], lbase, &at);
            }
        }
    }
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
    // The copy's own instructions name the callee's as their anchors
    // ([`Body::inlined`]), so a panic in the copy reports the callee's
    // frame, then the call's; one without a position keeps none, as in
    // the callee, and so reports its enclosing one.
    let anchor = out.anchors[c];
    let first = out.inlined.last().map_or(0, |e| e[0] + e[1]);
    out.inlined.push([first, u32_of(m), k.item.raw(), anchor]);
    // The arguments, renumbered with the caller.
    let args = out.record(out.data[c][1]).to_vec();
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
    for &(p, a, _) in &params {
        put(
            Tag::Store,
            [(p + lbase) | place::BARE_LOCAL, args[a]],
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
    // The copy's fallback rows, sorted with the caller's once all are in.
    for &[i, l] in &k.fallback {
        out.fallback.push([i, if l == 0 { 0 } else { l + lbase }]);
    }
    for &(p, _, bound) in &params {
        if let Some(receiver) = bound {
            out.bound.push((p + lbase, receiver));
        }
    }
    Ok(())
}

/// Place word `w` of the copy, rooted at the argument's place when its
/// root is a parameter `at` lists (a local `lbase` past the callee's).
fn rooted(out: &mut Body, w: u32, lbase: u32, at: &[(u32, u32)]) -> u32 {
    let bare = w & place::BARE_LOCAL != 0;
    let l = if bare {
        w & !place::BARE_LOCAL
    } else if out.extra[w as usize + 1] == place::ROOT_LOCAL {
        out.extra[w as usize + 2]
    } else {
        return w;
    };
    let Some(&(_, arg)) = at.iter().find(|&&(p, _)| p + lbase == l) else {
        return w;
    };
    if bare {
        return arg;
    }
    // The argument's root and steps, then the copy's steps.
    let (n, an) = (
        out.extra[w as usize] as usize,
        out.extra[arg as usize] as usize,
    );
    let mut words = Vec::with_capacity(an + n);
    if arg & place::BARE_LOCAL == 0 {
        words.extend_from_slice(&out.extra[arg as usize + 1..][..an]);
    } else {
        words.extend([place::ROOT_LOCAL, arg & !place::BARE_LOCAL]);
    }
    words.extend_from_slice(&out.extra[w as usize + 3..][..n - 2]);
    push_record(out, &words)
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

    use super::{Bind, Site, inlinable, inline_calls, plain_call, rechoose, u32_of};
    use crate::ir::{
        Body, BodyKind, Callee, ChoiceKind, Op, Place, PlaceRoot, Proj, Providers, ReadMode, Ref,
        Tag, TirBuilder, TirSink, local_flags, map_body_types, verify, verify_owned,
    };
    use crate::own::tests::{Cx, show};
    use crate::own::{OwnScratch, run};

    const N: NodeIdx = NodeIdx::from_raw(1);

    /// Copies one call, as [`inline_calls`] does for each of a body's.
    fn inline_call(b: &mut Body, call: usize, copy: Body, binds: &[Bind]) -> Result<(), String> {
        let binds = binds.to_vec();
        match inline_calls(b, vec![Site { call, copy, binds }])?[..] {
            [true] => Ok(()),
            _ => Err("skipped".into()),
        }
    }
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
        inline_call(&mut b, 1, c, &[Bind::Store]).expect("inlines");
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

    /// `g() { r = 3(7("caller")); if true { 9(3(r)) } }`: two calls of
    /// callee 3 in two blocks, the second taking the first's result.
    fn two_calls() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let blk = b.open_block();
        let s = b.const_str("caller");
        let a = b.call(&item(7), &[s], Providers::None, Ty::STRING, N);
        let r = b.call(&item(3), &[a], Providers::None, Ty::STRING, N);
        let yes = b.const_value(Ty::BOOL, 1);
        let then = b.open_block();
        let r2 = b.call(&item(3), &[r], Providers::None, Ty::STRING, N);
        b.call(&item(9), &[r2], Providers::None, Ty::VOID, N);
        let then = b.close_block(then, None, Ty::VOID, N);
        let other = b.open_block();
        let other = b.close_block(other, None, Ty::VOID, N);
        let arms = b.refs_record(&[then, other]);
        b.emit(Tag::If, yes.0, arms, Ty::VOID, N);
        let root = b.close_block(blk, None, Ty::VOID, N);
        b.finish(root, &[]).expect("caller verifies")
    }

    /// One rebuild for all of `b`'s calls of callee 3 gives the body a
    /// rebuild per call gives, the latest call first.
    fn one_splice_equals_one_per_copy(b: Body, copy: &dyn Fn() -> Body, binds: &[Bind]) -> Body {
        let calls: Vec<usize> = (0..b.len())
            .rev()
            .filter(|&i| plain_call(&b, i) == Some(item(3)))
            .collect();
        let site = |call| Site {
            call,
            copy: copy(),
            binds: binds.to_vec(),
        };
        let mut one = b.clone();
        let sites = calls.iter().map(|&c| site(c)).collect();
        assert_eq!(inline_calls(&mut one, sites), Ok(vec![true, true]));
        let mut each = b;
        for &c in &calls {
            assert_eq!(inline_calls(&mut each, vec![site(c)]), Ok(vec![true]));
        }
        // Both leave the block records they replaced behind, with words
        // renumbered at different times; every record in use is the same.
        let live = |b: &Body| {
            let mut v = b.clone();
            v.extra.clear();
            for i in 0..b.len() {
                let (oa, ob) = b.tags[i].operands();
                for (w, op) in b.data[i].into_iter().zip([oa, ob]) {
                    let rec = matches!(op, Op::Values | Op::Blocks | Op::Record)
                        || (op == Op::Place && w & crate::ir::place::BARE_LOCAL == 0);
                    if rec && w != crate::ir::NONE {
                        v.extra.extend_from_slice(b.record(w));
                    }
                }
            }
            v
        };
        assert_eq!(live(&one), live(&each));
        one
    }

    #[test]
    fn one_splice_for_all_copies_equals_one_per_copy() {
        let mut b = one_splice_equals_one_per_copy(two_calls(), &callee, &[Bind::Store]);
        run(&mut b, &Cx, &mut OwnScratch::default()).expect("passes");
        assert_eq!(verify_owned(&b, &Cx), []);
        // `x = 7(); 3(x.1, "q"); 3(x.1, "q")`: each `p` is rooted at `x.1`,
        // and the later call's copy binds `q` first.
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let blk = b.open_block();
        let x = b.local(LIST, Symbol::from_raw(0), 0, N);
        let made = b.call(&item(7), &[], Providers::None, LIST, N);
        b.set(x, made, N);
        for _ in 0..2 {
            let w = b.place(&Place {
                root: PlaceRoot::Local(x),
                proj: vec![Proj::Field(1)],
            });
            let a = b.emit(Tag::Read, w, ReadMode::Use as u32, LIST, N);
            let q = b.const_str("q");
            b.call(&item(3), &[a, q], Providers::None, Ty::VOID, N);
        }
        let root = b.close_block(blk, None, Ty::VOID, N);
        let b = b.finish(root, &[]).expect("caller verifies");
        let b = one_splice_equals_one_per_copy(b, &|| changer(true), &[Bind::Place, Bind::Store]);
        let stores = (0..b.len()).filter(|&i| b.tags[i] == Tag::Store);
        let places: Vec<_> = stores
            .filter_map(|i| b.decode_place(b.data[i][0]).map(|p| p.to_string()))
            .collect();
        assert_eq!(places, ["[$0]", "[$4]", "[$0 .1]", "[$2]", "[$0 .1]"]);
    }

    /// The verifier checks what a copy promises: anchors in its rows, the
    /// rows in order, a bound local stored once, and places rooted at
    /// locals the body defines.
    #[test]
    fn the_verifier_checks_what_a_copy_promises() {
        let mut b = caller();
        inline_call(&mut b, 1, callee(), &[Bind::Store]).expect("inlines");
        run(&mut b, &Cx, &mut OwnScratch::default()).expect("passes");
        assert_eq!(verify_owned(&b, &Cx), []);
        let finds =
            |b: &Body, what: &str| verify_owned(b, &Cx).iter().any(|e| e.what.contains(what));
        let i = (0..b.len()).find(|&i| b.anchors[i] & crate::ir::INLINED != 0);
        let mut bad = b.clone();
        bad.anchors[i.expect("a copied instruction")] = crate::ir::INLINED | 0x63;
        assert!(finds(&bad, "in no inlined copy"));
        let mut bad = b.clone();
        bad.inlined[0][3] |= crate::ir::INLINED;
        assert!(finds(&bad, "called in a copy"));
        let mut bad = b.clone();
        bad.bound.push(bad.bound[0]);
        assert!(finds(&bad, "bound local"));
        // An in-place parameter's local left in its copy.
        let mut b = changing_caller(false);
        inline_call(&mut b, 5, changer(true), &[Bind::Place, Bind::Store]).expect("inlines");
        run(&mut b, &Cx, &mut OwnScratch::default()).expect("passes");
        assert!(!finds(&b, "nothing stores"));
        let copied = |i: usize| b.anchors[i] & crate::ir::INLINED != 0 && b.tags[i] == Tag::Read;
        let read = (0..b.len()).find(|&i| copied(i)).expect("the copy's read");
        b.data[read][0] = 1 | crate::ir::place::BARE_LOCAL;
        assert!(finds(&b, "rooted at $1, which nothing stores"));
    }

    /// The test context's list type ([`crate::own::tests::Cx`]).
    const LIST: Ty = Ty(1_000_000);

    fn item(d: u32) -> Callee {
        Callee::Item {
            def: DefId::from_raw(d),
            targs: TyList::EMPTY,
        }
    }

    /// `f(p: LIST, q: string) { 8(p.0); p = 7() }`, as callee 3, without
    /// the store unless `store`.
    fn changer(store: bool) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(3), BodyKind::Fn);
        let blk = b.open_block();
        let p = b.local(LIST, Symbol::from_raw(0), local_flags::PARAM, N);
        b.local(Ty::STRING, Symbol::from_raw(1), local_flags::PARAM, N);
        let w = b.place(&Place {
            root: PlaceRoot::Local(p),
            proj: vec![Proj::Field(0)],
        });
        let v = b.emit(Tag::Read, w, ReadMode::Use as u32, LIST, N);
        b.call(&item(8), &[v], Providers::None, Ty::VOID, N);
        if store {
            let made = b.call(&item(7), &[], Providers::None, LIST, N);
            b.set(p, made, N);
        }
        let root = b.close_block(blk, None, Ty::VOID, N);
        b.finish(root, &[]).expect("callee verifies")
    }

    /// `g() { x = 7(); 3(x.1, 5(x)) }`, where callee 5 changes `x` in place
    /// (`Cx`), or `3(x, ...)` with `whole`.
    fn changing_caller(whole: bool) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let blk = b.open_block();
        let x = b.local(LIST, Symbol::from_raw(0), 0, N);
        let made = b.call(&item(7), &[], Providers::None, LIST, N);
        b.set(x, made, N);
        let proj = if whole { vec![] } else { vec![Proj::Field(1)] };
        let w = b.place(&Place {
            root: PlaceRoot::Local(x),
            proj,
        });
        let a = b.emit(Tag::Read, w, ReadMode::Use as u32, LIST, N);
        let r = b.get(x, LIST, N);
        let s = b.call(&item(5), &[r], Providers::None, Ty::STRING, N);
        b.call(&item(3), &[a, s], Providers::None, Ty::VOID, N);
        let root = b.close_block(blk, None, Ty::VOID, N);
        b.finish(root, &[]).expect("caller verifies")
    }

    /// An in-place parameter names its argument's place: the copy reads
    /// `x.1.0` and stores `x.1`, and only the other parameter is bound.
    #[test]
    fn an_in_place_parameter_is_rooted_at_its_arguments_place() {
        let mut b = changing_caller(false);
        inline_call(&mut b, 5, changer(true), &[Bind::Place, Bind::Store]).expect("inlines");
        let mut v = b.clone();
        v.anchors.clear();
        assert_eq!(verify(&v), []);
        let place = |i: usize| b.decode_place(b.data[i][0]).map(|p| p.to_string());
        let rows: Vec<_> = (0..b.len())
            .filter(|&i| matches!(b.tags[i], Tag::Read | Tag::Store))
            .map(|i| place(i).unwrap_or_default())
            .collect();
        assert_eq!(
            rows,
            ["[$0]", "[$0 .1]", "[$0]", "[$2]", "[$0 .1 .0]", "[$0 .1]"]
        );
        assert_eq!(b.bound, [(2, false)]);
    }

    /// A list bound by a `Store` from a local a later argument changes takes
    /// a header copy right after the argument, as a call's argument does;
    /// a list receiver, which a call passes as is, takes none.
    #[test]
    fn a_bound_argument_is_copied_only_where_the_call_would_copy_it() {
        let copies = |bind| {
            let mut b = changing_caller(true);
            inline_call(&mut b, 5, changer(false), &[bind, Bind::Store]).expect("inlines");
            run(&mut b, &Cx, &mut OwnScratch::default()).expect("passes");
            assert_eq!(verify_owned(&b, &Cx), []);
            let text = show(&b);
            let copy = text.find(" Intrinsic %");
            let change = text.find("IsUnique").expect("the change");
            (copy.is_some_and(|c| c < change), copy.is_none())
        };
        assert_eq!(copies(Bind::Store), (true, false));
        assert_eq!(copies(Bind::Receiver), (false, true));
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
