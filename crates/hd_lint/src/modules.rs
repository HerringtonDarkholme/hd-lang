//! Advice over selected source modules and their executable graph membership.

use hd_diag::{Code, DiagBuf};
use hd_project::{MainSite, Role};

use crate::{Policy, severity};

/// A public `main` in an unselected source library remains an ordinary function.
#[must_use]
pub fn analyze_modules(sites: &[MainSite], policy: Policy) -> DiagBuf {
    let mut diags = DiagBuf::default();
    let Some(severity) = severity(policy.unselected_main) else {
        return diags;
    };
    for site in sites {
        if site.source_root && site.role == Role::Lib && !site.entry {
            diags.push(
                Code::UnselectedMain,
                severity,
                site.span,
                "this public `main` is an ordinary function, since no executable names its module; name it in an `[[executable]]` table of `hd.toml` to run it",
                None,
            );
        }
    }
    diags
}
