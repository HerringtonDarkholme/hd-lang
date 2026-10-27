//! One instance's TIR body to one Wasm function
//! (`crates/hd_wasm_linear/DESIGN.md` §23).
//!
//! Emission uses virtual Wasm locals; wide bodies later move them to
//! shadow storage (wide.rs). An instruction's value is stored in
//! fresh locals of its layout as it is computed, a constant is pushed as an
//! immediate, and a TIR local owns the locals of its layout. Structured
//! control is Wasm's own: a `Match` is a block per arm around its decision
//! tree, whose `ToArm` branches out to its arm, and a `Loop` is a block
//! around a loop. Integer arithmetic follows §13.15 of
//! `crates/hd_mono/DESIGN.md`, including its panic checks.

use hd_base::SliceSort as _;
use std::collections::{BTreeMap, BTreeSet};

use crate::op::Op as Instruction;
use crate::op::Op as I;
use hd_base::{DefId, Hash128, HashMap, InstId, StageResult};
use hd_mono::{ProgramEnv, Sub, Target, TargetKind, subst};
use hd_tir::ir::{
    Body, BodyKind, Callee, Coercion, IntrinsicOp, NONE, PrimOp, Ref, Tag, local_flags,
};
use hd_types::{InternPool, Ty, TyData, TyList};
use wasm_encoder::BlockType;

use crate::layout::{Pass, num, vals};
use crate::module::{Func, Global, HEAP, Import, Ins, Module, Stored, Sym, Val, mem};
use crate::rt::Helper;
use crate::{Input, unsupported};

mod closure;
mod count;
mod ctx;
pub(crate) use ctx::ctx_lookup;
pub(crate) use ctx::slot_ctx;
mod dyn_value;
mod erased;
mod iter;
mod list;
mod map;
mod refs;
mod scope;
mod shape;
mod susp;
mod text;
mod value;

/// The function of instance `id`; the module assigns its final index.
pub fn instance(input: &Input<'_>, m: &mut Module, id: InstId) -> StageResult<Func> {
    let t = &input.collected.table;
    let item = t.item[id.idx()];
    let args = t.args[id.idx()];
    // An erased body takes the witness of its caller's type arguments
    // after its parameters (`erased.rs`).
    let mut erased = None;
    let sub = match t.sub[id.idx()] {
        Sub::Body(sub) => sub,
        Sub::Adapter => return adapter(input, m, id),
        Sub::Erased => {
            erased = Some(erased::family(input, id)?);
            0
        }
        Sub::Thunk(_) => return erased::thunk(input, m, id),
    };
    let Some(b) = (input.body)(id) else {
        return unsupported("an instance without TIR");
    };
    let own = Own {
        item,
        args,
        sub: t.sub[id.idx()],
        key: t.key[id.idx()],
        erased,
        cold: None,
        lends: input.env.lends(item),
    };
    body_func(input, m, &own, sub, &input.collected.calls[id.idx()], b)
}

/// What [`body_func`] emits: item `item`'s instance at `args`, its sub-body
/// `sub` and key, and for an erased body its family. `cold` is the result
/// type of a plain function of a suspending item, which only makes the
/// item's cold call ([`call_func`]); it never suspends.
struct Own {
    item: DefId,
    args: TyList,
    sub: Sub,
    key: Hash128,
    erased: Option<Hash128>,
    cold: Option<Ty>,
    /// The parameters the item's release summary lends
    /// (`ProgramEnv::lends`): none for a body that only calls the item.
    lends: u64,
}

/// The function of body `b`'s sub-body `sub`, the instance `own`, whose
/// call targets are `targets`.
fn body_func<'a>(
    input: &Input<'a>,
    m: &mut Module,
    own: &Own,
    sub: u16,
    targets: &HashMap<u32, Target>,
    b: &'a Body,
) -> StageResult<Func> {
    let (item, args, erased) = (own.item, own.args, own.erased);
    // A suspending body is a state machine over a frame (`susp.rs`), but
    // a root that only the entry calls and that never suspends runs as a
    // plain function: it ends on its first poll.
    let plan = susp::plan(b, b.sub_root[sub as usize]);
    let suspending = if sub == 0 {
        own.cold.is_none()
            && input.env.suspends(item)
            && !(input.entry.root(item) && plan.0.is_empty())
    } else {
        closure::closure_suspends(input, b, args, sub)?
    };
    let roots = ref_roots(input, b, item, args, suspending);
    let cells = closure::cell_locals(b, &roots);
    let direct_json = input.direct_json_encoders.contains(&own.key);
    let mut em = Em::new(input, m, b, (item, args), targets, cells, direct_json);
    // A local only local references lend lives in the frame (§23.38): a
    // plain function's own, at a runtime that counts.
    if sub == 0 && !suspending && erased.is_none() && own.cold.is_none() && input.runtime.is_some()
    {
        em.plan_slots(&roots)?;
    }
    em.origin_root = b.sub_root[sub as usize];
    // Every body is the ownership passes' output, whose counts emission
    // follows (`docs/compiler/tir-v2.md`, "Stage 2").
    if b.tags
        .iter()
        .zip(&b.data)
        .any(|(t, d)| *t == Tag::Read && d[1] == hd_tir::ir::ReadMode::Use as u32)
    {
        return unsupported("a body the ownership passes did not run on");
    }
    em.owned = true;
    em.sub_id = sub;
    if sub == 0 {
        em.lends = own.lends;
    }
    em.own_consumers();
    let mut params = Vec::new();
    // A suspending body takes its frame first (`susp::FRAME`).
    if suspending {
        params.push(Val::I32);
    }
    // A closure's code takes its environment first, then its own
    // parameters and one context (`closure.rs`, `ctx.rs`).
    let (plocals, ret, keys, closure) = if sub == 0 {
        (
            param_locals(b),
            own.cold
                .unwrap_or_else(|| em.sub(input.env.ret(item).unwrap_or(Ty::VOID))),
            hd_mono::declared_row_keys(input.pool, input.env, item, args),
            None,
        )
    } else {
        let ci = closure::closure_of(b, sub)?;
        let ft = em.sub(b.ty[ci]);
        let TyData::Fn { result, .. } =
            em.pool().get(crate::layout::strip(em.pool(), em.env(), ft))
        else {
            return unsupported("a closure without a function type");
        };
        params.push(Val::I32);
        (
            sub_param_locals(b, sub),
            em.sub(result),
            Vec::new(),
            Some(ci),
        )
    };
    // A closure's copy of a capture is a value of its own, never the
    // cell a reference to the captured local made.
    if let Some(ci) = closure {
        for (l, shared) in closure::captures(b, ci) {
            if !shared {
                em.cells.remove(&l);
            }
        }
    }
    // A plain function takes its parameters by the conventions of
    // `layout::Pass` (§23.25); a suspending one's cold constructor counts
    // each argument into its frame, so the callee takes a count of each.
    let by_type = !suspending && (sub != 0 || !input.env.suspends(item));
    for &l in &plocals {
        let vs = em.vals(em.sub(b.local_ty[l]))?;
        let at = u32::try_from(params.len()).expect("params");
        em.locals.insert(
            l,
            (at..at + u32::try_from(vs.len()).expect("params")).collect(),
        );
        params.extend(vs);
    }
    let witness = erased.map(|_| {
        let at = u32::try_from(params.len()).expect("params");
        params.push(Val::I32);
        at
    });
    for key in keys {
        let at = u32::try_from(params.len()).expect("params");
        em.providers.push((key, [at, at + 1]));
        params.extend([Val::I32, Val::I32]);
    }
    // A function with row parameters takes their keys' context last, and
    // a closure's code its caller's (`ctx.rs`).
    let ctx = (closure.is_some() || hd_mono::has_row_params(input.env, item)).then(|| {
        let at = u32::try_from(params.len()).expect("params");
        params.push(Val::I32);
        at
    });
    // A closure's own row: each key's provider from the context, once the
    // body has its locals. A row that lists the open row keeps the
    // context for the keys it does not name.
    let mut row = Vec::new();
    if let Some(ci) = closure {
        let (ks, open) = em.fn_row(em.sub(b.ty[ci]))?;
        row = ks;
        if open {
            em.open_ctx = ctx;
        }
    } else {
        em.open_ctx = ctx;
    }
    em.open_base = em.providers.len();
    let results = em.vals(ret)?;
    em.results.clone_from(&results);
    let mut fres = results.clone();
    if suspending {
        fres.push(Val::I32);
    }
    em.f = em.m.func(
        format!("{}#{}", (input.path)(item), own.sub),
        params.clone(),
        fres,
    );
    if em.m.track_origins {
        em.f.origins = Some(Vec::new());
        em.f.set_origin_source_len(b.tags.len());
        em.f.origin_item = Some((input.path)(b.item));
        em.f.origin_root = Some(b.sub_root[sub as usize]);
        em.f.origin_at = em.f.origin_root;
        em.f.local_names = Some(BTreeMap::new());
        em.f.statics = Some(BTreeSet::new());
        em.f.origin_args = input
            .pool
            .list_items(args)
            .iter()
            .map(|t| hd_mono::printed_type_name(input.pool, input.env, *t))
            .collect();
        // The parameters that are no TIR local, by their role.
        let mut at = 0;
        if suspending {
            em.f.name_local(susp::FRAME, "frame");
            at += 1;
        }
        if closure.is_some() {
            em.f.name_local(at, "env");
        }
        if let Some(w) = witness {
            em.f.name_local(w, "witness");
        }
        for (n, (_, [data, vtable])) in em.providers.clone().into_iter().enumerate() {
            em.f.name_local(data, format!("provider{n}"));
            em.f.name_local(vtable, format!("provider{n}.vtable"));
        }
        if let Some(c) = ctx {
            em.f.name_local(c, "ctx");
        }
    }
    let key = own.key;
    let once = match (sub, own.sub, b.kind) {
        (0, Sub::Body(_), BodyKind::Fact) => Some(Some(b.label.as_ref())),
        (0, Sub::Body(_), BodyKind::Handle) => Some(None),
        _ => None,
    }
    .map(|fact| OnceCell {
        key,
        results: results.clone(),
        fact,
    });
    if let Some(cell) = &once {
        if b.kind == BodyKind::Handle {
            em.static_cells = Some(key);
        }
        if em.m.track_origins {
            let name = format!("{}#once", (input.path)(item));
            em.m.storage_names.insert(key, name);
        }
        cell.enter(&mut em, ret)?;
    }
    if suspending {
        let pc = em.f.local(Val::I32);
        let cancelled = em.f.local(Val::I32);
        // A frame holds a count of each provider's data, and of the
        // context, which holds the closure's own.
        let provs = em.providers.iter().map(|p| p.1[0]).chain(ctx).collect();
        em.susp = Some(susp::Susp {
            key,
            pc,
            states: plan.0,
            ranges: plan.1,
            cancel_to: None,
            cancelled,
            provs,
            alls: HashMap::default(),
        });
    }
    // A function that holds a counted reference ends at one exit, which
    // releases what it owns (`count.rs`): a return stores its value in
    // `ret` and branches there.
    // A suspending closure's body runs only over the frame its code
    // built, which holds the captures (`susp::finish`).
    if let Some(ci) = closure {
        if suspending {
            em.f.get(susp::FRAME);
            em.f.op(Instruction::I32Eqz);
            em.f.op(Instruction::If(BlockType::Empty));
        }
        em.load_captures(ci, u32::from(suspending), own.key)?;
        if suspending {
            em.f.op(Instruction::End);
        } else {
            em.entry_cells.extend(
                closure::captures(b, ci)
                    .into_iter()
                    .filter_map(|(l, shared)| shared.then_some(l)),
            );
        }
    }
    // Once-cells also need the common exit when their result is uncounted:
    // every return must publish the cached value and end fact evaluation.
    em.counting = em.counts_anything() || once.is_some();
    let mut held: Vec<(usize, Vec<u32>)> = em.locals.iter().map(|(k, v)| (*k, v.clone())).collect();
    held.sort_stable();
    if em.counting {
        em.ret = results.iter().map(|v| em.f.local(*v)).collect();
    }
    // A resumed body reloads its locals, which hold their counts, from the
    // frame; only a fresh start takes its parameters' counts.
    if suspending {
        em.f.get(susp::FRAME);
        em.f.op(Instruction::If(BlockType::Empty));
        em.f.push(crate::module::Ins::LoadAll);
        em.f.get(susp::FRAME);
        em.f.op(Instruction::I32Load(mem(HEAP, crate::task::F_STATE, 2)));
        em.f.set(em.susp.as_ref().map_or(0, |s| s.pc));
        em.f.op(Instruction::Else);
    }
    // The parameters this instance receives borrowed whose value the body
    // only lends or passes on to callees that borrow it here too
    // (`Em::instance_loans`).
    let lent_here = if em.counting && by_type && sub == 0 {
        em.instance_loans(&plocals)
    } else {
        hd_base::HashSet::default()
    };
    if em.counting {
        for (l, ls) in &held {
            // A parameter still holds its value until it moves into its
            // cell below.
            if plocals.contains(l) {
                let t = em.sub(b.local_ty[*l]);
                // A lent parameter the passes leave unowned is outside the
                // exit set: the body takes no count of it, and releases at
                // its exit the count an instance that owns it receives.
                let unowned = by_type
                    && !b
                        .exit_drops
                        .iter()
                        .any(|&(s, x)| u32::from(sub) == s.raw() && x.idx() == *l);
                if by_type {
                    let k = plocals.iter().position(|p| p == l).expect("parameter");
                    match em.pass_at(em.lends, k, t) {
                        Pass::Owned(_) if unowned => {
                            em.unowned_params.push(*l);
                            continue;
                        }
                        Pass::Borrowed if unowned || lent_here.contains(l) => {
                            em.borrowed.insert(*l);
                            continue;
                        }
                        // Its count arrives with it.
                        Pass::Uncounted | Pass::Owned(_) => continue,
                        // The body owns every other parameter: the passes
                        // drop each one.
                        Pass::Borrowed => {}
                    }
                }
                em.count_locals(ls, t, true)?;
            } else {
                em.count_local(*l, ls, true)?;
            }
        }
    }
    // A parameter a closure shares moves into its cell, which takes over
    // its count.
    for (l, ls) in &held {
        if plocals.contains(l) && em.is_cell(*l) {
            em.param_cell(*l, ls)?;
            if !suspending {
                em.entry_cells.insert(*l);
            }
        }
    }
    if suspending {
        em.f.op(Instruction::End);
    }
    if let Some(c) = ctx {
        for k in row {
            let id = ctx::key_id(input, k);
            let ls = ctx::ctx_lookup(&mut em.f, c, id);
            em.providers.push((k, ls));
        }
        em.open_base = em.providers.len();
    }
    if let (Some(family), Some(w)) = (erased, witness) {
        em.read_ops(family, key, w)?;
    }
    if em.counting {
        em.open(Ctl::Exit);
    }
    em.prepare_own_loans();
    let root = b.sub_root[sub as usize];
    em.block_into(root, None)?;
    let tail = em.live_tail(root);
    if !results.is_empty() {
        if tail == NONE || em.diverges(tail) {
            em.f.op(Instruction::Unreachable);
        } else if em.counting {
            let moved = em.take_from(tail, root);
            let s = if moved {
                value::Src::Ref(tail)
            } else {
                em.retain_ref(tail)?
            };
            em.push_src(&s)?;
            for l in em.ret.clone().iter().rev() {
                em.f.set(*l);
            }
            if moved {
                em.settle_value(tail);
            }
        } else {
            em.push(tail)?;
        }
    }
    if em.counting {
        em.close();
        em.release_all()?;
        for l in em.ret.clone() {
            em.f.get(l);
        }
    }
    if let Some(cell) = &once {
        cell.leave(&mut em, ret)?;
    }
    if em.f.local_names.is_some() {
        let mut named: Vec<(usize, Vec<u32>)> =
            em.locals.iter().map(|(k, v)| (*k, v.clone())).collect();
        named.sort_stable();
        for (l, ls) in named {
            if let [w] = ls[..] {
                em.f.name_local(w, format!("${l}"));
                continue;
            }
            // A local of several values names each by its field path.
            let paths = crate::layout::value_paths(input.pool, input.env, em.sub(b.local_ty[l]));
            for (k, w) in ls.iter().enumerate() {
                let name = if paths.len() == ls.len() {
                    format!("${l}.{}", paths[k])
                } else {
                    format!("${l}.{k}")
                };
                em.f.name_local(*w, name);
            }
        }
    }
    if !suspending {
        if let Some((base, size)) = em.frame.filter(|f| f.0 != NONE) {
            crate::wide::locals_frame(&mut em.f, base, size);
        }
        return Ok(em.f);
    }
    em.exit_frame();
    let held = if sub == 0 {
        input.entry.held(item)
    } else {
        None
    };
    susp::finish(em, key, &plocals, &params, ret, closure, held)
}

/// A facts or structure-handle body owns one cached result per instance.
/// Its flag follows the result globals: 0 empty, 2 evaluating, 1 ready.
struct OnceCell<'b> {
    key: Hash128,
    results: Vec<Val>,
    fact: Option<&'b str>,
}

impl OnceCell<'_> {
    fn global(&self, k: usize) -> Global {
        Global::Storage(
            self.key,
            u32::try_from(k).expect("once-cell values"),
            self.results.get(k).copied().unwrap_or(Val::I32),
        )
    }

    /// Opens the cached path and loads its result, before its retain.
    fn load_cached(&self, f: &mut Func) -> Vec<u32> {
        f.global_get(self.global(self.results.len()));
        f.i32(1);
        f.op(I::I32Eq);
        f.op(I::If(BlockType::Empty));
        let held: Vec<u32> = self.results.iter().map(|v| f.local(*v)).collect();
        for (k, l) in held.iter().enumerate() {
            f.global_get(self.global(k));
            f.set(*l);
        }
        held
    }

    fn return_cached(f: &mut Func, held: &[u32]) {
        for l in held {
            f.get(*l);
        }
        f.op(I::Return);
        f.op(I::End);
    }

    /// Publishes the values after the global has taken its own count.
    fn publish(&self, f: &mut Func, held: &[u32]) {
        for (k, l) in held.iter().enumerate() {
            f.get(*l);
            f.global_set(self.global(k));
        }
        f.i32(1);
        f.global_set(self.global(self.results.len()));
    }

    fn fact_import() -> Sym {
        Sym::Import(Import::new("hd:rt", "fact", &[Val::I32], &[]))
    }

    fn enter(&self, em: &mut Em<'_, '_, '_>, ret: Ty) -> StageResult<()> {
        if em.input.runtime.and_then(|r| r.checked).is_some() {
            // Teardown releases the cached count and zeros the whole cell,
            // including an uncounted result and the flag, between test cases.
            let mut vals = self.results.clone();
            vals.push(Val::I32);
            let walk = em.counted(ret).unwrap_or(crate::layout::Walk::Seq(vec![]));
            em.m.stored.insert(
                self.key,
                Stored {
                    vals,
                    walk,
                    slot: None,
                },
            );
        }
        let held = self.load_cached(&mut em.f);
        // The global keeps its count; every caller receives a fresh one.
        em.retain_locals(&held, ret)?;
        Self::return_cached(&mut em.f, &held);
        let flag = self.global(self.results.len());
        if let Some(label) = self.fact {
            let at = em.m.static_bytes(label.as_bytes(), 1);
            em.f.static_addr(at);
            em.f.u32(u32::try_from(label.len()).expect("fact label"));
            em.f.helper(Helper::ToBuf);
            em.f.call(Self::fact_import());
            em.f.global_get(flag);
            em.f.op(I::If(BlockType::Empty));
            em.panic("reentrant-evaluation: the fact reads itself");
            em.f.op(I::End);
            em.f.i32(2);
            em.f.global_set(flag);
            em.forbid_by(1);
        }
        Ok(())
    }

    fn leave(&self, em: &mut Em<'_, '_, '_>, ret: Ty) -> StageResult<()> {
        let held: Vec<u32> = self.results.iter().map(|v| em.f.local(*v)).collect();
        for l in held.iter().rev() {
            em.f.set(*l);
        }
        // The result already owns its caller's count; the global takes a
        // copy.
        let kept = em.copy_locals(&held, ret)?;
        self.publish(&mut em.f, &kept);
        if self.fact.is_some() {
            em.f.i32(-1);
            em.f.call(Self::fact_import());
            em.forbid_by(-1);
        }
        for l in &held {
            em.f.get(*l);
        }
        Ok(())
    }
}

/// A minimal handle body increments a counter on entry and returns it.
/// Execute the emitted cache twice: the body must have run only once.
#[test]
fn handle_once_cell_runs_its_body_once() {
    let cell = OnceCell {
        key: Hash128(616),
        results: vec![Val::I32],
        fact: None,
    };
    let counter = Global::Storage(Hash128(617), 0, Val::I32);
    let mut m = Module::default();
    let mut handle = m.func("handle", vec![], vec![Val::I32]);
    let cached = cell.load_cached(&mut handle);
    OnceCell::return_cached(&mut handle, &cached);
    handle.global_get(counter);
    handle.i32(1);
    handle.op(I::I32Add);
    let result = handle.local(Val::I32);
    handle.set(result);
    handle.get(result);
    handle.global_set(counter);
    cell.publish(&mut handle, &[result]);
    handle.get(result);
    let mut runs = m.func("runs", vec![], vec![Val::I32]);
    runs.global_get(counter);
    m.define(Sym::Inst(Hash128(616)), handle);
    m.export("handle", Sym::Inst(Hash128(616)));
    m.define(Sym::Inst(Hash128(617)), runs);
    m.export("runs", Sym::Inst(Hash128(617)));
    let wasm = m.finish().expect("link counter fixture");
    let mut child = std::process::Command::new("node")
        .args([
            "--input-type=module",
            "-e",
            r"
            const chunks = [];
            for await (const chunk of process.stdin) chunks.push(chunk);
            const {instance} = await WebAssembly.instantiate(Buffer.concat(chunks));
            const first = instance.exports.handle();
            const second = instance.exports.handle();
            const runs = instance.exports.runs();
            if (first !== 1 || second !== 1 || runs !== 1) {
                throw new Error(JSON.stringify({first, second, runs}));
            }
        ",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("start Node counter fixture");
    std::io::Write::write_all(&mut child.stdin.take().expect("fixture stdin"), &wasm)
        .expect("send counter fixture");
    let output = child.wait_with_output().expect("run counter fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The locals that some reference of body `b` refers to as its own place
/// (`shape::ref_local`) and that must live in memory: those of a reference
/// that is an address, not a trait value (`refs.rs`), but a suspending
/// body's parameters.
fn ref_roots(
    input: &Input<'_>,
    b: &Body,
    item: DefId,
    args: TyList,
    suspending: bool,
) -> Vec<usize> {
    let (pool, env) = (input.pool, input.env);
    (0..b.len())
        .filter_map(|i| {
            let l = shape::ref_local(b, i)?;
            let t = subst(pool, env, item, args, b.ty[i]);
            let addr = matches!(
                crate::layout::kind(pool, env, t),
                crate::layout::Kind::Ref { fat: false, .. }
            );
            if !addr {
                return None;
            }
            // A suspending body's parameter has no cell yet
            // (`susp::finish`), so a read-only reference to it is to a
            // copy; a `&mut` one keeps the cell, which `finish` refuses.
            let param = b.local_flags[l] & local_flags::PARAM != 0;
            if suspending && param && !crate::layout::ref_mutable(pool, env, t) {
                return None;
            }
            Some(l)
        })
        .collect()
}

/// The parameter locals of closure body `sub`, in order.
fn sub_param_locals(b: &Body, sub: u16) -> Vec<usize> {
    let r = b.sub_params[sub as usize];
    b.extra[r.start as usize + 1..(r.start + 1 + r.len) as usize]
        .iter()
        .map(|&l| l as usize)
        .collect()
}

/// The code of a function reference (`Sub::Adapter`), a closure's code
/// whose environment it never reads: `(environment, parameters,
/// providers) -> results`, one call of its target. A target the emitter
/// lowers at each call, an intrinsic, a built-in family's method or a
/// trait method at a trait-value `Self`, is called from a function of one
/// `Call` whose operands are the parameters ([`call_func`]), so the
/// reference lowers as a direct call at the same types does
/// (`crates/hd_mono/DESIGN.md` §13.11).
fn adapter(input: &Input<'_>, m: &mut Module, id: InstId) -> StageResult<Func> {
    let item = input.collected.table.item[id.idx()];
    match input.collected.calls[id.idx()].get(&0).cloned() {
        Some(Target::Call(ct)) if ct.kind == TargetKind::Instance => forward(
            input,
            m,
            item,
            (ct.item, ct.args),
            ct.ret,
            Sym::Inst(ct.key),
            input.env.lends(ct.item),
        ),
        Some(target @ (Target::Call(_) | Target::Dyn)) => {
            let (sym, ret) = call_func(input, m, id, target)?;
            let args = input.collected.table.args[id.idx()];
            forward(input, m, item, (item, args), ret, sym, 0)
        }
        _ => unsupported("a function reference that collection did not resolve"),
    }
}

/// The function of adapter `id`'s call body, one call of its item through
/// `target` whose operands are its parameters, which collection built
/// and the ownership passes ran over (`hd_mono::adapter_call_body`):
/// `(parameters, providers) -> results`, as a plain instance of the item
/// takes them. It is defined under a key derived from the adapter's;
/// with its result type.
fn call_func(
    input: &Input<'_>,
    m: &mut Module,
    id: InstId,
    target: Target,
) -> StageResult<(Sym, Ty)> {
    let t = &input.collected.table;
    let (item, args, key) = (t.item[id.idx()], t.args[id.idx()], t.key[id.idx()]);
    let (pool, env) = (input.pool, input.env);
    let ret = subst(pool, env, item, args, env.ret(item).unwrap_or(Ty::VOID));
    // A suspending item's call is its cold constructor, whose frame the
    // function returns (`fn.ref.suspending`).
    let cold = env.suspends(item).then(|| {
        pool.intern_ty(&TyData::Adt {
            def: env.suspend_def(),
            args: pool.list(&[ret]),
        })
    });
    let Some(b) = (input.body)(id) else {
        return unsupported("a function reference without its call body");
    };
    let Some(call) = b.tags.iter().position(|&t| t == Tag::Call) else {
        return unsupported("a function reference's call body without its call");
    };
    let calls = HashMap::from_iter([(u32::try_from(call).expect("insts"), target)]);
    let mut k = hd_base::StableHasher::new("linear-adapter-call");
    k.hash(key);
    let own = Own {
        item,
        args,
        sub: Sub::Body(0),
        key: k.finish(),
        erased: None,
        cold,
        lends: 0,
    };
    let f = body_func(input, m, &own, 0, &calls, b)?;
    let sym = Sym::Inst(own.key);
    m.define(sym.clone(), f);
    Ok((sym, ret))
}

/// The closure code of a reference to `item` that forwards its
/// parameters and its row's providers, read from the caller's context,
/// to one call of `sym`, a function of `callee` at its arguments, of
/// result type `ret`. The value's caller passes each parameter as the
/// function type says; `sym` borrows those in `lends`
/// (`ProgramEnv::lends`), which the adapter releases after the call.
fn forward(
    input: &Input<'_>,
    m: &Module,
    item: DefId,
    (callee, cargs): (DefId, TyList),
    ret: Ty,
    sym: Sym,
    lends: u64,
) -> StageResult<Func> {
    let (pool, env) = (input.pool, input.env);
    let mut params = vec![Val::I32];
    let mut lent = Vec::new();
    for (k, p) in env
        .params(callee)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
    {
        let t = subst(pool, env, callee, cargs, p);
        let vs = vals(pool, env, t)?;
        if k < 64
            && lends & (1 << k) != 0
            && let Pass::Owned(w) = crate::layout::pass(pool, env, m.runtime.is_some(), t)
        {
            lent.push((u32::try_from(params.len()).expect("params"), vs.clone(), w));
        }
        params.extend(vs);
    }
    // The context of the value's caller, which holds the value's row.
    let c = u32::try_from(params.len()).expect("params");
    params.push(Val::I32);
    // A suspending target's plain call is its cold constructor, which
    // returns the frame.
    let results = if env.suspends(callee) {
        vec![Val::I32]
    } else {
        vals(pool, env, ret)?
    };
    let mut f = m.func(
        format!("{}#adapter", (input.path)(item)),
        params.clone(),
        results,
    );
    let mut provs = Vec::new();
    for k in hd_mono::declared_row_keys(pool, env, callee, cargs) {
        provs.push(ctx::ctx_lookup(&mut f, c, ctx::key_id(input, k)));
    }
    for k in 1..c {
        f.get(k);
    }
    for [d, v] in provs {
        f.get(d);
        f.get(v);
    }
    // The target's own row parameters read their keys from the same
    // context, which passes on.
    if hd_mono::has_row_params(env, callee) {
        f.get(c);
    }
    f.call(sym);
    if !lent.is_empty() {
        let out: Vec<u32> = f.results.clone().into_iter().map(|v| f.local(v)).collect();
        for &l in out.iter().rev() {
            f.set(l);
        }
        for (at, vals, walk) in lent {
            for k in at..at + u32::try_from(vals.len()).expect("values") {
                f.get(k);
            }
            f.helper(Helper::Release { vals, walk });
        }
        for l in out {
            f.get(l);
        }
    }
    Ok(f)
}

/// The parameter types of instance `key` of `item` at `args`, as its own
/// body's parameter locals have them ([`param_locals`]), substituted: what
/// the callee itself counts by (`count.rs`). An instance without a body of
/// its own has its item's declared parameters.
pub(crate) fn callee_params(
    input: &Input<'_>,
    m: &Module,
    key: hd_base::Hash128,
    item: DefId,
    args: TyList,
) -> Vec<Ty> {
    let (pool, env) = (input.pool, input.env);
    let t = &input.collected.table;
    if let Some(&id) = m.inst_of.get(&key)
        && t.sub[id.idx()] == Sub::Body(0)
        && let Some(b) = (input.body)(id)
    {
        let (it, ia) = (t.item[id.idx()], t.args[id.idx()]);
        return param_locals(b)
            .into_iter()
            .map(|l| subst(pool, env, it, ia, b.local_ty[l]))
            .collect();
    }
    env.params(item)
        .unwrap_or_default()
        .into_iter()
        .map(|p| subst(pool, env, item, args, p))
        .collect()
}

/// The parameter locals of the body's own function, in order: those
/// flagged as parameters that belong to no closure.
fn param_locals(b: &Body) -> Vec<usize> {
    let mut in_sub = vec![false; b.local_ty.len()];
    for r in &b.sub_params[1..] {
        for &l in &b.extra[r.start as usize + 1..(r.start + 1 + r.len) as usize] {
            if let Some(x) = in_sub.get_mut(l as usize) {
                *x = true;
            }
        }
    }
    (0..b.local_ty.len())
        .filter(|&l| b.local_flags[l] & local_flags::PARAM != 0 && !in_sub[l])
        .collect()
}

/// An open Wasm block, for branch depths.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctl {
    Plain,
    /// The end of a `Loop`, a `Match` or a labelled `Block`.
    Brk(u32),
    /// The head of a `Loop`.
    Cont(u32),
    /// Arm `k` of `Match` `i`: its code follows this block's end.
    Arm(u32, u32),
    /// The body of a function that counts: its end is the one exit.
    Exit,
    /// The body of cleanup `Scope` `i`: its end runs the scope's suites
    /// (`scope.rs`).
    Scope(u32),
}

struct Em<'i, 'a, 'm> {
    input: &'i Input<'a>,
    m: &'m mut Module,
    b: &'a Body,
    /// This final body's instruction types at the instance's arguments.
    inst_ty: Vec<Ty>,
    item: DefId,
    direct_json: bool,
    origin_root: u32,
    args: TyList,
    calls: &'i HashMap<u32, Target>,
    f: Func,
    /// Each computed instruction's locals.
    vals: HashMap<u32, Vec<u32>>,
    /// Each TIR local's locals.
    locals: HashMap<usize, Vec<u32>>,
    /// The providers in scope: each key's data and vtable locals.
    providers: Vec<(Ty, [u32; 2])>,
    /// The local of the context this body takes, when its row lists an
    /// open row (`ctx.rs`): a key no provider in scope covers is read
    /// from it.
    open_ctx: Option<u32>,
    /// How many of `providers` the body had on entry: those after them
    /// are a `$.with` scope's, which go in front of the open context.
    open_base: usize,
    ctl: Vec<Ctl>,
    /// The `Match` instructions whose decision trees are being emitted.
    deciding: Vec<u32>,
    /// How many `Loop`s the instruction being emitted is inside.
    loops: u32,
    /// Whether the function counts, so that a return goes through its
    /// exit ([`Ctl::Exit`]).
    counting: bool,
    /// The locals of the function's results, at its exit.
    ret: Vec<u32>,
    /// The function's results.
    results: Vec<Val>,
    /// Each open cleanup scope's exit ladder (`scope.rs`).
    scopes: HashMap<u32, scope::ScopeInfo>,
    /// Each `defer` suite's "registered" flag local.
    defer_flags: HashMap<u32, u32>,
    /// The reads that take no count (`borrow` reads, lent parameters'
    /// reads and references, [`Em::prepare_own_loans`],
    /// [`Em::instance_loans`]): their places are no place's to release.
    loans: hd_base::HashSet<u32>,
    /// The locals the passes store a borrow into, with the stored value
    /// ([`Em::prepare_own_loans`]): they hold no count.
    loan_locals: HashMap<usize, u32>,
    /// What each projection read reads, by its base value's type
    /// ([`Em::projected`]).
    projs: Vec<shape::Projected>,
    /// A loan owns a temporary only when its source is an erased view.
    view_loans: HashMap<u32, u32>,
    /// In an owned body: the lent parameters the passes left unowned that
    /// this instance takes owned (`count.rs`): its exit releases them.
    unowned_params: Vec<usize>,
    /// The stable lists' loop caches ([`Em::own_list_caches`]), dropped at
    /// the loop's exit.
    list_caches: Vec<(list::ListRoot, list::ListCache)>,
    cached_buffers: Vec<(u32, u32)>,
    /// Exact range-index reads proven in bounds in this body's loops.
    proven_list_indices: Vec<u32>,
    /// The parameters borrowed from the caller, which the body never
    /// counts ([`crate::layout::Pass::Borrowed`]).
    borrowed: hd_base::HashSet<usize>,
    /// The parameters of the body's own function that its item's release
    /// summary lends (`ProgramEnv::lends`).
    lends: u64,
    /// The instructions being emitted, innermost last.
    emitting: Vec<u32>,
    /// The operands whose counts moved into an instruction being emitted,
    /// with that instruction: their locals are zeroed once it has read
    /// them ([`Em::take`]).
    moving: Vec<(u32, u32)>,
    /// The state machine of a suspending body (`susp.rs`).
    susp: Option<susp::Susp>,
    /// How many `using` dispose suites that wait the instruction being
    /// emitted is inside: their points are shielded (`susp.rs`).
    shield: u32,
    /// The suspension point, or the cleanup scope whose ladder, an
    /// `Exit::Cancel` being taken leaves from: an owned body releases the
    /// unwind rows of the points it holds ([`Em::release_unwound`]).
    cancel_from: u32,
    /// Locals that each own one count until the exit (`susp.rs`).
    temps: Vec<u32>,
    /// The locals that closures share, each in a cell, with the store
    /// that declares it (`closure.rs`).
    cells: HashMap<usize, u32>,
    /// The cell locals whose cell the prologue made or received: a
    /// parameter moved into its cell, a closure's or a thunk's shared
    /// capture (`closure.rs`, [`Em::cell_made`]).
    entry_cells: hd_base::HashSet<usize>,
    /// The cell locals that live in a slot of the function's frame on the
    /// local stack, not in a counted cell (§23.38), each with its slot's
    /// offset in the frame ([`closure::slot_locals`]).
    slots: HashMap<usize, u32>,
    /// A handle body's once-cell key: its cells live in static data, as
    /// the body runs once per instance ([`closure::Em::raw_cell`]).
    static_cells: Option<Hash128>,
    /// The references that take a slot local's value at its last access
    /// (`ref_word::TAKES`), each with its local: a loan, whose `Drop`
    /// releases the value in the slot, as the local's `Drop` the move
    /// pass deleted would have.
    slot_takes: HashMap<u32, usize>,
    /// With slots: the local holding the frame's address, and the
    /// frame's size.
    frame: Option<(u32, u32)>,
    /// One reusable caller-local host leaf, after ordinary addressable locals.
    drive_buffer: Option<(u32, u32)>,
    /// In an erased body, the local holding the address of its thunks'
    /// table, read from the witness (`erased.rs`).
    ops: Option<u32>,
    /// Whether the body is the ownership passes' output ([`Own::owned`]):
    /// emission then lowers their read modes, `Drop`s and exit set, and
    /// decides no count of its own. The sub-body it emits, whose exit set
    /// its exit drops.
    owned: bool,
    sub_id: u16,
    /// In an owned body, the instruction or block that consumes each
    /// value, or `NONE`, by the passes' rule (`hd_tir::own::consumers`).
    /// A consumer that cannot move the count in takes a count of its own,
    /// and the value is released after it ([`Em::release_untaken`]).
    consumer: Vec<u32>,
    /// The body's block structure.
    topo: hd_tir::ir::Topology,
    own_ops: Vec<hd_tir::own::Operand>,
    /// The hoisted `IsUnique`s met before the loop that follows them
    /// (`docs/compiler/tir-v2.md`, "Hoisting"), which run at that loop
    /// under its entry test ([`Em::hoisted_uniques`]).
    hoisted: Vec<u32>,
    /// While a hoisted `IsUnique` runs: a dereference on its place reads
    /// the reference's address, in a module no reference view reaches.
    deref_physical: bool,
    /// The `Read` right after a plain `IsUnique` of its place, and the
    /// local holding the buffer the check left ([`Em::own_unique`]).
    walked: Option<(u32, u32)>,
    /// In an outlined change's thunk: the local holding the buffer the
    /// change left, which the thunk returns for its caller to store
    /// ([`Em::put_back`]); `NONE` until the change leaves one.
    changed: Option<u32>,
    /// Each `block_on` whose cold dynamic call went through its drive
    /// slot (§23.39): the locals holding that call's results.
    driven: HashMap<u32, (Vec<u32>, u32)>,
}

impl<'i, 'a, 'm> Em<'i, 'a, 'm> {
    fn annotate_func(&self, f: &mut Func) {
        if self.m.track_origins {
            f.origins = Some(Vec::new());
            f.set_origin_source_len(self.b.tags.len());
            f.origin_item = Some((self.input.path)(self.b.item));
            f.origin_root = Some(self.origin_root);
            f.origin_at = f.origin_root;
        }
    }

    fn inherit_origin_source(&mut self) {
        if self.m.track_origins {
            self.f.origins = Some(Vec::new());
            self.f.set_origin_source_len(self.b.tags.len());
            self.f.origin_item = Some((self.input.path)(self.b.item));
            self.f.origin_root = Some(self.origin_root);
            self.f.origin_at = self.f.origin_root;
        }
    }

    /// An emitter of `b`'s instance of `item` at `args`, with no function
    /// yet.
    fn new(
        input: &'i Input<'a>,
        m: &'m mut Module,
        b: &'a Body,
        (item, args): (DefId, TyList),
        calls: &'i HashMap<u32, Target>,
        shared: HashMap<usize, u32>,
        direct_json: bool,
    ) -> Self {
        let f = m.func("", vec![], vec![]);
        let mut em = Em {
            input,
            m,
            b,
            inst_ty: hd_mono::instruction_types(input.pool, input.env, item, args, b),
            item,
            direct_json,
            origin_root: b.sub_root[0],
            args,
            calls,
            f,
            vals: HashMap::default(),
            locals: HashMap::default(),
            providers: Vec::new(),
            open_ctx: None,
            open_base: 0,
            ctl: Vec::new(),
            deciding: Vec::new(),
            loops: 0,
            counting: false,
            ret: Vec::new(),
            results: Vec::new(),
            scopes: HashMap::default(),
            defer_flags: HashMap::default(),
            loans: hd_base::HashSet::default(),
            loan_locals: HashMap::default(),
            projs: Vec::new(),
            view_loans: HashMap::default(),
            unowned_params: Vec::new(),
            list_caches: Vec::new(),
            cached_buffers: Vec::new(),
            proven_list_indices: Vec::new(),
            borrowed: hd_base::HashSet::default(),
            lends: 0,
            emitting: Vec::new(),
            moving: Vec::new(),
            susp: None,
            shield: 0,
            cancel_from: NONE,
            temps: Vec::new(),
            cells: shared,
            entry_cells: hd_base::HashSet::default(),
            slots: HashMap::default(),
            static_cells: None,
            slot_takes: HashMap::default(),
            frame: None,
            drive_buffer: None,
            ops: None,
            owned: false,
            sub_id: 0,
            consumer: Vec::new(),
            topo: hd_tir::ir::Topology::new(b),
            own_ops: Vec::new(),
            hoisted: Vec::new(),
            deref_physical: false,
            walked: None,
            changed: None,
            driven: HashMap::default(),
        };
        em.projs = (0..b.len()).map(|i| em.projected(i)).collect();
        em
    }
}

impl Em<'_, '_, '_> {
    fn pool(&self) -> &InternPool {
        self.input.pool
    }

    fn env(&self) -> &dyn ProgramEnv {
        self.input.env
    }

    /// Whether integer arithmetic wraps and shift counts are masked: in
    /// the release profile (`types.arith.release`), and in `std.rt`'s
    /// runtime modules, whose arithmetic stays in range by construction
    /// and which only std may use (`module.rt.std-only`).
    fn wraps(&self) -> bool {
        let p = (self.input.path)(self.item);
        self.input.release || p.starts_with("std/rt.") || p.starts_with("std/rt/")
    }

    fn sub(&self, t: Ty) -> Ty {
        subst(self.input.pool, self.input.env, self.item, self.args, t)
    }

    fn vals(&self, t: Ty) -> StageResult<Vec<Val>> {
        vals(self.input.pool, self.input.env, t)
    }

    fn rec(&self, at: u32) -> Vec<u32> {
        self.b.record(at).to_vec()
    }

    /// The type of value `r`, substituted; `NONE` is the unit value.
    /// What projection read `i` reads ([`shape::Projected`]): by the type
    /// of the value it projects from, a struct's, newtype's or data
    /// object's field, a tuple's element, or a payload field. A read
    /// through a reference, `[*%r .k]`, is none.
    fn projected(&self, i: usize) -> shape::Projected {
        use hd_tir::ir::Step;
        let Some((base, step)) = self.b.read_step(i) else {
            return shape::Projected::None;
        };
        let bt = self.ty_of(base);
        match step {
            Step::Field(_) if self.tuple(bt) => shape::Projected::Element,
            Step::Field(_) => shape::Projected::Field,
            Step::DerefField(_) if matches!(self.kind(bt), crate::layout::Kind::Data(_)) => {
                shape::Projected::Field
            }
            Step::DerefField(_) => shape::Projected::None,
            Step::Variant(..) => shape::Projected::Payload,
        }
    }

    /// Whether `t` is a tuple type, `mut` or not.
    fn tuple(&self, t: Ty) -> bool {
        matches!(
            self.pool()
                .get(crate::layout::strip(self.pool(), self.env(), t)),
            TyData::Tuple { .. }
        )
    }

    /// The value projection read `i` reads from, and what it reads.
    fn projection(&self, i: usize) -> Option<(u32, shape::Projected)> {
        shape::projection(self.b, i, &self.projs)
    }

    /// [`shape::read_through`] of instruction `i`.
    fn read_through(&self, i: usize) -> Option<(shape::Through, bool)> {
        shape::read_through(self.b, i, &self.projs)
    }

    fn ty_of(&self, r: u32) -> Ty {
        if r == NONE {
            return Ty::VOID;
        }
        match Ref(r).as_inst() {
            Some(i) => self.inst_ty[i.0 as usize],
            None => self.sub(self.b.consts[(r & !Ref::CONST_BIT) as usize].0),
        }
    }

    /// The type of instruction `i`, substituted.
    fn ty(&self, i: u32) -> Ty {
        self.inst_ty[i as usize]
    }

    /// The position of the first of `items`, a block's, that has type
    /// `never` and is followed by code that reads values, such as a
    /// `panic(..)` used as a call argument. Nothing after it runs, and the
    /// values that code would read, such as the call's other arguments,
    /// were never computed: the block ends there, with that item as its
    /// tail. A `break`, `continue` or `return` ends its path itself, and
    /// drops after an item read no value of it, so both stay as they are.
    fn dead_from(&self, items: &[u32], tail: u32) -> Option<usize> {
        let k = items.iter().position(|&i| {
            !matches!(
                self.b.tags[i as usize],
                Tag::Block | Tag::Break | Tag::Continue | Tag::Return | Tag::Unreachable
            ) && self.diverges(i)
        })?;
        let reads = (tail != NONE && tail != items[k])
            || items[k + 1..]
                .iter()
                .any(|&i| self.b.tags[i as usize] != Tag::Drop);
        reads.then_some(k)
    }

    /// The value block `blk` ends with: its tail, or the item
    /// [`Em::dead_from`] ends it at.
    fn live_tail(&self, blk: u32) -> u32 {
        let [list, tail] = self.b.data[blk as usize];
        let items = self.items(list);
        self.dead_from(&items, tail).map_or(tail, |k| items[k])
    }

    /// The instructions of block list `list`, without the blocks it holds,
    /// which run where the instruction that owns them runs.
    fn items(&self, list: u32) -> Vec<u32> {
        self.rec(list)
            .into_iter()
            .filter(|i| self.b.tags[*i as usize] != Tag::Block)
            .collect()
    }

    /// Whether value `r` never produces a value: its type is `never`.
    fn diverges(&self, r: u32) -> bool {
        self.pool().get(self.ty_of(r)) == TyData::Never
    }

    /// The arithmetic view of a type: its width, signedness and float kind.
    fn num(&self, t: Ty) -> (u8, bool, bool) {
        let t = hd_mono::layout::strip_newtype(self.pool(), self.env(), t);
        num(self.pool(), t)
    }

    /// Pushes every value of `r`. `NONE` is the unit value, which has none.
    fn push(&mut self, r: u32) -> StageResult<()> {
        if r == NONE {
            return Ok(());
        }
        if Ref(r).as_inst().is_none() {
            return self.konst(r);
        }
        let Some(ls) = self.vals.get(&r).cloned() else {
            return unsupported("a value its instruction did not compute");
        };
        for l in ls {
            self.f.get(l);
        }
        Ok(())
    }

    /// Pushes value `k` of `r`.
    fn comp(&mut self, r: u32, k: usize) -> StageResult<()> {
        if Ref(r).as_inst().is_none() {
            let Some(ops) = self.konst_ops(r)?.into_iter().nth(k) else {
                return unsupported("a component a constant does not have");
            };
            for op in ops {
                self.f.push(op);
            }
            return Ok(());
        }
        let Some(l) = self.vals.get(&r).and_then(|ls| ls.get(k)).copied() else {
            return unsupported("a value its instruction did not compute");
        };
        self.f.get(l);
        Ok(())
    }

    /// Pushes constant `r`.
    fn konst(&mut self, r: u32) -> StageResult<()> {
        for op in self.konst_ops(r)?.into_iter().flatten() {
            self.f.push(op);
        }
        Ok(())
    }

    /// The value of `r` when it is a constant of one `i64`.
    fn const_i64(&self, r: u32) -> Option<i64> {
        if r == NONE || Ref(r).as_inst().is_some() {
            return None;
        }
        let (ty, bits) = self.b.consts[(r & !Ref::CONST_BIT) as usize];
        matches!(self.vals(self.sub(ty)).ok()?.as_slice(), [Val::I64]).then(|| bits.cast_signed())
    }

    /// The value of `r` when it is a constant of one `i32`.
    fn const_u32(&self, r: u32) -> Option<u32> {
        if r == NONE || Ref(r).as_inst().is_some() {
            return None;
        }
        let (ty, bits) = self.b.consts[(r & !Ref::CONST_BIT) as usize];
        matches!(self.vals(self.sub(ty)).ok()?.as_slice(), [Val::I32])
            .then(|| u32::try_from(bits & 0xffff_ffff).expect("low bits"))
    }

    /// Constant `r`'s values, each as the instructions that push it: a
    /// string's bytes are static data, and the string is their address
    /// and length.
    fn konst_ops(&mut self, r: u32) -> StageResult<Vec<Vec<Ins>>> {
        let (ty, bits) = self.b.consts[(r & !Ref::CONST_BIT) as usize];
        let ty = self.sub(ty);
        let stripped = hd_mono::layout::strip_newtype(self.pool(), self.env(), ty);
        if stripped == Ty::STRING {
            let text = self.b.strings[usize::try_from(bits).expect("string constant")].clone();
            // Every empty string is the one static empty value (§23.7).
            let at = if text.is_empty() {
                crate::module::EMPTY
            } else {
                self.m.static_bytes(text.as_bytes(), 1)
            };
            self.f.mark_static(at);
            let len = u32::try_from(text.len()).expect("string constant");
            return Ok(vec![
                vec![Ins::Static(at)],
                vec![Ins::Op(I::I32Const(len.cast_signed()))],
            ]);
        }
        let low = u32::try_from(bits & 0xffff_ffff).expect("low bits");
        let ops = match self.vals(ty)?.as_slice() {
            [] => vec![],
            [Val::I32] => vec![vec![I::I32Const(low.cast_signed())]],
            [Val::I64] => vec![vec![I::I64Const(bits.cast_signed())]],
            // A float literal's constant holds its `f64` bits whatever
            // type the literal ends up with (`hd_check`'s literal).
            [Val::F32] => vec![vec![
                I::F64Const(wasm_encoder::Ieee64::new(bits)),
                I::F32DemoteF64,
            ]],
            [Val::F64] => vec![vec![I::F64Const(wasm_encoder::Ieee64::new(bits))]],
            _ => return unsupported(format!("a constant of type `{}`", self.pool().display(ty))),
        };
        Ok(ops
            .into_iter()
            .map(|ops| ops.into_iter().map(Ins::Op).collect())
            .collect())
    }

    /// Instruction `i`'s locals, made on first use.
    fn slot(&mut self, i: u32) -> StageResult<Vec<u32>> {
        if let Some(ls) = self.vals.get(&i) {
            return Ok(ls.clone());
        }
        let vs = self.vals(self.ty(i))?;
        self.f.stack_values |= vs.len() > crate::wide::WIDTH;
        let ls: Vec<u32> = vs.into_iter().map(|v| self.f.local(v)).collect();
        self.vals.insert(i, ls.clone());
        Ok(ls)
    }

    /// Coercion `i` of `a` that keeps its values: a consumed operand's
    /// locals become `i`'s, count and all, with nothing emitted; any other
    /// is copied with a count of its own. In a suspending body the operand
    /// keeps its locals, which a cancelled point's unwind may release.
    fn same_value(&mut self, i: u32, a: u32) -> StageResult<()> {
        if self.take(a)
            && self.susp.is_none()
            && !self.vals.contains_key(&i)
            && let Some(ls) = self.vals.remove(&a)
        {
            self.vals.insert(i, ls);
            return Ok(());
        }
        let moved = self.moving.contains(&(i, a));
        self.push(a)?;
        self.store_as(i, moved)
    }

    /// Stores the values on the stack as instruction `i`'s, a copy that
    /// takes a count of its own ([`Em::store_as`]).
    fn store(&mut self, i: u32) -> StageResult<()> {
        self.store_as(i, false)
    }

    /// TIR local `l`'s locals, made on first use.
    fn local(&mut self, l: u32) -> StageResult<Vec<u32>> {
        let l = l as usize;
        if let Some(ls) = self.locals.get(&l) {
            return Ok(ls.clone());
        }
        // A shared local is its cell's address (`closure.rs`).
        let vs = if self.is_cell(l) {
            vec![Val::I32]
        } else {
            self.vals(self.sub(self.b.local_ty[l]))?
        };
        self.f.stack_values |= vs.len() > crate::wide::WIDTH;
        let ls: Vec<u32> = vs.into_iter().map(|v| self.f.local(v)).collect();
        self.locals.insert(l, ls.clone());
        Ok(ls)
    }

    fn open(&mut self, c: Ctl) {
        self.f.op(Instruction::Block(BlockType::Empty));
        self.ctl.push(c);
    }

    fn close(&mut self) {
        self.ctl.pop();
        self.f.op(Instruction::End);
    }

    /// The branch depth of the open block `c`.
    fn depth(&self, c: Ctl) -> StageResult<u32> {
        match self.ctl.iter().rev().position(|x| *x == c) {
            Some(d) => Ok(u32::try_from(d).expect("depth")),
            None => unsupported("a branch to a block that is not open"),
        }
    }

    /// Emits block `blk`'s instructions and stores its tail as `dest`'s
    /// value. Blocks in the list are owned by an instruction, which emits
    /// them.
    fn block_into(&mut self, blk: u32, dest: Option<u32>) -> StageResult<()> {
        if blk == NONE {
            return Ok(());
        }
        if self.b.tags[blk as usize] != Tag::Block {
            return unsupported("a block operand that is not a block");
        }
        let [list, mut tail] = self.b.data[blk as usize];
        let labelled = self.topo.label_of[blk as usize] != NONE;
        if labelled {
            self.open(Ctl::Brk(blk));
        }
        let mut items = self.items(list);
        if let Some(k) = self.dead_from(&items, tail) {
            tail = items[k];
            items.truncate(k + 1);
        }
        if self.range(blk).is_some() {
            self.resume_list(&items)?;
        } else {
            for i in items {
                self.inst(i)?;
            }
        }
        if !self.hoisted.is_empty() {
            self.hoisted_uniques(None)?;
        }
        if tail != NONE
            && let Some(d) = dest
        {
            self.tail_into(tail, d, blk)?;
        } else if tail != NONE
            && self.owned
            && self.consumer.get(tail as usize) == Some(&blk)
            && !self.topo.is_root[blk as usize]
        {
            // A tail the passes gave to a block whose value no one reads;
            // a sub-body's root's is its result, which `body_func` takes.
            self.release_value(tail)?;
        }
        if labelled {
            self.close();
            if let Some(d) = dest {
                // A break out of the block stored its value in the
                // block's own locals; in an owned body its count moves on.
                if d != blk && self.vals.contains_key(&blk) {
                    let ls = self.slot(blk)?;
                    for l in &ls {
                        self.f.get(*l);
                    }
                    if self.owned {
                        self.store_owned(d)?;
                        for l in ls {
                            self.f.zero(l);
                        }
                    } else {
                        self.store(d)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Stores value `r`, which instruction or block `user` reads, as
    /// `dest`'s, or ends the path when `r` diverges.
    fn tail_into(&mut self, r: u32, dest: u32, user: u32) -> StageResult<()> {
        // A value with no parts, such as a field-less struct, is computed
        // with no locals; a later read of `dest` pushes nothing.
        if self.vals(self.ty(dest))?.is_empty() {
            self.slot(dest)?;
            return Ok(());
        }
        if self.diverges(r) {
            self.f.op(Instruction::Unreachable);
            return Ok(());
        }
        // A break's value moves at its only use; a block's tail, used
        // only as the block's value, moves into it. In an owned body, the
        // passes said which.
        let moved = self.owned && self.take_from(r, user);
        self.push(r)?;
        if moved {
            self.store_owned(dest)?;
            self.settle_value(r);
            Ok(())
        } else {
            self.store(dest)
        }
    }

    /// Emits instruction `i`; the operands whose counts it took are zeroed
    /// once it has read them ([`Em::take`]).
    fn inst(&mut self, i: u32) -> StageResult<()> {
        // A hoisted `IsUnique` waits for its loop; anything else first
        // runs any that wait, unguarded (a body a default argument's
        // specialization folded may have lost the loop's test).
        if !self.hoisted.is_empty() && !matches!(self.b.tags[i as usize], Tag::Loop | Tag::IsUnique)
        {
            self.hoisted_uniques(None)?;
        }
        // An instruction with a source position anchors the calls it
        // writes, for a panic's frames (`crates/hd_wasm_linear/DESIGN.md` §15.9);
        // the enclosing anchor holds again after it.
        let outer = self.f.at;
        let origin_outer = self.f.origin_at;
        if self.anchored(i) {
            // The checker's instruction, which the `check` entry the
            // anchor is resolved against numbers (`Body::anchors`).
            let at = self.b.anchor(i);
            if at != NONE {
                let (frame, outer) = self.frame_anchor(at);
                self.f.at = Some(frame);
                self.f.origin_at = Some(outer);
            }
        }
        self.emitting.push(i);
        let mut r = match self.walked.take() {
            Some((x, h)) if x == i => self.read_walked(i, h),
            _ => self.inst_body(i),
        };
        self.emitting.pop();
        self.f.at = outer;
        if r.is_ok() && self.owned {
            r = self.release_untaken(i);
        }
        self.settle(i);
        self.f.origin_at = origin_outer;
        r
    }

    /// The anchor a call at body anchor `at` writes, and the body's own
    /// anchor it stands in: an inlined copy's instruction is an entry of
    /// the function's [`Func::inlined`] frames, whose parent is the call's.
    fn frame_anchor(&mut self, at: u32) -> (u32, u32) {
        let Some((callee, inst, call)) = self.b.inlined_frame(at) else {
            return (at, at);
        };
        let (parent, outer) = self.frame_anchor(call);
        let frame = ((self.input.path)(callee), inst, parent);
        let t = &mut self.f.inlined;
        let k = t.iter().position(|e| *e == frame).unwrap_or_else(|| {
            t.push(frame);
            t.len() - 1
        });
        (
            hd_tir::ir::INLINED | u32::try_from(k).expect("frames"),
            outer,
        )
    }

    /// Whether instruction `i` has a source position, so it anchors calls.
    fn anchored(&self, i: u32) -> bool {
        self.b
            .syn
            .get(i as usize)
            .is_some_and(|n| *n != hd_base::NodeIdx::NONE)
    }

    fn inst_body(&mut self, i: u32) -> StageResult<()> {
        let [a, bw] = self.b.data[i as usize];
        // An erased body's open instruction calls its thunk (`erased.rs`);
        // a tag switch outlines only its tag read.
        if let Some(&Target::Open(k)) = self.calls.get(&i)
            && self.b.tags[i as usize] != Tag::SwitchTag
        {
            return self.open_inst(i, k);
        }
        match self.b.tags[i as usize] {
            Tag::Read | Tag::Store if let Some(p) = self.b.ptr_target(i as usize) => {
                self.ptr_place(i, p)?;
            }
            // A plain local read, `Read [$l]`.
            Tag::Read if let Some(l) = self.b.local_read(i as usize) => {
                self.local_read(i, l.idx())?;
            }
            // A move of a whole value, `Read move [%v]`: its count, and
            // zeros left in its locals.
            Tag::Read
                if self.owned
                    && bw == hd_tir::ir::ReadMode::Move as u32
                    && let Some(r) = self.b.value_read(i as usize) =>
            {
                self.push(r)?;
                self.store_owned(i)?;
                if self.counted(self.ty_of(r)).is_some() {
                    for l in self.slot(r)? {
                        self.f.zero(l);
                    }
                }
            }
            // A move of a part of a local or a value laid out in its
            // locals; any other part is read as a copy below, and the
            // root's drop releases the rest.
            Tag::Read
                if self.owned
                    && bw == hd_tir::ir::ReadMode::Move as u32
                    && self.own_move_part(i)? => {}
            // A plain read of a top-level binding, `Read [@g]`.
            Tag::Read if let Some(def) = self.b.global_read(i as usize) => {
                self.global_read(i, def)?;
            }
            // A projection read: a field, a tuple element or a payload
            // field of a value, or a data object's field.
            Tag::Read if let Some((base, p)) = self.projection(i as usize) => {
                self.projection_read(i, base, p)?;
            }
            // A copy of a whole value, `Read copy [%v]`, which the
            // ownership passes write for a consumer of a value used more
            // than once: one count more.
            Tag::Read if let Some(r) = self.b.value_read(i as usize) => {
                self.push(r)?;
                self.store(i)?;
            }
            // The ownership passes' statements, in an owned body
            // (`count.rs`); the checker's TIR has none (invariant 22).
            Tag::Drop if self.owned => self.own_drop(i)?,
            Tag::IsUnique => self.own_unique(i)?,
            Tag::Pin => self.own_pin(i)?,
            Tag::Drop | Tag::FlagSet | Tag::FlagClear => {
                return unsupported(format!(
                    "an ownership statement `{}` the backend does not lower here",
                    self.b.tags[i as usize].name()
                ));
            }
            // Places (`emit/refs.rs`).
            Tag::Ref => self.place_ref(i)?,
            Tag::Read => self.place_read(i)?,
            Tag::Store => self.place_store(i)?,
            // The address form is the trait value its block makes.
            Tag::RefDyn => self.block_into(a, Some(i))?,
            Tag::RefIdentity => self.ref_identity(i, a)?,
            Tag::Prim => self.prim(i, a, bw)?,
            Tag::And | Tag::Or => {
                let r = self.slot(i)?[0];
                self.push(a)?;
                if self.b.tags[i as usize] == Tag::Or {
                    self.f.op(Instruction::I32Eqz);
                }
                self.resume_branch(i, bw);
                self.f.op(Instruction::If(BlockType::Empty));
                self.ctl.push(Ctl::Plain);
                self.block_into(bw, Some(i))?;
                self.f.op(Instruction::Else);
                self.f.i32(i32::from(self.b.tags[i as usize] == Tag::Or));
                self.f.set(r);
                self.close();
            }
            Tag::Call => self.call(i, a, bw)?,
            Tag::DefaultCall => self.default_call(i, bw)?,
            Tag::Interp => {
                let parts = self.rec(bw);
                self.concat(i, &parts)?;
            }
            Tag::Coerce => {
                let kind = self.rec(bw).first().copied().unwrap_or(NONE);
                match kind {
                    k if k == Coercion::Never as u32 => {
                        self.f.op(Instruction::Unreachable);
                        self.slot(i)?;
                    }
                    k if [
                        Coercion::Weaken,
                        Coercion::Variance,
                        Coercion::RowSubsume,
                        Coercion::SuspendFnToCtor,
                        Coercion::Identity,
                    ]
                    .iter()
                    .any(|c| *c as u32 == k) =>
                    {
                        if self.vals(self.ty_of(a))? != self.vals(self.ty(i))? {
                            return unsupported("a coercion that changes the layout");
                        }
                        // A function value's code looks its row up in the
                        // context its caller passes, so a row that changes
                        // changes nothing (`ctx.rs`). The value is the
                        // operand's, which it consumes: it takes the
                        // operand's count, or none of a borrow.
                        self.same_value(i, a)?;
                    }
                    k if k == Coercion::WrapSome as u32 => {
                        self.new_variant(i, 1, &[value::Src::Ref(a)])?;
                    }
                    k if [
                        Coercion::ToTraitValue,
                        Coercion::Supertrait,
                        Coercion::RefToTraitValue,
                    ]
                    .iter()
                    .any(|c| *c as u32 == k) =>
                    {
                        self.trait_value(i, a)?;
                    }
                    k if k == Coercion::RefAsProvider as u32 => self.ref_as_provider(i, a)?,
                    // The provider is the reference itself.
                    k if k == Coercion::ProviderAsRef as u32 => self.same_value(i, a)?,
                    _ => return unsupported("provider references and `Any` coercions"),
                }
            }
            Tag::ProviderGet => {
                let key = self.sub(Ty(self.rec(a).first().copied().unwrap_or(NONE)));
                let [d, v] = self.provider(key)?;
                self.f.get(d);
                self.f.get(v);
                self.store(i)?;
            }
            Tag::If => {
                let r = self.rec(bw);
                self.slot(i)?;
                self.push(a)?;
                self.resume_branch(i, r[0]);
                self.f.op(Instruction::If(BlockType::Empty));
                self.ctl.push(Ctl::Plain);
                self.block_into(r[0], Some(i))?;
                if let Some(&els) = r.get(1).filter(|e| **e != NONE) {
                    self.f.op(Instruction::Else);
                    self.block_into(els, Some(i))?;
                }
                self.close();
            }
            Tag::Loop => {
                self.slot(i)?;
                let caches = self.begin_list_loop(i)?;
                self.open(Ctl::Brk(i));
                self.f.op(Instruction::Loop(BlockType::Empty));
                self.ctl.push(Ctl::Cont(i));
                self.loops += 1;
                self.block_into(a, None)?;
                self.loops -= 1;
                self.f.op(Instruction::Br(0));
                self.close();
                self.close();
                for root in caches {
                    self.list_caches.retain(|(key, _)| *key != root);
                }
            }
            Tag::Break | Tag::Continue => {
                let Some(&target) = self.b.label_inst.get(a as usize) else {
                    return unsupported("a break with an unknown label");
                };
                if self.b.tags[i as usize] == Tag::Break {
                    if bw != NONE {
                        self.tail_into(bw, target, i)?;
                    }
                    self.settle(i);
                    self.exit(scope::Exit::Brk(target))?;
                } else {
                    self.exit(scope::Exit::Cont(target))?;
                }
            }
            Tag::Return => {
                if a != NONE && self.diverges(a) {
                    self.f.op(Instruction::Unreachable);
                    return Ok(());
                }
                self.ret_value(a)?;
            }
            Tag::Unreachable => self.f.op(Instruction::Unreachable),
            Tag::Scope if !self.rec(bw).is_empty() => self.scope(i, a, bw)?,
            Tag::Defer => self.defer(a)?,
            Tag::Scope => {
                self.slot(i)?;
                self.open(Ctl::Plain);
                self.block_into(a, Some(i))?;
                self.close();
            }
            Tag::Match => {
                let blocks = self.rec(bw);
                let n = u32::try_from(blocks.len() - 1).expect("arms");
                self.slot(i)?;
                self.open(Ctl::Brk(i));
                for k in (0..n).rev() {
                    self.open(Ctl::Arm(i, k));
                }
                self.resume_arms(i, &blocks[1..])?;
                self.deciding.push(i);
                self.block_into(blocks[0], None)?;
                self.deciding.pop();
                self.f.op(Instruction::Unreachable);
                for arm in &blocks[1..] {
                    self.close();
                    self.block_into(*arm, Some(i))?;
                    let d = self.depth(Ctl::Brk(i))?;
                    self.f.op(Instruction::Br(d));
                }
                self.close();
            }
            Tag::ToArm => {
                let Some(&m) = self.deciding.last() else {
                    return unsupported("an arm jump outside a match decision");
                };
                let d = self.depth(Ctl::Arm(m, a))?;
                self.f.op(Instruction::Br(d));
            }
            Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar => {
                let r = self.rec(bw);
                let [case, then, other] = r[..] else {
                    return unsupported("a switch that is not one case and a default");
                };
                if self.b.tags[i as usize] != Tag::SwitchTag
                    && self.vals(self.ty_of(a))? != [Val::I32]
                {
                    return unsupported("a switch on a 64-bit integer");
                }
                if let Some(&Target::Open(k)) = self.calls.get(&i) {
                    self.open_call(i, k)?;
                    self.f.u32(case);
                    self.f.op(Instruction::I32Eq);
                } else if self.b.tags[i as usize] == Tag::SwitchTag && self.nullable(self.ty_of(a))
                {
                    // Variant 1 is a non-null address.
                    self.comp(a, 0)?;
                    if case == 0 {
                        self.f.op(Instruction::I32Eqz);
                    }
                } else {
                    if self.b.tags[i as usize] == Tag::SwitchTag {
                        self.tag(a)?;
                    } else {
                        self.comp(a, 0)?;
                    }
                    self.f.u32(case);
                    self.f.op(Instruction::I32Eq);
                }
                self.resume_branch(i, then);
                self.f.op(Instruction::If(BlockType::Empty));
                self.ctl.push(Ctl::Plain);
                self.block_into(then, Some(i))?;
                self.f.op(Instruction::Else);
                if other == NONE {
                    self.f.op(Instruction::Unreachable);
                } else {
                    self.block_into(other, Some(i))?;
                }
                self.close();
            }
            Tag::NewVariant => {
                let tys = crate::layout::variants(self.pool(), self.env(), self.ty(i))
                    .and_then(|vs| vs.get(a as usize).cloned())
                    .unwrap_or_default();
                let payload: Vec<value::Src> =
                    self.rec(bw).into_iter().map(value::Src::Ref).collect();
                if payload.len() != tys.len() {
                    return unsupported("a variant built from other fields than it has");
                }
                self.new_variant(i, a, &payload)?;
            }
            Tag::Intrinsic => {
                let op = hd_tir::ir::IntrinsicOp::from_u32(a);
                let args = self.rec(bw);
                match op {
                    Some(IntrinsicOp::ListLen) => self.list_len(i, args[0])?,
                    Some(IntrinsicOp::ListPush) => self.list_push(&args)?,
                    Some(IntrinsicOp::ListReserve) => self.list_reserve(&args)?,
                    Some(hd_tir::ir::IntrinsicOp::StrConcat) => self.concat(i, &args)?,
                    Some(hd_tir::ir::IntrinsicOp::StrIndex) => self.str_at(i, args[0], args[1])?,
                    Some(hd_tir::ir::IntrinsicOp::StrEq) => {
                        self.push(args[0])?;
                        self.push(args[1])?;
                        self.f.helper(Helper::StrEq);
                        self.store(i)?;
                    }
                    Some(IntrinsicOp::MapLen) => self.map_len(i, args[0])?,
                    Some(
                        op @ (IntrinsicOp::MapGet
                        | IntrinsicOp::MapSet
                        | IntrinsicOp::MapRemove
                        | IntrinsicOp::MapEntry),
                    ) => self.map_op(i, op, &args)?,
                    Some(IntrinsicOp::MapSlots) => self.map_slots(i, args[0])?,
                    Some(IntrinsicOp::MapSlotNext) => self.map_slot_next(i, &args)?,
                    Some(IntrinsicOp::MapSlotEntry) => self.map_slot_entry(i, args[0], args[1])?,
                    _ => return unsupported("an intrinsic operation without its kernel"),
                }
            }
            Tag::Hook => {}
            Tag::NewStruct => self.new_data(i, bw)?,
            Tag::CopyStruct => self.copy_data(i, a, bw)?,
            Tag::NewTuple => {
                // The rest list, if any, is the last member.
                let mut members = self.rec(bw);
                if a != NONE {
                    members.push(a);
                }
                let tys = crate::layout::tuple_members(
                    self.pool(),
                    crate::layout::strip(self.pool(), self.env(), self.ty(i)),
                );
                let srcs: Vec<value::Src> = members.into_iter().map(value::Src::Ref).collect();
                // Construction consumes its parts (§23.25).
                let srcs = self.count_parts(&srcs, &tys)?;
                for (s, mt) in srcs.iter().zip(tys) {
                    for k in 0..self.vals(mt)?.len() {
                        self.src(s, k)?;
                    }
                }
                self.store_owned(i)?;
            }
            Tag::Is => self.is(i, a, bw)?,
            Tag::NewList => self.new_list(i, a, bw)?,
            Tag::NewMap => self.new_map(i, bw)?,
            Tag::Closure => self.closure(i)?,
            Tag::ItemRef => {
                let Some(Target::Closure(key)) = self.calls.get(&i).cloned() else {
                    return unsupported("a function reference without its adapter");
                };
                self.f.table_slot(Sym::Inst(key));
                self.f.i32(0);
                self.store_owned(i)?;
            }
            Tag::CallValue => {
                let args = self.rec(bw);
                self.call_value(i, a, &args)?;
            }
            Tag::CallDyn => return unsupported("a `CallDyn` instruction"),
            Tag::Await | Tag::AwaitValue | Tag::AwaitAll | Tag::AwaitRace => {
                self.point(i, a, bw)?;
            }
            Tag::With => self.with(i, a, bw)?,
            Tag::ContextNew => self.context_new(i, a)?,
            // The condition block's instructions, then its `bool` tail.
            Tag::Guard => {
                let r = self.rec(bw);
                let c = self.f.local(Val::I32);
                self.entered(a, |em| {
                    // Conditions own temporaries like every other block:
                    // resume at the pending statement and run its drops.
                    em.block_into(a, Some(a))?;
                    em.push(a)?;
                    em.f.set(c);
                    Ok(())
                })?;
                self.f.get(c);
                self.resume_branch(i, r[0]);
                self.f.op(Instruction::If(BlockType::Empty));
                self.ctl.push(Ctl::Plain);
                self.block_into(r[0], None)?;
                self.f.op(Instruction::Else);
                self.block_into(r[1], None)?;
                self.close();
            }
            tag => return unsupported(format!("`{}` (job 3, #540)", tag.name())),
        }
        Ok(())
    }

    /// `Read [$l]` `i`: local `l`'s values, from its cell or its Wasm
    /// locals, as the read's mode says ([`Em::own_local_read`]); a thunk's
    /// read is a copy with its own count.
    fn local_read(&mut self, i: u32, l: usize) -> StageResult<()> {
        if self.owned {
            return self.own_local_read(i, l);
        }
        if self.is_cell(l) {
            return self.cell_get(i, l);
        }
        for x in self.local(u32::try_from(l).expect("locals"))? {
            self.f.get(x);
        }
        self.store(i)
    }

    /// Projection read `i` of `base` ([`shape::Projected`]): a struct's,
    /// newtype's or data object's field, a tuple's element, or a field of
    /// a payload.
    fn projection_read(&mut self, i: u32, base: u32, p: shape::Projected) -> StageResult<()> {
        use hd_tir::ir::Step;
        let Some((_, step)) = self.b.read_step(i as usize) else {
            return unsupported("a projection read without its step");
        };
        let data = matches!(self.kind(self.ty_of(base)), crate::layout::Kind::Data(_));
        match (p, step) {
            (shape::Projected::Field, Step::Field(_)) if data => {
                unsupported("a data object's field read without its dereference")
            }
            (shape::Projected::Field, Step::Field(k) | Step::DerefField(k)) => {
                self.field(i, base, k)
            }
            (shape::Projected::Element, Step::Field(k)) => self.tuple_get(i, base, k),
            (shape::Projected::Payload, Step::Variant(v, k)) => self.payload(i, base, v, k),
            _ => unsupported("a projection read of the wrong kind of value"),
        }
    }

    /// `Read [@def]` `i`: the binding's static slot when a reference
    /// refers to it, else its Wasm globals.
    fn global_read(&mut self, i: u32, def: DefId) -> StageResult<()> {
        if let Some(at) = self.def_slot(def) {
            return self.slot_get(i, at);
        }
        let h = self.def_hash(def);
        for (k, v) in self.vals(self.ty(i))?.into_iter().enumerate() {
            self.f
                .global_get(Global::Storage(h, u32::try_from(k).expect("k"), v));
        }
        self.store(i)
    }

    /// The storage hash of top-level binding `def`, named for development
    /// tools when they track origins.
    fn def_hash(&mut self, def: DefId) -> hd_base::Hash128 {
        let h = self.env().path_hash(def);
        if self.m.track_origins {
            let path = (self.input.path)(def);
            self.m.storage_names.entry(h).or_insert(path);
        }
        h
    }

    /// `$.with(K: v, ...) body` (`req.with.*`): each pair's value is the
    /// provider of its key inside the body. The provider borrows the
    /// value's locals, whose count lasts to the function's exit.
    fn with(&mut self, i: u32, rec: u32, body: u32) -> StageResult<()> {
        let pairs = self.rec(rec);
        let mut pushed = 0;
        for c in pairs.chunks(2) {
            let key = self.sub(Ty(c[0]));
            // A spread context's providers, key by key.
            if matches!(self.pool().get(key), TyData::Context(_)) {
                pushed += self.push_context(c[1], key)?;
                continue;
            }
            if !matches!(self.pool().get(key), TyData::TraitValue { .. }) {
                return unsupported("a `$.with` key that is not a trait");
            }
            let ls = [self.f.local(Val::I32), self.f.local(Val::I32)];
            for (k, l) in ls.iter().enumerate() {
                self.comp(c[1], k)?;
                self.f.set(*l);
            }
            self.providers.push((key, ls));
            pushed += 1;
        }
        self.slot(i)?;
        self.open(Ctl::Plain);
        let r = self.block_into(body, Some(i));
        self.close();
        self.providers.truncate(self.providers.len() - pushed);
        r
    }

    /// The parameter types of plain callee `item` at `args`, instance
    /// `key`, by which it takes its arguments (`layout::Pass`); `None` for
    /// a suspending callee, whose cold constructor takes a count of each
    /// argument itself, so they are all lent to it.
    pub(super) fn param_tys(
        &self,
        key: hd_base::Hash128,
        item: DefId,
        args: TyList,
    ) -> Option<Vec<Ty>> {
        if self.env().suspends(item) {
            return None;
        }
        Some(callee_params(self.input, self.m, key, item, args))
    }

    /// How a plain function takes a parameter of type `t`.
    pub(super) fn pass(&self, t: Ty) -> Pass {
        crate::layout::pass(self.pool(), self.env(), self.input.runtime.is_some(), t)
    }

    /// How an instance whose release summary lends the parameters in
    /// `lends` (`ProgramEnv::lends`) takes its parameter `k`, of type `t`:
    /// [`Em::pass`], but borrowed where it is lent.
    pub(super) fn pass_at(&self, lends: u64, k: usize, t: Ty) -> Pass {
        match self.pass(t) {
            Pass::Owned(_) if k < 64 && lends & (1 << k) != 0 => Pass::Borrowed,
            p => p,
        }
    }

    /// Pushes a call's arguments `args` for a callee with parameters of
    /// types `params` (`None`: all lent). Each argument the callee owns
    /// moves in at its only use ([`Em::take`]) or takes a count of its own;
    /// a borrowed one is lent. Arguments spread from a tuple do not line up
    /// with the parameters one to one: each owned parameter's values then
    /// take a count, wherever they come from.
    pub(super) fn push_args(
        &mut self,
        args: &[u32],
        params: Option<&[Ty]>,
        lends: u64,
    ) -> StageResult<()> {
        let Some(params) = params else {
            for r in args {
                self.push(*r)?;
            }
            return Ok(());
        };
        if params.len() == args.len() {
            for (k, (r, t)) in args.iter().zip(params).enumerate() {
                if matches!(self.pass_at(lends, k, *t), Pass::Owned(_)) {
                    let s = self.consumed(*r)?;
                    self.push_src(&s)?;
                } else {
                    self.push(*r)?;
                }
            }
            return Ok(());
        }
        // Each value of the arguments, as its argument and index.
        let mut flat = Vec::new();
        for r in args {
            for k in 0..self.vals(self.ty_of(*r))?.len() {
                flat.push((*r, k));
            }
        }
        let mut at = 0;
        for (k, t) in params.iter().enumerate() {
            let vs = self.vals(*t)?;
            let Some(part) = flat.get(at..at + vs.len()) else {
                return unsupported("a call whose arguments do not fill its parameters");
            };
            if let Pass::Owned(walk) = self.pass_at(lends, k, *t) {
                for &(r, k) in part {
                    self.comp(r, k)?;
                }
                self.f.helper(Helper::Retain {
                    vals: vs.clone(),
                    walk,
                });
            }
            at += vs.len();
        }
        if at != flat.len() {
            return unsupported("a call whose arguments do not fill its parameters");
        }
        for r in args {
            self.push(*r)?;
        }
        Ok(())
    }

    /// Stores a call's results as `i`'s, or ends the path when its result
    /// is `never`.
    fn call_results(&mut self, i: u32, ret: Ty) -> StageResult<()> {
        if self.vals(ret)? != self.vals(self.ty(i))? {
            return unsupported("a call whose result is erased");
        }
        if self.pool().get(self.ty(i)) == TyData::Never {
            self.f.op(Instruction::Unreachable);
            return Ok(());
        }
        // A function returns its result with a count for the caller.
        self.store_owned(i)
    }

    fn call(&mut self, i: u32, a: u32, bw: u32) -> StageResult<()> {
        let mut args = self.rec(bw);
        args.truncate(args.len().saturating_sub(3));
        match self.calls.get(&i).cloned() {
            Some(Target::Call(ct)) => match &ct.kind {
                TargetKind::Instance => {
                    if self.direct_json && self.input.json_fallbacks.contains(&ct.item) {
                        self.f.op(Instruction::Unreachable);
                        return self.call_results(i, ct.ret);
                    }
                    let params = self.param_tys(ct.key, ct.item, ct.args);
                    let lends = self.env().lends(ct.item);
                    self.push_args(&args, params.as_deref(), lends)?;
                    let c = self.push_call_providers(ct.item, ct.args)?;
                    self.f.call(Sym::Inst(ct.key));
                    // A suspending callee called cold returns its frame.
                    let ret = if self.env().suspends(ct.item) {
                        self.ty(i)
                    } else {
                        ct.ret
                    };
                    self.call_results(i, ret)?;
                    self.release_ctx(c);
                    Ok(())
                }
                TargetKind::Intrinsic(key) if key == "type_id_of" => self.type_id_of(i, ct.args),
                TargetKind::Intrinsic(key)
                    if matches!(key.as_str(), "downcast_ref" | "downcast_mut") =>
                {
                    self.downcast(i, ct.item, args[0])
                }
                TargetKind::Intrinsic(key) if key.starts_with("ptr_") => {
                    self.ptr(i, key, ct.args, &args)
                }
                TargetKind::Intrinsic(key) => self.intrinsic(i, key, &args),
                TargetKind::Builtin { self_ty, .. } => {
                    let path = (self.input.path)(ct.item);
                    let name = path.rsplit(['.', '/']).next().unwrap_or("").to_owned();
                    self.builtin(i, &name, *self_ty, &args)
                }
            },
            Some(Target::Dyn) => self.call_dyn(i, a, &args),
            _ => unsupported("a call without a direct target (job 11)"),
        }
    }

    /// A trait method call at a trait-value receiver `args[0]`: an
    /// indirect call through the slot of the receiver's vtable that the
    /// method's position in the trait names.
    fn call_dyn(&mut self, i: u32, a: u32, args: &[u32]) -> StageResult<()> {
        let ty = self.ty(i);
        if let Some(on) = self.drive_target(i, a) {
            return self.call_drive(i, a, args, on);
        }
        self.push_dyn(a, args, Some(ty), None)?;
        self.call_results(i, ty)
    }

    /// The `block_on` that drives cold dynamic call `i` at once through
    /// the method's drive slot (§23.39), if one does.
    fn drive_target(&self, i: u32, a: u32) -> Option<u32> {
        let (trait_, method) = crate::drive::callee(self.b, a)?;
        if !self.m.blocking_driven.contains(&method)
            || !crate::drive::drivable(self.env(), trait_, method)
        {
            return None;
        }
        crate::drive::block_on_of(self.b, &self.topo, self.calls, i)
    }

    /// Cold dynamic call `i`, which `block_on` `on` drives at once
    /// (§23.39): the forbidden-context check `on` would make, then the
    /// drive slot. The call's value is the frame, 0 when the call is done;
    /// its results wait in locals for `on`.
    fn call_drive(&mut self, i: u32, a: u32, args: &[u32], on: u32) -> StageResult<()> {
        let inner = self.susp_inner(self.ty(i))?;
        let res: Vec<u32> = self.vals(inner)?.iter().map(|v| self.f.local(*v)).collect();
        self.f.helper(Helper::ForbidCheck);
        let buffer = self.blocking_buffer(args)?;
        self.push_dyn(a, args, None, Some((inner, Some(buffer))))?;
        for l in res.iter().rev() {
            self.f.set(*l);
        }
        self.driven.insert(on, (res, buffer));
        self.store_owned(i)
    }

    /// Reserve a leaf in the running Wasm invocation's shadow frame. A synchronous
    /// drive cannot suspend that invocation, so later calls may reuse this storage.
    fn blocking_buffer(&mut self, args: &[u32]) -> StageResult<u32> {
        let mut values = 0;
        for arg in args.iter().skip(1) {
            values += self.vals(self.ty_of(*arg))?.len();
        }
        let capacity =
            (crate::module::HEADER + crate::task::host_frame_capacity(values)).next_multiple_of(8);
        let (mut base, size) = self.frame.unwrap_or((NONE, 0));
        let (offset, previous) = self.drive_buffer.unwrap_or((size, 0));
        let capacity = previous.max(capacity);
        if base == NONE {
            base = self.f.local(Val::I32);
        }
        self.frame = Some((base, offset + capacity));
        self.drive_buffer = Some((offset, capacity));
        let buffer = self.f.local(Val::I32);
        self.f.get(base);
        self.f.u32(offset + crate::module::HEADER);
        self.f.op(Instruction::I32Add);
        self.f.set(buffer);
        Ok(buffer)
    }

    /// Pushes a dynamic call's results, of type `result`, or its cold frame
    /// when `result` is `None`: callee record `a` dispatched through its
    /// receiver's vtable with `args`, the receiver first. A generic
    /// method's slot takes its open arguments as open values and the
    /// witness of the call's type arguments, and an open result comes
    /// back as an open value (`erased.rs`).
    fn push_dyn(
        &mut self,
        a: u32,
        args: &[u32],
        result: Option<Ty>,
        drive: Option<(Ty, Option<u32>)>,
    ) -> StageResult<()> {
        let Some(Callee::TraitMethod {
            trait_,
            method,
            targs,
            ..
        }) = Callee::from_words(self.b.record(a))
        else {
            return unsupported("a dynamic call without a trait method");
        };
        let Some(slot) = self
            .env()
            .trait_methods(trait_)
            .iter()
            .position(|m| *m == method)
        else {
            return unsupported("a dynamic call of a method without a slot");
        };
        let takes_ctx = ctx::slot_ctx(self.env(), method);
        if !takes_ctx && !self.env().row_keys(method, targs).is_empty() {
            return unsupported("a dynamic call of a method whose row is a trait's row parameter");
        }
        let open = self.dyn_open(trait_, method, targs)?;
        if drive.is_some() && open.is_some() {
            return unsupported("a drive slot of a generic method");
        }
        // A suspension kept as a value is polled where its result type is
        // concrete: a frame over the erased one unboxes its open result.
        let kept = result.is_some() && self.env().suspends(method);
        let results = match (result, &open, drive) {
            // The drive slot (§23.39): the frame or 0, then the results.
            (_, _, Some((t, _))) => [vec![Val::I32], self.vals(t)?].concat(),
            (Some(_), Some(o), _) if o.ret => vec![Val::I32],
            (Some(t), _, _) => self.vals(t)?,
            (None, _, _) => vec![Val::I32],
        };
        let slot = match drive {
            Some((_, buffer)) => {
                crate::drive::index(self.env(), self.m, trait_, method, buffer.is_some()) as usize
            }
            None => slot,
        };
        let Some((&recv, rest)) = args.split_first() else {
            return unsupported("a dynamic call without a receiver");
        };
        let receiver_ty = self.strip_mut(self.ty_of(recv));
        let value_ty = self.dyn_value_ty(receiver_ty);
        let (payload, owned) = if value_ty == receiver_ty {
            (None, false)
        } else {
            let (_, ls, owned) = self.read_dyn_value(recv)?;
            (Some(ls), owned)
        };
        let mut params = vec![Val::I32];
        if let Some(value) = &payload {
            self.f.get(value[0]);
        } else {
            self.comp(recv, 0)?;
        }
        // Open arguments cross as open values the call owns until it
        // has returned; a copied reference is written back then.
        let mut boxed = Vec::new();
        let mut backs = Vec::new();
        for (j, r) in rest.iter().enumerate() {
            if let Some(et) = open
                .as_ref()
                .and_then(|o| o.params.get(j).copied().flatten())
            {
                let ls = self.local_value_of(*r)?;
                let returns = !self.env().suspends(method);
                let bk = returns.then_some(&mut backs);
                if let Some(w) = self.erase_arg(&ls, et, self.ty_of(*r), bk)? {
                    self.f.get(w);
                    params.push(Val::I32);
                    continue;
                }
                let o = self.erase(&ls, et, self.ty_of(*r), false)?;
                self.f.get(o);
                params.push(Val::I32);
                boxed.push(o);
                continue;
            }
            params.extend(self.vals(self.ty_of(*r))?);
            self.push(*r)?;
        }
        if let Some(o) = &open {
            self.f.push(Ins::VTable(o.witness));
            params.push(Val::I32);
        }
        let c = if takes_ctx {
            params.push(Val::I32);
            self.push_dyn_ctx(method, targs)?
        } else {
            None
        };
        if let Some((_, Some(buffer))) = drive {
            self.f.get(buffer);
            params.push(Val::I32);
        }
        let steps = self.dyn_steps(value_ty, trait_, targs)?;
        if let Some(value) = &payload {
            self.f.get(value[1]);
        } else {
            self.comp(recv, 1)?;
        }
        for at in steps {
            self.f.op(Instruction::I32Load(mem(HEAP, u64::from(at), 2)));
        }
        self.f.op(Instruction::I32Load(mem(
            HEAP,
            4 * u64::try_from(slot).expect("slot"),
            2,
        )));
        self.f.call_indirect(params, results.clone());
        if let (Some(t), Some(o)) = (result, &open)
            && o.ret
            && kept
        {
            self.keep_open(o.ret_ty, t)?;
        } else if let (Some(t), Some(o)) = (result, &open)
            && o.ret
        {
            let x = self.f.local(Val::I32);
            self.f.set(x);
            for l in self.unerase(x, o.ret_ty, t, true)? {
                self.f.get(l);
            }
        }
        if let Some(value) = payload
            && owned
            && self.input.runtime.is_some()
        {
            self.f.get(value[0]);
            self.f.helper(Helper::ReleaseRef);
        }
        self.write_back(backs)?;
        for o in boxed {
            self.release_open(o);
        }
        self.release_ctx(c);
        Ok(())
    }

    /// An omitted argument: its default body's instance, called with the
    /// earlier arguments, or a constant default inlined.
    fn default_call(&mut self, i: u32, bw: u32) -> StageResult<()> {
        match self.calls.get(&i).cloned() {
            Some(Target::ConstDefault(bits)) => {
                match self.vals(self.ty(i))?.as_slice() {
                    [Val::I32] => self.f.i32(i32::from_ne_bytes(
                        u32::try_from(bits & 0xffff_ffff)
                            .expect("low bits")
                            .to_ne_bytes(),
                    )),
                    [Val::I64] => self.f.op(Instruction::I64Const(i64::from_ne_bytes(
                        bits.to_ne_bytes(),
                    ))),
                    [Val::F64] => self
                        .f
                        .op(Instruction::F64Const(wasm_encoder::Ieee64::new(bits))),
                    _ => return unsupported("a constant default of this layout"),
                }
                self.store(i)
            }
            Some(Target::Default { call, bracket }) => {
                let args = self.rec(bw);
                let params = self.param_tys(call.key, call.item, call.args);
                let lends = self.env().lends(call.item);
                self.push_args(&args, params.as_deref(), lends)?;
                let c = self.push_call_providers(call.item, call.args)?;
                if bracket {
                    self.forbid_by(1);
                }
                self.f.call(Sym::Inst(call.key));
                if bracket {
                    self.forbid_by(-1);
                }
                self.call_results(i, call.ret)?;
                self.release_ctx(c);
                Ok(())
            }
            _ => unsupported("a default argument without a target"),
        }
    }

    fn forbid_by(&mut self, by: i32) {
        self.f.global_get(Global::Forbid);
        self.f.i32(by);
        self.f.op(Instruction::I32Add);
        self.f.global_set(Global::Forbid);
    }

    fn intrinsic(&mut self, i: u32, key: &str, args: &[u32]) -> StageResult<()> {
        match key {
            // `panic(message)`: the `explicit-panic` report.
            "panic_message" => {
                self.push(args[0])?;
                self.f.helper(Helper::PanicStr);
                Ok(())
            }
            // std's `panic_with(category, message)`.
            "panic" => {
                self.push(args[0])?;
                self.push(args[1])?;
                self.f.helper(Helper::PanicWith);
                Ok(())
            }
            // Explicit wrapping arithmetic uses the native operation even
            // in checked builds, with the usual sub-word canonical form.
            "int_wrapping_add" | "int_wrapping_sub" | "int_wrapping_mul" => {
                let (bits, signed, float) = self.num(self.ty_of(args[0]));
                if float {
                    return unsupported("wrapping arithmetic of a float");
                }
                let op = match key {
                    "int_wrapping_add" => PrimOp::Add,
                    "int_wrapping_sub" => PrimOp::Sub,
                    _ => PrimOp::Mul,
                };
                self.wrapping(op, args, bits, signed)?;
                self.store(i)
            }
            // Bounds need no operand or bit loop: the result's type fixes
            // their width and signedness, including 32-bit `usize`.
            "int_min" | "int_max" => {
                let (bits, signed, float) = self.num(self.ty_of(i));
                if float {
                    return unsupported("integer bounds of a float");
                }
                let minimum = key == "int_min";
                if bits == 64 {
                    let value = match (minimum, signed) {
                        (true, true) => i64::MIN,
                        (true, false) => 0,
                        (false, true) => i64::MAX,
                        (false, false) => -1,
                    };
                    self.f.op(I::I64Const(value));
                } else {
                    let value = match (minimum, signed, bits) {
                        (true, false, _) => 0,
                        (true, true, 32) => i32::MIN,
                        (true, true, _) => -(1i32 << (bits - 1)),
                        (false, true, 32) => i32::MAX,
                        (false, false, 32) => -1,
                        (false, true, _) => width_mask(bits - 1),
                        (false, false, _) => width_mask(bits),
                    };
                    self.f.i32(value);
                }
                self.store(i)
            }
            // Count only the declared width, not a narrow signed value's
            // sign extension; every public count result is `u32`.
            "int_count_ones" | "int_leading_zeros" | "int_trailing_zeros" => {
                let (bits, signed, float) = self.num(self.ty_of(args[0]));
                if float {
                    return unsupported("integer bit count of a float");
                }
                self.push(args[0])?;
                let ones = key == "int_count_ones";
                let trailing = key == "int_trailing_zeros";
                if bits == 64 {
                    self.f.op(if ones {
                        I::I64Popcnt
                    } else if trailing {
                        I::I64Ctz
                    } else {
                        I::I64Clz
                    });
                    self.f.op(I::I32WrapI64);
                } else {
                    if bits < 32 && trailing {
                        // The sentinel makes zero count to its declared
                        // width; high sign bits cannot affect trailing zeros.
                        self.f.i32(1i32 << bits);
                        self.f.op(I::I32Or);
                    } else if bits < 32 && signed {
                        self.f.i32(width_mask(bits));
                        self.f.op(I::I32And);
                    }
                    self.f.op(if ones {
                        I::I32Popcnt
                    } else if trailing {
                        I::I32Ctz
                    } else {
                        I::I32Clz
                    });
                    if !ones && !trailing && bits < 32 {
                        self.f.i32(i32::from(32 - bits));
                        self.f.op(I::I32Sub);
                    }
                }
                self.store(i)
            }
            // std's `format_int[N < Integer](value: N)`.
            "format_int" => {
                let t = self.ty_of(args[0]);
                let (bits, signed, float) = self.num(t);
                if float {
                    return unsupported("`format_int` of a float");
                }
                self.push(args[0])?;
                if bits < 64 {
                    self.f.op(if signed {
                        Instruction::I64ExtendI32S
                    } else {
                        Instruction::I64ExtendI32U
                    });
                }
                self.f.helper(Helper::FmtInt { signed });
                self.store_owned(i)
            }
            "block_on" => self.block_on(i, args[0]),
            // `cancel!`'s cleanup frame over the task, a `Suspend[bool]`
            // that starts or continues the task's one cleanup and waits it
            // out (`crates/hd_wasm_linear/DESIGN.md` §14.6).
            "task_cleanup_frame" => {
                self.comp(args[0], 0)?;
                self.f.helper(Helper::CleanupCold);
                self.store_owned(i)
            }
            // A suspension that never completes, for std's `timeout!`.
            "task_pending_frame" => {
                let results = self.vals(self.susp_inner(self.ty_of(i))?)?;
                self.f.helper(Helper::PendingCold { results });
                self.store_owned(i)
            }
            // `race!`'s frame over its list of tasks (§14.5).
            "task_race_frame" => {
                let inner = self.susp_inner(self.ty_of(i))?;
                let results = self.vals(inner)?;
                let walk = self.counted(inner);
                self.comp(args[0], 0)?;
                self.f.helper(Helper::Race {
                    part: 0,
                    results,
                    walk,
                });
                self.store_owned(i)
            }
            "task_all_frame" => self.all_frame(i, args),
            // `Suspend[T].abandon`: the task's own cancel with the
            // `abandon()` depth raised (§14.6, "Abandoning"). An abandon
            // that did something may have ended a cleanup that parties
            // wait on with no wake: the driver polls again.
            "task_abandon" => {
                self.f.helper(Helper::CancellationRoot);
                let (s, before) = (self.f.local(Val::I32), self.f.local(Val::I32));
                self.comp(args[0], 0)?;
                self.f.set(s);
                self.f.get(s);
                self.f
                    .op(Instruction::I32Load(mem(HEAP, crate::task::F_FLAGS, 2)));
                self.f.set(before);
                crate::task::abandon_by(&mut self.f, 1);
                self.f.get(s);
                self.f.get(s);
                self.f
                    .op(Instruction::I32Load(mem(HEAP, crate::task::F_CANCEL, 2)));
                self.f.call_indirect(vec![Val::I32], vec![]);
                crate::task::abandon_by(&mut self.f, -1);
                self.f.get(s);
                self.f
                    .op(Instruction::I32Load(mem(HEAP, crate::task::F_FLAGS, 2)));
                self.f.get(before);
                self.f.op(Instruction::I32Ne);
                self.f.op(Instruction::If(BlockType::Empty));
                self.f.i32(1);
                self.f.global_set(Global::Rewake);
                self.f.op(Instruction::End);
                Ok(())
            }
            "map_hash_seed" => self.hash_seed(i),
            // A line to the host's standard error (an entry's error
            // report, `dbg`) or standard output (the REPL's value line),
            // through the runtime, which needs no `Console`.
            "entry_write" | "dbg_write" | "repl_write" => {
                let stream = if matches!(key, "entry_write" | "dbg_write") {
                    "stderr"
                } else {
                    "stdout"
                };
                let out = Sym::Import(crate::module::Import::new(
                    "hd:rt",
                    stream,
                    &[Val::I32],
                    &[],
                ));
                self.push(args[0])?;
                self.f.helper(Helper::ToBuf);
                self.f.call(out.clone());
                let nl = self.m.static_bytes(b"\n", 1);
                self.f.static_addr(nl);
                self.f.i32(1);
                self.f.helper(Helper::ToBuf);
                self.f.call(out);
                Ok(())
            }
            // std's list changes through a reference (`emit/list.rs`).
            "list_truncate" | "list_extend" | "list_reserve" | "bytes_append" => {
                self.list_ref_change(key, args)
            }
            // std's map cursor step closure (`emit/iter.rs`).
            "map_steps" => self.map_iter(i, args[0]),
            "bytes_len" => {
                self.comp(args[0], 1)?;
                self.store(i)
            }
            "bytes_at" => self.str_at(i, args[0], args[1]),
            "bytes_slice" => self.str_slice(i, args[0], args[1], args[2]),
            "bytes_concat" => self.concat(i, args),
            // A SIMD128 scan in place of the hd reference body (#428).
            "bytes_index_json" => {
                self.comp(args[0], 0)?;
                self.comp(args[0], 1)?;
                self.comp(args[1], 0)?;
                self.f.helper(Helper::BytesIndexJson);
                self.store(i)
            }
            "bytes_index_pair" => {
                self.comp(args[0], 0)?;
                self.comp(args[0], 1)?;
                for arg in &args[1..5] {
                    self.comp(*arg, 0)?;
                }
                self.f.helper(Helper::BytesIndexPair);
                self.store(i)
            }
            "bytes_index" => {
                self.comp(args[0], 0)?;
                self.comp(args[0], 1)?;
                self.comp(args[1], 0)?;
                self.comp(args[2], 0)?;
                self.f.helper(Helper::BytesIndex);
                self.store(i)
            }
            "bytes_mismatch" => {
                self.comp(args[0], 0)?;
                self.comp(args[0], 1)?;
                self.comp(args[1], 0)?;
                self.comp(args[1], 1)?;
                self.f.helper(Helper::BytesMismatch);
                self.store(i)
            }
            "utf8_valid_len" => {
                self.comp(args[0], 0)?;
                self.f.helper(Helper::Utf8ValidLen);
                self.store(i)
            }
            // A `char` is its scalar value at run time.
            "char_scalar" => {
                self.comp(args[0], 0)?;
                self.store(i)
            }
            "char_from_scalar" => self.char_from_scalar(i, args[0]),
            // The string kernels over lists (§23.17).
            "concat_all" => {
                self.comp(args[0], 0)?;
                self.f.helper(Helper::ConcatAll);
                self.store_owned(i)
            }
            // The list's count goes to the helper, which takes over an
            // unshared buffer: the moved-in argument's, else one of its own.
            "string_from_bytes" => {
                let moved = self.take(args[0]);
                self.comp(args[0], 0)?;
                if !moved {
                    self.comp(args[0], 0)?;
                    self.f.helper(Helper::RetainKnown);
                }
                self.f.helper(Helper::StrFromBytes);
                self.store_owned(i)
            }
            // The bytes of a string as a new list: one sized buffer, one copy.
            "string_to_bytes" => {
                let (_, elem) = self.elem(self.ty(i))?;
                self.comp(args[0], 0)?;
                self.comp(args[0], 1)?;
                self.f.helper(Helper::StrToBytes { elem });
                self.store_owned(i)
            }
            // The bit casts keep every bit, a NaN's sign and payload too.
            "f64_to_bits" | "f64_from_bits" | "f32_to_bits" | "f32_from_bits" => {
                self.comp(args[0], 0)?;
                self.f.op(match key {
                    "f64_to_bits" => I::I64ReinterpretF64,
                    "f64_from_bits" => I::F64ReinterpretI64,
                    "f32_to_bits" => I::I32ReinterpretF32,
                    _ => I::F32ReinterpretI32,
                });
                self.store(i)
            }
            "format_f64" | "format_f32" | "format_f64_fixed" | "string_lower" | "string_upper"
            | "parse_f64" | "snapshot_update" => self.host_prim(i, key, args),
            key if key.starts_with("mem_") => self.memory(i, key, args),
            other => unsupported(format!("the intrinsic `{other}`")),
        }
    }

    /// `std.rt.memory`'s intrinsics on the heap, memory 0
    /// (`module.rt.memory.*`): each is one instruction. A load or a store
    /// is little-endian and takes any address; `grow` reads Wasm's `-1` as
    /// `.None`; `copy` is `memory.copy`, which allows overlap. A new
    /// buffer is a list's buffer, whose glue drops the values it holds
    /// (`module.rt.buffer.*`); a string's bytes are its address, and a new
    /// string of no bytes the static empty one (`module.rt.string.*`).
    fn memory(&mut self, i: u32, key: &str, args: &[u32]) -> StageResult<()> {
        let op = match key {
            "mem_buffer" => {
                let (_, elem) = self.elem(self.ty(i))?;
                self.push(args[0])?;
                self.f.helper(Helper::ListBuf { elem });
                return self.store_owned(i);
            }
            "mem_string_bytes" => {
                self.comp(args[0], 0)?;
                return self.store(i);
            }
            "mem_new_string" => {
                let s = self.f.local(Val::I32);
                self.f.u32(crate::module::EMPTY);
                self.f.set(s);
                self.push(args[0])?;
                self.f.op(I::If(BlockType::Empty));
                self.push(args[0])?;
                self.f.helper(Helper::Str);
                self.f.set(s);
                self.f.op(I::End);
                self.f.get(s);
                self.push(args[0])?;
                return self.store_owned(i);
            }
            "mem_pages" => I::MemorySize(HEAP),
            "mem_grow" => {
                self.push(args[0])?;
                self.f.op(I::MemoryGrow(HEAP));
                let before = self.f.local(Val::I32);
                self.f.set(before);
                self.f.get(before);
                self.f.i32(-1);
                self.f.op(I::I32Ne);
                self.f.get(before);
                return self.store(i);
            }
            "mem_load_u8" => I::I32Load8U(mem(HEAP, 0, 0)),
            "mem_load_u16" => I::I32Load16U(mem(HEAP, 0, 1)),
            "mem_load_u32" => I::I32Load(mem(HEAP, 0, 2)),
            "mem_load_u64" => I::I64Load(mem(HEAP, 0, 3)),
            "mem_store_u8" => I::I32Store8(mem(HEAP, 0, 0)),
            "mem_store_u16" => I::I32Store16(mem(HEAP, 0, 1)),
            "mem_store_u32" => I::I32Store(mem(HEAP, 0, 2)),
            "mem_store_u64" => I::I64Store(mem(HEAP, 0, 3)),
            "mem_copy" => I::MemoryCopy {
                src_mem: HEAP,
                dst_mem: HEAP,
            },
            "mem_fill" => I::MemoryFill(HEAP),
            other => return unsupported(format!("the intrinsic `{other}`")),
        };
        for a in args {
            self.push(*a)?;
        }
        self.f.op(op);
        self.store(i)
    }

    /// `std.rt.memory`'s typed intrinsics on the heap (`module.rt.ptr.*`).
    /// A `Ptr[T]` is its address, and a `T` there has the layout of a
    /// list's element of `T`, so `size_of` is that size and `offset` one
    /// multiply and add.
    fn ptr(&mut self, i: u32, key: &str, targs: TyList, args: &[u32]) -> StageResult<()> {
        let Some(&t) = self.pool().list_items(targs).last() else {
            return unsupported("a typed memory intrinsic without its type argument");
        };
        let m = crate::layout::mem(self.pool(), self.env(), self.sub(t))?;
        if key == "ptr_offset" {
            self.push(args[0])?;
            self.push(args[1])?;
            self.f.u32(m.size);
            self.f.op(I::I32Mul);
            self.f.op(I::I32Add);
        } else {
            self.f.u32(m.size);
        }
        self.store(i)
    }

    /// A `Read` or a `Store` of the place `[ptr %p]`, the `T` at the
    /// address of pointer `p`, laid out as above. A read loads its leaves:
    /// a `move` takes the heap's count, a `copy` one more, a `borrow`
    /// none. A store writes its value as an element store does and drops
    /// nothing there (`module.rt.ptr.write.over`).
    fn ptr_place(&mut self, i: u32, p: u32) -> StageResult<()> {
        let w = self.b.data[i as usize][1];
        let store = self.b.tags[i as usize] == Tag::Store;
        let t = if store { self.ty_of(w) } else { self.ty(i) };
        let m = crate::layout::mem(self.pool(), self.env(), t)?;
        let at = self.f.local(Val::I32);
        self.comp(p, 0)?;
        self.f.set(at);
        if !store {
            for l in &m.leaves {
                self.f.get(at);
                self.f.op(l.load(0));
            }
            return self.store_as(i, w == hd_tir::ir::ReadMode::Move as u32);
        }
        let s = self.retain_src(&value::Src::Ref(w), t)?;
        self.write_new(value::Dst::Mem(at, 0, &m.leaves), &s)
    }

    /// The result type `T` of a `Suspend[T]`.
    pub(super) fn susp_inner(&self, t: Ty) -> StageResult<Ty> {
        let TyData::Adt { args, .. } = self.pool().get(self.strip_mut(t)) else {
            return unsupported("a suspension of a type that is not `Suspend`");
        };
        match self.pool().list_items(args).first() {
            Some(&inner) => Ok(inner),
            None => unsupported("a suspension without its result type"),
        }
    }

    /// `block_on(s)` (`req.drive.block-on.*`): polls the suspension's frame
    /// through its poll slot, and while it is pending waits in the host
    /// (`hd:rt.block`), then gives `.Completed(value)`, or `.Cancelled` or
    /// `.Abandoned` once a cancelled suspension's cleanup has ended.
    fn block_on(&mut self, i: u32, s: u32) -> StageResult<()> {
        let inner = self.susp_inner(self.ty_of(s))?;
        let result = self.vals(inner)?;
        // Through a drive slot (§23.39), the cold call made the
        // forbidden-context check and the first poll, and left its results
        // in locals: `frame` is 0 when it is done.
        let fused = self.driven.remove(&i);
        if fused.is_none() {
            self.f.helper(Helper::ForbidCheck);
        }
        let (frame, ready, flags, seen) = (
            self.f.local(Val::I32),
            self.f.local(Val::I32),
            self.f.local(Val::I32),
            self.f.local(Val::I32),
        );
        let tmp: Vec<u32> = match &fused {
            Some((ls, _)) => ls.clone(),
            None => result.iter().map(|v| self.f.local(*v)).collect(),
        };
        self.comp(s, 0)?;
        self.f.set(frame);
        self.f.i32(0);
        self.f.set(seen);
        // Poll; when Pending, wait in the host and take its wakes. A
        // cancelled suspension never completes: each poll continues its
        // cleanup, and the drive ends once that has ended
        // (`req.drive.block-on.cancelled`). An abandon in the drive may
        // have ended a cleanup that a party waits on with no wake, and the
        // completions a host wait hands over may be an outer driver's,
        // which no later wait repeats: poll again, and hand the request on
        // to the outer driver. After a drive the loop polls last.
        let poll = |em: &mut Self| {
            em.f.get(frame);
            em.f.get(frame);
            em.f.op(Instruction::I32Load(mem(HEAP, crate::task::F_POLL, 2)));
            em.f.call_indirect(vec![Val::I32], [vec![Val::I32], result.clone()].concat());
            for l in tmp.iter().rev() {
                em.f.set(*l);
            }
            em.f.set(ready);
        };
        if fused.is_some() {
            self.f.get(frame);
            self.f.op(Instruction::I32Eqz);
            self.f.set(ready);
        }
        self.f.op(Instruction::Block(BlockType::Empty));
        self.f.op(Instruction::Loop(BlockType::Empty));
        if fused.is_none() {
            poll(self);
        }
        self.f.get(ready);
        self.f.op(Instruction::BrIf(1));
        self.f.helper(Helper::CanCancel);
        self.f.op(Instruction::If(BlockType::Empty));
        self.f.get(frame);
        self.f
            .op(Instruction::I32Load(mem(HEAP, crate::task::F_FLAGS, 2)));
        self.f.set(flags);
        self.f.get(flags);
        self.f.i32(crate::task::CANCELLED | crate::task::UNWINDING);
        self.f.op(Instruction::I32And);
        self.f.i32(crate::task::CANCELLED);
        self.f.op(Instruction::I32Eq);
        self.f.op(Instruction::BrIf(2));
        self.f.op(Instruction::End);
        self.f.global_get(Global::Rewake);
        self.f.op(Instruction::If(BlockType::Empty));
        self.f.i32(0);
        self.f.global_set(Global::Rewake);
        if fused.is_some() {
            self.f.op(Instruction::Else);
        } else {
            self.f.i32(1);
            self.f.set(seen);
            self.f.op(Instruction::Br(1));
            self.f.op(Instruction::End);
        }
        self.f.call(Sym::Import(crate::module::Import::new(
            "hd:rt",
            "block",
            &[],
            &[Val::I32],
        )));
        self.f.helper(Helper::WakeMark);
        if fused.is_some() {
            self.f.op(Instruction::End);
        }
        self.f.i32(1);
        self.f.set(seen);
        if fused.is_some() {
            poll(self);
        }
        self.f.op(Instruction::Br(0));
        self.f.op(Instruction::End);
        self.f.op(Instruction::End);
        self.f.get(seen);
        self.f.global_get(Global::Rewake);
        self.f.op(Instruction::I32Or);
        self.f.global_set(Global::Rewake);
        // `.Completed(value)`, which counts its own copy of the poll's
        // result; or how the cleanup ended: `.Abandoned` when the frame
        // carries `ABANDONED`, else `.Cancelled`.
        self.slot(i)?;
        self.f.helper(Helper::CanCancel);
        self.f.op(Instruction::If(BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        self.f.get(ready);
        self.f.op(Instruction::If(BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        self.new_variant(i, 0, &[value::Src::Locals(tmp.clone())])?;
        self.release_locals(&tmp, inner)?;
        self.f.op(Instruction::Else);
        self.f.get(flags);
        self.f.i32(crate::task::ABANDONED);
        self.f.op(Instruction::I32And);
        self.f.op(Instruction::If(BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        self.new_variant(i, 2, &[])?;
        self.f.op(Instruction::Else);
        self.new_variant(i, 1, &[])?;
        self.close();
        self.close();
        self.f.op(Instruction::Else);
        self.new_variant(i, 0, &[value::Src::Locals(tmp.clone())])?;
        self.release_locals(&tmp, inner)?;
        self.close();
        if let Some((_, buffer)) = fused {
            self.f.get(frame);
            self.f.get(buffer);
            self.f.op(Instruction::I32Eq);
            self.f.op(Instruction::If(BlockType::Empty));
            let drop = self.f.local(Val::I32);
            self.f.get(frame);
            self.f.op(Instruction::I32Load(mem(
                HEAP,
                crate::task::BORROWED_DROP,
                2,
            )));
            self.f.op(Instruction::LocalTee(drop));
            self.f.i32(0);
            self.f.op(Instruction::I32GeS);
            self.f.op(Instruction::If(BlockType::Empty));
            // Static shadow storage has no automatic descriptor drop. Clear the
            // callback before invoking it, so every terminal outcome drops once.
            self.f.get(frame);
            self.f.i32(-1);
            self.f.op(Instruction::I32Store(mem(
                HEAP,
                crate::task::BORROWED_DROP,
                2,
            )));
            self.f.get(frame);
            self.f.get(drop);
            self.f.call_indirect(vec![Val::I32], vec![]);
            self.f.op(Instruction::End);
            self.f.op(Instruction::End);
        }
        Ok(())
    }

    /// A body-less method of a primitive's std impl at `self_ty`
    /// (`expr.op.std.intrinsic-method`, `expr.ord.std.intrinsic`): the
    /// operator's lowering, an `Ordering`, or the value's text.
    fn builtin(&mut self, i: u32, name: &str, self_ty: Ty, args: &[u32]) -> StageResult<()> {
        let t = hd_mono::layout::strip_newtype(self.pool(), self.env(), self.strip_mut(self_ty));
        let prim = matches!(self.pool().get(t), TyData::Prim(_));
        let op = match name {
            "add" => Some(PrimOp::Add),
            "sub" => Some(PrimOp::Sub),
            "mul" => Some(PrimOp::Mul),
            "div" => Some(PrimOp::Div),
            "rem" => Some(PrimOp::Rem),
            "neg" => Some(PrimOp::Neg),
            "bit_and" => Some(PrimOp::BitAnd),
            "bit_or" => Some(PrimOp::BitOr),
            "bit_xor" => Some(PrimOp::BitXor),
            "not" => Some(PrimOp::Not),
            "shl" => Some(PrimOp::Shl),
            "shr" => Some(PrimOp::Shr),
            "eq" if t != Ty::STRING => Some(PrimOp::Eq),
            _ => None,
        };
        if prim && let Some(op) = op {
            self.prim_value(op, args, self.ty(i))?;
            return self.store(i);
        }
        if name == "runtime_type" {
            return self.runtime_type(i, self.strip_mut(self_ty), args[0]);
        }
        let (_, signed, float) = self.num(t);
        match name {
            "cmp" | "partial_cmp" if prim && t != Ty::STRING => {
                self.ordering(i, args, name == "partial_cmp", float)
            }
            "to_string" if t == Ty::BOOL => {
                let yes = self.m.static_bytes(b"true", 1);
                let no = self.m.static_bytes(b"false", 1);
                self.f.mark_static(yes);
                self.f.mark_static(no);
                self.f.static_addr(yes);
                self.f.static_addr(no);
                self.comp(args[0], 0)?;
                self.f.op(I::Select);
                self.f.u32(4);
                self.f.u32(5);
                self.comp(args[0], 0)?;
                self.f.op(I::Select);
                self.store(i)
            }
            "to_string" if t == Ty::STRING => {
                self.push(args[0])?;
                self.store(i)
            }
            "to_string" if prim && !float => {
                let (bits, _, _) = self.num(t);
                self.push(args[0])?;
                if bits < 64 {
                    self.f.op(if signed {
                        I::I64ExtendI32S
                    } else {
                        I::I64ExtendI32U
                    });
                }
                self.f.helper(Helper::FmtInt { signed });
                self.store_owned(i)
            }
            // A float's text is the host primitive its `Display`
            // implementation calls.
            "to_string" if prim && float => {
                let key = if self.vals(t)? == [Val::F32] {
                    "format_f32"
                } else {
                    "format_f64"
                };
                self.host_prim(i, key, args)
            }
            "cmp" | "partial_cmp" if t == Ty::STRING => {
                self.str_ordering(i, args, name == "partial_cmp")
            }
            "eq" if t == Ty::STRING => {
                self.push(args[0])?;
                self.push(args[1])?;
                self.f.helper(Helper::StrEq);
                self.store(i)
            }
            "sqrt" if prim && float => {
                self.comp(args[0], 0)?;
                self.f.op(if self.vals(t)? == [Val::F32] {
                    I::F32Sqrt
                } else {
                    I::F64Sqrt
                });
                self.store(i)
            }
            _ => unsupported(format!("the built-in method `{name}`")),
        }
    }

    /// `cmp` or `partial_cmp` of two numbers or characters: `>` minus
    /// `<`, plus one, as an `Ordering` (`Less`, `Equal`, `Greater`), which
    /// `partial_cmp` wraps in `.Some`, or `.None` when a float operand is
    /// NaN (`expr.ord.unordered`).
    fn ordering(&mut self, i: u32, args: &[u32], partial: bool, float: bool) -> StageResult<()> {
        let ops = [args[0], args[1]];
        let tag = self.f.local(Val::I32);
        self.f.i32(1);
        self.prim_value(PrimOp::Gt, &ops, Ty::BOOL)?;
        self.f.op(I::I32Add);
        self.prim_value(PrimOp::Lt, &ops, Ty::BOOL)?;
        self.f.op(I::I32Sub);
        self.f.set(tag);
        let want = if partial { 2 } else { 1 };
        if self.vals(self.ty(i))?.len() != want {
            return unsupported("an `Ordering` of this layout");
        }
        if partial {
            if float {
                let some = self.f.local(Val::I32);
                self.prim_value(PrimOp::Eq, &[ops[0], ops[0]], Ty::BOOL)?;
                self.prim_value(PrimOp::Eq, &[ops[1], ops[1]], Ty::BOOL)?;
                self.f.op(I::I32And);
                self.f.set(some);
                self.f.get(some);
                self.f.get(tag);
                self.f.get(some);
                self.f.op(I::I32Mul);
            } else {
                self.f.i32(1);
                self.f.get(tag);
            }
        } else {
            self.f.get(tag);
        }
        self.store(i)
    }

    fn strip_mut(&self, t: Ty) -> Ty {
        match self.pool().get(t) {
            TyData::Mut(inner) => inner,
            _ => t,
        }
    }

    fn panic(&mut self, msg: &str) {
        self.f.helper(Helper::Panic(msg.to_owned()));
    }

    fn prim(&mut self, i: u32, op: u32, rec: u32) -> StageResult<()> {
        let Some(op) = PrimOp::from_u32(op) else {
            return unsupported("an unknown operator");
        };
        let ops = self.rec(rec);
        self.prim_value(op, &ops, self.ty(i))?;
        self.store(i)
    }

    /// Pushes the value of operator `op` on `ops`, whose result has type
    /// `ty`: the `Prim` instruction and the intrinsic methods of the
    /// primitives' operator and comparison impls
    /// (`expr.op.std.intrinsic-method`) share it.
    fn prim_value(&mut self, op: PrimOp, ops: &[u32], ty: Ty) -> StageResult<()> {
        let t0 = ops.first().map_or(Ty::I32, |r| self.ty_of(*r));
        let (bits, signed, float) = self.num(t0);
        let v0 = self.vals(t0)?;
        let [v] = v0[..] else {
            return unsupported(format!("the operator {op:?} on this layout"));
        };
        let wide = v == Val::I64;
        if float && op != PrimOp::Conv {
            return self.float_value(op, ops, v);
        }
        match op {
            PrimOp::Add | PrimOp::Sub | PrimOp::Mul | PrimOp::Neg if self.wraps() => {
                self.wrapping(op, ops, bits, signed)?;
            }
            PrimOp::Add | PrimOp::Sub | PrimOp::Mul | PrimOp::Div | PrimOp::Rem | PrimOp::Neg => {
                self.checked(op, ops, bits, signed)?;
            }
            PrimOp::Pow => self.int_pow(ops, bits, signed)?,
            PrimOp::Eq | PrimOp::Ne | PrimOp::Lt | PrimOp::Le | PrimOp::Gt | PrimOp::Ge => {
                for r in ops {
                    self.comp(*r, 0)?;
                }
                self.f.op(match (op, wide, signed) {
                    (PrimOp::Eq, false, _) => I::I32Eq,
                    (PrimOp::Ne, false, _) => I::I32Ne,
                    (PrimOp::Lt, false, true) => I::I32LtS,
                    (PrimOp::Lt, false, false) => I::I32LtU,
                    (PrimOp::Le, false, true) => I::I32LeS,
                    (PrimOp::Le, false, false) => I::I32LeU,
                    (PrimOp::Gt, false, true) => I::I32GtS,
                    (PrimOp::Gt, false, false) => I::I32GtU,
                    (PrimOp::Ge, false, true) => I::I32GeS,
                    (PrimOp::Ge, false, false) => I::I32GeU,
                    (PrimOp::Eq, true, _) => I::I64Eq,
                    (PrimOp::Ne, true, _) => I::I64Ne,
                    (PrimOp::Lt, true, true) => I::I64LtS,
                    (PrimOp::Lt, true, false) => I::I64LtU,
                    (PrimOp::Le, true, true) => I::I64LeS,
                    (PrimOp::Le, true, false) => I::I64LeU,
                    (PrimOp::Gt, true, true) => I::I64GtS,
                    (PrimOp::Gt, true, false) => I::I64GtU,
                    (PrimOp::Ge, true, true) => I::I64GeS,
                    _ => I::I64GeU,
                });
            }
            PrimOp::And | PrimOp::BitAnd | PrimOp::Or | PrimOp::BitOr | PrimOp::BitXor => {
                for r in ops {
                    self.comp(*r, 0)?;
                }
                self.f.op(match (op, wide) {
                    (PrimOp::And | PrimOp::BitAnd, false) => I::I32And,
                    (PrimOp::And | PrimOp::BitAnd, true) => I::I64And,
                    (PrimOp::Or | PrimOp::BitOr, false) => I::I32Or,
                    (PrimOp::Or | PrimOp::BitOr, true) => I::I64Or,
                    (_, false) => I::I32Xor,
                    (_, true) => I::I64Xor,
                });
            }
            // Bits shifted past a sub-word width are dropped, as at 32 and
            // 64 bits, so the result is brought back to canonical form.
            PrimOp::Shl => {
                self.shift(ops, bits, wide)?;
                if wide {
                    self.f.op(Instruction::I64Shl);
                } else {
                    self.f.op(Instruction::I32Shl);
                    self.wrap_to(bits, signed);
                }
            }
            PrimOp::Shr => {
                self.shift(ops, bits, wide)?;
                self.f.op(match (wide, signed) {
                    (false, true) => Instruction::I32ShrS,
                    (false, false) => Instruction::I32ShrU,
                    (true, true) => Instruction::I64ShrS,
                    (true, false) => Instruction::I64ShrU,
                });
            }
            PrimOp::Not => {
                self.comp(ops[0], 0)?;
                let stripped = hd_mono::layout::strip_newtype(self.pool(), self.env(), t0);
                if stripped == Ty::BOOL {
                    self.f.op(Instruction::I32Eqz);
                } else if wide {
                    self.f.op(Instruction::I64Const(-1));
                    self.f.op(Instruction::I64Xor);
                } else {
                    // A sign-extended value stays so under `~`; a
                    // zero-extended one flips only its own bits.
                    self.f.i32(if signed || bits >= 32 {
                        -1
                    } else {
                        width_mask(bits)
                    });
                    self.f.op(Instruction::I32Xor);
                }
            }
            PrimOp::Conv => self.convert(ops[0], v, (signed, float), ty)?,
        }
        Ok(())
    }

    /// A shift's count: any unsigned type (`expr.shift.count-unsigned`),
    /// below the shifted value's width in a checked build
    /// (`types.arith.shift-count.debug`) and masked to it in a release
    /// build (`types.arith.shift-count.release`), brought to the value's
    /// width. Pushes the value, then the count.
    fn shift(&mut self, ops: &[u32], bits: u8, wide: bool) -> StageResult<()> {
        self.comp(ops[0], 0)?;
        let [cv] = self.vals(self.ty_of(ops[1]))?[..] else {
            return unsupported("a shift count without one value");
        };
        let n = self.f.local(cv);
        self.comp(ops[1], 0)?;
        self.f.set(n);
        self.f.get(n);
        let c64 = cv == Val::I64;
        if self.wraps() {
            if bits < 32 {
                if c64 {
                    self.f.op(Instruction::I64Const(i64::from(bits) - 1));
                    self.f.op(Instruction::I64And);
                } else {
                    self.f.i32(i32::from(bits) - 1);
                    self.f.op(Instruction::I32And);
                }
            }
        } else {
            if c64 {
                self.f.op(Instruction::I64Const(i64::from(bits)));
                self.f.op(Instruction::I64GeU);
            } else {
                self.f.i32(i32::from(bits));
                self.f.op(Instruction::I32GeU);
            }
            self.f.op(Instruction::If(BlockType::Empty));
            self.panic("invalid-shift: shift count out of range");
            self.f.op(Instruction::End);
            self.f.get(n);
        }
        match (c64, wide) {
            (false, true) => self.f.op(Instruction::I64ExtendI32U),
            (true, false) => self.f.op(Instruction::I32WrapI64),
            _ => {}
        }
        Ok(())
    }

    /// A numeric cast of `op` from layout `from` to type `ty`
    /// (`types.cast.*`); no cast panics (`types.cast.no-panic`).
    fn convert(
        &mut self,
        op: u32,
        from: Val,
        (signed, float): (bool, bool),
        ty: Ty,
    ) -> StageResult<()> {
        let [to] = self.vals(ty)?[..] else {
            return unsupported("a numeric cast to a type without one value");
        };
        let (tbits, tsigned, tfloat) = self.num(ty);
        self.comp(op, 0)?;
        match (from, to) {
            (Val::I32, Val::I64) => self.f.op(if signed {
                I::I64ExtendI32S
            } else {
                I::I64ExtendI32U
            }),
            (Val::I64, Val::I32) => self.f.op(I::I32WrapI64),
            (Val::I32, Val::F32) => self.f.op(if signed {
                I::F32ConvertI32S
            } else {
                I::F32ConvertI32U
            }),
            (Val::I64, Val::F32) => self.f.op(if signed {
                I::F32ConvertI64S
            } else {
                I::F32ConvertI64U
            }),
            (Val::I32, Val::F64) => self.f.op(if signed {
                I::F64ConvertI32S
            } else {
                I::F64ConvertI32U
            }),
            (Val::I64, Val::F64) => self.f.op(if signed {
                I::F64ConvertI64S
            } else {
                I::F64ConvertI64U
            }),
            (Val::F32, Val::F64) => self.f.op(I::F64PromoteF32),
            (Val::F64, Val::F32) => self.f.op(I::F32DemoteF64),
            (Val::F32 | Val::F64, Val::I32 | Val::I64) => {
                self.float_to_int(from, tbits, tsigned);
            }
            (a, b) if a == b => {}
            _ => return unsupported("this numeric conversion"),
        }
        // An integer cast keeps the target's low bits (`types.cast.wrap`).
        if !tfloat && !float {
            self.wrap_to(tbits, tsigned);
        }
        Ok(())
    }

    /// Truncates the float on the stack toward zero and saturates it to
    /// the integer type of `bits` and `signed` (`types.cast.saturate`).
    fn float_to_int(&mut self, from: Val, bits: u8, signed: bool) {
        let single = from == Val::F32;
        self.f.op(match (bits, single, signed) {
            (64, true, true) => I::I64TruncSatF32S,
            (64, true, false) => I::I64TruncSatF32U,
            (64, false, true) => I::I64TruncSatF64S,
            (64, false, false) => I::I64TruncSatF64U,
            (32, true, true) => I::I32TruncSatF32S,
            (32, true, false) => I::I32TruncSatF32U,
            (32, false, true) => I::I32TruncSatF64S,
            (32, false, false) => I::I32TruncSatF64U,
            (_, true, _) => I::I32TruncSatF32S,
            (_, false, _) => I::I32TruncSatF64S,
        });
        if bits >= 32 {
            return;
        }
        let (lo, hi) = if signed {
            (-(1i32 << (bits - 1)), (1i32 << (bits - 1)) - 1)
        } else {
            (0, width_mask(bits))
        };
        let v = self.f.local(Val::I32);
        self.f.set(v);
        for (bound, cmp) in [(lo, I::I32LtS), (hi, I::I32GtS)] {
            self.f.i32(bound);
            self.f.get(v);
            self.f.get(v);
            self.f.i32(bound);
            self.f.op(cmp);
            self.f.op(I::Select);
            self.f.set(v);
        }
        self.f.get(v);
    }

    /// Brings an `i32` holding a `bits`-wide integer's low bits back to
    /// canonical form: sign-extended when `signed`, zero-extended otherwise.
    fn wrap_to(&mut self, bits: u8, signed: bool) {
        match (bits, signed) {
            (8, true) => self.f.op(Instruction::I32Extend8S),
            (16, true) => self.f.op(Instruction::I32Extend16S),
            (8 | 16, false) => {
                self.f.i32(width_mask(bits));
                self.f.op(Instruction::I32And);
            }
            _ => {}
        }
    }

    /// Float operations and comparisons (`expr.float.basic`): power and
    /// remainder use the same portable helpers as the GC backend.
    fn float_value(&mut self, op: PrimOp, ops: &[u32], v: Val) -> StageResult<()> {
        let single = v == Val::F32;
        if matches!(op, PrimOp::Pow | PrimOp::Rem) {
            for r in ops {
                self.comp(*r, 0)?;
                if single {
                    self.f.op(I::F64PromoteF32);
                }
            }
            self.f.helper(if op == PrimOp::Pow {
                Helper::PowF64
            } else {
                Helper::RemF64
            });
            if single {
                self.f.op(I::F32DemoteF64);
            }
            return Ok(());
        }
        for r in ops {
            self.comp(*r, 0)?;
        }
        self.f.op(match (op, single) {
            (PrimOp::Neg, false) => I::F64Neg,
            (PrimOp::Add, false) => I::F64Add,
            (PrimOp::Sub, false) => I::F64Sub,
            (PrimOp::Mul, false) => I::F64Mul,
            (PrimOp::Div, false) => I::F64Div,
            (PrimOp::Eq, false) => I::F64Eq,
            (PrimOp::Ne, false) => I::F64Ne,
            (PrimOp::Lt, false) => I::F64Lt,
            (PrimOp::Le, false) => I::F64Le,
            (PrimOp::Gt, false) => I::F64Gt,
            (PrimOp::Ge, false) => I::F64Ge,
            (PrimOp::Neg, true) => I::F32Neg,
            (PrimOp::Add, true) => I::F32Add,
            (PrimOp::Sub, true) => I::F32Sub,
            (PrimOp::Mul, true) => I::F32Mul,
            (PrimOp::Div, true) => I::F32Div,
            (PrimOp::Eq, true) => I::F32Eq,
            (PrimOp::Ne, true) => I::F32Ne,
            (PrimOp::Lt, true) => I::F32Lt,
            (PrimOp::Le, true) => I::F32Le,
            (PrimOp::Gt, true) => I::F32Gt,
            (PrimOp::Ge, true) => I::F32Ge,
            _ => return unsupported(format!("the float operator {op:?} (job 3, #540)")),
        });
        Ok(())
    }

    /// Release-build `+`, `-`, `*` and unary `-` (`types.arith.release`):
    /// the native operation, brought back to a sub-word type's canonical
    /// form. Division and remainder stay checked (`types.arith.always`).
    fn wrapping(&mut self, op: PrimOp, ops: &[u32], bits: u8, signed: bool) -> StageResult<()> {
        let wide = bits == 64;
        if op == PrimOp::Neg {
            self.f.op(if wide {
                Instruction::I64Const(0)
            } else {
                Instruction::I32Const(0)
            });
            self.comp(ops[0], 0)?;
        } else {
            self.comp(ops[0], 0)?;
            self.comp(ops[1], 0)?;
        }
        self.f.op(match (op, wide) {
            (PrimOp::Add, true) => I::I64Add,
            (PrimOp::Mul, true) => I::I64Mul,
            (_, true) => I::I64Sub,
            (PrimOp::Add, false) => I::I32Add,
            (PrimOp::Mul, false) => I::I32Mul,
            (_, false) => I::I32Sub,
        });
        self.wrap_to(bits, signed);
        Ok(())
    }

    /// Checked integer arithmetic (`types.arith.checked`): sub-word widths
    /// compute exactly in `i32` and 32-bit widths in `i64`, each checked
    /// against the type's range; 64-bit widths test the native result.
    fn checked(&mut self, op: PrimOp, ops: &[u32], bits: u8, signed: bool) -> StageResult<()> {
        // An unsigned quotient or remainder is never larger than its
        // dividend, so it needs only the zero check, at its own width.
        if !signed && bits <= 32 && matches!(op, PrimOp::Div | PrimOp::Rem) {
            // A nonzero literal divisor needs no check, and a power of two
            // is a shift or a mask.
            if let Some(c) = self.const_u32(ops[1]).filter(|c| *c != 0) {
                self.comp(ops[0], 0)?;
                if c.is_power_of_two() {
                    if op == PrimOp::Div {
                        self.f.u32(c.trailing_zeros());
                        self.f.op(I::I32ShrU);
                    } else {
                        self.f.u32(c - 1);
                        self.f.op(I::I32And);
                    }
                } else {
                    self.f.u32(c);
                    self.f.op(if op == PrimOp::Div {
                        I::I32DivU
                    } else {
                        I::I32RemU
                    });
                }
                return Ok(());
            }
            let y = self.f.local(Val::I32);
            self.comp(ops[0], 0)?;
            self.comp(ops[1], 0)?;
            self.f.set(y);
            self.f.get(y);
            self.f.op(Instruction::I32Eqz);
            self.f.op(Instruction::If(BlockType::Empty));
            self.panic("integer-division-by-zero: division by zero");
            self.f.op(Instruction::End);
            self.f.get(y);
            self.f.op(if op == PrimOp::Div {
                I::I32DivU
            } else {
                I::I32RemU
            });
            return Ok(());
        }
        // A remainder's magnitude is below its divisor's, so it always
        // fits the operand width (`types.arith.min-remainder`): the native
        // operation at that width, with only the zero check, which a
        // nonzero constant divisor makes unnecessary.
        if op == PrimOp::Rem {
            let wide = bits == 64;
            let nonzero = if wide {
                self.const_i64(ops[1]).is_some_and(|c| c != 0)
            } else {
                self.const_u32(ops[1]).is_some_and(|c| c != 0)
            };
            self.comp(ops[0], 0)?;
            if nonzero {
                self.comp(ops[1], 0)?;
            } else {
                let y = self.f.local(if wide { Val::I64 } else { Val::I32 });
                self.comp(ops[1], 0)?;
                self.f.set(y);
                self.f.get(y);
                self.f.op(if wide {
                    Instruction::I64Eqz
                } else {
                    Instruction::I32Eqz
                });
                self.f.op(Instruction::If(BlockType::Empty));
                self.panic("integer-division-by-zero: division by zero");
                self.f.op(Instruction::End);
                self.f.get(y);
            }
            self.f.op(match (wide, signed) {
                (true, true) => I::I64RemS,
                (true, false) => I::I64RemU,
                (false, true) => I::I32RemS,
                (false, false) => I::I32RemU,
            });
            return Ok(());
        }
        let wide = bits == 64;
        let in64 = bits >= 32;
        let exact = if in64 { Val::I64 } else { Val::I32 };
        let (x, y, r) = (
            self.f.local(exact),
            self.f.local(exact),
            self.f.local(exact),
        );
        let (lhs, rhs) = if op == PrimOp::Neg {
            (None, ops[0])
        } else {
            (Some(ops[0]), ops[1])
        };
        for (src, dst) in [(lhs, x), (Some(rhs), y)] {
            match src {
                Some(v) => {
                    self.comp(v, 0)?;
                    if bits == 32 {
                        self.f.op(if signed {
                            Instruction::I64ExtendI32S
                        } else {
                            Instruction::I64ExtendI32U
                        });
                    }
                }
                None => self.f.op(if in64 {
                    Instruction::I64Const(0)
                } else {
                    Instruction::I32Const(0)
                }),
            }
            self.f.set(dst);
        }
        if matches!(op, PrimOp::Div | PrimOp::Rem) {
            self.f.get(y);
            self.f.op(if in64 {
                Instruction::I64Eqz
            } else {
                Instruction::I32Eqz
            });
            self.f.op(Instruction::If(BlockType::Empty));
            self.panic("integer-division-by-zero: division by zero");
            self.f.op(Instruction::End);
        }
        // `MIN / -1` overflows in every build (`types.arith.min-division`);
        // at 64 bits it is tested before `i64.div_s`, which would trap.
        if wide && signed && op == PrimOp::Div {
            self.f.get(x);
            self.f.op(Instruction::I64Const(i64::MIN));
            self.f.op(Instruction::I64Eq);
            self.f.get(y);
            self.f.op(Instruction::I64Const(-1));
            self.f.op(Instruction::I64Eq);
            self.f.op(Instruction::I32And);
            self.f.op(Instruction::If(BlockType::Empty));
            self.panic("integer-overflow: integer overflow");
            self.f.op(Instruction::End);
        }
        self.f.get(x);
        self.f.get(y);
        self.f.op(match (op, signed, in64) {
            (PrimOp::Add, _, true) => I::I64Add,
            (PrimOp::Sub | PrimOp::Neg, _, true) => I::I64Sub,
            (PrimOp::Mul, _, true) => I::I64Mul,
            (PrimOp::Div, true, true) => I::I64DivS,
            (PrimOp::Div, false, true) => I::I64DivU,
            (PrimOp::Rem, true, true) => I::I64RemS,
            (_, _, true) => I::I64RemU,
            (PrimOp::Add, _, false) => I::I32Add,
            (PrimOp::Sub | PrimOp::Neg, _, false) => I::I32Sub,
            (PrimOp::Mul, _, false) => I::I32Mul,
            (PrimOp::Div, true, false) => I::I32DivS,
            (PrimOp::Div, false, false) => I::I32DivU,
            (PrimOp::Rem, true, false) => I::I32RemS,
            (_, _, false) => I::I32RemU,
        });
        self.f.set(r);
        self.int_overflow(op, bits, signed, (x, y, r));
        self.f.op(I::If(BlockType::Empty));
        self.panic("integer-overflow: integer overflow");
        self.f.op(I::End);
        self.f.get(r);
        if bits == 32 {
            self.f.op(I::I32WrapI64);
        }
        Ok(())
    }

    /// Whether a computed integer result exceeds its declared width.
    fn int_overflow(&mut self, op: PrimOp, bits: u8, signed: bool, (x, y, r): (u32, u32, u32)) {
        let wide = bits == 64;
        let in64 = bits >= 32;
        if wide {
            self.wide_overflow(op, signed, (x, y, r));
        } else {
            // The exact result against the type's range; an unsigned result
            // below zero reads as a large unsigned value.
            let (lo, hi): (i64, i64) = if signed {
                (-(1i64 << (bits - 1)), (1i64 << (bits - 1)) - 1)
            } else {
                (0, (1i64 << bits) - 1)
            };
            let bound = |em: &mut Self, v: i64| {
                if in64 {
                    em.f.op(I::I64Const(v));
                } else {
                    em.f.i32(i32::try_from(v).expect("a sub-word bound"));
                }
            };
            self.f.get(r);
            bound(self, hi);
            self.f.op(match (signed, in64) {
                (true, true) => I::I64GtS,
                (false, true) => I::I64GtU,
                (true, false) => I::I32GtS,
                (false, false) => I::I32GtU,
            });
            if signed {
                self.f.get(r);
                bound(self, lo);
                self.f.op(if in64 { I::I64LtS } else { I::I32LtS });
                self.f.op(I::I32Or);
            }
        }
    }

    /// Integer power by repeated squaring, checking only products needed
    /// for the result. Exponents use their full unsigned width.
    fn int_pow(&mut self, ops: &[u32], bits: u8, signed: bool) -> StageResult<()> {
        let exact = if bits >= 32 { Val::I64 } else { Val::I32 };
        let (base, acc, product) = (
            self.f.local(exact),
            self.f.local(exact),
            self.f.local(exact),
        );
        let exponent = self.f.local(Val::I64);
        self.comp(ops[0], 0)?;
        if bits == 32 {
            self.f.op(if signed {
                I::I64ExtendI32S
            } else {
                I::I64ExtendI32U
            });
        }
        self.f.set(base);
        self.comp(ops[1], 0)?;
        if self.vals(self.ty_of(ops[1]))? == [Val::I32] {
            self.f.op(I::I64ExtendI32U);
        }
        self.f.set(exponent);
        self.f.op(if exact == Val::I64 {
            I::I64Const(1)
        } else {
            I::I32Const(1)
        });
        self.f.set(acc);
        self.f.op(I::Block(BlockType::Empty));
        self.f.op(I::Loop(BlockType::Empty));
        self.f.get(exponent);
        self.f.op(I::I64Const(1));
        self.f.op(I::I64And);
        self.f.op(I::I64Eqz);
        self.f.op(I::I32Eqz);
        self.f.op(I::If(BlockType::Empty));
        self.pow_mul(bits, signed, (acc, base, product));
        self.f.get(product);
        self.f.set(acc);
        self.f.op(I::End);
        self.f.get(exponent);
        self.f.op(I::I64Const(1));
        self.f.op(I::I64ShrU);
        self.f.set(exponent);
        self.f.get(exponent);
        self.f.op(I::I64Eqz);
        self.f.op(I::BrIf(1));
        self.pow_mul(bits, signed, (base, base, product));
        self.f.get(product);
        self.f.set(base);
        self.f.op(I::Br(0));
        self.f.op(I::End);
        self.f.op(I::End);
        self.f.get(acc);
        if bits == 32 {
            self.f.op(I::I32WrapI64);
        }
        if self.wraps() {
            self.wrap_to(bits, signed);
        }
        Ok(())
    }

    fn pow_mul(&mut self, bits: u8, signed: bool, (x, y, r): (u32, u32, u32)) {
        self.f.get(x);
        self.f.get(y);
        self.f.op(if bits >= 32 { I::I64Mul } else { I::I32Mul });
        self.f.set(r);
        if !self.wraps() {
            self.int_overflow(PrimOp::Mul, bits, signed, (x, y, r));
            self.f.op(I::If(BlockType::Empty));
            self.panic("integer-overflow: integer overflow");
            self.f.op(I::End);
        }
    }

    /// Whether the 64-bit `r = x op y` overflowed, as an `i32` on the
    /// stack.
    fn wide_overflow(&mut self, op: PrimOp, signed: bool, (x, y, r): (u32, u32, u32)) {
        match (op, signed) {
            (PrimOp::Add, true) => {
                for i in [I::LocalGet(x), I::LocalGet(r), I::I64Xor] {
                    self.f.op(i);
                }
                for i in [I::LocalGet(y), I::LocalGet(r), I::I64Xor, I::I64And] {
                    self.f.op(i);
                }
                self.f.op(I::I64Const(0));
                self.f.op(I::I64LtS);
            }
            (PrimOp::Sub | PrimOp::Neg, true) => {
                for i in [I::LocalGet(x), I::LocalGet(y), I::I64Xor] {
                    self.f.op(i);
                }
                for i in [I::LocalGet(x), I::LocalGet(r), I::I64Xor, I::I64And] {
                    self.f.op(i);
                }
                self.f.op(I::I64Const(0));
                self.f.op(I::I64LtS);
            }
            (PrimOp::Add, false) => {
                for i in [I::LocalGet(r), I::LocalGet(x), I::I64LtU] {
                    self.f.op(i);
                }
            }
            (PrimOp::Sub | PrimOp::Neg, false) => {
                for i in [I::LocalGet(x), I::LocalGet(y), I::I64LtU] {
                    self.f.op(i);
                }
            }
            // The product overflowed when `x` is not zero and `r / x` is
            // not `y`; `x = -1` is tested apart, since `MIN / -1` traps.
            (PrimOp::Mul, _) => {
                let o = self.f.local(Val::I32);
                self.f.get(x);
                self.f.op(I::I64Eqz);
                self.f.op(I::I32Eqz);
                self.f.op(I::If(BlockType::Empty));
                if signed {
                    self.f.get(x);
                    self.f.op(I::I64Const(-1));
                    self.f.op(I::I64Eq);
                    self.f.op(I::If(BlockType::Empty));
                    self.f.get(y);
                    self.f.op(I::I64Const(i64::MIN));
                    self.f.op(I::I64Eq);
                    self.f.set(o);
                    self.f.op(I::Else);
                }
                self.f.get(r);
                self.f.get(x);
                self.f.op(if signed { I::I64DivS } else { I::I64DivU });
                self.f.get(y);
                self.f.op(I::I64Ne);
                self.f.set(o);
                if signed {
                    self.f.op(I::End);
                }
                self.f.op(I::End);
                self.f.get(o);
            }
            // Division and remainder overflow only at `MIN / -1`, tested
            // before the division.
            _ => self.f.i32(0),
        }
    }
}

/// The mask of a sub-word width's bits.
const fn width_mask(bits: u8) -> i32 {
    (1i32 << bits) - 1
}
