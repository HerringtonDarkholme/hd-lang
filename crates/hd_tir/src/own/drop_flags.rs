//! Drop flags as TIR locals (#1420, `docs/future-work/INSTANCE_TIR.md`
//! step 7), for value roots: a local no closure captures and an
//! instruction's value. A captured local's cell keeps its flags in its
//! layout, and a frame's in the frame (#1420 part 2).
//!
//! Follows rustc's `MoveData` (`rustc_mir_dataflow::move_paths`) and
//! `ElaborateDrops`, which runs on generic MIR before monomorphization:
//! one path per place a move, a store or a drop of the body names,
//! interned by (parent, step), not one per node of the root's type. A
//! path needs a flag only when a move empties it or a path under it; any
//! other hooked node reads the flag of its nearest path above it. A hook
//! at node P runs when that flag and the flags of every flagged path
//! under P hold, so a store sets only the paths at and under its place
//! and never recomputes a parent. A root whose type may hold a hook
//! ([`OwnCx::holds_drop`] not `No`, so a type parameter too) gets flags;
//! an instance whose type holds none leaves them unread.
//!
//! The statements: `FlagSet [p] $f` sets path `p`'s flag local `$f`,
//! `FlagClear [p] $f` clears it. Each init of a root (a parameter at its
//! sub-body's entry, a `Store` to the local, an instruction's value where
//! it is made) sets the flags at and under its place; each `Read move`,
//! and each `Drop` of a root dropped at more than one site, clears them.
//!
//! Cost: one walk of the body, O(N + S) for N instructions and S place
//! steps, one hash probe per step; then one statement per flagged path
//! under each init, move or drop, which is the output's size.

use hd_base::HashMap;
use hd_types::Ty;

use super::{OwnCx, Tri, splice};
use crate::ir::{
    Body, CaptureMode, NONE, ReadMode, Root, Storage, Tag, Topology, local_flags, place, u32_of,
};

/// The root of a move path: a local, or an instruction's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Local(u32),
    Value(u32),
}

/// The body's move paths, roots first in the order the walk meets them.
#[derive(Default)]
pub struct MovePaths {
    pub key: Vec<Key>,
    /// The path one step up, or `NONE` for a root's whole path.
    pub parent: Vec<u32>,
    /// The step from the parent, as its place word (a field or a
    /// variant), or `NONE` for a root's whole path.
    pub step: Vec<u32>,
    /// A `Read move`, or a `Drop` of a root dropped at more than one site,
    /// empties the path.
    pub moved: Vec<bool>,
    /// The first path one step down, and the next path under the same
    /// parent, or `NONE`.
    pub child: Vec<u32>,
    pub sibling: Vec<u32>,
    by_key: HashMap<Key, u32>,
    by_step: HashMap<(u32, u32), u32>,
}

/// The step words of place word `word`, one per field or variant step
/// of a place [`key_of`] admits.
fn step_words(b: &Body, word: u32) -> &[u32] {
    let v = b.view(word);
    &b.extra[v.from as usize..v.to as usize]
}

/// The root key of place word `word`, when it names a part of a local or
/// an instruction's value in place (fields and variants only).
fn key_of(b: &Body, word: u32) -> Option<Key> {
    let p = b.view(word);
    let key = match p.root {
        Root::Local(l) => Key::Local(l),
        Root::Value(v) => Key::Value(v),
        _ => return None,
    };
    p.in_place(b).then_some(key)
}

impl MovePaths {
    /// The table of `b`'s moves, stores and drops of places that lie in
    /// a root `tracked` admits. `dropped` counts each root's drop sites
    /// beyond its `Drop` statements (exit and unwind rows).
    #[must_use]
    pub fn build(b: &Body, tracked: impl Fn(Key) -> bool, dropped: &HashMap<Key, u32>) -> Self {
        let mut out = Self::default();
        let mut drops = HashMap::<Key, u32>::default();
        for i in 0..b.len() {
            if b.tags[i] == Tag::Drop
                && let Some(key) = key_of(b, b.data[i][0])
            {
                *drops.entry(key).or_default() += 1;
            }
        }
        for i in 0..b.len() {
            let word = b.data[i][0];
            let moved = match b.tags[i] {
                Tag::Read => b.data[i][1] == ReadMode::Move as u32,
                Tag::Store => false,
                Tag::Drop => key_of(b, word)
                    .is_some_and(|k| drops[&k] + dropped.get(&k).copied().unwrap_or(0) > 1),
                _ => continue,
            };
            let Some(key) = key_of(b, word).filter(|&k| tracked(k)) else {
                continue;
            };
            let mut at = out.root(key);
            for &step in step_words(b, word) {
                at = out.child_of(at, step);
            }
            out.moved[at as usize] |= moved;
        }
        out
    }

    fn push(&mut self, key: Key, parent: u32, step: u32) -> u32 {
        let at = u32_of(self.key.len());
        self.key.push(key);
        self.parent.push(parent);
        self.step.push(step);
        self.moved.push(false);
        self.child.push(NONE);
        self.sibling.push(NONE);
        if parent != NONE {
            self.sibling[at as usize] = self.child[parent as usize];
            self.child[parent as usize] = at;
        }
        at
    }

    fn root(&mut self, key: Key) -> u32 {
        if let Some(&at) = self.by_key.get(&key) {
            return at;
        }
        let at = self.push(key, NONE, NONE);
        self.by_key.insert(key, at);
        at
    }

    fn child_of(&mut self, parent: u32, step: u32) -> u32 {
        if let Some(&at) = self.by_step.get(&(parent, step)) {
            return at;
        }
        let at = self.push(self.key[parent as usize], parent, step);
        self.by_step.insert((parent, step), at);
        at
    }

    /// The path of place word `word`, when the table holds it.
    fn find(&self, b: &Body, word: u32) -> Option<u32> {
        let mut at = *self.by_key.get(&key_of(b, word)?)?;
        for &step in step_words(b, word) {
            at = *self.by_step.get(&(at, step))?;
        }
        Some(at)
    }

    /// Per path, whether it needs a flag local: a moved path, and every
    /// path above one (its whole root included), since a drop of the
    /// parent must skip the moved part's hook. One pass up from each
    /// moved path, stopping at a path already marked: O(paths).
    #[must_use]
    pub fn flagged(&self) -> Vec<bool> {
        let mut out = vec![false; self.key.len()];
        for at in (0..self.key.len()).filter(|&at| self.moved[at]) {
            let mut p = u32_of(at);
            while p != NONE && !out[p as usize] {
                out[p as usize] = true;
                p = self.parent[p as usize];
            }
        }
        out
    }

    /// Calls `f` on `at` and each path under it whose flag `flags` holds
    /// (`NONE` for none), with no recursion. A path with no flag has
    /// none under it ([`MovePaths::flagged`]).
    fn each_flag(&self, at: u32, flags: &[u32], mut f: impl FnMut(u32)) {
        let mut stack = vec![at];
        while let Some(p) = stack.pop() {
            if flags[p as usize] == NONE {
                continue;
            }
            f(p);
            let mut c = self.child[p as usize];
            while c != NONE {
                stack.push(c);
                c = self.sibling[c as usize];
            }
        }
    }
}

/// The place word of path `at`: a bare local, or a record in `b.extra`.
fn path_word(b: &mut Body, paths: &MovePaths, at: u32) -> u32 {
    let mut steps = Vec::new();
    let mut p = at;
    while paths.step[p as usize] != NONE {
        steps.push(paths.step[p as usize]);
        p = paths.parent[p as usize];
    }
    let (root, v) = match paths.key[at as usize] {
        Key::Local(l) if steps.is_empty() => return place::BARE_LOCAL | l,
        Key::Local(l) => (place::ROOT_LOCAL, l),
        Key::Value(v) => (place::ROOT_VALUE, v),
    };
    let word = u32_of(b.extra.len());
    b.extra.push(u32_of(steps.len() + 2));
    b.extra.extend([root, v]);
    b.extra.extend(steps.iter().rev());
    word
}

/// The insertion point just after instruction `i`: past the count
/// statements that follow it, which stay next to their operation.
fn after(b: &Body, topo: &Topology, i: usize) -> (u32, u32) {
    let (blk, pos) = (topo.blk[i], topo.pos[i] + 1);
    let members = b.record(b.data[blk as usize][0]);
    let counts = members[pos as usize..]
        .iter()
        .take_while(|&&m| {
            super::flow::charge(1);
            matches!(
                b.tags[m as usize],
                Tag::Retain | Tag::Release | Tag::MakeUnique
            )
        })
        .count();
    (blk, pos + u32_of(counts))
}

/// Writes the drop flags of `b`'s value roots (module doc) as edits.
pub(crate) fn run(b: &mut Body, cx: &dyn OwnCx, topo: &Topology, edits: &mut splice::Tables) {
    // A local a closure shares, through a cell, keeps its flags in the
    // cell (#1420 part 2).
    let mut cells = vec![false; b.local_ty.len()];
    for r in b.storage.iter().filter(|r| r.2 == Storage::Cell) {
        cells[r.1.idx()] = true;
    }
    for (l, _) in (b.cap_local.iter().zip(&b.cap_mode))
        .filter(|(_, m)| matches!(m, CaptureMode::Shared | CaptureMode::Frame))
    {
        cells[l.idx()] = true;
    }
    let mut dropped = HashMap::<Key, u32>::default();
    for &(_, l) in &b.exit_drops {
        *dropped.entry(Key::Local(l.raw())).or_default() += 1;
    }
    for &(_, v) in &b.unwind_drops {
        *dropped.entry(Key::Value(v)).or_default() += 1;
    }
    let mut hooked = HashMap::<Key, bool>::default();
    let paths = {
        let b = &*b;
        let tracked = |k: Key| match k {
            Key::Local(l) => !cells[l as usize] && cx.holds_drop(b.local_ty[l as usize]) != Tri::No,
            Key::Value(v) => cx.holds_drop(b.ty[v as usize]) != Tri::No,
        };
        let memo = std::cell::RefCell::new(&mut hooked);
        MovePaths::build(
            b,
            |k| *memo.borrow_mut().entry(k).or_insert_with(|| tracked(k)),
            &dropped,
        )
    };
    // A cancellation ladder releases every unwind row of its scope, so a
    // value with one that an operand consumes is moved whole there, as
    // the known-null pass clears its bit (`known.rs`).
    let mut paths = paths;
    let mut consumed = Vec::new();
    // A closure that moves a capture moves all of its local.
    for i in (0..b.len()).filter(|&i| b.tags[i] == Tag::Closure && topo.blk[i] != NONE) {
        let [start, len] = b.record(b.data[i][1])[..2] else {
            continue;
        };
        for c in (start..start + len).map(|c| c as usize) {
            let key = Key::Local(b.cap_local[c].raw());
            if b.cap_mode[c] == CaptureMode::Move
                && *hooked.entry(key).or_insert_with(|| {
                    !cells[b.cap_local[c].idx()]
                        && cx.holds_drop(b.local_ty[b.cap_local[c].idx()]) != Tri::No
                })
            {
                let at = paths.root(key);
                paths.moved[at as usize] = true;
                consumed.push((u32_of(i), at));
            }
        }
    }
    let mut ops = Vec::new();
    if !b.unwind_drops.is_empty() {
        for i in 0..b.len() {
            super::operands(b, i, &mut ops);
            for op in &ops {
                let key = Key::Value(op.value);
                if dropped.contains_key(&key)
                    && topo.blk[i] != NONE
                    && *hooked
                        .entry(key)
                        .or_insert_with(|| cx.holds_drop(b.ty[op.value as usize]) != Tri::No)
                    && !matches!(op.role, super::Role::Dropped | super::Role::Tail)
                    && super::consumes(b, cx, i, op, NONE)
                {
                    let at = paths.root(key);
                    paths.moved[at as usize] = true;
                    consumed.push((u32_of(i), at));
                }
            }
        }
    }
    let flagged = paths.flagged();
    if !flagged.contains(&true) {
        return;
    }
    // One flag local and one place word per flagged path.
    let mut flags = vec![NONE; flagged.len()];
    let mut words = vec![NONE; flagged.len()];
    for at in (0..flagged.len()).filter(|&at| flagged[at]) {
        flags[at] = u32_of(b.local_ty.len());
        let syn = match paths.key[at] {
            Key::Local(l) => b.local_syn[l as usize],
            Key::Value(v) => b.syn[v as usize],
        };
        b.local_ty.push(Ty::BOOL);
        b.local_name.push(cx.temporary_owner_name());
        b.local_syn.push(syn);
        b.local_flags
            .push(local_flags::ASSIGNED | local_flags::READ);
        words[at] = path_word(b, &paths, u32_of(at));
    }
    let mut put = |blk: u32, pos: u32, tag: Tag, at: u32, syn| {
        paths.each_flag(at, &flags, |p| {
            edits.insert(
                blk,
                pos,
                tag,
                [words[p as usize], flags[p as usize]],
                Ty::VOID,
                syn,
            );
        });
    };
    for &(i, at) in &consumed {
        let (blk, pos) = after(b, topo, i as usize);
        put(blk, pos, Tag::FlagClear, at, b.syn[i as usize]);
    }
    // A parameter is whole at its sub-body's entry: a sub-body's own, and
    // the root's, each marked `PARAM` and no sub-body's.
    let mut sub_of = vec![0; b.local_ty.len()];
    for s in 1..b.sub_params.len() {
        for &l in b.sub_param_locals(s) {
            sub_of[l as usize] = s;
        }
    }
    for (l, &s) in sub_of.iter().enumerate() {
        if b.local_flags[l] & local_flags::PARAM != 0
            && let Some(&at) = paths.by_key.get(&Key::Local(u32_of(l)))
        {
            let root = b.sub_root[s];
            put(root, 0, Tag::FlagSet, at, b.syn[root as usize]);
        }
    }
    for i in 0..b.len() {
        if topo.blk[i] == NONE {
            continue;
        }
        // `after` runs only where a statement goes: a count statement is
        // never one, so each run of them is scanned once, O(N) in all.
        let syn = b.syn[i];
        if let Some(&at) = paths.by_key.get(&Key::Value(u32_of(i))) {
            let (blk, pos) = after(b, topo, i);
            put(blk, pos, Tag::FlagSet, at, syn);
        }
        let tag = match b.tags[i] {
            Tag::Store => Tag::FlagSet,
            Tag::Read if b.data[i][1] == ReadMode::Move as u32 => Tag::FlagClear,
            Tag::Drop => Tag::FlagClear,
            _ => continue,
        };
        if let Some(at) = paths.find(b, b.data[i][0])
            && (tag != Tag::FlagClear || b.tags[i] == Tag::Read || paths.moved[at as usize])
        {
            let (blk, pos) = after(b, topo, i);
            put(blk, pos, tag, at, syn);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, place};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    /// `n` locals, each with a field moved out, put back and the whole
    /// dropped, and one more local only dropped.
    fn body(n: u32) -> Body {
        let mut b = Body::new(DefId::from_raw(1), BodyKind::Fn);
        for _ in 0..=n {
            b.local_ty.push(Ty::VOID);
            b.local_name.push(Symbol::from_raw(0));
            b.local_syn.push(NodeIdx::NONE);
            b.local_flags.push(0);
        }
        let push = |b: &mut Body, tag: Tag, data: [u32; 2]| {
            b.tags.push(tag);
            b.data.push(data);
            b.ty.push(Ty::VOID);
            b.syn.push(NodeIdx::NONE);
        };
        for l in 0..n {
            let at = crate::ir::u32_of(b.extra.len());
            b.extra.extend([
                4,
                place::ROOT_LOCAL,
                l,
                place::PROJ_FIELD | 1,
                place::PROJ_FIELD,
            ]);
            push(&mut b, Tag::Read, [at, ReadMode::Move as u32]);
            push(&mut b, Tag::Store, [at, 0]);
            push(&mut b, Tag::Drop, [place::BARE_LOCAL | l, 0]);
        }
        push(&mut b, Tag::Drop, [place::BARE_LOCAL | n, 0]);
        b
    }

    #[test]
    fn a_moved_field_flags_its_path_and_the_paths_above_it_only() {
        let paths = MovePaths::build(&body(1), |_| true, &HashMap::default());
        // $0, $0 .1, $0 .1 .0, then $1, which nothing moves.
        assert_eq!(paths.key.len(), 4);
        assert_eq!(paths.parent, [NONE, 0, 1, NONE]);
        assert_eq!(paths.moved, [false, false, true, false]);
        assert_eq!(paths.flagged(), [true, true, true, false]);
    }

    #[test]
    fn the_table_grows_linearly_with_the_places_named() {
        let small = MovePaths::build(&body(100), |_| true, &HashMap::default());
        let large = MovePaths::build(&body(1000), |_| true, &HashMap::default());
        assert_eq!(small.key.len(), 3 * 100 + 1);
        assert_eq!(large.key.len(), 3 * 1000 + 1);
        assert_eq!(large.flagged().iter().filter(|&&f| f).count(), 3 * 1000);
    }
}
