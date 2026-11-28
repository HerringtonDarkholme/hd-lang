//! Read-only source and declaration queries used by diagnostics and tooling.

use hd_base::{FileId, NodeIdx, TokenIdx, UnitId};
use hd_intern::{PathTable, ShardedInterner};
use hd_resolve::facts::{DeclView, callee_of, fact_lines, is_marker, name_path, path_def};
use hd_resolve::iface::{IfaceIndex, unit_iface};
use hd_resolve::{
    Binding, BindingKind, ImplKind, Item, ItemData, Items, KnownItems, Lookup, ModuleScope, Names,
    Origin, Src, TypeAttrs,
};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind, parse};
use hd_types::AdtId;
use hd_types::{InternPool, TyData};
use std::sync::Arc;

fn node<'a>(src: &Src<'a>, kind: SyntaxKind) -> Result<NodeRef<'a>, &'static str> {
    src.root()
        .descendants()
        .find(|n| n.kind() == kind)
        .ok_or("expected syntax node")
}

#[test]
fn empty_source_queries_keep_the_file_and_have_empty_ranges() {
    let parsed = parse(b"");
    let src = Src {
        parse: &parsed,
        text: "",
        file: FileId::from_raw(7),
    };
    let root = src.root();
    assert_eq!(src.head_span(root), src.span(root));
    assert_eq!(src.span(root).file, src.file);
    assert_eq!((src.span(root).lo, src.span(root).hi), (0, 0));
    assert_eq!(src.node_text(root), "");
    assert_eq!(src.tokens(root).count(), 0);
    assert_eq!(src.first_ident(root), None);
    assert_eq!(src.name_after(root, TokenKind::KwFn), None);
    for token in [TokenIdx::NONE, TokenIdx::from_raw(0)] {
        assert_eq!(src.tkind(token), None);
        assert_eq!(src.text(token), "");
        assert_eq!(src.token_span(token), src.span(root));
    }
}

#[test]
fn headers_end_before_the_body_and_bodyless_items_use_the_whole_span() -> Result<(), &'static str> {
    let text = "pub fn sample() -> i32:\n    +1\n\ntype Number = i32\n";
    let parsed = parse(text.as_bytes());
    assert!(parsed.is_ok(), "{:?}", parsed.diagnostics);
    let src = Src {
        parse: &parsed,
        text,
        file: FileId::from_raw(2),
    };
    let function = node(&src, SyntaxKind::FnDecl)?;
    let alias = node(&src, SyntaxKind::TypeDecl)?;
    assert!(src.is_pub(function));
    assert!(!src.is_pub(alias));
    let header = src.head_span(function);
    assert_eq!(
        &text[header.lo as usize..header.hi as usize],
        "pub fn sample() -> i32:"
    );
    assert!(header.hi < src.span(function).hi);
    assert_eq!(src.head_span(alias), src.span(alias));
    assert_eq!(
        src.text(src.name_after(function, TokenKind::KwFn).ok_or("name")?),
        "sample"
    );
    assert_eq!(src.name_after(alias, TokenKind::KwFn), None);
    assert_eq!(src.line_column(header.lo), (1, 1));
    Ok(())
}

#[test]
fn broken_items_are_disjoint_from_the_items_tooling_reads() {
    let text = "fn broken(value) -> void: pass\nfn kept() -> void: pass\n";
    let parsed = parse(text.as_bytes());
    assert!(!parsed.is_ok());
    let src = Src {
        parse: &parsed,
        text,
        file: FileId::from_raw(0),
    };
    let good: Vec<_> = src.items().map(|n| src.node_text(n)).collect();
    let bad: Vec<_> = src.broken().map(|n| src.node_text(n)).collect();
    assert_eq!(good, ["fn kept() -> void: pass"]);
    assert_eq!(bad, ["fn broken(value) -> void: pass"]);
}

#[test]
fn a_call_result_member_has_no_name_path_but_a_named_member_keeps_its_root()
-> Result<(), &'static str> {
    for (text, expected) in [
        (
            "fn f() -> void: pkg.Type.value\n",
            vec!["pkg", "Type", "value"],
        ),
        ("fn f() -> void: make().value\n", Vec::new()),
    ] {
        let parsed = parse(text.as_bytes());
        assert!(parsed.is_ok(), "{:?}", parsed.diagnostics);
        let src = Src {
            parse: &parsed,
            text,
            file: FileId::from_raw(0),
        };
        let field = src
            .root()
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FieldExpr && src.text(src.last(*n)) == "value")
            .ok_or("outer field")?;
        assert_eq!(name_path(&src, field), expected);
    }
    Ok(())
}

#[test]
fn marker_and_value_decorators_are_separated_in_source_order() -> Result<(), &'static str> {
    let text = "@first\n@cfg(false)\n@pkg.second::[i32](+1)\n@last\nfn f() -> void: pass\n";
    let parsed = parse(text.as_bytes());
    assert!(parsed.is_ok(), "{:?}", parsed.diagnostics);
    let src = Src {
        parse: &parsed,
        text,
        file: FileId::from_raw(0),
    };
    let function = node(&src, SyntaxKind::FnDecl)?;
    let values = fact_lines(&src, function, |d| src.node_text(d) == "@last");
    assert_eq!(
        values.iter().map(|n| src.node_text(*n)).collect::<Vec<_>>(),
        ["first", "pkg.second::[i32](+1)"]
    );
    let callee = callee_of(values[1]).ok_or("callee")?;
    assert_eq!(name_path(&src, callee), ["pkg", "second"]);
    for (name, marker) in [
        ("intrinsic", true),
        ("from", true),
        ("source", true),
        ("auto_closing", true),
        ("track_caller", true),
        ("cfg", false),
        ("custom", false),
    ] {
        let text = format!("@{name}\nfn f() -> void: pass\n");
        let parsed = parse(text.as_bytes());
        assert!(parsed.is_ok(), "{:?}", parsed.diagnostics);
        let src = Src {
            parse: &parsed,
            text: &text,
            file: FileId::from_raw(0),
        };
        assert_eq!(
            is_marker(&src, node(&src, SyntaxKind::Decorator)?),
            marker,
            "{name}"
        );
    }
    Ok(())
}

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
    fn item(&self, label: &str, data: ItemData) -> Item {
        Item::new(
            self.names().item("app.data", label),
            self.syms.intern(label),
            true,
            data,
        )
    }
}

#[test]
fn path_resolution_distinguishes_root_modules_from_imported_nested_modules() {
    let run = Run::new();
    let names = run.names();
    let target = names.item("app.data", "Target");
    let mut scope = ModuleScope::default();
    scope.bind(
        run.syms.intern("Target"),
        Binding {
            kind: BindingKind::Item,
            value: target.raw(),
        },
        Origin::Own,
        0,
    );
    scope.modules = vec!["app".into(), "app.data".into()];
    for (label, value) in [("root", 0), ("nested", 1)] {
        scope.bind(
            run.syms.intern(label),
            Binding {
                kind: BindingKind::Module,
                value,
            },
            Origin::Use,
            0,
        );
    }
    let exports = |module: &str, symbol| {
        (module == "app.data" && symbol == run.syms.intern("Target")).then_some(target)
    };
    for path in [
        vec!["Target"],
        vec!["root", "data", "Target"],
        vec!["nested", "Target"],
    ] {
        assert_eq!(path_def(&names, &scope, &exports, &path), Some(target));
    }
    for path in [
        vec![],
        vec!["missing"],
        vec!["Target", "member"],
        vec!["nested"],
        vec!["nested", "child", "Target"],
        vec!["root", "data", "missing"],
    ] {
        assert_eq!(path_def(&names, &scope, &exports, &path), None);
    }
}

#[test]
fn a_poisoned_binding_is_replaced_but_cannot_poison_an_existing_binding() {
    let run = Run::new();
    let name = run.syms.intern("value");
    let item = Binding {
        kind: BindingKind::Item,
        value: run.names().item("app.data", "value").raw(),
    };
    let poison = Binding {
        kind: BindingKind::Poison,
        value: 0,
    };
    let mut scope = ModuleScope::default();
    scope.bind(name, poison, Origin::Use, 1);
    scope.bind(name, item, Origin::Own, 2);
    assert_eq!(scope.lookup(name), Some(item));
    assert!(!scope.bound_from_outside(name));
    scope.bind(name, poison, Origin::Use, 3);
    assert_eq!(scope.lookup(name), Some(item));
    assert_eq!(scope.origin, [Origin::Own]);
    assert_eq!(scope.use_row, [2]);
}

#[test]
fn declaration_views_report_names_blocks_and_only_derived_nominal_impls() -> Result<(), &'static str>
{
    let run = Run::new();
    let names = run.names();
    let record = run.item("Record", ItemData::Struct(Vec::new(), TypeAttrs::default()));
    let target = run.pool.intern_ty(&TyData::Adt(
        AdtId::Declared(record.def),
        run.pool.list(&[]),
    ));
    let trait_ = names.known.eq;
    let implementation = |label, kind| {
        run.item(
            label,
            ItemData::Impl {
                trait_,
                trait_args: run.pool.list(&[]),
                trait_lifetimes: Vec::new(),
                self_lifetimes: Vec::new(),
                trait_slots: Vec::new(),
                self_ty: target,
                methods: Vec::new(),
                assoc: Vec::new(),
                assoc_lifetimes: Vec::new(),
                by: None,
                kind,
                fills_defaults: false,
                skips: Vec::new(),
            },
        )
    };
    let derived = implementation("derived", ImplKind::Derived);
    let written = implementation("written", ImplKind::Written);
    let own = [record.clone(), derived.clone(), written];
    let unit = unit_iface(
        "app.data",
        Items::new(vec![record.clone(), derived]),
        Vec::new(),
        Arc::from([]),
        None,
        &[],
        Vec::new(),
    );
    let unit_id = UnitId::from_raw(0);
    let index = IfaceIndex::default();
    index.insert(unit_id, &unit);
    let interfaces = [(unit_id, &unit)];
    let lookup = Lookup::new(
        &own,
        &interfaces,
        &index,
        hd_resolve::iface::ImportHints::Index(&hd_resolve::iface::EMPTY_IMPORT_INDEX),
    );
    let mut scope = ModuleScope::default();
    scope.bind(
        record.name,
        Binding {
            kind: BindingKind::Item,
            value: record.def.raw(),
        },
        Origin::Own,
        0,
    );
    scope.modules.push("app.data".into());
    scope.bind(
        run.syms.intern("module"),
        Binding {
            kind: BindingKind::Module,
            value: 0,
        },
        Origin::Use,
        0,
    );
    let blocks = [NodeIdx::from_raw(4), NodeIdx::from_raw(9)];
    let view = DeclView::new(names, unit_id, &scope, &lookup, &blocks);
    assert!(view.derivation_block(blocks[0]));
    assert!(view.derivation_block(blocks[1]));
    assert!(!view.derivation_block(NodeIdx::from_raw(5)));
    let (def, item) = view.item_named("Record").ok_or("bound item")?;
    assert_eq!(def, record.def);
    assert_eq!(item, &record);
    assert!(view.item_named("module").is_none());
    assert!(view.item_named("missing").is_none());
    assert_eq!(view.display_name(def), "Record");
    assert_eq!(view.package_of(def), names.package_node(def));
    assert_eq!(
        view.derived().collect::<Vec<_>>(),
        [(record.def, trait_), (record.def, trait_)]
    );
    Ok(())
}

#[test]
fn scope_overlay_rebases_modules_and_ambiguities_and_hides_unreadable_names() {
    let run = Run::new();
    let names = run.names();
    let symbol = |label| run.syms.intern(label);
    let item = |label| Binding {
        kind: BindingKind::Item,
        value: names.item("app.data", label).raw(),
    };
    let mut outer = ModuleScope::default();
    outer.modules.push("app.outer".into());
    outer.bind(
        symbol("outer"),
        Binding {
            kind: BindingKind::Module,
            value: 0,
        },
        Origin::Use,
        0,
    );
    outer.bind(symbol("value"), item("Old"), Origin::Own, 0);
    let mut inner = ModuleScope::default();
    inner.modules.push("app.inner".into());
    inner.bind(
        symbol("inner"),
        Binding {
            kind: BindingKind::Module,
            value: 0,
        },
        Origin::Use,
        2,
    );
    inner.bind(symbol("value"), item("New"), Origin::Own, 3);
    inner.bind(symbol("choice"), item("A"), Origin::Use, 4);
    inner.bind(symbol("choice"), item("B"), Origin::Use, 5);
    inner.bind(
        symbol("broken"),
        Binding {
            kind: BindingKind::Poison,
            value: 0,
        },
        Origin::Use,
        6,
    );
    inner.bind(
        symbol("prelude"),
        Binding {
            kind: BindingKind::Prelude,
            value: names.known.eq.raw(),
        },
        Origin::Prelude,
        7,
    );
    let merged = outer.overlay(&inner);
    assert_eq!(outer.modules, ["app.outer"]);
    assert_eq!(outer.lookup(symbol("value")), Some(item("Old")));
    assert_eq!(merged.modules, ["app.outer", "app.inner"]);
    assert_eq!(
        merged.lookup(symbol("inner")),
        Some(Binding {
            kind: BindingKind::Module,
            value: 1
        })
    );
    assert_eq!(
        merged.lookup(symbol("choice")),
        Some(Binding {
            kind: BindingKind::Ambiguous,
            value: merged.index[&symbol("choice")]
        })
    );
    assert_eq!(merged.lookup(symbol("value")), Some(item("New")));
    let readable = merged.named(&names);
    assert_eq!(readable.len(), 4);
    assert!(readable.contains(&(
        "inner".into(),
        hd_resolve::Named::Module("app.inner".into())
    )));
    assert!(readable.contains(&(
        "outer".into(),
        hd_resolve::Named::Module("app.outer".into())
    )));
    assert!(readable.contains(&(
        "value".into(),
        hd_resolve::Named::Item {
            module: "app.data".into(),
            name: "New".into()
        }
    )));
    assert!(
        readable
            .iter()
            .all(|(label, _)| label != "choice" && label != "broken")
    );
}

/// Worlds may expose their already-owned module text; the recorder must
/// preserve that fast path without making the path-text query a dependency.
#[test]
fn recorded_world_node_queries_preserve_text_and_track_only_lookups() {
    use hd_base::PathId;
    use hd_resolve::{Reads, Recorded, UnitIface, World};
    use std::cell::RefCell;

    struct IndexedWorld {
        node: PathId,
        calls: RefCell<Vec<String>>,
    }
    impl World for IndexedWorld {
        fn import_hints(&self) -> hd_resolve::iface::ImportHints<'_> {
            hd_resolve::iface::ImportHints::Index(&hd_resolve::iface::EMPTY_IMPORT_INDEX)
        }

        fn module_path_node(&self, module: PathId) -> Option<&str> {
            (module == self.node).then_some("app.data")
        }
        fn module_unit(&self, module: &str) -> Option<u32> {
            self.calls.borrow_mut().push(module.to_owned());
            (module == "app.data").then_some(7)
        }
        fn usable(&self, module: &str) -> bool {
            module == "app.data"
        }
        fn test_only(&self, _module: &str) -> bool {
            false
        }
        fn published(&self, module: &str) -> Option<bool> {
            self.usable(module).then_some(false)
        }
        fn iface(&self, _unit: u32) -> Option<Arc<UnitIface>> {
            None
        }
    }

    let run = Run::new();
    let names = run.names();
    let data = names.module("app.data");
    let missing = names.module("app.missing");
    let world = IndexedWorld {
        node: data,
        calls: RefCell::default(),
    };
    let recorded = Recorded::new(&world);
    assert_eq!(recorded.module_path_node(data), Some("app.data"));
    assert_eq!(recorded.module_path_node(missing), None);
    assert_eq!(recorded.reads(), Reads::default());
    assert!(world.calls.borrow().is_empty());

    // The default node lookup takes owned text for data and renders the
    // missing node's text. Every lookup reaches the world exactly once.
    assert_eq!(recorded.module_unit_node(&names, missing), None);
    assert_eq!(recorded.module_unit_node(&names, data), Some(7));
    assert_eq!(recorded.module_unit_node(&names, data), Some(7));
    assert_eq!(
        *world.calls.borrow(),
        ["app.missing", "app.data", "app.data"]
    );
    let expected = Reads {
        modules: vec!["app.data".into(), "app.missing".into()],
        units: Vec::new(),
    };
    assert_eq!(recorded.reads(), expected);

    // Holding dependency reads must not suppress the underlying lookup.
    recorded.hold_reads(true);
    assert_eq!(
        recorded.module_unit_node(&names, names.module("app.held")),
        None
    );
    assert_eq!(
        world.calls.borrow().last().map(String::as_str),
        Some("app.held")
    );
    assert_eq!(recorded.reads(), expected);
    recorded.hold_reads(false);
    assert_eq!(
        recorded.module_unit_node(&names, names.module("app.held")),
        None
    );
    assert_eq!(
        recorded.reads().modules,
        ["app.data", "app.held", "app.missing"]
    );
}

/// `module.rt.std-only`: neither a runtime module nor a public re-export
/// of its declaration is a usable import repair in another package.
#[test]
fn import_hints_exclude_runtime_reexports_and_cache_unrelated_exports() {
    use hd_resolve::HeadKind;
    use hd_resolve::iface::{Export, ImportHint, ImportIndex};
    use hd_resolve::work::count_hint_work;

    let mut warm_work = Vec::new();
    for width in [100u64, 1000] {
        let run = Run::new();
        let names = run.names();
        let visible = names.item("std.text", "Shared");
        let hidden = names.item("std.rt.memory", "Shared");
        let tree = names.item("std.rtree", "Node");
        let export = |module: &str, label: &str, def| Export {
            module: module.into(),
            name: run.syms.intern(label),
            def,
            kind: HeadKind::Struct,
        };
        let mut exports = vec![
            export("std.text", "Shared", visible),
            export("std.facade", "Shared", visible),
            export("std.other_facade", "Shared", hidden),
            export("std.rt.memory", "Shared", hidden),
            export("std.leaky", "RuntimeOnly", hidden),
            export("std.rt", "RootOnly", visible),
            export("std.rt.memory", "NestedOnly", visible),
            export("std.rtree", "Node", tree),
        ];
        for i in 0..width {
            let label = format!("Filler{i}");
            exports.push(export("std.misc", &label, names.item("std.misc", &label)));
        }
        let ((index, hint), cold) = count_hint_work(|| {
            let index = ImportIndex::new(&names, &exports);
            let hint = index.hint("Shared");
            (index, hint)
        });
        assert_eq!(hint.path.as_deref(), Some("std.text.Shared"));
        assert_eq!(hint.text, "; add `use std.text.Shared`");
        // Explicit advice is independent of the lookup's visible interfaces.
        let directory = IfaceIndex::default();
        let interfaces = [];
        let lookup = Lookup::new(
            &[],
            &interfaces,
            &directory,
            hd_resolve::iface::ImportHints::Index(&index),
        );
        assert_eq!(lookup.import_hint("Shared"), hint);

        assert!(cold >= width, "the first hint indexes all exports");
        for label in [
            "RuntimeOnly",
            "RootOnly",
            "NestedOnly",
            "leaky",
            "rt",
            "memory",
        ] {
            let (hint, work) = count_hint_work(|| index.hint(label));
            assert_eq!(hint, ImportHint::default(), "{width}: {label}");
            assert_eq!(work, 1, "an absent name probes the explicit index once");
        }
        for (label, path) in [
            ("Node", "std.rtree.Node"),
            ("rtree", "std.rtree"),
            ("text", "std.text"),
        ] {
            assert_eq!(index.hint(label).path.as_deref(), Some(path));
        }
        let (again, work) = count_hint_work(|| index.hint("Shared"));
        assert_eq!(again, hint);
        warm_work.push(work);
    }
    assert_eq!(
        warm_work[0], warm_work[1],
        "tenfold unrelated exports must not increase a warm lookup's work"
    );
}

#[test]
fn unknown_use_root_diagnostics_keep_empty_and_written_paths() {
    for (path, root) in [
        (Vec::<&str>::new(), ""),
        (vec!["missing", "name"], "missing"),
    ] {
        let (code, message) =
            hd_resolve::lower::use_root_diagnostic(hd_project::UseRootError::UnknownRoot, &path);
        assert_eq!(code, hd_diag::Code::UnknownModule);
        assert_eq!(
            message,
            format!(
                "`{root}` is no use root; a path starts with `pkg`, `std`, `dep`, `self` or `super`"
            )
        );
    }
}
