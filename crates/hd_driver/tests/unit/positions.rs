use hd_driver::testing::positions::*;

mod tests {
    use std::sync::{Arc, OnceLock};

    use hd_base::wire::Writer;
    use hd_base::{DefId, FileId, HashMap, NodeIdx, PathId};
    use hd_intern::{PathKind, PathTable};
    use hd_resolve::Src;
    use hd_syntax::parse;

    use super::*;

    #[test]
    fn anchor_lines_preserve_lf_positions_for_all_syntax_nodes() {
        for text in [
            "",
            "\n\n",
            "fn f() -> i32:\n    +1",
            "# λ\r\n\r\nfn f() -> string:\r\n    \"é\"\r\n",
            "# first\r# second\r\nfn f() -> i32:\r    +1\n",
            "# before\n\nfn f() -> string:\n    \"first\\nsecond\"\n",
        ] {
            let parse = parse(text.as_bytes());
            let syn: Vec<_> = (0..parse.tree.len())
                .map(|n| NodeIdx::from_raw(u32::try_from(n).unwrap()))
                .chain([NodeIdx::NONE, NodeIdx::from_raw(u32::MAX - 1)])
                .collect();
            let src = Src {
                parse: &parse,
                text,
                file: FileId::from_raw(0),
            };
            let expected: Vec<_> = syn
                .iter()
                .map(|&node| {
                    if node == NodeIdx::NONE || node.idx() >= parse.tree.len() {
                        return None;
                    }
                    let lo = src.span(parse.tree.node(node)).lo as usize;
                    Some((
                        "source.hd",
                        u32::try_from(text[..lo].matches('\n').count()).unwrap() + 1,
                    ))
                })
                .collect();
            let line_starts = lf_line_starts(&parse, text);
            assert_eq!(text_line_starts(text), line_starts, "{text:?}");
            let positions = positions_of(
                vec![(
                    "pkg".into(),
                    "pkg".into(),
                    Arc::new(ModuleSource {
                        file: "source.hd".into(),
                        hash: hd_base::hash128(text.as_bytes()),
                        texts: hd_driver::testing::texts_ready(vec![text.into()]),
                        index: 0,
                        check: None,
                        input: Arc::new(ModuleInput::default()),
                        read: OnceLock::from(Some(Module {
                            bodies: [(
                                "pkg/f".into(),
                                Body {
                                    syn: syn.into(),
                                    name: "f".into(),
                                    hidden: false,
                                    fallback: Arc::from([]),
                                    sites: None,
                                },
                            )]
                            .into_iter()
                            .collect(),
                            parse: OnceLock::from(Arc::new(parse)),
                            line_starts: OnceLock::from(line_starts),
                        })),
                    }),
                )],
                HashMap::default(),
            );
            for (inst, expected) in expected.into_iter().enumerate() {
                assert_eq!(
                    positions.line("pkg/f", u32::try_from(inst).unwrap()),
                    expected,
                    "{text:?}"
                );
            }
            assert_eq!(positions.line("pkg/f", u32::MAX), None);
            assert_eq!(positions.line("pkg/missing", 0), None);
        }
    }

    #[test]
    fn retained_sources_match_wire_positions_on_cold_warm_and_comment_edits() {
        use hd_cache::MemoryStore;
        use hd_driver::{Executor, Goal, Host, NoClock, PositionPool, build};
        use hd_project::MemorySources;
        use hd_sched::SerialOrder;

        let original = include_str!("programs/positions/retained_sources.hd");
        let edited = format!("# λ: current source lines\n\n{original}");
        let store = MemoryStore::default();
        let mut pool = PositionPool::default();
        for (round, text) in [original, original, edited.as_str()]
            .into_iter()
            .enumerate()
        {
            let mut sources = MemorySources::default();
            sources.insert("main.hd", text);
            let host = Host {
                render_tir: &[],
                sources: &sources,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            };
            let mut output = build(
                &host,
                "app",
                &Goal::Program {
                    entry: "main".into(),
                },
            );
            assert!(!output.diags.has_errors(), "{}", output.render());
            let reused = output.positions(&mut pool);
            // A warm run's inputs: no bodies and no parse, only the skims'
            // code lines, which the entries' frame sites are relative to.
            let warm = |keep_lines: bool| -> Vec<_> {
                position_inputs(&output)
                    .iter()
                    .map(|i| {
                        Arc::new(ModuleInput {
                            code_lines: i.code_lines.clone().filter(|_| keep_lines),
                            ..ModuleInput::default()
                        })
                    })
                    .collect()
            };
            let (sited, unsited) = (warm(true), warm(false));
            let retained = std::mem::replace(position_inputs_mut(&mut output), sited);
            let wire = output.positions(&mut PositionPool::default());
            *position_inputs_mut(&mut output) = unsited;
            let parsed = output.positions(&mut PositionPool::default());
            *position_inputs_mut(&mut output) = retained;
            let table = hd_run::frames::LineTable::parse(output.wasm.as_ref().expect("Wasm"))
                .expect("line table");
            let anchors: Vec<_> = table
                .functions()
                .into_iter()
                .flat_map(|(_, _, rows)| rows.into_iter().flatten())
                .collect();
            for hd_run::frames::Anchored { item, inst } in &anchors {
                assert_eq!(reused.name(item), wire.name(item));
                assert_eq!(reused.line(item, *inst), wire.line(item, *inst));
                assert_eq!(reused.line(item, *inst), parsed.line(item, *inst));
                assert_eq!(reused.fallback(item, *inst), wire.fallback(item, *inst));
            }
            // The frames' lines read no parse of a module (#2423).
            for (_, _, source) in position_modules(&wire) {
                if let Some(Some(read)) = source.read.get() {
                    assert!(read.parse.get().is_none(), "{} was parsed", source.file);
                }
            }
            for hd_run::frames::Anchored { item, inst } in &anchors {
                assert_eq!(reused.span(item, *inst), wire.span(item, *inst));
            }
            assert!(anchors.len() > 10);
            if round == 0 {
                assert!(
                    position_inputs(&output)
                        .iter()
                        .any(|i| !i.bodies.is_empty())
                );
            }
            for (m, input) in position_inputs(&output).iter().enumerate() {
                if let Some(parse) = &input.parse
                    && let Some((_, _, source)) = position_modules(&reused)
                        .iter()
                        .find(|(p, _, _)| p == &output.modules[m])
                    && let Some(Some(read)) = source.read.get()
                    && let Some(read_parse) = read.parse.get()
                {
                    assert!(Arc::ptr_eq(parse, read_parse));
                }
            }
        }
    }

    /// A body's frame name comes from the item the impl segment names, not
    /// from how the segment spells its head: these segments are not impl
    /// heads at all, and the names still follow the recorded types.
    #[test]
    fn a_frame_name_comes_from_the_record_not_the_segment_spelling() {
        let paths = PathTable::new();
        let module = paths.item("pkg", "m", "x");
        let m = PathId::from_raw(module.raw())
            .get()
            .map(|p| paths.parent(p));
        let m = m.expect("module");
        let def = |parent: PathId, kind: PathKind, seg: &str| {
            let id = paths.intern(parent, kind, seg);
            (id, DefId::from_raw(id.raw()))
        };
        let (generic, generic_impl) = def(m, PathKind::Impl, "??? generic spelling ???");
        let (_, generic_method) = def(generic, PathKind::Member, "remove");
        let (derived, derived_impl) = def(m, PathKind::Impl, "something else #2");
        let (_, derived_method) = def(derived, PathKind::Member, "eq");
        let (inherent, inherent_impl) = def(m, PathKind::Impl, "impl Cart");
        let (_, inherent_method) = def(inherent, PathKind::Member, "total");
        let (_, free) = def(m, PathKind::Item, "run");
        let record = |d: DefId| -> Option<String> {
            [
                (generic_impl, "List"),
                (derived_impl, "Cart"),
                (inherent_impl, "Cart"),
            ]
            .iter()
            .find(|(i, _)| *i == d)
            .map(|(_, t)| (*t).to_owned())
        };
        let items = [generic_method, derived_method, inherent_method, free];
        let mut w = Writer::default();
        write_frame_names(&mut w, &paths, &items, &record, &|d| d == free);
        let names = read_frame_names(&w.bytes).expect("record");
        let of = |d: DefId| names[&super::path_of(&paths, d)].clone();
        assert_eq!(of(generic_method), ("List.remove".to_owned(), false));
        assert_eq!(of(derived_method), ("Cart.eq".to_owned(), false));
        assert_eq!(of(inherent_method), ("Cart.total".to_owned(), false));
        assert_eq!(of(free), ("run".to_owned(), true));
    }

    #[test]
    fn a_test_name_is_quoted_as_a_literal() {
        assert_eq!(quoted("sums prices"), "\"sums prices\"");
        assert_eq!(quoted("a \"b\" \\"), "\"a \\\"b\\\" \\\\\"");
    }
}
