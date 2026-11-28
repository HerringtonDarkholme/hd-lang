//! The lexer and parser messages a source file cannot hold: bytes that are
//! not UTF-8, and a nesting depth built by repetition. Every other message
//! is a stage case under `crates/hd_check/tests/cases/syntax-messages/`,
//! whose `diagnostics` view prints each one with its code and position.

use hd_syntax::parse;

use super::corpus::{blocks, stage_case, stage_cases};

/// No lexer or parser message is empty or only repeats its code, over every
/// diagnostic of the syntax message and fix-it cases.
#[test]
fn no_message_repeats_its_code() {
    for topic in ["syntax-messages", "syntax-fix-its"] {
        // cost: one walk of each topic's folder, each case read and parsed once.
        for path in stage_cases("hd_check", topic) {
            let source = std::fs::read(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            let parsed = parse(&source);
            assert!(!parsed.is_ok(), "{}: no diagnostic", path.display());
            for d in &parsed.diagnostics {
                assert!(
                    !d.message.trim().is_empty(),
                    "{}: empty message of {}",
                    path.display(),
                    d.code.as_str()
                );
                assert_ne!(
                    d.message,
                    d.code.as_str(),
                    "{}: message is the code",
                    path.display()
                );
            }
        }
    }
}

/// Each foreign form has exactly one diagnostic alone, and the forms
/// together have no more.
#[test]
fn each_foreign_form_has_one_diagnostic() {
    let source = stage_case("hd_check", "syntax-messages/foreign-syntax.hd");
    let snippets = blocks(&source);
    for snippet in &snippets {
        let parsed = parse(snippet.as_bytes());
        assert_eq!(
            parsed.diagnostics.len(),
            1,
            "{snippet}: {:?}",
            parsed.diagnostics
        );
    }
    assert_eq!(
        parse(source.as_bytes()).diagnostics.len(),
        snippets.len(),
        "the forms together"
    );
}

#[test]
fn source_that_is_not_utf8_says_so() {
    let parsed = parse(&[0xff, 0xfe, b'\n']);
    assert_eq!(parsed.diagnostics.len(), 1);
    assert_eq!(parsed.diagnostics[0].code.as_str(), "invalid-token");
    assert_eq!(
        parsed.diagnostics[0].message,
        "source text is not valid UTF-8"
    );
}

#[test]
fn source_nested_too_deep_names_the_limit() {
    let source = format!("x := {}1{}\n", "(".repeat(200), ")".repeat(200));
    let parsed = parse(source.as_bytes());
    assert!(parsed.diagnostics.iter().any(|d| {
        d.code.as_str() == "nesting-too-deep"
            && d.message == "source nests deeper than the implementation limit"
    }));
}
