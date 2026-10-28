use hd_base::{FileId, Span};
use hd_diag::{Code, DiagBuf, Severity};
use hd_driver::analysis::Events;
use hd_lint::{Level, Policy, RULES};
use hd_project::parse_manifest;

const EXPECTED: [(&str, Code, Option<Events>); 14] = [
    ("broken-doc-link", Code::BrokenDocLink, None),
    (
        "dbg-in-dependency",
        Code::DbgInDependency,
        Some(Events::FETCHED_DEBUG),
    ),
    (
        "derivation-line-drift",
        Code::DerivationLineDrift,
        Some(Events::DERIVATIONS),
    ),
    ("lost-write", Code::LostWrite, Some(Events::PARAMETER_WRITE)),
    (
        "mixed-script-identifier",
        Code::MixedScriptIdentifier,
        Some(Events::IDENT),
    ),
    ("private-main", Code::PrivateMain, Some(Events::MAIN_SITE)),
    (
        "redundant-let-mut",
        Code::RedundantLetMut,
        Some(Events::BINDING),
    ),
    (
        "unreachable-code",
        Code::UnreachableCode,
        Some(Events::STATEMENT),
    ),
    (
        "unselected-main",
        Code::UnselectedMain,
        Some(Events::MAIN_SITE),
    ),
    (
        "unsigned-comparison-always",
        Code::UnsignedComparisonAlways,
        Some(Events::COMPARISON),
    ),
    (
        "unused-debug-text",
        Code::UnusedDebugText,
        Some(Events::DISCARD),
    ),
    (
        "unused-derivation-fact",
        Code::UnusedDerivationFact,
        Some(Events::DERIVATIONS),
    ),
    (
        "unused-local-binding",
        Code::UnusedLocalBinding,
        Some(Events::BINDING),
    ),
    (
        "variant-binding-name-mismatch",
        Code::VariantBindingNameMismatch,
        Some(Events::PATTERN),
    ),
];

fn policy_free_findings() -> DiagBuf {
    let mut findings = DiagBuf::default();
    for (i, rule) in RULES.iter().copied().enumerate() {
        let at = u32::try_from(i).unwrap();
        findings.push(
            rule.code(),
            Severity::Warning,
            Span {
                file: FileId::from_raw(0),
                lo: at,
                hi: at + 1,
            },
            rule.name(),
            None,
        );
    }
    findings
}

#[test]
fn registry_is_the_exact_unique_thirteen_rule_surface() {
    assert_eq!(RULES.len(), EXPECTED.len());
    for (rule, (name, code, events)) in RULES.iter().zip(EXPECTED) {
        assert_eq!(rule.name(), name);
        assert_eq!(rule.code(), code);
        assert_eq!(rule.default_level(), Level::Warn);
        assert_eq!(rule.events(), events);
    }
    for (i, rule) in RULES.iter().enumerate() {
        assert!(
            RULES[..i]
                .iter()
                .all(|earlier| earlier.name() != rule.name() && earlier.code() != rule.code()),
            "duplicate registry entry for {}",
            rule.name()
        );
    }
}

#[test]
fn indexed_policy_filters_and_relevels_a_policy_free_buffer() {
    let raw = policy_free_findings();
    assert_eq!(
        raw.code,
        RULES.iter().map(|rule| rule.code()).collect::<Vec<_>>()
    );
    assert!(
        raw.severity
            .iter()
            .all(|severity| *severity == Severity::Warning)
    );

    let defaults = Policy::default().apply(&raw);
    assert_eq!(defaults.code, raw.code);
    assert!(
        defaults
            .severity
            .iter()
            .all(|severity| *severity == Severity::Warning)
    );

    for rule in RULES.iter().copied() {
        let mut allow = Policy::default();
        allow.set(rule.name(), Level::Allow).unwrap();
        let allowed = allow.apply(&raw);
        assert_eq!(allowed.len(), RULES.len() - 1);
        assert!(!allowed.code.contains(&rule.code()));

        let mut deny = Policy::default();
        deny.set(rule.name(), Level::Deny).unwrap();
        let denied = deny.apply(&raw);
        assert_eq!(denied.code, raw.code);
        for (code, severity) in denied.code.iter().zip(&denied.severity) {
            assert_eq!(
                *severity,
                if *code == rule.code() {
                    Severity::Error
                } else {
                    Severity::Warning
                }
            );
        }
    }
}

#[test]
fn registry_names_reject_unknown_and_conflicting_levels() {
    let mut policy = Policy::default();
    assert_eq!(
        policy.set("not-a-lint", Level::Warn),
        Err("unknown lint `not-a-lint`".to_owned())
    );
    policy.set(EXPECTED[0].0, Level::Warn).unwrap();
    policy.set(EXPECTED[0].0, Level::Warn).unwrap();
    assert_eq!(
        policy.set(EXPECTED[0].0, Level::Deny),
        Err(format!("conflicting levels for lint `{}`", EXPECTED[0].0))
    );
}

#[test]
fn manifest_policy_validates_then_cli_levels_overlay_it() {
    let manifest = parse_manifest(
        "[lint]\ndeny = [\"unsigned-comparison-always\"]\nallow = [\"unused-debug-text\"]\n",
    )
    .unwrap();
    let policy = Policy::from_manifest(manifest.lint.as_ref()).unwrap();
    let raw = policy_free_findings();
    let applied = policy.apply(&raw);
    assert!(!applied.code.contains(&Code::UnusedDebugText));
    let unsigned = applied
        .code
        .iter()
        .position(|code| *code == Code::UnsignedComparisonAlways)
        .unwrap();
    assert_eq!(applied.severity[unsigned], Severity::Error);

    let mut explicit = Policy::default();
    explicit
        .set("unsigned-comparison-always", Level::Allow)
        .unwrap();
    let applied = policy.overlay(explicit).apply(&raw);
    assert!(!applied.code.contains(&Code::UnsignedComparisonAlways));
}

#[test]
fn invalid_manifest_values_names_and_conflicts_keep_their_line() {
    for (text, line, message) in [
        (
            "[lint]\nallow = \"unused-debug-text\"\n",
            2,
            "`lint.allow` must be a list of strings",
        ),
        (
            "[lint]\nallow = [\"not-a-lint\"]\n",
            2,
            "unknown lint `not-a-lint`",
        ),
        (
            "[lint]\nallow = [\"unused-debug-text\"]\ndeny = [\"unused-debug-text\"]\n",
            3,
            "conflicting levels for lint `unused-debug-text`",
        ),
    ] {
        let manifest = parse_manifest(text).unwrap();
        let error = Policy::from_manifest(manifest.lint.as_ref()).unwrap_err();
        assert_eq!(error.line, line);
        assert_eq!(error.message, message);
    }
}

#[test]
fn an_explicit_allow_for_every_registered_rule_disables_analysis() {
    let mut policy = Policy::default();
    for rule in RULES {
        policy.set(rule.name(), Level::Allow).unwrap();
    }
    assert!(policy.all_allow());
}

#[test]
#[should_panic(expected = "lint finding is absent from the registry")]
fn policy_rejects_a_finding_that_bypassed_registration() {
    let mut raw = DiagBuf::default();
    raw.push(
        Code::TypeMismatch,
        Severity::Error,
        Span {
            file: FileId::from_raw(0),
            lo: 0,
            hi: 1,
        },
        "not a lint",
        None,
    );
    let _ = Policy::default().apply(&raw);
}
