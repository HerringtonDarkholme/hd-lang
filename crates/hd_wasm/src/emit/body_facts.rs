use std::sync::{Arc, OnceLock};

use hd_base::HashMap;
use hd_tir::ir::{Body, Topology};
use hd_tir::own::{OwnCx, Reason};

use crate::{Input, module::Module};

/// Immutable final-body queries, shared across monomorphic instances and
/// closure sub-bodies. A module owns these cells only for this link; bodies
/// cannot be spliced while Input is borrowed by emission. The instance
/// table releases its cell references as emission consumes them.
pub(crate) type BodyFactCell = Arc<OnceLock<BodyFacts>>;

pub(crate) struct BodyFacts {
    pub(super) topo: Arc<Topology>,
    pub(super) recursive: Arc<hd_tir::own::RecursiveBindings>,
    pub(super) uses: Arc<UseFacts>,
    pub(super) borrow: Arc<OnceLock<super::borrow::BorrowFacts>>,
    /// How many times each value is a value operand, counted by the first
    /// emitter that asks (`indirect.rs`), once per body however many
    /// instances and closure sub-bodies it has.
    pub(super) slot_uses: Arc<OnceLock<Arc<HashMap<u32, u32>>>>,
    /// Each local some closure of the body captures.
    pub(super) captured: Arc<hd_base::HashSet<usize>>,
    pub(super) short_uses: OnceLock<hd_tir::own::short::Uses>,
    pub(super) rows: Vec<Arc<[usize]>>,
    pub(super) break_counts: Arc<[u32]>,
    pub(super) storage: Vec<Vec<(hd_base::LocalId, hd_tir::ir::Storage)>>,
    pub(super) cell_stores: Arc<[u32]>,
    pub(super) closures: Vec<Option<usize>>,
    pub(super) root_params: Arc<[usize]>,
    pub(super) transfers: Arc<HashMap<(u32, u32), u32>>,
}

/// One census per final body, rather than one per emitted closure.
/// O(instructions + operands), with no instance-type or sub-body state.
pub(super) struct UseFacts {
    pub consumer: Vec<u32>,
    pub copy_reasons: Vec<Option<Reason>>,
    pub value_uses: Vec<u32>,
    pub part_only: Vec<bool>,
}

impl BodyFacts {
    pub(super) fn new(
        b: &Body,
        cx: &dyn OwnCx,
        origins: bool,
        checked: bool,
        boundary: &hd_tir::own::boundary::Boundary,
    ) -> Self {
        let topo = Arc::new(Topology::new(b));
        let mut rows = vec![Vec::new(); b.sub_root.len()];
        let mut closures = vec![None; b.sub_root.len()];
        let mut break_counts = vec![0; b.label_inst.len()];
        for i in 0..b.len() {
            rows[topo.sub[i] as usize].push(i);
            if b.tags[i] == hd_tir::ir::Tag::Break {
                break_counts[b.data[i][0] as usize] += 1;
            }
            if b.tags[i] == hd_tir::ir::Tag::Closure {
                closures[b.data[i][0] as usize].get_or_insert(i);
            }
        }
        let recursive = Arc::clone(&boundary.recursive);
        let mut storage = vec![Vec::new(); b.sub_root.len()];
        for &(sub, local, kind) in &b.storage {
            storage[sub.idx()].push((local, kind));
        }
        let cell_stores = hd_tir::own::declarations(b, &topo.sub).into();
        let transfers = hd_tir::own::scoped_provider_transfers(b, &topo)
            .into_iter()
            .map(|(scope, operand, call)| ((scope, operand), call))
            .collect();
        crate::work::use_facts(b.len());
        Self {
            borrow: Arc::default(),
            slot_uses: Arc::default(),
            captured: Arc::new(b.cap_local.iter().map(|l| l.idx()).collect()),
            short_uses: OnceLock::new(),
            rows: rows.into_iter().map(Arc::from).collect(),
            break_counts: break_counts.into(),
            closures,
            storage,
            cell_stores,
            root_params: super::param_locals(b).into(),
            transfers: Arc::new(transfers),
            recursive,
            uses: Arc::new(UseFacts::new(
                b,
                &topo,
                cx,
                origins,
                checked,
                &boundary.consumers,
            )),
            topo,
        }
    }
}

impl UseFacts {
    fn new(
        b: &Body,
        topo: &Topology,
        cx: &dyn OwnCx,
        origins: bool,
        checked: bool,
        consumers: &[u32],
    ) -> Self {
        let mut facts = Self {
            consumer: consumers.to_vec(),
            copy_reasons: Vec::new(),
            value_uses: Vec::new(),
            part_only: Vec::new(),
        };
        let verify = cfg!(not(target_arch = "wasm32")) && checked;
        if origins || verify {
            hd_tir::own::copy_reasons(b, topo, cx, &mut facts.copy_reasons);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if verify {
            // ruled out: final owned bodies satisfy the copy-reason verifier;
            // checked builds retain this existing verification as an invariant.
            let bad = hd_tir::own::check_copy_reasons(b, topo, cx, &facts.copy_reasons);
            assert!(
                bad.is_empty(),
                "the reason verifier rejects a body of item {:?}:\n{}",
                b.item,
                bad.join("\n")
            );
        }
        facts.value_uses.clear();
        facts.value_uses.resize(b.len(), 0);
        facts.part_only.clear();
        facts.part_only.resize(b.len(), true);
        let mut ops = Vec::new();
        for i in 0..b.len() {
            hd_tir::own::operands(b, i, &mut ops);
            let part = super::refs::part_use(b, i);
            for op in &ops {
                if op.role != hd_tir::ir::Role::Dropped {
                    facts.value_uses[op.value as usize] += 1;
                }
                if part != Some(op.value) {
                    facts.part_only[op.value as usize] = false;
                }
            }
        }
        crate::work::use_facts(b.len());
        facts
    }
}

/// Group by final-body identity while Input owns every body; retain only
/// cells indexed by stable instance ID. One pass over emitted instances.
pub(crate) fn prepare(input: &Input<'_>, module: &mut Module) {
    let mut grouped = HashMap::default();
    module.body_facts = vec![None; input.collected.table.item.len()];
    for &id in input.order {
        let Some(body) = (input.body)(id) else {
            continue;
        };
        let facts = grouped
            .entry(std::ptr::from_ref(body))
            .or_insert_with(BodyFactCell::default);
        module.body_facts[id.idx()] = Some(Arc::clone(facts));
    }
}
