//! Syntax colour for the line being edited at the REPL prompt. The token
//! classes and their ANSI colours are those of the website highlighter
//! (`website/src/highlight.ts`), except that operators stay uncoloured.
//! Colouring only wraps spans of the line in SGR sequences, which take no
//! terminal columns, so the visible text and the cursor position are those
//! of the original line. It lexes one line per keystroke and does not parse.

use hd_base::SliceSort as _;
use std::borrow::Cow;
use std::io::IsTerminal;

use hd_syntax::TokenKind as T;

use super::complete::{Names, complete};
use rustyline::completion::Completer;
use rustyline::highlight::{CmdKind, Highlighter};
use rustyline::hint::Hinter;
use rustyline::validate::Validator;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Keyword,
    Literal,
    Type,
    Function,
    Number,
    Str,
    Interpolation,
    Comment,
}

impl Class {
    /// The SGR parameters of the class's colour.
    fn sgr(self) -> &'static str {
        match self {
            Class::Keyword => "35",
            Class::Literal | Class::Number => "33",
            Class::Type => "36",
            Class::Function => "34",
            Class::Str => "32",
            Class::Interpolation => "1;32",
            Class::Comment => "90",
        }
    }
}

const PRIMITIVE_TYPES: &[&str] = &[
    "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "usize", "f32", "f64", "bool", "char",
    "string", "void", "never",
];

/// Whether the REPL colours its input line: standard output is a terminal
/// and `NO_COLOR` is not set.
pub(crate) fn color_enabled() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

/// The rustyline helper: colours the input line when `color` is set, and
/// completes names from those the session declares.
pub(crate) struct Helper {
    color: bool,
    names: Names,
}

impl Helper {
    pub(crate) fn new(color: bool) -> Self {
        Self {
            color,
            names: Names::default(),
        }
    }

    /// The names the helper completes from, for the session to update.
    pub(crate) fn names(&self) -> Names {
        self.names.clone()
    }
}

impl rustyline::Helper for Helper {}
impl Completer for Helper {
    type Candidate = String;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<String>)> {
        Ok(complete(&self.names, line, pos))
    }
}
impl Hinter for Helper {
    type Hint = String;
}
impl Validator for Helper {}

impl Highlighter for Helper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if self.color {
            Cow::Owned(paint(line))
        } else {
            Cow::Borrowed(line)
        }
    }

    /// Typing and deleting change colours, so each such key redraws the
    /// line; moving the cursor does not.
    fn highlight_char(&self, _line: &str, _pos: usize, kind: CmdKind) -> bool {
        self.color && kind != CmdKind::MoveCursor
    }
}

/// `line` with each coloured span wrapped in SGR sequences; the text
/// between them is `line` unchanged.
pub(crate) fn paint(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 32);
    let mut at = 0;
    for (lo, hi, class) in spans(line) {
        if lo < at || hi <= lo {
            continue;
        }
        out.push_str(&line[at..lo]);
        out.push_str("\x1b[");
        out.push_str(class.sgr());
        out.push('m');
        out.push_str(&line[lo..hi]);
        out.push_str("\x1b[0m");
        at = hi;
    }
    out.push_str(&line[at..]);
    out
}

/// The coloured spans of `line`, ordered and disjoint, as byte ranges.
fn spans(line: &str) -> Vec<(usize, usize, Class)> {
    let lexed = hd_syntax::lex(line.as_bytes());
    let tokens = &lexed.tokens;
    let mut found: Vec<(usize, usize, Class)> = Vec::new();
    for (&lo, &hi) in tokens.com_start.iter().zip(&tokens.com_end) {
        found.push((lo as usize, hi as usize, Class::Comment));
    }
    for i in 0..tokens.len() {
        let (lo, hi) = (tokens.start[i] as usize, tokens.end[i] as usize);
        let Some(text) = line.get(lo..hi) else {
            continue;
        };
        match tokens.kind[i] {
            T::Number => found.push((lo, hi, Class::Number)),
            T::Char => found.push((lo, hi, Class::Str)),
            T::String | T::StrHead | T::StrMid | T::StrTail => {
                string_spans(tokens.kind[i], lo, text, &mut found);
            }
            // An unterminated string is an error token that runs to the end
            // of the line.
            T::Error
                if text
                    .trim_start_matches(|c: char| c.is_alphabetic())
                    .starts_with('"') =>
            {
                string_spans(T::String, lo, text, &mut found);
            }
            T::KwTrue | T::KwFalse | T::KwSelfValue | T::KwSelfType => {
                found.push((lo, hi, Class::Literal));
            }
            T::Ident => {
                if let Some(class) = word_class(line, lo, hi) {
                    found.push((lo, hi, class));
                }
            }
            kind if kind.is_keyword() => found.push((lo, hi, Class::Keyword)),
            _ => {}
        }
    }
    found.sort_stable_by_key(|&(lo, _, _)| lo);
    found
}

/// The spans of one string piece at `base`: text, and the `${`, `}` and
/// `$name` that mark interpolation.
fn string_spans(kind: T, base: usize, text: &str, found: &mut Vec<(usize, usize, Class)>) {
    let mut from = 0;
    let mut to = text.len();
    if matches!(kind, T::StrMid | T::StrTail) && text.starts_with('}') {
        found.push((base, base + 1, Class::Interpolation));
        from = 1;
    }
    let mut tail = None;
    if matches!(kind, T::StrHead | T::StrMid) && text.ends_with("${") {
        to -= 2;
        tail = Some((base + to, base + to + 2, Class::Interpolation));
    }
    let body = &text[from..to];
    let mut text_start = from;
    let mut chars = body.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '\\' {
            chars.next();
        } else if c == '$' {
            let name = i + 1;
            let len: usize = body[name..]
                .char_indices()
                .take_while(|&(j, n)| {
                    if j == 0 {
                        n.is_alphabetic() || n == '_'
                    } else {
                        n.is_alphanumeric() || n == '_'
                    }
                })
                .map(|(_, n)| n.len_utf8())
                .sum();
            if len > 0 {
                let (lo, hi) = (from + i, from + name + len);
                found.push((base + text_start, base + lo, Class::Str));
                found.push((base + lo, base + hi, Class::Interpolation));
                text_start = hi;
                while chars.peek().is_some_and(|&(j, _)| j < hi - from) {
                    chars.next();
                }
            }
        }
    }
    found.push((base + text_start, base + to, Class::Str));
    found.extend(tail);
}

/// The class of the identifier at `lo..hi`: a contextual keyword, a type,
/// a function name (before a call), or none.
fn word_class(line: &str, lo: usize, hi: usize) -> Option<Class> {
    let word = &line[lo..hi];
    if contextual_keyword(line, lo, hi) {
        return Some(Class::Keyword);
    }
    if PRIMITIVE_TYPES.contains(&word) || word.chars().next().is_some_and(char::is_uppercase) {
        return Some(Class::Type);
    }
    let rest = line[hi..].trim_start();
    let calls = rest.starts_with('(')
        || rest
            .strip_prefix('[')
            .and_then(|r| r.split_once(']'))
            .is_some_and(|(_, r)| r.trim_start().starts_with('('));
    calls.then_some(Class::Function)
}

/// The words that are keywords only in a place: `use`, `super` and `as` in
/// a use declaration, a provider word after `$.`, and `derive` after `@`.
fn contextual_keyword(line: &str, lo: usize, hi: usize) -> bool {
    let word = &line[lo..hi];
    let before = &line[..lo];
    match word {
        "using" if before.trim().is_empty() => {
            let after = line[hi..].strip_prefix('!').unwrap_or(&line[hi..]);
            after.starts_with(char::is_whitespace)
                && after
                    .trim_start()
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
        }
        "use" => {
            before.ends_with("$.") || (use_declaration(line) && matches!(before.trim(), "" | "pub"))
        }
        "with" | "context" | "Context" => before.ends_with("$."),
        "derive" => before.ends_with('@'),
        "super" | "as" => use_declaration(line),
        _ => false,
    }
}

/// Whether `line` is a use declaration: `use` or `pub use`, then a root.
fn use_declaration(line: &str) -> bool {
    let rest = line.trim_start();
    let rest = rest
        .strip_prefix("pub")
        .filter(|r| r.starts_with(char::is_whitespace))
        .map_or(rest, str::trim_start);
    let Some(rest) = rest
        .strip_prefix("use")
        .filter(|r| r.starts_with(char::is_whitespace))
    else {
        return false;
    };
    let rest = rest.trim_start();
    let root_len = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    matches!(&rest[..root_len], "pkg" | "std" | "dep" | "self" | "super")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `text` with SGR sequences removed.
    fn strip(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for n in chars.by_ref() {
                    if n == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    const LINES: &[&str] = &[
        "let x = 1 + 2",
        "fn greet(name: string) -> string:",
        "    return \"hello ${name}, you are ${age + 1}!\"",
        "s := \"a $name b\"  # a comment",
        "# only a comment",
        "x := 0xff + 1_000 + 250ms + 1.5e3",
        "let s = \"unterminated ${x",
        "let t = \"unterminated",
        "let u = 'c' + '\\n'",
        "let greeting = \"héllo wörld ${名前} ✓\"  # ünïcode",
        "use std.io.{print}",
        "pub use pkg.a as b",
        "@derive(Eq) data Point(x: f64, y: f64)",
        "",
        "   ",
        "\"\"\"",
        "`fn` + `bad",
    ];

    #[test]
    fn colouring_never_changes_the_text() {
        for line in LINES {
            assert_eq!(strip(&paint(line)), *line, "{line:?}");
        }
    }

    #[test]
    fn classes_get_distinct_codes() {
        let line = "let s = \"a\"  # note";
        let painted = paint(line);
        assert!(painted.contains("\x1b[35mlet\x1b[0m"), "{painted:?}");
        assert!(painted.contains("\x1b[32m\"a\"\x1b[0m"), "{painted:?}");
        assert!(painted.contains("\x1b[90m# note\x1b[0m"), "{painted:?}");
        let number = paint("x = 12");
        assert!(number.contains("\x1b[33m12\x1b[0m"), "{number:?}");
        let interp = paint("\"a ${b}\"");
        assert!(interp.contains("\x1b[1;32m${\x1b[0m"), "{interp:?}");
        assert!(interp.contains("\x1b[1;32m}\x1b[0m"), "{interp:?}");
        let name = paint("\"a $b c\"");
        assert!(name.contains("\x1b[1;32m$b\x1b[0m"), "{name:?}");
        assert!(paint("Point(1)").starts_with("\x1b[36mPoint"));
        assert!(paint("go(1)").starts_with("\x1b[34mgo"));
        assert!(paint("use std.io").starts_with("\x1b[35muse"));
        assert!(paint("    using guard = open()").contains("\x1b[35musing\x1b[0m"));
        assert!(paint("    using! guard = open!()").contains("\x1b[35musing\x1b[0m!"));
        assert!(!paint("using := 1").starts_with("\x1b[35musing"));
    }

    #[test]
    fn an_unterminated_string_is_coloured_as_a_string() {
        assert!(paint("x = \"abc").ends_with("\x1b[32m\"abc\x1b[0m"));
    }

    #[test]
    fn colour_off_leaves_the_line_unchanged() {
        let helper = Helper::new(false);
        for line in LINES {
            assert!(matches!(helper.highlight(line, 0), Cow::Borrowed(l) if l == *line));
            assert!(!helper.highlight_char(line, 0, CmdKind::Other));
        }
        let on = Helper::new(true);
        assert!(on.highlight_char("x", 0, CmdKind::Other));
        assert!(!on.highlight_char("x", 0, CmdKind::MoveCursor));
        assert_ne!(on.highlight("let x", 0), "let x");
    }
}
