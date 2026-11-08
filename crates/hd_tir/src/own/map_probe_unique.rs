//! A map probe can need an unshared table before lending its value.
//! State that operation's action without moving its check ahead of the
//! hash/probe or out of its hit/miss and short-loan conditions.

use super::OwnCx;
use crate::ir::{
    Body, Callee, Tag,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    b.tags
        .iter()
        .enumerate()
        .filter_map(|(i, &tag)| {
            if tag != Tag::Call
                || !Callee::from_words(b.record(b.data[i][0]))
                    .is_some_and(|callee| cx.is_map_probe(&callee))
            {
                return None;
            }
            let args = b.record(b.data[i][1]).len().saturating_sub(3);
            (args == 3).then_some((
                Tag::MakeUnique,
                u32_of(i),
                Info::new(At::Operation, Part::Operand(0), 5),
            ))
        })
        .collect()
}
