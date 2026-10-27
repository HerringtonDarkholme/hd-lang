//! Derivation advice over nominal, resolved declaration facts.

use std::sync::Arc;

use hd_base::{SliceSort as _, Span};
use hd_diag::{Code, DiagBuf, FixSafety};
use hd_resolve::facts::{BlockDecorator, DerivationFacts};

use crate::{Policy, severity};

/// The existing block-decorator and member-line advice, with independent policy.
#[must_use]
pub fn analyze_derivations(facts: &DerivationFacts, texts: &[Arc<str>], policy: Policy) -> DiagBuf {
    let mut diags = DiagBuf::default();
    if let Some(severity) = severity(policy.unused_derivation) {
        let mut decorators: Vec<_> = facts.decorators.iter().collect();
        decorators.sort_stable_by_key(|d| (d.span.file.raw(), d.span.lo, d.span.hi));
        decorators.dedup();
        for decorator in decorators {
            if decorator.marker || !decorator.valid {
                continue;
            }
            diags.push(
                Code::UnusedDerivationFact,
                severity,
                decorator.span,
                "no derivation reads this value; write it in the block as `Self += [...]`",
                None,
            );
            move_fix(&mut diags, decorator, texts);
        }
    }
    let Some(severity) = severity(policy.derivation_drift) else {
        return diags;
    };
    let mut blocks: Vec<_> = facts.blocks.iter().collect();
    blocks.sort_stable_by_key(|b| (b.span.file.raw(), b.span.lo, b.span.hi));
    blocks.dedup();
    for (i, block) in blocks.iter().enumerate() {
        // The old resolver compares blocks within one module. Nominal
        // identities separate unrelated target types and trait packages.
        let first = blocks[..i].iter().find(|first| {
            first.span.file == block.span.file
                && first.target == block.target
                && first.package == block.package
        });
        let drift = first
            .filter(|first| first.lines != block.lines)
            .map(|first| {
                format!(
                    "`{}` configures different member lines for `{}` than `{}` does",
                    block.trait_name, block.target_name, first.trait_name
                )
            });
        let message = drift.or_else(|| {
            if block.lines.is_empty() {
                return None;
            }
            let derived = facts
                .derived
                .iter()
                .filter(|derived| {
                    derived.folder == block.folder
                        && derived.target == block.target
                        && derived.package == block.package
                        && derived.trait_ != block.trait_
                })
                .map(|derived| derived.name.as_str())
                .min()?;
            Some(format!(
                "`{}` configures member lines for `{}` that its `@derive({derived})` does not",
                block.trait_name, block.target_name
            ))
        });
        if let Some(message) = message {
            diags.push(
                Code::DerivationLineDrift,
                severity,
                block.span,
                &message,
                None,
            );
        }
    }
    diags
}

fn move_fix(diags: &mut DiagBuf, fact: &BlockDecorator, texts: &[Arc<str>]) {
    let (Some(value), Some(first), Some(text)) =
        (fact.value, fact.first, texts.get(fact.span.file.idx()))
    else {
        return;
    };
    let text = text.as_ref();
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
