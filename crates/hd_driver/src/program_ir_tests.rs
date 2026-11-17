//! Exercise IR record transformations on bodies emitted by the checker.
use super::*;
use hd_base::PathId;
use hd_tir::ir::{Body, BodyKind, Callee, Providers, Ref, Tag, TirBuilder};

const SOURCE: &str = include_str!("../tests/cases/functions/shared-closure-records.hd");

struct Checked {
    body: Body,
    pool: InternPool,
    paths: PathTable,
    syms: ShardedInterner,
}

fn checked(name: &str) -> Checked {
    checked_source(SOURCE, name)
}

fn checked_source(source: &str, name: &str) -> Checked {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = warm_std_store();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(hd_sched::SerialOrder::Priority),
    };
    let out = build_packages(&host, "ir_program", &Packages::default(), &Goal::Analyze);
    assert!(!out.diags.has_errors(), "{}", out.render());
    let module = out
        .modules
        .iter()
        .position(|m| m == "ir_program.main")
        .expect("main module");
    let entry = out.checks[module].as_ref().expect("checked program entry");
    let (pool, paths, syms) = (
        InternPool::new(),
        PathTable::new(),
        ShardedInterner::default(),
    );
    for (_, range) in body_index(entry).expect("program body index") {
        let body =
            hd_tir::wire::read_body(&entry[range], &pool, &paths, &syms).expect("checker body");
        if paths
            .display(PathId::from_raw(body.item.raw()))
            .to_string()
            .ends_with(&format!("/{name}"))
        {
            assert!(hd_tir::ir::verify_checked(&body).is_empty());
            return Checked {
                body,
                pool,
                paths,
                syms,
            };
        }
    }
    panic!("missing program body {name}");
}

#[test]
fn copied_program_projections_and_shared_cells_keep_checked_metadata() {
    let source = include_str!("../tests/cases/functions/local-closure-copy-metadata.hd");
    for name in ["projected", "shared_counter", "shared_environment"] {
        let Checked { mut body, .. } = checked_source(source, name);
        assert!(
            hd_tir::inline::inline_local_closures(&mut body, 64, &|_, _, _| true)
                .expect("copy the real program's local call"),
            "{name}"
        );
        // Captured shared cells introduce Borrow reads and anchors during
        // copying, so the output uses the ownership-stage representation.
        let errors = hd_tir::ir::verify_checked(&body)
            .into_iter()
            .filter(|error| error.invariant != 22)
            .collect::<Vec<_>>();
        assert!(errors.is_empty(), "{name}: {errors:?}");
        hd_tir::inline::inline_local_closures(&mut body, 64, &|_, _, _| true)
            .expect("revisit the retained shared-cell facts");
        let errors = hd_tir::ir::verify_checked(&body)
            .into_iter()
            .filter(|error| error.invariant != 22)
            .collect::<Vec<_>>();
        assert!(errors.is_empty(), "{name}: {errors:?}");
    }
}

#[test]
fn a_shared_capture_of_an_immutable_callback_keeps_its_environment_live() {
    use hd_tir::ir::CaptureMode;
    let Checked { mut body, .. } = checked_source(
        include_str!("../tests/cases/functions/closure-copy-refusal-paths.hd"),
        "nested",
    );
    let closures = body
        .tags
        .iter()
        .enumerate()
        .filter_map(|(i, &tag)| (tag == Tag::Closure).then_some(i))
        .collect::<Vec<_>>();
    assert_eq!(closures.len(), 2);
    let captures = body.record(body.data[closures[1]][1]);
    assert_eq!(captures[1], 1);
    let capture = captures[0] as usize;
    // Sharing the immutable callback cell is equivalent to copying its value.
    // Retain that logical alias after the immediately-called closure is erased.
    body.cap_mode[capture] = CaptureMode::Shared;
    assert!(hd_tir::ir::verify_checked(&body).is_empty());
    assert!(
        hd_tir::inline::inline_local_closures(&mut body, 64, &|_, _, _| true)
            .expect("copy the callback's immediate consumer")
    );
    assert!(!body.inlined_shared.is_empty());
    hd_tir::inline::inline_local_closures(&mut body, 64, &|_, _, _| true)
        .expect("respect the surviving logical capture");
    assert_eq!(
        body.tags.iter().filter(|&&tag| tag == Tag::Closure).count(),
        1
    );
    let errors = hd_tir::ir::verify_checked(&body)
        .into_iter()
        .filter(|error| error.invariant != 22)
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:?}");
}

fn record(body: &mut Body, words: &[u32]) -> u32 {
    let at = u32::try_from(body.extra.len()).expect("small test record");
    body.extra
        .push(u32::try_from(words.len()).expect("small test record"));
    body.extra.extend_from_slice(words);
    at
}

#[test]
fn a_shared_read_with_another_owner_does_not_use_the_returned_loan_abi() {
    use hd_tir::ir::{NONE, Topology};
    use hd_tir::own::{BatchInsert, insert_batch};
    let Checked { mut body, pool, .. } = checked_source(
        include_str!("../tests/cases/counts/returned-call-owner-handoffs.hd"),
        "identity",
    );
    assert_eq!(hd_tir::own::return_loan::parameter(&body), Some(0));
    let root = body.sub_root[0] as usize;
    let value = body.data[root][1];
    // CSE may share the return's immutable read with an otherwise discarded
    // tuple. That extra owner requires the ordinary result convention.
    let args = record(&mut body, &[value]);
    let tuple = pool.intern_ty(&TyData::Tuple {
        elems: pool.list(&[Ty::STRING]),
        rest: None,
    });
    let at = BatchInsert {
        block: u32::try_from(root).expect("root"),
        pos: u32::try_from(body.record(body.data[root][0]).len()).expect("members"),
        tag: Tag::NewTuple,
        data: [NONE, args],
        ty: tuple,
        syn: body.syn[root],
    };
    let topo = Topology::new(&body);
    insert_batch(&mut body, &topo, &[at]).expect("append the shared-read tuple");
    let errors = hd_tir::ir::verify_checked(&body);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(hd_tir::own::return_loan::parameter(&body), None);
}

#[test]
fn call_records_preserve_program_arguments_across_provider_forms_and_bad_headers() {
    let Checked {
        mut body,
        pool,
        paths,
        syms,
    } = checked("projected");
    let call = body
        .tags
        .iter()
        .position(|t| *t == Tag::Call)
        .expect("program call");
    let args = body.call_args(call).to_vec();
    assert_eq!(args.len(), 1);
    let Providers::Pairs(pairs) = body.call_providers(call) else {
        panic!("checked supplier projection")
    };
    let provider = body.extra[pairs.start as usize + 1];
    let bytes = hd_tir::wire::write_body(&body, &pool, &paths, &syms).expect("projected wire");
    assert!(hd_tir::wire::read_body(&bytes, &pool, &paths, &syms).is_some());

    for providers in [
        Providers::None,
        Providers::Context(Ref(provider)),
        Providers::Pairs(pairs),
    ] {
        assert!(body.set_call_providers(call, providers));
        assert_eq!(body.call_args(call), args);
        assert_eq!(body.call_providers(call), providers);
        assert!(hd_tir::ir::verify_checked(&body).is_empty());
        // The constructor and mutator use the same footer representation.
        let mut builder = TirBuilder::new(body.item, BodyKind::Fn);
        *builder.body_mut() = body.clone();
        let words = args.iter().copied().map(Ref).collect::<Vec<_>>();
        let at = builder.call_args_record(&words, providers);
        builder.body_mut().data[call][1] = at;
        assert_eq!(builder.body_mut().call_args(call), args);
        assert_eq!(builder.body_mut().call_providers(call), providers);
    }
    assert!(body.call_args(body.len()).is_empty());
    assert!(!body.set_call_providers(body.len(), Providers::None));

    // Negative wire/record cases start with the real call's operands.
    let mut host = body.clone();
    host.tags[call] = Tag::CallHost;
    host.data[call][1] = record(&mut host, &args);
    assert_eq!(host.call_args(call), args);
    assert!(
        hd_tir::ir::verify_checked(&host)
            .iter()
            .any(|e| e.invariant == 9)
    );

    let target = match Callee::from_words(body.record(body.data[call][0])).expect("real callee") {
        Callee::Item { def, .. } => def.raw(),
        _ => panic!("program uses an item call"),
    };
    let mut dynamic = body.clone();
    dynamic.tags[call] = Tag::CallDyn;
    let mut words = vec![target, TyList::EMPTY.0, 0];
    words.extend_from_slice(&args);
    words.extend_from_slice(&[0, 0, 0]);
    dynamic.data[call][1] = record(&mut dynamic, &words);
    assert_eq!(dynamic.call_args(call), args);
    let lists = std::cell::Cell::new(0);
    hd_tir::ir::map_body_types(&mut dynamic, &|t| t, &|list| {
        lists.set(lists.get() + 1);
        list
    });
    assert!(lists.get() > 0);
    for words in [&[0, 0, 0][..], &[target, 0, u32::MAX, 0, 0, 0][..]] {
        dynamic.data[call][1] = record(&mut dynamic, words);
        assert!(dynamic.call_args(call).is_empty());
    }
}

#[test]
fn interning_live_capture_records_preserves_them_when_another_closure_is_copied() {
    let Checked {
        mut body,
        pool,
        paths,
        syms,
    } = checked("make");
    let closures = body
        .tags
        .iter()
        .enumerate()
        .filter_map(|(i, t)| (*t == Tag::Closure).then_some(i))
        .collect::<Vec<_>>();
    assert_eq!(closures.len(), 3);
    let descriptors = closures
        .iter()
        .map(|&i| body.record(body.data[i][1]).to_vec())
        .collect::<Vec<_>>();
    let [first, second, _] = descriptors.as_slice() else {
        panic!("three closure records")
    };
    let range = |r: &[u32]| r[0] as usize..(r[0] + r[1]) as usize;
    assert_eq!(body.cap_local[range(first)], body.cap_local[range(second)]);
    assert_eq!(body.cap_mode[range(first)], body.cap_mode[range(second)]);
    // CSE may also share an operand record for two equivalent additions.
    // The captured seed is immutable; both reads and constants agree.
    let sub = body.data[closures[2]][0];
    let subs = body.inst_subs();
    let adds = body
        .tags
        .iter()
        .enumerate()
        .filter_map(|(i, &tag)| {
            (tag == Tag::Prim
                && subs[i] == sub
                && body.data[i][0] == hd_tir::ir::PrimOp::Add as u32)
                .then_some(i)
        })
        .collect::<Vec<_>>();
    assert_eq!(adds.len(), 3);
    let a = body.record(body.data[adds[0]][1]);
    let b = body.record(body.data[adds[1]][1]);
    assert_eq!(
        body.local_read(a[0] as usize),
        body.local_read(b[0] as usize)
    );
    assert_eq!(
        body.consts[(a[1] & !Ref::CONST_BIT) as usize],
        body.consts[(b[1] & !Ref::CONST_BIT) as usize]
    );
    body.data[adds[1]][1] = body.data[adds[0]][1];
    // Equal descriptor interning is representation preserving. The two
    // escaping closures now share the same record, as a wire compactor may.
    body.data[closures[1]][1] = body.data[closures[0]][1];
    assert!(hd_tir::ir::verify_checked(&body).is_empty());
    let mut refused = body.clone();
    assert!(
        !hd_tir::inline::inline_local_closures(&mut refused, 16, &|_, _, _| false)
            .expect("admission can decline a real closure")
    );
    assert!(hd_tir::ir::verify_checked(&refused).is_empty());
    let before = body.len();
    let mut budgeted = body.clone();
    assert!(
        !hd_tir::inline::inline_local_closures(&mut budgeted, 16, &|caller, _, _| {
            caller.len() == before
        })
        .expect("capture bindings can exhaust the caller budget")
    );
    assert!(hd_tir::ir::verify_checked(&budgeted).is_empty());
    assert!(
        hd_tir::inline::inline_local_closures(&mut body, 16, &|_, _, _| true)
            .expect("copy scratch")
    );
    assert_eq!(body.tags.iter().filter(|&&t| t == Tag::Closure).count(), 2);
    assert!(!body.inlined.is_empty());
    let before = hd_tir::wire::write_body(&body, &pool, &paths, &syms).expect("copied wire");
    let mapped = std::cell::Cell::new(0);
    hd_tir::wire::map_ids(
        &mut body,
        &|def| {
            mapped.set(mapped.get() + 1);
            def
        },
        &|t| t,
        &|l| l,
    )
    .expect("identity remap");
    assert!(mapped.get() >= body.inlined.len() + 1);
    let after = hd_tir::wire::write_body(&body, &pool, &paths, &syms).expect("remapped wire");
    assert_eq!(before, after);
}

#[test]
fn bad_splices_of_a_real_call_report_the_operand_and_insertion_position() {
    use hd_tir::ir::Topology;
    use hd_tir::own::{BatchInsert, insert_batch};
    let Checked { body, .. } = checked("projected");
    let root = body.sub_root[0];
    let call = body
        .tags
        .iter()
        .position(|t| *t == Tag::Call)
        .expect("real call");
    let source = Ref(body.call_args(call)[0])
        .as_inst()
        .expect("argument read")
        .0 as usize;
    let topo = Topology::new(&body);
    let edit = BatchInsert {
        block: root,
        pos: 0,
        tag: body.tags[source],
        data: body.data[source],
        ty: body.ty[source],
        syn: body.syn[source],
    };
    for invalid in [
        BatchInsert {
            block: u32::try_from(call).expect("call"),
            ..edit
        },
        BatchInsert {
            pos: u32::MAX,
            ..edit
        },
    ] {
        let result = insert_batch(&mut body.clone(), &topo, &[invalid]);
        assert!(matches!(result, Err(ref e) if e.contains("invalid insertion position")));
    }
    let mut bad = body.clone();
    let at = bad.data[call][1] as usize + 1;
    bad.extra[at] = u32::try_from(body.len() + 1).expect("small bad operand");
    let result = insert_batch(&mut bad, &topo, &[edit]);
    assert!(matches!(result, Err(ref e) if e.contains("out of range while splicing")));
}

#[test]
fn malformed_program_members_and_places_do_not_become_ownership_facts() {
    let Checked { body, .. } = checked_source(
        include_str!("../tests/cases/counts/local-reference-alias-adaptations.hd"),
        "scalar_element",
    );
    assert!(body.tags.contains(&Tag::Call) && body.tags.contains(&Tag::Ref));
    let root = body.sub_root[0] as usize;
    let mut missing_member = body.clone();
    let mut members = body.record(body.data[root][0]).to_vec();
    members.push(u32::try_from(body.len() + 7).expect("invalid member"));
    missing_member.data[root][0] = record(&mut missing_member, &members);
    assert!(!hd_tir::ir::verify_checked(&missing_member).is_empty());
    let facts = hd_tir::own::ElementChecks::new(&missing_member);
    assert!(!facts.repeats_bounds(body.len() + 7));

    let reference = body
        .tags
        .iter()
        .position(|&tag| tag == Tag::Ref)
        .expect("element loan");
    let mut missing_place = body.clone();
    missing_place.data[reference][0] = u32::try_from(body.extra.len() + 17).expect("invalid place");
    assert!(!hd_tir::ir::verify_checked(&missing_place).is_empty());
    let bindings = hd_tir::own::RecursiveBindings::new(&missing_place);
    assert_eq!(bindings.self_local(reference), None);
}

#[test]
fn a_constant_provider_has_no_temporary_owner_to_transfer() {
    let Checked { mut body, .. } = checked("scoped_call");
    let scope = body
        .tags
        .iter()
        .position(|&tag| tag == Tag::With)
        .expect("provider scope");
    let pairs = body.data[scope][0] as usize;
    let source = Ref(body.extra[pairs + 2])
        .as_inst()
        .expect("source provider");
    let constant = u32::try_from(body.consts.len()).expect("provider constant");
    body.consts.push((body.ty[source.0 as usize], 0));
    body.extra[pairs + 2] = Ref::CONST_BIT | constant;
    assert!(hd_tir::ir::verify_checked(&body).is_empty());
    let topology = hd_tir::ir::Topology::new(&body);
    let transfers = hd_tir::own::scoped_provider_transfers(&body, &topology);
    assert!(transfers.iter().all(|&(from, _, _)| from as usize != scope));
}

#[test]
fn a_temporary_reference_with_another_address_cannot_be_promoted() {
    use hd_tir::ir::{Topology, ref_word};
    use hd_tir::own::{BatchInsert, insert_batch};
    let Checked { mut body, .. } = checked_source(
        include_str!("../tests/cases/counts/local-reference-alias-adaptations.hd"),
        "mixed_effects",
    );
    let reference = body
        .tags
        .iter()
        .enumerate()
        .find_map(|(i, &tag)| {
            (tag == Tag::Ref && body.data[i][1] & ref_word::MUT == 0).then_some(i)
        })
        .expect("immutable list address");
    let consumer = (0..body.len())
        .find(|&i| body.tags[i] == Tag::Call && body.call_args(i).contains(&(reference as u32)))
        .expect("read-only list call");
    // A last-use temporary address may transfer its holder's ownership.
    // A second address makes it a cell, so promotion must preserve the holder.
    body.data[reference][1] |= ref_word::TAKES;
    assert!(hd_tir::own::promote::promotable_ref(
        &body, reference, consumer
    ));
    let topo = Topology::new(&body);
    let block = topo.blk[consumer];
    let pos = body
        .record(body.data[block as usize][0])
        .iter()
        .position(|&i| i == consumer as u32)
        .expect("call in block");
    let other = BatchInsert {
        block,
        pos: pos as u32,
        tag: Tag::Ref,
        data: [
            body.data[reference][0],
            body.data[reference][1] & !ref_word::TAKES,
        ],
        ty: body.ty[reference],
        syn: body.syn[reference],
    };
    let mapped = insert_batch(&mut body, &topo, &[other]).expect("second address");
    assert!(!hd_tir::own::promote::promotable_ref(
        &body,
        mapped[reference] as usize,
        mapped[consumer] as usize
    ));
}
