//! A syntactic review screen for std Drop hooks, not an effect/provenance proof.
//! It checks direct pointer-field writes/calls and direct raw-memory writes.
//! Helpers and raw ownership still require the documented std invariant audit.
use std::fmt::Write as _;

use hd_base::{HashMap, HashSet, TokenIdx};
use hd_project::SourceSet;
use hd_syntax::{ImplDecl, NodeRef, Parse, SyntaxKind, TokenKind};

fn text<'a>(source: &'a str, parsed: &Parse, node: NodeRef<'_>) -> &'a str {
    let (first, last) = parsed.tree.span_tokens(node.index());
    let (lo, _) = parsed.tokens.span(first);
    let (_, hi) = parsed.tokens.span(last);
    &source[lo as usize..hi as usize]
}

fn name<'a>(source: &'a str, parsed: &Parse, node: NodeRef<'_>) -> Option<&'a str> {
    node.name(&parsed.tokens)
        .map(|token| parsed.tokens.text(token, source))
}

fn self_field<'a>(source: &'a str, parsed: &Parse, node: NodeRef<'_>) -> Option<&'a str> {
    if node.kind() != SyntaxKind::FieldExpr {
        return None;
    }
    let root = node.children().next()?;
    (root.kind() == SyntaxKind::NameExpr && text(source, parsed, root) == "self")
        .then(|| name(source, parsed, node))
        .flatten()
}

#[derive(Default)]
struct Screen {
    findings: Vec<usize>,
    hooks: usize,
    work: usize,
}

fn screen(source: &str) -> Screen {
    let parsed = hd_syntax::parse(source.as_bytes());
    assert!(parsed.is_ok(), "the source screen requires parsed HD");
    let root = parsed.tree.root();
    let mut fields = HashMap::default();
    let mut out = Screen::default();
    let mut drop_names = HashSet::default();
    drop_names.insert("Drop");
    for import in root.children() {
        out.work += 1;
        if import.kind() != SyntaxKind::UseDecl {
            continue;
        }
        let (first, last) = parsed.tree.span_tokens(import.index());
        for token in first.raw()..=last.raw() {
            out.work += 1;
            if token + 2 <= last.raw()
                && parsed.tokens.text(TokenIdx::from_raw(token), source) == "Drop"
                && parsed.tokens.text(TokenIdx::from_raw(token + 1), source) == "as"
            {
                drop_names.insert(parsed.tokens.text(TokenIdx::from_raw(token + 2), source));
            }
        }
    }
    // Each declaration/type belongs to one top-level struct; no body rescan.
    for structure in root.children() {
        out.work += 1;
        if structure.kind() != SyntaxKind::StructDecl {
            continue;
        }
        let mut pointers = HashMap::default();
        for field in structure.descendants() {
            out.work += 1;
            if field.kind() != SyntaxKind::DataField {
                continue;
            }
            let Some(ty) = field.children().find(|n| n.kind().is_type()) else {
                continue;
            };
            let (first, last) = parsed.tree.span_tokens(ty.index());
            let mut tokens = (first.raw()..=last.raw())
                .map(|index| parsed.tokens.kind(TokenIdx::from_raw(index)))
                .filter(|kind| *kind != TokenKind::LParen);
            let counted_mutable =
                tokens.next() == Some(TokenKind::Star) && tokens.next() == Some(TokenKind::KwMut);
            let ty = text(source, &parsed, ty).trim_start_matches('(').trim();
            // A borrowed collection's lifetime/exclusivity excludes the live
            // snapshot. A counted pointer/raw Ptr field has no such contract.
            let pointer = !ty.starts_with('&')
                && (counted_mutable || ty.starts_with("Ptr[") || ty.contains(".Ptr["));
            if let Some(field) = name(source, &parsed, field) {
                pointers.insert(field, pointer);
            }
            out.work += 1;
        }
        if let Some(structure) = name(source, &parsed, structure) {
            fields.insert(structure, pointers);
        }
    }
    let mut pointer = vec![false; parsed.tree.len()];
    let mut receiver_field = vec![false; parsed.tree.len()];
    out.work += 2 * parsed.tree.len();
    for implementation in root.children() {
        out.work += 1;
        if implementation.kind() != SyntaxKind::ImplDecl {
            continue;
        }
        let (Some(trait_), Some(target)) = ImplDecl::cast(implementation).unwrap().header_types()
        else {
            continue;
        };
        if !text(source, &parsed, trait_)
            .rsplit('.')
            .next()
            .is_some_and(|name| drop_names.contains(name))
        {
            continue;
        }
        out.hooks += 1;
        let target = text(source, &parsed, target)
            .split('[')
            .next()
            .unwrap()
            .trim();
        let declared = fields.get(target);
        // Reverse preorder computes expression flags once. It does not chase
        // values, calls, aliases or pointers, and adds no compiler assumption.
        let nodes: Vec<_> = implementation.descendants().collect();
        out.work += nodes.len();
        for node in nodes.into_iter().rev() {
            out.work += 1;
            let at = node.index().idx();
            if let Some(field) = self_field(source, &parsed, node) {
                receiver_field[at] = true;
                pointer[at] = declared
                    .and_then(|fields| fields.get(field))
                    .copied()
                    .unwrap_or(false);
            }
            for child in node.children() {
                out.work += 1;
                pointer[at] |= pointer[child.index().idx()];
                receiver_field[at] |= receiver_field[child.index().idx()];
            }
            let flagged = match node.kind() {
                SyntaxKind::AssignmentStmt => node.children().next().is_some_and(|lhs| {
                    // Replacing a field itself writes the receiver's own
                    // storage, unlike writing through the field's pointer.
                    pointer[lhs.index().idx()] && self_field(source, &parsed, lhs).is_none()
                }),
                SyntaxKind::CallExpr => {
                    let callee = node.children().next().unwrap();
                    let operation = name(source, &parsed, callee).unwrap_or("");
                    let raw_write = matches!(
                        operation,
                        "store_u8"
                            | "store_u16"
                            | "store_u32"
                            | "store_u64"
                            | "copy"
                            | "copy_bytes"
                            | "fill"
                            | "write"
                    );
                    let destination = if operation == "write" {
                        Some(callee)
                    } else {
                        node.children()
                            .find(|child| child.kind() == SyntaxKind::ArgumentList)
                            .and_then(|arguments| arguments.children().next())
                    };
                    pointer[at]
                        || raw_write
                            && destination.is_some_and(|value| receiver_field[value.index().idx()])
                }
                _ => false,
            };
            if flagged {
                let (first, _) = parsed.tree.span_tokens(node.index());
                out.findings.push(parsed.tokens.span(first).0 as usize);
            }
        }
    }
    out.findings.reverse();
    out
}

#[test]
fn std_drop_hooks_pass_the_pointer_write_screen() {
    let sources = hd_driver::std_sources();
    let mut hooks = 0;
    for entry in sources.list() {
        let bytes = sources.read(&entry.path).unwrap();
        let source = std::str::from_utf8(&bytes).unwrap();
        let screened = screen(source);
        hooks += screened.hooks;
        assert!(
            screened.findings.is_empty(),
            "{}: review Drop pointer writes at byte offsets {:?} against module.rt.drop.snapshot",
            entry.path,
            screened.findings
        );
    }
    assert!(
        hooks >= 7,
        "the screen must visit the existing runtime hooks"
    );
}

#[test]
fn pointer_write_screen_distinguishes_own_borrowed_and_foreign_storage() {
    for (field, body, flagged) in [
        ("*mut Counter", "self.target.value += 1", true),
        ("*mut Counter", "self.target.clear()", true),
        ("*mut Counter", "change(self.target)", true),
        ("Ptr[usize]", "self.target.write(1)", true),
        ("usize", "store_u32(self.target + 4, 1u32)", true),
        ("usize", "self.target += 1", false),
        ("usize", "free_room(self.target)", false),
        ("usize", "copy_bytes(target, source, self.target)", false),
        ("&'a mut List[usize]", "self.target.push(1)", false),
        ("usize", "counter += 1", false),
    ] {
        let source = format!(
            "struct Resource['a]:\n    target: {field}\nimpl['a] Drop for Resource['a]:\n    fn drop(&mut self) -> void:\n        {body}\n"
        );
        assert_eq!(!screen(&source).findings.is_empty(), flagged, "{body}");
    }
}

#[test]
fn pointer_write_screen_follows_a_renamed_drop_import() {
    let source = "use std.rt.Drop as Release\nstruct Resource:\n    target: * mut Counter\nimpl Release for Resource:\n    fn drop(&mut self) -> void:\n        self.target.value += 1\n";
    let checked = screen(source);
    assert_eq!(checked.hooks, 1);
    assert_eq!(checked.findings.len(), 1);
}

#[test]
fn pointer_write_screen_has_linear_width_and_depth_work() {
    let run = |width: usize, depth: usize| {
        let mut source = String::from(
            "struct Resource:\n    target: *mut Counter\nimpl Drop for Resource:\n    fn drop(&mut self) -> void:\n",
        );
        let path = format!("self.target{}", ".field".repeat(depth));
        for _ in 0..width {
            writeln!(source, "        {path} += 1").unwrap();
        }
        let screened = screen(&source);
        assert_eq!(screened.findings.len(), width);
        let nodes = hd_syntax::parse(source.as_bytes()).tree.len();
        assert!(
            screened.work <= 7 * nodes,
            "{} visits for {nodes} nodes",
            screened.work
        );
        eprintln!(
            "std Drop screen {width}x{depth}: {} visits / {nodes} nodes",
            screened.work
        );
        screened.work
    };
    assert!(run(1000, 20) <= 12 * run(100, 20));
    run(20, 1000);
}

#[test]
fn deliberately_broken_drop_invariant_has_a_checked_boundary_fixture() {
    use hd_cache::MemoryStore;
    use hd_driver::{
        BuildProfile, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages,
    };
    use hd_sched::SerialOrder;

    let source = include_str!("std_drop_snapshot_boundary.hd");
    assert!(
        screen(source).findings.len() >= 2,
        "the intentional violation must be flagged"
    );
    let mut sources = hd_driver::std_sources();
    sources.insert("main.hd", source);
    let store = MemoryStore::default();
    let host = Host {
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
        render_tir: &[],
    };
    let out = build_packages(
        &host,
        "std",
        &Packages {
            build_profile: BuildProfile::Release,
            level: OptLevel::Optimized,
            checked: true,
            verify_own: true,
            std_leaf: Some("main.hd"),
            ..Packages::default()
        },
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    // ruled out: successful Program compilation emits its Wasm program.
    let wasm = out.wasm.expect("boundary program emitted");
    // Cost: one fixed checked std program, one Node process via the existing runner.
    let run = super::release_profile_run::run_wasm("drop-invariant-boundary", &wasm);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        !run.status.success() && stderr.contains("violated Drop snapshot invariant"),
        "{stderr}"
    );
    assert!(
        stderr.contains("3739147998"),
        "checked freed-header poison is shown: {stderr}"
    );
}
