//! The inliner's body copy (`docs/compiler/tir-v2.md`, "The inliner"): a
//! `Call` or driven `Await` of a small body becomes a copy, run before the
//! ownership passes so they see the combined body. The copy's locals,
//! labels, constants, strings and records move past the caller's; its
//! used parameters are bound by a `Store` of the call's argument (an
//! unused earlier default argument needs no binding); each `Return` is
//! a `Break` to its root block, which a `Scope` with no suites owns in the
//! call's place. The caller substitutes the call's type arguments first
//! ([`crate::ir::map_body_types`]); choosing what to inline is the
//! driver's.

use hd_base::SliceSort as _;
use hd_base::{DefId, HashMap, LocalId, NodeIdx, SubId};
use hd_types::{Ty, TyList};

use crate::ir::{
    Body, BodyKind, Callee, ChoiceKind, INLINED, NONE, Op, Remap, Role, SideRows, Tag, Through,
    Word, copy_rows, grow, local_flags, place, push_frame, renumber, slots, u32_of,
};
use crate::own::splice::permute_columns;

mod closures;
pub(crate) use closures::extract_subs;
pub use closures::{function_body, inline_local_closures, remove_unused_instantiations};

/// The statically known callee of call `i` when its requirement context
/// needs no copying: a `Call` or driven `Await` with no providers. Its
/// argument record may still contain an omitted argument's `DefaultCall`.
#[must_use]
pub fn known_call(b: &Body, i: usize) -> Option<Callee> {
    if b.call_providers(i) != crate::ir::Providers::None {
        return None;
    }
    if b.tags[i] == Tag::DefaultCall {
        let [def, targs] = b.record(b.data[i][0]) else {
            return None;
        };
        return Some(Callee::Item {
            def: DefId::from_raw(*def),
            targs: TyList(*targs),
        });
    }
    if !matches!(b.tags[i], Tag::Call | Tag::Await) {
        return None;
    }
    Callee::from_words(b.record(b.data[i][0]))
}

/// Arguments of any call, excluding its provider footer and dynamic header.
#[must_use]
pub fn arguments(b: &Body, i: usize) -> &[u32] {
    b.call_args(i)
}

/// The callee of call `i` under the ordinary inlining rule: a
/// [`known_call`] with no omitted argument.
#[must_use]
pub fn plain_call(b: &Body, i: usize) -> Option<Callee> {
    let callee = known_call(b, i)?;
    plain_args(b, i).then_some(callee)
}

/// Whether call `i` passes every argument, none an omitted one's
/// `DefaultCall`.
#[must_use]
pub fn plain_args(b: &Body, i: usize) -> bool {
    let args = arguments(b, i);
    !args
        .iter()
        .any(|&a| b.tags.get(a as usize) == Some(&Tag::DefaultCall))
}

/// What a trait call chosen through a bound runs once its receiver is
/// concrete: an impl item, or a compiler-supplied implementation, whose
/// word is the checker's (`1 +` the solver's `BuiltinImpl`, or 0 for one
/// the solver does not name, `hd_check` `choice_of`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chosen {
    Impl(DefId),
    Builtin(u32),
}

impl Chosen {
    /// The callee record's choice words.
    #[must_use]
    pub fn choice(self) -> (ChoiceKind, u32) {
        match self {
            Chosen::Impl(d) => (ChoiceKind::Impl, d.raw()),
            Chosen::Builtin(v) => (ChoiceKind::Builtin, v),
        }
    }
}

/// Chooses each `Bound` trait call of `k` again as what `pick` names for
/// its trait, receiver type and type arguments, as substitution may have
/// made them concrete: an impl call or a builtin call. A call `pick`
/// answers `None` for (an open receiver, a trait value) stays `Bound`.
/// One pass over the instructions.
pub fn rechoose(k: &mut Body, pick: &dyn Fn(DefId, Ty, TyList) -> Option<Chosen>) {
    for i in 0..k.len() {
        let at = k.data[i][0] as usize;
        if matches!(k.tags[i], Tag::Call | Tag::Await)
            && let Some(Callee::TraitMethod {
                trait_,
                self_ty,
                targs,
                choice: (ChoiceKind::Bound, _),
                ..
            }) = Callee::from_words(k.record(k.data[i][0]))
            && let Some(chosen) = pick(trait_, self_ty, targs)
        {
            // `[len, 1, trait, method, self_ty, targs, kind, value]`.
            let (kind, value) = chosen.choice();
            k.extra[at + 6] = kind as u32;
            k.extra[at + 7] = value;
        }
    }
}

/// Whether `c` may be copied structurally: a function of one sub-body, its
/// root block last, with no `defer`, closure, escaping context or
/// ownership statement, and no call with explicit providers. A generic
/// local function's instantiation, which names its item's sub-body, is
/// not a closure here: only that item's bodies copy one. A copied `With`
/// retains its lexical scope. Direct awaits are allowed while preparing
/// nested copies; the optimizer must eliminate every suspension point before
/// publishing an inline copy. Copied `ProviderGet`s resolve there, exactly
/// as they resolved at the call. Context construction keeps its call boundary.
#[must_use]
pub fn copyable(c: &Body) -> bool {
    let n = c.len();
    let held: usize = (0..n)
        .filter(|&i| c.is_instantiation(i))
        .map(|i| c.record(c.data[i][1])[1] as usize)
        .sum();
    let refused = |i: usize| match c.tags[i] {
        Tag::Call | Tag::Await => c.record(c.data[i][1]).iter().rev().nth(2) != Some(&0),
        Tag::Closure => !c.is_instantiation(i),
        t => matches!(
            t,
            Tag::AwaitValue
                | Tag::AwaitAll
                | Tag::AwaitRace
                | Tag::Hook
                | Tag::Defer
                | Tag::Closure
                | Tag::ContextFor
                | Tag::ContextNew
                | Tag::Hole
                | Tag::Poison
                | Tag::Drop
                | Tag::LoanEnd
                | Tag::Retain
                | Tag::Release
                | Tag::MakeUnique
                | Tag::IsUnique
                | Tag::FlagSet
                | Tag::FlagClear
                | Tag::Box
        ),
    };
    matches!(c.kind, BodyKind::Fn | BodyKind::Default)
        && n >= 1
        && c.sub_root == [u32_of(n - 1)]
        // Only eliminated shared-cell facts and instantiations' captures
        // remain, not a callee environment.
        && c.cap_local.len() == c.inlined_shared.len() + held

        && c.label.is_empty()
        && !(0..n).any(refused)
}

/// Whether `c` is [`copyable`] and fits the caller's size budget.
/// Plain suite wrappers have no instruction of their own. A labelled
/// block does, as does the root that inlining turns into a return scope.
/// Calls also need their argument bindings and a returned-value binding.
/// Keep the existing raw-row bound for bodies it already admits; the
/// lowering estimate can additionally admit structurally larger bodies.
#[must_use]
pub fn inlinable(c: &Body, limit: usize) -> bool {
    if c.len() <= limit {
        return copyable(c);
    }
    let mut size = 0;
    for (i, &tag) in c.tags.iter().enumerate() {
        if tag != Tag::Block || i + 1 == c.len() || c.label_inst.contains(&u32_of(i)) {
            size += 1;
        }
        if matches!(
            tag,
            Tag::Call
                | Tag::CallValue
                | Tag::CallDyn
                | Tag::CallHost
                | Tag::DefaultCall
                | Tag::Await
                | Tag::AwaitValue
                | Tag::AwaitAll
                | Tag::AwaitRace
        ) {
            slots(c, i, |_, _, word| {
                if matches!(
                    word,
                    Word::Value(Role::Arg(_)) | Word::Provider | Word::ProviderRef
                ) {
                    size += 1;
                }
            });
            size += usize::from(!matches!(c.ty[i], Ty::VOID | Ty::NEVER));
        }
        if size > limit {
            return false;
        }
    }
    copyable(c)
}

/// Whether `c` calls nothing, not even a function value, so that its copy
/// adds no call to its caller.
#[must_use]
pub fn leaf(c: &Body) -> bool {
    !c.tags.iter().any(|t| {
        matches!(
            t,
            Tag::Call
                | Tag::CallValue
                | Tag::CallDyn
                | Tag::CallHost
                | Tag::DefaultCall
                | Tag::Await
                | Tag::AwaitValue
                | Tag::AwaitAll
                | Tag::AwaitRace
        )
    })
}

/// A call to copy ([`inline_calls`]): the caller's `Call` `call` and the
/// callee's copy `copy` with the call's types. Each parameter in
/// `by_value` (bit k: parameter k) takes the value its argument, a `Ref`
/// of the caller's (`crate::own::promote::inline_args`), refers to: the
/// copy is promoted (`crate::own::promote::promote`), and the `Ref`
/// becomes a `Read` of its place.
pub struct Site {
    pub call: usize,
    pub copy: Body,
    pub by_value: u64,
    /// Reference parameters substituted with their already evaluated
    /// argument, without an intermediate owning binding. The caller proves
    /// that removing the binding preserves the call-entry loan boundary.
    pub by_ref: u64,
}

/// A site that passed its checks: its parameters a `Store` binds (local,
/// argument), the call's block, and where the copy will start.
struct Ready {
    c: usize,
    k: Body,
    params: Vec<(u32, usize)>,
    refs: Vec<(u32, usize)>,
    outer: usize,
    base: u32,
    /// The copy's free captured locals and their already evaluated caller locals.
    environment: Vec<(u32, u32)>,
    sub: SubId,
}

impl Ready {
    /// The rows of the cleanup suite that keeps the environment's locals
    /// alive across the copy: a read of each, then their block. A copy of
    /// a body that runs once (`sub_flags::ONCE`) takes them instead.
    fn kept(&self) -> usize {
        let once = self.k.sub_flags.first().copied().unwrap_or(0) & crate::ir::sub_flags::ONCE;
        if self.environment.is_empty() || once != 0 {
            0
        } else {
            self.environment.len() + 1
        }
    }
}

/// Replaces each site's `Call` of `b` by its copy, all in one rebuild:
/// the caller is renumbered once and its columns permuted once, however
/// many calls are copied. `sites` come in descending call order, which is
/// the order the copies' locals, constants, labels and records are
/// appended in; the result says which sites were copied. The copy stands
/// where its call stood, keeping evaluation order, the `Scope` that holds
/// it last.
///
/// A site is skipped, the rest still copied, when its callee's parameters
/// do not match the call's arguments, or the call is in no block. A caller
/// the renumbering
/// refuses (a call's providers of an unknown kind) copies none and is
/// unchanged.
///
/// # Errors
/// If the renumbering refuses what that check let through, or a
/// by-value argument is no `Ref`.
pub fn inline_calls(b: &mut Body, sites: Vec<Site>) -> Result<Vec<bool>, String> {
    inline_bound_calls(
        b,
        None,
        sites.into_iter().map(|site| (site, Vec::new())).collect(),
    )
}

/// [`inline_calls`] with each site's environment: the copy's captured
/// locals, sorted, and the caller's locals each stands for, and the
/// caller's [`crate::ir::Topology::sub`] when the caller has it.
///
/// # Errors
/// As [`inline_calls`].
pub fn inline_bound_calls(
    b: &mut Body,
    subs: Option<Vec<u32>>,
    sites: Vec<(Site, Vec<(u32, u32)>)>,
) -> Result<Vec<bool>, String> {
    let mut copied = vec![false; sites.len()];
    if sites.is_empty() {
        return Ok(copied);
    }
    // Each instruction's block, the first that lists it.
    let mut outer = vec![NONE; b.len()];
    for i in (0..b.len()).filter(|&i| b.tags[i] == Tag::Block) {
        for &x in b.record(b.data[i][0]) {
            if let Some(o) = outer.get_mut(x as usize).filter(|o| **o == NONE) {
                *o = u32_of(i);
            }
        }
    }
    let subs = subs.unwrap_or_else(|| b.inst_subs());
    let (mut ready, mut by_value_reads) = (Vec::new(), Vec::new());
    for (s, (site, environment)) in sites.into_iter().enumerate() {
        let (c, by_value) = (site.call, site.by_value);
        if let Some(mut r) = prepare(b, site, &outer) {
            r.environment = environment;
            r.sub = SubId::from_raw(subs[c]);
            // A by-value parameter's argument reads its place, at the
            // type the copy's local has.
            let args = b.record(b.data[c][1]);
            for &(l, a) in r
                .params
                .iter()
                .filter(|p| p.1 < 64 && by_value & (1 << p.1) != 0)
            {
                let arg = crate::ir::Ref(args[a])
                    .as_inst()
                    .filter(|v| b.tags[v.0 as usize] == Tag::Ref);
                by_value_reads.push((
                    arg.ok_or("a by-value argument that is no `Ref`")?.0,
                    r.k.local_ty[l as usize],
                ));
            }
            copied[s] = true;
            ready.push(r);
        }
    }
    // Each caller instruction moves past every copy at or before it, so a
    // call's number becomes its `Scope`'s; each copy starts where its call
    // moves to, less its own rows.
    let n = b.len();
    let (mut fin, mut shift, mut s) = (Vec::with_capacity(n), 0, ready.len());
    for i in 0..n {
        while s > 0 && ready[s - 1].c == i {
            s -= 1;
            ready[s].base = u32_of(i + shift);
            shift += ready[s].params.len() + ready[s].k.len() + 1 + ready[s].kept();
        }
        fin.push(u32_of(i + shift));
    }
    renumber(
        b,
        &mut Through {
            new_of: &fin,
            konst: 0,
        },
    )?;
    // Rows keep their places until the permutation below.
    let mut unaddressed = Vec::new();
    for (i, ty) in by_value_reads {
        b.tags[i as usize] = Tag::Read;
        b.data[i as usize][1] = crate::ir::ReadMode::Use as u32;
        b.ty[i as usize] = ty;
        unaddressed.extend(b.view(b.data[i as usize][0]).local());
    }
    if b.anchors.is_empty() {
        b.anchors = (0..u32_of(n)).collect();
    }
    // The copies' rows go at the columns' end, then into place by one
    // permutation, as a pass's rebuild does.
    let mut spans = Vec::with_capacity(ready.len());
    let rows = b.fallback.len();
    // Each call's block gains the copy's bindings and root before its
    // `Scope`. A block holding many calls is rebuilt once, after every
    // copy is in: one new record per site would leave a stale copy of
    // the block's list in `extra` each time, quadratic in its length.
    let mut inserts: HashMap<usize, HashMap<u32, Vec<u32>>> = HashMap::default();
    for r in ready {
        let (c, start) = (r.c, b.len());
        let (outer, scope, members) = copy_in(b, r)?;
        inserts.entry(outer).or_default().insert(scope, members);
        spans.push((c, start..b.len()));
    }
    let mut outers: Vec<usize> = inserts.keys().copied().collect();
    outers.sort_stable();
    for outer in outers {
        let before = &inserts[&outer];
        let mut list = Vec::new();
        for &x in b.record(b.data[outer][0]) {
            if let Some(members) = before.get(&x) {
                list.extend_from_slice(members);
            }
            list.push(x);
        }
        b.data[outer][0] = b.push_record(&list);
    }
    let mut order = Vec::with_capacity(b.len());
    for i in 0..n {
        match spans.last() {
            Some((c, span)) if *c == i => {
                order.extend(span.clone().map(u32_of));
                spans.pop();
            }
            _ => order.push(u32_of(i)),
        }
    }
    permute_columns(b, &order);
    if b.fallback.len() > rows {
        b.fallback.sort_stable_by_key(|r| r[0]);
    }
    for l in unaddressed {
        if !still_reached(b, l) {
            b.local_flags[l as usize] &= !local_flags::CAPTURED;
        }
    }
    Ok(copied)
}

/// Whether local `l` of `b`, whose `Ref`s a by-value binding now reads
/// instead (`Site::by_value`), is still reached other than by its own
/// instructions. The checker flags a local `CAPTURED` for a `Ref` of it,
/// a closure's capture (`cap_local`) and a `&dyn`'s held source, which
/// its `RefDyn`'s address block reads; a `&mut` (`CAPTURED_ASSIGNED`) is
/// never bound by value. With none of them left, the flag goes: only the
/// body's instructions change the local, as the passes and the backend
/// then assume.
fn still_reached(b: &Body, l: u32) -> bool {
    let reads_l = |x: u32| {
        let x = x as usize;
        b.tags.get(x) == Some(&Tag::Read) && b.view(b.data[x][0]).local() == Some(l)
    };
    b.local_flags[l as usize] & local_flags::CAPTURED_ASSIGNED != 0
        || b.cap_local.iter().any(|c| c.raw() == l)
        || (0..b.len()).any(|i| match b.tags[i] {
            Tag::Ref => b.view(b.data[i][0]).local() == Some(l),
            Tag::RefDyn => {
                let blk = b.data[i][0] as usize;
                b.tags.get(blk) == Some(&Tag::Block)
                    && b.record(b.data[blk][0]).iter().any(|&x| reads_l(x))
            }
            _ => false,
        })
}

/// Checks `site` against the caller, unchanged yet, and lists the
/// parameters it binds; `None` skips it.
fn prepare(b: &Body, site: Site, outer: &[u32]) -> Option<Ready> {
    let Site {
        call: c,
        copy: k,
        by_ref,
        by_value,
    } = site;
    let args = arguments(b, c);
    // Earlier default arguments are already evaluated for the enclosing
    // call. Bind only the ones this copy actually reads or addresses.
    // Inspect places instead of declaration flags, including raw TIR bodies.
    let default_uses = (k.kind == BodyKind::Default).then(|| {
        let mut used = vec![false; k.local_ty.len()];
        for (i, tag) in k.tags.iter().enumerate() {
            let (a, b) = tag.operands();
            for (side, op) in [a, b].into_iter().enumerate() {
                if op == Op::Place
                    && let Some(local) = k.view(k.data[i][side]).local()
                {
                    used[local as usize] = true;
                }
            }
        }
        used
    });
    let mut params = Vec::new();
    let mut refs = Vec::new();
    let mut count = 0;
    for l in 0..k.local_flags.len() {
        if k.local_flags[l] & local_flags::PARAM == 0 {
            continue;
        }
        if default_uses.as_ref().is_none_or(|used| used[l]) {
            if count < 64 && by_ref & (1 << count) != 0 {
                refs.push((u32_of(l), count));
            } else {
                params.push((u32_of(l), count));
            }
        }
        count += 1;
    }
    let outer = *outer.get(c).filter(|&&o| o != NONE)?;
    (args.len() == count && by_ref & by_value == 0 && !k.is_empty()).then_some(Ready {
        c,
        k,
        params,
        refs,
        outer: outer as usize,
        base: 0,
        environment: Vec::new(),
        sub: SubId::from_raw(0),
    })
}

/// The copy's tables: its rows' numbers once permuted, its records,
/// constants, locals and labels after the caller's, a captured local's the
/// caller's.
struct Moved<'a> {
    new_of: Vec<u32>,
    xbase: u32,
    konst: u32,
    lbase: u32,
    environment: &'a [(u32, u32)],
    lab: u32,
}

impl Remap for Moved<'_> {
    fn inst(&self, w: u32) -> Option<u32> {
        self.new_of.get(w as usize).copied()
    }

    fn konst(&mut self, _: &mut Body, c: u32) -> Option<u32> {
        Some(c + self.konst)
    }

    fn local(&self, l: u32) -> Option<u32> {
        Some(relocated_local(l, self.lbase, self.environment))
    }

    fn label(&mut self, _: &mut Body, l: u32) -> Option<u32> {
        Some(l + self.lab)
    }

    fn at(&self, w: u32) -> Option<u32> {
        Some(w + self.xbase)
    }
}

fn relocated_local(local: u32, lbase: u32, environment: &[(u32, u32)]) -> u32 {
    environment
        .binary_search_by_key(&local, |&(copy, _)| copy)
        .map_or(local + lbase, |i| environment[i].1)
}

/// Appends site `r`'s rows to the renumbered caller, numbered as they will
/// stand once permuted: the `Store`s, the callee's instructions but its
/// root, the captured reads and their cleanup, the final `Break`, the
/// root, the `Scope`. Each `Return` becomes a `Break` to the root. Returns
/// the call's block, the `Scope`, and the members the block lists before
/// it.
fn copy_in(out: &mut Body, r: Ready) -> Result<(usize, u32, Vec<u32>), String> {
    let keep = r.kept();
    let Ready {
        c,
        mut k,
        params,
        refs,
        outer,
        base,
        environment,
        sub,
    } = r;
    let (m, np) = (k.len(), params.len());
    let args = out.record(out.data[c][1]).to_vec();
    let root_at = base + u32_of(np + m + keep);
    let (start, lbase) = (out.len(), u32_of(out.local_ty.len()));
    let label = u32_of(out.label_inst.len() + k.label_inst.len());
    let mut map = Moved {
        new_of: (0..m)
            .map(|j| {
                if j + 1 == m {
                    root_at
                } else {
                    base + u32_of(np + j)
                }
            })
            .collect(),
        xbase: u32_of(out.extra.len()),
        konst: u32_of(out.consts.len()),
        lbase,
        environment: &environment,
        lab: u32_of(out.label_inst.len()),
    };
    let root = start + np + m + keep;
    grow(out, root + 2);
    // The callee's records move whole, after the caller's.
    out.extra.extend_from_slice(&k.extra);
    let rows: Vec<(u32, u32)> = (0..m)
        .map(|j| {
            (
                u32_of(j),
                u32_of(if j + 1 == m { root } else { start + np + j }),
            )
        })
        .collect();
    copy_rows(&k, &rows, out, &mut map)?;
    SideRows::take(&mut k).copy(out, &mut map)?;
    out.label_inst
        .extend(k.label_inst.iter().map(|&x| map.new_of[x as usize]));
    out.label_inst.push(root_at);
    for &(_, n) in &rows[..m - 1] {
        let n = n as usize;
        if out.tags[n] == Tag::Return {
            out.tags[n] = Tag::Break;
            out.data[n] = [label, out.data[n][0]];
        }
    }
    let refs: Vec<(u32, u32)> = refs
        .into_iter()
        .map(|(local, arg)| (relocated_local(local, lbase, &environment), args[arg]))
        .collect();
    substitute_refs(out, start + np..start + np + m - 1, &refs)?;
    let syn = out.syn[c];
    let ty = out.ty[c];
    // The bindings stand right before the `Scope` in the call's block, so
    // an argument is consumed in the block that made it; the block also
    // lists the blocks the `Scope` runs, its cleanup suite and its root: a
    // child block stands in the block around its runner (`Topology::blk`).
    let scope = root_at + 1;
    let cleanup_root = if keep == 0 { root_at } else { root_at - 2 };
    let mut before: Vec<u32> = (0..np).map(|p| base + u32_of(p)).collect();
    before.extend((cleanup_root != root_at).then_some(cleanup_root));
    before.push(root_at);
    // The root's members: the callee's, then the final `Break`.
    let [members, tail] = out.data[root];
    let mut list = out.record(members).to_vec();
    list.push(root_at - 1);
    let rec = out.push_record(&list);
    let cleanup_start = base + u32_of(np + m - 1);
    let cleanup_members: Vec<u32> = (0..u32_of(keep.saturating_sub(1)))
        .map(|i| cleanup_start + i)
        .collect();
    let cleanup_record = out.push_record(&cleanup_members);
    let cleanups = out.push_record(&if keep == 0 {
        Vec::new()
    } else {
        vec![cleanup_root]
    });
    // The copy's own instructions name the callee's as their anchors
    // ([`Body::inlined`]), so a panic in the copy reports the callee's
    // frame, then the call's; one without a position keeps none, as in
    // the callee, and so reports its enclosing one. A copy that holds
    // copies of its own (a release build's, inlined bottom-up) brings their
    // rows after its own, each called at its anchor in the copy.
    let anchor = out.anchors[c];
    let first = out.inlined.last().map_or(0, |e| e[0] + e[1]);
    // A sparse closure body retains enclosing-body source indices. Give
    // every referenced source row a dense virtual ID, instead of reserving
    // its entire enclosing body's address range for each call.
    let mut source_rows: Vec<u32> = if k.anchors.is_empty() {
        (0..u32_of(m)).collect()
    } else {
        k.anchors
            .iter()
            .copied()
            .chain(k.inlined.iter().map(|f| f[3]))
            .filter(|&a| a != NONE && a & INLINED == 0)
            .collect()
    };
    source_rows.sort_stable();
    source_rows.dedup();
    if source_rows.is_empty() {
        source_rows.push(0);
    }
    let nested = first + u32_of(source_rows.len());
    let source_ids: HashMap<_, _> = source_rows
        .iter()
        .enumerate()
        .map(|(i, &a)| (a, u32_of(i)))
        .collect();
    let moved = |a: u32| match a {
        NONE => anchor,
        a if a & INLINED != 0 => INLINED | (nested + (a & !INLINED)),
        // Ruled out: the same anchor and call-site columns populated source_ids.
        a => INLINED | (first + source_ids[&a]),
    };
    push_frame(out, source_rows.iter().copied(), k.item.raw(), anchor);
    for &[f, len, callee, call] in &k.inlined {
        out.inlined.push([nested + f, len, callee, moved(call)]);
    }
    for &[at, source] in &k.inlined_origins {
        out.inlined_origins.push([nested + at, source]);
    }
    // Each row the copy adds takes the call's position; the callee's keep
    // theirs, or none.
    let mut put = |n: usize, row: Option<(Tag, [u32; 2], Ty)>, own: Option<usize>| {
        if let Some((tag, data, ty)) = row {
            out.tags[n] = tag;
            out.data[n] = data;
            out.ty[n] = ty;
        }
        let own = own.map(|j| (k.syn[j], moved(k.anchor(u32_of(j)))));
        out.syn[n] = match own {
            Some((NodeIdx::NONE, _)) => NodeIdx::NONE,
            _ => syn,
        };
        out.anchors[n] = own.map_or(anchor, |o| o.1);
    };
    for (p, &(l, a)) in params.iter().enumerate() {
        let data = [(l + lbase) | place::BARE_LOCAL, args[a]];
        put(start + p, Some((Tag::Store, data, Ty::VOID)), None);
    }
    for j in 0..m - 1 {
        put(start + np + j, None, Some(j));
    }
    let mut n = start + np + m - 1;
    for &(local, parent) in environment.iter().take(keep.saturating_sub(1)) {
        let data = [
            place::BARE_LOCAL | parent,
            crate::ir::ReadMode::Borrow as u32,
        ];
        put(n, Some((Tag::Read, data, k.local_ty[local as usize])), None);
        n += 1;
    }
    if keep != 0 {
        put(
            n,
            Some((Tag::Block, [cleanup_record, NONE], Ty::VOID)),
            None,
        );
        n += 1;
    }
    put(n, Some((Tag::Break, [label, tail], Ty::NEVER)), None);
    // The root takes the call's type, which its `Break`s carry.
    put(root, Some((Tag::Block, [rec, NONE], ty)), Some(m - 1));
    put(root + 1, Some((Tag::Scope, [root_at, cleanups], ty)), None);
    let sbase = out.strings.len() as u64;
    for l in 0..k.local_ty.len() {
        out.local_ty.push(k.local_ty[l]);
        out.local_name.push(k.local_name[l]);
        out.local_syn.push(syn);
        out.local_flags.push(k.local_flags[l] & !local_flags::PARAM);
    }
    for &(local, _) in &refs {
        out.local_flags[local as usize] = 0;
    }
    for &(t, bits) in &k.consts {
        out.consts
            .push((t, if t == Ty::STRING { bits + sbase } else { bits }));
    }
    out.strings.append(&mut k.strings);
    // An instantiation's captures moved with it (`copy_rows`); the shared
    // cells' facts take rows of their own.
    for &(_, c) in &k.inlined_shared {
        let local = relocated_local(k.cap_local[c as usize].raw(), lbase, &environment);
        out.inlined_shared.push((sub, u32_of(out.cap_local.len())));
        out.cap_local.push(LocalId::from_raw(local));
        out.cap_mode.push(k.cap_mode[c as usize]);
    }
    out.inlined_capture_origins
        .extend(
            k.inlined_capture_origins
                .iter()
                .map(|&(_, snapshot, creator)| {
                    (
                        sub,
                        LocalId::from_raw(relocated_local(snapshot.raw(), lbase, &environment)),
                        LocalId::from_raw(relocated_local(creator.raw(), lbase, &environment)),
                    )
                }),
        );
    Ok((outer, scope, before))
}

/// The reader proof permits only reads through a reference parameter,
/// including a whole read used as a match scrutinee. Substitute the
/// caller's evaluated reference in the copied rows `rows`, so the argument
/// is neither duplicated nor mistaken for a callee value.
fn substitute_refs(
    body: &mut Body,
    rows: std::ops::Range<usize>,
    refs: &[(u32, u32)],
) -> Result<(), String> {
    if refs.is_empty() {
        return Ok(());
    }
    for i in rows {
        let ops = body.tags[i].operands();
        for (x, op) in [ops.0, ops.1].into_iter().enumerate() {
            if op != Op::Place {
                continue;
            }
            let Some(mut place) = body.decode_place(body.data[i][x]) else {
                return Err("a reader with a malformed place".to_owned());
            };
            let crate::ir::PlaceRoot::Local(local) = place.root else {
                continue;
            };
            let Some(&(_, argument)) = refs.iter().find(|&&(l, _)| l == local.raw()) else {
                continue;
            };
            if place.proj.is_empty() {
                if body.tags[i] != Tag::Read {
                    return Err("a reader that changes its reference binding".to_owned());
                }
                let identity = body.push_record(&[crate::ir::Coercion::Identity as u32, NONE]);
                body.tags[i] = Tag::Coerce;
                body.data[i] = [argument, identity];
            } else {
                place.root = crate::ir::PlaceRoot::Value(crate::ir::Ref(argument));
                // ruled out: only a bare local lacks words; this root is a value.
                body.data[i][x] = body.push_record(&place.words().expect("a value-rooted place"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::{Ty, TyList};

    use super::{
        Chosen, Site, copyable, inlinable, inline_calls, known_call, leaf, plain_call, rechoose,
        u32_of,
    };
    use crate::ir::{
        Body, BodyKind, Callee, ChoiceKind, IntrinsicOp, Op, Place, PlaceRoot, Proj, Providers,
        ReadMode, Ref, Tag, TirBuilder, TirSink, local_flags, map_body_types, verify, verify_owned,
    };
    use crate::own::tests::{Cx, show};
    use crate::own::{OwnScratch, run};

    const N: NodeIdx = NodeIdx::from_raw(1);

    /// Copies one call, as [`inline_calls`] does for each of a body's.
    fn inline_call(b: &mut Body, call: usize, copy: Body) -> Result<(), String> {
        match inline_calls(
            b,
            vec![Site {
                call,
                copy,
                by_value: 0,
                by_ref: 0,
            }],
        )?[..]
        {
            [true] => Ok(()),
            _ => Err("skipped".into()),
        }
    }
    /// The callee's type parameter, which the call substitutes by `string`.
    const T: Ty = Ty(1_000_001);

    #[test]
    fn a_read_through_copy_uses_its_evaluated_reference_without_a_binding() {
        let pool = hd_types::InternPool::default();
        let reference = pool.intern_ty(&hd_types::TyData::Ref {
            inner: Ty::I32,
            mutable: false,
            counted: false,
        });
        let mut b = TirBuilder::new(DefId::from_raw(3), BodyKind::Fn);
        let block = b.open_block();
        let parameter = b.local(reference, Symbol::from_raw(0), local_flags::PARAM, N);
        let place = b.place(&Place {
            root: PlaceRoot::Local(parameter),
            proj: vec![Proj::Deref],
        });
        let value = b.emit(Tag::Read, place, ReadMode::Use as u32, Ty::I32, N);
        let root = b.close_block(block, Some(value), Ty::I32, N);
        let copy = b.finish(root, &[]).expect("reader verifies");
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let block = b.open_block();
        let argument = b.call(&item(7), &[], Providers::None, reference, N);
        let call = b.call(&item(3), &[argument], Providers::None, Ty::I32, N);
        let root = b.close_block(block, Some(call), Ty::I32, N);
        let mut body = b.finish(root, &[]).expect("caller verifies");
        assert_eq!(
            inline_calls(
                &mut body,
                vec![Site {
                    call: call.0 as usize,
                    copy,
                    by_value: 0,
                    by_ref: 1,
                }]
            ),
            Ok(vec![true])
        );
        assert_eq!(body.tags.iter().filter(|&&t| t == Tag::Store).count(), 0);
        assert_eq!(
            (0..body.len())
                .filter(|&i| plain_call(&body, i) == Some(item(7)))
                .count(),
            1
        );
        let read = (0..body.len())
            .find(|&i| body.tags[i] == Tag::Read)
            .expect("the dereference");
        assert_eq!(
            body.decode_place(body.data[read][0]),
            Some(Place {
                root: PlaceRoot::Value(argument),
                proj: vec![Proj::Deref],
            })
        );
        body.anchors.clear();
        assert_eq!(verify(&body), []);
    }

    #[test]
    fn provider_keys_keep_their_type_when_the_copy_moves_its_records() -> Result<(), String> {
        let mut b = TirBuilder::new(DefId::from_raw(3), BodyKind::Fn);
        let block = b.open_block();
        let record = b.words_record(&[T.0]);
        let value = b.emit(Tag::ProviderGet, record, crate::ir::NONE, Ty::I32, N);
        let root = b.close_block(block, Some(value), Ty::I32, N);
        let copy = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
        assert!(copyable(&copy));
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let block = b.open_block();
        // The caller already owns extra records with unrelated numeric words.
        b.words_record(&[0x8000_0000]);
        let call = b.call(&item(3), &[], Providers::None, Ty::I32, N);
        let root = b.close_block(block, Some(call), Ty::I32, N);
        let mut body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
        inline_call(&mut body, call.0 as usize, copy)?;
        let keys: Vec<_> = (0..body.len())
            .filter(|&i| body.tags[i] == Tag::ProviderGet)
            .map(|i| body.record(body.data[i][0]))
            .collect();
        assert_eq!(keys, [&[T.0][..]]);
        // The raw verifier precedes ownership and its source-anchor rows.
        body.anchors.clear();
        assert_eq!(verify(&body), []);
        Ok(())
    }

    /// `fn f(x: T) -> T { loop { if true { return x } break }; "s" }`:
    /// a loop, a `return` inside it, a constant and a string.
    fn callee() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(3), BodyKind::Fn);
        let blk = b.open_block();
        let x = b.local(T, Symbol::from_raw(0), local_flags::PARAM, N);
        let lp = b.open_loop();
        let body = b.open_block();
        let yes = b.const_value(Ty::BOOL, 1);
        let then = b.open_block();
        let v = b.get(x, T, N);
        let ret = b.emit(Tag::Return, v.0, crate::ir::NONE, Ty::NEVER, N);
        let then = b.close_block(then, Some(ret), Ty::NEVER, N);
        let other = b.open_block();
        let other = b.close_block(other, None, Ty::VOID, N);
        let arms = b.refs_record(&[then, other]);
        b.emit(Tag::If, yes.0, arms, Ty::VOID, N);
        b.emit(Tag::Break, lp.0.raw(), crate::ir::NONE, Ty::NEVER, N);
        let body = b.close_block(body, None, Ty::VOID, N);
        b.close_loop(lp, body, Ty::VOID, N);
        let s = b.const_str("s");
        let root = b.close_block(blk, Some(s), T, N);
        let mut c = b.finish(root, &[]).expect("callee verifies");
        map_body_types(&mut c, &|t| if t == T { Ty::STRING } else { t }, &|l| l);
        c
    }

    /// `g() { 9(3(7())) }`, with call 3 inlined.
    fn caller() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let blk = b.open_block();
        let s = b.const_str("caller");
        let item = |d| Callee::Item {
            def: DefId::from_raw(d),
            targs: TyList::EMPTY,
        };
        let a = b.call(&item(7), &[s], Providers::None, Ty::STRING, N);
        let r = b.call(&item(3), &[a], Providers::None, Ty::STRING, N);
        b.call(&item(9), &[r], Providers::None, Ty::VOID, N);
        let root = b.close_block(blk, None, Ty::VOID, N);
        b.finish(root, &[]).expect("caller verifies")
    }

    /// The copy binds the parameter with a `Store` in the call's block,
    /// turns the `return` into a `Break` to its root block, which a `Scope`
    /// owns in the call's place, and keeps the caller's constants apart
    /// from its own. The ownership passes then move the argument into the
    /// parameter, move the parameter out at the `return`, and drop it after
    /// the `Scope` on the path that breaks the loop.
    #[test]
    fn a_call_becomes_its_callee_bound_and_breaking_to_its_block() {
        let mut b = caller();
        let c = callee();
        assert!(inlinable(&c, 6) && !inlinable(&c, 5));
        assert!(leaf(&c) && !leaf(&b));
        let item = Callee::Item {
            def: DefId::from_raw(3),
            targs: TyList::EMPTY,
        };
        assert_eq!(plain_call(&b, 1), Some(item));
        inline_call(&mut b, 1, c).expect("inlines");
        // Anchors are the driver's, which the checker's TIR has none of.
        let mut v = b.clone();
        v.anchors.clear();
        assert_eq!(verify(&v), []);
        // The binding, the `Break` and the `Scope` stand for the call; the
        // callee's instructions for themselves, called at anchor 1.
        let i = |j| crate::ir::INLINED | j;
        assert_eq!(
            b.anchors,
            [
                0,
                1,
                i(0),
                i(1),
                i(2),
                i(3),
                i(4),
                i(5),
                i(6),
                i(7),
                1,
                i(8),
                1,
                2,
                3
            ]
        );
        assert_eq!(b.inlined_frame(i(4)), Some((DefId::from_raw(3), 4, 1)));
        assert_eq!(b.inlined_frame(1), None);
        let k = b.consts.len() - 1;
        let s = b.consts[k];
        assert_eq!((s.0, s.1, &*b.strings[1]), (Ty::STRING, 1, "s"));
        let root = b.label_inst[1];
        assert_eq!(
            (b.tags[12], root, b.data[root as usize][1]),
            (Tag::Scope, 11, crate::ir::NONE)
        );
        assert_eq!(
            (b.tags[10], b.data[10]),
            (Tag::Break, [1, Ref::konst(u32_of(k)).0])
        );
        run(&mut b, &Cx, &mut OwnScratch::default()).expect("passes");
        assert_eq!(verify_owned(&b, &Cx), []);
        assert_eq!(
            show(&b),
            "%0 Call\n%1 Store [$0] %0\n%2 Read move [$0]\n%3 Break %2\n\
             %4 Block [%2 %3] -> %3\n%5 Block []\n%6 If\n%7 Break\n\
             %8 Block [%4 %5 %6 %7]\n%9 Loop\n%10 Break\n%11 Block [%8 %9 %10]\n\
             %12 Scope\n%13 Drop [$0]\n%14 Call %12\n\
             %15 Block [%0 %1 %11 %12 %13 %14]\n"
        );
    }

    /// `fn m(x: string) -> string { 3(x) }`, as callee 5.
    fn middle() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(5), BodyKind::Fn);
        let blk = b.open_block();
        let x = b.local(Ty::STRING, Symbol::from_raw(0), local_flags::PARAM, N);
        let v = b.get(x, Ty::STRING, N);
        let r = b.call(&item(3), &[v], Providers::None, Ty::STRING, N);
        let root = b.close_block(blk, Some(r), Ty::STRING, N);
        b.finish(root, &[]).expect("middle verifies")
    }

    /// A copy that holds a copy (a release build inlines bottom-up) brings
    /// its row after its own, called at an anchor inside it: a frame in the
    /// inner copy names callee 3, then callee 5, then the caller's call.
    #[test]
    fn a_copy_inside_a_copy_is_called_from_its_outer_copy() {
        let mut m = middle();
        let call = (0..m.len()).find(|&i| m.tags[i] == Tag::Call);
        inline_call(&mut m, call.expect("the call of 3"), callee()).expect("inlines 3");
        assert!(leaf(&m));
        let mut b = caller();
        inline_call(&mut b, 1, m).expect("inlines 5");
        let [outer, inner] = b.inlined[..] else {
            panic!("two rows: {:?}", b.inlined);
        };
        assert_eq!((outer[2], outer[3]), (5, 1));
        let i = crate::ir::INLINED;
        assert_eq!(inner[2], 3);
        assert!(inner[3] & i != 0 && (inner[3] & !i) < inner[0]);
        let deepest = (0..b.len())
            .map(|k| b.anchors[k])
            .find(|&a| a & i != 0 && (a & !i) >= inner[0])
            .expect("an instruction of the inner copy");
        let (callee, _, at) = b.inlined_frame(deepest).expect("inner frame");
        let (outer_callee, _, call) = b.inlined_frame(at).expect("outer frame");
        assert_eq!((callee.raw(), outer_callee.raw(), call), (3, 5, 1));
        run(&mut b, &Cx, &mut OwnScratch::default()).expect("passes");
        assert_eq!(verify_owned(&b, &Cx), []);
    }

    /// `g() { r = 3(7("caller")); if true { 9(3(r)) } }`: two calls of
    /// callee 3 in two blocks, the second taking the first's result.
    fn two_calls() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let blk = b.open_block();
        let s = b.const_str("caller");
        let a = b.call(&item(7), &[s], Providers::None, Ty::STRING, N);
        let r = b.call(&item(3), &[a], Providers::None, Ty::STRING, N);
        let yes = b.const_value(Ty::BOOL, 1);
        let then = b.open_block();
        let r2 = b.call(&item(3), &[r], Providers::None, Ty::STRING, N);
        b.call(&item(9), &[r2], Providers::None, Ty::VOID, N);
        let then = b.close_block(then, None, Ty::VOID, N);
        let other = b.open_block();
        let other = b.close_block(other, None, Ty::VOID, N);
        let arms = b.refs_record(&[then, other]);
        b.emit(Tag::If, yes.0, arms, Ty::VOID, N);
        let root = b.close_block(blk, None, Ty::VOID, N);
        b.finish(root, &[]).expect("caller verifies")
    }

    /// One rebuild for all of `b`'s calls of callee 3 gives the body a
    /// rebuild per call gives, the latest call first.
    fn one_splice_equals_one_per_copy(b: Body, copy: &dyn Fn() -> Body) -> Body {
        let calls: Vec<usize> = (0..b.len())
            .rev()
            .filter(|&i| plain_call(&b, i) == Some(item(3)))
            .collect();
        let site = |call| Site {
            call,
            copy: copy(),
            by_value: 0,
            by_ref: 0,
        };
        let mut one = b.clone();
        let sites = calls.iter().map(|&c| site(c)).collect();
        assert_eq!(inline_calls(&mut one, sites), Ok(vec![true, true]));
        let mut each = b;
        for &c in &calls {
            assert_eq!(inline_calls(&mut each, vec![site(c)]), Ok(vec![true]));
        }
        // The records may lie in `extra` in another order, and one per copy
        // leaves the block records it replaced behind; every record in use
        // is the same. Each is compared at its place in use order.
        let live = |b: &Body| {
            let mut v = b.clone();
            v.extra.clear();
            for i in 0..b.len() {
                let (oa, ob) = b.tags[i].operands();
                for (k, (w, op)) in b.data[i].into_iter().zip([oa, ob]).enumerate() {
                    let rec = matches!(op, Op::Values | Op::Blocks | Op::Record)
                        || (op == Op::Place && w & crate::ir::place::BARE_LOCAL == 0);
                    if rec && w != crate::ir::NONE {
                        v.data[i][k] = u32_of(v.extra.len());
                        v.extra.extend_from_slice(b.record(w));
                    }
                }
            }
            v
        };
        assert_eq!(live(&one), live(&each));
        one
    }

    #[test]
    fn one_splice_for_all_copies_equals_one_per_copy() {
        let mut b = one_splice_equals_one_per_copy(two_calls(), &callee);
        run(&mut b, &Cx, &mut OwnScratch::default()).expect("passes");
        assert_eq!(verify_owned(&b, &Cx), []);
        // `x = 7(); 3(x.1, "q"); 3(x.1, "q")`: each copy binds `p` and `q`
        // and stores `p`; the later call's copy, appended first, has the
        // lower locals.
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let blk = b.open_block();
        let x = b.local(LIST, Symbol::from_raw(0), 0, N);
        let made = b.call(&item(7), &[], Providers::None, LIST, N);
        b.set(x, made, N);
        for _ in 0..2 {
            let w = b.place(&Place {
                root: PlaceRoot::Local(x),
                proj: vec![Proj::Field(1)],
            });
            let a = b.emit(Tag::Read, w, ReadMode::Use as u32, LIST, N);
            let q = b.const_str("q");
            b.call(&item(3), &[a, q], Providers::None, Ty::VOID, N);
        }
        let root = b.close_block(blk, None, Ty::VOID, N);
        let b = b.finish(root, &[]).expect("caller verifies");
        let b = one_splice_equals_one_per_copy(b, &|| changer(true));
        let stores = (0..b.len()).filter(|&i| b.tags[i] == Tag::Store);
        let places: Vec<_> = stores
            .filter_map(|i| b.decode_place(b.data[i][0]).map(|p| p.to_string()))
            .collect();
        assert_eq!(
            places,
            ["[$0]", "[$3]", "[$4]", "[$3]", "[$1]", "[$2]", "[$1]"]
        );
    }

    /// The test context's list type ([`crate::own::tests::Cx`]).
    const LIST: Ty = Ty(1_000_000);

    fn item(d: u32) -> Callee {
        Callee::Item {
            def: DefId::from_raw(d),
            targs: TyList::EMPTY,
        }
    }

    /// `f(p: LIST, q: string) { 8(p.0); p = 7() }`, as callee 3, without
    /// the store unless `store`.
    fn changer(store: bool) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(3), BodyKind::Fn);
        let blk = b.open_block();
        let p = b.local(LIST, Symbol::from_raw(0), local_flags::PARAM, N);
        b.local(Ty::STRING, Symbol::from_raw(1), local_flags::PARAM, N);
        let w = b.place(&Place {
            root: PlaceRoot::Local(p),
            proj: vec![Proj::Field(0)],
        });
        let v = b.emit(Tag::Read, w, ReadMode::Use as u32, LIST, N);
        b.call(&item(8), &[v], Providers::None, Ty::VOID, N);
        if store {
            let made = b.call(&item(7), &[], Providers::None, LIST, N);
            b.set(p, made, N);
        }
        let root = b.close_block(blk, None, Ty::VOID, N);
        b.finish(root, &[]).expect("callee verifies")
    }

    /// `g() { x = 7(); 3(x, { x.push("s"); "q" }) }`.
    fn changing_caller() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let blk = b.open_block();
        let x = b.local(LIST, Symbol::from_raw(0), 0, N);
        let made = b.call(&item(7), &[], Providers::None, LIST, N);
        b.set(x, made, N);
        let a = b.get(x, LIST, N);
        let inner = b.open_block();
        let r = b.get(x, LIST, N);
        let v = b.const_str("s");
        let rec = b.refs_record(&[r, v]);
        b.emit(
            Tag::Intrinsic,
            IntrinsicOp::ListPush as u32,
            rec,
            Ty::VOID,
            N,
        );
        let q = b.const_str("q");
        let s = b.close_block(inner, Some(q), Ty::STRING, N);
        b.call(&item(3), &[a, s], Providers::None, Ty::VOID, N);
        let root = b.close_block(blk, None, Ty::VOID, N);
        b.finish(root, &[]).expect("caller verifies")
    }

    /// A list bound by a `Store` from a local a later argument changes
    /// takes no copy of its own: its count is its copy, which the change
    /// makes unique.
    #[test]
    fn a_bound_argument_shares_its_list_with_the_local_a_later_argument_changes() {
        let mut b = changing_caller();
        let c = (0..b.len()).find(|&i| plain_call(&b, i) == Some(item(3)));
        let c = c.expect("the call");
        inline_call(&mut b, c, changer(false)).expect("inlines");
        run(&mut b, &Cx, &mut OwnScratch::default()).expect("passes");
        assert_eq!(verify_owned(&b, &Cx), []);
        let text = show(&b);
        let change = text.find("IsUnique").expect("the change");
        let copies = text.find(" Intrinsic %").is_some_and(|c| c < change);
        assert!(!copies);
    }

    /// A `Bound` call at a receiver substitution made concrete is chosen
    /// again as the impl or builtin `pick` names; one `pick` refuses stays
    /// a `Bound`.
    #[test]
    fn a_concrete_bound_call_is_chosen_again_as_an_impl_or_a_builtin() {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let blk = b.open_block();
        let at = |self_ty| Callee::TraitMethod {
            trait_: DefId::from_raw(5),
            method: DefId::from_raw(6),
            self_ty,
            targs: TyList::EMPTY,
            choice: (ChoiceKind::Bound, 0),
        };
        let s = b.const_str("s");
        b.call(&at(Ty::STRING), &[s], Providers::None, Ty::VOID, N);
        b.call(&at(T), &[s], Providers::None, Ty::VOID, N);
        b.call(&at(Ty::I32), &[s], Providers::None, Ty::VOID, N);
        let root = b.close_block(blk, None, Ty::VOID, N);
        let mut b = b.finish(root, &[]).expect("verifies");
        rechoose(&mut b, &|_, t, _| match t {
            Ty::STRING => Some(Chosen::Impl(DefId::from_raw(7))),
            Ty::I32 => Some(Chosen::Builtin(3)),
            _ => None,
        });
        let choice = |i| match plain_call(&b, i) {
            Some(Callee::TraitMethod { choice, .. }) => choice,
            c => panic!("{c:?}"),
        };
        assert_eq!(choice(0), (ChoiceKind::Impl, 7));
        assert_eq!(choice(1), (ChoiceKind::Bound, 0));
        assert_eq!(choice(2), (ChoiceKind::Builtin, 3));
        assert_eq!(verify(&b), []);
    }

    #[test]
    fn switches_dynamic_calls_and_host_calls_are_copyable() {
        let make = |tag, data, extra| {
            let mut body = Body::new(DefId::from_raw(1), BodyKind::Fn);
            body.tags = vec![tag, Tag::Block];
            body.data = vec![data, [crate::ir::NONE, crate::ir::NONE]];
            body.ty = vec![Ty::VOID, Ty::VOID];
            body.syn = vec![N, N];
            body.extra = extra;
            body.sub_root = vec![1];
            body
        };
        let switch = make(Tag::SwitchInt, [0, 0], vec![1, 1]);
        let dynamic = make(Tag::CallDyn, [0, 0], vec![6, 10, 20, 0, 0, 0, 0]);
        let host = make(Tag::CallHost, [0, 0], vec![0]);
        assert!(copyable(&switch));
        assert!(copyable(&dynamic));
        assert!(copyable(&host));
    }

    #[test]
    fn copies_switch_dynamic_and_host_callees() {
        let copies = |copy: Body, tag| {
            let mut body = caller();
            inline_call(&mut body, 1, copy).expect("inlines");
            assert!(body.tags.contains(&tag));
            body.anchors.clear();
            assert_eq!(verify(&body), []);
        };

        let mut switch = callee();
        let at = switch
            .tags
            .iter()
            .position(|tag| *tag == Tag::If)
            .expect("callee has an if");
        let arms = switch.record(switch.data[at][1]);
        let (yes, no) = (arms[0], arms[1]);
        let cases = switch.push_record(&[1, yes, no]);
        switch.tags[at] = Tag::SwitchInt;
        switch.data[at][1] = cases;
        copies(switch, Tag::SwitchInt);

        let mut dynamic = callee();
        let record = dynamic.push_record(&[8, 0, 0, Ref::konst(0).0, 0, 0, 0]);
        dynamic.tags[0] = Tag::CallDyn;
        dynamic.data[0] = [Ref::konst(0).0, record];
        copies(dynamic, Tag::CallDyn);

        let mut host = callee();
        let record = host.push_record(&[Ref::konst(0).0]);
        host.tags[0] = Tag::CallHost;
        host.data[0] = [Ref::konst(0).0, record];
        copies(host, Tag::CallHost);
    }

    #[test]
    fn a_dynamic_call_in_the_caller_does_not_block_a_copy() {
        let mut body = caller();
        let receiver = body.record(body.data[0][1])[0];
        let record = u32_of(body.extra.len());
        body.extra.extend([6, 8, 0, 0, 0, 0, 0]);
        body.tags[0] = Tag::CallDyn;
        body.data[0] = [receiver, record];
        assert_eq!(
            inline_calls(
                &mut body,
                vec![Site {
                    call: 1,
                    copy: callee(),
                    by_value: 0,
                    by_ref: 0,
                }]
            ),
            Ok(vec![true])
        );
    }

    #[test]
    fn a_decorated_call_can_copy_an_omitted_default_argument() {
        let mut body = caller();
        body.tags[0] = Tag::DefaultCall;
        assert_eq!(known_call(&body, 1), Some(item(3)));
        assert_eq!(plain_call(&body, 1), None);
        inline_call(&mut body, 1, callee()).expect("decorated call inlines");
    }

    #[test]
    fn a_default_copy_keeps_argument_evaluation_and_binds_only_used_parameters() {
        for entries in [4u32, 65, 1000] {
            let mut default = TirBuilder::new(DefId::from_raw(3), BodyKind::Default);
            let block = default.open_block();
            let mut last = None;
            for k in 0..entries {
                last = Some(default.local(Ty::STRING, Symbol::from_raw(k), local_flags::PARAM, N));
            }
            let tail = default.get(last.expect("at least four parameters"), Ty::STRING, N);
            let root = default.close_block(block, Some(tail), Ty::STRING, N);
            let copy = default.finish(root, &[]).expect("default verifies");
            assert!(copyable(&copy));

            let mut caller = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
            let block = caller.open_block();
            let text = caller.const_str("earlier");
            let effect = caller.call(&item(7), &[], Providers::None, Ty::STRING, N);
            let target = caller.refs_record(&[Ref(3), Ref(TyList::EMPTY.0)]);
            let mut args = vec![text; entries as usize];
            *args.last_mut().expect("at least four arguments") = effect;
            let earlier = caller.call_args_record(&args, Providers::None);
            let call = caller.emit(Tag::DefaultCall, target, earlier, Ty::STRING, N);
            let root = caller.close_block(block, Some(call), Ty::STRING, N);
            let mut body = caller.finish(root, &[]).expect("caller verifies");
            assert_eq!(known_call(&body, call.0 as usize), Some(item(3)));
            inline_call(&mut body, call.0 as usize, copy).expect("default inlines");
            assert!(!body.tags.contains(&Tag::DefaultCall));
            assert_eq!(
                body.tags.iter().filter(|&&tag| tag == Tag::Store).count(),
                1
            );
            assert_eq!(body.tags.iter().filter(|&&tag| tag == Tag::Call).count(), 1);
            body.anchors.clear();
            assert_eq!(verify(&body), []);
        }
    }
}
