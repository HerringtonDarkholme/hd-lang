//! Advice over selected source modules and their executable graph membership.

use hd_diag::{Code, DiagBuf};
use hd_driver::analysis::Event;
#[cfg(test)]
use hd_project::MainSite;
use hd_project::Role;

pub(crate) fn unselected_main(
    event: &Event<'_>,
    _: &hd_check::analysis::CheckedView<'_>,
    out: &mut DiagBuf,
) {
    let Event::MainSite(site) = event else {
        return;
    };
    if site.source_root && site.role == Role::Lib && !site.entry {
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
        if site.source_root && site.role == Role::Lib && !site.entry {
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
}
