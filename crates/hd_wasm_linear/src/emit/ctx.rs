//! Requirement rows and context values (`crates/hd_wasm_linear/DESIGN.md`
//! §23.26, `crates/hd_mono/DESIGN.md` §12.4). A provider is a trait value,
//! `(data, vtable)`. A function with row parameters takes one provider per
//! key its row lists itself, then one context of its row parameters' keys;
//! a function value's code takes one context, whatever its row, so row
//! subsumption is no instruction. A context is a counted list buffer of
//! entries (`rt::ctx_elem`), each a key id and a provider holding a count
//! of its data; 0 is the empty context. A key is looked up by its id and
//! the first entry wins, so a context built in front of an open one shadows
//! it. A `$.Context` value is one such buffer, its entries in key order.

use hd_base::StageResult;
use hd_mono::layout::{CanonMemo, canon};
use hd_types::{Ty, TyData};
use wasm_encoder::Instruction as I;

use super::Em;
use crate::layout::strip;
use crate::module::{Func, HEAP, Val, mem};
use crate::rt::{CTX_DATA, CTX_ENTRY, CTX_VTABLE, Helper};
use crate::unsupported;

/// The id a context gives key `k`: the low bits of its content hash, so
/// `Repo[User]` and `Repo[Post]` have two ids, as on Wasm GC
/// (the canonical requirement-key hash).
pub(crate) fn key_id(input: &crate::Input<'_>, k: Ty) -> i64 {
    let env = input.env;
    let ph = |d: hd_base::DefId| env.path_hash(d);
    let bytes = canon(input.pool, &ph, &mut CanonMemo::default(), k)
        .0
        .to_le_bytes();
    i64::from_le_bytes(bytes[..8].try_into().unwrap_or_default())
}

/// Whether trait method `m`'s vtable slot takes a context: whether its row
/// lists a key, or it has row parameters. A dynamic call cannot know the
/// implementation's row, which may list fewer keys, so the slot looks
/// them up by id.
pub(crate) fn slot_ctx(env: &dyn hd_mono::ProgramEnv, m: hd_base::DefId) -> bool {
    hd_mono::has_row_params(env, m) || !env.row_keys(m, hd_types::TyList::EMPTY).is_empty()
}

/// Reads the provider of key id `id` from the context in local `c` into
/// two fresh locals of `f`, its data and its vtable. The provider is
/// borrowed from the context.
pub(crate) fn ctx_lookup(f: &mut Func, c: u32, id: i64) -> [u32; 2] {
    let (at, d, v) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    f.get(c);
    f.op(I::I64Const(id));
    f.helper(Helper::CtxFind);
    f.set(at);
    f.get(at);
    f.op(I::I32Load(mem(HEAP, CTX_DATA, 2)));
    f.set(d);
    f.get(at);
    f.op(I::I32Load(mem(HEAP, CTX_VTABLE, 2)));
    f.set(v);
    [d, v]
}

impl Em<'_, '_, '_> {
    /// The locals of the innermost provider of the whole key `k`: one in
    /// scope, else one the open context holds. Checking covers every key,
    /// so a miss is internal.
    pub(super) fn provider(&mut self, k: Ty) -> StageResult<[u32; 2]> {
        if let Some((_, ls)) = self.providers.iter().rev().find(|p| p.0 == k) {
            return Ok(*ls);
        }
        match self.open_ctx {
            Some(c) => {
                let id = key_id(self.input, k);
                Ok(ctx_lookup(&mut self.f, c, id))
            }
            None => unsupported(format!(
                "a provider of `{}` this function does not take",
                self.pool().display(k)
            )),
        }
    }

    /// The keys of function type `t`'s row in key order, but its open
    /// row, and whether it lists the open row, whose keys only the
    /// context names.
    pub(super) fn fn_row(&self, t: Ty) -> StageResult<(Vec<Ty>, bool)> {
        let TyData::Fn { row, .. } = self.pool().get(strip(self.pool(), self.env(), t)) else {
            return unsupported("a call of a value that is not a function");
        };
        let data = self.pool().row_data(row);
        if !data.params.is_empty() {
            return unsupported("a function value with an unsolved row");
        }
        let open = hd_mono::row_is_open(self.pool(), row);
        let keys: Vec<Ty> = data
            .keys
            .into_iter()
            .map(|k| self.sub(k))
            .filter(|k| !matches!(self.pool().get(*k), TyData::Canon(_)))
            .collect();
        let env = self.env();
        Ok((
            hd_mono::key_order(self.pool(), &|d| env.path_hash(d), keys),
            open,
        ))
    }

    /// The keys of a context type's row, whole and in key order: the
    /// order of a `$.Context` value's entries.
    fn context_keys(&self, t: Ty) -> StageResult<Vec<Ty>> {
        let TyData::Context(row) = self.pool().get(strip(self.pool(), self.env(), t)) else {
            return unsupported("a context entry of a type that is not a context");
        };
        let env = self.env();
        Ok(hd_mono::key_order(
            self.pool(),
            &|d| env.path_hash(d),
            self.pool().row_data(row).keys,
        ))
    }

    /// Adds the keys of the `$.with` scopes open in a body with an open
    /// context: one may be a key of the open row too, and the nearer
    /// provider wins (`req.with.nearest.row-parameter`).
    fn scope_keys(&self, keys: &mut Vec<Ty>) {
        if self.open_ctx.is_none() {
            return;
        }
        for at in (self.open_base..self.providers.len()).rev() {
            let k = self.providers[at].0;
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
    }

    /// Pushes a context of `keys`, each with its innermost provider, in
    /// front of the open context when `open`: the open context itself
    /// when no key goes in front, 0 when there is neither. Returns the
    /// local of a context it built, which the caller releases once the
    /// call has returned; a callee that keeps it takes a count of its own.
    pub(super) fn push_ctx_of(&mut self, keys: &[Ty], open: bool) -> StageResult<Option<u32>> {
        let base = if open { self.open_ctx } else { None };
        if keys.is_empty() {
            match base {
                Some(c) => self.f.get(c),
                None => self.f.i32(0),
            }
            return Ok(None);
        }
        if self.input.runtime.is_none() {
            return unsupported("a context without std's allocator");
        }
        let mut provs = Vec::new();
        for k in keys {
            provs.push(self.provider(*k)?);
        }
        let c = self.f.local(Val::I32);
        self.f.u32(u32::try_from(keys.len()).expect("keys"));
        match base {
            Some(b) => self.f.get(b),
            None => self.f.i32(0),
        }
        self.f.helper(Helper::CtxNew);
        self.f.set(c);
        for (j, (k, [d, v])) in keys.iter().zip(provs).enumerate() {
            let id = key_id(self.input, *k);
            self.fill_entry(c, j, id, d, v);
        }
        self.f.get(c);
        Ok(Some(c))
    }

    /// Writes entry `j` of the new context in local `c`: key id `id` and
    /// the provider in locals `d` and `v`, whose data takes a count for it.
    fn fill_entry(&mut self, c: u32, j: usize, id: i64, d: u32, v: u32) {
        let off = u64::from(CTX_ENTRY) * u64::try_from(j).expect("entries");
        self.f.get(c);
        self.f.op(I::I64Const(id));
        self.f.op(I::I64Store(mem(HEAP, CTX_DATA - 8 + off, 3)));
        self.f.get(c);
        self.f.get(d);
        self.f.op(I::I32Store(mem(HEAP, CTX_DATA + off, 2)));
        self.f.get(c);
        self.f.get(v);
        self.f.op(I::I32Store(mem(HEAP, CTX_VTABLE + off, 2)));
        if let Some(r) = self.input.runtime {
            self.f.get(d);
            self.f.call(crate::module::Sym::Inst(r.retain));
        }
    }

    /// One count less on a context that [`Em::push_ctx_of`] built.
    pub(super) fn release_ctx(&mut self, c: Option<u32>) {
        if let Some(c) = c {
            self.f.get(c);
            self.f.helper(Helper::ReleaseRef);
        }
    }

    /// Pushes the context a call of a function value of type `ft` passes:
    /// the providers of its row's keys, in front of the open context when
    /// the row lists the open row (§12.4, one callable ABI).
    pub(super) fn push_fn_ctx(&mut self, ft: Ty) -> StageResult<Option<u32>> {
        let (mut keys, open) = self.fn_row(ft)?;
        if open {
            self.scope_keys(&mut keys);
        }
        self.push_ctx_of(&keys, open)
    }

    /// Pushes the providers a call of `item` at `args` passes: one per key
    /// its row lists itself, in key order, then, when it has row
    /// parameters, one context of their arguments' keys, in front of the
    /// open context when an argument lists the open row, so forwarding
    /// `R` costs nothing. Returns a context to release after the call.
    pub(super) fn push_call_providers(
        &mut self,
        item: hd_base::DefId,
        args: hd_types::TyList,
    ) -> StageResult<Option<u32>> {
        let (pool, env) = (self.input.pool, self.input.env);
        for key in hd_mono::declared_row_keys(pool, env, item, args) {
            let [d, v] = self.provider(key)?;
            self.f.get(d);
            self.f.get(v);
        }
        if !hd_mono::has_row_params(env, item) {
            return Ok(None);
        }
        let mut keys = hd_mono::row_param_keys(pool, env, item, args);
        let open = hd_mono::row_args_open(pool, env, item, args);
        if open {
            self.scope_keys(&mut keys);
        }
        self.push_ctx_of(&keys, open)
    }

    /// Pushes the context a dynamic call of trait method `method` at
    /// `targs` passes its slot ([`slot_ctx`]): every key of the method's
    /// row, in front of the open context when the row lists the open row.
    pub(super) fn push_dyn_ctx(
        &mut self,
        method: hd_base::DefId,
        targs: hd_types::TyList,
    ) -> StageResult<Option<u32>> {
        // A trait method call's arguments leave `Self` out: a trait
        // value's is unknown, and no key names it.
        // Its arguments as this instance has them: a thunk's row argument
        // is its caller's row, whose keys the row lists (`erased.rs`).
        let mut all = vec![hd_mono::class_ref(self.pool())];
        all.extend(self.pool().list_items(targs).iter().map(|t| self.sub(*t)));
        let all = self.pool().list(&all);
        let mut keys = Vec::new();
        let mut open = false;
        for k in self.env().row_keys(method, all) {
            let k = self.sub(k);
            if matches!(self.pool().get(k), TyData::Canon(_)) {
                open = true;
            } else {
                keys.push(k);
            }
        }
        if open {
            self.scope_keys(&mut keys);
        }
        self.push_ctx_of(&keys, open)
    }

    /// `$.context(...)` (`req.context.order`): a new context with one
    /// entry per key of its type's row in key order, the provider of the
    /// last pair that binds it: a trait value of the key, or the entry of a
    /// spread context.
    pub(super) fn context_new(&mut self, i: u32, rec: u32) -> StageResult<()> {
        if self.input.runtime.is_none() {
            return unsupported("a context without std's allocator");
        }
        let keys = self.context_keys(self.ty(i))?;
        let pairs = self.rec(rec);
        let mut from = Vec::new();
        for k in &keys {
            let mut src = None;
            for c in pairs.chunks(2).rev() {
                let kt = self.sub(Ty(c[0]));
                if kt == *k {
                    src = Some((c[1], None));
                    break;
                }
                if matches!(self.pool().get(kt), TyData::Context(_))
                    && let Some(at) = self.context_keys(kt)?.iter().position(|x| x == k)
                {
                    src = Some((c[1], Some(at)));
                    break;
                }
            }
            let Some(src) = src else {
                return unsupported("a context key that no entry binds");
            };
            from.push(src);
        }
        let mut provs = Vec::new();
        for (v, at) in from {
            let ls = [self.f.local(Val::I32), self.f.local(Val::I32)];
            match at {
                None => {
                    for (k, l) in ls.iter().enumerate() {
                        self.comp(v, k)?;
                        self.f.set(*l);
                    }
                }
                Some(at) => self.context_entry(v, at, ls)?,
            }
            provs.push(ls);
        }
        let c = self.f.local(Val::I32);
        self.f.u32(u32::try_from(keys.len()).expect("keys"));
        self.f.i32(0);
        self.f.helper(Helper::CtxNew);
        self.f.set(c);
        for (j, (k, [d, v])) in keys.iter().zip(provs).enumerate() {
            let id = key_id(self.input, *k);
            self.fill_entry(c, j, id, d, v);
        }
        self.f.get(c);
        self.store_owned(i)
    }

    /// Reads entry `at` of `$.Context` value `v` into locals `ls`.
    fn context_entry(&mut self, v: u32, at: usize, ls: [u32; 2]) -> StageResult<()> {
        let off = u64::from(CTX_ENTRY) * u64::try_from(at).expect("entries");
        for (l, o) in ls.iter().zip([CTX_DATA, CTX_VTABLE]) {
            self.comp(v, 0)?;
            self.f.op(I::I32Load(mem(HEAP, o + off, 2)));
            self.f.set(*l);
        }
        Ok(())
    }

    /// The providers of a spread `$.Context` value `v` of type `ct`, key
    /// by key, as providers in scope (`req.context.spread`), borrowed from
    /// the value. Returns how many it added.
    pub(super) fn push_context(&mut self, v: u32, ct: Ty) -> StageResult<usize> {
        let keys = self.context_keys(ct)?;
        for (at, k) in keys.iter().enumerate() {
            let ls = [self.f.local(Val::I32), self.f.local(Val::I32)];
            self.context_entry(v, at, ls)?;
            self.providers.push((*k, ls));
        }
        Ok(keys.len())
    }
}
