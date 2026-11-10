use hd_base::{NodeIdx, TokenIdx};

use crate::{TokenBuf, TokenKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum SyntaxKind {
    Root,
    // Items and their parts.
    UseDecl,
    UseGroup,
    UseItem,
    FnDecl,
    StructDecl,
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
    /// `N < const usize` in a generic parameter list.
    ConstParameter,
    /// `'name` in a generic list (`types.lifetime.declare`).
    LifetimeParameter,
    BoundList,
    TypeDefault,
    TypeArgumentList,
    /// An expression-shaped positional generic argument, such as `4` or `N + 1`.
    ConstArgument,
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
    UsingStmt,
    ExprStmt,
    ElseClause,
    ElseIfClause,
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
    RefExpr,
    DerefExpr,
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
    ImplType,
    RefType,
    OptionalType,
    UnionType,
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
    pub fn reconstruct(&self, tokens: &TokenBuf, source: &str) -> String {
        tokens.reconstruct(source)
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

    /// One past the preorder index of the node's last descendant, so
    /// node `m` lies inside this one when `index() <= m < end()`.
    #[must_use]
    pub fn end(self) -> NodeIdx {
        NodeIdx::from_raw(self.index.raw() + self.tree.subtree_len[self.index.idx()])
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

/// A typed view of one node kind: the two that callers outside the parser
/// name, `Block` (a declaration's suite or member block, `Decl::body`) and
/// `ImplDecl` (`ImplDecl::header_types`). Everything else reads children
/// by kind (`NodeRef::part`, `Src::child`).
macro_rules! typed_nodes {
    ($( $name:ident ),+ $(,)?) => {$ (
        #[derive(Clone, Copy)]
        pub struct $name<'t>(NodeRef<'t>);

        impl<'t> $name<'t> {
            #[must_use]
            pub fn cast(node: NodeRef<'t>) -> Option<Self> {
                (node.kind() == SyntaxKind::$name).then_some(Self(node))
            }

            #[must_use]
            pub fn node(self) -> NodeRef<'t> {
                self.0
            }
        }
    )+ };
}

typed_nodes!(Block, ImplDecl);

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

    /// The first `N` children of a node the grammar builds with them, such
    /// as a spread's operand or an `if`'s condition and suite. Stages past
    /// the parser see only trees that parsed without a diagnostic.
    #[must_use]
    pub fn parts<const N: usize>(self) -> [NodeRef<'t>; N] {
        let mut children = self.children();
        std::array::from_fn(|_| {
            children
                .next()
                .expect("the parser builds the node's required children")
        })
    }

    /// The name token of a node the grammar builds with one, such as a
    /// declaration, a projection or a name pattern ([`Self::parts`]).
    #[must_use]
    pub fn name_part(self, tokens: &TokenBuf) -> TokenIdx {
        self.name(tokens)
            .expect("the parser builds the node's name")
    }

    /// The child of `kind` of a node the grammar builds with one, such as
    /// a function's or a closure's suite ([`Self::parts`]).
    #[must_use]
    pub fn part(self, kind: SyntaxKind) -> NodeRef<'t> {
        self.child(kind)
            .expect("the parser builds the node's required children")
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

/// A type a declaration writes: every kind of the `type` production but
/// the inferred `_`, which only a body writes
/// (`types.generic.placeholder.local`), and a requirement row, which
/// only a type argument list, function type or declaration holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeForm {
    Named,
    Dyn,
    Impl,
    Ref,
    Optional,
    Union,
    Tuple,
    Paren,
    Function,
    Projection,
    Context,
    Rest,
}

impl SyntaxKind {
    /// The kinds of the `type` production.
    #[must_use]
    pub fn is_type(self) -> bool {
        self.type_form().is_some() || matches!(self, Self::InferType | Self::RequirementRow)
    }

    /// The declaration type this kind writes, if any.
    #[must_use]
    pub const fn type_form(self) -> Option<TypeForm> {
        Some(match self {
            Self::NamedType => TypeForm::Named,
            Self::DynType => TypeForm::Dyn,
            Self::ImplType => TypeForm::Impl,
            Self::RefType => TypeForm::Ref,
            Self::OptionalType => TypeForm::Optional,
            Self::UnionType => TypeForm::Union,
            Self::TupleType => TypeForm::Tuple,
            Self::ParenType => TypeForm::Paren,
            Self::FunctionType => TypeForm::Function,
            Self::ProjectionType => TypeForm::Projection,
            Self::ContextType => TypeForm::Context,
            Self::RestType => TypeForm::Rest,
            _ => return None,
        })
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
                | SyntaxKind::StructDecl
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

    /// The suite or member block.
    #[must_use]
    pub fn body(self) -> Option<Block<'t>> {
        self.0.children().find_map(Block::cast)
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

pub(crate) fn build(events: &mut [Event]) -> GreenTree {
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
    // `Parser::complete` opens a start and pushes its finish together, and
    // a precede chain's parents are completed starts too.
    debug_assert!(stack.is_empty(), "every opened node finishes");
    tree
}

fn as_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX - 1)
}
