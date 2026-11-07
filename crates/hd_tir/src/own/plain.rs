//! References that count nothing (#1360). A reference owns something only
//! when it is a representation view or a new object it made
//! (`crates/hd_wasm_linear/DESIGN.md` §23.20), so a `Drop` of a value that
//! holds only references of neither kind has no release to do; all it still
//! does is end its holder's loans. This pass, the last, turns such a `Drop`
//! into a `LoanEnd`.
//!
//! A value qualifies by where its reference comes from, since the type
//! cannot say (`OwnCx::plain_reference` only rules out the rest):
//!
//! - a `Ref` of a local or a top-level binding, through fields only: an
//!   address in a slot, a cell or a frame, none of which it counts. A
//!   dereference stays out: the reference it reads may be a view, whose
//!   projection is a new object, and a variant step may make a payload view.
//!   A capture's copy stays out too: a closure's reference to it is a new
//!   object holding it;
//! - a `Ref` of an element, or of fields of one, of a list no type parameter
//!   opens: an address in the buffer, whatever came before the element;
//! - a pointer's `as_ref` or `as_mut` (`OwnCx::plain_result`);
//! - a variant, tuple or struct of such values and uncounted ones, a
//!   same-value coercion, a part of one, a block's value, and a local that
//!   only such values are stored into.

use hd_base::SliceSort as _;

use super::OwnCx;
use crate::ir::{Body, Callee, NONE, Proj, Ref, Root, Tag, local_flags, ref_word, u32_of};

/// How far a value's origin is followed: past it, a `Drop` stays.
const DEPTH: u32 = 64;

struct Origins<'a> {
    b: &'a Body,
    cx: &'a dyn OwnCx,
    /// Each instruction's answer once asked, then each local's: `false` while
    /// it is being asked too, so a cycle counts as no proof.
    known: Vec<Option<bool>>,
    /// The values that flow into each `known` slot other than through its
    /// instruction, sorted: a block's breaks, a local's whole stores.
    flows: Vec<(u32, u32)>,
}

impl Origins<'_> {
    /// Whether slot `k` (an instruction, or `b.len()` plus a local) holds
    /// only references that own nothing.
    fn plain(&mut self, k: usize, depth: u32) -> bool {
        if depth == 0 {
            return false;
        }
        if let Some(known) = self.known[k] {
            return known;
        }
        self.known[k] = Some(false);
        let answer = self.origin(k, depth - 1) && {
            let at = u32_of(k);
            let from = self.flows.partition_point(|&(t, _)| t < at);
            let mut j = from;
            loop {
                match self.flows.get(j) {
                    Some(&(t, v)) if t == at => {
                        if !self.value(v, depth - 1) {
                            break false;
                        }
                        j += 1;
                    }
                    _ => break true,
                }
            }
        };
        self.known[k] = Some(answer);
        answer
    }

    fn value(&mut self, v: u32, depth: u32) -> bool {
        Ref(v).as_inst().is_some() && self.plain(v as usize, depth)
    }

    fn origin(&mut self, k: usize, depth: u32) -> bool {
        let b = self.b;
        let Some(&[a, w]) = b.data.get(k) else {
            // A local: its stores are its flows, and none means a parameter
            // or a capture's copy.
            let l = k - b.len();
            return self.cx.plain_reference(b.local_ty[l]) && {
                let at = u32_of(k);
                let i = self.flows.partition_point(|&(t, _)| t < at);
                self.flows.get(i).is_some_and(|f| f.0 == at)
            };
        };
        if !self.cx.plain_reference(b.ty[k]) {
            return false;
        }
        match b.tags[k] {
            Tag::Ref => {
                let p = b.view(a);
                // Whether only fields follow an element step, or no such
                // step exists yet.
                let (mut elem, mut fields) = (false, true);
                for s in p.steps(b) {
                    match s {
                        Proj::Elem(_) => (elem, fields) = (true, true),
                        Proj::Field(_) => {}
                        Proj::Deref | Proj::Variant(_) => fields = false,
                    }
                }
                let root = match p.root {
                    Root::Local(l) => Some(b.local_ty[l as usize]),
                    Root::Value(v) if Ref(v).as_inst().is_some() => Some(b.ty[v as usize]),
                    _ => None,
                };
                fields
                    && w & ref_word::TAKES == 0
                    && match p.root {
                        _ if elem => root.is_some(),
                        Root::Local(l) => !b.cap_local.iter().any(|c| c.raw() == l),
                        r => matches!(r, Root::Global(..)),
                    }
            }
            Tag::Call => Callee::from_words(b.record(a)).is_some_and(|c| self.cx.plain_result(&c)),
            Tag::NewVariant | Tag::NewTuple | Tag::NewStruct => b.record(w).iter().all(|&part| {
                let ty = match Ref(part).as_inst() {
                    Some(i) => b.ty[i.0 as usize],
                    None => b.consts[(part & !Ref::CONST_BIT) as usize].0,
                };
                !self.cx.counted(ty) || self.value(part, depth)
            }),
            Tag::Coerce => b.same_value_coercion(k) && !b.provider_ref(k) && self.value(a, depth),
            Tag::Read => {
                let p = b.view(a);
                p.steps(b)
                    .all(|s| matches!(s, Proj::Field(_) | Proj::Variant(_)))
                    && match p.root {
                        Root::Value(r) => self.value(r, depth),
                        Root::Local(l) => self.plain(b.len() + l as usize, depth),
                        _ => false,
                    }
            }
            Tag::Scope => self.value(a, depth),
            Tag::Block => w == NONE || self.value(w, depth),
            Tag::If | Tag::Match => b.record(w).iter().all(|&arm| self.value(arm, depth)),
            _ => false,
        }
    }
}

/// Turns each `Drop` of a whole value or local that holds only references
/// owning nothing into a `LoanEnd`.
pub(crate) fn loan_ends(b: &mut Body, cx: &dyn OwnCx) {
    let n = b.len();
    // The slot of `Drop` `i`'s value or local.
    let slot = |b: &Body, i: usize| {
        let p = b.view(b.data[i][0]);
        match p.root {
            _ if b.tags[i] != Tag::Drop || !p.whole() => None,
            Root::Local(l) => Some(n + l as usize),
            Root::Value(v) if Ref(v).as_inst().is_some() => Some(v as usize),
            _ => None,
        }
    };
    let ty = |b: &Body, k: usize| {
        if k < n { b.ty[k] } else { b.local_ty[k - n] }
    };
    if !(0..n).any(|i| slot(b, i).is_some_and(|k| cx.plain_reference(ty(b, k)))) {
        return;
    }
    let mut ends = Vec::new();
    {
        let body: &Body = b;
        let locals = body.local_ty.len();
        let mut t = Origins {
            b: body,
            cx,
            known: vec![None; n + locals],
            flows: Vec::new(),
        };
        for l in 0..locals {
            if body.local_flags[l] & local_flags::PARAM != 0 {
                t.known[n + l] = Some(false);
            }
        }
        for c in &body.cap_local {
            t.known[n + c.idx()] = Some(false);
        }
        for i in 0..n {
            let [a, w] = body.data[i];
            match body.tags[i] {
                Tag::Break if w != NONE => {
                    if let Some(&target) = body.label_inst.get(a as usize) {
                        t.flows.push((target, w));
                    }
                }
                Tag::Store | Tag::Ref => {
                    let p = body.view(a);
                    if let Root::Local(l) = p.root {
                        let at = n + l as usize;
                        if body.tags[i] == Tag::Store && p.whole() {
                            t.flows.push((u32_of(at), w));
                        } else if p.first(body) != Some(Proj::Deref) {
                            // Stored in part, or a reference reaches it.
                            t.known[at] = Some(false);
                        }
                    }
                }
                _ => {}
            }
        }
        t.flows.sort_stable();
        for i in 0..n {
            if slot(body, i).is_some_and(|k| t.plain(k, DEPTH)) {
                ends.push(i);
            }
        }
    }
    for i in ends {
        b.tags[i] = Tag::LoanEnd;
    }
}
