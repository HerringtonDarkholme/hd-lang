//! Borrowed views exposed only while optional post-check analysis runs.
//!
//! The view owns no compiler identifiers or inferred state. Later analysis
//! slices add narrow checked queries here instead of exporting `BodyCx`.

use hd_base::{FileId, Span};
use hd_resolve::Src;
use hd_syntax::{NodeRef, SyntaxKind};

/// The completed compiler state visible during one analysis callback.
#[derive(Clone, Copy)]
pub struct CheckedView<'v> {
    source: SourceView<'v>,
}

impl<'v> CheckedView<'v> {
    /// Construct the callback view from the checked module's frozen source.
    /// Tooling receives this through the driver rather than retaining it.
    #[doc(hidden)]
    #[must_use]
    pub fn from_source(source: Src<'v>) -> Self {
        Self {
            source: SourceView { source },
        }
    }

    #[must_use]
    pub const fn source(self) -> SourceView<'v> {
        self.source
    }
}

/// Syntax and text for the source snapshot whose module was checked.
#[derive(Clone, Copy)]
pub struct SourceView<'v> {
    source: Src<'v>,
}

impl<'v> SourceView<'v> {
    #[must_use]
    pub const fn file(self) -> FileId {
        self.source.file
    }

    #[must_use]
    pub fn root(self) -> NodeRef<'v> {
        self.source.root()
    }

    #[must_use]
    pub fn kind(self, node: NodeRef<'_>) -> SyntaxKind {
        node.kind()
    }

    #[must_use]
    pub fn span(self, node: NodeRef<'_>) -> Span {
        self.source.span(node)
    }

    #[must_use]
    pub const fn text(self) -> &'v str {
        self.source.text
    }

    #[must_use]
    pub fn text_at(self, span: Span) -> Option<&'v str> {
        (span.file == self.source.file)
            .then(|| self.source.text.get(span.lo as usize..span.hi as usize))
            .flatten()
    }
}
