//! `hd test --update` for in-code snapshots (`cli.test.snapshot.update`):
//! a failing `snapshot(text, expect="...")` call has its `expect` literal
//! rewritten in the source file to the text the call saw.

use std::collections::BTreeMap;
use std::path::PathBuf;

use hd_syntax::TokenKind as T;

/// How `std.testing.snapshot` words its failure, up to the expected text's
/// `debug` literal (`lib/std/testing.hd`).
const PREFIX: &str = "the text differs from the snapshot's expectation: expected ";

/// The `debug` string literal of the text a failed `snapshot` saw, from
/// the panic message `message`. It is a string literal in source form
/// (`std-format.debug.source.string`), so it is written back as it is.
/// `None` for any other message, or a message that is not exactly the
/// snapshot's.
pub(crate) fn actual_literal(message: &str) -> Option<&str> {
    let rest = message.strip_prefix(PREFIX)?;
    let after = literal_end(rest)?;
    let actual = rest[after..].strip_prefix(", actual ")?;
    (literal_end(actual)? == actual.len()).then_some(actual)
}

/// The byte index after the string literal that starts `s`.
fn literal_end(s: &str) -> Option<usize> {
    let mut chars = s.char_indices();
    if chars.next()?.1 != '"' {
        return None;
    }
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next()?;
            }
            '"' => return Some(i + 1),
            _ => {}
        }
    }
    None
}

/// What the failing runs want at one call site.
enum Want {
    Literal(String),
    /// Runs that want different texts leave the literal as it is.
    Conflict,
}

/// The rewrites of one `hd test --update` run: each call site, by its
/// file and its line, with the literal the failing runs want there.
#[derive(Default)]
pub(crate) struct Updates {
    sites: BTreeMap<(String, u32), Want>,
}

impl Updates {
    /// Notes that a run of the call at `file:line` failed and saw
    /// `literal`.
    pub(crate) fn record(&mut self, file: &str, line: u32, literal: &str) {
        match self.sites.entry((file.to_owned(), line)) {
            std::collections::btree_map::Entry::Vacant(e) => {
                e.insert(Want::Literal(literal.to_owned()));
            }
            std::collections::btree_map::Entry::Occupied(mut e) => {
                if !matches!(e.get(), Want::Literal(l) if l == literal) {
                    e.insert(Want::Conflict);
                }
            }
        }
    }

    /// Rewrites the files. `read` gives a file's text as the run compiled
    /// it, and `path` where the file is on disk; a file that changed on
    /// disk since the run is left as it is. Returns one `FILE:LINE` note
    /// for each literal written, and one message for each file that could
    /// not be written.
    pub(crate) fn apply(
        self,
        read: impl Fn(&str) -> Option<String>,
        path: impl Fn(&str) -> PathBuf,
    ) -> (Vec<String>, Vec<String>) {
        let mut by_file: BTreeMap<&str, Vec<(u32, &str)>> = BTreeMap::new();
        for ((file, line), want) in &self.sites {
            if let Want::Literal(l) = want {
                by_file.entry(file).or_default().push((*line, l));
            }
        }
        let (mut notes, mut errors) = (Vec::new(), Vec::new());
        for (file, sites) in by_file {
            let Some(text) = read(file) else {
                continue;
            };
            let mut edits: Vec<(usize, usize, String, u32)> = Vec::new();
            for (line, literal) in sites {
                if let Some((lo, hi, new)) = locate(&text, line, literal) {
                    edits.push((lo, hi, new, line));
                }
            }
            edits.sort_by_key(|e| std::cmp::Reverse(e.0));
            // Two sites never share a span, but a call that two lines both
            // name would; the later edit is dropped.
            edits.dedup_by(|b, a| b.1 > a.0);
            if edits.is_empty() {
                continue;
            }
            let mut out = text.clone();
            for (lo, hi, new, _) in &edits {
                out.replace_range(*lo..*hi, new);
            }
            let disk = path(file);
            match write(&disk, &text, &out) {
                Ok(()) => {
                    let mut lines: Vec<u32> = edits.iter().map(|e| e.3).collect();
                    lines.sort_unstable();
                    notes.extend(lines.iter().map(|l| format!("updated {file}:{l}")));
                }
                Err(e) => errors.push(format!("{file}: {e}")),
            }
        }
        (notes, errors)
    }
}

/// Replaces the file at `path` with `text`, through a temporary file
/// beside it, unless it no longer holds `before`.
fn write(path: &std::path::Path, before: &str, text: &str) -> Result<(), String> {
    let now = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if now != before {
        return Err("the file changed while the tests ran, so it is left as it is".to_owned());
    }
    let temp = path.with_extension("hd.update-tmp");
    std::fs::write(&temp, text)
        .and_then(|()| std::fs::rename(&temp, path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            e.to_string()
        })
}

/// The edit that sets the `expect` of the `snapshot` call that covers
/// 1-based `line` to `literal`: the byte span to replace and its new text.
/// `None` when no such call is there, or its `expect` is no single string
/// literal.
fn locate(text: &str, line: u32, literal: &str) -> Option<(usize, usize, String)> {
    let lexed = hd_syntax::lex(text.as_bytes());
    let t = &lexed.tokens;
    let line_of = |at: u32| 1 + text[..at as usize].matches('\n').count();
    // The covering call that starts last, which is the innermost.
    let mut best: Option<(usize, usize)> = None;
    for i in 0..t.len().saturating_sub(1) {
        let named = t.kind[i] == T::Ident
            && &text[t.start[i] as usize..t.end[i] as usize] == "snapshot"
            && t.kind[i + 1] == T::LParen
            && (i == 0 || t.kind[i - 1] != T::Dot);
        if !named {
            continue;
        }
        let Some(close) = matching(t, i + 1) else {
            continue;
        };
        let (first, last) = (line_of(t.start[i]), line_of(t.end[close] - 1));
        if first <= line as usize && (line as usize) <= last {
            best = Some((i, close));
        }
    }
    let (call, close) = best?;
    // The arguments: token ranges split at the commas of depth one.
    let mut args: Vec<(usize, usize)> = Vec::new();
    let (mut depth, mut from) = (0usize, call + 2);
    for k in call + 2..close {
        match t.kind[k] {
            T::LParen | T::LBracket | T::LBrace => depth += 1,
            T::RParen | T::RBracket | T::RBrace => depth = depth.saturating_sub(1),
            T::Comma if depth == 0 => {
                args.push((from, k));
                from = k + 1;
            }
            _ => {}
        }
    }
    if from < close {
        args.push((from, close));
    }
    let is_named =
        |(lo, hi): (usize, usize)| hi > lo + 1 && t.kind[lo] == T::Ident && t.kind[lo + 1] == T::Eq;
    let name_of = |lo: usize| &text[t.start[lo] as usize..t.end[lo] as usize];
    let mut value: Option<(usize, usize)> = None;
    let mut positional = 0;
    for &(lo, hi) in &args {
        if is_named((lo, hi)) {
            if name_of(lo) == "expect" {
                value = Some((lo + 2, hi));
            }
        } else {
            if positional == 1 && value.is_none() {
                value = Some((lo, hi));
            }
            positional += 1;
        }
    }
    if let Some((lo, hi)) = value {
        // A literal is one string token; anything else is left alone.
        (hi == lo + 1 && t.kind[lo] == T::String)
            .then(|| (t.start[lo] as usize, t.end[lo] as usize, literal.to_owned()))
    } else {
        // No `expect`: its empty default is filled on the first update.
        let last = close.checked_sub(1).filter(|l| *l > call + 1)?;
        let at = t.end[last] as usize;
        Some((at, at, format!(", expect={literal}")))
    }
}

/// The `)` that closes the `(` at token `open`.
fn matching(t: &hd_syntax::TokenBuf, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for k in open..t.len() {
        match t.kind[k] {
            T::LParen | T::LBracket | T::LBrace => depth += 1,
            T::RParen | T::RBracket | T::RBrace => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return (t.kind[k] == T::RParen).then_some(k);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rewrite(text: &str, line: u32, literal: &str) -> Option<String> {
        let (lo, hi, new) = locate(text, line, literal)?;
        let mut out = text.to_owned();
        out.replace_range(lo..hi, &new);
        Some(out)
    }

    #[test]
    fn the_actual_text_is_the_last_literal() {
        let m = r#"the text differs from the snapshot's expectation: expected "a, actual \"x\"", actual "b\n""#;
        assert_eq!(actual_literal(m), Some(r#""b\n""#));
        assert_eq!(actual_literal("other"), None);
        assert_eq!(
            actual_literal(
                r#"the text differs from the snapshot's expectation: expected "a", actual "b" tail"#
            ),
            None
        );
    }

    #[test]
    fn a_literal_is_replaced_on_one_line_or_across_lines() {
        let one = "    snapshot(f(), expect=\"old\")\n";
        assert_eq!(
            rewrite(one, 1, "\"new\"").as_deref(),
            Some("    snapshot(f(), expect=\"new\")\n")
        );
        let many = "snapshot(g(\n    1), \n  expect=\"\"\"\n  a\n  \"\"\")\n";
        assert_eq!(
            rewrite(many, 1, "\"x\\n\"").as_deref(),
            Some("snapshot(g(\n    1), \n  expect=\"x\\n\")\n")
        );
    }

    #[test]
    fn a_missing_expect_is_added() {
        assert_eq!(
            rewrite("snapshot(f())\n", 1, "\"v\"").as_deref(),
            Some("snapshot(f(), expect=\"v\")\n")
        );
    }

    #[test]
    fn a_computed_expect_is_left_alone() {
        assert_eq!(rewrite("snapshot(f(), expect=g())\n", 1, "\"v\""), None);
        assert_eq!(rewrite("x.snapshot(f(), expect=\"a\")\n", 1, "\"v\""), None);
    }
}
