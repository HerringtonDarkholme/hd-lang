//! Advice over selected, lexically valid source tokens, without checked-body facts.

#[cfg(test)]
use hd_base::{FileId, Span};
use hd_diag::{Code, DiagBuf};
use hd_driver::analysis::Event;
use hd_syntax::SyntaxKind;
#[cfg(test)]
use hd_syntax::{TokenKind, lex};

/// `types.lifetime.convention`: a lifetime parameter written before a type
/// parameter of the same generic list.
pub(crate) fn lifetime_order(
    event: &Event<'_>,
    view: &hd_check::analysis::CheckedView<'_>,
    out: &mut DiagBuf,
) {
    if !matches!(event, Event::Module(_)) {
        return;
    }
    let source = view.source();
    for list in source.root().descendants() {
        if list.kind() != SyntaxKind::GenericParameterList {
            continue;
        }
        let kids: Vec<_> = list.children().collect();
        let Some(last_type) = kids
            .iter()
            .rposition(|c| c.kind() == SyntaxKind::GenericParameter)
        else {
            continue;
        };
        for p in &kids[..last_type] {
            if p.kind() != SyntaxKind::LifetimeParameter {
                continue;
            }
            let span = source.span(*p);
            let text = source.text_at(span).unwrap_or("lifetime");
            out.push(
                Code::LifetimeOrder,
                hd_diag::Severity::Warning,
                span,
                &format!(
                    "lifetime `{text}` comes before a type parameter; write type parameters first"
                ),
                None,
            );
        }
    }
}

pub(crate) fn mixed_script(
    event: &Event<'_>,
    view: &hd_check::analysis::CheckedView<'_>,
    out: &mut DiagBuf,
) {
    let Event::Ident(span) = event else {
        return;
    };
    let Some(text) = view.source().text_at(*span) else {
        return;
    };
    if text.is_ascii() {
        return;
    }
    let scripts = mixed_scripts(text);
    if scripts.len() > 1 {
        out.push(
            Code::MixedScriptIdentifier,
            hd_diag::Severity::Warning,
            *span,
            &format!(
                "identifier '{text}' mixes {} scripts",
                join_scripts(&scripts)
            ),
            None,
        );
    }
}

#[cfg(test)]
fn collect_sources<'a>(sources: impl IntoIterator<Item = (FileId, &'a str)>) -> DiagBuf {
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

#[cfg(test)]
mod tests {
    use hd_base::FileId;
    use hd_diag::Code;

    use super::collect_sources;

    #[test]
    fn exact_messages_and_utf8_spans_follow_identifier_tokens() {
        let source = "# аbc is a comment\nаbc aαя \"аbc\" `аbc` 日本a";
        let file = FileId::from_raw(17);
        let findings = collect_sources([(file, source)]);
        assert_eq!(findings.code, [Code::MixedScriptIdentifier; 2]);
        for (i, name, scripts) in [
            (0, "аbc", "Latin and Cyrillic"),
            (1, "aαя", "Latin, Greek and Cyrillic"),
        ] {
            assert_eq!(findings.primary[i].file, file);
            let lo = source.find('\n').unwrap() + 1;
            let start = source[lo..].find(name).unwrap() + lo;
            assert_eq!(
                (
                    findings.primary[i].lo as usize,
                    findings.primary[i].hi as usize,
                ),
                (start, start + name.len()),
            );
            assert_eq!(
                findings.get_text(findings.message[i]),
                format!("identifier '{name}' mixes {scripts} scripts"),
            );
        }
    }

    #[test]
    fn script_ranges_and_fixed_order_are_preserved() {
        for (text, scripts) in [
            ("éя", "Latin and Cyrillic"),
            ("ḁα", "Latin and Greek"),
            ("яα", "Greek and Cyrillic"),
            ("яαa", "Latin, Greek and Cyrillic"),
        ] {
            let findings = collect_sources([(FileId::from_raw(0), text)]);
            assert_eq!(findings.len(), 1, "{text}");
            assert_eq!(
                findings.get_text(findings.message[0]),
                format!("identifier '{text}' mixes {scripts} scripts"),
            );
        }
        for text in [
            "abc_1",
            "язык",
            "Ελλάδα",
            "日本a",
            "`аbc`",
            "\"аbc\"",
            "# аbc",
        ] {
            assert!(
                collect_sources([(FileId::from_raw(0), text)]).is_empty(),
                "{text}",
            );
        }
    }

    #[test]
    fn invalid_sources_and_an_empty_input_are_quiet() {
        for invalid in ["a\u{301}я", "`1а`", "аbc\0"] {
            assert!(
                hd_syntax::lex(invalid.as_bytes())
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == Code::InvalidToken),
            );
            assert!(collect_sources([(FileId::from_raw(0), invalid)]).is_empty());
        }
        assert!(collect_sources(std::iter::empty()).is_empty());
    }

    #[test]
    fn caller_file_ids_and_input_order_are_preserved() {
        let first = FileId::from_raw(8);
        let second = FileId::from_raw(3);
        let findings = collect_sources([(first, "аbc"), (second, "aα")]);
        assert_eq!(
            findings
                .primary
                .iter()
                .map(|span| span.file)
                .collect::<Vec<_>>(),
            [first, second],
        );
    }
}
