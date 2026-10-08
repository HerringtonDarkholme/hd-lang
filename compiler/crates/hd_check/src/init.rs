//! Module initialization (checking-and-tir.md §4.13.10; spec/lang/
//! 10-modules.md, "Module Initialization"): a module's top-level
//! statements are one `Init` body, checked before its functions so their
//! bodies see the top-level bindings; each body records the init facts
//! (the top-level bindings it reads, the same-module functions it calls);
//! definite initialization combines them bottom-up over the call graph.

use std::collections::{HashMap, HashSet};

use hd_base::{DefId, NodeIdx, StageResult, Symbol};
use hd_diag::{Code, DiagBuf};
use hd_resolve::ItemData;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::Body;
use hd_tir::ir::{BodyKind, NONE, Ref, Tag, TirSink};
use hd_types::{RowData, RowId, Ty};

use crate::body::{BodyCx, Ck, new_ck, unsupported};

/// A top-level binding: its item, type and the statement that initializes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Global {
    pub def: DefId,
    pub ty: Ty,
    pub stmt: usize,
}

/// What one body or top-level statement reaches directly.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InitFacts {
    pub reads: HashSet<DefId>,
    pub calls: HashSet<DefId>,
}

/// A module's top-level bindings and the facts its bodies recorded.
#[derive(Default)]
pub struct ModuleInit {
    pub globals: HashMap<Symbol, Global>,
    pub facts: HashMap<DefId, InitFacts>,
}

/// The top-level executable statements of a module, in source order. An
/// enum declaration with shared constructor data counts as one: its
/// constructor expressions run at its source position
/// (`data.shared.module-init.order`).
#[must_use]
pub fn init_statements(root: NodeRef<'_>) -> Vec<NodeRef<'_>> {
    root.children()
        .filter(|c| {
            matches!(
                c.kind(),
                SyntaxKind::LetStmt
                    | SyntaxKind::ExprStmt
                    | SyntaxKind::AssignmentStmt
                    | SyntaxKind::DiscardStmt
                    | SyntaxKind::ReturnStmt
                    | SyntaxKind::DeferStmt
                    | SyntaxKind::BreakStmt
                    | SyntaxKind::ContinueStmt
            ) || (c.kind() == SyntaxKind::EnumDecl
                && c.descendants()
                    .any(|d| d.kind() == SyntaxKind::VariantSharedData))
        })
        .collect()
}

/// The global that holds one variant's shared field (`data.shared.not-stored`).
#[must_use]
pub fn shared_global(names: &hd_resolve::Names<'_>, variant: DefId, field: &str) -> DefId {
    names.member(variant, hd_intern::PathKind::Hidden, field)
}

/// Checks a module's top-level statements as its `Init` body. `entry`:
/// the entry module, whose top level may use an inferred entry row
/// (`module.init.script-row`); any other module's is requirement-free
/// (`module.init.requirement-free`). Returns the body and each
/// statement's facts.
pub fn check_init(
    cx: &BodyCx<'_>,
    item: DefId,
    module: &str,
    stmts: &[NodeRef<'_>],
    entry: bool,
    diags: &mut DiagBuf,
) -> StageResult<(Body, Vec<InitFacts>)> {
    let pool = cx.names.pool;
    let row = if entry {
        // An inferred row: every key is available at the top level.
        pool.row(&RowData {
            keys: vec![],
            params: vec![hd_types::RowParamRef {
                owner: item,
                index: 0,
            }],
        })
    } else {
        RowId::EMPTY
    };
    let mut ck = new_ck(cx, item, item, BodyKind::Init, (Ty::VOID, row), diags);
    ck.module_init = Some(module.to_owned());
    let blk = ck.b.open_block();
    let mut per_stmt = Vec::new();
    for (i, s) in stmts.iter().enumerate() {
        ck.init_stmt = i;
        ck.top_level(*s).map_err(|e| e.at(cx.src.span(*s)))?;
        per_stmt.push(std::mem::take(&mut ck.facts));
    }
    // Final types of the bindings, before the body's own sweep.
    let mut globals: Vec<(Symbol, Global)> = cx
        .init
        .borrow()
        .globals
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect();
    for (_, g) in &mut globals {
        g.ty = ck.zonk(g.ty);
    }
    cx.init.borrow_mut().globals = globals.into_iter().collect();
    let root = ck.b.close_block(blk, None, Ty::VOID, NodeIdx::NONE);
    let body = ck.finish_body(root)?;
    Ok((body, per_stmt))
}

impl Ck<'_, '_> {
    /// A top-level statement: a simple binding becomes a global.
    fn top_level(&mut self, s: NodeRef<'_>) -> StageResult<()> {
        match s.kind() {
            SyntaxKind::EnumDecl => return self.shared_init(s),
            // `flow.return.outside`, `flow.defer.outside`.
            SyntaxKind::ReturnStmt => {
                self.err(
                    Code::ReturnOutsideFunction,
                    s,
                    "return-outside-function: `return` needs a function or closure",
                );
                return Ok(());
            }
            SyntaxKind::DeferStmt => {
                self.err(
                    Code::DeferOutsideCleanupScope,
                    s,
                    "defer-outside-cleanup-scope: a module's top level is not a cleanup scope",
                );
                return Ok(());
            }
            _ => {}
        }
        let kids: Vec<NodeRef<'_>> = s.children().collect();
        let binding = match s.kind() {
            SyntaxKind::LetStmt => {
                let pat = kids.first().copied();
                let ty_node = kids.iter().copied().find(|k| k.kind().is_type());
                let rhs = kids
                    .iter()
                    .copied()
                    .skip(1)
                    .find(|k| !k.kind().is_type() && k.kind() != SyntaxKind::ElseClause);
                pat.zip(rhs).map(|(p, r)| (p, ty_node, r))
            }
            SyntaxKind::ExprStmt => match kids.first() {
                Some(b) if b.kind() == SyntaxKind::BindingExpr => {
                    let bk: Vec<NodeRef<'_>> = b.children().collect();
                    match bk.as_slice() {
                        [p, r] => Some((*p, None, *r)),
                        _ => None,
                    }
                }
                _ => None,
            },
            _ => None,
        };
        let Some((pat, ty_node, rhs)) = binding else {
            self.stmt(s)?;
            return Ok(());
        };
        if pat.kind() != SyntaxKind::BindingPattern || pat.children().next().is_some() {
            return unsupported("a top-level binding with a pattern");
        }
        let Some(nt) = pat.direct_tokens().find(|t| {
            matches!(
                self.cx.src.tkind(*t),
                Some(TokenKind::Ident | TokenKind::RawIdent)
            )
        }) else {
            return unsupported("a top-level binding without a name");
        };
        let text = self.cx.src.text(nt).to_owned();
        let name = self.cx.names.syms.intern(&text);
        let annot = match ty_node {
            Some(t) => Some(self.ty_node(t)?),
            None => None,
        };
        let (r, t) = self.expr(rhs, annot)?;
        let (r, t) = match annot {
            Some(a) => (self.coerce(r, t, a, rhs, "binding"), a),
            None => (r, t),
        };
        let module = self.module_init.clone().unwrap_or_default();
        let def = self.cx.names.item(&module, &text);
        self.cx.init.borrow_mut().globals.insert(
            name,
            Global {
                def,
                ty: t,
                stmt: self.init_stmt,
            },
        );
        let a = self.b.refs_record(&[Ref(def.raw())]);
        self.b.emit(Tag::GlobalSet, a, r.0, Ty::VOID, s.index());
        Ok(())
    }

    /// An enum's shared constructor data (`data.shared.module-init`): each
    /// variant's constructor call, checked against the shared parameters,
    /// sets one global per shared field. Omitted defaults are `DefaultCall`s
    /// over the earlier values (`data.shared.default.eval-order`).
    fn shared_init(&mut self, s: NodeRef<'_>) -> StageResult<()> {
        let tokens = &self.cx.src.parse.tokens;
        let Some(t) = s.name(tokens) else {
            return unsupported("an enum without a name");
        };
        let ename = self.cx.src.text(t).to_owned();
        let module = self.module_init.clone().unwrap_or_default();
        let def = self.cx.names.item(&module, &ename);
        let Some(item) = self.cx.lookup.item(def) else {
            return unsupported("shared data of an unknown enum");
        };
        let ItemData::Enum { shared, variants } = item.data.clone() else {
            return unsupported("shared data outside an enum");
        };
        if !item.generics.is_empty() {
            return unsupported("shared data of a generic enum");
        }
        let params: Vec<(Symbol, Ty)> = shared.iter().map(|f| (f.name, f.ty)).collect();
        for v in s
            .descendants()
            .filter(|c| c.kind() == SyntaxKind::EnumVariant)
        {
            let Some(vt) = v.name(&self.cx.src.parse.tokens) else {
                continue;
            };
            let vname = self.cx.src.text(vt).to_owned();
            let Some(var) = variants
                .iter()
                .find(|x| self.cx.names.text(x.name) == vname)
            else {
                continue;
            };
            let Some(sd) = v
                .children()
                .find(|c| c.kind() == SyntaxKind::VariantSharedData)
            else {
                let msg = format!(
                    "missing-required-field: variant `{vname}` gives no shared data of `{ename}`"
                );
                self.err(Code::MissingRequiredField, v, &msg);
                continue;
            };
            let owner = self.cx.src.text(self.cx.src.first(sd)).to_owned();
            if owner != ename {
                let msg =
                    format!("variant-result-owner: `{owner}` is not the enclosing enum `{ename}`");
                self.err(Code::VariantResultOwner, sd, &msg);
                continue;
            }
            let al = sd.children().find(|c| c.kind() == SyntaxKind::ArgumentList);
            let args = self.args_of(al)?;
            let vals = self.shared_args(s, &params, &args, sd, &ename)?;
            for (f, r) in shared.iter().zip(vals) {
                let fname = self.cx.names.text(f.name).to_owned();
                let g = shared_global(&self.cx.names, var.def, &fname);
                let a = self.b.refs_record(&[Ref(g.raw())]);
                self.b.emit(Tag::GlobalSet, a, r.0, Ty::VOID, sd.index());
            }
        }
        Ok(())
    }

    /// One variant's constructor arguments against the shared parameters:
    /// explicit arguments first, then each omitted default in parameter
    /// order, inline, with the earlier parameters in scope
    /// (`data.shared.default.eval-order`, `data.shared.default.scope`).
    fn shared_args(
        &mut self,
        decl: NodeRef<'_>,
        params: &[(Symbol, Ty)],
        args: &crate::call::Args<'_>,
        at: NodeRef<'_>,
        ename: &str,
    ) -> StageResult<Vec<Ref>> {
        let mut slots: Vec<Option<Ref>> = vec![None; params.len()];
        if args.positional.len() > params.len() {
            let msg = format!("argument-count: `{ename}` takes {} arguments", params.len());
            self.err(Code::ArgumentCount, at, &msg);
        }
        for (i, e) in args.positional.iter().enumerate() {
            let w = params.get(i).map(|p| p.1);
            let (r, t) = self.expr(*e, w)?;
            if let (Some(w), Some(s)) = (w, slots.get_mut(i)) {
                *s = Some(self.coerce(r, t, w, *e, "argument"));
            }
        }
        for (pname, e) in &args.named {
            let Some(i) = params
                .iter()
                .position(|p| self.cx.names.text(p.0) == pname.as_str())
            else {
                let msg = format!("unknown-named-argument: `{ename}` has no parameter `{pname}`");
                self.err(Code::UnknownNamedArgument, *e, &msg);
                continue;
            };
            if slots[i].is_some() {
                let msg = format!("duplicate-argument: `{pname}` is given twice");
                self.err(Code::DuplicateArgument, *e, &msg);
                continue;
            }
            let (r, t) = self.expr(*e, Some(params[i].1))?;
            slots[i] = Some(self.coerce(r, t, params[i].1, *e, "argument"));
        }
        let defaults: Vec<Option<NodeRef<'_>>> = decl
            .children()
            .find(|c| c.kind() == SyntaxKind::ParameterList)
            .map(|pl| {
                pl.children()
                    .map(|p| {
                        p.children()
                            .find(|c| c.kind() == SyntaxKind::DefaultValue)
                            .and_then(|dv| dv.children().next())
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut out = Vec::new();
        for (i, s) in slots.into_iter().enumerate() {
            if let Some(r) = s {
                out.push(r);
                continue;
            }
            let Some(e) = defaults.get(i).copied().flatten() else {
                let msg = format!(
                    "missing-required-field: `{ename}` needs `{}`",
                    self.cx.names.text(params[i].0)
                );
                self.err(Code::MissingRequiredField, at, &msg);
                return Ok(Vec::new());
            };
            self.scopes.push(HashMap::new());
            for (k, r) in out.iter().enumerate() {
                let l = self.bind_local(params[k].0, params[k].1, e);
                self.b.set(l, *r, e.index());
            }
            let (r, t) = self.expr(e, Some(params[i].1))?;
            self.scopes.pop();
            out.push(self.coerce(r, t, params[i].1, e, "default"));
        }
        Ok(out)
    }

    /// A shared field read on an enum value (`data.shared.per-variant`):
    /// the global of the value's variant, chosen by a `SwitchTag` chain.
    pub(crate) fn shared_field(
        &mut self,
        r: Ref,
        t: Ty,
        name: &str,
        n: NodeRef<'_>,
    ) -> StageResult<Option<(Ref, Ty)>> {
        let pool = self.cx.names.pool;
        let hd_types::TyData::Adt { def, .. } = pool.get(t) else {
            return Ok(None);
        };
        let Some(item) = self.cx.lookup.item(def) else {
            return Ok(None);
        };
        let ItemData::Enum { shared, variants } = &item.data else {
            return Ok(None);
        };
        let sym = self.cx.names.syms.intern(name);
        let Some(f) = shared.iter().find(|f| f.name == sym) else {
            return Ok(None);
        };
        if !item.generics.is_empty() {
            return unsupported("a shared field of a generic enum");
        }
        let ft = f.ty;
        let globals: Vec<DefId> = variants
            .iter()
            .map(|v| shared_global(&self.cx.names, v.def, name))
            .collect();
        if globals.is_empty() {
            return Ok(None);
        }
        Ok(Some((self.shared_chain(r, &globals, 0, ft, n), ft)))
    }

    fn shared_chain(&mut self, r: Ref, globals: &[DefId], k: usize, ft: Ty, n: NodeRef<'_>) -> Ref {
        let get = |ck: &mut Self, g: DefId| {
            let a = ck.b.refs_record(&[Ref(g.raw())]);
            ck.b.emit(Tag::GlobalGet, a, NONE, ft, n.index())
        };
        if k + 1 >= globals.len() {
            return get(self, globals[k]);
        }
        let then_b = self.b.open_block();
        let v = get(self, globals[k]);
        let then = self.b.close_block(then_b, Some(v), ft, n.index());
        let else_b = self.b.open_block();
        let w = self.shared_chain(r, globals, k + 1, ft, n);
        let other = self.b.close_block(else_b, Some(w), ft, n.index());
        let rec = self
            .b
            .refs_record(&[Ref(u32::try_from(k).unwrap_or(0)), then, other]);
        self.b.emit(Tag::SwitchTag, r.0, rec, ft, n.index())
    }

    /// A top-level binding read from any body of the module.
    pub(crate) fn global_get(&mut self, name: Symbol, n: NodeRef<'_>) -> Option<(Ref, Ty)> {
        let g = self.cx.init.borrow().globals.get(&name).copied()?;
        // Inside the init body, a binding is visible from its statement on.
        if self.module_init.is_some() && g.stmt >= self.init_stmt {
            return None;
        }
        self.facts.reads.insert(g.def);
        let a = self.b.refs_record(&[Ref(g.def.raw())]);
        Some((self.b.emit(Tag::GlobalGet, a, NONE, g.ty, n.index()), g.ty))
    }

    /// Records a call or reference of a same-module function.
    pub(crate) fn note_call(&mut self, def: DefId) {
        self.facts.calls.insert(def);
    }
}

/// `module.init.definite`: each top-level statement's transitive read set
/// (through the module's call graph) holds only bindings initialized by
/// an earlier statement. Reports `top-level-read-before-initialization`.
pub fn definite_init(
    cx: &BodyCx<'_>,
    stmts: &[NodeRef<'_>],
    per_stmt: &[InitFacts],
    diags: &mut DiagBuf,
) {
    let init = cx.init.borrow();
    let at: HashMap<DefId, usize> = init.globals.values().map(|g| (g.def, g.stmt)).collect();
    for (i, f) in per_stmt.iter().enumerate() {
        let mut seen = HashSet::new();
        let mut reads = f.reads.clone();
        let mut work: Vec<DefId> = f.calls.iter().copied().collect();
        while let Some(d) = work.pop() {
            if !seen.insert(d) {
                continue;
            }
            if let Some(ff) = init.facts.get(&d) {
                reads.extend(ff.reads.iter().copied());
                work.extend(ff.calls.iter().copied());
            }
        }
        // The earliest late binding, by statement then name: a stable choice.
        let seg = |d: &DefId| {
            cx.names
                .paths
                .segment(hd_base::PathId::from_raw(d.raw()))
                .to_owned()
        };
        let mut late: Vec<(usize, String)> = reads
            .iter()
            .filter_map(|g| at.get(g).filter(|s| **s >= i).map(|s| (*s, seg(g))))
            .collect();
        late.sort();
        if let Some((_, late)) = late.first()
            && let Some(s) = stmts.get(i)
        {
            let msg = format!(
                "top-level-read-before-initialization: this statement reads `{late}` before its initialization"
            );
            diags.error(
                Code::TopLevelReadBeforeInitialization,
                cx.src.span(*s),
                &msg,
            );
        }
    }
}
