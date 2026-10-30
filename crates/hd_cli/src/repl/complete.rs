//! Tab completion of names at the REPL prompt (`cli.repl` line editing). The
//! word before the cursor completes from the keywords, the prelude's names,
//! `src/lib.hd`'s top-level names and those of the session's earlier
//! inputs; at the start of a `:` line the REPL commands complete instead.
//! Names are read once, when an input succeeds or the session reloads, and
//! kept in [`Names`], which the line editor's helper shares with the session
//! thread. A keystroke lexes only the line being edited.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use hd_syntax::TokenKind as T;

/// The words that are keywords in every place (the lexer's keyword table),
/// and `use`, which is one in a use declaration.
const KEYWORDS: &[&str] = &[
    "Self", "break", "continue", "data", "defer", "dyn", "else", "enum", "false", "fn", "for",
    "if", "impl", "in", "is", "let", "match", "mut", "pass", "pub", "return", "self", "struct",
    "tests", "trait", "true", "type", "use", "while",
];

const COMMANDS: &[&str] = &[":help", ":quit", ":reload"];

#[derive(Default)]
struct Sets {
    /// The top-level names of `src/lib.hd`, when a package is open.
    package: BTreeSet<String>,
    /// The top-level names of each input that joined the session.
    inputs: BTreeMap<u32, Vec<String>>,
}

/// The names a session declares so far, shared between the session thread
/// that updates them and the line editor's helper that reads them.
#[derive(Clone, Default)]
pub(crate) struct Names(Arc<Mutex<Sets>>);

impl Names {
    fn sets(&self) -> MutexGuard<'_, Sets> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Replaces the names of `src/lib.hd` with those declared by `text`.
    pub(crate) fn set_package(&self, text: Option<&str>) {
        self.sets().package = text
            .map(hd_syntax::top_level_names)
            .unwrap_or_default()
            .into_iter()
            .collect();
    }

    /// Records the names that the input `number`, which joined the session,
    /// declares.
    pub(crate) fn add_input(&self, number: u32, text: &str) {
        self.sets()
            .inputs
            .insert(number, hd_syntax::top_level_names(text));
    }

    /// Forgets the names of the input `number`.
    pub(crate) fn drop_input(&self, number: u32) {
        self.sets().inputs.remove(&number);
    }

    /// The sorted, distinct words that start with `prefix`.
    fn matching(&self, prefix: &str) -> Vec<String> {
        let sets = self.sets();
        let prelude = hd_resolve::lower::PRELUDE
            .iter()
            .flat_map(|(_, l)| l.iter().copied());
        let found: BTreeSet<&str> = KEYWORDS
            .iter()
            .copied()
            .chain(prelude)
            .chain(sets.package.iter().map(String::as_str))
            .chain(sets.inputs.values().flatten().map(String::as_str))
            .filter(|w| w.starts_with(prefix))
            .collect();
        found.into_iter().map(str::to_owned).collect()
    }
}

/// The candidates for the word before byte `pos` of `line`, and where that
/// word starts. Nothing completes an empty word, a member after `.`, or
/// text inside a string or a comment.
pub(crate) fn complete(names: &Names, line: &str, pos: usize) -> (usize, Vec<String>) {
    let Some(head) = line.get(..pos) else {
        return (pos, Vec::new());
    };
    if head.trim_start().starts_with(':') {
        let at = head.len() - head.trim_start().len();
        let word = &head[at..];
        if word.contains(char::is_whitespace) {
            return (pos, Vec::new());
        }
        let found = COMMANDS.iter().filter(|c| c.starts_with(word));
        return (at, found.map(|c| (*c).to_owned()).collect());
    }
    let word = head.len()
        - head
            .trim_end_matches(|c: char| c.is_alphanumeric() || c == '_')
            .len();
    let start = pos - word;
    let prefix = &head[start..];
    let starts_name = prefix
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_');
    if !starts_name || head[..start].ends_with('.') || inside_text(head, start) {
        return (pos, Vec::new());
    }
    (start, names.matching(prefix))
}

/// Whether byte `at` of `line` is inside a string, a character literal or
/// a comment, as the lexer reads the line up to there.
fn inside_text(head: &str, at: usize) -> bool {
    let lexed = hd_syntax::lex(head.as_bytes());
    let tokens = &lexed.tokens;
    let inside = |lo: u32, hi: u32| (lo as usize) < at && at < hi as usize;
    if tokens
        .com_start
        .iter()
        .zip(&tokens.com_end)
        .any(|(&lo, &hi)| {
            // A comment runs to the end of the line, so the cursor at its end is inside.
            (lo as usize) < at && at <= hi as usize
        })
    {
        return true;
    }
    (0..tokens.len()).any(|i| {
        let (lo, hi) = (tokens.start[i], tokens.end[i]);
        if !inside(lo, hi) {
            return false;
        }
        match tokens.kind[i] {
            T::String | T::StrHead | T::StrMid | T::StrTail | T::Char | T::Lifetime => true,
            // An unterminated string is an error token to the end of the line.
            T::Error => head[lo as usize..hi as usize]
                .trim_start_matches(|c: char| c.is_alphabetic())
                .starts_with('"'),
            _ => false,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offered(names: &Names, line: &str) -> Vec<String> {
        complete(names, line, line.len()).1
    }

    fn session() -> Names {
        let names = Names::default();
        names.set_package(Some("pub fn libname() -> i32: 1\n"));
        names.add_input(1, "fn price(n: i32) -> i32: n * rate\n");
        names.add_input(2, "rate := +3\nlet mut total = +0\n");
        names.add_input(
            3,
            "@derive(Eq) struct Point(x: f64, y: f64)\npub enum Color:\n    Red\n",
        );
        names.add_input(4, "use std.collections.{Deque, Set as Bag}\nuse pkg.shapes\ntrait Named:\n    fn name(self) -> string\n");
        names
    }

    #[test]
    fn a_declared_name_and_a_prelude_name_complete() {
        let names = session();
        assert_eq!(offered(&names, "pri"), ["price", "println"]);
        assert_eq!(offered(&names, "1 + ra"), ["rate"]);
        assert_eq!(offered(&names, "to"), ["total"]);
        assert_eq!(offered(&names, "Po"), ["Point"]);
        assert_eq!(offered(&names, "Co"), ["Color", "Console", "ConsoleError"]);
        assert_eq!(offered(&names, "Ba"), ["Bag"]);
        assert_eq!(offered(&names, "Deq"), ["Deque"]);
        assert_eq!(offered(&names, "sha"), ["shapes"]);
        assert_eq!(offered(&names, "Na"), ["Named"]);
        assert_eq!(offered(&names, "lib"), ["libname"]);
        assert!(
            offered(&names, "na").is_empty(),
            "members are not top-level names"
        );
    }

    #[test]
    fn a_keyword_prefix_completes() {
        let names = session();
        assert_eq!(offered(&names, "ret"), ["return"]);
        assert_eq!(offered(&names, "x := if tr"), ["trait", "true"]);
        assert_eq!(offered(&names, "pub us"), ["use", "usize"]);
    }

    #[test]
    fn the_word_before_the_cursor_completes_and_reports_where_it_starts() {
        let names = session();
        let line = "f(pri) + 1";
        assert_eq!(
            complete(&names, line, 5),
            (2, vec!["price".into(), "println".into()])
        );
    }

    #[test]
    fn nothing_completes_for_an_empty_word_a_member_or_a_number() {
        let names = session();
        assert!(offered(&names, "").is_empty());
        assert!(offered(&names, "f(").is_empty());
        assert!(offered(&names, "p.pri").is_empty());
        assert!(offered(&names, "12pri").is_empty());
    }

    #[test]
    fn nothing_completes_inside_a_string_or_a_comment() {
        let names = session();
        assert!(offered(&names, "\"pri").is_empty());
        assert!(offered(&names, "\"a pri").is_empty());
        assert!(offered(&names, "x := 1 # pri").is_empty());
        assert!(offered(&names, "#pri").is_empty());
        assert!(offered(&names, "'p").is_empty());
        assert_eq!(offered(&names, "\"a ${pri"), ["price", "println"]);
        assert_eq!(offered(&names, "\"a\" + pri"), ["price", "println"]);
    }

    #[test]
    fn commands_complete_at_the_start_of_a_colon_line() {
        let names = session();
        assert_eq!(offered(&names, ":re"), [":reload"]);
        assert_eq!(offered(&names, ":"), [":help", ":quit", ":reload"]);
        assert_eq!(complete(&names, "  :he", 5), (2, vec![":help".to_owned()]));
        assert!(offered(&names, ":reload pri").is_empty());
    }

    #[test]
    fn a_dropped_input_s_names_are_not_offered() {
        let names = session();
        names.drop_input(2);
        assert!(offered(&names, "ra").is_empty());
        assert!(offered(&names, "to").is_empty());
        assert_eq!(offered(&names, "pri"), ["price", "println"]);
    }

    #[test]
    fn candidates_are_sorted_and_distinct() {
        let names = session();
        names.add_input(5, "fn price() -> i32: 1\nfn println() -> i32: 1\n");
        assert_eq!(offered(&names, "pri"), ["price", "println"]);
    }
}
