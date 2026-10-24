//! Borrow (`docs/compiler/tir-v2.md`, "Borrow"): a `copy` read whose
//! value can live on its place's own count reads with mode `borrow`: it
//! takes no count, and its `Drop`s go.
//!
//! A read borrows when, over its **span**, all of these hold. The span is
//! the statements of the block that holds the read's `Drop` (insertion put
//! it after the last use, in the read's home block), from the one that
//! holds the read to the drop, with every block they run. A statement's
//! blocks precede it in the columns but not always in their order (a
//! match's arms precede its decision tree), so the span is checked as the
//! range of instructions from the first of any of those statements.
//!
//! 1. **Every use lends**: no use consumes the value (an owned argument, a
//!    construction's part, a `Return` or `Break` value, a block's tail),
//!    passes it on as another value (a coercion), or addresses its storage
//!    (a `Ref` rooted at it). A copy of it for a consumer is a read rooted
//!    at it, which lends. One consumer is let through: a `Store` of the
//!    read into all of a **binding local** (below).
//! 2. **Nothing writes what it reads.** The place is resolved through the
//!    values its root reads (`%h = Read [$xs]` makes `[%h [%i]]` an
//!    element of `$xs`), and through binding locals to the read they hold:
//!    a chain of roots, each with the chain's place below it. No `Store`,
//!    `Drop`, `move` read or mutable `Ref` whose own place, resolved the
//!    same way, overlaps that place at one of its roots stands in the
//!    span. A write that goes into a list's buffer or behind a reference
//!    keeps the list header or the reference before that step, so it
//!    overlaps only a place that goes past it; a write through the read's
//!    own value (an element store through a borrowed header) keeps that
//!    value. Each read in the chain also refuses what the verifier's
//!    overlap counts against its own place (`crate::ir::verify_owned`): a
//!    write of the same root, `IsUnique` included, and, for a place behind
//!    a reference or in a buffer, any write a reference can reach.
//! 3. **Nothing reaches it behind the body's back.** A chain ending at a
//!    local needs the local unaddressed and unshared with a closure; then
//!    only the body's own instructions change it, calls included, and so
//!    the buffer of a list it holds, when every value stored into it is a
//!    fresh one (a call's result, a copy or snapshot, a literal) and so
//!    shares its header with no other place. Any other place (behind a
//!    dereference, in a top-level binding, or in the buffer of a shared or
//!    unknown header) is the **heap**: no write that reaches the heap, and
//!    no call, may stand in the span; a call that may not write
//!    (`OwnCx::may_write`) is allowed when the one dereference is of a
//!    reference that a parameter the body never stores holds.
//! 4. **No suspension point** stands in the span.
//!
//! A **binding local** is a local whose one `Store` is of a read that
//! borrows: a loop's element binding (`for word in words`), a `:=` of a
//! field. It is not a parameter, never addressed, captured or changed in
//! place, no read of it is consumed (that read would copy, where the
//! binding could take the value), and every read of it lies in the span,
//! which ends at its last `Drop`. A read of it that another binding local
//! holds, decided first, is not consumed: an arm precedes its match's
//! bindings, so in `for part in parts: n += part.len()` the inlined `len`'s
//! `self` borrows `part`, and then `part` borrows the element. The inner
//! span lies in the outer one, since the outer's `Drop` in it would have
//! stopped the inner borrow, so the outer's check covers both.
//!
//! A binding local holds a borrowed value, so it owns nothing: its `Drop`s
//! go, it leaves the exit set, and its reads resolve through it to the
//! read it holds. The backend stores into it with no count
//! (`Moves::loan_locals`).
//!
//! A **lent parameter** ([`OwnCx::received`]) that the body never stores,
//! addresses, captures or changes in place (an `IsUnique` gives up a count
//! of the buffer it replaces, which must be the body's) is the caller's for
//! the whole call: the body owns none of it. Its `Drop`s go, it leaves the exit set, and a read of it is
//! limited by nothing in the body. The backend takes no count of it on
//! entry, and releases at the exit one an instance receives owned. An
//! owned parameter of a body that does not suspend is left so too when no
//! read of it keeps its value (every use lends and no store takes it): its
//! count is then released once at the exit (§23.31's parameter loans), not
//! after its last read, and a read of it is not cut short by its own drop.
//!
//! A **lent reference** (`ref_word::LENT`) is a `Ref` of a field of what
//! such a parameter refers to (`[*$p .f]`, or through a borrowed read of
//! all of `$p`) whose every use lends: it points into the block the
//! caller's reference keeps alive for the call, so it takes no count; its
//! `Drop`s stay for an instance whose reference view makes the referent
//! fresh.
//!
//! A read whose value an instance may have to make (one through a
//! dereference, which a view's getter makes fresh; one that names a type
//! parameter, which an erased body's thunk makes) keeps its `Drop`s: in
//! such an instance the read holds that fresh value's count, and the drop
//! releases it; where the read borrows, the drop releases nothing.
//!
//! The decisions run in instruction order, so the reads a chain goes
//! through are decided first. Every write, call and suspension point is
//! indexed once, by the root it writes (resolved the same way), in rows
//! sorted by instruction; a span's check is a binary search per row.
//! Nothing is spliced here: the `Drop`s that go are recorded as edits for
//! the pipeline's one rebuild, and the move pass after this one reads the
//! body with them still in place.

use hd_types::Ty;

use super::splice::for_blocks;
use super::{Loc, OwnCx, Pass, Role, Tri, consumes, operands, splice};
use crate::ir::{
    Body, BodyKind, Callee, IntrinsicOp, NONE, ReadMode, Ref, Tag, is_suspension, local_flags,
    place,
};

/// A projection step's code in a path: a field (its index), a variant
/// (its index with [`VARIANT`]), an element (any index) or a dereference.
const ELEM: u32 = NONE - 1;
const DEREF: u32 = NONE - 2;
const VARIANT: u32 = 1 << 30;

/// The longest chain of roots a read is resolved through.
const DEPTH: usize = 8;

/// Whether places with step codes `a` and `c` below one root overlap: one
/// is a prefix of the other (element indices are not compared).
fn overlap(a: &[u32], c: &[u32]) -> bool {
    a.iter().zip(c).all(|(x, y)| x == y)
}

/// One write at one root: where, its place below that root (`len` codes of
/// `Tables::wpaths` from `start`), the last step there that goes into a
/// buffer or behind a reference (`cut`, or `NONE`), whether it changes
/// nothing a value holds (`IsUnique`, `Pin`), and whether the root is the
/// write's own (not one its place is resolved through).
///
/// A write replaces the value at its place. One whose place goes into a
/// list's buffer or behind a reference at step `cut` keeps what the place
/// before that step holds, a list header or a reference: a read of a
/// place no longer than `cut` keeps its value.
#[derive(Clone, Copy, Debug)]
struct Write {
    at: u32,
    start: u32,
    len: u32,
    cut: u32,
    keep: bool,
    direct: bool,
}

/// One root of a chain: a local (`key` < the local count) or a value
/// (`key` = local count + value); the place read at it, the read's own
/// projections (the first `own` codes of `Tables::cpath` from `start`),
/// then the chain's place below them (`len` codes in all); and whether the
/// read's own place is behind a reference or in a buffer, or a top-level
/// binding's (the verifier's reachable places).
#[derive(Clone, Copy)]
struct Link {
    key: u32,
    start: u32,
    own: u32,
    len: u32,
    behind: bool,
    global: bool,
}

/// A resolved chain ([`Tables::walk`]).
#[derive(Clone, Copy, Default)]
struct Chain {
    /// Resolved to a root the pass can reason about.
    ok: bool,
    /// What the place names may be reached by calls and by writes through
    /// references (rule 3's heap).
    heap: bool,
    /// The one dereference is of a reference a never-stored parameter
    /// holds, read whole: calls that may not write leave it alone.
    param: bool,
}

/// The borrow pass's dense tables, reused across bodies.
#[derive(Default)]
pub(crate) struct Tables {
    /// Each instruction's block, each block's owner, and each
    /// instruction's first instruction (its own index, or the first of the
    /// blocks it runs, which precede it).
    blk: Vec<u32>,
    pos: Vec<u32>,
    owner: Vec<u32>,
    first: Vec<u32>,
    /// Per value: its uses, whether a use keeps it (rule 1), the `Store`
    /// that takes it, its last `Drop` of all of it.
    uses: Vec<u32>,
    keeps: Vec<bool>,
    stored: Vec<u32>,
    last_drop: Vec<u32>,
    /// Per local: the stores into it, whether a reference or a closure
    /// reaches it, whether an `IsUnique` or a `Pin` changes it in place,
    /// whether every value stored into it is fresh, the first and last read
    /// of it, its last `Drop`, the read a binding local holds, and whether
    /// it left the exit set.
    stores: Vec<u32>,
    reached: Vec<bool>,
    changed: Vec<bool>,
    captured: Vec<bool>,
    fresh: Vec<bool>,
    first_read: Vec<u32>,
    last_read: Vec<u32>,
    last_ldrop: Vec<u32>,
    holds: Vec<u32>,
    unowned: Vec<bool>,
    /// Per local: the reads of it of a counted type whose value a use
    /// keeps or a store takes, less those a binding local borrows.
    kept_read: Vec<u32>,
    /// Per local: a closure's parameter.
    inner: Vec<bool>,
    /// Writes by root (locals, then values), in compressed rows in
    /// instruction order.
    start: Vec<u32>,
    next: Vec<u32>,
    rows: Vec<Write>,
    pending: Vec<(u32, Write)>,
    /// Writes that reach the heap, with the value their place is rooted at
    /// (or `NONE`): one through a read's own value keeps that value, a
    /// list header or a reference, as it is; writes the verifier's overlap
    /// counts
    /// as reachable, and whether each is behind a reference or in a
    /// buffer; calls with whether each may write; suspension points.
    heap: Vec<(u32, u32)>,
    reach: Vec<(u32, bool, u32)>,
    calls: Vec<(u32, bool)>,
    susp: Vec<u32>,
    /// Per instruction: a `Drop` this pass deletes.
    gone: Vec<bool>,
    links: Vec<Link>,
    /// The step codes of the chain being walked, and of every write.
    cpath: Vec<u32>,
    wpaths: Vec<u32>,
    ops: Vec<super::Operand>,
}

fn fill<T: Clone>(v: &mut Vec<T>, n: usize, x: T) {
    v.clear();
    v.resize(n, x);
}

fn u32_of(i: usize) -> u32 {
    u32::try_from(i).expect("insts")
}

/// What a place starts at.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Root {
    Local(u32),
    Value(u32),
    Global(Ty),
    Other,
}

/// The root of the place at `word`, and the `extra` range of its
/// projection words (empty for a bare local).
fn root_of(b: &Body, word: u32) -> (Root, u32, u32) {
    if word & place::BARE_LOCAL != 0 {
        return (Root::Local(word & !place::BARE_LOCAL), 0, 0);
    }
    let at = word as usize;
    let (Some(&n), Some(&kind), Some(&id)) =
        (b.extra.get(at), b.extra.get(at + 1), b.extra.get(at + 2))
    else {
        return (Root::Other, 0, 0);
    };
    let Some(end) = word.checked_add(1).and_then(|x| x.checked_add(n)) else {
        return (Root::Other, 0, 0);
    };
    match kind {
        place::ROOT_LOCAL => (Root::Local(id), word + 3, end),
        place::ROOT_VALUE if Ref(id).as_inst().is_some() && (id as usize) < b.len() => {
            (Root::Value(id), word + 3, end)
        }
        place::ROOT_GLOBAL => match b.extra.get(at + 3) {
            Some(&ty) => (Root::Global(Ty(ty)), word + 4, end),
            None => (Root::Other, 0, 0),
        },
        _ => (Root::Other, 0, 0),
    }
}

/// Appends the codes of the projection words `from..to` to `out`; how many
/// dereferences there are, whether the first step is one, and whether an
/// element step is among them.
fn push_steps(b: &Body, from: u32, to: u32, out: &mut Vec<u32>) -> (u32, bool, bool) {
    let (mut derefs, mut elem) = (0, false);
    let first_deref = from < to && b.extra[from as usize] & place::PROJ_KIND == place::PROJ_DEREF;
    let mut x = from;
    while x < to {
        let w = b.extra[x as usize];
        out.push(match w & place::PROJ_KIND {
            place::PROJ_FIELD => w & place::PROJ_INDEX,
            place::PROJ_VARIANT => VARIANT | (w & place::PROJ_INDEX),
            place::PROJ_DEREF => {
                derefs += 1;
                DEREF
            }
            _ => {
                elem = true;
                x += 1;
                ELEM
            }
        });
        x += 1;
    }
    (derefs, first_deref, elem)
}

/// How many dereferences the projection words `from..to` hold, and
/// whether an element step is among them.
fn kinds(b: &Body, from: u32, to: u32) -> (u32, bool) {
    let (mut derefs, mut elem) = (0, false);
    let mut x = from;
    while x < to {
        match b.extra[x as usize] & place::PROJ_KIND {
            place::PROJ_DEREF => derefs += 1,
            place::PROJ_ELEM => {
                elem = true;
                x += 1;
            }
            _ => {}
        }
        x += 1;
    }
    (derefs, elem)
}

impl Tables {
    fn reset(&mut self, n: usize, locals: usize) {
        fill(&mut self.blk, n, NONE);
        fill(&mut self.pos, n, 0);
        fill(&mut self.owner, n, NONE);
        fill(&mut self.first, n, 0);
        fill(&mut self.uses, n, 0);
        fill(&mut self.keeps, n, false);
        fill(&mut self.stored, n, NONE);
        fill(&mut self.last_drop, n, NONE);
        fill(&mut self.stores, locals, 0);
        fill(&mut self.reached, locals, false);
        fill(&mut self.changed, locals, false);
        fill(&mut self.captured, locals, false);
        fill(&mut self.fresh, locals, true);
        fill(&mut self.first_read, locals, NONE);
        fill(&mut self.last_read, locals, 0);
        fill(&mut self.last_ldrop, locals, NONE);
        fill(&mut self.holds, locals, NONE);
        fill(&mut self.unowned, locals, false);
        fill(&mut self.gone, n, false);
        self.pending.clear();
        self.rows.clear();
        self.heap.clear();
        self.reach.clear();
        self.calls.clear();
        self.susp.clear();
        self.links.clear();
        self.cpath.clear();
        self.wpaths.clear();
    }

    /// Resolves the place at `word` into `self.links`: its root, then
    /// while the root is a value that a `Read` made, that read's root, and
    /// with `follow`, while it is a binding local, the root of the read it
    /// holds.
    fn walk(&mut self, b: &Body, word: u32, follow: bool) -> Chain {
        self.links.clear();
        self.cpath.clear();
        self.walk_on(b, word, follow, 0, 0)
    }

    /// [`Tables::walk`] from the place at `word`, with the codes
    /// `below_start..` (`below` of them) of `self.cpath` below it.
    fn walk_on(
        &mut self,
        b: &Body,
        word: u32,
        follow: bool,
        below_start: u32,
        below: u32,
    ) -> Chain {
        let locals = u32_of(b.local_ty.len());
        let mut chain = Chain::default();
        let (mut word, mut below_start, mut below) = (word, below_start, below);
        let (mut derefs, mut elem, mut first_deref) = (0, false, false);
        // A projection between the dereference and the chain's end.
        let (mut seen_deref, mut above) = (false, false);
        for _ in 0..DEPTH {
            let (root, from, to) = root_of(b, word);
            let start = u32_of(self.cpath.len());
            let (d, first, e) = push_steps(b, from, to, &mut self.cpath);
            let own = u32_of(self.cpath.len()) - start;
            self.cpath
                .extend_from_within(below_start as usize..(below_start + below) as usize);
            if seen_deref && own > 0 {
                above = true;
            }
            if d > 0 {
                seen_deref = true;
                first_deref = first;
            }
            derefs += d;
            elem |= e;
            let link = Link {
                key: NONE,
                start,
                own,
                len: own + below,
                behind: d > 0 || e,
                global: false,
            };
            (below_start, below) = (start, own + below);
            match root {
                Root::Local(l) => {
                    self.links.push(Link { key: l, ..link });
                    let held = self.holds[l as usize];
                    if follow && held != NONE {
                        word = b.data[held as usize][0];
                        continue;
                    }
                    let li = l as usize;
                    let param = b.local_flags[li] & local_flags::PARAM != 0;
                    chain.ok = !self.reached[li];
                    chain.heap = derefs > 0 || (elem && (param || !self.fresh[li]));
                    chain.param =
                        derefs == 1 && first_deref && !above && param && self.stores[li] == 0;
                    return chain;
                }
                Root::Value(v) => {
                    self.links.push(Link {
                        key: locals + v,
                        ..link
                    });
                    if b.tags[v as usize] == Tag::Read {
                        word = b.data[v as usize][0];
                        continue;
                    }
                    chain.ok = true;
                    chain.heap = derefs > 0 || elem;
                    return chain;
                }
                Root::Global(_) => {
                    self.links.push(Link {
                        global: true,
                        ..link
                    });
                    chain.ok = true;
                    chain.heap = true;
                    return chain;
                }
                Root::Other => return chain,
            }
        }
        chain
    }

    /// The links of the collection value `c` that a map's change writes
    /// in: `[%c [..]]`, and through what `c` reads.
    fn walk_collection(&mut self, b: &Body, c: u32) -> Chain {
        let locals = u32_of(b.local_ty.len());
        self.links.clear();
        self.cpath.clear();
        self.cpath.push(ELEM);
        self.links.push(Link {
            key: locals + c,
            start: 0,
            own: 1,
            len: 1,
            behind: true,
            global: false,
        });
        if b.tags[c as usize] == Tag::Read {
            return self.walk_on(b, b.data[c as usize][0], false, 0, 1);
        }
        Chain {
            ok: true,
            heap: true,
            param: false,
        }
    }

    /// Records a write at every link of the chain just walked.
    fn write_links(&mut self, at: u32, keep: bool) {
        for k in 0..self.links.len() {
            let link = self.links[k];
            if link.key == NONE {
                continue;
            }
            let path = &self.cpath[link.start as usize..(link.start + link.len) as usize];
            let cut = path
                .iter()
                .rposition(|&c| c == ELEM || c == DEREF)
                .map_or(NONE, u32_of);
            let start = u32_of(self.wpaths.len());
            self.wpaths.extend_from_slice(path);
            self.pending.push((
                link.key,
                Write {
                    at,
                    start,
                    len: link.len,
                    cut,
                    keep,
                    direct: k == 0,
                },
            ));
        }
    }

    /// The writes at root `key` in `lo..=hi`.
    fn row(&self, key: u32, lo: u32, hi: u32) -> &[Write] {
        let row =
            &self.rows[self.start[key as usize] as usize..self.start[key as usize + 1] as usize];
        let a = row.partition_point(|w| w.at < lo);
        let z = row.partition_point(|w| w.at <= hi);
        &row[a..z.max(a)]
    }
}

/// Whether a sorted list of positions has one in `lo..=hi`.
fn any_in(list: &[u32], lo: u32, hi: u32) -> bool {
    let k = list.partition_point(|&x| x < lo);
    list.get(k).is_some_and(|&x| x <= hi)
}

/// Runs the borrow pass over every sub-body of `b`; the `Drop`s it deletes
/// go to `edits`, which the pipeline applies after the move pass.
pub(crate) fn run(b: &mut Body, cx: &dyn OwnCx, t: &mut Tables, edits: &mut splice::Tables) {
    edits.clear();
    let n = b.len();
    let locals = b.local_ty.len();
    t.reset(n, locals);
    scan(b, cx, t);
    // Per local: the reads of it whose value a use keeps or a store takes.
    fill(&mut t.kept_read, locals, 0);
    for i in 0..n {
        if b.tags[i] == Tag::Read
            && let (Root::Local(l), _, _) = root_of(b, b.data[i][0])
            && (t.keeps[i] || t.stored[i] != NONE)
            && cx.counted(b.ty[i]) != Tri::No
        {
            t.kept_read[l as usize] += 1;
        }
    }
    index_writes(b, cx, t);
    lent_params(b, cx, t, edits);
    for r in 0..n {
        decide(b, cx, t, edits, r);
    }
    lent_refs(b, t);
    if t.unowned.iter().any(|&u| u) {
        let unowned = &t.unowned;
        b.exit_drops.retain(|(_, l)| !unowned[l.idx()]);
    }
}

/// The per-value and per-local facts: uses, drops, stores, reads.
fn scan(b: &Body, cx: &dyn OwnCx, t: &mut Tables) {
    let n = b.len();
    let mut ops = std::mem::take(&mut t.ops);
    for i in 0..n {
        let [a, w] = b.data[i];
        let tag = b.tags[i];
        let mut f = u32_of(i);
        if tag == Tag::Block {
            for (p, &m) in b.record(a).iter().enumerate() {
                if (m as usize) < n {
                    t.blk[m as usize] = u32_of(i);
                    t.pos[m as usize] = u32_of(p);
                    f = f.min(t.first[m as usize]);
                }
            }
        }
        // A cleanup scope owns its suites, not the `Defer` that registers
        // one.
        let defer = tag == Tag::Defer;
        let (owner, first) = (&mut t.owner, &t.first);
        for_blocks(b, i, |blk| {
            if (blk as usize) < i {
                f = f.min(first[blk as usize]);
            }
            if !(defer && owner[blk as usize] != NONE) {
                owner[blk as usize] = u32_of(i);
            }
        });
        t.first[i] = f;
        match tag {
            Tag::Drop => match root_of(b, a) {
                (Root::Value(v), from, to) if from == to => t.last_drop[v as usize] = u32_of(i),
                (Root::Local(l), from, to) if from == to => t.last_ldrop[l as usize] = u32_of(i),
                _ => {}
            },
            // A store into the local itself, not into what it refers to.
            Tag::Store => {
                if let Some(l) = b.local_in_place(a) {
                    let li = l.idx();
                    t.stores[li] += 1;
                    // A fresh value shares its header with no place.
                    let fresh = Ref(w).as_inst().is_none_or(|_| {
                        matches!(
                            b.tags.get(w as usize),
                            Some(
                                Tag::Call
                                    | Tag::CallValue
                                    | Tag::CallHost
                                    | Tag::DefaultCall
                                    | Tag::NewList
                                    | Tag::NewMap
                            )
                        ) || (b.tags.get(w as usize) == Some(&Tag::Intrinsic)
                            && matches!(
                                IntrinsicOp::from_u32(b.data[w as usize][0]),
                                Some(IntrinsicOp::SnapBegin)
                            ))
                    });
                    t.fresh[li] &= fresh;
                }
            }
            Tag::Ref => {
                if let Some(l) = b.local_in_place(a) {
                    t.reached[l.idx()] = true;
                }
            }
            Tag::IsUnique | Tag::Pin => {
                if let Some(l) = b.local_in_place(a) {
                    t.changed[l.idx()] = true;
                }
            }
            Tag::Read => {
                if let (Root::Local(l), _, _) = root_of(b, a) {
                    let li = l as usize;
                    t.first_read[li] = t.first_read[li].min(u32_of(i));
                    t.last_read[li] = u32_of(i);
                }
            }
            Tag::Closure => {
                let rec = b.record(w);
                for c in rec[0] as usize..(rec[0] + rec[1]) as usize {
                    let l = b.cap_local[c].idx();
                    t.captured[l] = true;
                    if b.cap_mode[c] == crate::ir::CaptureMode::Shared {
                        t.reached[l] = true;
                    }
                }
            }
            _ => {}
        }
        operands(b, i, &mut ops);
        for op in &ops {
            let v = op.value as usize;
            if op.role == Role::Dropped {
                continue;
            }
            t.uses[v] += 1;
            let keeps = match op.role {
                Role::Consume if tag == Tag::Store && op.at == Loc::Data(1) => {
                    if t.stored[v] == NONE {
                        t.stored[v] = u32_of(i);
                        false
                    } else {
                        true
                    }
                }
                Role::Consume | Role::Tail => true,
                Role::Arg(_) => consumes(b, cx, i, op, NONE),
                Role::Dropped => false,
                Role::Lend => match tag {
                    Tag::Read
                    | Tag::Store
                    | Tag::IsUnique
                    | Tag::Pin
                    | Tag::Prim
                    | Tag::Intrinsic
                    | Tag::Call
                    | Tag::CallValue
                    | Tag::CallHost
                    | Tag::DefaultCall
                    | Tag::Await
                    | Tag::Interp
                    | Tag::Match
                    | Tag::SwitchTag
                    | Tag::SwitchInt
                    | Tag::SwitchChar
                    | Tag::SwitchStr
                    | Tag::If
                    | Tag::Is
                    | Tag::With
                    | Tag::ContextNew => false,
                    // A reference into the value's own storage outlives
                    // the read; one through it, to a referent, does not.
                    Tag::Ref => {
                        let (_, from, to) = root_of(b, a);
                        !(from < to
                            && b.extra[from as usize] & place::PROJ_KIND == place::PROJ_DEREF)
                    }
                    _ => true,
                },
            };
            t.keeps[v] |= keeps;
        }
    }
    t.ops = ops;
}

/// Indexes every write by the roots it writes, and the heap writes, the
/// reachable writes, the calls and the suspension points.
fn index_writes(b: &Body, cx: &dyn OwnCx, t: &mut Tables) {
    let n = b.len();
    let locals = u32_of(b.local_ty.len());
    for i in 0..n {
        let x = u32_of(i);
        let [a, w] = b.data[i];
        let tag = b.tags[i];
        // The verifier's writes: a `Store`, a `Drop`, an `IsUnique`, a
        // `move` read, a mutable `Ref`.
        let verifier = match tag {
            Tag::Store | Tag::Drop | Tag::IsUnique => true,
            Tag::Read => w == ReadMode::Move as u32,
            Tag::Ref => w & crate::ir::ref_word::MUT != 0,
            _ => false,
        };
        let place = matches!(
            tag,
            Tag::Store | Tag::Drop | Tag::IsUnique | Tag::Pin | Tag::Read | Tag::Ref
        );
        let through = match place.then(|| root_of(b, a).0) {
            Some(Root::Value(v)) => v,
            _ => NONE,
        };
        if verifier {
            let (root, from, to) = root_of(b, a);
            let (derefs, elem) = kinds(b, from, to);
            let behind = derefs > 0 || elem;
            let reachable = behind
                || match root {
                    Root::Local(l) => t.reached[l as usize],
                    Root::Global(_) | Root::Other => true,
                    Root::Value(_) => false,
                };
            // A write of a top-level binding counts against a read of any
            // binding, as one behind a reference does.
            if reachable {
                let global = matches!(root, Root::Global(_));
                t.reach.push((x, behind || global, through));
            }
        }
        match tag {
            Tag::Store | Tag::Ref if tag == Tag::Store || w & crate::ir::ref_word::MUT != 0 => {
                let chain = t.walk(b, a, false);
                t.write_links(x, false);
                let reached = matches!(t.links.last(), Some(l) if l.key < locals && t.reached[l.key as usize]);
                if !chain.ok || chain.heap || reached {
                    t.heap.push((x, through));
                }
            }
            Tag::Read if w == ReadMode::Move as u32 => {
                t.walk(b, a, false);
                t.links.truncate(1);
                t.write_links(x, false);
            }
            Tag::Drop => {
                t.walk(b, a, false);
                t.links.truncate(1);
                t.write_links(x, false);
            }
            Tag::IsUnique | Tag::Pin => {
                t.walk(b, a, false);
                t.links.truncate(1);
                t.write_links(x, true);
            }
            Tag::Intrinsic => match IntrinsicOp::from_u32(a) {
                Some(IntrinsicOp::MapSet | IntrinsicOp::MapRemove) => {
                    if let Some(&c) = b.record(w).first()
                        && Ref(c).as_inst().is_some()
                        && (c as usize) < n
                    {
                        let chain = t.walk_collection(b, c);
                        t.write_links(x, false);
                        if !chain.ok || chain.heap {
                            t.heap.push((x, c));
                        }
                    }
                }
                Some(IntrinsicOp::Item) => t.calls.push((x, true)),
                _ => {}
            },
            Tag::Call => {
                let may = Callee::from_words(b.record(a)).is_none_or(|c| cx.may_write(&c));
                t.calls.push((x, may));
            }
            Tag::CallValue | Tag::CallDyn | Tag::CallHost | Tag::DefaultCall | Tag::Hook => {
                t.calls.push((x, true));
            }
            Tag::Await => t.calls.push((x, true)),
            _ => {}
        }
        if is_suspension(tag) {
            t.susp.push(x);
        }
    }
    // The rows, by root.
    let keys = locals as usize + n;
    fill(&mut t.start, keys + 1, 0);
    for &(k, _) in &t.pending {
        t.start[k as usize + 1] += 1;
    }
    for k in 0..keys {
        t.start[k + 1] += t.start[k];
    }
    let blank = Write {
        at: 0,
        start: 0,
        len: 0,
        cut: NONE,
        keep: true,
        direct: false,
    };
    t.rows.resize(t.start[keys] as usize, blank);
    // A stable placement: rows keep instruction order.
    t.next.clear();
    t.next.extend_from_slice(&t.start[..keys]);
    for &(k, wr) in &t.pending {
        t.rows[t.next[k as usize] as usize] = wr;
        t.next[k as usize] += 1;
    }
}

/// Decides read `r`: whether it borrows, and if so, its edits.
fn decide(b: &mut Body, cx: &dyn OwnCx, t: &mut Tables, edits: &mut splice::Tables, r: usize) {
    if b.tags[r] != Tag::Read || b.data[r][1] != ReadMode::Copy as u32 || t.keeps[r] {
        return;
    }
    if cx.counted(b.ty[r]) == Tri::No {
        return;
    }
    let x = u32_of(r);
    // The span's end: the read's last drop, or a binding local's.
    let (end, bind) = if t.last_drop[r] != NONE && t.stored[r] == NONE {
        (t.last_drop[r], NONE)
    } else if t.last_drop[r] == NONE && t.stored[r] != NONE && t.uses[r] == 1 {
        match binding(b, t, t.stored[r]) {
            Some(l) => (t.last_ldrop[l as usize], l),
            None => return,
        }
    } else {
        return;
    };
    let keep = keeps_drops(b, cx, r);
    // A binding read for a consumer would copy there: the read copies (or
    // moves) into the binding instead.
    if bind != NONE && (keep || t.kept_read[bind as usize] > 0) {
        return;
    }
    // The span is the statements of the end's block from the one that
    // holds the read to the end, with every block they run. A statement's
    // blocks precede it in the columns but not always in their order (a
    // match's arms precede its decision tree, a guard's condition may
    // precede the bindings before it), so the span's instructions are a
    // range from the first instruction of any of those statements.
    let h = t.blk[end as usize];
    let Some(s) = statement(b, t, x, h) else {
        return;
    };
    let members = b.record(b.data[h as usize][0]);
    let lo = members[t.pos[s as usize] as usize..=t.pos[end as usize] as usize]
        .iter()
        .map(|&m| t.first[m as usize])
        .min()
        .unwrap_or(x);
    if bind != NONE
        && t.first_read[bind as usize] != NONE
        && (t.first_read[bind as usize] < lo || t.last_read[bind as usize] > end)
    {
        return;
    }
    let chain = t.walk(b, b.data[r][0], true);
    if !chain.ok || conflicts(t, chain, x, lo, end) {
        return;
    }
    b.data[r][1] = ReadMode::Borrow as u32;
    let locals = u32_of(b.local_ty.len());
    if bind == NONE {
        if !keep {
            for k in t.start[(locals + x) as usize]..t.start[(locals + x) as usize + 1] {
                let d = t.rows[k as usize].at;
                if is_whole_drop(b, d, Root::Value(x)) {
                    t.gone[d as usize] = true;
                    edits.remove(t.blk[d as usize], d);
                }
            }
        }
    } else {
        // A binding local's read into this one keeps nothing of it, so it
        // may borrow too (`for part in parts` and an inlined `part.len()`
        // binding its `self`).
        if let (Root::Local(l), _, _) = root_of(b, b.data[r][0]) {
            t.kept_read[l as usize] -= 1;
        }
        t.holds[bind as usize] = x;
        t.unowned[bind as usize] = true;
        for k in t.start[bind as usize]..t.start[bind as usize + 1] {
            let d = t.rows[k as usize].at;
            if is_whole_drop(b, d, Root::Local(bind)) {
                t.gone[d as usize] = true;
                edits.remove(t.blk[d as usize], d);
            }
        }
    }
}

/// The body's lent parameters (`OwnCx::received`) that it never stores,
/// addresses, captures or changes in place: the caller keeps each alive
/// for the call, so the body owns none of them. Their `Drop`s go and they
/// leave the exit set; every read of one may borrow, and no read moves out
/// of it. The backend takes no count of such a parameter on entry, and
/// releases at the exit one that an instance passes owned.
///
/// An owned parameter of a body that does not suspend is left unowned the
/// same way when no read of it keeps its value (every use lends, and no
/// store takes it): the instance releases the count it receives at its
/// exit, once, as for a lent one an instance owns (§23.31's parameter
/// loans), instead of after its last read and before every early return
/// of a value read from it, and its reads borrow over spans its own drop
/// no longer ends.
fn lent_params(b: &Body, cx: &dyn OwnCx, t: &mut Tables, edits: &mut splice::Tables) {
    if b.kind != BodyKind::Fn && b.kind != BodyKind::DeriveInstance {
        return;
    }
    // A closure's parameters are its own sub-body's.
    fill(&mut t.inner, b.local_ty.len(), false);
    for r in b.sub_params.iter().skip(1) {
        for k in 0..r.len {
            let l = b.extra[(r.start + 1 + k) as usize] as usize;
            t.inner[l] = true;
        }
    }
    let param = |t: &Tables, l: usize| b.local_flags[l] & local_flags::PARAM != 0 && !t.inner[l];
    let n = (0..b.local_ty.len()).filter(|&l| param(t, l)).count();
    if n == 0 {
        return;
    }
    let suspends = cx.suspending(b.item);
    let mut k = 0;
    for l in 0..b.local_ty.len() {
        if !param(t, l) {
            continue;
        }
        let at = k;
        k += 1;
        // A parameter changed in place is owned: the change gives up a
        // count of the buffer it replaces, which must be the body's.
        if t.stores[l] != 0
            || t.captured[l]
            || t.reached[l]
            || t.changed[l]
            || cx.counted(b.local_ty[l]) == Tri::No
        {
            continue;
        }
        let lent = match cx.received(b.item, at, n) {
            Pass::Lent => true,
            Pass::Owned => !suspends && t.kept_read[l] == 0,
        };
        if !lent {
            continue;
        }
        t.unowned[l] = true;
        for x in t.start[l]..t.start[l + 1] {
            let d = t.rows[x as usize].at;
            if is_whole_drop(b, d, Root::Local(u32_of(l))) {
                t.gone[d as usize] = true;
                edits.remove(t.blk[d as usize], d);
            }
        }
    }
}

/// The `Ref`s of a field of what a lent parameter refers to, which take no
/// count (`ref_word::LENT`): `[*$p .f ...]`, or `[*%v .f ...]` with `%v`
/// a borrowed read of all of `$p`, where `$p` is a parameter the body owns
/// none of ([`lent_params`]) and never stores, with only field steps after
/// the dereference, so the reference points into the block the caller's
/// reference keeps alive for the whole call; and every use of the
/// reference lends. Its `Drop`s stay: where an instance's reference view
/// makes the referent fresh, the reference holds that value's count, and
/// the drop releases it (the backend's flag, `Em::view_loans`).
fn lent_refs(b: &mut Body, t: &Tables) {
    for r in 0..b.len() {
        let [a, w] = b.data[r];
        if b.tags[r] != Tag::Ref
            || w & crate::ir::ref_word::TAKES != 0
            || t.keeps[r]
            || t.stored[r] != NONE
        {
            continue;
        }
        let (root, from, to) = root_of(b, a);
        if from >= to || b.extra[from as usize] & place::PROJ_KIND != place::PROJ_DEREF {
            continue;
        }
        let fields =
            (from + 1..to).all(|x| b.extra[x as usize] & place::PROJ_KIND == place::PROJ_FIELD);
        let param = match root {
            Root::Local(l) => Some(l),
            Root::Value(v)
                if b.tags[v as usize] == Tag::Read
                    && b.data[v as usize][1] == ReadMode::Borrow as u32
                    && b.data[v as usize][0] & place::BARE_LOCAL != 0 =>
            {
                Some(b.data[v as usize][0] & !place::BARE_LOCAL)
            }
            _ => None,
        };
        if fields
            && let Some(p) = param
            && t.unowned[p as usize]
            && t.stores[p as usize] == 0
            && b.local_flags[p as usize] & local_flags::PARAM != 0
        {
            b.data[r][1] |= crate::ir::ref_word::LENT;
        }
    }
}

/// Whether instruction `d` is a `Drop` of all of `root`.
fn is_whole_drop(b: &Body, d: u32, root: Root) -> bool {
    if b.tags[d as usize] != Tag::Drop {
        return false;
    }
    let (r, from, to) = root_of(b, b.data[d as usize][0]);
    from == to && r == root
}

/// The local `Store` `s` makes a binding local of (see the module): all of
/// a local that is no parameter, that no reference or closure reaches,
/// stored once, and dropped. Its reads lie in the span, which [`decide`]
/// checks.
fn binding(b: &Body, t: &Tables, s: u32) -> Option<u32> {
    let (Root::Local(l), from, to) = root_of(b, b.data[s as usize][0]) else {
        return None;
    };
    let li = l as usize;
    (from == to
        && b.local_flags[li] & local_flags::PARAM == 0
        && !t.reached[li]
        && !t.changed[li]
        && !t.captured[li]
        && t.stores[li] == 1
        && t.last_ldrop[li] != NONE)
        .then_some(l)
}

/// The statement of block `h` that holds instruction `x`, or `None` when
/// `h` is not around it.
fn statement(b: &Body, t: &Tables, mut x: u32, h: u32) -> Option<u32> {
    for _ in 0..b.len() {
        if b.tags[x as usize] == Tag::Block {
            x = t.owner[x as usize];
            if x == NONE {
                return None;
            }
            continue;
        }
        let p = t.blk[x as usize];
        if p == NONE {
            return None;
        }
        if p == h {
            return Some(x);
        }
        x = p;
    }
    None
}

/// Whether read `r` keeps its `Drop`s when it borrows: an instance may
/// make its value fresh (see the module). A read of a bare local never
/// does.
fn keeps_drops(b: &Body, cx: &dyn OwnCx, r: usize) -> bool {
    let word = b.data[r][0];
    if word & place::BARE_LOCAL != 0 {
        return false;
    }
    let (root, from, to) = root_of(b, word);
    let (derefs, _) = kinds(b, from, to);
    let root_ty = match root {
        Root::Local(l) => b.local_ty[l as usize],
        Root::Value(v) => b.ty[v as usize],
        Root::Global(t) => t,
        Root::Other => return true,
    };
    derefs > 0 || cx.counted(root_ty) == Tri::Maybe || cx.counted(b.ty[r]) == Tri::Maybe
}

/// Whether anything in read `r`'s span, the instructions `lo..=end`, may
/// change or release what its resolved chain reads (rules 2 to 4).
fn conflicts(t: &Tables, chain: Chain, r: u32, lo: u32, end: u32) -> bool {
    for link in &t.links {
        if link.key == NONE {
            continue;
        }
        let at = link.start as usize;
        let (own, below) = (
            &t.cpath[at..at + link.own as usize],
            &t.cpath[at..at + link.len as usize],
        );
        for w in t.row(link.key, lo, end) {
            if w.at == r || t.gone[w.at as usize] {
                continue;
            }
            let path = &t.wpaths[w.start as usize..(w.start + w.len) as usize];
            // A write in the place of the read at this root: the verifier's
            // overlap. Any other: one that replaces what the chain reads.
            let hit = (w.direct && overlap(own, path))
                || (!w.keep && overlap(below, path) && (w.cut == NONE || link.len > w.cut));
            if hit {
                return true;
            }
        }
    }
    // The verifier's overlap of places in two roots: one behind a
    // reference or in a buffer, the other where a reference can reach.
    let behind = t.links.iter().any(|l| l.behind);
    if behind || t.links.iter().any(|l| l.global) {
        let k = t.reach.partition_point(|&(x, _, _)| x < lo);
        for &(x, w_behind, through) in &t.reach[k..] {
            if x > end {
                break;
            }
            if !t.gone[x as usize] && through != r && (behind || w_behind) {
                return true;
            }
        }
    }
    if chain.heap {
        let k = t.heap.partition_point(|&(x, _)| x < lo);
        if t.heap[k..]
            .iter()
            .take_while(|&&(x, _)| x <= end)
            .any(|&(_, through)| through != r)
        {
            return true;
        }
        let k = t.calls.partition_point(|&(x, _)| x < lo);
        for &(x, may) in &t.calls[k..] {
            if x > end {
                break;
            }
            if may || !chain.param {
                return true;
            }
        }
    }
    any_in(&t.susp, lo, end)
}
