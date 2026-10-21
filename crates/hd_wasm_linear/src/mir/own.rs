//! Ownership as passes over MIR (`docs/compiler/memory-ir.md`,
//! "Ownership as passes"). The lowering counts conservatively; these
//! passes, each local to one function, make it precise:
//!
//! 1. **move**: a copy of a variable at its last read takes the
//!    variable's count instead of a new one, and zeroes the variable
//!    (Rust's move, with the zero as the drop flag);
//! 2. **place**: each owned value is released right after its last use,
//!    on every path (Rust's drop elaboration; `live.rs` today);
//! 3. **borrow**: a copy whose source can neither change nor die while
//!    the copy lives takes no count (SIL's `load_borrow`; #676's span
//!    loans);
//! 4. **null**: a release of a variable that is zero on every path goes,
//!    and so does a zeroing that nothing reads.
//!
//! The facts come from two dataflow analyses over a graph of the
//! structured body: liveness (backward) and each variable's ownership
//! state (forward), which [`super::verify`] also checks.

use hd_base::{HashMap, HashSet};

use super::{Count, Func, Kind, Mode, Operand, Rvalue, Stmt, VarId, VarKind, each_stmt, reads};

/// One node per statement, plus the end.
pub(super) struct Cfg<'a> {
    pub stmt: Vec<Option<&'a Stmt>>,
    pub succ: Vec<Vec<usize>>,
    pub entry: usize,
    /// Each statement's node, by statement id.
    pub node: HashMap<u32, usize>,
}

impl<'a> Cfg<'a> {
    pub fn new(f: &'a Func) -> Self {
        let mut g = Cfg {
            stmt: vec![None],
            succ: vec![Vec::new()],
            entry: 0,
            node: HashMap::default(),
        };
        let end = 0;
        let mut labels = Vec::new();
        let exit = g.list(&f.exit, end, end, &mut labels);
        g.entry = g.list(&f.body, exit, exit, &mut labels);
        g
    }

    fn add(&mut self, s: &'a Stmt) -> usize {
        self.stmt.push(Some(s));
        self.succ.push(Vec::new());
        let n = self.stmt.len() - 1;
        self.node.insert(s.id, n);
        n
    }

    fn list(
        &mut self,
        l: &'a [Stmt],
        next: usize,
        exit: usize,
        labels: &mut Vec<(u32, usize)>,
    ) -> usize {
        let mut cur = next;
        for s in l.iter().rev() {
            cur = self.one(s, cur, exit, labels);
        }
        cur
    }

    fn one(
        &mut self,
        s: &'a Stmt,
        next: usize,
        exit: usize,
        labels: &mut Vec<(u32, usize)>,
    ) -> usize {
        let n = self.add(s);
        self.succ[n] = match &s.kind {
            Kind::If { then, els, .. } => {
                let t = self.list(then, next, exit, labels);
                let e = self.list(els, next, exit, labels);
                vec![t, e]
            }
            Kind::Block { label, body } => {
                labels.push((*label, next));
                let b = self.list(body, next, exit, labels);
                labels.pop();
                vec![b]
            }
            Kind::Loop { label, body } => {
                labels.push((*label, n));
                let b = self.list(body, n, exit, labels);
                labels.pop();
                vec![b]
            }
            Kind::Br { label } => labels
                .iter()
                .rev()
                .find(|(l, _)| l == label)
                .map(|(_, t)| vec![*t])
                .unwrap_or_default(),
            Kind::Return => vec![exit],
            Kind::Unreachable => Vec::new(),
            _ => vec![next],
        };
        n
    }
}

/// The variables statement `s` reads for liveness; releases count only
/// when `releases` is set, and zeroings (a move's end) when `kills` is.
fn uses(s: &Stmt, releases: bool, kills: bool) -> Vec<VarId> {
    match &s.kind {
        Kind::Count {
            op: Count::Release,
            var,
            ..
        } => {
            if releases {
                vec![*var]
            } else {
                Vec::new()
            }
        }
        Kind::Kill { proj: None, var } => {
            if kills {
                vec![*var]
            } else {
                Vec::new()
            }
        }
        _ => reads(s),
    }
}

/// The variables statement `s` defines whole.
fn defs(s: &Stmt) -> Vec<VarId> {
    match &s.kind {
        Kind::Assign {
            dst, proj: None, ..
        }
        | Kind::Kill {
            var: dst,
            proj: None,
        }
        | Kind::Call { dst: Some(dst), .. } => vec![*dst],
        _ => Vec::new(),
    }
}

/// Live variables after each node. A read of an address computed from a
/// variable (`base + 8`) also reads that variable: its count keeps the
/// object the address points into alive.
pub(super) struct Live {
    pub inn: Vec<HashSet<VarId>>,
    pub out: Vec<HashSet<VarId>>,
}

pub(super) fn liveness(g: &Cfg<'_>, releases: bool, kills: bool) -> Live {
    let n = g.stmt.len();
    let mut base: HashMap<VarId, Vec<VarId>> = HashMap::default();
    for s in g.stmt.iter().flatten() {
        if let Kind::Assign {
            dst,
            proj: None,
            rv:
                Rvalue::Offset { base: b, .. }
                | Rvalue::IndexAddr { base: b, .. }
                | Rvalue::Unique { addr: b, .. },
        } = &s.kind
            && let Some(v) = b.var()
            && v != *dst
        {
            base.entry(*dst).or_default().push(v);
        }
    }
    let with_bases = |vs: Vec<VarId>| {
        let mut out = vs;
        let mut k = 0;
        while k < out.len() {
            if let Some(bs) = base.get(&out[k]) {
                for b in bs {
                    if !out.contains(b) {
                        out.push(*b);
                    }
                }
            }
            k += 1;
        }
        out
    };
    let mut live_in: Vec<HashSet<VarId>> = vec![HashSet::default(); n];
    let mut live_out: Vec<HashSet<VarId>> = vec![HashSet::default(); n];
    let mut changed = true;
    while changed {
        changed = false;
        for k in (0..n).rev() {
            let mut out = HashSet::default();
            for &s in &g.succ[k] {
                out.extend(live_in[s].iter().copied());
            }
            let mut inn = out.clone();
            if let Some(s) = g.stmt[k] {
                for d in defs(s) {
                    inn.remove(&d);
                }
                inn.extend(with_bases(uses(s, releases, kills)));
            }
            if inn != live_in[k] || out != live_out[k] {
                live_in[k] = inn;
                live_out[k] = out;
                changed = true;
            }
        }
    }
    Live {
        inn: live_in,
        out: live_out,
    }
}

/// What a counted variable holds at a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum St {
    /// Zero.
    Null,
    /// A value with a count of its own.
    Owned,
    /// Owned on some paths, zero on the others: its release tests.
    Maybe,
    /// A value it holds no count of: borrowed.
    Unowned,
    /// Released or consumed; its bits are stale.
    Gone,
    /// The paths disagree.
    Conflict,
}

fn join(a: St, b: St) -> St {
    use St::{Conflict, Gone, Maybe, Null, Owned, Unowned};
    match (a, b) {
        (x, y) if x == y => x,
        (Null, Owned | Maybe) | (Owned | Maybe, Null) | (Owned, Maybe) | (Maybe, Owned) => Maybe,
        (Null, Gone) | (Gone, Null) => Gone,
        (Null, Unowned) | (Unowned, Null) => Unowned,
        (Unowned, Gone) | (Gone, Unowned) => Gone,
        _ => Conflict,
    }
}

/// Each counted variable's state, and what bits it copied from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct State {
    pub st: Vec<St>,
    /// `alias[u] = (s, path)`: `u` holds `s`'s values (at `path`) and no
    /// count of them, so zeroing `s` hands its count to `u`.
    pub alias: Vec<Option<(VarId, Option<String>)>>,
}

impl State {
    fn entry(f: &Func) -> Self {
        let st = f
            .vars
            .iter()
            .map(|v| match v.kind {
                VarKind::Param(Mode::Owned) => St::Owned,
                VarKind::Param(_) => St::Unowned,
                _ => St::Null,
            })
            .collect();
        State {
            st,
            alias: vec![None; f.vars.len()],
        }
    }

    fn join(&mut self, o: &State) {
        for (a, b) in self.st.iter_mut().zip(&o.st) {
            *a = join(*a, *b);
        }
        for (a, b) in self.alias.iter_mut().zip(&o.alias) {
            if a != b {
                *a = None;
            }
        }
    }
}

/// The ownership step of statement `s` from `st`; each problem goes to
/// `err` (the verifier's findings).
pub(super) fn step(f: &Func, s: &Stmt, st: &mut State, err: &mut dyn FnMut(String)) {
    let counted = |v: VarId| f.vars[v as usize].counted;
    let name = |v: VarId| f.vars[v as usize].name.clone();
    let id = s.id;
    for v in reads(s) {
        if counted(v) && st.st[v as usize] == St::Gone && !matches!(s.kind, Kind::Count { .. }) {
            err(format!("s{id}: {} read after its release", name(v)));
        }
    }
    let unalias = |st: &mut State, v: VarId| {
        for a in &mut st.alias {
            if a.as_ref().is_some_and(|(s, _)| *s == v) {
                *a = None;
            }
        }
    };
    // A construction consumes its counted parts, as a store into a
    // variable's field consumes the stored value.
    let consume = |st: &mut State, o: &Operand, err: &mut dyn FnMut(String)| {
        if let Operand::Var(v, None) = o
            && counted(*v)
        {
            match st.st[*v as usize] {
                St::Owned | St::Null => st.st[*v as usize] = St::Gone,
                other => err(format!("s{id}: {} consumed while {other:?}", name(*v))),
            }
        }
    };
    match &s.kind {
        Kind::Assign { dst, proj, rv } => {
            match rv {
                Rvalue::Make { parts, .. } | Rvalue::Variant { parts, .. } => {
                    for p in parts {
                        consume(st, p, err);
                    }
                }
                Rvalue::Use(o) if proj.is_some() => consume(st, o, err),
                _ => {}
            }
            if !counted(*dst) {
                return;
            }
            if proj.is_some() {
                return;
            }
            if let Rvalue::Unique { addr, .. } = rv
                && addr.var() == Some(*dst)
            {
                return;
            }
            if matches!(st.st[*dst as usize], St::Owned) {
                err(format!(
                    "s{id}: {} overwritten while it owns a count",
                    name(*dst)
                ));
            }
            unalias(st, *dst);
            st.st[*dst as usize] = if matches!(
                rv,
                Rvalue::Alloc { .. }
                    | Rvalue::Take { .. }
                    | Rvalue::Make { .. }
                    | Rvalue::Variant { .. }
            ) {
                St::Owned
            } else {
                St::Unowned
            };
            st.alias[*dst as usize] = match rv {
                Rvalue::Use(Operand::Var(src, p)) => {
                    Some((*src, p.as_ref().map(|p| p.path.clone())))
                }
                _ => None,
            };
        }
        Kind::Count {
            op: Count::Retain,
            var,
            ..
        } => match st.st[*var as usize] {
            St::Unowned | St::Null => st.st[*var as usize] = St::Owned,
            other => err(format!("s{id}: retain of {} while {other:?}", name(*var))),
        },
        Kind::Count {
            op: Count::Release,
            var,
            ..
        } => match st.st[*var as usize] {
            St::Owned | St::Maybe => st.st[*var as usize] = St::Gone,
            St::Null => {}
            other => err(format!("s{id}: release of {} while {other:?}", name(*var))),
        },
        Kind::Kill { var, proj } => {
            let path = proj.as_ref().map(|p| p.path.clone());
            let owned = matches!(st.st[*var as usize], St::Owned | St::Maybe);
            if owned && counted(*var) {
                // The count goes to the variable that copied these bits.
                let heir = (0..st.alias.len()).find(|&u| {
                    st.alias[u].as_ref() == Some(&(*var, path.clone())) && st.st[u] == St::Unowned
                });
                match heir {
                    Some(u) => st.st[u] = St::Owned,
                    None if proj.is_some() => {}
                    None => err(format!(
                        "s{id}: {} zeroed while it owns a count",
                        name(*var)
                    )),
                }
            }
            // Zeroing the one counted value of a variable zeroes its count.
            if proj.is_none() || f.vars[*var as usize].refs == 1 {
                unalias(st, *var);
                st.st[*var as usize] = St::Null;
            }
            for a in &mut st.alias {
                if a.as_ref() == Some(&(*var, path.clone())) {
                    *a = None;
                }
            }
        }
        Kind::Call { dst, args, .. } => {
            for (a, m) in args {
                if *m == Mode::Owned
                    && let Operand::Var(v, None) = a
                    && counted(*v)
                {
                    match st.st[*v as usize] {
                        St::Owned | St::Null => st.st[*v as usize] = St::Gone,
                        other => err(format!(
                            "s{id}: {} passed as owned while {other:?}",
                            name(*v)
                        )),
                    }
                }
            }
            if let Some(d) = dst
                && counted(*d)
            {
                let was = st.st[*d as usize];
                if was == St::Owned && !args.iter().any(|(a, _)| a.var() == Some(*d)) {
                    err(format!(
                        "s{id}: {} overwritten while it owns a count",
                        name(*d)
                    ));
                }
                unalias(st, *d);
                st.st[*d as usize] = St::Owned;
                st.alias[*d as usize] = None;
            }
        }
        Kind::Store { val, .. } => {
            if let Operand::Var(v, None) = val
                && counted(*v)
            {
                match st.st[*v as usize] {
                    St::Owned | St::Null => st.st[*v as usize] = St::Gone,
                    other => err(format!("s{id}: {} stored while {other:?}", name(*v))),
                }
            }
        }
        _ => {}
    }
}

/// Each node's ownership state on entry; problems go to `err`.
pub(super) fn states(f: &Func, g: &Cfg<'_>, err: &mut dyn FnMut(String)) -> Vec<Option<State>> {
    let n = g.stmt.len();
    let mut at: Vec<Option<State>> = vec![None; n];
    at[g.entry] = Some(State::entry(f));
    let mut work: Vec<usize> = vec![g.entry];
    let mut rounds = 0;
    while let Some(k) = work.pop() {
        rounds += 1;
        if rounds > 100_000 {
            err("the ownership analysis does not settle".into());
            break;
        }
        let Some(mut st) = at[k].clone() else {
            continue;
        };
        if let Some(s) = g.stmt[k] {
            step(f, s, &mut st, &mut |_| {});
        }
        for &s in &g.succ[k] {
            let next = match &at[s] {
                None => st.clone(),
                Some(old) => {
                    let mut j = old.clone();
                    j.join(&st);
                    j
                }
            };
            if at[s].as_ref() != Some(&next) {
                at[s] = Some(next);
                work.push(s);
            }
        }
    }
    // The findings, once, from the settled states.
    for (s, st) in g.stmt.iter().zip(&at) {
        if let (Some(s), Some(st)) = (s, st) {
            let mut st = st.clone();
            step(f, s, &mut st, err);
        }
    }
    if let Some(end) = &at[0] {
        for (v, s) in end.st.iter().enumerate() {
            let var = &f.vars[v];
            if var.counted && var.kind != VarKind::Result && matches!(s, St::Owned | St::Maybe) {
                err(format!("{} still owns a count at the end", var.name));
            }
            if *s == St::Conflict {
                err(format!("{}: the paths disagree on its count", var.name));
            }
        }
    }
    at
}

/// Runs the passes in order.
pub fn run(f: &mut Func) {
    let count = |f: &Func, op: Count| {
        let mut n = 0;
        each_stmt(&f.body, &mut |s| {
            if matches!(&s.kind, Kind::Count { op: o, .. } if *o == op) {
                n += 1;
            }
        });
        n
    };
    f.stats.retains_before = count(f, Count::Retain);
    f.stats.releases_before = count(f, Count::Release) + f.exit.len();
    moves(f);
    place(f);
    nulls(f);
    borrows(f);
    nulls(f);
    dead(f);
    copies(f);
    chains(f);
    fold(f);
    hoist(f);
    dead(f);
}

/// Edits to apply to the tree: statements to drop, to add after a
/// statement, or to add at the start of an `if`'s arm (`true`: `then`).
#[derive(Default)]
struct Edits {
    drop: HashSet<u32>,
    after: HashMap<u32, Vec<Kind>>,
    arm: HashMap<(u32, bool), Vec<Kind>>,
    start: Vec<Kind>,
}

impl Edits {
    fn apply(self, f: &mut Func) {
        fn walk(l: &mut Vec<Stmt>, e: &Edits, mk: &mut dyn FnMut(u32, Kind) -> Stmt) {
            let old = std::mem::take(l);
            for mut s in old {
                let origin = s.origin;
                let id = s.id;
                match &mut s.kind {
                    Kind::If { then, els, .. } => {
                        walk(then, e, mk);
                        walk(els, e, mk);
                        for (arm, list) in [(true, then), (false, els)] {
                            if let Some(ks) = e.arm.get(&(id, arm)) {
                                let mut head: Vec<Stmt> =
                                    ks.iter().map(|k| mk(origin, k.clone())).collect();
                                head.append(list);
                                *list = head;
                            }
                        }
                    }
                    Kind::Block { body, .. } | Kind::Loop { body, .. } => walk(body, e, mk),
                    _ => {}
                }
                if !e.drop.contains(&id) {
                    l.push(s);
                }
                if let Some(ks) = e.after.get(&id) {
                    for k in ks {
                        l.push(mk(origin, k.clone()));
                    }
                }
            }
        }
        let mut next = f.next_id;
        let mut mk = |origin: u32, kind: Kind| {
            next += 1;
            Stmt {
                id: next - 1,
                origin,
                kind,
            }
        };
        walk(&mut f.body, &self, &mut mk);
        walk(&mut f.exit, &self, &mut mk);
        let origin = f.body.first().map_or(hd_tir::ir::NONE, |s| s.origin);
        let mut head: Vec<Stmt> = self.start.iter().map(|k| mk(origin, k.clone())).collect();
        head.append(&mut f.body);
        f.body = head;
        f.next_id = next;
    }
}

fn release(f: &Func, v: VarId) -> [Kind; 2] {
    [
        Kind::Count {
            op: Count::Release,
            var: v,
            ty: f.vars[v as usize].ty.clone(),
        },
        Kind::Kill { var: v, proj: None },
    ]
}

/// Pass 1: a copy at its source's last read moves the count: the copy
/// takes no count of its own, and the source is zeroed (Rust's move,
/// with the zero as its drop flag).
fn moves(f: &mut Func) {
    let mut e = Edits::default();
    let mut n_moved = 0;
    {
        let g = Cfg::new(f);
        let live = liveness(&g, false, false);
        let st = states(f, &g, &mut |_| {});
        let mut lists: Vec<&[Stmt]> = Vec::new();
        super::each_list(&f.body, &mut |l| lists.push(l));
        for l in lists {
            for w in l.windows(2) {
                let (
                    Kind::Assign {
                        dst,
                        proj: None,
                        rv: Rvalue::Use(Operand::Var(src, p)),
                    },
                    Kind::Count {
                        op: Count::Retain,
                        var,
                        ..
                    },
                ) = (&w[0].kind, &w[1].kind)
                else {
                    continue;
                };
                let n = g.node[&w[1].id];
                let Some(at) = &st[g.node[&w[0].id]] else {
                    continue;
                };
                let var_s = &f.vars[*src as usize];
                let movable = dst == var
                    && src != dst
                    && var_s.counted
                    && !matches!(var_s.kind, VarKind::Param(Mode::Borrowed) | VarKind::Result)
                    && matches!(at.st[*src as usize], St::Owned | St::Maybe)
                    && !live.out[n].contains(src);
                if !movable {
                    continue;
                }
                e.drop.insert(w[1].id);
                e.after.insert(
                    w[1].id,
                    vec![Kind::Kill {
                        var: *src,
                        proj: p.clone(),
                    }],
                );
                n_moved += 1;
            }
        }
    }
    f.stats.moved += n_moved;
    e.apply(f);
}

/// Pass 2: each owned value is released right after its last use.
fn place(f: &mut Func) {
    let mut e = Edits::default();
    let mut n_placed = 0;
    {
        let g = Cfg::new(f);
        let live = liveness(&g, false, true);
        let st = states(f, &g, &mut |_| {});
        let owns = |s: &State, v: VarId| matches!(s.st[v as usize], St::Owned | St::Maybe);
        let candidate = |v: VarId| {
            let var = &f.vars[v as usize];
            var.counted && var.kind != VarKind::Result
        };
        // A copy and the retain that makes it owned are one step: a value
        // the copy reads dies after the retain, not between them.
        let mut anchor: HashMap<u32, u32> = HashMap::default();
        super::each_list(&f.body, &mut |l| {
            for w in l.windows(2) {
                if let (
                    Kind::Assign { dst, .. },
                    Kind::Count {
                        op: Count::Retain,
                        var,
                        ..
                    },
                ) = (&w[0].kind, &w[1].kind)
                    && dst == var
                {
                    anchor.insert(w[0].id, w[1].id);
                }
            }
        });
        // Owned parameters the body never reads.
        if let Some(at) = &st[g.entry] {
            for &p in &f.params {
                if candidate(p) && owns(at, p) && !live.inn[g.entry].contains(&p) {
                    e.start.extend(release(f, p));
                    n_placed += 1;
                }
            }
        }
        for (k, (s, before)) in g.stmt.iter().zip(&st).enumerate().skip(1) {
            let (Some(s), Some(before)) = (s, before) else {
                continue;
            };
            let mut after = before.clone();
            step(f, s, &mut after, &mut |_| {});
            match &s.kind {
                // A value that only one arm reads dies at the other's start.
                Kind::If { .. } => {
                    for (arm, &to) in [true, false].iter().zip(&g.succ[k]) {
                        let mut vs: Vec<VarId> = live.out[k]
                            .iter()
                            .copied()
                            .filter(|v| {
                                candidate(*v) && !live.inn[to].contains(v) && owns(&after, *v)
                            })
                            .collect();
                        vs.sort_unstable();
                        for v in vs {
                            e.arm.entry((s.id, *arm)).or_default().extend(release(f, v));
                            n_placed += 1;
                        }
                    }
                }
                Kind::Count {
                    op: Count::Release, ..
                }
                | Kind::Kill { .. }
                | Kind::Br { .. }
                | Kind::Return
                | Kind::Unreachable
                | Kind::Block { .. }
                | Kind::Loop { .. } => {}
                // A value live before the statement (or defined by it) and
                // dead after it dies here.
                _ => {
                    let mut vs: Vec<VarId> = live.inn[k].iter().copied().chain(defs(s)).collect();
                    vs.sort_unstable();
                    vs.dedup();
                    let at = anchor.get(&s.id).copied().unwrap_or(s.id);
                    for v in vs {
                        if candidate(v) && !live.out[k].contains(&v) && owns(&after, v) {
                            e.after.entry(at).or_default().extend(release(f, v));
                            n_placed += 1;
                        }
                    }
                }
            }
        }
    }
    f.stats.placed += n_placed;
    e.apply(f);
}

/// Each variable's single definition, when it has exactly one.
fn single_defs(f: &Func) -> HashMap<VarId, Rvalue> {
    let mut defs_of: HashMap<VarId, Rvalue> = HashMap::default();
    let mut n: HashMap<VarId, usize> = HashMap::default();
    let mut add = |s: &Stmt| match &s.kind {
        Kind::Assign { dst, proj, rv } => {
            *n.entry(*dst).or_default() += if proj.is_some() { 2 } else { 1 };
            defs_of.insert(*dst, rv.clone());
        }
        Kind::Call { dst: Some(d), .. } => {
            *n.entry(*d).or_default() += 2;
        }
        _ => {}
    };
    each_stmt(&f.body, &mut add);
    defs_of.retain(|v, _| n.get(v) == Some(&1));
    defs_of
}

/// The variable whose value `v` holds, following copies and address
/// arithmetic back to it: one count identity (Swift's RC identity).
fn rc(defs: &HashMap<VarId, Rvalue>, v: VarId) -> VarId {
    let mut v = v;
    for _ in 0..32 {
        match defs.get(&v) {
            Some(
                Rvalue::Use(Operand::Var(s, None))
                | Rvalue::Offset {
                    base: Operand::Var(s, None),
                    ..
                }
                | Rvalue::IndexAddr {
                    base: Operand::Var(s, None),
                    ..
                },
            ) => v = *s,
            _ => return v,
        }
    }
    v
}

/// Pass 3: borrows. A retain of `x` and the releases that end its count
/// cancel when, on every path from the retain to those releases, another
/// holder of the same value (a protector) keeps it alive and nothing can
/// end that hold, and nothing takes the count elsewhere (SIL's
/// `load_borrow`; #676's span loans). The releases may be of variables
/// that took `x`'s value by moves, a payload moved out of it included. A
/// value read from memory is protected by the object it was read from, as
/// long as no store, call or copy-on-write on the way can replace it.
fn borrows(f: &mut Func) {
    let mut e = Edits::default();
    let mut n_borrowed = 0;
    {
        let g = Cfg::new(f);
        let st = states(f, &g, &mut |_| {});
        let defs = single_defs(f);
        let id = |v: VarId| rc(&defs, v);
        // Each node's statement list position is not needed: the region
        // is walked on the graph.
        let mut done: HashSet<u32> = HashSet::default();
        for (i, s) in g.stmt.iter().enumerate().skip(1) {
            let Some(s) = s else { continue };
            let Kind::Count {
                op: Count::Retain,
                var: x,
                ..
            } = &s.kind
            else {
                continue;
            };
            let x = *x;
            let Some(at) = &st[i] else { continue };
            // The variables holding `x`'s count: `x`, then those that
            // copied it whole or a part of it, when it has one counted value.
            let one = f.vars[x as usize].refs == 1;
            let member = |v: VarId| {
                let mut v = v;
                for _ in 0..32 {
                    if v == x {
                        return true;
                    }
                    match defs.get(&v) {
                        Some(Rvalue::Use(Operand::Var(s, None))) => v = *s,
                        Some(Rvalue::Use(Operand::Var(s, Some(_)))) if one => v = *s,
                        _ => return false,
                    }
                }
                false
            };
            let r = id(x);
            let held = |v: VarId| {
                matches!(f.vars[v as usize].kind, VarKind::Param(Mode::Borrowed))
                    || matches!(at.st[v as usize], St::Owned | St::Maybe)
            };
            let mut protectors: Vec<(VarId, bool)> = (0..f.vars.len())
                .map(|v| VarId::try_from(v).expect("vars"))
                .filter(|v| id(*v) == r && !member(*v) && held(*v))
                .map(|v| (v, false))
                .collect();
            if let Some(Rvalue::Load { addr, .. } | Rvalue::Unique { addr, .. }) = defs.get(&r)
                && let Some(a) = addr.var()
                && held(id(a))
            {
                protectors.push((id(a), true));
            }
            if protectors.is_empty() {
                continue;
            }
            // The region: every node a path from the retain reaches before
            // a release of a member.
            let mut ends: Vec<usize> = Vec::new();
            let mut region: Vec<usize> = Vec::new();
            let mut seen: HashSet<usize> = HashSet::default();
            let mut work: Vec<usize> = g.succ[i].clone();
            let mut ok = true;
            while let Some(n) = work.pop() {
                if !seen.insert(n) {
                    continue;
                }
                // The count must not reach the exit or come round again.
                if n == i || n == 0 || g.stmt[n].is_some_and(|s| matches!(s.kind, Kind::Return)) {
                    ok = false;
                    break;
                }
                let Some(s) = g.stmt[n] else {
                    ok = false;
                    break;
                };
                if f.exit.iter().any(|e| e.id == s.id) {
                    ok = false;
                    break;
                }
                if let Kind::Count {
                    op: Count::Release,
                    var,
                    ..
                } = &s.kind
                    && member(*var)
                {
                    ends.push(n);
                    continue;
                }
                region.push(n);
                work.extend(g.succ[n].iter().copied());
            }
            if !ok
                || ends.is_empty()
                || ends
                    .iter()
                    .any(|n| done.contains(&g.stmt[*n].expect("node").id))
            {
                continue;
            }
            // Nothing in the region takes the count elsewhere.
            let takes = |s: &Stmt| -> bool {
                let m = |o: &Operand| o.var().is_some_and(&member);
                match &s.kind {
                    Kind::Call { args, .. } => {
                        args.iter().any(|(a, md)| *md == Mode::Owned && m(a))
                    }
                    Kind::Store { val, .. } => m(val),
                    Kind::Assign { dst, proj, rv } => match rv {
                        Rvalue::Make { parts, .. } | Rvalue::Variant { parts, .. } => {
                            parts.iter().any(m)
                        }
                        // A copy into a field, or into a variable outside
                        // the chain, may carry the count away.
                        Rvalue::Use(o) => {
                            m(o) && f.vars[*dst as usize].counted
                                && (proj.is_some() || !member(*dst))
                        }
                        _ => false,
                    },
                    _ => false,
                }
            };
            if region.iter().any(|n| g.stmt[*n].is_some_and(takes)) {
                continue;
            }
            let safe = |(p, memory): (VarId, bool)| {
                !region
                    .iter()
                    .any(|n| g.stmt[*n].is_some_and(|s| disturbs(s, p, memory)))
            };
            if !protectors.into_iter().any(safe) {
                continue;
            }
            done.insert(s.id);
            e.drop.insert(s.id);
            for n in ends {
                let id = g.stmt[n].expect("node").id;
                done.insert(id);
                e.drop.insert(id);
            }
            n_borrowed += 1;
        }
    }
    f.stats.borrowed += n_borrowed;
    e.apply(f);
}

/// Whether statement `s` may end protector `p`'s hold: replace or drop
/// it, or, for a protector of memory, change the memory (a store, a call,
/// a copy-on-write, a pin).
fn disturbs(s: &Stmt, p: VarId, memory: bool) -> bool {
    match &s.kind {
        Kind::Assign { dst, rv, .. } => {
            *dst == p
                || (memory && matches!(rv, Rvalue::Unique { .. }))
                || matches!(rv, Rvalue::Unique { addr, .. } if addr.var() == Some(p))
        }
        Kind::Kill { var, proj: None } => *var == p,
        Kind::Count {
            op: Count::Release,
            var,
            ..
        } => *var == p,
        Kind::Call { dst, args, .. } => {
            memory
                || *dst == Some(p)
                || args
                    .iter()
                    .any(|(a, m)| *m == Mode::Owned && a.var() == Some(p))
        }
        Kind::Store { val, .. } => memory || val.var() == Some(p),
        Kind::Pin { .. } => memory,
        _ => false,
    }
}

/// Last: `t = move s` whose `t` one later statement reads and then
/// zeroes is that statement reading `s` (`d = move t` is `d = move s`).
fn chains(f: &mut Func) {
    for _ in 0..8 {
        let mut appear: HashMap<VarId, usize> = HashMap::default();
        each_stmt(&f.body, &mut |s| {
            let mut vs = reads(s);
            if let Kind::Assign { dst, .. }
            | Kind::Call { dst: Some(dst), .. }
            | Kind::Kill { var: dst, .. } = &s.kind
            {
                vs.push(*dst);
            }
            for v in vs {
                *appear.entry(v).or_default() += 1;
            }
        });
        let temps: Vec<bool> = f.vars.iter().map(|v| v.kind == VarKind::Temp).collect();
        let mut changed = false;
        super::each_list_mut(&mut f.body, &mut |l| {
            let mut k = 0;
            while k + 1 < l.len() {
                let (
                    Kind::Assign {
                        dst: t,
                        proj: None,
                        rv: Rvalue::Use(Operand::Var(src, p)),
                    },
                    Kind::Kill { var, proj },
                ) = (&l[k].kind, &l[k + 1].kind)
                else {
                    k += 1;
                    continue;
                };
                let (t, src, p) = (*t, *src, p.clone());
                // `t`: its definition, one read and one zeroing.
                if !temps[t as usize] || *var != src || *proj != p {
                    k += 1;
                    continue;
                }
                let read = (k + 2..l.len()).find(|&m| reads(&l[m]).contains(&t));
                let kill = (k + 2..l.len())
                    .find(|&m| matches!(&l[m].kind, Kind::Kill { var, proj: None } if *var == t));
                // Without a zeroing, the read consumes `t` (a construction,
                // a store, an owned argument).
                let consumes = |s: &Stmt| match &s.kind {
                    Kind::Assign {
                        rv: Rvalue::Make { .. } | Rvalue::Variant { .. },
                        ..
                    }
                    | Kind::Store { .. } => true,
                    Kind::Call { args, .. } => args
                        .iter()
                        .any(|(a, m)| *m == Mode::Owned && a.var() == Some(t)),
                    _ => false,
                };
                let (m, z, uses) = match (read, kill) {
                    (Some(m), Some(z)) => (m, z, 3),
                    (Some(m), None) if consumes(&l[m]) => (m, m, 2),
                    _ => {
                        k += 1;
                        continue;
                    }
                };
                if appear.get(&t) != Some(&uses) {
                    k += 1;
                    continue;
                }
                let touches = |s: &Stmt| {
                    let mut hit = false;
                    each_stmt(std::slice::from_ref(s), &mut |s| {
                        hit |= reads(s).contains(&src)
                            || matches!(&s.kind, Kind::Assign { dst, .. } | Kind::Kill { var: dst, .. } | Kind::Call { dst: Some(dst), .. } if *dst == src);
                    });
                    hit
                };
                if z < m
                    || !matches!(
                        l[m].kind,
                        Kind::Assign { .. } | Kind::Call { .. } | Kind::Store { .. }
                    )
                    || l[k + 2..=z]
                        .iter()
                        .enumerate()
                        .any(|(x, s)| x + k + 2 != m && touches(s))
                {
                    k += 1;
                    continue;
                }
                let swap = |o: &mut Operand| {
                    if let Operand::Var(v, None) = o
                        && *v == t
                    {
                        *o = Operand::Var(src, p.clone());
                    }
                };
                super::operands_mut(&mut l[m], &swap);
                if z == m {
                    let id = l[k + 1].id;
                    let origin = l[k + 1].origin;
                    l.insert(
                        m + 1,
                        Stmt {
                            id,
                            origin,
                            kind: Kind::Kill {
                                var: src,
                                proj: p.clone(),
                            },
                        },
                    );
                } else {
                    l[z].kind = Kind::Kill {
                        var: src,
                        proj: p.clone(),
                    };
                }
                l.drain(k..k + 2);
                changed = true;
            }
        });
        if !changed {
            return;
        }
    }
}

/// Folds constant element indices into offsets, and an address that is
/// an offset from a variable into the load or store that uses it (Go's
/// `OffPtr` folded into `Load`).
fn fold(f: &mut Func) {
    super::each_list_mut(&mut f.body, &mut |l| {
        for s in l.iter_mut() {
            if let Kind::Assign {
                rv:
                    rv @ Rvalue::IndexAddr {
                        index: Operand::Const(_),
                        ..
                    },
                ..
            } = &mut s.kind
                && let Rvalue::IndexAddr {
                    base,
                    index: Operand::Const(k),
                    size,
                    off,
                } = rv.clone()
                && let Ok(k) = k.parse::<u32>()
            {
                *rv = Rvalue::Offset {
                    base,
                    off: off + k * size,
                };
            }
        }
    });
    let defs = single_defs(f);
    let mut reads_of: HashMap<VarId, usize> = HashMap::default();
    each_stmt(&f.body, &mut |s| {
        for v in reads(s) {
            *reads_of.entry(v).or_default() += 1;
        }
    });
    let offset_of = |a: &Operand| -> Option<(Operand, u32)> {
        let v = a.var()?;
        if f.vars[v as usize].counted || reads_of.get(&v) != Some(&1) {
            return None;
        }
        match defs.get(&v) {
            Some(Rvalue::Offset { base, off }) => Some((base.clone(), *off)),
            _ => None,
        }
    };
    let mut edits: Vec<(u32, Operand, u32)> = Vec::new();
    each_stmt(&f.body, &mut |s| match &s.kind {
        Kind::Assign {
            rv: Rvalue::Load { addr, .. } | Rvalue::Take { addr, .. },
            ..
        }
        | Kind::Store { addr, .. } => {
            if let Some((base, k)) = offset_of(addr) {
                edits.push((s.id, base, k));
            }
        }
        _ => {}
    });
    let edits: HashMap<u32, (Operand, u32)> =
        edits.into_iter().map(|(id, b, k)| (id, (b, k))).collect();
    super::each_list_mut(&mut f.body, &mut |l| {
        for s in l.iter_mut() {
            let Some((base, k)) = edits.get(&s.id) else {
                continue;
            };
            match &mut s.kind {
                Kind::Assign {
                    rv: Rvalue::Load { addr, off, .. } | Rvalue::Take { addr, off, .. },
                    ..
                }
                | Kind::Store { addr, off, .. } => {
                    *addr = base.clone();
                    *off += k;
                }
                _ => {}
            }
        }
    });
}

/// Hoists loop-invariant loads and copy-on-write checks out of loops
/// (`docs/compiler/memory-ir.md`, "Hoisting"): a load whose address the
/// loop does not change, when nothing in the loop can write that memory;
/// a `unique` of a list variable that nothing in the loop can share. An
/// element store writes past a buffer's header, so it changes no length
/// load (`crates/hd_wasm_linear/DESIGN.md` §23.31).
fn hoist(f: &mut Func) {
    let defs = single_defs(f);
    let counted: Vec<bool> = f.vars.iter().map(|v| v.counted).collect();
    let root = |v: VarId| rc(&defs, v);
    hoist_list(&mut f.body, &defs, &counted, &root);
}

fn hoist_list(
    l: &mut Vec<Stmt>,
    defs: &HashMap<VarId, Rvalue>,
    counted: &[bool],
    root: &dyn Fn(VarId) -> VarId,
) {
    for s in l.iter_mut() {
        match &mut s.kind {
            Kind::If { then, els, .. } => {
                hoist_list(then, defs, counted, root);
                hoist_list(els, defs, counted, root);
            }
            Kind::Block { body, .. } | Kind::Loop { body, .. } => {
                hoist_list(body, defs, counted, root);
            }
            _ => {}
        }
    }
    let mut k = 0;
    while k < l.len() {
        let Kind::Loop { body, .. } = &mut l[k].kind else {
            k += 1;
            continue;
        };
        let mut hoisted: Vec<Stmt> = Vec::new();
        while let Some(id) = invariant(body, defs, counted, root) {
            let mut taken = None;
            each_list_take(body, id, &mut taken);
            match taken {
                Some(s) => hoisted.push(s),
                None => break,
            }
        }
        let n = hoisted.len();
        for (j, s) in hoisted.into_iter().enumerate() {
            l.insert(k + j, s);
        }
        k += n + 1;
    }
}

/// A statement of loop body `body` that can move before the loop.
fn invariant(
    body: &[Stmt],
    defs: &HashMap<VarId, Rvalue>,
    counted: &[bool],
    root: &dyn Fn(VarId) -> VarId,
) -> Option<u32> {
    // What the loop assigns, and what memory it may write.
    let mut assigned: HashMap<VarId, usize> = HashMap::default();
    let mut opaque = false;
    let mut element_stores: HashSet<VarId> = HashSet::default();
    let mut shared: HashSet<VarId> = HashSet::default();
    each_stmt(body, &mut |s| match &s.kind {
        Kind::Assign { dst, rv, .. } => {
            *assigned.entry(*dst).or_default() += 1;
            if let Rvalue::Use(o) = rv
                && let Some(v) = o.var()
                && counted[v as usize]
            {
                shared.insert(v);
            }
            if matches!(rv, Rvalue::Alloc { .. }) {
                opaque = true;
            }
        }
        Kind::Call { dst, args, .. } => {
            opaque = true;
            if let Some(d) = dst {
                *assigned.entry(*d).or_default() += 2;
            }
            for (a, _) in args {
                if let Some(v) = a.var() {
                    shared.insert(v);
                }
            }
        }
        Kind::Kill { var, .. } => *assigned.entry(*var).or_default() += 2,
        Kind::Store { addr, .. } => match addr.var().and_then(|a| defs.get(&a)) {
            Some(Rvalue::IndexAddr { base, off, .. }) if *off >= 8 => {
                if let Some(b) = base.var() {
                    element_stores.insert(root(b));
                }
            }
            _ => opaque = true,
        },
        Kind::Pin { .. } | Kind::Count { .. } => opaque = true,
        _ => {}
    });
    let fixed = |o: &Operand| o.var().is_none_or(|v| !assigned.contains_key(&v));
    let mut found = None;
    each_list_index(body, &mut |_, s| {
        if found.is_some() {
            return;
        }
        let Kind::Assign {
            dst,
            proj: None,
            rv,
        } = &s.kind
        else {
            return;
        };
        let once = assigned.get(dst) == Some(&1);
        let ok = once
            && match rv {
                // Any load when the loop writes no memory; a list's header
                // load when it writes only that list's elements.
                Rvalue::Load { addr, off, .. } => {
                    fixed(addr)
                        && !opaque
                        && (element_stores.is_empty()
                            || (*off < 8
                                && element_stores.len() == 1
                                && addr
                                    .var()
                                    .is_some_and(|a| element_stores.contains(&root(a)))))
                }
                // A list the loop neither shares nor passes on is unique
                // after its first check. Hoisted out of the loop's test, it
                // also runs when the loop runs no time: at most one copy
                // the loop did not need (#710 adds an entry test).
                Rvalue::Unique { addr, .. } => {
                    addr.var() == Some(*dst) && !opaque && !shared.contains(dst)
                }
                Rvalue::Offset { base, .. } => fixed(base) && !counted[*dst as usize],
                _ => false,
            };
        if ok {
            found = Some(s.id);
        }
    });
    found
}

/// Calls `f` on every statement of `l` and its nested lists, with the
/// path of `if` arms it is under (empty at the loop body's top level).
fn each_list_index(l: &[Stmt], f: &mut dyn FnMut(&[usize], &Stmt)) {
    fn go(l: &[Stmt], path: &mut Vec<usize>, f: &mut dyn FnMut(&[usize], &Stmt)) {
        for (k, s) in l.iter().enumerate() {
            f(path, s);
            match &s.kind {
                Kind::If { then, els, .. } => {
                    path.push(k);
                    go(then, path, f);
                    go(els, path, f);
                    path.pop();
                }
                Kind::Block { body, .. } | Kind::Loop { body, .. } => {
                    path.push(k);
                    go(body, path, f);
                    path.pop();
                }
                _ => {}
            }
        }
    }
    go(l, &mut Vec::new(), f);
}

/// Takes statement `id` out of `l` or its nested lists.
fn each_list_take(l: &mut Vec<Stmt>, id: u32, out: &mut Option<Stmt>) {
    if let Some(k) = l.iter().position(|s| s.id == id) {
        *out = Some(l.remove(k));
        return;
    }
    for s in l.iter_mut() {
        if out.is_some() {
            return;
        }
        match &mut s.kind {
            Kind::If { then, els, .. } => {
                each_list_take(then, id, out);
                if out.is_none() {
                    each_list_take(els, id, out);
                }
            }
            Kind::Block { body, .. } | Kind::Loop { body, .. } => each_list_take(body, id, out),
            _ => {}
        }
    }
}

/// Last: a copy of a variable that never changes, which holds no count,
/// is that variable.
fn copies(f: &mut Func) {
    for _ in 0..8 {
        let defs = single_defs(f);
        let mut counted: HashSet<VarId> = HashSet::default();
        let mut changed: HashSet<VarId> = HashSet::default();
        each_stmt(&f.body, &mut |s| match &s.kind {
            Kind::Count { var, .. } | Kind::Pin { var } => {
                counted.insert(*var);
            }
            Kind::Kill { var, .. } => {
                changed.insert(*var);
            }
            Kind::Assign {
                dst, proj: Some(_), ..
            } => {
                changed.insert(*dst);
            }
            Kind::Assign {
                dst,
                rv: Rvalue::Unique { .. },
                ..
            } => {
                changed.insert(*dst);
            }
            _ => {}
        });
        let stable = |v: VarId| {
            !changed.contains(&v)
                && (defs.contains_key(&v) || matches!(f.vars[v as usize].kind, VarKind::Param(_)))
        };
        let pick = defs.iter().find_map(|(t, rv)| match rv {
            Rvalue::Use(Operand::Var(s, p))
                if matches!(f.vars[*t as usize].kind, VarKind::Temp | VarKind::Local)
                    && !f.vars[*t as usize].cell
                    && !counted.contains(t)
                    && !changed.contains(t)
                    && stable(*s) =>
            {
                Some((*t, *s, p.clone()))
            }
            _ => None,
        });
        let Some((t, src, p)) = pick else { return };
        let swap = |o: &mut Operand| {
            if let Operand::Var(v, q) = o
                && *v == t
            {
                *o = Operand::Var(
                    src,
                    match (&p, q.as_ref()) {
                        (None, q) => q.cloned(),
                        (Some(p), None) => Some(p.clone()),
                        (Some(p), Some(q)) => Some(super::Proj {
                            leaves: q.leaves.iter().map(|k| p.leaves[*k as usize]).collect(),
                            path: format!("{}{}", p.path, q.path),
                        }),
                    },
                );
            }
        };
        super::each_list_mut(&mut f.body, &mut |l| {
            l.retain(|s| !matches!(&s.kind, Kind::Assign { dst, proj: None, .. } if *dst == t));
            for s in l.iter_mut() {
                super::operands_mut(s, &swap);
            }
        });
    }
}

/// Pass 4: releases of zero variables, and zeroings nothing reads.
fn nulls(f: &mut Func) {
    let mut e = Edits::default();
    let mut n_null_releases = 0;
    {
        let g = Cfg::new(f);
        let st = states(f, &g, &mut |_| {});
        for (s, at) in g.stmt.iter().zip(&st).skip(1) {
            let (Some(s), Some(at)) = (s, at) else {
                continue;
            };
            if let Kind::Count {
                op: Count::Release,
                var,
                ..
            } = &s.kind
                && at.st[*var as usize] == St::Null
            {
                e.drop.insert(s.id);
                n_null_releases += 1;
            }
        }
    }
    f.stats.null_releases += n_null_releases;
    e.apply(f);
}

/// Last: zeroings and pure assignments whose variable nothing reads.
fn dead(f: &mut Func) {
    for _ in 0..4 {
        let mut e = Edits::default();
        {
            let g = Cfg::new(f);
            let live = liveness(&g, true, false);
            let st = states(f, &g, &mut |_| {});
            // A zeroing that hands a count on (a move) stays: it says where
            // the count went.
            let moves = |k: usize, v: VarId| {
                st[k]
                    .as_ref()
                    .is_some_and(|s| matches!(s.st[v as usize], St::Owned | St::Maybe))
            };
            for k in 1..g.stmt.len() {
                let Some(s) = g.stmt[k] else { continue };
                let dead = match &s.kind {
                    Kind::Kill { var, proj: None } => !live.out[k].contains(var) && !moves(k, *var),
                    Kind::Kill { var, proj: Some(_) } => {
                        !live.out[k].contains(var) && !moves(k, *var)
                    }
                    Kind::Assign { dst, rv, .. } => {
                        f.vars[*dst as usize].kind == VarKind::Temp
                            && !live.out[k].contains(dst)
                            && !matches!(rv, Rvalue::Unique { .. } | Rvalue::Alloc { .. })
                    }
                    _ => false,
                };
                if dead {
                    e.drop.insert(s.id);
                }
            }
            if e.drop.is_empty() {
                break;
            }
        }
        e.apply(f);
    }
}
