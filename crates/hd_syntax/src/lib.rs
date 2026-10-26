#![forbid(unsafe_code)]

mod docs;
mod green;
mod layout;
mod lexer;
mod names;
mod parser;
mod skim;

pub use docs::{doc_text, module_doc_text};
pub use green::*;
pub use layout::{Layout, LayoutCursor, SuiteKind, TokenOrLayout};
pub use lexer::{CommentKind, Lexed, LineFlags, TokenBuf, TokenKind, lex};
pub use names::{function_names, top_level_names};
pub use parser::{ItemIndex, ItemKind, Parse, parse};
pub use skim::{BodyRange, HeaderKind, HeaderSkeleton, skeleton_from_layout, skim};

/// Normalizes text to NFC, using the installed host on wasm32.
#[must_use]
pub fn normalize_nfc(text: &str) -> String {
    if text.is_ascii() {
        return text.to_owned();
    }
    #[cfg(target_arch = "wasm32")]
    {
        NFC_NORMALIZER.with(|hook| {
            hook.get()
                .expect("install the web NFC normalizer before lexing")(text)
        })
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use unicode_normalization::UnicodeNormalization as _;
        text.nfc().collect()
    }
}

/// Tests NFC without calling the host for ASCII input.
#[must_use]
pub fn is_nfc(text: &str) -> bool {
    if text.is_ascii() {
        return true;
    }
    #[cfg(target_arch = "wasm32")]
    {
        normalize_nfc(text) == text
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use unicode_normalization::UnicodeNormalization as _;
        text.nfc().eq(text.chars())
    }
}

/// A web embedding supplies an owned, normalized UTF-8 result.
#[cfg(target_arch = "wasm32")]
pub type NfcNormalizer = fn(&str) -> String;

#[cfg(target_arch = "wasm32")]
thread_local! {
    static NFC_NORMALIZER: std::cell::Cell<Option<NfcNormalizer>> = const { std::cell::Cell::new(None) };
}

/// Installs this worker's normalizer before it lexes any source.
#[cfg(target_arch = "wasm32")]
pub fn set_nfc_normalizer(normalize: NfcNormalizer) {
    NFC_NORMALIZER.with(|hook| hook.set(Some(normalize)));
}

#[cfg(test)]
mod normalization_tests {
    #[test]
    fn nfc_predicate_and_owned_normalization_agree() {
        for text in ["ascii", "é", "e\u{301}", "\u{344}", "\u{1100}\u{1161}"] {
            let normalized = super::normalize_nfc(text);
            assert!(super::is_nfc(&normalized));
            assert_eq!(super::is_nfc(text), normalized == text);
        }
        assert_eq!(super::normalize_nfc("e\u{301}"), "é");
        assert_eq!(super::normalize_nfc("\u{344}"), "\u{308}\u{301}");
    }
}
