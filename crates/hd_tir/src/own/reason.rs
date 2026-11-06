//! Why each count operation stands (`docs/future-work/OWNERSHIP_ARCHITECTURE.md`,
//! "The One Place Each Count Is Decided"). A `Drop` carries its reason in
//! its data word b; an `IsUnique`'s and a copy `Read`'s follow from the
//! statement ([`reason`]). Checked native builds independently verify the
//! named copy-read reasons. Where today's passes compute no fact behind
//! a count, it is `Conservative`, naming the site that placed it.

#[cfg(any(test, target_arch = "wasm32"))]
use super::OwnCx;
#[cfg(any(test, target_arch = "wasm32"))]
use super::values::{Level, after, statement_chain};
#[cfg(target_arch = "wasm32")]
use super::{Role, consumes, operands};
use crate::ir::{Body, Proj, ReadMode, Root, Tag, ref_word, unique_word};

#[cfg(any(test, target_arch = "wasm32"))]
use crate::ir::{NONE, Topology, u32_of};

#[cfg(not(target_arch = "wasm32"))]
mod future;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(all(test, not(target_arch = "wasm32")))]
use native::changes;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{check_copy_reasons, copy_reasons};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reason {
    /// A use consumes the value, which is lent or used again later.
    Stored,
    /// The value is read from a place a write may change while it lives.
    SecondOwner,
    /// The value is owned and its last use on this path lends, or it has
    /// none.
    LastUse,
    /// A store replaces an owned value in a place not known to be empty.
    Overwritten,
    /// A suspension point is cancelled while the value is live across it.
    Cancelled,
    /// The buffer is not fresh and may have been shared since its check.
    MayBeShared,
    /// A reference through a `*T` outlives its expression.
    PointerLoan,
    /// The value may be a representation view.
    View,
    /// The `&dyn` data count (#1354), a suspension's counted reference
    /// (#1355).
    ByDesign,
    /// No fact was computed: the site that placed the count.
    Conservative(&'static str),
}

const NAMED: [Reason; 9] = [
    Reason::Stored,
    Reason::SecondOwner,
    Reason::LastUse,
    Reason::Overwritten,
    Reason::Cancelled,
    Reason::MayBeShared,
    Reason::PointerLoan,
    Reason::View,
    Reason::ByDesign,
];

impl Reason {
    /// A `Drop`'s data word b: any other word is a conservative `Drop`.
    #[must_use]
    pub fn from_word(w: u32) -> Self {
        NAMED
            .get(w as usize)
            .copied()
            .unwrap_or(Reason::Conservative("drop"))
    }
}

/// The data word b of a `Drop` the passes place after a last use, or of
/// one that splits or sinks such a drop.
pub(crate) const LAST_USE: u32 = 2;
const _: () = assert!(matches!(NAMED[LAST_USE as usize], Reason::LastUse));

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Reason::Stored => "stored",
            Reason::SecondOwner => "second owner",
            Reason::LastUse => "last use",
            Reason::Overwritten => "overwritten",
            Reason::Cancelled => "cancelled",
            Reason::MayBeShared => "may be shared",
            Reason::PointerLoan => "pointer loan",
            Reason::View => "view",
            Reason::ByDesign => "by design",
            Reason::Conservative(site) => return write!(f, "conservative: {site}"),
        })
    }
}

/// The reason of count statement `i`: a `Drop`'s word; a `Drop` of a lent
/// `Ref` stays only for an instance whose view makes the referent fresh
/// (`ref_word::LENT`). Insertion puts an `IsUnique` before every change
/// and a copy before every read, proving neither; hoisting keeps a
/// covered `IsUnique` only for a view. `None` for any other instruction.
#[must_use]
pub fn reason(b: &Body, i: usize) -> Option<Reason> {
    let [a, w] = b.data[i];
    Some(match b.tags[i] {
        Tag::Drop => {
            let lent = b.view(a).value().is_some_and(|v| {
                b.tags[v as usize] == Tag::Ref && b.data[v as usize][1] & ref_word::LENT != 0
            });
            if lent {
                Reason::View
            } else {
                Reason::from_word(w)
            }
        }
        Tag::IsUnique if w == unique_word::COVERED => Reason::View,
        Tag::IsUnique => Reason::Conservative("unique"),
        Tag::Read if w == ReadMode::Copy as u32 => Reason::Conservative("copy read"),
        _ => return None,
    })
}

/// The reason of each `copy` read of `b`, a body after the passes, in
/// `out` (`None` for any other instruction): `Stored` when a use of the
/// read's value consumes it while its source keeps its own count (the
/// source owns none to give: a lent parameter, a borrowed temporary, a
/// place behind a dereference, in a buffer or in a top-level binding;
/// or the source is used again after the read), where a store of the
/// read alone into a value local whose every use lends binds the value
/// and consumes nothing; `SecondOwner` when the
/// value only lends, and a write may change its source while it lives (a
/// store into or change of the same local, a mutable `Ref` of it,
/// or, for a place behind a dereference or in a buffer, any write through
/// one and any call that may write; a drop or a move of the source is
/// no write: where the copy borrowed, the passes would place them after
/// it); otherwise `Conservative("copy
/// read")`, a copy no fact here explains. Labels only: the emitter's
/// counts view reads them.
#[cfg(target_arch = "wasm32")]
pub fn copy_reasons(b: &Body, topo: &Topology, cx: &dyn OwnCx, out: &mut Vec<Option<Reason>>) {
    let n = b.len();
    out.clear();
    out.resize(n, None);
    let copy = |i: usize| b.tags[i] == Tag::Read && b.data[i][1] == ReadMode::Copy as u32;
    if !(0..n).any(copy) {
        return;
    }
    // Per value: whether a use consumes it, its last use by index, and
    // how many uses it has besides drops.
    let (mut consumed, mut last, mut count) = (vec![false; n], vec![NONE; n], vec![0u32; n]);
    let mut ops = Vec::new();
    for i in 0..n {
        operands(b, i, &mut ops);
        for op in &ops {
            let v = op.value as usize;
            if op.role == Role::Dropped || v >= n {
                continue;
            }
            last[v] = if last[v] == NONE {
                u32_of(i)
            } else {
                last[v].max(u32_of(i))
            };
            count[v] += 1;
            let owner = if op.role == Role::Tail && !topo.is_root[i] {
                topo.owner[i]
            } else {
                NONE
            };
            consumed[v] |= consumes(b, cx, i, op, owner);
        }
    }
    // A store into all of a value local whose every use lends binds the
    // value, as a borrow's binding does: it consumes nothing, and the
    // value lives as long as the local is named.
    let mut classes = Vec::new();
    super::values::classify(b, &mut classes);
    let mut uses = Vec::new();
    super::values::uses(b, cx, topo, &classes, &mut uses);
    let mut lends_only: Vec<bool> = classes
        .iter()
        .map(|c| matches!(c, super::values::Local::Value(_)))
        .collect();
    for u in &uses {
        if u.how == super::values::Use::Consume {
            lends_only[u.local as usize] = false;
        }
    }
    for i in 0..n {
        if b.tags[i] != Tag::Store {
            continue;
        }
        let p = b.view(b.data[i][0]);
        let v = b.data[i][1] as usize;
        if let Some(l) = p.local()
            && v < n
            && p.whole()
            && lends_only[l as usize]
            && count[v] == 1
        {
            consumed[v] = false;
            for j in 0..n {
                if matches!(b.tags[j], Tag::Read | Tag::Drop | Tag::Ref)
                    && b.view(b.data[j][0]).local() == Some(l)
                {
                    last[v] = last[v].max(u32_of(j));
                }
            }
        }
    }
    let mut owned = vec![false; b.local_ty.len()];
    for &(_, l) in &b.exit_drops {
        owned[l.idx()] = true;
    }
    let (mut cu, mut cw) = (Vec::new(), Vec::new());
    for r in 0..n {
        if !copy(r) {
            continue;
        }
        let src = source(b, b.data[r][0]);
        out[r] = Some(if consumed[r] {
            let gives = match src.root {
                Some(Root::Local(l)) => owned[l as usize] && !src.behind,
                Some(Root::Value(v)) => !src.behind && !b.is_borrow(v),
                _ => false,
            };
            if !gives || used_again(b, topo, r, src.root, &mut cu, &mut cw) {
                Reason::Stored
            } else {
                Reason::Conservative("copy read")
            }
        } else if changed_while(b, topo, cx, r, last[r], src) {
            Reason::SecondOwner
        } else {
            Reason::Conservative("copy read")
        });
    }
}

/// What a read's place rests on ([`copy_reasons`]): its root, through the
/// reads of a chain (`%h = Read [$xs]`, then `[%h [%i]]`), and whether a
/// step on the way is a dereference or an element (`behind`).
#[derive(Clone, Copy)]
struct Source {
    root: Option<Root>,
    behind: bool,
}

fn source(b: &Body, mut w: u32) -> Source {
    let mut behind = false;
    for _ in 0..16 {
        let p = b.view(w);
        behind |= p.steps(b).any(|s| matches!(s, Proj::Deref | Proj::Elem(_)));
        match p.root {
            Root::Value(v) if b.tags[v as usize] == Tag::Read => w = b.data[v as usize][0],
            root => {
                return Source {
                    root: Some(root),
                    behind: behind || matches!(root, Root::Global(..) | Root::Ptr(_)),
                };
            }
        }
    }
    Source {
        root: None,
        behind: true,
    }
}

/// Whether root `root` of copy read `r` is used again after it: another
/// read, reference or capture of the same local (or use of the same
/// value) that may run after `r`, or one in a loop around `r` that holds
/// no whole store of the local.
#[cfg(any(test, target_arch = "wasm32"))]
fn used_again(
    b: &Body,
    topo: &Topology,
    r: usize,
    root: Option<Root>,
    cu: &mut Vec<Level>,
    cw: &mut Vec<Level>,
) -> bool {
    let touches = |i: usize| -> bool {
        match root {
            Some(Root::Local(l)) => match b.tags[i] {
                Tag::Read | Tag::Ref | Tag::IsUnique => b.view(b.data[i][0]).local() == Some(l),
                Tag::Closure => {
                    let rec = b.record(b.data[i][1]);
                    (rec[0]..rec[0] + rec[1]).any(|c| b.cap_local[c as usize].raw() == l)
                }
                _ => false,
            },
            Some(Root::Value(v)) => {
                let mut hit = false;
                crate::ir::slots(b, i, |_, x, s| {
                    hit |= x == v && matches!(s, crate::ir::Word::Value(_));
                });
                hit && b.tags[i] != Tag::Drop
            }
            _ => false,
        }
    };
    let whole_store = |i: usize| {
        b.tags[i] == Tag::Store
            && matches!(root, Some(Root::Local(l)) if {
                let p = b.view(b.data[i][0]);
                p.local() == Some(l) && p.whole()
            })
    };
    statement_chain(topo, u32_of(r), cu);
    let inner = topo.loop_of[r];
    let holds = |l: u32, i: usize| {
        let mut m = topo.loop_of[i];
        while m != NONE {
            if m == l {
                return true;
            }
            m = topo.loop_of[m as usize];
        }
        false
    };
    let renewed = inner != NONE && (0..b.len()).any(|i| whole_store(i) && holds(inner, i));
    (0..b.len()).any(|j| {
        if j == r || !touches(j) {
            return false;
        }
        if inner != NONE && !renewed && holds(inner, j) {
            return true;
        }
        statement_chain(topo, u32_of(j), cw);
        after(b, cu, cw)
    })
}

/// Whether a write may change the source of copy read `r` while its value
/// lives, up to its last use `end` ([`copy_reasons`]), over the range of
/// instructions from the first of the read's statement.
#[cfg(any(test, target_arch = "wasm32"))]
fn changed_while(
    b: &Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    r: usize,
    end: u32,
    src: Source,
) -> bool {
    if end == NONE {
        return false;
    }
    let lo = topo.first[r] as usize;
    let local = match src.root {
        Some(Root::Local(l)) => Some(l),
        _ => None,
    };
    (lo..=end as usize).any(|i| {
        if i == r {
            return false;
        }
        let names_local = |i: usize| {
            local.is_some() && {
                let p = b.view(b.data[i][0]);
                p.local() == local || matches!(p.root, Root::Local(l) if Some(l) == local)
            }
        };
        match b.tags[i] {
            Tag::Store | Tag::IsUnique => {
                names_local(i)
                    || (src.behind
                        && b.view(b.data[i][0])
                            .steps(b)
                            .any(|s| matches!(s, Proj::Deref | Proj::Elem(_))))
            }
            Tag::Ref => b.data[i][1] & ref_word::MUT != 0 && names_local(i),
            Tag::Call => {
                src.behind
                    && crate::ir::Callee::from_words(b.record(b.data[i][0]))
                        .is_none_or(|c| cx.may_write(&c))
            }
            Tag::CallValue | Tag::CallDyn => src.behind,
            _ => false,
        }
    })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, Callee, Providers, TirBuilder, TirSink, local_flags};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::{Ty, TyList};

    fn example(takes: bool, writes: bool, binding: bool) -> (Body, usize) {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let syn = NodeIdx::from_raw(1);
        let block = b.open_block();
        let local = b.local(Ty::STRING, Symbol::from_raw(0), local_flags::PARAM, syn);
        let read = b.get(local, Ty::STRING, syn);
        let argument = if binding {
            let bound = b.local(Ty::STRING, Symbol::from_raw(1), 0, syn);
            b.set(bound, read, syn);
            Some(bound)
        } else {
            None
        };
        if writes {
            let fresh = b.call(
                &Callee::Item {
                    def: DefId::from_raw(7),
                    targs: TyList::EMPTY,
                },
                &[],
                Providers::None,
                Ty::STRING,
                syn,
            );
            b.set(local, fresh, syn);
        }
        let argument = argument.map_or(read, |local| b.get(local, Ty::STRING, syn));
        b.call(
            &Callee::Item {
                def: DefId::from_raw(if takes { 9 } else { 8 }),
                targs: TyList::EMPTY,
            },
            &[argument],
            Providers::None,
            Ty::VOID,
            syn,
        );
        let root = b.close_block(block, None, Ty::VOID, syn);
        let mut body = b.finish(root, &[]).expect("TIR");
        for i in 0..body.len() {
            if body.tags[i] == Tag::Read {
                body.data[i][1] = ReadMode::Copy as u32;
            }
        }
        (body, read.0 as usize)
    }

    #[test]
    fn copy_reasons_need_a_consuming_use_or_a_write_during_the_loan() {
        for (takes, writes, binding, expected) in [
            (true, false, false, Reason::Stored),
            (false, true, false, Reason::SecondOwner),
            (false, true, true, Reason::SecondOwner),
            (false, false, false, Reason::Conservative("copy read")),
        ] {
            let (body, read) = example(takes, writes, binding);
            let topo = Topology::new(&body);
            let mut reasons = Vec::new();
            copy_reasons(&body, &topo, &crate::own::tests::Cx, &mut reasons);
            assert_eq!(reasons[read], Some(expected));
            assert!(check_copy_reasons(&body, &topo, &crate::own::tests::Cx, &reasons).is_empty());
        }
    }

    #[test]
    fn sabotage_a_stored_label_on_a_read_only_copy_is_rejected() {
        let (body, read) = example(false, false, false);
        let topo = Topology::new(&body);
        let mut reasons = vec![None; body.len()];
        reasons[read] = Some(Reason::Stored);
        assert_eq!(
            check_copy_reasons(&body, &topo, &crate::own::tests::Cx, &reasons).len(),
            1
        );
    }

    #[test]
    fn sabotage_a_second_owner_label_on_a_consumed_copy_is_rejected() {
        let (body, read) = example(true, false, false);
        let topo = Topology::new(&body);
        let mut reasons = vec![None; body.len()];
        reasons[read] = Some(Reason::SecondOwner);
        assert_eq!(
            check_copy_reasons(&body, &topo, &crate::own::tests::Cx, &reasons).len(),
            1
        );
    }
}
