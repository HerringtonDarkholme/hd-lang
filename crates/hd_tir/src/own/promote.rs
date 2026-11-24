//! Argument promotion (#1151): a `&T` parameter whose body only reads
//! through it takes its referent by value in a promoted copy of the body,
//! and a direct call whose argument is the address of the caller's own
//! local passes that local's value instead
//! (`crates/hd_wasm/DESIGN.md` §23.40). A promoted call is
//! release only; an inlined copy binds by value in every build. Whether `T` is small and
//! holds no counted leaf is the backend's question; these are the TIR
//! halves: which parameters the body only reads through
//! ([`read_only_params`]), the copy ([`promote`]), which arguments a call
//! may promote ([`promotable_args_in`]), and which an inlined copy may bind by value
//! ([`inline_args`]).

use hd_types::Ty;

use crate::ir::{Body, Proj, Ref, Root, Tag, place, u32_of};

/// The parameters of `b` among `candidates` (bit k: parameter k, the
/// receiver first) that the body only reads through: every place rooted
/// at the parameter's local is a `Read` that dereferences it first and
/// then names fields or variant payloads, or a `Drop` of the whole local,
/// and no closure captures it. A whole read of the reference (to store,
/// return, compare with `is` or pass it on), a `Ref` through it and a
/// `Store` through it each keep the address. A body with a suspension
/// point keeps every parameter.
#[must_use]
pub fn read_only_params(b: &Body, candidates: u64) -> u64 {
    if candidates == 0 || b.tags.iter().any(|&t| crate::ir::is_suspension(t)) {
        return 0;
    }
    let params = super::params(b);
    let mut mask = candidates;
    for (k, &l) in params.iter().enumerate().take(64) {
        if b.cap_local.iter().any(|c| c.raw() == l) {
            mask &= !(1 << k);
        }
    }
    mask &= u64::MAX
        .checked_shr(64 - u32::try_from(params.len().min(64)).expect("params"))
        .unwrap_or(0);
    for i in 0..b.len() {
        if mask == 0 {
            break;
        }
        let tag = b.tags[i];
        if !matches!(
            tag,
            Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::LoanEnd | Tag::IsUnique
        ) {
            continue;
        }
        let p = b.view(b.data[i][0]);
        let Root::Local(l) = p.root else {
            continue;
        };
        let Some(k) = params.iter().position(|&x| x == l).filter(|&k| k < 64) else {
            continue;
        };
        let mut steps = p.steps(b);
        let ok = match tag {
            Tag::Read if p.whole() => only_deref(b, u32_of(i)),
            Tag::Read => {
                steps.next() == Some(Proj::Deref)
                    && steps.all(|s| matches!(s, Proj::Field(_) | Proj::Variant(_)))
            }
            Tag::Drop | Tag::LoanEnd => p.whole(),
            _ => false,
        };
        if !ok {
            mask &= !(1 << k);
        }
    }
    mask
}

/// Whether value `v` of `b`, a read of all of a reference, is used only
/// through it: each place rooted at it is a `Read` that dereferences it
/// first and then names fields or variant payloads, or a `Drop` of all of
/// it, and its one other use is a `Match`'s scrutinee, as `match self:` in
/// a `&self` method reads its arms' tags and payloads through it.
fn only_deref(b: &Body, value: u32) -> bool {
    let mut ops = Vec::new();
    let mut uses = ReadThroughUses::default();
    (0..b.len()).all(|j| {
        super::operands(b, j, &mut ops);
        !ops.iter().any(|o| o.value == value) || uses.permits(b, j, value)
    })
}

/// A read-through-use index shared by parameter promotion, lending and its
/// verifier. Each distinct projection record is scanned once, even when many
/// instructions reuse it: expected O(instructions + distinct projection words).
#[derive(Default)]
pub(super) struct ReadThroughUses {
    places: hd_base::HashMap<u32, bool>,
}

impl ReadThroughUses {
    pub(super) fn permits(&mut self, b: &Body, at: usize, value: u32) -> bool {
        self.permits_with(b, at, value, &mut || {})
    }

    fn permits_with(&mut self, b: &Body, at: usize, value: u32, visit: &mut impl FnMut()) -> bool {
        visit();
        let tag = b.tags[at];
        if matches!(
            tag,
            Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::LoanEnd | Tag::IsUnique
        ) {
            let word = b.data[at][0];
            let place = b.view(word);
            if place.root == Root::Value(value) {
                return match tag {
                    Tag::Read => *self.places.entry(word).or_insert_with(|| {
                        let mut steps = place.steps(b).inspect(|_| visit());
                        steps.next() == Some(Proj::Deref)
                            && steps.all(|s| matches!(s, Proj::Field(_) | Proj::Variant(_)))
                    }),
                    Tag::Drop | Tag::LoanEnd => place.whole(),
                    _ => false,
                };
            }
        }
        tag == Tag::Match && b.data[at][0] == value
    }
}

/// Body `b` with each parameter in `mask` ([`read_only_params`]) taking
/// its referent by value: the local's type is `referent` of its reference
/// type, and each `Read [*$l rest]` reads `[$l rest]`. A read of all of
/// `$l` reads the referent, and a read through it, `[*%v rest]`, reads
/// `[%v rest]`.
#[must_use]
pub fn promote(b: &Body, mask: u64, referent: impl Fn(Ty) -> Ty) -> Body {
    let mut out = b.clone();
    let params = super::params(b);
    let promoted: Vec<u32> = (params.iter().enumerate())
        .filter(|&(k, _)| k < 64 && mask & (1 << k) != 0)
        .map(|(_, &l)| l)
        .collect();
    for &l in &promoted {
        out.local_ty[l as usize] = referent(b.local_ty[l as usize]);
    }
    // The whole reads of a promoted local, which now read its referent.
    let mut whole = Vec::new();
    for i in 0..b.len() {
        if b.tags[i] == Tag::Read
            && let Some(p) = b.decode_place(b.data[i][0])
            && let crate::ir::PlaceRoot::Local(l) = p.root
            && p.proj.is_empty()
            && promoted.contains(&l.raw())
        {
            out.ty[i] = referent(b.ty[i]);
            whole.push(u32::try_from(i).expect("insts"));
        }
    }
    for i in 0..b.len() {
        if b.tags[i] != Tag::Read {
            continue;
        }
        let Some(mut p) = b.decode_place(b.data[i][0]) else {
            continue;
        };
        let through = match p.root {
            crate::ir::PlaceRoot::Local(l) => promoted.contains(&l.raw()),
            crate::ir::PlaceRoot::Value(v) => whole.contains(&v.0),
            _ => false,
        };
        if !through || p.proj.first() != Some(&Proj::Deref) {
            continue;
        }
        p.proj.remove(0);
        out.data[i][0] = match p.words() {
            None => match p.root {
                crate::ir::PlaceRoot::Local(l) => l.raw() | place::BARE_LOCAL,
                _ => unreachable!("only a local's place has no record"),
            },
            Some(words) => {
                let at = u32::try_from(out.extra.len()).expect("extra");
                out.extra.push(u32::try_from(words.len()).expect("place"));
                out.extra.extend(words);
                at
            }
        };
    }
    out
}

/// The arguments of `Call` `call` of `b` whose values the passes keep
/// alive over the call (bit k: argument k): each a `Ref` of a frame local
/// (`ref_word::FRAME`), or one that takes its local's count
/// (`ref_word::TAKES`), as `make().method()` lends the temporary. A
/// promoted copy lends a value with counted parts with no count only
/// there: the frame local's `Drop`, or the taking `Ref`'s own, which
/// releases the value it moved out of the local, follows the call. Any
/// other `Ref` may outlive its local's `Drop`, which can come before the
/// call.
#[must_use]
pub fn held_args(b: &Body, call: usize) -> u64 {
    use crate::ir::ref_word::{FRAME, TAKES};
    if b.tags[call] != Tag::Call {
        return 0;
    }
    let args = b.call_args(call);
    (args.iter().enumerate().take(64))
        .filter(|&(_, &v)| {
            Ref(v).as_inst().is_some_and(|i| {
                let i = i.0 as usize;
                b.tags[i] == Tag::Ref && b.data[i][1] & (FRAME | TAKES) != 0
            })
        })
        .fold(0, |mask, (k, _)| mask | 1 << k)
}

/// What [`promotable_args_in`] reads of a whole body, made once for all its
/// call sites: whether it has a suspension point, and the [`Census`] of
/// its `Ref`s' uses. A caller that asks per call site of an unchanging
/// body makes one, so `n` calls cost one pass over the body, not `n`.
pub struct Promotion {
    suspends: bool,
    census: Census,
}

impl Promotion {
    #[must_use]
    pub fn new(b: &Body) -> Self {
        Self {
            suspends: b.tags.iter().any(|&t| crate::ir::is_suspension(t)),
            census: Census::new(b),
        }
    }
}

/// Arguments eligible for promotion, using `b`'s shared [`Promotion`]
/// facts: each argument is one census lookup.
#[must_use]
pub fn promotable_args_in(b: &Body, call: usize, facts: &Promotion) -> u64 {
    if !matches!(b.tags[call], Tag::Call | Tag::DefaultCall) || facts.suspends {
        return 0;
    }
    let args = crate::inline::arguments(b, call);
    (args.iter().enumerate().take(64))
        .filter(|&(_, &v)| {
            census_work(1);
            Ref(v).as_inst().is_some_and(|i| {
                lent_ref_with(b, i.0 as usize, call, Some(&facts.census), false)
                    .is_some_and(|steps| steps == Steps::InPlace)
            })
        })
        .fold(0, |mask, (k, _)| mask | 1 << k)
}

/// Whether `Ref` `i` of `b` is a read-only reference to a part of a local
/// no closure captures, used once by instruction `user` and otherwise only
/// dropped: the reference a promoted argument replaces with its value.
/// One that takes its local's count (`ref_word::TAKES`, a temporary's
/// `Ref` at its last access) then moves the value out of the local, and
/// its `Drop` releases it; it is the local's only `Ref`, so the local
/// lives in no cell.
#[must_use]
pub fn promotable_ref(b: &Body, i: usize, user: usize) -> bool {
    lent_ref(b, i, user).is_some_and(|steps| steps == Steps::InPlace)
}

/// How far the place of a lent `Ref` ([`lent_ref`]) goes from its local.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Steps {
    /// Fields and variant payloads only: a part of the local.
    InPlace,
    /// Elements too: a part of a list the local holds.
    Elements,
    /// A dereference too: a part of what a reference refers to.
    Deref,
}

/// The existing one-call reference-use proof, indexed once for all sites
/// in an immutable caller. `Drop`s and `LoanEnd`s do not add another use.
pub(super) struct Census {
    captured: Vec<bool>,
    addresses: Vec<u32>,
    counts: Vec<(u32, usize)>,
}

thread_local! {
    static CENSUS_WORK: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Counts, on this thread while `run` runs, each instruction and operand
/// a [`Census`] reads and each argument a promotion check looks up.
pub fn count_census<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let outer = CENSUS_WORK.with(|w| w.replace(Some(0)));
    let out = run();
    let work = CENSUS_WORK.with(|w| w.replace(outer)).unwrap_or(0);
    if let Some(outer) = outer {
        CENSUS_WORK.with(|w| w.set(Some(outer + work)));
    }
    (out, work)
}

fn census_work(n: usize) {
    CENSUS_WORK.with(|w| {
        if let Some(k) = w.get() {
            w.set(Some(k + n as u64));
        }
    });
}

impl Census {
    pub(super) fn new(b: &Body) -> Self {
        census_work(b.len());
        let mut out = Self {
            captured: vec![false; b.local_ty.len()],
            addresses: vec![0; b.local_ty.len()],
            counts: vec![(0, 0); b.len()],
        };
        for l in &b.cap_local {
            #[cfg(test)]
            super::effect_places::work(1);
            out.captured[l.idx()] = true;
        }
        let mut ops = Vec::new();
        for i in 0..b.len() {
            #[cfg(test)]
            super::effect_places::work(1);
            if b.tags[i] == Tag::Ref
                && let Root::Local(l) = b.view(b.data[i][0]).root
            {
                out.addresses[l as usize] += 1;
            }
            if matches!(b.tags[i], Tag::Drop | Tag::LoanEnd) {
                continue;
            }
            super::operands(b, i, &mut ops);
            for op in &ops {
                #[cfg(test)]
                super::effect_places::work(1);
                let entry = &mut out.counts[op.value as usize];
                entry.0 += 1;
                entry.1 = i;
            }
        }
        out
    }
}

/// [`promotable_ref`]'s test, with how far the place goes; `None` when the
/// `Ref` is no such reference. A place with an element or a dereference
/// in it is one only an inlined copy may read by value
/// ([`inline_args`]).
fn lent_ref(b: &Body, i: usize, user: usize) -> Option<Steps> {
    lent_ref_with(b, i, user, None, false)
}

/// The `Ref` `Call` `call`'s first argument is ([`reference_source`], each
/// link used once by the next), a `&mut` with [`promotable_ref`]'s proof
/// but for the change, taking no count, in a body with no suspension
/// point: a list change may read its place and store the result back. One
/// census lookup a link.
#[must_use]
pub fn changed_in_place(b: &Body, call: usize, facts: &Promotion) -> Option<usize> {
    let (mut user, mut v) = (call, Ref(*b.call_args(call).first()?).as_inst()?.0 as usize);
    while let Some(x) = link(b, v) {
        census_work(1);
        (facts.census.counts[x] == (1, v)).then_some(())?;
        (user, v) = (v, x);
    }
    let ok = !facts.suspends && b.data[v][1] & crate::ir::ref_word::TAKES == 0;
    (ok && lent_ref_with(b, v, user, Some(&facts.census), true) == Some(Steps::InPlace))
        .then_some(v)
}

/// The instruction whose reference `v` of `b` is again, through whole
/// reads and coercions to its own type, as an inlined `self` passes it on.
#[must_use]
pub fn reference_source(b: &Body, v: usize) -> usize {
    std::iter::successors(Some(v), |&x| link(b, x))
        .last()
        .unwrap_or(v)
}

/// The value `v` reads all of or coerces, of `v`'s type.
fn link(b: &Body, v: usize) -> Option<usize> {
    let x = match *b.tags.get(v)? {
        Tag::Read if b.view(b.data[v][0]).whole() => b.view(b.data[v][0]).value()? as usize,
        Tag::Coerce => Ref(b.data[v][0]).as_inst()?.0 as usize,
        _ => return None,
    };
    (b.ty.get(x) == Some(&b.ty[v])).then_some(x)
}

fn lent_ref_with(
    b: &Body,
    i: usize,
    user: usize,
    facts: Option<&Census>,
    changes: bool,
) -> Option<Steps> {
    let flags = b.data[i][1];
    if b.tags[i] != Tag::Ref || (!changes && flags & crate::ir::ref_word::MUT != 0) {
        return None;
    }
    let p = b.view(b.data[i][0]);
    let Root::Local(l) = p.root else {
        return None;
    };
    if facts.map_or_else(
        || b.cap_local.iter().any(|c| c.raw() == l),
        |facts| facts.captured[l as usize],
    ) {
        return None;
    }
    let steps = p.steps(b).fold(Steps::InPlace, |s, step| match step {
        Proj::Field(_) | Proj::Variant(_) => s,
        Proj::Elem(_) if s == Steps::InPlace => Steps::Elements,
        Proj::Elem(_) => s,
        Proj::Deref => Steps::Deref,
    });
    let addressed = |j: usize| {
        j != i
            && b.tags[j] == Tag::Ref
            && matches!(b.view(b.data[j][0]).root, Root::Local(m) if m == l)
    };
    if flags & crate::ir::ref_word::TAKES != 0
        && facts.map_or_else(
            || (0..b.len()).any(addressed),
            |facts| facts.addresses[l as usize] > 1,
        )
    {
        return None;
    }
    if let Some(facts) = facts {
        return (facts.counts[i] == (1, user)).then_some(steps);
    }
    let mut ops = Vec::new();
    let mut seen = 0;
    for j in 0..b.len() {
        super::operands(b, j, &mut ops);
        let n = ops.iter().filter(|o| o.value as usize == i).count();
        if n == 0 {
            continue;
        }
        if j == user {
            seen += n;
        } else if !matches!(b.tags[j], Tag::Drop | Tag::LoanEnd) {
            return None;
        }
    }
    (seen == 1).then_some(steps)
}

/// The arguments of `Call` `call` of `b` that an inlined copy may bind by
/// value (#1151; bit k: argument k), with
/// `hd_tir::inline::Site::by_value`: a lent `Ref` of a part of a local
/// as [`promotable_args_in`] takes, or, for an argument in `relaxed`, of an
/// element of a list the local holds or of what a reference reaches from
/// it (`xs[i].len()`, `self.s.len()` in a `&self` method). The inliner
/// relaxes the arguments whose referents hold counted parts, where the
/// reference would pin the list or count the referent and the value read
/// borrows instead. The copy reads the place once, at the `Ref`, where
/// the plain callee would read it through the reference at each use; the
/// two agree while nothing writes it in between. Only the local's own
/// instructions write a local's list, and a local no closure captures is
/// the caller's alone; a place behind a dereference may change by an
/// alias, so its argument counts only when no instruction from its `Ref`
/// to the call writes memory the body does not own (`super::writes_in`,
/// `may_write` for a call), and `copy_writes`, whether the copy may, is
/// false.
#[must_use]
pub fn inline_args(
    effects: &super::WriteEffects<'_>,
    call: usize,
    relaxed: u64,
    copy_writes: bool,
    may_write: &dyn Fn(&crate::ir::Callee) -> bool,
) -> u64 {
    let b = effects.body;
    if b.tags[call] != Tag::Call || effects.suspends {
        return 0;
    }
    let args = b.call_args(call);
    let facts = effects.promotion.get_or_init(|| Census::new(b));
    (args.iter().enumerate().take(64))
        .filter(|&(k, &v)| {
            Ref(v).as_inst().is_some_and(|i| {
                let i = i.0 as usize;
                match lent_ref_with(b, i, call, Some(facts), false) {
                    Some(Steps::InPlace) => true,
                    Some(_) if relaxed & 1 << k == 0 => false,
                    Some(Steps::Elements) => true,
                    Some(Steps::Deref) => !copy_writes && !effects.in_range(i + 1..call, may_write),
                    None => false,
                }
            })
        })
        .fold(0, |mask, (k, _)| mask | 1 << k)
}

#[cfg(test)]
mod projection_work_tests {
    use super::*;
    use crate::ir::{BodyKind, Place, PlaceRoot, TirBuilder, TirSink, local_flags, ref_word};
    use hd_base::{DefId, NodeIdx, Symbol};

    #[test]
    fn shared_projection_width_and_depth_are_indexed_once() -> Result<(), String> {
        for (width, depth) in [(1_000, 20), (10_000, 20), (1_000, 1_000)] {
            let n = NodeIdx::from_raw(1);
            let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
            let block = b.open_block();
            let local = b.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, n);
            let local_place = b.place(&Place::local(local));
            let reference = b.emit(Tag::Ref, local_place, ref_word::LOCAL, Ty::I32, n);
            let mut proj = vec![Proj::Deref];
            proj.extend(std::iter::repeat_n(Proj::Field(0), depth));
            let projected = b.place(&Place {
                root: PlaceRoot::Value(reference),
                proj,
            });
            let mut reads = Vec::new();
            for _ in 0..width {
                reads.push(b.emit(
                    Tag::Read,
                    projected,
                    crate::ir::ReadMode::Use as u32,
                    Ty::STRING,
                    n,
                ));
            }
            let root = b.close_block(block, None, Ty::VOID, n);
            let body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
            let mut index = ReadThroughUses::default();
            let mut visits = 0;
            for r in reads {
                assert!(index.permits_with(&body, r.0 as usize, reference.0, &mut || visits += 1));
            }
            assert_eq!(visits, width + depth + 1);
        }
        Ok(())
    }

    /// `n` locals, each changed by a call through `&mut`, as an inlined
    /// `self` passes it: `Ref`, a whole read of it, an identity coercion.
    /// Every third `Ref` has a second use, and every third local a
    /// closure captures: those keep their address. One census for the
    /// body, then one lookup a link: linear work.
    #[test]
    fn a_changed_reference_follows_its_links_in_linear_work() -> Result<(), String> {
        use crate::ir::{Callee, Coercion, Providers, ReadMode, ref_word};
        for n in [100_u32, 1_000] {
            let none = NodeIdx::NONE;
            let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
            let block = b.open_block();
            let callee = Callee::Item {
                def: DefId::from_raw(2),
                targs: hd_types::TyList::EMPTY,
            };
            let mut calls = Vec::new();
            for k in 0..n {
                let l = b.local(Ty::STRING, Symbol::NONE, 0, none);
                let place = b.place(&Place::local(l));
                let mode = ref_word::LOCAL | ref_word::MUT;
                let r = b.emit(Tag::Ref, place, mode, Ty::I32, none);
                let whole = b.place(&Place {
                    root: PlaceRoot::Value(r),
                    proj: Vec::new(),
                });
                let read = b.emit(Tag::Read, whole, ReadMode::Use as u32, Ty::I32, none);
                let arg = b.coerce(Coercion::Identity, crate::ir::NONE, read, Ty::I32, none);
                let call = b.call(&callee, &[arg], Providers::None, Ty::VOID, none);
                calls.push((call, r, k % 3));
                if k % 3 == 1 {
                    b.call(&callee, &[r], Providers::None, Ty::VOID, none);
                }
            }
            let root = b.close_block(block, None, Ty::VOID, none);
            let mut body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
            body.cap_local = (0..n)
                .filter(|k| k % 3 == 2)
                .map(hd_base::LocalId::from_raw)
                .collect();
            body.cap_mode = vec![crate::ir::CaptureMode::Shared; body.cap_local.len()];
            let (found, work) = count_census(|| {
                let facts = Promotion::new(&body);
                (calls.iter())
                    .map(|&(call, _, _)| changed_in_place(&body, call.0 as usize, &facts))
                    .collect::<Vec<_>>()
            });
            for (&(call, r, kind), got) in calls.iter().zip(found) {
                assert_eq!(got, (kind == 0).then_some(r.0 as usize));
                assert_eq!(
                    reference_source(&body, body.call_args(call.0 as usize)[0] as usize),
                    r.0 as usize
                );
            }
            // The census, then up to two links a call.
            assert!(work <= u64::try_from(body.len()).expect("len") + 2 * u64::from(n));
        }
        Ok(())
    }
}
