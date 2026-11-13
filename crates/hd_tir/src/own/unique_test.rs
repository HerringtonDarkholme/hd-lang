//! The `unique_mut` tests (`module.rt.unique`): which tests need no count
//! load, and which need one only until a test finds the object unique.
//!
//! A test reads the count of the box at its place `p`, the `&mut *T` its
//! `Ref` argument lends. Over the whole body, `p` is classified once:
//!
//! - **Fresh** (`ref_word::UNIQUE`): `p` is in a local that is no
//!   parameter and no closure captures, every store that sets `p` stores a
//!   new box (`box(e)`, a counted `Ref` the checker made, through a
//!   `Weaken`), and nothing copies `p`: its box always has one owner, so
//!   the test is true with no load.
//! - **Sticky** (`ref_word::STICKY`): the same, but `p` may also start
//!   with a box from outside, as behind a `&mut` parameter
//!   (`(*self).store`). Once a test finds it unique it stays so: a store
//!   sets a new box, and nothing makes a second owner. So the backend runs
//!   the test only until one finds the box unique, in a loop once.
//!
//! "Nothing copies `p`": every instruction whose place reaches `p` or a
//! place holding it (resolved through the local references a body
//! stores, as an inlined `self`) is a read through the box, a move, a
//! `Drop`, a test's argument, or a borrow or reference whose every use is
//! a place's root or a store into such a local reference. A parameter's
//! place also needs a body with no other call: a callee may reach the
//! object behind the parameter through another path.
//!
//! The classification is a hoist of the test to the body's entry: a fresh
//! box needs none, and a sticky one runs at most once per body entry while
//! it finds the box shared at most each time the body replaced it.
//!
//! Cost: one scan of the body per tested place to classify it, and one to
//! resolve the local references; bodies with no test do no work beyond
//! the scan for one.

use hd_base::SliceSort as _;

use super::hoist::{entry_test, loop_statements, pure, unconditional_arm, writes};
use super::{OwnCx, operands, splice};
use crate::ir::{
    Body, Callee, DEREF, NONE, ReadMode, Ref, Root, Tag, Topology, is_suspension, local_flags,
    place, ref_word, u32_of,
};

/// A place resolved through local references: its root local and steps
/// (`PlaceView::push_steps` codes), and whether the value is a reference
/// to that place rather than the place's value.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Canon {
    root: u32,
    steps: Vec<u32>,
    refto: bool,
}

/// How deep a place resolves through local references.
const DEPTH: u32 = 6;

/// How a resolved place relates to the tested place.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rel {
    /// No overlap, or a place through the box.
    Apart,
    /// The tested place itself.
    Equal,
    /// A place holding it; whether a reference stands between them.
    Above { through_ref: bool },
}

struct Places<'b> {
    b: &'b Body,
    own: &'b dyn OwnCx,
    /// Per local: its resolved referent, when it is a local reference
    /// every store sets to the same place, once `asked`.
    alias: Vec<Option<Canon>>,
    asked: Vec<bool>,
    params: Vec<bool>,
    captured: Vec<bool>,
}

impl Places<'_> {
    fn canon(&mut self, word: u32, depth: u32) -> Option<Canon> {
        if depth > DEPTH {
            return None;
        }
        let p = self.b.view(word);
        let mut steps = Vec::new();
        p.push_steps(self.b, &mut steps);
        match p.root {
            Root::Local(l) => {
                if let Some(mut t) = self.alias_of(l, depth + 1) {
                    // All of a local reference is a reference to its place.
                    if steps.is_empty() {
                        t.refto = true;
                        return Some(t);
                    }
                    if steps[0] != DEREF {
                        return None;
                    }
                    t.steps.extend_from_slice(&steps[1..]);
                    return Some(t);
                }
                Some(Canon {
                    root: l,
                    steps,
                    refto: false,
                })
            }
            Root::Value(v) => {
                let v = v as usize;
                match self.b.tags[v] {
                    Tag::Ref => {
                        let mut t = self.canon(self.b.data[v][0], depth + 1)?;
                        if steps.is_empty() {
                            t.refto = true;
                        } else if steps[0] == DEREF {
                            t.steps.extend_from_slice(&steps[1..]);
                        } else {
                            return None;
                        }
                        Some(t)
                    }
                    Tag::Read => {
                        let mut t = self.canon(self.b.data[v][0], depth + 1)?;
                        if t.refto {
                            if steps.first() != Some(&DEREF) {
                                return steps.is_empty().then_some(t);
                            }
                            t.refto = false;
                            t.steps.extend_from_slice(&steps[1..]);
                        } else {
                            t.steps.extend_from_slice(&steps);
                        }
                        Some(t)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The place local `l` refers to, when it is a local reference that
    /// is no parameter, no closure captures, nothing references, and every
    /// store sets to the same place.
    fn alias_of(&mut self, l: u32, depth: u32) -> Option<Canon> {
        let li = l as usize;
        if self.asked[li] {
            return self.alias[li].clone();
        }
        // A cycle reads as no alias.
        self.asked[li] = true;
        let found = self.find_alias(l, depth);
        self.alias[li].clone_from(&found);
        found
    }

    fn find_alias(&mut self, l: u32, depth: u32) -> Option<Canon> {
        let b = self.b;
        let li = l as usize;
        if self.params[li] || self.captured[li] || !self.own.plain_reference(b.local_ty[li]) {
            return None;
        }
        let mut target: Option<Canon> = None;
        for i in 0..b.len() {
            if !matches!(b.tags[i], Tag::Store | Tag::Ref) {
                continue;
            }
            let p = b.view(b.data[i][0]);
            if p.local() != Some(l) {
                continue;
            }
            // A reference to the local, or a store into a part of it.
            if b.tags[i] == Tag::Ref || !p.whole() {
                if p.first(b) != Some(crate::ir::Proj::Deref) {
                    return None;
                }
                continue;
            }
            let vi = Ref(b.data[i][1]).as_inst()?.0 as usize;
            let t = match b.tags[vi] {
                Tag::Ref => self.canon(b.data[vi][0], depth + 1)?,
                Tag::Read => {
                    let mut t = self.canon(b.data[vi][0], depth + 1)?;
                    if t.refto {
                        t.refto = false;
                    } else {
                        t.steps.push(DEREF);
                    }
                    t
                }
                _ => return None,
            };
            if target.as_ref().is_some_and(|x| *x != t) {
                return None;
            }
            target = Some(t);
        }
        target.map(|mut t| {
            t.refto = false;
            t
        })
    }
}

/// How `c` relates to the tested place `(root, steps)`.
fn relate(c: &Canon, root: u32, steps: &[u32]) -> Rel {
    if c.root != root {
        return Rel::Apart;
    }
    let k = c.steps.len().min(steps.len());
    if c.steps[..k] != steps[..k] {
        return Rel::Apart;
    }
    if c.steps.len() > steps.len() {
        return Rel::Apart;
    }
    if c.refto {
        // A reference to the place, or to one holding it.
        return Rel::Above { through_ref: true };
    }
    if c.steps.len() == steps.len() {
        return Rel::Equal;
    }
    Rel::Above {
        through_ref: steps[k..].contains(&DEREF),
    }
}

/// Whether value `v` is a new box: a counted `Ref` (no `LOCAL`), through
/// `Weaken`s.
fn fresh_box(b: &Body, mut v: u32) -> bool {
    for _ in 0..DEPTH {
        let Some(i) = Ref(v).as_inst() else {
            return false;
        };
        let i = i.0 as usize;
        match b.tags[i] {
            Tag::Ref => return b.data[i][1] & ref_word::LOCAL == 0,
            Tag::Coerce if b.same_value_coercion(i) => v = b.data[i][0],
            _ => return false,
        }
    }
    false
}

/// Whether the part of stored value `v` at field steps `rest` is a new
/// box: a struct literal's field, followed to the box.
fn fresh_part(b: &Body, mut v: u32, rest: &[u32]) -> bool {
    for &s in rest {
        let Some(i) = Ref(v).as_inst() else {
            return false;
        };
        let i = i.0 as usize;
        if b.tags[i] != Tag::NewStruct || s == DEREF {
            return false;
        }
        let Some(&f) = b.record(b.data[i][1]).get(s as usize) else {
            return false;
        };
        v = f;
    }
    fresh_box(b, v)
}

/// Marks each `unique_mut` test's argument `UNIQUE` or `STICKY`.
pub(crate) fn run(b: &mut Body, topo: &Topology, cx: &dyn OwnCx, edits: &mut splice::Tables) {
    let n = b.len();
    // (call, argument Ref) of each test.
    let mut tests: Vec<(usize, usize)> = Vec::new();
    for i in 0..n {
        if b.tags[i] != Tag::Call {
            continue;
        }
        let Some(c) = Callee::from_words(b.record(b.data[i][0])) else {
            continue;
        };
        if !cx.is_unique_test(&c) && !cx.is_make_unique(&c) {
            continue;
        }
        if let Some(a) = b.record(b.data[i][1]).first()
            && let Some(a) = Ref(*a).as_inst()
            && b.tags[a.0 as usize] == Tag::Ref
        {
            tests.push((i, a.0 as usize));
        }
    }
    if tests.is_empty() {
        return;
    }
    let locals = b.local_ty.len();
    let mut params = vec![false; locals];
    for (l, f) in b.local_flags.iter().enumerate() {
        params[l] = f & local_flags::PARAM != 0;
    }
    let mut captured = vec![false; locals];
    for sub in 1..b.sub_params.len() {
        for &x in b.sub_param_locals(sub) {
            if let Some(c) = captured.get_mut(x as usize) {
                *c = true;
            }
        }
    }
    for &c in &b.cap_local {
        if let Some(o) = captured.get_mut(c.idx()) {
            *o = true;
        }
    }
    let mut s = Places {
        b,
        own: cx,
        alias: vec![None; locals],
        asked: vec![false; locals],
        params,
        captured,
    };
    let mut marks: Vec<(usize, u32)> = Vec::new();
    let mut done: Vec<Canon> = Vec::new();
    let mut ops = Vec::new();
    let mut watched = vec![false; n];
    let hoists = loop_hoists(b, topo, cx, &tests, &mut s, &mut watched, &mut ops);
    for &(_, arg) in &tests {
        let Some(t) = s.canon(b.data[arg][0], 0) else {
            continue;
        };
        if t.refto || done.contains(&t) {
            continue;
        }
        done.push(t.clone());
        let x = t.root as usize;
        let param = s.params[x];
        // A local's own fields, or the fields behind a reference parameter.
        let shape_ok = if param {
            t.steps.first() == Some(&DEREF)
                && t.steps[1..]
                    .iter()
                    .all(|&k| k != DEREF && k < crate::ir::VARIANT)
                && cx.plain_reference(b.local_ty[x])
        } else {
            t.steps.iter().all(|&k| k < crate::ir::VARIANT)
        };
        if !shape_ok || s.captured[x] {
            continue;
        }
        let mut ok = true;
        let fresh = !param;
        fill_false(&mut watched);
        // The tests of this place.
        let own: Vec<usize> = tests
            .iter()
            .filter(|&&(_, a)| s.canon(b.data[a][0], 0).as_ref() == Some(&t))
            .map(|&(_, a)| a)
            .collect();
        for (j, w) in watched.iter_mut().enumerate() {
            if !ok {
                break;
            }
            let tag = b.tags[j];
            match tag {
                Tag::Call | Tag::CallValue | Tag::Await => {
                    let test = tests.iter().any(|&(c, _)| c == j);
                    if param && !test {
                        ok = false;
                    }
                    continue;
                }
                Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::IsUnique => {}
                _ => continue,
            }
            let p = b.view(b.data[j][0]);
            // A store of all of a local reference sets what it refers to.
            if tag == Tag::Store
                && p.whole()
                && let Some(l) = p.local()
                && s.alias_of(l, 0).is_some()
            {
                continue;
            }
            let Some(c) = s.canon(b.data[j][0], 0) else {
                continue;
            };
            let rel = relate(&c, t.root, &t.steps);
            if rel == Rel::Apart {
                continue;
            }
            match tag {
                Tag::Read => match rel {
                    Rel::Above { through_ref: true } => *w = true,
                    _ if b.data[j][1] == ReadMode::Copy as u32 => ok = false,
                    _ if b.data[j][1] == ReadMode::Move as u32 => {}
                    _ => *w = true,
                },
                Tag::Store => match rel {
                    Rel::Equal => {
                        if !fresh_box(b, b.data[j][1]) {
                            ok = false;
                        }
                    }
                    Rel::Above { through_ref: false } => {
                        if !fresh_part(b, b.data[j][1], &t.steps[c.steps.len()..]) {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                },
                Tag::Ref => {
                    if !(rel == Rel::Equal && own.contains(&j)) {
                        *w = true;
                    }
                }
                Tag::Drop => {}
                _ => ok = false,
            }
        }
        if !ok || !watched_uses_ok(&mut s, &watched, &mut ops) {
            continue;
        }
        let word = if fresh {
            ref_word::UNIQUE
        } else {
            ref_word::STICKY
        };
        for a in own {
            marks.push((a, word));
        }
    }
    for (a, word) in marks {
        b.data[a][1] |= word;
    }
    for h in hoists {
        hoist_before(b, topo, edits, &h);
    }
}

/// Whether every use of a watched value is a place's root, a store into a
/// local reference, an operator or a `Drop`: no copy of the tested place
/// leaves through a reference to it or to a place holding it.
fn watched_uses_ok(s: &mut Places<'_>, watched: &[bool], ops: &mut Vec<super::Operand>) -> bool {
    let b = s.b;
    let n = b.len();
    for k in 0..n {
        operands(b, k, ops);
        for op in ops.iter() {
            let v = op.value as usize;
            if v >= n || !watched[v] {
                continue;
            }
            let root = matches!(
                b.tags[k],
                Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::LoanEnd | Tag::IsUnique
            ) && b.view(b.data[k][0]).value() == Some(op.value);
            let alias_store = b.tags[k] == Tag::Store
                && b.data[k][1] == op.value
                && b.view(b.data[k][0]).whole()
                && b.view(b.data[k][0])
                    .local()
                    .is_some_and(|l| s.alias_of(l, 0).is_some());
            if !(root || alias_store || matches!(b.tags[k], Tag::Prim | Tag::Drop)) {
                return false;
            }
        }
    }
    true
}

/// A `make_unique` hoisted out of a loop: the loop, the place it makes
/// unique, its first call there and the tests of that place in the loop.
struct Hoist {
    lp: u32,
    place: Canon,
    call: usize,
    copy: u32,
    tests: Vec<usize>,
}

/// The statements that run on every entered iteration of `Loop` `l`
/// (`docs/compiler/tir-v2.md`, "Hoisting", rule 1): those of its
/// iteration block behind a pure entry-test prefix, at any depth through
/// scopes and unconditional match arms, up to the first statement that
/// [`leaves`] marks in each block: a `break`, `continue` or `return` there
/// may end the iteration before the statements after it. An inner loop's
/// statements are left out: it may run no iteration.
fn every_iteration(
    b: &Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    leaves: &[bool],
    l: u32,
    out: &mut Vec<u32>,
) {
    out.clear();
    let mut blocks = match entry_test(b, topo, l) {
        Some(e) => {
            let mut budget = b.len();
            if loop_statements(b, l)
                .take(e.prefix)
                .all(|x| pure(b, cx, x, &mut budget))
            {
                vec![e.iteration]
            } else {
                Vec::new()
            }
        }
        None => vec![b.data[l as usize][0]],
    };
    while let Some(blk) = blocks.pop() {
        for &x in b.record(b.data[blk as usize][0]) {
            match b.tags[x as usize] {
                // A block listed beside the statement that runs it, as a
                // loop's body: that statement stands for it.
                Tag::Block if topo.owner[x as usize] != NONE => continue,
                // An inner loop may run no iteration; its own hoist puts the
                // call before it.
                Tag::Loop => {}
                Tag::Scope => blocks.push(b.data[x as usize][0]),
                Tag::Match => {
                    if let Some(arm) = unconditional_arm(b, x as usize) {
                        blocks.push(arm);
                    }
                }
                _ => out.push(x),
            }
            if leaves[x as usize] {
                break;
            }
        }
    }
}

/// Per instruction, whether a jump inside it may go past its end: a
/// `break` or `continue` to a label outside it, or a `return`, which
/// leaves its sub-body. A jump marks each instruction from itself up to,
/// not including, the one that holds its target, or up to its sub-body's
/// root for a `return`. A `break` to a loop marks the statements around it
/// in the loop, never the loop: after the loop, the block goes on.
///
/// Cost: per jump, one step per instruction around it, up to its target.
fn leaves(b: &Body, topo: &Topology) -> Vec<bool> {
    let mut out = vec![false; b.len()];
    for i in 0..b.len() {
        let target = match b.tags[i] {
            Tag::Break | Tag::Continue => b
                .label_inst
                .get(b.data[i][0] as usize)
                .copied()
                .unwrap_or(NONE),
            Tag::Return => NONE,
            _ => continue,
        };
        let mut x = i;
        // The instruction that holds the target stops the walk.
        while target == NONE || !(topo.first[x] <= target && target <= u32_of(x)) {
            out[x] = true;
            // A block's parent is the instruction that runs it.
            let up = if b.tags[x] == Tag::Block && topo.owner[x] != NONE {
                topo.owner[x]
            } else {
                topo.blk[x]
            };
            if topo.is_root[x] || up == NONE {
                break;
            }
            x = up as usize;
        }
    }
    out
}

/// The `make_unique` calls a loop may run once before it (owner,
/// 2026-11-12, #1219): one that runs on every entered iteration, of a
/// place `p` whose root the loop never stores, in a loop that copies no
/// place holding `p`, stores only new boxes into `p`, lends `p` or a place
/// holding it only as a place's root, suspends nowhere and, for a place
/// behind a parameter, calls nothing that may write. Its function argument
/// is an item, which the copy before the loop names again. Once the call
/// before the loop leaves `p`'s box unique, nothing in the loop can share
/// it, so every test of `p` there is true: the passes mark them `UNIQUE`.
/// The backend runs the call under the loop's entry test, so an empty loop
/// copies nothing; it can't repeat the test in a body that suspends, so
/// there only a loop with no test, which always runs an iteration, hoists.
///
/// Cost: per loop with a `make_unique` on every iteration, one scan of the
/// loop per tested place, and one of the body for the uses of the
/// references the loop takes.
fn loop_hoists(
    b: &Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    tests: &[(usize, usize)],
    s: &mut Places<'_>,
    watched: &mut [bool],
    ops: &mut Vec<super::Operand>,
) -> Vec<Hoist> {
    let mut out: Vec<Hoist> = Vec::new();
    let makes = |i: usize| {
        Callee::from_words(b.record(b.data[i][0])).is_some_and(|c| cx.is_make_unique(&c))
    };
    if !tests.iter().any(|&(c, _)| makes(c)) {
        return out;
    }
    let mut loops: Vec<(u32, u32)> = (0..b.len())
        .filter(|&i| b.tags[i] == Tag::Loop)
        .map(|i| (topo.first[i], u32_of(i)))
        .collect();
    // Outermost first: an outer loop starts no later and ends later.
    loops.sort_stable_by_key(|&(f, l)| (f, std::cmp::Reverse(l)));
    let mut every = Vec::new();
    let leaves = leaves(b, topo);
    let mut hoisted = vec![false; b.len()];
    // The backend runs a hoisted call under its loop's entry test, which a
    // suspending body's emission can't repeat ([`hoist_before`]).
    let suspends = b.tags.iter().any(|&tag| is_suspension(tag));
    for (lo, l) in loops {
        if suspends && entry_test(b, topo, l).is_some() {
            continue;
        }
        every_iteration(b, topo, cx, &leaves, l, &mut every);
        for &x in &every {
            let x = x as usize;
            let Some(&(_, arg)) = tests.iter().find(|&&(c, _)| c == x) else {
                continue;
            };
            if hoisted[x] || !makes(x) {
                continue;
            }
            let Some(t) = s.canon(b.data[arg][0], 0) else {
                continue;
            };
            let copy = b.record(b.data[x][1]).get(1).copied().unwrap_or(NONE);
            let item = Ref(copy)
                .as_inst()
                .is_some_and(|c| b.tags[c.0 as usize] == Tag::ItemRef);
            if t.refto || !item {
                continue;
            }
            let Some(own) = leaves_alone(b, cx, tests, s, watched, ops, &t, lo, l) else {
                continue;
            };
            for &a in &own {
                hoisted[a] = true;
            }
            out.push(Hoist {
                lp: l,
                place: t,
                call: x,
                copy,
                tests: own,
            });
        }
    }
    out
}

/// Whether loop `lo..=l` leaves place `t` alone ([`loop_hoists`]): the
/// tests of `t` in the loop, each a call.
#[allow(clippy::too_many_arguments)]
fn leaves_alone(
    b: &Body,
    cx: &dyn OwnCx,
    tests: &[(usize, usize)],
    s: &mut Places<'_>,
    watched: &mut [bool],
    ops: &mut Vec<super::Operand>,
    t: &Canon,
    lo: u32,
    l: u32,
) -> Option<Vec<usize>> {
    let x = t.root as usize;
    let param = s.params[x];
    let shape_ok = if param {
        t.steps.first() == Some(&DEREF)
            && t.steps[1..]
                .iter()
                .all(|&k| k != DEREF && k < crate::ir::VARIANT)
            && cx.plain_reference(b.local_ty[x])
    } else {
        t.steps.iter().all(|&k| k < crate::ir::VARIANT)
    };
    if !shape_ok || s.captured[x] {
        return None;
    }
    let mut own: Vec<usize> = Vec::new();
    let mut args: Vec<usize> = Vec::new();
    for &(c, a) in tests {
        if (lo as usize..l as usize).contains(&c) && s.canon(b.data[a][0], 0).as_ref() == Some(t) {
            own.push(c);
            args.push(a);
        }
    }
    for w in watched.iter_mut() {
        *w = false;
    }
    for (j, w) in watched
        .iter_mut()
        .enumerate()
        .take(l as usize)
        .skip(lo as usize)
    {
        let tag = b.tags[j];
        if is_suspension(tag) {
            return None;
        }
        match tag {
            Tag::Call | Tag::CallValue | Tag::CallHost | Tag::DefaultCall => {
                // A callee may reach what a parameter refers to through
                // another path.
                if param && !own.contains(&j) && writes(b, cx, j) {
                    return None;
                }
                continue;
            }
            Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::IsUnique => {}
            _ => continue,
        }
        let p = b.view(b.data[j][0]);
        if tag == Tag::Store
            && p.whole()
            && let Some(lc) = p.local()
            && s.alias_of(lc, 0).is_some()
        {
            continue;
        }
        let Some(c) = s.canon(b.data[j][0], 0) else {
            continue;
        };
        match (tag, relate(&c, t.root, &t.steps)) {
            (_, Rel::Apart) => {}
            (Tag::Read, Rel::Above { through_ref: true }) => *w = true,
            (Tag::Read, _) if b.data[j][1] == ReadMode::Copy as u32 => return None,
            (Tag::Read, _) if b.data[j][1] == ReadMode::Move as u32 => {}
            (Tag::Read, _) => *w = true,
            // Only a new box replaces `p`'s; the loop never stores its root,
            // which the copy before the loop reads.
            (Tag::Store, Rel::Equal) if fresh_box(b, b.data[j][1]) => {}
            (Tag::Ref, Rel::Equal) if args.contains(&j) => {}
            (Tag::Ref, _) => *w = true,
            (Tag::Drop, _) => {}
            _ => return None,
        }
    }
    watched_uses_ok(s, watched, ops).then_some(own)
}

/// Puts hoist `h`'s `make_unique` before its loop, and marks its tests
/// there `UNIQUE`: `Ref mut local hoisted [p]`, the copy's item again, and
/// the call over them.
fn hoist_before(b: &mut Body, topo: &Topology, edits: &mut splice::Tables, h: &Hoist) {
    let n = u32_of(b.len());
    let call = h.call;
    let arg = b.record(b.data[call][1])[0] as usize;
    for &c in &h.tests {
        if let Some(&a) = b.record(b.data[c][1]).first() {
            b.data[a as usize][1] |= ref_word::UNIQUE;
        }
    }
    let word = place_word(b, &h.place);
    let (blk, pos) = (topo.blk[h.lp as usize], topo.pos[h.lp as usize]);
    let r = edits.insert(
        blk,
        pos,
        Tag::Ref,
        [word, ref_word::MUT | ref_word::LOCAL | ref_word::HOISTED],
        b.ty[arg],
        b.syn[arg],
    );
    let ci = h.copy as usize;
    let copy = edits.insert(blk, pos, Tag::ItemRef, b.data[ci], b.ty[ci], b.syn[ci]);
    let rec: Vec<u32> = b.record(b.data[call][1]).to_vec();
    let at = u32_of(b.extra.len());
    b.extra.push(u32_of(rec.len()));
    for (k, &w) in rec.iter().enumerate() {
        b.extra.push(match k {
            0 => n + r,
            1 => n + copy,
            _ => w,
        });
    }
    let made = edits.insert(
        blk,
        pos,
        Tag::Call,
        [b.data[call][0], at],
        b.ty[call],
        b.syn[call],
    );
    // Its result is unused, and the loan ends with the call.
    let result = u32_of(b.extra.len());
    b.extra.extend([2, place::ROOT_VALUE, n + made]);
    edits.insert(
        blk,
        pos,
        Tag::Drop,
        [result, super::reason::LAST_USE],
        hd_types::Ty::VOID,
        b.syn[call],
    );
    let lent = u32_of(b.extra.len());
    b.extra.extend([2, place::ROOT_VALUE, n + r]);
    edits.insert(
        blk,
        pos,
        Tag::LoanEnd,
        [lent, NONE],
        hd_types::Ty::VOID,
        b.syn[arg],
    );
    // The item's value drops as the loop's does: after the call, unless
    // the call takes it.
    let drops = (call..h.lp as usize)
        .any(|j| b.tags[j] == Tag::Drop && b.view(b.data[j][0]).value() == Some(h.copy));
    if drops {
        let dropped = u32_of(b.extra.len());
        b.extra.extend([2, place::ROOT_VALUE, n + copy]);
        edits.insert(
            blk,
            pos,
            Tag::Drop,
            [dropped, super::reason::LAST_USE],
            hd_types::Ty::VOID,
            b.syn[ci],
        );
    }
}

/// A place record for `c`: its root local, then its steps.
fn place_word(b: &mut Body, c: &Canon) -> u32 {
    if c.steps.is_empty() {
        return c.root | place::BARE_LOCAL;
    }
    let at = u32_of(b.extra.len());
    b.extra.push(2 + u32_of(c.steps.len()));
    b.extra.extend([place::ROOT_LOCAL, c.root]);
    for &s in &c.steps {
        b.extra.push(if s == DEREF {
            place::PROJ_DEREF
        } else {
            place::PROJ_FIELD | s
        });
    }
    at
}

fn fill_false(v: &mut [bool]) {
    for x in v {
        *x = false;
    }
}
