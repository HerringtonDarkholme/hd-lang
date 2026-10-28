//! Advice over selected source modules and their executable graph membership.

use hd_diag::{Code, DiagBuf, FixSafety};
use hd_driver::analysis::Event;
#[cfg(test)]
use hd_project::MainSite;
use hd_project::Role;
use hd_syntax::TokenKind;

pub(crate) fn private_main(
    event: &Event<'_>,
    view: &hd_check::analysis::CheckedView<'_>,
    out: &mut DiagBuf,
) {
    let Event::MainSite(site) = event else {
        return;
    };
    if site.public || !site.entry_module {
        return;
    }
    out.push(
        Code::PrivateMain,
        hd_diag::Severity::Warning,
        site.span,
        "main is not pub, so it is not the entry point",
        None,
    );
    let Some(header) = view.source().text_at(site.span) else {
        return;
    };
    let lexed = hd_syntax::lex(header.as_bytes());
    let Some(keyword) = lexed
        .tokens
        .kind
        .iter()
        .position(|kind| *kind == TokenKind::KwFn)
    else {
        return;
    };
    let at = site.span.lo + lexed.tokens.start[keyword];
    out.add_fix(
        "make `main` public",
        if site.make_public_exact {
            FixSafety::Exact
        } else {
            FixSafety::Suggestion
        },
        &[(
            hd_base::Span {
                file: site.span.file,
                lo: at,
                hi: at,
            },
            "pub ",
        )],
    );
}

pub(crate) fn unselected_main(
    event: &Event<'_>,
    _: &hd_check::analysis::CheckedView<'_>,
    out: &mut DiagBuf,
) {
    let Event::MainSite(site) = event else {
        return;
    };
    if site.public && site.source_root && site.role == Role::Lib && !site.entry {
        out.push(
            Code::UnselectedMain,
            hd_diag::Severity::Warning,
            site.span,
            "this public `main` is an ordinary function, since no executable names its module; name it in an `[[executable]]` table of `hd.toml` to run it",
            None,
        );
    }
}

#[cfg(test)]
fn collect_modules(sites: &[MainSite]) -> DiagBuf {
    let mut diags = DiagBuf::default();
    for site in sites {
        if !site.public && site.entry_module {
            diags.push(
                Code::PrivateMain,
                hd_diag::Severity::Warning,
                site.span,
                "main is not pub, so it is not the entry point",
                None,
            );
        }
        if site.public && site.source_root && site.role == Role::Lib && !site.entry {
            diags.push(
                Code::UnselectedMain,
                hd_diag::Severity::Warning,
                site.span,
                "this public `main` is an ordinary function, since no executable names its module; name it in an `[[executable]]` table of `hd.toml` to run it",
                None,
            );
        }
    }
    diags
}

#[cfg(test)]
mod tests {
    use hd_base::{FileId, Span};
    use hd_diag::Code;
    use hd_project::{MainSite, Role};

    use super::collect_modules;

    #[test]
    fn role_entry_and_source_root_select_the_finding() {
        let span = Span {
            file: FileId::from_raw(7),
            lo: 11,
            hi: 31,
        };
        for role in [Role::Lib, Role::Exe, Role::Test, Role::Task] {
            for source_root in [false, true] {
                for entry in [false, true] {
                    let findings = collect_modules(&[MainSite {
                        span,
                        public: true,
                        entry_module: false,
                        make_public_exact: false,
                        role,
                        entry,
                        source_root,
                    }]);
                    let expected = source_root && role == Role::Lib && !entry;
                    assert_eq!(findings.len(), usize::from(expected));
                    if expected {
                        assert_eq!(findings.code, [Code::UnselectedMain]);
                        assert_eq!(findings.primary, [span]);
                        assert_eq!(
                            findings.get_text(findings.message[0]),
                            "this public `main` is an ordinary function, since no executable names its module; name it in an `[[executable]]` table of `hd.toml` to run it",
                        );
                        assert_eq!(findings.fixes[0].len, 0);
                    }
                }
            }
        }
    }

    #[test]
    fn private_main_fires_only_in_an_entry_module() {
        let span = Span {
            file: FileId::from_raw(7),
            lo: 11,
            hi: 31,
        };
        for entry_module in [false, true] {
            let findings = collect_modules(&[MainSite {
                span,
                public: false,
                entry_module,
                make_public_exact: true,
                role: Role::Exe,
                entry: true,
                source_root: true,
            }]);
            assert_eq!(findings.len(), usize::from(entry_module));
            if entry_module {
                assert_eq!(findings.code, [Code::PrivateMain]);
                assert_eq!(findings.primary, [span]);
                assert_eq!(
                    findings.get_text(findings.message[0]),
                    "main is not pub, so it is not the entry point",
                );
            }
        }
    }
}
