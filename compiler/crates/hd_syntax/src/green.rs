use std::fmt::Write;

use hd_base::{NodeIdx, TokenIdx};
use hd_diag::Code;

use crate::{Layout, TokenBuf, TokenKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum SyntaxKind {
    Root,
    // Items and item parts.
    UseDecl,
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
    GenericParameterList,
    GenericParameter,
    TypeArgumentList,
    ArgumentList,
    Argument,
    NamedArgument,
    DataField,
    EnumVariant,
    AssociatedTypeDecl,
    Derivation,
    // Statements and suites.
    Block,
    Statement,
    LetStmt,
    DiscardStmt,
    AssignmentStmt,
    ReturnStmt,
    BreakStmt,
    ContinueStmt,
    DeferStmt,
    ExprStmt,
    MatchArm,
    // Expressions. Binary operators retain their precise token; these nodes
    // record the precedence level without inflating the tree with token kinds.
    Expression,
    BindingExpr,
    ConditionalExpr,
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
    PostfixExpr,
    CallExpr,
    IndexExpr,
    FieldExpr,
    LiteralExpr,
    NameExpr,
    TupleExpr,
    ListExpr,
    MapExpr,
    DataExpr,
    ClosureExpr,
    IfExpr,
    ForExpr,
    WhileExpr,
    MatchExpr,
    ComprehensionExpr,
    ContextExpr,
    StringExpr,
    Interpolation,
    // Patterns.
    Pattern,
    BindingPattern,
    LiteralPattern,
    VariantPattern,
    DataPattern,
    TuplePattern,
    SpreadPattern,
    RangePattern,
    // Types.
    Type,
    NamedType,
    DynType,
    MutType,
    OptionalType,
    TupleType,
    FunctionType,
    ProjectionType,
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
        for index in 0..self.len() {
            let parents = self.parent_links();
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
    GenericParameterList => GenericParameterList,
    GenericParameter => GenericParameter,
    TypeArgumentList => TypeArgumentList,
    ArgumentList => ArgumentList,
    Argument => Argument,
    NamedArgument => NamedArgument,
    DataField => DataField,
    EnumVariant => EnumVariant,
    AssociatedTypeDecl => AssociatedTypeDecl,
    Derivation => Derivation,
    Block => Block,
    Statement => Statement,
    LetStmt => LetStmt,
    DiscardStmt => DiscardStmt,
    AssignmentStmt => AssignmentStmt,
    ReturnStmt => ReturnStmt,
    BreakStmt => BreakStmt,
    ContinueStmt => ContinueStmt,
    DeferStmt => DeferStmt,
    ExprStmt => ExprStmt,
    MatchArm => MatchArm,
    Expression => Expression,
    BindingExpr => BindingExpr,
    ConditionalExpr => ConditionalExpr,
    RangeExpr => RangeExpr,
    LogicalOrExpr => LogicalOrExpr,
    LogicalAndExpr => LogicalAndExpr,
    ComparisonExpr => ComparisonExpr,
    Pattern => Pattern,
    BindingPattern => BindingPattern,
    LiteralPattern => LiteralPattern,
    VariantPattern => VariantPattern,
    DataPattern => DataPattern,
    TuplePattern => TuplePattern,
    SpreadPattern => SpreadPattern,
    RangePattern => RangePattern,
    Type => Type,
    NamedType => NamedType,
    DynType => DynType,
    MutType => MutType,
    OptionalType => OptionalType,
    FunctionType => FunctionType,
    TupleType => TupleType,
    ProjectionType => ProjectionType,
    RequirementRow => RequirementRow,
    IfExpr => IfExpr,
    ForExpr => ForExpr,
    WhileExpr => WhileExpr,
    MatchExpr => MatchExpr,
    PipeExpr => PipeExpr,
    BitwiseOrExpr => BitwiseOrExpr,
    BitwiseXorExpr => BitwiseXorExpr,
    BitwiseAndExpr => BitwiseAndExpr,
    ShiftExpr => ShiftExpr,
    AdditiveExpr => AdditiveExpr,
    MultiplicativeExpr => MultiplicativeExpr,
    UnaryExpr => UnaryExpr,
    PowerExpr => PowerExpr,
    PostfixExpr => PostfixExpr,
    CallExpr => CallExpr,
    IndexExpr => IndexExpr,
    FieldExpr => FieldExpr,
    LiteralExpr => LiteralExpr,
    NameExpr => NameExpr,
    TupleExpr => TupleExpr,
    ListExpr => ListExpr,
    MapExpr => MapExpr,
    DataExpr => DataExpr,
    ClosureExpr => ClosureExpr,
    ComprehensionExpr => ComprehensionExpr,
    ContextExpr => ContextExpr,
    StringExpr => StringExpr,
    Interpolation => Interpolation,
    ErrorNode => Error,
    SkippedBody => SkippedBody,
}

impl FnDecl<'_> {
    #[must_use]
    pub fn name_token(self, tokens: &TokenBuf) -> Option<TokenIdx> {
        let (first, last) = self.0.tree.span_tokens(self.0.index);
        let mut saw_fn = false;
        for raw in first.raw()..=last.raw() {
            let token = TokenIdx::from_raw(raw);
            match tokens.kind(token) {
                TokenKind::KwFn => saw_fn = true,
                TokenKind::Ident | TokenKind::RawIdent if saw_fn => return Some(token),
                _ => {}
            }
        }
        None
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Event {
    Start(SyntaxKind),
    Token(TokenIdx),
    Finish,
}

pub(crate) fn build(
    events: &[Event],
    layouts: &[(TokenIdx, Layout)],
    errors: &[(NodeIdx, Code)],
) -> GreenTree {
    let mut tree = GreenTree::default();
    let mut stack = Vec::<usize>::new();
    for event in events {
        match *event {
            Event::Start(kind) => {
                stack.push(tree.kind.len());
                tree.kind.push(kind);
                tree.first_token.push(TokenIdx::NONE);
                tree.last_token.push(TokenIdx::NONE);
                tree.subtree_len.push(0);
            }
            Event::Token(token) => {
                if let Some(&current) = stack.last() {
                    if tree.first_token[current].get().is_none() {
                        tree.first_token[current] = token;
                    }
                    tree.last_token[current] = token;
                }
            }
            Event::Finish => {
                let index = stack.pop().expect("balanced parser events");
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
    assert!(stack.is_empty(), "balanced parser events");
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
