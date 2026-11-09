#![forbid(unsafe_code)]
//! Read-only advice over checked source facts. Normal compiler clients do not depend on this crate.

mod derivation;
mod modules;
mod syntax;
mod walk;

use hd_base::{SliceSort as _, Span};
use hd_check::analysis::CheckedView;
use hd_diag::{Code, DiagBuf, FixSafety, Severity};
use hd_driver::Output;
use hd_driver::analysis::{AnalysisSink, Module, Requests};
use hd_driver::diagnostics::Diag;
use hd_project::LintTable;
use hd_resolve::{Src, literal};
use hd_syntax::{CommentKind, NodeRef, SyntaxKind, TokenKind};
use hd_types::Prim;
pub use walk::{BindingEvent, Cx, Event, Events, binding_name, walk};

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

type Check = for<'v> fn(&Event<'v>, &Cx<'v>, &mut DiagBuf);

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

fn no_event(_: &Event<'_>, _: &Cx<'_>, _: &mut DiagBuf) {}

fn unbound_disposable(event: &Event<'_>, cx: &Cx<'_>, out: &mut DiagBuf) {
    let (span, message) = match event {
        Event::Binding(binding) => {
            if !binding.view.is_unbound_disposable() {
                return;
            }
            (
                binding.span,
                format!(
                    "`{}` binds a disposable value without registering cleanup; use `using` or `using!`",
                    binding.name
                ),
            )
        }
        Event::Discard(call) => {
            if !call.is_unbound_disposable() {
                return;
            }
            (
                cx.src.span(call.node()),
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

fn dbg_in_dependency(event: &Event<'_>, _: &Cx<'_>, out: &mut DiagBuf) {
    let Event::FetchedDebug { package, span } = event else {
        return;
    };
    out.push(
        Code::DbgInDependency,
        Severity::Warning,
        *span,
        &format!("fetched package `{package}` contains `dbg`; its debug output is suppressed"),
        None,
    );
}

/// The first change of a `mut` parameter in one direct statement of its
/// function body.
#[derive(Clone, Copy)]
pub struct ParameterWriteEvent<'v> {
    /// The parameter's name; `self` for a `mut self` receiver.
    pub name: &'v str,
    /// The body's direct statement that contains the change.
    pub statement: NodeRef<'v>,
    /// The syntax recorded as the change itself.
    pub write: NodeRef<'v>,
    /// A closure anywhere in the body, or a later statement, names the
    /// parameter.
    pub named_later: bool,
}

/// Groups the checked parameter changes by the direct body statement they
/// sit in, keeping each parameter name's first change per statement.
///
/// Cost: one sort of the changes; each body's statement list once per run
/// of its changes; one pass over the body per changed parameter for the
/// names; one binary search per change for its statement.
fn parameter_writes<'v>(src: Src<'v>, checked: CheckedView<'v>) -> Vec<ParameterWriteEvent<'v>> {
    struct Parameter<'v> {
        node: hd_base::NodeIdx,
        name: &'v str,
        captured: bool,
        last_named: Option<usize>,
    }
    let mut writes: Vec<_> = checked
        .parameter_writes()
        .map(|write| (write.parameter(), write.write(), write.body()))
        .collect();
    writes
        .sort_stable_by_key(|(parameter, write, _)| (parameter.index().raw(), write.index().raw()));
    let mut out = Vec::new();
    let mut seen = hd_base::HashSet::default();
    let mut body = hd_base::NodeIdx::NONE;
    let mut statements: Vec<NodeRef<'v>> = Vec::new();
    let mut parameter: Option<Parameter<'v>> = None;
    for (node, write, of) in writes {
        if of.index() != body {
            body = of.index();
            statements = of.children().collect();
            parameter = None;
        }
        let span = src.span(write);
        let Some(index) = statements
            .partition_point(|statement| src.span(*statement).lo <= span.lo)
            .checked_sub(1)
            .filter(|&at| {
                let statement = src.span(statements[at]);
                statement.lo <= span.lo && span.hi <= statement.hi
            })
        else {
            continue;
        };
        let current = match parameter {
            Some(ref held) if held.node == node.index() => held,
            _ => {
                let name = if node
                    .direct_token(&src.parse.tokens, TokenKind::KwSelfValue)
                    .is_some()
                {
                    "self"
                } else {
                    node.name(&src.parse.tokens)
                        .map_or("", |token| src.text(token).trim_matches('`'))
                };
                parameter.insert(Parameter {
                    node: node.index(),
                    name,
                    captured: statements
                        .iter()
                        .any(|statement| captures_name(src, *statement, name)),
                    last_named: statements
                        .iter()
                        .rposition(|statement| names_name(src, *statement, name)),
                })
            }
        };
        if !seen.insert((statements[index].index(), current.name)) {
            continue;
        }
        out.push(ParameterWriteEvent {
            name: current.name,
            statement: statements[index],
            write,
            named_later: current.captured || current.last_named.is_some_and(|last| last > index),
        });
    }
    out
}

fn lost_write(event: &Event<'_>, cx: &Cx<'_>, out: &mut DiagBuf) {
    let Event::ParameterWrite(write) = event else {
        return;
    };
    let src = cx.src;
    let name = write.name;
    // Conservative: a closure anywhere in the body, a loop around the
    // write, or any later statement that names the parameter observes it.
    if write.named_later || loop_observes_name(src, write.statement, src.span(write.write), name) {
        return;
    }
    let message = if name == "self" {
        "this write to `self` is lost: the copy is never read afterwards or returned; take `&mut self` to change the caller's value".to_owned()
    } else {
        format!(
            "this write to the parameter `{name}` is lost: the copy is never read afterwards or returned; take `&mut T` to change the caller's value"
        )
    };
    out.push(
        Code::LostWrite,
        Severity::Warning,
        src.span(write.statement),
        &message,
        None,
    );
}

/// Whether the top-level statement containing a write also has a loop
/// that contains the write and another use of the parameter. A loop's
/// condition or body can observe a value written by an earlier iteration,
/// even though both uses belong to the same top-level statement.
fn loop_observes_name(
    src: Src<'_>,
    node: NodeRef<'_>,
    write_span: hd_base::Span,
    name: &str,
) -> bool {
    let span = src.span(node);
    if write_span.lo < span.lo || write_span.hi > span.hi {
        return false;
    }
    if matches!(node.kind(), SyntaxKind::WhileExpr | SyntaxKind::ForExpr)
        && names_name_outside(src, node, write_span, name)
    {
        return true;
    }
    node.children()
        .any(|child| loop_observes_name(src, child, write_span, name))
}

/// Whether `node` names `name` anywhere outside the syntax recorded as the
/// mutation itself.
fn names_name_outside(
    src: Src<'_>,
    node: NodeRef<'_>,
    write_span: hd_base::Span,
    name: &str,
) -> bool {
    node.direct_tokens().any(|token| {
        let span = src.token_span(token);
        !(write_span.lo <= span.lo && span.hi <= write_span.hi) && names_token(src, token, name)
    }) || node
        .children()
        .any(|child| names_name_outside(src, child, write_span, name))
}

fn names_name(src: Src<'_>, node: NodeRef<'_>, name: &str) -> bool {
    node.direct_tokens()
        .any(|token| names_token(src, token, name))
        || node.children().any(|child| names_name(src, child, name))
}

fn names_token(src: Src<'_>, token: hd_base::TokenIdx, name: &str) -> bool {
    matches!(
        src.tkind(token),
        Some(TokenKind::Ident | TokenKind::RawIdent | TokenKind::KwSelfValue)
    ) && src.text(token) == name
}

fn captures_name(src: Src<'_>, node: NodeRef<'_>, name: &str) -> bool {
    if node.kind() == SyntaxKind::ClosureExpr {
        names_name(src, node, name)
    } else {
        node.children().any(|child| captures_name(src, child, name))
    }
}

/// The single registered consumer used by `hd lint`.
pub struct Analyzer;

pub static ANALYZER: Analyzer = Analyzer;

impl AnalysisSink for Analyzer {
    fn requests(&self) -> Requests {
        interests().requests()
    }

    fn diagnostic_code(&self, name: &str) -> Option<Code> {
        RULES
            .iter()
            .find(|rule| rule.name() == name)
            .map(|rule| rule.code())
    }

    fn module(&self, module: &Module<'_>, out: &mut DiagBuf) {
        walk(module, interests(), out, &mut dispatch);
    }

    fn fetched_debug(&self, package: &str, span: Span, src: Src<'_>, out: &mut DiagBuf) {
        let cx = Cx {
            src,
            checked: CheckedView::new(src, None),
        };
        dispatch(&Event::FetchedDebug { package, span }, &cx, out);
    }
}

/// The event families some rule reads.
fn interests() -> Events {
    RULES.iter().fold(Events::default(), |events, rule| {
        rule.events.map_or(events, |more| events.union(more))
    })
}

/// Runs each rule that reads `event`'s family.
fn dispatch<'v>(event: &Event<'v>, cx: &Cx<'v>, out: &mut DiagBuf) {
    let family = event.family();
    for rule in RULES
        .iter()
        .filter(|rule| rule.events.is_some_and(|events| events.contains(family)))
    {
        (rule.check)(event, cx, out);
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

fn unsigned_comparison(event: &Event<'_>, cx: &Cx<'_>, out: &mut DiagBuf) {
    let Event::Comparison(comparison) = event else {
        return;
    };
    let src = cx.src;
    let node = comparison.node();
    let mut operands = node.children();
    let (Some(left), Some(right)) = (operands.next(), operands.next()) else {
        return;
    };
    // The parser builds a binary node as left operand, operator, right.
    let operator = src
        .parse
        .tokens
        .kind(hd_base::TokenIdx::from_raw(src.last(left).raw() + 1));
    let zero = |operand: NodeRef<'_>| zero_literal(src, operand);
    let (on_right, always, operator) = match operator {
        TokenKind::GtEq if zero(right) => (false, true, ">="),
        TokenKind::LtEq if zero(left) => (true, true, "<="),
        TokenKind::Lt if zero(right) => (false, false, "<"),
        TokenKind::Gt if zero(left) => (true, false, ">"),
        _ => return,
    };
    let Some(primitive) = comparison
        .primitive(on_right)
        .filter(|primitive| primitive.is_unsigned())
    else {
        return;
    };
    let operand = if on_right { right } else { left };
    let name = cx.text_at(src.span(operand)).unwrap_or("the value");
    let form = if on_right {
        format!("0 {operator} {name}")
    } else {
        format!("{name} {operator} 0")
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
        src.span(node),
        &message,
        None,
    );
}

/// Whether a node is an unsuffixed integer literal with the value zero, in
/// any radix (`expr.ord.unsigned-zero.literal`).
fn zero_literal(src: Src<'_>, n: NodeRef<'_>) -> bool {
    if n.kind() != SyntaxKind::LiteralExpr {
        return false;
    }
    let first = src.first(n);
    if src.tkind(first) != Some(TokenKind::Number) {
        return false;
    }
    let (digits, suffix) = literal::split_suffix(src.text(first));
    if !suffix.is_empty() {
        return false;
    }
    let digits = digits.replace('_', "");
    if let Some((radix, rest)) = literal::radix_digits(&digits) {
        return u64::from_str_radix(rest, radix).ok() == Some(0);
    }
    literal::decimal_digits(&digits) && digits.parse::<u64>().ok() == Some(0)
}

fn unused_binding(event: &Event<'_>, _: &Cx<'_>, out: &mut DiagBuf) {
    let Event::Binding(binding) = event else {
        return;
    };
    // Of the unread bindings on one line, only the first is reported.
    if binding.view.is_read() || binding.name.starts_with('_') || !binding.first_unread_on_line {
        return;
    }
    let name = binding.name;
    out.push(
        Code::UnusedLocalBinding,
        Severity::Warning,
        binding.span,
        &format!("`{name}` is never read; name it `_{name}` to keep it"),
        None,
    );
}

fn redundant_mut(event: &Event<'_>, cx: &Cx<'_>, out: &mut DiagBuf) {
    let Event::Binding(binding) = event else {
        return;
    };
    let Some(fact) = binding.view.mutability() else {
        return;
    };
    if fact.mutable_address_taken() {
        return;
    }
    if !fact.is_primitive() {
        return;
    }
    let message = "`let mut` on a primitive is needed only when `&mut` of it is taken; a plain `let` is already reassignable";
    out.push(
        Code::RedundantLetMut,
        Severity::Warning,
        binding.span,
        message,
        None,
    );
    let src = cx.src;
    if let Some(token) = fact
        .pattern()
        .direct_token(&src.parse.tokens, TokenKind::KwMut)
    {
        // The keyword and the whitespace after it.
        let lo = src.token_span(token).lo;
        let next = hd_base::TokenIdx::from_raw(token.raw() + 1);
        let span = hd_base::Span {
            file: src.file,
            lo,
            hi: src.token_span(next).lo.max(lo),
        };
        out.add_fix("remove `mut`", FixSafety::Exact, &[(span, "")]);
    }
}

fn variant_binding(event: &Event<'_>, cx: &Cx<'_>, out: &mut DiagBuf) {
    let Event::Pattern(pattern) = event else {
        return;
    };
    let src = cx.src;
    for (position, sub) in pattern.subpatterns().enumerate() {
        // A bare name binding directly in a payload position.
        let Some(sub) = sub.filter(|sub| {
            sub.kind() == SyntaxKind::BindingPattern && sub.children().next().is_none()
        }) else {
            continue;
        };
        let Some(token) = sub.direct_tokens().find(|token| {
            matches!(
                src.tkind(*token),
                Some(TokenKind::Ident | TokenKind::RawIdent)
            )
        }) else {
            continue;
        };
        let name = src.text(token);
        let Some(field) = pattern.fields().get(position) else {
            continue;
        };
        if name == *field
            || !pattern
                .fields()
                .iter()
                .enumerate()
                .any(|(i, other)| i != position && *other == name)
        {
            continue;
        }
        out.push(
            Code::VariantBindingNameMismatch,
            Severity::Warning,
            src.span(sub),
            &format!(
                "`{name}` is also a field of `{}`; the field in this position is `{field}`",
                pattern.variant()
            ),
            None,
        );
    }
}

fn unreachable(event: &Event<'_>, cx: &Cx<'_>, out: &mut DiagBuf) {
    let Event::Unreachable(statement) = event else {
        return;
    };
    out.push(
        Code::UnreachableCode,
        Severity::Warning,
        cx.src.span(*statement),
        "this statement is unreachable",
        None,
    );
}

fn unused_debug(event: &Event<'_>, cx: &Cx<'_>, out: &mut DiagBuf) {
    let Event::Discard(call) = event else {
        return;
    };
    if !call
        .selected_declaration()
        .is_some_and(hd_check::analysis::DeclRef::is_prelude_debug)
    {
        return;
    }
    out.push(
        Code::UnusedDebugText,
        Severity::Warning,
        cx.src.span(call.node()),
        "a `debug(...)` statement discards its text; use `dbg(...)` to print it",
        None,
    );
    let Some(shape) = CallShape::of(call.node()) else {
        return;
    };
    let span = cx.src.span(shape.name);
    if cx.text_at(span) == Some("debug")
        && shape.positional == 1
        && shape.named == 0
        && !shape.spread
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

/// The written shape of a direct call by a bare name.
struct CallShape<'v> {
    name: NodeRef<'v>,
    positional: usize,
    named: usize,
    /// A spread argument, with or without arguments after it.
    spread: bool,
}

impl<'v> CallShape<'v> {
    fn of(call: NodeRef<'v>) -> Option<Self> {
        if call.kind() != SyntaxKind::CallExpr {
            return None;
        }
        let name = call
            .children()
            .next()
            .filter(|callee| callee.kind() == SyntaxKind::NameExpr)?;
        let mut shape = Self {
            name,
            positional: 0,
            named: 0,
            spread: false,
        };
        for argument in Src::child(call, SyntaxKind::ArgumentList)
            .iter()
            .flat_map(|list| list.children())
        {
            // Arguments after a spread do not change the shape the fix needs.
            if argument.kind() == SyntaxKind::NamedArgument {
                shape.named += 1;
            } else if argument.kind() == SyntaxKind::Argument && !shape.spread {
                if argument
                    .children()
                    .next()
                    .is_some_and(|e| e.kind() == SyntaxKind::SpreadExpr)
                {
                    shape.spread = true;
                } else {
                    shape.positional += 1;
                }
            }
        }
        Some(shape)
    }
}
