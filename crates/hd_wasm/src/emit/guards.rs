//! Index guards proven false (#1689). An inlined std `Index` or `IndexSet`
//! body checks its key against the header's length,
//! `if key >= self.len: panic_with(...)`, and that `If` survives inlining.
//! Where the body's loops prove `key < len`, the guard's condition is
//! false and emission leaves the `If` out; the condition's operands are
//! pure reads, which the Wasm cleanup then drops.
//!
//! The proof is over values, not over the `List` intrinsic:
//!
//! 1. **An unchanged header.** A field read `[*p .f]` through a reference
//!    parameter `p` reads the same value everywhere in the body when the
//!    body cannot write `p`'s referent: it makes no call but the pointer
//!    intrinsics (`ptr_offset`, `ptr_ref`, `ptr_mut`, `ptr_size_of`) and
//!    `panic`, stores through no dereference or element, and writes
//!    through a `Ptr` only into the room of a buffer that lies inline in
//!    `p`'s referent. That room is a block of its own, which the header
//!    is never in (`module.rt.ptr.holds`, `module.rt.buffer.type`), and
//!    with no call nothing else runs while the body does. A shared `&T`
//!    parameter's referent is unchanged whatever the body does: nothing
//!    may write it while the reference lives (`ref.excl.shared`,
//!    `ref.excl.alias.conflict`, `module.rt.ptr.as-ref`). In a loop
//!    (#1706), the same holds of the loop's writes alone, and of a local
//!    in a cell as of a parameter's referent: no store into the cell or
//!    move out of it, and a `Ptr` store goes into a room, never a cell.
//!    The object of a thin `*T` held in such a root's field is a root too
//!    ([`BOX`]): the object is no room and no cell, the only places a
//!    write that keeps a root may reach, so whatever keeps the holder's
//!    root keeps the object while the field holds the box. A dereference
//!    of a value read from the field names that object only when the read
//!    is a `borrow`, which the field keeps while the value lives.
//! 2. **Values by their source.** A read of a local with one dominating
//!    store is the stored value; an identity or weakening coercion, and a
//!    block whose one break carries a value, are that value.
//! 3. **Facts.** `x < len` holds for the guard's key `x` when:
//!    - `x` is read from a loop's counter after the loop's test
//!      `cur < end`, before the counter's one step in the loop, and `end`
//!      is the same header field;
//!    - `x` is a falling index `v`, or `v - 1` read after a test `v > 0`,
//!      where `v` is set once before its loop and changes only by one
//!      `v = v - 1` after that test, and the value it was set to is below
//!      the length by these facts;
//!    - a guard of the same key and length that runs first panicked
//!      otherwise;
//!    - `x` is `y & (len - 1)`, in either order, where the `&` runs only
//!      once a test showed `len > 0`. Unsigned, `y & m <= m`, and with
//!      `len > 0` the subtraction does not wrap. A test shows it when it
//!      is a comparison that puts `len` above a value (`a < len`,
//!      `len > a`, a failed `a >= len`) or above zero (`len != 0`,
//!      `len >= 1`), and the `&` is in the arm that the outcome takes, or
//!      after the test where the other arm never falls through: a
//!      ring buffer's `if xs.len() > 0`, an early `if xs.len() == 0:
//!      return`, a loop's `i < xs.len()`, or an earlier index guard.
//! 4. **Header reads.** A copy read of an unchanged header field that an
//!    earlier read of the same field always runs before takes that read's
//!    value, kept in a local: a loop's `len` and buffer address load once,
//!    where the first access before it loaded them, as Rust's `noalias`
//!    on `&mut` lets LLVM do. A read in a loop that no earlier read runs
//!    before loads once at the start of the outermost loop whose writes
//!    don't reach its field ([`Em::hoist_header_reads`]).
//!
//! "Runs first" is [`Em::dominates`], or a statement of an inlined call's
//! block that no earlier statement of the block can leave (`clear`), whose
//! block in turn runs first.
//!
//! Cost per body: one pass over the instructions for the stores, breaks
//! and the header check, and one walk up from each break for `clear`;
//! one pass over the `If`s for the tests that show a field above zero;
//! each value's source is memoized, so resolving is
//! linear in instructions times a depth bound of [`DEPTH`]; each guard
//! looks up the facts of one local, and a falling fact recurses at most
//! [`DEPTH`] times; a read or guard is compared with at most [`READS`]
//! earlier ones of its field, and a masked key with at most [`READS`]
//! tests of its length. Dominance walks the block path, which is
//! the nesting depth. The loop summaries take one pass that adds each
//! write to its innermost loop, then a merge of each loop into the one
//! around it, deepest first, at most [`READS`] rooms and cells each; a
//! read's hoist loop walks out at most the nesting depth. No fixpoint.

use hd_base::{HashMap, SliceSort as _, StageResult};
use hd_mono::{Target, TargetKind};
use hd_tir::ir::{NONE, PrimOp, Proj, ReadMode, Ref, Root, Tag, local_flags};
use hd_types::Ty;

use super::Em;
use crate::module::Val;

/// The longest chain of local stores, coercions and breaks a value is
/// followed through, and the deepest chain of falling facts.
const DEPTH: u32 = 16;

/// The most earlier reads or guards of one field a read or guard is
/// compared with.
const READS: usize = 8;

/// Each comparison with its negation: one comes out false as the other
/// does true.
const NEGATION: [(PrimOp, PrimOp); 6] = [
    (PrimOp::Lt, PrimOp::Ge),
    (PrimOp::Le, PrimOp::Gt),
    (PrimOp::Gt, PrimOp::Le),
    (PrimOp::Ge, PrimOp::Lt),
    (PrimOp::Eq, PrimOp::Ne),
    (PrimOp::Ne, PrimOp::Eq),
];

/// What the unchanged headers of a body's reference parameters prove.
#[derive(Default)]
pub(super) struct HeaderFacts {
    /// Index guards `if key >= len: panic` whose condition is false.
    pub(super) proven_guards: hd_base::HashSet<u32>,
    /// Copy reads of a header field, each with the earlier read of the
    /// same field that dominates it and so read the same value.
    pub(super) same_reads: HashMap<u32, u32>,
    /// The reads `same_reads` names as earlier ones.
    pub(super) leaders: hd_base::HashSet<u32>,
    /// Each emitted leader's value, in a local of its own.
    pub(super) kept: HashMap<u32, u32>,
    /// The header fields that each loop reads but no write in it can
    /// reach, read once before it ([`Em::hoist_header_reads`]).
    pub(super) hoists: HashMap<u32, Vec<Hoist>>,
    /// Each read of a hoisted field, with the local its loop's start
    /// loaded the field into.
    pub(super) hoisted: HashMap<u32, u32>,
    /// The box roots ([`BOX`]): each one's holder field and its `*T` type.
    boxes: Vec<(u32, Path, Ty)>,
}

/// A header field that loop reads but does not write: the root that
/// holds it, its field path, and the copy reads of it in the loop.
pub(super) struct Hoist {
    root: u32,
    path: Path,
    reads: Vec<u32>,
}

/// The root bit of a local's own storage, in a cell: `c | OWN` names the
/// value in local `c`'s cell, as `p` names the referent of the reference
/// that parameter `p` holds.
const OWN: u32 = 1 << 31;

/// The root bit of a box's object: `k | BOX` names the object of the `*T`
/// in entry `k` of the box table, a field of another root. The object is
/// no room and no cell, the only places a write that keeps a root may
/// reach, so while that field holds the box, whatever keeps the holder's
/// root keeps it.
const BOX: u32 = 1 << 30;

/// The most box roots a body names.
const BOXES: usize = 64;

/// A value by its source. A root is a reference parameter `p` that the
/// body never stores into, which names its referent, or `c | OWN` for a
/// local `c` in a cell ([`OWN`]), which names the cell's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    /// An instruction's own value or a constant.
    Val(u32),
    /// A reference to a root's value: the one parameter `p` holds, or
    /// `&c`.
    Ref(u32),
    /// A field, at any depth of inline structs, of a root's value, or
    /// with the empty path a reference parameter's whole referent.
    Field(u32, Path),
    /// A reference to such a field.
    FieldRef(u32, Path),
}

/// A path of up to three field indices below 1,023, one per 10 bits,
/// each stored plus one; 0 is the empty path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Path(u32);

impl Path {
    fn push(self, k: u32) -> Option<Path> {
        let used = (32 - self.0.leading_zeros()).div_ceil(10);
        (k < 1023 && used < 3).then(|| Path(self.0 | ((k + 1) << (10 * used))))
    }

    /// The field indices, outermost first.
    fn fields(self) -> Vec<u32> {
        let mut out = Vec::new();
        let mut rest = self.0;
        while rest != 0 {
            out.push((rest & 1023) - 1);
            rest >>= 10;
        }
        out
    }
}

/// Where a value points: the parameter whose referent's inline storage
/// it was read from or refers to, or the room of a buffer stored there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    Other,
    /// A reference into the inline storage of `p`'s referent.
    InlineRef(u32),
    /// A value read from the inline storage of `p`'s referent.
    Inline(u32),
    /// A `Ptr` into the room of a buffer inline in `p`'s referent.
    PtrInto(u32),
}

/// A loop test `cur < end` that goes on when it holds.
struct Rising {
    test: u32,
    cond: u32,
    loop_inst: u32,
    end: Key,
}

/// A falling index: set once at `init` before its loop, then changed only
/// by `dec` after the loop's test `v > 0`.
struct Falling {
    test: u32,
    loop_inst: u32,
    init: u32,
    value: u32,
    dec: u32,
}

struct Proof<'e, 'x, 'a, 'm> {
    em: &'e Em<'x, 'a, 'm>,
    /// Each local's stores and the value of a whole one.
    sets: Vec<Vec<(u32, u32)>>,
    /// Locals whose value a reference may change: cells, captured ones,
    /// and those a `Ref` names.
    addressed: Vec<bool>,
    /// The value of the one break that leaves each labelled instruction.
    breaks: HashMap<u32, Option<u32>>,
    /// Reference parameters that hold one reference all through the body:
    /// no store and no `Ref` names them, and the reference is no trait
    /// value. Each is a root.
    fixed: Vec<bool>,
    /// Those whose reference is shared, `&T`: nothing writes the referent
    /// while the body runs (`ref.excl.shared`, `ref.excl.alias.conflict`,
    /// `module.rt.ptr.as-ref`), whatever the body does.
    shared: Vec<bool>,
    /// Locals in a cell that no closure shares, and that hold no
    /// reference: each one's cell value is a root (`c | OWN`).
    own: Vec<bool>,
    /// Roots whose value no write of the body reaches.
    unchanged: Vec<bool>,
    /// What the body's writes may reach.
    body: Writes,
    /// What each loop's writes, its inner loops' included, may reach.
    loops: HashMap<u32, Writes>,
    keys: HashMap<u32, Key>,
    /// Statements that run whenever their block runs to them: no earlier
    /// statement of the block can leave it and go on elsewhere.
    clear: Vec<bool>,
    rising: HashMap<usize, Vec<Rising>>,
    falling: HashMap<usize, Falling>,
    /// Tests that show a header field above zero, each with the outcome
    /// that shows it, at most [`READS`] per field.
    positive: HashMap<Key, Vec<(u32, bool)>>,
    /// The box roots, at most [`BOXES`] ([`HeaderFacts::boxes`]).
    boxes: Vec<(u32, Path, Ty)>,
}

/// What one instruction may write.
#[derive(Clone, Copy)]
enum Write {
    None,
    /// Any root's value but a shared referent's.
    Any,
    /// The room of a buffer inline in root `p`'s value.
    Room(u32),
    /// Cell local `c`'s value.
    Cell(u32),
}

/// What the writes of a loop, or of the body, may reach.
#[derive(Clone, Default)]
struct Writes {
    any: bool,
    /// The roots whose buffer rooms a `Ptr` writes, at most [`READS`].
    rooms: Vec<u32>,
    /// The cell locals written, at most [`READS`].
    cells: Vec<u32>,
}

impl Writes {
    fn add(&mut self, w: Write) {
        match w {
            Write::None => {}
            Write::Any => self.any = true,
            Write::Room(p) => self.any |= !add_capped(&mut self.rooms, p),
            Write::Cell(c) => self.any |= !add_capped(&mut self.cells, c),
        }
    }

    fn merge(&mut self, other: &Writes) {
        self.any |= other.any;
        for &p in &other.rooms {
            self.add(Write::Room(p));
        }
        for &c in &other.cells {
            self.add(Write::Cell(c));
        }
    }

    /// Whether no write reaches root `root`'s value. A buffer room is a
    /// block of its own (`module.rt.buffer.type`), which holds no cell
    /// and no header of the root whose buffer it is; it may hold another
    /// referent, so a room write keeps only its own root's referent.
    fn keeps(&self, root: u32) -> bool {
        !self.any
            && if root & OWN != 0 {
                !self.cells.contains(&(root & !OWN))
            } else {
                self.rooms.iter().all(|&p| p == root)
            }
    }
}

/// Adds `x` to `v` unless it is there; false when `v` is full.
fn add_capped(v: &mut Vec<u32>, x: u32) -> bool {
    if v.contains(&x) {
        return true;
    }
    if v.len() >= READS {
        return false;
    }
    v.push(x);
    true
}

impl Em<'_, '_, '_> {
    /// The guards and header reads the body's unchanged headers prove
    /// ([`HeaderFacts`]). Only a body with a loop is asked: elsewhere each
    /// access runs once.
    pub(super) fn header_facts(&self) -> HeaderFacts {
        let b = self.b;
        let mut facts = HeaderFacts::default();
        // Release only (owner, 2026-11-11): dev and checked builds keep every
        // guard and header read as written.
        if !self.input.optimized || !b.tags.contains(&Tag::Loop) {
            return facts;
        }
        let n = b.local_ty.len();
        let mut p = Proof {
            em: self,
            sets: self.local_sets(),
            addressed: vec![false; n],
            breaks: HashMap::default(),
            fixed: vec![false; n],
            shared: vec![false; n],
            own: vec![false; n],
            unchanged: vec![false; n],
            body: Writes::default(),
            loops: HashMap::default(),
            keys: HashMap::default(),
            clear: Vec::new(),
            rising: HashMap::default(),
            falling: HashMap::default(),
            positive: HashMap::default(),
            boxes: Vec::new(),
        };
        if !p.collect() {
            return facts;
        }
        let mut guards: HashMap<(Key, Key), Vec<u32>> = HashMap::default();
        let mut reads: HashMap<Key, Vec<u32>> = HashMap::default();
        let mut hoists: HashMap<(u32, Key), usize> = HashMap::default();
        for (i, x) in (0..b.len()).zip(0u32..) {
            if b.tags[i] == Tag::Read
                && b.data[i][1] == ReadMode::Copy as u32
                && let key @ Key::Field(root, path) = p.key(x, 0)
            {
                let unchanged = p.unchanged(root);
                let before = reads.entry(key).or_default();
                if let Some(&j) = before.iter().find(|&&j| unchanged && p.runs_before(j, x)) {
                    // An earlier read of the same unchanged field that
                    // runs first.
                    facts.same_reads.insert(x, j);
                    facts.leaders.insert(j);
                } else if b.local_read(i).is_none()
                    && let Some(l) = p.hoist_loop(root, x)
                {
                    // A read in a loop that no write in it reaches: read
                    // once before the outermost such loop. A read of a
                    // local stands for the field as it was when stored,
                    // maybe before the loop and a write that came between.
                    let group = facts.hoists.entry(l).or_default();
                    let k = *hoists.entry((l, key)).or_insert_with(|| {
                        group.push(Hoist {
                            root,
                            path,
                            reads: Vec::new(),
                        });
                        group.len() - 1
                    });
                    group[k].reads.push(x);
                } else if unchanged && before.len() < READS {
                    before.push(x);
                }
            }
            let Some((index, len)) = p.guard(x) else {
                continue;
            };
            let (ik, lk) = (p.key(index, 0), p.key(len, 0));
            let Key::Field(root, _) = lk else {
                continue;
            };
            let before = guards.entry((ik, lk)).or_default();
            if before
                .iter()
                .any(|&g| p.runs_before(g, x) && p.same_value(root, g, x))
                || p.below(index, lk, x, 0)
            {
                facts.proven_guards.insert(x);
            }
            if p.panics(x) && before.len() < READS {
                before.push(x);
            }
        }
        facts.boxes = std::mem::take(&mut p.boxes);
        facts
    }

    /// Loop `l` is about to start: each header field it reads and no
    /// write in it reaches ([`HeaderFacts::hoists`]) loads once, into a
    /// local that the reads in the loop take. A field this can't load
    /// here, a cell not yet made, is read where it was.
    pub(super) fn hoist_header_reads(&mut self, l: u32) -> StageResult<()> {
        // A suspending body's resume enters a loop in the middle, past
        // its start: its reads load where they are.
        let susp = self.susp.is_some();
        let Some(groups) = self.headers.hoists.remove(&l).filter(|_| !susp) else {
            return Ok(());
        };
        for h in groups {
            if let Some(kept) = self.hoisted_load(&h)? {
                for r in h.reads {
                    self.headers.hoisted.insert(r, kept);
                }
            }
        }
        Ok(())
    }

    /// Loads hoisted field `h` into a new local: one scalar leaf at its
    /// offset from the root's address. `None` for a field of more than
    /// one value, or of a cell that may not be made yet.
    fn hoisted_load(&mut self, h: &Hoist) -> StageResult<Option<u32>> {
        // A group starts with its first read.
        let ty = self.ty(h.reads[0]);
        let vals = self.vals(ty)?;
        let &[val] = &vals[..] else {
            return Ok(None);
        };
        if self.tracked(ty) {
            return Ok(None);
        }
        self.root_load(h.root, h.path, val)
    }

    /// Loads the one scalar leaf of type `val` at `path` in root `root`'s
    /// value into a new local; a box root first loads its holder's `*T`.
    /// `None` for a field of more than one value, or of a cell that may
    /// not be made yet. A box's holder comes before it in the box table,
    /// so the holders end within [`BOXES`] steps.
    fn root_load(&mut self, root: u32, path: Path, val: Val) -> StageResult<Option<u32>> {
        let path = path.fields();
        let (pool, env) = (self.pool(), self.env());
        let (leaves, address) = if root & OWN != 0 {
            let c = (root & !OWN) as usize;
            let (m, _) = self.cell_mem(c)?;
            let (_, part) = self.fold_path(self.sub(self.b.local_ty[c]), &path)?;
            let leaves = m.leaves[part.first..part.first + part.count].to_vec();
            let made = self.cell_made(c);
            (leaves, if made { Some(self.cell_of(c)?) } else { None })
        } else if root & BOX != 0 {
            let (holder, holder_path, box_ty) = self.headers.boxes[(root & !BOX) as usize];
            let target = pool.ref_inner(crate::layout::strip(pool, env, box_ty));
            let m = crate::layout::mem(pool, env, target)?;
            let (_, part) = self.fold_path(target, &path)?;
            let leaves = m.leaves[part.first..part.first + part.count].to_vec();
            // A thin `*T` is one address word.
            (leaves, self.root_load(holder, holder_path, Val::I32)?)
        } else {
            let ref_ty = self.sub(self.b.local_ty[root as usize]);
            let target = pool.ref_inner(crate::layout::strip(pool, env, ref_ty));
            let m = crate::layout::mem(pool, env, target)?;
            let (_, part) = self.fold_path(target, &path)?;
            let leaves = m.leaves[part.first..part.first + part.count].to_vec();
            // ruled out: a fixed root holds a reference that is no trait
            // value (`Proof::collect`), one address word.
            let address = self.local_value(root)?.first().copied();
            (leaves, address)
        };
        let (&[leaf], Some(address)) = (&leaves[..], address) else {
            return Ok(None);
        };
        if leaf.val != val {
            return Ok(None);
        }
        let kept = self.f.local(val);
        self.f.get(address);
        self.f.op(leaf.load(0));
        self.f.set(kept);
        Ok(Some(kept))
    }

    /// The operand that `Prim` condition `c` shows above zero when it
    /// comes out `outcome`, on `usize`: `a < n` puts `n` above `a`, so
    /// above zero, and `n != 0` and `n >= 1` do directly; a failed
    /// comparison is its negation, as is `!c`.
    pub(super) fn positive_operand(&self, c: u32, outcome: bool) -> Option<u32> {
        let b = self.b;
        if Ref(c).as_inst().is_none() || b.tags[c as usize] != Tag::Prim {
            return None;
        }
        let op = PrimOp::from_u32(b.data[c as usize][0])?;
        let [x, y] = *b.record(b.data[c as usize][1]) else {
            // `!c` comes out the other way: of the one-operand operations
            // only `!` gives a `bool`.
            let &[inner] = b.record(b.data[c as usize][1]).first_chunk::<1>()?;
            return (op == PrimOp::Not).then(|| self.positive_operand(inner, !outcome))?;
        };
        if self.ty_of(x) != Ty::prim(hd_types::Prim::Usize) {
            return None;
        }
        // On `usize` operands every `bool` operation is a comparison.
        let op = if outcome {
            op
        } else {
            NEGATION.iter().find(|(o, _)| *o == op)?.1
        };
        let at_least_one = |r: u32| self.const_u32(r).is_some_and(|k| k >= 1);
        match op {
            PrimOp::Lt => Some(y),
            PrimOp::Gt => Some(x),
            PrimOp::Le if at_least_one(x) => Some(y),
            PrimOp::Ge if at_least_one(y) => Some(x),
            PrimOp::Ne if self.const_u32(y) == Some(0) => Some(x),
            PrimOp::Ne if self.const_u32(x) == Some(0) => Some(y),
            PrimOp::Eq if at_least_one(y) => Some(x),
            PrimOp::Eq if at_least_one(x) => Some(y),
            _ => None,
        }
    }

    /// How arm `arm` of `If` `x` ends when it never ends normally: its
    /// last statement returns, panics, traps, or breaks or continues to a
    /// label past `x`, and no break inside it ends at the arm or at `x`.
    /// `Some(true)` when nothing in it leaves for a label outside it, so
    /// the run ends there; `Some(false)` when it may go on past the block
    /// that holds `x`. `None` when it may fall through.
    pub(super) fn arm_exit(&self, arm: u32, x: u32) -> Option<bool> {
        let b = self.b;
        let last = *(b.tags[arm as usize] == Tag::Block)
            .then(|| b.record(b.data[arm as usize][0]))?
            .iter()
            .rev()
            .find(|&&s| b.tags[s as usize] != Tag::Block)?;
        let first = self.topo.first[arm as usize];
        let target = |i: u32| b.label_inst[b.data[i as usize][0] as usize];
        let ends = match b.tags[last as usize] {
            Tag::Return | Tag::Unreachable => true,
            Tag::Break | Tag::Continue if target(last) != arm && target(last) != x => false,
            Tag::Call if self.calls_panic(last) => true,
            _ => return None,
        };
        let (mut inside, mut past) = (true, true);
        for i in first..arm {
            if matches!(b.tags[i as usize], Tag::Break | Tag::Continue) {
                let t = target(i);
                past &= t != arm && t != x;
                inside &= (first..arm).contains(&t);
            }
        }
        past.then_some(ends && inside)
    }

    /// The `x` of instruction `v` when it is `x - 1`, or
    /// `x.wrapping_sub(1)`, which is the same subtraction in a release
    /// build.
    pub(super) fn minus_one_of(&self, v: u32) -> Option<u32> {
        let b = self.b;
        let i = Ref(v).as_inst()?.0 as usize;
        let [r, one] = match b.tags[i] {
            Tag::Prim if b.data[i][0] == PrimOp::Sub as u32 => {
                // ruled out: the checker emits PrimOp::Sub with two operands;
                // cloning and substitution preserve the primitive operand record.
                super::required(
                    b.record(b.data[i][1]).try_into().ok(),
                    "subtraction has two operands",
                )
            }
            Tag::Call
                if matches!(
                    self.calls.get(&v),
                    Some(Target::Call(call)) if matches!(
                        &call.kind,
                        TargetKind::Intrinsic(key) if key == "int_wrapping_sub"
                    )
                ) =>
            {
                // A call's record ends with three words past its arguments.
                *b.record(b.data[i][1]).first_chunk::<2>()?
            }
            _ => return None,
        };
        (self.const_u32(one) == Some(1)).then_some(r)
    }

    /// Whether instruction `s` calls the panic intrinsic.
    fn calls_panic(&self, s: u32) -> bool {
        self.b.tags[s as usize] == Tag::Call
            && matches!(
                self.calls.get(&s),
                Some(Target::Call(call))
                    if matches!(&call.kind, TargetKind::Intrinsic(key) if key == "panic")
            )
    }
}

impl Proof<'_, '_, '_, '_> {
    /// The body's breaks, addressed locals and roots, what its writes and
    /// each loop's may reach, and its loop facts. False when no root is
    /// left.
    fn collect(&mut self) -> bool {
        let em = self.em;
        let b = em.b;
        let shared_flags = local_flags::CAPTURED | local_flags::CAPTURED_ASSIGNED;
        for l in 0..b.local_ty.len() {
            self.addressed[l] = em.is_cell(l) || b.local_flags[l] & shared_flags != 0;
        }
        let mut captured = hd_base::HashSet::default();
        for local in &b.cap_local {
            self.addressed[local.idx()] = true;
            captured.insert(local.idx());
        }
        for i in 0..b.len() {
            if let Some(l) = super::shape::ref_local(b, i) {
                self.addressed[l] = true;
            }
            if b.tags[i] == Tag::Break {
                let target = b.label_inst[b.data[i][0] as usize];
                let value = (b.data[i][1] != NONE).then_some(b.data[i][1]);
                self.breaks
                    .entry(target)
                    .and_modify(|v| *v = None)
                    .or_insert(value);
            }
        }
        for l in 0..b.local_ty.len() {
            let ty = em.sub(b.local_ty[l]);
            let target = em.ref_target(ty);
            self.fixed[l] = b.local_flags[l] & local_flags::PARAM != 0
                && !self.addressed[l]
                && self.sets[l].is_empty()
                && matches!(target, Some((_, false)));
            self.shared[l] = self.fixed[l] && !crate::layout::ref_mutable(em.pool(), em.env(), ty);
            // A cell's flags may say captured where only a `Ref` takes
            // it; a closure's capture list names those it shares.
            self.own[l] = em.is_cell(l) && !captured.contains(&l) && target.is_none();
        }
        if !self.fixed.contains(&true) && !self.own.contains(&true) {
            return false;
        }
        // Each write, in the body's summary and its innermost loop's.
        let mut loops = Vec::new();
        for (i, x) in (0..b.len()).zip(0u32..) {
            if b.tags[i] == Tag::Loop {
                loops.push(x);
            }
            let w = self.write_of(i, x);
            self.body.add(w);
            let l = em.topo.loop_of[i];
            if l != NONE && !matches!(w, Write::None) {
                self.loops.entry(l).or_default().add(w);
            }
        }
        // An inner loop's writes are its outer loop's too: deepest first.
        let depth = |mut l: u32| {
            let mut d = 0u32;
            while l != NONE {
                l = em.topo.loop_of[l as usize];
                d += 1;
            }
            d
        };
        let mut loops: Vec<(std::cmp::Reverse<u32>, u32)> = loops
            .into_iter()
            .map(|l| (std::cmp::Reverse(depth(l)), l))
            .collect();
        loops.sort_stable_by_key(|&(d, _)| d);
        for (_, l) in loops {
            let outer = em.topo.loop_of[l as usize];
            if outer != NONE
                && let Some(w) = self.loops.get(&l).cloned()
            {
                self.loops.entry(outer).or_default().merge(&w);
            }
        }
        for (l, root) in (0..b.local_ty.len()).zip(0u32..) {
            self.unchanged[l] = self.fixed[l] && (self.shared[l] || self.body.keeps(root));
        }
        self.clear = self.clear_statements();
        for (i, x) in (0..b.len()).zip(0u32..) {
            if b.tags[i] == Tag::Loop {
                self.loop_facts(x);
            } else if b.tags[i] == Tag::If {
                self.positive_test(x);
            }
        }
        true
    }

    /// The root that holds root `root`: a box root's holder's, through
    /// boxes in boxes; any other root is its own. A box's object changes
    /// only while its holder's root does ([`BOX`]).
    fn base(&self, mut root: u32) -> u32 {
        while root & (OWN | BOX) == BOX {
            root = self.boxes[(root & !BOX) as usize].0;
        }
        root
    }

    /// Whether no write of the body reaches root `root`'s value.
    fn unchanged(&self, root: u32) -> bool {
        let root = self.base(root);
        root & OWN == 0 && self.unchanged[root as usize]
    }

    /// Whether no write of loop `l` reaches root `root`'s value.
    fn kept_in(&self, root: u32, l: u32) -> bool {
        let root = self.base(root);
        (root & OWN == 0 && self.shared[root as usize])
            || self.loops.get(&l).is_none_or(|w| w.keeps(root))
    }

    /// The root that dereferencing value `k` reaches, of type `ty`, and
    /// the path in it: a reference to a root or its field, or a field
    /// that holds a thin `*T` ([`BOX`]), named in the box table, when
    /// the value is `current`: still the box the field holds.
    fn deref_root(&mut self, k: Key, ty: Ty, current: bool) -> Option<(u32, Path)> {
        match k {
            Key::Ref(p) => Some((p, Path::default())),
            Key::FieldRef(p, path) => Some((p, path)),
            Key::Field(holder, path) if current && self.is_box(ty) => {
                let entry = (holder, path, ty);
                let k = match self.boxes.iter().position(|b| *b == entry) {
                    Some(k) => k,
                    None if self.boxes.len() < BOXES => {
                        self.boxes.push(entry);
                        self.boxes.len() - 1
                    }
                    None => return None,
                };
                Some((u32::try_from(k).ok()? | BOX, Path::default()))
            }
            _ => None,
        }
    }

    /// Whether value `x` is a `borrow` read, through the one store of
    /// each local it reads and identity coercions, down to the read of a
    /// place: no count, so that place keeps it while it lives (`ReadMode`)
    /// and a dereference of it reads what the place holds then.
    fn borrowed(&self, x: u32, depth: u32) -> bool {
        let b = self.em.b;
        let Some(i) = Ref(x).as_inst().map(|i| i.0 as usize) else {
            return false;
        };
        if depth > DEPTH {
            return false;
        }
        match b.tags[i] {
            Tag::Read if b.data[i][1] == ReadMode::Borrow as u32 => match b.local_read(i) {
                Some(l) => self
                    .single_store(l.idx(), x)
                    .is_some_and(|v| self.borrowed(v, depth + 1)),
                None => true,
            },
            Tag::Coerce if b.same_value_coercion(i) => self.borrowed(b.data[i][0], depth + 1),
            _ => false,
        }
    }

    /// Whether `ty` is a thin `*T`: a counted reference that lends no
    /// writes, to no trait value. A `*mut T` lends them to every holder.
    fn is_box(&self, ty: Ty) -> bool {
        let em = self.em;
        let (pool, env) = (em.pool(), em.env());
        matches!(
            pool.get(crate::layout::strip(pool, env, ty)),
            hd_types::TyData::Ref {
                counted: true,
                mutable: false,
                ..
            }
        ) && matches!(em.ref_target(ty), Some((_, false)))
    }

    /// The outermost loop around `x` whose writes do not reach root
    /// `root`'s value. A loop's writes hold its inner loops', so the walk
    /// out stops at the first loop that may write it. At most the
    /// nesting depth.
    fn hoist_loop(&self, root: u32, x: u32) -> Option<u32> {
        let mut l = self.em.topo.loop_of[x as usize];
        let mut found = None;
        while l != NONE && self.kept_in(root, l) {
            found = Some(l);
            l = self.em.topo.loop_of[l as usize];
        }
        found
    }

    /// Whether root `root`'s fields hold one value at `a` and at `b`: no
    /// write of the body reaches its root, or both lie in one loop whose
    /// writes do not.
    fn same_value(&self, root: u32, a: u32, b: u32) -> bool {
        self.unchanged(root)
            || self
                .hoist_loop(root, b)
                .is_some_and(|l| a == l || self.em.in_loop(a, l))
    }

    /// What instruction `i` may write ([`Write`]): a store into a cell
    /// root, or a move out of one, which leaves zeros there; a `Ptr`
    /// store into a root's buffer room; anything [`Proof::may_write_header`]
    /// finds.
    fn write_of(&mut self, i: usize, x: u32) -> Write {
        let b = self.em.b;
        match b.tags[i] {
            Tag::Store => {
                let v = b.view(b.data[i][0]);
                let fields = v
                    .steps(b)
                    .all(|s| matches!(s, Proj::Field(_) | Proj::Variant(_)));
                match v.root {
                    Root::Ptr(r) => match self.origin(r, 0) {
                        Origin::PtrInto(p) => Write::Room(p),
                        _ => Write::Any,
                    },
                    Root::Local(l) if fields && self.own[l as usize] => Write::Cell(l),
                    Root::Local(_) | Root::Value(_) if fields => Write::None,
                    _ => Write::Any,
                }
            }
            Tag::Read if b.data[i][1] == ReadMode::Move as u32 => match b.view(b.data[i][0]).root {
                Root::Local(l) if self.own[l as usize] => Write::Cell(l),
                _ => Write::None,
            },
            _ if self.may_write_header(i, x) => Write::Any,
            _ => Write::None,
        }
    }

    /// Whether instruction `i`, no store, may write a root's value: a
    /// call other than [`Proof::pure_call`], a drop, a uniqueness check
    /// or a count of a value that may own drop code.
    fn may_write_header(&mut self, i: usize, x: u32) -> bool {
        let b = self.em.b;
        match b.tags[i] {
            Tag::Call => !self.pure_call(x),
            // A count that reaches zero runs drop code, which may write
            // anywhere. A value that counts nothing runs none, nor does a
            // local reference to a root, which owns nothing, nor the old
            // value of a local that only ever holds borrowed reads.
            Tag::Release | Tag::Retain => {
                let t = b.data[i][0] as usize;
                if b.tags.get(t) == Some(&Tag::Store) && self.holds_no_count(t) {
                    return false;
                }
                let v = if b.tags.get(t) == Some(&Tag::Store) {
                    b.data[t][1]
                } else {
                    b.data[i][0]
                };
                if Ref(v).as_inst().is_none() {
                    return true;
                }
                let ty = self.em.ty_of(v);
                self.em.tracked(ty)
                    && !(self.em.local_ref(ty) && matches!(self.key(v, 0), Key::Ref(_)))
            }
            Tag::CallValue
            | Tag::CallDyn
            | Tag::CallHost
            | Tag::DefaultCall
            | Tag::Intrinsic
            | Tag::Await
            | Tag::AwaitValue
            | Tag::AwaitAll
            | Tag::AwaitRace
            | Tag::Closure
            | Tag::With
            | Tag::Hook
            | Tag::Drop
            | Tag::IsUnique
            | Tag::MakeUnique
            | Tag::FlagSet
            | Tag::FlagClear => true,
            _ => false,
        }
    }
    /// Whether `Store` `t` sets all of a local whose every store is a
    /// `borrow` read: the local never holds a count of its own, so what
    /// the store replaces releases nothing.
    fn holds_no_count(&self, t: usize) -> bool {
        let b = self.em.b;
        let p = b.view(b.data[t][0]);
        let Some(l) = p.local().filter(|_| p.whole()) else {
            return false;
        };
        !self.addressed[l as usize]
            && self.sets[l as usize].iter().all(|&(_, v)| {
                Ref(v).as_inst().is_some()
                    && b.tags[v as usize] == Tag::Read
                    && b.data[v as usize][1] == ReadMode::Borrow as u32
            })
    }

    /// [`Proof::clear`]: one walk up from each break and continue marks
    /// the statements it leaves, then one pass over each block's list.
    fn clear_statements(&self) -> Vec<bool> {
        clear_statements(self.em.b, &self.em.topo)
    }

    /// Whether instruction `d` runs before `j` on every path that reaches
    /// `j`: `d`, or an inlined call's block that runs `d` whenever it
    /// runs, dominates `j` ([`Em::dominates`]).
    fn runs_before(&self, d: u32, j: u32) -> bool {
        runs_before(self.em.b, &self.em.topo, &self.clear, d, j, |x, j| {
            self.em.dominates(x, j)
        })
    }

    /// Whether call `x` is a pointer intrinsic or wrapping arithmetic,
    /// which write nothing, or the panic, which never returns.
    fn pure_call(&self, x: u32) -> bool {
        matches!(
            self.em.calls.get(&x),
            Some(Target::Call(call)) if matches!(
                &call.kind,
                TargetKind::Intrinsic(key) if matches!(
                    key.as_str(),
                    "ptr_offset"
                        | "ptr_ref"
                        | "ptr_mut"
                        | "ptr_size_of"
                        | "int_wrapping_add"
                        | "int_wrapping_sub"
                        | "int_wrapping_mul"
                        | "panic"
                )
            )
        )
    }

    /// The first argument of call `x`.
    fn first_arg(&self, x: u32) -> Option<u32> {
        let b = self.em.b;
        b.record(b.data[x as usize][1]).first().copied()
    }

    /// The value one dominating store gave local `l` where `at` reads it.
    fn single_store(&self, l: usize, at: u32) -> Option<u32> {
        if self.addressed[l] {
            return None;
        }
        match self.sets[l].as_slice() {
            [(set, value)] if *value != NONE && self.em.dominates(*set, at) => Some(*value),
            _ => None,
        }
    }

    /// The value `r` stands for: through a local's one store, an identity
    /// or weakening coercion, or a block's one break.
    fn follow(&self, r: u32) -> Option<u32> {
        let b = self.em.b;
        let i = Ref(r).as_inst()?.0 as usize;
        let [a, w] = b.data[i];
        match b.tags[i] {
            // A field of a struct built here, as a range's bounds are.
            Tag::Read if let Some((v, hd_tir::ir::Step::Field(k))) = b.read_step(i) => {
                let v = Ref(v).as_inst()?.0 as usize;
                (b.tags[v] == Tag::NewStruct)
                    .then(|| b.record(b.data[v][1]).get(k as usize).copied())
                    .flatten()
            }
            Tag::Read => self.single_store(b.local_read(i)?.idx(), r),
            Tag::Coerce if b.same_value_coercion(i) => Some(a),
            Tag::Scope if b.record(w).is_empty() && b.data[a as usize][1] == NONE => self
                .breaks
                .get(&r)
                .or_else(|| self.breaks.get(&a))
                .copied()
                .flatten(),
            Tag::Block if w == NONE => self.breaks.get(&r).copied().flatten(),
            _ => None,
        }
    }

    /// The source of value `r` ([`Key`]).
    fn key(&mut self, r: u32, depth: u32) -> Key {
        if let Some(&k) = self.keys.get(&r) {
            return k;
        }
        let k = self.key_uncached(r, depth);
        self.keys.insert(r, k);
        k
    }

    fn key_uncached(&mut self, r: u32, depth: u32) -> Key {
        let b = self.em.b;
        if depth > DEPTH || Ref(r).as_inst().is_none() {
            return Key::Val(r);
        }
        let i = r as usize;
        if let Some(l) = b.local_read(i)
            && self.fixed[l.idx()]
        {
            return Key::Ref(l.raw());
        }
        if let Some(next) = self.follow(r) {
            return self.key(next, depth + 1);
        }
        if matches!(b.tags[i], Tag::Read | Tag::Ref) {
            let v = b.view(b.data[i][0]);
            let mut steps = v.steps(b).peekable();
            let (root, mut path) = if steps.peek() == Some(&Proj::Deref) {
                steps.next();
                let (root, ty) = match v.root {
                    Root::Value(x) => (self.key(x, depth + 1), x),
                    Root::Local(l) if self.fixed[l as usize] => (Key::Ref(l), NONE),
                    Root::Local(l) => match self.single_store(l as usize, r) {
                        Some(x) => (self.key(x, depth + 1), x),
                        None => return Key::Val(r),
                    },
                    _ => return Key::Val(r),
                };
                let (ty, current) = if ty == NONE {
                    (Ty::VOID, false)
                } else {
                    (em_ty(self.em, ty), self.borrowed(ty, 0))
                };
                match self.deref_root(root, ty, current) {
                    Some(at) => at,
                    None => return Key::Val(r),
                }
            } else {
                // A cell root's own fields, read or referred to in place.
                match v.root {
                    Root::Local(c) if self.own[c as usize] => (c | OWN, Path::default()),
                    _ => return Key::Val(r),
                }
            };
            for step in steps {
                let Proj::Field(k) = step else {
                    return Key::Val(r);
                };
                let Some(longer) = path.push(k) else {
                    return Key::Val(r);
                };
                path = longer;
            }
            return match (b.tags[i], path == Path::default()) {
                (Tag::Ref, true) => Key::Ref(root),
                (Tag::Ref, false) => Key::FieldRef(root, path),
                // A whole referent, as `*value` of a `&u32`; a cell's whole
                // value is the local's own read.
                (_, true) if root & OWN == 0 => Key::Field(root, path),
                (_, true) => Key::Val(r),
                (_, false) => Key::Field(root, path),
            };
        }
        Key::Val(r)
    }

    /// Where value `r` points ([`Origin`]).
    fn origin(&mut self, r: u32, depth: u32) -> Origin {
        let b = self.em.b;
        if depth > DEPTH || Ref(r).as_inst().is_none() {
            return Origin::Other;
        }
        if let Some(next) = self.follow(r) {
            return self.origin(next, depth + 1);
        }
        let i = r as usize;
        match b.tags[i] {
            Tag::Read | Tag::Ref => {
                let v = b.view(b.data[i][0]);
                let mut steps = v.steps(b);
                let deref = steps.next();
                if !steps.all(|s| matches!(s, Proj::Field(_))) {
                    return Origin::Other;
                }
                let root = match (deref, v.root) {
                    (Some(Proj::Deref), Root::Value(x)) => {
                        let k = self.key(x, depth + 1);
                        let current = self.borrowed(x, 0);
                        match self.deref_root(k, em_ty(self.em, x), current) {
                            Some((p, path)) if path == Path::default() => Some(p),
                            _ => match self.origin(x, depth + 1) {
                                Origin::InlineRef(p) => Some(p),
                                _ => None,
                            },
                        }
                    }
                    (Some(Proj::Deref), Root::Local(l)) if self.fixed[l as usize] => Some(l),
                    (Some(Proj::Deref), Root::Local(l)) => match self.single_store(l as usize, r) {
                        Some(x) => {
                            let k = self.key(x, depth + 1);
                            let current = self.borrowed(x, 0);
                            match self.deref_root(k, em_ty(self.em, x), current) {
                                Some((p, path)) if path == Path::default() => Some(p),
                                _ => None,
                            }
                        }
                        None => None,
                    },
                    _ => None,
                };
                match (root, b.tags[i]) {
                    (Some(p), Tag::Ref) => Origin::InlineRef(p),
                    (Some(p), _) => Origin::Inline(p),
                    (None, _) => Origin::Other,
                }
            }
            Tag::NewStruct => constructed_origin(b.record(b.data[i][1]), |address| {
                self.origin(address, depth + 1)
            }),
            Tag::Call
                if matches!(
                    self.em.calls.get(&r),
                    Some(Target::Call(call))
                        if matches!(&call.kind, TargetKind::Intrinsic(key) if key == "ptr_offset")
                ) =>
            {
                match self.first_arg(r).map(|a| self.origin(a, depth + 1)) {
                    Some(Origin::PtrInto(p)) => Origin::PtrInto(p),
                    _ => Origin::Other,
                }
            }
            _ => Origin::Other,
        }
    }

    /// The local a value reads, through the values it stands for.
    fn read_of(&mut self, r: u32) -> Option<(usize, u32)> {
        let Key::Val(x) = self.key(r, 0) else {
            return None;
        };
        let l = super::shape::read_local(self.em.b, x)?;
        (!self.addressed[l]).then_some((l, x))
    }

    /// The loop facts of loop `l`: a rising test `cur < end`, or a falling
    /// index `v > 0` with its one decrement.
    fn loop_facts(&mut self, l: u32) {
        let em = self.em;
        let b = em.b;
        let Some((test, false)) = em.list_loop_guard(l) else {
            return;
        };
        let cond = b.data[test as usize][0];
        if Ref(cond).as_inst().is_none() || b.tags[cond as usize] != Tag::Prim {
            return;
        }
        let op = b.data[cond as usize][0];
        let [lhs, rhs] = *b.record(b.data[cond as usize][1]) else {
            return;
        };
        let usize_ty = Ty::prim(hd_types::Prim::Usize);
        if em.ty_of(lhs) != usize_ty {
            return;
        }
        let Some(cur) = super::shape::read_local(b, lhs).filter(|&c| !self.addressed[c]) else {
            return;
        };
        if op == PrimOp::Lt as u32 {
            let end = self.key(rhs, 0);
            self.rising.entry(cur).or_default().push(Rising {
                test,
                cond,
                loop_inst: l,
                end,
            });
        } else if (op == PrimOp::Gt as u32 || op == PrimOp::Ne as u32)
            && em.const_u32(rhs) == Some(0)
        {
            let [a, c] = self.sets[cur].as_slice() else {
                return;
            };
            let (init, dec) = if em.in_loop(a.0, l) {
                (*c, *a)
            } else {
                (*a, *c)
            };
            if em.in_loop(init.0, l)
                || !em.dominates(init.0, l)
                || init.1 == NONE
                || em.topo.loop_of[dec.0 as usize] != l
                || !self.decrement(dec.1, cur, test)
            {
                return;
            }
            self.falling.insert(
                cur,
                Falling {
                    test,
                    loop_inst: l,
                    init: init.0,
                    value: init.1,
                    dec: dec.0,
                },
            );
        }
    }

    /// Whether `value` is `v - 1`, reading `v` after test `test` went on.
    fn decrement(&mut self, value: u32, v: usize, test: u32) -> bool {
        let Some(read) = self.minus_one(value) else {
            return false;
        };
        super::shape::read_local(self.em.b, read) == Some(v) && self.em.after_test(test, read)
    }

    /// The `x` of a value `x - 1` ([`Em::minus_one_of`]).
    fn minus_one(&mut self, value: u32) -> Option<u32> {
        let Key::Val(x) = self.key(value, 0) else {
            return None;
        };
        self.em.minus_one_of(x)
    }

    /// The key and length of guard `x`: an `If` with no else arm whose
    /// condition is `key >= len` on `usize`.
    fn guard(&self, x: u32) -> Option<(u32, u32)> {
        let b = self.em.b;
        if b.tags[x as usize] != Tag::If {
            return None;
        }
        let [cond, arms] = b.data[x as usize];
        if !matches!(b.record(arms), [_] | [_, NONE]) {
            return None;
        }
        let c = Ref(cond).as_inst()?.0 as usize;
        if b.tags[c] != Tag::Prim || b.data[c][0] != PrimOp::Ge as u32 {
            return None;
        }
        // ruled out: the checker emits PrimOp::Ge with two operands;
        // cloning and substitution preserve the primitive operand record.
        let [index, len] = super::required(
            b.record(b.data[c][1]).try_into().ok(),
            "comparison has two operands",
        );
        (self.em.ty_of(index) == Ty::prim(hd_types::Prim::Usize)).then_some((index, len))
    }

    /// Whether guard `x`'s arm never falls through: it calls the panic.
    fn panics(&self, x: u32) -> bool {
        let b = self.em.b;
        let arm = b.record(b.data[x as usize][1])[0];
        b.record(b.data[arm as usize][0])
            .iter()
            .any(|&s| b.tags[s as usize] == Tag::Unreachable || self.em.calls_panic(s))
    }

    /// Records `If` `x` in [`Proof::positive`] when an outcome of its
    /// condition shows a header field above zero.
    fn positive_test(&mut self, x: u32) {
        let cond = self.em.b.data[x as usize][0];
        for outcome in [true, false] {
            if let Some(field) = self.positive_field(cond, outcome)
                && let key @ Key::Field(root, _) = self.key(field, 0)
                && self.unchanged(root)
            {
                let tests = self.positive.entry(key).or_default();
                if tests.len() < READS {
                    tests.push((x, outcome));
                }
            }
        }
    }

    /// The operand that condition `cond` shows above zero when it comes
    /// out `outcome` ([`Em::positive_operand`]), through the values it
    /// stands for.
    fn positive_field(&mut self, cond: u32, outcome: bool) -> Option<u32> {
        let Key::Val(c) = self.key(cond, 0) else {
            return None;
        };
        self.em.positive_operand(c, outcome)
    }

    /// Whether `at` runs only once a test showed header field `len` above
    /// zero: inside the arm its outcome takes, or after the test when the
    /// other arm never falls through ([`Em::arm_exit`]).
    fn positive_at(&self, len: Key, at: u32) -> bool {
        let em = self.em;
        let b = em.b;
        let path = em.block_path(at).unwrap_or_default();
        let tests = self.positive.get(&len).map_or(&[][..], Vec::as_slice);
        tests.iter().any(|&(x, outcome)| {
            let arms = b.record(b.data[x as usize][1]);
            let arm = |k: usize| arms.get(k).copied().filter(|&a| a != NONE);
            let (taken, other) = if outcome {
                (arm(0), arm(1))
            } else {
                (arm(1), arm(0))
            };
            // An arm that may go on past the test's block skips only what
            // the test dominates; one that ends the run, everything after.
            taken.is_some_and(|t| path.iter().any(|&(k, _)| k == t))
                || other.and_then(|o| em.arm_exit(o, x)).is_some_and(|ends| {
                    if ends {
                        self.runs_before(x, at)
                    } else {
                        em.dominates(x, at)
                    }
                })
        })
    }

    /// Whether value `index` is `y & (len - 1)`, in either order, computed
    /// where a test showed `len` above zero.
    fn masked(&mut self, index: u32, len: Key) -> bool {
        let b = self.em.b;
        let Key::Val(x) = self.key(index, 0) else {
            return false;
        };
        if Ref(x).as_inst().is_none()
            || b.tags[x as usize] != Tag::Prim
            || b.data[x as usize][0] != PrimOp::BitAnd as u32
        {
            return false;
        }
        // ruled out: the checker emits PrimOp::BitAnd with two operands;
        // cloning and substitution preserve the primitive operand record.
        let [y, z] = super::required(
            b.record(b.data[x as usize][1]).try_into().ok(),
            "bitwise and has two operands",
        );
        let mut mask_of_len = |m: u32| self.minus_one(m).is_some_and(|n| self.key(n, 0) == len);
        (mask_of_len(y) || mask_of_len(z)) && self.positive_at(len, x)
    }

    /// Whether value `index` is below header field `len` at guard `at`
    /// ([`Proof`]'s facts). Only a rising counter's test may read a
    /// length that the body changes, in a loop around `at` that doesn't.
    fn below(&mut self, index: u32, len: Key, at: u32, depth: u32) -> bool {
        if depth > DEPTH {
            return false;
        }
        // The guard pass asks only of a field: `len` is `Key::Field`.
        let root = if let Key::Field(r, _) = len { r } else { NONE };
        let whole = root != NONE && self.unchanged(root);
        if whole && self.masked(index, len) {
            return true;
        }
        let em = self.em;
        if let Some((v, read)) = self.read_of(index) {
            // A rising counter, read after its test and before its step.
            let rising = self.rising.get(&v).is_some_and(|facts| {
                facts.iter().any(|f| {
                    rising_stores(&self.sets[v], |x| em.in_loop(x, f.loop_inst)).is_some_and(
                        |[init, step]| {
                            f.end == len
                                && (whole || self.same_value(root, f.test, at))
                                && !em.in_loop(init.0, f.loop_inst)
                                && em.topo.loop_of[step.0 as usize] == f.loop_inst
                                && em.topo.loop_of[read as usize] == f.loop_inst
                                && em.dominates(f.cond, read)
                                && em.after_test(f.test, read)
                                && em.dominates(read, step.0)
                        },
                    )
                })
            });
            if rising {
                return true;
            }
            // A falling index, at most its value when set.
            if let Some(&Falling { init, value, .. }) = self.falling.get(&v)
                && whole
                && em.dominates(init, read)
            {
                return self.below(value, len, at, depth + 1);
            }
            return false;
        }
        // `v - 1` after the test `v > 0`, before `v`'s decrement.
        if whole
            && let Some(r) = self.minus_one(index)
            && let Some((v, read)) = self.read_of(r)
            && let Some(&Falling {
                test,
                loop_inst,
                value,
                dec,
                ..
            }) = self.falling.get(&v)
            && em.topo.loop_of[read as usize] == loop_inst
            && em.after_test(test, read)
            && em.dominates(read, dec)
        {
            return self.below(value, len, at, depth + 1);
        }
        false
    }
}

/// Clear statements depend on the represented block graph, not every
/// instruction column being reachable from a sub-body root.
fn clear_statements(b: &hd_tir::ir::Body, t: &hd_tir::ir::Topology) -> Vec<bool> {
    let n = b.len();
    let inside = |x: usize, s: usize| (t.first[s] as usize) <= x && x <= s;
    let mut leaves = vec![false; n];
    for i in 0..n {
        if !matches!(b.tags[i], Tag::Break | Tag::Continue) {
            continue;
        }
        let target = b.label_inst[b.data[i][0] as usize] as usize;
        let mut s = i;
        while !inside(target, s) {
            leaves[s] = true;
            if b.tags[s] == Tag::Block && t.owner[s] != NONE {
                leaves[t.owner[s] as usize] = true;
            }
            // A detached instruction has no enclosing block. It can
            // remain in a Body's columns without being executed.
            let blk = t.blk[s];
            if blk == NONE {
                break;
            }
            s = blk as usize;
        }
    }
    let mut clear = vec![false; n];
    for blk in 0..n {
        if b.tags[blk] != Tag::Block {
            continue;
        }
        for &s in b.record(b.data[blk][0]) {
            clear[s as usize] = true;
            if leaves[s as usize] {
                break;
            }
        }
    }
    clear
}

/// The existing bounded empty-scope walk, independent of emitter state.
fn runs_before(
    b: &hd_tir::ir::Body,
    t: &hd_tir::ir::Topology,
    clear: &[bool],
    d: u32,
    j: u32,
    mut dominates: impl FnMut(u32, u32) -> bool,
) -> bool {
    let mut x = d;
    for _ in 0..DEPTH {
        if dominates(x, j) {
            return true;
        }
        let blk = t.blk[x as usize];
        if !clear.get(x as usize).copied().unwrap_or(false) || blk == NONE {
            return false;
        }
        let owner = t.owner[blk as usize];
        if owner == NONE
            || b.tags[owner as usize] != Tag::Scope
            || b.data[owner as usize][0] != blk
            || !b.record(b.data[owner as usize][1]).is_empty()
        {
            return false;
        }
        x = owner;
    }
    false
}

/// Exactly one initialization and one loop step, ordered by execution.
/// Instruction identities need not follow the enclosing block's order.
fn rising_stores(
    stores: &[(u32, u32)],
    mut in_loop: impl FnMut(u32) -> bool,
) -> Option<[(u32, u32); 2]> {
    let [a, c] = stores else { return None };
    Some(if in_loop(a.0) { [*c, *a] } else { [*a, *c] })
}

/// The type of value `x` at the body's instance.
fn em_ty(em: &Em<'_, '_, '_>, x: u32) -> Ty {
    em.ty_of(x)
}

/// A `Ptr` constructor has one address field. A general aggregate is not
/// pointer provenance, even if one of its fields came from inline storage.
fn constructed_origin(fields: &[u32], resolve: impl FnOnce(u32) -> Origin) -> Origin {
    match fields {
        [address] => match resolve(*address) {
            Origin::Inline(p) => Origin::PtrInto(p),
            _ => Origin::Other,
        },
        _ => Origin::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_tir::ir::{Body, BodyKind, TirBuilder, TirSink, Topology};

    #[test]
    fn detached_control_transfers_do_not_clear_live_statements() -> Result<(), String> {
        for tag in [Tag::Break, Tag::Continue] {
            let mut build = TirBuilder::new(DefId::from_raw(0), BodyKind::Fn);
            let loop_mark = build.open_loop();
            let label = loop_mark.0.raw();
            let block = build.open_block();
            let block = build.close_block(block, None, Ty::VOID, NodeIdx::NONE);
            let loop_inst = build.close_loop(loop_mark, block, Ty::VOID, NodeIdx::NONE);
            // A Body can retain a detached control-transfer column while
            // the live root lists only the loop. The verifier accepts it.
            let detached = build.emit(tag, label, NONE, Ty::NEVER, NodeIdx::NONE);
            let record = build.words_record(&[loop_inst.0]);
            let root = build.emit(Tag::Block, record, NONE, Ty::VOID, NodeIdx::NONE);
            let body = build.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
            let topo = Topology::new(&body);
            assert_eq!(topo.blk[detached.0 as usize], NONE);
            let clear = clear_statements(&body, &topo);
            assert!(!clear[detached.0 as usize]);
            assert!(clear[loop_inst.0 as usize]);
        }
        Ok(())
    }

    #[test]
    fn a_general_aggregate_cannot_stand_for_a_buffer_room() {
        let from = |address| {
            if address == 7 {
                Origin::Inline(3)
            } else {
                Origin::Other
            }
        };
        assert_eq!(constructed_origin(&[7], from), Origin::PtrInto(3));
        assert_eq!(constructed_origin(&[8], from), Origin::Other);
        assert_eq!(constructed_origin(&[], from), Origin::Other);
        assert_eq!(constructed_origin(&[7, 8], from), Origin::Other);
    }

    #[test]
    fn empty_scope_dominance_stops_at_the_existing_depth_bound() -> Result<(), String> {
        for depth in [0, 1, 15, 16, 20] {
            let mut build = TirBuilder::new(DefId::from_raw(0), BodyKind::Fn);
            let root = build.open_block();
            let mut scopes = Vec::new();
            for _ in 0..depth {
                let scope = build.open_scope();
                let body = build.open_block();
                scopes.push((scope, body));
            }
            let leaf = build.open_block();
            let d = build.close_block(leaf, None, Ty::VOID, NodeIdx::NONE).0;
            while let Some((scope, body)) = scopes.pop() {
                let body = build.close_block(body, None, Ty::VOID, NodeIdx::NONE);
                build.close_scope(scope, body, Ty::VOID, NodeIdx::NONE);
            }
            let next = build.open_block();
            let j = build.close_block(next, None, Ty::VOID, NodeIdx::NONE).0;
            let root = build.close_block(root, None, Ty::VOID, NodeIdx::NONE);
            let body = build.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
            let topo = Topology::new(&body);
            // There is no break or continue: every statement is clear.
            let clear = vec![true; body.len()];
            let mut visits = 0;
            let found = runs_before(&body, &topo, &clear, d, j, |x, j| {
                visits += 1;
                topo.blk[x as usize] == topo.blk[j as usize]
                    && topo.pos[x as usize] < topo.pos[j as usize]
            });
            assert_eq!(found, depth < DEPTH);
            assert_eq!(visits, (depth + 1).min(DEPTH));
        }
        Ok(())
    }

    fn counter_body(init_first: bool, count: usize) -> Result<(Body, u32), String> {
        let mut build = TirBuilder::new(DefId::from_raw(0), BodyKind::Fn);
        let cursor = build.local(
            Ty::prim(hd_types::Prim::Usize),
            Symbol::default(),
            0,
            NodeIdx::NONE,
        );
        let zero = build.const_value(Ty::prim(hd_types::Prim::Usize), 0);
        let one = build.const_value(Ty::prim(hd_types::Prim::Usize), 1);
        let mut init = None;
        if init_first && count >= 2 {
            init = Some(u32::try_from(build.body_mut().len()).map_err(|e| e.to_string())?);
            build.set(cursor, zero, NodeIdx::NONE);
        }
        let loop_mark = build.open_loop();
        let loop_body = build.open_block();
        build.set(cursor, one, NodeIdx::NONE);
        let loop_body = build.close_block(loop_body, None, Ty::VOID, NodeIdx::NONE);
        let loop_inst = build.close_loop(loop_mark, loop_body, Ty::VOID, NodeIdx::NONE);
        if !init_first && count >= 2 {
            init = Some(u32::try_from(build.body_mut().len()).map_err(|e| e.to_string())?);
            build.set(cursor, zero, NodeIdx::NONE);
        }
        let mut statements = init.into_iter().collect::<Vec<_>>();
        statements.push(loop_inst.0);
        if count == 3 {
            statements.push(u32::try_from(build.body_mut().len()).map_err(|e| e.to_string())?);
            build.set(cursor, zero, NodeIdx::NONE);
        }
        let record = build.words_record(&statements);
        let root = build.emit(Tag::Block, record, NONE, Ty::VOID, NodeIdx::NONE);
        Ok((
            build.finish(root, &[]).map_err(|e| format!("{e:?}"))?,
            loop_inst.0,
        ))
    }

    #[test]
    fn rising_schedule_uses_control_order_and_requires_two_stores() -> Result<(), String> {
        for count in [1, 2, 3] {
            for init_first in [false, true] {
                let (body, loop_inst) = counter_body(init_first, count)?;
                let topo = Topology::new(&body);
                let stores = body
                    .tags
                    .iter()
                    .enumerate()
                    .filter(|(_, tag)| **tag == Tag::Store)
                    .map(|(i, _)| u32::try_from(i).map(|id| (id, body.data[i][1])))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?;
                let schedule = rising_stores(&stores, |x| topo.loop_of[x as usize] == loop_inst);
                assert_eq!(schedule.is_some(), count == 2);
                if let Some([init, step]) = schedule {
                    assert_ne!(topo.loop_of[init.0 as usize], loop_inst);
                    assert_eq!(topo.loop_of[step.0 as usize], loop_inst);
                    assert_eq!(init.0 < step.0, init_first);
                }
            }
        }
        Ok(())
    }
}
