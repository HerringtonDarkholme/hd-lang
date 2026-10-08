//! The instantiated-template check at a derivation opt-in (annotations,
//! `annot.template.checked`; checking-and-tir.md §4.13.9 "Derive
//! instances"). Each `@derive(X)` and each derivation block
//! `impl X for D by Structure` checks the methods of `X`'s template once
//! more, in `D`'s module, with the template's `T` standing for the
//! target `D` and the opt-in's own parameters and bounds in the
//! environment.
//!
//! The template's own module has already checked those bodies for every
//! `T < Structure`. What only an instance can check is the generated
//! traversal: each `walk`, `describe` or `build` call calls the
//! `member[F]` of its walker, describer or source once per member, with
//! `F` the member's type (`annot.walk.members`, `annot.describe.order`,
//! `annot.build.member`). So every member's type must meet that
//! `member`'s bounds (`annot.walker.obligation`). The walker type is what
//! ordinary checking infers at the call, wherever its value came from: a
//! helper that builds the walker counts as one written in place.
//!
//! A failing member is one `member-not-derivable` at the opt-in, naming
//! the member (`annot.walker.obligation.error`). Every other diagnostic of
//! the instance check is dropped: the template's module reports the
//! template's own mistakes.

use std::cell::RefCell;

use hd_base::{DefId, StageResult};
use hd_diag::DiagBuf;
use hd_resolve::{Item, ItemData, Lookup, Names};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_types::solver::{Answer, Evidence, TraitRef};
use hd_types::{ParamRef, Ty, TyData};

use crate::body::{BodyCx, Ck, check_fn_in};

/// One member a derivation walks, describes or builds.
#[derive(Clone, Debug)]
pub struct OptInMember {
    /// How a message names it: `name`, or `Variant.name` in an enum.
    pub label: String,
    /// Its type, over the opt-in's parameters.
    pub ty: Ty,
    /// Whether it declares a default (`data.default`).
    pub has_default: bool,
}

/// One derivation opt-in: a derived implementation or a derivation block,
/// the template it instantiates, and the members it covers.
#[derive(Debug)]
pub struct OptIn {
    /// The derived implementation or the derivation block.
    pub impl_: DefId,
    pub template: DefId,
    pub trait_: DefId,
    /// The target, over the opt-in's parameters.
    pub target: Ty,
    pub members: Vec<OptInMember>,
    /// Failing members found so far: member index and the bound it misses.
    failed: RefCell<Vec<(usize, Ty)>>,
}

impl OptIn {
    /// The opt-in of `it`, a derived implementation or a derivation block,
    /// through `template`. `omitted` names the members its member lines
    /// omit (`name = pass`), which contribute no obligation
    /// (`annot.bound.omitted`). `None` when the target is not a data type
    /// or an enum.
    #[must_use]
    pub fn new(
        names: &Names<'_>,
        lookup: &Lookup<'_>,
        it: &Item,
        template: DefId,
        omitted: &[String],
    ) -> Option<Self> {
        let ItemData::Impl {
            trait_, self_ty, ..
        } = &it.data
        else {
            return None;
        };
        let pool = names.pool.types();
        let TyData::Adt { def, args } = pool.get(*self_ty) else {
            return None;
        };
        let args = pool.list_items(args);
        let ty = |t: Ty| {
            pool.subst(t, &|p: ParamRef| {
                (p.owner == def).then(|| args.get(usize::from(p.index)).copied())?
            })
        };
        let label = |f: &hd_resolve::Field| {
            let n = names.text(f.name);
            // An unnamed payload parameter is `_0`, `_1`... (`data.derive.payload-names`).
            if n.starts_with(|c: char| c.is_ascii_digit()) {
                format!("_{n}")
            } else {
                n.to_owned()
            }
        };
        let mut members = Vec::new();
        match &lookup.item(def)?.data {
            // A data type's members are its fields, embedded ones included
            // (`data.derive.members`).
            ItemData::Data(fields) => {
                for f in fields {
                    let l = label(f);
                    if omitted.contains(&l) {
                        continue;
                    }
                    members.push(OptInMember {
                        label: l,
                        ty: ty(f.ty),
                        has_default: f.has_default,
                    });
                }
            }
            // An enum's members are each variant's payload parameters;
            // shared constructor data is not a member (`data.derive.shared`).
            ItemData::Enum { variants, .. } => {
                for v in variants {
                    for f in &v.fields {
                        members.push(OptInMember {
                            label: format!("{}.{}", names.text(v.name), label(f)),
                            ty: ty(f.ty),
                            has_default: f.has_default,
                        });
                    }
                }
            }
            _ => return None,
        }
        Some(Self {
            impl_: it.def,
            template,
            trait_: *trait_,
            target: *self_ty,
            members,
            failed: RefCell::new(Vec::new()),
        })
    }
}

/// The members a derivation block's member lines omit: `name = pass`.
#[must_use]
pub fn omitted_members(src: &hd_resolve::Src<'_>, block: NodeRef<'_>) -> Vec<String> {
    let mut out = Vec::new();
    let Some(body) = hd_resolve::Src::child(block, SyntaxKind::Block) else {
        return out;
    };
    for line in body
        .children()
        .filter(|c| c.kind() == SyntaxKind::Derivation)
    {
        let Some(first) = src.tokens(line).next() else {
            continue;
        };
        if src.tkind(first) != Some(TokenKind::Ident) {
            continue;
        }
        let pass = line.children().any(|e| {
            e.kind() == SyntaxKind::LiteralExpr
                && src
                    .tokens(e)
                    .any(|t| src.tkind(t) == Some(TokenKind::KwPass))
        });
        if pass {
            out.push(src.text(first).to_owned());
        }
    }
    out
}

/// Checks the template `methods` (their `DefId` and syntax) as `opt`'s
/// instance and returns one message per failing member, in member order.
/// `cx` sees the template's module. Diagnostics of the bodies themselves
/// are dropped.
pub fn check_opt_in(
    cx: &BodyCx<'_>,
    opt: &OptIn,
    methods: &[(DefId, NodeRef<'_>)],
) -> StageResult<Vec<String>> {
    for &(def, node) in methods {
        let mut scratch = DiagBuf::default();
        check_fn_in(cx, def, node, &mut scratch, Some(opt))?;
    }
    let mut failed = opt.failed.take();
    failed.sort_by_key(|f| f.0);
    failed.dedup_by_key(|f| f.0);
    let names = &cx.names;
    let target = hd_resolve::show_ty(names, opt.target);
    Ok(failed
        .into_iter()
        .map(|(i, bound)| {
            let m = &opt.members[i];
            format!(
                "the member `{}` of type {} does not implement {}, so {target} cannot derive {}",
                m.label,
                hd_resolve::show_ty(names, m.ty),
                hd_resolve::show_ty(names, bound),
                names.path(opt.trait_)
            )
        })
        .collect())
}

impl<'c> Ck<'_, 'c> {
    /// Enters an opt-in's instance: its parameters' bounds join the
    /// environment, under which the members' obligations are solved.
    pub(crate) fn enter_opt_in(&mut self, opt: &'c OptIn) {
        self.opt_in = Some(opt);
        let pool = self.pool();
        let gs = self
            .cx
            .lookup
            .item(opt.impl_)
            .map(|i| i.generics.clone())
            .unwrap_or_default();
        for (i, g) in gs.iter().enumerate() {
            if g.row {
                continue;
            }
            let p = pool.intern_ty(&TyData::Param(ParamRef {
                owner: opt.impl_,
                index: u16::try_from(i).unwrap_or(u16::MAX),
            }));
            for b in &g.bounds {
                self.add_bound(p, *b, 0);
            }
        }
        self.env.key = Some(self.cx.global.env_key(&self.env));
    }

    /// The member obligations of the instance's `walk`, `describe` and
    /// `build` calls (`annot.walker.obligation`): each member's type
    /// against the bounds of the `member` of the walker, describer or
    /// source implementation the call chose. Only a goal the solver
    /// answers `Fails` is a failing member.
    pub(crate) fn opt_in_obligations(&mut self) -> StageResult<()> {
        let Some(opt) = self.opt_in else {
            return Ok(());
        };
        let calls = std::mem::take(&mut self.structure_calls);
        let pool = self.pool();
        let k = self.cx.names.known;
        let t = pool.intern_ty(&TyData::Param(ParamRef {
            owner: opt.template,
            index: 0,
        }));
        for (method, w) in calls {
            let w = self.zonk(w);
            let w = self.strip_mut(w);
            let Some(name) = self.cx.lookup.item(method).map(|i| i.name) else {
                continue;
            };
            let protocol = match self.cx.names.text(name) {
                "walk" => k.walker,
                "describe" => k.describer,
                "build" => k.source,
                _ => continue,
            };
            let tref = TraitRef {
                trait_: protocol,
                self_ty: w,
                args: pool.list(&[t]),
            };
            // A generic walker's `member` is the trait's, which bounds
            // nothing (`annot.walker.generic-member-call`).
            let Answer::Holds {
                evidence: Evidence::Impl { row, args },
                ..
            } = self.solve(tref)?
            else {
                continue;
            };
            let Some(def) = self
                .cx
                .impl_table(row.module)
                .and_then(|tb| tb.def.get(row.row as usize).copied())
            else {
                continue;
            };
            let Some(m) = self.impl_member(def) else {
                continue;
            };
            let Some(bounds) = self
                .cx
                .lookup
                .item(m)
                .and_then(Item::sig)
                .and_then(|s| s.generics.first())
                .map(|g| g.bounds.clone())
            else {
                continue;
            };
            let impl_args = pool.list_items(args).to_vec();
            // A derived `Default` asks nothing of a member that declares a
            // default (`std-ops.default.derive.member-bound.declared`).
            let declared_exempt = protocol == k.source && opt.trait_ == k.default;
            for (i, mem) in opt.members.iter().enumerate() {
                if declared_exempt && mem.has_default {
                    continue;
                }
                for &b in &bounds {
                    let b = pool.subst(b, &|p: ParamRef| {
                        if p.owner == def {
                            impl_args.get(usize::from(p.index)).copied()
                        } else if p.owner == m && p.index == 0 {
                            Some(mem.ty)
                        } else if p.owner == opt.template && p.index == 0 {
                            Some(opt.target)
                        } else {
                            None
                        }
                    });
                    if self.member_fails(mem.ty, b)? {
                        opt.failed.borrow_mut().push((i, b));
                    }
                }
            }
        }
        Ok(())
    }

    /// The `member` method of a walker, describer or source implementation.
    fn impl_member(&self, impl_: DefId) -> Option<DefId> {
        let ItemData::Impl { methods, .. } = &self.cx.lookup.item(impl_)?.data else {
            return None;
        };
        methods
            .iter()
            .find(|(n, _)| self.cx.names.text(*n) == "member")
            .map(|(_, d)| *d)
    }

    /// Whether a member of type `ty` provably misses the trait value
    /// `bound`. Compiler-supplied traits the solver does not answer yet
    /// (trait-solver.md §3.9) never fail here, as in the header check.
    fn member_fails(&mut self, ty: Ty, bound: Ty) -> StageResult<bool> {
        let pool = self.pool();
        let TyData::TraitValue { def, args, .. } = pool.get(bound) else {
            return Ok(false);
        };
        let k = self.cx.names.known;
        if [
            k.any,
            k.any_val,
            k.any_ref,
            k.tuple,
            k.structure,
            k.inspectable,
        ]
        .contains(&def)
        {
            return Ok(false);
        }
        // A member whose type is a newtype: derived newtypes have no
        // implementation head yet (`trait.derive.newtype`), so it cannot
        // be judged.
        if let TyData::Adt { def: d, .. } = pool.get(self.strip_mut(ty))
            && matches!(
                self.cx.lookup.item(d).map(|i| &i.data),
                Some(ItemData::Newtype(_))
            )
        {
            return Ok(false);
        }
        let tref = TraitRef {
            trait_: def,
            self_ty: ty,
            args,
        };
        if pool.has_poison(ty) || self.builtin_holds(tref).is_some() {
            return Ok(false);
        }
        Ok(matches!(self.solve(tref)?, Answer::Fails(_)))
    }
}
