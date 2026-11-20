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
    let run = Run::new();
    let names = run.names();
    let wanted = run.items();
    let bytes = encode_items(&names, &wanted, &[]).map_err(|_| "encode")?;
    let (items, exports) = decode_lazy(&names, Arc::from(bytes)).ok_or("lazy index")?;
    assert!(!items.is_empty());
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
    let run = Run::new();
    let names = run.names();
    let bytes = encode_items(&names, &[], &[]).map_err(|_| "encode")?;
    let (items, exports) = decode_lazy(&names, Arc::from(bytes)).ok_or("empty interface")?;
    assert!(items.is_empty());
    assert_eq!(items.len(), 0);
    assert_eq!(items.iter().count(), 0);
    assert!(items.get(0).is_none());
    assert!(exports.is_empty());
    let eager = Items::new(run.items());
    assert!(!eager.is_empty());
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
