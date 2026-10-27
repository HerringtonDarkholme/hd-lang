//! Syntax-directed construction of the canonical formatting document.

use hd_base::{NodeIdx, SliceSort as _};
use hd_syntax::{NodeRef, Parse, SyntaxKind as S, TokenBuf, TokenKind as T};

use crate::document::{Command as C, Document, MAX_COLUMNS, PrintStats};

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

pub(crate) fn format(source: &str, parsed: &Parse) -> (String, PrintStats) {
    if parsed.tokens.is_empty() {
        return (String::new(), PrintStats::default());
    }
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
    comments: Vec<Vec<usize>>,
    optional_commas: Vec<bool>,
    added_group_commas: Vec<bool>,
    standalone_group_commas: Vec<bool>,
    suppressed_commas: Vec<bool>,
    chain_indents: Vec<bool>,
    pipe_indents: Vec<bool>,
}

impl<'s, 'p> Analysis<'s, 'p> {
    fn new(source: &'s str, parsed: &'p Parse) -> Self {
        let token_count = parsed.tokens.len();
        let mut this = Self {
            source,
            parsed,
            parents: parsed.tree.parent_links(),
            gaps: vec![Gap::Auto; token_count + 1],
            indent_starts: vec![0; token_count + 1],
            indent_ends: vec![0; token_count + 1],
            comments: vec![Vec::new(); token_count + 1],
            optional_commas: vec![false; token_count],
            added_group_commas: vec![false; parsed.tree.len()],
            standalone_group_commas: vec![false; parsed.tree.len()],
            suppressed_commas: vec![false; token_count],
            chain_indents: vec![false; parsed.tree.len()],
            pipe_indents: vec![false; parsed.tree.len()],
        };
        this.attach_comments();
        this.visit(parsed.tree.root());
        this
    }

    fn attach_comments(&mut self) {
        let tokens = &self.parsed.tokens;
        for (comment, &start) in tokens.com_start.iter().enumerate() {
            let boundary = tokens.end.partition_point(|&end| end <= start);
            self.comments[boundary].push(comment);
        }
    }

    fn visit(&mut self, node: NodeRef<'_>) {
        self.mark_sibling_lines(node);
        match node.kind() {
            S::Block => self.mark_block(node),
            kind if comma_group(kind, node, &self.parsed.tokens) => self.mark_comma_group(node),
            kind if binary(kind) => self.mark_binary(node),
            S::AssignmentStmt | S::BindingExpr | S::DefaultValue | S::TypeDefault => {
                self.mark_assignment(node);
            }
            S::NamedArgument => self.mark_named_argument(node),
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
        if !matches!(node.kind(), S::Root | S::Block | S::MatchExpr) {
            return;
        }
        let mut children = node
            .children()
            .filter(|child| child.first_token().get().is_some());
        let Some(mut previous) = children.next() else {
            return;
        };
        for child in children {
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
        let inline_pass = self.block_is_inline_pass(node);
        let inline = inline_pass || self.block_may_inline(node);
        let gap = if inline_pass {
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
        let one_statement = node
            .children()
            .filter(|child| child.first_token().get().is_some())
            .count()
            == 1;
        match parent.kind() {
            S::ClosureExpr | S::MatchArm => {
                one_statement
                    && !node
                        .descendants()
                        .any(|descendant| matches!(descendant.kind(), S::IfExpr | S::MatchExpr))
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

    fn if_is_value(&self, node: NodeRef<'_>) -> bool {
        if !node
            .descendants()
            .any(|descendant| descendant.kind() == S::ElseClause)
        {
            return false;
        }
        let mut current = node;
        while let Some(parent) = self.parent(current) {
            match parent.kind() {
                S::ParenExpr
                | S::BindingExpr
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
        if close > open + 1 {
            self.indent_starts[open + 1] += 1;
            self.indent_ends[close] += 1;
        }
        let padded = matches!(node.kind(), S::DataExpr | S::MapExpr) && close > open + 1;
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
            .is_some_and(contains_suite);
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
        for operator in node
            .direct_tokens()
            .filter(|&token| binary_operator(tokens.kind(token)))
        {
            let range = matches!(tokens.kind(operator), T::DotDot | T::DotDotEq);
            self.set_gap(
                operator.idx(),
                if range { Gap::None } else { Gap::Soft(" ") },
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
        }
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
        self.set_gap(
            first.idx() + 1,
            if glued_amp { Gap::Space } else { Gap::None },
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
        for &dot in dots.iter().skip(1) {
            self.set_gap(dot, Gap::Soft(""));
        }
        if let Some(&first) = dots.get(1) {
            self.indent_starts[first] += 1;
            self.chain_indents[node.index().idx()] = true;
        }
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
        self.ancestors(node)
            .any(|ancestor| ancestor.kind() == S::PipeExpr)
    }

    fn is_suite_header(&self, node: NodeRef<'_>) -> bool {
        let last = node.last_token().idx();
        self.ancestors(node).any(|ancestor| {
            matches!(
                ancestor.kind(),
                S::IfExpr | S::ForExpr | S::WhileExpr | S::MatchExpr
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
                | S::AssignmentStmt
                | S::ReturnStmt
                | S::ExprStmt
                | S::Argument
                | S::NamedArgument
                | S::IfExpr
                | S::ForExpr
                | S::WhileExpr
                | S::MatchExpr
        ) {
            return None;
        }
        let (first, last) = token_bounds(node)?;
        let lo = self.parsed.tokens.end[first] as usize;
        let hi = self.parsed.tokens.start[last] as usize;
        let comment_free = !self
            .parsed
            .tokens
            .com_start
            .iter()
            .any(|&start| (start as usize) >= lo && (start as usize) < hi);
        let start = self.parsed.tokens.start[first] as usize;
        (comment_free && self.flat_node_fits(child, column(self.source, start))).then_some(child)
    }

    fn flat_node_fits(&self, node: NodeRef<'_>, mut width: usize) -> bool {
        let Some((first, last)) = token_bounds(node) else {
            return true;
        };
        for index in first..=last {
            if index > first {
                if !self.comments[index].is_empty() {
                    return false;
                }
                let gap = match self.gaps[index] {
                    Gap::Auto => self.default_gap(index),
                    gap => gap,
                };
                width += match gap {
                    Gap::Auto | Gap::None => 0,
                    Gap::Space => 1,
                    Gap::Soft(flat) => flat.chars().count(),
                    Gap::Hard | Gap::Blank => return false,
                };
            }
            if self.optional_commas[index] || self.suppressed_commas[index] {
                continue;
            }
            let start = self.parsed.tokens.start[index] as usize;
            let end = self.parsed.tokens.end[index] as usize;
            let token = &self.source[start..end];
            if token.contains('\n') {
                return false;
            }
            width += token.chars().count();
            if width > MAX_COLUMNS {
                return false;
            }
        }
        true
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

    fn ancestors(&self, node: NodeRef<'_>) -> impl Iterator<Item = NodeRef<'_>> + '_ {
        let mut next = self.parent(node);
        std::iter::from_fn(move || {
            let current = next?;
            next = self.parent(current);
            Some(current)
        })
    }

    fn default_gap(&self, boundary: usize) -> Gap {
        canonical_gap(&self.parsed.tokens, boundary)
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
        self.emit_node(self.parsed.tree.root());
        self.emit_gap(self.parsed.tokens.len());
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

        let regular_binary = binary(node.kind())
            && node.kind() != S::PipeExpr
            && !self
                .analysis
                .parent(node)
                .is_some_and(|parent| parent.kind() == node.kind());
        let pipe = node.kind() == S::PipeExpr
            && !self
                .analysis
                .parent(node)
                .is_some_and(|parent| parent.kind() == S::PipeExpr);
        let chain_group = self.analysis.chain_indents[node.index().idx()];
        let wrapped_chain = chain_group && self.analysis.is_suite_header(node);
        let grouped = comma_group(node.kind(), node, &self.parsed.tokens)
            || matches!(node.kind(), S::IfExpr | S::ClosureExpr | S::MatchArm)
            || chain_group;

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
        } else if pipe || grouped {
            self.document.commands.push(C::EndGroup);
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
        while index <= last {
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
                    self.document.commands.push(C::ModeText {
                        flat: "",
                        broken: ",",
                    });
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
        let comments = self.analysis.comments[boundary].clone();
        let delay_ends = self.delay_indent_end(boundary, &comments);
        if !delay_ends {
            self.emit_indent_ends(boundary);
        }
        for _ in 0..self.analysis.indent_starts[boundary] {
            self.document.commands.push(C::StartIndent);
        }

        if comments.is_empty() {
            self.emit_gap_kind(boundary);
        } else {
            self.emit_comments(boundary, &comments);
        }
        if delay_ends {
            self.emit_indent_ends(boundary);
        }
    }

    fn emit_indent_ends(&mut self, boundary: usize) {
        for _ in 0..self.analysis.indent_ends[boundary] {
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

    fn emit_comments(&mut self, boundary: usize, comments: &[usize]) {
        let tokens = &self.parsed.tokens;
        let previous_end = boundary
            .checked_sub(1)
            .map_or(0, |index| tokens.end[index] as usize);
        let next_start = tokens
            .start
            .get(boundary)
            .map_or(self.source.len(), |&start| start as usize);
        let mut cursor = previous_end;
        for (position, &comment) in comments.iter().enumerate() {
            let start = tokens.com_start[comment] as usize;
            let end = tokens.com_end[comment] as usize;
            let inline =
                boundary > 0 && position == 0 && !self.source[cursor..start].contains('\n');
            if inline {
                self.document.commands.push(C::Atom(" "));
                self.document.commands.push(C::LineSuffix(start..end));
            } else {
                let blank = self.source[cursor..start].matches('\n').count() > 1;
                self.document
                    .commands
                    .push(if blank { C::BlankLine } else { C::HardLine });
                self.document.commands.push(C::LineSuffix(start..end));
            }
            cursor = end;
            let following = comments
                .get(position + 1)
                .map_or(next_start, |&next| tokens.com_start[next] as usize);
            let blank = self.source[cursor..following].matches('\n').count() > 1;
            self.document
                .commands
                .push(if blank { C::BlankLine } else { C::HardLine });
        }
    }

    fn delay_indent_end(&self, boundary: usize, comments: &[usize]) -> bool {
        if self.analysis.indent_ends[boundary] == 0 || comments.is_empty() {
            return false;
        }
        let tokens = &self.parsed.tokens;
        let previous_end = boundary
            .checked_sub(1)
            .map_or(0, |index| tokens.end[index] as usize);
        let first = tokens.com_start[comments[0]] as usize;
        if !self.source[previous_end..first].contains('\n') {
            return false;
        }
        let comment_column = column(self.source, first);
        let next_column = tokens
            .start
            .get(boundary)
            .map_or(0, |&start| column(self.source, start as usize));
        comment_column > next_column
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
        || previous.is_keyword() && (next == T::Dot || next == T::LParen && previous != T::KwFn)
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

fn contains_suite(node: NodeRef<'_>) -> bool {
    matches!(node.kind(), S::ClosureExpr | S::IfExpr | S::MatchExpr)
        || node.descendants().any(|descendant| {
            matches!(descendant.kind(), S::ClosureExpr | S::IfExpr | S::MatchExpr)
        })
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
        || word(previous) && matches!(next, T::LParen | T::LBracket)
        || matches!(previous, T::Question | T::Bang) && next == T::LParen
}

fn space_around(kind: T) -> bool {
    matches!(kind, T::Arrow | T::FatArrow)
}
