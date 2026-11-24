//! Run-time pointer loans (`docs/compiler/ownership.md`, "Pointer Loans
//! (Model A)"), placed as statements in the checker's body from its
//! lifetime regions: a `LoanBegin` right after each site whose loan a
//! live value holds, and a `LoanClose` wherever the last value or local
//! holding it dies, on every path.
//!
//! The checker gives the rows: each site, and each holder (a value, or a
//! local) whose lifetime region contains the site's loan. A loan is live
//! at a point while one of its holders is (rustc's NLL `Borrows`, whose
//! loans go out of scope where their region stops being live). Liveness is
//! a backward walk over the structured control flow
//! ([`super::flow::backward`]), one bit per holder: a value is live from
//! its definition to its last use, a local from a read to the whole store
//! that replaces it. At the solved fixed point one lexical pass closes a
//! loan at each edge from a point where it is live to one where it is
//! dead: after an instruction, at the start of a block a branch runs,
//! before a jump, and at a sub-body's end. A close at a dead point is
//! always safe, since the backend's close does nothing for a loan not
//! held, so a redundant close costs code, never a check.
//!
//! Cost per body: the flow's own (one transfer per instruction, states at
//! control boundaries, `holders / 64` words each); a close test at an
//! instruction looks only at the holders it uses, and one at a branch,
//! a jump or a block start reads the state's words once, then each
//! candidate loan's holders until one is live.

use hd_base::NodeIdx;
use hd_base::SliceSort as _;
use hd_types::Ty;

use super::flow::{BitArena, Flow, backward};
use super::splice::{BatchInsert, insert_batch};
use crate::ir::{Body, NONE, Ref, Root, Tag, Topology, loan_word, local_flags, u32_of};

/// A run-time loan site: the instruction whose loans it takes, and its
/// [`loan_word`]. `activate` is the call before which a two-phase site's
/// shared loan becomes exclusive, or `NONE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Site {
    pub at: u32,
    pub word: u32,
    pub activate: u32,
}

/// What may hold a site's loan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Holder {
    Value(u32),
    Local(u32),
}

/// Places the loans of `sites` in `b`; `rows` pairs each holder with an
/// index into `sites`.
///
/// # Errors
/// A malformed insertion, which is a compiler bug.
pub fn place(b: &mut Body, sites: &[Site], rows: &[(Holder, u32)]) -> Result<(), String> {
    if sites.is_empty() {
        return Ok(());
    }
    let topo = Topology::new(b);
    let mut walk = Walk::new(b, &topo, sites, rows);
    let mut flow = Flow::default();
    flow.arena.words = walk.bits.div_ceil(64).max(1);
    backward(b, &topo, &mut flow, &mut walk);
    // Nothing is live after a sub-body's end: the loans its root's tail
    // holds end there.
    for &r in &b.sub_root {
        let tail = b.data[r as usize][1];
        if let Some(&bit) = walk.vbit.get(tail as usize)
            && bit != NONE
        {
            let none = vec![0; flow.arena.words];
            walk.close(At::End(r), [bit], &none);
        }
    }
    let inserts = walk.inserts(b, &topo);
    insert_batch(b, &topo, &inserts).map(|_| ())
}

/// The pointer local an access's place, a loan site's among them, goes
/// through: its root local, or the root local of the read its root value
/// is (`&mut p.items` in a loop over it), where the step after that local
/// dereferences it; or, for a loan site (`lent`), the local a whole read
/// lends (`[$p]` passed as `&T`).
#[must_use]
pub fn site_pointer(b: &Body, site: u32, lent: bool) -> Option<u32> {
    let tag = *b.tags.get(site as usize)?;
    if !matches!(tag, Tag::Ref | Tag::Read | Tag::Store | Tag::IsUnique) {
        return None;
    }
    let p = b.view(b.data[site as usize][0]);
    let through = |q: crate::ir::PlaceView| match q.root {
        Root::Local(l) if q.first(b) == Some(crate::ir::Proj::Deref) => Some(l),
        _ => None,
    };
    match p.root {
        Root::Local(l) if lent && tag == Tag::Read && p.whole() => Some(l),
        Root::Local(_) => through(p),
        Root::Value(v) if b.tags.get(v as usize) == Some(&Tag::Read) => {
            through(b.view(b.data[v as usize][0]))
        }
        _ => None,
    }
}

/// Gives each pointer local that a closure captures and a loan site goes
/// through a local counting the function's own loans through it
/// (`docs/compiler/ownership.md`, "Closures"): its loans' `LoanBegin`s
/// name it ([`loan_word::COUNTER_SHIFT`]), and each closure that captures
/// the pointer captures the counter too, shared, so the storage pass gives
/// it a cell the creator and its closures count in, across suspensions.
/// An inlined closure's snapshot of a pointer counts as its creator's.
/// Runs before the storage pass. Cost: one pass over the body, its
/// captures and its `LoanBegin`s.
///
/// # Errors
/// A malformed insertion, which is a compiler bug.
pub(crate) fn share_counters(b: &mut Body, temporary: hd_base::Symbol) -> Result<(), String> {
    let mut captured = vec![false; b.local_ty.len()];
    // The captures of each closure's sub-body.
    let mut captured_in: hd_base::HashSet<(u32, u32)> = hd_base::HashSet::default();
    let mut closures = Vec::new();
    for i in 0..b.len() {
        if b.tags[i] == Tag::Closure {
            let r = b.record(b.data[i][1]);
            for c in r[0]..r[0] + r[1] {
                let l = b.cap_local[c as usize];
                captured[l.idx()] = true;
                captured_in.insert((b.data[i][0], l.raw()));
            }
            closures.push(i);
        }
    }
    if closures.is_empty() {
        return Ok(());
    }
    // A snapshot's creator, through chains of inlined closures.
    let mut creator: Vec<u32> = (0..u32_of(b.local_ty.len())).collect();
    for &(_, snapshot, made) in &b.inlined_capture_origins {
        creator[snapshot.idx()] = creator[made.idx()];
    }
    // A pointer's loans that its closures must see: one taken in a
    // sub-body that does not capture the pointer, its creator's.
    let topo = Topology::new(b);
    let mut shared = vec![false; b.local_ty.len()];
    let mut begins = Vec::new();
    for i in 0..b.len() {
        let [site, word] = b.data[i];
        if b.tags[i] != Tag::LoanBegin || word & loan_word::ACTIVATE != 0 {
            continue;
        }
        if let Some(local) = site_pointer(b, site, true) {
            let p = creator[local as usize] as usize;
            if captured[p] {
                begins.push((i, p));
                shared[p] |= !captured_in.contains(&(topo.sub[i], local));
            }
        }
    }
    begins.retain(|&(_, p)| shared[p]);
    if begins.is_empty() {
        return Ok(());
    }
    let mut counter = vec![NONE; b.local_ty.len()];
    for &(_, p) in &begins {
        if counter[p] == NONE {
            counter[p] = u32_of(b.local_ty.len());
            let syn = b.local_syn[p];
            b.local_ty.push(Ty::I32);
            b.local_name.push(temporary);
            b.local_syn.push(syn);
            b.local_flags.push(
                local_flags::ASSIGNED
                    | local_flags::READ
                    | local_flags::CAPTURED
                    | local_flags::CAPTURED_ASSIGNED,
            );
        }
    }
    for (i, p) in begins {
        // ruled out: a counter local past 2^24 - 1 does not fit above the
        // flags. Every TIR local lowers to at least one Wasm local, and V8
        // caps a Wasm function at 50,000 locals (`kV8MaxWasmFunctionLocals
        // = 50'000`, v8 `src/wasm/wasm-limits.h`), so no body reaches it;
        // a checked conversion states it.
        let word = u32::try_from((u64::from(counter[p]) + 1) << loan_word::COUNTER_SHIFT)
            .map_err(|_| "an own-loan counter local past 2^24 - 1".to_string())?;
        b.data[i][1] |= word;
    }
    // Each closure capturing a counted pointer takes its counter: its
    // captures move to the end of the list with the counters after them.
    for ci in closures {
        let r = b.record(b.data[ci][1]).to_vec();
        let range = r[0] as usize..(r[0] + r[1]) as usize;
        let mut add: Vec<u32> = b.cap_local[range.clone()]
            .iter()
            .map(|l| counter[creator[l.idx()] as usize])
            .filter(|&c| c != NONE)
            .collect();
        add.sort_stable();
        add.dedup();
        if add.is_empty() {
            continue;
        }
        let start = u32_of(b.cap_local.len());
        let (locals, modes) = (
            b.cap_local[range.clone()].to_vec(),
            b.cap_mode[range].to_vec(),
        );
        b.cap_local.extend(locals);
        b.cap_mode.extend(modes);
        for c in &add {
            b.cap_local.push(hd_base::LocalId::from_raw(*c));
            b.cap_mode.push(crate::ir::CaptureMode::Shared);
        }
        let len = u32_of(b.cap_local.len()) - start;
        let mut words = r.clone();
        words[0] = start;
        words[1] = len;
        b.data[ci][1] = b.push_record(&words);
    }
    // Each counter starts at 0 where the body starts, and is used, so
    // the passes keep it, right after each access through its pointer,
    // whose check reads it, and each loan statement through it.
    let zero = Ref::konst(u32_of(b.consts.len())).0;
    b.consts.push((Ty::I32, 0));
    let root = b.sub_root[0];
    let mut inserts: Vec<BatchInsert> = (0..counter.len())
        .filter(|&p| counter[p] != NONE)
        .map(|p| BatchInsert {
            block: root,
            pos: 0,
            tag: Tag::Store,
            data: [crate::ir::place::BARE_LOCAL | counter[p], zero],
            ty: Ty::VOID,
            syn: b.local_syn[p],
        })
        .collect();
    let mut used = |at: usize, c: u32| {
        inserts.push(BatchInsert {
            block: topo.blk[at],
            pos: topo.pos[at] + 1,
            tag: Tag::OwnLoans,
            data: [crate::ir::place::BARE_LOCAL | c, NONE],
            ty: Ty::VOID,
            syn: b.syn[at],
        });
    };
    for i in 0..b.len() {
        let tag = b.tags[i];
        if matches!(tag, Tag::LoanBegin | Tag::LoanClose) {
            let begin = if tag == Tag::LoanBegin {
                i
            } else {
                b.data[i][0] as usize
            };
            if let Some(c) = b.data.get(begin).and_then(|d| loan_word::counter(d[1])) {
                used(i, c);
            }
        } else if let Some(p) = site_pointer(b, u32_of(i), false)
            && counter[creator[p as usize] as usize] != NONE
        {
            used(i, counter[creator[p as usize] as usize]);
        }
    }
    insert_batch(b, &topo, &inserts).map(|_| ())
}

/// A close's position: after an instruction, before one, or at a block's
/// start or end.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum At {
    After(u32),
    Before(u32),
    Start(u32),
    End(u32),
}

struct Walk<'t> {
    topo: &'t Topology,
    sites: &'t [Site],
    bits: usize,
    /// Each value's and each local's bit, or `NONE`.
    vbit: Vec<u32>,
    lbit: Vec<u32>,
    /// Per bit, its loans; per loan, its holders' bits (CSR).
    loans_at: Vec<u32>,
    loans: Vec<u32>,
    holders_at: Vec<u32>,
    holders: Vec<u32>,
    /// Per instruction, the site it is (`NONE` for none).
    site_of: Vec<u32>,
    record: bool,
    /// The holders the instruction being walked uses, and the state after
    /// it.
    used: Vec<u32>,
    after: Vec<u64>,
    /// Per compound instruction, the state after it, and per block, the
    /// state at its start, kept until the instruction that runs it is
    /// reached.
    afters: hd_base::HashMap<u32, Vec<u64>>,
    starts: hd_base::HashMap<u32, Vec<u64>>,
    /// The sites a live holder makes their loan begin.
    begun: Vec<bool>,
    closes: Vec<(At, u32)>,
    /// Per loan, the last [`Walk::close`] call that tested it.
    stamp: Vec<u32>,
    epoch: u32,
}

impl<'t> Walk<'t> {
    fn new(b: &Body, topo: &'t Topology, sites: &'t [Site], rows: &[(Holder, u32)]) -> Self {
        let (mut vbit, mut lbit) = (vec![NONE; b.len()], vec![NONE; b.local_ty.len()]);
        let mut bits = 0u32;
        let mut bit_rows: Vec<(u32, u32)> = Vec::with_capacity(rows.len());
        for &(h, s) in rows {
            let slot = match h {
                Holder::Value(v) => vbit.get_mut(v as usize),
                Holder::Local(l) => lbit.get_mut(l as usize),
            };
            let Some(slot) = slot else { continue };
            if *slot == NONE {
                *slot = bits;
                bits += 1;
            }
            bit_rows.push((*slot, s));
        }
        let (loans_at, loans) = csr(bits as usize, bit_rows.iter().copied());
        let (holders_at, holders) = csr(sites.len(), bit_rows.iter().map(|&(bit, s)| (s, bit)));
        let mut site_of = vec![NONE; b.len()];
        for (k, s) in sites.iter().enumerate() {
            if let Some(x) = site_of.get_mut(s.at as usize) {
                *x = u32_of(k);
            }
        }
        Self {
            topo,
            sites,
            bits: bits as usize,
            vbit,
            lbit,
            loans_at,
            loans,
            holders_at,
            holders,
            site_of,
            record: false,
            used: Vec::new(),
            after: Vec::new(),
            afters: hd_base::HashMap::default(),
            starts: hd_base::HashMap::default(),
            begun: vec![false; sites.len()],
            closes: Vec::new(),
            stamp: vec![0; sites.len()],
            epoch: 0,
        }
    }

    /// Whether a holder of loan `s` is live in `row`.
    fn held(&self, s: u32, row: &[u64]) -> bool {
        let at = self.holders_at[s as usize] as usize..self.holders_at[s as usize + 1] as usize;
        self.holders[at].iter().any(|&h| bit(row, h))
    }

    /// Closes at `at` each loan of the holders `from` that no holder in
    /// `to` keeps.
    fn close(&mut self, at: At, from: impl IntoIterator<Item = u32>, to: &[u64]) {
        self.epoch += 1;
        for h in from {
            for k in self.loans_at[h as usize]..self.loans_at[h as usize + 1] {
                let s = self.loans[k as usize];
                if self.stamp[s as usize] != self.epoch {
                    self.stamp[s as usize] = self.epoch;
                    if !self.held(s, to) {
                        self.closes.push((at, s));
                    }
                }
            }
        }
    }

    /// The holders live in `before` but not in `after`.
    fn dropped<'a>(before: &'a [u64], after: &'a [u64]) -> impl Iterator<Item = u32> + 'a {
        before
            .iter()
            .zip(after)
            .enumerate()
            .flat_map(|(k, (&x, &y))| {
                let mut w = x & !y;
                std::iter::from_fn(move || {
                    (w != 0).then(|| {
                        let b = w.trailing_zeros();
                        w &= w - 1;
                        u32_of(k * 64) + b
                    })
                })
            })
    }

    /// The local instruction `x` reads: a place rooted at it but a whole
    /// store into it, which replaces what it holds.
    fn local_read(b: &Body, x: usize) -> Option<u32> {
        match b.tags[x] {
            Tag::Read | Tag::Ref => match b.view(b.data[x][0]).root {
                Root::Local(l) => Some(l),
                _ => None,
            },
            Tag::Store => {
                let p = b.view(b.data[x][0]);
                match p.root {
                    Root::Local(l) if !p.whole() => Some(l),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn whole_store(b: &Body, x: usize) -> Option<u32> {
        if b.tags[x] != Tag::Store {
            return None;
        }
        let p = b.view(b.data[x][0]);
        match p.root {
            Root::Local(l) if p.whole() => Some(l),
            _ => None,
        }
    }

    fn compound(b: &Body, x: usize) -> bool {
        matches!(
            b.tags[x],
            Tag::If
                | Tag::And
                | Tag::Or
                | Tag::Match
                | Tag::Loop
                | Tag::Scope
                | Tag::With
                | Tag::RefDyn
                | Tag::Guard
                | Tag::SwitchTag
                | Tag::SwitchInt
                | Tag::SwitchChar
                | Tag::SwitchStr
        )
    }

    fn mark(&mut self, s: &mut BitArena, live: usize, bit: u32) {
        if bit != NONE {
            s.set(live, bit, true);
            if self.record {
                self.used.push(bit);
            }
        }
    }

    /// The batch: each begin, its activation, and each close of a begun
    /// loan, closes first at one position.
    fn inserts(&self, b: &Body, topo: &Topology) -> Vec<BatchInsert> {
        let n = u32_of(b.len());
        let pos = |at: At| match at {
            At::After(x) => (topo.blk[x as usize], topo.pos[x as usize] + 1),
            At::Before(x) => (topo.blk[x as usize], topo.pos[x as usize]),
            At::Start(blk) => (blk, 0),
            At::End(blk) => (blk, u32_of(b.record(b.data[blk as usize][0]).len())),
        };
        // A close is of a begun loan, in the sub-body of its site.
        let sub_at = |at: At| match at {
            At::After(x) | At::Before(x) => topo.sub[x as usize],
            At::Start(blk) | At::End(blk) => topo.sub[blk as usize],
        };
        let mut closes = self.closes.clone();
        closes.retain(|&(at, s)| {
            self.begun[s as usize] && sub_at(at) == topo.sub[self.sites[s as usize].at as usize]
        });
        closes.sort_stable();
        closes.dedup();
        // A close before a begin at one position runs first (the batch's
        // order is the position's): a close where a loan's site runs again
        // ends its last run. A close at a block's end runs after a begin
        // there, of a site that is the block's last statement. So the
        // batch is the other closes, the begins, then the block-end closes,
        // and begin `k` is insertion `early + k`.
        closes.sort_stable_by_key(|&(at, _)| matches!(at, At::End(_)));
        let early = closes
            .iter()
            .filter(|(at, _)| !matches!(at, At::End(_)))
            .count();
        let mut begins = vec![NONE; self.sites.len()];
        let mut k = u32_of(early);
        for (s, site) in self.sites.iter().enumerate() {
            if self.begun[s] {
                begins[s] = n + k;
                k += 1;
                if site.activate != NONE {
                    k += 1;
                }
            }
        }
        let mut out = Vec::with_capacity(k as usize);
        for (s, site) in self.sites.iter().enumerate() {
            if !self.begun[s] {
                continue;
            }
            let (blk, p) = pos(At::After(site.at));
            out.push(insert(
                blk,
                p,
                Tag::LoanBegin,
                [site.at, site.word],
                b.syn[site.at as usize],
            ));
            if site.activate != NONE {
                let (blk, p) = pos(At::Before(site.activate));
                let syn = b.syn[site.activate as usize];
                out.push(insert(
                    blk,
                    p,
                    Tag::LoanBegin,
                    [site.at, loan_word::ACTIVATE],
                    syn,
                ));
            }
        }
        let mut placed: Vec<BatchInsert> = closes
            .into_iter()
            .map(|(at, s)| {
                let (blk, p) = pos(at);
                // A close names the line of the statement it follows or
                // precedes, of the branch whose block it starts, or of
                // the last statement of the block it ends.
                let syn = match at {
                    At::After(x) | At::Before(x) => b.syn[x as usize],
                    At::Start(blk) => match topo.owner[blk as usize] {
                        NONE => NodeIdx::NONE,
                        x => b.syn[x as usize],
                    },
                    At::End(blk) => (b.record(b.data[blk as usize][0]).last())
                        .map_or(NodeIdx::NONE, |&x| b.syn[x as usize]),
                };
                insert(blk, p, Tag::LoanClose, [begins[s as usize], NONE], syn)
            })
            .collect();
        let late = placed.split_off(early);
        placed.extend(out);
        placed.extend(late);
        placed
    }
}

fn insert(block: u32, pos: u32, tag: Tag, data: [u32; 2], syn: NodeIdx) -> BatchInsert {
    BatchInsert {
        block,
        pos,
        tag,
        data,
        ty: Ty::VOID,
        syn,
    }
}

fn bit(row: &[u64], b: u32) -> bool {
    row[(b / 64) as usize] & (1 << (b % 64)) != 0
}

/// A compressed row table of `n` rows from (row, value) pairs.
fn csr(n: usize, pairs: impl Iterator<Item = (u32, u32)> + Clone) -> (Vec<u32>, Vec<u32>) {
    let mut at = vec![0u32; n + 1];
    for (r, _) in pairs.clone() {
        at[r as usize + 1] += 1;
    }
    for k in 0..n {
        at[k + 1] += at[k];
    }
    let mut fill = at.clone();
    let mut out = vec![0u32; at[n] as usize];
    for (r, v) in pairs {
        out[fill[r as usize] as usize] = v;
        fill[r as usize] += 1;
    }
    (at, out)
}

impl super::flow::Backward for Walk<'_> {
    fn recording(&mut self, record: bool) {
        self.record = record;
        self.afters.clear();
        self.starts.clear();
    }

    fn stmt(&mut self, b: &Body, x: u32, s: &mut BitArena, live: usize) {
        let i = x as usize;
        if self.record {
            let row = s.row(live);
            let site = self.site_of[i];
            if site != NONE && self.held(site, row) {
                self.begun[site as usize] = true;
            }
            if Self::compound(b, i) {
                self.afters.insert(x, row.to_vec());
            } else {
                self.after.clear();
                self.after.extend_from_slice(row);
            }
            self.used.clear();
        }
        if self.vbit[i] != NONE {
            s.set(live, self.vbit[i], false);
        }
        if let Some(l) = Self::whole_store(b, i)
            && self.lbit[l as usize] != NONE
        {
            s.set(live, self.lbit[l as usize], false);
        }
    }

    fn uses(&mut self, _: &Body, v: u32, s: &mut BitArena, live: usize) {
        if let Some(&bit) = self.vbit.get(v as usize) {
            self.mark(s, live, bit);
        }
    }

    fn enter(&mut self, _: &Body, blk: u32, s: &mut BitArena, live: usize) {
        if self.record && self.topo.owner[blk as usize] != NONE {
            self.starts.insert(blk, s.row(live).to_vec());
        }
    }

    fn ran(&mut self, b: &Body, x: u32, s: &mut BitArena, live: usize) {
        if !self.record || !Self::compound(b, x as usize) {
            return;
        }
        let decided = s.row(live).to_vec();
        let after = self.afters.remove(&x).unwrap_or_default();
        let mut blocks = Vec::new();
        crate::ir::for_blocks(b, x as usize, |blk| blocks.push(blk));
        // A branch's arms: what is live deciding it but dead at an arm's
        // start ends there.
        for &blk in &blocks {
            if let Some(start) = self.starts.remove(&blk) {
                let dropped: Vec<u32> = Self::dropped(&decided, &start).collect();
                self.close(At::Start(blk), dropped, &start);
            }
        }
        // What an arm's tail or a fallthrough held, dead after it.
        let mut from: Vec<u32> = Self::dropped(&decided, &after).collect();
        for &blk in &blocks {
            let tail = b.data[blk as usize][1];
            if let Some(&bit) = self.vbit.get(tail as usize)
                && bit != NONE
                && !after.is_empty()
                && !self::bit(&after, bit)
            {
                from.push(bit);
            }
        }
        if !after.is_empty() {
            self.close(At::After(x), from, &after);
        }
    }

    fn reads(&mut self, b: &Body, x: u32, s: &mut BitArena, live: usize) {
        let i = x as usize;
        if let Some(l) = Self::local_read(b, i) {
            self.mark(s, live, self.lbit[l as usize]);
        }
        if b.tags[i] == Tag::Closure
            && !b.is_instantiation(i)
            && let [start, len, ..] = *b.record(b.data[i][1])
        {
            for c in start..start.saturating_add(len) {
                if let Some(l) = b.cap_local.get(c as usize) {
                    self.mark(s, live, self.lbit[l.idx()]);
                }
            }
        }
        if !self.record || Self::compound(b, i) {
            return;
        }
        let after = std::mem::take(&mut self.after);
        let used = std::mem::take(&mut self.used);
        match b.tags[i] {
            Tag::Break | Tag::Continue | Tag::Return => {
                let before = s.row(live).to_vec();
                let dropped: Vec<u32> = Self::dropped(&before, &after).collect();
                self.close(At::Before(x), dropped, &after);
            }
            _ => {
                let gone: Vec<u32> = used.iter().copied().filter(|&h| !bit(&after, h)).collect();
                self.close(At::After(x), gone, &after);
            }
        }
        self.after = after;
        self.used = used;
    }
}

#[cfg(test)]
mod tests {
    use super::{Holder, Site, place};
    use crate::ir::{
        Body, BodyKind, NONE, Place, PlaceRoot, Proj, ReadMode, Tag, TirBuilder, TirSink as _,
        local_flags, place as pw, u32_of,
    };
    use crate::own::flow::work;
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    /// `n` references through pointer `$0`, each stored in a local of its
    /// own and read after all of them, in one block or each in an `if`
    /// arm: the sites and their holder rows (the `Ref`, the local).
    fn body(n: u32, branchy: bool) -> (Body, Vec<Site>, Vec<(Holder, u32)>) {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let syn = NodeIdx::from_raw(1);
        let root = b.open_block();
        let p = b.local(Ty::I32, Symbol::from_raw(0), local_flags::PARAM, syn);
        let c = b.local(Ty::BOOL, Symbol::from_raw(0), local_flags::PARAM, syn);
        let deref = b.place(&Place {
            root: PlaceRoot::Local(p),
            proj: vec![Proj::Deref],
        });
        let (mut sites, mut rows, mut holders) = (Vec::new(), Vec::new(), Vec::new());
        for k in 0..n {
            let l = b.local(Ty::I32, Symbol::from_raw(0), 0, syn);
            let arm = branchy.then(|| b.open_block());
            let r = b.emit(Tag::Ref, deref, 0, Ty::I32, syn);
            b.set(l, r, syn);
            if let Some(arm) = arm {
                let then = b.close_block(arm, None, Ty::VOID, syn);
                let cond = b.get(c, Ty::BOOL, syn);
                let arms = b.refs_record(&[then, crate::ir::Ref(NONE)]);
                b.emit(Tag::If, cond.0, arms, Ty::VOID, syn);
            }
            sites.push(Site {
                at: r.0,
                word: 0,
                activate: NONE,
            });
            rows.push((Holder::Value(r.0), k));
            rows.push((Holder::Local(l.raw()), k));
            holders.push(l);
        }
        for l in holders {
            b.emit(
                Tag::Read,
                pw::BARE_LOCAL | l.raw(),
                ReadMode::Use as u32,
                Ty::I32,
                syn,
            );
        }
        let root = b.close_block(root, None, Ty::VOID, syn);
        let body = b.finish_tolerating(root, &[], &[22]).expect("TIR shape");
        (body, sites, rows)
    }

    fn placed(n: u32, branchy: bool) -> (Body, usize) {
        let (mut b, sites, rows) = body(n, branchy);
        let ((), work) = work::measure(|| place(&mut b, &sites, &rows).expect("placed"));
        (b, work)
    }

    fn count(b: &Body, tag: Tag) -> usize {
        b.tags.iter().filter(|&&t| t == tag).count()
    }

    #[test]
    fn each_loan_begins_after_its_site_and_closes_after_its_holders_last_read() {
        for branchy in [false, true] {
            let (b, _) = placed(3, branchy);
            assert_eq!(count(&b, Tag::LoanBegin), 3);
            // Each loan closes after its local's read; a branch's arms that
            // never run its site close nothing extra on the straight path.
            assert!(count(&b, Tag::LoanClose) >= 3);
            for i in 0..b.len() {
                if b.tags[i] == Tag::LoanClose {
                    assert_eq!(b.tags[b.data[i][0] as usize], Tag::LoanBegin);
                }
            }
            let _ = u32_of(0);
        }
    }

    /// `loans` references through `$0`, each in a local of its own; then
    /// `ifs` branches, each reading one of those locals in its arm; then a
    /// loop with `breaks` exits, each `if c: break` past a read of one;
    /// then a read of every local: the sites and their holder rows.
    fn shape(loans: u32, ifs: u32, breaks: u32) -> (Body, Vec<Site>, Vec<(Holder, u32)>) {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let syn = NodeIdx::from_raw(1);
        let root = b.open_block();
        let p = b.local(Ty::I32, Symbol::from_raw(0), local_flags::PARAM, syn);
        let c = b.local(Ty::BOOL, Symbol::from_raw(0), local_flags::PARAM, syn);
        let deref = b.place(&Place {
            root: PlaceRoot::Local(p),
            proj: vec![Proj::Deref],
        });
        let (mut sites, mut rows, mut holders) = (Vec::new(), Vec::new(), Vec::new());
        for k in 0..loans {
            let l = b.local(Ty::I32, Symbol::from_raw(0), 0, syn);
            let r = b.emit(Tag::Ref, deref, 0, Ty::I32, syn);
            b.set(l, r, syn);
            sites.push(Site {
                at: r.0,
                word: 0,
                activate: NONE,
            });
            rows.push((Holder::Value(r.0), k));
            rows.push((Holder::Local(l.raw()), k));
            holders.push(l);
        }
        let read = |b: &mut TirBuilder, k: u32| {
            let l = holders[(k % loans) as usize];
            b.emit(
                Tag::Read,
                pw::BARE_LOCAL | l.raw(),
                ReadMode::Use as u32,
                Ty::I32,
                syn,
            );
        };
        for k in 0..ifs {
            let arm = b.open_block();
            read(&mut b, k);
            let then = b.close_block(arm, None, Ty::VOID, syn);
            let cond = b.get(c, Ty::BOOL, syn);
            let arms = b.refs_record(&[then, crate::ir::Ref(NONE)]);
            b.emit(Tag::If, cond.0, arms, Ty::VOID, syn);
        }
        if breaks > 0 {
            let lp = b.open_loop();
            let body = b.open_block();
            for k in 0..breaks {
                read(&mut b, k);
                let exit = b.open_block();
                b.emit(Tag::Break, lp.0.raw(), NONE, Ty::NEVER, syn);
                let exit = b.close_block(exit, None, Ty::NEVER, syn);
                let cond = b.get(c, Ty::BOOL, syn);
                let arms = b.refs_record(&[exit, crate::ir::Ref(NONE)]);
                b.emit(Tag::If, cond.0, arms, Ty::VOID, syn);
            }
            b.emit(Tag::Break, lp.0.raw(), NONE, Ty::NEVER, syn);
            let body = b.close_block(body, None, Ty::NEVER, syn);
            b.close_loop(lp, body, Ty::VOID, syn);
        }
        for k in 0..loans {
            read(&mut b, k);
        }
        let root = b.close_block(root, None, Ty::VOID, syn);
        let body = b.finish_tolerating(root, &[], &[22]).expect("TIR shape");
        (body, sites, rows)
    }

    fn work(loans: u32, ifs: u32, breaks: u32) -> usize {
        let (mut b, sites, rows) = shape(loans, ifs, breaks);
        let ((), work) = work::measure(|| place(&mut b, &sites, &rows).expect("placed"));
        // Every loan begins, and every close names a begin.
        assert_eq!(count(&b, Tag::LoanBegin), loans as usize);
        for i in 0..b.len() {
            if b.tags[i] == Tag::LoanClose {
                assert_eq!(b.tags[b.data[i][0] as usize], Tag::LoanBegin);
            }
        }
        work
    }

    // Cost: the backward flow's, one state of `holders / 64` words per
    // control group, as the move pass's last uses (`moves::last_uses`):
    // linear in the loans at a fixed number of branches, and in the
    // branches and jumps at a fixed number of loans.
    #[test]
    fn placement_work_is_linear_in_the_loans() {
        let (small, large) = (work(100, 0, 0), work(1000, 0, 0));
        assert!(large <= 12 * small, "straight line: {small} -> {large}");
        let (small, large) = (work(100, 10, 10), work(1000, 10, 10));
        assert!(large <= 12 * small, "with branches: {small} -> {large}");
    }

    #[test]
    fn placement_work_is_linear_in_the_branches() {
        let (small, large) = (work(10, 100, 0), work(10, 1000, 0));
        assert!(large <= 12 * small, "{small} -> {large}");
    }

    #[test]
    fn placement_work_is_linear_in_the_breaks_out_of_one_loop() {
        let (small, large) = (work(10, 0, 100), work(10, 0, 1000));
        assert!(large <= 12 * small, "{small} -> {large}");
    }

    // Loans and branches growing together cost one state of `holders / 64`
    // words per control group: O(holders x groups), 41.8x at 10x each.
    // rustc's borrow check pays the same, one `MixedBitSet<BorrowIndex>`
    // per basic block (`compiler/rustc_borrowck/src/dataflow.rs`,
    // `BorrowsDomain`), as does the move pass's `last_uses`. The bound is
    // that product: at most 100x for 10x of both.
    #[test]
    fn placement_work_is_bounded_by_loans_times_branches() {
        let (small, large) = (work(100, 100, 0), work(1000, 1000, 0));
        assert!(large <= 100 * small, "{small} -> {large}");
    }
}
