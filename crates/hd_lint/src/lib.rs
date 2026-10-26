#![forbid(unsafe_code)]
//! Read-only advice over checked source facts. Normal compiler clients do not depend on this crate.

use std::sync::Arc;

use hd_base::SliceSort as _;
use hd_check::comparison::{Binding, Comparison, SourceFacts, VariantPattern};
use hd_diag::{Code, DiagBuf, FixSafety, Severity};
use hd_syntax::TokenKind;
use hd_types::Prim;

pub const REDUNDANT_MUT: &str = "redundant-let-mut";
pub const VARIANT_BINDING: &str = "variant-binding-name-mismatch";
pub const UNUSED_BINDING: &str = "unused-local-binding";
pub const UNSIGNED_COMPARISON: &str = "unsigned-comparison-always";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Allow,
    Warn,
    Deny,
}

/// Exact-name command policy. More producers join the registry in later slices.
#[derive(Clone, Copy, Debug, Default)]
pub struct Policy {
    unsigned: Option<Level>,
    unused: Option<Level>,
    redundant: Option<Level>,
    variant: Option<Level>,
}

impl Policy {
    pub fn set(&mut self, name: &str, level: Level) -> Result<(), String> {
        let slot = match name {
            UNSIGNED_COMPARISON => &mut self.unsigned,
            UNUSED_BINDING => &mut self.unused,
            REDUNDANT_MUT => &mut self.redundant,
            VARIANT_BINDING => &mut self.variant,
            _ => return Err(format!("unknown lint `{name}`")),
        };
        if slot.is_some_and(|old| old != level) {
            return Err(format!("conflicting levels for lint `{name}`"));
        }
        *slot = Some(level);
        Ok(())
    }

    #[must_use]
    pub fn unsigned_level(self) -> Level {
        self.unsigned.unwrap_or(Level::Warn)
    }
}

/// Preserves the existing literal-zero predicate, exact message, and comparison span.
#[must_use]
pub fn analyze(facts: &SourceFacts, texts: &[Arc<str>], policy: Policy) -> DiagBuf {
    let mut diags = comparisons(&facts.comparisons, texts, policy);
    bindings(&mut diags, &facts.bindings, texts, policy);
    mutability(&mut diags, &facts.bindings, policy);
    patterns(&mut diags, &facts.patterns, policy);
    diags
}

fn comparisons(facts: &[Comparison], texts: &[Arc<str>], policy: Policy) -> DiagBuf {
    let mut diags = DiagBuf::default();
    let severity = match policy.unsigned_level() {
        Level::Allow => return diags,
        Level::Warn => Severity::Warning,
        Level::Deny => Severity::Error,
    };
    let mut facts: Vec<_> = facts.iter().collect();
    facts.sort_stable_by(|a, b| {
        (a.span.file.raw(), a.span.lo, a.span.hi).cmp(&(b.span.file.raw(), b.span.lo, b.span.hi))
    });
    facts.dedup();
    for fact in facts {
        let (left, operand, always, operator) = match fact.operator {
            TokenKind::GtEq if fact.right.unsuffixed_zero => (true, &fact.left, true, ">="),
            TokenKind::LtEq if fact.left.unsuffixed_zero => (false, &fact.right, true, "<="),
            TokenKind::Lt if fact.right.unsuffixed_zero => (true, &fact.left, false, "<"),
            TokenKind::Gt if fact.left.unsuffixed_zero => (false, &fact.right, false, ">"),
            _ => continue,
        };
        let Some(primitive) = operand.primitive.filter(|p| p.is_unsigned()) else {
            continue;
        };
        let name = texts
            .get(operand.span.file.idx())
            .and_then(|text| text.get(operand.span.lo as usize..operand.span.hi as usize))
            .unwrap_or("the value");
        let form = if left {
            format!("{name} {operator} 0")
        } else {
            format!("0 {operator} {name}")
        };
        let ordering = if always {
            "always true"
        } else {
            "always false"
        };
        let mut message = format!("`{name}` is unsigned, so `{form}` is {ordering}");
        if primitive == Prim::Usize {
            message.push_str("; consider a signed literal such as `+10` or another condition");
        }
        diags.push(
            Code::UnsignedComparisonAlways,
            severity,
            fact.span,
            &message,
            None,
        );
    }
    diags
}

/// Preserve the original first unread binding per source line, independently per body.
fn bindings(diags: &mut DiagBuf, facts: &[Binding], texts: &[Arc<str>], policy: Policy) {
    let severity = match policy.unused.unwrap_or(Level::Warn) {
        Level::Allow => return,
        Level::Warn => Severity::Warning,
        Level::Deny => Severity::Error,
    };
    let mut facts: Vec<_> = facts.iter().collect();
    facts.sort_stable_by(|a, b| {
        (a.span.file.raw(), a.body.raw(), a.local.raw()).cmp(&(
            b.span.file.raw(),
            b.body.raw(),
            b.local.raw(),
        ))
    });
    facts.dedup();
    let mut lines = hd_base::HashSet::default();
    for fact in facts {
        if fact.read || fact.name.starts_with('_') {
            continue;
        }
        let source = texts.get(fact.span.file.idx()).map_or("", |s| s.as_ref());
        let lo = (fact.span.lo as usize).min(source.len());
        let line = source.get(..lo).and_then(|prefix| prefix.rfind('\n'));
        if !lines.insert((fact.span.file, fact.body, line)) {
            continue;
        }
        let name = &fact.name;
        diags.push(
            Code::UnusedLocalBinding,
            severity,
            fact.span,
            &format!("`{name}` is never read; name it `_{name}` to keep it"),
            None,
        );
    }
}

fn severity(level: Option<Level>) -> Option<Severity> {
    match level.unwrap_or(Level::Warn) {
        Level::Allow => None,
        Level::Warn => Some(Severity::Warning),
        Level::Deny => Some(Severity::Error),
    }
}

fn mutability(diags: &mut DiagBuf, facts: &[Binding], policy: Policy) {
    let Some(severity) = severity(policy.redundant) else {
        return;
    };
    for binding in facts {
        let Some(fact) = &binding.mutability else {
            continue;
        };
        if fact.address_taken {
            continue;
        }
        let message = if fact.primitive {
            "`let mut` on a primitive is needed only when `&mut` of it is taken; a plain `let` is already reassignable"
        } else if fact.annotation_writable {
            "the annotation already grants `mut`, and no `&mut` of this binding is taken; remove the `mut` before the name"
        } else {
            continue;
        };
        diags.push(Code::RedundantLetMut, severity, binding.span, message, None);
        if let Some(span) = fact.token {
            diags.add_fix("remove `mut`", FixSafety::Exact, &[(span, "")]);
        }
    }
}

fn patterns(diags: &mut DiagBuf, facts: &[VariantPattern], policy: Policy) {
    let Some(severity) = severity(policy.variant) else {
        return;
    };
    for pattern in facts {
        for binding in &pattern.bindings {
            let Some(field) = pattern.fields.get(binding.position) else {
                continue;
            };
            if !binding.bare
                || binding.name == *field
                || !pattern
                    .fields
                    .iter()
                    .enumerate()
                    .any(|(i, name)| i != binding.position && *name == binding.name)
            {
                continue;
            }
            diags.push(
                Code::VariantBindingNameMismatch,
                severity,
                binding.span,
                &format!(
                    "`{}` is also a field of `{}`; the field in this position is `{field}`",
                    binding.name, pattern.variant
                ),
                None,
            );
        }
    }
}
