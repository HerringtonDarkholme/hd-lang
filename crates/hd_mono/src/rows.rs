//! Row parameters: a row-polymorphic body is compiled once, whatever keys its
//! row argument lists, and takes those keys' providers in a context
//! (`req.poly.one-body`, `crates/hd_mono/DESIGN.md` §12.4).

use hd_base::{DefId, HashSet};
use hd_types::{InternPool, Ty, TyData, TyList};

use crate::ProgramEnv;

/// The open row: each row-polymorphic instance uses it for its row
/// parameters, including inherited impl parameters. Its keys
/// arrive in the context rather than specializing the body.
const ROW: u8 = 0xF1;

/// The open row (see [`ROW`]).
#[must_use]
pub fn open_row(pool: &InternPool) -> Ty {
    pool.intern_ty(&TyData::Canon(ROW))
}

/// Whether a row key is the open row, whose keys only a context names.
fn is_open_key(pool: &InternPool, k: Ty) -> bool {
    matches!(pool.get(k), TyData::Canon(n) if n == ROW)
}

/// Whether a row argument lists an open row (see [`row_is_open`]).
#[must_use]
pub fn arg_is_open(pool: &InternPool, t: Ty) -> bool {
    match pool.get(t) {
        TyData::Row(r) => row_is_open(pool, r),
        _ => is_open_key(pool, t),
    }
}

/// Whether an item or its parent declares a row parameter (`$R`): it takes the
/// context of those rows' keys.
#[must_use]
pub fn has_row_params(env: &dyn ProgramEnv, def: DefId) -> bool {
    env.generic_rows(env.generics_owner(def))
        .into_iter()
        .any(|r| r)
}

/// Whether a row lists the open row of a row-polymorphic instance, whose
/// keys only the context names.
#[must_use]
pub fn row_is_open(pool: &InternPool, row: hd_types::RowId) -> bool {
    pool.row_data(row)
        .keys
        .iter()
        .any(|k| is_open_key(pool, *k))
}

/// An instance's arguments with polymorphic row arguments made open.
/// Trait-owner arguments stay concrete because they select an implementation.
#[must_use]
pub fn rowless(pool: &InternPool, env: &dyn ProgramEnv, def: DefId, args: TyList) -> TyList {
    if !has_row_params(env, def) {
        return args;
    }
    let owner = env.generics_owner(def);
    let fixed = env.parent(owner).map_or(0, |(parent, n)| {
        if env.impl_head(parent).is_none() {
            n
        } else {
            0
        }
    });
    let open = open_row(pool);
    let mut all = pool.list_items(args).to_vec();
    for (j, row) in env
        .generic_rows(env.generics_owner(def))
        .into_iter()
        .enumerate()
    {
        if j >= fixed
            && row
            && let Some(a) = all.get_mut(j)
        {
            *a = open;
        }
    }
    pool.list(&all)
}

/// Whether any own or inherited row argument lists an open row: a call
/// at it passes the caller's open context on.
#[must_use]
pub fn row_args_open(pool: &InternPool, env: &dyn ProgramEnv, def: DefId, args: TyList) -> bool {
    let (rows, _) = split_rows(pool, env, def, args);
    rows.into_iter().any(|r| arg_is_open(pool, r))
}

/// The arguments of an item's own and inherited row parameters, and
/// `args` with each of them the empty row.
fn split_rows(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    def: DefId,
    args: TyList,
) -> (Vec<Ty>, TyList) {
    let empty = pool.intern_ty(&TyData::Row(hd_types::RowId::EMPTY));
    let mut rows = Vec::new();
    let mut emptied = pool.list_items(args).to_vec();
    for (j, row) in env
        .generic_rows(env.generics_owner(def))
        .into_iter()
        .enumerate()
    {
        if row && let Some(a) = emptied.get_mut(j) {
            rows.push(*a);
            *a = empty;
        }
    }
    (rows, pool.list(&emptied))
}

/// The keys of an instance's row that come from the arguments of the
/// item's own and inherited row parameters, in key order: their providers
/// arrive in one context, not one parameter per key. A key the row also
/// lists itself is one of them too.
#[must_use]
pub fn row_param_keys(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    def: DefId,
    args: TyList,
) -> Vec<Ty> {
    let (rows, _) = split_rows(pool, env, def, args);
    if rows.is_empty() {
        return Vec::new();
    }
    let mut from = HashSet::default();
    for r in rows {
        if let TyData::Row(r) = pool.get(r) {
            from.extend(pool.row_data(r).keys);
        }
    }
    env.row_keys(def, args)
        .into_iter()
        .filter(|k| from.contains(k))
        .collect()
}

/// The keys an instance's row lists itself, with own and inherited row
/// arguments left out, in key order: the providers it takes one per key.
#[must_use]
pub fn declared_row_keys(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    def: DefId,
    args: TyList,
) -> Vec<Ty> {
    if !has_row_params(env, def) {
        return env.row_keys(def, args);
    }
    let (_, emptied) = split_rows(pool, env, def, args);
    env.row_keys(def, emptied)
}

/// The methods of trait `trait_` that have a dispatch table slot, in slot
/// order: those with no type or row parameter of their own
/// (`trait.dyn.type-param.no-slot`).
#[must_use]
pub fn slot_methods(env: &dyn ProgramEnv, trait_: DefId) -> Vec<DefId> {
    let mut methods = env.trait_methods(trait_);
    methods.retain(|m| env.bounded(*m).is_none_or(|b| b.is_empty()));
    methods
}
