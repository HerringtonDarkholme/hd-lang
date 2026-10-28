//! Syntax-directed construction of the canonical formatting document.

use hd_base::{NodeIdx, SliceSort as _};
use hd_syntax::{NodeRef, Parse, SyntaxKind as S, TokenBuf, TokenKind as T};
use std::ops::Range;

use crate::document::{Command as C, Document, PrintStats};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
enum Gap {
    #[default]
    Auto,
    None,
    Space,
    Soft(&'static str),
    Hard,
    Blank,
}

const HAS_ELSE: u8 = 1;
const CONTAINS_SUITE: u8 = 1 << 1;
const IN_PIPE: u8 = 1 << 2;
const FORCE_FLAT: u8 = 1 << 3;

pub(crate) fn format(source: &str, parsed: &Parse) -> (String, PrintStats) {
    let analysis = Analysis::new(source, parsed);
    Builder::new(source, parsed, analysis).build()
}

struct Analysis<'s, 'p> {
    source: &'s str,
    parsed: &'p Parse,
    parents: Vec<NodeIdx>,
    gaps: Vec<Gap>,
    indent_starts: Vec<usize>,
    indent_ends: Vec<usize>,
    suite_indent_end_heads: Vec<Option<NodeIdx>>,
    suite_indent_end_next: Vec<Option<NodeIdx>>,
    comment_first: Vec<u32>,
    comment_end: Vec<u32>,
    comment_owners: Vec<NodeIdx>,
    comment_inline: Vec<bool>,
    token_owners: Vec<NodeIdx>,
    optional_commas: Vec<bool>,
    added_group_commas: Vec<bool>,
    standalone_group_commas: Vec<bool>,
    suppressed_commas: Vec<bool>,
    chain_indents: Vec<bool>,
    pipe_indents: Vec<bool>,
    node_flags: Vec<u8>,
}

impl<'s, 'p> Analysis<'s, 'p> {
    fn new(source: &'s str, parsed: &'p Parse) -> Self {
        let token_count = parsed.tokens.len();
        let parents = parsed.tree.parent_links();
        let mut node_flags = vec![0; parsed.tree.len()];
        let nodes: Vec<_> = parsed.tree.root().descendants().collect();
        let mut token_owners = vec![parsed.tree.root().index(); token_count];
        for node in &nodes {
            for token in node.direct_tokens() {
                token_owners[token.idx()] = node.index();
            }
            let index = node.index().idx();
            if node.kind() == S::IfExpr
                && node
                    .children()
                    .any(|child| matches!(child.kind(), S::ElseClause | S::ElseIfClause))
            {
                node_flags[index] |= HAS_ELSE;
            }
            if let Some(parent_index) = parents[index].get() {
                let parent = parsed.tree.node(parent_index);
                if parent.kind() == S::PipeExpr
                    || parent.kind() != S::ClosureExpr
                        && node_flags[parent_index.idx()] & IN_PIPE != 0
                {
                    node_flags[index] |= IN_PIPE;
                }
            }
        }
        for node in nodes.iter().rev() {
            let index = node.index().idx();
            if matches!(node.kind(), S::ClosureExpr | S::IfExpr | S::MatchExpr) {
                node_flags[index] |= CONTAINS_SUITE;
            }
            if node_flags[index] & CONTAINS_SUITE != 0
                && let Some(parent) = parents[index].get()
            {
                node_flags[parent.idx()] |= CONTAINS_SUITE;
            }
        }
        let mut this = Self {
            source,
            parsed,
            parents,
            gaps: vec![Gap::Auto; token_count + 1],
            indent_starts: vec![0; token_count + 1],
            indent_ends: vec![0; token_count + 1],
            suite_indent_end_heads: vec![None; token_count + 1],
            suite_indent_end_next: vec![None; parsed.tree.len()],
            comment_first: vec![u32::MAX; token_count + 1],
            comment_end: vec![0; token_count + 1],
            comment_owners: vec![parsed.tree.root().index(); parsed.tokens.com_start.len()],
            comment_inline: vec![false; parsed.tokens.com_start.len()],
            token_owners,
            optional_commas: vec![false; token_count],
            added_group_commas: vec![false; parsed.tree.len()],
            standalone_group_commas: vec![false; parsed.tree.len()],
            suppressed_commas: vec![false; token_count],
            chain_indents: vec![false; parsed.tree.len()],
            pipe_indents: vec![false; parsed.tree.len()],
            node_flags,
        };
        this.attach_comments();
        this.visit(parsed.tree.root());
        this
    }

    fn attach_comments(&mut self) {
        let tokens = &self.parsed.tokens;
        for (comment_index, &start) in tokens.com_start.iter().enumerate() {
            let comment =
                u32::try_from(comment_index).expect("formatter comment count fits in u32");
            let boundary = tokens.end.partition_point(|&end| end <= start);
            if self.comment_first[boundary] == u32::MAX {
                self.comment_first[boundary] = comment;
            }
            self.comment_end[boundary] = comment + 1;
            let previous_end = boundary
                .checked_sub(1)
                .map_or(0, |index| tokens.end[index] as usize);
            let inline = boundary > 0 && !self.source[previous_end..start as usize].contains('\n');
            self.comment_inline[comment_index] = inline;
            self.comment_owners[comment_index] =
                self.comment_owner(boundary, start as usize, inline);
        }
    }

    fn comment_range(&self, boundary: usize) -> Range<usize> {
        let first = self.comment_first[boundary];
        if first == u32::MAX {
            0..0
        } else {
            first as usize..self.comment_end[boundary] as usize
        }
    }

    fn comment_owner(&self, boundary: usize, start: usize, inline: bool) -> NodeIdx {
        if self.parsed.tokens.is_empty() {
            return self.parsed.tree.root().index();
        }
        let token = if boundary > 0 {
            boundary - 1
        } else {
            boundary.min(self.parsed.tokens.len() - 1)
        };
        let node = self.parsed.tree.node(self.token_owners[token]);
        if inline {
            return node.index();
        }
        let comment_column = column(self.source, start);
        let previous = boundary
            .checked_sub(1)
            .map(|token| self.token_owners[token]);
        let next = (boundary < self.parsed.tokens.len()).then(|| self.token_owners[boundary]);
        let mut owner = match (previous, next) {
            (Some(left), Some(right)) => self.common_ancestor(left, right),
            _ => self.parsed.tree.root().index(),
        };
        while !matches!(self.parsed.tree.node(owner).kind(), S::Root | S::Block) {
            let Some(parent) = self.parents[owner.idx()].get() else {
                break;
            };
            owner = parent;
        }

        if let Some(previous) = previous
            && let Some(deeper) = self.deepest_comment_block(previous, owner, comment_column)
        {
            return deeper;
        }
        if boundary > 0
            && self.parsed.tokens.kind[boundary - 1] == T::Colon
            && let Some(next) = next
            && let Some(deeper) = self.deepest_comment_block(next, owner, comment_column)
        {
            return deeper;
        }
        owner
    }

    fn common_ancestor(&self, mut left: NodeIdx, mut right: NodeIdx) -> NodeIdx {
        let mut left_depth = self.node_depth(left);
        let mut right_depth = self.node_depth(right);
        while left_depth > right_depth {
            left = self.parents[left.idx()]
                .get()
                .expect("non-root node has a parent");
            left_depth -= 1;
        }
        while right_depth > left_depth {
            right = self.parents[right.idx()]
                .get()
                .expect("non-root node has a parent");
            right_depth -= 1;
        }
        while left != right {
            left = self.parents[left.idx()]
                .get()
                .expect("non-root node has a parent");
            right = self.parents[right.idx()]
                .get()
                .expect("non-root node has a parent");
        }
        left
    }

    fn node_depth(&self, mut node: NodeIdx) -> usize {
        let mut depth = 0;
        while let Some(parent) = self.parents[node.idx()].get() {
            node = parent;
            depth += 1;
        }
        depth
    }

    fn deepest_comment_block(
        &self,
        mut candidate: NodeIdx,
        floor: NodeIdx,
        comment_column: usize,
    ) -> Option<NodeIdx> {
        loop {
            let node = self.parsed.tree.node(candidate);
            if node.kind() == S::Block
                && let Some(first) = node.first_token().get()
                && column(self.source, self.parsed.tokens.start[first.idx()] as usize)
                    <= comment_column
            {
                return Some(candidate);
            }
            if candidate == floor {
                return None;
            }
            let parent = self.parents[candidate.idx()].get()?;
            candidate = parent;
        }
    }

    fn contains_node(&self, ancestor: NodeIdx, mut node: NodeIdx) -> bool {
        loop {
            if node == ancestor {
                return true;
            }
            let Some(parent) = self.parents[node.idx()].get() else {
                return false;
            };
            node = parent;
        }
    }

    fn visit(&mut self, node: NodeRef<'_>) {
        self.mark_sibling_lines(node);
        match node.kind() {
            S::Block => self.mark_block(node),
            kind if comma_group(kind, node, &self.parsed.tokens) => self.mark_comma_group(node),
            kind if binary(kind) => self.mark_binary(node),
            S::AssignmentStmt
            | S::BindingExpr
            | S::DefaultValue
            | S::TypeDefault
            | S::AssociatedTypeBinding => {
                self.mark_assignment(node);
            }
            S::NamedArgument | S::ContextEntry => self.mark_named_argument(node),
            S::BoundList | S::RequirementRow | S::ImplDecl => self.mark_type_spacing(node),
            S::VariantExpr | S::VariantPattern => self.mark_variant(node),
            S::Decorator => {
                if let Some((_, last)) = token_bounds(node) {
                    self.set_gap(last + 1, Gap::Hard);
                }
            }
            S::ElseClause | S::ElseIfClause => self.mark_else(node),
            S::MatchExpr => self.mark_match(node),
            S::UnaryExpr | S::RefExpr | S::DerefExpr | S::SpreadExpr => {
                self.mark_prefix(node);
            }
            kind if chain(kind) && self.is_outer_chain(node) && !self.is_pipe_operand(node) => {
                self.mark_chain(node);
            }
            _ => {}
        }
        for child in node.children() {
            self.visit(child);
        }
    }

    fn mark_sibling_lines(&mut self, node: NodeRef<'_>) {
        if !matches!(
            node.kind(),
            S::Root | S::Block | S::MatchExpr | S::ComprehensionExpr
        ) {
            return;
        }
        let mut children = node
            .children()
            .filter(|child| child.first_token().get().is_some());
        let Some(mut previous) = children.next() else {
            return;
        };
        for child in children {
            if node.kind() == S::ComprehensionExpr
                && self.node_flags[previous.index().idx()] & CONTAINS_SUITE == 0
            {
                previous = child;
                continue;
            }
            let boundary = child.first_token().idx();
            let previous_end = self.parsed.tokens.end[previous.last_token().idx()] as usize;
            let next_start = self.parsed.tokens.start[boundary] as usize;
            let gap = if self.source[previous_end..next_start].matches('\n').count() > 1 {
                Gap::Blank
            } else {
                Gap::Hard
            };
            self.set_gap(boundary, gap);
            previous = child;
        }
    }

    fn mark_block(&mut self, node: NodeRef<'_>) {
        let Some((first, last)) = token_bounds(node) else {
            return;
        };
        self.indent_starts[first] += 1;
        self.indent_ends[last + 1] += 1;
        self.suite_indent_end_next[node.index().idx()] = self.suite_indent_end_heads[last + 1];
        self.suite_indent_end_heads[last + 1] = Some(node.index());
        let inline_pass = self.block_is_inline_pass(node);
        let force_flat = self.block_in_pipe_step(node) && self.block_can_inline(node);
        if force_flat {
            self.node_flags[node.index().idx()] |= FORCE_FLAT;
        }
        let inline = inline_pass || force_flat || self.block_may_inline(node);
        let gap = if inline_pass || force_flat {
            Gap::Space
        } else if inline {
            Gap::Soft(" ")
        } else {
            Gap::Hard
        };
        self.set_gap(first, gap);
        if !inline {
            self.set_gap(last + 1, Gap::Hard);
        }
    }

    fn block_is_inline_pass(&self, node: NodeRef<'_>) -> bool {
        // An inline `: pass` suite has a distinct syntax shape from a
        // line-indented `pass`: the former is the block's own token, while the
        // latter is an expression statement. Keep that syntax-owned choice so
        // formatting cannot turn one tree into the other.
        token_bounds(node).is_some_and(|(first, last)| {
            first == last
                && self.parsed.tokens.kind[first] == T::KwPass
                && node.children().next().is_none()
        })
    }

    fn block_may_inline(&self, node: NodeRef<'_>) -> bool {
        let Some(parent) = self.ancestors(node).find(|ancestor| {
            matches!(
                ancestor.kind(),
                S::ClosureExpr
                    | S::MatchArm
                    | S::IfExpr
                    | S::ElseClause
                    | S::ElseIfClause
                    | S::FnDecl
                    | S::ForExpr
                    | S::WhileExpr
                    | S::MatchExpr
            )
        }) else {
            return false;
        };
        let one_statement = Self::block_has_one_statement(node);
        match parent.kind() {
            S::ClosureExpr | S::MatchArm => {
                one_statement && self.node_flags[node.index().idx()] & CONTAINS_SUITE == 0
            }
            S::IfExpr | S::ElseClause | S::ElseIfClause => {
                let owner = if parent.kind() == S::IfExpr {
                    parent
                } else {
                    self.ancestors(parent)
                        .find(|ancestor| ancestor.kind() == S::IfExpr)
                        .unwrap_or(parent)
                };
                self.if_is_value(owner)
            }
            _ => false,
        }
    }

    fn block_has_one_statement(node: NodeRef<'_>) -> bool {
        node.children()
            .filter(|child| child.first_token().get().is_some())
            .count()
            == 1
    }

    fn block_can_inline(&self, node: NodeRef<'_>) -> bool {
        let Some(first) = node.first_token().get() else {
            return false;
        };
        Self::block_has_one_statement(node)
            && self.node_flags[node.index().idx()] & CONTAINS_SUITE == 0
            && self.comment_range(first.idx()).is_empty()
    }

    fn if_is_value(&self, node: NodeRef<'_>) -> bool {
        if self.node_flags[node.index().idx()] & HAS_ELSE == 0 {
            return false;
        }
        let mut current = node;
        while let Some(parent) = self.parent(current) {
            match parent.kind() {
                S::ParenExpr
                | S::BindingExpr
                | S::LetStmt
                | S::DiscardStmt
                | S::AssignmentStmt
                | S::RangeExpr
                | S::LogicalOrExpr
                | S::LogicalAndExpr
                | S::ComparisonExpr
                | S::PipeExpr
                | S::BitwiseOrExpr
                | S::BitwiseXorExpr
                | S::BitwiseAndExpr
                | S::ShiftExpr
                | S::AdditiveExpr
                | S::MultiplicativeExpr
                | S::PowerExpr
                | S::CallExpr
                | S::IndexExpr
                | S::Argument
                | S::NamedArgument
                | S::ReturnStmt
                | S::MatchArm
                | S::DataFieldInit
                | S::MapEntry => return true,
                S::ExprStmt => return self.is_tail_statement(parent),
                S::Block | S::Root => return false,
                _ => current = parent,
            }
        }
        false
    }

    fn is_tail_statement(&self, statement: NodeRef<'_>) -> bool {
        let Some(block) = self
            .parent(statement)
            .filter(|node| node.kind() == S::Block)
        else {
            return false;
        };
        let is_last = block
            .children()
            .filter(|child| child.first_token().get().is_some())
            .last()
            .is_some_and(|child| child.index() == statement.index());
        is_last && self.block_produces_value(block)
    }

    fn block_produces_value(&self, block: NodeRef<'_>) -> bool {
        let Some(owner) = self.ancestors(block).find(|ancestor| {
            matches!(
                ancestor.kind(),
                S::FnDecl
                    | S::ClosureExpr
                    | S::MatchArm
                    | S::IfExpr
                    | S::ElseClause
                    | S::ElseIfClause
                    | S::ForExpr
                    | S::WhileExpr
                    | S::MatchExpr
            )
        }) else {
            return false;
        };
        match owner.kind() {
            S::FnDecl => !self.function_returns_void(owner),
            S::ClosureExpr | S::MatchArm => true,
            S::IfExpr => self.if_is_value(owner),
            S::ElseClause | S::ElseIfClause => self
                .ancestors(owner)
                .find(|ancestor| ancestor.kind() == S::IfExpr)
                .is_some_and(|if_expr| self.if_is_value(if_expr)),
            _ => false,
        }
    }

    fn function_returns_void(&self, function: NodeRef<'_>) -> bool {
        let Some(result) = function
            .children()
            .find(|child| child.kind().is_type() && child.kind() != S::RequirementRow)
        else {
            return false;
        };
        let Some((first, last)) = token_bounds(result) else {
            return false;
        };
        let start = self.parsed.tokens.start[first] as usize;
        let end = self.parsed.tokens.end[last] as usize;
        &self.source[start..end] == "void"
    }

    fn mark_comma_group(&mut self, node: NodeRef<'_>) {
        let tokens = &self.parsed.tokens;
        let Some(open) = node
            .direct_tokens()
            .find(|&token| tokens.kind(token).is_open_delimiter())
        else {
            return;
        };
        let close = node.last_token();
        if !tokens.kind(close).is_close_delimiter() {
            return;
        }
        let open = open.idx();
        let close = close.idx();
        if matches!(node.kind(), S::DataExpr | S::DataPattern) {
            self.set_gap(open, Gap::Space);
        }
        if close > open + 1 {
            self.indent_starts[open + 1] += 1;
            self.indent_ends[close] += 1;
        }
        let padded =
            matches!(node.kind(), S::DataExpr | S::DataPattern | S::MapExpr) && close > open + 1;
        self.force_gap(open + 1, Gap::Soft(if padded { " " } else { "" }));
        for comma in node
            .direct_tokens()
            .filter(|&token| tokens.kind(token) == T::Comma)
        {
            self.set_gap(comma.idx() + 1, Gap::Soft(" "));
        }
        self.force_gap(close, Gap::Soft(if padded { " " } else { "" }));

        let comprehension =
            node.kind() == S::ListExpr && node.children().any(|child| child.kind() == S::ForExpr);
        if node.kind() == S::IndexExpr || comprehension || close == 0 || close <= open + 1 {
            return;
        }
        let last = close - 1;
        let trailing =
            tokens.kind[last] == T::Comma && node.direct_tokens().any(|token| token.idx() == last);
        let suite_final = node
            .children()
            .filter(|child| {
                child
                    .last_token()
                    .get()
                    .is_some_and(|token| token.idx() < close)
            })
            .last()
            .is_some_and(|child| self.node_flags[child.index().idx()] & CONTAINS_SUITE != 0);
        self.standalone_group_commas[node.index().idx()] = suite_final;
        if trailing {
            let comma_count = node
                .direct_tokens()
                .filter(|&token| tokens.kind(token) == T::Comma)
                .count();
            let singleton_tuple =
                matches!(node.kind(), S::TupleExpr | S::TupleType | S::TuplePattern)
                    && comma_count == 1;
            if !singleton_tuple {
                if suite_final {
                    self.suppressed_commas[last] = true;
                    self.added_group_commas[node.index().idx()] = true;
                } else {
                    self.optional_commas[last] = true;
                }
            }
        } else {
            self.added_group_commas[node.index().idx()] = true;
        }
    }

    fn mark_binary(&mut self, node: NodeRef<'_>) {
        let tokens = &self.parsed.tokens;
        let breakable = self.binary_breakable(node);
        let first = node.first_token().idx();
        for operator in node
            .direct_tokens()
            .filter(|&token| binary_operator(tokens.kind(token)))
        {
            let range = matches!(tokens.kind(operator), T::DotDot | T::DotDotEq);
            let prefix_range_needs_space = range
                && operator.idx() == first
                && operator.idx() > 0
                && (assignment_operator(tokens.kind[operator.idx() - 1])
                    || tokens.kind[operator.idx() - 1].is_keyword());
            self.set_gap(
                operator.idx(),
                if prefix_range_needs_space {
                    Gap::Space
                } else if range {
                    Gap::None
                } else if breakable {
                    Gap::Soft(" ")
                } else {
                    Gap::Space
                },
            );
            self.set_gap(
                operator.idx() + 1,
                if range { Gap::None } else { Gap::Space },
            );
        }
        if node.kind() == S::PipeExpr
            && !self
                .parent(node)
                .is_some_and(|parent| parent.kind() == S::PipeExpr)
            && let Some((first, last)) = token_bounds(node)
            && let Some(pipe) = (first..=last).find(|&index| tokens.kind[index] == T::PipeGt)
        {
            self.indent_starts[pipe] += 1;
            self.pipe_indents[node.index().idx()] = true;
            for child in node.children() {
                if self.pipe_step_can_flatten(child) {
                    self.node_flags[child.index().idx()] |= FORCE_FLAT;
                }
            }
        }
    }

    fn binary_breakable(&self, node: NodeRef<'_>) -> bool {
        let mut outer = node;
        while let Some(parent) = self.parent(outer) {
            if !binary(parent.kind()) {
                break;
            }
            outer = parent;
        }
        node.kind() == outer.kind()
            && (outer.kind() == S::PipeExpr
                || self.needs_continuation_wrapper(outer)
                || self.in_bracket_or_brace(outer))
    }

    fn mark_assignment(&mut self, node: NodeRef<'_>) {
        let tokens = &self.parsed.tokens;
        for operator in node
            .direct_tokens()
            .filter(|&token| assignment_operator(tokens.kind(token)))
        {
            self.set_gap(operator.idx(), Gap::Space);
            self.set_gap(operator.idx() + 1, Gap::Space);
        }
    }

    fn mark_named_argument(&mut self, node: NodeRef<'_>) {
        let tokens = &self.parsed.tokens;
        for operator in node
            .direct_tokens()
            .filter(|&token| tokens.kind(token) == T::Eq)
        {
            self.force_gap(operator.idx(), Gap::None);
            self.force_gap(operator.idx() + 1, Gap::None);
        }
    }

    fn mark_type_spacing(&mut self, node: NodeRef<'_>) {
        let tokens = &self.parsed.tokens;
        let Some((first, last)) = token_bounds(node) else {
            return;
        };
        let header_end = if node.kind() == S::ImplDecl {
            (first..=last)
                .find(|&index| tokens.kind[index] == T::Colon)
                .unwrap_or(last)
        } else {
            last
        };
        for index in first..=header_end {
            let kind = tokens.kind[index];
            let spaced = match node.kind() {
                S::BoundList => matches!(kind, T::Lt | T::Amp),
                S::RequirementRow => kind == T::Plus,
                S::ImplDecl => kind == T::KwFor,
                _ => false,
            };
            if spaced {
                self.set_gap(index, Gap::Space);
                self.set_gap(index + 1, Gap::Space);
            }
            if node.kind() == S::ImplDecl && kind == T::LBracket {
                self.force_gap(index, Gap::None);
            }
        }
    }

    fn mark_variant(&mut self, node: NodeRef<'_>) {
        let Some(first) = node.first_token().get() else {
            return;
        };
        if self.parsed.tokens.kind(first) == T::Dot && first.idx() > 0 {
            self.set_gap(first.idx(), Gap::Space);
        }
    }

    fn mark_prefix(&mut self, node: NodeRef<'_>) {
        let Some(first) = node.first_token().get() else {
            return;
        };
        if first.idx() > 0 {
            let previous = self.parsed.tokens.kind[first.idx() - 1];
            if word(previous) || previous.is_close_delimiter() {
                self.set_gap(first.idx(), Gap::Space);
            }
        }
        let glued_amp = self.parsed.tokens.kind(first) == T::Amp
            && self.parsed.tokens.kind.get(first.idx() + 1) == Some(&T::Amp);
        let separate_stars = self.parsed.tokens.kind(first) == T::Star
            && self.parsed.tokens.kind.get(first.idx() + 1) == Some(&T::Star);
        self.set_gap(
            first.idx() + 1,
            if glued_amp || separate_stars {
                Gap::Space
            } else {
                Gap::None
            },
        );
    }

    fn mark_else(&mut self, node: NodeRef<'_>) {
        let Some(first) = node.first_token().get() else {
            return;
        };
        let owner = self
            .ancestors(node)
            .find(|ancestor| ancestor.kind() == S::IfExpr);
        let gap = match owner {
            Some(owner) if self.if_is_value(owner) => Gap::Soft(" "),
            Some(_) => Gap::Hard,
            None => Gap::Space,
        };
        self.set_gap(first.idx(), gap);
    }

    fn mark_match(&mut self, node: NodeRef<'_>) {
        let arms: Vec<_> = node
            .children()
            .filter(|child| child.kind() == S::MatchArm)
            .collect();
        let Some(first) = arms.first() else {
            return;
        };
        let first = first.first_token().idx();
        let last = arms.last().expect("nonempty arms").last_token().idx();
        self.indent_starts[first] += 1;
        self.indent_ends[last + 1] += 1;
        self.set_gap(first, Gap::Hard);
    }

    fn mark_chain(&mut self, node: NodeRef<'_>) {
        let mut dots = Vec::new();
        self.collect_chain_dots(node, &mut dots);
        dots.sort_stable();
        let first_call_end = Self::first_chain_call_end(node);
        let first_break = dots.iter().copied().find(|&dot| {
            first_call_end.map_or_else(|| dots.first() != Some(&dot), |end| dot > end)
        });
        for &dot in dots
            .iter()
            .filter(|&&dot| first_break.is_some_and(|first| dot >= first))
        {
            self.set_gap(dot, Gap::Soft(""));
        }
        if let Some(first) = first_break {
            self.indent_starts[first] += 1;
            self.chain_indents[node.index().idx()] = true;
        }
    }

    fn first_chain_call_end(node: NodeRef<'_>) -> Option<usize> {
        for child in node.children().filter(|child| chain(child.kind())) {
            if let Some(end) = Self::first_chain_call_end(child) {
                return Some(end);
            }
        }
        (node.kind() == S::CallExpr).then(|| node.last_token().idx())
    }

    fn collect_chain_dots(&self, node: NodeRef<'_>, dots: &mut Vec<usize>) {
        let tokens = &self.parsed.tokens;
        if node.kind() == S::FieldExpr {
            for dot in node
                .direct_tokens()
                .filter(|&token| tokens.kind(token) == T::Dot)
            {
                dots.push(dot.idx());
            }
        }
        for child in node.children().filter(|child| chain(child.kind())) {
            self.collect_chain_dots(child, dots);
        }
    }

    fn is_outer_chain(&self, node: NodeRef<'_>) -> bool {
        !self.parent(node).is_some_and(|parent| chain(parent.kind()))
    }

    fn is_pipe_operand(&self, node: NodeRef<'_>) -> bool {
        self.node_flags[node.index().idx()] & IN_PIPE != 0
    }

    fn block_in_pipe_step(&self, node: NodeRef<'_>) -> bool {
        let Some(owner) = self.parent(node) else {
            return false;
        };
        owner.kind() == S::ClosureExpr && self.node_flags[owner.index().idx()] & IN_PIPE != 0
    }

    fn pipe_step_can_flatten(&self, node: NodeRef<'_>) -> bool {
        let mut saw_suite = false;
        for descendant in node.descendants() {
            if descendant.kind() == S::Block {
                saw_suite = true;
                if !self.block_can_inline(descendant) {
                    return false;
                }
            }
        }
        saw_suite
    }

    fn is_suite_header(&self, node: NodeRef<'_>) -> bool {
        let last = node.last_token().idx();
        self.ancestors(node).any(|ancestor| {
            matches!(
                ancestor.kind(),
                S::IfExpr | S::ElseIfClause | S::ForExpr | S::WhileExpr | S::MatchExpr
            ) && ancestor
                .direct_tokens()
                .find(|&token| self.parsed.tokens.kind(token) == T::Colon)
                .is_some_and(|colon| last < colon.idx())
        })
    }

    fn removable_paren<'t>(&self, node: NodeRef<'t>) -> Option<NodeRef<'t>> {
        if node.kind() != S::ParenExpr {
            return None;
        }
        let child = node.children().next()?;
        if !binary(child.kind()) && !chain(child.kind()) {
            return None;
        }
        let parent = self.parent(node)?;
        if !matches!(
            parent.kind(),
            S::BindingExpr
                | S::LetStmt
                | S::DiscardStmt
                | S::AssignmentStmt
                | S::ReturnStmt
                | S::ExprStmt
                | S::Argument
                | S::NamedArgument
                | S::IfExpr
                | S::ElseIfClause
                | S::ForExpr
                | S::WhileExpr
                | S::MatchExpr
        ) {
            return None;
        }
        let (first, last) = token_bounds(node)?;
        let lo = self.parsed.tokens.end[first] as usize;
        let hi = self.parsed.tokens.start[last] as usize;
        let comments = &self.parsed.tokens.com_start;
        let first_comment = comments.partition_point(|&start| (start as usize) < lo);
        let comment_free = comments
            .get(first_comment)
            .is_none_or(|&start| start as usize >= hi);
        comment_free.then_some(child)
    }

    fn parent(&self, node: NodeRef<'_>) -> Option<NodeRef<'_>> {
        self.parents[node.index().idx()]
            .get()
            .map(|index| self.parsed.tree.node(index))
    }

    fn has_preserved_paren_parent(&self, node: NodeRef<'_>) -> bool {
        self.parent(node).is_some_and(|parent| {
            parent.kind() == S::ParenExpr && self.removable_paren(parent).is_none()
        })
    }

    fn needs_continuation_wrapper(&self, node: NodeRef<'_>) -> bool {
        let Some(mut parent) = self.parent(node) else {
            return false;
        };
        if parent.kind() == S::ParenExpr
            && self.removable_paren(parent).is_some()
            && let Some(grandparent) = self.parent(parent)
        {
            parent = grandparent;
        }
        matches!(
            parent.kind(),
            S::BindingExpr
                | S::LetStmt
                | S::DiscardStmt
                | S::AssignmentStmt
                | S::ReturnStmt
                | S::ExprStmt
                | S::Argument
                | S::NamedArgument
        ) || self.is_suite_header(node)
    }

    fn in_bracket_or_brace(&self, node: NodeRef<'_>) -> bool {
        for ancestor in self.ancestors(node) {
            if matches!(ancestor.kind(), S::Block | S::Root) {
                return false;
            }
            if comma_group(ancestor.kind(), ancestor, &self.parsed.tokens)
                && ancestor
                    .direct_tokens()
                    .any(|token| matches!(self.parsed.tokens.kind(token), T::LBracket | T::LBrace))
            {
                return true;
            }
        }
        false
    }

    fn signature_parameters(&self, node: NodeRef<'_>) -> bool {
        node.kind() == S::ParameterList
            && self
                .parent(node)
                .is_some_and(|parent| parent.kind() == S::FnDecl)
    }

    fn ancestors(&self, node: NodeRef<'_>) -> impl Iterator<Item = NodeRef<'_>> + '_ {
        let mut next = self.parent(node);
        std::iter::from_fn(move || {
            let current = next?;
            next = self.parent(current);
            Some(current)
        })
    }

    fn set_gap(&mut self, boundary: usize, gap: Gap) {
        if gap > self.gaps[boundary] {
            self.gaps[boundary] = gap;
        }
    }

    fn force_gap(&mut self, boundary: usize, gap: Gap) {
        if !matches!(self.gaps[boundary], Gap::Hard | Gap::Blank) {
            self.gaps[boundary] = gap;
        }
    }
}

struct Builder<'s, 'p> {
    source: &'s str,
    parsed: &'p Parse,
    analysis: Analysis<'s, 'p>,
    document: Document<'s>,
}

impl<'s, 'p> Builder<'s, 'p> {
    fn new(source: &'s str, parsed: &'p Parse, analysis: Analysis<'s, 'p>) -> Self {
        Self {
            source,
            parsed,
            analysis,
            document: Document::new(source),
        }
    }

    fn build(mut self) -> (String, PrintStats) {
        self.emit_gap(0);
        if !self.parsed.tokens.is_empty() {
            self.emit_node(self.parsed.tree.root());
            self.emit_gap(self.parsed.tokens.len());
        }
        self.document.print()
    }

    fn emit_node(&mut self, node: NodeRef<'_>) {
        if node.first_token().get().is_none() {
            return;
        }
        if node.kind() == S::StringExpr {
            let first = node.first_token().idx();
            let last = node.last_token().idx();
            self.document.commands.push(C::Text(
                self.parsed.tokens.start[first] as usize..self.parsed.tokens.end[last] as usize,
            ));
            return;
        }
        if let Some(child) = self.analysis.removable_paren(node) {
            self.emit_node(child);
            return;
        }

        let outer_binary = binary(node.kind())
            && node.kind() != S::PipeExpr
            && !self
                .analysis
                .parent(node)
                .is_some_and(|parent| binary(parent.kind()));
        let regular_binary = outer_binary && self.analysis.needs_continuation_wrapper(node);
        let delimited_binary = outer_binary && self.analysis.in_bracket_or_brace(node);
        let pipe = node.kind() == S::PipeExpr
            && !self
                .analysis
                .parent(node)
                .is_some_and(|parent| parent.kind() == S::PipeExpr);
        let chain_group = self.analysis.chain_indents[node.index().idx()];
        let wrapped_chain = chain_group && self.analysis.is_suite_header(node);
        let grouped = comma_group(node.kind(), node, &self.parsed.tokens)
            || matches!(node.kind(), S::IfExpr | S::ClosureExpr | S::MatchArm)
            || chain_group
            || delimited_binary;
        let signature_parameters = self.analysis.signature_parameters(node);
        let force_flat = self.analysis.node_flags[node.index().idx()] & FORCE_FLAT != 0;

        if force_flat {
            self.document.commands.push(C::StartFlat);
        }

        if regular_binary || wrapped_chain {
            self.document.commands.push(C::StartGroup);
            let parenthesized = self.analysis.has_preserved_paren_parent(node);
            if !parenthesized {
                self.document.commands.push(C::ModeText {
                    flat: "",
                    broken: "(",
                });
            }
            self.document.commands.push(C::StartConditionalIndent);
            self.document.commands.push(C::SoftLine(""));
        } else if pipe || grouped {
            self.document.commands.push(C::StartGroup);
        }

        self.emit_contents(node);

        if chain_group {
            self.document.commands.push(C::EndIndent);
        }
        if self.analysis.pipe_indents[node.index().idx()] {
            self.document.commands.push(C::EndIndent);
        }

        if regular_binary || wrapped_chain {
            self.document.commands.push(C::EndConditionalIndent);
            self.document.commands.push(C::SoftLine(""));
            let parenthesized = self.analysis.has_preserved_paren_parent(node);
            if !parenthesized {
                self.document.commands.push(C::ModeText {
                    flat: "",
                    broken: ")",
                });
            }
            self.document.commands.push(C::EndGroup);
        } else if (pipe || grouped) && !signature_parameters {
            self.document.commands.push(C::EndGroup);
        }
        if force_flat {
            self.document.commands.push(C::EndFlat);
        }
    }

    fn emit_contents(&mut self, node: NodeRef<'_>) {
        let first = node.first_token().idx();
        let last = node.last_token().idx();
        let mut children = node
            .children()
            .filter(|child| child.first_token().get().is_some())
            .peekable();
        let mut index = first;
        let mut emitted = false;
        let signature_body = (node.kind() == S::FnDecl)
            .then(|| {
                node.children()
                    .find(|child| child.kind() == S::Block)
                    .map(|child| child.first_token().idx())
            })
            .flatten();
        let mut signature_group_open = node.kind() == S::FnDecl
            && node
                .children()
                .any(|child| self.analysis.signature_parameters(child));
        while index <= last {
            if signature_group_open && signature_body == Some(index) {
                self.document.commands.push(C::EndGroup);
                signature_group_open = false;
            }
            if emitted {
                let added = index == last && self.analysis.added_group_commas[node.index().idx()];
                let standalone = added && self.analysis.standalone_group_commas[node.index().idx()];
                if added && !standalone {
                    self.document.commands.push(C::ModeText {
                        flat: "",
                        broken: ",",
                    });
                }
                if standalone {
                    self.emit_gap(index);
                    self.document.commands.push(C::StartIndent);
                    self.document.commands.push(C::ModeText {
                        flat: "",
                        broken: ",",
                    });
                    self.document.commands.push(C::EndIndent);
                    self.document.commands.push(C::SoftLine(""));
                } else {
                    self.emit_gap(index);
                }
            }
            if children
                .peek()
                .is_some_and(|child| child.first_token().idx() == index)
            {
                let child = children.next().expect("peeked child");
                self.emit_node(child);
                index = child.last_token().idx() + 1;
            } else {
                self.emit_token(index);
                index += 1;
            }
            emitted = true;
        }
        if signature_group_open {
            self.document.commands.push(C::EndGroup);
        }
    }

    fn emit_token(&mut self, index: usize) {
        if self.analysis.suppressed_commas[index] {
            // The owning broken group prints this separator after its suite
            // has dedented; a flat group removes the optional separator.
        } else if self.analysis.optional_commas[index] {
            self.document.commands.push(C::ModeText {
                flat: "",
                broken: ",",
            });
        } else {
            self.document.commands.push(C::Text(
                self.parsed.tokens.start[index] as usize..self.parsed.tokens.end[index] as usize,
            ));
        }
    }

    fn emit_gap(&mut self, boundary: usize) {
        let comments = self.analysis.comment_range(boundary);
        let mut suite_ends = Vec::new();
        let mut suite = self.analysis.suite_indent_end_heads[boundary];
        while let Some(block) = suite {
            suite_ends.push(block);
            suite = self.analysis.suite_indent_end_next[block.idx()];
        }
        suite_ends.reverse();
        let immediate_ends = self.analysis.indent_ends[boundary] - suite_ends.len();
        self.emit_indent_ends(immediate_ends);
        for _ in 0..self.analysis.indent_starts[boundary] {
            self.document.commands.push(C::StartIndent);
        }

        let delayed_suite_ends = if comments.is_empty() {
            self.emit_gap_kind(boundary);
            suite_ends.len()
        } else {
            self.emit_comments(boundary, comments, &suite_ends)
        };
        self.emit_indent_ends(delayed_suite_ends);
    }

    fn emit_indent_ends(&mut self, count: usize) {
        for _ in 0..count {
            self.document.commands.push(C::EndIndent);
        }
    }

    fn emit_gap_kind(&mut self, boundary: usize) {
        let gap = match self.analysis.gaps[boundary] {
            Gap::Auto => self.default_gap(boundary),
            gap => gap,
        };
        match gap {
            Gap::Auto | Gap::None => {}
            Gap::Space => self.document.commands.push(C::Atom(" ")),
            Gap::Soft(flat) => self.document.commands.push(C::SoftLine(flat)),
            Gap::Hard => self.document.commands.push(C::HardLine),
            Gap::Blank => self.document.commands.push(C::BlankLine),
        }
    }

    fn emit_comments(
        &mut self,
        boundary: usize,
        comments: Range<usize>,
        suite_ends: &[NodeIdx],
    ) -> usize {
        let tokens = &self.parsed.tokens;
        let previous_end = boundary
            .checked_sub(1)
            .map_or(0, |index| tokens.end[index] as usize);
        let next_start = tokens
            .start
            .get(boundary)
            .map_or(self.source.len(), |&start| start as usize);
        let mut cursor = previous_end;
        let mut remaining_suite_ends = suite_ends.len();
        for (position, comment) in comments.clone().enumerate() {
            let start = tokens.com_start[comment] as usize;
            let end = tokens.com_end[comment] as usize;
            let inline = position == 0 && self.analysis.comment_inline[comment];
            if inline {
                self.document.commands.push(C::LineSuffix {
                    span: start..end,
                    leading_space: true,
                });
            } else {
                let owner = self.analysis.comment_owners[comment];
                while remaining_suite_ends > 0
                    && !self
                        .analysis
                        .contains_node(suite_ends[remaining_suite_ends - 1], owner)
                {
                    self.document.commands.push(C::EndIndent);
                    remaining_suite_ends -= 1;
                }
                let blank = self.source[cursor..start].matches('\n').count() > 1;
                self.document
                    .commands
                    .push(if blank { C::BlankLine } else { C::HardLine });
                self.document.commands.push(C::LineSuffix {
                    span: start..end,
                    leading_space: false,
                });
            }
            cursor = end;
            let following = if comment + 1 < comments.end {
                tokens.com_start[comment + 1] as usize
            } else {
                next_start
            };
            let blank = self.source[cursor..following].matches('\n').count() > 1;
            self.document
                .commands
                .push(if blank { C::BlankLine } else { C::HardLine });
        }
        remaining_suite_ends
    }

    fn default_gap(&self, boundary: usize) -> Gap {
        canonical_gap(&self.parsed.tokens, boundary)
    }
}

fn canonical_gap(tokens: &TokenBuf, boundary: usize) -> Gap {
    if boundary == 0 || boundary >= tokens.len() {
        return Gap::None;
    }
    let previous = tokens.kind[boundary - 1];
    let next = tokens.kind[boundary];
    if matches!(previous, T::Colon | T::Comma)
        || assignment_operator(previous)
        || assignment_operator(next)
        || space_around(previous)
        || space_around(next)
        || previous == T::Star && next == T::Star
        || previous.is_keyword()
            && (next == T::LBracket && !matches!(previous, T::KwSelfValue | T::KwSelfType)
                || next == T::LParen && previous != T::KwFn)
    {
        Gap::Space
    } else if no_space_after(previous, next) || no_space_before(next) {
        Gap::None
    } else if word(previous) && word(next)
        || previous == T::Dollar
        || next == T::Dollar
        || previous.is_close_delimiter() && word(next)
        || word(previous) && next == T::LBrace
    {
        Gap::Space
    } else {
        Gap::None
    }
}

fn token_bounds(node: NodeRef<'_>) -> Option<(usize, usize)> {
    Some((
        node.first_token().get()?.idx(),
        node.last_token().get()?.idx(),
    ))
}

fn column(source: &str, offset: usize) -> usize {
    let line = source[..offset].rfind('\n').map_or(0, |index| index + 1);
    source[line..offset].chars().count()
}

fn comma_group(kind: S, node: NodeRef<'_>, tokens: &TokenBuf) -> bool {
    matches!(
        kind,
        S::ParameterList
            | S::ArgumentList
            | S::DataExpr
            | S::ListExpr
            | S::UseGroup
            | S::GenericParameterList
            | S::TypeArgumentList
            | S::TupleExpr
            | S::MapExpr
            | S::PatternArgumentList
            | S::TupleType
            | S::TuplePattern
            | S::DataPattern
            | S::ContextExpr
            | S::IndexExpr
    ) && (kind != S::ContextExpr
        || node
            .direct_tokens()
            .any(|token| tokens.kind(token) == T::LParen))
}

fn binary(kind: S) -> bool {
    matches!(
        kind,
        S::RangeExpr
            | S::LogicalOrExpr
            | S::LogicalAndExpr
            | S::ComparisonExpr
            | S::PipeExpr
            | S::BitwiseOrExpr
            | S::BitwiseXorExpr
            | S::BitwiseAndExpr
            | S::ShiftExpr
            | S::AdditiveExpr
            | S::MultiplicativeExpr
            | S::PowerExpr
    )
}

fn chain(kind: S) -> bool {
    matches!(
        kind,
        S::CallExpr | S::IndexExpr | S::FieldExpr | S::TryExpr | S::TypeArgsExpr
    )
}

fn binary_operator(kind: T) -> bool {
    matches!(
        kind,
        T::Plus
            | T::Minus
            | T::Star
            | T::Slash
            | T::Percent
            | T::StarStar
            | T::Amp
            | T::Pipe
            | T::Caret
            | T::Shl
            | T::Shr
            | T::AndAnd
            | T::OrOr
            | T::PipeGt
            | T::EqEq
            | T::NotEq
            | T::Lt
            | T::LtEq
            | T::Gt
            | T::GtEq
            | T::KwIn
            | T::KwIs
            | T::DotDot
            | T::DotDotEq
    )
}

fn assignment_operator(kind: T) -> bool {
    matches!(
        kind,
        T::Eq
            | T::ColonEq
            | T::PlusEq
            | T::MinusEq
            | T::StarEq
            | T::SlashEq
            | T::PercentEq
            | T::AmpEq
            | T::PipeEq
            | T::CaretEq
            | T::ShlEq
            | T::ShrEq
    )
}

fn word(kind: T) -> bool {
    matches!(
        kind,
        T::Ident
            | T::RawIdent
            | T::Placeholder
            | T::Number
            | T::String
            | T::Char
            | T::StrHead
            | T::StrMid
            | T::StrTail
    ) || kind.is_keyword()
}

fn no_space_before(kind: T) -> bool {
    matches!(
        kind,
        T::RParen
            | T::RBracket
            | T::RBrace
            | T::Comma
            | T::Dot
            | T::Colon
            | T::Semicolon
            | T::Question
            | T::Bang
            | T::ColonColon
    )
}

fn no_space_after(previous: T, next: T) -> bool {
    matches!(
        previous,
        T::LParen | T::LBracket | T::LBrace | T::Dot | T::At | T::ColonColon | T::Ellipsis
    ) || previous == T::Dollar && next == T::Dot
        || matches!(
            previous,
            T::Ident | T::RawIdent | T::Placeholder | T::KwSelfValue | T::KwSelfType
        ) && matches!(next, T::LParen | T::LBracket)
        || matches!(previous, T::Question | T::Bang) && next == T::LParen
}

fn space_around(kind: T) -> bool {
    matches!(kind, T::Arrow | T::FatArrow)
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::fs;
    use std::path::{Path, PathBuf};

    use hd_base::SliceSort as _;
    use hd_syntax::{NodeRef, Parse, SyntaxKind as S, TokenKind as T};

    use super::{Analysis, format};

    fn sources(directory: &Path, output: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("source directory") {
            let path = entry.expect("source entry").path();
            if path.is_dir() {
                sources(&path, output);
            } else if path.extension().is_some_and(|extension| extension == "hd") {
                output.push(path);
            }
        }
    }

    fn oracle_removable_paren<'t>(
        node: NodeRef<'t>,
        analysis: &Analysis<'_, 't>,
    ) -> Option<NodeRef<'t>> {
        if node.kind() != S::ParenExpr {
            return None;
        }
        let child = node.children().next()?;
        if !matches!(
            child.kind(),
            S::RangeExpr
                | S::LogicalOrExpr
                | S::LogicalAndExpr
                | S::ComparisonExpr
                | S::PipeExpr
                | S::BitwiseOrExpr
                | S::BitwiseXorExpr
                | S::BitwiseAndExpr
                | S::ShiftExpr
                | S::AdditiveExpr
                | S::MultiplicativeExpr
                | S::PowerExpr
                | S::CallExpr
                | S::IndexExpr
                | S::FieldExpr
                | S::TryExpr
                | S::TypeArgsExpr
        ) {
            return None;
        }
        let parent = analysis.parent(node)?;
        if !matches!(
            parent.kind(),
            S::BindingExpr
                | S::LetStmt
                | S::DiscardStmt
                | S::AssignmentStmt
                | S::ReturnStmt
                | S::ExprStmt
                | S::Argument
                | S::NamedArgument
                | S::IfExpr
                | S::ElseIfClause
                | S::ForExpr
                | S::WhileExpr
                | S::MatchExpr
        ) {
            return None;
        }
        let first = node.first_token().idx();
        let last = node.last_token().idx();
        let lo = analysis.parsed.tokens.end[first] as usize;
        let hi = analysis.parsed.tokens.start[last] as usize;
        let comments = &analysis.parsed.tokens.com_start;
        let first_comment = comments.partition_point(|&start| (start as usize) < lo);
        comments
            .get(first_comment)
            .is_none_or(|&start| start as usize >= hi)
            .then_some(child)
    }

    fn fingerprint_node(
        node: NodeRef<'_>,
        source: &str,
        parsed: &Parse,
        analysis: &Analysis<'_, '_>,
        output: &mut String,
    ) {
        if let Some(child) = oracle_removable_paren(node, analysis) {
            fingerprint_node(child, source, parsed, analysis, output);
            return;
        }
        let _ = write!(output, "{:?}", node.kind());
        for token in node.direct_tokens() {
            if parsed.tokens.kind(token) == T::Comma {
                continue;
            }
            let start = parsed.tokens.start[token.idx()] as usize;
            let end = parsed.tokens.end[token.idx()] as usize;
            let _ = write!(
                output,
                " [{:?}:{}]",
                parsed.tokens.kind(token),
                &source[start..end]
            );
        }
        output.push('\n');
        for child in node.children() {
            fingerprint_node(child, source, parsed, analysis, output);
        }
    }

    fn fingerprint(source: &str, parsed: &Parse, analysis: &Analysis<'_, '_>) -> String {
        let mut output = String::new();
        fingerprint_node(parsed.tree.root(), source, parsed, analysis, &mut output);
        output
    }

    fn normalized_snippets(
        node: NodeRef<'_>,
        source: &str,
        parsed: &Parse,
        analysis: &Analysis<'_, '_>,
        output: &mut Vec<String>,
    ) {
        if let Some(child) = oracle_removable_paren(node, analysis) {
            normalized_snippets(child, source, parsed, analysis, output);
            return;
        }
        let snippet = if node.first_token().get().is_some() {
            let lo = parsed.tokens.start[node.first_token().idx()] as usize;
            let hi = parsed.tokens.end[node.last_token().idx()] as usize;
            source[lo..hi].lines().next().unwrap_or_default().to_owned()
        } else {
            String::new()
        };
        output.push(snippet);
        for child in node.children() {
            normalized_snippets(child, source, parsed, analysis, output);
        }
    }

    fn owner_path(node: NodeRef<'_>, analysis: &Analysis<'_, '_>) -> Vec<S> {
        let mut path = Vec::new();
        let mut index = node.index();
        loop {
            let node = analysis.parsed.tree.node(index);
            if node.kind() != S::ParenExpr || oracle_removable_paren(node, analysis).is_none() {
                path.push(node.kind());
            }
            let Some(parent) = analysis.parents[index.idx()].get() else {
                break;
            };
            index = parent;
        }
        path.reverse();
        path
    }

    fn comments(
        source: &str,
        parsed: &Parse,
        analysis: &Analysis<'_, '_>,
    ) -> Vec<(String, bool, Vec<S>)> {
        parsed
            .tokens
            .com_start
            .iter()
            .enumerate()
            .map(|(comment, &start)| {
                let end = parsed.tokens.com_end[comment] as usize;
                let owner = parsed.tree.node(analysis.comment_owners[comment]);
                (
                    source[start as usize..end].to_owned(),
                    analysis.comment_inline[comment],
                    owner_path(owner, analysis),
                )
            })
            .collect()
    }

    fn verify_corpus(directory: &Path) {
        let mut files = Vec::new();
        sources(directory, &mut files);
        files.sort_stable();
        for file in files {
            let source = fs::read_to_string(&file).expect("source text");
            let parsed = hd_syntax::parse(source.as_bytes());
            if !parsed.is_ok() {
                continue;
            }
            let before = Analysis::new(&source, &parsed);
            let (formatted, _) = format(&source, &parsed);
            let reparsed = hd_syntax::parse(formatted.as_bytes());
            assert!(
                reparsed.is_ok(),
                "{}: {:?}",
                file.display(),
                reparsed.diagnostics
            );
            let after = Analysis::new(&formatted, &reparsed);
            let original_tree = fingerprint(&source, &parsed, &before);
            let formatted_tree = fingerprint(&formatted, &reparsed, &after);
            if formatted_tree != original_tree {
                let mismatch = original_tree
                    .lines()
                    .zip(formatted_tree.lines())
                    .position(|(left, right)| left != right)
                    .unwrap_or_else(|| {
                        original_tree
                            .lines()
                            .count()
                            .min(formatted_tree.lines().count())
                    });
                let mut before_snippets = Vec::new();
                normalized_snippets(
                    parsed.tree.root(),
                    &source,
                    &parsed,
                    &before,
                    &mut before_snippets,
                );
                let mut after_snippets = Vec::new();
                normalized_snippets(
                    reparsed.tree.root(),
                    &formatted,
                    &reparsed,
                    &after,
                    &mut after_snippets,
                );
                panic!(
                    "{} changed its normalized syntax tree at node {}\nbefore: {}\n  {}\nafter: {}\n  {}",
                    file.display(),
                    mismatch + 1,
                    original_tree.lines().nth(mismatch).unwrap_or("<end>"),
                    before_snippets
                        .get(mismatch)
                        .map_or("<end>", String::as_str),
                    formatted_tree.lines().nth(mismatch).unwrap_or("<end>"),
                    after_snippets.get(mismatch).map_or("<end>", String::as_str)
                );
            }
            assert_eq!(
                comments(&formatted, &reparsed, &after),
                comments(&source, &parsed, &before),
                "{} changed comment bytes, roles, or owners",
                file.display()
            );
            let (twice, _) = format(&formatted, &reparsed);
            assert_eq!(twice, formatted, "{} is not idempotent", file.display());
        }
    }

    #[test]
    fn std_and_parseable_conformance_preserve_tree_comments_and_fixed_point() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        verify_corpus(&root.join("lib/std"));
        verify_corpus(&root.join("spec/conformance"));
    }

    #[test]
    fn tree_oracle_keeps_call_target_parentheses() {
        let source = "fn f(handler: Handler, event: Event):\n    (handler.callback)(event)\n";
        let parsed = hd_syntax::parse(source.as_bytes());
        assert!(parsed.is_ok(), "{:?}", parsed.diagnostics);
        let analysis = Analysis::new(source, &parsed);
        let call_target = parsed
            .tree
            .root()
            .descendants()
            .find(|node| node.kind() == S::ParenExpr)
            .expect("call target parentheses");
        assert!(oracle_removable_paren(call_target, &analysis).is_none());
        let before = fingerprint(source, &parsed, &analysis);
        let (formatted, _) = format(source, &parsed);
        let reparsed = hd_syntax::parse(formatted.as_bytes());
        let after = Analysis::new(&formatted, &reparsed);
        assert_eq!(fingerprint(&formatted, &reparsed, &after), before);
        assert!(formatted.contains("(handler.callback)(event)"));
    }
}
