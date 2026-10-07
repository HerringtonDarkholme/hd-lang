//! Module initialization (checking-and-tir.md §4.13.10; spec/lang/
//! 10-modules.md, "Module Initialization"): a module's top-level
//! statements are one `Init` body, checked before its functions so their
//! bodies see the top-level bindings; each body records the init facts
//! (the top-level bindings it reads, the same-module functions it calls);
//! definite initialization combines them bottom-up over the call graph.

use std::collections::{HashMap, HashSet};

use hd_base::{DefId, NodeIdx, StageResult, Symbol};
use hd_diag::{Code, DiagBuf};
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

/// The top-level executable statements of a module, in source order.
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
            )
        })
        .collect()
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
        ck.top_level(*s)?;
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
