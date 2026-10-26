#![forbid(unsafe_code)]
//! Read-only advice over checked source facts. Normal compiler clients do not depend on this crate.

use std::sync::Arc;

use hd_base::SliceSort as _;
use hd_check::comparison::Comparison;
use hd_diag::{Code, DiagBuf, Severity};
use hd_syntax::TokenKind;
use hd_types::Prim;

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
}

impl Policy {
    pub fn set(&mut self, name: &str, level: Level) -> Result<(), String> {
        if name != UNSIGNED_COMPARISON {
            return Err(format!("unknown lint `{name}`"));
        }
        if self.unsigned.is_some_and(|old| old != level) {
            return Err(format!("conflicting levels for lint `{name}`"));
        }
        self.unsigned = Some(level);
        Ok(())
    }

    #[must_use]
    pub fn unsigned_level(self) -> Level {
        self.unsigned.unwrap_or(Level::Warn)
    }
}

/// Preserves the existing literal-zero predicate, exact message, and comparison span.
#[must_use]
pub fn analyze(facts: &[Comparison], texts: &[Arc<str>], policy: Policy) -> DiagBuf {
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
