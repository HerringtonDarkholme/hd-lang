//! Move checking (spec/lang/13-ownership.md, Moves): the ownership pass's
//! first half over a finished body's TIR. One forward pass over the
//! structured control flow keeps, per owned local, whether it may hold
//! its value and whether it may have moved; loops iterate to a fixed
//! point. It reports `use-after-move` and `partial-move`, and writes the
//! `moves` and `drop_mode` columns.

use std::collections::BTreeMap;

use hd_base::{Fuel, HashMap, NodeIdx, StageResult};
use hd_diag::Code;
use hd_tir::ir::{Body, Coercion, DROP_ENTRY, DropMode, NONE, Ref, Tag};
use hd_types::{Ty, TyData};

use crate::body::Ck;

/// What a use of an owned local is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Use {
    /// The value goes on: it moves.
    Move,
    /// Lent to a `^` position, or read through a field: the local keeps it.
    Read,
}

/// One owned local's state on a path: whether it may hold its value,
/// whether it may have moved, and where it moved (the earliest move).
#[derive(Clone, PartialEq, Eq)]
struct State {
    holds: Vec<bool>,
    moved: Vec<bool>,
    site: Vec<u32>,
}

impl State {
    fn new(n: usize) -> Self {
        State {
            holds: vec![false; n],
            moved: vec![false; n],
            site: vec![NONE; n],
        }
    }

    fn join(&mut self, o: &State) {
        for k in 0..self.holds.len() {
            self.holds[k] |= o.holds[k];
            self.moved[k] |= o.moved[k];
            if o.site[k] < self.site[k] {
                self.site[k] = o.site[k];
            }
        }
    }

    fn give(&mut self, k: usize) {
        self.holds[k] = true;
        self.moved[k] = false;
        self.site[k] = NONE;
    }

    fn take(&mut self, k: usize, at: u32) {
        self.holds[k] = false;
        self.moved[k] = true;
        self.site[k] = self.site[k].min(at);
    }

    fn reset(&mut self, k: usize) {
        self.holds[k] = false;
        self.moved[k] = false;
        self.site[k] = NONE;
    }
}

fn join_into(acc: &mut Option<State>, s: &State) {
    match acc {
        Some(a) => a.join(s),
        None => *acc = Some(s.clone()),
    }
}

/// A use of a local after it may have moved: the use and the move.
pub(crate) struct AfterMove {
    pub at: u32,
    pub local: u32,
    pub site: u32,
}

/// An open frame on the walk: a scope whose drops an exit runs, or a
/// target a `Break` or `Continue` names.
#[derive(Clone, Copy)]
enum Frame {
    Scope(u32),
    Target(u32),
}

struct Flow<'a> {
    b: &'a Body,
    fuel: &'a mut Fuel,
    /// By local: its index among the owned locals, or `NONE`.
    slot: Vec<u32>,
    /// By instruction: whether its value is lent or read through, which
    /// keeps the place; any other use takes it.
    read: Vec<bool>,
    /// By `Scope` instruction: the owned locals it drops.
    scope_locals: HashMap<u32, Vec<usize>>,
    frames: Vec<Frame>,
    breaks: HashMap<u32, State>,
    continues: HashMap<u32, State>,
    /// The open `match`es: the state each arm is entered with.
    arms: Vec<Vec<Option<State>>>,
    /// By owned local: whether some drop point may find it holding its
    /// value, and whether one may find it moved.
    may_hold: Vec<bool>,
    may_lack: Vec<bool>,
    errors: BTreeMap<u32, AfterMove>,
    out_of_fuel: bool,
}

impl Flow<'_> {
    fn charge(&mut self) -> bool {
        if !self.fuel.charge(1) {
            self.out_of_fuel = true;
        }
        !self.out_of_fuel
    }

    fn slot_of(&self, l: u32) -> Option<usize> {
        self.slot
            .get(l as usize)
            .copied()
            .filter(|s| *s != NONE)
            .map(|s| s as usize)
    }

    /// A point where local `k` drops if it holds its value: a scope exit
    /// or an assignment.
    fn drop_point(&mut self, s: &State, k: usize) {
        if s.holds[k] || s.moved[k] {
            self.may_hold[k] |= s.holds[k];
            self.may_lack[k] |= s.moved[k];
        }
    }

    /// Leaves the frames above index `keep`: each scope's locals reach a
    /// drop point and are gone after it.
    fn leave(&mut self, s: &mut State, keep: usize) {
        let frames: Vec<Frame> = self.frames[keep..].to_vec();
        for f in frames.iter().rev() {
            if let Frame::Scope(sc) = f {
                let ls = self.scope_locals.get(sc).cloned().unwrap_or_default();
                for k in ls {
                    self.drop_point(s, k);
                    s.reset(k);
                }
            }
        }
    }

    fn use_local(&mut self, s: &mut State, at: u32, k: usize, l: u32, how: Use) {
        if s.moved[k] {
            self.errors.entry(at).or_insert(AfterMove {
                at,
                local: l,
                site: s.site[k],
            });
        }
        if how == Use::Move {
            s.take(k, at);
        }
    }

    /// The owned locals a `defer` suite reads, at any depth.
    fn suite_reads(&self, blk: u32, out: &mut Vec<u32>) {
        if self.b.tags.get(blk as usize) != Some(&Tag::Block) {
            return;
        }
        for &i in self.b.record(self.b.data[blk as usize][0]) {
            let i_us = i as usize;
            match self.b.tags[i_us] {
                Tag::Block => {
                    // A closure's root is its own body.
                    if !self.b.sub_root.contains(&i) {
                        self.suite_reads(i, out);
                    }
                }
                Tag::LocalGet => {
                    let l = self.b.data[i_us][0];
                    if self.slot_of(l).is_some() {
                        out.push(l);
                    }
                }
                _ => {}
            }
        }
    }

    fn block(&mut self, blk: u32, s: State) -> Option<State> {
        if blk == NONE || self.b.tags.get(blk as usize) != Some(&Tag::Block) {
            return Some(s);
        }
        self.frames.push(Frame::Target(blk));
        let mut cur = Some(s);
        let items: Vec<u32> = self.b.record(self.b.data[blk as usize][0]).to_vec();
        for i in items {
            if self.b.tags[i as usize] == Tag::Block {
                continue;
            }
            let Some(s) = cur.take() else {
                break;
            };
            cur = self.inst(i, s);
            if self.out_of_fuel {
                break;
            }
        }
        self.frames.pop();
        if let Some(b) = self.breaks.remove(&blk) {
            match &mut cur {
                Some(c) => c.join(&b),
                None => cur = Some(b),
            }
        }
        cur
    }

    fn target_frame(&self, target: u32) -> Option<usize> {
        self.frames
            .iter()
            .rposition(|f| matches!(f, Frame::Target(t) if *t == target))
    }

    fn inst(&mut self, i: u32, mut s: State) -> Option<State> {
        if !self.charge() {
            return None;
        }
        let iu = i as usize;
        let [a, bw] = self.b.data[iu];
        let tag = self.b.tags[iu];
        let out = match tag {
            Tag::LocalGet => {
                if let Some(k) = self.slot_of(a) {
                    let how = if self.read[iu] { Use::Read } else { Use::Move };
                    self.use_local(&mut s, i, k, a, how);
                }
                Some(s)
            }
            Tag::LocalSet => {
                if let Some(k) = self.slot_of(a) {
                    self.drop_point(&s, k);
                    s.give(k);
                }
                Some(s)
            }
            Tag::DropLocal => {
                if let Some(k) = self.slot_of(a) {
                    s.give(k);
                }
                Some(s)
            }
            Tag::Defer => {
                // The suite runs at the scope's exit, but it takes the owned
                // values it reads here, when the `defer` registers.
                let mut reads = Vec::new();
                self.suite_reads(a, &mut reads);
                let frames = std::mem::take(&mut self.frames);
                let _ = self.block(a, s.clone());
                self.frames = frames;
                for l in reads {
                    if let Some(k) = self.slot_of(l) {
                        s.take(k, i);
                    }
                }
                Some(s)
            }
            Tag::Scope => {
                self.frames.push(Frame::Scope(i));
                let out = self.block(a, s);
                let out = out.map(|mut o| {
                    let ls = self.scope_locals.get(&i).cloned().unwrap_or_default();
                    for k in ls {
                        self.drop_point(&o, k);
                        o.reset(k);
                    }
                    o
                });
                self.frames.pop();
                out
            }
            Tag::If => {
                let r = self.b.record(bw).to_vec();
                let mut acc = None;
                if let Some(t) = self.block(r[0], s.clone()) {
                    join_into(&mut acc, &t);
                }
                match r.get(1).copied().filter(|e| *e != NONE) {
                    Some(e) => {
                        if let Some(t) = self.block(e, s) {
                            join_into(&mut acc, &t);
                        }
                    }
                    None => join_into(&mut acc, &s),
                }
                acc
            }
            Tag::And | Tag::Or => {
                let mut acc = Some(s.clone());
                if let Some(t) = self.block(bw, s) {
                    join_into(&mut acc, &t);
                }
                acc
            }
            Tag::With => self.block(bw, s),
            Tag::Loop | Tag::ForRange | Tag::ForList | Tag::ForMap => self.looping(i, s),
            Tag::Break => {
                let target = self.b.label_inst.get(a as usize).copied().unwrap_or(NONE);
                if let Some(at) = self.target_frame(target) {
                    self.leave(&mut s, at + 1);
                    let acc = self.breaks.entry(target).or_insert_with(|| s.clone());
                    acc.join(&s);
                }
                None
            }
            Tag::Continue => {
                let target = self.b.label_inst.get(a as usize).copied().unwrap_or(NONE);
                if let Some(at) = self.target_frame(target) {
                    self.leave(&mut s, at + 1);
                    let acc = self.continues.entry(target).or_insert_with(|| s.clone());
                    acc.join(&s);
                }
                None
            }
            Tag::Return => {
                self.leave(&mut s, 0);
                None
            }
            Tag::Match => {
                let r = self.b.record(bw).to_vec();
                let n = r.len().saturating_sub(1);
                self.arms.push(vec![None; n]);
                let _ = self.block(r[0], s);
                let ins = self.arms.pop().unwrap_or_default();
                let mut acc = None;
                for (k, arm_in) in ins.into_iter().enumerate() {
                    if let Some(st) = arm_in
                        && let Some(t) = self.block(r[k + 1], st)
                    {
                        join_into(&mut acc, &t);
                    }
                }
                acc
            }
            Tag::ToArm => {
                if let Some(ins) = self.arms.last_mut()
                    && let Some(slot) = ins.get_mut(a as usize)
                {
                    join_into(slot, &s);
                }
                None
            }
            Tag::Guard => {
                let r = self.b.record(bw).to_vec();
                let c = self.block(a, s)?;
                let mut acc = None;
                for blk in r {
                    if let Some(t) = self.block(blk, c.clone()) {
                        join_into(&mut acc, &t);
                    }
                }
                acc
            }
            Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
                let r = self.b.record(bw).to_vec();
                let (cases, default) = r.split_at(r.len().saturating_sub(1));
                let mut blocks: Vec<u32> = cases.iter().skip(1).step_by(2).copied().collect();
                blocks.extend(default.iter().copied().filter(|d| *d != NONE));
                let mut acc = None;
                for blk in blocks {
                    if let Some(t) = self.block(blk, s.clone()) {
                        join_into(&mut acc, &t);
                    }
                }
                acc
            }
            _ => Some(s),
        };
        // A value of type `never` does not continue.
        if self.b.ty[iu] == Ty::NEVER {
            return None;
        }
        out
    }

    /// A loop: its body runs from the join of the entry and every way back
    /// to its head, until that join stops growing.
    fn looping(&mut self, i: u32, s: State) -> Option<State> {
        let tag = self.b.tags[i as usize];
        let [a, bw] = self.b.data[i as usize];
        let (body, els) = if tag == Tag::Loop {
            (a, bw)
        } else {
            let r = self.b.record(bw);
            (
                r.first().copied().unwrap_or(NONE),
                r.get(1).copied().unwrap_or(NONE),
            )
        };
        let mut head = s;
        loop {
            self.frames.push(Frame::Target(i));
            let end = self.block(body, head.clone());
            self.frames.pop();
            let mut next = head.clone();
            if let Some(e) = &end {
                next.join(e);
            }
            if let Some(c) = self.continues.remove(&i) {
                next.join(&c);
            }
            if next == head || self.out_of_fuel {
                break;
            }
            head = next;
        }
        let mut out = self.breaks.remove(&i);
        // A `for` loop also ends when its items run out, and its `else`
        // block runs then.
        if tag != Tag::Loop || els != NONE {
            let done = self.block(els, head);
            if let Some(d) = done {
                join_into(&mut out, &d);
            }
        }
        out
    }
}

impl Ck<'_, '_> {
    /// Move checking over the finished body (`own.move.*`): reports each
    /// use of an owned local that may have moved, and each field moved
    /// out of an owned value alone, then writes the `moves` and
    /// `drop_mode` columns.
    pub(crate) fn check_moves(&mut self, root: Ref) -> StageResult<()> {
        let pool = self.pool();
        let body = self.b.body_mut();
        let n = body.len();
        // How each value is used: lent, or read through a field, keeps the
        // place; anything else takes it.
        let mut read = vec![false; n];
        let mut partial = Vec::new();
        for i in 0..n {
            let [a, bw] = body.data[i];
            match body.tags[i] {
                Tag::Coerce => {
                    let kind = body.record(bw).first().copied();
                    let lends = [Coercion::Lend as u32, Coercion::LendMut as u32];
                    if kind.is_some_and(|k| lends.contains(&k))
                        && let Some(x) = read.get_mut(a as usize)
                    {
                        *x = true;
                    }
                }
                Tag::Field | Tag::TupleGet | Tag::FieldSet => {
                    if a & Ref::CONST_BIT == 0
                        && let Some(x) = read.get_mut(a as usize)
                    {
                        *x = true;
                    }
                }
                _ => {}
            }
        }
        // `own.move.partial`: a field of an owned value that owns, used as
        // a value.
        for (i, &kept) in read.iter().enumerate() {
            if kept || !matches!(body.tags[i], Tag::Field | Tag::TupleGet) {
                continue;
            }
            let base = body.data[i][0];
            if base & Ref::CONST_BIT != 0 {
                continue;
            }
            let bt = pool.get(body.ty[base as usize]);
            let bt = match bt {
                TyData::Mut(t) => pool.get(t),
                t => t,
            };
            if matches!(bt, TyData::Own(_)) && crate::own::holds_owned(pool, body.ty[i]) {
                partial.push(i);
            }
        }
        // The owned locals: those a `DropLocal` registers.
        let mut slot = vec![NONE; body.local_ty.len()];
        let mut drops = Vec::new();
        let mut count = 0u32;
        for i in 0..n {
            if body.tags[i] == Tag::DropLocal {
                let l = body.data[i][0] as usize;
                if let Some(s) = slot.get_mut(l)
                    && *s == NONE
                {
                    *s = count;
                    count += 1;
                }
                drops.push(l);
            }
        }
        let mut scope_locals: HashMap<u32, Vec<usize>> = HashMap::default();
        for i in 0..n {
            if body.tags[i] != Tag::Scope {
                continue;
            }
            for &w in body.record(body.data[i][1]) {
                if w & DROP_ENTRY != 0 {
                    let d = (w & !DROP_ENTRY) as usize;
                    if let Some(&k) = body.data.get(d).and_then(|x| slot.get(x[0] as usize))
                        && k != NONE
                    {
                        scope_locals
                            .entry(u32::try_from(i).expect("insts"))
                            .or_default()
                            .push(k as usize);
                    }
                }
            }
        }
        let moves: Vec<u32> = (0..n)
            .filter(|&i| {
                body.tags[i] == Tag::LocalGet
                    && slot
                        .get(body.data[i][0] as usize)
                        .is_some_and(|s| *s != NONE)
                    && !read[i]
            })
            .map(|i| u32::try_from(i).expect("insts"))
            .collect();
        let k = count as usize;
        let mut roots: Vec<u32> = body.sub_root.clone();
        // The body's own root is written at `finish`.
        if let Some(r0) = roots.first_mut() {
            *r0 = root.0;
        }
        let mut flow = Flow {
            b: &*body,
            fuel: &mut self.fuel,
            slot,
            read,
            scope_locals,
            frames: Vec::new(),
            breaks: HashMap::default(),
            continues: HashMap::default(),
            arms: Vec::new(),
            may_hold: vec![false; k],
            may_lack: vec![false; k],
            errors: BTreeMap::new(),
            out_of_fuel: false,
        };
        if k > 0 {
            for r in roots {
                if r == NONE {
                    continue;
                }
                // The scopes of the root drop their locals as it ends.
                let _ = flow.block(r, State::new(k));
                flow.frames.clear();
                if flow.out_of_fuel {
                    break;
                }
            }
        }
        if flow.out_of_fuel {
            return crate::body::out_of_fuel();
        }
        let drop_modes: Vec<DropMode> = drops
            .iter()
            .map(|&l| {
                let s = flow.slot[l] as usize;
                match (flow.may_hold[s], flow.may_lack[s]) {
                    (_, false) => DropMode::Always,
                    (false, true) => DropMode::Never,
                    (true, true) => DropMode::Flag,
                }
            })
            .collect();
        let errors: Vec<AfterMove> = std::mem::take(&mut flow.errors).into_values().collect();
        drop(flow);
        for e in errors {
            self.after_move(&e);
        }
        for i in partial {
            let at = self.b.body_mut().syn[i];
            if at == NodeIdx::NONE {
                continue;
            }
            let node = self.cx.src.parse.tree.node(at);
            self.err(
                Code::PartialMove,
                node,
                "a part that owns cannot move out of an owned value on its own; lend it to a `^` position, or move the whole value",
            );
        }
        if !drops.is_empty() {
            self.b.set_moves(moves);
            self.b.set_drop_modes(drop_modes);
        }
        Ok(())
    }

    /// Reports a use after a move, naming the move.
    fn after_move(&mut self, e: &AfterMove) {
        let body = self.b.body_mut();
        let at = body.syn[e.at as usize];
        if at == NodeIdx::NONE {
            return;
        }
        let name = self
            .cx
            .names
            .text(body.local_name[e.local as usize])
            .to_owned();
        let site =
            (e.site != NONE).then(|| (body.tags[e.site as usize], body.syn[e.site as usize]));
        let line = |me: &Self, n: NodeIdx| {
            let lo = me.cx.src.span(me.cx.src.parse.tree.node(n)).lo as usize;
            me.cx.src.text[..lo.min(me.cx.src.text.len())]
                .bytes()
                .filter(|c| *c == b'\n')
                .count()
                + 1
        };
        let msg = match site {
            Some(_) if e.site == e.at => format!(
                "`{name}` moves here on an earlier pass through the loop, so it holds no value on the next one; move it out of the loop, or assign it again before the loop repeats"
            ),
            Some((Tag::Defer, _)) if suite_holds(body, body.data[e.site as usize][0], e.at) => {
                format!(
                    "this `defer` takes `{name}` each time it registers, so a later pass through the loop finds it moved; register the `defer` outside the loop, or give the suite a `^` local"
                )
            }
            Some((Tag::Defer, s)) if s != NodeIdx::NONE => format!(
                "`{name}` moved into the `defer` on line {}, which takes every owned value its suite reads when it registers; give the suite a `^` local instead",
                line(self, s)
            ),
            Some((_, s)) if s != NodeIdx::NONE => format!(
                "`{name}` is used after it moved on line {}; use it before the move, lend it to a `^` position instead, or assign it again",
                line(self, s)
            ),
            _ => format!("`{name}` is used after it moved"),
        };
        let node = self.cx.src.parse.tree.node(at);
        self.err(Code::UseAfterMove, node, &msg);
    }
}

/// Whether block `blk` holds instruction `at`, at any depth.
fn suite_holds(b: &Body, blk: u32, at: u32) -> bool {
    if b.tags.get(blk as usize) != Some(&Tag::Block) {
        return false;
    }
    b.record(b.data[blk as usize][0])
        .iter()
        .any(|&i| i == at || (b.tags[i as usize] == Tag::Block && suite_holds(b, i, at)))
}
