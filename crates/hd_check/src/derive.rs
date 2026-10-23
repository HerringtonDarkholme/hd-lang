//! The instantiated-template check at a derivation opt-in (annotations,
//! `annot.template.checked`; `crates/hd_tir/DESIGN.md` §4.13.9 "Derive
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
//! the member (`annot.walker.obligation.error`); for `Eq`, `PartialOrd`,
//! `Ord` and `Hash` the driver reports it as `derive-field-missing-trait`
//! at the field instead (`trait.derive.field-missing-trait.template`),
//! using the member's `slot`. Every other diagnostic of
//! the instance check is dropped: the template's module reports the
//! template's own mistakes.

use std::cell::{Cell, RefCell};

use hd_base::{DefId, StageResult};
use hd_diag::{Code, DiagBuf};
use hd_resolve::{Item, ItemData, Lookup, Names};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_types::solver::{Answer, Evidence, MEMBER_PARAM, MemberOn, MemberStep, TraitRef};
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
    /// Its written type position in the data type or enum (`anchor`
    /// slots), where a diagnostic about it points.
    pub slot: u32,
}

/// One derivation opt-in: a derived implementation or a derivation block,
/// the template it instantiates, and the members it covers.
#[derive(Debug)]
pub struct OptIn {
    /// The derived implementation or the derivation block.
    pub impl_: DefId,
    pub template: DefId,
    pub trait_: DefId,
    /// The data type or enum the opt-in derives for.
    pub data: DefId,
    /// The target, over the opt-in's parameters.
    pub target: Ty,
    pub members: Vec<OptInMember>,
    /// The members its member lines omit (`name = pass`).
    pub omitted: Vec<String>,
    /// Whether its `build` fills a member that declares a default from
    /// that default, as an omitted member is: a `build` call of its
    /// template passes a source whose implementation carries
    /// `fills_defaults()` (`annot.impl-fact.fills-defaults`). Such a
    /// member is not offered to the source, so its type owes the source's
    /// bounds nothing (`annot.impl-fact.fills-defaults.exempt`). The
    /// instance check sets it, as only that check knows the source.
    fills_declared: Cell<bool>,
    /// Whether its block carries a member line, which gives it a
    /// `Structure` of its own (`annot.structure.per-derivation`).
    pub lines: bool,
    /// Failing members found so far: member index and the bound it misses.
    failed: RefCell<Vec<(usize, Ty)>>,
}

impl OptIn {
    /// The owner of the `Structure` this derivation sees
    /// (`annot.structure.per-derivation`): the derivation itself when its
    /// member lines edit it or its source fills declared defaults
    /// (`annot.impl-fact.fills-defaults.scope`), else the data type or
    /// enum, whose declaration `Structure` every such derivation shares.
    /// Known once the instance is checked.
    #[must_use]
    pub fn structure_owner(&self) -> DefId {
        if self.lines || self.fills_declared() {
            self.impl_
        } else {
            self.data
        }
    }

    /// Whether its `build` fills a member that declares a default from
    /// that default, and some member declares one. Known once the
    /// instance is checked.
    #[must_use]
    pub fn fills_declared(&self) -> bool {
        self.fills_declared.get() && self.members.iter().any(|m| m.has_default)
    }

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
        let slot_of = |i: usize| u32::try_from(i + 1).unwrap_or(0);
        let mut members = Vec::new();
        match &lookup.item(def)?.data {
            // A data type's members are its fields, embedded ones included
            // (`data.derive.members`).
            ItemData::Data(fields) => {
                for (i, f) in fields.iter().enumerate() {
                    let l = label(f);
                    if omitted.contains(&l) {
                        continue;
                    }
                    members.push(OptInMember {
                        label: l,
                        ty: ty(f.ty),
                        has_default: f.has_default,
                        slot: slot_of(i),
                    });
                }
            }
            // An enum's members are each variant's payload parameters;
            // shared constructor data is not a member (`data.derive.shared`).
            ItemData::Enum { shared, variants } => {
                // The shared fields' positions come first.
                let mut at = shared.len();
                for v in variants {
                    for f in &v.fields {
                        members.push(OptInMember {
                            label: format!("{}.{}", names.text(v.name), label(f)),
                            ty: ty(f.ty),
                            has_default: f.has_default,
                            slot: slot_of(at),
                        });
                        at += 1;
                    }
                }
            }
            _ => return None,
        }
        Some(Self {
            impl_: it.def,
            template,
            trait_: *trait_,
            data: def,
            target: *self_ty,
            members,
            omitted: omitted.to_vec(),
            fills_declared: Cell::new(false),
            lines: false,
            failed: RefCell::new(Vec::new()),
        })
    }
}

/// The member lines of a derivation block that omit their member
/// (`name = pass`), with the member's name.
fn omit_lines<'t>(src: &hd_resolve::Src<'t>, block: NodeRef<'t>) -> Vec<(String, NodeRef<'t>)> {
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
            out.push((src.text(first).to_owned(), line));
        }
    }
    out
}

/// The members a derivation block's member lines omit: `name = pass`.
#[must_use]
pub fn omitted_members(src: &hd_resolve::Src<'_>, block: NodeRef<'_>) -> Vec<String> {
    omit_lines(src, block).into_iter().map(|(n, _)| n).collect()
}

/// `annot.omit.no-default`: a member omitted with `= pass` declares a
/// default for `build` to fill it with, and an embedded part, which has
/// no default syntax, never does. The error is on the member line.
pub fn check_omitted_defaults(
    names: &Names<'_>,
    lookup: &Lookup<'_>,
    src: &hd_resolve::Src<'_>,
    (it, block): (&Item, NodeRef<'_>),
    diags: &mut DiagBuf,
) {
    let pool = names.pool.types();
    let ItemData::Impl { self_ty, .. } = &it.data else {
        return;
    };
    let TyData::Adt { def, .. } = pool.get(*self_ty) else {
        return;
    };
    let Some(ItemData::Data(fields)) = lookup.item(def).map(|i| &i.data) else {
        return;
    };
    for (name, line) in omit_lines(src, block) {
        if fields
            .iter()
            .any(|f| names.text(f.name) == name && !f.has_default)
        {
            let msg = format!("`{name}` is omitted but declares no default for `build` to use");
            diags.error(Code::OmittedMemberWithoutDefault, src.span(line), &msg);
        }
    }
}

/// One member line of a derivation block (`annot.line.*`): the member or
/// variant it names, `Self` for the type; whether it extends (`+=`) or
/// replaces (`=`); and its list, or `None` for `= pass`.
#[derive(Clone, Copy)]
pub struct MemberLine<'t> {
    pub name: &'t str,
    pub extend: bool,
    pub list: Option<NodeRef<'t>>,
}

/// The member lines of a derivation block, with or without a trait, in
/// source order.
#[must_use]
pub fn member_lines<'t>(src: &hd_resolve::Src<'t>, block: NodeRef<'t>) -> Vec<MemberLine<'t>> {
    let Some(body) = hd_resolve::Src::child(block, SyntaxKind::Block) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in body
        .children()
        .filter(|c| c.kind() == SyntaxKind::Derivation)
    {
        let mut toks = src.tokens(line);
        let (Some(name), Some(op)) = (toks.next(), toks.next()) else {
            continue;
        };
        let Some(value) = line.children().last() else {
            continue;
        };
        let pass = value.kind() == SyntaxKind::LiteralExpr
            && src
                .tokens(value)
                .any(|t| src.tkind(t) == Some(TokenKind::KwPass));
        out.push(MemberLine {
            name: src.text(name),
            extend: src.tkind(op) == Some(TokenKind::PlusEq),
            list: (!pass).then_some(value),
        });
    }
    out
}

/// What one checked opt-in yields: one message per failing member, with
/// the member's index in `opt.members`, in member order; and when it
/// checks clean, the checked template bodies by template method.
pub struct OptInCheck {
    pub failures: Vec<(usize, String)>,
    pub bodies: Vec<(DefId, hd_tir::Body)>,
}

/// Checks the template `methods` (their `DefId` and syntax) as `opt`'s
/// instance. `cx` sees the template's module. Diagnostics of the bodies
/// themselves are dropped: a body with one is not kept.
pub fn check_opt_in(
    cx: &BodyCx<'_>,
    opt: &OptIn,
    methods: &[(DefId, NodeRef<'_>)],
) -> StageResult<OptInCheck> {
    let mut bodies = Vec::new();
    for &(def, node) in methods {
        let mut scratch = DiagBuf::default();
        let b = check_fn_in(cx, def, node, &mut scratch, Some(opt))?;
        if !scratch.has_errors() {
            bodies.push((def, b));
        }
    }
    if bodies.len() != methods.len() {
        bodies.clear();
    }
    let mut failed = opt.failed.take();
    failed.sort_by_key(|f| f.0);
    failed.dedup_by_key(|f| f.0);
    let names = &cx.names;
    let target = hd_resolve::show_ty(names, opt.target);
    let failures: Vec<(usize, String)> = failed
        .into_iter()
        .map(|(i, bound)| {
            let m = &opt.members[i];
            let msg = format!(
                "the member `{}` of type {} does not implement {}, so {target} cannot derive {}",
                m.label,
                hd_resolve::show_ty(names, m.ty),
                hd_resolve::show_ty(names, bound),
                names.display_name(opt.trait_)
            );
            (i, msg)
        })
        .collect();
    if !failures.is_empty() {
        bodies.clear();
    }
    Ok(OptInCheck { failures, bodies })
}

/// The derive instance of one template method (`crates/hd_tir/DESIGN.md`
/// §4.13.9): the body that `check_opt_in` checked, as the method of the
/// derived implementation `opt.impl_` of the same name. The template's
/// `T` becomes the target, the template method's own parameters the
/// instance method's, and each `Structure` call, which the template
/// answers from its `T < Structure` bound, calls the generated method of
/// the `Structure` this derivation sees (`annot.structure.per-derivation`):
/// a hidden method of its `structure_owner`, at the owner's arguments and
/// the call's own. So derivations that see one `Structure` share its
/// instances (`crates/hd_mono/DESIGN.md` §13.6).
pub fn instance(
    cx: &BodyCx<'_>,
    opt: &OptIn,
    method: DefId,
    mut body: hd_tir::Body,
) -> StageResult<(Item, hd_tir::Body)> {
    let names = &cx.names;
    let pool = names.pool;
    let Some(mi) = cx.lookup.item(method) else {
        return crate::body::unsupported("a template method outside the module's view");
    };
    let Some(sig) = mi.sig() else {
        return crate::body::unsupported("a template method without a signature");
    };
    let def = names.member(opt.impl_, hd_intern::PathKind::Member, names.text(mi.name));
    let f = |p: ParamRef| {
        if p.owner == opt.template && p.index == 0 {
            Some(opt.target)
        } else if p.owner == method {
            Some(pool.intern_ty(&TyData::Param(ParamRef {
                owner: def,
                index: p.index,
            })))
        } else {
            None
        }
    };
    let structure = names.known.structure;
    let owner = opt.structure_owner();
    let owner_args = crate::structure::owner_args(cx, opt);
    for i in 0..body.len() {
        if !matches!(body.tags[i], hd_tir::Tag::Call | hd_tir::Tag::Await) {
            continue;
        }
        let at = body.data[i][0];
        let Some(hd_tir::Callee::TraitMethod {
            trait_,
            method: m,
            targs,
            choice: (hd_tir::ir::ChoiceKind::Bound, _),
            ..
        }) = hd_tir::Callee::from_words(body.record(at))
        else {
            continue;
        };
        if trait_ != structure {
            continue;
        }
        let Some(name) = cx.lookup.item(m).map(|it| names.text(it.name)) else {
            return crate::body::unsupported("a `Structure` method outside the module's view");
        };
        // The owner's parameters, then the method's own.
        let mut all = pool.list_items(owner_args).to_vec();
        all.extend_from_slice(pool.list_items(targs));
        let words = hd_tir::Callee::Item {
            def: hd_structure::hidden_method(names, owner, name),
            targs: pool.list(&all),
        }
        .words();
        // Rewrite the record in place: an appended one would leave the
        // trait call's record behind, unreachable, and its raw pool IDs
        // would reach the wire unmapped, so the cache entry would depend
        // on the build's interning order (#610). An item callee's record
        // is the shorter one; the words it frees are zeroed.
        let start = at as usize + 1;
        let old = body.record(at).len();
        if words.len() > old {
            return crate::body::unsupported("an item callee longer than its trait call");
        }
        body.extra[at as usize] = u32::try_from(words.len()).unwrap_or(u32::MAX);
        body.extra[start..start + words.len()].copy_from_slice(&words);
        body.extra[start + words.len()..start + old].fill(0);
    }
    body.kind = hd_tir::BodyKind::DeriveInstance;
    hd_tir::wire::map_ids(
        &mut body,
        &|d| if d == method { def } else { d },
        &|t| pool.subst(t, &f),
        &|l| {
            let v: Vec<Ty> = pool
                .list_items(l)
                .iter()
                .map(|t| pool.subst(*t, &f))
                .collect();
            pool.list(&v)
        },
    )?;
    let sig = hd_structure::subst_sig(names, sig, &f);
    let item = Item::new(
        def,
        mi.name,
        mi.public,
        ItemData::Method {
            owner: opt.impl_,
            sig,
            has_body: true,
        },
    );
    Ok((item, body))
}

/// What a template's `walk`, `describe` or `build` call asks of each
/// member: the bounds of the chosen implementation's
/// `member` and `rest` (for a walker that implements `rest`), each with
/// the method whose own parameter stands for the member; and whether the
/// call is `build` with a source whose implementation carries
/// `fills_defaults()` (`annot.impl-fact.fills-defaults`).
struct WalkerBounds {
    member: Vec<(DefId, Ty)>,
    rest: Option<Vec<(DefId, Ty)>>,
    fills_defaults: bool,
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

    /// The walker, describer or source implementation that a `walk`,
    /// `describe` or `build` call of a template body chose
    /// (`annot.walker.obligation`): its `member`'s bounds and,
    /// for a walker that implements `rest`, `rest`'s, each over the
    /// implementation's arguments, the template's `T` and the method's own
    /// parameter. `None` for another call, and for a generic walker, whose
    /// `member` is the trait's and bounds nothing
    /// (`annot.walker.generic-member-call`).
    fn walker_bounds(
        &mut self,
        template: DefId,
        (method, w): (DefId, Ty),
    ) -> StageResult<Option<WalkerBounds>> {
        let pool = self.pool();
        let k = self.cx.names.known;
        let t = pool.intern_ty(&TyData::Param(ParamRef {
            owner: template,
            index: 0,
        }));
        let w = self.zonk(w);
        let w = self.strip_mut(w);
        let Some(name) = self.cx.lookup.item(method).map(|i| i.name) else {
            return Ok(None);
        };
        let protocol = match self.cx.names.text(name) {
            "walk" => k.walker,
            "describe" => k.describer,
            "build" => k.source,
            _ => return Ok(None),
        };
        let tref = TraitRef {
            trait_: protocol,
            self_ty: w,
            args: pool.list(&[t]),
        };
        let Answer::Holds {
            evidence: Evidence::Impl { row, args },
            ..
        } = self.solve(tref)?
        else {
            return Ok(None);
        };
        let Some(def) = self
            .cx
            .impl_table(row.module)
            .and_then(|tb| tb.def.get(row.row as usize).copied())
        else {
            return Ok(None);
        };
        let impl_args = pool.list_items(args).to_vec();
        // A method's bounds on its own first parameter, with the
        // implementation's parameters replaced by its arguments.
        let bounds = |m: DefId| -> Option<Vec<(DefId, Ty)>> {
            let gs = self
                .cx
                .lookup
                .item(m)
                .and_then(Item::sig)?
                .generics
                .first()?
                .bounds
                .clone();
            Some(
                gs.into_iter()
                    .map(|b| {
                        let b = pool.subst(b, &|p: ParamRef| {
                            (p.owner == def)
                                .then(|| impl_args.get(usize::from(p.index)).copied())
                                .flatten()
                        });
                        (m, b)
                    })
                    .collect(),
            )
        };
        let Some(member) = self.impl_method(def, "member").and_then(bounds) else {
            return Ok(None);
        };
        let rest = if protocol == k.walker {
            self.impl_method(def, "rest").and_then(bounds)
        } else {
            None
        };
        let fills_defaults =
            protocol == k.source && self.cx.lookup.item(def).is_some_and(|it| it.fills_defaults);
        Ok(Some(WalkerBounds {
            member,
            rest,
            fills_defaults,
        }))
    }

    /// The member obligations of the instance's `walk`, `describe` and
    /// `build` calls (`annot.walker.obligation`): each member's type
    /// against the bounds of the `member` of the walker, describer or
    /// source implementation the call chose. Only a goal the solver
    /// answers `Fails` is a failing member. A source that fills declared
    /// defaults is offered no member that declares one, which so owes it
    /// nothing (`annot.impl-fact.fills-defaults.exempt`).
    pub(crate) fn opt_in_obligations(&mut self) -> StageResult<()> {
        let Some(opt) = self.opt_in else {
            return Ok(());
        };
        let calls = std::mem::take(&mut self.structure_calls);
        let pool = self.pool();
        for call in calls {
            let Some(wb) = self.walker_bounds(opt.template, call)? else {
                continue;
            };
            if wb.fills_defaults {
                opt.fills_declared.set(true);
            }
            for (i, mem) in opt.members.iter().enumerate() {
                if wb.fills_defaults && mem.has_default {
                    continue;
                }
                for &(m, b) in &wb.member {
                    let b = pool.subst(b, &|p: ParamRef| {
                        if p.owner == m && p.index == 0 {
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

    /// A tuple template's member steps (`crates/hd_check/DESIGN.md` (trait solver) §3.9): the bounds
    /// of each walker, describer or source its body passes, on every
    /// element, and on a rest element by `annot.walker.obligation.rest`.
    /// The template's `T` stays; the method's parameter becomes
    /// [`MEMBER_PARAM`].
    pub(crate) fn tuple_plan_steps(&mut self) -> StageResult<()> {
        let Some(sink) = self.plan_sink else {
            return Ok(());
        };
        let item = self.b.body_mut().item;
        let Some(ItemData::Method {
            owner: template, ..
        }) = self.cx.lookup.item(item).map(|i| i.data.clone())
        else {
            return Ok(());
        };
        let calls = std::mem::take(&mut self.structure_calls);
        let pool = self.pool();
        let step = |on: MemberOn, (m, b): (DefId, Ty)| {
            let b = pool.subst(b, &|p: ParamRef| {
                (p.owner == m && p.index == 0).then(|| {
                    pool.intern_ty(&TyData::Param(ParamRef {
                        owner: template,
                        index: MEMBER_PARAM,
                    }))
                })
            });
            match pool.get(b) {
                TyData::TraitValue { def, args, .. } => Some(MemberStep {
                    on,
                    trait_: def,
                    args,
                }),
                _ => None,
            }
        };
        let mut out = Vec::new();
        for call in calls {
            let Some(wb) = self.walker_bounds(template, call)? else {
                continue;
            };
            out.extend(
                wb.member
                    .iter()
                    .filter_map(|b| step(MemberOn::Elements, *b)),
            );
            match &wb.rest {
                Some(rest) => out.extend(rest.iter().filter_map(|b| step(MemberOn::RestItem, *b))),
                None => out.extend(
                    wb.member
                        .iter()
                        .filter_map(|b| step(MemberOn::RestList, *b)),
                ),
            }
        }
        sink.borrow_mut().extend(out);
        Ok(())
    }

    /// The method `name` that a walker, describer or source
    /// implementation writes.
    fn impl_method(&self, impl_: DefId, name: &str) -> Option<DefId> {
        let ItemData::Impl { methods, .. } = &self.cx.lookup.item(impl_)?.data else {
            return None;
        };
        methods
            .iter()
            .find(|(n, _)| self.cx.names.text(*n) == name)
            .map(|(_, d)| *d)
    }

    /// Whether a member of type `ty` provably misses the trait value
    /// `bound`: the solver answers `Fails`.
    fn member_fails(&mut self, ty: Ty, bound: Ty) -> StageResult<bool> {
        let pool = self.pool();
        let TyData::TraitValue { def, args, .. } = pool.get(bound) else {
            return Ok(false);
        };
        let tref = TraitRef {
            trait_: def,
            self_ty: ty,
            args,
        };
        if pool.has_poison(ty) {
            return Ok(false);
        }
        Ok(matches!(self.solve(tref)?, Answer::Fails(_)))
    }
}
