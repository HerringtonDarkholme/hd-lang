//! Optional post-verification analysis dispatch.

use hd_base::SliceSort as _;
use hd_base::{Span, TokenIdx};
use hd_check::analysis::CheckedView;
use hd_diag::DiagBuf;
use hd_resolve::Src;
use hd_syntax::{Decl, SyntaxKind, TokenKind};

/// Source event families requested by an analysis consumer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Events(u8);

impl Events {
    pub const IDENT: Self = Self(1 << 0);
    pub const DECL: Self = Self(1 << 1);
    pub const MODULE: Self = Self(1 << 2);
    pub const ALL: Self = Self(Self::IDENT.0 | Self::DECL.0 | Self::MODULE.0);

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// A declaration after its complete source subtree has been visited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclEvent {
    pub kind: SyntaxKind,
    pub span: Span,
    pub name: Option<Span>,
    pub public: bool,
}

/// A module after all of its source and declaration events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleEvent<'v> {
    pub path: &'v str,
    pub span: Span,
}

/// One neutral event in the verified module's source-order walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event<'v> {
    Ident(Span),
    Decl(DeclEvent),
    Module(ModuleEvent<'v>),
}

impl Event<'_> {
    #[must_use]
    pub const fn family(self) -> Events {
        match self {
            Self::Ident(_) => Events::IDENT,
            Self::Decl(_) => Events::DECL,
            Self::Module(_) => Events::MODULE,
        }
    }
}

/// A read-only consumer of one verified, source-order module walk.
pub trait AnalysisSink: Sync {
    fn interests(&self) -> Events;
    fn event(&self, event: &Event<'_>, view: &CheckedView<'_>, out: &mut DiagBuf);
}

enum Pending {
    Ident(Span),
    Decl(DeclEvent),
}

impl Pending {
    const fn key(&self) -> (u32, u8) {
        match self {
            Self::Ident(span) => (span.lo, 0),
            // Declaration events are finish events, after everything whose
            // source starts before the declaration's end.
            Self::Decl(event) => (event.span.hi, 1),
        }
    }
}

pub(crate) fn walk(
    sink: &dyn AnalysisSink,
    interests: Events,
    src: Src<'_>,
    module_path: &str,
    out: &mut DiagBuf,
) {
    let view = CheckedView::from_source(src);
    let mut pending = Vec::new();
    if interests.contains(Events::IDENT) {
        for raw in 0..src.parse.tokens.len() {
            let token = TokenIdx::from_raw(u32::try_from(raw).expect("source tokens"));
            if src.parse.tokens.kind(token) == TokenKind::Ident {
                pending.push(Pending::Ident(src.token_span(token)));
            }
        }
    }
    if interests.contains(Events::DECL) {
        for node in src.root().descendants() {
            let Some(decl) = Decl::cast(node) else {
                continue;
            };
            pending.push(Pending::Decl(DeclEvent {
                kind: node.kind(),
                span: src.span(node),
                name: decl
                    .name(&src.parse.tokens)
                    .map(|name| src.token_span(name)),
                public: decl.is_pub(&src.parse.tokens),
            }));
        }
    }
    pending.sort_stable_by_key(Pending::key);
    for event in pending {
        let event = match event {
            Pending::Ident(span) => Event::Ident(span),
            Pending::Decl(event) => Event::Decl(event),
        };
        sink.event(&event, &view, out);
    }
    if interests.contains(Events::MODULE) {
        sink.event(
            &Event::Module(ModuleEvent {
                path: module_path,
                span: src.span(src.root()),
            }),
            &view,
            out,
        );
    }
}
