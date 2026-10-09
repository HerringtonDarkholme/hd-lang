//! Test registrations (spec/lang/10-modules.md, "Test Cases"; checking-and-
//! tir.md §4.13.9): each registration call in test position, in a `tests:`
//! block or at the top level of a test module or an integration test
//! module, is listed by its literal name and options, and each runnable
//! `it` body is checked as a `TestCase` body of its own synthesized
//! function item, so collection and emission treat it as a root like
//! `main`.

use std::collections::HashSet;

use hd_base::{DefId, StageResult};
use hd_diag::{Code, DiagBuf};
use hd_resolve::{FnSig, Item, ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::Body;
use hd_tir::ir::{BodyKind, TirSink};
use hd_types::{RowData, RowId, Ty, TyData, TyList};

use crate::body::{BodyCx, RowFrame, new_ck};
use crate::expr::literal_text;

/// The stable panic categories (`flow.panic.stable-categories`), which an
/// `expect_panic` option must name.
pub const PANIC_CATEGORIES: [&str; 18] = [
    "assertion-failed",
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
    /// Why this case cannot run yet (property tests, timeouts: phase 2).
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

/// The statements of a module's `tests:` blocks.
#[must_use]
pub fn tests_block_statements(root: NodeRef<'_>) -> Vec<NodeRef<'_>> {
    root.children()
        .filter(|c| c.kind() == SyntaxKind::TestsBlock)
        .flat_map(hd_syntax::NodeRef::children)
        .filter(|b| b.kind() == SyntaxKind::Block)
        .flat_map(hd_syntax::NodeRef::children)
        .filter(|s| s.kind() != SyntaxKind::UseDecl)
        .collect()
}

/// Whether a block holds a `?` outside any nested closure or function
/// (`expr.try.test.with-try`).
fn has_try(n: NodeRef<'_>) -> bool {
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
    src: &Src<'_>,
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
    let kind = src.text(src.last(call.children().next()?)).to_owned();
    matches!(kind.as_str(), "it" | "it_each" | "it_prop" | "it_prop_with")
        .then_some((call, kind, block))
}

/// Whether a top-level statement is a test registration call: in a test
/// module or an integration test module, such a statement registers a
/// test case, as in a `tests:` block (`module.testing.test-position`),
/// and is not a module initialization statement.
#[must_use]
pub fn is_registration(src: &Src<'_>, s: NodeRef<'_>) -> bool {
    registration(src, s).is_some()
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
    let mut seen = HashSet::new();
    let src = &cx.src;
    for s in stmts.iter().copied() {
        let Some((call, kind, block)) = registration(src, s) else {
            diags.error(
                Code::InvalidTestStatement,
                src.span(s),
                "a `tests:` block holds only test registrations",
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
                        "timeout" => {
                            reg.unsupported = Some("the `timeout` option".into());
                        }
                        "body" => body_arg = Some(e),
                        other => {
                            let msg = format!("`{kind}` has no parameter `{other}`");
                            diags.error(Code::UnknownNamedArgument, src.span(a), &msg);
                        }
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
        if kind != "it" {
            reg.unsupported = Some(format!("`{kind}` test cases"));
        } else if body_arg.is_some() || block.is_none() {
            reg.unsupported
                .get_or_insert_with(|| "an explicit closure as a test body".into());
        }
        if reg.unsupported.is_none()
            && let Some(block) = block
        {
            let name = format!("$test{}", out.regs.len());
            let (body, item) = check_case(cx, module, &name, block, profile, diags)?;
            out.bodies.push(body);
            out.items.push(item);
            // A `?` makes the result `Result[void, dyn Error]`, and a
            // `dyn Error` value cannot be emitted yet (its vtable's type
            // is recursive): checked, but not run.
            if has_try(block) {
                reg.unsupported = Some("a test body that uses `?`".into());
            } else {
                reg.body = Some(name);
            }
        }
        out.regs.push(reg);
    }
    Ok(out)
}

/// One `it` body: a suspending function item with no parameters, whose
/// result is `void`, or `Result[void, dyn Error]` when it uses `?`
/// (`expr.try.test.with-try`), and whose row is the keys of `profile`
/// that its body uses, which the runner binds for it.
fn check_case(
    cx: &BodyCx<'_>,
    module: &str,
    name: &str,
    block: NodeRef<'_>,
    profile: RowId,
    diags: &mut DiagBuf,
) -> StageResult<(Body, Item)> {
    let pool = cx.names.pool;
    let def: DefId = cx.names.item(module, name);
    let ret = if has_try(block) {
        let error = pool.intern_ty(&TyData::TraitValue {
            def: cx.names.item("std.error", "Error"),
            args: TyList::EMPTY,
            bindings: vec![],
        });
        pool.intern_ty(&TyData::Adt {
            def: cx.names.item("std.core", "Result"),
            args: pool.list(&[Ty::VOID, error]),
        })
    } else {
        Ty::VOID
    };
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
    let (tail, _) = ck.block_value(block, Some(ret))?;
    let root = ck.b.close_block(blk, tail, ret, block.index());
    let row = match ck.rows.first() {
        Some(RowFrame::Profile { used, .. }) => pool.row(used),
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

/// Whether a source has a `tests:` block.
#[must_use]
pub fn has_tests(src: &Src<'_>) -> bool {
    src.root()
        .children()
        .any(|c| c.kind() == SyntaxKind::TestsBlock)
}
