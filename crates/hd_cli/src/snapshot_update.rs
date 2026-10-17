//! `hd test --update` for in-code snapshots (`cli.test.snapshot.update`):
//! a failing `snapshot(text, expect="...")` call has its `expect` literal
//! rewritten in the source file to the text the call saw.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use hd_syntax::TokenKind as T;

/// `text` as a string literal in source form, the one `debug` writes
/// (`std-format.debug.source.string`): `\`, `"` and `$` escaped, line feed,
/// carriage return, tab and NUL as `\n`, `\r`, `\t` and `\0`, and any
/// other ASCII control character as `\u{HEX}`.
pub(crate) fn debug_literal(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' | '"' | '$' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if c.is_ascii_control() => {
                let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// What the failing runs want at one call site: the literal, or `None`
/// when they want different texts, and how many runs recorded it.
struct Want {
    literal: Option<String>,
    runs: usize,
}

/// The rewrites of one `hd test --update` run: each call site, by its
/// file and its line, with the literal the failing runs want there.
#[derive(Default)]
pub(crate) struct Updates {
    sites: BTreeMap<(String, u32), Want>,
}

/// What [`Updates::apply`] did: a note for each literal written, and each
/// call it left as it was, with the message and the count of runs that
/// recorded it. Those runs passed, but their snapshots are not recorded,
/// so the caller counts them as failures.
#[derive(Default)]
pub(crate) struct Applied {
    pub(crate) notes: Vec<String>,
    pub(crate) unwritten: Vec<(String, usize)>,
}

impl Updates {
    /// Notes that a run of the call at `file:line` failed and saw
    /// `literal`.
    pub(crate) fn record(&mut self, file: &str, line: u32, literal: &str) {
        let want = self
            .sites
            .entry((file.to_owned(), line))
            .or_insert_with(|| Want {
                literal: Some(literal.to_owned()),
                runs: 0,
            });
        want.runs += 1;
        if want.literal.as_deref() != Some(literal) {
            want.literal = None;
        }
    }

    /// Rewrites the files. `read` gives a file's text as the run compiled
    /// it, and `path` where the file is on disk; a file that changed on
    /// disk since the run is left as it is.
    pub(crate) fn apply(
        self,
        read: impl Fn(&str) -> Option<String>,
        path: impl Fn(&str) -> PathBuf,
    ) -> Applied {
        let mut done = Applied::default();
        let mut by_file: BTreeMap<&str, Vec<(u32, &Want)>> = BTreeMap::new();
        for ((file, line), want) in &self.sites {
            by_file.entry(file).or_default().push((*line, want));
        }
        for (file, sites) in by_file {
            let text = read(file);
            // The edit of each site: (start, end, new text, line, runs).
            let mut edits: Vec<(usize, usize, String, u32, usize)> = Vec::new();
            for (line, want) in sites {
                let edit = match (&want.literal, &text) {
                    (None, _) => Err("failing cases want different texts"),
                    (Some(l), Some(t)) => locate(t, line, l).ok_or("its call was not found"),
                    (Some(_), None) => Err("its file was not found"),
                };
                match edit {
                    Ok((lo, hi, new)) => edits.push((lo, hi, new, line, want.runs)),
                    Err(why) => done
                        .unwritten
                        .push((format!("not updated {file}:{line}: {why}"), want.runs)),
                }
            }
            let Some(text) = text else {
                continue;
            };
            edits.sort_by_key(|e| std::cmp::Reverse(e.0));
            // Two sites never share a span, but a call that two lines both
            // name would; the later edit is dropped.
            edits.dedup_by(|b, a| b.1 > a.0);
            if edits.is_empty() {
                continue;
            }
            let mut out = text.clone();
            for (lo, hi, new, ..) in &edits {
                out.replace_range(*lo..*hi, new);
            }
            match write(&path(file), &text, &out) {
                Ok(()) => {
                    let mut lines: Vec<u32> = edits.iter().map(|e| e.3).collect();
                    lines.sort_unstable();
                    done.notes
                        .extend(lines.iter().map(|l| format!("updated {file}:{l}")));
                }
                Err(e) => {
                    for (_, _, _, line, runs) in &edits {
                        done.unwritten
                            .push((format!("not updated {file}:{line}: {e}"), *runs));
                    }
                }
            }
        }
        done
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
    fn a_text_is_written_as_debug_writes_it() {
        assert_eq!(
            debug_literal("a\"$\\\n\t\0\u{1}\u{7f}é"),
            "\"a\\\"\\$\\\\\\n\\t\\0\\u{1}\\u{7f}é\""
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
