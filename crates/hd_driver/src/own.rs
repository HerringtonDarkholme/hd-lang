//! The ownership passes' place in the driver (`docs/compiler/tir-v2.md`,
//! "Where and when the passes run", "What the passes read"): with
//! [`Packages::own_passes`](crate::Packages::own_passes) on, every body
//! the program reaches goes through `hd_tir::own` once, after decoding
//! (`Run::body_of`) or after collection generates it (a supplied or a
//! `dbg` body), and checked and debug builds verify the result
//! (`hd_tir::ir::verify_owned`). [`Env`] answers the passes' questions
//! from signatures and layouts, never from another body, and remembers
//! each answer for the run.

use std::cell::RefCell;

use hd_base::{DefId, Hash128, NotImplemented, StableHasher, Stage, StageResult};
use hd_mono::ProgramEnv;
use hd_mono::layout::LayoutEnv;
use hd_tir::ir::{Body, Callee, ChoiceKind};
use hd_tir::own::{OwnCx, OwnScratch, Pass, Tri};
use hd_types::{Ty, TyData};

use super::{Env, ProgramTables, Run, lock};

thread_local! {
    /// The passes' tables, reused across every body a thread runs them on.
    static SCRATCH: RefCell<OwnScratch> = RefCell::new(OwnScratch::default());
}

impl OwnCx for Env<'_> {
    fn counted(&self, ty: Ty) -> Tri {
        let types = self.run.pool.types();
        if types.has_param(ty) || types.has_assoc(ty) {
            return Tri::Maybe;
        }
        if self.layout_facts(ty).0 {
            Tri::Yes
        } else {
            Tri::No
        }
    }

    fn pass(&self, callee: &Callee, k: usize, n: usize) -> Pass {
        // The signature's parameters, the receiver first.
        let def = match callee {
            Callee::Item { def, .. } => *def,
            Callee::TraitMethod { method, .. } => *method,
        };
        let key = (
            def,
            u32::try_from(k).expect("arguments"),
            u32::try_from(n).expect("arguments"),
        );
        if let Some(&p) = lock(&self.run.own_params).get(&key) {
            return p;
        }
        let p = self.declared_pass(def, k, n);
        lock(&self.run.own_params).insert(key, p);
        p
    }

    fn pass_value(&self, fn_ty: Ty, k: usize, n: usize) -> Pass {
        let pool = &self.run.pool;
        let TyData::Fn {
            params, suspends, ..
        } = pool.get(hd_wasm_linear::strip_type(pool, self, fn_ty))
        else {
            return Pass::Lent;
        };
        let params = pool.list_items(params);
        if suspends || params.len() != n {
            return Pass::Lent;
        }
        self.value_parameter(params[k])
    }

    fn may_write(&self, callee: &Callee) -> bool {
        // The compiler's scalar square root reaches nothing but its
        // argument (`list.rs`'s loop proofs allow it the same way).
        match callee {
            Callee::Item { def, .. } => self.intrinsic(*def).as_deref() != Some("sqrt"),
            Callee::TraitMethod {
                method,
                self_ty,
                choice: (ChoiceKind::Impl | ChoiceKind::Builtin, _),
                ..
            } => {
                let float = matches!(self.run.pool.get(*self_ty), TyData::Prim(p) if p.is_float());
                !(float && self.run.names().path(*method).rsplit(['.', '/']).next() == Some("sqrt"))
            }
            Callee::TraitMethod { .. } => true,
        }
    }

    fn received(&self, item: DefId, k: usize, n: usize) -> Pass {
        let Some(sig) = self.sig(item) else {
            return Pass::Owned;
        };
        if sig.suspends || sig.params.len() != n {
            return Pass::Owned;
        }
        self.value_parameter(self.low(sig.params[k].1))
    }

    fn parts(&self, ty: Ty, variant: Option<u32>, out: &mut Vec<Ty>) -> bool {
        let types = self.run.pool.types();
        if types.has_param(ty) || types.has_assoc(ty) {
            out.clear();
            return false;
        }
        hd_wasm_linear::in_place_parts(&self.run.pool, self, ty, variant, out)
    }
}

impl Env<'_> {
    /// [`OwnCx::pass`] of parameter `k` of `def` called with `n`
    /// arguments, from its declaration. A body without a signature (a
    /// module's init) is never called.
    fn declared_pass(&self, def: DefId, k: usize, n: usize) -> Pass {
        let Some(sig) = self.sig(def) else {
            return Pass::Lent;
        };
        // A suspending callee's cold constructor counts what its frame
        // keeps; its callers lend every argument. So does a call of an
        // `@intrinsic` function, which the backend lowers in place and
        // which counts whatever it keeps itself (`Em::intrinsic`).
        if sig.suspends || sig.params.len() != n || self.intrinsic(def).is_some() {
            return Pass::Lent;
        }
        // The declared type decides, unsubstituted: substituting would read
        // the callee's generics, which a default body's owner keeps in its
        // body, and the passes read no other body.
        self.value_parameter(self.low(sig.params[k].1))
    }

    /// How a parameter of declared type `t` takes its argument: owned when
    /// it is a value in every instance, lent when it is a reference or may
    /// be one (a type parameter, an associated type, a newtype over a type
    /// parameter), or holds nothing counted. The backend counts a lent
    /// argument that an instance owns, and emits nothing for an owned one
    /// that holds nothing counted there.
    fn value_parameter(&self, t: Ty) -> Pass {
        let pool = &self.run.pool;
        let mut t = t;
        while let TyData::Mut(inner) = pool.get(t) {
            t = inner;
        }
        let types = pool.types();
        let open = types.has_param(t) || types.has_assoc(t);
        match pool.get(t) {
            TyData::Param(_) | TyData::Assoc { .. } | TyData::Ref { .. } => Pass::Lent,
            TyData::Adt { def, .. }
                if def == self.run.known.ref_cell || (open && self.newtype_base(def).is_some()) =>
            {
                Pass::Lent
            }
            _ if open => Pass::Owned,
            _ => match self.layout_facts(t) {
                (true, false) => Pass::Owned,
                _ => Pass::Lent,
            },
        }
    }

    /// Whether a value of type `t`, which names no parameter, holds a
    /// counted leaf, and whether it is a reference: the backend's layout
    /// facts, once per run (a layout walk allocates as it goes).
    fn layout_facts(&self, t: Ty) -> (bool, bool) {
        if let Some(&facts) = lock(&self.run.own_types).get(&t) {
            return facts;
        }
        let pool = &self.run.pool;
        let facts = (
            hd_wasm_linear::counted_type(pool, self, t),
            hd_wasm_linear::reference_type(pool, self, t),
        );
        lock(&self.run.own_types).insert(t, facts);
        facts
    }

    /// Runs the ownership passes over `body`, then, in checked and debug
    /// builds, verifies their output.
    pub(super) fn own_passes(&self, mut body: Body) -> StageResult<Body> {
        // A body's passes never run another's; the tables are taken out
        // anyway, so a nested run would only start from empty ones.
        let mut scratch = SCRATCH.with(RefCell::take);
        let refused = hd_tir::own::run(&mut body, self, &mut scratch);
        SCRATCH.with(|s| *s.borrow_mut() = scratch);
        let path = || self.run.names().path(body.item);
        if let Err(what) = refused {
            return Err(NotImplemented::bug(
                Stage::Collect,
                format!("the ownership passes refused `{}`: {what}", path()),
            ));
        }
        if cfg!(debug_assertions) || self.run.checked {
            let errs = hd_tir::ir::verify_owned(&body, self);
            if let Some(e) = errs.first() {
                return Err(NotImplemented::bug(
                    Stage::Collect,
                    format!(
                        "the ownership passes' output for `{}` fails invariant {} at %{}: {} ({} findings)",
                        path(),
                        e.invariant,
                        e.inst,
                        e.what,
                        errs.len()
                    ),
                ));
            }
        }
        Ok(body)
    }

    /// A body collection generates (a supplied method's, a `dbg` body),
    /// after the ownership passes when they are on.
    pub(super) fn owned_generated(&self, body: Body) -> StageResult<Body> {
        if self.run.own_passes {
            self.own_passes(body)
        } else {
            Ok(body)
        }
    }
}

impl Run<'_> {
    /// Body `body`, with TIR hash `hash`, after the ownership passes, and
    /// its hash, which changes with them; `None`, with the error noted for
    /// `Collect`, when the passes refuse it or their output fails its
    /// verification.
    pub(super) fn own_body(
        &self,
        p: &ProgramTables,
        body: Body,
        hash: Hash128,
    ) -> Option<(Body, Hash128)> {
        // The program's impls: a layout normalizes associated types.
        let folders = self.program_folders();
        let arity = p.arity();
        let view = self.program_view(p, &folders, &arity);
        let env = Env {
            run: self,
            p,
            impls: hd_types::solver::Impls::Owned(&view),
            sealed: self.known.sealed(),
        };
        match env.own_passes(body) {
            Ok(body) => {
                let mut k = StableHasher::new("own-passes");
                k.hash(hash);
                Some((body, k.finish()))
            }
            Err(e) => {
                lock(&p.bodies.own_error).get_or_insert_with(|| e.to_string());
                None
            }
        }
    }
}
