//! Width layouts over parsed groups. Token contents and expression structure survive.

use hd_base::SliceSort as _;
use hd_syntax::{NodeRef, Parse, SyntaxKind as S, TokenBuf, TokenKind as T};

pub const MAX_COLUMNS: usize = 100;

struct Change {
    start: usize,
    end: usize,
    text: String,
}

/// Each successful expansion splits an overlong group at its element boundaries.
/// Reparse before another expansion: bracket suffixes and same-line suites
/// depend on physical line boundaries, not only on a token sequence.
pub(super) fn layout(mut current: String) -> String {
    loop {
        let parsed = hd_syntax::parse(current.as_bytes());
        let ranges = protected(&parsed);
        let mut changes = Vec::new();
        collect(
            parsed.tree.root(),
            None,
            &parsed,
            &current,
            &ranges,
            &mut changes,
        );
        changes.sort_stable_by(|a, b| a.start.cmp(&b.start).then_with(|| b.end.cmp(&a.end)));
        let next = changes.into_iter().find_map(|change| {
            let mut next = current.clone();
            next.replace_range(change.start..change.end, &change.text);
            (next != current && equivalent(&parsed, &current, &next)).then_some(next)
        });
        let Some(next) = next else { return current };
        current = next;
    }
}

fn bounds(node: NodeRef<'_>, tokens: &TokenBuf) -> Option<(usize, usize)> {
    let (first, last) = (node.first_token().get()?, node.last_token().get()?);
    Some((
        tokens.start[first.idx()] as usize,
        tokens.end[last.idx()] as usize,
    ))
}

fn columns(text: &str) -> usize {
    text.chars().count()
}

fn indent(source: &str, at: usize) -> &str {
    let line = source[..at].rfind('\n').map_or(0, |i| i + 1);
    let len = source[line..]
        .bytes()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .count();
    &source[line..line + len]
}

fn needs_break(source: &str, tokens: &TokenBuf, start: usize, end: usize) -> bool {
    let mut line = source[..start].rfind('\n').map_or(0, |i| i + 1);
    loop {
        let last = source[line..].find('\n').map_or(source.len(), |i| line + i);
        let code_end = tokens
            .com_start
            .iter()
            .map(|&at| at as usize)
            .find(|&at| at >= line && at < last)
            .unwrap_or(last);
        if columns(source[line..code_end].trim_end()) > MAX_COLUMNS {
            return true;
        }
        if last >= end || last == source.len() {
            return false;
        }
        line = last + 1;
    }
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

fn group(kind: S) -> bool {
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
            | S::ContextType
    )
}

fn comma_group(node: NodeRef<'_>, tokens: &TokenBuf) -> bool {
    group(node.kind())
        && node.kind() != S::ContextType
        && (node.kind() != S::ContextExpr
            || node.direct_tokens().any(|t| tokens.kind(t) == T::LParen))
}

fn collect<'a>(
    node: NodeRef<'a>,
    parent: Option<NodeRef<'a>>,
    parsed: &Parse,
    source: &str,
    ranges: &[(usize, usize)],
    out: &mut Vec<Change>,
) {
    // Interpolation expressions are part of the string's written text too.
    if node.kind() == S::StringExpr {
        return;
    }
    if (group(node.kind()) || binary(node.kind()))
        && let Some((start, end)) = bounds(node, &parsed.tokens)
        && (needs_break(source, &parsed.tokens, start, end)
            || (group(node.kind()) && source[start..end].contains('\n')))
    {
        if group(node.kind()) {
            if let Some(text) = group_layout(node, parsed, source, ranges, false) {
                out.push(Change { start, end, text });
            }
            // A comma after an indented suite must first dedent out of its
            // body. The same parse/shape guard judges this alternate boundary.
            if source[start..end].contains('\n')
                && let Some(text) = group_layout(node, parsed, source, ranges, true)
            {
                out.push(Change { start, end, text });
            }
        } else if binary(node.kind()) && !source[start..end].contains('\n') {
            let container = parent.filter(|p| p.kind() == S::ParenExpr).unwrap_or(node);
            if let Some((start, end)) = bounds(container, &parsed.tokens)
                && let Some(text) =
                    binary_layout(node, source, &parsed.tokens, indent(source, start))
            {
                out.push(Change { start, end, text });
            }
            // A comment inside existing parentheses must not disappear.
            // A transparent inner group can preserve that surrounding text.
            if parent.is_some_and(|p| p.kind() == S::ParenExpr)
                && let Some(text) =
                    binary_layout(node, source, &parsed.tokens, indent(source, start))
            {
                out.push(Change { start, end, text });
            }
        }
    }
    for child in node.children() {
        collect(child, Some(node), parsed, source, ranges, out);
    }
}

fn protected(parsed: &Parse) -> Vec<(usize, usize)> {
    let mut ranges: Vec<_> = parsed
        .tokens
        .com_start
        .iter()
        .zip(&parsed.tokens.com_end)
        .map(|(&lo, &hi)| (lo as usize, hi as usize))
        .chain(
            parsed
                .tree
                .root()
                .descendants()
                .filter(|n| n.kind() == S::StringExpr)
                .filter_map(|n| bounds(n, &parsed.tokens)),
        )
        .collect();
    ranges.sort_stable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

fn inside(ranges: &[(usize, usize)], at: usize) -> bool {
    let index = ranges.partition_point(|&(start, _)| start <= at);
    index > 0 && at < ranges[index - 1].1
}

fn trim_layout(
    source: &str,
    ranges: &[(usize, usize)],
    mut lo: usize,
    mut hi: usize,
) -> (usize, usize) {
    let bytes = source.as_bytes();
    while lo < hi && bytes[lo].is_ascii_whitespace() && !inside(ranges, lo) {
        lo += 1;
    }
    while hi > lo && bytes[hi - 1].is_ascii_whitespace() && !inside(ranges, hi - 1) {
        hi -= 1;
    }
    (lo, hi)
}

fn element(
    source: &str,
    tokens: &TokenBuf,
    ranges: &[(usize, usize)],
    lo: usize,
    hi: usize,
    deeper: &str,
    comma: Option<usize>,
) -> String {
    let (lo, hi) = trim_layout(source, ranges, lo, hi);
    if lo == hi {
        return String::new();
    }
    let first = tokens
        .start
        .iter()
        .map(|&at| at as usize)
        .find(|&at| at >= lo && at < hi)
        .unwrap_or(lo);
    let old_indent = columns(indent(source, first));
    let new_indent = columns(deeper);
    let mut out = String::new();
    let mut at = lo;
    for (line, text) in source[lo..hi].split_inclusive('\n').enumerate() {
        if line == 0 || inside(ranges, at) {
            append_with_comma(&mut out, text, at, comma);
        } else {
            let spaces = text
                .bytes()
                .take_while(|b| matches!(b, b' ' | b'\t'))
                .count();
            let indentation = columns(&text[..spaces]);
            let shifted = if new_indent >= old_indent {
                indentation + new_indent - old_indent
            } else {
                indentation.saturating_sub(old_indent - new_indent)
            };
            out.push_str(&" ".repeat(shifted));
            append_with_comma(&mut out, &text[spaces..], at + spaces, comma);
        }
        at += text.len();
    }
    out
}

fn append_with_comma(out: &mut String, text: &str, start: usize, comma: Option<usize>) {
    if let Some(at) = comma
        && at >= start
        && at <= start + text.len()
    {
        out.push_str(&text[..at - start]);
        out.push(',');
        out.push_str(&text[at - start..]);
    } else {
        out.push_str(text);
    }
}

fn group_layout(
    node: NodeRef<'_>,
    parsed: &Parse,
    source: &str,
    ranges: &[(usize, usize)],
    standalone_comma: bool,
) -> Option<String> {
    let tokens = &parsed.tokens;
    let (start, end) = bounds(node, tokens)?;
    let open = node
        .direct_tokens()
        .find(|&t| tokens.kind(t).is_open_delimiter())?;
    let close = node.last_token();
    if !tokens.kind(close).is_close_delimiter() {
        return None;
    }
    let begin = tokens.end[open.idx()] as usize;
    let stop = tokens.start[close.idx()] as usize;
    if close.idx() <= open.idx() + 1 {
        return None;
    }
    let last = close.idx() - 1;
    let comma = (comma_group(node, tokens) && tokens.kind[last] != T::Comma)
        .then_some(tokens.end[last] as usize);
    let base = indent(source, tokens.start[open.idx()] as usize);
    let deeper = format!("{base}    ");
    let mut out = source[start..begin].trim_end().to_owned();
    let mut next = begin;
    let mut boundaries = Vec::new();
    for comma in node.direct_tokens().filter(|&t| tokens.kind(t) == T::Comma) {
        let mut end = tokens.end[comma.idx()] as usize;
        if let Some((i, &comment)) = tokens
            .com_start
            .iter()
            .enumerate()
            .find(|&(_, &lo)| lo as usize >= end)
            && (comment as usize) < stop
            && source[end..comment as usize]
                .bytes()
                .all(|b| matches!(b, b' ' | b'\t'))
        {
            end = tokens.com_end[i] as usize;
        }
        boundaries.push(end);
    }
    boundaries.push(stop);
    for end in boundaries {
        let item = element(
            source,
            tokens,
            ranges,
            next,
            end,
            &deeper,
            if standalone_comma { None } else { comma },
        );
        if !item.is_empty() {
            out.push('\n');
            out.push_str(&deeper);
            out.push_str(&item);
        }
        next = end;
    }
    if standalone_comma && comma.is_some() {
        out.push('\n');
        out.push_str(&deeper);
        out.push(',');
    }
    out.push('\n');
    out.push_str(base);
    out.push_str(&source[stop..end]);
    Some(out)
}

fn operands(node: NodeRef<'_>, kind: S, tokens: &TokenBuf, out: &mut Vec<(usize, usize)>) {
    for child in node.children() {
        if child.kind() == kind {
            operands(child, kind, tokens, out);
        } else if let Some(span) = bounds(child, tokens) {
            out.push(span);
        }
    }
}

fn binary_layout(node: NodeRef<'_>, source: &str, tokens: &TokenBuf, base: &str) -> Option<String> {
    let mut parts = Vec::new();
    operands(node, node.kind(), tokens, &mut parts);
    if parts.len() < 2 {
        return None;
    }
    let mut out = String::from("(\n");
    let deeper = format!("{base}    ");
    let mut at = parts[0].0;
    for (_, end) in parts {
        out.push_str(&deeper);
        out.push_str(source[at..end].trim());
        out.push('\n');
        at = end;
    }
    out.push_str(base);
    out.push(')');
    Some(out)
}

fn shape(node: NodeRef<'_>, out: &mut Vec<(S, bool)>) {
    let keep = node.kind() != S::ParenExpr;
    if keep {
        out.push((node.kind(), true));
    }
    for child in node.children() {
        shape(child, out);
    }
    if keep {
        out.push((node.kind(), false));
    }
}

fn trailing_commas(parsed: &Parse) -> (Vec<bool>, Vec<bool>) {
    let tokens = &parsed.tokens;
    let mut skip = vec![false; tokens.kind.len()];
    let mut present = Vec::new();
    for node in parsed
        .tree
        .root()
        .descendants()
        .filter(|&n| comma_group(n, tokens))
    {
        let close = node.last_token();
        let comma = close.idx().checked_sub(1).filter(|&i| {
            tokens.kind(close).is_close_delimiter()
                && tokens.kind[i] == T::Comma
                && node.direct_tokens().any(|t| t.idx() == i)
        });
        if let Some(i) = comma {
            skip[i] = true;
        }
        present.push(comma.is_some());
    }
    (skip, present)
}

fn token_text<'a>(parsed: &Parse, source: &'a str, skip: &[bool]) -> Vec<(T, &'a str)> {
    parsed
        .tokens
        .kind
        .iter()
        .enumerate()
        .filter(|(i, kind)| !skip[*i] && !matches!(kind, T::LParen | T::RParen))
        .map(|(i, &kind)| {
            (
                kind,
                &source[parsed.tokens.start[i] as usize..parsed.tokens.end[i] as usize],
            )
        })
        .collect()
}

fn comments<'a>(parsed: &Parse, source: &'a str) -> Vec<&'a str> {
    parsed
        .tokens
        .com_start
        .iter()
        .zip(&parsed.tokens.com_end)
        .map(|(&start, &end)| &source[start as usize..end as usize])
        .collect()
}

fn equivalent(before: &Parse, source: &str, next: &str) -> bool {
    let after = hd_syntax::parse(next.as_bytes());
    if !after.is_ok() || comments(before, source) != comments(&after, next) {
        return false;
    }
    let (mut old, mut new) = (Vec::new(), Vec::new());
    shape(before.tree.root(), &mut old);
    shape(after.tree.root(), &mut new);
    if old != new {
        return false;
    }
    // Matching tree shape also matches comma groups in preorder. Only a new
    // optional trailing comma is allowed: an existing one may never disappear,
    // and every nontrailing token (apart from transparent parentheses) is exact.
    let (old_skip, old_present) = trailing_commas(before);
    let (new_skip, new_present) = trailing_commas(&after);
    old_present.len() == new_present.len()
        && old_present
            .iter()
            .zip(new_present)
            .all(|(&old, new)| !old || new)
        && token_text(before, source, &old_skip) == token_text(&after, next, &new_skip)
}

#[cfg(test)]
mod tests {
    use super::equivalent;

    #[test]
    fn only_optional_trailing_commas_may_be_added_and_none_may_be_removed() {
        for (before, after, expected) in [
            (
                "fn f(): consume(value)\n",
                "fn f(): consume(value,)\n",
                true,
            ),
            (
                "fn f(): consume(value,)\n",
                "fn f(): consume(value)\n",
                false,
            ),
            (
                "fn f(): consume(inner(value), other)\n",
                "fn f(): consume(inner(value,), other,)\n",
                true,
            ),
            ("fn f(): (value)\n", "fn f(): (value,)\n", false),
            (
                "fn f(): consume(value)\n",
                "fn f(): consume(value, other)\n",
                false,
            ),
            (
                "fn f(): consume(value) # comment\n",
                "fn f(): consume(value,) # changed\n",
                false,
            ),
            (
                "fn f(): consume(\"text\")\n",
                "fn f(): consume(\"changed\",)\n",
                false,
            ),
        ] {
            let parsed = hd_syntax::parse(before.as_bytes());
            assert!(parsed.is_ok(), "{before}");
            assert_eq!(
                equivalent(&parsed, before, after),
                expected,
                "{before} -> {after}"
            );
        }
    }
}
