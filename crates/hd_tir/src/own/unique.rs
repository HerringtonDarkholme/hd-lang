//! What each instruction does to the counted block one place holds
//! (#2369): the facts behind `std.rt.is_unique` that the ownership passes
//! and the release proofs share.
//!
//! A holder is a local and the fields to a `*H` in it, never through a
//! dereference: `[$xs .block]`. While no other name reaches it (no `Ref`
//! of it or of what holds it, no capture, and each borrow of it lent
//! straight to a call or read through), its block's count changes only by
//! an instruction that names the holder. Each such instruction has one
//! effect:
//!
//! - [`Effect::Fresh`]: a store of a block nothing else holds, the result
//!   of `alloc_counted` or of a function whose every result is one
//!   ([`fresh_result`]): count 1;
//! - [`Effect::Shares`]: a copy of the block (a copy read, a store of a
//!   block that may be held elsewhere, a write of what holds it): its
//!   count may now be above 1;
//! - [`Effect::Ends`]: the block moves out or is dropped; the holder is
//!   read again only after a store;
//! - [`Effect::Releases`]: code that may let another holder of the same
//!   block go (a release, a drop, a call), so its count may fall to 1.
//!
//! So from a `Fresh` store, or a test that found count 1, up to the next
//! `Shares` or `Ends`, the block is unique on every path: nothing in
//! between can give it a second holder. That is Swift's
//! `end_cow_mutation [keep_unique]` after `makeMutableAndUnique`.

use super::{OwnCx, operands};
use crate::ir::count::{Info, Part};
use crate::ir::{Body, Callee, NONE, PlaceRoot, Proj, ReadMode, Ref, Tag, Topology};

/// A place that holds a counted block: a local and fields, no
/// dereference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holder {
    pub local: u32,
    pub fields: Vec<Proj>,
}

/// What an instruction does to the block a [`Holder`] names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Fresh,
    Shares,
    Ends,
    Releases,
    /// A borrow of the block used other than lent: it may be kept, and
    /// copied later by a name no instruction ties to the holder.
    Kept,
}

/// How a place stands to a holder's.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Relation {
    /// The holder itself.
    Exact,
    /// A place that holds the holder: its local or a field on the way.
    Contains,
    /// Unrelated, or through the block (its header or tail).
    Apart,
}

impl Holder {
    /// The holder place `word` names, if it is one.
    #[must_use]
    pub fn of(b: &Body, word: u32) -> Option<Self> {
        let p = b.decode_place(word)?;
        let PlaceRoot::Local(l) = p.root else {
            return None;
        };
        p.proj
            .iter()
            .all(|s| matches!(s, Proj::Field(_)))
            .then(|| Self {
                local: l.raw(),
                fields: p.proj,
            })
    }

    fn relation(&self, b: &Body, word: u32) -> Relation {
        let Some(p) = b.decode_place(word) else {
            return Relation::Apart;
        };
        if !matches!(p.root, PlaceRoot::Local(l) if l.raw() == self.local) {
            return Relation::Apart;
        }
        let shared = p
            .proj
            .iter()
            .zip(&self.fields)
            .take_while(|(a, b)| a == b)
            .count();
        if shared < p.proj.len().min(self.fields.len()) {
            Relation::Apart
        } else if p.proj.len() < self.fields.len() {
            Relation::Contains
        } else if p.proj.len() == self.fields.len() {
            Relation::Exact
        } else {
            // A step past a `*H` is its dereference: the block's memory.
            Relation::Apart
        }
    }
}

/// The value `v` stands for through same-type coercions and a scope
/// with one producing exit.
fn source(b: &Body, mut v: u32) -> u32 {
    for _ in 0..8 {
        let Some(i) = Ref(v)
            .as_inst()
            .map(|r| r.0 as usize)
            .filter(|&i| i < b.len())
        else {
            break;
        };
        v = match b.tags[i] {
            Tag::Coerce
                if Ref(b.data[i][0])
                    .as_inst()
                    .is_some_and(|o| b.ty.get(o.0 as usize) == Some(&b.ty[i])) =>
            {
                b.data[i][0]
            }
            Tag::Scope => match b.wrapper_value(i) {
                Some(w) => w,
                None => break,
            },
            _ => break,
        };
    }
    v
}

/// Whether value `v` is a block nothing else holds: a call of a callee
/// `fresh` names.
fn fresh_value(b: &Body, v: u32, fresh: &dyn Fn(&Callee) -> bool) -> bool {
    let v = source(b, v);
    Ref(v)
        .as_inst()
        .map(|r| r.0 as usize)
        .filter(|&i| i < b.len() && b.tags[i] == Tag::Call)
        .and_then(|i| Callee::from_words(b.record(b.data[i][0])))
        .is_some_and(|c| fresh(&c))
}

/// Per value, the instructions that use it (CSR), for the borrows.
pub struct Users {
    at: Vec<u32>,
    of: Vec<u32>,
}

impl Users {
    /// One census of `b`'s operands. O(instructions + operands).
    #[must_use]
    pub fn new(b: &Body) -> Self {
        let n = b.len();
        let mut count = vec![0u32; n + 1];
        let mut ops = Vec::new();
        for i in 0..n {
            operands(b, i, &mut ops);
            for op in &ops {
                count[op.value as usize + 1] += 1;
            }
            if let Some(v) = value_root(b, i) {
                count[v as usize + 1] += 1;
            }
        }
        for i in 0..n {
            count[i + 1] += count[i];
        }
        let mut fill = count.clone();
        let mut of = vec![0u32; count[n] as usize];
        for i in 0..n {
            operands(b, i, &mut ops);
            let root = value_root(b, i);
            for v in ops.iter().map(|op| op.value).chain(root) {
                of[fill[v as usize] as usize] = crate::ir::u32_of(i);
                fill[v as usize] += 1;
            }
        }
        Self { at: count, of }
    }

    fn of(&self, v: u32) -> &[u32] {
        &self.of[self.at[v as usize] as usize..self.at[v as usize + 1] as usize]
    }
}

/// The value a read's or store's place is rooted at, `[%v ..]`.
fn value_root(b: &Body, i: usize) -> Option<u32> {
    if !matches!(b.tags[i], Tag::Read | Tag::Store | Tag::Ref) {
        return None;
    }
    match b.decode_place(b.data[i][0])?.root {
        PlaceRoot::Value(v) if Ref(v.0).as_inst().is_some() && (v.0 as usize) < b.len() => {
            Some(v.0)
        }
        _ => None,
    }
}

/// Whether borrowed value `v` is only lent: passed to a call (whose
/// summary keeps none of it), coerced to be so, or read through.
fn only_lent(b: &Body, users: &Users, v: u32, depth: usize) -> bool {
    users.of(v).iter().all(|&u| {
        let u = u as usize;
        match b.tags[u] {
            Tag::Call | Tag::Read => true,
            Tag::Coerce => depth > 0 && only_lent(b, users, crate::ir::u32_of(u), depth - 1),
            _ => false,
        }
    })
}

/// Whether some name besides the holder reaches its block anywhere in
/// `b`: a capture of its local, or a `Ref` of it or of what holds it.
#[must_use]
pub fn reached_otherwise(b: &Body, h: &Holder) -> bool {
    b.cap_local.iter().any(|c| c.raw() == h.local)
        || (0..b.len())
            .any(|i| b.tags[i] == Tag::Ref && h.relation(b, b.data[i][0]) != Relation::Apart)
}

/// The effect of instruction `i` on `h`'s block, `None` for none.
/// `fresh` names the callees whose result is a fresh block; `pure` the
/// calls that run no other code.
pub fn effect(
    b: &Body,
    users: &Users,
    h: &Holder,
    i: usize,
    fresh: &dyn Fn(&Callee) -> bool,
    pure: &dyn Fn(usize) -> bool,
) -> Option<Effect> {
    match b.tags[i] {
        Tag::Read => match h.relation(b, b.data[i][0]) {
            Relation::Apart => None,
            _ if b.data[i][1] == ReadMode::Copy as u32 => Some(Effect::Shares),
            _ if b.data[i][1] == ReadMode::Move as u32 => Some(Effect::Ends),
            _ if only_lent(b, users, crate::ir::u32_of(i), 4) => None,
            _ => Some(Effect::Kept),
        },
        Tag::Store => match h.relation(b, b.data[i][0]) {
            Relation::Apart => None,
            Relation::Exact if fresh_value(b, b.data[i][1], fresh) => Some(Effect::Fresh),
            _ => Some(Effect::Shares),
        },
        Tag::Drop => match h.relation(b, b.data[i][0]) {
            Relation::Apart => Some(Effect::Releases),
            _ => Some(Effect::Ends),
        },
        Tag::Call if pure(i) => None,
        Tag::Ref
        | Tag::Prim
        | Tag::And
        | Tag::Or
        | Tag::Coerce
        | Tag::Is
        | Tag::Block
        | Tag::Scope
        | Tag::If
        | Tag::Loop
        | Tag::Break
        | Tag::Continue
        | Tag::Return
        | Tag::Unreachable
        | Tag::Match
        | Tag::SwitchTag
        | Tag::SwitchInt
        | Tag::SwitchChar
        | Tag::SwitchStr
        | Tag::Guard
        | Tag::ToArm
        | Tag::NewStruct
        | Tag::NewTuple
        | Tag::NewVariant
        | Tag::CopyStruct
        | Tag::RefIdentity
        | Tag::ItemRef
        | Tag::FlagSet
        | Tag::FlagClear
        | Tag::LoanBegin
        | Tag::LoanClose
        | Tag::LoanEnd
        | Tag::OwnLoans => None,
        Tag::Retain => retained(b, i)
            .filter(|&v| {
                let v = source(b, v);
                Ref(v)
                    .as_inst()
                    .map(|r| r.0 as usize)
                    .filter(|&r| r < b.len() && b.tags[r] == Tag::Read)
                    .is_some_and(|r| h.relation(b, b.data[r][0]) != Relation::Apart)
            })
            .map(|_| Effect::Shares),
        _ => Some(Effect::Releases),
    }
}

/// The value a `Retain` counts, when it names one.
fn retained(b: &Body, i: usize) -> Option<u32> {
    let info = Info::read(b, i)?;
    let at = b.data[i][0] as usize;
    match info.part {
        Part::Result => Some(crate::ir::u32_of(at)),
        Part::Operand(k) => b.call_args(at).get(k as usize).copied(),
        _ => None,
    }
}

/// Whether every result of `b`'s own sub-body is a block nothing else
/// holds (`Summary::fresh`): each `Return` value and the root's tail is a
/// fresh call's result or a move of one local whose every effect is a
/// fresh store, a release or that move. Read after the passes.
#[must_use]
pub fn fresh_result(b: &Body, cx: &dyn OwnCx) -> bool {
    let fresh = |c: &Callee| cx.returns_unique(c);
    let Some(&root) = b.sub_root.first() else {
        return false;
    };
    let topo = Topology::new(b);
    let mut results: Vec<u32> = (0..b.len())
        .filter(|&i| b.tags[i] == Tag::Return && topo.sub[i] == 0)
        .map(|i| b.data[i][0])
        .collect();
    match b.data[root as usize][1] {
        NONE => {}
        tail => results.push(tail),
    }
    if results.is_empty() {
        return false;
    }
    // The local the results move out, at most one.
    let mut held: Option<(Holder, Vec<u32>)> = None;
    for v in results {
        if fresh_value(b, v, &fresh) {
            continue;
        }
        let Some(i) = Ref(source(b, v)).as_inst().map(|r| r.0 as usize) else {
            return false;
        };
        if i >= b.len() || b.tags[i] != Tag::Read || b.data[i][1] != ReadMode::Move as u32 {
            return false;
        }
        let Some(h) = Holder::of(b, b.data[i][0]).filter(|h| h.fields.is_empty()) else {
            return false;
        };
        match &mut held {
            Some((g, moves)) if *g == h => moves.push(crate::ir::u32_of(i)),
            Some(_) => return false,
            None => held = Some((h, vec![crate::ir::u32_of(i)])),
        }
    }
    let Some((h, moves)) = held else {
        return true;
    };
    if b.local_flags[h.local as usize] & crate::ir::local_flags::PARAM != 0
        || reached_otherwise(b, &h)
    {
        return false;
    }
    let users = Users::new(b);
    let pure = |_: usize| true;
    let mut stored = false;
    for i in 0..b.len() {
        match effect(b, &users, &h, i, &fresh, &pure) {
            Some(Effect::Shares | Effect::Kept) => return false,
            Some(Effect::Fresh) => stored = true,
            Some(Effect::Ends) if !moves.contains(&crate::ir::u32_of(i)) => return false,
            // A release lets another block go; the returned move is the
            // result itself.
            Some(Effect::Ends | Effect::Releases) | None => {}
        }
    }
    stored
}
