//! Specialization of pure, omitted defaults before reachability collection.
//! The transformed TIR keeps instruction indices stable for source locations
//! and call targets; code generation receives this same body.

use hd_base::{Hash128, Range32, StableHasher};
use hd_tir::ir::{Body, Coercion, NONE, Op, PrimOp, Ref, Tag, Word, local_flags, ref_word, slots};
use hd_types::Ty;

/// Values that can be copied without allocating or repeating an effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Value {
    Scalar(u64),
    Variant(u32),
}

impl Value {
    pub fn hash_into(self, h: &mut StableHasher) {
        match self {
            Self::Scalar(bits) => {
                h.u8(0);
                h.u64(bits);
            }
            Self::Variant(tag) => {
                h.u8(1);
                h.u32(tag);
            }
        }
    }
}

/// The mark of an exclusive copy's instance among its defaults (#1118): a
/// slot no parameter has. Like a default, it gives the copy a key and
/// emitted code of its own, and its closure bodies carry it.
pub const EXCLUSIVE: (u32, Value) = (u32::MAX, Value::Variant(u32::MAX));

/// The mark of a promoted copy's instance among its defaults (#1151): a
/// slot no parameter has, with the mask of the parameters it takes by
/// value. Like [`EXCLUSIVE`], it gives the copy a key and code of its own.
#[must_use]
pub const fn promoted(mask: u64) -> (u32, Value) {
    (u32::MAX - 1, Value::Scalar(mask))
}

/// The parameters an instance with `defaults` takes by value: the mask
/// of its [`promoted`] mark, 0 for any other instance.
#[must_use]
pub fn promoted_mask(defaults: &[(u32, Value)]) -> u64 {
    defaults
        .iter()
        .find_map(|&d| match d {
            (p, Value::Scalar(mask)) if p == u32::MAX - 1 => Some(mask),
            _ => None,
        })
        .unwrap_or(0)
}

/// Preserve all pre-existing keys when no default was specialized.
#[must_use]
pub fn digest(known: &[(u32, Hash128)], defaults: &[(u32, Value)]) -> Hash128 {
    if defaults.is_empty() {
        return crate::layout::known_digest(known);
    }
    let mut h = StableHasher::new("instance-defaults");
    h.hash(crate::layout::known_digest(known));
    h.u32(1); // Transformation version: also separates emitted-code cache entries.
    h.u32(u32::try_from(defaults.len()).expect("defaults"));
    for &(p, value) in defaults {
        h.u32(p);
        value.hash_into(&mut h);
    }
    h.finish()
}

#[must_use]
pub fn key(plain: Hash128, known: &[(u32, Hash128)], defaults: &[(u32, Value)]) -> Hash128 {
    if defaults.is_empty() {
        return crate::layout::known_key(plain, known);
    }
    let mut h = StableHasher::new("instance-defaults-key");
    h.hash(plain);
    h.hash(digest(known, defaults));
    h.finish()
}

/// A default must consist entirely of constants and transparent coercions,
/// besides its parameters' exit releases, moot once no call passes them.
/// Returning a constant after a call or a write is deliberately not enough.
#[must_use]
pub fn constant(body: &Body) -> Option<Value> {
    if body.tags.iter().enumerate().any(|(i, tag)| match tag {
        Tag::Block | Tag::Release => false,
        Tag::NewVariant => !body.record(body.data[i][1]).is_empty(),
        Tag::Coerce => !transparent(body, i),
        _ => true,
    }) {
        return None;
    }
    let root = *body.sub_root.first()?;
    Values::compute(body, &[], &|_| None).word(body, root)
}

/// Whether variant construction `i` carries no value: no fields, or only
/// `void` ones (`.Ok(())`), which a match cannot read.
fn no_payload(body: &Body, i: usize) -> bool {
    body.record(body.data[i][1]).iter().all(|&f| {
        f == NONE
            || if f & Ref::CONST_BIT != 0 {
                body.consts
                    .get((f & !Ref::CONST_BIT) as usize)
                    .is_some_and(|c| c.0 == Ty::VOID)
            } else {
                body.ty.get(f as usize) == Some(&Ty::VOID)
            }
    })
}

fn transparent(body: &Body, i: usize) -> bool {
    matches!(body.record(body.data[i][1]).first().copied(),
        Some(k) if k == Coercion::Identity as u32 || k == Coercion::Weaken as u32)
}

/// The values a body's instructions are known to produce: pessimistic
/// forward constant propagation, the non-speculative half of SCCP
/// (Wegman and Zadeck) that rustc's MIR `ConstProp` and LLVM's
/// `InstSimplify` also run. One pass in index order suffices because TIR
/// is topological: an instruction's operands and a block's members come
/// before it (`hd_tir::ir::topo`). Where an operand comes later, its value
/// reads as unknown, which only loses a fold. A value is a scalar or a
/// fieldless variant, copied without allocating or repeating an effect;
/// knowing it never proves the instruction that made it has no effects,
/// so a fold keeps that instruction.
///
/// It follows inlined results (#1518): an inlined callee is a `Scope`
/// over a block its `return`s and final value leave with `Break`s, so a
/// block's value is the one value that its tail and every `Break` to it
/// agree on. A local stored once (whole) and never lent mutably reads as
/// the value stored. Cost: one pass over the instructions and one over
/// the `Break`s, O(n) in the body's size.
struct Values {
    val: Vec<Option<Value>>,
    /// Whether each instruction is pure: running it has no effect and
    /// leaves no way out but its value (reads, fieldless variants,
    /// transparent coercions, `!`, and blocks of those whose `Break`s
    /// leave only themselves). Unknown values are no obstacle.
    pure: Vec<bool>,
}

impl Values {
    /// `None` when a parameter `defaults` names does not exist. `calls`
    /// gives the result of a `Call` its caller knows (a flag method's
    /// solved value, `hd_opt::whole::solve_flags`); every other call's
    /// result is unknown.
    fn new(
        body: &Body,
        defaults: &[(u32, Value)],
        calls: &dyn Fn(usize) -> Option<Value>,
    ) -> Option<Self> {
        let params = params(body);
        let defaults = defaults
            .iter()
            .map(|&(parameter, value)| Some((*params.get(parameter as usize)?, value)))
            .collect::<Option<Vec<_>>>()?;
        Some(Self::compute(body, &defaults, calls))
    }

    /// Defaults already name valid local indices; call-only analysis has none.
    fn compute(
        body: &Body,
        defaults: &[(usize, Value)],
        calls: &dyn Fn(usize) -> Option<Value>,
    ) -> Self {
        let n = body.len();
        let locals = body.local_ty.len();
        let mut writes = vec![0u32; locals];
        let mut assigned = vec![NONE; locals];
        // A store into a field or variant payload of a local writes it
        // too, with no value of the whole local; a mutable reference to it
        // may write it anywhere, so the local has no single value.
        for i in 0..n {
            let [a, b] = body.data[i];
            let mutable = body.tags[i] == Tag::Ref && b & ref_word::MUT != 0;
            if (body.tags[i] == Tag::Store || mutable)
                && let Some(l) = body.local_in_place(a)
            {
                writes[l.idx()] += if mutable { 2 } else { 1 };
                assigned[l.idx()] = if !mutable && body.view(a).whole() {
                    b
                } else {
                    NONE
                };
            }
        }
        let mut seeded = vec![None; locals];
        for &(l, value) in defaults {
            if writes[l] == 0 && body.local_flags[l] & local_flags::CAPTURED_ASSIGNED == 0 {
                seeded[l] = Some(value);
            }
        }
        // The `Break`s to each block, bucketed by their target (a
        // counting sort: `from[t]..from[t + 1]` are target `t`'s values).
        let mut from = vec![0u32; n + 1];
        for i in 0..n {
            if body.tags[i] == Tag::Break
                && let Some(&t) = body.label_inst.get(body.data[i][0] as usize)
                && (t as usize) < n
            {
                from[t as usize + 1] += 1;
            }
        }
        for t in 0..n {
            from[t + 1] += from[t];
        }
        let mut next = from.clone();
        let mut breaks = vec![NONE; from[n] as usize];
        for i in 0..n {
            if body.tags[i] == Tag::Break
                && let Some(&t) = body.label_inst.get(body.data[i][0] as usize)
                && (t as usize) < n
            {
                breaks[next[t as usize] as usize] = index(i);
                next[t as usize] += 1;
            }
        }
        let mut out = Self {
            val: vec![None; n],
            pure: vec![false; n],
        };
        let mut arms = Arms::new(body);
        for i in 0..n {
            let [a, b] = body.data[i];
            out.pure[i] = out.pure_at(body, i);
            let v = match body.tags[i] {
                Tag::Block => {
                    let breaks = &breaks[from[i] as usize..from[i + 1] as usize];
                    let tail = (b != NONE).then_some(b);
                    // A `Break` in an arm a decision rules out never runs.
                    let live = breaks
                        .iter()
                        .filter(|&&k| arms.live(k, index(i)))
                        .map(|&k| body.data[k as usize][1]);
                    out.agree(body, tail.into_iter().chain(live))
                }
                Tag::Scope => out.word(body, a),
                Tag::Call => calls(i),
                Tag::Read if body.local_read(i).is_none() => {
                    // A whole copy of an instruction's value (a match's
                    // scrutinee) is that value.
                    body.decode_place(a).and_then(|p| match p.root {
                        hd_tir::ir::PlaceRoot::Value(r) if p.proj.is_empty() => out.word(body, r.0),
                        _ => None,
                    })
                }
                Tag::Read => body.local_read(i).and_then(|l| {
                    let l = l.idx();
                    seeded[l].or_else(|| {
                        // A body with no closure captures nothing, whatever
                        // its checker flags say (a template's derived body).
                        let flags = if body.sub_root.len() > 1 {
                            local_flags::PARAM | local_flags::CAPTURED_ASSIGNED
                        } else {
                            local_flags::PARAM
                        };
                        (writes[l] == 1 && body.local_flags[l] & flags == 0)
                            .then(|| out.word(body, assigned[l]))
                            .flatten()
                    })
                }),
                Tag::NewVariant if no_payload(body, i) => Some(Value::Variant(a)),
                Tag::Coerce if transparent(body, i) => out.word(body, a),
                Tag::Prim => match (PrimOp::from_u32(a), body.record(b).first()) {
                    (Some(PrimOp::Not), Some(&x)) if body.ty[i] == Ty::BOOL => {
                        match out.word(body, x) {
                            Some(Value::Scalar(bits)) => Some(Value::Scalar(u64::from(bits == 0))),
                            _ => None,
                        }
                    }
                    _ => None,
                },
                Tag::And | Tag::Or if body.ty[i] == Ty::BOOL => {
                    match (body.tags[i], out.word(body, a), out.word(body, b)) {
                        (Tag::And, Some(Value::Scalar(0)), _) => Some(Value::Scalar(0)),
                        (Tag::Or, Some(Value::Scalar(bits)), _) if bits != 0 => {
                            Some(Value::Scalar(1))
                        }
                        // A non-deciding left operand: the right decides.
                        (_, Some(Value::Scalar(_)), right) => right,
                        // An unknown left operand: a right one that decides
                        // the operator alone decides it either way, as
                        // Kleene's strong three-valued `and` and `or` do.
                        (Tag::And, None, Some(Value::Scalar(0))) => Some(Value::Scalar(0)),
                        (Tag::Or, None, Some(Value::Scalar(bits))) if bits != 0 => {
                            Some(Value::Scalar(1))
                        }
                        _ => None,
                    }
                }
                Tag::If | Tag::Match => {
                    let chosen = out.chosen(body, i);
                    arms.decide(i, chosen);
                    if let Some(k) = chosen {
                        out.word(body, k)
                    } else {
                        // An undecided decision whose every arm makes one
                        // value makes it whichever runs.
                        let arms = body.record(b);
                        let arms = if body.tags[i] == Tag::Match {
                            arms.get(1..).unwrap_or_default()
                        } else {
                            arms
                        };
                        if arms.len() < 2 || arms.contains(&NONE) {
                            None
                        } else {
                            out.agree(body, arms.iter().copied())
                        }
                    }
                }
                Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar => {
                    arms.decide(i, out.switched(body, i));
                    None
                }
                _ => None,
            };
            out.val[i] = v;
        }
        out
    }

    /// Whether instruction `i` is pure, from its operands' purity.
    fn pure_at(&self, body: &Body, i: usize) -> bool {
        let [a, b] = body.data[i];
        let word = |w: u32| w == NONE || w & Ref::CONST_BIT != 0 || self.pure[w as usize];
        match body.tags[i] {
            Tag::Block => {
                // Child blocks are listed too; their runners decide them.
                word(b)
                    && body.record(a).iter().all(|&m| {
                        let [ma, mb] = body.data[m as usize];
                        match body.tags[m as usize] {
                            Tag::Block => true,
                            Tag::Break => {
                                body.label_inst.get(ma as usize).map(|&t| t as usize) == Some(i)
                                    && word(mb)
                            }
                            _ => self.pure[m as usize],
                        }
                    })
            }
            Tag::Scope => body.record(b).is_empty() && word(a),
            Tag::Read => body.local_read(i).is_some(),
            Tag::NewVariant => no_payload(body, i),
            Tag::Coerce => transparent(body, i) && word(a),
            Tag::Prim => {
                PrimOp::from_u32(a) == Some(PrimOp::Not) && body.record(b).iter().all(|&x| word(x))
            }
            Tag::And | Tag::Or => word(a) && word(b),
            _ => false,
        }
    }

    /// The value of operand word `w`: a constant, or an instruction's.
    fn word(&self, body: &Body, w: u32) -> Option<Value> {
        if w == NONE {
            return None;
        }
        if w & Ref::CONST_BIT != 0 {
            let &(ty, bits) = body.consts.get((w & !Ref::CONST_BIT) as usize)?;
            return (ty != Ty::STRING).then_some(Value::Scalar(bits));
        }
        self.val.get(w as usize).copied().flatten()
    }

    /// The one value every word in `words` has, if there is at least one.
    fn agree(&self, body: &Body, words: impl Iterator<Item = u32>) -> Option<Value> {
        let mut out = None;
        for w in words {
            let v = self.word(body, w)?;
            if out.is_some_and(|o| o != v) {
                return None;
            }
            out = Some(v);
        }
        out
    }

    /// The block decision `i` (an `If` on a `bool` or a `Match`) runs,
    /// where its operand's value decides it.
    fn chosen(&self, body: &Body, i: usize) -> Option<u32> {
        let blocks = body.record(body.data[i][1]);
        let block = match body.tags[i] {
            Tag::Match => blocks.get(self.arm(body, blocks[0], 0)? as usize + 1),
            Tag::If => blocks.get(self.taken(body, i)?),
            _ => None,
        };
        block.copied().filter(|&k| k != NONE)
    }

    /// The arm `If` `i` on a `bool` runs, 0 or 1, where its condition's
    /// value decides it; the arm may be absent (`NONE`).
    fn taken(&self, body: &Body, i: usize) -> Option<usize> {
        let a = body.data[i][0];
        let bool_cond = body.ty.get(a as usize) == Some(&Ty::BOOL)
            || (a & Ref::CONST_BIT != 0
                && body
                    .consts
                    .get((a & !Ref::CONST_BIT) as usize)
                    .is_some_and(|c| c.0 == Ty::BOOL));
        match self.word(body, a).filter(|_| bool_cond)? {
            Value::Scalar(bits) => Some(usize::from(bits == 0)),
            Value::Variant(_) => None,
        }
    }

    /// The block switch `i` runs, where its operand's value decides it.
    fn switched(&self, body: &Body, i: usize) -> Option<u32> {
        let rec = body.record(body.data[i][1]);
        let equal = match (body.tags[i], self.word(body, body.data[i][0])?) {
            (Tag::SwitchTag, Value::Variant(tag)) => tag == rec[0],
            (Tag::SwitchInt | Tag::SwitchChar, Value::Scalar(bits)) => {
                bits & u64::from(u32::MAX) == u64::from(rec[0])
            }
            _ => return None,
        };
        rec.get(if equal { 1 } else { 2 })
            .copied()
            .filter(|&k| k != NONE)
    }

    /// Follow a pure match decision to its arm. Binding patterns and
    /// guards with unknown results retain the original match and all its
    /// effects. The decision tree's depth is the pattern's, capped at 64.
    fn arm(&self, body: &Body, block: u32, depth: usize) -> Option<u32> {
        if depth > 64 {
            return None;
        }
        for &i in body.record(body.data[block as usize][0]) {
            let i = i as usize;
            let [a, b] = body.data[i];
            match body.tags[i] {
                Tag::Block => {}
                Tag::Read if self.pure[i] || self.val[i].is_some() => {}
                Tag::ToArm => return Some(a),
                Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar => {
                    let val = self.word(body, a)?;
                    let rec = body.record(b);
                    let equal = match (body.tags[i], val) {
                        (Tag::SwitchTag, Value::Variant(tag)) => tag == rec[0],
                        (Tag::SwitchInt | Tag::SwitchChar, Value::Scalar(bits)) => {
                            bits & u64::from(u32::MAX) == u64::from(rec[0])
                        }
                        _ => return None,
                    };
                    return self.arm(body, rec[if equal { 1 } else { 2 }], depth + 1);
                }
                _ => return None,
            }
        }
        None
    }
}

/// Which arms the decisions seen so far rule out, for [`Values`]: the
/// executable-edge half of SCCP (Wegman and Zadeck) for the one question a
/// block's value asks, whether a `Break` to it can run. Each block knows
/// its enclosing block, and each arm its decision; a decision comes before
/// the block that holds it (`hd_tir::ir::topo`), so by the time a block
/// reads its `Break`s, every decision between them and it is settled. The
/// walk up from a `Break` keeps union-find's path compression: `jump[x]`
/// skips blocks already found live, so all walks together cost near O(n).
struct Arms {
    /// Per instruction: the block that lists it, or `NONE`.
    parent: Vec<u32>,
    /// Per arm block: its decision, or `NONE`.
    decision: Vec<u32>,
    /// Per decision: the arm block it runs, once settled, or `NONE`.
    chosen: Vec<u32>,
    /// Per block: an enclosing block such that every block strictly
    /// between them is live, or `NONE` for the parent.
    jump: Vec<u32>,
}

impl Arms {
    fn new(body: &Body) -> Self {
        let n = body.len();
        let mut parent = vec![NONE; n];
        let mut decision = vec![NONE; n];
        for i in 0..n {
            let [a, b] = body.data[i];
            let tag = body.tags[i];
            if tag == Tag::Block {
                for &m in body.record(a) {
                    if let Some(p) = parent.get_mut(m as usize) {
                        *p = index(i);
                    }
                }
            }
            // A decision's arms: an `if`'s or a `match`'s blocks, a
            // switch's two after its value.
            let arms = match tag {
                Tag::If | Tag::Match => body.record(b),
                Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar => {
                    body.record(b).get(1..3).unwrap_or(&[])
                }
                _ => &[],
            };
            for &k in arms {
                if let Some(d) = decision.get_mut(k as usize) {
                    *d = index(i);
                }
            }
        }
        Self {
            parent,
            decision,
            chosen: vec![NONE; n],
            jump: vec![NONE; n],
        }
    }

    fn decide(&mut self, i: usize, arm: Option<u32>) {
        self.chosen[i] = arm.unwrap_or(NONE);
    }

    /// The block enclosing `x`: the block that lists it, or for an arm
    /// listed by none, the one that lists its decision.
    fn up(&self, x: u32) -> u32 {
        match self.parent[x as usize] {
            NONE => match self.decision[x as usize] {
                NONE => NONE,
                d => self.parent[d as usize],
            },
            p => p,
        }
    }

    /// Whether `Break` `k` can run as far as its target block `t`: no
    /// block between them is an arm its settled decision does not run.
    fn live(&mut self, k: u32, t: u32) -> bool {
        let mut seen = Vec::new();
        let mut x = self.up(k);
        let alive = loop {
            if x == NONE || x >= t {
                break true;
            }
            let d = self.decision[x as usize];
            if d != NONE && self.chosen[d as usize] != NONE && self.chosen[d as usize] != x {
                break false;
            }
            seen.push(x);
            x = match self.jump[x as usize] {
                NONE => self.up(x),
                j => j,
            };
        };
        for s in seen {
            self.jump[s as usize] = x;
        }
        alive
    }
}

/// `n`, a length or position in a body's columns, as a TIR word.
fn index(n: usize) -> u32 {
    // ruled out: TIR addresses instructions, records and constants with
    // `u32` words, so no column of a body grows past `u32::MAX`.
    u32::try_from(n).expect("a TIR column fits u32 words")
}

/// The item's parameter locals, excluding parameters of nested functions.
fn params(body: &Body) -> Vec<usize> {
    let mut nested = vec![false; body.local_ty.len()];
    for sub in 1..body.sub_params.len() {
        for &l in body.sub_param_locals(sub) {
            nested[l as usize] = true;
        }
    }
    body.local_flags
        .iter()
        .enumerate()
        .filter_map(|(l, &f)| (f & local_flags::PARAM != 0 && !nested[l]).then_some(l))
        .collect()
}

/// Fold the decisions [`Values`] decides and remove what they no longer
/// run: LLVM `SimplifyCFG`'s constant-branch fold, over the values `defaults`
/// seeds and those the body computes itself (an inlined trait method's
/// constant result among them, #1518). Ordinary instances with nothing to
/// fold keep their original identity: `None`, and no copy is made. Cost:
/// [`Values`], then, only when something folds, one copy and one marking
/// walk, O(n).
#[must_use]
pub fn specialize(body: &Body, defaults: &[(u32, Value)]) -> Option<Body> {
    let values = Values::new(body, defaults, &|_| None)?;
    // A decisive short-circuit operand is itself a branch. Replace the
    // operator, not just an enclosing decision, so reachability does not
    // retain the unevaluated right-hand block. Keep the left operand as an
    // identity coercion because proving its value does not prove it has no
    // effects. A non-deciding left operand leaves the operator: its
    // right-hand block runs.
    let decisive = |i: usize| match (body.tags[i], values.word(body, body.data[i][0])) {
        (Tag::And, Some(Value::Scalar(0))) => body.ty[i] == Ty::BOOL,
        (Tag::Or, Some(Value::Scalar(bits))) => bits != 0 && body.ty[i] == Ty::BOOL,
        _ => false,
    };
    let folds: Vec<(usize, Fold)> = (0..body.len())
        .filter_map(|i| match body.tags[i] {
            Tag::And | Tag::Or => decisive(i).then_some((i, Fold::Left)),
            // A literal condition is the backend's to fold, as it folds
            // `if true:` (with no block left around the arm): this pass
            // folds what only the instance's own values decide.
            Tag::If if body.data[i][0] & Ref::CONST_BIT == 0 => {
                values.taken(body, i).map(|k| (i, Fold::Arm(k)))
            }
            Tag::Match => values.chosen(body, i).map(|k| (i, Fold::Scope(k))),
            _ => None,
        })
        .collect();
    if folds.is_empty() {
        return None;
    }
    // A pure condition only its folded decision used goes too, as
    // dead-code elimination would remove it: an inlined trait method's
    // constant body leaves no empty block behind.
    let mut uses = vec![0u32; body.len()];
    for i in 0..body.len() {
        slots(body, i, |_, v, w| {
            if matches!(w, Word::Value(_) | Word::Provider)
                && v & Ref::CONST_BIT == 0
                && let Some(u) = uses.get_mut(v as usize)
            {
                *u += 1;
            }
        });
    }
    let mut out = body.clone();
    let empty = index(out.extra.len());
    out.extra.push(0);
    for (i, fold) in folds {
        match fold {
            // A decision left with one arm that never finishes never
            // finishes either, so what follows it in its block is dead
            // (`mark`): a let-else whose `else` is the arm that runs
            // leaves its bindings unstored, and their reads go with it.
            Fold::Scope(block) => {
                out.tags[i] = Tag::Scope;
                out.data[i] = [block, empty];
                if body.ty[block as usize] == Ty::NEVER {
                    out.ty[i] = Ty::NEVER;
                }
            }
            // The `if` stays, on its condition's constant, so the backend
            // folds it as it folds `if true:` (no block is left around the
            // arm); the arm that cannot run keeps only an `Unreachable`.
            Fold::Arm(k) => {
                let [cond, arms] = body.data[i];
                let at = index(out.consts.len());
                out.consts.push((Ty::BOOL, u64::from(k == 0)));
                out.data[i][0] = Ref::konst(at).0;
                let c = cond as usize;
                if uses.get(c) == Some(&1) && values.pure[c] {
                    out.tags[c] = Tag::Block;
                    out.data[c] = [empty, NONE];
                    out.ty[c] = Ty::VOID;
                }
                if let Some(&dead) = body.record(arms).get(1 - k) {
                    unreachable_arm(&mut out, dead);
                }
                if let Some(&taken) = body.record(arms).get(k)
                    && body.ty.get(taken as usize) == Some(&Ty::NEVER)
                {
                    out.ty[i] = Ty::NEVER;
                }
            }
            Fold::Left => {
                let coercion = index(out.extra.len());
                out.extra.push(2);
                out.extra.push(Coercion::Identity as u32);
                out.extra.push(NONE);
                out.tags[i] = Tag::Coerce;
                out.data[i] = [body.data[i][0], coercion];
            }
        }
    }
    // Remove unreachable instructions, not just their calls: otherwise the
    // emitter could still reserve suspension states and frame fields.
    let live = mark(&out, out.sub_root[0]);
    for (i, reached) in live.iter().enumerate() {
        if !reached {
            out.tags[i] = Tag::Block;
            out.data[i] = [empty, NONE];
            out.ty[i] = Ty::VOID;
        }
    }
    // A block whose tail comes after a member that never finishes has no
    // value, as a dead arm's block has none (`unreachable_arm`).
    for i in 0..out.len() {
        let tail = out.data[i][1];
        if live[i]
            && out.tags[i] == Tag::Block
            && tail != NONE
            && tail & Ref::CONST_BIT == 0
            && !live[tail as usize]
        {
            out.data[i][1] = NONE;
        }
    }
    out.susp.retain(|s| live[s.inst as usize]);
    out.fallback.retain(|f| live[f[0] as usize]);
    out.held_args
        .retain(|r| live[r.0 as usize] && live[r.1 as usize]);
    out.unwind_drops
        .retain(|r| live[r.0 as usize] && live[r.1 as usize]);
    out.stable_lists.retain(|r| live[r.0 as usize]);
    out.full_cells.retain(|r| live[r.0 as usize]);
    Some(out)
}

/// A copy of `body` that only makes its result, where the result is a
/// known value and running the body has no effect: the inliner's copy of
/// a function whose every way out gives one constant (a derived per-type
/// flag over its members' flags, once their copies are folded too, #1518)
/// is one instruction, however large it was. Like LLVM's IPSCCP
/// replacing a call to a function that returns a constant. `None` for any
/// other body, one with sub-bodies, or one already that small. Cost: one
/// pass of [`Values`], O(n).
#[must_use]
pub fn collapse(body: &Body) -> Option<Body> {
    let &[root] = body.sub_root.as_slice() else {
        return None;
    };
    let values = Values::compute(body, &[], &|_| None);
    let value = values
        .word(body, root)
        .filter(|_| values.pure[root as usize])?;
    collapsed(body, root, value)
}

/// [`collapse`] for a body whose effects all stay in its own frame: it
/// writes only its own locals, makes values, and calls only what
/// `quiet(body, i)` vouches for (a `Call` or a `Drop` with no effect a
/// caller sees: a generated structure handle, a type with no drop glue).
/// A derived flag runs a describer over its type, which binds handles and
/// lends a describer local, and still only makes its result (#1445). Like
/// LLVM's `FunctionAttrs` inferring `readnone` from local-only stores
/// before IPSCCP folds its calls. Cost: one pass, O(n).
#[must_use]
pub fn collapse_quiet(body: &Body, quiet: &dyn Fn(&Body, usize) -> bool) -> Option<Body> {
    if !local_only(body, quiet) {
        return None;
    }
    constant_copy(body, value_given(body, &|_| None)?)
}

/// Whether running `body` has no effect its caller sees: its one root
/// only writes its own locals, makes values, and calls only what
/// `quiet(body, i)` vouches for ([`collapse_quiet`]'s test). Cost: one
/// pass, O(n).
#[must_use]
pub fn local_only(body: &Body, quiet: &dyn Fn(&Body, usize) -> bool) -> bool {
    if body.sub_root.len() != 1 {
        return false;
    }
    for i in 0..body.len() {
        let a = body.data[i][0];
        let ok = match body.tags[i] {
            Tag::Block
            | Tag::Scope
            | Tag::Break
            | Tag::ToArm
            | Tag::If
            | Tag::Match
            | Tag::SwitchTag
            | Tag::SwitchInt
            | Tag::SwitchChar
            | Tag::Unreachable
            | Tag::Read
            | Tag::Ref
            | Tag::LoanEnd
            | Tag::Release
            | Tag::NewStruct
            | Tag::NewVariant
            | Tag::NewTuple
            | Tag::And
            | Tag::Or => true,
            Tag::Coerce => transparent(body, i),
            Tag::Prim => PrimOp::from_u32(a) == Some(PrimOp::Not),
            // A store into a local of its own, not a parameter's referent.
            Tag::Store => body.decode_place(a).is_some_and(|p| {
                matches!(p.root, hd_tir::ir::PlaceRoot::Local(l)
                    if body.local_flags[l.idx()] & local_flags::PARAM == 0)
                    && !p.proj.contains(&hd_tir::ir::Proj::Deref)
            }),
            Tag::Call | Tag::Drop => quiet(body, i),
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    true
}

/// The value `body`'s root makes when each `Call` `calls` answers
/// returns that answer ([`Values`] over the seeded results), or `None`
/// when that does not decide it. An unknown result decides an operator or
/// a decision only where every value it could take gives one answer, as
/// Kleene's strong three-valued logic reads unknowns. Cost: one pass,
/// O(n).
#[must_use]
pub fn value_given(body: &Body, calls: &dyn Fn(usize) -> Option<Value>) -> Option<Value> {
    let &[root] = body.sub_root.as_slice() else {
        return None;
    };
    Values::compute(body, &[], calls).word(body, root)
}

/// Reachable instructions under independently proved call results, without
/// copying or rewriting the body. Known results never erase their producers.
/// Values and normal completion are computed once in topological order; an
/// explicit worklist marks each reachable instruction and its operand edges.
#[must_use]
pub fn reachable_given(body: &Body, calls: &dyn Fn(usize) -> Option<Value>) -> Vec<bool> {
    reachable_given_with(body, calls, &mut || {})
}

fn reachable_given_with(
    body: &Body,
    calls: &dyn Fn(usize) -> Option<Value>,
    visit: &mut impl FnMut(),
) -> Vec<bool> {
    let values = Values::compute(body, &[], calls);
    let chosen = |i: usize| match body.tags[i] {
        Tag::If | Tag::Match => values.chosen(body, i),
        Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar => values.switched(body, i),
        _ => None,
    };
    let mut finishes = vec![true; body.len()];
    for i in 0..body.len() {
        visit();
        finishes[i] = if let Some(block) = chosen(i) {
            block == NONE || finishes.get(block as usize).copied().unwrap_or(true)
        } else if body.tags[i] == Tag::Block {
            let [members, tail] = body.data[i];
            let mut end = true;
            for &member in body.record(members) {
                visit();
                if body.tags[member as usize] != Tag::Block && !finishes[member as usize] {
                    end = false;
                    break;
                }
            }
            end && (tail == NONE || Ref(tail).as_inst().is_none_or(|v| finishes[v.0 as usize]))
        } else {
            body.ty[i] != Ty::NEVER
        };
    }
    let mut live = vec![false; body.len()];
    let mut work = body
        .sub_root
        .first()
        .copied()
        .into_iter()
        .collect::<Vec<_>>();
    while let Some(word) = work.pop() {
        visit();
        let Some(at) = Ref(word).as_inst().filter(|v| (v.0 as usize) < body.len()) else {
            continue;
        };
        let i = at.0 as usize;
        if live[i] {
            continue;
        }
        live[i] = true;
        let [a, b] = body.data[i];
        if body.tags[i] == Tag::Block {
            let mut end = true;
            for &member in body.record(a) {
                visit();
                if body.tags[member as usize] == Tag::Block {
                    continue;
                }
                work.push(member);
                if !finishes[member as usize] {
                    end = false;
                    break;
                }
            }
            if end {
                work.push(b);
            }
            continue;
        }
        if let Some(block) = chosen(i) {
            work.push(a);
            work.push(block);
            continue;
        }
        if matches!(body.tags[i], Tag::And | Tag::Or)
            && let Some(Value::Scalar(value)) = values.word(body, a)
            && ((body.tags[i] == Tag::And && value == 0) || (body.tags[i] == Tag::Or && value != 0))
        {
            work.push(a);
            continue;
        }
        if body.tags[i] == Tag::Closure
            && let Some(&root) = body.sub_root.get(a as usize)
        {
            work.push(root);
        }
        if let Some(arguments) = body.value_operands(i) {
            work.extend(arguments);
        }
        if matches!(body.tags[i], Tag::If | Tag::Match) {
            work.push(a);
            work.extend_from_slice(body.record(b));
            continue;
        }
        if matches!(
            body.tags[i],
            Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr
        ) {
            work.push(a);
            work.extend(body.record(b).iter().skip(1).take(2));
            continue;
        }
        for (op, word) in [body.tags[i].operands().0, body.tags[i].operands().1]
            .into_iter()
            .zip([a, b])
        {
            match op {
                Op::Value => work.push(word),
                Op::Block => {
                    if body.tags.get(word as usize) == Some(&Tag::Block) {
                        work.push(word);
                    }
                }
                Op::Blocks => work.extend_from_slice(body.record(word)),
                // Value lists were visited by value_operands; metadata,
                // identities and places do not select a control-flow block.
                Op::None
                | Op::Inst
                | Op::Label
                | Op::Meta
                | Op::Values
                | Op::Record
                | Op::Place => {}
            }
        }
    }
    live
}

/// `body` as one instruction that makes `value` ([`collapse`]'s copy);
/// `None` for a body already that small.
#[must_use]
pub fn constant_copy(body: &Body, value: Value) -> Option<Body> {
    let &[root] = body.sub_root.as_slice() else {
        return None;
    };
    collapsed(body, root, value)
}

/// `body` as one instruction that makes `value`, its root's result.
fn collapsed(body: &Body, root: u32, value: Value) -> Option<Body> {
    let (ty, syn) = (body.ty[root as usize], body.syn[root as usize]);
    let mut out = Body::new(body.item, body.kind);
    // A constant copy has no operands, labels or non-parameter locals.
    // Keeping their dead tables makes a one-row result grow with every
    // enclosing copy, defeating both folding and the inliner's metadata
    // bound. Preserve only the argument contract; Values excludes string
    // constants, so no literal table is referenced by the known value.
    for local in hd_tir::own::params(body) {
        let local = local as usize;
        out.local_ty.push(body.local_ty[local]);
        out.local_name.push(body.local_name[local]);
        out.local_syn.push(body.local_syn[local]);
        out.local_flags.push(body.local_flags[local]);
    }
    out.sub_params.push(Range32::default());
    out.sub_parent.clone_from(&body.sub_parent);
    out.sub_flags.clone_from(&body.sub_flags);
    out.label.clone_from(&body.label);
    // An instruction the fold made has no checker instruction of its own;
    // a body with no anchors (the checker's) keeps none.
    let anchored = !body.anchors.is_empty();
    let mut push = |tag: Tag, data: [u32; 2], ty: Ty| {
        out.tags.push(tag);
        out.data.push(data);
        out.ty.push(ty);
        out.syn.push(syn);
        if anchored {
            out.anchors.push(NONE);
        }
        index(out.tags.len() - 1)
    };
    // Records: an empty one, then the root's list.
    let empty = 0;
    let (members, tail) = match value {
        Value::Scalar(bits) => {
            let at = index(out.consts.len());
            out.consts.push((ty, bits));
            (Vec::new(), Ref::konst(at).0)
        }
        Value::Variant(tag) => {
            let made = push(Tag::NewVariant, [tag, empty], ty);
            (vec![made], made)
        }
    };
    if body.len() <= members.len() + 1 {
        return None;
    }
    out.extra.push(0);
    out.extra.push(index(members.len()));
    out.extra.extend_from_slice(&members);
    let root = push(Tag::Block, [empty + 1, tail], ty);
    out.sub_root = vec![root];
    Some(out)
}

/// How [`specialize`] folds one decision.
enum Fold {
    /// A decisive `&&` or `||`: its left operand.
    Left,
    /// An `If`: arm 0 or 1 runs.
    Arm(usize),
    /// A `Match`: the arm block that runs.
    Scope(u32),
}

/// Makes `If` arm `dead`, which never runs, hold only an `Unreachable`:
/// the first instruction it lists becomes one (its index comes before the
/// block's, as TIR's order needs), and every other instruction of the arm
/// is left unreachable for [`specialize`]'s marking to remove. An arm that
/// lists no instruction (its tail a constant or an earlier value) stays.
fn unreachable_arm(out: &mut Body, dead: u32) {
    if dead == NONE || out.tags.get(dead as usize) != Some(&Tag::Block) {
        return;
    }
    let list = out.data[dead as usize][0];
    let member = out
        .record(list)
        .iter()
        .copied()
        .find(|&m| out.tags[m as usize] != Tag::Block);
    let Some(u) = member else {
        return;
    };
    out.tags[u as usize] = Tag::Unreachable;
    out.data[u as usize] = [NONE, NONE];
    out.ty[u as usize] = Ty::NEVER;
    let rec = index(out.extra.len());
    out.extra.push(1);
    out.extra.push(u);
    out.data[dead as usize] = [rec, NONE];
}

/// The instructions reachable from `root`, by a worklist: each is pushed
/// once, so the walk is O(n) and its depth is the heap's, not the stack's.
/// A block's members run in order, so none after one that never finishes
/// (`never`: a `return`, a `break`, a folded decision whose arm returns)
/// is reached, and neither is the block's tail.
fn mark(body: &Body, root: u32) -> Vec<bool> {
    let mut live = vec![false; body.len()];
    let mut work = vec![root];
    while let Some(i) = work.pop() {
        if i == NONE || i & Ref::CONST_BIT != 0 {
            continue;
        }
        let i = i as usize;
        if i >= live.len() || live[i] {
            continue;
        }
        live[i] = true;
        let [a, b] = body.data[i];
        if body.tags[i] == Tag::Block {
            let mut finishes = true;
            for &child in body.record(a) {
                if body.tags[child as usize] == Tag::Block {
                    continue;
                }
                work.push(child);
                if body.ty[child as usize] == Ty::NEVER {
                    finishes = false;
                    break;
                }
            }
            if finishes {
                work.push(b);
            }
            continue;
        }
        if body.tags[i] == Tag::Closure
            && let Some(&root) = body.sub_root.get(a as usize)
        {
            work.push(root);
        }
        if let Some(values) = body.value_operands(i) {
            work.extend(values);
        }
        if body.tags[i] == Tag::Match || body.tags[i] == Tag::If {
            work.extend_from_slice(body.record(b));
            work.push(a);
            continue;
        }
        if matches!(
            body.tags[i],
            Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr
        ) {
            work.extend(body.record(b).iter().skip(1).take(2));
            work.push(a);
            continue;
        }
        for (op, word) in [body.tags[i].operands().0, body.tags[i].operands().1]
            .into_iter()
            .zip([a, b])
        {
            match op {
                Op::Value => work.push(word),
                Op::Block if body.tags.get(word as usize) == Some(&Tag::Block) => {
                    work.push(word);
                }
                Op::Blocks => work.extend_from_slice(body.record(word)),
                _ => {}
            }
        }
    }
    live
}

#[cfg(test)]
mod tests {
    use super::*;
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_tir::ir::{BodyKind, Callee, Providers, TirBuilder, TirSink};
    use hd_types::TyList;

    #[test]
    fn constant_result_after_a_call_is_not_a_constant_default() {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Default);
        let block = b.open_block();
        b.call(
            &Callee::Item {
                def: DefId::from_raw(2),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let result = b.const_value(Ty::BOOL, 0);
        let root = b.close_block(block, Some(result), Ty::BOOL, NodeIdx::NONE);
        let body = b.finish(root, &[]).expect("valid default body");
        assert_eq!(constant(&body), None);
    }

    #[test]
    fn folding_drops_the_unselected_calls_and_retains_source_indices() {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let param = b.local(
            Ty::BOOL,
            Symbol::from_raw(0),
            local_flags::PARAM,
            NodeIdx::NONE,
        );
        let cond = b.get(param, Ty::BOOL, NodeIdx::NONE);
        let yes = b.open_block();
        let dead = b.call(
            &Callee::Item {
                def: DefId::from_raw(2),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let yes = b.close_block(yes, None, Ty::VOID, NodeIdx::NONE);
        let no = b.open_block();
        let kept = b.call(
            &Callee::Item {
                def: DefId::from_raw(3),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let no = b.close_block(no, None, Ty::VOID, NodeIdx::NONE);
        let blocks = b.refs_record(&[yes, no]);
        b.emit(Tag::If, cond.0, blocks, Ty::VOID, NodeIdx::NONE);
        let root = b.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        let body = b.finish(root, &[]).expect("valid function body");
        let out = specialize(&body, &[(0, Value::Scalar(0))]).expect("constant false branch");
        assert_eq!(out.tags.len(), body.tags.len());
        assert_eq!(out.syn, body.syn);
        assert!(gone(&out, dead));
        assert_eq!(out.tags[kept.0 as usize], Tag::Call);
        let errors = hd_tir::ir::verify(&out);
        assert!(errors.is_empty(), "specialized TIR: {errors:?}");
    }

    #[test]
    fn decisive_short_circuit_drops_its_rhs_and_unselected_branch() {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let left = b.open_block();
        let left_call = b.call(
            &Callee::Item {
                def: DefId::from_raw(5),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let left_value = b.const_value(Ty::BOOL, 0);
        let left = b.close_block(left, Some(left_value), Ty::BOOL, NodeIdx::NONE);
        let rhs = b.open_block();
        let rhs_call = b.call(
            &Callee::Item {
                def: DefId::from_raw(2),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let rhs_value = b.const_value(Ty::BOOL, 1);
        let rhs = b.close_block(rhs, Some(rhs_value), Ty::BOOL, NodeIdx::NONE);
        let cond = b.emit(Tag::And, left.0, rhs.0, Ty::BOOL, NodeIdx::NONE);
        let yes = b.open_block();
        let yes_call = b.call(
            &Callee::Item {
                def: DefId::from_raw(3),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let yes = b.close_block(yes, None, Ty::VOID, NodeIdx::NONE);
        let no = b.open_block();
        let no_call = b.call(
            &Callee::Item {
                def: DefId::from_raw(4),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let no = b.close_block(no, None, Ty::VOID, NodeIdx::NONE);
        let blocks = b.refs_record(&[yes, no]);
        b.emit(Tag::If, cond.0, blocks, Ty::VOID, NodeIdx::NONE);
        let root = b.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        let body = b.finish(root, &[]).expect("valid function body");

        let out = specialize(&body, &[]).expect("constant false short circuit");
        assert_eq!(out.tags[left_call.0 as usize], Tag::Call);
        assert!(gone(&out, rhs_call));
        assert!(gone(&out, yes_call));
        assert_eq!(out.tags[no_call.0 as usize], Tag::Call);
        let errors = hd_tir::ir::verify(&out);
        assert!(errors.is_empty(), "specialized TIR: {errors:?}");
    }

    /// Whether instruction `r` no longer runs: removed, or the
    /// `Unreachable` a dead arm keeps.
    fn gone(out: &Body, r: Ref) -> bool {
        matches!(out.tags[r.0 as usize], Tag::Block | Tag::Unreachable)
    }

    fn item(def: u32) -> Callee {
        Callee::Item {
            def: DefId::from_raw(def),
            targs: TyList::EMPTY,
        }
    }

    /// `if <cond>: 2() else: 3()` closed into a function body, where
    /// `cond` builds the condition; gives the body and the two calls.
    fn decide(cond: impl FnOnce(&mut TirBuilder) -> Ref) -> (Body, Ref, Ref) {
        let n = NodeIdx::NONE;
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let cond = cond(&mut b);
        let yes = b.open_block();
        let two = b.call(&item(2), &[], Providers::None, Ty::VOID, n);
        let yes = b.close_block(yes, None, Ty::VOID, n);
        let no = b.open_block();
        let three = b.call(&item(3), &[], Providers::None, Ty::VOID, n);
        let no = b.close_block(no, None, Ty::VOID, n);
        let blocks = b.refs_record(&[yes, no]);
        b.emit(Tag::If, cond.0, blocks, Ty::VOID, n);
        let root = b.close_block(root, None, Ty::VOID, n);
        (
            b.finish(root, &[]).expect("valid function body"),
            two,
            three,
        )
    }

    /// An inlined callee's shape: a `Scope` over a labelled block that
    /// leaves with `Break`s carrying `results`, and `tail` as its value.
    fn inlined(b: &mut TirBuilder, results: &[u64], tail: Option<u64>) -> Ref {
        let n = NodeIdx::NONE;
        let label = b.open_loop();
        let inner = b.open_block();
        for &r in results {
            let v = b.const_value(Ty::BOOL, r);
            b.emit(Tag::Break, label.0.raw(), v.0, Ty::NEVER, n);
        }
        let tail = tail.map(|t| b.const_value(Ty::BOOL, t));
        let inner = b.close_block(inner, tail, Ty::BOOL, n);
        b.body_mut().label_inst[label.0.idx()] = inner.0;
        let scope = b.open_scope();
        b.close_scope(scope, inner, Ty::BOOL, n)
    }

    /// #1445: a `return` in an arm its own constant condition rules out
    /// (a `?` on a known `.Ok`) never runs, so the copy's tail alone is
    /// its value and the caller's branch folds.
    #[test]
    fn a_break_in_a_ruled_out_arm_does_not_unsettle_its_block() {
        let n = NodeIdx::NONE;
        let mut cond = Ref(NONE);
        let (body, two, three) = decide(|b| {
            let label = b.open_loop();
            let inner = b.open_block();
            let yes = b.open_block();
            let yes = b.close_block(yes, None, Ty::VOID, n);
            let no = b.open_block();
            let zero = b.const_value(Ty::BOOL, 0);
            b.emit(Tag::Break, label.0.raw(), zero.0, Ty::NEVER, n);
            let no = b.close_block(no, None, Ty::NEVER, n);
            let known = b.const_value(Ty::BOOL, 1);
            let arms = b.refs_record(&[yes, no]);
            b.emit(Tag::If, known.0, arms, Ty::VOID, n);
            let one = b.const_value(Ty::BOOL, 1);
            let inner = b.close_block(inner, Some(one), Ty::BOOL, n);
            b.body_mut().label_inst[label.0.idx()] = inner.0;
            let scope = b.open_scope();
            cond = b.close_scope(scope, inner, Ty::BOOL, n);
            cond
        });
        // ruled out: the constant condition settles the `if`, so the fold
        // has a decision to make.
        let out = specialize(&body, &[]).expect("the dead arm's break is no value");
        assert_eq!(out.tags[two.0 as usize], Tag::Call);
        assert!(gone(&out, three));
        // The condition's `if` is no pure value, so the scope stays.
        assert_eq!(out.tags[cond.0 as usize], Tag::Scope);
    }

    /// #1518: an inlined trait method's constant result decides the
    /// branch; the pure condition and the other arm's call go, and the
    /// `if` stays on a literal for the backend to fold.
    #[test]
    fn an_inlined_constant_result_drops_the_other_arm() {
        let mut cond = Ref(NONE);
        let (body, two, three) = decide(|b| {
            cond = inlined(b, &[1], None);
            cond
        });
        let out = specialize(&body, &[]).expect("a constant inlined result");
        assert_eq!(out.tags[two.0 as usize], Tag::Call);
        assert!(gone(&out, three));
        assert_eq!(out.tags[cond.0 as usize], Tag::Block);
        let at = out.tags.iter().position(|t| *t == Tag::If).expect("the if");
        let word = out.data[at][0];
        assert_ne!(word & Ref::CONST_BIT, 0);
        assert_eq!(out.consts[(word & !Ref::CONST_BIT) as usize], (Ty::BOOL, 1));
        let errors = hd_tir::ir::verify(&out);
        assert!(errors.is_empty(), "specialized TIR: {errors:?}");
    }

    /// A constant result after an effect still decides the branch, but
    /// the condition stays, so its call still runs.
    #[test]
    fn an_effect_before_a_constant_result_stays() {
        let n = NodeIdx::NONE;
        let (mut effect, mut cond) = (Ref(NONE), Ref(NONE));
        let (body, two, three) = decide(|b| {
            let label = b.open_loop();
            let inner = b.open_block();
            effect = b.call(&item(5), &[], Providers::None, Ty::VOID, n);
            let v = b.const_value(Ty::BOOL, 0);
            b.emit(Tag::Break, label.0.raw(), v.0, Ty::NEVER, n);
            let inner = b.close_block(inner, None, Ty::BOOL, n);
            b.body_mut().label_inst[label.0.idx()] = inner.0;
            let scope = b.open_scope();
            cond = b.close_scope(scope, inner, Ty::BOOL, n);
            cond
        });
        let out = specialize(&body, &[]).expect("a constant after an effect");
        assert_eq!(out.tags[effect.0 as usize], Tag::Call);
        assert_eq!(out.tags[cond.0 as usize], Tag::Scope);
        assert!(gone(&out, two));
        assert_eq!(out.tags[three.0 as usize], Tag::Call);
        let errors = hd_tir::ir::verify(&out);
        assert!(errors.is_empty(), "specialized TIR: {errors:?}");
    }

    /// A block's value is the one its tail and every `Break` to it agree
    /// on: an early `return false` before a final `true` decides nothing.
    #[test]
    fn breaks_that_disagree_with_the_tail_decide_nothing() {
        let (body, ..) = decide(|b| inlined(b, &[0], Some(1)));
        assert!(specialize(&body, &[]).is_none());
        let (body, two, three) = decide(|b| inlined(b, &[0, 0], Some(0)));
        let out = specialize(&body, &[]).expect("every way out agrees");
        assert!(gone(&out, two));
        assert_eq!(out.tags[three.0 as usize], Tag::Call);
    }

    /// A local stored once but lent mutably may change anywhere, so a
    /// read of it is no constant.
    #[test]
    fn a_mutably_lent_local_is_no_constant() {
        let n = NodeIdx::NONE;
        for lend in [false, true] {
            let (body, ..) = decide(|b| {
                let l = b.local(Ty::BOOL, Symbol::from_raw(0), 0, n);
                let t = b.const_value(Ty::BOOL, 1);
                b.set(l, t, n);
                if lend {
                    let place = b.place(&hd_tir::ir::Place::local(l));
                    b.emit(Tag::Ref, place, hd_tir::ir::ref_word::MUT, Ty::VOID, n);
                }
                b.get(l, Ty::BOOL, n)
            });
            assert_eq!(specialize(&body, &[]).is_some(), !lend, "lent: {lend}");
        }
    }

    /// A decided `if` whose arm that runs returns ends its block: a read
    /// after it of a local only the other arm stores goes with the block's
    /// tail, as in a let-else whose `else` is known to run.
    #[test]
    fn a_decision_whose_arm_returns_drops_what_follows() {
        let n = NodeIdx::NONE;
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let param = b.local(Ty::BOOL, Symbol::from_raw(0), local_flags::PARAM, n);
        let bound = b.local(Ty::I32, Symbol::from_raw(1), 0, n);
        let cond = b.get(param, Ty::BOOL, n);
        let yes = b.open_block();
        let minus = b.const_value(Ty::I32, 1);
        b.emit(Tag::Return, minus.0, NONE, Ty::NEVER, n);
        let yes = b.close_block(yes, None, Ty::NEVER, n);
        let no = b.open_block();
        let five = b.const_value(Ty::I32, 5);
        b.set(bound, five, n);
        let no = b.close_block(no, None, Ty::VOID, n);
        let blocks = b.refs_record(&[yes, no]);
        b.emit(Tag::If, cond.0, blocks, Ty::VOID, n);
        let read = b.get(bound, Ty::I32, n);
        let root = b.close_block(root, Some(read), Ty::I32, n);
        let body = b.finish(root, &[]).expect("valid function body");
        let store = body
            .tags
            .iter()
            .position(|t| *t == Tag::Store)
            .expect("the store");
        let out = specialize(&body, &[(0, Value::Scalar(1))]).expect("a decided if");
        assert!(gone(&out, Ref(index(store))));
        assert!(gone(&out, read));
        assert_eq!(out.data[root.0 as usize][1], NONE);
        let errors = hd_tir::ir::verify(&out);
        assert!(errors.is_empty(), "specialized TIR: {errors:?}");
    }

    /// A body whose only effect is one known result collapses to it; one
    /// that also calls something does not.
    #[test]
    fn a_pure_constant_body_collapses_to_its_value() {
        let n = NodeIdx::NONE;
        for call in [false, true] {
            let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
            let root = b.open_block();
            if call {
                b.call(&item(2), &[], Providers::None, Ty::VOID, n);
            }
            let left = inlined(&mut b, &[1], None);
            let rhs = b.open_block();
            let right = inlined(&mut b, &[1, 1], Some(1));
            let rhs = b.close_block(rhs, Some(right), Ty::BOOL, n);
            let and = b.emit(Tag::And, left.0, rhs.0, Ty::BOOL, n);
            let root = b.close_block(root, Some(and), Ty::BOOL, n);
            let body = b.finish(root, &[]).expect("valid function body");
            let Some(out) = collapse(&body) else {
                assert!(call, "a pure constant body collapses");
                continue;
            };
            assert!(!call, "a call is an effect");
            assert_eq!(out.len(), 1);
            let tail = out.data[0][1];
            assert_eq!(out.consts[(tail & !Ref::CONST_BIT) as usize], (Ty::BOOL, 1));
            let errors = hd_tir::ir::verify(&out);
            assert!(errors.is_empty(), "collapsed TIR: {errors:?}");
        }
    }

    #[test]
    fn constant_copies_keep_parameters_without_dead_metadata() -> Result<(), String> {
        for width in [1000, 10000] {
            let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
            let root = b.open_block();
            // A dead local before the parameter also checks its dense remap.
            b.local(Ty::BOOL, Symbol::from_raw(0), 0, NodeIdx::NONE);
            b.local(
                Ty::I32,
                Symbol::from_raw(1),
                local_flags::PARAM,
                NodeIdx::NONE,
            );
            for _ in 0..width {
                b.local(Ty::BOOL, Symbol::from_raw(0), 0, NodeIdx::NONE);
                b.words_record(&[1, 2, 3]);
                b.const_str("dead literal");
            }
            let value = b.const_value(Ty::BOOL, 1);
            let args = b.refs_record(&[value]);
            let not = b.emit(Tag::Prim, PrimOp::Not as u32, args, Ty::BOOL, NodeIdx::NONE);
            let args = b.refs_record(&[not]);
            let value = b.emit(Tag::Prim, PrimOp::Not as u32, args, Ty::BOOL, NodeIdx::NONE);
            let root = b.close_block(root, Some(value), Ty::BOOL, NodeIdx::NONE);
            let body = b
                .finish(root, &[])
                .map_err(|errors| format!("{errors:?}"))?;
            let out = collapse(&body).ok_or("pure boolean collapses")?;
            assert_eq!(out.local_ty, [Ty::I32]);
            assert_eq!(out.local_name, [Symbol::from_raw(1)]);
            assert_eq!(hd_tir::own::params(&out), [0]);
            assert_eq!(out.extra, [0, 0]);
            assert_eq!(out.consts, [(Ty::BOOL, 1)]);
            assert!(out.strings.is_empty());
            assert_eq!(hd_tir::ir::verify(&out), []);
        }
        Ok(())
    }

    #[test]
    fn specialization_keys_separate_values_and_keep_plain_keys() {
        let plain = Hash128(42);
        assert_eq!(key(plain, &[], &[]), plain);
        assert_ne!(
            key(plain, &[], &[(0, Value::Scalar(0))]),
            key(plain, &[], &[(0, Value::Variant(0))])
        );
        assert_ne!(
            key(plain, &[], &[(0, Value::Scalar(0))]),
            key(plain, &[], &[(1, Value::Scalar(0))])
        );
    }
    #[test]
    fn known_call_result_keeps_its_producer_and_stops_after_a_returning_arm() -> Result<(), String>
    {
        use hd_tir::ir::{BodyKind, Callee, Providers, TirBuilder, TirSink};
        use hd_types::TyList;
        let mut b = TirBuilder::new(DefId::from_raw(20), BodyKind::Fn);
        let root = b.open_block();
        let condition = b.call(
            &Callee::Item {
                def: DefId::from_raw(21),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::BOOL,
            NodeIdx::NONE,
        );
        let yes = b.open_block();
        let unit = b.const_value(Ty::VOID, 0);
        b.emit(Tag::Return, unit.0, NONE, Ty::NEVER, NodeIdx::NONE);
        let yes = b.close_block(yes, None, Ty::NEVER, NodeIdx::NONE);
        let arms = b.refs_record(&[yes, Ref(NONE)]);
        b.emit(Tag::If, condition.0, arms, Ty::VOID, NodeIdx::NONE);
        let later = b.call(
            &Callee::Item {
                def: DefId::from_raw(22),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let root = b.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        let body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
        let known = reachable_given(&body, &|i| {
            (i == condition.0 as usize).then_some(Value::Scalar(1))
        });
        assert!(known[condition.0 as usize]);
        assert!(!known[later.0 as usize]);
        assert!(reachable_given(&body, &|_| None)[later.0 as usize]);
        Ok(())
    }
    #[test]
    fn reachability_follows_closure_roots_and_rejects_single_result_analysis() -> Result<(), String>
    {
        let n = NodeIdx::NONE;
        let mut b = TirBuilder::new(DefId::from_raw(30), BodyKind::Fn);
        let root = b.open_block();
        let pool = hd_types::InternPool::default();
        let fn_ty = pool.intern_ty(&hd_types::TyData::Fn {
            params: TyList::EMPTY,
            result: Ty::VOID,
            row: hd_types::RowId::EMPTY,
            suspends: false,
            inputs: hd_types::Inputs::Fixed,
            deps: hd_types::FnDeps::NONE,
        });
        let sub = b.open_sub(&[]);
        let inner = b.open_block();
        let effect = b.call(&item(31), &[], Providers::None, Ty::VOID, n);
        let inner = b.close_block(inner, None, Ty::VOID, n);
        let closure = b.close_sub(sub, inner, fn_ty, n);
        let root = b.close_block(root, Some(closure), fn_ty, n);
        let scope = b.open_scope();
        let root = b.close_scope(scope, root, fn_ty, n);
        let body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
        assert!(reachable_given(&body, &|_| None)[effect.0 as usize]);
        assert_eq!(value_given(&body, &|_| None), None);
        Ok(())
    }

    #[test]
    fn reachability_short_circuits_only_the_selected_boolean_rhs() -> Result<(), String> {
        let n = NodeIdx::NONE;
        for tag in [Tag::And, Tag::Or] {
            for left in [0, 1] {
                let mut b = TirBuilder::new(DefId::from_raw(32), BodyKind::Fn);
                let root = b.open_block();
                let lhs = b.const_value(Ty::BOOL, left);
                let rhs = b.open_block();
                let effect = b.call(&item(33), &[], Providers::None, Ty::BOOL, n);
                let rhs = b.close_block(rhs, Some(effect), Ty::BOOL, n);
                let value = b.emit(tag, lhs.0, rhs.0, Ty::BOOL, n);
                let root = b.close_block(root, Some(value), Ty::BOOL, n);
                let body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
                let live = reachable_given(&body, &|_| None);
                assert_eq!(live[effect.0 as usize], (tag == Tag::And) == (left != 0));
            }
        }
        Ok(())
    }

    #[test]
    fn reachability_preserves_unknown_switch_arms_and_selects_known_ones() -> Result<(), String> {
        let n = NodeIdx::NONE;
        for known in [false, true] {
            let mut b = TirBuilder::new(DefId::from_raw(34), BodyKind::Fn);
            let root = b.open_block();
            let selector = b.call(&item(35), &[], Providers::None, Ty::I32, n);
            let yes = b.open_block();
            let yes_call = b.call(&item(36), &[], Providers::None, Ty::VOID, n);
            let yes = b.close_block(yes, None, Ty::VOID, n);
            let no = b.open_block();
            let no_call = b.call(&item(37), &[], Providers::None, Ty::VOID, n);
            let no = b.close_block(no, None, Ty::VOID, n);
            let arms = b.words_record(&[7, yes.0, no.0]);
            b.emit(Tag::SwitchInt, selector.0, arms, Ty::VOID, n);
            let root = b.close_block(root, None, Ty::VOID, n);
            let body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
            let live = reachable_given(&body, &|i| {
                (known && i == selector.0 as usize).then_some(Value::Scalar(7))
            });
            assert!(live[selector.0 as usize] && live[yes_call.0 as usize]);
            assert_eq!(live[no_call.0 as usize], !known);
        }
        Ok(())
    }
    #[test]
    fn call_only_value_queries_distinguish_constants_known_calls_and_unknown_calls()
    -> Result<(), String> {
        let n = NodeIdx::NONE;
        let mut b = TirBuilder::new(DefId::from_raw(38), BodyKind::Default);
        let root = b.open_block();
        let literal = b.const_value(Ty::BOOL, 1);
        let root = b.close_block(root, Some(literal), Ty::BOOL, n);
        let body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
        assert_eq!(constant(&body), Some(Value::Scalar(1)));
        assert_eq!(value_given(&body, &|_| None), Some(Value::Scalar(1)));

        let mut b = TirBuilder::new(DefId::from_raw(39), BodyKind::Fn);
        let root = b.open_block();
        let called = b.call(&item(40), &[], Providers::None, Ty::BOOL, n);
        let root = b.close_block(root, Some(called), Ty::BOOL, n);
        let body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            value_given(&body, &|_| Some(Value::Scalar(0))),
            Some(Value::Scalar(0))
        );
        assert_eq!(value_given(&body, &|_| None), None);
        assert_eq!(constant(&body), None);
        Ok(())
    }
}
