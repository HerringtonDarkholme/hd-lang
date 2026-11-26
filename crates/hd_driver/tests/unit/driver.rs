//! Tests of the driver's crate-root code: program items, unit cycles,
//! cache sections, host declarations and ABIs, capabilities, verification,
//! instantiation chains and label codecs.

use std::sync::{Arc, OnceLock};

use hd_base::{DefId, Hash128};
use hd_cache::{EntryKind, MemoryStore};
use hd_diag::{Code, DiagBuf};
use hd_project::{MemorySources, ModuleTable, PackageIn, Scope, SourceSet};
use hd_sched::SerialOrder;

#[cfg(feature = "host-artifacts")]
use crate::support::run_package_host_javascript;
use crate::support::warm_std_store;
use hd_driver::testing::*;
use hd_driver::*;

use hd_resolve::{Item, ItemData};

mod program_items_tests {
    use super::{DefId, Item, ItemData, ProgramItems};
    use hd_base::Symbol;

    fn item(raw: u32, public: bool) -> Item {
        Item::new(
            DefId::from_raw(raw),
            Symbol::from_raw(raw),
            public,
            ItemData::Struct(Vec::new(), hd_resolve::TypeAttrs::default()),
        )
    }

    fn items(list: Vec<Item>) -> hd_resolve::Items {
        hd_resolve::Items::new(list)
    }

    #[test]
    fn sparse_and_unknown_declarations_are_absent() {
        let mut program = ProgramItems::default();
        assert!(program.get(DefId::NONE).is_none());
        assert!(program.get(DefId::from_raw(0)).is_none());
        program.add_checked(items(vec![item(37, true), item(2, false)]));
        assert_eq!(program.get(DefId::from_raw(37)), Some(&item(37, true)));
        assert_eq!(program.get(DefId::from_raw(2)), Some(&item(2, false)));
        for raw in [0, 1, 3, 36, 38, u32::MAX - 1, u32::MAX] {
            assert!(program.get(DefId::from_raw(raw)).is_none());
        }
        assert_eq!(program.slots(), 38);
    }

    #[test]
    fn a_later_check_replaces_an_earlier_one() {
        let mut program = ProgramItems::default();
        program.add_checked(items(vec![item(11, false), item(4, false)]));
        program.add_checked(items(vec![item(11, true)]));
        assert_eq!(program.get(DefId::from_raw(11)), Some(&item(11, true)));
        assert_eq!(program.get(DefId::from_raw(4)), Some(&item(4, false)));
    }
}

mod cache_section_tests {
    use super::{Arc, EntryKind, Hash128, LAYOUT, cache_sections};

    /// The compile unit changed from a folder to a module group (#2347),
    /// with `LAYOUT_VERSION` bumped: every entry a pre-unit build left at
    /// a key this build asks for is framed with the old layout, so it misses
    /// and is rebuilt, quietly, never decoded as a module group's.
    #[test]
    fn entries_of_the_folder_layout_are_misses() {
        use hd_cache::{CacheStore, MemoryStore, encode_entry};
        use hd_project::MemorySources;

        let mut program = MemorySources::default();
        program.insert("main.hd", include_str!("programs/folder_layout/main.hd"));
        program.insert("shop.hd", include_str!("programs/folder_layout/shop.hd"));
        let compile = |store: &MemoryStore| {
            let host = super::Host {
                render_tir: &[],
                sources: &program,
                store,
                clock: &super::NoClock,
                executor: super::Executor::Serial(hd_sched::SerialOrder::Priority),
            };
            let goal = super::Goal::Program {
                entry: "main".into(),
            };
            let out = super::build_packages(&host, "shop", &super::Packages::default(), &goal);
            assert!(out.diags.is_empty(), "{}", out.render());
            assert!(out.wasm.is_some(), "the program builds");
            out
        };
        let store = MemoryStore::default();
        let cold = compile(&store);
        let control = compile(&store);
        assert!(control.counters.hit("iface") + control.counters.hit("check") > 0);
        let mut replaced = 0;
        for (kind, key) in store.drain_new() {
            if matches!(kind, EntryKind::Iface | EntryKind::Check | EntryKind::Graph) {
                let old = encode_entry(kind, LAYOUT - 1, key, Hash128(1), 0, &[(0, 0, &[])]);
                store.put(kind, &key, &old);
                replaced += 1;
            }
        }
        assert!(replaced > 0);
        let warm = compile(&store);
        assert_eq!(warm.wasm, cold.wasm);
        assert_eq!(warm.counters.hit("iface") + warm.counters.hit("check"), 0);
    }

    #[test]
    fn a_partial_repl_build_publishes_complete_std_skims() {
        use hd_cache::{CacheStore, MemoryStore};
        use hd_project::MemorySources;

        let compile = |store: &MemoryStore, sources: &MemorySources, goal: super::Goal| {
            let host = super::Host {
                render_tir: &[],
                sources,
                store,
                clock: &super::NoClock,
                executor: super::Executor::Serial(hd_sched::SerialOrder::Priority),
            };
            let out = super::build_packages(&host, "shop", &super::Packages::default(), &goal);
            assert!(out.diags.is_empty(), "{}", out.render());
            assert!(out.wasm.is_some(), "the program builds");
        };
        let std_pack = |store: &MemoryStore| {
            let keys: Vec<_> = store
                .drain_new()
                .into_iter()
                .filter(|(kind, _)| *kind == EntryKind::Depfiles)
                .collect();
            assert_eq!(keys.len(), 1, "a cold build publishes the std skim pack");
            store.get(keys[0].0, &keys[0].1).expect("published pack")
        };

        let full = MemoryStore::default();
        let mut program = MemorySources::default();
        program.insert("main.hd", "pub fn main() -> void: pass\n");
        compile(
            &full,
            &program,
            super::Goal::Program {
                entry: "main".into(),
            },
        );
        let expected = std_pack(&full);

        let partial = MemoryStore::default();
        let mut repl = MemorySources::default();
        repl.insert(&hd_project::repl_file(1), "+1\n");
        compile(&partial, &repl, super::Goal::ReplInput { input: 1 });
        let actual = std_pack(&partial);
        assert!(
            actual == expected,
            "the same embedded std has one skim pack, including files the REPL never reads"
        );

        // A later test build shares the std pack with the partial build.
        let mut tests = MemorySources::default();
        tests.insert("tests/one.hd", include_str!("programs/lib/it_one.hd"));
        compile(
            &partial,
            &tests,
            super::Goal::Tests {
                module: None,
                filter: None,
            },
        );
    }

    #[test]
    fn cached_payloads_share_the_loaded_entry_and_keep_it_alive() {
        let key = Hash128(17);
        let toolchain = Hash128(29);
        let encoded = hd_cache::encode_entry(
            EntryKind::Graph,
            LAYOUT,
            key,
            toolchain,
            0,
            &[(0, 0, b"first"), (1, 0, b""), (2, 0, b"third")],
        );
        let old: Arc<[u8]> = Arc::from(hd_cache::encode_entry(
            EntryKind::Graph,
            LAYOUT - 1,
            key,
            toolchain,
            0,
            &[(0, 0, b"old layout")],
        ));
        assert!(cache_sections(&old, key, toolchain).is_none());
        let entry: Arc<[u8]> = Arc::from(encoded);
        let retained = Arc::downgrade(&entry);
        let sections = cache_sections(&entry, key, toolchain).expect("valid entry");
        assert_eq!(sections.len(), 3);
        for section in &sections {
            assert!(Arc::ptr_eq(section.backing(), &entry));
        }
        assert_eq!(sections[0].as_ref(), b"first");
        assert_eq!(sections[1].as_ref(), b"");
        assert_eq!(sections[2].as_ref(), b"third");
        drop(entry);
        assert!(retained.upgrade().is_some());
        assert_eq!(sections[2].as_ref(), b"third");
        drop(sections);
        assert!(retained.upgrade().is_none());
    }
}

mod host_declaration_tests {
    use super::*;

    fn metadata(package: u16, paths: &[&str]) -> HostPackage {
        HostPackage {
            package,
            identity: format!("/sdk-{package}"),
            versioned_source: None,
            declarations: Some(hd_project::HostDeclarations {
                providers: Some(
                    paths
                        .iter()
                        .map(|p| hd_project::HostProvider {
                            trait_path: (*p).to_owned(),
                            path: Some("provider.mjs".into()),
                            line: 7,
                        })
                        .collect(),
                ),
                line: 7,
            }),
            manifest: format!("/sdk-{package}/hd.toml"),
        }
    }

    fn sources(text: &str) -> MemorySources {
        let mut files = MemorySources::default();
        files.insert("server.hd", text);
        files
    }

    fn declarations(files: &[&MemorySources], packages: &[HostPackage]) -> DiagBuf {
        let inputs: Vec<_> = files
            .iter()
            .map(|sources| PackageIn {
                name: "sdk",
                identity: "sdk",
                sources: *sources,
                scope: Scope::All,
                requires: Vec::new(),
                entries: Vec::new(),
                dev_requires: Vec::new(),
            })
            .collect();
        let table = ModuleTable::discover_all(&inputs);
        let texts = texts_ready(
            table
                .sources
                .iter()
                .map(|(p, file)| {
                    Arc::from(
                        String::from_utf8_lossy(&files[usize::from(*p)].read(file).unwrap())
                            .into_owned(),
                    )
                })
                .collect(),
        );
        let parsed = (0..texts.len())
            .map(|_| OnceLock::new())
            .collect::<Vec<_>>();
        let declaration_sources: Vec<&dyn SourceSet> = files
            .iter()
            .map(|sources| *sources as &dyn SourceSet)
            .collect();
        let mut diags = DiagBuf::default();
        check_host_declarations(
            &table,
            &texts,
            &|m| texts[m].contains('@') && texts[m].contains("host"),
            &parsed,
            packages,
            &declaration_sources,
            &mut diags,
        );
        diags
    }

    #[test]
    fn paired_host_declarations_match_canonical_trait_paths() {
        let files = sources(include_str!("programs/lib/paired_traits.hd"));
        let diags = declarations(
            &[&files],
            &[metadata(0, &["server.type", "server.Σurface"])],
        );
        assert!(diags.is_empty(), "{}", diags.text);
    }

    #[test]
    fn each_missing_side_names_its_own_trait_and_manifest_location() {
        let marked = sources(&format!(
            "\n{}",
            include_str!("programs/lib/marked_trait.hd")
        ));
        let d = declarations(&[&marked], &[metadata(0, &[])]);
        assert_eq!(d.code, [Code::InvalidManifest]);
        assert_eq!(d.primary[0].lo, 1);
        assert!(d.text.contains("server.Marked"));
        assert!(d.text.contains("/sdk-0/hd.toml:7"));

        let plain = sources(&format!(
            "\n\n{}",
            include_str!("programs/lib/plain_trait.hd")
        ));
        let d = declarations(&[&plain], &[metadata(0, &["server.Plain"])]);
        assert_eq!(d.code, [Code::InvalidManifest]);
        assert_eq!(d.primary[0].lo, 12, "the actual trait name is highlighted");
        assert!(d.text.contains("server.Plain"));
        assert!(d.text.contains("/sdk-0/hd.toml:7"));
        assert!(d.text.contains("server.hd"));
    }

    #[test]
    fn same_named_packages_cannot_supply_each_others_marker() {
        let marked = sources(include_str!("programs/lib/marked_server.hd"));
        let plain = sources(include_str!("programs/lib/plain_server.hd"));
        let d = declarations(
            &[&marked, &plain],
            &[
                metadata(0, &["server.Server"]),
                metadata(1, &["server.Server"]),
            ],
        );
        assert_eq!(d.code, [Code::InvalidManifest]);
        assert!(d.text.contains("/sdk-1/hd.toml:7"));
        assert!(!d.text.contains("/sdk-0/hd.toml:7"));
    }

    #[test]
    fn a_package_named_std_is_not_the_compiler_owned_standard_library() {
        let files = sources(include_str!("programs/lib/marked_server.hd"));
        let table = ModuleTable::discover_all(&[PackageIn {
            name: "std",
            identity: "/local/std",
            sources: &files,
            scope: Scope::All,
            requires: Vec::new(),
            entries: Vec::new(),
            dev_requires: Vec::new(),
        }]);
        let texts = texts_ready(vec![Arc::from(include_str!(
            "programs/lib/marked_server.hd"
        ))]);
        let mut diags = DiagBuf::default();
        let declaration_sources: [&dyn SourceSet; 1] = [&files];
        check_host_declarations(
            &table,
            &texts,
            &|_| true,
            &[OnceLock::new()],
            &[metadata(0, &[])],
            &declaration_sources,
            &mut diags,
        );
        assert_eq!(diags.code, [Code::InvalidManifest]);
    }

    #[test]
    fn imports_and_nontraits_do_not_declare_owned_host_traits() {
        for text in [
            "use std.console.Console as Server\n",
            "pub struct Server: pass\n",
        ] {
            let files = sources(text);
            let d = declarations(&[&files], &[metadata(0, &["server.Server"])]);
            assert_eq!(d.code, [Code::InvalidManifest], "{text}");
            assert!(d.text.contains("server.Server"));
        }
    }

    #[test]
    fn manifest_only_edits_cannot_hide_behind_cached_checks() {
        let mut files = sources(include_str!("programs/lib/marked_server.hd"));
        files.insert("main.hd", "pub fn main() -> void: pass\n");
        let store = crate::support::warm_std_store();
        let host = Host {
            sources: &files,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
            render_tir: &[],
        };
        let mut packages = Packages {
            host_packages: vec![metadata(0, &["server.Server"])],
            ..Packages::default()
        };
        for _ in 0..2 {
            let out = build_packages(&host, "sdk", &packages, &Goal::Analyze);
            assert!(!out.diags.has_errors(), "{}", out.render());
        }
        packages.host_packages[0].declarations = None;
        let out = build_packages(&host, "sdk", &packages, &Goal::Analyze);
        assert_eq!(out.diags.code, [Code::InvalidManifest], "{}", out.render());
        assert!(out.render().contains("server.Server"));
    }
}

#[cfg(feature = "host-artifacts")]
mod package_host_abi_tests {
    use super::*;

    #[test]
    fn typed_contract_glue_and_nominal_versions_survive_warm_links() {
        let mut files = MemorySources::default();
        files.insert("src/main.hd", include_str!("programs/lib/host_abi_main.hd"));
        let store = MemoryStore::default();
        let host = Host {
            sources: &files,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
            render_tir: &[],
        };
        let mut packages = Packages {
            host_packages: vec![HostPackage {
                package: 0,
                identity: "example.org/sdk@1".into(),
                versioned_source: Some("example.org/sdk@1.2.3".into()),
                declarations: Some(hd_project::HostDeclarations {
                    providers: Some(vec![hd_project::HostProvider {
                        trait_path: "main.Server".into(),
                        path: Some("provider.mjs".into()),
                        line: 4,
                    }]),
                    line: 4,
                }),
                manifest: "hd.toml".into(),
            }],
            ..Packages::default()
        };
        let goal = Goal::Program {
            entry: "main".into(),
        };
        let first = build_packages(&host, "app", &packages, &goal);
        assert!(!first.diags.has_errors(), "{}", first.render());
        let wasm = first.wasm.as_ref().expect("SDK program builds");
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(wasm)
            .expect("valid Wasm with nominal imports");
        let artifacts = first
            .host_artifacts
            .as_ref()
            .expect("generated host artifacts");
        assert_eq!(artifacts.contract.interfaces.len(), 1);
        let interface = &artifacts.contract.interfaces[0];
        assert_eq!(interface.identity.package, "example.org/sdk");
        assert_eq!(interface.identity.version, "1.2.3");
        assert_eq!(interface.identity.trait_path, "main.Server");
        assert_eq!(interface.selectors, ["pkg.main.Server"]);
        assert!(interface.module.starts_with("hd:pkg:"));
        assert_eq!(
            interface
                .methods
                .iter()
                .map(|m| (m.name.as_str(), m.suspends))
                .collect::<Vec<_>>(),
            [
                ("ready", false),
                ("secret", false),
                ("custom", false),
                ("wait", true)
            ]
        );
        assert!(
            matches!(&interface.methods[0].params[0].value, hd_host_abi::schema::Value::Record(name, fields) if name == "Message" && fields[0].name == "text" && fields[1].name == "count")
        );
        assert!(
            matches!(&interface.methods[3].result, hd_host_abi::schema::Value::Result(_,error) if matches!(error.as_ref(), hd_host_abi::schema::Value::Enum(name, variants) if name == "Problem" && variants[0].name == "Broken" && variants[0].fields[0].name == "reason"))
        );
        assert_eq!(interface.methods[0].params[0].name, "message");
        assert!(artifacts.javascript.contains("runtime.record"));
        let cached = build_packages(&host, "app", &packages, &goal);
        assert!(!cached.diags.has_errors(), "{}", cached.render());
        assert_eq!(cached.wasm.as_ref(), Some(wasm));
        assert_eq!(
            cached.host_artifacts.as_ref().unwrap().javascript,
            artifacts.javascript
        );
        run_package_host_javascript(
            &first,
            r"
    assert.throws(() => createCoreHost(sink, { module }), /missing package host provider.*main.Server/);
    const implementation = {
      ready(message) { return { text: message.text.toUpperCase(), count: message.count + 1n }; },
      secret(value) {
    assert.equal(typeof value, 'object');
    assert.equal(value.large, 18446744073709551615n);
    assert.equal(value.signed, -9223372036854775808n);
    assert.equal(value.ratio, 1);
    assert.equal(typeof value.ratio, 'number');
    assert.ok(Object.is(value.zero, -0));
    assert.deepEqual(value.children, ['nested']);
    return { ...value, large: value.large - 1n, signed: value.signed + 1n };
      },
      custom(value) { assert.equal(value, 'custom:private'); return 'custom reply'; },
      async wait(context) { assert.equal(context.signal.aborted, false); await new Promise(r => setTimeout(r, 2)); return { ok: true, value: 'from async' }; }
    };
    const root = glue.interfaces.find(i => !i.handle);
    const providers = new Map([[root.module, implementation]]);
    assert.throws(() => createCoreHost(sink, { module, grants: { 'pkg.main.Server': false }, packageBindings: rt => glue.bind(rt, providers) }), /capability denied/);
    assert.throws(() => createCoreHost(sink, { module, packageBindings: rt => glue.bind(rt, new Map([[root.module, Object.create(implementation)]])) }), /plain object/);
    const host = createCoreHost(sink, { module, packageBindings: rt => glue.bind(rt, providers) });
    try {
      const instance = await WebAssembly.instantiate(module, host.imports);
      const result = await host.run(instance, 'hd.init', 'hd.poll');
      assert.equal(result.status, 0);
      assert.equal(output.join(''), 'HELLO 8\n18446744073709551614 -9223372036854775807 4607182418800017408 9223372036854775808 nested\ncustom reply\nfrom async\n');
      assert.equal(errors.join(''), '');
    } finally { host.close(); }
    // Exercise the generated Pending/finish/abort protocol without invoking a
    // guest turn after abort. A late Promise cannot revive the consumed ID.
    let deliver, signal;
    const waiting = {
      ...implementation,
      wait(context) { signal = context.signal; return new Promise(resolve => { deliver = resolve; }); }
     };
    const cancelled = createCoreHost(sink, { module, packageBindings: rt => glue.bind(rt, new Map([[root.module, waiting]])) });
    try {
      const instance = await WebAssembly.instantiate(module, cancelled.imports);
      cancelled.prime(instance);
      const target = cancelled.imports[root.module];
      const [state, id] = target['wait.start']();
      assert.equal(state, 1);
      assert.throws(() => target['wait.finish'](id), /not pending/);
      cancelled.imports['hd:rt'].abort(id);
      assert.equal(signal.aborted, true);
      deliver({ ok: true, value: 'late' });
      await Promise.resolve();
      assert.equal(cancelled.core.st.ops.size, 0);
      assert.throws(() => target['wait.finish'](id), /not pending/);
      const [, next] = target['wait.start']();
      assert.notEqual(next, id);
      deliver({ ok: true, value: 'delivered' });
      await Promise.resolve();
      assert.equal(cancelled.imports['hd:rt'].block(), 1);
      assert.ok(target['wait.finish'](next) > 0);
      assert.throws(() => target['wait.finish'](next), /not pending/);
      target['wait.start']();
    } finally { cancelled.close(); }
    assert.equal(signal.aborted, true);
    assert.equal(cancelled.core.st.ops.size, 0);
    ",
        );
        packages.host_packages[0].versioned_source = Some("example.org/sdk@1.2.4".into());
        let updated = build_packages(&host, "app", &packages, &goal);
        assert!(!updated.diags.has_errors(), "{}", updated.render());
        let new = &updated.host_artifacts.as_ref().unwrap().contract.interfaces[0];
        assert_eq!(new.identity.version, "1.2.4");
        assert_ne!(
            new.module, interface.module,
            "a version-only edit invalidates the cached host ABI"
        );
        assert_ne!(updated.wasm.as_ref(), Some(wasm));
    }
}

#[cfg(feature = "host-artifacts")]
mod package_host_inheritance_tests {
    use super::*;

    #[test]
    fn inherited_concrete_methods_and_disposable_handles_have_typed_nominal_imports() {
        let mut files = MemorySources::default();
        files.insert(
            "src/main.hd",
            include_str!("programs/lib/inherited_host.hd"),
        );
        let store = MemoryStore::default();
        let host = Host {
            sources: &files,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
            render_tir: &[],
        };
        let packages = Packages {
            host_packages: vec![HostPackage {
                package: 0,
                identity: "/local/sdk".into(),
                versioned_source: None,
                declarations: Some(hd_project::HostDeclarations {
                    providers: Some(vec![hd_project::HostProvider {
                        trait_path: "main.Server".into(),
                        path: Some("provider.mjs".into()),
                        line: 4,
                    }]),
                    line: 4,
                }),
                manifest: "hd.toml".into(),
            }],
            ..Packages::default()
        };
        let out = build_packages(
            &host,
            "app",
            &packages,
            &Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(!out.diags.has_errors(), "{}", out.render());
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(out.wasm.as_ref().unwrap())
            .expect("inherited vtables validate");
        let contract = &out.host_artifacts.as_ref().unwrap().contract;
        assert_eq!(contract.interfaces.len(), 2);
        let provider = contract.interfaces.iter().find(|i| !i.handle).unwrap();
        let echo = provider
            .methods
            .iter()
            .find(|m| m.member == "echo")
            .unwrap();
        assert!(echo.name.ends_with("/main.Base::echo"));
        assert_eq!(echo.params.len(), 1);
        assert_eq!(echo.params[0].value, hd_host_abi::schema::Value::String);
        let handle = contract.interfaces.iter().find(|i| i.handle).unwrap();
        assert!(
            handle
                .methods
                .iter()
                .any(|m| m.member == "next" && m.suspends)
        );
        assert!(
            handle
                .methods
                .iter()
                .any(|m| m.member == "dispose" && m.name.ends_with("/ops.AsyncDisposable::dispose"))
        );
        assert_ne!(provider.module, handle.module);
        assert!(
            contract
                .interfaces
                .iter()
                .all(|i| !i.identity.package.contains("/local/"))
        );
        run_package_host_javascript(
            &out,
            r"
    let disposed = 0, runtime;
    const provider = {
      echo(text) { return text; },
      async open() { return { async next() { return { ok: true, value: 'event' }; }, async dispose() { disposed++; } }; }
     };
    const root = glue.interfaces.find(i => !i.handle);
    const host = createCoreHost(sink, { module, packageBindings: rt => { runtime = rt; return glue.bind(rt, new Map([[root.module, provider]])); } });
    try {
      const instance = await WebAssembly.instantiate(module, host.imports);
      assert.ok(WebAssembly.Module.imports(module).some(i => i.name === '$release'));
      const result = await host.run(instance, 'hd.init', 'hd.poll');
      assert.equal(result.status, 0);
      assert.equal(output.join(''), 'inherited\nevent\n');
      assert.equal(errors.join(''), '');
      assert.equal(disposed, 1);
      assert.equal(runtime.handleCount(), 0, 'last HD reference drops the JS binding');
    } finally { host.close(); }
    ",
        );
    }
}

#[cfg(feature = "host-artifacts")]
mod package_host_reexport_tests {
    use super::*;

    #[test]
    fn a_transitive_sdk_keeps_its_identity_through_a_public_dependency_alias() {
        let mut app = MemorySources::default();
        app.insert("main.hd", include_str!("programs/lib/alias_app.hd"));
        let mut wrapper = MemorySources::default();
        wrapper.insert("src/lib.hd", "pub use dep.real.{Server as Service}\n");
        let mut sdk = MemorySources::default();
        sdk.insert("src/lib.hd", include_str!("programs/lib/sdk_server.hd"));
        let store = MemoryStore::default();
        let host = Host {
            sources: &app,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
            render_tir: &[],
        };
        let packages = Packages {
            requires: vec![("wrap".into(), 1)],
            deps: vec![
                Dependency {
                    name: "wrapper".into(),
                    identity: "/local/wrapper".into(),
                    sources: &wrapper,
                    requires: vec![("real".into(), 2)],
                    fetched: false,
                },
                Dependency {
                    name: "sdk".into(),
                    identity: "/local/sdk".into(),
                    sources: &sdk,
                    requires: vec![],
                    fetched: false,
                },
            ],
            host_packages: vec![HostPackage {
                package: 2,
                identity: "/local/sdk".into(),
                versioned_source: Some("example.org/sdk@1.2.3".into()),
                declarations: Some(hd_project::HostDeclarations {
                    providers: Some(vec![hd_project::HostProvider {
                        trait_path: "Server".into(),
                        path: Some("provider.mjs".into()),
                        line: 4,
                    }]),
                    line: 4,
                }),
                manifest: "sdk/hd.toml".into(),
            }],
            ..Packages::default()
        };
        let out = build_packages(
            &host,
            "app",
            &packages,
            &Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(!out.diags.has_errors(), "{}", out.render());
        let c = &out.host_artifacts.as_ref().unwrap().contract;
        assert_eq!(c.interfaces.len(), 1);
        assert_eq!(c.interfaces[0].identity.package, "example.org/sdk");
        assert_eq!(c.interfaces[0].identity.trait_path, "Server");
        assert_eq!(c.interfaces[0].selectors, ["dep.wrap.Service"]);
    }
}

mod main_capability_tests {
    use super::*;

    #[test]
    fn unused_entry_capabilities_are_available_on_cold_and_warm_checks() {
        let source = include_str!("programs/lib/process_main.hd");
        let mut files = MemorySources::default();
        files.insert("src/main.hd", source);
        let store = crate::support::warm_std_store();
        let host = Host {
            sources: &files,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
            render_tir: &[],
        };
        let packages = Packages {
            entry_capabilities: true,
            entries: vec!["main".to_owned()],
            ..Packages::default()
        };
        for warm in [false, true] {
            let out = build_packages(&host, "app", &packages, &Goal::Analyze);
            assert!(!out.diags.has_errors(), "{}", out.render());
            assert_eq!(out.entry_capabilities.len(), 1);
            let need = &out.entry_capabilities[0];
            assert_eq!(need.selectors, ["Process"]);
            assert!(source[need.span.lo as usize..need.span.hi as usize].contains("Process"));
            if warm {
                assert!(out.counters.modules_checked.is_empty());
            }
        }
    }

    #[test]
    fn test_capability_rows_are_available_on_cold_and_warm_checks() {
        let mut files = MemorySources::default();
        files.insert("src/lib.hd", "pub fn value() -> i32: +1\n");
        files.insert("tests/one.hd", include_str!("programs/lib/console_test.hd"));
        let store = crate::support::warm_std_store();
        let host = Host {
            sources: &files,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
            render_tir: &[],
        };
        let packages = Packages {
            entry_capabilities: true,
            ..Packages::default()
        };
        for _ in 0..2 {
            let out = build_packages(&host, "app", &packages, &Goal::CheckTests);
            assert!(!out.diags.has_errors(), "{}", out.render());
            assert_eq!(
                out.entry_capabilities.len(),
                1,
                "{:?}",
                out.entry_capabilities
            );
            let need = &out.entry_capabilities[0];
            assert_eq!(need.selectors, ["Console"]);
            assert_eq!(need.entry, CapabilityEntry::Test("needs console".into()));
            assert!(!need.written_row);
            assert_eq!(out.files[need.span.file.idx()], "tests/one.hd");
            assert_eq!(
                &out.texts[need.span.file.idx()][need.span.lo as usize..need.span.hi as usize],
                "it"
            );
        }
    }

    #[test]
    fn task_scripts_have_development_capability_rows() {
        let mut files = MemorySources::default();
        files.insert("tasks/report.hd", "println(\"report\")\n");
        let store = crate::support::warm_std_store();
        let host = Host {
            sources: &files,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
            render_tir: &[],
        };
        let packages = Packages {
            entry_capabilities: true,
            ..Packages::default()
        };
        let out = build_packages(
            &host,
            "app",
            &packages,
            &Goal::Program {
                entry: "$tasks.report".into(),
            },
        );
        assert!(!out.diags.has_errors(), "{}", out.render());
        assert_eq!(
            out.entry_capabilities.len(),
            1,
            "{:?}",
            out.entry_capabilities
        );
        assert_eq!(out.entry_capabilities[0].selectors, ["Console"]);
        assert_eq!(out.entry_capabilities[0].entry, CapabilityEntry::Task);
    }

    #[test]
    fn task_init_requirements_are_script_only_on_cold_and_warm_checks() {
        for main in ["", "\npub fn main() -> void: pass\n"] {
            let source = format!("println(\"unlisted\")\n{main}");
            let mut files = MemorySources::default();
            files.insert("tasks/probe.hd", source.as_str());
            let store = crate::support::warm_std_store();
            let host = Host {
                sources: &files,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
                render_tir: &[],
            };
            let packages = Packages {
                entry_capabilities: true,
                ..Packages::default()
            };
            for warm in [false, true] {
                let out = build_packages(&host, "app", &packages, &Goal::Analyze);
                if !main.is_empty() {
                    assert_eq!(
                        out.diags.code,
                        [Code::MissingRequirement],
                        "{}",
                        out.render()
                    );
                    continue;
                }
                assert!(!out.diags.has_errors(), "{}", out.render());
                assert_eq!(
                    out.entry_capabilities.len(),
                    1,
                    "{:?}",
                    out.entry_capabilities
                );
                let need = &out.entry_capabilities[0];
                assert_eq!(need.selectors, ["Console"]);
                assert_eq!(need.entry, CapabilityEntry::Task);
                assert!(!need.written_row);
                assert_eq!(need.span.lo, 0);
                assert_eq!(out.files[need.span.file.idx()], "tasks/probe.hd");
                if warm {
                    assert!(out.counters.modules_checked.is_empty());
                }
            }
        }
    }

    #[test]
    fn task_script_provider_reads_distinguish_local_and_host_grants() {
        let local = include_str!("programs/lib/clock_with_local.hd");
        let spread = include_str!("programs/lib/clock_with_spread.hd");
        let trait_method = include_str!("programs/lib/clock_trait_method.hd");
        for (source, wants_clock) in [
            (trait_method.to_owned(), true),
            (spread.to_owned(), false),
            (format!("{spread}\n_ := now()\n"), true),
            (local.to_owned(), false),
            (format!("{local}\n_ := now()\n"), true),
            (include_str!("programs/lib/clock_use.hd").to_owned(), true),
            (
                include_str!("programs/lib/clock_action_called.hd").to_owned(),
                true,
            ),
            (
                include_str!("programs/lib/clock_action_unused.hd").to_owned(),
                false,
            ),
        ] {
            let mut files = MemorySources::default();
            files.insert("tasks/report.hd", &source);
            let store = crate::support::warm_std_store();
            let host = Host {
                sources: &files,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
                render_tir: &[],
            };
            let packages = Packages {
                entry_capabilities: true,
                ..Packages::default()
            };
            for _ in 0..2 {
                let out = build_packages(&host, "app", &packages, &Goal::Analyze);
                assert!(!out.diags.has_errors(), "{}", out.render());
                assert_eq!(
                    out.entry_capabilities.len(),
                    usize::from(wants_clock),
                    "{source}: {:?}",
                    out.entry_capabilities
                );
                if wants_clock {
                    assert_eq!(out.entry_capabilities[0].selectors, ["Clock"]);
                }
            }
        }
    }

    #[test]
    fn private_main_rows_still_require_manifest_capabilities() {
        let mut files = MemorySources::default();
        files.insert("src/main.hd", include_str!("programs/lib/private_main.hd"));
        let store = crate::support::warm_std_store();
        let host = Host {
            sources: &files,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
            render_tir: &[],
        };
        let packages = Packages {
            entry_capabilities: true,
            entries: vec!["main".to_owned()],
            ..Packages::default()
        };
        for _ in 0..2 {
            let out = build_packages(&host, "app", &packages, &Goal::Analyze);
            assert!(!out.diags.has_errors(), "{}", out.render());
            assert_eq!(out.entry_capabilities.len(), 1);
            assert_eq!(out.entry_capabilities[0].selectors, ["Process"]);
        }
    }

    #[test]
    fn a_package_trait_named_process_is_not_the_builtin_capability() {
        let mut files = MemorySources::default();
        files.insert(
            "src/main.hd",
            include_str!("programs/lib/process_trait_main.hd"),
        );
        let store = crate::support::warm_std_store();
        let host = Host {
            sources: &files,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
            render_tir: &[],
        };
        let declarations =
            hd_project::parse_manifest("[host]\n\"main.Process\" = \"provider.mjs\"\n")
                .unwrap()
                .host;
        let packages = Packages {
            entry_capabilities: true,
            entries: vec!["main".to_owned()],
            host_packages: vec![HostPackage {
                package: 0,
                identity: "app".to_owned(),
                versioned_source: None,
                declarations,
                manifest: "hd.toml".to_owned(),
            }],
            ..Packages::default()
        };
        let out = build_packages(&host, "app", &packages, &Goal::Analyze);
        assert!(!out.diags.has_errors(), "{}", out.render());
        assert_eq!(out.entry_capabilities.len(), 1);
        assert_eq!(out.entry_capabilities[0].selectors, ["pkg.main.Process"]);
    }

    #[test]
    fn library_rows_do_not_become_entry_capabilities() {
        let mut files = MemorySources::default();
        files.insert("src/lib.hd", include_str!("programs/lib/library_main.hd"));
        let store = crate::support::warm_std_store();
        let host = Host {
            sources: &files,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
            render_tir: &[],
        };
        let packages = Packages {
            entry_capabilities: true,
            ..Packages::default()
        };
        let out = build_packages(&host, "app", &packages, &Goal::Analyze);
        assert!(!out.diags.has_errors(), "{}", out.render());
        assert!(out.entry_capabilities.is_empty());
    }
}

mod verification_tests {
    use super::verifies_tir;
    use hd_base::DefId;
    use hd_tir::ir::{BodyKind, TirBuilder, TirSink};

    #[test]
    fn checked_body_verification_rejects_corruption_in_debug_checked_and_ci() {
        assert!(!verifies_tir(false, false, false));
        let mut ordinary = TirBuilder::new(DefId::NONE, BodyKind::Fn).with_verification(false);
        let root = ordinary.reserve().as_ref();
        assert_eq!(ordinary.finish(root, &[]).is_ok(), !cfg!(debug_assertions));
        for (debug, checked, requested) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            // An unfilled reserved slot is malformed compiler IR. Exercise
            // CI's request independently of the compiler's debug assertions.
            let enabled = verifies_tir(debug, checked, requested);
            let mut builder = TirBuilder::new(DefId::NONE, BodyKind::Fn).with_verification(enabled);
            let root = builder.reserve().as_ref();
            assert!(
                matches!(builder.finish(root, &[]), Err(errors) if errors.iter().any(|error| error.invariant == 14))
            );
        }
    }
}

mod instantiation_chain_tests {
    use super::*;

    /// Each generic instance's body is built once per run
    /// (`hd_mono::instantiate`), however many calls reach it, in dev and
    /// release: `n` types, each with two calls of a never-inlined generic
    /// function, build `n - 1` more instances than one type does, at n
    /// and 10n.
    #[test]
    fn each_generic_instance_is_built_once() {
        use std::fmt::Write as _;
        for (name, packages) in [
            ("dev", Packages::default()),
            (
                "release",
                Packages {
                    level: OptLevel::Optimized,
                    build_profile: BuildProfile::Release,
                    ..Packages::default()
                },
            ),
        ] {
            let built = |n: usize| {
                let mut source = String::from(
                    "use std.meta.inline\n\
                     @inline(.Never)\n\
                     fn depth[T](x: T, k: i32) -> i32:\n    \
                     if k == 0: 0 else: depth(x, k - 1) + 1\n",
                );
                let mut main = String::from("pub fn main() -> void $ Console:\n");
                for i in 0..n {
                    writeln!(source, "struct S{i}:\n    v: i32").unwrap();
                    for _ in 0..2 {
                        writeln!(main, "    println(depth(S{i} {{ v: {i} }}, 2))").unwrap();
                    }
                }
                source.push_str(&main);
                let mut sources = MemorySources::default();
                sources.insert("main.hd", &source);
                let store = warm_std_store();
                let host = Host {
                    render_tir: &[],
                    sources: &sources,
                    store: &store,
                    clock: &NoClock,
                    executor: Executor::Serial(hd_sched::SerialOrder::Priority),
                };
                let out = build_packages(
                    &host,
                    "instances_once",
                    &packages,
                    &Goal::Program {
                        entry: "main".into(),
                    },
                );
                assert!(!out.diags.has_errors(), "{}", out.render());
                assert!(out.wasm.is_some());
                out.counters.instances_built
            };
            // One type's run builds `depth` at it plus what `println`
            // needs; each further type adds `depth` at it alone.
            let base = built(1);
            for n in [10, 100] {
                assert_eq!(built(n) - base, n - 1, "{name}, n = {n}");
            }
        }
    }

    /// A release build owns one level of strongly connected components
    /// at a time, and cleans up the linked functions and puts their
    /// count calls behind tests, on the pool's threads (#2383): `bench/json-decode`, with recursive components
    /// and generic instances, links to the same bytes on one thread and
    /// on eight.
    #[test]
    fn release_ownership_bytes_match_at_one_and_eight_threads() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/json-decode");
        let mut sources = MemorySources::default();
        for file in ["main.hd", "types20.hd"] {
            sources.insert(
                file,
                &std::fs::read_to_string(root.join(file)).expect("bench source"),
            );
        }
        let packages = Packages {
            level: OptLevel::Optimized,
            build_profile: BuildProfile::Release,
            ..Packages::default()
        };
        let wasm = |executor| {
            let store = MemoryStore::default();
            let host = Host {
                render_tir: &[],
                sources: &sources,
                store: &store,
                clock: &NoClock,
                executor,
            };
            let goal = Goal::Program {
                entry: "main".into(),
            };
            let out = build_packages(&host, "json_decode_bench", &packages, &goal);
            assert!(!out.diags.has_errors(), "{}", out.render());
            out.wasm.expect("wasm")
        };
        let one = wasm(Executor::Serial(SerialOrder::Priority));
        assert_eq!(wasm(Executor::Pool(8)), one, "eight threads");
        assert_eq!(wasm(Executor::Pool(1)), one, "one pool thread");
    }

    /// A generic local function is lifted (`hd_tir::ir::lift`, #2180):
    /// each distinct type arguments make one instance of its enclosing
    /// item, which emits that sub-body. `n` types, each instantiating it
    /// twice, build `n` more instances than one type does, at n and 10n,
    /// in dev and release. In release each call inlines its function's
    /// copy, built once per instance (`InlineEnv::lifted_body`) and not
    /// once per call.
    #[test]
    fn each_local_instantiation_is_one_instance() {
        use std::fmt::Write as _;
        for (name, packages) in [
            ("dev", Packages::default()),
            (
                "release",
                Packages {
                    level: OptLevel::Optimized,
                    build_profile: BuildProfile::Release,
                    ..Packages::default()
                },
            ),
        ] {
            let made = |n: usize| {
                let mut source = String::new();
                let mut main = String::from(
                    "pub fn main() -> void $ Console:\n    \
                     fn tag[S](s: S, k: i32) -> i32:\n        k\n",
                );
                for i in 0..n {
                    writeln!(source, "struct S{i}:\n    v: i32").unwrap();
                    for _ in 0..2 {
                        writeln!(main, "    println(tag(S{i} {{ v: {i} }}, {i}))").unwrap();
                    }
                }
                source.push_str(&main);
                let mut sources = MemorySources::default();
                sources.insert("main.hd", &source);
                let store = warm_std_store();
                let host = Host {
                    render_tir: &[],
                    sources: &sources,
                    store: &store,
                    clock: &NoClock,
                    executor: Executor::Serial(hd_sched::SerialOrder::Priority),
                };
                let out = build_packages(
                    &host,
                    "local_instances_once",
                    &packages,
                    &Goal::Program {
                        entry: "main".into(),
                    },
                );
                assert!(!out.diags.has_errors(), "{}", out.render());
                assert!(out.wasm.is_some());
                out.counters.instances_built
            };
            let base = made(1);
            for n in [10, 100] {
                assert_eq!(made(n) - base, n - 1, "{name}, n = {n}");
            }
        }
    }

    #[test]
    fn pathological_optimized_chains_have_linear_callee_preparation_work() {
        let mut measured = Vec::new();
        for count in [1000, 10000] {
            let mut source = String::new();
            for i in 0..count {
                use std::fmt::Write as _;
                let tail = if i + 1 == count {
                    "x".to_owned()
                } else {
                    format!("g_{}(x)", i + 1)
                };
                writeln!(source, "fn g_{i}(x: i32) -> i32: {tail}").unwrap();
            }
            source.push_str("pub fn main() -> void $ Console: println(g_0(7))\n");
            let mut sources = MemorySources::default();
            sources.insert("main.hd", &source);
            let store = warm_std_store();
            let host = Host {
                render_tir: &[],
                sources: &sources,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(hd_sched::SerialOrder::Priority),
            };
            let out = build_packages(
                &host,
                "inline_chain",
                &Packages {
                    level: OptLevel::Optimized,
                    build_profile: BuildProfile::Release,
                    ..Packages::default()
                },
                &Goal::Program {
                    entry: "main".into(),
                },
            );
            assert!(!out.diags.has_errors(), "{}", out.render());
            assert!(out.wasm.is_some());
            let work = out.counters.inline_work;
            eprintln!("inliner chain {count}: {work:?}");
            assert!(work.preparations >= count, "{work:?}");
            assert!(work.preparations <= count + 1000, "{work:?}");
            assert!(work.polls <= 3 * work.preparations + 1000, "{work:?}");
            assert!(work.rows <= 32 * count + 10_000, "{work:?}");
            assert!(work.ancestor_queries <= 4 * count + 10_000, "{work:?}");
            assert_eq!(work.depth, count);
            measured.push(work);
        }
        assert!(measured[1].preparations <= 11 * measured[0].preparations);
        assert!(measured[1].polls <= 11 * measured[0].polls);
        assert!(measured[1].rows <= 11 * measured[0].rows);
        assert!(measured[1].ancestor_queries <= 11 * measured[0].ancestor_queries);
    }

    #[test]
    fn pathological_wide_deep_inlining_shares_prepared_callees() {
        let mut measured = Vec::new();
        for width in [100, 1000] {
            let depth = 20;
            let mut source = String::new();
            for branch in 0..width {
                for level in 0..depth {
                    use std::fmt::Write as _;
                    let tail = if level + 1 == depth {
                        "x".to_owned()
                    } else {
                        format!("g_{branch}_{}(x)", level + 1)
                    };
                    writeln!(source, "fn g_{branch}_{level}(x: i32) -> i32: {tail}").unwrap();
                }
            }
            source.push_str("pub fn main() -> void $ Console:\n    let x = 7i32\n");
            for branch in 0..width {
                use std::fmt::Write as _;
                writeln!(source, "    x = g_{branch}_0(x)").unwrap();
            }
            source.push_str("    println(x)\n");
            let mut sources = MemorySources::default();
            sources.insert("main.hd", &source);
            let store = warm_std_store();
            let host = Host {
                render_tir: &[],
                sources: &sources,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(hd_sched::SerialOrder::Priority),
            };
            let out = build_packages(
                &host,
                "inline_tree",
                &Packages {
                    level: OptLevel::Optimized,
                    build_profile: BuildProfile::Release,
                    ..Packages::default()
                },
                &Goal::Program {
                    entry: "main".into(),
                },
            );
            assert!(!out.diags.has_errors(), "{}", out.render());
            assert!(out.wasm.is_some());
            let work = out.counters.inline_work;
            eprintln!("inliner {width}x{depth}: {work:?}");
            let nodes = width * depth;
            assert!(work.preparations <= nodes + 1000, "{work:?}");
            assert!(work.rows <= 32 * nodes + 10_000, "{work:?}");
            assert!(work.ancestor_queries <= 4 * nodes + 10_000, "{work:?}");
            assert_eq!(work.depth, depth);
            measured.push(work);
        }
        assert!(measured[1].preparations <= 11 * measured[0].preparations);
        assert!(measured[1].polls <= 11 * measured[0].polls);
        assert!(measured[1].rows <= 11 * measured[0].rows);
        assert!(measured[1].ancestor_queries <= 11 * measured[0].ancestor_queries);
    }

    #[test]
    fn pathological_plain_chains_build_with_linear_instance_body_work() {
        let mut counts = Vec::new();
        for count in [1000, 10000] {
            let mut source = String::new();
            for i in 0..count {
                use std::fmt::Write as _;
                if i + 1 == count {
                    writeln!(source, "fn g_{i}(x: i32) -> i32: x").unwrap();
                } else {
                    writeln!(source, "fn g_{i}(x: i32) -> i32: g_{}(x)", i + 1).unwrap();
                }
            }
            source.push_str("pub fn main() -> void $ Console: println(g_0(7))\n");
            let mut sources = MemorySources::default();
            sources.insert("main.hd", &source);
            let store = warm_std_store();
            let host = Host {
                render_tir: &[],
                sources: &sources,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(hd_sched::SerialOrder::Priority),
            };
            let out = build_packages(
                &host,
                "chain",
                &Packages::default(),
                &Goal::Program {
                    entry: "main".into(),
                },
            );
            assert!(!out.diags.has_errors(), "{}", out.render());
            assert!(out.wasm.is_some());
            let work = out.counters.bodies_decoded;
            assert!(
                (count..=count + 1000).contains(&work),
                "{count} calls: {work} decoded bodies"
            );
            counts.push(work);
        }
        assert!(counts[1] <= 11 * counts[0], "decoded bodies: {counts:?}");
    }
}

mod secondary_label_codec_tests {
    use super::*;

    #[test]
    fn header_and_module_label_transport_keeps_anchors() {
        let labels = vec![(
            "app.other".to_owned(),
            hd_resolve::anchor::Anchor {
                decl: 2,
                member: 1,
                tok: 3,
                count: 4,
            },
            "other implementation".to_owned(),
        )];
        let (got, clean) = labels_round_trip(&labels);
        assert!(clean);
        assert_eq!(got.len(), labels.len());
        assert_eq!(got[0].0, labels[0].0);
        assert_eq!(got[0].1, labels[0].1);
        assert_eq!(got[0].2, labels[0].2);
    }

    #[test]
    fn missing_overlap_endpoint_leaves_graph_uncached() {
        let found = [(DefId::from_raw(1), DefId::from_raw(2), "overlap".into())];
        assert!(encode_overlaps(&found).is_none());
    }

    #[test]
    fn coherence_rows_keep_both_impl_identities() {
        let found = vec![("app/first".into(), "app/second".into(), "overlap".into())];
        let bytes = encode_coh_rows(vec![(Hash128(1), "app.Show".into(), found.clone())]);
        let got = decode_coh_rows(&bytes).expect("coherence rows");
        assert_eq!(got[0].2, found);
        assert!(decode_coh_rows(&bytes[..bytes.len() - 1]).is_none());
    }
}

mod privacy_key_tests {
    use super::{Executor, Goal, Host, MemorySources, NoClock, Packages, build_packages};

    /// `module.vis.parent`: whether a nested module is visible is a fact of
    /// its parent's text, which the user's own unit does not depend on. A
    /// warm run after the parent adds or drops `pub use pkg.shop.{ledger}`
    /// must still give the new verdict: the unit's keys hash the
    /// publication facts its uses read.
    #[test]
    fn a_parent_reexport_edit_changes_a_warm_runs_verdict() {
        let store = super::warm_std_store();
        let mut sources = MemorySources::default();
        sources.insert("main.hd", include_str!("programs/privacy/main.hd"));
        sources.insert("shop/ledger.hd", include_str!("programs/privacy/ledger.hd"));
        let mut check = |shop: &str| {
            sources.insert("shop/mod.hd", shop);
            let host = Host {
                render_tir: &[],
                sources: &sources,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(hd_sched::SerialOrder::Priority),
            };
            let goal = Goal::Program {
                entry: "main".into(),
            };
            build_packages(&host, "app", &Packages::default(), &goal).render()
        };
        let private = include_str!("programs/privacy/shop_private.hd");
        let public = include_str!("programs/privacy/shop_public.hd");
        assert!(
            check(private).contains("private-module"),
            "cold: shop hides ledger"
        );
        let shown = check(public);
        assert!(
            !shown.contains("private-module"),
            "warm: shop re-exports ledger\n{shown}"
        );
        assert!(
            check(private).contains("private-module"),
            "warm: hidden again"
        );
    }
}

mod std_runtime_boundary_tests {
    use hd_project::SourceSet as _;

    /// A std module outside `std.rt` names only `std.rt`, never a child of
    /// it: a use makes its edge to the module it names
    /// (`module.cycle.edge`), so no module outside the runtime has an edge
    /// into `rt.memory` or `rt.bytes`, and the runtime's own loops stay
    /// inside it (#2355). `std.rt` re-exports what the rest of std uses.
    #[test]
    fn std_outside_the_runtime_names_only_std_rt() {
        let std = hd_driver::std_sources();
        let mut found = Vec::new();
        for entry in std.list().iter().filter(|e| !e.path.starts_with("rt/")) {
            let text = std.read(&entry.path).expect("listed file");
            let text = String::from_utf8_lossy(&text);
            for (n, line) in text.lines().enumerate() {
                let code = line.trim_start().trim_start_matches("pub ").trim_start();
                let named_child = code
                    .strip_prefix("use std.rt.")
                    .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_lowercase()));
                if named_child {
                    found.push(format!("{}:{}: {line}", entry.path, n + 1));
                }
            }
        }
        assert!(found.is_empty(), "uses past std.rt:\n{}", found.join("\n"));
    }
}
