#![forbid(unsafe_code)]
//! Read-only advice over checked source facts. Normal compiler clients do not depend on this crate.

mod derivation;
mod modules;
mod syntax;

use hd_check::analysis::CheckedView;
use hd_diag::{Code, DiagBuf, FixSafety, Severity};
use hd_driver::Output;
use hd_driver::analysis::{AnalysisSink, Event, Events};
use hd_driver::diagnostics::Diag;
use hd_project::LintTable;
use hd_syntax::{CommentKind, SyntaxKind, TokenKind};
use hd_types::Prim;

pub const DERIVATION_DRIFT: &str = Code::DerivationLineDrift.as_str();
pub const BROKEN_DOC_LINK: &str = Code::BrokenDocLink.as_str();
pub const DBG_IN_DEPENDENCY: &str = Code::DbgInDependency.as_str();
pub const LOST_WRITE: &str = Code::LostWrite.as_str();
pub const UNUSED_DERIVATION: &str = Code::UnusedDerivationFact.as_str();
pub const MIXED_SCRIPT: &str = Code::MixedScriptIdentifier.as_str();
pub const PRIVATE_MAIN: &str = Code::PrivateMain.as_str();
pub const UNSELECTED_MAIN: &str = Code::UnselectedMain.as_str();
pub const UNREACHABLE: &str = Code::UnreachableCode.as_str();
pub const UNUSED_DEBUG: &str = Code::UnusedDebugText.as_str();
pub const REDUNDANT_MUT: &str = Code::RedundantLetMut.as_str();
pub const VARIANT_BINDING: &str = Code::VariantBindingNameMismatch.as_str();
pub const UNUSED_BINDING: &str = Code::UnusedLocalBinding.as_str();
pub const UNSIGNED_COMPARISON: &str = Code::UnsignedComparisonAlways.as_str();
pub const UNBOUND_DISPOSABLE: &str = Code::UnboundDisposable.as_str();

/// One stable lint entry: identity, default policy, requested events and check.
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    code: Code,
    default: Level,
    events: Option<Events>,
    check: Check,
}

type Check = for<'v> fn(&Event<'v>, &CheckedView<'v>, &mut DiagBuf);

impl Rule {
    const fn new(code: Code, default: Level, events: Events, check: Check) -> Self {
        Self {
            code,
            default,
            events: Some(events),
            check,
        }
    }

    const fn command(code: Code, default: Level, check: Check) -> Self {
        Self {
            code,
            default,
            events: None,
            check,
        }
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

    #[must_use]
    pub const fn events(self) -> Option<Events> {
        self.events
    }
}

/// The complete migrated lint surface, in stable registry order.
pub const RULES: &[Rule] = &[
    Rule::command(Code::BrokenDocLink, Level::Warn, no_event),
    Rule::new(
        Code::DbgInDependency,
        Level::Warn,
        Events::FETCHED_DEBUG,
        dbg_in_dependency,
    ),
    Rule::new(
        Code::DerivationLineDrift,
        Level::Warn,
        Events::DERIVATIONS,
        derivation::drift,
    ),
    Rule::new(
        Code::LifetimeOrder,
        Level::Warn,
        Events::MODULE,
        syntax::lifetime_order,
    ),
    Rule::new(
        Code::LostWrite,
        Level::Warn,
        Events::PARAMETER_WRITE,
        lost_write,
    ),
    Rule::new(
        Code::MixedScriptIdentifier,
        Level::Warn,
        Events::IDENT,
        syntax::mixed_script,
    ),
    Rule::new(
        Code::PrivateMain,
        Level::Warn,
        Events::MAIN_SITE,
        modules::private_main,
    ),
    Rule::new(
        Code::RedundantLetMut,
        Level::Warn,
        Events::BINDING,
        redundant_mut,
    ),
    Rule::new(
        Code::UnreachableCode,
        Level::Warn,
        Events::STATEMENT,
        unreachable,
    ),
    Rule::new(
        Code::UnselectedMain,
        Level::Warn,
        Events::MAIN_SITE,
        modules::unselected_main,
    ),
    Rule::new(
        Code::UnsignedComparisonAlways,
        Level::Warn,
        Events::COMPARISON,
        unsigned_comparison,
    ),
    Rule::new(
        Code::UnboundDisposable,
        Level::Deny,
        Events::BINDING.union(Events::DISCARD),
        unbound_disposable,
    ),
    Rule::new(
        Code::UnusedDebugText,
        Level::Warn,
        Events::DISCARD,
        unused_debug,
    ),
    Rule::new(
        Code::UnusedDerivationFact,
        Level::Warn,
        Events::DERIVATIONS,
        derivation::unused,
    ),
    Rule::new(
        Code::UnusedLocalBinding,
        Level::Warn,
        Events::BINDING,
        unused_binding,
    ),
    Rule::new(
        Code::VariantBindingNameMismatch,
        Level::Warn,
        Events::PATTERN,
        variant_binding,
    ),
];

fn no_event(_: &Event<'_>, _: &CheckedView<'_>, _: &mut DiagBuf) {}

fn unbound_disposable(event: &Event<'_>, view: &CheckedView<'_>, out: &mut DiagBuf) {
    let (span, message) = match event {
        Event::Binding(source) => {
            let Some(binding) = view.binding(*source) else {
                return;
            };
            if !binding.is_unbound_disposable() {
                return;
            }
            (
                binding.span(),
                format!(
                    "`{}` binds a disposable value without registering cleanup; use `using` or `using!`",
                    binding.name()
                ),
            )
        }
        Event::Discard(source) => {
            let Some(call) = view.call(*source) else {
                return;
            };
            if !call.is_unbound_disposable() {
                return;
            }
            (
                call.span(),
                "this disposable value is discarded without registering cleanup; use `using` or `using!`"
                    .to_owned(),
            )
        }
        _ => return,
    };
    out.push(
        Code::UnboundDisposable,
        Severity::Warning,
        span,
        &message,
        None,
    );
}

/// The policy-selected diagnostic for a documentation link which resolved
/// to no documented item. Link resolution stays in `hd_doc`; this crate owns
/// the lint's stable code, message and level.
#[must_use]
pub fn broken_doc_link(policy: Policy, name: &str, private: bool) -> Option<Diag> {
    let level = policy.level(Code::BrokenDocLink);
    if level == Level::Allow {
        return None;
    }
    let why = if private {
        "is private and not documented"
    } else {
        "names no item"
    };
    Some(Diag::new(
        Some(Code::BrokenDocLink),
        match level {
            Level::Deny => Severity::Error,
            Level::Warn | Level::Allow => Severity::Warning,
        },
        &format!("the link `[`{name}`]` {why}"),
    ))
}

fn dbg_in_dependency(event: &Event<'_>, _view: &CheckedView<'_>, out: &mut DiagBuf) {
    let Event::FetchedDebug(event) = event else {
        return;
    };
    out.push(
        Code::DbgInDependency,
        Severity::Warning,
        event.span,
        &format!(
            "fetched package `{}` contains `dbg`; its debug output is suppressed",
            event.package
        ),
        None,
    );
}

fn lost_write(event: &Event<'_>, view: &CheckedView<'_>, out: &mut DiagBuf) {
    let Event::ParameterWrite(source) = event else {
        return;
    };
    let Some(write) = view.parameter_write(*source) else {
        return;
    };
    if write.observed_after() {
        return;
    }
    let message = if write.is_receiver() {
        "this write to `self` is lost: the copy is never read afterwards or returned; take `&mut self` to change the caller's value".to_owned()
    } else {
        format!(
            "this write to the parameter `{}` is lost: the copy is never read afterwards or returned; take `&mut T` to change the caller's value",
            write.name()
        )
    };
    out.push(
        Code::LostWrite,
        Severity::Warning,
        write.span(),
        &message,
        None,
    );
}

/// The single registered consumer used by `hd lint`.
pub struct Analyzer;

pub static ANALYZER: Analyzer = Analyzer;

impl AnalysisSink for Analyzer {
    fn interests(&self) -> Events {
        RULES.iter().fold(Events::default(), |events, rule| {
            rule.events.map_or(events, |more| events.union(more))
        })
    }

    fn diagnostic_code(&self, name: &str) -> Option<Code> {
        RULES
            .iter()
            .find(|rule| rule.name() == name)
            .map(|rule| rule.code())
    }

    fn event(&self, event: &Event<'_>, view: &CheckedView<'_>, out: &mut DiagBuf) {
        let family = event.family();
        for rule in RULES
            .iter()
            .filter(|rule| rule.events.is_some_and(|events| events.contains(family)))
        {
            (rule.check)(event, view, out);
        }
    }
}

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

    /// The selected package's manifest policy, before explicit CLI levels
    /// are overlaid.
    pub fn from_manifest(table: Option<&LintTable>) -> Result<Self, ManifestPolicyError> {
        let Some(table) = table else {
            return Ok(Self::default());
        };
        if !table.table {
            return Err(ManifestPolicyError {
                line: table.line,
                message: "`[lint]` must be a table".to_owned(),
            });
        }
        let mut policy = Self::default();
        for (level_name, entry, level) in [
            ("allow", table.allow.as_ref(), Level::Allow),
            ("warn", table.warn.as_ref(), Level::Warn),
            ("deny", table.deny.as_ref(), Level::Deny),
        ] {
            let Some(entry) = entry else { continue };
            let Some(names) = &entry.names else {
                return Err(ManifestPolicyError {
                    line: entry.line,
                    message: format!("`lint.{level_name}` must be a list of strings"),
                });
            };
            for name in names {
                policy
                    .set(name, level)
                    .map_err(|message| ManifestPolicyError {
                        line: entry.line,
                        message,
                    })?;
            }
        }
        Ok(policy)
    }

    /// Explicit command-line levels win over the manifest for the same
    /// exact code. Conflicts were already rejected within each source.
    #[must_use]
    pub fn overlay(mut self, explicit: Self) -> Self {
        for (level, over) in self.levels.iter_mut().zip(explicit.levels) {
            if over.is_some() {
                *level = over;
            }
        }
        self
    }

    /// Whether no registered rule can emit after level policy. Source
    /// directives still need validation in this state.
    #[must_use]
    pub fn all_allow(self) -> bool {
        RULES
            .iter()
            .all(|rule| self.level(rule.code) == Level::Allow)
    }

    /// Whether command policy suppresses one registered lint completely.
    #[must_use]
    pub fn allows(self, code: Code) -> bool {
        self.level(code) == Level::Allow
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
        self.apply_with_source(findings, &SourcePolicy::default())
    }

    /// Apply level policy and then source-site allows to policy-free
    /// findings. Compiler errors never enter this path.
    #[must_use]
    pub fn apply_with_source(self, findings: &DiagBuf, source: &SourcePolicy) -> DiagBuf {
        let mut selected = DiagBuf::default();
        for row in 0..findings.len() {
            let code = findings.code[row];
            if self.level(code) == Level::Allow || source.suppresses(code, findings.primary[row]) {
                continue;
            }
            append_row(&mut selected, findings, row);
            let severity = selected
                .severity
                .last_mut()
                .expect("appended lint diagnostic has severity");
            *severity = match self.level(code) {
                Level::Deny => Severity::Error,
                Level::Warn | Level::Allow => Severity::Warning,
            };
        }
        selected
    }
}

/// A located invalid `[lint]` policy value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestPolicyError {
    pub line: u32,
    pub message: String,
}

/// A located invalid source suppression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourcePolicyError {
    pub file: String,
    pub line: usize,
    pub message: String,
}

/// Valid source-site allows for one selected build.
#[derive(Debug, Default)]
pub struct SourcePolicy {
    allows: Vec<SiteAllow>,
}

#[derive(Debug)]
struct SiteAllow {
    file: usize,
    code: Code,
    owner: (u32, u32),
    nested: Vec<(u32, u32)>,
}

impl SourcePolicy {
    /// Read and validate directives in the selected root files of an
    /// already-built output. Existing parses are reused; a warm build that
    /// retained only text materializes syntax without checking bodies.
    pub fn from_output(output: &Output, selected: &[bool]) -> Result<Self, Vec<SourcePolicyError>> {
        let mut policy = Self::default();
        let mut errors = Vec::new();
        for file in 0..output.files.len() {
            if !selected.get(file).copied().unwrap_or(false) {
                continue;
            }
            let (Some(text), Some(parsed)) = (output.texts.get(file), output.source_parse(file))
            else {
                continue;
            };
            let tokens = &parsed.tokens;
            for comment in 0..tokens.com_start.len() {
                if tokens.com_kind[comment] != CommentKind::Plain {
                    continue;
                }
                let start = tokens.com_start[comment] as usize;
                let end_offset = tokens.com_end[comment];
                let end = end_offset as usize;
                let line_start = text[..start].rfind('\n').map_or(0, |at| at + 1);
                if !text[line_start..start]
                    .bytes()
                    .all(|byte| matches!(byte, b' ' | b'\t'))
                {
                    continue;
                }
                let Some(name) = text[start..end].strip_prefix("# lint-allow:") else {
                    continue;
                };
                let name = name.trim();
                let line = tokens.line_of(tokens.com_start[comment]) + 1;
                let Some(rule) = RULES.iter().find(|rule| rule.name() == name) else {
                    errors.push(SourcePolicyError {
                        file: output.files[file].clone(),
                        line,
                        message: format!("unknown lint `{name}`"),
                    });
                    continue;
                };
                if !site_reachable(rule.code) {
                    errors.push(SourcePolicyError {
                        file: output.files[file].clone(),
                        line,
                        message: format!("site allow cannot reach `{name}`; allow it in `[lint]`"),
                    });
                    continue;
                }
                let boundary = tokens.start.partition_point(|&at| at < end_offset);
                let Some(&next_start) = tokens.start.get(boundary) else {
                    continue;
                };
                if tokens.line_of(next_start) != line {
                    continue;
                }
                let Some(owner) = parsed
                    .tree
                    .root()
                    .descendants()
                    .filter(|node| statement_kind(node.kind()))
                    .filter(|node| {
                        let (first, _) = parsed.tree.span_tokens(node.index());
                        first.get().is_some_and(|first| first.idx() == boundary)
                    })
                    .min_by_key(|node| {
                        let (first, last) = parsed.tree.span_tokens(node.index());
                        last.raw().saturating_sub(first.raw())
                    })
                else {
                    continue;
                };
                let span = |node: hd_syntax::NodeRef<'_>| {
                    let (first, last) = parsed.tree.span_tokens(node.index());
                    (tokens.start[first.idx()], tokens.end[last.idx()])
                };
                let nested = owner
                    .descendants()
                    .filter(|node| statement_kind(node.kind()))
                    .map(span)
                    .collect();
                policy.allows.push(SiteAllow {
                    file,
                    code: rule.code,
                    owner: span(owner),
                    nested,
                });
            }
        }
        if errors.is_empty() {
            Ok(policy)
        } else {
            Err(errors)
        }
    }

    fn suppresses(&self, code: Code, span: hd_base::Span) -> bool {
        self.allows.iter().any(|allow| {
            allow.file == span.file.idx()
                && allow.code == code
                && contains(allow.owner, span.lo, span.hi)
                && !allow
                    .nested
                    .iter()
                    .any(|&nested| contains(nested, span.lo, span.hi))
        })
    }
}

fn contains(range: (u32, u32), lo: u32, hi: u32) -> bool {
    range.0 <= lo && hi <= range.1
}

fn statement_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LetStmt
            | SyntaxKind::DiscardStmt
            | SyntaxKind::AssignmentStmt
            | SyntaxKind::ReturnStmt
            | SyntaxKind::BreakStmt
            | SyntaxKind::ContinueStmt
            | SyntaxKind::UsingStmt
            | SyntaxKind::ExprStmt
    )
}

fn site_reachable(code: Code) -> bool {
    matches!(
        code,
        Code::UnboundDisposable | Code::UnsignedComparisonAlways | Code::UnusedDebugText
    )
}

fn append_row(out: &mut DiagBuf, source: &DiagBuf, row: usize) {
    if !out.push(
        source.code[row],
        source.severity[row],
        source.primary[row],
        source.get_text(source.message[row]),
        source.root[row],
    ) {
        return;
    }
    for label in source.labels[row].range() {
        out.add_label(
            source.label_span[label],
            source.get_text(source.label_msg[label]),
        );
    }
    for fix in source.fixes[row].range() {
        let edits: Vec<_> = source.edits[source.fix_edits[fix].range()]
            .iter()
            .map(|edit| (edit.span, source.get_text(edit.text)))
            .collect();
        out.add_fix(
            source.get_text(source.fix_title[fix]),
            source.fix_safety[fix],
            &edits,
        );
    }
}

fn unsigned_comparison(event: &Event<'_>, view: &CheckedView<'_>, out: &mut DiagBuf) {
    let Event::Comparison(event) = event else {
        return;
    };
    let Some(left) = view.expression(event.left) else {
        return;
    };
    let Some(right) = view.expression(event.right) else {
        return;
    };
    let (operand, always, operator) = match event.operator {
        TokenKind::GtEq if right.is_unsuffixed_zero() => (event.left, true, ">="),
        TokenKind::LtEq if left.is_unsuffixed_zero() => (event.right, true, "<="),
        TokenKind::Lt if right.is_unsuffixed_zero() => (event.left, false, "<"),
        TokenKind::Gt if left.is_unsuffixed_zero() => (event.right, false, ">"),
        _ => return,
    };
    let Some(primitive) = view
        .adjusted_type(operand)
        .and_then(hd_check::analysis::TypeRef::primitive)
        .filter(|primitive| primitive.is_unsigned())
    else {
        return;
    };
    let Some(operand) = view.expression(operand) else {
        return;
    };
    let name = view.source().text_at(operand.span()).unwrap_or("the value");
    let left_operand = operand.span() == left.span();
    let form = if left_operand {
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
    out.push(
        Code::UnsignedComparisonAlways,
        Severity::Warning,
        event.span,
        &message,
        None,
    );
}

fn unused_binding(event: &Event<'_>, view: &CheckedView<'_>, out: &mut DiagBuf) {
    let Event::Binding(source) = event else {
        return;
    };
    let Some(binding) = view.binding(*source) else {
        return;
    };
    if binding.is_read() || binding.name().starts_with('_') {
        return;
    }
    let text = view.source().text();
    let line_of = |span: hd_base::Span| {
        let lo = (span.lo as usize).min(text.len());
        text.get(..lo).and_then(|prefix| prefix.rfind('\n'))
    };
    let line = line_of(binding.span());
    let earlier = view.bindings().any(|other| {
        let Some(other) = view.binding(other) else {
            return false;
        };
        !other.is_read()
            && !other.name().starts_with('_')
            && other.same_owner(binding)
            && other.precedes(binding)
            && line_of(other.span()) == line
    });
    if earlier {
        return;
    }
    let name = binding.name();
    out.push(
        Code::UnusedLocalBinding,
        Severity::Warning,
        binding.span(),
        &format!("`{name}` is never read; name it `_{name}` to keep it"),
        None,
    );
}

fn redundant_mut(event: &Event<'_>, view: &CheckedView<'_>, out: &mut DiagBuf) {
    let Event::Binding(source) = event else {
        return;
    };
    let Some(binding) = view.binding(*source) else {
        return;
    };
    let Some(fact) = binding.mutability() else {
        return;
    };
    if fact.mutable_address_taken() {
        return;
    }
    let message = if fact.is_primitive() {
        "`let mut` on a primitive is needed only when `&mut` of it is taken; a plain `let` is already reassignable"
    } else if fact.annotation_is_writable() {
        "the annotation already grants `mut`, and no `&mut` of this binding is taken; remove the `mut` before the name"
    } else {
        return;
    };
    out.push(
        Code::RedundantLetMut,
        Severity::Warning,
        binding.span(),
        message,
        None,
    );
    if let Some(span) = fact.token() {
        out.add_fix("remove `mut`", FixSafety::Exact, &[(span, "")]);
    }
}

fn variant_binding(event: &Event<'_>, view: &CheckedView<'_>, out: &mut DiagBuf) {
    let Event::Pattern(source) = event else {
        return;
    };
    let Some(pattern) = view.pattern(*source) else {
        return;
    };
    for binding in pattern.bindings() {
        let Some(field) = pattern.fields().get(binding.position) else {
            continue;
        };
        if !binding.bare
            || binding.name == *field
            || !pattern
                .fields()
                .iter()
                .enumerate()
                .any(|(i, name)| i != binding.position && *name == binding.name)
        {
            continue;
        }
        out.push(
            Code::VariantBindingNameMismatch,
            Severity::Warning,
            binding.span,
            &format!(
                "`{}` is also a field of `{}`; the field in this position is `{field}`",
                binding.name,
                pattern.variant()
            ),
            None,
        );
    }
}

fn unreachable(event: &Event<'_>, view: &CheckedView<'_>, out: &mut DiagBuf) {
    let Event::Statement(source) = event else {
        return;
    };
    let Some(statement) = view.statement(*source) else {
        return;
    };
    if !statement.is_reachable() {
        out.push(
            Code::UnreachableCode,
            Severity::Warning,
            statement.span(),
            "this statement is unreachable",
            None,
        );
    }
}

fn unused_debug(event: &Event<'_>, view: &CheckedView<'_>, out: &mut DiagBuf) {
    let Event::Discard(source) = event else {
        return;
    };
    if !view
        .selected_declaration(*source)
        .is_some_and(hd_check::analysis::DeclRef::is_prelude_debug)
    {
        return;
    }
    let Some(call) = view.call(*source) else {
        return;
    };
    out.push(
        Code::UnusedDebugText,
        Severity::Warning,
        call.span(),
        "a `debug(...)` statement discards its text; use `dbg(...)` to print it",
        None,
    );
    let Some(span) = call.name() else {
        return;
    };
    if view.source().text_at(span) == Some("debug")
        && call.positional() == 1
        && call.named() == 0
        && !call.has_spread()
        && call.after_spread() == 0
        && call.standard_debug()
        && call.standard_dbg()
    {
        out.add_fix(
            "replace discarded `debug` with `dbg`",
            FixSafety::Exact,
            &[(span, "dbg")],
        );
    }
}
