//! One verified module's lint walk: every requested fact as an event, in
//! source order, then the module itself.
//!
//! Cost per module: one pass over the tokens for identifier events, one
//! pass over each checked fact family, one pass over the bindings to find
//! the first unread one per body line, and one stable sort of the events,
//! O(n log n) in the events.

use crate::checked::{BindingView, BodyRef, CallView, CheckedView, ComparisonView, PatternView};
use hd_base::{HashMap, SliceSort as _, Span, TokenIdx};
use hd_diag::DiagBuf;
use hd_driver::analysis::{Module, Requests};
use hd_project::MainSite;
use hd_resolve::Src;
use hd_resolve::facts::{DeclView, DerivationFacts};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};

/// Event families a rule reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Events(u16);

impl Events {
    pub const IDENT: Self = Self(1 << 0);
    pub const MODULE: Self = Self(1 << 1);
    pub const COMPARISON: Self = Self(1 << 2);
    pub const BINDING: Self = Self(1 << 3);
    pub const STATEMENT: Self = Self(1 << 4);
    pub const PATTERN: Self = Self(1 << 5);
    pub const DISCARD: Self = Self(1 << 6);
    pub const DERIVATIONS: Self = Self(1 << 7);
    pub const MAIN_SITE: Self = Self(1 << 8);
    pub const FETCHED_DEBUG: Self = Self(1 << 9);
    pub const PARAMETER_WRITE: Self = Self(1 << 10);
    const CHECKED: Self = Self(
        Self::COMPARISON.0
            | Self::BINDING.0
            | Self::STATEMENT.0
            | Self::PATTERN.0
            | Self::DISCARD.0
            | Self::PARAMETER_WRITE.0,
    );

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// What the driver collects for these families.
    #[must_use]
    pub const fn requests(self) -> Requests {
        let mut requests = Requests::NONE;
        if self.intersects(Self(Self::IDENT.0 | Self::MODULE.0)) {
            requests = requests.union(Requests::MODULE);
        }
        if self.intersects(Self::CHECKED) {
            requests = requests.union(Requests::CHECKED);
        }
        if self.contains(Self::DERIVATIONS) {
            requests = requests.union(Requests::DERIVATIONS);
        }
        if self.contains(Self::MAIN_SITE) {
            requests = requests.union(Requests::MAIN_SITE);
        }
        if self.contains(Self::FETCHED_DEBUG) {
            requests = requests.union(Requests::FETCHED_DEBUG);
        }
        requests
    }
}

/// A checked binding with its written name and span.
#[derive(Clone, Copy)]
pub struct BindingEvent<'v> {
    pub view: BindingView<'v>,
    pub name: &'v str,
    pub span: Span,
    /// No unread binding before it in its body starts on its line.
    pub first_unread_on_line: bool,
}

/// A module's derivation facts, with the scope its written names resolve in.
#[derive(Clone, Copy)]
pub struct Derivations<'v> {
    pub facts: &'v DerivationFacts,
    pub decls: DeclView<'v>,
}

/// One event of a module walk.
#[derive(Clone, Copy)]
pub enum Event<'v> {
    Ident(Span),
    Module,
    Comparison(ComparisonView<'v>),
    Binding(BindingEvent<'v>),
    Unreachable(NodeRef<'v>),
    ParameterWrite(crate::ParameterWriteEvent<'v>),
    Pattern(PatternView<'v>),
    Discard(CallView<'v>),
    Derivations(Derivations<'v>),
    MainSite(MainSite),
    /// The first cached `dbg` reference found in one fetched package.
    FetchedDebug {
        package: &'v str,
        span: Span,
    },
}

impl Event<'_> {
    #[must_use]
    pub const fn family(&self) -> Events {
        match self {
            Self::Ident(_) => Events::IDENT,
            Self::Module => Events::MODULE,
            Self::Comparison(_) => Events::COMPARISON,
            Self::Binding(_) => Events::BINDING,
            Self::Unreachable(_) => Events::STATEMENT,
            Self::ParameterWrite(_) => Events::PARAMETER_WRITE,
            Self::Pattern(_) => Events::PATTERN,
            Self::Discard(_) => Events::DISCARD,
            Self::Derivations(_) => Events::DERIVATIONS,
            Self::MainSite(_) => Events::MAIN_SITE,
            Self::FetchedDebug { .. } => Events::FETCHED_DEBUG,
        }
    }
}

/// The module an event belongs to: its syntax and its checked facts.
#[derive(Clone, Copy)]
pub struct Cx<'v> {
    pub src: Src<'v>,
    pub checked: CheckedView<'v>,
}

impl<'v> Cx<'v> {
    #[must_use]
    pub fn text_at(&self, span: Span) -> Option<&'v str> {
        (span.file == self.src.file)
            .then(|| self.src.text.get(span.lo as usize..span.hi as usize))
            .flatten()
    }
}

/// A binding's name as the checker spells it: its first direct identifier,
/// unquoted, except a data pattern field's, which keeps its quotes.
#[must_use]
pub fn binding_name<'v>(src: Src<'v>, node: NodeRef<'_>) -> &'v str {
    node.name(&src.parse.tokens).map_or("", |token| {
        let text = src.text(token);
        if node.kind() == SyntaxKind::DataPatternField {
            text
        } else {
            text.trim_matches('`')
        }
    })
}

struct Pending<'v> {
    at: u32,
    finish: bool,
    family: u8,
    event: Event<'v>,
}

/// Sends each requested event of `module`, in source order, then the
/// module event, to `sink`.
pub fn walk<'v>(
    module: &Module<'v>,
    interests: Events,
    out: &mut DiagBuf,
    sink: &mut dyn FnMut(&Event<'v>, &Cx<'v>, &mut DiagBuf),
) {
    let src = module.src;
    let cx = Cx {
        src,
        checked: CheckedView::new(src, module.facts),
    };
    let at = |node: NodeRef<'_>| src.span(node).lo;
    let mut pending = Vec::new();
    if interests.contains(Events::IDENT) {
        for raw in 0..src.parse.tokens.len() {
            // ruled out: a parse holds fewer than `u32::MAX` tokens, its
            // spans being `u32` offsets.
            let token = TokenIdx::from_raw(u32::try_from(raw).expect("source tokens"));
            if src.parse.tokens.kind(token) == TokenKind::Ident {
                let span = src.token_span(token);
                pending.push(Pending {
                    at: span.lo,
                    finish: false,
                    family: 6,
                    event: Event::Ident(span),
                });
            }
        }
    }
    if interests.contains(Events::MAIN_SITE)
        && let Some(site) = module.main_site
    {
        pending.push(Pending {
            at: site.span.lo,
            finish: false,
            family: 5,
            event: Event::MainSite(site),
        });
    }
    let checked = cx.checked;
    if interests.contains(Events::STATEMENT) {
        pending.extend(checked.unreachable().map(|node| Pending {
            at: at(node),
            finish: false,
            family: 0,
            event: Event::Unreachable(node),
        }));
    }
    if interests.contains(Events::BINDING) {
        let first = first_unread(src, checked);
        pending.extend(checked.bindings().map(|view| {
            let node = view.node();
            let span = src.span(node);
            Pending {
                at: span.lo,
                finish: false,
                family: 1,
                event: Event::Binding(BindingEvent {
                    view,
                    name: binding_name(src, view.name_node()),
                    span,
                    first_unread_on_line: first
                        .get(&(view.owner(), line_of(src.text, span)))
                        .is_none_or(|held| !held.precedes(view)),
                }),
            }
        }));
    }
    if interests.contains(Events::PARAMETER_WRITE) {
        pending.extend(
            crate::parameter_writes(src, checked)
                .into_iter()
                .map(|write| Pending {
                    at: at(write.statement),
                    finish: false,
                    family: 5,
                    event: Event::ParameterWrite(write),
                }),
        );
    }
    if interests.contains(Events::PATTERN) {
        pending.extend(checked.patterns().map(|pattern| Pending {
            at: at(pattern.node()),
            finish: false,
            family: 2,
            event: Event::Pattern(pattern),
        }));
    }
    if interests.contains(Events::COMPARISON) {
        pending.extend(checked.comparisons().map(|comparison| Pending {
            at: at(comparison.node()),
            finish: false,
            family: 3,
            event: Event::Comparison(comparison),
        }));
    }
    if interests.contains(Events::DISCARD) {
        pending.extend(checked.discarded().map(|call| Pending {
            at: at(call.node()),
            finish: false,
            family: 4,
            event: Event::Discard(call),
        }));
    }
    if interests.contains(Events::DERIVATIONS)
        && let (Some(facts), Some(decls)) = (module.derivations, module.decls)
    {
        pending.push(Pending {
            // ruled out: source text is shorter than `u32::MAX` bytes, its
            // spans being `u32` offsets.
            at: u32::try_from(src.text.len()).expect("source length"),
            finish: true,
            family: 1,
            event: Event::Derivations(Derivations { facts, decls }),
        });
    }
    pending.sort_stable_by_key(|p| (p.at, p.finish, p.family));
    for pending in pending {
        sink(&pending.event, &cx, out);
    }
    if interests.contains(Events::MODULE) {
        sink(&Event::Module, &cx, out);
    }
}

/// The byte offset of the line break before `span`, which names its line.
fn line_of(text: &str, span: Span) -> Option<usize> {
    let lo = (span.lo as usize).min(text.len());
    text.get(..lo).and_then(|prefix| prefix.rfind('\n'))
}

/// The first unread, not underscore-named binding of each body line, in one
/// pass over the bindings with a map keyed by body and line.
fn first_unread<'v>(
    src: Src<'v>,
    checked: CheckedView<'v>,
) -> HashMap<(BodyRef<'v>, Option<usize>), BindingView<'v>> {
    let mut first: HashMap<_, BindingView<'v>> = HashMap::default();
    for view in checked.bindings() {
        let node = view.node();
        if view.is_read() || binding_name(src, view.name_node()).starts_with('_') {
            continue;
        }
        let key = (view.owner(), line_of(src.text, src.span(node)));
        first
            .entry(key)
            .and_modify(|held| {
                if view.precedes(*held) {
                    *held = view;
                }
            })
            .or_insert(view);
    }
    first
}
