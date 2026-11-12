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
//! A weak reference's upgrade also makes a second owner, from a
//! reference to the object rather than a copy of `p`; the backend ignores
//! both marks in a module that links weak references.
//!
//! The classification is a hoist of the test to the body's entry: a fresh
//! box needs none, and a sticky one runs at most once per body entry while
//! it finds the box shared at most each time the body replaced it.
//!
//! Cost: one scan of the body per tested place to classify it, and one to
//! resolve the local references; bodies with no test do no work beyond
//! the scan for one.

use super::{OwnCx, operands};
use crate::ir::{Body, Callee, DEREF, ReadMode, Ref, Root, Tag, local_flags, ref_word};

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
pub(crate) fn run(b: &mut Body, cx: &dyn OwnCx) {
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
        if !cx.is_unique_test(&c) {
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
        // Every use of a watched value is a place's root, a store into a
        // local reference, an operator or a `Drop`.
        for k in 0..n {
            if !ok {
                break;
            }
            operands(b, k, &mut ops);
            for op in &ops {
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
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
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
}

fn fill_false(v: &mut [bool]) {
    for x in v {
        *x = false;
    }
}
