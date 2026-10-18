//! Owned temporaries (spec/lang/13-ownership.md, Drops): an owned value
//! that no binding, call, construction or result takes is dropped at the end
//! of its statement, as a `DropTemp`. A temporary that is only lent or read
//! through a field lives until then too.

use hd_base::NodeIdx;
use hd_tir::ir::{Body, Coercion, DropMark, NONE, Ref, Tag, TirSink};

use crate::body::Ck;

/// The value operands of instruction `i`, or `None` when its schema is not
/// known here.
fn value_refs(b: &Body, i: usize) -> Option<Vec<u32>> {
    if let Some(v) = b.value_operands(i) {
        return Some(v);
    }
    let [a, w] = b.data[i];
    let one = |v: u32| if v == NONE { vec![] } else { vec![v] };
    Some(match b.tags[i] {
        Tag::LocalSet | Tag::Break | Tag::Block => one(w),
        Tag::Return
        | Tag::If
        | Tag::Match
        | Tag::SwitchTag
        | Tag::SwitchInt
        | Tag::SwitchChar
        | Tag::SwitchStr
        | Tag::And
        | Tag::Or
        | Tag::AwaitValue
        | Tag::AwaitRace
        | Tag::DropTemp => one(a),
        Tag::CallHost | Tag::AwaitAll => b.record(w).to_vec(),
        Tag::ForList | Tag::ForMap => b.record(a).first().copied().into_iter().collect(),
        Tag::With => b.record(a).iter().skip(1).step_by(2).copied().collect(),
        Tag::LocalGet
        | Tag::GlobalGet
        | Tag::ProviderGet
        | Tag::Hole
        | Tag::Poison
        | Tag::ItemRef
        | Tag::Closure
        | Tag::Scope
        | Tag::Defer
        | Tag::Loop
        | Tag::Continue
        | Tag::Unreachable
        | Tag::Guard
        | Tag::ToArm
        | Tag::Hook
        | Tag::ContextNew
        | Tag::ContextFor
        | Tag::DropLocal => vec![],
        _ => return None,
    })
}

/// Whether instruction `i` reads a place, so its value is part of another
/// value and never a temporary of its own.
fn place_read(t: Tag) -> bool {
    matches!(
        t,
        Tag::LocalGet
            | Tag::GlobalGet
            | Tag::ProviderGet
            | Tag::Field
            | Tag::TupleGet
            | Tag::Unwrap
            | Tag::Payload
    )
}

impl Ck<'_, '_> {
    /// Whether `t` holds a borrow anywhere.
    fn holds_borrow(&self, t: hd_types::Ty) -> bool {
        use hd_types::TyData;
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        if !pool.has_mode(t) {
            return false;
        }
        match pool.get(t) {
            TyData::Borrow { .. } => true,
            TyData::Option(i) | TyData::Mut(i) => self.holds_borrow(i),
            TyData::Tuple { elems, rest } => pool
                .list_items(elems)
                .iter()
                .chain(rest.iter())
                .any(|&e| self.holds_borrow(e)),
            TyData::Adt { args, .. } => pool.list_items(args).iter().any(|&a| self.holds_borrow(a)),
            TyData::Fn { params, result, .. } => {
                self.holds_borrow(result)
                    || pool
                        .list_items(params)
                        .iter()
                        .any(|&p| self.holds_borrow(p))
            }
            _ => false,
        }
    }

    /// Drops the owned temporaries of the statement that started at `m`:
    /// every owned value it made that no use takes, newest first. `keep` is the statement's own value when it leaves as
    /// its block's result. A statement that cannot complete drops nothing.
    pub(crate) fn drop_temps(&mut self, m: DropMark, keep: Option<Ref>, done: bool) {
        if !done {
            return;
        }
        let items = self.b.statement_items(m);
        let Some(&first) = items.first() else {
            return;
        };
        let pool = self.pool();
        let body = self.b.body_mut();
        // Most statements hold no owned value: decide that cheaply first.
        let maybe = items.iter().any(|&i| {
            let t = body.ty[i as usize];
            pool.has_mode(t) || pool.has_infer(t)
        });
        if !maybe {
            return;
        }
        let n = self.b.body_mut().len();
        let mut temps = Vec::new();
        for &i in &items {
            let iu = i as usize;
            let (tag, ty) = {
                let body = self.b.body_mut();
                (body.tags[iu], body.ty[iu])
            };
            // A coercion makes a value of its own; a lend makes a borrow,
            // which owns nothing.
            if place_read(tag) || keep.is_some_and(|k| k.0 == i) || !self.owned_part(ty) {
                continue;
            }
            temps.push(i);
        }
        if temps.is_empty() {
            return;
        }
        // A borrow that the statement keeps past its end: a temporary lent
        // there would drop under it, so that form waits for loans.
        let escapes = {
            let body = self.b.body_mut();
            let kept: Vec<u32> = (first as usize..n)
                .filter_map(|k| {
                    let [a, w] = body.data[k];
                    match body.tags[k] {
                        Tag::LocalSet | Tag::Break => Some(w),
                        Tag::Return => Some(a),
                        Tag::FieldSet => body.record(w).get(1).copied(),
                        _ => None,
                    }
                })
                .chain(keep.map(|k| k.0))
                .filter(|v| *v != NONE && v & Ref::CONST_BIT == 0)
                .collect();
            kept.into_iter().any(|v| {
                let t = self.b.body_mut().ty[v as usize];
                self.holds_borrow(t)
            })
        };
        let body = self.b.body_mut();
        let mut dropped = Vec::new();
        let mut gap = None;
        for &c in &temps {
            let mut taken = false;
            let mut lent = false;
            let mut exit_after = false;
            for j in (c as usize + 1)..n {
                let tag = body.tags[j];
                if matches!(tag, Tag::Return | Tag::Break | Tag::Continue | Tag::Closure) {
                    exit_after = true;
                }
                let Some(refs) = value_refs(body, j) else {
                    // An instruction whose operands are not known here:
                    // any word naming the temporary takes it, so it is
                    // never dropped twice; the form waits for a later
                    // slice.
                    let [a, w] = body.data[j];
                    if a == c || w == c {
                        gap = Some(body.syn[j]);
                        taken = true;
                    }
                    continue;
                };
                if !refs.contains(&c) {
                    continue;
                }
                let lends = tag == Tag::Coerce && {
                    let kind = body.record(body.data[j][1]).first().copied();
                    kind.is_some_and(|k| {
                        [Coercion::Lend as u32, Coercion::LendMut as u32].contains(&k)
                    })
                };
                lent |= lends;
                let read = matches!(tag, Tag::Field | Tag::TupleGet | Tag::Is)
                    && (body.data[j][0] == c || tag == Tag::Is);
                let base = tag == Tag::FieldSet
                    && body.data[j][0] == c
                    && body.record(body.data[j][1]).get(1) != Some(&c);
                if !(lends || read || base) {
                    taken = true;
                }
            }
            if taken {
                continue;
            }
            // A `return`, `break`, `continue` or failing `?` after the
            // temporary leaves its statement before the statement's end.
            if exit_after || (lent && escapes) {
                gap = Some(body.syn[c as usize]);
                continue;
            }
            dropped.push(c);
        }
        if let Some(at) = gap
            && self.mode_gap.is_none()
        {
            self.mode_gap = Some(if at == NodeIdx::NONE {
                self.b.body_mut().syn[first as usize]
            } else {
                at
            });
        }
        for c in dropped.into_iter().rev() {
            let syn = self.b.body_mut().syn[c as usize];
            self.b.drop_temp(Ref(c), syn);
        }
    }
}
