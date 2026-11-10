//! Derivation advice over nominal, resolved declaration facts.

use crate::{Cx, Event};
use hd_base::{DefId, PathId, SliceSort as _, Span};
use hd_diag::{Code, DiagBuf, FixSafety, Severity};
use hd_resolve::facts::{BlockDecorator, DerivationFacts};
use hd_resolve::{HeadKind, ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};

pub(crate) fn unused(event: &Event<'_>, cx: &Cx<'_>, out: &mut DiagBuf) {
    let Event::Derivations(derivations) = event else {
        return;
    };
    collect_unused(derivations.facts, cx.src.text, out);
}

fn collect_unused(facts: &DerivationFacts, text: &str, out: &mut DiagBuf) {
    let mut decorators: Vec<_> = facts.decorators.iter().collect();
    decorators.sort_stable_by_key(|d| (d.span.file.raw(), d.span.lo, d.span.hi));
    decorators.dedup();
    for decorator in decorators {
        if decorator.marker || !decorator.valid {
            continue;
        }
        out.push(
            Code::UnusedDerivationFact,
            Severity::Warning,
            decorator.span,
            "no derivation reads this value; write it in the block as `Self += [...]`",
            None,
        );
        move_fix_text(out, decorator, text);
    }
}

pub(crate) fn drift(event: &Event<'_>, cx: &Cx<'_>, out: &mut DiagBuf) {
    let Event::Derivations(derivations) = event else {
        return;
    };
    let (src, decls) = (cx.src, derivations.decls);
    // One pass over the top-level declarations: each `by` block whose
    // trait and struct or enum target are single written names, as the
    // module's scope resolves them.
    let blocks: Vec<_> = src
        .root()
        .children()
        .filter(|n| {
            n.kind() == SyntaxKind::ImplDecl && n.direct_tokens().any(|t| src.text(t) == "by")
        })
        .filter_map(|n| {
            let written = written(src, n)?;
            let (target, item) = decls.item_named(&written.target_name)?;
            if !matches!(item.kind(), Some(HeadKind::Struct | HeadKind::Enum)) {
                return None;
            }
            let (trait_, item) = decls.item_named(&written.trait_name)?;
            if !matches!(item.data, ItemData::Trait(_)) {
                return None;
            }
            let block = Block {
                target,
                package: decls.package_of(trait_),
                trait_,
                span: src.span(n),
            };
            Some((block, written))
        })
        .collect();
    if blocks.is_empty() {
        return;
    }
    let derived: Vec<_> = decls
        .derived()
        .map(|(target, trait_)| Derived {
            target,
            package: decls.package_of(trait_),
            trait_,
            name: decls.display_name(trait_).to_owned(),
        })
        .collect();
    collect_drift(blocks, &derived, out);
}

/// The nominal trait and target a `by` derivation block's single written
/// names resolve to; `span` is the block's `impl` declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Block {
    target: DefId,
    package: PathId,
    trait_: DefId,
    span: Span,
}

/// An ordinary `@derive` opt-in of the module's folder.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Derived {
    target: DefId,
    package: PathId,
    trait_: DefId,
    name: String,
}

/// A derivation block's names and member lines as written.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Written {
    trait_name: String,
    target_name: String,
    /// Each member line's drift signature (`annot.line.drift`): the member
    /// it names, whether it extends (`+=`), and the right side's tokens
    /// (`pass` for an omit).
    lines: Vec<(String, bool, Vec<String>)>,
}

fn written(src: Src<'_>, n: NodeRef<'_>) -> Option<Written> {
    let name = |ty: NodeRef<'_>| {
        let mut names = ty
            .direct_tokens()
            .filter(|t| is_name(src.tkind(*t)))
            .map(|t| src.text(t));
        let name = names.next()?;
        names.next().is_none().then(|| name.to_owned())
    };
    let tys: Vec<_> = n.children().filter(|c| c.kind().is_type()).collect();
    let [trait_ty, target_ty] = tys.as_slice() else {
        return None;
    };
    let mut lines = Vec::new();
    for line in Src::child(n, SyntaxKind::Block)
        .into_iter()
        .flat_map(NodeRef::children)
        .filter(|c| c.kind() == SyntaxKind::Derivation)
    {
        let mut toks = src.tokens(line);
        let (Some(member), Some(op)) = (toks.next(), toks.next()) else {
            continue;
        };
        if !is_name(src.tkind(member)) {
            continue;
        }
        let Some(rhs) = line.children().last() else {
            continue;
        };
        let omit = rhs.kind() == SyntaxKind::LiteralExpr
            && src
                .tokens(rhs)
                .any(|t| src.tkind(t) == Some(TokenKind::KwPass));
        let rhs = if omit {
            vec!["pass".to_owned()]
        } else {
            src.tokens(rhs).map(|t| src.text(t).to_owned()).collect()
        };
        lines.push((
            src.text(member).to_owned(),
            src.tkind(op) == Some(TokenKind::PlusEq),
            rhs,
        ));
    }
    Some(Written {
        trait_name: name(*trait_ty)?,
        target_name: name(*target_ty)?,
        lines,
    })
}

const fn is_name(kind: Option<TokenKind>) -> bool {
    matches!(
        kind,
        Some(TokenKind::Ident | TokenKind::RawIdent | TokenKind::KwSelfType)
    )
}

fn collect_drift(mut blocks: Vec<(Block, Written)>, derived: &[Derived], out: &mut DiagBuf) {
    blocks.sort_stable_by_key(|(b, _)| (b.span.file.raw(), b.span.lo, b.span.hi));
    blocks.dedup();
    for (i, (block, written)) in blocks.iter().enumerate() {
        let first = blocks[..i].iter().find(|(first, _)| {
            first.span.file == block.span.file
                && first.target == block.target
                && first.package == block.package
        });
        let drift = first
            .filter(|(_, first)| first.lines != written.lines)
            .map(|(_, first)| {
                format!(
                    "`{}` configures different member lines for `{}` than `{}` does",
                    written.trait_name, written.target_name, first.trait_name
                )
            });
        let message = drift.or_else(|| {
            if written.lines.is_empty() {
                return None;
            }
            let derived = derived
                .iter()
                .filter(|derived| {
                    derived.target == block.target
                        && derived.package == block.package
                        && derived.trait_ != block.trait_
                })
                .map(|derived| derived.name.as_str())
                .min()?;
            Some(format!(
                "`{}` configures member lines for `{}` that its `@derive({derived})` does not",
                written.trait_name, written.target_name
            ))
        });
        if let Some(message) = message {
            out.push(
                Code::DerivationLineDrift,
                Severity::Warning,
                block.span,
                &message,
                None,
            );
        }
    }
}

fn move_fix_text(diags: &mut DiagBuf, fact: &BlockDecorator, text: &str) {
    let (Some(value), Some(first)) = (fact.value, fact.first) else {
        return;
    };
    let (start, end, first) = (fact.span.lo as usize, value.hi as usize, first.lo as usize);
    let Some(prefix) = text.get(..start) else {
        return;
    };
    let start = prefix.rfind('\n').map_or(0, |i| i + 1);
    let Some(suffix) = text.get(end..) else {
        return;
    };
    let line_end = suffix.find('\n').map_or(text.len(), |i| end + i);
    let newline = if text[..line_end].ends_with('\r') {
        "\r\n"
    } else {
        "\n"
    };
    let suffix = text[end..line_end].trim_end_matches('\r');
    let Some(prefix) = text.get(..first) else {
        return;
    };
    let at = prefix.rfind('\n').map_or(0, |i| i + 1);
    let indent = &text[at..first];
    if !indent.bytes().all(|b| matches!(b, b' ' | b'\t')) {
        return;
    }
    let Some(value) = text.get(value.lo as usize..value.hi as usize) else {
        return;
    };
    let member = format!("{indent}Self += [{value}]{suffix}{newline}");
    let delete = Span {
        file: fact.span.file,
        lo: u32::try_from(start).expect("source offset"),
        hi: u32::try_from((line_end + 1).min(text.len())).expect("source offset"),
    };
    let insert = Span {
        file: fact.span.file,
        lo: u32::try_from(at).expect("source offset"),
        hi: u32::try_from(at).expect("source offset"),
    };
    diags.add_fix(
        "move the fact into the derivation block as `Self += [...]`",
        FixSafety::Exact,
        &[(delete, ""), (insert, &member)],
    );
}

#[cfg(test)]
mod tests {
    use hd_base::{DefId, FileId, PathId, Span};
    use hd_diag::{Code, DiagBuf};
    use hd_resolve::facts::{BlockDecorator, DerivationFacts};

    use super::{Block, Derived, Written, collect_drift, collect_unused};

    fn span(lo: usize, hi: usize) -> Span {
        Span {
            file: FileId::from_raw(0),
            lo: u32::try_from(lo).unwrap(),
            hi: u32::try_from(hi).unwrap(),
        }
    }

    fn block(lo: u32, trait_: u32, name: &str, member: &str) -> (Block, Written) {
        (
            Block {
                target: DefId::from_raw(1),
                package: PathId::from_raw(2),
                trait_: DefId::from_raw(trait_),
                span: Span {
                    file: FileId::from_raw(0),
                    lo,
                    hi: lo + 1,
                },
            },
            Written {
                trait_name: name.to_owned(),
                target_name: "Session".to_owned(),
                lines: vec![(member.to_owned(), false, vec!["pass".to_owned()])],
            },
        )
    }

    #[test]
    fn drift_prefers_the_first_nominal_block_then_the_derive_fallback() {
        let first = block(1, 3, "Print", "cache");
        let second = block(2, 4, "Encode", "id");
        let derived = [
            Derived {
                target: first.0.target,
                package: first.0.package,
                trait_: DefId::from_raw(6),
                name: "Zed".to_owned(),
            },
            Derived {
                target: first.0.target,
                package: first.0.package,
                trait_: DefId::from_raw(7),
                name: "Alpha".to_owned(),
            },
        ];
        let blocks = [second, first];
        let mut findings = DiagBuf::default();
        collect_drift(blocks.to_vec(), &derived, &mut findings);
        assert_eq!(findings.code, [Code::DerivationLineDrift; 2]);
        assert_eq!(
            findings.get_text(findings.message[0]),
            "`Print` configures member lines for `Session` that its `@derive(Alpha)` does not",
        );
        assert_eq!(
            findings.get_text(findings.message[1]),
            "`Encode` configures different member lines for `Session` than `Print` does",
        );
    }

    #[test]
    fn utf8_comments_and_crlf_are_preserved_in_move_fixes() {
        let newline = "\r\n";
        let source = format!(
            "# café{newline}@\"雪\"  # note{newline}impl T by Structure:{newline}    pass{newline}",
        );
        let start = source.find('@').unwrap();
        let value = start + 1;
        let end = value + "\"雪\"".len();
        let first = source.find("pass").unwrap();
        let fact = BlockDecorator {
            span: span(start, end),
            value: Some(span(value, end)),
            first: Some(span(first, first + 4)),
            marker: false,
            valid: true,
        };
        let facts = DerivationFacts {
            decorators: vec![fact.clone(), fact],
        };
        let mut findings = DiagBuf::default();
        collect_unused(&facts, &source, &mut findings);
        assert_eq!(findings.code, [Code::UnusedDerivationFact]);
        assert_eq!(findings.edits.len(), 2);
        assert_eq!(
            findings.get_text(findings.edits[1].text),
            "    Self += [\"雪\"]  # note\r\n",
        );
    }

    #[test]
    fn consumed_markers_and_invalid_occurrences_are_quiet() {
        let occurrence = BlockDecorator {
            span: span(0, 1),
            value: None,
            first: None,
            marker: false,
            valid: true,
        };
        let facts = DerivationFacts {
            decorators: vec![
                occurrence.clone(),
                BlockDecorator {
                    marker: true,
                    ..occurrence.clone()
                },
                BlockDecorator {
                    valid: false,
                    ..occurrence
                },
            ],
        };
        let mut findings = DiagBuf::default();
        collect_unused(&facts, "x", &mut findings);
        assert_eq!(findings.code, [Code::UnusedDerivationFact]);
    }

    #[test]
    fn block_drift_keeps_file_target_and_package_boundaries() {
        let first = block(1, 3, "Print", "cache");
        let mut other_file = block(2, 4, "Encode", "id");
        other_file.0.span.file = FileId::from_raw(1);
        let mut other_target = block(3, 5, "Inspect", "id");
        other_target.0.target = DefId::from_raw(11);
        let mut other_package = block(4, 6, "Visit", "id");
        other_package.0.package = PathId::from_raw(12);
        let mut blocks = vec![first, other_file.clone(), other_target, other_package];
        let mut findings = DiagBuf::default();
        collect_drift(blocks.clone(), &[], &mut findings);
        assert!(findings.is_empty());

        other_file.0.span.file = FileId::from_raw(0);
        blocks.push(other_file);
        collect_drift(blocks, &[], &mut findings);
        assert_eq!(findings.code, [Code::DerivationLineDrift]);
    }
}
