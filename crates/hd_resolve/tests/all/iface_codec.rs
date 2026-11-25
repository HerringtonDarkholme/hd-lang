//! Interface cache codecs: auxiliary sections, damaged blobs and lazy reads.

use std::sync::Arc;

use hd_intern::{PathTable, ShardedInterner};
use hd_resolve::KnownItems;
use hd_resolve::iface::{
    CallAttrs, FnAttrs, FnSig, Item, ItemData, Items, Names, TypeAttrs, decode_items, decode_lazy,
    decode_lazy_in, decode_module_uses, decode_private_names, encode_docs, encode_items,
    encode_module_uses, encode_private_names,
};
use hd_types::{InternPool, Prim, TyData};

struct Run {
    pool: InternPool,
    paths: PathTable,
    syms: ShardedInterner,
    known: KnownItems,
}

impl Run {
    fn new() -> Self {
        let paths = PathTable::new();
        let known = KnownItems::new(&paths);
        Self {
            pool: InternPool::new(),
            paths,
            syms: ShardedInterner::default(),
            known,
        }
    }

    fn names(&self) -> Names<'_> {
        Names {
            pool: &self.pool,
            paths: &self.paths,
            syms: &self.syms,
            known: &self.known,
        }
    }

    fn items(&self) -> Vec<Item> {
        let names = self.names();
        let integer = self.pool.intern_ty(&TyData::Prim(Prim::I32));
        vec![
            Item::new(
                names.item("app.data", "Record"),
                self.syms.intern("Record"),
                true,
                ItemData::Struct(Vec::new(), TypeAttrs::default()),
            ),
            Item::new(
                names.item("app.data", "make"),
                self.syms.intern("make"),
                true,
                ItemData::Fn(
                    FnSig::simple(Vec::new(), Vec::new(), integer),
                    FnAttrs {
                        call: CallAttrs {
                            configured: true,
                            ..CallAttrs::default()
                        },
                        ..FnAttrs::default()
                    },
                ),
            ),
        ]
    }
}

#[test]
fn module_alias_section_preserves_order_and_unicode_and_rejects_truncation() {
    let rows = vec![
        ("app.one".into(), "renamed".into(), "dep.math".into()),
        ("app.two".into(), "測定".into(), "std.time".into()),
    ];
    let bytes = encode_module_uses(&rows);
    assert_eq!(decode_module_uses(&bytes), Some(rows));
    for end in 0..bytes.len() {
        assert!(decode_module_uses(&bytes[..end]).is_none(), "prefix {end}");
    }
    assert_eq!(
        decode_module_uses(&encode_module_uses(&[])),
        Some(Vec::new())
    );
}

#[test]
fn private_name_section_preserves_distinct_modules_and_rejects_truncation() {
    let run = Run::new();
    let names = run.names();
    let rows = vec![
        ("app.one".into(), run.syms.intern("hidden")),
        ("app.two".into(), run.syms.intern("hidden")),
    ];
    let bytes = encode_private_names(&names, &rows);
    assert_eq!(decode_private_names(&names, &bytes), Some(rows));
    for end in 0..bytes.len() {
        assert!(
            decode_private_names(&names, &bytes[..end]).is_none(),
            "prefix {end}"
        );
    }
    assert_eq!(
        decode_private_names(&names, &encode_private_names(&names, &[])),
        Some(Vec::new())
    );
}

#[test]
fn auxiliary_sections_reject_invalid_utf8() -> Result<(), &'static str> {
    let run = Run::new();
    let names = run.names();
    let mut bytes = encode_module_uses(&[("app.unique".into(), "alias".into(), "std.time".into())]);
    let at = bytes
        .windows(10)
        .position(|s| s == b"app.unique")
        .ok_or("module text")?;
    bytes[at] = 0xff;
    assert!(decode_module_uses(&bytes).is_none());
    let mut bytes =
        encode_private_names(&names, &[("app.unique".into(), run.syms.intern("secret"))]);
    let at = bytes
        .windows(6)
        .position(|s| s == b"secret")
        .ok_or("private text")?;
    bytes[at] = 0xff;
    assert!(decode_private_names(&names, &bytes).is_none());
    Ok(())
}

#[test]
fn lazy_index_and_configured_iteration_decode_only_requested_records() -> Result<(), &'static str> {
    // Exercise the public query itself, even when MIR can fold the known
    // vector length. A function pointer keeps coverage independent of that
    // pre-instrumentation optimization, without decoding any item.
    let is_empty = std::hint::black_box(Items::is_empty as fn(&Items) -> bool);
    let run = Run::new();
    let names = run.names();
    let wanted = run.items();
    let bytes = encode_items(&names, &wanted, &[]).map_err(|_| "encode")?;
    let (items, exports) = decode_lazy(&names, Arc::from(bytes)).ok_or("lazy index")?;
    assert!(!is_empty(&items));
    assert_eq!(items.len(), 2);
    assert!(exports.is_empty());
    assert_eq!(items.defs(), &[wanted[0].def, wanted[1].def]);
    assert_eq!(items.decoded(), 0);
    assert!(items.get(2).is_none());
    assert_eq!(items.decoded(), 0);
    assert_eq!(items.configured().collect::<Vec<_>>(), vec![&wanted[1]]);
    assert_eq!(items.decoded(), 1);
    assert_eq!(items.configured().collect::<Vec<_>>(), vec![&wanted[1]]);
    assert_eq!(items.decoded(), 1);
    assert_eq!(
        (&items).into_iter().collect::<Vec<_>>(),
        wanted.iter().collect::<Vec<_>>()
    );
    assert_eq!(items.decoded(), 2);
    Ok(())
}

#[test]
fn empty_interface_and_eager_items_have_consistent_empty_indexes() -> Result<(), &'static str> {
    let is_empty = std::hint::black_box(Items::is_empty as fn(&Items) -> bool);
    let run = Run::new();
    let names = run.names();
    let bytes = encode_items(&names, &[], &[]).map_err(|_| "encode")?;
    let (items, exports) = decode_lazy(&names, Arc::from(bytes)).ok_or("empty interface")?;
    assert!(is_empty(&items));
    assert_eq!(items.len(), 0);
    assert_eq!(items.iter().count(), 0);
    assert!(items.get(0).is_none());
    assert!(exports.is_empty());
    let eager = Items::new(run.items());
    assert!(!is_empty(&eager));
    assert_eq!(eager.decoded_other(), 0);
    assert_eq!(eager.configured().count(), 1);
    Ok(())
}

#[test]
fn docs_refresh_updates_already_decoded_and_still_lazy_records() -> Result<(), &'static str> {
    let run = Run::new();
    let names = run.names();
    let mut wanted = run.items();
    let bytes = encode_items(&names, &wanted, &[]).map_err(|_| "encode")?;
    let (mut items, _) = decode_lazy(&names, Arc::from(bytes.clone())).ok_or("decode")?;
    assert_eq!(items.get(0), Some(&wanted[0]));
    assert_eq!(items.decoded(), 1);
    for revision in ["initial", "updated"] {
        for item in &mut wanted {
            item.docs = vec![
                (Vec::new(), format!("{revision} docs")),
                (vec!["member".into()], "member docs".into()),
            ];
        }
        items
            .attach_docs(&encode_docs(&wanted))
            .ok_or("attach docs")?;
        assert_eq!(items.decoded(), 1, "metadata never forces the second item");
        assert_eq!(items.get(0), Some(&wanted[0]));
    }
    assert_eq!(items.get(1), Some(&wanted[1]));
    assert_eq!(items.decoded(), 2);
    assert_eq!(
        encode_items(&names, &wanted, &[]).map_err(|_| "encode docs")?,
        bytes
    );
    Ok(())
}

#[test]
fn rejected_docs_leave_all_existing_docs_unchanged() -> Result<(), &'static str> {
    let run = Run::new();
    let names = run.names();
    let mut wanted = run.items();
    for item in &mut wanted {
        item.docs = vec![(Vec::new(), "original".into())];
    }
    let bytes = encode_items(&names, &wanted, &[]).map_err(|_| "encode")?;
    let (mut items, _) = decode_lazy(&names, Arc::from(bytes)).ok_or("decode")?;
    let docs = encode_docs(&wanted);
    items.attach_docs(&docs).ok_or("original docs")?;
    assert_eq!(items.get(0), Some(&wanted[0]));
    assert!(items.attach_docs(&encode_docs(&wanted[..1])).is_none());
    for end in 0..docs.len() {
        assert!(items.attach_docs(&docs[..end]).is_none(), "prefix {end}");
    }
    let mut trailing = docs;
    trailing.push(0);
    assert!(items.attach_docs(&trailing).is_none());
    assert_eq!(items.get(0), Some(&wanted[0]));
    assert_eq!(items.get(1), Some(&wanted[1]));
    Ok(())
}

#[test]
fn interface_range_owns_the_blob_without_reading_surrounding_bytes() -> Result<(), &'static str> {
    let run = Run::new();
    let names = run.names();
    let wanted = run.items();
    let bytes = encode_items(&names, &wanted, &[]).map_err(|_| "encode")?;
    let mut packed = vec![0xff; 17];
    packed.extend_from_slice(&bytes);
    packed.extend_from_slice(&[0xff; 13]);
    let packed: Arc<[u8]> = Arc::from(packed);
    let (items, _) =
        decode_lazy_in(&names, Arc::clone(&packed), 17..17 + bytes.len()).ok_or("range")?;
    drop(packed);
    assert_eq!(items.get(1), Some(&wanted[1]));
    assert_eq!(items.get(0), Some(&wanted[0]));
    assert!(decode_lazy_in(&names, Arc::from(bytes.as_slice()), 0..bytes.len() + 1).is_none());
    Ok(())
}

#[test]
fn every_truncated_item_blob_is_a_cache_miss() -> Result<(), &'static str> {
    let run = Run::new();
    let names = run.names();
    let wanted = run.items();
    let bytes = encode_items(&names, &wanted, &[]).map_err(|_| "encode")?;
    assert_eq!(decode_items(&names, &bytes), Some((wanted, Vec::new())));
    for end in 0..bytes.len() {
        assert!(
            decode_items(&names, &bytes[..end]).is_none(),
            "prefix {end}"
        );
    }
    Ok(())
}

/// A corrupt item record is an ordinary cache miss, including on lazy reads.
/// The builtin control also has none of a callable's or declared type's attrs.
#[test]
fn builtin_records_round_trip_and_unknown_item_tags_are_memoized_cache_misses() {
    let run = Run::new();
    let names = run.names();
    let mut builtin = hd_resolve::seed::items(&names, "std.core")
        .into_iter()
        .find(|item| item.def == run.known.list)
        .expect("the real builtin List declaration");
    assert!(builtin.call_mut().is_none());
    assert!(builtin.type_attrs().is_none());
    assert_eq!(builtin.type_decl(), hd_types::sealed::TypeDecl::Builtin);
    let mut bytes = encode_items(&names, &[builtin.clone()], &[]).expect("encode builtin");
    assert_eq!(
        decode_items(&names, &bytes),
        Some((vec![builtin.clone()], vec![]))
    );
    let (good, _) = decode_lazy(&names, Arc::from(bytes.clone())).expect("valid index");
    assert_eq!(good.get(0), Some(&builtin));

    // put_item's builtin discriminator is followed only by the empty export
    // count. Check that boundary explicitly before corrupting its kind byte.
    let tag = bytes.len() - size_of::<u32>() - 1;
    assert_eq!(bytes[tag], 9);
    assert_eq!(&bytes[tag + 1..], &[0; size_of::<u32>()]);
    bytes[tag] = u8::MAX;
    assert!(decode_items(&names, &bytes).is_none());
    let (bad, _) =
        decode_lazy(&names, Arc::from(bytes)).expect("record corruption leaves index valid");
    assert_eq!(bad.decoded(), 0);
    assert!(bad.get(0).is_none());
    assert_eq!(bad.decoded(), 1);
    assert!(bad.get(0).is_none());
    assert_eq!(bad.decoded(), 1, "a cached failure is not decoded again");
}

#[test]
fn damaged_exports_are_cache_misses() -> Result<(), &'static str> {
    let run = Run::new();
    let names = run.names();
    let wanted = run.items();
    let exports = [hd_resolve::iface::Export {
        module: "app.exported".into(),
        name: wanted[0].name,
        def: wanted[0].def,
        kind: hd_resolve::iface::HeadKind::Struct,
    }];
    let bytes = encode_items(&names, &wanted, &exports).map_err(|_| "encode")?;
    assert_eq!(
        decode_items(&names, &bytes),
        Some((wanted, exports.to_vec()))
    );
    let mut bad_kind = bytes.clone();
    let last = bad_kind.last_mut().ok_or("export tag")?;
    *last = 0xff;
    assert!(decode_items(&names, &bad_kind).is_none());
    let mut bad_module = bytes;
    let at = bad_module
        .windows(12)
        .position(|s| s == b"app.exported")
        .ok_or("export module")?;
    bad_module[at] = 0xff;
    assert!(decode_items(&names, &bad_module).is_none());
    Ok(())
}

/// Result-path metadata is read back from disk independently of the source
/// resolver (ref.result.path.form, ref.result.path.lifetime). Re-intern in a
/// different session so matching numeric symbol IDs cannot mask a lost path.
#[test]
fn result_paths_survive_eager_and_lazy_interfaces_in_a_fresh_session() {
    let source = include_str!("inputs/result_paths.hd");
    let writer = Run::new();
    let (items, exports) = super::const_generics::resolve_in(writer.names(), source);
    let bytes = encode_items(&writer.names(), &items, &exports).expect("encode result paths");

    let reader = Run::new();
    reader.paths.item("other", "before", "Earlier");
    reader.syms.intern("an earlier symbol");
    assert_ne!(writer.syms.intern("name"), reader.syms.intern("name"));
    let names = reader.names();
    let (eager, _) = decode_items(&names, &bytes).expect("eager result paths");
    let (lazy, _) = decode_lazy(&names, Arc::from(bytes)).expect("lazy result paths");
    assert_eq!(lazy.decoded(), 0);

    for (function, expected) in [
        (
            "parts",
            vec![(0, 1, vec!["name", "first"]), (1, 0, vec!["name", "last"])],
        ),
        ("tuple", vec![(0, 0, vec!["_1", "name", "first"])]),
    ] {
        let index = eager
            .iter()
            .position(|item| names.text(item.name) == function)
            .expect("function in eager interface");
        let item = lazy.get(index).expect("function in lazy interface");
        assert_eq!(item, &eager[index]);
        let sig = item.sig().expect("function signature");
        let actual: Vec<_> = sig
            .paths
            .iter()
            .map(|path| {
                (
                    path.slot,
                    path.param,
                    path.fields
                        .iter()
                        .map(|field| names.text(*field))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        assert_eq!(actual, expected, "{function}");
    }
    assert_eq!(
        lazy.decoded(),
        2,
        "only the requested signatures are decoded"
    );
}

/// Generic kind and impl metadata are queried the same way before and
/// after caching, even when diagnostics do not ask for a const-slot label.
#[test]
fn generic_and_impl_metadata_survive_interface_roundtrip() -> Result<(), &'static str> {
    let run = Run::new();
    let names = run.names();
    let text = include_str!("inputs/generic_item_metadata.hd");
    let (items, exports) = super::const_generics::resolve_in(names, text);
    let bytes = encode_items(&names, &items, &exports).map_err(|_| "encode resolved items")?;
    let (decoded, decoded_exports) = decode_items(&names, &bytes).ok_or("decode resolved items")?;
    assert_eq!(decoded_exports, exports);
    for view in [&items, &decoded] {
        let configure = view
            .iter()
            .find(|item| item.def == names.item("app", "configure"))
            .ok_or("resolved function")?;
        let ItemData::Fn(signature, _) = &configure.data else {
            return Err("function signature");
        };
        let kinds: Vec<_> = signature
            .generics
            .iter()
            .map(|generic| (names.text(generic.name), generic.kind_name()))
            .collect();
        assert_eq!(kinds, [("T", "type"), ("N", "const"), ("R", "row")]);
        let mark = names.item("app", "Mark");
        let implementation = view
            .iter()
            .find(|item| {
                matches!(&item.data,
            ItemData::Impl { trait_, .. } if *trait_ == mark)
            })
            .ok_or("resolved impl")?;
        assert_eq!(implementation.kind(), Some(hd_resolve::HeadKind::Impl));
    }
    Ok(())
}
