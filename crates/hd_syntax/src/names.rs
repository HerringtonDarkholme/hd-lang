//! The names a file's top-level declarations bring in, from its tokens
//! alone: no parse, so a file that does not parse still has them. The
//! REPL completes them, and decides by them whether an input needs
//! `src/lib.hd`.

use core::ops::Range;

use crate::{TokenBuf, TokenKind as T, lex};

/// The top-level names that `text` declares: functions, bindings, types,
/// traits, and the names a `use` brings in. A line lexed once, no parse.
#[must_use]
pub fn top_level_names(text: &str) -> Vec<String> {
    let lexed = lex(text.as_bytes());
    let tokens = &lexed.tokens;
    let col0 = |i: usize| {
        let lo = tokens.start[i] as usize;
        lo == 0 || text.as_bytes()[lo - 1] == b'\n'
    };
    // The tokens of each top-level declaration: from a token at the start
    // of a line outside brackets, up to the next one.
    let mut heads = Vec::new();
    let mut depth = 0usize;
    for i in 0..tokens.len() {
        if depth == 0 && col0(i) {
            heads.push(i);
        }
        let kind = tokens.kind[i];
        if kind.is_open_delimiter() || kind == T::StrHead {
            depth += 1;
        } else if kind.is_close_delimiter() || kind == T::StrTail {
            depth = depth.saturating_sub(1);
        }
    }
    heads.push(tokens.len());
    let mut out = Vec::new();
    for pair in heads.windows(2) {
        declaration(text, tokens, pair[0]..pair[1], &mut out);
    }
    out
}

/// The names that one top-level declaration, the tokens `range`, brings in.
fn declaration(text: &str, tokens: &TokenBuf, range: Range<usize>, out: &mut Vec<String>) {
    let kind = |i: usize| (i < range.end).then(|| tokens.kind[i]);
    let word = |i: usize| {
        let (lo, hi) = (tokens.start[i] as usize, tokens.end[i] as usize);
        text.get(lo..hi).unwrap_or("")
    };
    let mut at = range.start;
    // Past `pub` and annotations such as `@derive(Eq)`.
    loop {
        match kind(at) {
            Some(T::KwPub) => at += 1,
            Some(T::At) => {
                at += 1;
                while matches!(kind(at), Some(T::Ident | T::Dot)) {
                    at += 1;
                }
                if kind(at) == Some(T::LParen) {
                    let mut depth = 0;
                    while let Some(k) = kind(at) {
                        depth += usize::from(k == T::LParen);
                        depth -= usize::from(k == T::RParen);
                        at += 1;
                        if depth == 0 {
                            break;
                        }
                    }
                }
            }
            _ => break,
        }
    }
    let name = |i: usize, out: &mut Vec<String>| {
        if kind(i) == Some(T::Ident) {
            out.push(word(i).to_owned());
        }
    };
    match kind(at) {
        Some(T::KwFn | T::KwType | T::KwData | T::KwStruct | T::KwEnum | T::KwTrait) => {
            name(at + 1, out);
        }
        Some(T::KwLet) => name(at + 1 + usize::from(kind(at + 1) == Some(T::KwMut)), out),
        Some(T::Ident) if word(at) == "use" && kind(at + 1) == Some(T::Ident) => {
            for i in at + 1..range.end {
                let next_is = |k: T| kind(i + 1) == Some(k);
                let before_as = next_is(T::Ident) && word(i + 1) == "as";
                let root = i == at + 1 && matches!(word(i), "pkg" | "std" | "dep");
                if !(before_as || next_is(T::Dot) || root || matches!(word(i), "as" | "super")) {
                    name(i, out);
                }
            }
        }
        Some(T::Ident) if kind(at + 1) == Some(T::ColonEq) => name(at, out),
        _ => {}
    }
}

/// The name after each `fn` of `text`, at any depth: its top-level
/// functions, and the methods of its traits and implementations.
#[must_use]
pub fn function_names(text: &str) -> Vec<String> {
    let lexed = lex(text.as_bytes());
    let tokens = &lexed.tokens;
    (1..tokens.len())
        .filter(|&i| tokens.kind[i - 1] == T::KwFn && tokens.kind[i] == T::Ident)
        .filter_map(|i| text.get(tokens.start[i] as usize..tokens.end[i] as usize))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_method_name_is_a_function_name_but_not_a_top_level_one() {
        let text = "use std.cmp.{max as most}\n\ntrait Shape:\n    fn area(self) -> f64\n\nfn bonus() -> i32:\n    +1\n\nrate := +3\n";
        assert_eq!(top_level_names(text), ["most", "Shape", "bonus", "rate"]);
        assert_eq!(function_names(text), ["area", "bonus"]);
    }
}
