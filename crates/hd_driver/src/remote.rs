//! Collection's inputs to the generated parts of remote traits
//! (`hd_structure::remote`, `module.remote.*`): the build ID, interface
//! numbers, the std `Stub`'s methods, and the remote traits a root serves.

use std::sync::Arc;

use hd_base::{DefId, HashSet, SliceSort as _, StageResult};
use hd_resolve::{ImplKind, ItemData};
use hd_structure::remote::Served;
use hd_types::sealed::Declarations;
use hd_types::{ParamRef, Ty, TyData, TyList};

use super::{Env, SuppliedFamily, lock};

impl Env<'_> {
    /// Whether `trait_` extends `Remote`, directly or through another
    /// trait (`module.remote.trait`). Cost: its supertraits once.
    pub(super) fn is_remote_trait(&self, trait_: DefId) -> bool {
        let remote = self.sealed.remote;
        let mut seen = HashSet::default();
        let mut stack = vec![trait_];
        while let Some(t) = stack.pop() {
            if t == remote || !seen.insert(t) {
                continue;
            }
            for &s in Declarations::supertraits(self, t) {
                if let TyData::TraitValue { def, .. } = self.run.pool.get(s) {
                    if def == remote {
                        return true;
                    }
                    stack.push(def);
                }
            }
        }
        false
    }

    /// Whether `self_ty` is std's `Stub` and `trait_` a remote trait: the
    /// stub answers every remote trait, each method generated
    /// (`module.remote.stub`).
    pub(super) fn stub_answers(&self, trait_: DefId, self_ty: Ty) -> bool {
        matches!(self.run.pool.get(self_ty), TyData::Adt { def, .. } if def == self.run.known.stub)
            && self.is_remote_trait(trait_)
    }

    /// The build ID every frame carries (`module.remote.handle-call.build`):
    /// the toolchain key and the path and source hash of each library and
    /// executable module, which every executable of a build holds alike.
    /// Cost: one pass over the program's modules, once per build.
    pub(super) fn build_id(&self) -> u64 {
        *self.p.build_id.get_or_init(|| {
            let run = self.run;
            let mut modules: Vec<_> = (0..run.table.modules.len())
                .filter(|&m| {
                    matches!(
                        run.table.modules[m].role,
                        hd_project::Role::Lib | hd_project::Role::Exe
                    )
                })
                .map(|m| (&run.table.modules[m].path, run.skim_of(m).file.source_hash))
                .collect();
            modules.sort_stable_by(|a, b| a.0.cmp(b.0));
            let mut h = hd_base::StableHasher::new("remote-build-id");
            h.hash(run.toolchain);
            for (path, source) in modules {
                h.bytes(path.as_bytes());
                h.hash(source);
            }
            low_bits(h.finish())
        })
    }

    /// The interface number of the remote trait `def[args]`
    /// (`module.remote.interface`): the digest of its path and type
    /// arguments, so every target of a build numbers it alike.
    pub(super) fn interface(&self, def: DefId, args: TyList) -> u64 {
        let tv = self.run.pool.intern_ty(&TyData::TraitValue {
            def,
            args,
            bindings: vec![],
        });
        self.type_number(tv)
    }

    /// The low bits of `ty`'s canonical digest, the same in every target
    /// of a build: an interface's number, and a lifted crossing literal's
    /// part of its number (`fn.crossing.lift`).
    pub(super) fn type_number(&self, tv: Ty) -> u64 {
        let pool = &self.run.pool;
        let names = self.run.names();
        let digest = hd_mono::layout::canon(
            pool,
            &|d| names.path_hash(d),
            &mut hd_mono::layout::CanonMemo::default(),
            tv,
        );
        low_bits(digest)
    }

    /// The remote traits the root `s` implements through a written impl,
    /// at its arguments (`module.remote.handle-call.interface`); for the
    /// trait value of a returned object (`module.remote.r3.returned`), its
    /// trait and each remote supertrait. Cost: one head match per impl of a
    /// remote trait, once per root type; a trait value's supertraits once.
    pub(super) fn served(&self, s: Ty) -> Vec<Served> {
        let pool = &self.run.pool;
        let mut out = Vec::new();
        if let TyData::TraitValue { def, args, .. } = pool.get(s) {
            let mut stack = vec![(def, args)];
            while let Some((trait_, args)) = stack.pop() {
                if !self.is_remote_trait(trait_) || out.iter().any(|o: &Served| o.trait_ == trait_)
                {
                    continue;
                }
                let targs = pool.list_items(args).to_vec();
                let at = |p: ParamRef| {
                    (p.owner == trait_).then(|| match p.index {
                        0 => s,
                        i => targs[usize::from(i) - 1],
                    })
                };
                for &sup in Declarations::supertraits(self, trait_) {
                    if let TyData::TraitValue { def, args, .. } = pool.get(pool.subst(sup, &at)) {
                        stack.push((def, args));
                    }
                }
                let interface = self.interface(trait_, args);
                out.push(Served {
                    trait_,
                    args,
                    impl_: None,
                    interface,
                });
            }
            return out;
        }
        // gap: #2288 — a delegated impl (`by E`) of a remote trait is not
        // served yet; its frames are `WrongHome`.
        for item in self
            .p
            .items
            .impls(|kind, inherent| kind == ImplKind::Written && !inherent)
        {
            let ItemData::Impl {
                trait_,
                trait_args,
                self_ty,
                ..
            } = &item.data
            else {
                continue;
            };
            if !self.is_remote_trait(*trait_) {
                continue;
            }
            let mut binds = Vec::new();
            hd_types::solver::bind_head(pool.types(), item.def, &[*self_ty], &[s], &mut binds);
            let at = |p: ParamRef| {
                (p.owner == item.def)
                    .then(|| binds.get(usize::from(p.index)).copied().flatten())
                    .flatten()
            };
            if pool.subst(*self_ty, &at) != s {
                continue;
            }
            let args: Vec<Ty> = (pool.list_items(*trait_args).iter())
                .map(|&t| pool.subst(t, &at))
                .collect();
            let args = pool.list(&args);
            out.push(Served {
                trait_: *trait_,
                args,
                impl_: Some(item.def),
                interface: self.interface(*trait_, args),
            });
        }
        out
    }

    /// Parameter `k` of `sig`, of type `ty`, as its body holds it: a `Wire`
    /// crossing parameter is a `std.remote.Crossing` (`fn.crossing.lift`).
    pub(super) fn crossing_param(&self, sig: &hd_resolve::FnSig, k: usize, ty: Ty) -> Ty {
        let wire = self.run.known.wire;
        if !(sig.crossing.iter()).any(|c| usize::from(c.param) == k && c.marker == wire) {
            return ty;
        }
        let pool = &self.run.pool;
        pool.intern_ty(&TyData::Adt {
            def: self
                .run
                .names()
                .item(hd_resolve::known::REMOTE_MODULE, "Crossing"),
            args: pool.list(&[ty]),
        })
    }

    /// The family of one generated method of the std `Stub` at a remote
    /// trait (`hd_structure::remote::stub_method_body`), hashed as a
    /// supplied family is.
    pub(super) fn stub_family(
        &self,
        cx: &hd_structure::Cx<'_, '_>,
        method: DefId,
        self_ty: Ty,
    ) -> StageResult<Option<Arc<SuppliedFamily>>> {
        let names = self.run.names();
        let trait_ = DefId::from_raw(
            names
                .paths
                .parent(hd_base::PathId::from_raw(method.raw()))
                .raw(),
        );
        if !self.stub_answers(trait_, self_ty) {
            return Ok(None);
        }
        let cell = lock(&self.p.supplied_families)
            .entry((method, self_ty))
            .or_default()
            .clone();
        cell.get_or_init(|| {
            let body = hd_structure::remote::stub_method_body(cx, trait_, method, self_ty)?;
            let bytes =
                hd_tir::wire::write_body(&body, &self.run.pool, &self.run.paths, &self.run.syms)?;
            let inlineable = hd_opt::inline::bounded_inline_metadata(&body).is_some();
            Ok(Some(Arc::new(SuppliedFamily {
                hash: hd_tir::wire::tir_hash(&bytes),
                index: std::iter::once((method, 0)).collect(),
                bodies: vec![body],
                inlineable,
                factories_read_only: false,
            })))
        })
        .clone()
    }
}

/// The low 64 bits of a digest, a wire number's width.
fn low_bits(h: hd_base::Hash128) -> u64 {
    u64::try_from(h.0 & u128::from(u64::MAX)).unwrap_or(0)
}
