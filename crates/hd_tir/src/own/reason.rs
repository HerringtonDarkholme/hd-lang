//! Why each count operation stands (`docs/future-work/OWNERSHIP_ARCHITECTURE.md`,
//! "The One Place Each Count Is Decided"). A `Drop` carries its reason in
//! its data word b; an `IsUnique`'s and a copy `Read`'s follow from the
//! statement ([`reason`]). Nothing checks a reason yet: where today's
//! passes compute no fact behind a count, it is `Conservative`, naming the
//! site that placed it.

use crate::ir::{Body, ReadMode, Tag, ref_word, unique_word};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reason {
    /// A use consumes the value, which is lent or used again later.
    Stored,
    /// The value is read from a place a write may change while it lives.
    SecondOwner,
    /// The value is owned and its last use on this path lends, or it has
    /// none.
    LastUse,
    /// A store replaces an owned value in a place not known to be empty.
    Overwritten,
    /// A suspension point is cancelled while the value is live across it.
    Cancelled,
    /// The buffer is not fresh and may have been shared since its check.
    MayBeShared,
    /// A reference through a `*T` outlives its expression.
    PointerLoan,
    /// The value may be a representation view.
    View,
    /// The `&dyn` data count (#1354), a suspension's counted reference
    /// (#1355).
    ByDesign,
    /// No fact was computed: the site that placed the count.
    Conservative(&'static str),
}

const NAMED: [Reason; 9] = [
    Reason::Stored,
    Reason::SecondOwner,
    Reason::LastUse,
    Reason::Overwritten,
    Reason::Cancelled,
    Reason::MayBeShared,
    Reason::PointerLoan,
    Reason::View,
    Reason::ByDesign,
];

impl Reason {
    /// A `Drop`'s data word b: any other word is a conservative `Drop`.
    #[must_use]
    pub fn from_word(w: u32) -> Self {
        NAMED
            .get(w as usize)
            .copied()
            .unwrap_or(Reason::Conservative("drop"))
    }
}

/// The data word b of a `Drop` the passes place after a last use, or of
/// one that splits or sinks such a drop.
pub(crate) const LAST_USE: u32 = 2;
const _: () = assert!(matches!(NAMED[LAST_USE as usize], Reason::LastUse));

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Reason::Stored => "stored",
            Reason::SecondOwner => "second owner",
            Reason::LastUse => "last use",
            Reason::Overwritten => "overwritten",
            Reason::Cancelled => "cancelled",
            Reason::MayBeShared => "may be shared",
            Reason::PointerLoan => "pointer loan",
            Reason::View => "view",
            Reason::ByDesign => "by design",
            Reason::Conservative(site) => return write!(f, "conservative: {site}"),
        })
    }
}

/// The reason of count statement `i`: a `Drop`'s word; a `Drop` of a lent
/// `Ref` stays only for an instance whose view makes the referent fresh
/// (`ref_word::LENT`). Insertion puts an `IsUnique` before every change
/// and a copy before every read, proving neither; hoisting keeps a
/// covered `IsUnique` only for a view. `None` for any other instruction.
#[must_use]
pub fn reason(b: &Body, i: usize) -> Option<Reason> {
    let [a, w] = b.data[i];
    Some(match b.tags[i] {
        Tag::Drop => {
            let lent = b.view(a).value().is_some_and(|v| {
                b.tags[v as usize] == Tag::Ref && b.data[v as usize][1] & ref_word::LENT != 0
            });
            if lent {
                Reason::View
            } else {
                Reason::from_word(w)
            }
        }
        Tag::IsUnique if w == unique_word::COVERED => Reason::View,
        Tag::IsUnique => Reason::Conservative("unique"),
        Tag::Read if w == ReadMode::Copy as u32 => Reason::Conservative("copy read"),
        _ => return None,
    })
}
