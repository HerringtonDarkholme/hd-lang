//! The std items the compiler recognizes, resolved once per run.
//!
//! A `DefId` is a path interned in the run's `PathTable`, so a known item's
//! id is fixed by its path alone: it is looked up here exactly once, and
//! every recognition site compares ids instead of rendering a path and
//! matching text. Paths stay text only where text is the output: messages,
//! TIR text, wire encodings and host names.

use hd_base::{DefId, PathId};
use hd_intern::{PathKind, PathTable};

/// The std items and modules the compiler gives meaning to.
#[derive(Clone, Debug)]
pub struct KnownItems {
    // std.core
    pub any: DefId,
    pub any_val: DefId,
    pub any_ref: DefId,
    pub result: DefId,
    pub list: DefId,
    pub map: DefId,
    // std.function
    pub function_module: PathId,
    pub tuple: DefId,
    // std.structure
    pub structure: DefId,
    pub field: DefId,
    pub walker: DefId,
    pub describer: DefId,
    pub source: DefId,
    // std.inspect
    pub inspectable: DefId,
    // std.num
    pub num: DefId,
    pub integer: DefId,
    pub float: DefId,
    // std.ops
    pub default: DefId,
    pub range: DefId,
    pub range_from: DefId,
    pub range_to: DefId,
    pub template: DefId,
    pub index: DefId,
    pub index_set: DefId,
    pub neg: DefId,
    pub not: DefId,
    pub add: DefId,
    pub sub: DefId,
    pub mul: DefId,
    pub div: DefId,
    pub rem: DefId,
    pub bit_and: DefId,
    pub bit_or: DefId,
    pub bit_xor: DefId,
    pub shl: DefId,
    pub shr: DefId,
    // std.cmp, std.hash, std.format
    pub eq: DefId,
    pub partial_ord: DefId,
    pub ord: DefId,
    pub ordering: DefId,
    pub hash: DefId,
    /// `std.hash.hash_of`, which a map hashes a non-inline key by.
    pub hash_of: DefId,
    pub display: DefId,
    pub debug: DefId,
    // std.iter, std.convert, std.error
    pub iterator: DefId,
    pub iterable: DefId,
    pub from: DefId,
    pub error: DefId,
    // std.task
    pub suspend: DefId,
    pub block_on: DefId,
    // std.annotation, std.testing
    pub annotate: DefId,
    pub testing_module: PathId,
    pub check_equal: DefId,
    sealed: [DefId; 4],
}

impl KnownItems {
    /// The sealed traits the solver's compiler-supplied rows answer
    /// (trait-solver.md §3.9).
    #[must_use]
    pub fn sealed(&self) -> hd_types::solver::SealedTraits {
        hd_types::solver::SealedTraits {
            any: self.any,
            any_val: self.any_val,
            any_ref: self.any_ref,
            inspectable: self.inspectable,
            tuple: self.tuple,
            structure: self.structure,
        }
    }

    /// Every known item, by its path. The ids exist whether or not the run
    /// declares the item; comparing against an absent one never matches.
    #[must_use]
    pub fn new(paths: &PathTable) -> Self {
        let module = |m: &str| {
            let mut segs = m.split('.');
            let mut p = paths.intern(PathId::NONE, PathKind::Package, segs.next().unwrap_or(""));
            for s in segs {
                p = paths.intern(p, PathKind::Module, s);
            }
            p
        };
        let item =
            |m: &str, n: &str| DefId::from_raw(paths.intern(module(m), PathKind::Item, n).raw());
        let any_val = item("std.core", "AnyVal");
        let any_ref = item("std.core", "AnyRef");
        let structure = item("std.structure", "Structure");
        let tuple = item("std.function", "Tuple");
        Self {
            any: item("std.core", "Any"),
            any_val,
            any_ref,
            result: item("std.core", "Result"),
            list: item("std.core", "List"),
            map: item("std.core", "Map"),
            function_module: module("std.function"),
            tuple,
            structure,
            field: item("std.structure", "Field"),
            walker: item("std.structure", "Walker"),
            describer: item("std.structure", "Describer"),
            source: item("std.structure", "Source"),
            inspectable: item("std.inspect", "Inspectable"),
            num: item("std.num", "Num"),
            integer: item("std.num", "Integer"),
            float: item("std.num", "Float"),
            default: item("std.ops", "Default"),
            range: item("std.ops", "Range"),
            range_from: item("std.ops", "RangeFrom"),
            range_to: item("std.ops", "RangeTo"),
            template: item("std.ops", "Template"),
            index: item("std.ops", "Index"),
            index_set: item("std.ops", "IndexSet"),
            neg: item("std.ops", "Neg"),
            not: item("std.ops", "Not"),
            add: item("std.ops", "Add"),
            sub: item("std.ops", "Sub"),
            mul: item("std.ops", "Mul"),
            div: item("std.ops", "Div"),
            rem: item("std.ops", "Rem"),
            bit_and: item("std.ops", "BitAnd"),
            bit_or: item("std.ops", "BitOr"),
            bit_xor: item("std.ops", "BitXor"),
            shl: item("std.ops", "Shl"),
            shr: item("std.ops", "Shr"),
            eq: item("std.cmp", "Eq"),
            partial_ord: item("std.cmp", "PartialOrd"),
            ord: item("std.cmp", "Ord"),
            ordering: item("std.cmp", "Ordering"),
            hash: item("std.hash", "Hash"),
            hash_of: item("std.hash", "hash_of"),
            display: item("std.format", "Display"),
            debug: item("std.format", "Debug"),
            iterator: item("std.iter", "Iterator"),
            iterable: item("std.iter", "Iterable"),
            from: item("std.convert", "From"),
            error: item("std.error", "Error"),
            suspend: item("std.task", "Suspend"),
            block_on: item("std.task", "block_on"),
            annotate: item("std.annotation", "annotate"),
            testing_module: module("std.testing"),
            check_equal: item("std.testing", "check_equal"),
            sealed: [any_val, any_ref, structure, tuple],
        }
    }

    /// The compiler-implemented sealed traits (types.sealed.no-impl).
    #[must_use]
    pub fn is_sealed(&self, d: DefId) -> bool {
        self.sealed.contains(&d)
    }
}
