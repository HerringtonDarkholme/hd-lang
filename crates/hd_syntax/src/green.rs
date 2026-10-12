use std::fmt::Write;

use hd_base::{NodeIdx, TokenIdx};
use hd_diag::Code;

use crate::{Layout, TokenBuf, TokenKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum SyntaxKind {
    Root,
    // Items and their parts.
    UseDecl,
    UseGroup,
    UseItem,
    FnDecl,
    DataDecl,
    EnumDecl,
    TraitDecl,
    ImplDecl,
    TypeDecl,
    TestsBlock,
    Decorator,
    ParameterList,
    Parameter,
    DefaultValue,
    GenericParameterList,
    GenericParameter,
    BoundList,
    TypeDefault,
    TypeArgumentList,
    AssociatedTypeBinding,
    ArgumentList,
    Argument,
    NamedArgument,
    DataField,
    EmbeddedField,
    EnumVariant,
    VariantSharedData,
    AssociatedTypeDecl,
    Derivation,
    // Statements and suites.
    Block,
    LetStmt,
    DiscardStmt,
    AssignmentStmt,
    ReturnStmt,
    BreakStmt,
    ContinueStmt,
    DeferStmt,
    ExprStmt,
    ElseClause,
    MatchArm,
    MatchGuard,
    // Expressions. A binary node keeps its operator token; the kind records
    // the precedence level.
    BindingExpr,
    RangeExpr,
    LogicalOrExpr,
    LogicalAndExpr,
    ComparisonExpr,
    PipeExpr,
    BitwiseOrExpr,
    BitwiseXorExpr,
    BitwiseAndExpr,
    ShiftExpr,
    AdditiveExpr,
    MultiplicativeExpr,
    UnaryExpr,
    PowerExpr,
    CallExpr,
    IndexExpr,
    FieldExpr,
    TryExpr,
    SuspendExpr,
    PathExpr,
    TypeArgsExpr,
    TrailingCallExpr,
    LiteralExpr,
    NameExpr,
    VariantExpr,
    PlaceholderExpr,
    ParenExpr,
    TupleExpr,
    ListExpr,
    MapExpr,
    MapEntry,
    DataExpr,
    DataFieldInit,
    SpreadExpr,
    ClosureExpr,
    IfExpr,
    ForExpr,
    WhileExpr,
    MatchExpr,
    ComprehensionExpr,
    ComprehensionFor,
    ComprehensionIf,
    ContextExpr,
    ContextEntry,
    StringExpr,
    Interpolation,
    // Patterns.
    WildcardPattern,
    BindingPattern,
    LiteralPattern,
    VariantPattern,
    PatternArgumentList,
    NamedPattern,
    DataPattern,
    DataPatternField,
    TuplePattern,
    SpreadPattern,
    RangePattern,
    // Types.
    NamedType,
    DynType,
    MutType,
    OptionalType,
    TupleType,
    ParenType,
    FunctionType,
    ProjectionType,
    ContextType,
    RestType,
    InferType,
    RequirementRow,
    Error,
    SkippedBody,
}

const _: () = assert!(core::mem::size_of::<SyntaxKind>() == 2);

#[derive(Clone, Debug, Default)]
pub struct GreenTree {
    kind: Vec<SyntaxKind>,
    first_token: Vec<TokenIdx>,
    last_token: Vec<TokenIdx>,
    subtree_len: Vec<u32>,
    pub layout_at: Vec<TokenIdx>,
    pub layout_kind: Vec<Layout>,
    pub err_node: Vec<NodeIdx>,
    pub err_code: Vec<Code>,
}

impl GreenTree {
    #[must_use]
    pub fn len(&self) -> usize {
        self.kind.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.kind.is_empty()
    }

    #[must_use]
    pub fn root(&self) -> NodeRef<'_> {
        self.node(NodeIdx::from_raw(0))
    }

    #[must_use]
    pub fn node(&self, index: NodeIdx) -> NodeRef<'_> {
        NodeRef { tree: self, index }
    }

    #[must_use]
    pub fn kind(&self, index: NodeIdx) -> SyntaxKind {
        self.kind[index.idx()]
    }

    #[must_use]
    pub fn span_tokens(&self, index: NodeIdx) -> (TokenIdx, TokenIdx) {
        (self.first_token[index.idx()], self.last_token[index.idx()])
    }

    #[must_use]
    pub fn parent_links(&self) -> Vec<NodeIdx> {
        let mut parents = vec![NodeIdx::NONE; self.len()];
        for parent in 0..self.len() {
            let parent_idx = NodeIdx::from_raw(as_u32(parent));
            let mut child = parent + 1;
            let end = parent + self.subtree_len[parent] as usize;
            while child < end {
                parents[child] = parent_idx;
                child += self.subtree_len[child] as usize;
            }
        }
        parents
    }

    #[must_use]
    pub fn debug_tree(&self, tokens: &TokenBuf, source: &str) -> String {
        let mut output = String::new();
        let parents = self.parent_links();
        for index in 0..self.len() {
            let mut depth = 0;
            let mut at = parents[index];
            while let Some(parent) = at.get() {
                depth += 1;
                at = parents[parent.idx()];
            }
            let node = NodeIdx::from_raw(as_u32(index));
            let (first, last) = self.span_tokens(node);
            let text = if first.get().is_some() && last.get().is_some() {
                let lo = tokens.start[first.idx()] as usize;
                let hi = tokens.end[last.idx()] as usize;
                source[lo..hi].lines().next().unwrap_or_default()
            } else {
                ""
            };
            let _ = writeln!(
                output,
                "{}{:?} {:?}..{:?} {text:?}",
                "  ".repeat(depth),
                self.kind(node),
                first.get().map(TokenIdx::raw),
                last.get().map(TokenIdx::raw)
            );
        }
        output
    }

    /// A snapshot form: one node per line, indented by depth, with the
    /// node's own tokens (not its children's) after its kind.
    #[must_use]
    pub fn outline(&self, tokens: &TokenBuf, source: &str) -> String {
        fn walk(
            node: NodeRef<'_>,
            depth: usize,
            tokens: &TokenBuf,
            source: &str,
            out: &mut String,
        ) {
            let own: Vec<&str> = node
                .direct_tokens()
                .map(|token| tokens.text(token, source))
                .collect();
            let _ = write!(out, "{}{:?}", "  ".repeat(depth), node.kind());
            if !own.is_empty() {
                let _ = write!(out, " {}", own.join(" ").replace('\n', "\\n"));
            }
            out.push('\n');
            for child in node.children() {
                walk(child, depth + 1, tokens, source, out);
            }
        }
        let mut out = String::new();
        if !self.is_empty() {
            walk(self.root(), 0, tokens, source, &mut out);
        }
        out
    }

    #[must_use]
    pub fn reconstruct(&self, tokens: &TokenBuf, source: &str) -> String {
        tokens.reconstruct(source)
    }

    #[must_use]
    pub fn to_wire(&self, tokens: &TokenBuf, source: &str) -> Vec<u8> {
        const SECTIONS: usize = 16;
        let mut output = vec![0_u8; 8 + SECTIONS * 4];
        output[0..4].copy_from_slice(b"HDST");
        output[4..6].copy_from_slice(&1_u16.to_le_bytes());
        output[6..8].copy_from_slice(&1_u16.to_le_bytes());
        let mut offsets = Vec::with_capacity(SECTIONS);
        let mut section = |bytes: &[u8]| {
            while !output.len().is_multiple_of(8) {
                output.push(0);
            }
            offsets.push(as_u32(output.len()));
            output.extend_from_slice(bytes);
        };
        section(
            &tokens
                .kind
                .iter()
                .map(|kind| *kind as u8)
                .collect::<Vec<_>>(),
        );
        section(&u32_bytes(&tokens.start));
        section(&u32_bytes(&tokens.end));
        section(&u32_bytes(&tokens.line_first));
        section(&u32_bytes(&tokens.line_start));
        let utf16 = utf16_line_starts(source, &tokens.line_start);
        section(&u32_bytes(&utf16));
        section(
            &tokens
                .line_flags
                .iter()
                .map(|flags| flags.0)
                .collect::<Vec<_>>(),
        );
        let comments: Vec<u32> = tokens
            .com_start
            .iter()
            .zip(&tokens.com_end)
            .zip(&tokens.com_kind)
            .flat_map(|((&start, &end), &kind)| [start, end, kind as u32])
            .collect();
        section(&u32_bytes(&comments));
        section(&u16_bytes(
            &self
                .kind
                .iter()
                .map(|kind| *kind as u16)
                .collect::<Vec<_>>(),
        ));
        section(&u32_bytes(
            &self
                .first_token
                .iter()
                .map(|token| token.raw())
                .collect::<Vec<_>>(),
        ));
        section(&u32_bytes(
            &self
                .last_token
                .iter()
                .map(|token| token.raw())
                .collect::<Vec<_>>(),
        ));
        section(&u32_bytes(&self.subtree_len));
        let layouts: Vec<u32> = self
            .layout_at
            .iter()
            .zip(&self.layout_kind)
            .flat_map(|(&at, &kind)| [at.raw(), kind as u32])
            .collect();
        section(&u32_bytes(&layouts));
        let errors: Vec<u32> = self
            .err_node
            .iter()
            .zip(&self.err_code)
            .flat_map(|(&node, &code)| [node.raw(), code as u32])
            .collect();
        section(&u32_bytes(&errors));
        section(source.as_bytes());
        section(&[]);
        for (index, offset) in offsets.into_iter().enumerate() {
            let start = 8 + index * 4;
            output[start..start + 4].copy_from_slice(&offset.to_le_bytes());
        }
        output
    }
}

#[derive(Clone, Copy)]
pub struct NodeRef<'t> {
    tree: &'t GreenTree,
    index: NodeIdx,
}

impl<'t> NodeRef<'t> {
    #[must_use]
    pub fn index(self) -> NodeIdx {
        self.index
    }

    #[must_use]
    pub fn kind(self) -> SyntaxKind {
        self.tree.kind(self.index)
    }

    pub fn children(self) -> impl Iterator<Item = NodeRef<'t>> {
        let start = self.index.idx() + 1;
        let end = self.index.idx() + self.tree.subtree_len[self.index.idx()] as usize;
        ChildIter {
            tree: self.tree,
            next: start,
            end,
        }
    }

    pub fn descendants(self) -> impl Iterator<Item = NodeRef<'t>> {
        let start = self.index.idx() + 1;
        let end = self.index.idx() + self.tree.subtree_len[self.index.idx()] as usize;
        (start..end).map(|index| NodeRef {
            tree: self.tree,
            index: NodeIdx::from_raw(as_u32(index)),
        })
    }
}

struct ChildIter<'t> {
    tree: &'t GreenTree,
    next: usize,
    end: usize,
}

impl<'t> Iterator for ChildIter<'t> {
    type Item = NodeRef<'t>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.end {
            return None;
        }
        let index = self.next;
        self.next += self.tree.subtree_len[index] as usize;
        Some(NodeRef {
            tree: self.tree,
            index: NodeIdx::from_raw(as_u32(index)),
        })
    }
}

pub trait AstNode<'t>: Copy {
    const KIND: SyntaxKind;

    fn cast(node: NodeRef<'t>) -> Option<Self>;

    fn node(self) -> NodeRef<'t>;
}

macro_rules! ast_nodes {
    ($( $name:ident => $kind:ident ),+ $(,)?) => {$ (
        #[derive(Clone, Copy)]
        pub struct $name<'t>(NodeRef<'t>);

        impl<'t> AstNode<'t> for $name<'t> {
            const KIND: SyntaxKind = SyntaxKind::$kind;

            fn cast(node: NodeRef<'t>) -> Option<Self> {
                (node.kind() == Self::KIND).then_some(Self(node))
            }

            fn node(self) -> NodeRef<'t> {
                self.0
            }
        }

        impl<'t> $name<'t> {
            #[must_use]
            pub fn cast(node: NodeRef<'t>) -> Option<Self> {
                <Self as AstNode>::cast(node)
            }

            #[must_use]
            pub fn node(self) -> NodeRef<'t> {
                self.0
            }
        }
    )+ };
}

ast_nodes! {
    SourceFile => Root,
    UseDecl => UseDecl,
    UseGroup => UseGroup,
    UseItem => UseItem,
    FnDecl => FnDecl,
    DataDecl => DataDecl,
    EnumDecl => EnumDecl,
    TraitDecl => TraitDecl,
    ImplDecl => ImplDecl,
    TypeDecl => TypeDecl,
    TestsBlock => TestsBlock,
    Decorator => Decorator,
    ParameterList => ParameterList,
    Parameter => Parameter,
    DefaultValue => DefaultValue,
    GenericParameterList => GenericParameterList,
    GenericParameter => GenericParameter,
    BoundList => BoundList,
    TypeDefault => TypeDefault,
    TypeArgumentList => TypeArgumentList,
    AssociatedTypeBinding => AssociatedTypeBinding,
    ArgumentList => ArgumentList,
    Argument => Argument,
    NamedArgument => NamedArgument,
    DataField => DataField,
    EmbeddedField => EmbeddedField,
    EnumVariant => EnumVariant,
    VariantSharedData => VariantSharedData,
    AssociatedTypeDecl => AssociatedTypeDecl,
    Derivation => Derivation,
    Block => Block,
    LetStmt => LetStmt,
    DiscardStmt => DiscardStmt,
    AssignmentStmt => AssignmentStmt,
    ReturnStmt => ReturnStmt,
    BreakStmt => BreakStmt,
    ContinueStmt => ContinueStmt,
    DeferStmt => DeferStmt,
    ExprStmt => ExprStmt,
    ElseClause => ElseClause,
    MatchArm => MatchArm,
    MatchGuard => MatchGuard,
    BindingExpr => BindingExpr,
    RangeExpr => RangeExpr,
    LogicalOrExpr => LogicalOrExpr,
    LogicalAndExpr => LogicalAndExpr,
    ComparisonExpr => ComparisonExpr,
    PipeExpr => PipeExpr,
    BitwiseOrExpr => BitwiseOrExpr,
    BitwiseXorExpr => BitwiseXorExpr,
    BitwiseAndExpr => BitwiseAndExpr,
    ShiftExpr => ShiftExpr,
    AdditiveExpr => AdditiveExpr,
    MultiplicativeExpr => MultiplicativeExpr,
    UnaryExpr => UnaryExpr,
    PowerExpr => PowerExpr,
    CallExpr => CallExpr,
    IndexExpr => IndexExpr,
    FieldExpr => FieldExpr,
    TryExpr => TryExpr,
    SuspendExpr => SuspendExpr,
    PathExpr => PathExpr,
    TypeArgsExpr => TypeArgsExpr,
    TrailingCallExpr => TrailingCallExpr,
    LiteralExpr => LiteralExpr,
    NameExpr => NameExpr,
    VariantExpr => VariantExpr,
    PlaceholderExpr => PlaceholderExpr,
    ParenExpr => ParenExpr,
    TupleExpr => TupleExpr,
    ListExpr => ListExpr,
    MapExpr => MapExpr,
    MapEntry => MapEntry,
    DataExpr => DataExpr,
    DataFieldInit => DataFieldInit,
    SpreadExpr => SpreadExpr,
    ClosureExpr => ClosureExpr,
    IfExpr => IfExpr,
    ForExpr => ForExpr,
    WhileExpr => WhileExpr,
    MatchExpr => MatchExpr,
    ComprehensionExpr => ComprehensionExpr,
    ComprehensionFor => ComprehensionFor,
    ComprehensionIf => ComprehensionIf,
    ContextExpr => ContextExpr,
    ContextEntry => ContextEntry,
    StringExpr => StringExpr,
    Interpolation => Interpolation,
    WildcardPattern => WildcardPattern,
    BindingPattern => BindingPattern,
    LiteralPattern => LiteralPattern,
    VariantPattern => VariantPattern,
    PatternArgumentList => PatternArgumentList,
    NamedPattern => NamedPattern,
    DataPattern => DataPattern,
    DataPatternField => DataPatternField,
    TuplePattern => TuplePattern,
    SpreadPattern => SpreadPattern,
    RangePattern => RangePattern,
    NamedType => NamedType,
    DynType => DynType,
    MutType => MutType,
    OptionalType => OptionalType,
    TupleType => TupleType,
    ParenType => ParenType,
    FunctionType => FunctionType,
    ProjectionType => ProjectionType,
    ContextType => ContextType,
    RestType => RestType,
    InferType => InferType,
    RequirementRow => RequirementRow,
    ErrorNode => Error,
    SkippedBody => SkippedBody,
}

/// Direct tokens of a node: its span minus its children's spans.
pub struct DirectTokens<'t> {
    next: u32,
    end: u32,
    children: ChildIter<'t>,
    child: Option<(u32, u32)>,
}

impl Iterator for DirectTokens<'_> {
    type Item = TokenIdx;

    fn next(&mut self) -> Option<TokenIdx> {
        while let Some((lo, hi)) = self.child {
            if self.next < lo {
                break;
            }
            if self.next <= hi {
                self.next = hi + 1;
            }
            self.child = next_span(&mut self.children);
        }
        if self.next > self.end {
            return None;
        }
        let token = self.next;
        self.next += 1;
        Some(TokenIdx::from_raw(token))
    }
}

fn next_span(children: &mut ChildIter<'_>) -> Option<(u32, u32)> {
    for child in children.by_ref() {
        let (lo, hi) = child.tree.span_tokens(child.index);
        if lo.get().is_some() && hi.get().is_some() {
            return Some((lo.raw(), hi.raw()));
        }
    }
    None
}

impl<'t> NodeRef<'t> {
    /// The tokens this node holds itself, outside its children.
    #[must_use]
    pub fn direct_tokens(self) -> DirectTokens<'t> {
        let (lo, hi) = self.tree.span_tokens(self.index);
        let (next, end) = if lo.get().is_some() && hi.get().is_some() {
            (lo.raw(), hi.raw())
        } else {
            (1, 0)
        };
        let mut children = ChildIter {
            tree: self.tree,
            next: self.index.idx() + 1,
            end: self.index.idx() + self.tree.subtree_len[self.index.idx()] as usize,
        };
        let child = next_span(&mut children);
        DirectTokens {
            next,
            end,
            children,
            child,
        }
    }

    /// The first direct token of `kind`.
    #[must_use]
    pub fn direct_token(self, tokens: &TokenBuf, kind: TokenKind) -> Option<TokenIdx> {
        self.direct_tokens()
            .find(|&token| tokens.kind(token) == kind)
    }

    /// The first direct identifier token (plain or raw).
    #[must_use]
    pub fn name(self, tokens: &TokenBuf) -> Option<TokenIdx> {
        self.direct_tokens()
            .find(|&token| matches!(tokens.kind(token), TokenKind::Ident | TokenKind::RawIdent))
    }

    /// The first child of `kind`.
    #[must_use]
    pub fn child(self, kind: SyntaxKind) -> Option<NodeRef<'t>> {
        self.children().find(|child| child.kind() == kind)
    }

    /// The first token of the node's span.
    #[must_use]
    pub fn first_token(self) -> TokenIdx {
        self.tree.span_tokens(self.index).0
    }

    /// The last token of the node's span.
    #[must_use]
    pub fn last_token(self) -> TokenIdx {
        self.tree.span_tokens(self.index).1
    }
}

impl SyntaxKind {
    /// The kinds of the `type` production.
    #[must_use]
    pub fn is_type(self) -> bool {
        matches!(
            self,
            Self::NamedType
                | Self::DynType
                | Self::MutType
                | Self::OptionalType
                | Self::TupleType
                | Self::ParenType
                | Self::FunctionType
                | Self::ProjectionType
                | Self::ContextType
                | Self::RestType
                | Self::InferType
                | Self::RequirementRow
        )
    }

    /// The kinds of the `statement` production inside a block.
    #[must_use]
    pub fn is_statement(self) -> bool {
        matches!(
            self,
            Self::LetStmt
                | Self::DiscardStmt
                | Self::AssignmentStmt
                | Self::ReturnStmt
                | Self::BreakStmt
                | Self::ContinueStmt
                | Self::DeferStmt
                | Self::ExprStmt
        )
    }
}

/// Any named declaration: function, data, enum, trait, type, or impl.
#[derive(Clone, Copy)]
pub struct Decl<'t>(NodeRef<'t>);

impl<'t> Decl<'t> {
    #[must_use]
    pub fn cast(node: NodeRef<'t>) -> Option<Self> {
        matches!(
            node.kind(),
            SyntaxKind::FnDecl
                | SyntaxKind::DataDecl
                | SyntaxKind::EnumDecl
                | SyntaxKind::TraitDecl
                | SyntaxKind::TypeDecl
                | SyntaxKind::ImplDecl
        )
        .then_some(Self(node))
    }

    #[must_use]
    pub fn node(self) -> NodeRef<'t> {
        self.0
    }

    /// The declared name (none for an implementation).
    #[must_use]
    pub fn name(self, tokens: &TokenBuf) -> Option<TokenIdx> {
        if self.0.kind() == SyntaxKind::ImplDecl {
            return None;
        }
        self.0.name(tokens)
    }

    /// `pub` stands before the declaration keyword.
    #[must_use]
    pub fn is_pub(self, tokens: &TokenBuf) -> bool {
        self.0.direct_token(tokens, TokenKind::KwPub).is_some()
    }

    pub fn decorators(self) -> impl Iterator<Item = Decorator<'t>> {
        self.0.children().filter_map(Decorator::cast)
    }

    /// The suite or member block.
    #[must_use]
    pub fn body(self) -> Option<Block<'t>> {
        self.0.children().find_map(Block::cast)
    }
}

impl<'t> FnDecl<'t> {
    #[must_use]
    pub fn name_token(self, tokens: &TokenBuf) -> Option<TokenIdx> {
        self.0.name(tokens)
    }

    #[must_use]
    pub fn generics(self) -> Option<GenericParameterList<'t>> {
        self.0.children().find_map(GenericParameterList::cast)
    }

    #[must_use]
    pub fn params(self) -> Option<ParameterList<'t>> {
        self.0.children().find_map(ParameterList::cast)
    }

    /// The written result type (after `->`).
    #[must_use]
    pub fn result(self) -> Option<NodeRef<'t>> {
        self.0
            .children()
            .find(|child| child.kind().is_type() && child.kind() != SyntaxKind::RequirementRow)
    }

    #[must_use]
    pub fn row(self) -> Option<RequirementRow<'t>> {
        self.0.children().find_map(RequirementRow::cast)
    }

    #[must_use]
    pub fn body(self) -> Option<Block<'t>> {
        self.0.children().find_map(Block::cast)
    }
}

impl<'t> Parameter<'t> {
    /// The parameter's type.
    #[must_use]
    pub fn ty(self) -> Option<NodeRef<'t>> {
        self.0.children().find(|child| child.kind().is_type())
    }

    #[must_use]
    pub fn default(self) -> Option<DefaultValue<'t>> {
        self.0.children().find_map(DefaultValue::cast)
    }
}

impl<'t> GenericParameter<'t> {
    #[must_use]
    pub fn bounds(self) -> Option<BoundList<'t>> {
        self.0.children().find_map(BoundList::cast)
    }

    #[must_use]
    pub fn default(self) -> Option<TypeDefault<'t>> {
        self.0.children().find_map(TypeDefault::cast)
    }
}

impl<'t> BoundList<'t> {
    pub fn traits(self) -> impl Iterator<Item = NamedType<'t>> {
        self.0.children().filter_map(NamedType::cast)
    }
}

impl<'t> DataField<'t> {
    #[must_use]
    pub fn ty(self) -> Option<NodeRef<'t>> {
        self.0.children().find(|child| child.kind().is_type())
    }
}

impl<'t> Block<'t> {
    /// The block's statements and local declarations, in order.
    pub fn items(self) -> impl Iterator<Item = NodeRef<'t>> {
        self.0.children()
    }
}

impl<'t> IfExpr<'t> {
    #[must_use]
    pub fn condition(self) -> Option<NodeRef<'t>> {
        self.0.children().next()
    }

    #[must_use]
    pub fn then_block(self) -> Option<Block<'t>> {
        self.0.children().find_map(Block::cast)
    }

    #[must_use]
    pub fn else_clause(self) -> Option<ElseClause<'t>> {
        self.0.children().find_map(ElseClause::cast)
    }
}

impl<'t> ImplDecl<'t> {
    /// The trait (for `impl Trait for T`) and the target type.
    #[must_use]
    pub fn header_types(self) -> (Option<NodeRef<'t>>, Option<NodeRef<'t>>) {
        let mut types = self.0.children().filter(|child| child.kind().is_type());
        let first = types.next();
        let second = types.next();
        match second {
            Some(target) => (first, Some(target)),
            None => (None, first),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Event {
    Start {
        kind: SyntaxKind,
        forward_parent: u32,
        open: bool,
    },
    Token(TokenIdx),
    Finish,
    Tombstone,
}

pub(crate) fn build(
    events: &mut [Event],
    layouts: &[(TokenIdx, Layout)],
    errors: &[(NodeIdx, Code)],
) -> GreenTree {
    let mut tree = GreenTree::default();
    let mut stack = Vec::<usize>::new();
    let mut parents = Vec::<SyntaxKind>::new();
    for index in 0..events.len() {
        match core::mem::replace(&mut events[index], Event::Tombstone) {
            Event::Start {
                kind,
                forward_parent,
                open: true,
            } => {
                parents.push(kind);
                let mut at = index;
                let mut forward = forward_parent;
                while forward != 0 {
                    at += forward as usize;
                    match core::mem::replace(&mut events[at], Event::Tombstone) {
                        Event::Start {
                            kind,
                            forward_parent,
                            ..
                        } => {
                            parents.push(kind);
                            forward = forward_parent;
                        }
                        _ => break,
                    }
                }
                for kind in parents.drain(..).rev() {
                    stack.push(tree.kind.len());
                    tree.kind.push(kind);
                    tree.first_token.push(TokenIdx::NONE);
                    tree.last_token.push(TokenIdx::NONE);
                    tree.subtree_len.push(0);
                }
            }
            Event::Start { .. } | Event::Tombstone => {}
            Event::Token(token) => {
                if let Some(&current) = stack.last() {
                    if tree.first_token[current].get().is_none() {
                        tree.first_token[current] = token;
                    }
                    tree.last_token[current] = token;
                }
            }
            Event::Finish => {
                let Some(index) = stack.pop() else { continue };
                tree.subtree_len[index] = as_u32(tree.kind.len() - index);
                if let Some(&parent) = stack.last() {
                    if tree.first_token[parent].get().is_none() {
                        tree.first_token[parent] = tree.first_token[index];
                    }
                    if tree.last_token[index].get().is_some() {
                        tree.last_token[parent] = tree.last_token[index];
                    }
                }
            }
        }
    }
    while let Some(index) = stack.pop() {
        tree.subtree_len[index] = as_u32(tree.kind.len() - index);
    }
    for &(at, kind) in layouts {
        tree.layout_at.push(at);
        tree.layout_kind.push(kind);
    }
    for &(node, code) in errors {
        tree.err_node.push(node);
        tree.err_code.push(code);
    }
    tree
}

fn utf16_line_starts(source: &str, starts: &[u32]) -> Vec<u32> {
    starts
        .iter()
        .map(|&start| as_u32(source[..start as usize].encode_utf16().count()))
        .collect()
}

fn u32_bytes(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn u16_bytes(values: &[u16]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn as_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX - 1)
}
