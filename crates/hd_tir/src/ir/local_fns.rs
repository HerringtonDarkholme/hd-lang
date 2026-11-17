//! Generic local functions instantiated in an instance body
//! (`docs/future-work/INSTANCE_TIR.md`, "One builder", step 4): each
//! generic local function's sub-body (`sub_flags::GENERIC`) is replaced by
//! one plain sub-body per distinct instantiation, and each `Closure` that
//! instantiated it now names that sub-body. So no pass after `hd_mono`
//! sees a generic local function.
//!
//! The unit is a *context*: the body's own instructions (the root context),
//! or one copy of a generic local function's sub-body, and the plain
//! closures inside it, at one binding of its own type parameters (and of
//! those of the generic local functions around it). An instantiation of
//! function `t` found in context `c` names the context `(a, t, args)`,
//! where `a` is the context around `t`'s declaration (an ancestor of `c`)
//! and `args` its type arguments under `c`'s binding: a recursive call
//! inside `t` finds its own context again, so the copies are finite.
//! rustc instead keeps one MIR body per closure and monomorphizes it
//! through its parent's substitution (`ClosureArgs`) as it lowers; hd
//! follows Swift, which specializes a closure with its context function:
//! the copy is a sub-body of the instance body, built once.
//!
//! The output is a new body that holds each context's instructions, in
//! source order, with every instruction, record, local, label, sub-body
//! and capture word renumbered into it. A context's instructions precede
//! those of the context it is declared in, so a block lists the roots of
//! the copies declared in it after they are made, and the body's root
//! stays last. A generic function's defining `Closure`, which makes no
//! value, and a generic function nothing instantiates are not copied.
//!
//! Cost: one pass over the body to sort its instructions, locals, labels,
//! sub-bodies and side rows by context region, with each instruction's and
//! sub-body's place in its region in a table (O(1) lookups; locals and
//! labels are binary-searched), then each context's region copied once:
//! O(body + total output), with each
//! record copied once per context (a memo) and each distinct type mapped
//! once per context by the caller. An instantiation finds its declaring
//! context by walking its context's ancestors, as deep as generic local
//! functions nest.

use hd_base::SliceSort as _;
use hd_base::{HashMap, LocalId, Range32, SubId};
use hd_types::{InternPool, Ty, TyList};

use super::{Body, Loc, NONE, Op, Ref, Tag, Topology, place, slots, sub_flags, u32_of};

/// The types of one context: `t` with the generic local functions' own
/// parameters bound by `bind`, each `(parameter index of the item's
/// generics owner, type)`. `ctx` names the context, for a memo.
pub trait LocalTypes {
    fn ty(&mut self, ctx: u32, bind: &[(u32, Ty)], t: Ty) -> Ty;
}

/// One copy: a region (the root, 0, or a generic local function's
/// sub-body) at a binding.
struct Ctx {
    region: u32,
    parent: u32,
    bind: Vec<(u32, Ty)>,
    local_base: u32,
    label_base: u32,
    sub_base: u32,
}

/// `b` with its generic local functions instantiated (the module), and
/// the number of sub-bodies made for them; `None` when it has none.
///
/// # Errors
/// A word that names an instruction outside its own context, which the
/// checker's closures never do.
pub fn instantiate_local_functions(
    b: &Body,
    pool: &InternPool,
    types: &mut dyn LocalTypes,
) -> Result<Option<(Body, usize)>, String> {
    let nsub = b.sub_root.len();
    if !(0..nsub).any(|s| b.sub_is_generic(s)) {
        return Ok(None);
    }
    let n = b.len();
    let topo = Topology::new(b);
    // Each sub-body's region: the nearest generic sub-body at or around
    // it, or 0. A parent is opened before its children.
    // A sub-body's parent holds its defining closure (an instantiation
    // may sit anywhere, inside the function itself too), opened before
    // it, so it has the smaller number.
    let mut parent = vec![0u32; nsub];
    for i in 0..n {
        if b.tags[i] == Tag::Closure && !b.is_instantiation(i) {
            let k = b.data[i][0] as usize;
            if k < nsub && topo.sub[i] < b.data[i][0] {
                parent[k] = topo.sub[i];
            }
        }
    }
    let mut region = vec![0u32; nsub];
    let mut depth = vec![0u32; nsub];
    for s in 1..nsub {
        let p = parent[s] as usize;
        region[s] = if b.sub_is_generic(s) {
            u32_of(s)
        } else {
            region[p]
        };
        depth[s] = depth[p] + u32::from(b.sub_is_generic(s));
    }
    let region_of = |i: usize| region[topo.sub[i] as usize] as usize;
    let mut insts: Vec<Vec<u32>> = vec![Vec::new(); nsub];
    let mut defining = hd_base::HashSet::default();
    for i in 0..n {
        // A generic function's defining closure makes no value.
        if b.tags[i] == Tag::Closure
            && b.sub_is_generic(b.data[i][0] as usize)
            && !b.is_instantiation(i)
        {
            defining.insert(u32_of(i));
            continue;
        }
        insts[region_of(i)].push(u32_of(i));
    }
    let mut subs: Vec<Vec<u32>> = vec![Vec::new(); nsub];
    for s in 0..nsub {
        subs[region[s] as usize].push(u32_of(s));
    }
    // Each instruction's and sub-body's place in its region's list, so a
    // copy finds a word's new number in O(1).
    let mut inst_region = vec![NONE; n];
    let mut inst_pos = vec![NONE; n];
    for (r, list) in insts.iter().enumerate() {
        for (p, &i) in list.iter().enumerate() {
            inst_region[i as usize] = u32_of(r);
            inst_pos[i as usize] = u32_of(p);
        }
    }
    let mut sub_pos = vec![0u32; nsub];
    for list in &subs {
        for (p, &s) in list.iter().enumerate() {
            sub_pos[s as usize] = u32_of(p);
        }
    }
    // Each side row's region, so a context copies only its own rows.
    let mut side = vec![Side::default(); nsub];
    for (k, &[i, _]) in b.fallback.iter().enumerate() {
        side[region_of(i as usize)].fallback.push(u32_of(k));
    }
    for (k, &(call, _)) in b.held_args.iter().enumerate() {
        side[region_of(call as usize)].held.push(u32_of(k));
    }
    for (k, row) in b.susp.iter().enumerate() {
        side[region_of(row.inst as usize)].susp.push(u32_of(k));
    }
    let mut labels: Vec<Vec<u32>> = vec![Vec::new(); nsub];
    for (l, &at) in b.label_inst.iter().enumerate() {
        labels[region_of(at as usize)].push(u32_of(l));
    }
    // Each local's region: the outermost one that names it, which is
    // where it is declared; a capture names it in an inner one too.
    let nl = b.local_ty.len();
    let mut owner = vec![NONE; nl];
    let mut note = |l: u32, r: usize| {
        if let Some(o) = owner.get_mut(l as usize)
            && (*o == NONE || depth[r] < depth[*o as usize])
        {
            *o = u32_of(r);
        }
    };
    for i in 0..n {
        let r = region_of(i);
        local_words(b, i, &mut |l| note(l, r));
    }
    for (s, &r) in region.iter().enumerate() {
        for &l in b.sub_param_locals(s) {
            note(l, r as usize);
        }
    }
    for &[i, l] in &b.fallback {
        if l != 0 {
            note(l - 1, region_of(i as usize));
        }
    }
    // A local nothing names is dropped: a generic local function's own
    // binding, whose uses are its instantiations. A root parameter is
    // kept, so the parameters keep their places.
    let mut locals: Vec<Vec<u32>> = vec![Vec::new(); nsub];
    for (l, &o) in owner.iter().enumerate() {
        if o != NONE {
            locals[o as usize].push(u32_of(l));
        } else if b.local_flags[l] & super::local_flags::PARAM != 0 {
            locals[0].push(u32_of(l));
        }
    }

    // The contexts, parents first, and each instantiation's target.
    let mut ctxs = vec![Ctx {
        region: 0,
        parent: NONE,
        bind: Vec::new(),
        local_base: 0,
        label_base: 0,
        sub_base: 0,
    }];
    let mut keys: HashMap<(u32, u32, TyList), u32> = HashMap::default();
    let mut target: HashMap<(u32, u32), u32> = HashMap::default();
    let mut k = 0;
    while k < ctxs.len() {
        let r = ctxs[k].region as usize;
        for &i in &insts[r] {
            let i = i as usize;
            if !b.is_instantiation(i) {
                continue;
            }
            let t = b.data[i][0];
            let rec = b.record(b.data[i][1]);
            let (first, targs) = (rec[2], TyList(rec[3]));
            let home = region[parent[t as usize] as usize];
            let mut a = u32_of(k);
            while ctxs[a as usize].region != home {
                a = ctxs[a as usize].parent;
                if a == NONE {
                    return Err(format!(
                        "an instantiation of sub-body {t} outside its scope"
                    ));
                }
            }
            let args = Map {
                types: &mut *types,
                pool,
                ctx: u32_of(k),
                bind: &ctxs[k].bind,
                lists: HashMap::default(),
            }
            .list(targs);
            let next = u32_of(ctxs.len());
            let c = *keys.entry((a, t, args)).or_insert(next);
            if c == next {
                let mut bind = ctxs[a as usize].bind.clone();
                for (j, &arg) in pool.list_items(args).iter().enumerate() {
                    bind.push((first + u32_of(j), arg));
                }
                ctxs.push(Ctx {
                    region: t,
                    parent: a,
                    bind,
                    local_base: 0,
                    label_base: 0,
                    sub_base: 0,
                });
            }
            target.insert((u32_of(k), u32_of(i)), c);
        }
        k += 1;
    }
    let built = ctxs.len() - 1;
    // Children per context, by function, in creation order.
    let mut children: HashMap<(u32, u32), Vec<u32>> = HashMap::default();
    for (c, x) in ctxs.iter().enumerate().skip(1) {
        children
            .entry((x.parent, x.region))
            .or_default()
            .push(u32_of(c));
    }
    // Instruction order: each context's in source order, with the copies
    // of a function declared in it where that function's body stood
    // (before the first later instruction), so a block's members and a
    // statement's first instruction keep their order, and the body's
    // root stays last. An explicit stack: generic local functions nest
    // as deep as the source does.
    let mut kids: Vec<Vec<(u32, u32)>> = vec![Vec::new(); ctxs.len()];
    for &(c, t) in children.keys() {
        kids[c as usize].push((b.sub_root[t as usize], t));
    }
    for k in &mut kids {
        k.sort_stable();
    }
    let mut newpos: Vec<Vec<u32>> = ctxs
        .iter()
        .map(|x| vec![NONE; insts[x.region as usize].len()])
        .collect();
    let mut at = 0u32;
    // (context, next instruction, next function, next copy of it)
    let mut stack: Vec<(u32, usize, usize, usize)> = vec![(0, 0, 0, 0)];
    while let Some(top) = stack.last_mut() {
        let (c, pos, ti, ki) = *top;
        let list = &insts[ctxs[c as usize].region as usize];
        let next = list.get(pos).copied().unwrap_or(NONE);
        if let Some(&(key, t)) = kids[c as usize].get(ti)
            && key < next
        {
            let made = children.get(&(c, t)).map_or(&[][..], Vec::as_slice);
            if let Some(&k) = made.get(ki) {
                top.3 += 1;
                stack.push((k, 0, 0, 0));
            } else {
                top.2 += 1;
                top.3 = 0;
            }
            continue;
        }
        if pos < list.len() {
            newpos[c as usize][pos] = at;
            at += 1;
            top.1 += 1;
            continue;
        }
        stack.pop();
    }
    let (mut lo, mut la, mut su) = (0u32, 0u32, 0u32);
    for x in &mut ctxs {
        let r = x.region as usize;
        x.local_base = lo;
        x.label_base = la;
        x.sub_base = su;
        lo += u32_of(locals[r].len());
        la += u32_of(labels[r].len());
        su += u32_of(subs[r].len());
    }

    let mut out = Body::new(b.item, b.kind);
    out.phase = b.phase;
    out.label.clone_from(&b.label);
    out.consts.clone_from(&b.consts);
    out.strings.clone_from(&b.strings);
    out.inlined.clone_from(&b.inlined);
    out.inlined_origins.clone_from(&b.inlined_origins);
    out.tags.resize(at as usize, Tag::Poison);
    out.data.resize(at as usize, [NONE, NONE]);
    out.ty.resize(at as usize, Ty::VOID);
    out.syn.resize(at as usize, hd_base::NodeIdx::NONE);
    out.anchors.resize(at as usize, NONE);
    out.local_ty.resize(lo as usize, Ty::VOID);
    out.local_name
        .resize(lo as usize, hd_base::Symbol::from_raw(0));
    out.local_syn.resize(lo as usize, hd_base::NodeIdx::NONE);
    out.local_flags.resize(lo as usize, 0);
    out.label_inst.resize(la as usize, NONE);
    out.sub_root.resize(su as usize, NONE);
    out.sub_params.resize(su as usize, Range32::default());
    out.sub_parent.resize(su as usize, SubId::from_raw(0));
    out.sub_flags.resize(su as usize, 0);
    let mut generic_roots = HashMap::default();
    for s in 1..nsub {
        if b.sub_is_generic(s) {
            generic_roots.insert(b.sub_root[s], u32_of(s));
        }
    }
    let cx = Tables {
        b,
        newpos: &newpos,
        inst_region: &inst_region,
        inst_pos: &inst_pos,
        sub_pos: &sub_pos,
        parent: &parent,
        pool,
        generic_roots,
        defining,
        ctxs: &ctxs,
        insts: &insts,
        locals: &locals,
        labels: &labels,
        subs: &subs,
        target: &target,
        children: &children,
        side: &side,
    };
    for c in 0..ctxs.len() {
        cx.copy(&mut out, u32_of(c), types)?;
    }
    out.fallback.sort_stable_by_key(|r| r[0]);
    out.held_args.sort_stable_by_key(|r| r.0);
    out.susp.sort_stable_by_key(|r| r.inst);
    Ok(Some((out, built)))
}

/// One context's type map: the caller's per type, a memo per list.
struct Map<'a> {
    types: &'a mut dyn LocalTypes,
    pool: &'a InternPool,
    ctx: u32,
    bind: &'a [(u32, Ty)],
    lists: HashMap<TyList, TyList>,
}

impl Map<'_> {
    fn ty(&mut self, t: Ty) -> Ty {
        if self.bind.is_empty() {
            return t;
        }
        self.types.ty(self.ctx, self.bind, t)
    }

    fn list(&mut self, l: TyList) -> TyList {
        if self.bind.is_empty() {
            return l;
        }
        if let Some(&m) = self.lists.get(&l) {
            return m;
        }
        let items: Vec<Ty> = self.pool.list_items(l).to_vec();
        let items: Vec<Ty> = items.into_iter().map(|t| self.ty(t)).collect();
        let m = self.pool.list(&items);
        self.lists.insert(l, m);
        m
    }
}

/// Appends a record of `words`; its length word's position.
fn push_record(out: &mut Body, words: &[u32]) -> u32 {
    let at = u32_of(out.extra.len());
    out.extra.push(u32_of(words.len()));
    out.extra.extend_from_slice(words);
    at
}

/// Record `w` of `b` copied into `out`, once per context (`memo`).
fn copy_record(out: &mut Body, b: &Body, memo: &mut HashMap<u32, u32>, w: u32) -> u32 {
    if let Some(&at) = memo.get(&w) {
        return at;
    }
    let at = push_record(out, b.record(w));
    memo.insert(w, at);
    at
}

/// The `extra` words one context has mapped, from where its records
/// start, so a record its instructions share is mapped once.
struct Done {
    base: usize,
    seen: Vec<bool>,
}

/// Whether `extra` word `at` was already mapped; marks it.
fn done_at(done: &mut Done, at: usize) -> bool {
    let k = at - done.base;
    if done.seen.len() <= k {
        done.seen.resize(k + 1, false);
    }
    std::mem::replace(&mut done.seen[k], true)
}

fn mark(done: &mut Done, at: usize, len: usize) {
    for p in at..at + len {
        done_at(done, p);
    }
}

/// Calls `f` with each local instruction `i` names: a place's root, and
/// a closure's captures.
fn local_words(b: &Body, i: usize, f: &mut dyn FnMut(u32)) {
    let (oa, ob) = b.tags[i].operands();
    for (w, op) in [(b.data[i][0], oa), (b.data[i][1], ob)] {
        if op != Op::Place || w == NONE {
            continue;
        }
        if w & place::BARE_LOCAL != 0 {
            f(w & !place::BARE_LOCAL);
        } else if b.extra.get(w as usize + 1) == Some(&place::ROOT_LOCAL) {
            f(b.extra[w as usize + 2]);
        }
    }
    if b.tags[i] == Tag::Closure {
        let rec = b.record(b.data[i][1]);
        for l in &b.cap_local[rec[0] as usize..(rec[0] + rec[1]) as usize] {
            f(l.raw());
        }
    }
}

/// One region's side rows (`fallback`, `held_args`, `susp`), by index.
#[derive(Clone, Default)]
struct Side {
    fallback: Vec<u32>,
    held: Vec<u32>,
    susp: Vec<u32>,
}

struct Tables<'a> {
    b: &'a Body,
    newpos: &'a [Vec<u32>],
    /// Each instruction's region and place in it (`NONE`: not copied).
    inst_region: &'a [u32],
    inst_pos: &'a [u32],
    /// Each sub-body's place in its region's list.
    sub_pos: &'a [u32],
    parent: &'a [u32],
    pool: &'a InternPool,
    /// Each generic sub-body's root, and its defining closures.
    generic_roots: HashMap<u32, u32>,
    defining: hd_base::HashSet<u32>,
    ctxs: &'a [Ctx],
    insts: &'a [Vec<u32>],
    locals: &'a [Vec<u32>],
    labels: &'a [Vec<u32>],
    subs: &'a [Vec<u32>],
    target: &'a HashMap<(u32, u32), u32>,
    children: &'a HashMap<(u32, u32), Vec<u32>>,
    side: &'a [Side],
}

fn find(list: &[u32], x: u32, base: u32) -> Option<u32> {
    list.binary_search(&x).ok().map(|p| base + u32_of(p))
}

impl Tables<'_> {
    /// Instruction `i`'s copy in context `c`, when `c` copies it.
    fn inst(&self, c: u32, i: u32) -> Option<u32> {
        let i = i as usize;
        if self.inst_region.get(i) != Some(&self.ctxs[c as usize].region) {
            return None;
        }
        Some(self.newpos[c as usize][self.inst_pos[i] as usize])
    }

    /// Sub-body `s`'s copy in context `c`, which copies `s`'s region.
    fn sub_in(&self, c: u32, s: u32) -> u32 {
        self.ctxs[c as usize].sub_base + self.sub_pos[s as usize]
    }

    /// Local `l` seen from context `c`: its own copy, or its ancestors'.
    fn local(&self, mut c: u32, l: u32) -> Result<u32, String> {
        while c != NONE {
            let x = &self.ctxs[c as usize];
            if let Some(n) = find(&self.locals[x.region as usize], l, x.local_base) {
                return Ok(n);
            }
            c = x.parent;
        }
        Err(format!("local {l} outside its scope"))
    }

    fn sub(&self, c: u32, s: u32) -> Option<u32> {
        let x = &self.ctxs[c as usize];
        find(&self.subs[x.region as usize], s, x.sub_base)
    }

    /// Copies context `c` into `out`.
    #[allow(clippy::too_many_lines)]
    fn copy(&self, out: &mut Body, c: u32, types: &mut dyn LocalTypes) -> Result<(), String> {
        let b = self.b;
        let x = &self.ctxs[c as usize];
        let r = x.region as usize;
        let bind = &x.bind[..];
        let mut map = Map {
            types,
            pool: self.pool,
            ctx: c,
            bind,
            lists: HashMap::default(),
        };
        let mut records: HashMap<u32, u32> = HashMap::default();
        let mut consts: HashMap<u32, u32> = HashMap::default();
        let mut done = Done {
            base: out.extra.len(),
            seen: Vec::new(),
        };
        let bad = |what: &str, w: u32| format!("{what} {w} outside its context");
        // Locals, labels and sub-bodies of the region.
        for (k, &l) in self.locals[r].iter().enumerate() {
            let (n, l) = ((x.local_base as usize) + k, l as usize);
            out.local_ty[n] = map.ty(b.local_ty[l]);
            out.local_name[n] = b.local_name[l];
            out.local_syn[n] = b.local_syn[l];
            out.local_flags[n] = b.local_flags[l];
        }
        for (k, &l) in self.labels[r].iter().enumerate() {
            let at = b.label_inst[l as usize];
            out.label_inst[x.label_base as usize + k] =
                self.inst(c, at).ok_or_else(|| bad("label target", at))?;
        }
        for (k, &s) in self.subs[r].iter().enumerate() {
            let n = x.sub_base as usize + k;
            // A sub-body's root is its own instruction, in its region,
            // which `c` copies.
            let root = b.sub_root[s as usize] as usize;
            out.sub_root[n] = self.newpos[c as usize][self.inst_pos[root] as usize];
            out.sub_flags[n] = b.sub_flags[s as usize] & !sub_flags::GENERIC;
            let p = self.parent[s as usize];
            // A plain sub-body's parent shares its region; a copy's head's
            // parent is in the declaring context's region, where the
            // context loop found its declaration.
            out.sub_parent[n] = SubId::from_raw(if s == 0 {
                0
            } else if s as usize == r {
                self.sub_in(x.parent, p)
            } else {
                self.sub_in(c, p)
            });
            let params = b.sub_param_locals(s as usize);
            if params.is_empty() {
                out.sub_params[n] = Range32 {
                    start: b.sub_params[s as usize].start,
                    len: 0,
                };
            } else {
                let mut words = Vec::with_capacity(params.len());
                for &l in params {
                    words.push(self.local(c, l)?);
                }
                let at = push_record(out, &words);
                out.sub_params[n] = Range32 {
                    start: at,
                    len: u32_of(words.len()),
                };
            }
        }
        // Instructions: columns and records first, then every word.
        for (k, &i) in self.insts[r].iter().enumerate() {
            let (n, i) = (self.newpos[c as usize][k] as usize, i as usize);
            let tag = b.tags[i];
            out.tags[n] = tag;
            out.ty[n] = map.ty(b.ty[i]);
            out.syn[n] = b.syn[i];
            out.anchors[n] = b.anchor(u32_of(i));
            let (oa, ob) = tag.operands();
            let mut data = b.data[i];
            for (slot, op) in [oa, ob].into_iter().enumerate() {
                let w = data[slot];
                if w == NONE {
                    continue;
                }
                data[slot] = match op {
                    Op::Place if w & place::BARE_LOCAL != 0 => {
                        place::BARE_LOCAL | self.local(c, w & !place::BARE_LOCAL)?
                    }
                    Op::Place => {
                        let at = copy_record(out, b, &mut records, w);
                        // A local root is renumbered and a global root's type
                        // mapped here; a value, frame or pointer root's word
                        // is an instruction, which `slots` renumbers below.
                        let root = out.extra[at as usize + 1];
                        if root == place::ROOT_LOCAL && !done_at(&mut done, at as usize + 2) {
                            let l = out.extra[at as usize + 2];
                            out.extra[at as usize + 2] = self.local(c, l)?;
                        } else if root == place::ROOT_GLOBAL && !done_at(&mut done, at as usize + 3)
                        {
                            out.extra[at as usize + 3] = map.ty(Ty(out.extra[at as usize + 3])).0;
                        }
                        at
                    }
                    Op::Label => {
                        find(&self.labels[r], w, x.label_base).ok_or_else(|| bad("label", w))?
                    }
                    Op::Meta if tag == Tag::Closure => {
                        if b.is_instantiation(i) {
                            // ruled out: the context loop gives each
                            // instantiation in each context's region a target.
                            self.ctxs[self.target[&(c, u32_of(i))] as usize].sub_base
                        } else {
                            self.sub(c, w).ok_or_else(|| bad("sub-body", w))?
                        }
                    }
                    Op::Record if tag == Tag::Closure => {
                        let rec = b.record(w);
                        let (start, count) = (rec[0], rec[1]);
                        // Each closure its own rows: the passes read a
                        // capture's row as that closure's use.
                        let first = u32_of(out.cap_local.len());
                        for j in start..start + count {
                            let l = self.local(c, b.cap_local[j as usize].raw())?;
                            out.cap_local.push(LocalId::from_raw(l));
                            // The checker's `Move` assumed the one
                            // defining closure; each instantiation
                            // makes an environment, so it copies.
                            let mode = b.cap_mode[j as usize];
                            out.cap_mode.push(
                                if b.is_instantiation(i) && mode == super::CaptureMode::Move {
                                    super::CaptureMode::Copy
                                } else {
                                    mode
                                },
                            );
                        }
                        push_record(out, &[first, count])
                    }
                    Op::Record if tag == Tag::Block => {
                        let mut members = Vec::new();
                        for &m in b.record(w) {
                            if let Some(new) = self.inst(c, m) {
                                members.push(new);
                            } else if let Some(&t) = self.generic_roots.get(&m) {
                                // A generic function's root lists its
                                // copies made here.
                                for &k in self.children.get(&(c, t)).map_or(&[][..], Vec::as_slice)
                                {
                                    // Copy `k` copies `t`'s region, which
                                    // holds its root `m`.
                                    members.push(
                                        self.newpos[k as usize][self.inst_pos[m as usize] as usize],
                                    );
                                }
                            } else if !self.defining.contains(&m) {
                                return Err(bad("block member", m));
                            }
                        }
                        let at = push_record(out, &members);
                        mark(&mut done, at as usize, members.len() + 1);
                        at
                    }
                    Op::Values | Op::Blocks | Op::Record => {
                        let at = copy_record(out, b, &mut records, w);
                        if let Some(footer) = b.call_footer(i)
                            && slot == 1
                            && let super::Providers::Pairs(p) = b.call_providers(i)
                            && p.len > 0
                        {
                            let foot = at as usize + (footer - w as usize);
                            if !done_at(&mut done, foot + 1) {
                                let start = u32_of(out.extra.len());
                                for q in 0..p.len {
                                    let word = b.extra[(p.start + q) as usize];
                                    out.extra.push(if q % 2 == 0 {
                                        map.ty(Ty(word)).0
                                    } else {
                                        word
                                    });
                                }
                                out.extra[foot + 1] = start;
                            }
                        }
                        at
                    }
                    Op::Meta if matches!(tag, Tag::ItemRef | Tag::ProviderGet) => {
                        copy_record(out, b, &mut records, w)
                    }
                    _ => w,
                };
            }
            out.data[n] = data;
            // The type words of its records.
            if !bind.is_empty() {
                let mut words = Vec::new();
                super::inst_record_type_words(out, n, &mut |at, word| words.push((at, word)));
                for (at, word) in words {
                    if done_at(&mut done, at) {
                        continue;
                    }
                    let w = out.extra[at];
                    out.extra[at] = match word {
                        super::TypeWord::Ty => map.ty(Ty(w)).0,
                        super::TypeWord::List => map.list(TyList(w)).0,
                    };
                }
            }
        }
        // Every instruction and constant word.
        for k in 0..self.insts[r].len() {
            let n = self.newpos[c as usize][k] as usize;
            let mut locs = Vec::new();
            slots(out, n, |loc, w, _| locs.push((loc, w)));
            for (loc, w) in locs {
                if w == NONE {
                    continue;
                }
                if let Loc::Extra(p) = loc
                    && done_at(&mut done, p as usize)
                {
                    continue;
                }
                let new = if w & Ref::CONST_BIT != 0 {
                    let old = w & !Ref::CONST_BIT;
                    let (t, bits) = b.consts[old as usize];
                    let u = map.ty(t);
                    Ref::CONST_BIT
                        | if u == t {
                            old
                        } else {
                            *consts.entry(old).or_insert_with(|| {
                                out.consts.push((u, bits));
                                u32_of(out.consts.len() - 1)
                            })
                        }
                } else {
                    self.inst(c, w).ok_or_else(|| bad("instruction", w))?
                };
                match loc {
                    Loc::Data(d) => out.data[n][usize::from(d)] = new,
                    Loc::Extra(p) => out.extra[p as usize] = new,
                }
            }
        }
        // Side rows.
        let side = &self.side[r];
        for &k in &side.fallback {
            let [i, l] = b.fallback[k as usize];
            if let Some(n) = self.inst(c, i) {
                let l = if l == 0 { 0 } else { self.local(c, l - 1)? + 1 };
                out.fallback.push([n, l]);
            }
        }
        for &k in &side.held {
            let (call, arg) = b.held_args[k as usize];
            if let (Some(call), Some(arg)) = (self.inst(c, call), self.inst(c, arg)) {
                out.held_args.push((call, arg));
            }
        }
        for &k in &side.susp {
            let row = &b.susp[k as usize];
            let Some(inst) = self.inst(c, row.inst) else {
                continue;
            };
            let (s, len) = (row.scopes.start as usize, row.scopes.len as usize);
            let mut words = Vec::with_capacity(len);
            for &w in &b.extra[s..s + len] {
                words.push(self.inst(c, w).ok_or_else(|| bad("scope", w))?);
            }
            let at = push_record(out, &words);
            out.susp.push(super::SuspRow {
                inst,
                scopes: Range32 {
                    start: at,
                    len: u32_of(len),
                },
                hook_site: if row.hook_site == NONE {
                    NONE
                } else {
                    self.inst(c, row.hook_site).unwrap_or(NONE)
                },
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{parse, verify_structure};

    /// Leaves every type as it is: the cases check numbering, not types.
    struct Same;

    impl LocalTypes for Same {
        fn ty(&mut self, _: u32, _: &[(u32, Ty)], t: Ty) -> Ty {
            t
        }
    }

    /// The checker's body of
    /// `fn a[S](s: S) -> S: (fn b[U](u: U) -> U: u); b(s)` then
    /// `println(a(1))`, with its type-argument lists `inner` (at `b`'s
    /// instantiation) and `outer` (at `a`'s).
    fn nested(inner: TyList, outer: TyList) -> String {
        format!(
            "body 1 Fn
local 4484 2213 11 33
local 4486 405 7 66
local 4489 3086 19 33
local 4491 406 15 66
sub 11 0 0 - 0
sub 6 49 1 0 1
sub 1 51 1 0 1
labels
caps
modes
extra 1 0 2 0 0 4 0 0 1 {} 4 4 0 0 0 5 1 2 3 4 5 2 0 0 4 0 0 0 {} 4 2147483648 0 0 0 3 0 1009 53 4 9 0 0 0 5 6 7 8 9 10 1 0 1 2
const 8 1
%0 = Read 2147483650 0 : 4489 @24
%1 = Block 0 0 : 4489 @15
%2 = Closure 2 2 : 4491 @15
%3 = Closure 2 5 : 4486 @27
%4 = Read 2147483648 0 : 4484 @30
%5 = CallValue 3 10 : 4484 @26
%6 = Block 15 5 : 4484 @7
%7 = Closure 1 21 : 4486 @7
%8 = Closure 1 24 : 4497 @37
%9 = CallValue 8 29 : 8 @36
%10 = Call 34 38 : 18 @32
%11 = Block 43 - : 18 @6
",
            inner.0, outer.0
        )
    }

    /// `fn a[S](s: S) -> S: s`, `fn b[U](u: U) -> U: u`, then
    /// `println(a(1) + b(2))`.
    fn siblings(a: TyList, b: TyList) -> String {
        format!(
            "body 1 Fn
local 4484 2213 11 33
local 4486 405 7 66
local 4489 3194 21 33
local 4491 406 17 66
sub 12 0 0 - 0
sub 1 53 1 0 1
sub 4 55 1 0 1
labels
caps
modes
extra 1 0 2 0 0 1 3 2 0 0 4 0 0 0 {} 4 2147483648 0 0 0 4 0 0 1 {} 4 2147483649 0 0 0 2 7 9 3 0 1009 53 4 10 0 0 0 10 1 2 4 5 6 7 8 9 10 11 1 0 1 2
fallback 10 0
const 8 1
const 8 2
%0 = Read 2147483648 0 : 4484 @16
%1 = Block 0 0 : 4484 @7
%2 = Closure 1 2 : 4486 @7
%3 = Read 2147483650 0 : 4489 @26
%4 = Block 5 3 : 4489 @17
%5 = Closure 2 7 : 4491 @17
%6 = Closure 1 10 : 4495 @34
%7 = CallValue 6 15 : 8 @33
%8 = Closure 2 20 : 4495 @39
%9 = CallValue 8 25 : 8 @38
%10 = Prim 0 30 : 8 @32
%11 = Call 33 37 : 18 @28
%12 = Block 42 - : 18 @6
",
            a.0, b.0
        )
    }

    fn run(text: &str, pool: &InternPool) -> Result<Option<(Body, usize)>, String> {
        let b = parse(text).expect("parses");
        instantiate_local_functions(&b, pool, &mut Same)
    }

    #[test]
    fn nested_and_sibling_functions_get_one_plain_sub_body_each() {
        let pool = InternPool::new();
        let one = pool.list(&[Ty::I32]);
        for text in [nested(one, one), siblings(one, one)] {
            let (out, made) = run(&text, &pool).expect("instantiates").expect("has some");
            assert_eq!(made, 2);
            assert!((0..out.sub_root.len()).all(|s| !out.sub_is_generic(s)));
            assert!((0..out.len()).all(|i| !out.is_instantiation(i)));
            // Each copy keeps its instruction's anchor, as the inliner's.
            let errs = verify_structure(&out, &Topology::new(&out));
            assert_eq!(errs, Vec::new(), "{}", crate::ir::print(&out));
        }
    }

    #[test]
    fn a_body_without_generic_functions_is_left_alone() {
        let pool = InternPool::new();
        let one = pool.list(&[Ty::I32]);
        let mut b = parse(&siblings(one, one)).expect("parses");
        b.sub_flags
            .iter_mut()
            .for_each(|f| *f &= !sub_flags::GENERIC);
        assert_eq!(instantiate_local_functions(&b, &pool, &mut Same), Ok(None));
    }

    /// `b`'s instantiation moved into the root, outside `a` where `b` is
    /// declared.
    #[test]
    fn an_instantiation_outside_its_declaring_function_is_refused() {
        let pool = InternPool::new();
        let one = pool.list(&[Ty::I32]);
        let text = nested(one, one).replace("%8 = Closure 1 24", "%8 = Closure 2 24");
        assert_eq!(
            run(&text, &pool),
            Err("an instantiation of sub-body 2 outside its scope".to_owned())
        );
    }

    /// `b` reads `a`'s parameter: a sibling's local is in neither scope.
    #[test]
    fn a_sibling_functions_local_is_refused() {
        let pool = InternPool::new();
        let one = pool.list(&[Ty::I32]);
        let text = siblings(one, one).replace("%3 = Read 2147483650", "%3 = Read 2147483648");
        assert_eq!(
            run(&text, &pool),
            Err("local 0 outside its scope".to_owned())
        );
    }

    /// `a` breaks to a label of the root, and a label targets `a`'s
    /// defining closure, which no copy keeps.
    #[test]
    fn labels_of_another_context_are_refused() {
        let pool = InternPool::new();
        let one = pool.list(&[Ty::I32]);
        let text = siblings(one, one)
            .replace("labels\n", "labels 12\n")
            .replace("%0 = Read 2147483648 0 : 4484", "%0 = Break 0 - : 4484");
        assert_eq!(
            run(&text, &pool),
            Err("label 0 outside its context".to_owned())
        );
        let text = siblings(one, one).replace("labels\n", "labels 2\n");
        assert_eq!(
            run(&text, &pool),
            Err("label target 2 outside its context".to_owned())
        );
    }

    /// A root suspension row whose scope is `a`'s instruction.
    #[test]
    fn a_suspension_scope_in_another_context_is_refused() {
        let pool = InternPool::new();
        let one = pool.list(&[Ty::I32]);
        // Its one scope word is `extra[1]`, which names %0.
        let text = siblings(one, one)
            .replace("fallback 10 0\n", "fallback 10 0\nsusp 11 1 1 4294967295\n");
        assert_eq!(
            run(&text, &pool),
            Err("scope 0 outside its context".to_owned())
        );
    }

    /// A plain closure in `a` names the root's sub-body.
    #[test]
    fn a_closure_of_another_contexts_sub_body_is_refused() {
        let pool = InternPool::new();
        let one = pool.list(&[Ty::I32]);
        let text =
            siblings(one, one).replace("%0 = Read 2147483648 0 : 4484", "%0 = Closure 0 2 : 4484");
        assert_eq!(
            run(&text, &pool),
            Err("sub-body 0 outside its context".to_owned())
        );
    }

    /// The root's block lists `a`'s parameter read, and a call takes it.
    #[test]
    fn words_naming_another_contexts_instruction_are_refused() {
        let pool = InternPool::new();
        let one = pool.list(&[Ty::I32]);
        let member =
            siblings(one, one).replace(" 10 1 2 4 5 6 7 8 9 10 11 ", " 10 0 2 4 5 6 7 8 9 10 11 ");
        assert_eq!(
            run(&member, &pool),
            Err("block member 0 outside its context".to_owned())
        );
        let operand = siblings(one, one).replace("%7 = CallValue 6 15", "%7 = CallValue 0 15");
        assert_eq!(
            run(&operand, &pool),
            Err("instruction 0 outside its context".to_owned())
        );
    }
}
