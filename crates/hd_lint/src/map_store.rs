//! `map-get-store-back`: a binding copied out of a `Map` with `get`,
//! changed, then stored back under the same key (clippy's `map_entry`).
//! The copy shares the value's storage with the map, so its first change
//! copies the whole value; over a loop the idiom is quadratic, while
//! `get_or_insert` changes the value in place.
//!
//! Shapes, each a direct statement of one block:
//!
//! ```text
//! let mut xs = m.get(k).unwrap_or(d)      let mut xs = match m.get(k):
//! <statements changing xs>                    .Some(v) => v
//! m[k] = xs                                   .None => d      # or `_ => d`
//! ```
//!
//! The rule stays silent when a statement between the two names `m` or the
//! key's name, when one can leave the block early (`return`, `break`,
//! `continue`, `?`) or holds a closure, when `xs` is used other than as the
//! base of a method call, field or element, or when a later statement of the
//! block names `xs`: then the old value may be observed after the store.
//!
//! Cost per module: nothing unless the checker recorded a `Map.get` call.
//! Otherwise one pass over the tokens indexes each name's occurrences and
//! the early-exit tokens, one pass over the nodes adds the closures, and each
//! block holding a candidate collects its statements once. Each candidate
//! then costs a constant climb from its `get` call, binary searches, and a
//! scan of its own name's occurrences between its `let` and its store.
//! Such a scan stops at the next `let` of the same name, so the scans of
//! one name never overlap: the total is linear in the module.

use hd_base::{HashMap, NodeIdx, SliceSort as _, Span, TokenIdx};
use hd_resolve::Src;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};

use crate::checked::CheckedView;

/// One get, change and store back found in a block.
#[derive(Clone, Copy)]
pub struct StoreBack<'v> {
    /// The `get` call, where the finding is reported.
    pub get: NodeRef<'v>,
    /// From the `let` statement's start to the store's end: the text the
    /// fix replaces.
    pub whole: Span,
    /// The replacement's head: `m.get_or_insert(k, d)` in pieces.
    pub map: &'v str,
    pub key: &'v str,
    pub default: &'v str,
    /// The changed binding's name.
    pub binding: &'v str,
    /// The changes between the `let` and the store: a lone statement whose
    /// only use of the binding is its first token gets the call in place of
    /// that token; otherwise the binding is rebound to the reference.
    pub changes: Changes,
}

#[derive(Clone, Copy)]
pub enum Changes {
    /// The text after the binding's token, to the lone statement's end.
    Inline(Span),
    /// The text from the `let` statement's end to the last change's end.
    Rebind(Span),
}

/// Every token position of each identifier, and of each early exit.
struct Index<'v> {
    names: HashMap<&'v str, Vec<u32>>,
    /// `return`, `break`, `continue`, `?` and each closure's first token.
    exits: Vec<u32>,
    /// Each block's direct statements, by their first tokens.
    blocks: HashMap<NodeIdx, Vec<(u32, NodeRef<'v>)>>,
}

impl<'v> Index<'v> {
    fn new(src: Src<'v>) -> Self {
        let tokens = &src.parse.tokens;
        let mut names: HashMap<&'v str, Vec<u32>> = HashMap::default();
        let mut exits = Vec::new();
        for raw in 0..tokens.len() {
            // ruled out: a parse holds fewer than `u32::MAX` tokens, its
            // spans being `u32` offsets.
            let at = u32::try_from(raw).expect("source tokens");
            let token = TokenIdx::from_raw(at);
            match tokens.kind(token) {
                TokenKind::Ident | TokenKind::RawIdent | TokenKind::KwSelfValue => {
                    names.entry(src.text(token)).or_default().push(at);
                }
                TokenKind::KwReturn
                | TokenKind::KwBreak
                | TokenKind::KwContinue
                | TokenKind::Question => exits.push(at),
                // ruled out: a match on a token kind; every other token
                // neither names a binding nor leaves the block.
                _ => {}
            }
        }
        let tree = &src.parse.tree;
        let closures = (0..tree.len()).filter_map(|raw| {
            let node = tree.node(NodeIdx::from_raw(u32::try_from(raw).ok()?));
            (node.kind() == SyntaxKind::ClosureExpr).then(|| node.first_token().raw())
        });
        exits.extend(closures);
        exits.sort_stable();
        Self {
            names,
            exits,
            blocks: HashMap::default(),
        }
    }

    fn occurrences(&self, name: &str) -> &[u32] {
        self.names.get(name).map_or(&[], Vec::as_slice)
    }

    /// The first occurrence of `name` after token `after`.
    fn next(&self, name: &str, after: u32) -> Option<u32> {
        let all = self.occurrences(name);
        all.get(all.partition_point(|&at| at <= after)).copied()
    }

    fn exit_between(&self, lo: u32, hi: u32) -> bool {
        let at = self.exits.partition_point(|&at| at < lo);
        self.exits.get(at).is_some_and(|&at| at <= hi)
    }

    /// The direct statement of `block` that starts at token `first`, and
    /// its position among the block's statements.
    fn statement(&mut self, block: NodeRef<'v>, first: u32) -> Option<(usize, NodeRef<'v>)> {
        let statements = self.blocks.entry(block.index()).or_insert_with(|| {
            block
                .children()
                .map(|child| (child.first_token().raw(), child))
                .collect()
        });
        let at = statements.partition_point(|(lo, _)| *lo < first);
        statements
            .get(at)
            .filter(|(lo, _)| *lo == first)
            .map(|&(_, node)| (at, node))
    }
}

/// Each get, change and store back of the module, in `get` call order.
#[must_use]
pub fn store_backs<'v>(src: Src<'v>, checked: CheckedView<'v>) -> Vec<StoreBack<'v>> {
    let mut index: Option<Index<'v>> = None;
    let mut out = Vec::new();
    for get in checked.map_gets() {
        let Some(shape) = Shape::of(src, get) else {
            continue;
        };
        let index = index.get_or_insert_with(|| Index::new(src));
        if let Some(found) = shape.confirm(src, index) {
            out.push(found);
        }
    }
    out
}

/// What the syntax around one `Map.get` call says, before any name is
/// looked up.
struct Shape<'v> {
    get: NodeRef<'v>,
    /// The map: a name, or fields of one.
    receiver: NodeRef<'v>,
    key: NodeRef<'v>,
    default: NodeRef<'v>,
    binding: TokenIdx,
    statement: NodeRef<'v>,
    block: NodeRef<'v>,
}

impl<'v> Shape<'v> {
    /// The constant climb from the call to its `let` statement.
    fn of(src: Src<'v>, get: NodeRef<'v>) -> Option<Self> {
        let tree = &src.parse.tree;
        let parent = |node: NodeRef<'v>| tree.parent(node.index()).map(|up| tree.node(up));
        let [callee, arguments] = two(get)?;
        let [receiver] = only(callee)?;
        let key = argument(arguments)?;
        if get.kind() != SyntaxKind::CallExpr
            || callee.kind() != SyntaxKind::FieldExpr
            || !named_place(src, receiver)
            || !(matches!(key.kind(), SyntaxKind::NameExpr | SyntaxKind::LiteralExpr)
                || key.kind() == SyntaxKind::StringExpr && key.children().next().is_none())
        {
            return None;
        }
        let above = parent(get)?;
        let (initializer, default) = match above.kind() {
            SyntaxKind::FieldExpr if field_name(src, above) == Some("unwrap_or") => {
                let call = parent(above)?;
                let [_, arguments] = two(call)?;
                let default = argument(arguments)?;
                (call.kind() == SyntaxKind::CallExpr).then_some((call, default))?
            }
            SyntaxKind::MatchExpr
                if above.children().next().map(NodeRef::index) == Some(get.index()) =>
            {
                (above, match_default(src, above)?)
            }
            _ => return None,
        };
        let statement = parent(initializer)?;
        let block = parent(statement)?;
        let [pattern] = first(statement)?;
        if statement.kind() != SyntaxKind::LetStmt
            || statement.children().last().map(NodeRef::index) != Some(initializer.index())
            || pattern.kind() != SyntaxKind::BindingPattern
            || pattern.children().next().is_some()
            || block.kind() != SyntaxKind::Block
        {
            return None;
        }
        let tokens = &src.parse.tokens;
        pattern
            .direct_token(tokens, TokenKind::KwMut)
            .or_else(|| statement.direct_token(tokens, TokenKind::KwMut))?;
        Some(Self {
            get,
            receiver,
            key,
            default,
            binding: pattern.name(tokens)?,
            statement,
            block,
        })
    }

    /// The store and the changes between, from the module's indexes.
    fn confirm(&self, src: Src<'v>, index: &mut Index<'v>) -> Option<StoreBack<'v>> {
        // The map's root name: a statement that names it may read or
        // change the map, through a method of the root for a field.
        let map = src.text(self.receiver.first_token());
        let binding = src.text(self.binding);
        let key_name = (self.key.kind() == SyntaxKind::NameExpr)
            .then(|| {
                self.key
                    .name(&src.parse.tokens)
                    .map(|token| src.text(token))
            })
            .flatten();
        if binding == map || key_name == Some(binding) {
            return None;
        }
        let end = self.statement.last_token().raw();
        let block_end = self.block.last_token().raw();
        // The default runs before the map is borrowed for the call.
        let default = (
            self.default.first_token().raw(),
            self.default.last_token().raw(),
        );
        if index
            .next(map, default.0.saturating_sub(1))
            .is_some_and(|at| at <= default.1)
        {
            return None;
        }
        // The first statement after the `let` that names the map is the store.
        let first = index.next(map, end).filter(|&at| at <= block_end)?;
        let (at, store) = index.statement(self.block, first)?;
        let (line, _) = index.statement(self.block, self.statement.first_token().raw())?;
        if at < line + 2 || !self.is_store(src, store, binding) {
            return None;
        }
        let window = (end + 1, first - 1);
        if index.exit_between(window.0, window.1)
            || key_name.is_some_and(|key| index.next(key, end).is_some_and(|at| at < first))
            || index
                .next(binding, store.last_token().raw())
                .is_some_and(|at| at <= block_end && !index.rebinds(src, self.block, at))
        {
            return None;
        }
        // Each use of the binding between is the base of a place, and is
        // this binding: a later `let` of the name ends the scan.
        let uses = index.occurrences(binding);
        let lo = uses.partition_point(|&at| at < window.0);
        let hi = uses.partition_point(|&at| at <= window.1);
        let uses = &uses[lo..hi];
        if uses.is_empty() || !uses.iter().all(|&at| place_base(src, at)) {
            return None;
        }
        let between = src.span(index.statement_at(self.block, line + 1)?);
        let last = src.span(index.statement_at(self.block, at - 1)?);
        let changes = if at == line + 2
            && uses
                == [index
                    .statement_at(self.block, line + 1)?
                    .first_token()
                    .raw()]
        {
            Changes::Inline(Span {
                lo: src.token_span(TokenIdx::from_raw(uses[0])).hi,
                ..between
            })
        } else {
            Changes::Rebind(Span {
                lo: src.span(self.statement).hi,
                ..last
            })
        };
        Some(StoreBack {
            get: self.get,
            whole: Span {
                lo: src.span(self.statement).lo,
                ..src.span(store)
            },
            map: src.node_text(self.receiver),
            key: src.node_text(self.key),
            default: src.node_text(self.default),
            binding,
            changes,
        })
    }

    /// `map[key] = binding`, with the key written as in the `get`.
    fn is_store(&self, src: Src<'v>, store: NodeRef<'v>, binding: &str) -> bool {
        let tokens = &src.parse.tokens;
        let Some([target, value]) = two(store) else {
            return false;
        };
        let Some([receiver, key]) = two(target) else {
            return false;
        };
        store.kind() == SyntaxKind::AssignmentStmt
            && store.direct_token(tokens, TokenKind::Eq).is_some()
            && target.kind() == SyntaxKind::IndexExpr
            && same_tokens(src, receiver, self.receiver)
            && value.kind() == SyntaxKind::NameExpr
            && value.name(tokens).is_some_and(|t| src.text(t) == binding)
            && same_tokens(src, key, self.key)
    }
}

impl<'v> Index<'v> {
    /// The name token at `at` starts a new binding of its name: the
    /// pattern of a direct `let` of `block` whose initializer does not
    /// name it, so no later statement can read the old binding.
    fn rebinds(&self, src: Src<'v>, block: NodeRef<'v>, at: u32) -> bool {
        let Some(statements) = self.blocks.get(&block.index()) else {
            return false;
        };
        let Some(&(_, statement)) = statements
            .partition_point(|(lo, _)| *lo <= at)
            .checked_sub(1)
            .and_then(|line| statements.get(line))
        else {
            return false;
        };
        let name = src.text(TokenIdx::from_raw(at));
        statement.kind() == SyntaxKind::LetStmt
            && first(statement).is_some_and(|[pattern]| {
                pattern.kind() == SyntaxKind::BindingPattern
                    && pattern.name(&src.parse.tokens).map(TokenIdx::raw) == Some(at)
            })
            && self
                .next(name, at)
                .is_none_or(|next| next > statement.last_token().raw())
    }

    fn statement_at(&self, block: NodeRef<'v>, at: usize) -> Option<NodeRef<'v>> {
        self.blocks
            .get(&block.index())
            .and_then(|statements| statements.get(at))
            .map(|&(_, node)| node)
    }
}

/// The token at `at` names a binding used as the base of a method call,
/// field or element: no `.` before it, and `.` or `[` after it.
fn place_base(src: Src<'_>, at: u32) -> bool {
    let kind = |raw: u32| src.tkind(TokenIdx::from_raw(raw));
    at.checked_sub(1)
        .is_none_or(|before| kind(before) != Some(TokenKind::Dot))
        && matches!(kind(at + 1), Some(TokenKind::Dot | TokenKind::LBracket))
}

/// A name, or a field of a named place: `m`, `self.counts`, `a.b.c`.
fn named_place(src: Src<'_>, mut node: NodeRef<'_>) -> bool {
    loop {
        match node.kind() {
            SyntaxKind::NameExpr => {
                return matches!(
                    src.tkind(node.first_token()),
                    Some(TokenKind::Ident | TokenKind::RawIdent | TokenKind::KwSelfValue)
                );
            }
            SyntaxKind::FieldExpr => [node] = node.parts(),
            _ => return false,
        }
    }
}

/// Both nodes are written with the same tokens.
fn same_tokens(src: Src<'_>, a: NodeRef<'_>, b: NodeRef<'_>) -> bool {
    src.tokens(a)
        .map(|t| src.text(t))
        .eq(src.tokens(b).map(|t| src.text(t)))
}

/// The field a `FieldExpr` names: its first direct identifier.
fn field_name<'v>(src: Src<'v>, field: NodeRef<'_>) -> Option<&'v str> {
    field.name(&src.parse.tokens).map(|token| src.text(token))
}

/// The `d` of a two-arm `match m.get(k):` whose arms are `.Some(v) => v`
/// and `.None => d` or `_ => d`, in either order.
fn match_default<'v>(src: Src<'v>, matched: NodeRef<'v>) -> Option<NodeRef<'v>> {
    let tokens = &src.parse.tokens;
    let mut arms = matched.children().skip(1);
    let (Some(a), Some(b), None) = (arms.next(), arms.next(), arms.next()) else {
        return None;
    };
    let variant = |arm: NodeRef<'v>| -> Option<(&'v str, NodeRef<'v>, NodeRef<'v>)> {
        let [pattern, body] = two(arm)?;
        let body = arm_value(body)?;
        (arm.kind() == SyntaxKind::MatchArm).then_some(())?;
        let name = match pattern.kind() {
            SyntaxKind::WildcardPattern => "_",
            SyntaxKind::VariantPattern => {
                let last = pattern
                    .direct_tokens()
                    .filter(|&t| tokens.kind(t) == TokenKind::Ident)
                    .last()?;
                src.text(last)
            }
            _ => return None,
        };
        Some((name, pattern, body))
    };
    let (some, none) = match (variant(a)?, variant(b)?) {
        (some @ ("Some", ..), none @ ("None" | "_", ..)) => (some, none),
        (none @ ("None", ..), some @ ("Some", ..)) => (some, none),
        _ => return None,
    };
    let (_, pattern, body) = some;
    let [arguments] = only(pattern)?;
    let [bound] = only(arguments)?;
    let (_, none_pattern, default) = none;
    (bound.kind() == SyntaxKind::BindingPattern
        && bound.children().next().is_none()
        && none_pattern.children().next().is_none()
        && body.kind() == SyntaxKind::NameExpr
        && same_tokens(src, body, bound))
    .then_some(default)
}

/// An arm's value: the one expression statement of its block.
fn arm_value(body: NodeRef<'_>) -> Option<NodeRef<'_>> {
    let [statement] = only(body)?;
    let [value] = only(statement)?;
    (body.kind() == SyntaxKind::Block && statement.kind() == SyntaxKind::ExprStmt).then_some(value)
}

/// The expression of a call's one positional argument.
fn argument(list: NodeRef<'_>) -> Option<NodeRef<'_>> {
    let [argument] = only(list)?;
    let [value] = only(argument)?;
    (argument.kind() == SyntaxKind::Argument).then_some(value)
}

fn only(node: NodeRef<'_>) -> Option<[NodeRef<'_>; 1]> {
    let mut children = node.children();
    match (children.next(), children.next()) {
        (Some(a), None) => Some([a]),
        _ => None,
    }
}

fn two(node: NodeRef<'_>) -> Option<[NodeRef<'_>; 2]> {
    let mut children = node.children();
    match (children.next(), children.next(), children.next()) {
        (Some(a), Some(b), None) => Some([a, b]),
        _ => None,
    }
}

fn first(node: NodeRef<'_>) -> Option<[NodeRef<'_>; 1]> {
    node.children().next().map(|a| [a])
}
