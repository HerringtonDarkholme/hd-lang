//! Emission of TIR short-loan marks. Eligibility, holder propagation and
//! intervening-write checks live in [`hd_tir::own::short`]. The late pass
//! reads resolved call/type/storage facts through its context; this file
//! installs the marks; there are no representation temporaries to release.

use super::Em;

impl Em<'_, '_, '_> {
    /// Read the late TIR pass's marks; no loan eligibility is decided here.
    pub(super) fn prepare_short_loans(&mut self, marks: Vec<hd_tir::own::short::Mark>) {
        for mark in marks {
            for value in mark.values {
                self.loans.insert(value);
                self.short_holders.insert(value);
            }
            for local in mark.locals {
                self.loan_locals.entry(local).or_insert(mark.origin);
            }
            self.short_skips.extend(mark.skips);
            self.short_origins.insert(mark.origin);
        }
    }
}
