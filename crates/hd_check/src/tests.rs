//! Test registrations (spec/lang/10-modules.md, "Test Cases"; checking-and-
//! tir.md §4.13.9): each registration call in test position, in a `tests:`
//! block or at the top level of a test module or an integration test
//! module, is listed by its literal name and options, and each `it` body,
//! a trailing block or an explicit closure that the case calls, is checked
//! as a `TestCase` body of its own synthesized function item, so
//! collection and emission treat it as a root like `main`. An `it_each`, `it_prop` or `it_prop_with` call is checked
//! against its declaration in `lib/std/testing.hd`, and its test case runs
//! the std body that drives the rows or the property's cases through the
//! runner (`case_runner`).

use hd_base::HashSet;

use hd_base::{DefId, StageResult};
use hd_diag::{Code, DiagBuf};
use hd_resolve::{FnSig, Item, ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::Body;
use hd_tir::ir::{BodyKind, Ref, TirSink};
use hd_types::solver::TraitRef;
use hd_types::{RowData, RowId, Ty, TyData, TyList};

use crate::body::{BodyCx, Ck, ExpectPosition, RowFrame, is_out_of_fuel, new_ck, too_complex};
use crate::call::Args;
use crate::expr::literal_text;

/// The stable panic categories (`flow.panic.known-categories`), which an
/// `expect_panic` option must name.
pub const PANIC_CATEGORIES: [&str; 20] = [
    "assertion-failed",
    "collection-pinned",
    "conflicting-access",
    "explicit-panic",
    "fact-evaluation-failed",
    "heap-exhausted",
    "host-contract",
    "integer-overflow",
    "integer-division-by-zero",
    "invalid-shift",
    "index-out-of-bounds",
    "iterator-invalidated",
    "stack-exhausted",
    "structure-variant-mismatch",
    "suspension-competing-driver",
    "suspension-deadlock",
    "suspension-forbidden-context",
    "suspension-invalid-state",
    "suspension-reentrant-poll",
    "time-limit",
];

/// One registration in test position, in source order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TestReg {
    pub name: String,
    /// `it`, `it_each`, `it_prop` or `it_prop_with`.
    pub kind: String,
    /// The body's item name in the module (`$test0`), when it is checked.
    pub body: Option<String>,
    pub ignore: Option<String>,
    pub expect_panic: Option<String>,
    /// Why this case cannot run yet; every body form runs today.
    pub unsupported: Option<String>,
    /// The registration's byte offset in its file.
    pub at: u32,
}

/// What checking a module's `tests:` blocks gives.
#[derive(Default)]
pub struct TestsOut {
    pub bodies: Vec<Body>,
    pub items: Vec<Item>,
    pub regs: Vec<TestReg>,
}

/// The statements of a module's `tests:` block: its items other than use
/// declarations and declarations (`grammar.tests.item-forms`), which are
/// the module's test items.
#[must_use]
pub fn tests_block_statements<'t>(src: &Src<'t>) -> Vec<NodeRef<'t>> {
    hd_resolve::tests_items(src)
        .filter(|s| {
            !matches!(
                s.kind(),
                SyntaxKind::UseDecl
                    | SyntaxKind::FnDecl
                    | SyntaxKind::DataDecl
                    | SyntaxKind::EnumDecl
                    | SyntaxKind::TraitDecl
                    | SyntaxKind::TypeDecl
                    | SyntaxKind::ImplDecl
            )
        })
        .collect()
}

/// Whether a block, or a closure's body, holds a `?` outside any nested
/// closure or function (`expr.try.test.with-try.reference-values`).
pub(crate) fn has_try(n: NodeRef<'_>) -> bool {
    n.children().any(|c| match c.kind() {
        SyntaxKind::TryExpr => true,
        SyntaxKind::ClosureExpr | SyntaxKind::FnDecl => false,
        _ => has_try(c),
    })
}

/// A test registration call statement (`module.testing.position-statements`):
/// its call node, the registration function's name, and its trailing
/// block, if any.
fn registration<'t>(
    cx: &BodyCx<'_>,
    s: NodeRef<'t>,
) -> Option<(NodeRef<'t>, String, Option<NodeRef<'t>>)> {
    let e = (s.kind() == SyntaxKind::ExprStmt)
        .then(|| s.children().next())
        .flatten()?;
    let (call, block) = match e.kind() {
        SyntaxKind::TrailingCallExpr => (
            e.children().find(|c| c.kind() == SyntaxKind::CallExpr)?,
            e.children().find(|c| c.kind() == SyntaxKind::Block),
        ),
        SyntaxKind::CallExpr => (e, None),
        _ => return None,
    };
    let kind = registration_kind(cx, call.children().next()?)?;
    Some((call, kind.to_owned(), block))
}

fn unknown_registration_hint(cx: &BodyCx<'_>, s: NodeRef<'_>) -> String {
    let Some(expr) = (s.kind() == SyntaxKind::ExprStmt)
        .then(|| s.children().next())
        .flatten()
    else {
        return String::new();
    };
    let call = match expr.kind() {
        SyntaxKind::CallExpr => Some(expr),
        SyntaxKind::TrailingCallExpr => expr.children().find(|c| c.kind() == SyntaxKind::CallExpr),
        _ => None,
    };
    let Some(callee) = call.and_then(|c| c.children().next()) else {
        return String::new();
    };
    let name = cx.src.text(cx.src.first(callee));
    let sym = cx.names.syms.intern(name);
    let at = cx.src.span(callee).lo;
    if callee.kind() != SyntaxKind::NameExpr
        || cx.scope_at(at).lookup(sym).is_some()
        || hd_resolve::local_at(cx.locals, sym, at).is_some()
        || hd_resolve::test_prelude_item(&cx.names, name).is_some()
    {
        return String::new();
    }
    let import = hd_resolve::iface::std_import_hint(
        &cx.names,
        cx.lookup.ifaces.iter().flat_map(|f| &f.exports),
        name,
    );
    format!("; `{name}` is unknown or not imported{import}")
}

/// The registration function that a call's callee names, by declaration
/// identity and not by spelling (`module.testing.reg.identity`): a bare or
/// renamed name, or `module.name` through a module. `None` when the callee
/// is any other function, including a local one that happens to be named
/// `it`.
fn registration_kind(cx: &BodyCx<'_>, callee: NodeRef<'_>) -> Option<&'static str> {
    let src = &cx.src;
    let at = src.span(callee).lo;
    let scope = cx.scope_at(at);
    let def = match callee.kind() {
        SyntaxKind::NameExpr => {
            let sym = src.symbol(src.first(callee), cx.names.syms);
            if hd_resolve::local_at(cx.locals, sym, at).is_some() {
                return None;
            }
            match scope.lookup(sym) {
                Some(b) if b.kind == hd_resolve::BindingKind::Item => DefId::from_raw(b.value),
                Some(_) => return None,
                // A prelude name of test code, which registration
                // statements are (`module.prelude.test-only`).
                None => hd_resolve::test_prelude_item(&cx.names, src.text(src.first(callee)))?,
            }
        }
        SyntaxKind::FieldExpr => {
            let base = callee.children().next()?;
            if base.kind() != SyntaxKind::NameExpr {
                return None;
            }
            let sym = src.symbol(src.first(base), cx.names.syms);
            if hd_resolve::local_at(cx.locals, sym, at).is_some() {
                return None;
            }
            let b = scope.lookup(sym)?;
            if b.kind != hd_resolve::BindingKind::Module {
                return None;
            }
            let module = scope.modules.get(b.value as usize)?;
            let name = src.symbol(src.last(callee), cx.names.syms);
            cx.lookup
                .ifaces
                .iter()
                .find_map(|f| f.export(module, name))
                .map(|e| e.def)?
        }
        _ => return None,
    };
    let known = &cx.names.known;
    if def == known.it_each {
        Some("it_each")
    } else if def == known.it_prop {
        Some("it_prop")
    } else if def == known.it_prop_with {
        Some("it_prop_with")
    } else if cx
        .lookup
        .item(def)
        .and_then(|i| i.intrinsic)
        .is_some_and(|k| cx.names.text(k) == "test_case")
    {
        Some("it")
    } else {
        None
    }
}

/// Whether a top-level statement is a test registration call: in a test
/// module or an integration test module, such a statement registers a
/// test case, as in a `tests:` block (`module.testing.test-position`),
/// and is not a module initialization statement.
#[must_use]
pub fn is_registration(cx: &BodyCx<'_>, s: NodeRef<'_>) -> bool {
    registration(cx, s).is_some()
}

/// Checks the statements in test position of `module`
/// (spec/lang/10-modules.md `module.testing.*`): its top-level
/// registrations when it is test code, then its `tests:` blocks'
/// statements. `profile` holds the keys the runner binds for a case body:
/// `TestRunner` alone for a unit test case
/// (`module.testing.unit-row.test-runner`), and the test profile's host
/// traits too for an integration test case
/// (`module.testing.integration-row`).
pub fn check_tests(
    cx: &BodyCx<'_>,
    module: &str,
    stmts: &[NodeRef<'_>],
    profile: RowId,
    diags: &mut DiagBuf,
) -> StageResult<TestsOut> {
    let mut out = TestsOut::default();
    let mut seen = HashSet::default();
    // The names of the module's `it_each` calls.
    let mut tables: HashSet<String> = HashSet::default();
    let src = &cx.src;
    for s in stmts.iter().copied() {
        let Some((call, kind, block)) = registration(cx, s) else {
            diags.error(
                Code::InvalidTestStatement,
                src.span(s),
                &format!(
                    "a `tests:` block holds only test registrations{}",
                    unknown_registration_hint(cx, s)
                ),
            );
            continue;
        };
        let mut reg = TestReg {
            kind: kind.clone(),
            at: src.span(call).lo,
            ..TestReg::default()
        };
        let args = call
            .children()
            .find(|c| c.kind() == SyntaxKind::ArgumentList);
        let mut positional = Vec::new();
        let mut body_arg = None;
        let mut timeout = None;
        for a in args.iter().flat_map(|l| l.children()) {
            match a.kind() {
                SyntaxKind::Argument => positional.extend(a.children().next()),
                SyntaxKind::NamedArgument => {
                    let name = src.text(src.first(a)).to_owned();
                    let Some(e) = a.children().next() else {
                        continue;
                    };
                    let literal = |diags: &mut DiagBuf| {
                        let t = literal_text(src, e);
                        if t.is_none() {
                            let msg = format!("`{name}` takes a string literal");
                            diags.error(Code::NonLiteralTestArgument, src.span(e), &msg);
                        }
                        t
                    };
                    match name.as_str() {
                        "ignore" => reg.ignore = literal(diags),
                        "expect_panic" => {
                            reg.expect_panic = literal(diags);
                            if let Some(c) = &reg.expect_panic
                                && !PANIC_CATEGORIES.contains(&c.as_str())
                            {
                                let msg = format!("`{c}` names no panic category");
                                diags.error(Code::UnknownPanicCategory, src.span(e), &msg);
                            }
                        }
                        "timeout" => timeout = Some(e),
                        "body" => body_arg = Some(e),
                        "prop" if kind != "it" => body_arg = Some(e),
                        other if kind == "it" => {
                            let msg = format!("`{kind}` has no parameter `{other}`");
                            diags.error(Code::UnknownNamedArgument, src.span(a), &msg);
                        }
                        // The call's own check reports any other name.
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        if let Some(name) = positional.first().and_then(|n| literal_text(src, *n)) {
            reg.name = name;
        } else {
            diags.error(
                Code::NonLiteralTestArgument,
                src.span(call),
                "a test name is a string literal",
            );
            continue;
        }
        if !seen.insert(reg.name.clone()) {
            let msg = format!("`{}` is registered twice", reg.name);
            diags.error(Code::DuplicateTestName, src.span(call), &msg);
            continue;
        }
        // `module.testing.reg.row-name-clash`: no other case is named like
        // a row `name[i]` of an `it_each` call, before it or after it.
        let clash = if kind == "it_each" {
            seen.iter().find(|n| row_base(n) == Some(reg.name.as_str()))
        } else {
            row_base(&reg.name).and_then(|b| tables.get(b))
        };
        if let Some(other) = clash {
            let msg = format!(
                "`{}` and the rows of `{}` share a name",
                reg.name,
                row_base(other).unwrap_or(other)
            );
            diags.error(Code::DuplicateTestName, src.span(call), &msg);
            continue;
        }
        if kind == "it_each" {
            tables.insert(reg.name.clone());
        }
        if kind == "it" {
            // `it`'s body (`module.testing.it.form`): a trailing block,
            // `body=`, or the argument after the name.
            if positional.len() > 2 {
                let msg = "`it` takes a name and a body";
                diags.error(Code::ArgumentCount, src.span(call), msg);
                continue;
            }
            let mut given = positional.get(1).copied().into_iter().chain(body_arg);
            let body = match (block, given.next(), given.next()) {
                (Some(block), None, None) => CaseBody::Block(block),
                (None, Some(e), None) => CaseBody::Value(e),
                (None, None, None) => {
                    let msg = "`it` takes a body";
                    diags.error(Code::ArgumentCount, src.span(call), msg);
                    continue;
                }
                _ => {
                    let msg = "`body` is given twice";
                    diags.error(Code::DuplicateArgument, src.span(call), msg);
                    continue;
                }
            };
            let name = format!("$test{}", out.regs.len());
            let case = ItCase {
                name: &name,
                body,
                timeout,
            };
            let (body, item) = match check_case(cx, module, &case, profile, diags) {
                Err(e) if is_out_of_fuel(&e) => too_complex_case(cx, module, &name, call, diags)?,
                r => r?,
            };
            out.bodies.push(body);
            out.items.push(item);
            reg.body = Some(name);
        } else {
            // The body closure of the call: by name, or last in position.
            let body = body_arg.or_else(|| positional.last().copied());
            let tries = body.is_some_and(|b| b.kind() == SyntaxKind::ClosureExpr && has_try(b));
            let name = format!("$test{}", out.regs.len());
            let Some(e) = s.children().next() else {
                continue;
            };
            let call = RegCall {
                name: &name,
                expr: e,
                kind: &kind,
                tries,
                timed: timeout.is_some(),
            };
            let (body, item) = match check_registration(cx, module, &call, profile, diags) {
                Err(err) if is_out_of_fuel(&err) => {
                    too_complex_case(cx, module, &name, call.expr, diags)?
                }
                r => r?,
            };
            out.bodies.push(body);
            out.items.push(item);
            reg.body = Some(name);
        }
        out.regs.push(reg);
    }
    Ok(out)
}

/// The test case `name` whose check ran out of fuel: one
/// `item-too-complex` on its registration `call` (`too_complex`), and a
/// poison body. The error fails the test run, so the case never passes.
fn too_complex_case(
    cx: &BodyCx<'_>,
    module: &str,
    name: &str,
    call: NodeRef<'_>,
    diags: &mut DiagBuf,
) -> StageResult<(Body, Item)> {
    let def: DefId = cx.names.item(module, name);
    let body = too_complex(cx, (def, BodyKind::TestCase), call, diags)?;
    let sig = FnSig {
        generics: vec![],
        params: vec![],
        defaults: vec![],
        ret: Ty::POISON,
        row: RowId::EMPTY,
        suspends: true,
        variadic: false,
    };
    let item = Item::new(def, cx.names.syms.intern(name), false, ItemData::Fn(sig));
    Ok((body, item))
}

/// An `it` call whose test case is checked: the case's item name, its
/// body, and its `timeout` argument, if it has one.
#[derive(Clone, Copy)]
struct ItCase<'t> {
    name: &'t str,
    body: CaseBody<'t>,
    timeout: Option<NodeRef<'t>>,
}

/// The body of an `it` call: a trailing block, or an explicit closure (any
/// function value) passed as `body=` or after the name.
#[derive(Clone, Copy)]
enum CaseBody<'t> {
    Block(NodeRef<'t>),
    Value(NodeRef<'t>),
}

/// One `it` body: a suspending function item with no parameters, whose
/// row is the keys of `profile` that its body uses, which the runner binds
/// for it. A trailing block is the item's body, with the fixed result
/// `void`, or `Result[void, &dyn Error]` when it uses `?`
/// (`expr.try.test.fixed-result`). An explicit closure keeps its own
/// result: the item's body calls it and returns what it returns
/// (`expr.try.test.explicit-closure`). A `timeout` argument is evaluated
/// first, in the case's instance, and reported to the runner
/// (`std-testing.option.timeout-at-run`), so the row also holds
/// `TestRunner`.
fn check_case(
    cx: &BodyCx<'_>,
    module: &str,
    case: &ItCase<'_>,
    profile: RowId,
    diags: &mut DiagBuf,
) -> StageResult<(Body, Item)> {
    let ItCase {
        name,
        body,
        timeout,
    } = *case;
    let pool = cx.names.pool;
    let def: DefId = cx.names.item(module, name);
    let (node, fixed) = match body {
        CaseBody::Block(b) => (b, case_result(cx, has_try(b))),
        CaseBody::Value(e) => (e, Ty::VOID),
    };
    let local = hd_types::LocalPool::new();
    let mut ck = new_ck(
        cx,
        &local,
        def,
        def,
        BodyKind::TestCase,
        (fixed, RowId::EMPTY),
        diags,
    );
    // The body sees what is in scope in its `tests:` block.
    ck.move_to(cx.src.span(node).lo);
    ck.suspends = vec![true];
    ck.rows = vec![RowFrame::Profile {
        row: profile,
        used: RowData::default(),
    }];
    let blk = ck.b.open_block();
    if let Some(e) = timeout {
        ck.move_to(cx.src.span(e).lo);
        let want = timeout_ty(cx);
        let (r, t) = ck.expr(e, Some(want))?;
        let r = ck.coerce(r, t, want, e, "argument", ExpectPosition::General);
        ck.report_timeout(r, e);
        ck.move_to(cx.src.span(node).lo);
    }
    let (tail, ret) = match body {
        CaseBody::Block(block) => (ck.block_value(block, Some(fixed))?.0, fixed),
        CaseBody::Value(e) => {
            let (r, t) = call_body(&mut ck, e)?;
            (Some(r), t)
        }
    };
    ck.rets[0] = ret;
    let root = ck.b.close_block(blk, tail, ret, node.index());
    let row = match ck.rows.first() {
        Some(RowFrame::Profile { used, .. }) => {
            let mut used = used.clone();
            if timeout.is_some() {
                add_key(cx, &mut used, cx.names.known.test_runner);
            }
            pool.row(&used)
        }
        _ => RowId::EMPTY,
    };
    let body = ck.finish_body(root)?;
    let sig = FnSig {
        generics: vec![],
        params: vec![],
        defaults: vec![],
        ret,
        row,
        suspends: true,
        variadic: false,
    };
    let item = Item::new(def, cx.names.syms.intern(name), false, ItemData::Fn(sig));
    Ok((body, item))
}

/// Calls an explicit test body `e`, a bang call when it suspends, and
/// gives its result `T`. The body is checked with no expected type, so it
/// keeps its written or inferred result, which must implement
/// `std.process.Termination`, the bound on `it`
/// (`expr.try.test.explicit-closure`; `unsatisfied-trait-bound`). The
/// call's row check adds the body's row to the case's.
fn call_body(ck: &mut Ck<'_, '_>, e: NodeRef<'_>) -> StageResult<(Ref, Ty)> {
    let pool = ck.pool();
    let (f, ft) = ck.expr(e, None)?;
    let ft = ck.normalize_deep(ft)?;
    let suspends = match pool.get(ck.strip_mut(ft)) {
        TyData::Fn { suspends, .. } => suspends,
        _ => false,
    };
    let none = Args {
        positional: vec![],
        spread: None,
        after_spread: vec![],
        named: vec![],
        trailing: None,
    };
    let (r, t) = ck.call_value(f, ft, &none, e, suspends)?;
    let t = ck.normalize_deep(t)?;
    let tref = TraitRef {
        trait_: ck.cx.names.item("std.process", "Termination"),
        self_ty: t,
        args: TyList::EMPTY,
    };
    ck.require_ref(tref, e)?;
    Ok((r, t))
}

/// A test body's result: `void`, or `Result[void, &dyn Error]` when it
/// uses `?` (`expr.try.test.with-try.reference-values`, `expr.try.test.without-try`).
pub(crate) fn case_result(cx: &BodyCx<'_>, tries: bool) -> Ty {
    let pool = cx.names.pool;
    if !tries {
        return Ty::VOID;
    }
    let error = pool.intern_ty(&TyData::TraitValue {
        def: cx.names.item("std.error", "Error"),
        args: TyList::EMPTY,
        bindings: vec![],
    });
    let error = pool.intern_ty(&TyData::Ref {
        inner: error,
        mutable: false,
    });
    pool.intern_ty(&TyData::Adt {
        def: cx.names.item("std.core", "Result"),
        args: pool.list(&[Ty::VOID, error]),
    })
}

/// The std body that a test case of registration function `def` runs. Its
/// generics and parameters are the registration's, so the case passes the
/// call's arguments as they are, each default after the values it reads
/// (lib/std/testing.hd; `crates/hd_tir/DESIGN.md` §4.13.9, invariant 15).
pub(crate) fn case_runner(known: &hd_resolve::KnownItems, def: DefId) -> Option<DefId> {
    if def == known.it_each {
        Some(known.each_case)
    } else if def == known.it_prop {
        Some(known.prop_case)
    } else if def == known.it_prop_with {
        Some(known.prop_with_case)
    } else {
        None
    }
}

/// The table name of a row name `name[i]` (`std-testing.it-each.name`).
fn row_base(name: &str) -> Option<&str> {
    let (base, rest) = name.rsplit_once('[')?;
    let index = rest.strip_suffix(']')?;
    (!index.is_empty() && index.bytes().all(|b| b.is_ascii_digit())).then_some(base)
}

/// A registration call whose test case is checked: the case's item name,
/// the call expression, the registration function's name, whether its
/// body closure uses `?`, and whether it has a `timeout` argument.
#[derive(Clone, Copy)]
struct RegCall<'t> {
    name: &'t str,
    expr: NodeRef<'t>,
    kind: &'t str,
    tries: bool,
    timed: bool,
}

/// The type of a registration's `timeout` option, `std.time.Duration?`
/// (`std-testing.option.timeout-any-duration`).
pub(crate) fn timeout_ty(cx: &BodyCx<'_>) -> Ty {
    let pool = cx.names.pool;
    let duration = pool.intern_ty(&TyData::Adt {
        def: cx.names.item("std.time", "Duration"),
        args: TyList::EMPTY,
    });
    pool.intern_ty(&TyData::Option(duration))
}

/// Adds the runner capability `runner` to a case's row.
fn add_key(cx: &BodyCx<'_>, used: &mut RowData, runner: DefId) {
    let key = cx.names.pool.intern_ty(&TyData::TraitValue {
        def: runner,
        args: TyList::EMPTY,
        bindings: vec![],
    });
    if !used.keys.contains(&key) {
        used.keys.push(key);
    }
}

/// The test case of an `it_each`, `it_prop` or `it_prop_with` call: a
/// suspending function item with no parameters, whose body checks the call
/// `e` against the registration function's signature and runs the std body
/// of its test case with the call's arguments (`case_runner`). Its result
/// is the registration body's, `void` or, when that body uses `?`,
/// `Result[void, &dyn Error]` (`module.testing.reg.body-closure-result.reference-values`). Its
/// row is the keys of `profile` that the body uses, and the runner
/// capability that the std body uses: `TestRunner` for a table,
/// `PropertyRunner` for a property, which the runner binds for that std
/// code alone (`std-testing.runner.binding`, `.runner.body-property`).
fn check_registration(
    cx: &BodyCx<'_>,
    module: &str,
    call: &RegCall<'_>,
    profile: RowId,
    diags: &mut DiagBuf,
) -> StageResult<(Body, Item)> {
    let RegCall {
        name,
        expr: e,
        kind,
        tries,
        timed,
    } = *call;
    let pool = cx.names.pool;
    let def: DefId = cx.names.item(module, name);
    let ret = case_result(cx, tries);
    let local = hd_types::LocalPool::new();
    let mut ck = new_ck(
        cx,
        &local,
        def,
        def,
        BodyKind::TestCase,
        (ret, RowId::EMPTY),
        diags,
    );
    ck.suspends = vec![true];
    ck.rows = vec![RowFrame::Profile {
        row: profile,
        used: RowData::default(),
    }];
    let blk = ck.b.open_block();
    ck.move_to(cx.src.span(e).lo);
    let start = ck.infer.var_count();
    ck.registering = true;
    let (r, t) = ck.expr(e, Some(ret))?;
    ck.registering = false;
    ck.close_literals(start);
    ck.close_open_params(0);
    ck.close_ref_params(0);
    let r = ck.coerce(r, t, ret, e, "result", ExpectPosition::General);
    let root = ck.b.close_block(blk, Some(r), ret, e.index());
    let mut used = match ck.rows.first() {
        Some(RowFrame::Profile { used, .. }) => used.clone(),
        _ => RowData::default(),
    };
    let known = &cx.names.known;
    let runner = if kind == "it_each" {
        known.test_runner
    } else {
        known.property_runner
    };
    add_key(cx, &mut used, runner);
    // A `timeout` is reported through `TestRunner` (`case_timeout`).
    if timed {
        add_key(cx, &mut used, known.test_runner);
    }
    let row = pool.row(&used);
    let body = ck.finish_body(root)?;
    let sig = FnSig {
        generics: vec![],
        params: vec![],
        defaults: vec![],
        ret,
        row,
        suspends: true,
        variadic: false,
    };
    let item = Item::new(def, cx.names.syms.intern(name), false, ItemData::Fn(sig));
    Ok((body, item))
}

/// Whether a source has a `tests:` block.
#[must_use]
pub fn has_tests(src: &Src<'_>) -> bool {
    src.root()
        .children()
        .any(|c| c.kind() == SyntaxKind::TestsBlock)
}
