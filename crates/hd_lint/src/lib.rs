#![forbid(unsafe_code)]
//! Read-only advice over checked source facts. Normal compiler clients do not depend on this crate.

mod derivation;
mod modules;
mod syntax;

pub use derivation::{analyze_derivations, collect_derivations};
pub use modules::{analyze_modules, collect_modules};
pub use syntax::{analyze_sources, collect_sources};

use std::sync::Arc;

use hd_base::SliceSort as _;
use hd_check::comparison::{
    Binding, Comparison, DiscardedExpression, SourceFacts, Statement, VariantPattern,
};
use hd_diag::{Code, DiagBuf, FixSafety, Severity};
use hd_syntax::TokenKind;
use hd_types::Prim;

pub const DERIVATION_DRIFT: &str = Code::DerivationLineDrift.as_str();
pub const UNUSED_DERIVATION: &str = Code::UnusedDerivationFact.as_str();
pub const MIXED_SCRIPT: &str = Code::MixedScriptIdentifier.as_str();
pub const UNSELECTED_MAIN: &str = Code::UnselectedMain.as_str();
pub const UNREACHABLE: &str = Code::UnreachableCode.as_str();
pub const UNUSED_DEBUG: &str = Code::UnusedDebugText.as_str();
pub const REDUNDANT_MUT: &str = Code::RedundantLetMut.as_str();
pub const VARIANT_BINDING: &str = Code::VariantBindingNameMismatch.as_str();
pub const UNUSED_BINDING: &str = Code::UnusedLocalBinding.as_str();
pub const UNSIGNED_COMPARISON: &str = Code::UnsignedComparisonAlways.as_str();

/// One stable lint entry. Checks move behind this registry in a later
/// architecture slice; policy already uses the registry as its sole index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    code: Code,
    default: Level,
}

impl Rule {
    const fn new(code: Code, default: Level) -> Self {
        Self { code, default }
    }

    #[must_use]
    pub const fn code(self) -> Code {
        self.code
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        self.code.as_str()
    }

    #[must_use]
    pub const fn default_level(self) -> Level {
        self.default
    }
}

/// The complete migrated lint surface, in stable registry order.
pub const RULES: &[Rule] = &[
    Rule::new(Code::DerivationLineDrift, Level::Warn),
    Rule::new(Code::MixedScriptIdentifier, Level::Warn),
    Rule::new(Code::RedundantLetMut, Level::Warn),
    Rule::new(Code::UnreachableCode, Level::Warn),
    Rule::new(Code::UnselectedMain, Level::Warn),
    Rule::new(Code::UnsignedComparisonAlways, Level::Warn),
    Rule::new(Code::UnusedDebugText, Level::Warn),
    Rule::new(Code::UnusedDerivationFact, Level::Warn),
    Rule::new(Code::UnusedLocalBinding, Level::Warn),
    Rule::new(Code::VariantBindingNameMismatch, Level::Warn),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Allow,
    Warn,
    Deny,
}

/// Exact-name command policy, indexed by [`RULES`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Policy {
    levels: [Option<Level>; RULES.len()],
}

impl Policy {
    pub fn set(&mut self, name: &str, level: Level) -> Result<(), String> {
        let Some(index) = RULES.iter().position(|rule| rule.name() == name) else {
            return Err(format!("unknown lint `{name}`"));
        };
        let slot = &mut self.levels[index];
        if slot.is_some_and(|old| old != level) {
            return Err(format!("conflicting levels for lint `{name}`"));
        }
        *slot = Some(level);
        Ok(())
    }

    fn level(self, code: Code) -> Level {
        let index = RULES
            .iter()
            .position(|rule| rule.code == code)
            .expect("lint finding is absent from the registry");
        self.levels[index].unwrap_or(RULES[index].default)
    }

    /// Apply command policy after analysis has built a policy-free buffer.
    #[must_use]
    pub fn apply(self, findings: &DiagBuf) -> DiagBuf {
        let mut selected = DiagBuf::default();
        selected.append_if(findings, &|code| self.level(code) != Level::Allow);
        for (code, severity) in selected.code.iter().zip(&mut selected.severity) {
            *severity = match self.level(*code) {
                Level::Deny => Severity::Error,
                Level::Warn | Level::Allow => Severity::Warning,
            };
        }
        selected
    }
}

/// Preserves the existing literal-zero predicate, exact message, and comparison span.
#[must_use]
pub fn analyze(facts: &SourceFacts, texts: &[Arc<str>], policy: Policy) -> DiagBuf {
    policy.apply(&collect(facts, texts))
}

/// Build all checked-source findings without command policy.
#[must_use]
pub fn collect(facts: &SourceFacts, texts: &[Arc<str>]) -> DiagBuf {
    let mut diags = comparisons(&facts.comparisons, texts);
    bindings(&mut diags, &facts.bindings, texts);
    mutability(&mut diags, &facts.bindings);
    patterns(&mut diags, &facts.patterns);
    statements(&mut diags, &facts.statements);
    discarded(&mut diags, &facts.discarded, texts);
    diags
}

fn comparisons(facts: &[Comparison], texts: &[Arc<str>]) -> DiagBuf {
    let mut diags = DiagBuf::default();
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
            Severity::Warning,
            fact.span,
            &message,
            None,
        );
    }
    diags
}

/// Preserve the original first unread binding per source line, independently per body.
fn bindings(diags: &mut DiagBuf, facts: &[Binding], texts: &[Arc<str>]) {
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
            Severity::Warning,
            fact.span,
            &format!("`{name}` is never read; name it `_{name}` to keep it"),
            None,
        );
    }
}

fn mutability(diags: &mut DiagBuf, facts: &[Binding]) {
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
        diags.push(
            Code::RedundantLetMut,
            Severity::Warning,
            binding.span,
            message,
            None,
        );
        if let Some(span) = fact.token {
            diags.add_fix("remove `mut`", FixSafety::Exact, &[(span, "")]);
        }
    }
}

fn patterns(diags: &mut DiagBuf, facts: &[VariantPattern]) {
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
                Severity::Warning,
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

fn statements(diags: &mut DiagBuf, facts: &[Statement]) {
    for fact in facts {
        if !fact.reachable {
            diags.push(
                Code::UnreachableCode,
                Severity::Warning,
                fact.span,
                "this statement is unreachable",
                None,
            );
        }
    }
}

fn discarded(diags: &mut DiagBuf, facts: &[DiscardedExpression], texts: &[Arc<str>]) {
    for fact in facts {
        if !fact.prelude_debug {
            continue;
        }
        diags.push(
            Code::UnusedDebugText,
            Severity::Warning,
            fact.span,
            "a `debug(...)` statement discards its text; use `dbg(...)` to print it",
            None,
        );
        let Some(call) = &fact.call else {
            continue;
        };
        let Some(span) = call.name else {
            continue;
        };
        let name = texts
            .get(span.file.idx())
            .and_then(|text| text.get(span.lo as usize..span.hi as usize));
        if name == Some("debug")
            && call.positional == 1
            && call.named == 0
            && !call.spread
            && call.after_spread == 0
            && call.standard_debug
            && call.standard_dbg
        {
            diags.add_fix(
                "replace discarded `debug` with `dbg`",
                FixSafety::Exact,
                &[(span, "dbg")],
            );
        }
    }
}
