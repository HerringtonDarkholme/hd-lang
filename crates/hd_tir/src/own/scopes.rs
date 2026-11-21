//! Cleanup scopes and cancel exits lowered to blocks (steps 7a and 7b of
//! `docs/future-work/BACKEND_NEUTRAL_TIR.md`), the first ownership pass.
//! A `Scope` with suites becomes a plain wrapper. Its body's end runs a
//! copy of each suite registered so far, last in, first out; each way out
//! of it breaks to a chain shared per (target, registered count), a
//! labelled block whose end runs the suites and takes the jump, as rustc's
//! drop tree shares a drop chain per target. A `Defer` registers its suite
//! where it stands, so no flag is needed; a suite no `Defer` names (an
//! inlined closure's environment reads) runs on every way out. Each
//! suspension point gets its cancel block ([`Body::cancel`]): the suites
//! around it, then a cancelled `Return`, as rustc's `Yield` names its
//! `drop` target; a shielded point in a waiting suite's copy gets the
//! scope's cancel exit (flow.using.async.cancel.shielded). Dispose copies
//! are named in [`Body::suites`].
//!
//! Cost: one walk, O(instructions), plus the copies, O(exit sites +
//! targets x suites + points x suites around them); one snapshot of a body
//! with a point or a cleanup scope, and one renumbering.

use hd_base::NodeIdx;
use hd_base::{HashMap, SliceSort as _};
use hd_types::Ty;

use super::splice::permute_columns;

use crate::ir::{
    Body, NONE, Ref, Remap, SideRows, Tag, Through, Topology, copy_rows, for_blocks, grow, inside,
    is_suspension, renumber, return_word, u32_of,
};

/// One open cleanup scope: its suites, and how many its body registered.
#[derive(Clone)]
struct Open {
    body: u32,
    suites: Vec<u32>,
    registered: usize,
    /// The shared exit chains of the ways out that leave this scope as
    /// their innermost: one per (target, registered count), in order.
    exits: Vec<Chain>,
}

/// A shared exit chain: what a way out breaks to (`label`, carrying a
/// value of type `ty`), the jump it then takes (`exit`, a source row),
/// the outermost scope it leaves (`from`) and how many of this scope's
/// suites ran (`registered`).
#[derive(Clone, Copy)]
struct Chain {
    key: (u32, usize),
    label: u32,
    ty: Ty,
    exit: u32,
    from: usize,
    registered: usize,
}

/// A suite's source rows, ascending, and its side rows.
struct Suite {
    rows: Vec<u32>,
    side: SideRows,
    copied: bool,
}

struct Copies<'a> {
    src: &'a Body,
    suites: HashMap<u32, Suite>,
    /// New rows as (anchor, first, end): each span goes before its
    /// anchor's row; `NONE` keeps it at the end.
    spans: Vec<(u32, u32, u32)>,
    /// Each point's cancel block, as (point, block).
    cancel: Vec<(u32, u32)>,
    /// The suites a `Defer` registers.
    deferred: hd_base::HashSet<u32>,
}

struct Map<'a> {
    rows: &'a [u32],
    base: u32,
    /// The source's label rows, and the copy's own label for each label
    /// a copied row is the target of (an inlined call's, in release).
    labels: &'a [u32],
    fresh: HashMap<u32, u32>,
    /// The suite's first copy keeps its labels, which then name it: the
    /// suite itself is deleted.
    first: bool,
}

impl Map<'_> {
    fn row(&self, w: u32) -> Option<u32> {
        self.rows
            .binary_search(&w)
            .ok()
            .map(|k| self.base + u32_of(k))
    }
}

impl Remap for Map<'_> {
    fn inst(&self, w: u32) -> Option<u32> {
        Some(self.row(w).unwrap_or(w))
    }

    fn label(&mut self, out: &mut Body, l: u32) -> Option<u32> {
        let Some(target) = self.labels.get(l as usize).and_then(|&t| self.row(t)) else {
            return Some(l);
        };
        if self.first {
            out.label_inst[l as usize] = target;
            return Some(l);
        }
        Some(*self.fresh.entry(l).or_insert_with(|| {
            out.label_inst.push(target);
            u32_of(out.label_inst.len() - 1)
        }))
    }
}

/// [`Map`] for side rows: only the copied rows have one.
struct Only<'a>(Map<'a>);

impl Remap for Only<'_> {
    fn inst(&self, w: u32) -> Option<u32> {
        self.0.row(w)
    }
}

impl Copies<'_> {
    /// Appends a copy of suite `s` and its wrapper `Scope` to `b`, before
    /// `anchor`: the copy's block and the wrapper, as a list holds them.
    fn copy(&mut self, b: &mut Body, s: u32, anchor: u32) -> Result<[u32; 2], String> {
        let src = self.src;
        let suite = self.suites.entry(s).or_insert_with(|| {
            let mut rows = Vec::new();
            inside(src, &[s], &mut Vec::new(), |i| {
                rows.push(u32_of(i));
                true
            });
            // A child block is listed and run by its owner: walked twice.
            rows.sort_stable();
            rows.dedup();
            let side = SideRows::group(src, 2, |i| usize::from(rows.binary_search(&i).is_ok()))
                .swap_remove(1);
            Suite {
                rows,
                side,
                copied: false,
            }
        });
        let base = u32_of(b.len());
        let len = u32_of(suite.rows.len());
        grow(b, (base + len + 1) as usize);
        let pairs: Vec<(u32, u32)> = (0..len)
            .map(|k| (suite.rows[k as usize], base + k))
            .collect();
        let mut map = Map {
            rows: &suite.rows,
            base,
            labels: &src.label_inst,
            fresh: HashMap::default(),
            first: !std::mem::replace(&mut suite.copied, true),
        };
        copy_rows(src, &pairs, b, &mut map)?;
        suite.side.copy(b, &mut Only(map))?;
        let blk = base + u32_of(suite.rows.binary_search(&s).unwrap_or_default());
        let wrap = base + len;
        let empty = b.push_record(&[]);
        b.tags[wrap as usize] = Tag::Scope;
        b.data[wrap as usize] = [blk, empty];
        b.syn[wrap as usize] = src.syn[s as usize];
        // A dispose suite; an inlined closure's environment reads are not.
        if self.deferred.contains(&s) {
            b.suites.push(wrap);
        }
        self.spans.push((anchor, base, wrap + 1));
        Ok([blk, wrap])
    }

    /// Copies, before `anchor`, of the suites `open[from..]` registered,
    /// innermost scope first and each scope's last registered first. Each
    /// point of a copy is shielded. On a way out that is not a cancellation
    /// (`exits`), it gets the cancel exit a scope takes once its suites ran
    /// in a cancelled task (flow.using.async.cancel.shielded): the rest of
    /// the suites, every outer scope's, then out as cancelled; on one, an
    /// empty block.
    fn run(
        &mut self,
        b: &mut Body,
        open: &[Open],
        from: usize,
        anchor: u32,
        exits: bool,
        list: &mut Vec<u32>,
    ) -> Result<(), String> {
        let order = |from: usize| -> Vec<u32> {
            let scopes = open.iter().skip(from).rev();
            scopes
                .flat_map(|o| o.suites[..o.registered].iter().rev().copied())
                .collect()
        };
        let (mine, outer) = (order(from), order(0));
        for (j, &s) in mine.iter().enumerate() {
            let rest = if exits { &outer[j + 1..] } else { &[][..] };
            self.suite(b, s, anchor, rest, exits, list)?;
        }
        Ok(())
    }

    /// A copy of suite `s` into `list`, before `anchor`; each of its
    /// points' cancel blocks runs copies of `rest`, then leaves when
    /// `leaves`.
    fn suite(
        &mut self,
        b: &mut Body,
        s: u32,
        anchor: u32,
        rest: &[u32],
        leaves: bool,
        list: &mut Vec<u32>,
    ) -> Result<(), String> {
        let [blk, wrap] = self.copy(b, s, anchor)?;
        list.extend([blk, wrap]);
        let start = self.spans.last().map_or(0, |s| s.1);
        let points: Vec<u32> = (start..wrap)
            .filter(|&i| is_suspension(b.tags[i as usize]))
            .collect();
        for p in points {
            let mut exit = Vec::new();
            for &r in rest {
                self.suite(b, r, NONE, &[], false, &mut exit)?;
            }
            let blk = self.leave(b, exit, leaves, NodeIdx::NONE);
            self.cancel.push((p, blk));
        }
        Ok(())
    }

    /// Appends instruction `tag` `data` before `anchor`.
    fn row(
        &mut self,
        b: &mut Body,
        anchor: u32,
        tag: Tag,
        data: [u32; 2],
        ty: Ty,
        syn: NodeIdx,
    ) -> u32 {
        let r = u32_of(b.len());
        grow(b, r as usize + 1);
        b.tags[r as usize] = tag;
        b.data[r as usize] = data;
        b.ty[r as usize] = ty;
        b.syn[r as usize] = syn;
        self.spans.push((anchor, r, r + 1));
        r
    }

    /// Scope `s`'s body `body` with its shared exit chains
    /// ([`Open::exits`]): each chain is the end of a labelled block its
    /// ways out break to, which runs the suites and takes the jump; the
    /// body's own end breaks past them all. Returns the new body.
    fn chains(&mut self, b: &mut Body, s: u32, body: u32, open: &[Open]) -> Result<u32, String> {
        let src = self.src;
        let Some(o) = open.last() else {
            return Ok(body);
        };
        let syn = src.syn[s as usize];
        b.label_inst.push(NONE);
        let out = u32_of(b.label_inst.len() - 1);
        let ty = src.ty[s as usize];
        let empty = b.push_record(&[]);
        let w = self.row(b, s, Tag::Scope, [body, empty], ty, syn);
        let past = self.row(b, s, Tag::Break, [out, w], Ty::NEVER, syn);
        let mut list = vec![body, w, past];
        for c in &o.exits {
            let rec = b.push_record(&list);
            let x = self.row(b, s, Tag::Block, [rec, NONE], c.ty, syn);
            b.label_inst[c.label as usize] = x;
            let empty = b.push_record(&[]);
            let w = self.row(b, s, Tag::Scope, [x, empty], c.ty, syn);
            list = vec![x, w];
            let mut at = open.to_vec();
            if let Some(last) = at.last_mut() {
                last.registered = c.registered;
            }
            self.run(b, &at, c.from, s, true, &mut list)?;
            let e = c.exit as usize;
            let mut data = src.data[e];
            match src.tags[e] {
                Tag::Return if data[0] != NONE => data[0] = w,
                Tag::Break if data[1] != NONE => data[1] = w,
                _ => {}
            }
            list.push(self.row(b, s, src.tags[e], data, src.ty[e], src.syn[e]));
        }
        let rec = b.push_record(&list);
        let outer = self.row(b, s, Tag::Block, [rec, NONE], ty, syn);
        b.label_inst[out as usize] = outer;
        Ok(outer)
    }

    /// Appends a cancel block of `list` and, when it leaves, a cancelled
    /// `Return`, at the body's end: no list holds it, its point runs it.
    fn leave(&mut self, b: &mut Body, mut list: Vec<u32>, leaves: bool, syn: NodeIdx) -> u32 {
        if leaves {
            let data = [NONE, return_word::CANCELLED];
            list.push(self.row(b, NONE, Tag::Return, data, Ty::NEVER, syn));
        }
        let rec = b.push_record(&list);
        let ty = if leaves { Ty::NEVER } else { Ty::VOID };
        self.row(b, NONE, Tag::Block, [rec, NONE], ty, NodeIdx::NONE)
    }
}

struct Frame {
    blk: u32,
    k: usize,
    list: Vec<u32>,
    changed: bool,
    /// The scope whose body this block is.
    scope: Option<u32>,
}

/// Lowers every cleanup scope of `b` (see the module comment).
///
/// # Errors
/// A suite's copy names a word outside the suite.
pub(crate) fn lower(b: &mut Body) -> Result<(), String> {
    if !(0..u32_of(b.len())).any(|i| is_suspension(b.tags[i as usize]) || cleanup(b, i)) {
        return Ok(());
    }
    let src = b.clone();
    let topo = Topology::new(&src);
    if b.anchors.is_empty() {
        b.anchors = (0..u32_of(b.len())).collect();
    }
    let n = src.len();
    let mut cx = Copies {
        src: &src,
        suites: HashMap::default(),
        spans: Vec::new(),
        cancel: Vec::new(),
        deferred: (0..n)
            .filter(|&i| src.tags[i] == Tag::Defer)
            .map(|i| src.data[i][0])
            .collect(),
    };
    // A suite runs only as its copies; a list may hold its block before
    // the walk reaches its scope.
    let mut gone = vec![false; n];
    let mut stack = Vec::new();
    for x in 0..n {
        if cleanup(&src, u32_of(x)) {
            inside(&src, src.record(src.data[x][1]), &mut stack, |r| {
                gone[r] = true;
                true
            });
        }
    }
    let mut depth = vec![0u32; n];
    for &root in &src.sub_root {
        if root == NONE {
            continue;
        }
        let mut open: Vec<Open> = Vec::new();
        let mut stack = vec![Frame {
            blk: root,
            k: 0,
            list: Vec::new(),
            changed: false,
            scope: None,
        }];
        while let Some(top) = stack.last_mut() {
            let items = src.record(src.data[top.blk as usize][0]);
            let Some(&x) = items.get(top.k) else {
                let Some(mut f) = stack.pop() else {
                    break;
                };
                if let Some(s) = f.scope {
                    // The body's own end, unless it leaves by its last item.
                    let ends = f.list.last().is_some_and(|&l| {
                        matches!(
                            b.tags[l as usize],
                            Tag::Break | Tag::Continue | Tag::Return | Tag::Unreachable
                        )
                    });
                    if !ends {
                        let at = open.len() - 1;
                        cx.run(b, &open, at, f.blk, true, &mut f.list)?;
                        f.changed = true;
                    }
                    if f.changed {
                        b.data[f.blk as usize][0] = b.push_record(&f.list);
                        f.changed = false;
                    }
                    if open.last().is_some_and(|o| !o.exits.is_empty()) {
                        let outer = cx.chains(b, s, f.blk, &open)?;
                        b.data[s as usize][0] = outer;
                        // A scope's body is listed beside it
                        // (`TirBuilder::close_block` in its open block).
                        let k = stack.last_mut().and_then(|p| {
                            let k = p.list.iter().position(|&m| m == f.blk)?;
                            p.list[k] = outer;
                            p.changed = true;
                            Some(k)
                        });
                        if k.is_none() {
                            return Err(format!("scope %{s}'s body is listed apart from it"));
                        }
                    }
                    open.pop();
                }
                if f.changed {
                    b.data[f.blk as usize][0] = b.push_record(&f.list);
                }
                continue;
            };
            top.k += 1;
            let i = x as usize;
            // A list holds a child block for its owner, which may stand
            // anywhere (a deferred `?` block): the walk enters a block
            // from its owner.
            if src.tags[i] == Tag::Block {
                if gone[i] {
                    top.changed = true;
                } else {
                    top.list.push(x);
                }
                continue;
            }
            match src.tags[i] {
                Tag::Defer => {
                    // ruled out: the checker emits each `Defer` as a
                    // statement of its cleanup scope's body (`body.rs`
                    // `lines`, `using.rs` `dispose_suite`).
                    let Some(o) = open.last_mut().filter(|o| o.body == top.blk) else {
                        return Err(format!("Defer %{x} stands outside its scope's body"));
                    };
                    o.registered += 1;
                    top.changed = true;
                    gone[i] = true;
                    continue;
                }
                tag @ (Tag::Break | Tag::Continue | Tag::Return) => {
                    let from = if tag == Tag::Return {
                        0
                    } else {
                        let target = src.label_inst[src.data[i][0] as usize];
                        depth[target as usize] as usize
                    };
                    // A break to the scope's own body (an inlined call's
                    // root) ends the body: its suites run before it.
                    let own = tag == Tag::Break
                        && open
                            .last()
                            .is_some_and(|o| o.body == src.label_inst[src.data[i][0] as usize]);
                    if own {
                        cx.run(b, &open, from, x, true, &mut top.list)?;
                        top.changed = true;
                    } else if open[from..].iter().any(|o| o.registered > 0)
                        && let Some(o) = open.last_mut()
                    {
                        // To the scope's shared chain for this target and
                        // this many registered suites, as rustc's drop tree
                        // shares a drop chain per target.
                        let target = match tag {
                            Tag::Return => NONE,
                            Tag::Continue => src.data[i][0] | 1 << 31,
                            _ => src.data[i][0],
                        };
                        let key = (target, o.registered);
                        let value = match tag {
                            Tag::Return => src.data[i][0],
                            Tag::Break => src.data[i][1],
                            _ => NONE,
                        };
                        let k = if let Some(k) = o.exits.iter().position(|c| c.key == key) {
                            k
                        } else {
                            b.label_inst.push(NONE);
                            let ty = match Ref(value).as_inst() {
                                _ if value == NONE => Ty::VOID,
                                Some(v) => src.ty[v.0 as usize],
                                None => src.consts[(value & !Ref::CONST_BIT) as usize].0,
                            };
                            o.exits.push(Chain {
                                key,
                                label: u32_of(b.label_inst.len() - 1),
                                ty,
                                exit: x,
                                from,
                                registered: o.registered,
                            });
                            o.exits.len() - 1
                        };
                        let label = o.exits[k].label;
                        let brk = cx.row(b, x, Tag::Break, [label, value], Ty::NEVER, src.syn[i]);
                        gone[i] = true;
                        top.list.push(brk);
                        top.changed = true;
                        continue;
                    }
                }
                tag if is_suspension(tag) => {
                    // The point's cancel exit: the suites registered around
                    // it, then out of the body as cancelled. Its block is
                    // numbered after the body, as what the point holds may
                    // be defined later in a loop (`known` releases it).
                    let mut list = Vec::new();
                    cx.run(b, &open, 0, NONE, false, &mut list)?;
                    let blk = cx.leave(b, list, true, src.syn[i]);
                    cx.cancel.push((x, blk));
                }
                _ => {}
            }
            top.list.push(x);
            let d = u32_of(open.len());
            depth[i] = d;
            let mut blocks = Vec::new();
            for_blocks(&src, i, |blk| blocks.push(blk));
            let scope = cleanup(&src, x);
            if scope {
                b.data[i][1] = b.push_record(&[]);
            }
            for &blk in blocks.iter().rev() {
                if gone[blk as usize] || topo.is_root[blk as usize] {
                    continue;
                }
                depth[blk as usize] = d;
                let body = scope && src.data[i][0] == blk;
                if body {
                    // A suite no `Defer` registers runs on every way out:
                    // an inlined closure's environment reads (`inline`).
                    let suites = src.record(src.data[i][1]).to_vec();
                    let registered = suites
                        .iter()
                        .take_while(|s| !cx.deferred.contains(*s))
                        .count();
                    open.push(Open {
                        body: blk,
                        suites,
                        registered,
                        exits: Vec::new(),
                    });
                }
                stack.push(Frame {
                    blk,
                    k: 0,
                    list: Vec::new(),
                    changed: false,
                    scope: body.then_some(x),
                });
            }
        }
    }
    // The copies' rows go before their anchors and the cancel blocks
    // after the body.
    b.cancel = std::mem::take(&mut cx.cancel);
    place(b, n, cx.spans, &gone)?;
    b.cancel.sort_stable();
    Ok(())
}

/// Moves each span of new rows (anchor, first, end) of `b`, whose first
/// `n` rows are old, before its anchor's row (`NONE`: to the end), drops
/// the old rows `gone` marks, and renumbers once: one permutation.
pub(crate) fn place(
    b: &mut Body,
    n: usize,
    mut spans: Vec<(u32, u32, u32)>,
    gone: &[bool],
) -> Result<(), String> {
    spans.sort_stable_by_key(|s| s.0);
    let mut order = Vec::with_capacity(b.len());
    let mut k = 0;
    for i in 0..=u32_of(n) {
        while k < spans.len() && spans[k].0 == i {
            order.extend(spans[k].1..spans[k].2);
            k += 1;
        }
        if i < u32_of(n) && !gone[i as usize] {
            order.push(i);
        }
    }
    for s in &spans[k..] {
        order.extend(s.1..s.2);
    }
    let mut new_of = vec![NONE; b.len()];
    for (k, &o) in order.iter().enumerate() {
        new_of[o as usize] = u32_of(k);
    }
    permute_columns(b, &order);
    let konst = 0;
    renumber(
        b,
        &mut Through {
            new_of: &new_of,
            konst,
        },
    )
}

/// Whether `i` is a cleanup scope: a `Scope` with suites.
fn cleanup(b: &Body, i: u32) -> bool {
    b.tags[i as usize] == Tag::Scope
        && b.data[i as usize][1] != NONE
        && !b.record(b.data[i as usize][1]).is_empty()
}
