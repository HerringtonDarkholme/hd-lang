//! `hd test --update` for in-code snapshots (`cli.test.snapshot.update`):
//! a failing `snapshot(text, expect="...")` call has its `expect` literal
//! rewritten in the source file to the text the call saw. A call in a doc
//! test (`cli.test.doc.update-prefix`) is rewritten inside its `##` lines.
//!
//! The call says where its `expect` is: `std.testing.snapshot` is a
//! `@track_caller` function that takes the span from
//! `Location::caller_args()` (`module.testing.snapshot.site`) and gives it
//! to the host with the text ([`Site::parse`]). So an update reads no
//! source to find the call; it reads the file it rewrites.

use hd_base::SliceSort as _;
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

/// What an update rewrites at a failing `snapshot` call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// The `expect` argument's span, which must be a string literal.
    Expect,
    /// No `expect`: `, expect=` and the literal go at the span's end, the
    /// end of the last argument (`cli.test.snapshot.update.omitted`).
    Add,
}

/// Where an update rewrites a failing `snapshot` call: the file as the
/// run names it, the line where the call begins, which messages name, and
/// the 1-based line and column, in characters, of the span's start and
/// end. A doc test's span is in its block's code, the
/// `##` lines with their prefix cut (`doc`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Site {
    pub(crate) kind: Kind,
    pub(crate) file: String,
    pub(crate) line: u32,
    pub(crate) start: (u32, u32),
    pub(crate) end: (u32, u32),
    pub(crate) doc: bool,
}

impl Site {
    /// The site and the text that `std.testing.snapshot_site` wrote,
    /// fields separated by U+001F: the kind, the file, the call's line,
    /// the start line and column, the end line and column, then the text. `Ok(None)` with the
    /// text for a call the update cannot rewrite, one from outside the
    /// user's own code; `Err` for a record no std writes.
    pub(crate) fn parse(record: &str) -> Result<(Option<Site>, &str), ()> {
        let mut f = record.splitn(8, '\u{1f}');
        let mut field = || f.next().ok_or(());
        let kind = field()?;
        let file = field()?;
        let mut number = || field()?.parse::<u32>().map_err(|_| ());
        let line = number()?;
        let start = (number()?, number()?);
        let end = (number()?, number()?);
        let text = field()?;
        let kind = match kind {
            "expect" => Kind::Expect,
            "add" => Kind::Add,
            "" => return Ok((None, text)),
            _ => return Err(()),
        };
        let site = Site {
            kind,
            file: file.to_owned(),
            line,
            start,
            end,
            doc: false,
        };
        Ok((Some(site), text))
    }
}

/// What the failing runs want at one call site: the literal, or `None`
/// when they want different texts, and how many runs recorded it.
struct Want {
    site: Site,
    literal: Option<String>,
    runs: usize,
}

/// The rewrites of one `hd test --update` run: each call site, by its
/// file and where its span starts, with the literal the failing runs want
/// there.
#[derive(Default)]
pub(crate) struct Updates {
    sites: BTreeMap<(String, u32, u32), Want>,
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
    /// Notes that a run of the call at `site` failed and saw `literal`.
    pub(crate) fn record(&mut self, site: Site, literal: &str) {
        let key = (site.file.clone(), site.start.0, site.start.1);
        let want = self.sites.entry(key).or_insert_with(|| Want {
            site,
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
    /// disk since the run is left as it is. Each file is read and written
    /// once, whatever its count of sites.
    pub(crate) fn apply(
        self,
        read: impl Fn(&str) -> Option<String>,
        path: impl Fn(&str) -> PathBuf,
    ) -> Applied {
        let mut done = Applied::default();
        let mut by_file: BTreeMap<&str, Vec<&Want>> = BTreeMap::new();
        for ((file, ..), want) in &self.sites {
            by_file.entry(file).or_default().push(want);
        }
        for (file, sites) in by_file {
            let text = read(file);
            let starts = text.as_deref().map(line_starts).unwrap_or_default();
            // The edit of each site: (start, end, new text, line, runs).
            let mut edits: Vec<(usize, usize, String, u32, usize)> = Vec::new();
            for want in sites {
                let line = want.site.line;
                let edit = match (&want.literal, &text) {
                    (None, _) => Err("failing cases want different texts"),
                    (Some(l), Some(t)) => locate(t, &starts, &want.site, l),
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
            edits.sort_stable_by_key(|e| std::cmp::Reverse(e.0));
            // Two sites never share a span, but nested calls' spans may
            // overlap; the later edit is dropped.
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
                    lines.sort_stable();
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

/// The edit of one call site: the byte span to replace and its new text,
/// or why there is none.
type Edit = Result<(usize, usize, String), &'static str>;

/// The span is not in the file as the run compiled it
/// (`cli.test.snapshot.update.unfixable`).
const NOT_FOUND: &str = "its call was not found";

/// The call's `expect` is no single string literal
/// (`cli.test.snapshot.update.non-literal`).
const NOT_LITERAL: &str = "its `expect` is not a string literal";

/// The byte offset where each line of `text` starts.
fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

/// The length of a doc line's prefix: its indentation, `##`, and one
/// space after it, as a doc test's code cuts it (`hd_project::doctest`).
fn doc_prefix(line: &str) -> usize {
    let rest = line.trim_start();
    let cut = line.len() - rest.len();
    match rest.strip_prefix("##") {
        Some(code) => cut + 2 + usize::from(code.starts_with(' ')),
        None => 0,
    }
}

/// The byte offset in `text` of 1-based `line` and `column`, a column in
/// characters after the line's doc prefix when `doc`.
fn offset(text: &str, starts: &[usize], (line, column): (u32, u32), doc: bool) -> Option<usize> {
    let lo = *starts.get((line as usize).checked_sub(1)?)?;
    let hi = starts.get(line as usize).map_or(text.len(), |e| e - 1);
    let row = text.get(lo..hi)?;
    let cut = if doc { doc_prefix(row) } else { 0 };
    let code = &row[cut..];
    let column = (column as usize).checked_sub(1)?;
    let at = code
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(code.len()))
        .nth(column)?;
    Some(lo + cut + at)
}

/// The edit that sets the `expect` of the call at `site` to `literal`.
fn locate(text: &str, starts: &[usize], site: &Site, literal: &str) -> Edit {
    let lo = offset(text, starts, site.start, site.doc).ok_or(NOT_FOUND)?;
    let hi = offset(text, starts, site.end, site.doc).ok_or(NOT_FOUND)?;
    if lo > hi {
        return Err(NOT_FOUND);
    }
    match site.kind {
        Kind::Add => Ok((hi, hi, format!(", expect={literal}"))),
        Kind::Expect => {
            // The span's code: a doc test's later lines without their prefix.
            let mut code = String::new();
            for (k, row) in text[lo..hi].split('\n').enumerate() {
                let cut = if site.doc && k > 0 {
                    doc_prefix(row)
                } else {
                    0
                };
                if k > 0 {
                    code.push('\n');
                }
                code.push_str(&row[cut..]);
            }
            is_literal(&code)
                .then(|| (lo, hi, literal.to_owned()))
                .ok_or(NOT_LITERAL)
        }
    }
}

/// Whether `code` is one string literal and nothing else.
fn is_literal(code: &str) -> bool {
    let lexed = hd_syntax::lex(code.as_bytes());
    let t = &lexed.tokens;
    lexed.diagnostics.is_empty() && t.len() == 1 && t.kind[0] == T::String
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(kind: Kind, start: (u32, u32), end: (u32, u32), doc: bool) -> Site {
        Site {
            kind,
            file: "f.hd".into(),
            line: start.0,
            start,
            end,
            doc,
        }
    }

    fn rewrite(text: &str, site: &Site, literal: &str) -> Result<String, &'static str> {
        let (lo, hi, new) = locate(text, &line_starts(text), site, literal)?;
        let mut out = text.to_owned();
        out.replace_range(lo..hi, &new);
        Ok(out)
    }

    #[test]
    fn a_text_is_written_as_debug_writes_it() {
        assert_eq!(
            debug_literal("a\"$\\\n\t\0\u{1}\u{7f}é"),
            "\"a\\\"\\$\\\\\\n\\t\\0\\u{1}\\u{7f}é\""
        );
    }

    /// The record `std.testing.snapshot_site` writes; the text keeps any
    /// separator it holds.
    #[test]
    fn a_site_record_is_read_back() {
        let (s, text) =
            Site::parse("expect\u{1f}src/a.hd\u{1f}3\u{1f}3\u{1f}20\u{1f}4\u{1f}7\u{1f}a\u{1f}b")
                .expect("record");
        assert_eq!(
            s,
            Some(site(Kind::Expect, (3, 20), (4, 7), false)).map(|mut s| {
                s.file = "src/a.hd".into();
                s
            })
        );
        assert_eq!(text, "a\u{1f}b");
        let (s, text) = Site::parse("\u{1f}<std>/x.hd\u{1f}1\u{1f}1\u{1f}1\u{1f}1\u{1f}1\u{1f}t")
            .expect("record");
        assert_eq!((s, text), (None, "t"));
        assert_eq!(
            Site::parse("other\u{1f}f\u{1f}1\u{1f}1\u{1f}1\u{1f}1\u{1f}1\u{1f}t"),
            Err(())
        );
        assert_eq!(Site::parse("expect\u{1f}f\u{1f}x"), Err(()));
    }

    /// `cli.test.snapshot.update.non-literal`: a computed `expect` is
    /// named as such, apart from a span that is not there.
    #[test]
    fn a_computed_expect_is_not_rewritten() {
        let computed = "    snapshot(f(), expect=g())\n";
        let s = site(Kind::Expect, (1, 26), (1, 29), false);
        assert_eq!(rewrite(computed, &s, "\"new\""), Err(NOT_LITERAL));
        let second = "    snapshot(f(), \"a\" + b)\n";
        let s = site(Kind::Expect, (1, 19), (1, 26), false);
        assert_eq!(rewrite(second, &s, "\"new\""), Err(NOT_LITERAL));
        let s = site(Kind::Expect, (7, 1), (7, 2), false);
        assert_eq!(rewrite("x\n", &s, "\"new\""), Err(NOT_FOUND));
        let s = site(Kind::Expect, (1, 9), (1, 2), false);
        assert_eq!(rewrite("x\n", &s, "\"new\""), Err(NOT_FOUND));
    }

    #[test]
    fn a_literal_is_replaced_on_one_line_or_across_lines() {
        let one = "    snapshot(f(), expect=\"old\")\n";
        let s = site(Kind::Expect, (1, 26), (1, 31), false);
        assert_eq!(
            rewrite(one, &s, "\"new\"").as_deref(),
            Ok("    snapshot(f(), expect=\"new\")\n")
        );
        let many = "snapshot(g(\n    1), \n  expect=\"\"\"\n  a\n  \"\"\")\n";
        let s = site(Kind::Expect, (3, 10), (5, 6), false);
        assert_eq!(
            rewrite(many, &s, "\"x\\n\"").as_deref(),
            Ok("snapshot(g(\n    1), \n  expect=\"x\\n\")\n")
        );
        // Columns count characters, not bytes.
        let wide = "é(\"é\", expect=\"old\")\n";
        let s = site(Kind::Expect, (1, 15), (1, 20), false);
        assert_eq!(
            rewrite(wide, &s, "\"v\"").as_deref(),
            Ok("é(\"é\", expect=\"v\")\n")
        );
    }

    #[test]
    fn a_doc_test_literal_is_replaced_inside_its_comment_lines() {
        let text = "## Doc.\n##\n## ```hd\n## snapshot(f(),\n##     expect=\"\"\"\n##     a\n##     \"\"\")\n## ```\nfn f(): 1\n    ## snapshot(g())\n";
        let s = site(Kind::Expect, (5, 12), (7, 8), true);
        assert_eq!(
            rewrite(text, &s, "\"new\"").as_deref(),
            Ok(
                "## Doc.\n##\n## ```hd\n## snapshot(f(),\n##     expect=\"new\")\n## ```\nfn f(): 1\n    ## snapshot(g())\n"
            )
        );
        let s = site(Kind::Add, (10, 10), (10, 13), true);
        assert_eq!(
            rewrite(text, &s, "\"x\"").as_deref(),
            Ok(&*text.replace("snapshot(g())", "snapshot(g(), expect=\"x\")"))
        );
    }

    #[test]
    fn a_missing_expect_is_added() {
        let s = site(Kind::Add, (1, 10), (1, 13), false);
        assert_eq!(
            rewrite("snapshot(f())\n", &s, "\"v\"").as_deref(),
            Ok("snapshot(f(), expect=\"v\")\n")
        );
    }
}
