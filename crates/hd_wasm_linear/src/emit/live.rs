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
use hd_tir::ir::{Body, IntrinsicOp, NONE, Op, Ref, Tag, local_flags};

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
    /// Each instruction value whose only use is one later statement of its
    /// own block: that statement runs exactly once after the value is made
    /// and nothing reads the value after it, so a consuming use there can
    /// take the value's count. With the use, the statement after which the
    /// value's release was planned.
    pub sole: HashMap<u32, (u32, (u32, usize))>,
    /// Each `LocalGet` that is its local's last access: the statement
    /// after which the local is released ([`plan`]), so the value it makes
    /// can take the local's count instead of a count of its own. With the
    /// local and that statement.
    pub last_get: HashMap<u32, (usize, (u32, usize))>,
    /// Each block tail whose only use is as its block's value.
    pub tails: hd_base::HashSet<u32>,
    /// Reads whose source remains owned past every use. Emission filters
    /// these candidates by physical representation before borrowing them.
    pub loans: HashSet<u32>,
    /// Single-assignment bindings of such reads, with their source value.
    pub loan_locals: HashMap<usize, u32>,
}

/// The block structure of a body: each instruction's block and position,
/// and each block's owning instruction.
struct Tree {
    /// The block whose list holds each instruction, and its position.
    parent: Vec<(u32, usize)>,
    /// The instruction that runs each block (an `If`, a `Loop`, ...).
    owner: Vec<u32>,
}

impl Tree {
    fn new(b: &Body) -> Tree {
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
    fn path(&self, b: &Body, mut x: u32) -> Option<Vec<(u32, usize)>> {
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
}

/// The instruction values instruction `i` reads: TIR's own list where it
/// has one, else every operand word that names an instruction, an
/// over-approximation that only ever delays a release.
fn uses(b: &Body, i: usize) -> Vec<u32> {
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
                    if word != NONE
                        && matches!(
                            tag,
                            Tag::CallDyn
                                | Tag::Await
                                | Tag::NewList
                                | Tag::ForRange
                                | Tag::ForList
                                | Tag::ForMap
                        ) =>
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

/// The releases of sub-body `sub`'s values and locals at their last uses,
/// and the uses that may move a count instead ([`Moves`]). `entry` are the
/// locals that hold a value from the prologue: the parameters, and a
/// closure's captures.
pub(super) fn plan(b: &Body, sub: u32, entry: &[usize]) -> (Drops, Moves) {
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
            Tag::LocalGet | Tag::LocalSet => {
                access.entry(b.data[i][0] as usize).or_default().push(x);
            }
            // A reference reads the locals its place starts from.
            Tag::RefPlace => {
                for l in b.place_locals(i) {
                    access.entry(l as usize).or_default().push(x);
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
        // The statement at `last` is a read of the local itself: nothing
        // else in it, or after it, reads the local.
        if let Some(&x) = xs.iter().find(|&&x| tree.parent[x as usize] == (blk, last))
            && b.tags[x as usize] == Tag::LocalGet
        {
            moves.last_get.insert(x, (l, (blk, last)));
        }
        drops.entry((blk, last)).or_default().push(Drop::Local(l));
    }
    (moves.loans, moves.loan_locals) = loans(b, sub, &tree, &drops, &users);
    (drops, moves)
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
) -> (HashSet<u32>, HashMap<usize, u32>) {
    fn source(
        b: &Body,
        x: u32,
        stable: &HashSet<usize>,
        bound: &HashMap<usize, usize>,
        sets: &HashMap<usize, Vec<u32>>,
        depth: usize,
    ) -> Option<usize> {
        if depth > 32 || x == NONE || Ref(x).as_inst().is_none() {
            return None;
        }
        let [a, w] = b.data[x as usize];
        let next = match b.tags[x as usize] {
            Tag::LocalGet => {
                let l = a as usize;
                return bound
                    .get(&l)
                    .copied()
                    .or_else(|| stable.contains(&l).then_some(l));
            }
            Tag::Field | Tag::TupleGet | Tag::Payload | Tag::Unwrap => a,
            Tag::Intrinsic => match IntrinsicOp::from_u32(a) {
                Some(IntrinsicOp::ValueCopy) => *b.record(w).first()?,
                Some(IntrinsicOp::ListIndex | IntrinsicOp::MapSlotEntry) => {
                    let root = source(b, *b.record(w).first()?, stable, bound, sets, depth + 1)?;
                    let [init] = sets.get(&root)?.as_slice() else {
                        return None;
                    };
                    // The checker's private loop snapshot owns its buffer
                    // until SnapEnd; user code cannot mutate that header.
                    if b.tags[*init as usize] == Tag::Intrinsic
                        && IntrinsicOp::from_u32(b.data[*init as usize][0])
                            == Some(IntrinsicOp::SnapBegin)
                    {
                        return Some(root);
                    }
                    return None;
                }
                _ => return None,
            },
            _ => return None,
        };
        source(b, next, stable, bound, sets, depth + 1)
    }
    fn loop_read(b: &Body, x: u32, depth: usize) -> bool {
        if depth > 32 || x == NONE || Ref(x).as_inst().is_none() {
            return false;
        }
        let [a, w] = b.data[x as usize];
        match b.tags[x as usize] {
            Tag::Field | Tag::TupleGet | Tag::Payload | Tag::Unwrap => loop_read(b, a, depth + 1),
            Tag::Intrinsic => match IntrinsicOp::from_u32(a) {
                Some(IntrinsicOp::ListIndex | IntrinsicOp::MapSlotEntry) => true,
                Some(IntrinsicOp::ValueCopy) => b
                    .record(w)
                    .first()
                    .is_some_and(|r| loop_read(b, *r, depth + 1)),
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
        match b.tags[i] {
            Tag::LocalGet | Tag::LocalSet => {
                let l = b.data[i][0] as usize;
                accesses.entry(l).or_default().push(x);
                if b.tags[i] == Tag::LocalSet {
                    sets.entry(l).or_default().push(b.data[i][1]);
                }
            }
            Tag::RefPlace => escapes.extend(b.place_locals(i).into_iter().map(|l| l as usize)),
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
        if !stable.contains(&l) || !loop_read(b, *v, 0) {
            continue;
        }
        if let Some(root) = source(b, *v, &stable, &empty, &sets, 0)
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
        if let Some(root) = source(b, x, &stable, &bound, &sets, 0)
            && users
                .get(&x)
                .is_some_and(|us| !us.is_empty() && held(root, us))
        {
            reads.insert(x);
        }
    }
    (reads, locals)
}

/// Additional reads backed by a parameter the caller keeps alive. Referent
/// reads stay within one straight-line interval without an invalidating call
/// or store; copying the reference parameter itself needs no such interval.
pub(super) fn parameter_loans(
    b: &Body,
    parameters: &HashSet<usize>,
    headers: &HashSet<u32>,
) -> HashSet<u32> {
    fn root(b: &Body, x: u32, parameters: &HashSet<usize>) -> bool {
        if x == NONE || Ref(x).as_inst().is_none() {
            return false;
        }
        match b.tags[x as usize] {
            Tag::LocalGet => parameters.contains(&(b.data[x as usize][0] as usize)),
            Tag::RefPlace => b.place(x as usize).is_some_and(|p| {
                p.kind == hd_tir::ir::place_root::DEREF && parameters.contains(&(p.x as usize))
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
        if b.tags[i] == Tag::LocalGet && root(b, x, parameters) {
            out.insert(x);
            continue;
        }
        // This is an address inside a caller-owned place, even if a call
        // changes the value stored there. Views retain their owned fallback.
        if b.tags[i] == Tag::RefPlace && root(b, x, parameters) {
            out.insert(x);
            continue;
        }
        if !match b.tags[i] {
            Tag::RefRead => {
                let source = b.data[i][0];
                root(b, source, parameters)
                    && (b.tags[source as usize] == Tag::LocalGet || out.contains(&source))
            }
            Tag::RefPlace => root(b, x, parameters),
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
                    Tag::LocalGet
                    | Tag::Field
                    | Tag::TupleGet
                    | Tag::Payload
                    | Tag::Unwrap
                    | Tag::Prim
                    | Tag::NewData
                    | Tag::CopyData
                    | Tag::NewTuple
                    | Tag::NewVariant
                    | Tag::Coerce
                    | Tag::RefRead
                    | Tag::RefPlace => true,
                    Tag::LocalSet => {
                        us.contains(&u32::try_from(j).expect("insts")) && tree.parent[j].1 == end
                    }
                    Tag::Intrinsic
                        if headers.contains(&x)
                            && matches!(
                                IntrinsicOp::from_u32(b.data[j][0]),
                                Some(IntrinsicOp::ListSet | IntrinsicOp::ListIndexMut)
                            ) =>
                    {
                        true
                    }
                    Tag::Intrinsic => matches!(
                        IntrinsicOp::from_u32(b.data[j][0]),
                        Some(
                            IntrinsicOp::ListLen | IntrinsicOp::ListIndex | IntrinsicOp::ValueCopy
                        )
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
