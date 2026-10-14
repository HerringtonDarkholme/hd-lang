//! Module initialization (checking-and-tir.md §4.13.10; spec/lang/
//! 10-modules.md, "Module Initialization"): a module's top-level
//! statements are one `Init` body, checked before its functions so their
//! bodies see the top-level bindings; each body records the init facts
//! (the top-level bindings it reads, the same-module functions it calls);
//! definite initialization combines them bottom-up over the call graph.

use std::collections::{HashMap, HashSet};

use hd_base::{DefId, NodeIdx, StageResult, Symbol};
use hd_diag::{Code, DiagBuf};
use hd_resolve::{ImplKind, ItemData};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::Body;
use hd_tir::ir::{BodyKind, NONE, Ref, Tag, TirSink};
use hd_types::lookup::OWN_TABLE;
use hd_types::solver::{Evidence, PlanStep};
use hd_types::{RowData, RowId, Ty};

use crate::body::{BodyCx, Ck, ExpectPosition, RowFrame, new_ck, unsupported};

type Set = std::collections::HashSet<hd_base::LocalId>;

/// A top-level binding: its item, type and the statement that initializes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Global {
    pub def: DefId,
    pub ty: Ty,
    pub stmt: usize,
    /// The byte where the statement starts: a body written before it
    /// cannot see the binding (`names.exec.not-before`).
    pub at: u32,
    /// Bound with `:=`: not reassignable (expr.bind.one-name).
    pub short: bool,
}

impl Global {
    /// The binding as a REPL input's interface carries it.
    #[must_use]
    pub fn binding(&self) -> InputBinding {
        InputBinding {
            def: self.def,
            ty: self.ty,
            short: self.short,
        }
    }
}

/// A top-level binding as a REPL input's interface carries it for later
/// inputs (`cli.repl.declarations.bindings`): its item, whose path names
/// its storage, its final type, and whether it was bound with `:=`, so not
/// reassignable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputBinding {
    pub def: DefId,
    pub ty: Ty,
    pub short: bool,
}

/// The top-level bindings of the module `cx` checked, as its input
/// interface carries them, in statement order.
#[must_use]
pub fn input_bindings(cx: &BodyCx<'_>) -> Vec<InputBinding> {
    let init = cx.init.borrow();
    let mut out: Vec<(usize, InputBinding)> = init
        .globals
        .values()
        .map(|g| (g.stmt, g.binding()))
        .collect();
    out.sort_by_cached_key(|(s, b)| (*s, cx.names.path(b.def)));
    out.into_iter().map(|(_, b)| b).collect()
}

/// The functions and types of a REPL input whose names an earlier input
/// bound as top-level bindings (`cli.repl.redeclare.binding`,
/// `cli.repl.redeclare.type`). The folder interface cannot see those
/// names: the bindings come from the earlier inputs' checks.
pub fn earlier_binding_clashes(
    cx: &BodyCx<'_>,
    heads: &[hd_resolve::Head<'_>],
    diags: &mut DiagBuf,
) {
    let init = cx.init.borrow();
    for h in heads
        .iter()
        .filter(|h| h.kind != hd_resolve::HeadKind::Impl && h.local.is_none() && h.test.is_none())
    {
        let Some(by) = init
            .earlier
            .contains_key(&h.name)
            .then(|| init.declared_by.get(&h.name))
            .flatten()
        else {
            continue;
        };
        let (code, hint) = if h.kind.is_type() || h.kind == hd_resolve::HeadKind::Trait {
            (Code::DuplicateType, "give the new one another name")
        } else {
            (
                Code::DuplicateBinding,
                "give it another name, or bind a `let` and assign it",
            )
        };
        let msg = format!(
            "`{}` is already declared by {by}; {hint}",
            cx.names.text(h.name)
        );
        diags.error(code, hd_resolve::head_name_span(&cx.src, h), &msg);
    }
}

/// What a condition leaves on its two paths: the `:=` bindings that may
/// be uninitialized after it is true, and after it is false
/// (`names.definite.path`, `names.definite.merge`).
#[derive(Clone, Debug)]
pub(crate) struct CondFlow {
    pub node: NodeIdx,
    pub on_true: std::collections::HashSet<hd_base::LocalId>,
    pub on_false: std::collections::HashSet<hd_base::LocalId>,
}

/// What one body or top-level statement reaches directly.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InitFacts {
    pub reads: HashSet<DefId>,
    pub calls: HashSet<DefId>,
}

/// One top-level statement of a module's init summary.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StmtSummary {
    /// The first instruction the statement added to the `Init` body: the
    /// statement's instructions in the root block are those from here to
    /// the next statement's first.
    pub first_inst: u32,
    /// The statement's source bytes.
    pub lo: u32,
    pub hi: u32,
    pub facts: InitFacts,
}

/// A module's init summary (checking-and-tir.md §4.13.10): what
/// `InitOrder` reads of a checked module to order the statements of its
/// module cycle (spec 10 `module.init.cycle.dependency`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InitSummary {
    /// The top-level executable statements, in source order.
    pub stmts: Vec<StmtSummary>,
    /// Each top-level binding and the statement that initializes it.
    pub globals: Vec<(DefId, u32)>,
    /// What each body of the module reaches directly, when anything.
    pub fns: Vec<(DefId, InitFacts)>,
}

/// The init summary of the module `cx` checked: its statements, its
/// bindings, and its bodies' facts. Only the calls `keep` accepts stay: a
/// module cycle never leaves its folder (`module.init.use-graph`), so a
/// call out of it reaches no binding of the cycle.
#[must_use]
pub fn summary(
    cx: &BodyCx<'_>,
    mut stmts: Vec<StmtSummary>,
    keep: &dyn Fn(DefId) -> bool,
) -> InitSummary {
    let init = cx.init.borrow();
    let mut globals: Vec<(DefId, u32)> = init
        .globals
        .values()
        .map(|g| (g.def, u32::try_from(g.stmt).expect("statements")))
        .collect();
    globals.sort_by_cached_key(|(d, s)| (*s, cx.names.path(*d)));
    for s in &mut stmts {
        s.facts.calls.retain(|d| keep(*d));
    }
    let mut fns: Vec<(DefId, InitFacts)> = init
        .facts
        .iter()
        .map(|(d, f)| {
            let calls = f.calls.iter().copied().filter(|c| keep(*c)).collect();
            (
                *d,
                InitFacts {
                    reads: f.reads.clone(),
                    calls,
                },
            )
        })
        .filter(|(_, f)| !f.reads.is_empty() || !f.calls.is_empty())
        .collect();
    fns.sort_by_cached_key(|(d, _)| cx.names.path(*d));
    InitSummary {
        stmts,
        globals,
        fns,
    }
}

/// One run of a split `Init` body (spec 10 `module.init.cycle.step`):
/// `body` with only the root-block instructions in `insts`, the
/// instructions of a run of consecutive statements, under the item
/// `item`. The other statements' instructions stay, unreached.
#[must_use]
pub fn init_segment(body: &Body, item: DefId, insts: std::ops::Range<u32>) -> Body {
    let mut b = body.clone();
    b.item = item;
    let root = b.sub_root[0] as usize;
    let at = b.data[root][0] as usize;
    let kids: Vec<u32> = b
        .record(b.data[root][0])
        .iter()
        .copied()
        .filter(|k| insts.contains(k))
        .collect();
    // The record shrinks in place; the words past its new length are
    // never read.
    b.extra[at] = u32::try_from(kids.len()).expect("record");
    b.extra[at + 1..at + 1 + kids.len()].copy_from_slice(&kids);
    b
}

/// A module's top-level bindings and the facts its bodies recorded.
#[derive(Default)]
pub struct ModuleInit {
    pub globals: HashMap<Symbol, Global>,
    /// For a REPL input: the top-level bindings of the earlier inputs, by
    /// name, from their input interfaces (`cli.repl.declarations.bindings`).
    /// They ran before this input, so every body and statement sees them,
    /// and no initialization order involves them. The module's own
    /// bindings come first.
    pub earlier: HashMap<Symbol, InputBinding>,
    /// For a REPL input: what declared each name its session holds, by
    /// name (`src/lib.hd` or `repl#N`). A top-level binding of the input
    /// repeats none of them (`cli.repl.redeclare.binding`).
    pub declared_by: HashMap<Symbol, String>,
    pub facts: HashMap<DefId, InitFacts>,
    /// The methods of the module's own trait impls, by trait; built on
    /// first use (`module.init.definite.dispatch`).
    impl_methods: Option<HashMap<DefId, Vec<(Symbol, DefId)>>>,
}

impl ModuleInit {
    /// The state of a REPL input before its check: the earlier inputs'
    /// bindings and nothing of its own.
    #[must_use]
    pub fn after(
        earlier: HashMap<Symbol, InputBinding>,
        declared_by: HashMap<Symbol, String>,
    ) -> ModuleInit {
        ModuleInit {
            earlier,
            declared_by,
            ..ModuleInit::default()
        }
    }
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

/// The statement of a REPL input that is one expression, whose value the
/// REPL shows (`cli.repl.value`): the input's only top-level item, an
/// expression statement that binds nothing. An input of declarations or
/// other statements shows nothing of its own.
fn shown_statement<'t>(cx: &BodyCx<'_>, stmts: &[NodeRef<'t>]) -> Option<NodeRef<'t>> {
    let [s] = stmts else { return None };
    let only = cx.src.root().children().count() == 1;
    let binds = s
        .children()
        .next()
        .is_none_or(|e| e.kind() == SyntaxKind::BindingExpr);
    (only && s.kind() == SyntaxKind::ExprStmt && !binds).then_some(*s)
}

/// The global that holds one variant's shared field (`data.shared.not-stored`).
#[must_use]
pub fn shared_global(names: &hd_resolve::Names<'_>, variant: DefId, field: &str) -> DefId {
    names.member(variant, hd_intern::PathKind::Hidden, field)
}

/// Whose top level [`check_init`] checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopLevel {
    /// A module that is not the entry: requirement-free
    /// (`module.init.requirement-free`).
    Module,
    /// A program's entry module: an inferred entry row
    /// (`module.init.script-row`).
    Entry,
    /// A REPL input: an inferred row too, and a driver context, as
    /// `main!`'s body is, so it may make bang calls. An input that is one
    /// expression shows its value (`cli.repl.value`).
    Input,
}

/// Checks a module's top-level statements as its `Init` body. Returns
/// the body and each statement's summary: its facts and its first
/// instruction.
pub fn check_init(
    cx: &BodyCx<'_>,
    item: DefId,
    module: &str,
    stmts: &[NodeRef<'_>],
    top: TopLevel,
    diags: &mut DiagBuf,
) -> StageResult<(Body, Vec<StmtSummary>)> {
    let pool = cx.names.pool;
    let entry = top != TopLevel::Module;
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
    let local = hd_types::LocalPool::new();
    let mut ck = new_ck(
        cx,
        &local,
        item,
        item,
        BodyKind::Init,
        (Ty::VOID, row),
        diags,
    );
    ck.module_init = Some(module.to_owned());
    ck.forbidden = (!entry).then_some("a module's initialization");
    if top == TopLevel::Input {
        ck.suspends = vec![true];
        ck.shown = shown_statement(cx, stmts).map(|s| (s.index(), None));
    }
    let blk = ck.b.open_block();
    // A body that M1 checks for a statement may read a later binding. An
    // annotated one has its type before its statement runs, so that read
    // is a read before initialization (`module.init.definite`), not an
    // unknown name.
    for (i, s) in stmts.iter().enumerate() {
        ck.declare_annotated(i, *s);
        if let Some(name) = ck.binding_name(*s) {
            ck.later_names.entry(name).or_insert(i);
        }
    }
    let mut per_stmt = Vec::new();
    for (i, s) in stmts.iter().enumerate() {
        ck.init_stmt = i;
        let start = ck.infer.var_count();
        let first_inst = u32::try_from(ck.b.body_mut().tags.len()).expect("insts");
        ck.top_level(*s).map_err(|e| e.at(cx.src.span(*s)))?;
        // A binding takes its initializer's type, and no later statement
        // changes it (`types.literal.local.statement`).
        ck.close_literals(start);
        if let Some((_, Some((r, t)))) = ck.shown.take() {
            ck.repl_value(r, t, *s);
        }
        let span = cx.src.span(*s);
        per_stmt.push(StmtSummary {
            first_inst,
            lo: span.lo,
            hi: span.hi,
            facts: std::mem::take(&mut ck.facts),
        });
        // The statement's bindings as another body reads them: a body M1
        // checks for a call further down sees the types decided so far.
        let pool = ck.pool();
        for g in cx.init.borrow_mut().globals.values_mut() {
            if g.stmt != i {
                continue;
            }
            let t = ck.infer.resolve(pool, g.ty);
            if !t.is_local() {
                g.ty = t;
            }
        }
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
    /// The bindings that may be uninitialized after `cond` is true and
    /// after it is false; one state for both when `cond` is not a
    /// short-circuit chain.
    pub(crate) fn take_cond_flow(&mut self, cond: NodeRef<'_>) -> (Set, Set) {
        match self.cond_flow.take() {
            Some(f) if f.node == cond.index() => (f.on_true, f.on_false),
            _ => (self.uninit.clone(), self.uninit.clone()),
        }
    }

    /// `l && r` and `l || r`: `r` runs only on the path where `l` is
    /// true (`&&`) or false (`||`), so the bindings `r` declares in the
    /// enclosing scope are skipped on the other path
    /// (`names.definite.skipped`).
    pub(crate) fn short_circuit(
        &mut self,
        n: NodeRef<'_>,
        (l, r): (NodeRef<'_>, NodeRef<'_>),
        and: bool,
        check_rhs: impl FnOnce(&mut Self) -> StageResult<()>,
    ) -> StageResult<()> {
        let (lt, lf) = self.take_cond_flow(l);
        self.uninit = if and { lt.clone() } else { lf.clone() };
        let (mark, depth) = (self.bound.len(), self.scopes.len());
        check_rhs(self)?;
        let (rt, rf) = self.take_cond_flow(r);
        let skipped: Set = self.bound[mark..]
            .iter()
            .filter(|(_, d)| d + 1 == depth)
            .map(|(l, _)| *l)
            .collect();
        let union = |sets: [&Set; 3]| sets.into_iter().flatten().copied().collect::<Set>();
        let (on_true, on_false) = if and {
            (rt, union([&lf, &skipped, &rf]))
        } else {
            (union([&lt, &skipped, &rt]), rf)
        };
        self.uninit = on_true.union(&on_false).copied().collect();
        self.cond_flow = Some(CondFlow {
            node: n.index(),
            on_true,
            on_false,
        });
        Ok(())
    }

    /// Declares statement `i`'s binding ahead of its statement when it is
    /// a simple `let` with a type annotation that resolves without error;
    /// the statement declares it again when it runs.
    /// The name a top-level statement binds, for a plain `name` pattern.
    fn binding_name(&mut self, s: NodeRef<'_>) -> Option<Symbol> {
        let first = s.children().next()?;
        let pat = match s.kind() {
            SyntaxKind::LetStmt => first,
            SyntaxKind::ExprStmt if first.kind() == SyntaxKind::BindingExpr => {
                first.children().next()?
            }
            _ => return None,
        };
        if pat.kind() != SyntaxKind::BindingPattern || pat.children().next().is_some() {
            return None;
        }
        let nt = pat.direct_tokens().find(|t| {
            matches!(
                self.cx.src.tkind(*t),
                Some(TokenKind::Ident | TokenKind::RawIdent)
            )
        })?;
        let text = self.cx.src.text(nt).to_owned();
        Some(self.cx.names.syms.intern(&text))
    }

    fn declare_annotated(&mut self, i: usize, s: NodeRef<'_>) {
        if s.kind() != SyntaxKind::LetStmt {
            return;
        }
        let kids: Vec<NodeRef<'_>> = s.children().collect();
        let (Some(pat), Some(tn)) = (
            kids.first().copied(),
            kids.iter().copied().find(|k| k.kind().is_type()),
        ) else {
            return;
        };
        if pat.kind() != SyntaxKind::BindingPattern || pat.children().next().is_some() {
            return;
        }
        let Some(nt) = pat.direct_tokens().find(|t| {
            matches!(
                self.cx.src.tkind(*t),
                Some(TokenKind::Ident | TokenKind::RawIdent)
            )
        }) else {
            return;
        };
        // Its statement resolves the annotation again and reports its
        // errors once.
        let mark = self.diags.mark();
        let ty = self.ty_node(tn);
        let failed = self.diags.errors_since(mark) > 0;
        self.diags.truncate(mark);
        let Ok(ty) = ty else { return };
        if failed || ty.is_local() {
            return;
        }
        let text = self.cx.src.text(nt).to_owned();
        let name = self.cx.names.syms.intern(&text);
        let module = self.module_init.clone().unwrap_or_default();
        let def = self.cx.names.item(&module, &text);
        self.cx.init.borrow_mut().globals.insert(
            name,
            Global {
                def,
                ty,
                stmt: i,
                at: self.cx.src.span(s).lo,
                short: false,
            },
        );
    }

    /// A top-level statement: a simple binding becomes a global.
    fn top_level(&mut self, s: NodeRef<'_>) -> StageResult<()> {
        match s.kind() {
            SyntaxKind::EnumDecl => return self.shared_init(s),
            // `flow.return.outside`, `flow.defer.outside`.
            SyntaxKind::ReturnStmt => {
                self.err(
                    Code::ReturnOutsideFunction,
                    s,
                    "`return` needs a function or closure",
                );
                return Ok(());
            }
            SyntaxKind::DeferStmt => {
                self.err(
                    Code::DeferOutsideCleanupScope,
                    s,
                    "a module's top level is not a cleanup scope",
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
            self.stmt(s, false)?;
            return Ok(());
        };
        self.top_binding(s, pat, ty_node, rhs).map(|_| ())
    }

    /// One top-level binding: a global set from its initializer. A chain
    /// declares its inner binding first and returns the global it declared.
    fn top_binding(
        &mut self,
        s: NodeRef<'_>,
        pat: NodeRef<'_>,
        ty_node: Option<NodeRef<'_>>,
        rhs: NodeRef<'_>,
    ) -> StageResult<(DefId, Ty)> {
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
        self.check_prelude_shadow(name, pat);
        let by = self.cx.init.borrow().declared_by.get(&name).cloned();
        if let Some(by) = by {
            let msg = format!(
                "`{text}` is already declared by {by}; give it another name, or bind a `let` and assign it"
            );
            self.err(Code::DuplicateBinding, pat, &msg);
        }
        let chain = (s.kind() == SyntaxKind::ExprStmt && rhs.kind() == SyntaxKind::BindingExpr)
            .then(|| {
                let mut inner = rhs.children();
                inner.next().zip(inner.next())
            })
            .flatten();
        let annot = match ty_node {
            Some(t) => Some(self.ty_node(t)?),
            None => None,
        };
        // `grammar.stmt.chain`: `a := b := e` binds `b` to `e`, then `a` to
        // the value of that binding, in order.
        let (r, t) = match chain {
            Some((inner_pat, inner_rhs)) => {
                let (def, ty) = self.top_binding(s, inner_pat, None, inner_rhs)?;
                let a = self.b.refs_record(&[Ref(def.raw())]);
                (self.b.emit(Tag::GlobalGet, a, NONE, ty, rhs.index()), ty)
            }
            None => self.expr(rhs, annot)?,
        };
        let (r, t) = match annot {
            Some(a) => (
                self.coerce(r, t, a, rhs, "binding", ExpectPosition::General),
                a,
            ),
            None => (r, t),
        };
        // The binding forms' views (types.bind.short, types.bind.let-mut-infer).
        let t = if pat
            .direct_tokens()
            .any(|t| self.cx.src.tkind(t) == Some(TokenKind::KwMut))
        {
            self.let_mut_binding(t, annot.is_some(), pat);
            t
        } else if annot.is_none() {
            self.readonly_view(t)
        } else {
            t
        };
        let module = self.module_init.clone().unwrap_or_default();
        let def = self.cx.names.item(&module, &text);
        self.cx.init.borrow_mut().globals.insert(
            name,
            Global {
                def,
                ty: t,
                stmt: self.init_stmt,
                at: self.cx.src.span(s).lo,
                short: s.kind() == SyntaxKind::ExprStmt,
            },
        );
        let a = self.b.refs_record(&[Ref(def.raw())]);
        self.b.emit(Tag::GlobalSet, a, r.0, Ty::VOID, s.index());
        Ok((def, t))
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
        if shared.iter().any(|f| self.pool().has_param(f.ty)) {
            return unsupported("shared data whose type names a generic parameter");
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
                let msg = format!("variant `{vname}` gives no shared data of `{ename}`");
                self.err(Code::MissingRequiredField, v, &msg);
                continue;
            };
            let owner = self.cx.src.text(self.cx.src.first(sd)).to_owned();
            if owner != ename {
                let msg = format!("`{owner}` is not the enclosing enum `{ename}`");
                self.err(Code::VariantResultOwner, sd, &msg);
                continue;
            }
            let al = sd.children().find(|c| c.kind() == SyntaxKind::ArgumentList);
            let args = self.args_of(al);
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
        if args.spread.is_some() {
            return unsupported("a spread argument for a shared variant parameter");
        }
        let mut slots: Vec<Option<Ref>> = vec![None; params.len()];
        if args.positional.len() > params.len() {
            let msg = format!("`{ename}` takes {} arguments", params.len());
            self.err(Code::ArgumentCount, at, &msg);
        }
        for (i, e) in args.positional.iter().enumerate() {
            let w = params.get(i).map(|p| p.1);
            let (r, t) = self.expr(*e, w)?;
            if let (Some(w), Some(s)) = (w, slots.get_mut(i)) {
                *s = Some(self.coerce(r, t, w, *e, "argument", ExpectPosition::General));
            }
        }
        for (pname, e) in &args.named {
            let Some(i) = params
                .iter()
                .position(|p| self.cx.names.text(p.0) == pname.as_str())
            else {
                let msg = format!("`{ename}` has no parameter `{pname}`");
                self.err(Code::UnknownNamedArgument, *e, &msg);
                continue;
            };
            if slots[i].is_some() {
                let msg = format!("`{pname}` is given twice");
                self.err(Code::DuplicateArgument, *e, &msg);
                continue;
            }
            let (r, t) = self.expr(*e, Some(params[i].1))?;
            slots[i] =
                Some(self.coerce(r, t, params[i].1, *e, "argument", ExpectPosition::General));
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
                let msg = format!("`{ename}` needs `{}`", self.cx.names.text(params[i].0));
                self.err(Code::MissingRequiredField, at, &msg);
                return Ok(Vec::new());
            };
            self.scopes.push(HashMap::new());
            for (k, r) in out.iter().enumerate() {
                // A shared constructor parameter is a field, not a local.
                self.check_local_name(params[k].0, e);
                let l = self.b.local(
                    params[k].1,
                    params[k].0,
                    hd_tir::ir::local_flags::ASSIGNED,
                    e.index(),
                );
                self.scopes
                    .last_mut()
                    .expect("scope")
                    .insert(params[k].0, l);
                self.b.set(l, *r, e.index());
            }
            // A shared default is requirement-free
            // (`data.shared.default.requirement-free`).
            self.rows.push(RowFrame::Declared(RowId::EMPTY));
            let saved = self.default_row.replace(false);
            let checked = self.expr(e, Some(params[i].1));
            self.default_row = saved;
            self.rows.pop();
            let (r, t) = checked?;
            self.scopes.pop();
            out.push(self.coerce(r, t, params[i].1, e, "default", ExpectPosition::General));
        }
        Ok(out)
    }

    /// The shared parameter of an enum type that `name` selects: its
    /// stored name and type. An unnamed parameter is selected as `_N` and
    /// is stored under its position `N` (`data.shared.parameters`).
    pub(crate) fn shared_param(&self, t: Ty, name: &str) -> Option<(Symbol, Ty)> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let hd_types::TyData::Adt { def, .. } = pool.get(t) else {
            return None;
        };
        let ItemData::Enum { shared, .. } = &self.cx.lookup.item(def)?.data else {
            return None;
        };
        let sym = self.cx.names.syms.intern(name);
        let position = name
            .strip_prefix('_')
            .filter(|d| d.parse::<usize>().is_ok())
            .map(|d| self.cx.names.syms.intern(d));
        shared
            .iter()
            .find(|f| f.name == sym)
            .or_else(|| shared.iter().find(|f| Some(f.name) == position))
            .map(|f| (f.name, f.ty))
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
        let Some((field, ft)) = self.shared_param(t, name) else {
            return Ok(None);
        };
        let pool = self.pool();
        let hd_types::TyData::Adt { def, .. } = pool.get(t) else {
            return Ok(None);
        };
        let Some(item) = self.cx.lookup.item(def) else {
            return Ok(None);
        };
        let ItemData::Enum { variants, .. } = &item.data else {
            return Ok(None);
        };
        let name = self.cx.names.text(field).to_owned();
        let name = name.as_str();
        if self.pool().has_param(ft) {
            return unsupported("a shared field whose type names a generic parameter");
        }
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
        let (own, earlier) = {
            let init = self.cx.init.borrow();
            (
                init.globals.get(&name).copied(),
                init.earlier.get(&name).copied(),
            )
        };
        // A statement, or a closure it creates, that names a binding a
        // later statement declares reads it before its initialization
        // (`module.init.definite`); the binding has no type yet.
        if self.module_init.is_some()
            && own.is_none_or(|g| g.stmt > self.init_stmt)
            && self
                .later_names
                .get(&name)
                .is_some_and(|s| *s > self.init_stmt)
        {
            let msg = format!(
                "`{}` is bound by a later statement, so this statement reads it before its initialization",
                self.cx.names.text(name)
            );
            self.err(Code::TopLevelReadBeforeInitialization, n, &msg);
            return Some(self.poison_value(n));
        }
        if own.is_none() && earlier.is_none() {
            return None;
        }
        // A template body names no top-level binding
        // (`module.package.template-private.no-binding`).
        if self.in_template.is_some() {
            let msg = format!(
                "a template body cannot name the top-level binding `{}`",
                self.cx.names.text(name)
            );
            self.err(Code::TemplateNamesBinding, n, &msg);
            return Some(self.poison_value(n));
        }
        let Some(g) = own else {
            // An earlier REPL input's binding (`cli.repl.declarations.bindings`)
            // is initialized already: any statement or body sees it, and
            // reading it orders nothing in this module's initialization.
            let g = earlier?;
            let a = self.b.refs_record(&[Ref(g.def.raw())]);
            return Some((self.b.emit(Tag::GlobalGet, a, NONE, g.ty, n.index()), g.ty));
        };
        // Inside the init body, a binding is visible from its statement on.
        if self.module_init.is_some() && g.stmt >= self.init_stmt {
            return None;
        }
        // `names.exec.not-before`: a function declared before the binding
        // does not see it, though it runs later.
        if self.module_init.is_none() && self.at < g.at {
            let msg = format!(
                "`{}` is bound further down the module, so it is not visible here",
                self.cx.names.text(name)
            );
            self.err(Code::BindingNotYetVisible, n, &msg);
            return Some(self.poison_value(n));
        }
        // Read by a body M1 checks while the top level is being checked,
        // a binding whose type is still open holds the init body's
        // variables: it has no type yet.
        if self.module_init.is_none() && g.ty.is_local() {
            let msg = format!(
                "the type of `{}` is not decided before this function's result is inferred; annotate the binding",
                self.cx.names.text(name)
            );
            self.err(Code::CannotInferType, n, &msg);
            return Some(self.poison_value(n));
        }
        self.facts.reads.insert(g.def);
        let a = self.b.refs_record(&[Ref(g.def.raw())]);
        Some((self.b.emit(Tag::GlobalGet, a, NONE, g.ty, n.index()), g.ty))
    }

    /// Records a call or reference of a same-module function. A call of a
    /// trait method reaches every implementation of it in the module
    /// (`module.init.definite.dispatch`).
    pub(crate) fn note_call(&mut self, def: DefId) {
        self.facts.calls.insert(def);
        let lookup = self.cx.lookup;
        let Some(item) = lookup.item(def) else { return };
        let ItemData::Method { owner, .. } = &item.data else {
            return;
        };
        if matches!(
            lookup.item(*owner).map(|o| &o.data),
            Some(ItemData::Trait(_))
        ) {
            self.note_dispatch(*owner, Some(item.name));
        }
    }

    /// Every method of the module's impls of `trait_`, or only those named
    /// `name`: what a call through a bound, a trait value or an
    /// implementation that is not known reaches.
    fn note_dispatch(&mut self, trait_: DefId, name: Option<Symbol>) {
        let cx = self.cx;
        let mut init = cx.init.borrow_mut();
        let by_trait = init.impl_methods.get_or_insert_with(|| {
            let mut by_trait: HashMap<DefId, Vec<(Symbol, DefId)>> = HashMap::new();
            for it in cx.lookup.own {
                let ItemData::Method { owner, .. } = &it.data else {
                    continue;
                };
                if let Some(ItemData::Impl { trait_, .. }) = cx.lookup.item(*owner).map(|i| &i.data)
                    && *trait_ != DefId::NONE
                {
                    by_trait.entry(*trait_).or_default().push((it.name, it.def));
                }
            }
            by_trait
        });
        for (n, d) in by_trait.get(&trait_).into_iter().flatten() {
            if name.is_none_or(|x| x == *n) {
                self.facts.calls.insert(*d);
            }
        }
    }

    /// Records that a body took the implementation `ev` of `trait_`, which
    /// calls its methods now, as an operator, an interpolation, a `for`
    /// or a `?` does, or later, as a generic function does with a bound
    /// (`module.init.definite.implicit`). `None`: not decided yet. A
    /// written impl of this module is reached by name; an implementation
    /// that is not known reaches every impl of the traits it needs.
    pub(crate) fn note_impl_use(&mut self, trait_: DefId, ev: Option<&Evidence>) {
        let Some(Evidence::Impl { row, .. }) = ev else {
            if !matches!(ev, Some(Evidence::Builtin(_) | Evidence::Poison)) {
                self.note_dispatch(trait_, None);
            }
            return;
        };
        let Some(table) = self.cx.impl_table(row.module) else {
            return self.note_dispatch(trait_, None);
        };
        let at = row.row as usize;
        if row.module == OWN_TABLE
            && let Some(ItemData::Impl {
                kind: ImplKind::Written | ImplKind::Error,
                methods,
                ..
            }) = self.cx.lookup.item(table.def[at]).map(|i| &i.data)
        {
            self.facts.calls.extend(methods.iter().map(|m| m.1));
            return;
        }
        // An impl of another module has no body here. It reaches this
        // module only through the types it is generic over.
        if row.module != OWN_TABLE && table.plan[at].is_empty() {
            return;
        }
        self.note_dispatch(trait_, None);
        for step in &table.plan[at] {
            let (PlanStep::Bound { trait_, .. } | PlanStep::Bind { trait_, .. }) = step;
            self.note_dispatch(*trait_, None);
        }
    }
}

/// `module.init.definite`: each top-level statement's transitive read set
/// (through the module's call graph) holds only bindings initialized by
/// an earlier statement. Reports `top-level-read-before-initialization`.
pub fn definite_init(
    cx: &BodyCx<'_>,
    stmts: &[NodeRef<'_>],
    per_stmt: &[StmtSummary],
    diags: &mut DiagBuf,
) {
    let init = cx.init.borrow();
    let at: HashMap<DefId, usize> = init.globals.values().map(|g| (g.def, g.stmt)).collect();
    // The latest statement among the bindings each function reads,
    // through its calls: its callees' values pushed to the callers, so a
    // statement is tested without walking the graph again.
    let mut latest: HashMap<DefId, usize> = HashMap::new();
    let mut callers: HashMap<DefId, Vec<DefId>> = HashMap::new();
    let mut work = Vec::new();
    for (d, f) in &init.facts {
        if let Some(m) = f.reads.iter().filter_map(|g| at.get(g)).max() {
            latest.insert(*d, *m);
            work.push(*d);
        }
        for c in &f.calls {
            callers.entry(*c).or_default().push(*d);
        }
    }
    while let Some(d) = work.pop() {
        let v = latest[&d];
        for c in callers.get(&d).into_iter().flatten() {
            if latest.get(c).is_none_or(|x| *x < v) {
                latest.insert(*c, v);
                work.push(*c);
            }
        }
    }
    for (i, f) in per_stmt.iter().map(|s| &s.facts).enumerate() {
        let worst = f
            .reads
            .iter()
            .filter_map(|g| at.get(g))
            .chain(f.calls.iter().filter_map(|c| latest.get(c)))
            .max();
        if worst.is_none_or(|w| *w < i) {
            continue;
        }
        // Only an error names the binding: the earliest one, by statement.
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
            let msg = format!("this statement reads `{late}` before its initialization");
            diags.error(
                Code::TopLevelReadBeforeInitialization,
                cx.src.span(*s),
                &msg,
            );
        }
    }
}
