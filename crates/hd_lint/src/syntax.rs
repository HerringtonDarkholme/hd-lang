//! Advice over selected, lexically valid source tokens, without checked-body facts.

use hd_base::{FileId, Span};
use hd_diag::{Code, DiagBuf};
use hd_syntax::{TokenKind, lex};

use crate::Policy;

/// Inspect the selected package/file/test scope using its original diagnostic file IDs.
/// Callers keep prerequisite errors in the compiler; this pass reports advice only.
#[must_use]
pub fn analyze_sources<'a>(
    sources: impl IntoIterator<Item = (FileId, &'a str)>,
    policy: Policy,
) -> DiagBuf {
    policy.apply(&collect_sources(sources))
}

/// Build source-token findings without command policy.
#[must_use]
pub fn collect_sources<'a>(sources: impl IntoIterator<Item = (FileId, &'a str)>) -> DiagBuf {
    let mut diags = DiagBuf::default();
    for (file, source) in sources {
        if source.is_ascii() {
            continue;
        }
        let lexed = lex(source.as_bytes());
        if !lexed.diagnostics.is_empty() {
            continue;
        }
        for (i, kind) in lexed.tokens.kind.iter().enumerate() {
            // Quoted identifiers historically do not receive this advice.
            if *kind != TokenKind::Ident {
                continue;
            }
            let (lo, hi) = (lexed.tokens.start[i], lexed.tokens.end[i]);
            let text = &source[lo as usize..hi as usize];
            if text.is_ascii() {
                continue;
            }
            let scripts = mixed_scripts(text);
            if scripts.len() > 1 {
                diags.push(
                    Code::MixedScriptIdentifier,
                    hd_diag::Severity::Warning,
                    Span { file, lo, hi },
                    &format!(
                        "identifier '{text}' mixes {} scripts",
                        join_scripts(&scripts)
                    ),
                    None,
                );
            }
        }
    }
    diags
}

/// The confusable scripts an identifier's letters use, in fixed order:
/// Latin, Greek, Cyrillic (`lex.ident.mixed-script.lint`). Letters of
/// any other script never count, so Han with Latin stays quiet.
fn mixed_scripts(text: &str) -> Vec<&'static str> {
    let (mut latin, mut greek, mut cyrillic) = (false, false, false);
    for ch in text.chars().filter(|ch| ch.is_alphabetic()) {
        let c = ch as u32;
        if ch.is_ascii_alphabetic()
            || (0x00C0..=0x024F).contains(&c)
            || (0x1E00..=0x1EFF).contains(&c)
            || (0x2C60..=0x2C7F).contains(&c)
            || (0xA720..=0xA7FF).contains(&c)
            || (0xAB30..=0xAB6F).contains(&c)
        {
            latin = true;
        } else if (0x0370..=0x03FF).contains(&c) || (0x1F00..=0x1FFF).contains(&c) {
            greek = true;
        } else if (0x0400..=0x052F).contains(&c)
            || (0x2DE0..=0x2DFF).contains(&c)
            || (0xA640..=0xA69F).contains(&c)
            || (0x1C80..=0x1C8F).contains(&c)
        {
            cyrillic = true;
        }
    }
    [("Latin", latin), ("Greek", greek), ("Cyrillic", cyrillic)]
        .into_iter()
        .filter(|(_, found)| *found)
        .map(|(name, _)| name)
        .collect()
}

/// `"Latin"`, `"Latin and Cyrillic"`, `"Latin, Greek and Cyrillic"`.
fn join_scripts(scripts: &[&str]) -> String {
    let mut out = String::new();
    for (i, name) in scripts.iter().enumerate() {
        if i > 0 {
            out.push_str(if i + 1 == scripts.len() {
                " and "
            } else {
                ", "
            });
        }
        out.push_str(name);
    }
    out
}
