use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use hd_syntax::parse;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n.max(1)).unwrap_or(1)).unwrap_or(0)
    }
}

#[test]
fn fixed_budget_parser_fuzz_has_no_panic() {
    let mut rng = Rng(0x4844_5f53_594e_5441);
    for case in 0..4_096 {
        let len = rng.below(256);
        let source: Vec<u8> = (0..len).map(|_| rng.next().to_le_bytes()[0]).collect();
        let result = catch_unwind(AssertUnwindSafe(|| parse(&source)));
        assert!(
            result.is_ok(),
            "parser panicked for fuzz case {case}: {source:?}"
        );
    }
}

/// Fragments that steer mutations into the grammar's layout and suite paths.
const PIECES: &[&str] = &[
    ":",
    "(",
    ")",
    "[",
    "]",
    "{",
    "}",
    ",",
    ".",
    "..",
    "...",
    "=>",
    "|>",
    ":=",
    "=",
    "\n",
    "\n    ",
    "\n        ",
    "if ",
    "else",
    "for ",
    " in ",
    "match ",
    "fn ",
    "fn(x): ",
    "$.with(",
    "\"a ${",
    "}\"",
    "'",
    "\"",
    "@",
    "pub ",
    "let ",
    "return ",
    "-",
    "<",
    "::[",
    "!",
    "?",
    "_",
    "tests:",
    "#",
    "##",
    "`",
];

#[test]
fn mutated_std_sources_parse_without_panic_and_round_trip() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lib/std");
    let mut sources: Vec<String> = std::fs::read_dir(&root)
        .expect("lib/std")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "hd"))
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .collect();
    sources.sort();
    let mut rng = Rng(0x6d75_7461_7465_2121);
    for case in 0..600 {
        let base = &sources[rng.below(sources.len())];
        let mut text = base.clone();
        for _ in 0..=rng.below(6) {
            let mut at = rng.below(text.len() + 1);
            while !text.is_char_boundary(at) {
                at -= 1;
            }
            match rng.below(3) {
                0 => text.insert_str(at, PIECES[rng.below(PIECES.len())]),
                1 => {
                    let mut end = (at + rng.below(40)).min(text.len());
                    while !text.is_char_boundary(end) {
                        end -= 1;
                    }
                    text.replace_range(at..end, "");
                }
                _ => {
                    let mut end = (at + rng.below(80)).min(text.len());
                    while !text.is_char_boundary(end) {
                        end -= 1;
                    }
                    let copy = text[at..end].to_owned();
                    let mut to = rng.below(text.len() + 1);
                    while !text.is_char_boundary(to) {
                        to -= 1;
                    }
                    text.insert_str(to, &copy);
                }
            }
        }
        let result = catch_unwind(AssertUnwindSafe(|| {
            let parsed = parse(text.as_bytes());
            assert_eq!(parsed.tree.reconstruct(&parsed.tokens, &text), text);
        }));
        assert!(result.is_ok(), "mutation case {case} panicked:\n{text}");
    }
}

#[test]
fn deep_nesting_reports_instead_of_overflowing() {
    for opener in ["(", "[", "-", "fn(): ", "\"${", "if a: "] {
        let source = format!("x := {}1\n", opener.repeat(20_000));
        let parsed = parse(source.as_bytes());
        assert!(!parsed.diagnostics.is_empty(), "{opener}");
    }
}
