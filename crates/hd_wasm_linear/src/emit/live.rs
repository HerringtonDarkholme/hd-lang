//! Releases at last use (`crates/hd_wasm_linear/DESIGN.md` §23.20, job 12). A
//! reference is released after its last use, not at the end of its block
//! (`OBJECT_MODEL_TIER1.md` §6.3 rule 5), so a counted value whose last use
//! has passed must give up its count there, or an element reference would
//! pin its list until the function returns. Within one body, at statement
//! granularity: an instruction's value is released after the statement of
//! its own block that holds its last use, and a TIR local after the
//! statement of the innermost block holding all its uses that holds the
//! last. Each place is zeroed once released, so the function's exit, which
//! still releases every place, releases it as nothing. A value used where
//! no statement of its block holds the use (a block's result, a match
//! decision's value read in an arm) keeps its count to the exit.

use hd_base::{HashMap, HashSet};
use hd_tir::ir::{Body, IntrinsicOp, NONE, Op, PlaceRoot, Proj, Ref, Step, Tag, local_flags};

/// What to release after a statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Drop {
    /// An instruction's value.
    Val(u32),
    /// A TIR local.
    Local(usize),
}

/// The releases to make after each statement, keyed by the statement's
/// block and its position in that block.
pub(super) type Drops = HashMap<(u32, usize), Vec<Drop>>;

/// Where a body's counts can move instead of being copied
/// (`crates/hd_wasm_linear/DESIGN.md` §23.25, job 17).
#[derive(Default)]
pub(super) struct Moves {
    /// Reference reads with no value consumer. A physical load has no
    /// effect; a representation view must still run and drop its getter.
    pub unused_reads: HashSet<u32>,
    /// Each instruction value whose only use is one later statement of its
    /// own block: that statement runs exactly once after the value is made
    /// and nothing reads the value after it, so a consuming use there can
    /// take the value's count. With the use, the statement after which the
    /// value's release was planned.
    pub sole: HashMap<u32, (u32, (u32, usize))>,
    /// Each plain local read, and each reference to a local's own place
    /// ([`ref_local`]), that is its local's last access: the statement after which the local is
    /// released ([`plan`]), so the value it makes can take the local's
    /// count instead of a count of its own (`Em::take_local`,
    /// `Em::take_cell`). With the local and that statement.
    pub last_access: HashMap<u32, (usize, (u32, usize))>,
    /// Each block tail whose only use is as its block's value.
    pub tails: hd_base::HashSet<u32>,
    /// Reads whose source remains owned past every use. Emission filters
    /// these candidates by physical representation before borrowing them.
    pub loans: HashSet<u32>,
    /// Single-assignment bindings of such reads, with their source value.
    pub loan_locals: HashMap<usize, u32>,
    /// Local reads borrowed for their span ([`span_loans`]). Emission
    /// drops the ones whose local lives in a cell or whose value is open
    /// before adding the rest to [`Moves::loans`].
    pub span: HashSet<u32>,
    /// Fields owned by one consuming match or propagation decision.
    pub payloads: HashMap<u32, PayloadMove>,
}

/// The block structure of a body: each instruction's block and position,
/// and each block's owning instruction.
pub(super) struct Tree {
    /// The block whose list holds each instruction, and its position.
    parent: Vec<(u32, usize)>,
    /// The instruction that runs each block (an `If`, a `Loop`, ...).
    owner: Vec<u32>,
}

impl Tree {
    pub(super) fn new(b: &Body) -> Tree {
        let n = b.len();
        let mut parent = vec![(NONE, 0); n];
        let mut owner = vec![NONE; n];
        for i in 0..n {
            let [a, w] = b.data[i];
            let tag = b.tags[i];
            if tag == Tag::Block {
                for (p, &c) in b.record(a).iter().enumerate() {
                    if let Some(x) = parent.get_mut(c as usize) {
                        *x = (u32::try_from(i).expect("insts"), p);
                    }
                }
                continue;
            }
            let mut blocks = Vec::new();
            for (word, op) in [(a, tag.operands().0), (w, tag.operands().1)] {
                match op {
                    Op::Block => blocks.push(word),
                    Op::Blocks if word != NONE => blocks.extend_from_slice(b.record(word)),
                    _ => {}
                }
            }
            if matches!(
                tag,
                Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr
            ) {
                blocks.extend(b.record(w).iter().skip(1).take(2));
            }
            for blk in blocks {
                if b.tags.get(blk as usize) == Some(&Tag::Block) {
                    owner[blk as usize] = u32::try_from(i).expect("insts");
                }
            }
        }
        Tree { parent, owner }
    }

    /// The blocks around instruction `x`, innermost first, each with the
    /// position of the statement in it that holds `x`; `None` when the
    /// chain does not reach a sub-body's root (a closure's own body).
    pub(super) fn path(&self, b: &Body, mut x: u32) -> Option<Vec<(u32, usize)>> {
        let mut out = Vec::new();
        loop {
            if b.tags[x as usize] == Tag::Block {
                if b.sub_root.contains(&x) {
                    return Some(out);
                }
                x = self.owner[x as usize];
                if x == NONE {
                    return None;
                }
                continue;
            }
            let (blk, p) = self.parent[x as usize];
            if blk == NONE {
                return None;
            }
            out.push((blk, p));
            x = blk;
        }
    }

    /// Whether instruction `d` runs before instruction `j` on every path
    /// of their sub-body that reaches `j`: `d` is a statement of a block
    /// around `j`, before the statement that holds `j`. Blocks are entered
    /// only at their start (a resumed suspending body is the exception,
    /// `susp.rs`), so reaching that statement ran `d`.
    pub(super) fn dominates(&self, b: &Body, d: u32, j: u32) -> bool {
        let (blk, p) = self.parent[d as usize];
        blk != NONE
            && self
                .path(b, j)
                .is_some_and(|path| path.iter().any(|&(k, q)| k == blk && q > p))
    }

    /// Whether `i` belongs to this loop, including its nested control
    /// blocks but excluding separately invoked closure bodies.
    pub(super) fn in_loop(&self, b: &Body, i: u32, loop_inst: u32) -> bool {
        self.path(b, i).is_some_and(|path| {
            path.iter()
                .any(|(blk, _)| self.owner[*blk as usize] == loop_inst)
        })
    }

    /// A write guaranteed on an entered iteration: no intervening
    /// conditional or inner loop may skip it. `guard` is the loop test.
    pub(super) fn iteration_write(&self, b: &Body, i: u32, loop_inst: u32, guard: u32) -> bool {
        self.path(b, i).is_some_and(|path| {
            for (blk, _) in path {
                let owner = self.owner[blk as usize];
                if owner == loop_inst {
                    return true;
                }
                if owner != NONE && owner != guard && b.tags[owner as usize] != Tag::Scope {
                    return false;
                }
            }
            false
        })
    }
}

/// The local that value `r` reads when it is a plain local read,
/// `Read [$l]`; `None` for a constant or any other instruction.
pub(super) fn read_local(b: &Body, r: u32) -> Option<usize> {
    let i = Ref(r).as_inst()?.0 as usize;
    (i < b.len())
        .then(|| b.local_read(i))
        .flatten()
        .map(hd_base::LocalId::idx)
}

/// A `Store` `i` into a whole local: the local and the value it takes.
pub(super) fn local_store(b: &Body, i: usize) -> Option<(usize, u32)> {
    match b.stored_local(i)? {
        (l, true) => Some((l.idx(), b.data[i][1])),
        (_, false) => None,
    }
}

/// A `Store` `i` into a local or a struct field of it (`[$l]`,
/// `[$l .x .y]`): the local and the value stored. A field store replaces
/// part of the local, as a whole store replaces all of it.
pub(super) fn local_part_store(b: &Body, i: usize) -> Option<(usize, u32)> {
    b.stored_local(i).map(|(l, _)| (l.idx(), b.data[i][1]))
}

/// The list value whose element `Store` `i` writes (`[%xs [%i] ..]`), as
/// an element store intrinsic names its list.
pub(super) fn elem_store_list(b: &Body, i: usize) -> Option<u32> {
    if b.tags[i] != Tag::Store {
        return None;
    }
    let p = b.decode_place(b.data[i][0])?;
    match (p.root, p.proj.first()) {
        (PlaceRoot::Value(r), Some(Proj::Elem(_))) => Some(r.0),
        _ => None,
    }
}

/// The local at the root of the place of `Ref`, `Read` or `Store` `i`:
/// an access of that local, as a plain local read `[$l]` is.
pub(super) fn place_local(b: &Body, i: usize) -> Option<usize> {
    if !matches!(b.tags[i], Tag::Ref | Tag::Read | Tag::Store) {
        return None;
    }
    match b.decode_place(b.data[i][0])?.root {
        PlaceRoot::Local(l) => Some(l.idx()),
        _ => None,
    }
}

/// The local a reference instruction's place starts from, which the
/// reference reaches: a `Ref`'s root local. An element index is a value.
pub(super) fn ref_locals(b: &Body, i: usize) -> Vec<u32> {
    match b.tags[i] {
        Tag::Ref => place_local(b, i)
            .map(|l| u32::try_from(l).expect("locals"))
            .into_iter()
            .collect(),
        _ => vec![],
    }
}

/// The local whose own place reference `i` names, `&x` or `&x.f`: a
/// `Ref` rooted at a local with only field projections.
pub(super) fn ref_local(b: &Body, i: usize) -> Option<usize> {
    if b.tags[i] != Tag::Ref {
        return None;
    }
    let p = b.decode_place(b.data[i][0])?;
    match p.root {
        PlaceRoot::Local(l) if p.proj.iter().all(|s| matches!(s, Proj::Field(_))) => Some(l.idx()),
        _ => None,
    }
}

/// What a projection read of one step from a value reads, by the type
/// of that value (`Em::projected`): one fact per instruction, in a table
/// indexed by instruction. A read through a reference is `None`, though
/// `Read [*%r .k]` has the shape of a data object's field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Projected {
    /// No projection read.
    #[default]
    None,
    /// A field of a struct, newtype or data value (`[%v .k]`,
    /// `[*%o .k]`).
    Field,
    /// An element of a tuple (`[%v .k]`).
    Element,
    /// A field of an enum value's payload (`[%v as .V .k]`).
    Payload,
}

/// The value projection read `i` reads from, and what it reads
/// ([`Projected`]); `None` for any other instruction.
pub(super) fn projection(b: &Body, i: usize, projs: &[Projected]) -> Option<(u32, Projected)> {
    match projs.get(i).copied().unwrap_or_default() {
        Projected::None => None,
        p => b.read_step(i).map(|(v, _)| (v, p)),
    }
}

/// What a read through a reference reads through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Through {
    /// The reference a local holds.
    Local(usize),
    /// A reference value.
    Value(u32),
}

/// What a `Read` `i` whose place starts with a dereference reads
/// through, and whether it reads the whole referenced value. A data
/// object's field ([`Projected`]) is no read through a reference.
pub(super) fn read_through(b: &Body, i: usize, projs: &[Projected]) -> Option<(Through, bool)> {
    match b.tags[i] {
        Tag::Read if projection(b, i, projs).is_none() => {
            let p = b.decode_place(b.data[i][0])?;
            if p.proj.first() != Some(&Proj::Deref) {
                return None;
            }
            let through = match p.root {
                PlaceRoot::Local(l) => Through::Local(l.idx()),
                PlaceRoot::Value(r) => Through::Value(r.0),
                _ => return None,
            };
            Some((through, p.proj.len() == 1))
        }
        _ => None,
    }
}

/// The instruction values instruction `i` reads: TIR's own list where it
/// has one, else every operand word that names an instruction, an
/// over-approximation that only ever delays a release.
pub(super) fn uses(b: &Body, i: usize) -> Vec<u32> {
    let [a, w] = b.data[i];
    let tag = b.tags[i];
    // A block's record is its statements; only its tail is a use.
    if tag == Tag::Block {
        return if w != NONE && Ref(w).as_inst().is_some() {
            vec![w]
        } else {
            vec![]
        };
    }
    let mut out = if let Some(mut v) = b.value_operands(i) {
        // A call's last three words are its providers: a range of key and
        // value pairs (1), or a context value (2).
        if tag == Tag::Call {
            let r = b.record(w);
            if let [.., kind, x, n] = *r {
                match kind {
                    1 => v.extend(b.extra.iter().skip(x as usize).take(n as usize)),
                    2 => v.push(x),
                    _ => {}
                }
            }
        }
        v
    } else if matches!(tag, Tag::With | Tag::ContextNew) {
        // Keys and values in pairs.
        b.record(a).iter().skip(1).step_by(2).copied().collect()
    } else {
        let mut out = Vec::new();
        for (word, op) in [(a, tag.operands().0), (w, tag.operands().1)] {
            match op {
                Op::Value => out.push(word),
                Op::Values if word != NONE => out.extend_from_slice(b.record(word)),
                // Only these records hold values; a closure's holds its
                // captures' range, a switch's its case and arms.
                Op::Record
                    if word != NONE && matches!(tag, Tag::CallDyn | Tag::Await | Tag::NewList) =>
                {
                    out.extend_from_slice(b.record(word));
                }
                _ => {}
            }
        }
        out
    };
    out.retain(|&r| r != NONE && Ref(r).as_inst().is_some() && (r as usize) < b.len());
    out
}

/// An enum field extracted once within one consuming match. Earlier
/// extractions of the same variant dominate this one; the emitter can
/// clear the whole temporary only when they cover every field and no
/// later tag/payload read can run on this path.
pub(super) struct PayloadMove {
    pub fields: Vec<(u32, u32)>,
    pub last: bool,
}

type ArmReads = HashMap<u32, (usize, (u32, usize))>;

/// Matches and the tag switches that implement propagation consume their
/// own temporary, never a value also used outside their decision region.
/// Repeated field extractions and loops conservatively keep their counts.
fn match_payloads(
    b: &Body,
    sub: u32,
    tree: &Tree,
    users: &HashMap<u32, Vec<u32>>,
    access: &HashMap<usize, Vec<u32>>,
) -> (HashMap<u32, PayloadMove>, ArmReads) {
    let inside = |root: u32, i: u32| {
        root == i
            || tree
                .path(b, i)
                .is_some_and(|p| p.iter().any(|(k, _)| tree.owner[*k as usize] == root))
    };
    let cyclic = |root: u32, i: u32| {
        root != i
            && tree.path(b, i).is_none_or(|p| {
                p.iter()
                    .map(|(k, _)| tree.owner[*k as usize])
                    .take_while(|&owner| owner != root)
                    .any(|owner| owner != NONE && b.tags[owner as usize] == Tag::Loop)
            })
    };
    let before = |a: u32, c: u32| {
        tree.dominates(b, a, c) || (b.tags[a as usize] == Tag::SwitchTag && inside(a, c))
    };
    let exclusive = |a: u32, c: u32| {
        let (Some(ap), Some(cp)) = (tree.path(b, a), tree.path(b, c)) else {
            return false;
        };
        ap.iter().any(|&(ak, _)| {
            let owner = tree.owner[ak as usize];
            owner != NONE
                && matches!(
                    b.tags[owner as usize],
                    Tag::If | Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr
                )
                && cp
                    .iter()
                    .any(|&(ck, _)| ck != ak && tree.owner[ck as usize] == owner)
        })
    };
    let payload = |i: u32| match b.read_step(i as usize) {
        Some((_, Step::Variant(v, f))) => Some((v, f)),
        _ => None,
    };
    let mut out = HashMap::default();
    let mut reads = ArmReads::default();
    let cleanup = defers(b, sub);
    for (&source, us) in users {
        let Some(&root) = us.iter().find(|&&r| {
            matches!(b.tags[r as usize], Tag::Match | Tag::SwitchTag)
                && us.iter().all(|&u| {
                    (u == r || payload(u).is_some() || b.tags[u as usize] == Tag::SwitchTag)
                        && inside(r, u)
                        && !cyclic(r, u)
                })
        }) else {
            continue;
        };
        // The source is made before this region, at the same loop
        // frequency. A decision inside a loop must not drain a temporary
        // that was computed only once outside that loop.
        if !tree.dominates(b, source, root)
            || tree.path(b, root).is_none_or(|path| {
                path.iter().any(|(k, _)| {
                    let owner = tree.owner[*k as usize];
                    owner != NONE && b.tags[owner as usize] == Tag::Loop && !inside(owner, source)
                })
            })
        {
            continue;
        }
        let fields: Vec<_> = us
            .iter()
            .filter_map(|&i| payload(i).map(|key| (i, key)))
            .collect();
        let unique: Vec<_> = fields
            .iter()
            .filter(|(_, key)| fields.iter().filter(|(_, other)| other == key).count() == 1)
            .copied()
            .collect();
        for &(i, (variant, _)) in &unique {
            let prior = unique
                .iter()
                .filter(|(p, (v, _))| *v == variant && (*p == i || before(*p, i)))
                .map(|(p, (_, field))| (*p, *field))
                .collect();
            let last = us
                .iter()
                .all(|&u| u == root || u == i || before(u, i) || exclusive(u, i));
            out.insert(
                i,
                PayloadMove {
                    fields: prior,
                    last,
                },
            );

            // A pattern binding read once in its chosen arm can pass its
            // own count to that read. Keep the enclosing cleanup: a failed
            // guard may have assigned it without ever entering this arm.
            let Some([set]) = users.get(&i).map(Vec::as_slice) else {
                continue;
            };
            if cleanup || b.tags[root as usize] != Tag::Match {
                continue;
            }
            let Some((l, _)) = local_store(b, *set as usize) else {
                continue;
            };
            let Some(xs) = access.get(&l).filter(|xs| xs.len() == 2) else {
                continue;
            };
            let Some(&get) = xs.iter().find(|&&x| b.local_read(x as usize).is_some()) else {
                continue;
            };
            let arms = &b.record(b.data[root as usize][1])[1..];
            if !cyclic(root, get)
                && arms.iter().any(|&arm| {
                    tree.path(b, get)
                        .is_some_and(|p| p.iter().any(|&(k, _)| k == arm))
                })
            {
                // This is the read's arm statement, not the outer drop's
                // position, so take_local does not skip the guard fallback's
                // cleanup. It still zeroes the local on the taking path.
                reads.insert(get, (l, tree.parent[get as usize]));
            }
        }
    }
    (out, reads)
}

/// The releases of sub-body `sub`'s values and locals at their last uses,
/// and the uses that may move a count instead ([`Moves`]). `entry` are the
/// locals that hold a value from the prologue: the parameters, and a
/// closure's captures.
pub(super) fn plan(b: &Body, sub: u32, entry: &[usize], projs: &[Projected]) -> (Drops, Moves) {
    let tree = Tree::new(b);
    let subs = b.inst_subs();
    let mut drops = Drops::default();
    let mut moves = Moves::default();
    let mut users: HashMap<u32, Vec<u32>> = HashMap::default();
    let mut access: HashMap<usize, Vec<u32>> = HashMap::default();
    for (i, &s) in subs.iter().enumerate() {
        if s != sub {
            continue;
        }
        let x = u32::try_from(i).expect("insts");
        for r in uses(b, i) {
            users.entry(r).or_default().push(x);
        }
        match b.tags[i] {
            // A place's root local is an access, a plain local read's
            // among them.
            Tag::Ref | Tag::Read | Tag::Store => {
                if let Some(l) = place_local(b, i) {
                    access.entry(l).or_default().push(x);
                }
            }
            Tag::Closure => {
                let rec = b.record(b.data[i][1]);
                let (start, n) = (rec[0] as usize, rec[1] as usize);
                for l in &b.cap_local[start..start + n] {
                    access.entry(l.raw() as usize).or_default().push(x);
                }
            }
            _ => {}
        }
    }
    let (payloads, arm_reads) = match_payloads(b, sub, &tree, &users, &access);
    moves.payloads = payloads;
    // Values: the last statement of the defining block that holds a use.
    for (i, &s) in subs.iter().enumerate() {
        if s != sub || b.tags[i] == Tag::Block {
            continue;
        }
        let x = u32::try_from(i).expect("insts");
        let (blk, def) = tree.parent[i];
        if blk == NONE {
            continue;
        }
        let mut last = def;
        let mut ok = true;
        let us = users.get(&x).map_or(&[][..], Vec::as_slice);
        if us.is_empty() && read_through(b, i, projs).is_some() {
            moves.unused_reads.insert(x);
        }
        if let [u] = *us {
            if b.tags[u as usize] == Tag::Block {
                if u == blk && b.data[u as usize][1] == x {
                    moves.tails.insert(x);
                }
            } else if tree.parent[u as usize].0 == blk && tree.parent[u as usize].1 > def {
                moves.sole.insert(x, (u, tree.parent[u as usize]));
            }
        }
        for &u in us {
            match tree
                .path(b, u)
                .and_then(|p| p.into_iter().find(|(k, _)| *k == blk))
            {
                Some((_, p)) => last = last.max(p),
                None => ok = false,
            }
        }
        if ok {
            drops.entry((blk, last)).or_default().push(Drop::Val(x));
        }
    }
    // Locals: the innermost block that holds every access, after the last
    // statement there that holds one. By local, so locals released at one
    // statement are released in the same order in every build.
    let mut access: Vec<(usize, Vec<u32>)> = access.into_iter().collect();
    access.sort_unstable();
    for (l, xs) in access {
        let paths: Option<Vec<Vec<(u32, usize)>>> = xs.iter().map(|&x| tree.path(b, x)).collect();
        let Some(paths) = paths else {
            continue;
        };
        let Some(first) = paths.first() else {
            continue;
        };
        // The blocks of the first path, outermost first, that every path
        // passes through; the last of them is the innermost common one.
        let common = first
            .iter()
            .rev()
            .take_while(|(k, _)| paths.iter().all(|p| p.iter().any(|(q, _)| q == k)));
        let Some(&(mut blk, _)) = common.last() else {
            continue;
        };
        // A parameter or a capture holds its value from the prologue, so
        // it lives in the root block, never only in a loop's body.
        if entry.contains(&l) {
            blk = b.sub_root[sub as usize];
        }
        let last = paths
            .iter()
            .filter_map(|p| p.iter().find(|(k, _)| *k == blk).map(|(_, p)| *p))
            .max()
            .unwrap_or(0);
        // The statement at `last` is a read of the local itself, or a
        // reference whose place is the local: nothing else in it, or
        // after it, reaches the local.
        if let Some(&x) = xs.iter().find(|&&x| tree.parent[x as usize] == (blk, last))
            && (b.local_read(x as usize).is_some() || ref_local(b, x as usize) == Some(l))
        {
            moves.last_access.insert(x, (l, (blk, last)));
        }
        drops.entry((blk, last)).or_default().push(Drop::Local(l));
    }
    (moves.loans, moves.loan_locals) = loans(b, sub, &tree, &drops, &users, projs);
    moves.span = span_loans(b, sub, &tree, &drops, &users);
    for (get, last) in arm_reads {
        moves.last_access.entry(get).or_insert(last);
    }
    (drops, moves)
}

/// Whether sub-body `sub` registers cleanup that runs later than where it
/// stands (`defer`, a scope's cleanup). Its counts stay as they are, since
/// the lifetime plan does not model that edge.
fn defers(b: &Body, sub: u32) -> bool {
    b.inst_subs().iter().enumerate().any(|(i, of)| {
        *of == sub
            && (b.tags[i] == Tag::Defer
                || (b.tags[i] == Tag::Scope
                    && b.data[i][1] != NONE
                    && !b.record(b.data[i][1]).is_empty()))
    })
}

/// Span loans (`crates/hd_wasm_linear/DESIGN.md` §23.36, issue #676): a
/// read of a local borrows the local's count, taking no count of its own,
/// when nothing between the read and its last use can drop or replace the
/// local.
///
/// The read is a `Read [$l]` of local `l` at statement `s` of block `k`.
/// Its span is the statements of `k` after `s` up to the last one that
/// holds a use of the read, with every block those statements run. The
/// read borrows when:
///
/// 1. No reference or closure reaches `l`: no `Ref` starts
///    from it and no closure captures it. Then a call, a write through a
///    reference or a global cannot reach `l`, and only this body's own
///    instructions change it. (Emission also refuses a local that lives in
///    a cell, and an open value, whose thunk may make a fresh box.)
/// 2. No `Store` into all of `l` ([`local_store`])
///    stands in the span: that is the only instruction that replaces `l`,
///    and it releases the old value.
/// 3. `l`'s planned release ([`plan`]) is not inside the span: it is in a
///    block around `k`, or in `k` at or after the span's last statement.
///    The release after `l`'s last access is the only other point that
///    drops `l`'s count, and moving that count into a read
///    (`Moves::last_access`) happens at the same statement.
/// 4. No suspension point stands in the span, so the frame never holds
///    the read's value, and the sub-body registers no deferred cleanup.
///
/// A jump out of the span leaves the read's remaining uses behind, so it
/// needs no rule. A change made in place through the value (a list push
/// or element store) keeps its header, which is the read's whole value
/// (§23.7), and copy-on-write counts the buffer, not the header. A use
/// that keeps the value (an owned argument, a store, a return) still takes
/// a count of its own, because a borrowed read has none to move.
fn span_loans(
    b: &Body,
    sub: u32,
    tree: &Tree,
    drops: &Drops,
    users: &HashMap<u32, Vec<u32>>,
) -> HashSet<u32> {
    let mut out = HashSet::default();
    if defers(b, sub) {
        return out;
    }
    let subs = b.inst_subs();
    let mut reached = HashSet::default();
    let mut sets: HashMap<usize, Vec<u32>> = HashMap::default();
    let mut points = Vec::new();
    for (i, &of) in subs.iter().enumerate() {
        let x = u32::try_from(i).expect("insts");
        match b.tags[i] {
            Tag::Ref => {
                reached.extend(ref_locals(b, i).into_iter().map(|l| l as usize));
            }
            Tag::Closure => {
                let r = b.record(b.data[i][1]);
                reached.extend(
                    b.cap_local[r[0] as usize..(r[0] + r[1]) as usize]
                        .iter()
                        .map(|l| l.raw() as usize),
                );
            }
            t if of == sub && super::susp::is_point(t) => points.push(x),
            _ => {}
        }
        if of == sub
            && let Some((l, _)) = local_part_store(b, i)
        {
            sets.entry(l).or_default().push(x);
        }
    }
    let ends: HashMap<usize, (u32, usize)> = drops
        .iter()
        .flat_map(|(at, ds)| {
            ds.iter().filter_map(move |d| match d {
                Drop::Local(l) => Some((*l, *at)),
                Drop::Val(_) => None,
            })
        })
        .collect();
    // The position in block `k` of the statement that holds `x`.
    let at = |x: u32, k: u32| {
        tree.path(b, x)
            .and_then(|p| p.into_iter().find(|(of, _)| *of == k))
            .map(|(_, p)| p)
    };
    for (i, &of) in subs.iter().enumerate() {
        if of != sub {
            continue;
        }
        let Some(l) = b.local_read(i).map(hd_base::LocalId::idx) else {
            continue;
        };
        let x = u32::try_from(i).expect("insts");
        let (k, s) = tree.parent[i];
        if k == NONE || reached.contains(&l) {
            continue;
        }
        let Some(us) = users.get(&x).filter(|us| !us.is_empty()) else {
            continue;
        };
        // The span's last statement; every use after the read.
        let Some(end) = us
            .iter()
            .map(|&u| at(u, k).filter(|p| *p > s))
            .collect::<Option<Vec<usize>>>()
            .and_then(|ps| ps.into_iter().max())
        else {
            continue;
        };
        let inside = |y: &u32| at(*y, k).is_some_and(|p| s < p && p <= end);
        if sets.get(&l).is_some_and(|v| v.iter().any(inside)) || points.iter().any(inside) {
            continue;
        }
        if let Some(&(dk, dp)) = ends.get(&l) {
            // Released in a block around `k`, or in `k` once the span
            // has ended; anywhere else the release may fall inside it.
            let around = dk != k && at(x, dk).is_some();
            let after = dk == k && dp >= end;
            if !(around || after) {
                continue;
            }
        }
        out.insert(x);
    }
    out
}

/// Borrow only from an unaliased, unchanged local whose existing lifetime
/// strictly contains every use. Never extend its release: doing so could
/// keep another field's element reference pinned beyond its last use.
fn loans(
    b: &Body,
    sub: u32,
    tree: &Tree,
    drops: &Drops,
    users: &HashMap<u32, Vec<u32>>,
    projs: &[Projected],
) -> (HashSet<u32>, HashMap<usize, u32>) {
    fn source(
        b: &Body,
        x: u32,
        stable: &HashSet<usize>,
        bound: &HashMap<usize, usize>,
        sets: &HashMap<usize, Vec<u32>>,
        (depth, projs): (usize, &[Projected]),
    ) -> Option<usize> {
        if depth > 32 || x == NONE || Ref(x).as_inst().is_none() {
            return None;
        }
        let [a, w] = b.data[x as usize];
        let next = match b.tags[x as usize] {
            Tag::Read if let Some(l) = b.local_read(x as usize) => {
                let l = l.idx();
                return bound
                    .get(&l)
                    .copied()
                    .or_else(|| stable.contains(&l).then_some(l));
            }
            Tag::Read if let Some((v, _)) = projection(b, x as usize, projs) => v,
            Tag::Read if let Some((list, _)) = b.element_read(x as usize) => {
                return snapshot_source(b, list, stable, bound, sets, (depth, projs));
            }
            Tag::Intrinsic => match IntrinsicOp::from_u32(a) {
                Some(IntrinsicOp::ValueCopy) => *b.record(w).first()?,
                Some(IntrinsicOp::MapSlotEntry) => {
                    let map = *b.record(w).first()?;
                    return snapshot_source(b, map, stable, bound, sets, (depth, projs));
                }
                _ => return None,
            },
            _ => return None,
        };
        source(b, next, stable, bound, sets, (depth + 1, projs))
    }
    /// The source of collection `c` that a list element or a map entry
    /// is read from, when it is the checker's private loop snapshot.
    fn snapshot_source(
        b: &Body,
        c: u32,
        stable: &HashSet<usize>,
        bound: &HashMap<usize, usize>,
        sets: &HashMap<usize, Vec<u32>>,
        (depth, projs): (usize, &[Projected]),
    ) -> Option<usize> {
        let root = source(b, c, stable, bound, sets, (depth + 1, projs))?;
        let [init] = sets.get(&root)?.as_slice() else {
            return None;
        };
        // The checker's private loop snapshot owns its buffer until
        // SnapEnd; user code cannot mutate that header.
        (b.tags[*init as usize] == Tag::Intrinsic
            && IntrinsicOp::from_u32(b.data[*init as usize][0]) == Some(IntrinsicOp::SnapBegin))
        .then_some(root)
    }
    fn loop_read(b: &Body, x: u32, depth: usize, projs: &[Projected]) -> bool {
        if depth > 32 || x == NONE || Ref(x).as_inst().is_none() {
            return false;
        }
        let [a, w] = b.data[x as usize];
        match b.tags[x as usize] {
            Tag::Read if b.element_read(x as usize).is_some() => true,
            Tag::Read => projection(b, x as usize, projs)
                .is_some_and(|(v, _)| loop_read(b, v, depth + 1, projs)),
            Tag::Intrinsic => match IntrinsicOp::from_u32(a) {
                Some(IntrinsicOp::MapSlotEntry) => true,
                Some(IntrinsicOp::ValueCopy) => b
                    .record(w)
                    .first()
                    .is_some_and(|r| loop_read(b, *r, depth + 1, projs)),
                _ => false,
            },
            _ => false,
        }
    }
    let subs = b.inst_subs();
    // Deferred uses run later than their structural registration point.
    // Leave their counts alone until the lifetime plan models that edge.
    if subs.iter().enumerate().any(|(i, of)| {
        *of == sub
            && (b.tags[i] == Tag::Defer
                || (b.tags[i] == Tag::Scope
                    && b.data[i][1] != NONE
                    && !b.record(b.data[i][1]).is_empty()))
    }) {
        return (HashSet::default(), HashMap::default());
    }
    let mut sets: HashMap<usize, Vec<u32>> = HashMap::default();
    let mut escapes = HashSet::default();
    let mut accesses: HashMap<usize, Vec<u32>> = HashMap::default();
    for (i, &of) in subs.iter().enumerate() {
        if of != sub {
            continue;
        }
        let x = u32::try_from(i).expect("insts");
        if let Some((l, v)) = local_store(b, i) {
            accesses.entry(l).or_default().push(x);
            sets.entry(l).or_default().push(v);
            continue;
        }
        // A store into a field of the local changes it too; it binds no
        // value of its own (the store is no read of a loop's element).
        if let Some((l, _)) = local_part_store(b, i) {
            accesses.entry(l).or_default().push(x);
            sets.entry(l).or_default().push(x);
            continue;
        }
        match b.tags[i] {
            // A plain read of a local, and a read or store through the
            // reference a local holds, read the local.
            Tag::Read | Tag::Store => {
                if let Some(l) = place_local(b, i) {
                    accesses.entry(l).or_default().push(x);
                }
            }
            Tag::Ref => {
                escapes.extend(ref_locals(b, i).into_iter().map(|l| l as usize));
            }
            Tag::Closure => {
                let r = b.record(b.data[i][1]);
                escapes.extend(
                    b.cap_local[r[0] as usize..(r[0] + r[1]) as usize]
                        .iter()
                        .map(|l| l.raw() as usize),
                );
            }
            _ => {}
        }
    }
    let stable: HashSet<usize> = (0..b.local_ty.len())
        .filter(|l| {
            !escapes.contains(l)
                && b.local_flags[*l]
                    & (local_flags::MUTATED
                        | local_flags::CAPTURED
                        | local_flags::CAPTURED_ASSIGNED)
                    == 0
                && sets.get(l).is_none_or(|v| v.len() == 1)
        })
        .collect();
    let ends: HashMap<usize, (u32, usize)> = drops
        .iter()
        .flat_map(|(at, ds)| {
            ds.iter().filter_map(move |d| match d {
                Drop::Local(l) => Some((*l, *at)),
                Drop::Val(_) => None,
            })
        })
        .collect();
    let held = |root: usize, reads: &[u32]| {
        let Some(&(blk, end)) = ends.get(&root) else {
            return false;
        };
        reads.iter().all(|&u| {
            tree.path(b, u)
                .and_then(|path| path.into_iter().find(|(of, _)| *of == blk))
                .is_some_and(|(_, at)| at < end)
        })
    };
    // Borrow bindings only directly from the private loop snapshot.
    // General aliases keep counts: re-borrowing through a binding that
    // later loses ownership would otherwise require a transitive proof.
    let empty = HashMap::default();
    let mut bound = HashMap::default();
    for (&l, values) in &sets {
        let [v] = values.as_slice() else {
            continue;
        };
        if !stable.contains(&l) || !loop_read(b, *v, 0, projs) {
            continue;
        }
        if let Some(root) = source(b, *v, &stable, &empty, &sets, (0, projs))
            && root != l
            && held(root, &accesses[&l])
        {
            bound.insert(l, root);
        }
    }
    let locals = bound
        .keys()
        .filter_map(|l| sets.get(l).map(|v| (*l, v[0])))
        .collect();
    let mut reads = HashSet::default();
    for (i, &of) in subs.iter().enumerate() {
        if of != sub {
            continue;
        }
        let x = u32::try_from(i).expect("insts");
        if let Some(root) = source(b, x, &stable, &bound, &sets, (0, projs))
            && users
                .get(&x)
                .is_some_and(|us| !us.is_empty() && held(root, us))
        {
            reads.insert(x);
        }
    }
    (reads, locals)
}

/// A field span and the reads/bindings borrowing from it. Keep these
/// separate from `Moves::span`, whose entries are local reads only.
pub(super) struct FieldSpan {
    pub field: u32,
    pub reads: HashSet<u32>,
    pub locals: HashMap<usize, u32>,
}

/// A caller-backed field can cross match decisions when the entire span
/// is free of calls, reference stores, suspension and root replacement.
/// Follow pattern bindings to their last use, rather than stopping at the
/// scrutinee's test. Owned consumers still take their own count.
pub(super) fn field_spans(
    b: &Body,
    fields: &[u32],
    roots: &HashSet<usize>,
    projs: &[Projected],
) -> Vec<FieldSpan> {
    if fields.is_empty() || b.tags.iter().any(|t| matches!(t, Tag::Defer | Tag::Scope)) {
        return Vec::new();
    }
    let tree = Tree::new(b);
    let mut users: HashMap<u32, Vec<u32>> = HashMap::default();
    let mut sets: HashMap<usize, Vec<u32>> = HashMap::default();
    let mut gets: HashMap<usize, Vec<u32>> = HashMap::default();
    for i in 0..b.len() {
        let x = u32::try_from(i).expect("insts");
        for r in uses(b, i) {
            users.entry(r).or_default().push(x);
        }
        if let Some((l, _)) = local_part_store(b, i) {
            sets.entry(l).or_default().push(x);
        } else if let Some(l) = b.local_read(i) {
            gets.entry(l.idx()).or_default().push(x);
        }
    }
    let mut spans = Vec::new();
    for &field in fields {
        let (blk, start) = tree.parent[field as usize];
        if blk == NONE
            || !users
                .get(&field)
                .is_some_and(|us| us.iter().any(|&u| b.tags[u as usize] == Tag::Match))
        {
            continue;
        }
        let mut span = FieldSpan {
            field,
            reads: HashSet::from_iter([field]),
            locals: HashMap::default(),
        };
        let mut pending = vec![field];
        let mut touched = HashSet::default();
        while let Some(source) = pending.pop() {
            for &u in users.get(&source).into_iter().flatten() {
                touched.insert(u);
                let i = u as usize;
                match b.tags[i] {
                    Tag::Read if projection(b, i, projs).is_some_and(|(v, _)| v == source) => {
                        if span.reads.insert(u) {
                            pending.push(u);
                        }
                    }
                    // The source stored into a whole local.
                    _ if let Some((l, v)) = local_store(b, i)
                        && v == source =>
                    {
                        let local = u32::try_from(l).expect("locals");
                        if sets.get(&l).is_none_or(|ss| ss.len() != 1)
                            || b.local_flags[l]
                                & (local_flags::MUTATED
                                    | local_flags::CAPTURED
                                    | local_flags::CAPTURED_ASSIGNED)
                                != 0
                            || (0..b.len()).any(|j| ref_locals(b, j).contains(&local))
                        {
                            continue;
                        }
                        span.locals.insert(l, source);
                        for &read in gets.get(&l).into_iter().flatten() {
                            if span.reads.insert(read) {
                                pending.push(read);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let position = |x| {
            tree.path(b, x)
                .and_then(|path| path.into_iter().find(|(k, _)| *k == blk))
                .map(|(_, p)| p)
        };
        let Some(end) = touched
            .iter()
            .chain(&span.reads)
            .map(|&x| position(x).filter(|p| *p >= start))
            .collect::<Option<Vec<_>>>()
            .and_then(|ps| ps.into_iter().max())
        else {
            continue;
        };
        let safe = (0..b.len()).all(|i| {
            let x = u32::try_from(i).expect("insts");
            if !position(x).is_some_and(|p| start <= p && p <= end) {
                return true;
            }
            match b.tags[i] {
                Tag::Call
                | Tag::CallValue
                | Tag::CallDyn
                | Tag::CallHost
                | Tag::DefaultCall
                | Tag::Closure
                | Tag::Defer
                | Tag::Scope
                | Tag::Hook => false,
                t if super::susp::is_point(t) => false,
                // A store into a local, whole or a field of it, replaces
                // only that local; any other store, through a reference
                // among them, may replace the field.
                Tag::Store => local_part_store(b, i).is_some_and(|(l, _)| !roots.contains(&l)),
                Tag::Intrinsic => matches!(
                    IntrinsicOp::from_u32(b.data[i][0]),
                    Some(IntrinsicOp::ValueCopy | IntrinsicOp::ListLen)
                ),
                _ => true,
            }
        });
        // Do not borrow a binding beyond the enclosing match: a typed
        // view's owned field fallback can be released after that decision.
        let bounded = span.locals.keys().all(|l| {
            gets.get(l).into_iter().flatten().all(|&read| {
                users.get(&field).into_iter().flatten().any(|&u| {
                    b.tags[u as usize] == Tag::Match
                        && tree.path(b, read).is_some_and(|path| {
                            path.iter().any(|(k, _)| tree.owner[*k as usize] == u)
                        })
                })
            })
        });
        if safe && bounded {
            spans.push(span);
        }
    }
    spans
}

/// Additional reads backed by a parameter the caller keeps alive. Referent
/// reads stay within one straight-line interval without an invalidating call
/// or store; copying the reference parameter itself needs no such interval.
/// A field of such a referent read borrows over the same kind of interval
/// (§23.36). A list header read may see an element store or a push in its
/// interval: both change the buffer and keep the header.
///
/// `refs` are the parameters that hold references: a place that
/// dereferences one of them is inside what the caller keeps alive.
pub(super) fn parameter_loans(
    b: &Body,
    parameters: &HashSet<usize>,
    refs: &HashSet<usize>,
    headers: &HashSet<u32>,
    projs: &[Projected],
) -> HashSet<u32> {
    fn root(b: &Body, x: u32, parameters: &HashSet<usize>, refs: &HashSet<usize>) -> bool {
        if x == NONE || Ref(x).as_inst().is_none() {
            return false;
        }
        match b.tags[x as usize] {
            Tag::Read => read_local(b, x).is_some_and(|l| parameters.contains(&l)),
            // A place through such a reference, held by a local or read
            // from one (`[*%r ..]` where `%r` reads it).
            Tag::Ref => b.decode_place(b.data[x as usize][0]).is_some_and(|p| {
                p.proj.first() == Some(&Proj::Deref)
                    && match p.root {
                        PlaceRoot::Local(l) => refs.contains(&l.idx()),
                        PlaceRoot::Value(r) => {
                            read_local(b, r.0).is_some_and(|l| refs.contains(&l))
                        }
                        _ => false,
                    }
            }),
            _ => false,
        }
    }
    if parameters.is_empty() || b.tags.iter().any(|t| matches!(t, Tag::Defer | Tag::Scope)) {
        return HashSet::default();
    }
    let tree = Tree::new(b);
    let mut users: HashMap<u32, Vec<u32>> = HashMap::default();
    for i in 0..b.len() {
        for r in uses(b, i) {
            users
                .entry(r)
                .or_default()
                .push(u32::try_from(i).expect("insts"));
        }
    }
    let mut out = HashSet::default();
    for i in 0..b.len() {
        let x = u32::try_from(i).expect("insts");
        if b.local_read(i).is_some() && root(b, x, parameters, refs) {
            out.insert(x);
            continue;
        }
        // This is an address inside a caller-owned place, even if a call
        // changes the value stored there. Views retain their owned fallback.
        if matches!(b.tags[i], Tag::Ref) && root(b, x, parameters, refs) {
            out.insert(x);
            continue;
        }
        if !match b.tags[i] {
            // A field of such a read stays in the caller-owned place over
            // the same kind of interval; emission keeps it only where the
            // read loads that one field (`Em::prepare_parameter_loans`).
            Tag::Read if let Some((base, Projected::Field)) = projection(b, i, projs) => {
                Ref(base).as_inst().is_some()
                    && matches!(b.tags[base as usize], Tag::Read)
                    && read_local(b, base).is_none()
                    && out.contains(&base)
            }
            Tag::Read => match read_through(b, i, projs) {
                Some((Through::Value(source), _)) => {
                    root(b, source, parameters, refs)
                        && (read_local(b, source).is_some() || out.contains(&source))
                }
                Some((Through::Local(l), _)) => refs.contains(&l),
                None => false,
            },
            _ => false,
        } {
            continue;
        }
        let (blk, start) = tree.parent[i];
        let Some(us) = users.get(&x).filter(|us| !us.is_empty()) else {
            continue;
        };
        if blk == NONE || us.iter().any(|u| tree.parent[*u as usize].0 != blk) {
            continue;
        }
        let end = us
            .iter()
            .map(|u| tree.parent[*u as usize].1)
            .max()
            .unwrap_or(start);
        if end <= start {
            continue;
        }
        let safe = b.record(b.data[blk as usize][0])[start + 1..=end]
            .iter()
            .all(|j| {
                let j = *j as usize;
                match b.tags[j] {
                    // Stripped reference closure forms leave empty blocks
                    // in the containing statement list; they do not run.
                    Tag::Block => b.data[j][1] == NONE && b.record(b.data[j][0]).is_empty(),
                    Tag::Prim
                    | Tag::NewStruct
                    | Tag::CopyStruct
                    | Tag::NewTuple
                    | Tag::NewVariant
                    | Tag::Coerce
                    | Tag::Read
                    | Tag::Ref => true,
                    Tag::Store if local_store(b, j).is_some() => {
                        us.contains(&u32::try_from(j).expect("insts")) && tree.parent[j].1 == end
                    }
                    Tag::Store if headers.contains(&x) && elem_store_list(b, j).is_some() => true,
                    Tag::Intrinsic
                        if headers.contains(&x)
                            && matches!(
                                IntrinsicOp::from_u32(b.data[j][0]),
                                Some(IntrinsicOp::ListIndexMut | IntrinsicOp::ListPush)
                            ) =>
                    {
                        true
                    }
                    Tag::Intrinsic => matches!(
                        IntrinsicOp::from_u32(b.data[j][0]),
                        Some(IntrinsicOp::ListLen | IntrinsicOp::ValueCopy)
                    ),
                    _ => false,
                }
            });
        if safe {
            out.insert(x);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A body of one local: a store into it, a reference to it, a read
    /// and a write through that reference, then a read of the local.
    fn reference_body() -> Body {
        use hd_tir::ir::Place;
        fn record(b: &mut Body, words: &[u32]) -> u32 {
            let at = u32::try_from(b.extra.len()).unwrap();
            b.extra.push(u32::try_from(words.len()).unwrap());
            b.extra.extend_from_slice(words);
            at
        }
        fn inst(b: &mut Body, tag: Tag, a: u32, w: u32) -> u32 {
            let i = u32::try_from(b.tags.len()).unwrap();
            b.tags.push(tag);
            b.data.push([a, w]);
            b.ty.push(hd_types::Ty::VOID);
            b.syn.push(hd_base::NodeIdx::NONE);
            i
        }
        fn place(b: &mut Body, p: &Place) -> u32 {
            match (p.words(), p.root) {
                (Some(words), _) => record(b, &words),
                (None, PlaceRoot::Local(l)) => l.raw() | hd_tir::ir::place::BARE_LOCAL,
                (None, _) => unreachable!("only a bare local has no record"),
            }
        }
        let mut b = Body::new(hd_base::DefId::from_raw(0), hd_tir::ir::BodyKind::Fn);
        b.local_ty.push(hd_types::Ty::VOID);
        b.local_name.push(hd_base::Symbol::from_raw(0));
        b.local_syn.push(hd_base::NodeIdx::NONE);
        b.local_flags.push(local_flags::ASSIGNED);
        let local = Place::local(hd_base::LocalId::from_raw(0));
        let none = record(&mut b, &[]);
        let value = inst(&mut b, Tag::NewTuple, NONE, none);
        let mut list = vec![value];
        let at = place(&mut b, &local);
        list.push(inst(&mut b, Tag::Store, at, value));
        let r = inst(&mut b, Tag::Ref, at, 1);
        let through = Place {
            root: PlaceRoot::Value(Ref(r)),
            proj: vec![Proj::Deref],
        };
        let at = place(&mut b, &through);
        let read = inst(&mut b, Tag::Read, at, 0);
        list.extend([r, read, inst(&mut b, Tag::Store, at, value)]);
        list.push(inst(&mut b, Tag::Read, hd_tir::ir::place::BARE_LOCAL, 0));
        let list = record(&mut b, &list);
        let root = inst(&mut b, Tag::Block, list, NONE);
        b.sub_root.push(root);
        b.sub_parent.push(hd_base::SubId::NONE);
        b.sub_flags.push(0);
        b.sub_params.push(hd_base::Range32::default());
        b
    }

    /// The place helpers name the locals of `Ref`, `Read` and `Store`: a
    /// store into the whole local, the reference's root, and a read and
    /// a write through the reference, which reach no local.
    #[test]
    fn places_name_their_locals() {
        let new = reference_body();
        assert_eq!(local_store(&new, 1), Some((0, 0)));
        assert_eq!(place_local(&new, 1), Some(0));
        assert_eq!(ref_local(&new, 2), Some(0));
        assert_eq!(ref_locals(&new, 2), [0]);
        assert_eq!(read_through(&new, 3, &[]), Some((Through::Value(2), true)));
        assert_eq!(place_local(&new, 4), None);
        assert_eq!(local_part_store(&new, 4), None);
    }

    /// The same decision inside a loop may drain a value made on each
    /// iteration, but must never drain a value made once before the loop.
    #[test]
    fn matched_source_must_be_recomputed_on_each_loop_iteration() {
        fn record(b: &mut Body, words: &[u32]) -> u32 {
            let at = u32::try_from(b.extra.len()).unwrap();
            b.extra.push(u32::try_from(words.len()).unwrap());
            b.extra.extend_from_slice(words);
            at
        }
        fn inst(b: &mut Body, tag: Tag, a: u32, w: u32) -> u32 {
            let i = u32::try_from(b.tags.len()).unwrap();
            b.tags.push(tag);
            b.data.push([a, w]);
            b.ty.push(hd_types::Ty::VOID);
            b.syn.push(hd_base::NodeIdx::NONE);
            i
        }
        for each_iteration in [false, true] {
            let mut b = Body::new(hd_base::DefId::from_raw(0), hd_tir::ir::BodyKind::Fn);
            let empty = record(&mut b, &[]);
            let source = inst(&mut b, Tag::NewVariant, 0, empty);
            let field = record(
                &mut b,
                &[
                    hd_tir::ir::place::ROOT_VALUE,
                    source,
                    hd_tir::ir::place::PROJ_VARIANT,
                    hd_tir::ir::place::PROJ_FIELD,
                ],
            );
            let payload = inst(&mut b, Tag::Read, field, 0);
            let taken = record(&mut b, &[payload]);
            let then = inst(&mut b, Tag::Block, taken, payload);
            let other = inst(&mut b, Tag::Block, empty, NONE);
            let choices = record(&mut b, &[0, then, other]);
            let decision = inst(&mut b, Tag::SwitchTag, source, choices);
            let iterations = if each_iteration {
                vec![source, decision]
            } else {
                vec![decision]
            };
            let iterations = record(&mut b, &iterations);
            let body = inst(&mut b, Tag::Block, iterations, NONE);
            let looping = inst(&mut b, Tag::Loop, body, NONE);
            let top = if each_iteration {
                vec![looping]
            } else {
                vec![source, looping]
            };
            let top = record(&mut b, &top);
            let root = inst(&mut b, Tag::Block, top, NONE);
            b.sub_root.push(root);
            b.sub_parent.push(hd_base::SubId::NONE);
            b.sub_flags.push(0);
            b.sub_params.push(hd_base::Range32::default());
            let mut projs = vec![Projected::None; b.len()];
            projs[payload as usize] = Projected::Payload;
            let (_, moves) = plan(&b, 0, &[], &projs);
            assert_eq!(moves.payloads.contains_key(&payload), each_iteration);
        }
    }
}
