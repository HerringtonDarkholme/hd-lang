//! The std items the compiler recognizes, resolved once per run.
//!
//! A `DefId` is a path interned in the run's `PathTable`, so a known item's
//! id is fixed by its path alone: it is looked up here exactly once, and
//! every recognition site compares ids instead of rendering a path and
//! matching text. Paths stay text only where text is the output: messages,
//! TIR text, wire encodings and host names.

use hd_base::{DefId, PathId};
use hd_intern::{PathKind, PathTable};

/// Shared module spellings for source seeding, runtime roots and textual import hints.
pub const CORE_MODULE: &str = "std.core";
/// `Marker`, `Send` and `Wire` (`module.marker.std`).
pub const MARKER_MODULE: &str = "std.marker";
pub const TASK_MODULE: &str = "std.task";
pub const FUNCTION_MODULE: &str = "std.function";
pub const INSPECT_MODULE: &str = "std.inspect";
pub const STRUCTURE_MODULE: &str = "std.structure";
pub const TESTING_MODULE: &str = "std.testing";
pub const ENTRY_MODULE: &str = "std.rt.entry";
pub const CHECKED_MODULE: &str = "std.rt.checked";
pub const RUNTIME_MODULE: &str = "std.rt";
pub const FORMAT_MODULE: &str = "std.format";
pub const BYTES_MODULE: &str = "std.rt.bytes";

/// The modules with compiler-supplied declarations, in seed order.
pub const SEEDED_MODULES: &[&str] = &[
    CORE_MODULE,
    TASK_MODULE,
    FUNCTION_MODULE,
    INSPECT_MODULE,
    STRUCTURE_MODULE,
    TESTING_MODULE,
];

/// The std items and modules the compiler gives meaning to.
#[derive(Clone, Debug)]
pub struct KnownItems {
    // std.core
    pub any: DefId,
    pub any_val: DefId,
    pub any_ref: DefId,
    /// The prelude enum `Option[T]`, which `T?` names (`types.option.sugar`).
    pub option: DefId,
    pub result: DefId,
    pub list: DefId,
    /// `List.is_empty`, whose result is known for a generated empty facts list.
    pub list_is_empty: DefId,
    pub map: DefId,
    // std.function
    pub function_module: PathId,
    /// The `std.core` module, which owns every built-in type's methods.
    pub core_module: PathId,
    /// The `Fn[I, O, $R]` trait. In a type position the same declaration
    /// remains the compiler's `Fn[I, O, $R]` function type constructor.
    pub fn_trait: DefId,
    /// `Fn.call`, which the checker lowers to a function-value call.
    pub fn_call: DefId,
    /// The `FnMut[I, O, $R]` trait, `Fn`'s supertrait.
    pub fn_mut_trait: DefId,
    /// `FnMut.call_mut`, lowered as `Fn.call` is.
    pub fn_mut_call: DefId,
    pub tuple: DefId,
    // std.structure
    pub structure: DefId,
    pub union_type: DefId,
    /// The `'static` bound (`ref.generic.static`), a sealed trait no
    /// path names.
    pub static_bound: DefId,
    /// The owner of every type alias's own parameters
    /// (`types.alias.same`): an alias body names its parameter `k` as
    /// `ParamRef { owner: alias_params, index: k }`, by position, as
    /// rustc's `ParamTy` does. So `type B[S] = (A[S], A[S])` holds `A`'s
    /// body itself, an identity instance, and a chain of aliases interns
    /// one node per alias, not a copy of each body below it (#1984). No
    /// path names it.
    pub alias_params: DefId,
    /// `Facts`, whose `items` list may be inspected only after the value is
    /// proved to be declaration facts.
    pub facts: DefId,
    /// `FieldRecord`, which a `Field` handle refers to.
    pub field: DefId,
    /// `FieldRecord.get`, whose generated handle call a closed Structure walk
    /// may replace with a direct projection.
    pub field_get: DefId,
    /// `VariantRecord.holds`, whose generated handle call a closed data walk
    /// may replace with the known result.
    pub variant_holds: DefId,
    pub walker: DefId,
    /// The data-only `Walker` hooks, whose defaults bridge to the ordinary
    /// variant/member protocol.
    pub walker_data_variant: DefId,
    pub walker_data_member: DefId,
    pub describer: DefId,
    pub source: DefId,
    /// `fills_defaults`, whose fact on a source implementation fills
    /// declared member defaults (`annot.impl-fact.fills-defaults.decl`).
    pub fills_defaults: DefId,
    /// `skips`, whose fact on a walker, describer or source
    /// implementation omits the members that carry a fact type
    /// (`annot.impl-fact.skips.decl`).
    pub skips: DefId,
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
    /// The std cursors a `for` over a `Range` or `RangeFrom` of integers
    /// iterates with (`flow.for.range.cursor`).
    pub range_iter: DefId,
    pub range_from_iter: DefId,
    pub template: DefId,
    pub num_suffix: DefId,
    pub str_prefix: DefId,
    pub index: DefId,
    pub index_mut: DefId,
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
    /// (spec/std/hash.md#r-std-hash.map.foldhash-fast): `std.hash`'s seeded
    /// `hash_key` for a key that is not inline, `hash_string` for a string
    /// and `hash_integer` for any other inline key.
    pub map_key_hash: DefId,
    /// `std.hash.key_equal`, the by-value `Eq` comparison of two map keys
    /// that are not inline.
    pub key_equal: DefId,
    pub map_string_hash: DefId,
    pub map_integer_hash: DefId,
    pub display: DefId,
    pub debug: DefId,
    /// `std.debug`'s private decoders of a call-site record
    /// (spec/lang/07-functions.md "Call-Site Locations"): the checker lowers
    /// `Location::caller()` and `Location::caller_args()` to calls of them.
    pub site_location: DefId,
    pub site_args: DefId,
    /// `std.debug.site_function_text`: `dbg`'s `<fn name(..)>` from a
    /// call-site record (`module.dbg.value.function`).
    pub site_function_text: DefId,
    /// `std.format.dbg`, each reference to which the checker notes for the
    /// release check (spec/lang/10-modules.md "Release Builds And Dependencies").
    pub dbg: DefId,
    /// `std.ops.default`, the decorator that marks an enum's default
    /// variant for a derived `Default`.
    pub default_marker: DefId,
    /// `std.format.debug`, the prelude function whose bare call of a
    /// statement value the checker leaves unwarned.
    pub debug_fn: DefId,
    /// `std.format.DbgStructural`, the structural `dbg` rendering trait.
    pub dbg_structural: DefId,
    /// `std.format`'s REPL support functions, which render and record a
    /// REPL line's value.
    pub repl_text: DefId,
    pub repl_value: DefId,
    /// `std.text.StringBuilder`, which a template literal builds into.
    pub string_builder: DefId,
    // std.serde
    pub serialize: DefId,
    pub deserialize: DefId,
    /// The serde facts the checker reads from declarations: `SkipNone`,
    /// the `EnumTagging` type with its `tag` and `untagged` constructors,
    /// and the `Rename`, `RenameAll` and `RenameCase` types with the
    /// `rename` and `rename_all` constructors.
    pub skip_none: DefId,
    pub enum_tagging: DefId,
    pub tag: DefId,
    pub untagged: DefId,
    pub rename_type: DefId,
    pub rename: DefId,
    pub rename_all_type: DefId,
    pub rename_all: DefId,
    pub rename_case: DefId,
    /// `std.process.Termination`, the trait a `main` result implements.
    pub termination: DefId,
    /// `std.rt.entry.host_provider`, which shares a host provider object.
    pub host_provider: DefId,
    /// The duration literal functions `ms`, `s`, `min` and `h` of
    /// `std.time`, in that order.
    pub duration_suffix: [DefId; 4],
    // std.iter, std.convert, std.error
    pub iterator: DefId,
    pub iterable: DefId,
    pub from: DefId,
    pub error: DefId,
    // std.ops: cleanup protocols and the auto-registration marker
    pub disposable: DefId,
    pub async_disposable: DefId,
    pub auto_closing: DefId,
    /// The `Copy` marker (`ref.copy.marker`), a sealed trait the compiler
    /// answers from a type's structure.
    pub copy: DefId,
    /// `Marker`, `Send`, `Wire` (`module.marker.std`) and `Remote`
    /// (`module.remote.std`).
    pub marker: DefId,
    pub send: DefId,
    pub wire: DefId,
    pub remote: DefId,
    // std.task
    pub suspend: DefId,
    /// `std.rt.memory.Buffer`, raw memory whose room holds values of its
    /// type argument (`module.rt.buffer.type`).
    pub buffer: DefId,
    /// `std.rt.memory.Ptr`, an address that may point into what a
    /// reference reaches (`module.rt.ptr.as-ref`).
    pub ptr: DefId,
    /// `std.rt.Drop`, whose implementation a value's release glue calls
    /// before releasing its fields (`module.rt.drop.glue`).
    pub drop_trait: DefId,
    /// `std.rt.Drop.drop`.
    pub drop_method: DefId,
    /// `std.rt.memory.room` and `std.rt.alloc.alloc`, which a checked
    /// build's room allocation swaps for `std.rt.checked.allocate`.
    pub room: DefId,
    pub alloc: DefId,
    /// `std.rt.alloc.retain_value` and `release_value`: a count shim is
    /// one of these at its type (`hd_layout::shims`).
    pub retain_value: DefId,
    pub release_value: DefId,
    pub checked_allocate: DefId,
    /// `std.rt.memory.free_room` and `std.rt.cycle.collect`, whose
    /// `std.rt.alloc.free_block` a checked build swaps for
    /// `std.rt.checked.free_acyclic`.
    pub free_room: DefId,
    pub cycle_collect: DefId,
    pub free_block: DefId,
    pub checked_free_acyclic: DefId,
    /// The `std.host` functions a host boundary calls
    /// (`hd_mono::HostItems`).
    pub host_encode: DefId,
    pub host_decode: DefId,
    pub package_encode: DefId,
    pub package_decode: DefId,
    pub block_on: DefId,
    /// `std.task.race`, whose call needs a task (`req.combinator.race-empty`).
    pub race: DefId,
    /// `std.task.all_list`, the engine of `join!`'s list form
    /// (`req.combinator.join-list`).
    pub all_list: DefId,
    // std.annotation, std.testing
    pub annotate: DefId,
    /// `inline` and `cold`, the decorators that choose how calls of a
    /// function are compiled (`annot.inline.recognized`).
    pub inline: DefId,
    pub cold: DefId,
    pub testing_module: PathId,
    /// The test registration functions besides `it`
    /// (`module.testing.reg.calls-ref`), and the private std bodies that their
    /// test cases run, which take the generics of the registration
    /// function in order.
    /// `hd_run!`, legal only in an integration test module
    /// (`std-testing.hd-run.integration-only`).
    pub hd_run: DefId,
    /// `case_timeout`, which a test case's timeout argument lowers to.
    pub case_timeout: DefId,
    pub it_each: DefId,
    pub it_prop: DefId,
    pub it_prop_with: DefId,
    pub each_case: DefId,
    pub prop_case: DefId,
    pub prop_with_case: DefId,
    /// The runner capabilities that a test case's std body uses
    /// (`std-testing.runner.binding`).
    pub test_runner: DefId,
    pub property_runner: DefId,
    /// The sealed traits, in the order of the table in spec/lang/09-traits.md
    /// "Sealed Traits" (`trait.sealed.list`).
    sealed: [DefId; 19],
}

impl KnownItems {
    /// A struct declaration.
    #[must_use]
    pub fn is_struct(&self, item: &crate::Item) -> bool {
        matches!(item.data, crate::ItemData::Struct(_, _))
    }

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
            fn_trait: self.fn_trait,
            fn_mut_trait: self.fn_mut_trait,
            structure: self.structure,
            union_type: self.union_type,
            static_bound: self.static_bound,
            copy: self.copy,
            list: self.list,
            map: self.map,
            suspend: self.suspend,
            marker: self.marker,
            send: self.send,
            wire: self.wire,
            remote: self.remote,
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
        let inherent_method = |m: &str, imp: &str, n: &str| {
            let owner = paths.intern(module(m), PathKind::Impl, imp);
            DefId::from_raw(paths.intern(owner, PathKind::Member, n).raw())
        };
        let trait_method = |owner: DefId, n: &str| {
            DefId::from_raw(
                paths
                    .intern(PathId::from_raw(owner.raw()), PathKind::Member, n)
                    .raw(),
            )
        };
        let any = item(CORE_MODULE, "Any");
        let any_val = item(CORE_MODULE, "AnyVal");
        let any_ref = item(CORE_MODULE, "AnyRef");
        let structure = item(STRUCTURE_MODULE, "Structure");
        let union_type = item(STRUCTURE_MODULE, "UnionType");
        let static_bound = item(CORE_MODULE, "'static");
        let walker = item(STRUCTURE_MODULE, "Walker");
        let tuple = item(FUNCTION_MODULE, "Tuple");
        let fn_trait = item(FUNCTION_MODULE, "Fn");
        let fn_mut_trait = item(FUNCTION_MODULE, "FnMut");
        let suspend = item(TASK_MODULE, "Suspend");
        let inspectable = item(INSPECT_MODULE, "Inspectable");
        let num = item("std.num", "Num");
        let integer = item("std.num", "Integer");
        let float = item("std.num", "Float");
        let copy = item(CORE_MODULE, "Copy");
        let marker = item(MARKER_MODULE, "Marker");
        let send = item(MARKER_MODULE, "Send");
        let wire = item(MARKER_MODULE, "Wire");
        let remote = item("std.remote", "Remote");
        let drop_trait = item(RUNTIME_MODULE, "Drop");
        Self {
            any,
            any_val,
            any_ref,
            option: item(CORE_MODULE, "Option"),
            result: item(CORE_MODULE, "Result"),
            list: item(CORE_MODULE, "List"),
            list_is_empty: inherent_method("std.collections", "impl[T] List[T]", "is_empty"),
            map: item(CORE_MODULE, "Map"),
            buffer: item("std.rt.memory", "Buffer"),
            ptr: item("std.rt.memory", "Ptr"),
            drop_trait,
            drop_method: trait_method(drop_trait, "drop"),
            room: item("std.rt.memory", "room"),
            alloc: item("std.rt.alloc", "alloc"),
            retain_value: item("std.rt.alloc", "retain_value"),
            release_value: item("std.rt.alloc", "release_value"),
            checked_allocate: item(CHECKED_MODULE, "allocate"),
            free_room: item("std.rt.memory", "free_room"),
            cycle_collect: item("std.rt.cycle", "collect"),
            free_block: item("std.rt.alloc", "free_block"),
            checked_free_acyclic: item(CHECKED_MODULE, "free_acyclic"),
            host_encode: item("std.host", "host_encode"),
            host_decode: item("std.host", "host_decode"),
            package_encode: item("std.host", "package_encode"),
            package_decode: item("std.host", "package_decode"),
            function_module: module(FUNCTION_MODULE),
            core_module: module(CORE_MODULE),
            fn_trait,
            fn_call: trait_method(fn_trait, "call"),
            fn_mut_trait,
            fn_mut_call: trait_method(fn_mut_trait, "call_mut"),
            tuple,
            structure,
            union_type,
            static_bound,
            alias_params: item(CORE_MODULE, "'alias"),
            facts: item(STRUCTURE_MODULE, "Facts"),
            field: item(STRUCTURE_MODULE, "FieldRecord"),
            field_get: inherent_method(
                STRUCTURE_MODULE,
                "impl[HdS < ?Copy, HdF < ?Copy] FieldRecord[HdS, HdF]",
                "get",
            ),
            variant_holds: inherent_method(
                STRUCTURE_MODULE,
                "impl[HdS < ?Copy] VariantRecord[HdS]",
                "holds",
            ),
            walker,
            walker_data_variant: trait_method(walker, "data_variant"),
            walker_data_member: trait_method(walker, "data_member"),
            describer: item(STRUCTURE_MODULE, "Describer"),
            source: item(STRUCTURE_MODULE, "Source"),
            fills_defaults: item(STRUCTURE_MODULE, "fills_defaults"),
            skips: item(STRUCTURE_MODULE, "skips"),
            inspectable,
            num,
            integer,
            float,
            default: item("std.ops", "Default"),
            range: item("std.ops", "Range"),
            range_from: item("std.ops", "RangeFrom"),
            range_to: item("std.ops", "RangeTo"),
            range_full: item("std.ops", "RangeFull"),
            range_iter: item("std.ops", "RangeIter"),
            range_from_iter: item("std.ops", "RangeFromIter"),
            template: item("std.ops", "Template"),
            num_suffix: item("std.ops", "num_suffix"),
            str_prefix: item("std.ops", "str_prefix"),
            index: item("std.ops", "Index"),
            index_mut: item("std.ops", "IndexMut"),
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
            async_disposable: item("std.ops", "AsyncDisposable"),
            auto_closing: item("std.ops", "AutoClosing"),
            copy,
            eq: item(CORE_MODULE, "Eq"),
            partial_ord: item("std.cmp", "PartialOrd"),
            ord: item("std.cmp", "Ord"),
            ordering: item("std.cmp", "Ordering"),
            hash: item(CORE_MODULE, "Hash"),
            map_key_hash: item("std.hash", "hash_key"),
            key_equal: item("std.hash", "key_equal"),
            map_string_hash: item("std.hash", "hash_string"),
            map_integer_hash: item("std.hash", "hash_integer"),
            display: item(FORMAT_MODULE, "Display"),
            debug: item(FORMAT_MODULE, "Debug"),
            site_location: item("std.debug", "site_location"),
            site_args: item("std.debug", "site_args"),
            site_function_text: item("std.debug", "site_function_text"),
            dbg: item(FORMAT_MODULE, "dbg"),
            default_marker: item("std.ops", "default"),
            debug_fn: item(FORMAT_MODULE, "debug"),
            dbg_structural: item(FORMAT_MODULE, "DbgStructural"),
            repl_text: item(FORMAT_MODULE, "repl_text"),
            repl_value: item(FORMAT_MODULE, "repl_value"),
            string_builder: item("std.text", "StringBuilder"),
            serialize: item("std.serde", "Serialize"),
            deserialize: item("std.serde", "Deserialize"),
            skip_none: item("std.serde", "SkipNone"),
            enum_tagging: item("std.serde", "EnumTagging"),
            tag: item("std.serde", "tag"),
            untagged: item("std.serde", "untagged"),
            rename_type: item("std.serde", "Rename"),
            rename: item("std.serde", "rename"),
            rename_all_type: item("std.serde", "RenameAll"),
            rename_all: item("std.serde", "rename_all"),
            rename_case: item("std.serde", "RenameCase"),
            termination: item("std.process", "Termination"),
            host_provider: item(ENTRY_MODULE, "host_provider"),
            duration_suffix: [
                item("std.time", "ms"),
                item("std.time", "s"),
                item("std.time", "min"),
                item("std.time", "h"),
            ],
            iterator: item("std.iter", "Iterator"),
            iterable: item("std.iter", "Iterable"),
            from: item("std.convert", "From"),
            error: item("std.error", "Error"),
            suspend,
            block_on: item(TASK_MODULE, "block_on"),
            race: item(TASK_MODULE, "race"),
            all_list: item(TASK_MODULE, "all_list"),
            annotate: item("std.annotation", "annotate"),
            inline: item("std.annotation", "inline"),
            cold: item("std.annotation", "cold"),
            testing_module: module(TESTING_MODULE),
            hd_run: item(TESTING_MODULE, "hd_run"),
            case_timeout: item(TESTING_MODULE, "case_timeout"),
            it_each: item(TESTING_MODULE, "it_each"),
            it_prop: item(TESTING_MODULE, "it_prop"),
            it_prop_with: item(TESTING_MODULE, "it_prop_with"),
            each_case: item(TESTING_MODULE, "each_case"),
            prop_case: item(TESTING_MODULE, "prop_case"),
            prop_with_case: item(TESTING_MODULE, "prop_with_case"),
            test_runner: item(TESTING_MODULE, "TestRunner"),
            property_runner: item(TESTING_MODULE, "PropertyRunner"),
            sealed: [
                any,
                any_val,
                any_ref,
                suspend,
                inspectable,
                structure,
                union_type,
                num,
                integer,
                float,
                tuple,
                fn_trait,
                fn_mut_trait,
                static_bound,
                copy,
                marker,
                send,
                wire,
                remote,
            ],
            marker,
            send,
            wire,
            remote,
        }
    }

    /// `Fn` or `FnMut` when `d` is that function trait.
    #[must_use]
    pub fn fn_trait_name(&self, d: DefId) -> Option<&'static str> {
        if d == DefId::NONE {
            None
        } else if d == self.fn_trait {
            Some("Fn")
        } else if d == self.fn_mut_trait {
            Some("FnMut")
        } else {
            None
        }
    }

    /// The sealed traits (`trait.sealed.list`), which only the compiler
    /// and the standard library implement (`types.sealed.no-impl`).
    #[must_use]
    pub fn is_sealed(&self, d: DefId) -> bool {
        self.sealed.contains(&d)
    }
}

/// The prelude (spec/lang/10-modules.md#prelude) as fixed uses: origin
/// module and names. `lib/std/prelude.hd` re-exports exactly these; a test
/// checks the two agree. The `std.testing` row is test code only.
pub const PRELUDE: &[(&str, &[&str])] = &[
    (
        CORE_MODULE,
        &[
            "never", "bool", "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "usize", "f32",
            "f64", "char", "string", "void", "List", "Map", "Any", "AnyVal", "AnyRef", "Option",
            "Result", "panic", "box",
        ],
    ),
    (FORMAT_MODULE, &["Display", "Debug", "debug", "dbg"]),
    ("std.cmp", &["Eq", "PartialOrd", "Ord", "Ordering"]),
    ("std.hash", &["Hash"]),
    ("std.iter", &["Iterator", "Iterable"]),
    ("std.console", &["Console", "ConsoleError", "println"]),
    (TASK_MODULE, &["Suspend"]),
    ("std.ops", &["Copy"]),
];

/// The prelude names of test code (`module.prelude.test-assertions`): bound only
/// in test code, but no declaration of a module may take their place.
pub(crate) const TEST_PRELUDE: &[(&str, &[&str])] =
    &[(TESTING_MODULE, &["it", "assert", "assert_equal"])];

/// Whether `module` is a runtime module, `std.rt` or one under it
/// (`module.rt.std-only`).
#[must_use]
pub fn runtime_module(module: &str) -> bool {
    module
        .strip_prefix(RUNTIME_MODULE)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// The prelude rows `module` sees: `std.core` and a runtime module, std's
/// leaf layer, see only `std.core`'s names (`module.core.prelude`,
/// `module.rt.core-prelude`); every other module sees them all.
#[must_use]
pub fn prelude_rows(module: &str) -> &'static [(&'static str, &'static [&'static str])] {
    if module == CORE_MODULE || runtime_module(module) {
        &PRELUDE[..1]
    } else {
        PRELUDE
    }
}
