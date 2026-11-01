//! Argument promotion (#1151), release only: a `&T` parameter whose body
//! only reads through it takes its referent by value in a promoted copy
//! of the body, and a direct call whose argument is the address of the
//! caller's own local passes that local's value instead
//! (`crates/hd_wasm_linear/DESIGN.md` §23.40). Whether `T` is small and
//! holds no counted leaf is the backend's question; these are the TIR
//! halves: which parameters the body only reads through
//! ([`read_only_params`]), the copy ([`promote`]), and which calls may
//! run it ([`promoted_call`]).

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

/// Body `b` with each parameter in `mask` ([`read_only_params`]) taking
/// its referent by value: the local's type is `referent` of its reference
/// type, and each `Read [*$l rest]` reads `[$l rest]`.
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
    for i in 0..b.len() {
        if b.tags[i] != Tag::Read {
            continue;
        }
        let Some(mut p) = b.decode_place(b.data[i][0]) else {
            continue;
        };
        let crate::ir::PlaceRoot::Local(l) = p.root else {
            continue;
        };
        if !promoted.contains(&l.raw()) || p.proj.first() != Some(&Proj::Deref) {
            continue;
        }
        p.proj.remove(0);
        out.data[i][0] = match p.words() {
            None => l.raw() | place::BARE_LOCAL,
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
    let flags = b.data[i][1];
    if b.tags[i] != Tag::Ref || flags & crate::ir::ref_word::MUT != 0 {
        return false;
    }
    let p = b.view(b.data[i][0]);
    let Root::Local(l) = p.root else {
        return false;
    };
    if !p.in_place(b) || b.cap_local.iter().any(|c| c.raw() == l) {
        return false;
    }
    let addressed = |j: usize| {
        j != i
            && b.tags[j] == Tag::Ref
            && matches!(b.view(b.data[j][0]).root, Root::Local(m) if m == l)
    };
    if flags & crate::ir::ref_word::TAKES != 0 && (0..b.len()).any(addressed) {
        return false;
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
            return false;
        }
    }
    seen == 1
}
