//! Argument promotion (#1151): a `&T` parameter whose body only reads
//! through it takes its referent by value in a promoted copy of the body,
//! and a direct call whose argument is the address of the caller's own
//! local passes that local's value instead
//! (`crates/hd_wasm_linear/DESIGN.md` §23.40). A promoted call is
//! release only; an inlined copy binds by value in every build. Whether `T` is small and
//! holds no counted leaf is the backend's question; these are the TIR
//! halves: which parameters the body only reads through
//! ([`read_only_params`]), the copy ([`promote`]), which calls may run
//! it ([`promoted_call`]), and which an inlined copy may bind by value
//! ([`inline_args`]).

use hd_types::Ty;

use crate::ir::{Body, Proj, Ref, Root, Tag, place};

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
            Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::IsUnique | Tag::Pin
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
            Tag::Read if p.whole() => only_deref(b, i),
            Tag::Read => {
                steps.next() == Some(Proj::Deref)
                    && steps.all(|s| matches!(s, Proj::Field(_) | Proj::Variant(_)))
            }
            Tag::Drop => p.whole(),
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
fn only_deref(b: &Body, v: usize) -> bool {
    let mut ops = Vec::new();
    (0..b.len()).all(|j| {
        let tag = b.tags[j];
        if matches!(
            tag,
            Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::IsUnique | Tag::Pin
        ) {
            let p = b.view(b.data[j][0]);
            if p.root == Root::Value(u32::try_from(v).expect("insts")) {
                let mut steps = p.steps(b);
                return match tag {
                    Tag::Read => {
                        steps.next() == Some(Proj::Deref)
                            && steps.all(|s| matches!(s, Proj::Field(_) | Proj::Variant(_)))
                    }
                    Tag::Drop => p.whole(),
                    _ => false,
                };
            }
        }
        super::operands(b, j, &mut ops);
        !ops.iter().any(|o| o.value as usize == v)
            || (tag == Tag::Match && b.data[j][0] as usize == v)
    })
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

/// Whether `Call` `call` of `b` may run its callee's promoted copy, which
/// takes the parameters in `mask` by value: each such argument is a
/// read-only `Ref` of a part of a local of `b` no closure captures, whose
/// only other uses are `Drop`s. The emitter then reads the local's value
/// for the argument, and the local needs no memory for it. A body with a
/// suspension point keeps the plain callee.
#[must_use]
pub fn promoted_call(b: &Body, call: usize, mask: u64) -> bool {
    mask != 0 && promotable_args(b, call) & mask == mask
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
    let words = b.record(b.data[call][1]);
    let args = &words[..words.len().saturating_sub(3)];
    (args.iter().enumerate().take(64))
        .filter(|&(_, &v)| {
            Ref(v).as_inst().is_some_and(|i| {
                let i = i.0 as usize;
                b.tags[i] == Tag::Ref && b.data[i][1] & (FRAME | TAKES) != 0
            })
        })
        .fold(0, |mask, (k, _)| mask | 1 << k)
}

/// The arguments of `Call` `call` of `b` that a promoted copy may take by
/// value (bit k: argument k), as [`promoted_call`] reads them; none in a
/// body with a suspension point.
#[must_use]
pub fn promotable_args(b: &Body, call: usize) -> u64 {
    if b.tags[call] != Tag::Call || b.tags.iter().any(|&t| crate::ir::is_suspension(t)) {
        return 0;
    }
    let words = b.record(b.data[call][1]);
    let args = &words[..words.len().saturating_sub(3)];
    (args.iter().enumerate().take(64))
        .filter(|&(_, &v)| {
            Ref(v)
                .as_inst()
                .is_some_and(|i| promotable_ref(b, i.0 as usize, call))
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

/// Whether `Ref` `i` names a place behind a reference that `b`'s caller
/// lends (`lent_param`): `[*$p ...]`, where `$p` is such a parameter or a
/// local stored once from a read of one, as an inlined copy's `self` is.
/// The body owns nothing there, so none of its drops reaches the place.
fn through_param(b: &Body, i: usize, lent_param: &dyn Fn(u32) -> bool) -> bool {
    let p = b.view(b.data[i][0]);
    let Root::Local(l) = p.root else {
        return false;
    };
    lends_referent(b, l, lent_param, 0) && matches!(p.steps(b).next(), Some(Proj::Deref))
}

/// Whether local `l` of `b` is a reference its caller lends: a lent
/// parameter, or a local stored only once, from a read of all of one.
fn lends_referent(b: &Body, l: u32, lent_param: &dyn Fn(u32) -> bool, depth: u32) -> bool {
    if b.local_flags[l as usize] & crate::ir::local_flags::PARAM != 0 {
        return lent_param(l);
    }
    if depth > 8 || !lent_param(l) {
        return false;
    }
    let mut stores = (0..b.len())
        .filter(|&j| b.tags[j] == Tag::Store && b.view(b.data[j][0]).local() == Some(l));
    let (Some(j), None) = (stores.next(), stores.next()) else {
        return false;
    };
    let Some(v) = Ref(b.data[j][1]).as_inst().map(|v| v.0 as usize) else {
        return false;
    };
    b.tags[v] == Tag::Read
        && b.view(b.data[v][0]).whole()
        && matches!(b.view(b.data[v][0]).root, Root::Local(m)
            if lends_referent(b, m, lent_param, depth + 1))
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

/// [`promotable_ref`]'s test, with how far the place goes; `None` when the
/// `Ref` is no such reference. A place with an element or a dereference
/// in it is one only an inlined copy may read by value
/// ([`inline_args`]).
fn lent_ref(b: &Body, i: usize, user: usize) -> Option<Steps> {
    let flags = b.data[i][1];
    if b.tags[i] != Tag::Ref || flags & crate::ir::ref_word::MUT != 0 {
        return None;
    }
    let p = b.view(b.data[i][0]);
    let Root::Local(l) = p.root else {
        return None;
    };
    if b.cap_local.iter().any(|c| c.raw() == l) {
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
    if flags & crate::ir::ref_word::TAKES != 0 && (0..b.len()).any(addressed) {
        return None;
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
        } else if b.tags[j] != Tag::Drop {
            return None;
        }
    }
    (seen == 1).then_some(steps)
}

/// The arguments of `Call` `call` of `b` that an inlined copy may bind by
/// value (#1151; bit k: argument k), with
/// `hd_tir::inline::Site::by_value`: a lent `Ref` of a part of a local
/// as [`promotable_args`] takes, or, for an argument in `relaxed`, of an
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
/// false. An argument in `rooted`, whose referent holds a value with a
/// `std.rt.Drop`, counts only as what one of the body's reference
/// parameters refers to, with no element on the way (`self.buffer` in a
/// `&self` method): its caller keeps it alive and in place for the whole
/// call, so the read the copy makes always borrows, where a local's last
/// use could drop it while the copy still reads it.
#[must_use]
pub fn inline_args(
    b: &Body,
    call: usize,
    (relaxed, rooted): (u64, u64),
    copy_writes: bool,
    may_write: &dyn Fn(&crate::ir::Callee) -> bool,
    lent_param: &dyn Fn(u32) -> bool,
) -> u64 {
    if b.tags[call] != Tag::Call || b.tags.iter().any(|&t| crate::ir::is_suspension(t)) {
        return 0;
    }
    let words = b.record(b.data[call][1]);
    let args = &words[..words.len().saturating_sub(3)];
    (args.iter().enumerate().take(64))
        .filter(|&(k, &v)| {
            Ref(v).as_inst().is_some_and(|i| {
                let i = i.0 as usize;
                if rooted & 1 << k != 0 && !through_param(b, i, lent_param) {
                    return false;
                }
                match lent_ref(b, i, call) {
                    Some(Steps::InPlace) => true,
                    Some(_) if relaxed & 1 << k == 0 => false,
                    Some(Steps::Elements) => true,
                    Some(Steps::Deref) => {
                        !copy_writes && !super::writes_in(b, i + 1..call, may_write)
                    }
                    None => false,
                }
            })
        })
        .fold(0, |mask, (k, _)| mask | 1 << k)
}
