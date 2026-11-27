//! What each reference value refers to, as a key in one place tree per
//! body (#2374), for [`super::loop_loads`] and [`super::load_reuse`].
//!
//! A key is a node of the tree: a local, then fields and dereferences.
//! Equal places are one node (hash-consed, as rustc's `MovePathIndex`
//! names each move path once), so a key compares and hashes as one word,
//! and a longer place is one child of its parent's node.
//!
//! A value's definition has at most one parent: the value
//! a local's one store gives it, an identity coercion's source, a scope's
//! wrapped value, or the reference a place starts from. It is computed
//! once, from its parent's, and kept: a chain of definitions of any length
//! costs one step per definition, with no cap on how far a key follows.
//! A parent is always an earlier instruction, so the walk ends.
//!
//! A definition depends on two facts that hold or not where a consumer
//! uses it, which it checks itself: the root local keeps the value it had
//! where the chain read it ([`Def::root_at`]), and memory read on the way
//! ([`Def::mem_at`]) is not written before the consumer's end.
//!
//! Cost: O(1) per definition plus O(1) per projection step, each hashed
//! once; a key's projections are listed in O(its length).

use super::{Nest, Sets, Topology, scans};
use hd_tir::ir::{Body, NONE, Place, PlaceRoot, Proj, Ref, Tag, local_flags, u32_of};
use std::cell::RefCell;

/// A place through a local: a node of the body's place tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Key(u32);

/// A place tree node: its parent and last step (`NONE` and no step for a
/// local), its root local and its number of dereferences.
struct Node {
    parent: u32,
    step: Proj,
    local: u32,
    derefs: u32,
}

/// The definition of a reference value: the key of its referent, the
/// instruction that reads the key's root local, and the first one on its
/// way that reads memory through a reference (`NONE` for none).
#[derive(Clone, Copy, Debug)]
pub(super) struct Def {
    pub(super) key: Key,
    pub(super) root_at: u32,
    pub(super) mem_at: u32,
}

/// An instruction's definition: not computed yet, or computed.
#[derive(Clone, Copy)]
enum Slot {
    Todo,
    Done(Option<Def>),
}

/// One body's place tree and memoized definitions.
pub(super) struct Defs<'a> {
    b: &'a Body,
    topo: &'a Topology,
    nest: &'a Nest,
    sets: &'a Sets,
    /// Whether a local lives in a cell or slot of a sub-body.
    stored: &'a dyn Fn(u32, usize) -> bool,
    nodes: RefCell<Vec<Node>>,
    children: RefCell<hd_base::HashMap<(u32, Proj), u32>>,
    roots: RefCell<Vec<u32>>,
    /// Each instruction's definition, once computed.
    memo: RefCell<Vec<Slot>>,
}

impl<'a> Defs<'a> {
    pub(super) fn new(
        b: &'a Body,
        topo: &'a Topology,
        nest: &'a Nest,
        sets: &'a Sets,
        stored: &'a dyn Fn(u32, usize) -> bool,
    ) -> Self {
        Self {
            b,
            topo,
            nest,
            sets,
            stored,
            nodes: RefCell::new(Vec::new()),
            children: RefCell::default(),
            roots: RefCell::new(vec![NONE; b.local_ty.len()]),
            memo: RefCell::new(vec![Slot::Todo; b.len()]),
        }
    }

    /// The key of local `k` itself.
    fn root(&self, k: u32) -> Key {
        let mut roots = self.roots.borrow_mut();
        let mut nodes = self.nodes.borrow_mut();
        let at = &mut roots[k as usize];
        if *at == NONE {
            *at = u32_of(nodes.len());
            nodes.push(Node {
                parent: NONE,
                step: Proj::Deref,
                local: k,
                derefs: 0,
            });
        }
        Key(*at)
    }

    /// The key of `step` from the place `k` names.
    fn child(&self, k: Key, step: Proj) -> Key {
        let mut nodes = self.nodes.borrow_mut();
        let next = u32_of(nodes.len());
        let id = *self
            .children
            .borrow_mut()
            .entry((k.0, step))
            .or_insert(next);
        if id == next {
            let parent = &nodes[k.0 as usize];
            let node = Node {
                parent: k.0,
                step,
                local: parent.local,
                derefs: parent.derefs + u32::from(step == Proj::Deref),
            };
            nodes.push(node);
        }
        Key(id)
    }

    /// The local key `k` starts from.
    pub(super) fn local(&self, k: Key) -> u32 {
        self.nodes.borrow()[k.0 as usize].local
    }

    /// How many dereferences key `k` takes.
    pub(super) fn derefs(&self, k: Key) -> u32 {
        self.nodes.borrow()[k.0 as usize].derefs
    }

    /// Key `k`'s steps, first to last.
    pub(super) fn proj(&self, k: Key) -> Vec<Proj> {
        let nodes = self.nodes.borrow();
        let mut out = Vec::new();
        let mut at = k.0;
        while nodes[at as usize].parent != NONE {
            out.push(nodes[at as usize].step);
            at = nodes[at as usize].parent;
        }
        out.reverse();
        out
    }

    /// Whether places `a` and `b` are provably apart: through one local,
    /// they part at two fields. A place through another local may be
    /// reached by both, so it is not.
    pub(super) fn apart(&self, a: Key, b: Key) -> bool {
        if self.local(a) != self.local(b) {
            return false;
        }
        let (pa, pb) = (self.proj(a), self.proj(b));
        pa.iter()
            .zip(&pb)
            .find(|(x, y)| x != y)
            .is_some_and(|steps| matches!(steps, (Proj::Field(_), Proj::Field(_))))
    }

    /// The definition of place `p`, which instruction `at` names.
    pub(super) fn place(&self, p: &Place, at: u32) -> Option<Def> {
        loop {
            match self.of_place(p, at as usize) {
                Ok(d) => return d,
                Err(j) => self.resolve(j),
            }
        }
    }

    /// Computes instruction `i`'s definition and those it needs, each
    /// once, latest first off a stack (no recursion).
    fn resolve(&self, i: usize) {
        let mut stack = vec![i];
        while let Some(&x) = stack.last() {
            if let Slot::Done(_) = self.memo.borrow()[x] {
                stack.pop();
                continue;
            }
            match self.compute(x) {
                Ok(d) => {
                    scans(1);
                    self.memo.borrow_mut()[x] = Slot::Done(d);
                    stack.pop();
                }
                Err(j) => stack.push(j),
            }
        }
    }

    /// The definition of value `v` as the parent of instruction `i`, or
    /// the earlier instruction to compute first.
    fn parent(&self, v: u32, i: usize) -> Result<Option<Def>, usize> {
        let Some(j) = Ref(v).as_inst().map(|j| j.0 as usize).filter(|&j| j < i) else {
            return Ok(None);
        };
        match self.memo.borrow()[j] {
            Slot::Done(d) => Ok(d),
            Slot::Todo => Err(j),
        }
    }

    /// The value local `k`'s one store gives it where `at` reads it: a
    /// local no reference, cell or capture can change behind its store.
    fn single(&self, k: usize, at: usize) -> Option<u32> {
        let b = self.b;
        let [(set, value)] = self.sets.stores[k].as_slice() else {
            return None;
        };
        (*value != NONE
            && !self.sets.referenced[k]
            && !(self.stored)(self.topo.sub[at], k)
            && b.local_flags[k] & (local_flags::CAPTURED | local_flags::CAPTURED_ASSIGNED) == 0
            && self.nest.dominates(self.topo, *set, u32_of(at)))
        .then_some(*value)
    }

    /// Instruction `i`'s definition, or the earlier instruction it needs.
    fn compute(&self, i: usize) -> Result<Option<Def>, usize> {
        let b = self.b;
        Ok(match b.tags[i] {
            Tag::Read if let Some(k) = b.local_read(i) => {
                let parent = match self.single(k.idx(), i) {
                    Some(w) => self.parent(w, i)?,
                    None => None,
                };
                Some(parent.unwrap_or_else(|| Def {
                    key: self.child(self.root(k.raw()), Proj::Deref),
                    root_at: u32_of(i),
                    mem_at: NONE,
                }))
            }
            Tag::Read => {
                let Some(p) = b.decode_place(b.data[i][0]) else {
                    return Ok(None);
                };
                self.of_place(&p, i)?.map(|d| {
                    let through = self.derefs(d.key) != 0;
                    Def {
                        key: self.child(d.key, Proj::Deref),
                        root_at: d.root_at,
                        mem_at: if d.mem_at == NONE && through {
                            u32_of(i)
                        } else {
                            d.mem_at
                        },
                    }
                })
            }
            Tag::Ref => match b.decode_place(b.data[i][0]) {
                Some(p) => self.of_place(&p, i)?,
                None => None,
            },
            Tag::Coerce
                if b.record(b.data[i][1]).first()
                    == Some(&(hd_tir::ir::Coercion::Identity as u32)) =>
            {
                self.parent(b.data[i][0], i)?
            }
            Tag::Scope
                if let Some(w) = b.wrapper_value(i)
                    && self.sets.breaks.get(b.data[i][0] as usize) == Some(&1) =>
            {
                self.parent(w, i)?
            }
            _ => None,
        })
    }

    /// The definition of place `p` that instruction `at` names, or the
    /// earlier instruction it needs. A reference local set once is keyed
    /// by what it refers to, so two copies of one reference (an inlined
    /// `self` each) name one place.
    fn of_place(&self, p: &Place, at: usize) -> Result<Option<Def>, usize> {
        let deref = p.proj.first() == Some(&Proj::Deref);
        let own = |k: u32| Def {
            key: self.root(k),
            root_at: u32_of(at),
            mem_at: NONE,
        };
        let (mut def, rest) = match p.root {
            PlaceRoot::Local(k) if deref && let Some(w) = self.single(k.idx(), at) => {
                match self.parent(w, at)? {
                    Some(d) => (d, &p.proj[1..]),
                    None => (own(k.raw()), &p.proj[..]),
                }
            }
            PlaceRoot::Local(k) => (own(k.raw()), &p.proj[..]),
            PlaceRoot::Value(v) if deref => match self.parent(v.0, at)? {
                Some(d) => (d, &p.proj[1..]),
                None => return Ok(None),
            },
            _ => return Ok(None),
        };
        for step in rest {
            match step {
                Proj::Field(_) | Proj::Deref => def.key = self.child(def.key, *step),
                Proj::Elem(_) | Proj::Variant(_) => return Ok(None),
            }
        }
        Ok(Some(def))
    }
}
