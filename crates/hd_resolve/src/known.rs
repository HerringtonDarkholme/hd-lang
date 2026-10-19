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
    /// The data type a reference type lowers to for the backend
    /// (`hd_resolve::seed`, `crates/hd_wasm/DESIGN.md` §15.2 "References").
    pub ref_cell: DefId,
    /// The fieldless data type whose one value is the identity base of a
    /// reference to a local, a new object or a top-level binding.
    pub ref_root: DefId,
    /// The intrinsic that numbers locals and new objects for reference
    /// identity (`crates/hd_wasm/DESIGN.md` §15.2 "References").
    pub ref_serial: DefId,
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
    pub range_full: DefId,
    pub template: DefId,
    pub num_suffix: DefId,
    pub str_prefix: DefId,
    pub index: DefId,
    pub index_set: DefId,
    pub apply: DefId,
    pub update: DefId,
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
    /// The keyed hashes a map buckets its keys by
    /// (spec/std/hash.md#r-std-hash.map.bucket-hash): `std.hash`'s
    /// `key_bucket_hash` for a key that is not inline,
    /// `string_bucket_hash` for a string and `int_bucket_hash` for any
    /// other inline key.
    pub key_bucket_hash: DefId,
    pub string_bucket_hash: DefId,
    pub int_bucket_hash: DefId,
    pub display: DefId,
    pub debug: DefId,
    /// `std.format.dbg`, whose call the checker writes out (spec/lang/10-modules.md "Debug Printing").
    pub dbg: DefId,
    // std.json, std.serde
    pub json_encode: DefId,
    pub json_decode: DefId,
    pub serialize: DefId,
    pub deserialize: DefId,
    // std.iter, std.convert, std.error
    pub iterator: DefId,
    pub iterable: DefId,
    pub from: DefId,
    pub error: DefId,
    // std.ops: the trait a `using` binding disposes through
    pub disposable: DefId,
    // std.task
    pub suspend: DefId,
    pub block_on: DefId,
    /// `std.task.race`, whose call needs a task (`req.combinator.race-empty`).
    pub race: DefId,
    pub println: DefId,
    // std.annotation, std.testing
    pub annotate: DefId,
    pub testing_module: PathId,
    pub check_equal: DefId,
    /// The test registration functions besides `it`
    /// (`module.testing.reg.calls-ref`), and the private std bodies that their
    /// test cases run, which take the generics of the registration
    /// function in order.
    pub it_each: DefId,
    pub it_prop: DefId,
    pub it_prop_with: DefId,
    pub each_case: DefId,
    pub prop_case: DefId,
    pub prop_with_case: DefId,
    /// `std.testing.snapshot`, whose `expect` is a literal
    /// (`module.testing.snapshot.literal`).
    pub snapshot: DefId,
    /// The runner capabilities that a test case's std body uses
    /// (`std-testing.runner.binding`).
    pub test_runner: DefId,
    pub property_runner: DefId,
    /// The sealed traits, in the order of the table in spec/lang/09-traits.md
    /// "Sealed Traits" (`trait.sealed.list`).
    sealed: [DefId; 10],
}

impl KnownItems {
    /// The sealed traits the solver's compiler-supplied rows answer
    /// (`crates/hd_check/DESIGN.md` (trait solver) §3.9).
    #[must_use]
    pub fn sealed(&self) -> hd_types::solver::SealedTraits {
        hd_types::solver::SealedTraits {
            any: self.any,
            any_val: self.any_val,
            any_ref: self.any_ref,
            inspectable: self.inspectable,
            tuple: self.tuple,
            structure: self.structure,
            list: self.list,
            map: self.map,
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
        let any = item("std.core", "Any");
        let any_val = item("std.core", "AnyVal");
        let any_ref = item("std.core", "AnyRef");
        let structure = item("std.structure", "Structure");
        let tuple = item("std.function", "Tuple");
        let suspend = item("std.task", "Suspend");
        let inspectable = item("std.inspect", "Inspectable");
        let num = item("std.num", "Num");
        let integer = item("std.num", "Integer");
        let float = item("std.num", "Float");
        Self {
            any,
            any_val,
            any_ref,
            result: item("std.core", "Result"),
            list: item("std.core", "List"),
            map: item("std.core", "Map"),
            ref_cell: item("std.core", "Ref"),
            ref_root: item("std.core", "RefRoot"),
            ref_serial: item("std.core", "ref_serial"),
            function_module: module("std.function"),
            tuple,
            structure,
            field: item("std.structure", "Field"),
            walker: item("std.structure", "Walker"),
            describer: item("std.structure", "Describer"),
            source: item("std.structure", "Source"),
            inspectable,
            num,
            integer,
            float,
            default: item("std.ops", "Default"),
            range: item("std.ops", "Range"),
            range_from: item("std.ops", "RangeFrom"),
            range_to: item("std.ops", "RangeTo"),
            range_full: item("std.ops", "RangeFull"),
            template: item("std.ops", "Template"),
            num_suffix: item("std.ops", "num_suffix"),
            str_prefix: item("std.ops", "str_prefix"),
            index: item("std.ops", "Index"),
            index_set: item("std.ops", "IndexSet"),
            apply: item("std.ops", "Apply"),
            update: item("std.ops", "Update"),
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
            disposable: item("std.ops", "Disposable"),
            eq: item("std.cmp", "Eq"),
            partial_ord: item("std.cmp", "PartialOrd"),
            ord: item("std.cmp", "Ord"),
            ordering: item("std.cmp", "Ordering"),
            hash: item("std.hash", "Hash"),
            key_bucket_hash: item("std.hash", "key_bucket_hash"),
            string_bucket_hash: item("std.hash", "string_bucket_hash"),
            int_bucket_hash: item("std.hash", "int_bucket_hash"),
            display: item("std.format", "Display"),
            debug: item("std.format", "Debug"),
            dbg: item("std.format", "dbg"),
            json_encode: item("std.json", "encode"),
            json_decode: item("std.json", "decode"),
            serialize: item("std.serde", "Serialize"),
            deserialize: item("std.serde", "Deserialize"),
            iterator: item("std.iter", "Iterator"),
            iterable: item("std.iter", "Iterable"),
            from: item("std.convert", "From"),
            error: item("std.error", "Error"),
            suspend,
            block_on: item("std.task", "block_on"),
            race: item("std.task", "race"),
            println: item("std.console", "println"),
            annotate: item("std.annotation", "annotate"),
            testing_module: module("std.testing"),
            check_equal: item("std.testing", "check_equal"),
            it_each: item("std.testing", "it_each"),
            it_prop: item("std.testing", "it_prop"),
            it_prop_with: item("std.testing", "it_prop_with"),
            each_case: item("std.testing", "each_case"),
            prop_case: item("std.testing", "prop_case"),
            prop_with_case: item("std.testing", "prop_with_case"),
            snapshot: item("std.testing", "snapshot"),
            test_runner: item("std.testing", "TestRunner"),
            property_runner: item("std.testing", "PropertyRunner"),
            sealed: [
                any,
                any_val,
                any_ref,
                suspend,
                inspectable,
                structure,
                num,
                integer,
                float,
                tuple,
            ],
        }
    }

    /// The sealed traits (`trait.sealed.list`), which only the compiler
    /// and the standard library implement (`types.sealed.no-impl`).
    #[must_use]
    pub fn is_sealed(&self, d: DefId) -> bool {
        self.sealed.contains(&d)
    }
}
