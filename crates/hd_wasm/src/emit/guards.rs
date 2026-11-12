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
//!    with no call nothing else runs while the body does.
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
//!    on `&mut` lets LLVM do.
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
//! the nesting depth. No fixpoint.

use hd_base::HashMap;
use hd_mono::{Target, TargetKind};
use hd_tir::ir::{NONE, PrimOp, Proj, ReadMode, Ref, Root, Tag, local_flags};
use hd_types::Ty;

use super::Em;

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
}

/// A value by its source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    /// An instruction's own value or a constant.
    Val(u32),
    /// The reference that parameter local `p` holds.
    Param(u32),
    /// A field, at any depth of inline structs, of the referent of
    /// parameter `p`, which the body never writes.
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
    /// Reference parameters whose referent the body never writes.
    unchanged: Vec<bool>,
    keys: HashMap<u32, Key>,
    /// Statements that run whenever their block runs to them: no earlier
    /// statement of the block can leave it and go on elsewhere.
    clear: Vec<bool>,
    rising: HashMap<usize, Vec<Rising>>,
    falling: HashMap<usize, Falling>,
    /// Tests that show a header field above zero, each with the outcome
    /// that shows it, at most [`READS`] per field.
    positive: HashMap<Key, Vec<(u32, bool)>>,
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
        let mut p = Proof {
            em: self,
            sets: self.local_sets(),
            addressed: vec![false; b.local_ty.len()],
            breaks: HashMap::default(),
            unchanged: vec![false; b.local_ty.len()],
            keys: HashMap::default(),
            clear: Vec::new(),
            rising: HashMap::default(),
            falling: HashMap::default(),
            positive: HashMap::default(),
        };
        if !p.collect() {
            return facts;
        }
        let mut guards: HashMap<(Key, Key), Vec<u32>> = HashMap::default();
        let mut reads: HashMap<Key, Vec<u32>> = HashMap::default();
        for (i, x) in (0..b.len()).zip(0u32..) {
            // A copy read of an unchanged header field that an earlier
            // one of the same field dominates.
            if b.tags[i] == Tag::Read
                && b.data[i][1] == ReadMode::Copy as u32
                && let key @ Key::Field(..) = p.key(x, 0)
            {
                let before = reads.entry(key).or_default();
                if let Some(&j) = before.iter().find(|&&j| p.runs_before(j, x)) {
                    facts.same_reads.insert(x, j);
                    facts.leaders.insert(j);
                } else if before.len() < READS {
                    before.push(x);
                }
            }
            let Some((index, len)) = p.guard(x) else {
                continue;
            };
            let (ik, lk) = (p.key(index, 0), p.key(len, 0));
            if !matches!(lk, Key::Field(..)) {
                continue;
            }
            let before = guards.entry((ik, lk)).or_default();
            if before.iter().any(|&g| p.runs_before(g, x)) || p.below(index, lk, 0) {
                facts.proven_guards.insert(x);
            }
            if p.panics(x) && before.len() < READS {
                before.push(x);
            }
        }
        facts
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
    /// The body's breaks, addressed locals and unchanged parameters, and
    /// its loop facts. False when the body may write a parameter's
    /// referent some other way than these facts allow.
    fn collect(&mut self) -> bool {
        let b = self.em.b;
        for l in 0..b.local_ty.len() {
            self.addressed[l] = self.em.is_cell(l)
                || b.local_flags[l] & (local_flags::CAPTURED | local_flags::CAPTURED_ASSIGNED) != 0;
        }
        for local in &b.cap_local {
            self.addressed[local.idx()] = true;
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
            self.unchanged[l] = b.local_flags[l] & local_flags::PARAM != 0
                && !self.addressed[l]
                && self.sets[l].is_empty();
        }
        // Every write the body makes, and every call.
        let mut ptr_stores = Vec::new();
        for (i, x) in (0..b.len()).zip(0u32..) {
            if self.may_write_header(i, x, &mut ptr_stores) {
                return false;
            }
        }
        // A `Ptr` write keeps the referent of the parameter whose buffer
        // room it writes, and no other.
        for r in ptr_stores {
            match self.origin(r, 0) {
                Origin::PtrInto(p) => {
                    for l in 0..self.unchanged.len() {
                        if l != p as usize {
                            self.unchanged[l] = false;
                        }
                    }
                }
                _ => self.unchanged.fill(false),
            }
        }
        if !self.unchanged.contains(&true) {
            return false;
        }
        self.clear = self.clear_statements();
        // Keys found before a parameter turned out changed are stale.
        self.keys.clear();
        for (i, x) in (0..b.len()).zip(0u32..) {
            if b.tags[i] == Tag::Loop {
                self.loop_facts(x);
            } else if b.tags[i] == Tag::If {
                self.positive_test(x);
            }
        }
        true
    }

    /// Whether instruction `i` may write a referent other than as
    /// [`Proof`] allows: a call other than [`Proof::pure_call`], a store
    /// through a dereference or an element, a drop, a uniqueness check or
    /// a count of a value that may own drop code. A `Ptr` store goes to
    /// `ptr_stores`, whose room [`Proof::collect`] checks.
    fn may_write_header(&mut self, i: usize, x: u32, ptr_stores: &mut Vec<u32>) -> bool {
        let b = self.em.b;
        match b.tags[i] {
            Tag::Call => !self.pure_call(x),
            Tag::Store => {
                let v = b.view(b.data[i][0]);
                match v.root {
                    Root::Ptr(r) => {
                        ptr_stores.push(r);
                        false
                    }
                    Root::Local(_) | Root::Value(_) => !v
                        .steps(b)
                        .all(|s| matches!(s, Proj::Field(_) | Proj::Variant(_))),
                    Root::Const(_) | Root::Global(..) | Root::Other => true,
                }
            }
            // A count that reaches zero runs drop code, which may write
            // anywhere. A value that counts nothing runs none, nor does a
            // local reference a parameter lent, which owns nothing.
            Tag::Release | Tag::Retain => {
                let t = b.data[i][0] as usize;
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
                    && !(self.em.local_ref(ty) && matches!(self.key(v, 0), Key::Param(_)))
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
            && self.unchanged[l.idx()]
        {
            return Key::Param(l.raw());
        }
        if let Some(next) = self.follow(r) {
            return self.key(next, depth + 1);
        }
        if matches!(b.tags[i], Tag::Read | Tag::Ref) {
            let v = b.view(b.data[i][0]);
            let mut steps = v.steps(b);
            if steps.next() != Some(Proj::Deref) {
                return Key::Val(r);
            }
            let root = match v.root {
                Root::Value(x) => self.key(x, depth + 1),
                Root::Local(l) if self.unchanged[l as usize] => Key::Param(l),
                Root::Local(l) => match self.single_store(l as usize, r) {
                    Some(x) => self.key(x, depth + 1),
                    None => return Key::Val(r),
                },
                _ => return Key::Val(r),
            };
            let (p, mut path) = match root {
                Key::Param(p) => (p, Path::default()),
                Key::FieldRef(p, path) => (p, path),
                _ => return Key::Val(r),
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
            if path == Path::default() {
                return Key::Val(r);
            }
            return if b.tags[i] == Tag::Ref {
                Key::FieldRef(p, path)
            } else {
                Key::Field(p, path)
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
                if steps.next() != Some(Proj::Deref) || !steps.all(|s| matches!(s, Proj::Field(_)))
                {
                    return Origin::Other;
                }
                let root = match v.root {
                    Root::Value(x) => match self.key(x, depth + 1) {
                        Key::Param(p) => Some(p),
                        _ => match self.origin(x, depth + 1) {
                            Origin::InlineRef(p) => Some(p),
                            _ => None,
                        },
                    },
                    Root::Local(l) if self.unchanged[l as usize] => Some(l),
                    Root::Local(l) => match self.single_store(l as usize, r) {
                        Some(x) => match self.key(x, depth + 1) {
                            Key::Param(p) => Some(p),
                            _ => None,
                        },
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
                && let key @ Key::Field(..) = self.key(field, 0)
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

    /// Whether value `index` is below header field `len` wherever it is
    /// read ([`Proof`]'s facts).
    fn below(&mut self, index: u32, len: Key, depth: u32) -> bool {
        if depth > DEPTH {
            return false;
        }
        if self.masked(index, len) {
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
                && em.dominates(init, read)
            {
                return self.below(value, len, depth + 1);
            }
            return false;
        }
        // `v - 1` after the test `v > 0`, before `v`'s decrement.
        if let Some(r) = self.minus_one(index)
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
            return self.below(value, len, depth + 1);
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
