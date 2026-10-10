//! Generic methods called through a trait value (codegen.md §13.5.1, the
//! erased method ABI): a vtable slot of a generic method holds the erased
//! body of its implementation, one per `(impl, method)`, whose own type
//! parameters stay open. Every instruction of that body whose types are
//! open is outlined: emission calls it through the witness the caller
//! passes, and each witness field holds, per erased body, one thunk per
//! open instruction at the caller's concrete type arguments. Collection
//! records each `(method, type arguments)` pair a `dyn` call reaches and
//! each erased body in the program, and pushes the thunks of every pair
//! and body of one family, to a fixed point.

use std::collections::BTreeMap;

use hd_base::{DefId, Hash128, InstId, StableHasher, StageResult};
use hd_tir::ir::{Body, Callee, Tag};
use hd_types::{InternPool, Ty, TyData, TyList};

use crate::layout::{CanonMemo, Sub, canon};
use crate::{CallTarget, Cx, Picked, ProgramEnv, TargetKind, err, subst};

/// `open_param(k)` is `Canon(OPEN + k)`, a placeholder that never occurs
/// in a concrete instance otherwise.
const OPEN: u8 = 0xE0;

/// How many own type parameters an erased body may have.
pub const MAX_OPEN: u8 = 16;

/// Open method type parameter `k` of an erased body.
#[must_use]
pub fn open_param(pool: &InternPool, k: usize) -> Ty {
    let k = u8::try_from(k).map_or(MAX_OPEN - 1, |k| k.min(MAX_OPEN - 1));
    pool.intern_ty(&TyData::Canon(OPEN + k))
}

/// Whether `t` mentions an open parameter (a projection of one
/// included): a value of it is the caller's representation, viewed as
/// `eqref` (§13.5.1, "Open types and open values").
#[must_use]
pub fn is_open(pool: &InternPool, t: Ty) -> bool {
    if !pool.types().has_canon(t) {
        return false;
    }
    let list = |l: TyList| pool.list_items(l).iter().any(|x| is_open(pool, *x));
    match pool.get(t) {
        TyData::Canon(n) => (OPEN..OPEN + MAX_OPEN).contains(&n),
        TyData::Adt { args, .. } => list(args),
        TyData::Tuple { elems, rest } => list(elems) || rest.is_some_and(|r| is_open(pool, r)),
        TyData::Option(i) | TyData::Mut(i) => is_open(pool, i),
        TyData::Fn { params, result, .. } => list(params) || is_open(pool, result),
        TyData::TraitValue { args, bindings, .. } => {
            list(args) || bindings.iter().any(|(_, b)| is_open(pool, *b))
        }
        TyData::Assoc { self_ty, args, .. } => is_open(pool, self_ty) || list(args),
        _ => false,
    }
}

/// How many type parameters an item declares itself.
#[must_use]
pub fn own_count(env: &dyn ProgramEnv, def: DefId) -> usize {
    env.bounded(def).map_or(0, |b| b.len())
}

/// The erased view of an instance's arguments: the owner's as given, the
/// item's own replaced by open parameters.
#[must_use]
pub fn erased_args(pool: &InternPool, env: &dyn ProgramEnv, item: DefId, args: TyList) -> TyList {
    let n = env.parent(env.generics_owner(item)).map_or(0, |p| p.1);
    let all: Vec<Ty> = pool
        .list_items(args)
        .iter()
        .enumerate()
        .map(|(k, t)| if k < n { *t } else { open_param(pool, k - n) })
        .collect();
    pool.list(&all)
}

/// The words of instruction `i` that its operand schema marks as values.
fn schema_values(b: &Body, i: usize) -> Vec<u32> {
    let [a, w] = b.data[i];
    let (oa, ow) = b.tags[i].operands();
    let mut out = Vec::new();
    for (word, op) in [(a, oa), (w, ow)] {
        match op {
            hd_tir::ir::Op::Value => out.push(word),
            hd_tir::ir::Op::Values => {
                let r = b.record(word);
                let n = if matches!(b.tags[i], Tag::Call | Tag::Await) {
                    r.len().saturating_sub(3)
                } else {
                    r.len()
                };
                out.extend_from_slice(&r[..n]);
            }
            _ => {}
        }
    }
    out
}

/// The type of a value word: an instruction's, or a constant's.
fn word_ty(b: &Body, word: u32) -> Option<Ty> {
    if word == hd_tir::ir::NONE {
        return None;
    }
    Some(crate::value_ty(b, word))
}

/// The open instructions of the erased body of `item` at `erased`
/// (`erased_args`), in TIR order: the number of each is its thunk index
/// (§13.5.1 step 1). Control and local moves stay in the body; a match,
/// loop or branch whose operand is open, and an open instruction that
/// owns blocks or suspends, has no outlined form yet.
pub fn open_insts(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    item: DefId,
    erased: TyList,
) -> StageResult<Vec<u32>> {
    let s = |t: Ty| subst(pool, env, item, erased, t);
    let open_word = |w: u32| word_ty(b, w).is_some_and(|t| is_open(pool, s(t)));
    let open_list = |l: TyList| pool.list_items(l).iter().any(|t| is_open(pool, s(*t)));
    let mut out = Vec::new();
    for i in 0..b.len() {
        let tag = b.tags[i];
        match tag {
            Tag::LocalGet
            | Tag::LocalSet
            | Tag::Block
            | Tag::Scope
            | Tag::Defer
            | Tag::If
            | Tag::Match
            | Tag::Loop
            | Tag::Break
            | Tag::Continue
            | Tag::Return
            | Tag::Unreachable
            | Tag::ToArm
            | Tag::Hole
            | Tag::Poison => continue,
            // A tag switch reads its scrutinee's tag; over an open value,
            // that read is outlined and the switch stays (§13.5.1 step 3).
            Tag::SwitchTag => {
                if open_word(b.data[i][0]) {
                    out.push(u32::try_from(i).expect("insts"));
                }
                continue;
            }
            Tag::SwitchInt
            | Tag::SwitchChar
            | Tag::SwitchStr
            | Tag::Guard
            | Tag::And
            | Tag::Or
            | Tag::ForRange
            | Tag::ForList
            | Tag::ForMap
            | Tag::With => {
                if schema_values(b, i).into_iter().any(open_word) {
                    return err(&format!(
                        "a `{}` over an open value in an erased body",
                        tag.name()
                    ));
                }
                continue;
            }
            _ => {}
        }
        let [a, w] = b.data[i];
        let mut open = is_open(pool, s(b.ty[i])) || schema_values(b, i).into_iter().any(open_word);
        if let Some(ops) = b.value_operands(i) {
            open |= ops.into_iter().any(open_word);
        }
        open |= match tag {
            Tag::Call | Tag::Await => match Callee::from_words(b.record(a)) {
                Some(Callee::Item { targs, .. }) => open_list(targs),
                Some(Callee::TraitMethod { self_ty, targs, .. }) => {
                    is_open(pool, s(self_ty)) || open_list(targs)
                }
                None => false,
            },
            Tag::DefaultCall => b.record(a).get(1).is_some_and(|l| open_list(TyList(*l))),
            Tag::ItemRef => b.record(w).first().is_some_and(|l| open_list(TyList(*l))),
            _ => false,
        };
        if !open {
            continue;
        }
        if tag == Tag::Await || b.value_operands(i).is_none() {
            return err(&format!("an open `{}` in an erased body", tag.name()));
        }
        out.push(u32::try_from(i).expect("insts"));
    }
    // A closure's body would reach the thunks through the body's witness,
    // as a captured local; closures are not erased yet.
    if !out.is_empty() && b.tags.contains(&Tag::Closure) {
        return err("a closure in an erased body with open instructions");
    }
    Ok(out)
}

/// The operands an open instruction passes to its thunk: its value
/// operands that are instructions, in emission order (the thunk emits a
/// constant itself); a tag switch's scrutinee.
#[must_use]
pub fn thunk_operands(b: &Body, i: u32) -> Vec<u32> {
    let words = if b.tags[i as usize] == Tag::SwitchTag {
        vec![b.data[i as usize][0]]
    } else {
        b.value_operands(i as usize).unwrap_or_default()
    };
    words
        .into_iter()
        .filter(|r| *r != hd_tir::ir::NONE && hd_tir::ir::Ref(*r).as_inst().is_some())
        .collect()
}

/// A witness family (§13.5.1, "The witness"): the trait method and its
/// trait's arguments. Every erased body of the family has a field in
/// each of the family's witnesses.
#[must_use]
pub fn witness_family(
    pool: &InternPool,
    path_hash: &dyn Fn(DefId) -> Hash128,
    memo: &mut CanonMemo,
    method: DefId,
    trait_args: &[Ty],
) -> Hash128 {
    let mut h = StableHasher::new("witness-family");
    h.hash(path_hash(method));
    h.u32(u32::try_from(trait_args.len()).expect("args"));
    for t in trait_args {
        h.hash(canon(pool, path_hash, memo, *t));
    }
    h.finish()
}

/// A witness's method type arguments, as its name in its family.
#[must_use]
pub fn witness_args(
    pool: &InternPool,
    path_hash: &dyn Fn(DefId) -> Hash128,
    memo: &mut CanonMemo,
    targs: &[Ty],
) -> Hash128 {
    let mut h = StableHasher::new("witness-args");
    h.u32(u32::try_from(targs.len()).expect("args"));
    for t in targs {
        h.hash(canon(pool, path_hash, memo, *t));
    }
    h.finish()
}

/// One erased body of a family.
#[derive(Clone, Debug)]
pub(crate) struct ErasedBody {
    pub key: Hash128,
    pub item: DefId,
    pub args: TyList,
}

/// What collection gathers for witnesses.
#[derive(Default, Debug)]
pub(crate) struct WitnessCx {
    /// Per family, its erased bodies.
    pub bodies: BTreeMap<Hash128, Vec<ErasedBody>>,
    /// Per erased body's key, its family.
    pub family_of: std::collections::HashMap<Hash128, Hash128>,
    /// Per family, the method type arguments `dyn` calls pass, by name.
    pub pairs: BTreeMap<Hash128, BTreeMap<Hash128, Vec<Ty>>>,
    /// Per `(family, arguments, erased body)`, its thunks in order.
    pub thunks: std::collections::HashMap<(Hash128, Hash128, Hash128), Vec<Hash128>>,
}

/// The program's witnesses (§13.5.1), which link writes as constant
/// globals.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Witnesses {
    /// Per family, its erased bodies' instance keys in field order.
    pub families: Vec<(Hash128, Vec<Hash128>)>,
    /// Per witness: its family, its arguments' name, and per field of the
    /// family its erased body's thunks at those arguments.
    pub witnesses: Vec<(Hash128, Hash128, Vec<Vec<Hash128>>)>,
}

impl WitnessCx {
    pub(crate) fn finish(&self) -> Witnesses {
        let mut out = Witnesses::default();
        // A call can reach a family no vtable of the program fills: its
        // witnesses have no fields.
        let families: std::collections::BTreeSet<&Hash128> =
            self.bodies.keys().chain(self.pairs.keys()).collect();
        for family in families {
            let mut keys: Vec<Hash128> = self
                .bodies
                .get(family)
                .into_iter()
                .flatten()
                .map(|e| e.key)
                .collect();
            keys.sort_unstable();
            for args in self.pairs.get(family).into_iter().flat_map(BTreeMap::keys) {
                let fields = keys
                    .iter()
                    .map(|k| {
                        self.thunks
                            .get(&(*family, *args, *k))
                            .cloned()
                            .unwrap_or_default()
                    })
                    .collect();
                out.witnesses.push((*family, *args, fields));
            }
            out.families.push((*family, keys));
        }
        out
    }
}

impl Cx<'_> {
    /// The slot target of the generic trait method `m` at the concrete
    /// type `from` (§13.5.1): the erased body of the implementation's
    /// method, or of the trait's default, at the impl's arguments.
    pub(crate) fn erased_target(
        &mut self,
        trait_: DefId,
        m: DefId,
        from: Ty,
        targs: &[Ty],
        parent: InstId,
    ) -> StageResult<CallTarget> {
        let (pool, env) = (self.pool, self.env);
        let n = own_count(env, m);
        if n > usize::from(MAX_OPEN) {
            return err("a method with more type parameters than an erased body takes");
        }
        if env.generic_rows(m).into_iter().any(|r| r) {
            return err("a method with a row parameter called through a trait value");
        }
        let opens: Vec<Ty> = (0..n).map(|k| open_param(pool, k)).collect();
        let (item, args) = match self.select(trait_, from, pool.list(targs), None)? {
            Picked::Impl(impl_, mut impl_args) => match env.impl_method(impl_, m) {
                Some(mi) if env.body(mi).is_some() => {
                    impl_args.extend(opens);
                    (mi, impl_args)
                }
                Some(_) => return err("a compiler-supplied generic method in a vtable"),
                None => {
                    let mut all = vec![from];
                    all.extend_from_slice(targs);
                    all.extend(opens);
                    (m, all)
                }
            },
            Picked::Builtin => return err("a compiler-supplied generic method in a vtable"),
        };
        let args = pool.list(&args);
        let fresh = self.out.table.get(item, Sub::Erased, args).is_none();
        let key = self.push(item, Sub::Erased, args, parent)?;
        if fresh {
            let ph = |d: DefId| env.path_hash(d);
            let family = witness_family(pool, &ph, &mut self.canons, m, targs);
            let body = ErasedBody { key, item, args };
            self.witness.family_of.insert(key, family);
            self.witness
                .bodies
                .entry(family)
                .or_default()
                .push(body.clone());
            let pairs: Vec<(Hash128, Vec<Ty>)> = self
                .witness
                .pairs
                .get(&family)
                .map(|p| p.iter().map(|(k, v)| (*k, v.clone())).collect())
                .unwrap_or_default();
            for (name, own) in pairs {
                self.thunks_for(family, name, &own, &body)?;
            }
        }
        Ok(CallTarget {
            key,
            item,
            args,
            ret: subst(pool, env, item, args, env.ret(item).unwrap_or(Ty::VOID)),
            kind: TargetKind::Instance,
        })
    }

    /// Records a `dyn` call of `method` at `targs` (the trait's arguments,
    /// then the method's own): a generic method's call reaches the witness
    /// of its own arguments, whose thunks every erased body of its family
    /// gets.
    pub(crate) fn record_pair(
        &mut self,
        trait_: DefId,
        method: DefId,
        targs: &[Ty],
    ) -> StageResult<()> {
        let (pool, env) = (self.pool, self.env);
        let n = env.trait_arity(trait_);
        if own_count(env, method) == 0 || targs.len() <= n {
            return Ok(());
        }
        let ph = |d: DefId| env.path_hash(d);
        let family = witness_family(pool, &ph, &mut self.canons, method, &targs[..n]);
        let own = targs[n..].to_vec();
        let name = witness_args(pool, &ph, &mut self.canons, &own);
        let pairs = self.witness.pairs.entry(family).or_default();
        if pairs.contains_key(&name) {
            return Ok(());
        }
        pairs.insert(name, own.clone());
        let bodies = self
            .witness
            .bodies
            .get(&family)
            .cloned()
            .unwrap_or_default();
        for body in bodies {
            self.thunks_for(family, name, &own, &body)?;
        }
        Ok(())
    }

    /// The thunks of one erased body at the method type arguments `own`.
    fn thunks_for(
        &mut self,
        family: Hash128,
        name: Hash128,
        own: &[Ty],
        e: &ErasedBody,
    ) -> StageResult<()> {
        let (pool, env) = (self.pool, self.env);
        let Some(body) = env.body(e.item) else {
            return err("an erased body without TIR");
        };
        let opens = open_insts(pool, env, body, e.item, e.args)?;
        let all = pool.list_items(e.args);
        let keep = all.len().saturating_sub(own.len());
        if keep + own.len() != all.len() || own.len() != own_count(env, e.item) {
            return err("a witness whose type arguments do not fit its erased body");
        }
        let mut args = all[..keep].to_vec();
        args.extend_from_slice(own);
        let args = pool.list(&args);
        let parent = self
            .out
            .table
            .get(e.item, Sub::Erased, e.args)
            .unwrap_or(InstId::NONE);
        let mut keys = Vec::new();
        for k in 0..opens.len() {
            let k = u16::try_from(k).expect("open instructions");
            keys.push(self.push(e.item, Sub::Thunk(k), args, parent)?);
        }
        self.witness.thunks.insert((family, name, e.key), keys);
        Ok(())
    }
}
