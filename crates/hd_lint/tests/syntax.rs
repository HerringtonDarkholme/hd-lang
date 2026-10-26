use hd_base::FileId;
use hd_diag::{Code, Severity};
use hd_lint::{Level, MIXED_SCRIPT, Policy, analyze_sources};

#[test]
fn exact_messages_and_original_utf8_spans_follow_identifier_tokens() {
    let source = "# аbc is a comment\nаbc aαя \"аbc\" `аbc` 日本a";
    let file = FileId::from_raw(17);
    let findings = analyze_sources([(file, source)], Policy::default());
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
                findings.primary[i].hi as usize
            ),
            (start, start + name.len())
        );
        assert_eq!(
            findings.get_text(findings.message[i]),
            format!("identifier '{name}' mixes {scripts} scripts")
        );
        assert_eq!(findings.fixes[i].len, 0);
    }
}

#[test]
fn the_existing_script_ranges_and_fixed_script_order_are_preserved() {
    for (text, scripts) in [
        ("éя", "Latin and Cyrillic"),
        ("ḁα", "Latin and Greek"),
        ("яα", "Greek and Cyrillic"),
        ("яαa", "Latin, Greek and Cyrillic"),
    ] {
        let findings = analyze_sources([(FileId::from_raw(0), text)], Policy::default());
        assert_eq!(findings.len(), 1, "{text}");
        assert_eq!(
            findings.get_text(findings.message[0]),
            format!("identifier '{text}' mixes {scripts} scripts")
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
            analyze_sources([(FileId::from_raw(0), text)], Policy::default()).is_empty(),
            "{text}"
        );
    }
}

#[test]
fn mixed_script_levels_are_independent_and_keep_error_prerequisites() {
    for (level, expected) in [
        (Level::Allow, None),
        (Level::Warn, Some(Severity::Warning)),
        (Level::Deny, Some(Severity::Error)),
    ] {
        let mut policy = Policy::default();
        policy.set(MIXED_SCRIPT, level).unwrap();
        let findings = analyze_sources([(FileId::from_raw(0), "аbc")], policy);
        assert_eq!(findings.severity.first().copied(), expected);
        for invalid in ["a\u{301}я", "`1а`", "аbc\0"] {
            let lexical = hd_syntax::lex(invalid.as_bytes());
            assert!(
                lexical
                    .diagnostics
                    .iter()
                    .any(|d| d.code == Code::InvalidToken)
            );
            assert!(analyze_sources([(FileId::from_raw(0), invalid)], policy).is_empty());
        }
    }
    let mut policy = Policy::default();
    policy.set(MIXED_SCRIPT, Level::Warn).unwrap();
    assert!(policy.set(MIXED_SCRIPT, Level::Deny).is_err());
}

#[test]
fn repeated_source_analysis_keeps_caller_file_ids_and_no_implicit_sources() {
    let first = FileId::from_raw(8);
    let second = FileId::from_raw(3);
    for _ in 0..3 {
        let findings = analyze_sources([(first, "аbc"), (second, "aα")], Policy::default());
        assert_eq!(
            findings
                .primary
                .iter()
                .map(|span| span.file)
                .collect::<Vec<_>>(),
            [first, second]
        );
    }
    assert!(analyze_sources(std::iter::empty(), Policy::default()).is_empty());
}
