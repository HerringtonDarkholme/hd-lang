//! Bounds-check elimination over owned TIR (step 6 of
//! `docs/future-work/BACKEND_NEUTRAL_TIR.md`, #2175). Each element step of
//! a place is checked until a proof here clears its check
//! ([`hd_tir::ir::place::ELEM_UNCHECKED`]); the backend writes the check
//! only where the bit is clear. Go's `prove.go` folds its explicit
//! `IsInBounds` values the same way.
//!
//! It runs after the ownership passes, whose hoisted `IsUnique` may
//! replace a buffer, so a proof sees the final reads and writes.
//!
//! Today's proof: an element of a private local list that a call already
//! received in the same straight-line run, at the same index
//! ([`ElementChecks::repeats_bounds`]). Cost: that table, one pass over
//! the body, and one record copy per proven access.

use hd_tir::ir::Body;
use hd_tir::own::ElementChecks;

/// Clears the bounds check of each element access `b` proves in range.
pub fn run(b: &mut Body) {
    let checks = ElementChecks::new(b);
    for i in 0..b.len() {
        if checks.repeats_bounds(i) {
            b.uncheck_elem(i, 0);
        }
    }
}
