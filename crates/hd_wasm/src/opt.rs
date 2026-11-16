//! A function's local cleanup before encoding (`crates/hd_wasm/DESIGN.md`
//! §23.22): emission gives every TIR value a fresh local, so most values
//! pass through a `local.set` and a `local.get` that follow each other.
//! Each rule below keeps what every path through the function computes:
//!
//! 1. `local.set t; local.get t`, where that get is `t`'s only read, is the
//!    value left on the stack;
//! 2. another `local.set t; local.get t` is `local.tee t`;
//! 3. a `local.set t` of a local no instruction reads is a `drop`, and a
//!    constant or `local.get` it drops goes too;
//! 4. a copy `P; local.set t`, where `P` pushes a local, a global or a
//!    constant and that set is `t`'s only write, moves to `t`'s only read
//!    when the read follows in the same straight-line run of
//!    instructions and nothing between writes what `P` reads;
//!    constant bulk widths may also enter empty blocks on that run;
//! 5. `if A else br N end`, with `N` past the `if` and no branch in `A`
//!    to the `if` itself, is `i32.eqz; br_if N-1; A`, and `if br N else A
//!    end` is `br_if N-1; A`, `A` empty when there is no `else`: the shape
//!    of a `while` loop's test;
//! 6. `if A; local.set x else B; local.set x end; local.get x`, where
//!    those are `x`'s only writes and that get its only read and neither
//!    arm branches to the `if` itself, is `if (result T) A else B end`;
//! 7. constants fold: a nonzero constant's zero check goes, two constants
//!    joined by `+`, `-` or `*` are one constant, adding 0 on either side
//!    goes, `+ a ± b` is one addition, `x == 0` is `i32.eqz`, a test or a
//!    zero check of `x != 0`, `x >u 0` or `!!x` is one of `x`, a
//!    multiplication, quotient or remainder by
//!    a power of two is a shift or a mask, and a constant added to a load's
//!    address is its offset (a store's is rule 8's);
//! 8. `i32.const c; i32.add; R; store`, where `R` is a straight-line run
//!    that pushes one value over what it found, is `R` and the store at
//!    `c` more offset;
//! 9. `X; local.set a; P; local.get a`, where those are `a`'s only write
//!    and read, `X` is a straight-line run that pushes one value and `P`
//!    pushes a local, a global or a constant that `X` cannot change, is
//!    `P; X`;
//! 10. a local no instruction names is not declared, and the others are
//!     numbered again in order;
//! 11. a release of zero, a redundant store of zero and an `if` with no
//!     `else` on a local of zero go when forward local facts prove them
//!     zero on every path, including loop backedges;
//! 12. a `local.set` or `local.tee` of a local that no path reads before
//!     its next write goes, a set's value dropped: the zero a move leaves
//!     in a local stays only where some release still reads it;
//! 13. in `local.set t; R; local.get t`, with that get the first access
//!     of `t` after the set and `R` a straight-line run (panic guards
//!     included), a pure push before the set moves to the get when `t`
//!     is dead after it; else, when `R` leaves the stack as it found it,
//!     the get goes, the value staying under `R`, and the set goes too,
//!     or becomes a tee when `t` is read again;
//! 14. once the rules are done ([`coalesce`]), locals of one type whose
//!     values are never live at once share one local, and a copy between
//!     two locals that share one goes;
//! 15. a `drop` takes back the instruction that pushed its value when
//!     that instruction has no other effect: a push without operands goes
//!     with it, a load or another operation that cannot trap becomes the
//!     drop of its operands, and `local.tee t; drop` is `local.set t`. A
//!     load cannot trap here: emitted code loads only from a place it
//!     holds or an index it checked. An `if` with nothing in it drops its
//!     test;
//! 16. a count whose every operand is a constant, pushed directly, held
//!     in a local that only constants are written to, or received by a
//!     closed-world function at only constant call sites, goes
//!     ([`static_values`], [`static_counts`]): a constant address is null
//!     or static data;
//! 17. `local.set r_k; ...; local.set r_1; br L`, where `L` is a block
//!     whose `end` only `local.get r_1; ...; local.get r_k` follows to the
//!     function's end, is `return`: the values the exit would read back
//!     are on the stack already ([`exits`]);
//! 18. a `br 0` straight before the `end` or `else` of a block or an `if`
//!     goes when the stack holds exactly the label's values there, or no
//!     more of them in unreachable code: it jumps where falling through
//!     goes.
//!     An `else` straight before the `end` of a void `if` goes, and a void
//!     block that loses its last branch this way goes too ([`blocks`]);
//! 19. a short-circuit value `if (result i32) X else i32.const K end`
//!     (`&&` for K = 0, `||` for K = 1) whose result only feeds a
//!     `br_if`, an `if`, or either after an `i32.eqz` is a chain of
//!     branches: no join of the two arms, and no test of the joined value
//!     ([`chains`]).
//!
//! Across functions, [`constants`] reads a top-level binding that only
//! its init writes, with a constant, as that constant.

mod bounds;
mod branch_shapes;
mod chains;

/// Select a checked or range-proven copy of an innermost counted loop.
pub(crate) fn version_loop_bounds(f: &mut Func) -> bool {
    bounds::run(f)
}
mod constant;
mod empty_loops;
mod live_set;

/// Delete finite induction loops whose counters are unobserved.
pub fn delete_empty_loops(f: &mut Func) -> bool {
    empty_loops::run(f)
}

/// Propagate literal local values after instruction scheduling exposes them.
pub fn fold_local_constants(f: &mut crate::module::Func) -> bool {
    constant::run(f)
}

use hd_base::SliceSort as _;
use live_set::{Set, Sets};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

use crate::op::{LabelListId, Op as I};
use hd_base::Hash128;

use crate::module::{Func, Global, Ins, Sym, SymId, Symbols};
use crate::origins::Remap;
use crate::rt::Helper;

/// Reads each top-level binding that one instruction writes, a constant
/// stored by an init, as that constant, and drops the store. An init runs
/// before any code that reads its bindings (`module.init.binding`), so every
/// read sees the constant.
pub fn constants(funcs: &mut [Option<Func>], symbols: &Symbols, inits: &[Hash128], arity: &Arity) {
    let mut inits = inits.to_vec();
    for (id, _) in funcs.iter().enumerate().filter(|(_, f)| f.is_some()) {
        match &*symbols.get(SymId(u32::try_from(id).expect("symbols"))) {
            Sym::Helper(Helper::EntryInit { inits: ks }) => inits.extend(ks.iter().map(|k| k.0)),
            Sym::Helper(Helper::TestInit { inits: ks }) => {
                inits.extend(ks.iter().flatten().map(|k| k.0));
            }
            Sym::Helper(Helper::EntryPoll { then, .. }) => inits.extend(then.iter().map(|k| k.0)),
            _ => {}
        }
    }
    // A value held across the old storage's release reaches its global
    // through a temporary. Expose those constants before looking for the
    // single initializing write, rather than only cleaning locals afterward.
    for (id, f) in funcs
        .iter_mut()
        .enumerate()
        .filter_map(|(id, f)| Some((id, f.as_mut()?)))
    {
        if matches!(&*symbols.get(SymId(u32::try_from(id).expect("symbols"))), Sym::Inst(key) if inits.contains(key))
        {
            locals(f, arity);
        }
    }
    let mut sets: BTreeMap<Global, (u32, Option<Write>)> = BTreeMap::new();
    for (id, f) in funcs
        .iter()
        .enumerate()
        .filter_map(|(i, f)| Some((i, f.as_ref()?)))
    {
        let id = SymId(u32::try_from(id).expect("symbols"));
        let init = matches!(&*symbols.get(id), Sym::Inst(key) if inits.contains(key));
        for (k, i) in f.body.iter().enumerate() {
            if let Ins::GlobalSet(g) = i {
                let global = symbols.global(*g);
                if !matches!(global, Global::Storage(..)) {
                    continue;
                }
                let e = sets.entry(global).or_insert((0, None));
                e.0 += 1;
                e.1 = if init {
                    producer(&f.body, k, 0, arity, symbols).and_then(|(p, result)| {
                        match f.body[p] {
                            c @ Ins::Op(
                                I::I32Const(_) | I::I64Const(_) | I::F32Const(_) | I::F64Const(_),
                            ) if result == 0
                                && !f.body[p + 1..k]
                                    .iter()
                                    .any(|ins| matches!(ins, Ins::Op(I::LocalTee(_)))) =>
                            {
                                Some((id, k, p, c))
                            }
                            _ => None,
                        }
                    })
                } else {
                    None
                };
            }
        }
    }
    let known = hd_base::btree_map(
        sets.into_iter()
            .filter_map(|(g, (n, at))| (n == 1).then_some(at).flatten().map(|at| (g, at))),
    );
    if known.is_empty() {
        return;
    }
    for f in funcs.iter_mut().flatten() {
        for i in &mut f.body {
            if let Ins::GlobalGet(g) = i
                && let Some((_, _, _, c)) = known.get(&symbols.global(*g))
            {
                *i = *c;
            }
        }
    }
    let mut drops = vec![Vec::new(); funcs.len()];
    for (id, store, producer, _) in known.into_values() {
        drops[id.idx()].extend([store, producer]);
    }
    for (id, ks) in drops.iter_mut().enumerate() {
        ks.sort_stable();
        if let Some(f) = &mut funcs[id] {
            for &k in ks.iter().rev() {
                f.remove(k);
            }
        }
    }
}

/// Whether `ins` only reads and writes locals, tests an `i32` or
/// branches forward: code made of these has no effect outside its frame.
fn local_only(ins: &Ins) -> bool {
    use wasm_encoder::BlockType;
    matches!(
        ins,
        Ins::Op(
            I::LocalGet(_)
                | I::LocalSet(_)
                | I::LocalTee(_)
                | I::Drop
                | I::If(BlockType::Empty)
                | I::Block(BlockType::Empty)
                | I::Else
                | I::End
                | I::Br(_)
                | I::BrIf(_)
                | I::Return
                | I::I32Const(_)
                | I::I32Eqz
                | I::I32Eq
                | I::I32Ne,
        ) | Ins::Anchor(_)
    )
}

/// Whether a call of `f` does nothing: it takes and gives no values, and
/// its body has no effect outside its frame ([`local_only`]). An init whose
/// constant stores [`constants`] took is one.
#[must_use]
pub fn does_nothing(f: &Func) -> bool {
    f.raw.is_none() && f.params.is_empty() && f.results.is_empty() && f.body.iter().all(local_only)
}

/// A binding's one write when an init stores a constant: the function,
/// the store and its scalar producer's indices, and the constant.
type Write = (SymId, usize, usize, Ins);

/// The parameter and result counts of each function a call can name.
pub type Arity = Vec<Option<(usize, usize)>>;

/// Closed-world parameters proved to receive only constants at every direct
/// call, and results proved constant under those parameter facts. A function
/// with an open-world entry, or called nowhere, has no parameter facts: an
/// unknown caller may supply any value. The least fixed point deliberately
/// leaves recursive-only evidence unproved.
///
/// Driven by what was proved, not by rounds over the module: every fact
/// only turns from unproved to proved, and a function's facts read only
/// its own parameter facts and its callees' result facts. Each call
/// argument's and each result's producer is found once, as the body is.
/// A function is looked at whole (its fixed locals, then each unproved
/// argument and result) first, and again when a parameter of it is
/// proved or a callee whose result a local write reads proves one. A
/// parameter is proved when the last call site that had not proved its
/// argument does (a count per callee and parameter). A callee's proved
/// result otherwise re-reads only the arguments and results it produces.
/// So a chain of `n` calls that proves one level per step reads each
/// function a constant number of times, not `n` rounds of every function
/// (#1912). Cost: each body once per look, plus one check per argument or
/// result a proved result produces.
pub fn static_values(
    funcs: &[Option<Func>],
    open: impl IntoIterator<Item = SymId>,
    arity: &Arity,
) -> Vec<Vec<bool>> {
    let mut unknown = vec![false; funcs.len()];
    for id in open {
        unknown[id.idx()] = true;
    }
    for f in funcs.iter().flatten() {
        for ins in &f.body {
            if let Ins::TableSlot(id) | Ins::Glue(id) = ins {
                unknown[id.idx()] = true;
            }
        }
    }
    let mut params: Vec<Vec<bool>> = funcs
        .iter()
        .map(|f| {
            f.as_ref()
                .map_or_else(Vec::new, |f| vec![false; f.params.len()])
        })
        .collect();
    let mut results: Vec<Vec<bool>> = funcs
        .iter()
        .map(|f| {
            f.as_ref()
                .map_or_else(Vec::new, |f| vec![false; f.results.len()])
        })
        .collect();
    let joins: Vec<_> = funcs
        .iter()
        .map(|f| {
            f.as_ref()
                .map_or_else(Vec::new, |f| constant_joins(&f.body))
        })
        .collect();
    let mut facts: Vec<Facts> = funcs
        .iter()
        .zip(&joins)
        .map(|(f, joins)| {
            f.as_ref()
                .map_or_else(Facts::default, |f| Facts::new(f, arity, joins))
        })
        .collect();
    let mut waiting: Vec<Vec<usize>> = params.iter().map(|p| vec![0; p.len()]).collect();
    let mut callers: Vec<Vec<usize>> = vec![Vec::new(); funcs.len()];
    let mut called = vec![false; funcs.len()];
    for (c, cf) in facts.iter_mut().enumerate() {
        for site in &mut cf.sites {
            let g = site.callee;
            called[g] = true;
            if callers[g].last() != Some(&c) {
                callers[g].push(c);
            }
            site.args.truncate(params[g].len());
            for w in &mut waiting[g][..site.args.len()] {
                *w += 1;
            }
        }
    }
    // A parameter no call site supplies is proved by every site at once.
    for g in 0..funcs.len() {
        if unknown[g] || !called[g] {
            continue;
        }
        for (proved, &w) in params[g].iter_mut().zip(&waiting[g]) {
            *proved |= w == 0;
        }
    }
    let mut looks = Queue {
        order: std::collections::VecDeque::new(),
        queued: vec![false; funcs.len()],
    };
    let mut grown: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    for c in 0..funcs.len() {
        looks.push(c);
    }
    loop {
        // What a look or a check proved: arguments, by callee and
        // parameter, and the functions a result of which grew.
        let mut proved: Vec<(usize, usize)> = Vec::new();
        if let Some(c) = looks.pop() {
            let Some(f) = &funcs[c] else { continue };
            crate::work::static_looks(f.body.len());
            let cf = &mut facts[c];
            cf.fixed = fixed_locals(f, &params[c], &results, arity, &joins[c]);
            for site in &mut cf.sites {
                for (k, arg) in site.args.iter_mut().enumerate() {
                    if !arg.proved && arg.holds(f, &cf.fixed, &results) {
                        arg.proved = true;
                        proved.push((site.callee, k));
                    }
                }
            }
            if prove_ends(f, cf, c, None, &mut results) {
                grown.push_back(c);
            }
        } else if let Some(g) = grown.pop_front() {
            for &c in &callers[g] {
                let (Some(f), cf) = (&funcs[c], &mut facts[c]) else {
                    continue;
                };
                if cf.writes_read.contains(&g) {
                    looks.push(c);
                    continue;
                }
                crate::work::static_looks(1);
                for &(s, k) in cf.produced_by.get(&g).map_or(&[][..], Vec::as_slice) {
                    let site = &mut cf.sites[s];
                    let arg = &mut site.args[k];
                    if !arg.proved && arg.holds(f, &cf.fixed, &results) {
                        arg.proved = true;
                        proved.push((site.callee, k));
                    }
                }
                if prove_ends(f, cf, c, Some(g), &mut results) {
                    grown.push_back(c);
                }
            }
        } else {
            break;
        }
        for (g, k) in proved {
            waiting[g][k] -= 1;
            if waiting[g][k] == 0 && !unknown[g] && called[g] && !params[g][k] {
                params[g][k] = true;
                looks.push(g);
            }
        }
    }
    params
}

/// Proves each unproved result of function `c` whose producer holds, or
/// only those `from` produces when given; gives whether one was.
fn prove_ends(
    f: &Func,
    cf: &Facts,
    c: usize,
    from: Option<usize>,
    results: &mut [Vec<bool>],
) -> bool {
    let ends: Vec<usize> = cf
        .ends
        .iter()
        .enumerate()
        .filter(|&(k, end)| {
            !results[c][k]
                && from.is_none_or(|g| end.from == Some(g))
                && end.holds(f, &cf.fixed, results)
        })
        .map(|(k, _)| k)
        .collect();
    for &k in &ends {
        results[c][k] = true;
    }
    !ends.is_empty()
}

/// One function's facts for [`static_values`]: its direct call sites,
/// the producers of its results, which callees a local write may read,
/// and its fixed locals as of its last look.
#[derive(Default)]
struct Facts {
    sites: Vec<Site>,
    /// The producer of each result at the body's end, when the body is
    /// one straight run: a branch, loop, `if` or early return may answer a
    /// different value, so those results stay unproved until this analysis
    /// carries facts over every control-flow edge.
    ends: Vec<Produced>,
    /// The callees whose results a local write may read: those that
    /// produce a write's value, or every callee when a write may read an
    /// `if`'s arms. A proved result of one looks at the function whole.
    writes_read: hd_base::HashSet<usize>,
    /// Each argument a callee's result produces, as (site, argument).
    produced_by: hd_base::HashMap<usize, Vec<(usize, usize)>>,
    fixed: Vec<bool>,
}

impl Facts {
    fn new(f: &Func, arity: &Arity, joins: &[Option<usize>]) -> Self {
        let produced = |at: usize, depth: usize| {
            let at = producer(&f.body, at, depth, arity, &f.symbols);
            let from = at.and_then(|(p, _)| match f.body[p] {
                Ins::Call(id) => Some(id.idx()),
                _ => None,
            });
            Produced {
                at,
                from,
                proved: false,
            }
        };
        let mut facts = Facts::default();
        for (at, ins) in f.body.iter().enumerate() {
            if let Ins::Op(I::LocalSet(_) | I::LocalTee(_)) = ins {
                facts.writes_read.extend(produced(at, 0).from);
                continue;
            }
            let Ins::Call(id) = ins else { continue };
            if !joins.is_empty() {
                facts.writes_read.insert(id.idx());
            }
            let Some((n, _)) = arity.get(id.idx()).copied().flatten() else {
                continue;
            };
            let s = facts.sites.len();
            let args: Vec<Produced> = (0..n).map(|k| produced(at, n - 1 - k)).collect();
            for (k, arg) in args.iter().enumerate() {
                if let Some(g) = arg.from {
                    facts.produced_by.entry(g).or_default().push((s, k));
                }
            }
            facts.sites.push(Site {
                callee: id.idx(),
                args,
            });
        }
        let straight = !f.body.iter().any(|ins| {
            matches!(
                ins,
                Ins::Op(
                    I::Loop(_)
                        | I::If(_)
                        | I::Else
                        | I::Br(_)
                        | I::BrIf(_)
                        | I::BrTable(..)
                        | I::Return
                        | I::Unreachable
                )
            )
        });
        if straight {
            let n = f.results.len();
            facts.ends = (0..n).map(|k| produced(f.body.len(), n - 1 - k)).collect();
        }
        facts
    }
}

/// A direct call site of [`static_values`]: the callee, and each
/// argument the callee has a parameter for.
struct Site {
    callee: usize,
    args: Vec<Produced>,
}

/// A value [`static_values`] proves: its producer and the result of it,
/// the callee when a call produces it, and whether it is proved.
struct Produced {
    at: Option<(usize, usize)>,
    from: Option<usize>,
    proved: bool,
}

impl Produced {
    fn holds(&self, f: &Func, fixed: &[bool], results: &[Vec<bool>]) -> bool {
        self.at
            .is_some_and(|(p, result)| constant_at(&f.body[p], result, fixed, results))
    }
}

/// [`static_values`]'s functions to look at again, each queued at most
/// once at a time.
struct Queue {
    order: std::collections::VecDeque<usize>,
    queued: Vec<bool>,
}

impl Queue {
    fn push(&mut self, id: usize) {
        if !self.queued[id] {
            self.queued[id] = true;
            self.order.push_back(id);
        }
    }

    fn pop(&mut self) -> Option<usize> {
        let id = self.order.pop_front()?;
        self.queued[id] = false;
        Some(id)
    }
}

/// Cleans up `f`'s locals by the rules of the module documentation.
///
/// Two sweeps of the nine rule groups, independent of body width and
/// nesting depth. Zero and liveness cleanup run in each sweep, exposing
/// their results to the next one without waiting for a global fixpoint.
/// The rewrites are optional: any remaining pattern is valid input code.
/// [`Settled`] still skips a group that has already seen the current body.
/// This bounds group scheduling; each group's own walks are unchanged.
pub fn locals(f: &mut Func, arity: &Arity) {
    locals_with_work(f, arity, &mut |_| {});
}

fn locals_with_work(f: &mut Func, arity: &Arity, visit_round: &mut impl FnMut(usize)) {
    let np = u32::try_from(f.params.len()).expect("params");
    let mut settled = Settled::new();
    for _ in 0..2 {
        visit_round(f.body.len());
        let mut changed = settled.run(f, Rule::Pairs, |f| pairs(f, np));
        changed |= settled.run(f, Rule::Forward, |f| forward(f, np));
        changed |= settled.run(f, Rule::Shapes, |f| shapes(f, np, arity));
        changed |= settled.run(f, Rule::Exits, exits);
        changed |= settled.run(f, Rule::Fold, fold);
        changed |= settled.run(f, Rule::Drops, drops);
        changed |= settled.run(f, Rule::Schedule, |f| schedule(f, np, arity));
        changed |= settled.run(f, Rule::Zero, zero_releases);
        changed |= settled.run(f, Rule::Live, |f| live_rules(f, arity));
        if !changed {
            break;
        }
    }
    renumber(f);
}

/// The rule groups of [`locals`], each one call of its pass.
#[derive(Clone, Copy)]
enum Rule {
    Pairs,
    Forward,
    Shapes,
    Exits,
    Fold,
    Drops,
    Schedule,
    Zero,
    Live,
}

/// Which of [`locals`]' rules have seen the function as it is now.
struct Settled {
    /// How many times a rule has edited the function.
    edits: usize,
    /// `clean[rule]`: the edit count at which that rule last found
    /// nothing to do, or `usize::MAX`.
    clean: [usize; 9],
}

impl Settled {
    fn new() -> Self {
        Self {
            edits: 0,
            clean: [usize::MAX; 9],
        }
    }

    /// Runs `rule`'s pass unless it has found nothing in this very
    /// function before, and gives whether it edited the function. A pass
    /// that repeats itself until it finds nothing ([`shapes`],
    /// [`live_rules`]) has seen its own result, so it is settled after an
    /// edit too.
    fn run(&mut self, f: &mut Func, rule: Rule, pass: impl FnOnce(&mut Func) -> bool) -> bool {
        let k = rule as usize;
        if self.clean[k] == self.edits {
            return false;
        }
        let changed = pass(f);
        if changed {
            self.edits += 1;
        }
        if !changed || matches!(rule, Rule::Shapes | Rule::Live) {
            self.clean[k] = self.edits;
        }
        changed
    }
}

/// Rules 1 to 3: a `local.set` and a `local.get` of one local that follow
/// each other, and writes of a local no instruction reads.
fn pairs(f: &mut Func, np: u32) -> bool {
    let (reads, _) = counts(f);
    let mut out: Vec<Ins> = Vec::with_capacity(f.body.len());
    let mut origins = Remap::new(f.origins.as_deref());
    let mut changed = false;
    let mut k = 0;
    while k < f.body.len() {
        let next = f.body.get(k + 1);
        match (&f.body[k], next) {
            (Ins::Op(I::LocalSet(t)), Some(Ins::Op(I::LocalGet(u)))) if t == u => {
                let t = *t as usize;
                changed = true;
                k += 2;
                // The value stays on the stack when this get is `t`'s
                // only read, whatever else writes `t`. Removing a pair
                // can expose another (`set b; set a; get a; get b`), so
                // a nest of them unwinds here, not one pass per layer.
                if *u >= np && reads[t] == 1 {
                    while let (Some(Ins::Op(I::LocalSet(a))), Some(Ins::Op(I::LocalGet(b)))) =
                        (out.last(), f.body.get(k))
                    {
                        if a != b || *b < np || reads[*a as usize] != 1 {
                            break;
                        }
                        out.pop();
                        origins.pop();
                        k += 1;
                    }
                } else {
                    out.push(Ins::Op(I::LocalTee(*u)));
                    origins.merged(k - 2..k, 1);
                }
            }
            (Ins::Op(I::LocalSet(t)), _) if reads[*t as usize] == 0 => {
                changed = true;
                if let Some(Ins::Op(
                    I::LocalGet(_)
                    | I::I32Const(_)
                    | I::I64Const(_)
                    | I::F32Const(_)
                    | I::F64Const(_),
                )) = out.last()
                {
                    out.pop();
                    origins.pop();
                } else {
                    out.push(Ins::Op(I::Drop));
                    origins.copy(k..k + 1);
                }
                k += 1;
            }
            (Ins::Op(I::LocalTee(t)), _) if reads[*t as usize] == 0 => {
                changed = true;
                k += 1;
            }
            (other, _) => {
                out.push(*other);
                origins.copy(k..k + 1);
                k += 1;
            }
        }
    }
    f.body = out;
    origins.finish(f);
    changed
}

/// Local zero facts at structured control-flow edges. Branches meet by
/// intersection; loop backedges participate in the same fixed point.
enum ZeroEdges {
    Next,
    Exit,
    Jump(usize),
    Either(usize, usize),
    Table(Vec<usize>),
}

impl ZeroEdges {
    fn each(&self, next: usize, mut visit: impl FnMut(usize)) {
        match self {
            Self::Next => visit(next),
            Self::Exit => {}
            Self::Jump(to) => visit(*to),
            Self::Either(a, b) => {
                visit(*a);
                visit(*b);
            }
            Self::Table(targets) => targets.iter().copied().for_each(visit),
        }
    }
}

/// The CFG of the closed set of control instructions the backend emits.
/// Its compact IR has no exception, reference-branch or tail-call opcodes.
fn zero_edges(body: &[Ins], label_lists: &[Vec<u32>]) -> Option<Vec<ZeroEdges>> {
    let mut ends = vec![body.len(); body.len()];
    let mut other = vec![None; body.len()];
    let mut labels = Vec::new();
    for (at, ins) in body.iter().enumerate() {
        match ins {
            Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => labels.push(at),
            Ins::Op(I::Else) => other[*labels.last()?] = Some(at),
            Ins::Op(I::End) => {
                if let Some(open) = labels.pop() {
                    ends[open] = at;
                }
            }
            _ => {}
        }
    }
    if !labels.is_empty() {
        return None;
    }
    let mut edges = Vec::with_capacity(body.len());
    for (at, ins) in body.iter().enumerate() {
        let target = |depth: u32, labels: &[usize]| {
            let Some(pos) = labels.len().checked_sub(depth as usize + 1) else {
                return body.len(); // The implicit function label.
            };
            let open = labels[pos];
            if matches!(body[open], Ins::Op(I::Loop(_))) {
                open + 1
            } else {
                ends[open] + 1
            }
        };
        let edge = match ins {
            Ins::Op(I::Block(_) | I::Loop(_)) => {
                labels.push(at);
                ZeroEdges::Next
            }
            Ins::Op(I::If(_)) => {
                labels.push(at);
                ZeroEdges::Either(at + 1, other[at].map_or(ends[at] + 1, |e| e + 1))
            }
            Ins::Op(I::Else) => ZeroEdges::Jump(ends[*labels.last()?] + 1),
            Ins::Op(I::End) => {
                labels.pop();
                ZeroEdges::Next
            }
            Ins::Op(I::Br(depth)) => ZeroEdges::Jump(target(*depth, &labels)),
            Ins::Op(I::BrIf(depth)) => ZeroEdges::Either(at + 1, target(*depth, &labels)),
            Ins::Op(I::BrTable(table, default)) => ZeroEdges::Table(
                label_lists[table.0 as usize]
                    .iter()
                    .chain([default])
                    .map(|d| target(*d, &labels))
                    .collect(),
            ),
            Ins::Op(I::Return | I::Unreachable) => ZeroEdges::Exit,
            _ => ZeroEdges::Next,
        };
        edges.push(edge);
    }
    Some(edges)
}

/// Rule 11's facts: the locals known to hold zero, as persistent sets
/// ([`live_set`]) at block entries, so handing a block's facts to a
/// successor copies a handle and a meet costs the locals where two paths
/// differ. The sets are not hash-consed ([`Sets::unconsed`]): the only
/// use of one node per content was the fixed point's change test, and a
/// meet's size answers it. Inside a block the facts are its entry set and a dense row of
/// what its instructions wrote since, so a write costs one store, not a
/// path of `log2 n` hash-consed nodes; [`Zeros::leave`] makes the block's
/// exit set once, from the locals whose fact changed, and only for a block
/// with a successor.
struct Zeros {
    sets: Sets,
    /// The entry facts of the block being carried.
    base: Set,
    /// Per local, what the block's writes made it since `base`: 0 none,
    /// 1 known zero, 2 not known zero.
    wrote: Vec<u8>,
    /// The locals `wrote` marks, to reset and to apply.
    touched: Vec<u32>,
}

impl Zeros {
    fn new(locals: usize) -> Self {
        Zeros {
            sets: Sets::unconsed(),
            base: Set::EMPTY,
            wrote: vec![0; locals],
            touched: Vec::new(),
        }
    }

    /// Starts carrying a block whose entry facts are `known`.
    fn enter(&mut self, known: Set) {
        for &l in &self.touched {
            self.wrote[l as usize] = 0;
        }
        self.touched.clear();
        self.base = known;
    }

    /// The carried block's facts so far, as a set: its entry set with each
    /// changed local inserted or removed, a path each.
    fn leave(&mut self) -> Set {
        let mut known = self.base;
        for &l in &self.touched {
            known = if self.wrote[l as usize] == 1 {
                self.sets.insert(known, l)
            } else {
                self.sets.remove(known, l)
            };
        }
        known
    }

    fn has(&self, local: u32) -> bool {
        match self.wrote.get(local as usize) {
            Some(1) => true,
            Some(2) => false,
            _ => self.sets.contains(self.base, local),
        }
    }

    /// A pure push of zero bits; every other expression remains unknown.
    fn pushes(&self, ins: &Ins) -> bool {
        match ins {
            Ins::Op(I::LocalGet(l)) => self.has(*l),
            other => pushes_zero_constant(other),
        }
    }

    /// Carries the facts over one original instruction. A store's source
    /// is its physical predecessor, including at a control-flow target.
    fn after(&mut self, body: &[Ins], at: usize) {
        let Ins::Op(I::LocalSet(l) | I::LocalTee(l)) = &body[at] else {
            return;
        };
        let zero = at.checked_sub(1).is_some_and(|p| {
            self.pushes(&body[p]) || matches!(body[p], Ins::Op(I::LocalTee(t)) if self.has(t))
        });
        let Some(slot) = self.wrote.get_mut(*l as usize) else {
            return;
        };
        if *slot == 0 {
            self.touched.push(*l);
        }
        *slot = if zero { 1 } else { 2 };
    }
}

impl Drop for Zeros {
    fn drop(&mut self) {
        crate::work::zero_facts(self.sets.work());
    }
}

/// A constant push of zero bits.
fn pushes_zero_constant(ins: &Ins) -> bool {
    match ins {
        Ins::Op(I::I32Const(0) | I::I64Const(0)) => true,
        Ins::Op(I::F32Const(v)) => v.bits() == 0,
        Ins::Op(I::F64Const(v)) => v.bits() == 0,
        _ => false,
    }
}

/// Deletes only release arguments/calls and stores proved redundant on
/// every incoming path. Facts are persistent sets ([`Zeros`]) kept only at
/// basic-block entries; one working set carries facts through each block.
fn zero_releases(f: &mut Func) -> bool {
    zero_releases_with(f, &mut || {})
}

/// [`zero_releases`], calling `visit` at each block its fixed point
/// visits.
fn zero_releases_with(f: &mut Func, visit: &mut dyn FnMut()) -> bool {
    let symbols = f.symbols.clone();
    let release_width = |ins: &Ins| {
        let Ins::Call(id) = ins else {
            return None;
        };
        match &*symbols.get(*id) {
            Sym::Helper(Helper::ReleaseRef | Helper::ReleaseKnown) => Some(1),
            Sym::Helper(Helper::Release { vals, .. }) => Some(vals.len()),
            _ => None,
        }
    };
    // A store of zero is redundant in a local known zero, with or without a
    // release in the body.
    let stores_zero = |w: &[Ins]| {
        matches!(w[1], Ins::Op(I::LocalSet(_) | I::LocalTee(_))) && pushes_zero_constant(&w[0])
    };
    if !f.body.iter().any(|i| release_width(i).is_some()) && !f.body.windows(2).any(stores_zero) {
        return false;
    }
    let Some(edges) = zero_edges(&f.body, &f.labels) else {
        return false;
    };
    if f.body.is_empty() {
        return false;
    }
    let (point_at, starts) = basic_blocks(&edges, f.body.len());
    let n = f.params.len() + f.locals.len();
    let mut zeros = Zeros::new(n);
    // Every local but a parameter starts zero: one set made in one pass.
    // ruled out: a local index past u32; Wasm counts locals in u32.
    let all: Vec<u32> = (f.params.len()..n)
        .map(|l| u32::try_from(l).expect("locals"))
        .collect();
    let known = zeros.sets.of_sorted(&all);
    let gone = if forward_only(&edges, &point_at, &starts, f.body.len()) {
        zero_sweep(
            f,
            &edges,
            &point_at,
            &starts,
            (&mut zeros, known),
            &release_width,
        )
    } else {
        zero_fixpoint(
            f,
            &edges,
            &point_at,
            &starts,
            (&mut zeros, known),
            &release_width,
            visit,
        )
    };
    if !gone.iter().any(|g| *g) {
        return false;
    }
    let mut origins = Remap::new(f.origins.as_deref());
    f.body = std::mem::take(&mut f.body)
        .into_iter()
        .enumerate()
        .filter_map(|(at, ins)| {
            if gone[at] {
                None
            } else {
                origins.copy(at..at + 1);
                Some(ins)
            }
        })
        .collect();
    origins.finish(f);
    true
}

/// Whether every edge of a body's basic blocks leads to a later block, so
/// that blocks in order see each predecessor first.
fn forward_only(edges: &[ZeroEdges], point_at: &[usize], starts: &[usize], len: usize) -> bool {
    let mut forward = true;
    for p in 0..starts.len() {
        let end = starts.get(p + 1).copied().unwrap_or(len);
        edges[end - 1].each(end, |to| {
            if point_at.get(to).is_some_and(|&next| next <= p) {
                forward = false;
            }
        });
    }
    forward
}

/// [`zero_releases`]' decisions for a body whose blocks only branch
/// forward: one pass in block order meets each block's facts from its
/// predecessors, decides, and drops them. A block's facts are a
/// persistent set, so handing them to a successor copies nothing, and a
/// meet costs where the two differ. It decides as [`zero_fixpoint`] does.
fn zero_sweep(
    f: &Func,
    edges: &[ZeroEdges],
    point_at: &[usize],
    starts: &[usize],
    (zeros, entry): (&mut Zeros, Set),
    release_width: &dyn Fn(&Ins) -> Option<usize>,
) -> Vec<bool> {
    let mut waiting: Vec<Option<Set>> = vec![None; starts.len()];
    waiting[0] = Some(entry);
    let mut gone = vec![false; f.body.len()];
    for point in 0..starts.len() {
        let Some(known) = waiting[point].take() else {
            continue;
        };
        let end = starts.get(point + 1).copied().unwrap_or(f.body.len());
        decide_block(
            f,
            starts[point]..end,
            (zeros, known),
            &mut gone,
            release_width,
        );
        // The exit set is made on the first successor, if there is one.
        let mut out = None;
        edges[end - 1].each(end, |to| {
            let Some(&next) = point_at.get(to) else {
                return;
            };
            let known = *out.get_or_insert_with(|| zeros.leave());
            let slot = &mut waiting[next];
            *slot = Some(slot.map_or(known, |before| zeros.sets.intersection(before, known)));
        });
    }
    gone
}

/// [`zero_releases`]' decisions for any body: the facts at each block's
/// entry to a fixed point, then the decisions.
///
/// The worklist hands out the lowest queued block first, as clang's
/// `ForwardDataflowWorklist` hands out blocks in reverse post-order.
/// Blocks are numbered in body order, where every edge but a loop's
/// backedge leads forward, so a loop settles before the blocks after it
/// are visited, and each of those is visited once, not once more per
/// round of the loops before it: `O(V log B)` heap steps for `V` visits.
fn zero_fixpoint(
    f: &Func,
    edges: &[ZeroEdges],
    point_at: &[usize],
    starts: &[usize],
    (zeros, entry): (&mut Zeros, Set),
    release_width: &dyn Fn(&Ins) -> Option<usize>,
    visit: &mut dyn FnMut(),
) -> Vec<bool> {
    // `None`: not reached yet. A function without locals has only empty
    // sets but can still have removable releases.
    let mut facts: Vec<Option<Set>> = vec![None; starts.len()];
    facts[0] = Some(entry);
    let mut pending = std::collections::BinaryHeap::from([std::cmp::Reverse(0)]);
    let mut queued = vec![false; starts.len()];
    queued[0] = true;
    while let Some(std::cmp::Reverse(point)) = pending.pop() {
        visit();
        queued[point] = false;
        let Some(known) = facts[point] else {
            continue;
        };
        let end = starts.get(point + 1).copied().unwrap_or(f.body.len());
        zeros.enter(known);
        for at in starts[point]..end {
            zeros.after(&f.body, at);
        }
        let mut out = None;
        edges[end - 1].each(end, |to| {
            let Some(&next) = point_at.get(to) else {
                return;
            };
            debug_assert_ne!(next, usize::MAX, "a block successor begins a block");
            let known = *out.get_or_insert_with(|| zeros.leave());
            // A meet keeps a subset of what it meets, so it changed the
            // facts exactly when it dropped a local: one size comparison,
            // with no hash-consing to make equal sets one node.
            let (meet, changed) = match facts[next] {
                None => (known, true),
                Some(before) => {
                    let meet = zeros.sets.intersection(before, known);
                    (meet, zeros.sets.len(meet) != zeros.sets.len(before))
                }
            };
            if changed {
                facts[next] = Some(meet);
                if !queued[next] {
                    pending.push(std::cmp::Reverse(next));
                    queued[next] = true;
                }
            }
        });
    }
    let mut gone = vec![false; f.body.len()];
    for (point, &start) in starts.iter().enumerate() {
        let Some(known) = facts[point] else {
            continue;
        };
        let end = starts.get(point + 1).copied().unwrap_or(f.body.len());
        decide_block(f, start..end, (zeros, known), &mut gone, release_width);
    }
    gone
}

/// Marks in `gone` the releases and stores of `block` that its entry
/// facts `known` prove redundant, carrying the facts through it
/// ([`Zeros::leave`] makes its exit set).
fn decide_block(
    f: &Func,
    block: std::ops::Range<usize>,
    (zeros, known): (&mut Zeros, Set),
    gone: &mut [bool],
    release_width: &dyn Fn(&Ins) -> Option<usize>,
) {
    zeros.enter(known);
    for at in block {
        let ins = &f.body[at];
        if let Some(n) = release_width(ins) {
            let mut args = Vec::new();
            for p in (0..at).rev() {
                if matches!(f.body[p], Ins::Anchor(_)) {
                    continue;
                }
                if args.len() == n || !zeros.pushes(&f.body[p]) {
                    break;
                }
                args.push(p);
            }
            if args.len() == n {
                gone[at] = true;
                for p in args {
                    gone[p] = true;
                }
            }
        } else if let Ins::Op(I::If(wasm_encoder::BlockType::Empty)) = ins
            && let Some(p) = (0..at)
                .rev()
                .find(|&p| !matches!(f.body[p], Ins::Anchor(_)))
            && matches!(f.body[p], Ins::Op(I::LocalGet(_)))
            && zeros.pushes(&f.body[p])
            && let Some(end) = block_end(&f.body, at)
        {
            // A guard on a local proved zero never runs its arm: a frame
            // slot's release where the slot holds nothing.
            gone[p] = true;
            gone[at..=end].fill(true);
        } else if let Ins::Op(I::LocalSet(l) | I::LocalTee(l)) = ins
            && zeros.has(*l)
            && at.checked_sub(1).is_some_and(|p| zeros.pushes(&f.body[p]))
        {
            gone[at] = true;
            if matches!(ins, Ins::Op(I::LocalSet(_))) {
                gone[at - 1] = true;
            }
        }
        // Replay original transfers after deciding from the incoming
        // facts; pending removals must not affect later decisions.
        zeros.after(&f.body, at);
    }
}

/// The basic blocks of a body whose control edges are `edges`: each
/// instruction's block, and each block's first instruction. A
/// terminator ends its block and every branch target starts one.
fn basic_blocks(edges: &[ZeroEdges], len: usize) -> (Vec<usize>, Vec<usize>) {
    let mut point_at = vec![usize::MAX; len];
    if let Some(first) = point_at.first_mut() {
        *first = 0;
    }
    for (at, edge) in edges.iter().enumerate() {
        if !matches!(edge, ZeroEdges::Next) {
            if let Some(next) = point_at.get_mut(at + 1) {
                *next = 0;
            }
            edge.each(at + 1, |to| {
                if let Some(target) = point_at.get_mut(to) {
                    *target = 0;
                }
            });
        }
    }
    let mut starts = Vec::new();
    for (at, point) in point_at.iter_mut().enumerate() {
        if *point == 0 {
            *point = starts.len();
            starts.push(at);
        } else {
            *point = starts.len() - 1;
        }
    }
    (point_at, starts)
}

/// The locals live into each basic block of a body, by a backward pass
/// to a fixed point (loop backedges included). Each block's set is a
/// persistent set ([`live_set`]) that shares every subtree its block
/// leaves alone with the sets it was built from: `n` locals live across
/// `n` blocks take about `n log n` nodes, not `n²` entries.
///
/// A block is summarised once, as the textbook `UEVar` and `VarKill` sets
/// are (Cooper and Torczon, "Engineering a Compiler", 8.6.1): the
/// first access of each local it touches, a read putting the local in
/// and a write taking it out. A visit then costs one path per local the
/// block touches, not one per access, and the fixed point never walks
/// the instructions again. Dead accesses come from each block's live-out
/// set and the next access in the block, as LLVM's `LiveVariables`
/// marks kills within a block.
struct Live {
    edges: Vec<ZeroEdges>,
    point_at: Vec<usize>,
    starts: Vec<usize>,
    /// Every edge leads to a later block, so one backward pass settles.
    forward: bool,
    sets: Sets,
    into: Vec<Set>,
    /// What is live out of each block, as its last visit found it: at the
    /// fixed point, the union of its successors' `into`.
    outs: Vec<Set>,
    /// The first access of each local block `p` touches, in
    /// `firsts[first[p]..first[p + 1]]`: the local, and whether it reads.
    first: Vec<usize>,
    firsts: Vec<(u32, bool)>,
}

impl Drop for Live {
    fn drop(&mut self) {
        crate::work::liveness(self.starts.len() + self.firsts.len() + self.sets.work());
    }
}

impl Live {
    fn new(f: &Func, sets: Sets) -> Option<Self> {
        let mut me = Self::blocks(f, sets)?;
        me.settle(f);
        Some(me)
    }

    /// Runs the backward pass to its fixed point. Blocks go last to
    /// first, so in a body that only branches forward each block's
    /// successors are final when it is visited, and one pass settles. A
    /// backedge can grow its target's set after the target's
    /// predecessors were visited; then only those predecessors go again,
    /// from a worklist, not every block of the body once more.
    fn settle(&mut self, f: &Func) {
        self.settle_with(f, || {});
    }

    /// [`Self::settle`], calling `visit` at each block it visits.
    fn settle_with(&mut self, f: &Func, mut visit: impl FnMut()) {
        let len = f.body.len();
        let blocks = self.starts.len();
        let preds = (!self.forward).then(|| self.predecessors(len));
        // Popped last first; `queued[p]`: `p` is in `pending`.
        let mut pending: Vec<usize> = (0..blocks).collect();
        let mut queued = vec![true; blocks];
        while let Some(p) = pending.pop() {
            visit();
            queued[p] = false;
            let out = self.out(p, len);
            self.outs[p] = out;
            let mut live = out;
            for &(l, reads) in &self.firsts[self.first[p]..self.first[p + 1]] {
                live = if reads {
                    self.sets.insert(live, l)
                } else {
                    self.sets.remove(live, l)
                };
            }
            // Sets only grow from empty here, so a new size is a change.
            if self.sets.len(self.into[p]) == self.sets.len(live) {
                continue;
            }
            self.into[p] = live;
            if let Some((first, list)) = &preds {
                for &q in &list[first[p]..first[p + 1]] {
                    if !queued[q] {
                        queued[q] = true;
                        pending.push(q);
                    }
                }
            }
        }
    }

    /// Each block's predecessors: those of block `p` are
    /// `list[first[p]..first[p + 1]]`.
    fn predecessors(&self, len: usize) -> (Vec<usize>, Vec<usize>) {
        let blocks = self.starts.len();
        let mut first = vec![0usize; blocks + 1];
        let each = |visit: &mut dyn FnMut(usize, usize)| {
            for p in 0..blocks {
                let end = self.end(p, len);
                self.edges[end - 1].each(end, |to| {
                    if let Some(&q) = self.point_at.get(to) {
                        visit(p, q);
                    }
                });
            }
        };
        each(&mut |_, q| first[q + 1] += 1);
        for p in 0..blocks {
            first[p + 1] += first[p];
        }
        let mut list = vec![0usize; first[blocks]];
        let mut next = first.clone();
        each(&mut |p, q| {
            list[next[q]] = p;
            next[q] += 1;
        });
        (first, list)
    }

    /// A body's basic blocks, with nothing live into any of them yet,
    /// their sets made in `sets`: hash-consed only for [`coalesce`],
    /// whose differences need one node per content, and an unconsed
    /// arena for passes that only ask membership and union. Each block's
    /// first accesses are found in one forward pass, a local's stamp
    /// telling whether this block met it already: `O(B + n)`.
    fn blocks(f: &Func, sets: Sets) -> Option<Self> {
        let n = f.params.len() + f.locals.len();
        if n == 0 || f.body.is_empty() {
            return None;
        }
        let edges = zero_edges(&f.body, &f.labels)?;
        let (point_at, starts) = basic_blocks(&edges, f.body.len());
        let blocks = starts.len();
        let mut first = Vec::with_capacity(blocks + 1);
        let mut firsts = Vec::new();
        let mut met = vec![usize::MAX; n];
        for (at, ins) in f.body.iter().enumerate() {
            let p = point_at[at];
            if starts[p] == at {
                first.push(firsts.len());
            }
            let Ins::Op(I::LocalGet(l) | I::LocalSet(l) | I::LocalTee(l)) = ins else {
                continue;
            };
            if std::mem::replace(&mut met[*l as usize], p) != p {
                firsts.push((*l, matches!(ins, Ins::Op(I::LocalGet(_)))));
            }
        }
        first.push(firsts.len());
        Some(Self {
            into: vec![Set::EMPTY; blocks],
            outs: vec![Set::EMPTY; blocks],
            forward: forward_only(&edges, &point_at, &starts, f.body.len()),
            edges,
            point_at,
            starts,
            sets,
            first,
            firsts,
        })
    }

    /// For each instruction of a body: a write that no read follows, or a
    /// read after which its local is dead. Walking each block back, a
    /// local's next access in the block answers, a stamp per local saying
    /// whether there is one; the block's live-out set answers only for
    /// its last access of each local: `O(B + a log n)` after the fixed
    /// point, for `a` first accesses, with no set made.
    fn dead_accesses(f: &Func) -> Option<Vec<bool>> {
        let body = &f.body;
        let me = Self::new(f, Sets::unconsed())?;
        let mut dead = vec![false; body.len()];
        let n = f.params.len() + f.locals.len();
        // `later[l]`: the block whose walk met `l` already;
        // `next_writes[l]`: whether that next access writes it.
        let mut later = vec![usize::MAX; n];
        let mut next_writes = vec![false; n];
        for p in 0..me.starts.len() {
            for at in (me.starts[p]..me.end(p, body.len())).rev() {
                let (l, write) = match body[at] {
                    Ins::Op(I::LocalGet(l)) => (l, false),
                    Ins::Op(I::LocalSet(l) | I::LocalTee(l)) => (l, true),
                    _ => continue,
                };
                let i = l as usize;
                dead[at] = if later[i] == p {
                    next_writes[i]
                } else {
                    !me.sets.contains(me.outs[p], l)
                };
                later[i] = p;
                next_writes[i] = write;
            }
        }
        Some(dead)
    }

    fn end(&self, p: usize, len: usize) -> usize {
        self.starts.get(p + 1).copied().unwrap_or(len)
    }

    /// The locals live after block `p`: the union of those live into its
    /// successors, which costs where they differ, not their size.
    fn out(&mut self, p: usize, len: usize) -> Set {
        let end = self.end(p, len);
        let (point_at, into, sets) = (&self.point_at, &self.into, &mut self.sets);
        let mut live = Set::EMPTY;
        self.edges[end - 1].each(end, |to| {
            if let Some(&q) = point_at.get(to) {
                live = sets.union(live, into[q]);
            }
        });
        live
    }

    fn step(sets: &mut Sets, ins: &Ins, live: Set) -> Set {
        match ins {
            Ins::Op(I::LocalGet(l)) => sets.insert(live, *l),
            Ins::Op(I::LocalSet(l) | I::LocalTee(l)) => sets.remove(live, *l),
            _ => live,
        }
    }
}

/// The locals of suspending body `f` live just after its instruction at
/// `at`, ascending, where a `Save`, which returns at once, reads none:
/// what a run from there may read before it writes. `None` when the
/// body's branches are not analysed. Only `at`'s block is walked, back
/// from its live-out set.
pub(crate) fn live_after(f: &Func, at: usize) -> Option<Vec<u32>> {
    let mut me = Live::new(f, Sets::unconsed())?;
    let p = me.point_at[at];
    let mut live = me.outs[p];
    for ins in f.body[at + 1..me.end(p, f.body.len())].iter().rev() {
        live = Live::step(&mut me.sets, ins, live);
    }
    let mut members = Vec::new();
    me.sets.members(live, &mut members);
    Some(members)
}

/// How a local is defined at a position, for [`coalesce`]'s bundles: a
/// write, a copy of a local read at a position, or at the entry, a
/// parameter or the zero every other local starts with.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Def {
    Write,
    Copy(u32, u32),
    Param,
    Zero,
}

/// A class of locals that share one: its first local, their type, the
/// pieces of its members' ranges from the scan's position on, in order,
/// its key in its type's pool while no piece holds the scan's position
/// (the next piece's start, or `u32::MAX` when none is left), and a stamp
/// that changes with each change of state, so a heap entry of an older
/// state is skipped when it comes up.
struct Class {
    local: u32,
    ty: crate::module::Val,
    pieces: BTreeMap<u32, u32>,
    pooled: Option<u32>,
    stamp: u32,
}

/// The classes, each type's pool of classes no piece holds at the scan's
/// position, by the start of their next piece, and the positions where a
/// class's state changes next.
struct Scan {
    classes: Vec<Class>,
    pools: BTreeMap<crate::module::Val, BTreeSet<(u32, usize)>>,
    events: BinaryHeap<Reverse<(u32, usize, u32)>>,
    steps: usize,
}

impl Scan {
    /// Brings class `c` to position `at`: drops its pieces that end
    /// before, pools it when no piece holds `at`, and queues its next
    /// change.
    fn refresh(&mut self, c: usize, at: u32) {
        self.steps += 1;
        let class = &mut self.classes[c];
        while class.pieces.first_key_value().is_some_and(|p| *p.1 < at) {
            class.pieces.pop_first();
        }
        let front = class.pieces.first_key_value().map(|(&s, &e)| (s, e));
        let key = match front {
            Some(p) if p.0 <= at => None,
            Some(p) => Some(p.0),
            None => Some(u32::MAX),
        };
        if class.pooled != key {
            let pool = self.pools.entry(class.ty).or_default();
            if let Some(old) = class.pooled {
                pool.remove(&(old, c));
            }
            if let Some(new) = key {
                pool.insert((new, c));
            }
            class.pooled = key;
        }
        class.stamp += 1;
        let next = match front {
            Some(p) if p.0 <= at => Some(p.1 + 1),
            Some(p) => Some(p.0),
            None => None,
        };
        if let Some(next) = next {
            self.events.push(Reverse((next, c, class.stamp)));
        }
    }

    /// Brings every class whose state changes at or before `at` to `at`.
    fn advance(&mut self, at: u32) {
        while let Some(&Reverse((when, c, stamp))) = self.events.peek()
            && when <= at
        {
            self.events.pop();
            if self.classes[c].stamp == stamp {
                self.refresh(c, at);
            }
        }
    }
}

/// Rule 14: locals of one type whose values are never live at once share
/// one local. Binaryen's `CoalesceLocals` decides this from a pairwise
/// interference matrix, which costs the square of the locals live at once;
/// this is the linear scan with lifetime holes of regalloc2 and of Traub,
/// Holloway and Smith ("Quality and Speed in Linear-scan Register
/// Allocation", 1998), which keeps the same gaps without the pairs.
///
/// Positions: 0 is the entry, where the parameters are written and every
/// other local holds zero, and instruction `at` is at `at + 1`. A local's
/// ranges cover each position where it is written or live just after.
/// One backward walk over the basic blocks ([`Live`]) builds them: a read
/// opens a range, a write closes it, and at a block's end the locals open
/// and not live after it may close (a hole), and those live after it and
/// not open open. A block's end closes at most as many holes as the block
/// has local accesses, since a range fits a hole only if it begins or ends
/// at an access inside it; what it leaves open stays open until a block
/// that pays for its hole or a write. So `m` values read past the `n` arms
/// of an else-if chain make holes only where an arm has accesses, not
/// `m n`, and a panic guard's arm with no access makes none.
/// The sets are hash-consed persistent sets, so each difference costs the
/// locals it finds.
///
/// Each range carries a value number ([`value_numbers`]): a copy's range
/// holds its source's value. Then, as regalloc2 does before it allocates,
/// copies join their two locals' bundles, the most repeated copy first,
/// when wherever the bundles' ranges overlap they hold one value, which
/// is Binaryen's rule that a copy does not interfere with its source:
/// the smaller bundle's ranges are probed in the larger's ordered map and
/// moved into it. Two parameters hold different values at the entry, so
/// they never join.
///
/// Last, bundles go in order of their first position, parameters first,
/// each into a class of its type whose pieces leave a hole holding all of
/// it, the one whose next piece starts soonest after it, else a class of
/// its own. A copy between two locals of one class then goes, a pair of a
/// multi-value copy too ([`same_local_copies`]).
///
/// The one probe is a priced limit (#2025): a bundle that fits only a
/// free class other than the one it probes opens a class, so 13 stage
/// baselines declare one or two locals more than the quadratic first fit
/// did, with no code bytes. An exact fit needs a probe per class. Putting
/// the bundles of several ranges in a sweep before those of one range, as
/// regalloc2 allocates by priority, left the stage baselines' locals equal
/// in total and made the 38 bench builds 8 bytes larger, so the order
/// stays by first position.
///
/// Cost, for a body of `B` instructions, `n` locals, `c` copies and `R`
/// ranges (at most the writes, the reads, `n`, and two per access a hole
/// is paid by): the liveness pass and the walk, `O((B + R) log n)`; a
/// probe of the smaller bundle per copy and each range moved `log R`
/// times, `O((c + R log R) log R)`; and a pool and heap step per piece,
/// `O(R log n)`.
pub fn coalesce(f: &mut Func) {
    if f.raw.is_some() {
        return;
    }
    let Some(mut live) = Live::new(f, Sets::default()) else {
        return;
    };
    let np = f.params.len();
    let n = np + f.locals.len();
    let body = &f.body;
    let len = body.len();
    // ruled out: a body past `u32::MAX` instructions or locals; Wasm
    // counts both in u32, so no function reaches here larger.
    let n32 = u32::try_from(n).expect("locals");
    let pos = |at: usize| u32::try_from(at + 1).expect("body");
    let sources = copy_sources(body, n);
    // Each copy's two locals, the copied one second.
    let mut copies: Vec<(u32, u32)> = Vec::new();
    for (at, ins) in body.iter().enumerate() {
        if let (Ins::Op(I::LocalSet(x) | I::LocalTee(x)), Some((y, _))) = (*ins, sources[at])
            && y != x
        {
            copies.push((x, y));
        }
    }
    // The ranges: each local's, and the end of the one open in the walk,
    // or `NONE`.
    let mut ranges: Vec<(u32, u32, u32)> = Vec::new();
    let mut open = vec![NONE; n];
    let mut found = Vec::new();
    // The open ranges' locals, as a persistent set: those live, and those
    // not live whose hole no access paid for yet.
    let mut now = Set::EMPTY;
    for p in (0..live.starts.len()).rev() {
        let (start, end) = (live.starts[p], live.end(p, len));
        // What is open and not live after this block has a hole from here
        // back to where it is live again, as many of them as the block
        // has local accesses: a range that fits a hole begins or ends at
        // an access in it, so more holes than accesses hold no range. So
        // `m` values read past `n` arms of an else-if chain make no `m n`
        // holes, and a panic guard `if ... unreachable end` with no
        // access makes none. The others stay open, and an earlier block's
        // accesses may pay for theirs. What is live after it and not open
        // opens.
        let out = live.outs[p];
        let accesses = body[start..end]
            .iter()
            .filter(|i| matches!(i, Ins::Op(I::LocalGet(_) | I::LocalSet(_) | I::LocalTee(_))))
            .count();
        found.clear();
        live.sets.difference_upto(now, out, &mut found, accesses);
        for &x in &found {
            let e = std::mem::replace(&mut open[x as usize], NONE);
            ranges.push((x, pos(end), e));
            now = live.sets.remove(now, x);
        }
        found.clear();
        live.sets.difference(out, now, &mut found);
        for &x in &found {
            open[x as usize] = pos(end - 1);
            now = live.sets.insert(now, x);
        }
        for at in (start..end).rev() {
            match body[at] {
                Ins::Op(I::LocalGet(x)) if open[x as usize] == NONE => {
                    open[x as usize] = pos(at);
                }
                Ins::Op(I::LocalSet(x) | I::LocalTee(x)) => {
                    let e = std::mem::replace(&mut open[x as usize], NONE);
                    ranges.push((x, pos(at), if e == NONE { pos(at) } else { e }));
                }
                _ => {}
            }
            now = Live::step(&mut live.sets, &body[at], now);
        }
    }
    // What is open at the entry holds a parameter or a zero there, and
    // a parameter is written there whether it is read or not.
    for (x, e) in (0..n32).zip(&open) {
        if *e != NONE || (x as usize) < np {
            ranges.push((x, 0, if *e == NONE { 0 } else { *e }));
        }
    }
    // Each local's ranges, by start, and its definitions: its writes, by
    // position and local, each with the local and get it copies when it
    // copies one, and its value at the entry, a parameter's or a zero.
    let mut occ: Vec<BTreeMap<u32, u32>> = vec![BTreeMap::new(); n];
    let mut defs: Vec<BTreeMap<(u32, u32), Def>> = vec![BTreeMap::new(); n];
    for &(x, s, e) in &ranges {
        occ[x as usize].insert(s, e);
        if s == 0 && x as usize >= np {
            defs[x as usize].insert((0, x), Def::Zero);
        }
    }
    for p in 0..u32::try_from(np).expect("locals") {
        defs[p as usize].insert((0, p), Def::Param);
    }
    for (at, ins) in body.iter().enumerate() {
        if let Ins::Op(I::LocalSet(x) | I::LocalTee(x)) = *ins {
            let def = match sources[at] {
                Some((y, get)) if y != x => Def::Copy(y, pos(get)),
                _ => Def::Write,
            };
            defs[x as usize].insert((pos(at), x), def);
        }
    }
    let mut steps = ranges.len() + defs.iter().map(BTreeMap::len).sum::<usize>();
    // Bundles, as regalloc2 makes them before it allocates: locals joined
    // by copies that never hold two values at once. Each local's bundle is
    // found through `root`; a bundle's ranges, merged where they overlap,
    // and its definitions are kept at its root.
    let mut root: Vec<u32> = (0..n32).collect();
    let mut members: Vec<Vec<u32>> = root.iter().map(|&x| vec![x]).collect();
    let ty = |l: usize| {
        if l < np {
            f.params[l]
        } else {
            f.locals[l - np]
        }
    };
    // Each copy once, the most repeated first, as Binaryen ranks them.
    copies.sort_stable();
    let mut ranked: Vec<(u32, u32, usize)> = Vec::new();
    for &(x, y) in &copies {
        match ranked.last_mut() {
            Some((a, b, k)) if (*a, *b) == (x, y) => *k += 1,
            _ => ranked.push((x, y, 1)),
        }
    }
    ranked.sort_stable_by_key(|&(.., k)| Reverse(k));
    let find = |root: &mut [u32], mut x: u32| {
        while root[x as usize] != x {
            let up = root[root[x as usize] as usize];
            root[x as usize] = up;
            x = up;
        }
        x
    };
    for &(x, y, _) in &ranked {
        let (a, b) = (find(&mut root, x), find(&mut root, y));
        if a == b || ty(x as usize) != ty(y as usize) {
            continue;
        }
        let size = |c: u32| occ[c as usize].len() + defs[c as usize].len();
        let (small, big) = if size(a) <= size(b) { (a, b) } else { (b, a) };
        // Two values live at once means one's definition lies where the
        // other is live (Chaitin; Budimlić et al., "Fast Copy Coalescing
        // and Live-Range Identification", 2002), so each side's
        // definitions are probed where the other is live: the small
        // side's by a lookup each, the big side's by a range query per
        // small range.
        let clash = |def: (u32, Def), other: u32, root: &mut [u32]| {
            let (at, kind) = def;
            match kind {
                Def::Param => true,
                Def::Zero => defs[other as usize]
                    .values()
                    .next()
                    .is_some_and(|d| *d == Def::Param),
                Def::Write => true,
                Def::Copy(src, get) => {
                    find(root, src) != other
                        || defs[other as usize]
                            .range((get + 1, 0)..(at, 0))
                            .next()
                            .is_some()
                }
            }
        };
        let mut apart = true;
        'probe: {
            for (&(at, _), &kind) in &defs[small as usize] {
                steps += 1;
                let live = occ[big as usize]
                    .range(..=at)
                    .next_back()
                    .is_some_and(|(_, &e)| e >= at);
                if live && clash((at, kind), big, &mut root) {
                    apart = false;
                    break 'probe;
                }
            }
            for (&s, &e) in &occ[small as usize] {
                for (&(at, _), &kind) in defs[big as usize].range((s, 0)..=(e, u32::MAX)) {
                    steps += 1;
                    if clash((at, kind), small, &mut root) {
                        apart = false;
                        break 'probe;
                    }
                }
            }
        }
        if !apart {
            continue;
        }
        let (si, bi) = (small as usize, big as usize);
        // Ranges that overlap become one.
        for (s, e) in std::mem::take(&mut occ[si]) {
            let theirs = &mut occ[bi];
            let (mut lo, mut hi) = (s, e);
            while let Some((&bs, &be)) = theirs.range(..=e).next_back()
                && be >= s
            {
                steps += 1;
                theirs.remove(&bs);
                (lo, hi) = (lo.min(bs), hi.max(be));
            }
            theirs.insert(lo, hi);
        }
        let moved = std::mem::take(&mut defs[si]);
        defs[bi].extend(moved);
        let moved = std::mem::take(&mut members[si]);
        members[bi].extend(moved);
        root[si] = big;
    }
    // The bundles, by their first position, parameters first.
    let mut order: Vec<u32> = (0..n32)
        .filter(|&x| root[x as usize] == x && !occ[x as usize].is_empty())
        .collect();
    order.sort_stable_by_key(|&x| occ[x as usize].first_key_value().map(|(&s, _)| s));
    let mut scan = Scan {
        classes: Vec::new(),
        pools: BTreeMap::new(),
        events: BinaryHeap::new(),
        steps,
    };
    let mut to: Vec<u32> = (0..n32).collect();
    for &b in &order {
        let bi = b as usize;
        let mine = &occ[bi];
        let (Some((&s, _)), Some((_, &e))) = (mine.first_key_value(), mine.last_key_value()) else {
            continue;
        };
        // A root's members hold the root itself.
        let local = members[bi].iter().copied().min().unwrap_or(b);
        let t = ty(local as usize);
        scan.advance(s);
        // A parameter keeps its own local; another bundle takes the class
        // whose next piece starts soonest after its last position, else,
        // as Traub et al.'s binpacking fills a lifetime hole, the free
        // class whose next piece starts latest when the bundle's ranges
        // fit between its pieces: one probe per range of the bundle.
        let found = if (local as usize) < np {
            None
        } else {
            scan.pools.get(&t).and_then(|pool| {
                if let Some(&(_, c)) = pool.range((e + 1, 0)..).next() {
                    return Some(c);
                }
                let &(_, c) = pool.last()?;
                let theirs = &scan.classes[c].pieces;
                scan.steps += mine.len();
                mine.iter()
                    .all(|(&s, &e)| theirs.range(..=e).next_back().is_none_or(|(_, &te)| te < s))
                    .then_some(c)
            })
        };
        let pieces = mine.iter().map(|(&start, &end)| (start, end));
        let c = if let Some(c) = found {
            scan.classes[c].pieces.extend(pieces);
            c
        } else {
            scan.classes.push(Class {
                local,
                ty: t,
                pieces: pieces.collect(),
                pooled: None,
                stamp: 0,
            });
            scan.classes.len() - 1
        };
        scan.steps += mine.len();
        scan.refresh(c, s);
        for &m in &members[bi] {
            to[m as usize] = scan.classes[c].local;
        }
    }
    crate::work::liveness(scan.steps);
    if to.iter().enumerate().all(|(x, t)| *t as usize == x) {
        return;
    }
    let renamed: Vec<Ins> = body
        .iter()
        .map(|ins| match *ins {
            Ins::Op(I::LocalGet(l)) => Ins::Op(I::LocalGet(to[l as usize])),
            Ins::Op(I::LocalSet(l)) => Ins::Op(I::LocalSet(to[l as usize])),
            Ins::Op(I::LocalTee(l)) => Ins::Op(I::LocalTee(to[l as usize])),
            other => other,
        })
        .collect();
    let gone = same_local_copies(&renamed);
    let mut origins = Remap::new(f.origins.as_deref());
    let mut out: Vec<Ins> = Vec::with_capacity(body.len());
    for (at, ins) in renamed.into_iter().enumerate() {
        if gone[at] {
            continue;
        }
        if let (Some(Ins::Op(I::LocalGet(a))), Ins::Op(I::LocalTee(b))) = (out.last(), ins)
            && *a == b
        {
            continue;
        }
        out.push(ins);
        origins.copy(at..at + 1);
    }
    f.body = out;
    origins.finish(f);
    if let Some(names) = &mut f.local_names {
        let mut merged = BTreeMap::new();
        for (l, name) in std::mem::take(names) {
            merged.entry(to[l as usize]).or_insert(name);
        }
        *names = merged;
    }
    renumber(f);
}

/// For each `local.set` or `local.tee`, the local whose value it writes
/// when a `local.get` pushed it, and that get's index: the get just
/// before it, or, for the `j`th set of a run of sets, the `j`th get back
/// from the end of the run of gets before them, a multi-value copy, so
/// long as no set of the run before it writes that local. One pass: each
/// run's sets stamp the locals they write with the run's start, so the
/// check is one probe, not a rescan of the run per set.
fn copy_sources(body: &[Ins], locals: usize) -> Vec<Option<(u32, usize)>> {
    let mut out = vec![None; body.len()];
    // Where the run of gets, and of sets, ending at each index began.
    let mut gets_from = 0;
    let mut sets_from = 0;
    let mut stamp = vec![usize::MAX; locals];
    for (at, ins) in body.iter().enumerate() {
        let get = matches!(ins, Ins::Op(I::LocalGet(_)));
        let after_get = at > 0 && matches!(body[at - 1], Ins::Op(I::LocalGet(_)));
        let after_set = at > 0 && matches!(body[at - 1], Ins::Op(I::LocalSet(_)));
        if get && !after_get {
            gets_from = at;
        }
        let Ins::Op(I::LocalSet(x) | I::LocalTee(x)) = *ins else {
            continue;
        };
        let start = if after_set { sets_from } else { at };
        let gets_end = start;
        if let Some(first) = start.checked_sub(1 + at - start)
            && gets_end > 0
            && matches!(body[gets_end - 1], Ins::Op(I::LocalGet(_)))
            && gets_from <= first
            && let Ins::Op(I::LocalGet(y)) = body[first]
            && stamp[y as usize] != start
        {
            out[at] = Some((y, first));
        }
        if matches!(ins, Ins::Op(I::LocalSet(_))) {
            if !after_set {
                sets_from = at;
            }
            stamp[x as usize] = start;
        }
    }
    out
}

/// The pairs of multi-value copies `local.get a ...; ... local.set a`
/// whose two ends name one local and that no set of the run before writes
/// over: each such pair is a no-op, and taking both ends out leaves every
/// other pair matched.
fn same_local_copies(body: &[Ins]) -> Vec<bool> {
    let mut gone = vec![false; body.len()];
    let mut at = 0;
    while at < body.len() {
        if !matches!(body[at], Ins::Op(I::LocalSet(_))) {
            at += 1;
            continue;
        }
        let start = at;
        while at < body.len() && matches!(body[at], Ins::Op(I::LocalSet(_))) {
            at += 1;
        }
        let mut gets = start;
        while gets > 0 && matches!(body[gets - 1], Ins::Op(I::LocalGet(_))) {
            gets -= 1;
        }
        for j in 0..(at - start).min(start - gets) {
            let (Ins::Op(I::LocalSet(x)), Ins::Op(I::LocalGet(y))) =
                (body[start + j], body[start - 1 - j])
            else {
                continue;
            };
            let overwritten = body[start..start + j]
                .iter()
                .any(|i| matches!(i, Ins::Op(I::LocalSet(z)) if *z == x));
            if x == y && !overwritten {
                gone[start + j] = true;
                gone[start - 1 - j] = true;
            }
        }
    }
    gone
}

/// Rules 12 and 13, from one backward liveness pass over the basic blocks
/// (loop backedges to a fixed point).
///
/// Rule 12 deletes a `local.set` or `local.tee` of a local that no path
/// reads before its next write, dropping a set's value, or the pure push
/// that made it. A zero left in a moved-from local stays exactly when
/// some path still releases it (the one exit, a loop's next drop, a
/// suspension's saved frame).
///
/// Rule 13 keeps a value on the stack. In `local.set t; R; local.get t`,
/// where that get is the first access of `t` after the set and `R` is a
/// straight-line run (panic guards `if ... unreachable end` included):
///
/// - when `t` is dead after the get, the pure push the set takes (the
///   one just before it, or its partner in a multi-value copy) moves to
///   the get, if `R` writes nothing it reads: rule 4 by liveness rather
///   than by counts; a `local.tee t` after a pure push goes the same
///   way, the get pushing that push again;
/// - otherwise, when `R` leaves the stack as it found it without
///   reaching below, the get goes and the value stays under `R`; the
///   set goes too when `t` is dead after the get, and becomes a tee
///   when it is not.
///
/// A rewrite's span is entered only at its write, its get is the write's
/// only reader, and nothing in it writes a moved push's local, so no
/// rewrite changes a liveness fact another one reads. A rewrite that
/// leaves its value under its run nests in or follows another such
/// rewrite, and holds both or neither end of a moved multi-value copy,
/// so each run keeps the stack level its rewrite needs.
///
/// The liveness pass runs once per call, not once per round: a round's
/// rewrites change liveness only inside their straight-line spans, so
/// [`live_once`] hands the next round its dead accesses for the new
/// body, as LLVM's `LiveIntervals::handleMove` and `LiveVariables`'
/// kill updates keep liveness across a local rewrite instead of solving
/// the function again.
fn live_rules(f: &mut Func, arity: &Arity) -> bool {
    let Some(mut dead) = Live::dead_accesses(f) else {
        return false;
    };
    let mut any = false;
    while let Some(next) = live_once(f, arity, &dead) {
        debug_assert!(
            Live::dead_accesses(f).is_none_or(|solved| solved == next),
            "rule 13's carried dead accesses differ from a new liveness pass"
        );
        dead = next;
        any = true;
    }
    any
}

/// No row, in a `u32` table of [`live_once`].
const NONE: u32 = u32::MAX;

/// One round of rules 12 and 13 on the body whose dead accesses are
/// `dead`, as [`Live::dead_accesses`] gives them. When it rewrites the
/// body, gives the new body's dead accesses, with no liveness pass.
///
/// A rewrite moves liveness only inside its span: a removed pair's local
/// is dead between its set and its get, and nothing else changes, as a
/// set kills its local and the get was its first read. A moved push's
/// local `m` is read later, at the get, with no write of `m` between,
/// so in each maximal union of one local's move spans, every read of `m`
/// but the last is live after, and the last, a moved get, is dead
/// exactly when `m`'s last old access in the union was: what follows the
/// union is unchanged. Each access is walked in at most one union, and
/// the moves are sorted once: `O(B + M log M)` for `M` moves, no set.
fn live_once(f: &mut Func, arity: &Arity, dead: &[bool]) -> Option<Vec<bool>> {
    crate::work::liveness(f.body.len());
    let body = &f.body;
    let mut origins = Remap::new(f.origins.as_deref());
    let mut out = Vec::with_capacity(body.len());
    let dead_write =
        |at: usize| matches!(body[at], Ins::Op(I::LocalSet(_) | I::LocalTee(_))) && dead[at];
    let mut gone = vec![false; body.len()];
    // `moved[j]`: the push a get takes the place of, or `NONE`. Words of
    // four bytes, as `next` is: a long body has a million rows.
    let mut moved = vec![NONE; body.len()];
    // The spans of the pairs that leave their value under their run, and
    // the pushes and sets between a moved push and its set in a
    // multi-value copy: a pair that keeps the stack level must not hold
    // only one end of such a copy.
    let mut open: Vec<(usize, usize)> = Vec::new();
    let mut in_copy = vec![false; body.len()];
    let mut teed = vec![false; body.len()];
    // `next[at]`: the next access of the local that `at` writes.
    let mut next = vec![NONE; body.len()];
    // `(m, p, j)`: the read of local `m` at `p` moves to `j`.
    let mut reads_moved: Vec<(u32, u32, usize)> = Vec::new();
    let mut seen = vec![NONE; f.params.len() + f.locals.len()];
    for (at, ins) in body.iter().enumerate().rev() {
        if let Ins::Op(I::LocalSet(l) | I::LocalTee(l) | I::LocalGet(l)) = ins {
            next[at] = seen[*l as usize];
            seen[*l as usize] = u32::try_from(at).expect("body");
        }
    }
    for k in 0..body.len() {
        let tee = match body[k] {
            Ins::Op(I::LocalSet(_)) => false,
            Ins::Op(I::LocalTee(_)) => true,
            _ => continue,
        };
        if dead[k] {
            continue;
        }
        while open.last().is_some_and(|&(_, e)| e < k) {
            open.pop();
        }
        let j = next[k];
        if j == NONE {
            continue;
        }
        let j = j as usize;
        if !matches!(body[j], Ins::Op(I::LocalGet(_))) {
            continue;
        }
        let last = dead[j];
        let Some(level) = single_use(body, k, j, arity, &f.symbols) else {
            continue;
        };
        if let Some(p) = if tee {
            k.checked_sub(1)
        } else {
            matching_push(body, k)
        } && last
            && pure_push(&body[p])
            && !gone[p]
            && moved[p] == NONE
            && open.last().is_none_or(|&(s, _)| s <= p)
            && !body[p + 1..j].iter().any(|i| clobbers(&body[p], i))
        {
            // A tee's push stays where it is, and the get reads it again.
            gone[p] = !tee;
            moved[j] = u32::try_from(p).expect("body");
            if let Ins::Op(I::LocalGet(m)) = body[p] {
                reads_moved.push((m, moved[j], j));
            }
            in_copy[p + 1..k].fill(true);
        } else if level && !tee && !in_copy[k] && open.last().is_none_or(|&(_, e)| j <= e) {
            // The value stays under the run; a set read again later
            // becomes a tee, unless the get feeds a copy, which rule 13
            // moves instead.
            if !last && matches!(body.get(j + 1), Some(Ins::Op(I::LocalSet(_)))) {
                continue;
            }
            gone[j] = true;
            open.push((k, j));
            if !last {
                teed[k] = true;
                continue;
            }
        } else {
            continue;
        }
        gone[k] = true;
    }
    if !gone.iter().any(|g| *g) && !(0..body.len()).any(dead_write) {
        return None;
    }
    // The dead accesses after the rewrites, by old position: a moved get
    // that reads no local is no access, and each union of one local's
    // move spans is settled as the doc comment says.
    let mut after = dead.to_vec();
    for &(_, _, j) in &reads_moved {
        after[j] = false;
    }
    for (at, &p) in moved.iter().enumerate() {
        if p != NONE && !matches!(body[p as usize], Ins::Op(I::LocalGet(_))) {
            after[at] = false;
        }
    }
    reads_moved.sort_stable();
    let mut i = 0;
    while i < reads_moved.len() {
        let (m, lo, mut hi) = reads_moved[i];
        i += 1;
        while let Some(&(n, p, j)) = reads_moved.get(i)
            && n == m
            && (p as usize) < hi
        {
            hi = hi.max(j);
            i += 1;
        }
        // `m`'s old accesses in the union are reads, as no move's span
        // writes its local.
        let mut last = dead[lo as usize];
        let mut r = lo;
        while r != NONE && (r as usize) < hi {
            last = dead[r as usize];
            after[r as usize] = false;
            r = next[r as usize];
        }
        after[hi] = last;
    }
    let mut dead_after = Vec::with_capacity(body.len());
    for (at, ins) in body.iter().enumerate() {
        if gone[at] {
            continue;
        }
        dead_after.push(after[at] && !dead_write(at));
        if let (true, Ins::Op(I::LocalSet(t))) = (teed[at], ins) {
            out.push(Ins::Op(I::LocalTee(*t)));
            origins.copy(at..at + 1);
        } else if moved[at] != NONE {
            let p = moved[at] as usize;
            out.push(body[p]);
            let origin = origins
                .first(p..p + 1)
                .or_else(|| origins.first(at..at + 1));
            origins.repeat(origin, 1);
        } else if dead_write(at) {
            // A dead set drops its value; rule 15 takes back a pure push
            // just before it.
            if matches!(ins, Ins::Op(I::LocalTee(_))) {
                dead_after.pop();
                continue;
            }
            out.push(Ins::Op(I::Drop));
            origins.copy(at..at + 1);
        } else {
            out.push(*ins);
            origins.copy(at..at + 1);
        }
    }
    f.body = out;
    origins.finish(f);
    Some(dead_after)
}

/// The pure push that the `local.set` at `k` takes its value from, when
/// a row of pure pushes and then a row of sets ends at `k`: the sets
/// take the pushes back in reverse, as a multi-value copy does.
fn matching_push(b: &[Ins], k: usize) -> Option<usize> {
    let mut i = k;
    while i > 0 && matches!(b[i - 1], Ins::Op(I::LocalSet(_))) {
        i -= 1;
    }
    let p = i.checked_sub(1 + (k - i))?;
    b[p..i].iter().all(pure_push).then_some(p)
}

/// Whether the read at `j` is in the same straight-line run as the write
/// at `k` (a panic guard counts as straight line), and if so, whether
/// the run between leaves the stack as it found it without reaching
/// below.
fn single_use(b: &[Ins], k: usize, j: usize, arity: &Arity, symbols: &Symbols) -> Option<bool> {
    // Bounds the scan, so the rule stays linear in the body.
    const REACH: usize = 128;
    if j - k > REACH {
        return None;
    }
    let (mut depth, mut low) = (0usize, false);
    let mut at = k + 1;
    while at < j {
        let i = &b[at];
        let (pops, pushes) = match i {
            Ins::Op(I::If(wasm_encoder::BlockType::Empty)) => {
                at = guard_end(b, at)?;
                (1, 0)
            }
            _ if ends_run(i) => return None,
            _ => effect(i, arity, symbols)?,
        };
        low |= pops > depth;
        depth = depth.saturating_sub(pops) + pushes;
        at += 1;
    }
    (at == j).then_some(depth == 0 && !low)
}

/// The `end` of the `if` at `at` when its one arm calls with constant
/// arguments and ends in `unreachable`, as a bounds or overflow check's
/// panic does: no local, branch or stack value of the code around it.
fn guard_end(b: &[Ins], at: usize) -> Option<usize> {
    for (j, i) in b.iter().enumerate().skip(at + 1) {
        match i {
            Ins::Op(I::End) => return matches!(b[j - 1], Ins::Op(I::Unreachable)).then_some(j),
            Ins::Op(I::Unreachable | I::I32Const(_) | I::I64Const(_))
            | Ins::Call(_)
            | Ins::Anchor(_) => {}
            _ => return None,
        }
    }
    None
}

/// An instruction that pushes one value and has no other effect.
fn pure_push(i: &Ins) -> bool {
    matches!(
        i,
        Ins::Op(I::LocalGet(_) | I::I32Const(_) | I::I64Const(_) | I::F32Const(_) | I::F64Const(_))
            | Ins::GlobalGet(_)
    )
}

/// Rule 4: moves each copy into a local read once to its read.
fn forward(f: &mut Func, np: u32) -> bool {
    forward_with(f, np, || {})
}

/// O(instructions + locals + globals): index uses once, carry the next
/// clobber/control edge while walking backwards, then compact once.
fn forward_with(f: &mut Func, np: u32, mut visit: impl FnMut()) -> bool {
    let end = f.body.len();
    if end < 2 {
        return false;
    }
    let (reads, writes) = counts_with(f, &mut visit);
    let mut read_at = vec![end; reads.len()];
    for (i, ins) in f.body.iter().enumerate() {
        visit();
        if let Ins::Op(I::LocalGet(local)) = ins {
            read_at[*local as usize] = i;
        }
    }
    // The prefix is still intact at each reverse step. Removing its last
    // push/set pair links the preceding position directly to the suffix.
    // The same links answer adjacency at a moved read across earlier holes.
    let mut next: Vec<_> = (1..=end).collect();
    let mut next_write = vec![end; reads.len()];
    let mut global_writes = hd_base::HashMap::default();
    let (mut call, mut boundary, mut hard_boundary, mut empty_block) = (end, end, end, end);
    let mut changed = false;
    for k in (0..end).rev() {
        visit();
        let p = f.body[k];
        if let Ins::Op(I::LocalSet(local) | I::LocalTee(local)) = p {
            next_write[local as usize] = k;
        }
        if let Ins::GlobalSet(global) = p {
            global_writes.insert(global, k);
        }
        if matches!(p, Ins::Call(_) | Ins::CallIndirect(_)) {
            call = k;
        }
        if ends_run(&p) {
            boundary = k;
            if matches!(p, Ins::Op(I::Block(wasm_encoder::BlockType::Empty))) {
                empty_block = k;
            } else {
                hard_boundary = k;
            }
        }
        let set = next[k];
        if set >= end {
            continue;
        }
        let Ins::Op(I::LocalSet(t)) = f.body[set] else {
            continue;
        };
        let pure = matches!(
            p,
            Ins::Op(
                I::LocalGet(_) | I::I32Const(_) | I::I64Const(_) | I::F32Const(_) | I::F64Const(_)
            ) | Ins::GlobalGet(_)
                | Ins::TableSlot(_)
                | Ins::Glue(_)
                | Ins::VTable(_)
        );
        #[cfg(feature = "checked-report")]
        let pure = pure || matches!(p, Ins::Descriptor(..));
        if !pure || t < np || reads[t as usize] != 1 || writes[t as usize] != 1 {
            continue;
        }
        visit();
        let j = read_at[t as usize];
        if j <= set || j >= end {
            continue;
        }
        // Only an i32 literal may cross an empty block, and only to expose
        // a bulk-memory width. All other control edges remain barriers.
        let literal = matches!(p, Ins::Op(I::I32Const(_)));
        let stop = if literal { hard_boundary } else { boundary };
        if stop < j {
            continue;
        }
        let clobber = match p {
            Ins::Op(I::LocalGet(local)) => next_write[local as usize],
            Ins::GlobalGet(global) => global_writes.get(&global).copied().unwrap_or(end).min(call),
            _ => end,
        };
        if set < clobber && clobber < j {
            continue;
        }
        if literal && empty_block < j {
            let after = next[j];
            if after >= end
                || !matches!(
                    f.body[after],
                    Ins::Op(I::MemoryCopy { .. } | I::MemoryFill(_))
                )
            {
                continue;
            }
        }
        f.body[j] = p;
        if let Some(origins) = &mut f.origins {
            origins[j] = origins[k].or(origins[set]).or(origins[j]);
        }
        read_at[t as usize] = end;
        if let Ins::Op(I::LocalGet(local)) = p
            && reads[local as usize] == 1
        {
            read_at[local as usize] = j;
        }
        // This destination had exactly one write, which is now gone.
        next_write[t as usize] = end;
        let after = next[set];
        next[k] = usize::MAX;
        next[set] = usize::MAX;
        if k > 0 {
            next[k - 1] = after;
        }
        changed = true;
    }
    if !changed {
        return false;
    }
    let body = std::mem::take(&mut f.body);
    let mut out = Vec::with_capacity(body.len());
    let mut origins = Remap::new(f.origins.as_deref());
    for (i, ins) in body.into_iter().enumerate() {
        visit();
        if next[i] != usize::MAX {
            out.push(ins);
            origins.copy(i..i + 1);
        }
    }
    f.body = out;
    origins.finish(f);
    true
}

/// Rule 7: folds the constants of each window of instructions.
fn fold(f: &mut Func) -> bool {
    use wasm_encoder::BlockType;

    let mut origins = Remap::new(f.origins.as_deref());
    let b = std::mem::take(&mut f.body);
    let mut out: Vec<Ins> = Vec::with_capacity(b.len());
    let mut changed = false;
    let mut k = 0;
    let c32 = |i: Option<&Ins>| match i {
        Some(Ins::Op(I::I32Const(c))) => Some(*c),
        _ => None,
    };
    let push = |i: Option<&Ins>| {
        matches!(
            i,
            Some(
                Ins::Op(
                    I::LocalGet(_)
                        | I::I32Const(_)
                        | I::I64Const(_)
                        | I::F32Const(_)
                        | I::F64Const(_)
                ) | Ins::GlobalGet(_)
            )
        )
    };
    while k < b.len() {
        let from = k;
        let output_start = out.len();
        let at = |j: usize| b.get(k + j);
        // `c; local.tee t; i32.eqz; if ... end` of a nonzero `c`, with no
        // `else`: the test never holds.
        if let (
            Some(c),
            Some(Ins::Op(I::LocalTee(t))),
            Some(Ins::Op(I::I32Eqz)),
            Some(Ins::Op(I::If(_))),
        ) = (c32(at(0)), at(1), at(2), at(3))
            && c != 0
            && let Some(end) = block_end(&b, k + 3)
        {
            out.push(Ins::Op(I::I32Const(c)));
            out.push(Ins::Op(I::LocalSet(*t)));
            k = end + 1;
            changed = true;
            origins.merged(from..k, out.len() - output_start);
            continue;
        }
        match (at(0), at(1), at(2), at(3)) {
            // Multiplication by one leaves the existing i32 on the stack.
            (Some(Ins::Op(I::I32Const(1))), Some(Ins::Op(I::I32Mul)), ..) => {
                k += 2;
                changed = true;
            }
            // A void block with no body, or only a branch to its own end,
            // leaves the stack alone. Reduce inside out in the same walk.
            (Some(Ins::Op(I::End)), ..)
                if matches!(out.last(), Some(Ins::Op(I::Block(BlockType::Empty))))
                    || matches!(
                        out.as_slice(),
                        [.., Ins::Op(I::Block(BlockType::Empty)), Ins::Op(I::Br(0))]
                    ) =>
            {
                if matches!(out.last(), Some(Ins::Op(I::Br(0)))) {
                    out.pop();
                    origins.pop();
                }
                out.pop();
                origins.pop();
                k += 1;
                changed = true;
                continue;
            }
            // Two constants joined by `+`, `-` or `*` are one constant
            // (`i32` wraps either way).
            (
                Some(Ins::Op(I::I32Const(a))),
                Some(Ins::Op(I::I32Const(b))),
                Some(Ins::Op(op @ (I::I32Add | I::I32Sub | I::I32Mul))),
                _,
            ) => {
                let c = match op {
                    I::I32Add => a.wrapping_add(*b),
                    I::I32Sub => a.wrapping_sub(*b),
                    _ => a.wrapping_mul(*b),
                };
                out.push(Ins::Op(I::I32Const(c)));
                k += 3;
                changed = true;
            }
            // Adding 0 goes, on either side.
            (Some(Ins::Op(I::I32Const(0))), Some(Ins::Op(I::I32Add)), ..) => {
                k += 2;
                changed = true;
            }
            (Some(Ins::Op(I::I32Const(0))), v, Some(Ins::Op(I::I32Add)), _) if push(v) => {
                out.push(*v.expect("a push"));
                k += 3;
                changed = true;
            }
            // `+ a ± b` is one addition, of `a ± b` (`i32` wraps either way).
            (
                Some(Ins::Op(I::I32Const(a))),
                Some(Ins::Op(I::I32Add)),
                Some(Ins::Op(I::I32Const(b))),
                Some(Ins::Op(op @ (I::I32Add | I::I32Sub))),
            ) => {
                let c = if matches!(op, I::I32Add) {
                    a.wrapping_add(*b)
                } else {
                    a.wrapping_sub(*b)
                };
                out.extend([Ins::Op(I::I32Const(c)), Ins::Op(I::I32Add)]);
                k += 4;
                changed = true;
            }
            (Some(Ins::Op(I::I32Const(0))), Some(Ins::Op(I::I32Eq)), ..) => {
                out.push(Ins::Op(I::I32Eqz));
                k += 2;
                changed = true;
            }
            // A test of `x != 0`, `x >u 0` or `!!x` is a test of `x`, and
            // so is its zero check.
            (
                Some(Ins::Op(I::I32Const(0))),
                Some(Ins::Op(I::I32Ne | I::I32GtU)),
                Some(Ins::Op(I::If(_) | I::BrIf(_) | I::I32Eqz)),
                _,
            )
            | (
                Some(Ins::Op(I::I32Eqz)),
                Some(Ins::Op(I::I32Eqz)),
                Some(Ins::Op(I::If(_) | I::BrIf(_) | I::I32Eqz)),
                _,
            ) => {
                k += 2;
                changed = true;
            }
            (
                Some(Ins::Op(I::I32Const(c))),
                Some(Ins::Op(op @ (I::I32DivU | I::I32RemU | I::I32Mul))),
                ..,
            ) if *c > 0
                && c.cast_unsigned().is_power_of_two()
                && (*c >= 64 || !matches!(op, I::I32Mul)) =>
            {
                let n = c.cast_unsigned().trailing_zeros().cast_signed();
                match op {
                    I::I32DivU => out.extend([Ins::Op(I::I32Const(n)), Ins::Op(I::I32ShrU)]),
                    I::I32RemU => out.extend([Ins::Op(I::I32Const(c - 1)), Ins::Op(I::I32And)]),
                    _ => out.extend([Ins::Op(I::I32Const(n)), Ins::Op(I::I32Shl)]),
                }
                k += 2;
                changed = true;
            }
            (Some(Ins::Op(I::I32Const(c))), Some(Ins::Op(I::I32Add)), Some(Ins::Op(load)), _)
                if *c >= 0 && with_offset(load, 0).is_some() && is_load(load) =>
            {
                out.push(Ins::Op(
                    with_offset(load, c.cast_unsigned()).expect("a load"),
                ));
                k += 3;
                changed = true;
            }
            _ => {
                out.push(b[k]);
                k += 1;
            }
        }
        origins.merged(from..k, out.len() - output_start);
    }
    f.body = out;
    origins.finish(f);
    changed
}

/// Rule 15: each `drop` takes back what pushed its value while that has
/// no other effect, so a discarded read leaves no code.
pub(crate) fn drops(f: &mut Func) -> bool {
    let mut origins = Remap::new(f.origins.as_deref());
    let b = std::mem::take(&mut f.body);
    let mut out: Vec<Ins> = Vec::with_capacity(b.len());
    let mut changed = false;
    // The `end` of an `if` taken for a `drop`, which goes with it.
    let mut empty_end = None;
    for (k, ins) in b.iter().enumerate() {
        if empty_end == Some(k) {
            continue;
        }
        let end = matches!(ins, Ins::Op(I::If(wasm_encoder::BlockType::Empty)))
            .then(|| {
                k + 1
                    + b[k + 1..]
                        .iter()
                        .take_while(|i| matches!(i, Ins::Anchor(_)))
                        .count()
            })
            .filter(|&e| matches!(b.get(e), Some(Ins::Op(I::End))));
        if end.is_some() {
            empty_end = end;
            changed = true;
        } else if !matches!(ins, Ins::Op(I::Drop)) {
            out.push(*ins);
            origins.copy(k..k + 1);
            continue;
        }
        // The values still to drop, the last one on top. An anchor writes
        // no code, so the search looks past it and keeps it.
        let mut n = 1usize;
        let mut anchors = Vec::new();
        while n > 0 {
            while let Some(&a @ Ins::Anchor(_)) = out.last() {
                out.pop();
                anchors.push((a, origins.pop()));
            }
            let Some(last) = out.last() else { break };
            let operands = match last {
                Ins::Op(I::LocalTee(t)) => {
                    let t = *t;
                    out.pop();
                    origins.pop();
                    out.push(Ins::Op(I::LocalSet(t)));
                    origins.copy(k..k + 1);
                    n -= 1;
                    changed = true;
                    break;
                }
                Ins::Op(i) => match pure_operands(i) {
                    Some(m) => m,
                    None => break,
                },
                Ins::Static(_)
                | Ins::GlobalGet(_)
                | Ins::TableSlot(_)
                | Ins::Glue(_)
                | Ins::VTable(_) => 0,
                _ => break,
            };
            out.pop();
            origins.pop();
            n = n - 1 + operands;
            changed = true;
        }
        for (a, origin) in anchors.into_iter().rev() {
            out.push(a);
            origins.repeat(origin, 1);
        }
        for _ in 0..n {
            out.push(Ins::Op(I::Drop));
            origins.copy(k..k + 1);
        }
    }
    f.body = out;
    origins.finish(f);
    changed
}

/// How many operands `i` takes when it pushes one value and does nothing
/// else (it cannot trap, a load aside, which rule 15 covers); `None` for
/// any other instruction.
fn pure_operands(i: &I) -> Option<usize> {
    Some(match i {
        I::LocalGet(_)
        | I::I32Const(_)
        | I::I64Const(_)
        | I::F32Const(_)
        | I::F64Const(_)
        | I::MemorySize(_) => 0,
        _ if is_load(i) => 1,
        I::I32Eqz
        | I::I64Eqz
        | I::I32Clz
        | I::I32Ctz
        | I::I32Popcnt
        | I::I64Clz
        | I::I64Ctz
        | I::I64Popcnt
        | I::I32Extend8S
        | I::I32Extend16S
        | I::I32WrapI64
        | I::I64ExtendI32S
        | I::I64ExtendI32U
        | I::I32ReinterpretF32
        | I::I64ReinterpretF64
        | I::F32ReinterpretI32
        | I::F64ReinterpretI64
        | I::I32TruncSatF32S
        | I::I32TruncSatF32U
        | I::I32TruncSatF64S
        | I::I32TruncSatF64U
        | I::I64TruncSatF32S
        | I::I64TruncSatF32U
        | I::I64TruncSatF64S
        | I::I64TruncSatF64U
        | I::F32ConvertI32S
        | I::F32ConvertI32U
        | I::F32ConvertI64S
        | I::F32ConvertI64U
        | I::F64ConvertI32S
        | I::F64ConvertI32U
        | I::F64ConvertI64S
        | I::F64ConvertI64U
        | I::F32DemoteF64
        | I::F64PromoteF32
        | I::F32Neg
        | I::F64Neg
        | I::F32Abs
        | I::F64Abs
        | I::F32Trunc
        | I::F64Trunc
        | I::F32Floor
        | I::F64Floor
        | I::F32Ceil
        | I::F64Ceil
        | I::F32Nearest
        | I::F64Nearest
        | I::F32Sqrt
        | I::F64Sqrt => 1,
        I::I32Add
        | I::I32Sub
        | I::I32Mul
        | I::I32And
        | I::I32Or
        | I::I32Xor
        | I::I32Shl
        | I::I32ShrS
        | I::I32ShrU
        | I::I32Eq
        | I::I32Ne
        | I::I32LtS
        | I::I32LtU
        | I::I32GtS
        | I::I32GtU
        | I::I32LeS
        | I::I32LeU
        | I::I32GeS
        | I::I32GeU
        | I::I64Add
        | I::I64Sub
        | I::I64Mul
        | I::I64And
        | I::I64Or
        | I::I64Xor
        | I::I64Shl
        | I::I64ShrS
        | I::I64ShrU
        | I::I64Eq
        | I::I64Ne
        | I::I64LtS
        | I::I64LtU
        | I::I64GtS
        | I::I64GtU
        | I::I64LeS
        | I::I64LeU
        | I::I64GeS
        | I::I64GeU
        | I::F32Add
        | I::F32Sub
        | I::F32Mul
        | I::F32Div
        | I::F32Copysign
        | I::F32Eq
        | I::F32Ne
        | I::F32Lt
        | I::F32Gt
        | I::F32Le
        | I::F32Ge
        | I::F64Add
        | I::F64Sub
        | I::F64Mul
        | I::F64Div
        | I::F64Copysign
        | I::F64Eq
        | I::F64Ne
        | I::F64Lt
        | I::F64Gt
        | I::F64Le
        | I::F64Ge => 2,
        I::Select => 3,
        _ => return None,
    })
}

/// Rules 8 and 9 operate on straight-line stack effects. Schedule the
/// single-use locals first so the store walk sees the resulting stack order.
fn schedule(f: &mut Func, np: u32, arity: &Arity) -> bool {
    schedule_with(f, np, arity, || {})
}

fn schedule_with(f: &mut Func, np: u32, arity: &Arity, mut visit: impl FnMut()) -> bool {
    let locals = schedule_locals(f, np, arity, &mut visit);
    let stores = schedule_stores(f, arity, &mut visit);
    locals || stores
}

/// The last boundary at each still reachable depth of a straight-line run.
/// A multi-result instruction can skip depths, so store only the boundaries
/// that actually occurred. Each instruction adds at most one boundary.
#[derive(Default)]
struct StackStarts {
    depth: usize,
    starts: Vec<(usize, StackBoundary)>,
}

#[derive(Clone, Copy)]
struct StackBoundary {
    instruction: usize,
    /// Original position of the first instruction of the run. Only pure
    /// pushes move, so this remains the cut for earlier writes.
    before: usize,
}

impl StackStarts {
    fn advance(&mut self, i: usize, effect: Option<(usize, usize)>, visit: &mut impl FnMut()) {
        let Some((pops, pushes)) = effect else {
            self.depth = 0;
            self.starts.clear();
            return;
        };
        let below = self.depth.saturating_sub(pops);
        if pops > self.depth {
            self.starts.clear();
        }
        self.depth = below + pushes;
        while self
            .starts
            .last()
            .is_some_and(|&(d, _)| d > below || d == self.depth)
        {
            visit();
            self.starts.pop();
        }
        if pops == 0 && pushes != 0 {
            self.starts.push((
                below,
                StackBoundary {
                    instruction: i,
                    before: i,
                },
            ));
        }
    }

    fn single_result(&self) -> Option<StackBoundary> {
        let &(depth, start) = self.starts.last()?;
        (depth + 1 == self.depth).then_some(start)
    }

    fn moved_push(&mut self, at: usize, start: StackBoundary) {
        if let Some((_, boundary)) = self.starts.last_mut() {
            boundary.instruction = at;
        }
        self.starts.push((self.depth, start));
        self.depth += 1;
    }
}

/// Instruction indices stay stable until all edits are committed. The
/// sentinel is both the predecessor of the first and successor of the last.
struct ScheduleOrder {
    prev: Vec<usize>,
    next: Vec<usize>,
}

impl ScheduleOrder {
    fn new(n: usize, visit: &mut impl FnMut()) -> Self {
        let (prev, next) = (0..=n)
            .map(|i| {
                visit();
                (
                    i.checked_sub(1).unwrap_or(n),
                    if i == n { 0 } else { i + 1 },
                )
            })
            .unzip();
        Self { prev, next }
    }

    fn remove(&mut self, i: usize) {
        let (prev, next) = (self.prev[i], self.next[i]);
        self.next[prev] = next;
        self.prev[next] = prev;
    }

    fn move_before(&mut self, i: usize, before: usize) {
        self.remove(i);
        let prev = self.prev[before];
        self.prev[i] = prev;
        self.next[i] = before;
        self.next[prev] = i;
        self.prev[before] = i;
    }

    fn finish(self, f: &mut Func, visit: &mut impl FnMut()) {
        let n = f.body.len();
        let mut out = Vec::with_capacity(n);
        let mut origins = Remap::new(f.origins.as_deref());
        let mut i = self.next[n];
        while i != n {
            visit();
            out.push(f.body[i]);
            origins.copy(i..i + 1);
            i = self.next[i];
        }
        f.body = out;
        origins.finish(f);
    }
}

fn schedule_locals(f: &mut Func, np: u32, arity: &Arity, visit: &mut impl FnMut()) -> bool {
    let (reads, writes) = counts_with(f, visit);
    let n = f.body.len();
    let mut stack = StackStarts::default();
    let mut local_write = vec![None; reads.len()];
    let mut global_write = BTreeMap::new();
    let mut call = None;
    let mut order = None;
    let mut i = 0;
    while i < n {
        visit();
        if let [Ins::Op(I::LocalSet(a)), push, Ins::Op(I::LocalGet(g)), ..] = &f.body[i..]
            && a == g
            && *a >= np
            && reads[*a as usize] == 1
            && writes[*a as usize] == 1
            && let Some(start) = stack.single_result()
        {
            let last_write = match push {
                Ins::Op(I::LocalGet(l)) => Some(local_write[*l as usize]),
                Ins::GlobalGet(g) => Some(global_write.get(g).copied().max(call)),
                Ins::Op(I::I32Const(_) | I::I64Const(_)) => Some(None),
                _ => None,
            };
            if last_write.is_some_and(|last| last.is_none_or(|at| at < start.before)) {
                let order = order.get_or_insert_with(|| ScheduleOrder::new(n, visit));
                order.move_before(i + 1, start.instruction);
                order.remove(i);
                order.remove(i + 2);
                stack.moved_push(i + 1, start);
                i += 3;
                continue;
            }
        }
        if let Ins::Op(I::LocalSet(l) | I::LocalTee(l)) = f.body[i] {
            local_write[l as usize] = Some(i);
        } else if let Ins::GlobalSet(g) = f.body[i] {
            global_write.insert(g, i);
        } else if matches!(f.body[i], Ins::Call(_) | Ins::CallIndirect(_)) {
            call = Some(i);
        }
        stack.advance(i, effect(&f.body[i], arity, &f.symbols), visit);
        i += 1;
    }
    let Some(order) = order else {
        return false;
    };
    order.finish(f, visit);
    true
}

#[derive(Clone, Copy)]
struct StoreOffset {
    at: usize,
    addend: u32,
    parent: Option<usize>,
}

fn schedule_stores(f: &mut Func, arity: &Arity, visit: &mut impl FnMut()) -> bool {
    let mut depth = 0usize;
    let mut active = Vec::<(usize, usize)>::new();
    let mut offsets = Vec::<StoreOffset>::new();
    let mut order = None;
    for i in 0..f.body.len() {
        visit();
        let Some((pops, pushes)) = effect(&f.body[i], arity, &f.symbols) else {
            depth = 0;
            active.clear();
            continue;
        };
        // An offset candidate lives at the address's depth until the
        // first consumer reaches it. One intervening value above the
        // address makes that consumer eligible when it is a store.
        // Markers have distinct increasing stack depths. Only the top
        // value's marker can stand above the address marker, so a two-pop
        // instruction needs at most the last two markers.
        let address = if pops == 2 {
            active
                .iter()
                .rev()
                .take(2)
                .find_map(|&(d, id)| (d + 1 == depth).then_some(id))
        } else {
            None
        };
        let below = depth.saturating_sub(pops);
        while active.last().is_some_and(|&(d, _)| d > below) {
            visit();
            active.pop();
        }
        depth = below + pushes;
        if let (Some(Ins::Op(I::I32Const(c))), Ins::Op(I::I32Add)) =
            (i.checked_sub(1).and_then(|p| f.body.get(p)), f.body[i])
            && *c >= 0
        {
            active.push((depth, offsets.len()));
            offsets.push(StoreOffset {
                at: i - 1,
                addend: c.cast_unsigned(),
                parent: address,
            });
        } else if let Ins::Op(mut op) = f.body[i]
            && !is_load(&op)
            && with_offset(&op, 0).is_some()
        {
            let mut address = address;
            while let Some(id) = address {
                visit();
                let StoreOffset { at, addend, parent } = offsets[id];
                let Some(store) = with_offset(&op, addend) else {
                    break;
                };
                op = store;
                let order = order.get_or_insert_with(|| ScheduleOrder::new(f.body.len(), visit));
                order.remove(at);
                order.remove(at + 1);
                if let Some(origins) = &mut f.origins {
                    origins[i] = origins[at].or(origins[at + 1]).or(origins[i]);
                }
                address = parent;
            }
            f.body[i] = Ins::Op(op);
        }
    }
    let Some(order) = order else {
        return false;
    };
    order.finish(f, visit);
    true
}

/// What straight-line instruction `i` pops and pushes; `None` for control
/// and for anything this table does not know.
fn effect(i: &Ins, arity: &Arity, symbols: &Symbols) -> Option<(usize, usize)> {
    Some(match i {
        Ins::GlobalGet(_) | Ins::TableSlot(_) | Ins::Glue(_) | Ins::VTable(_) => (0, 1),
        Ins::Descriptor(..) => (0, 1),
        Ins::Anchor(_) => (0, 0),
        Ins::GlobalSet(_) => (1, 0),
        Ins::Call(id) => match &*symbols.get(*id) {
            Sym::Import(im) => (im.params.len(), im.results.len()),
            _ => (*arity.get(id.idx())?)?,
        },
        Ins::CallIndirect(id) => {
            let signature = symbols.signature(*id);
            (signature.0.len() + 1, signature.1.len())
        }
        Ins::Op(op) => match op {
            I::LocalGet(_) | I::I32Const(_) | I::I64Const(_) | I::F32Const(_) | I::F64Const(_) => {
                (0, 1)
            }
            I::LocalTee(_) => (1, 1),
            I::LocalSet(_) | I::Drop => (1, 0),
            I::Select => (3, 1),
            I::I32Eqz
            | I::I64Eqz
            | I::I32WrapI64
            | I::I64ExtendI32S
            | I::I64ExtendI32U
            | I::I32Clz
            | I::I64Clz
            | I::I32Ctz
            | I::I64Ctz
            | I::I32Popcnt
            | I::I64Popcnt
            | I::I32Extend8S
            | I::I32Extend16S
            | I::I32ReinterpretF32
            | I::I64ReinterpretF64
            | I::F32ReinterpretI32
            | I::F64ReinterpretI64
            | I::I32TruncSatF32S
            | I::I32TruncSatF32U
            | I::I32TruncSatF64S
            | I::I32TruncSatF64U
            | I::I64TruncSatF32S
            | I::I64TruncSatF32U
            | I::I64TruncSatF64S
            | I::I64TruncSatF64U
            | I::F32ConvertI32S
            | I::F32ConvertI32U
            | I::F32ConvertI64S
            | I::F32ConvertI64U
            | I::F64ConvertI32S
            | I::F64ConvertI32U
            | I::F64ConvertI64S
            | I::F64ConvertI64U
            | I::F32DemoteF64
            | I::F64PromoteF32
            | I::F32Neg
            | I::F64Neg
            | I::F32Abs
            | I::F64Abs
            | I::F32Trunc
            | I::F64Trunc
            | I::F32Floor
            | I::F64Floor
            | I::F32Ceil
            | I::F64Ceil
            | I::F32Nearest
            | I::F64Nearest
            | I::F32Sqrt
            | I::F64Sqrt
            | I::MemoryGrow(_) => (1, 1),
            I::MemorySize(_) => (0, 1),
            I::MemoryCopy { .. } | I::MemoryFill(_) => (3, 0),
            I::I32Add
            | I::I32Sub
            | I::I32Mul
            | I::I32And
            | I::I32Or
            | I::I32Xor
            | I::I32Shl
            | I::I32ShrU
            | I::I32ShrS
            | I::I32Eq
            | I::I32Ne
            | I::I32LtU
            | I::I32LtS
            | I::I32GtU
            | I::I32GtS
            | I::I32LeU
            | I::I32LeS
            | I::I32GeU
            | I::I32GeS
            | I::I64Add
            | I::I64Sub
            | I::I64Mul
            | I::I64And
            | I::I64Or
            | I::I64Xor
            | I::I64Shl
            | I::I64ShrU
            | I::I64ShrS
            | I::I64Eq
            | I::I64Ne
            | I::I64LtU
            | I::I64LtS
            | I::I64GtU
            | I::I64GtS
            | I::I64LeU
            | I::I64LeS
            | I::I64GeU
            | I::I64GeS
            | I::I32DivS
            | I::I32DivU
            | I::I32RemS
            | I::I32RemU
            | I::I64DivS
            | I::I64DivU
            | I::I64RemS
            | I::I64RemU
            | I::F32Add
            | I::F32Sub
            | I::F32Mul
            | I::F32Div
            | I::F32Copysign
            | I::F32Eq
            | I::F32Ne
            | I::F32Lt
            | I::F32Gt
            | I::F32Le
            | I::F32Ge
            | I::F64Add
            | I::F64Sub
            | I::F64Mul
            | I::F64Div
            | I::F64Copysign
            | I::F64Eq
            | I::F64Ne
            | I::F64Lt
            | I::F64Gt
            | I::F64Le
            | I::F64Ge => (2, 1),
            other if is_load(other) => (1, 1),
            other if with_offset(other, 0).is_some() => (2, 0),
            _ => return None,
        },
        _ => return None,
    })
}

/// The `end` of the block that instruction `at` of `b` opens, when it has
/// no `else`.
fn block_end(b: &[Ins], at: usize) -> Option<usize> {
    let mut d = 0u32;
    for (k, i) in b.iter().enumerate().skip(at) {
        match i {
            Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => d += 1,
            Ins::Op(I::Else) if d == 1 => return None,
            Ins::Op(I::End) => {
                d -= 1;
                if d == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// Rule 17: each `br` to the function's exit block, after the sets of the
/// locals the exit pushes, returns those values from the stack.
fn exits(f: &mut Func) -> bool {
    let b = &f.body;
    let Some(end) = b
        .iter()
        .rposition(|i| !matches!(i, Ins::Op(I::LocalGet(_))))
    else {
        return false;
    };
    if !matches!(b[end], Ins::Op(I::End)) {
        return false;
    }
    let rets: Vec<u32> = b[end + 1..]
        .iter()
        .filter_map(|i| match i {
            Ins::Op(I::LocalGet(l)) => Some(*l),
            _ => None,
        })
        .collect();
    let mut distinct = rets.clone();
    distinct.sort_stable();
    distinct.dedup();
    if distinct.len() != rets.len() {
        return false;
    }
    let mut depth = 0u32;
    let mut open = None;
    for at in (0..end).rev() {
        match b[at] {
            Ins::Op(I::End) => depth += 1,
            Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) if depth > 0 => depth -= 1,
            Ins::Op(I::Block(wasm_encoder::BlockType::Empty)) => {
                open = Some(at);
                break;
            }
            Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => return false,
            _ => {}
        }
    }
    let Some(open) = open else {
        return false;
    };
    let n = rets.len();
    let mut gone = vec![false; b.len()];
    let mut returns = Vec::new();
    let mut depth = 0u32;
    for at in open + 1..end {
        match b[at] {
            Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => depth += 1,
            Ins::Op(I::End) => depth -= 1,
            Ins::Op(I::Br(d))
                if d == depth
                    && at >= n
                    && (0..n).all(|j| {
                        !gone[at - 1 - j]
                            && matches!(b[at - 1 - j], Ins::Op(I::LocalSet(l)) if l == rets[j])
                    }) =>
            {
                gone[at - n..at].fill(true);
                returns.push(at);
            }
            _ => {}
        }
    }
    if returns.is_empty() {
        return false;
    }
    let mut origins = Remap::new(f.origins.as_deref());
    let mut out = Vec::with_capacity(b.len());
    for (at, ins) in b.iter().enumerate() {
        if gone[at] {
            continue;
        }
        out.push(if returns.binary_search(&at).is_ok() {
            Ins::Op(I::Return)
        } else {
            *ins
        });
        origins.copy(at..at + 1);
    }
    f.body = out;
    origins.finish(f);
    true
}

/// Whether memory instruction `i` loads.
fn is_load(i: &I) -> bool {
    matches!(
        i,
        I::I32Load(_)
            | I::I64Load(_)
            | I::F32Load(_)
            | I::F64Load(_)
            | I::I32Load8S(_)
            | I::I32Load8U(_)
            | I::I32Load16S(_)
            | I::I32Load16U(_)
    )
}

/// Load or store `i` with `by` more on its offset; `None` for any other
/// instruction, or an offset past 32 bits.
fn with_offset(i: &I, by: u32) -> Option<I> {
    let bump = |m: &wasm_encoder::MemArg| {
        let offset = m
            .offset
            .checked_add(u64::from(by))
            .filter(|o| u32::try_from(*o).is_ok())?;
        Some(wasm_encoder::MemArg { offset, ..*m })
    };
    Some(match i {
        I::I32Load(m) => I::I32Load(bump(m)?),
        I::I64Load(m) => I::I64Load(bump(m)?),
        I::F32Load(m) => I::F32Load(bump(m)?),
        I::F64Load(m) => I::F64Load(bump(m)?),
        I::I32Load8S(m) => I::I32Load8S(bump(m)?),
        I::I32Load8U(m) => I::I32Load8U(bump(m)?),
        I::I32Load16S(m) => I::I32Load16S(bump(m)?),
        I::I32Load16U(m) => I::I32Load16U(bump(m)?),
        I::I32Store(m) => I::I32Store(bump(m)?),
        I::I64Store(m) => I::I64Store(bump(m)?),
        I::F32Store(m) => I::F32Store(bump(m)?),
        I::F64Store(m) => I::F64Store(bump(m)?),
        I::I32Store8(m) => I::I32Store8(bump(m)?),
        I::I32Store16(m) => I::I32Store16(bump(m)?),
        _ => return None,
    })
}

/// Remove control instructions that only fall through (rule 18), then unused void
/// labels, after the passes that can empty their bodies. A direct
/// stack-polymorphic instruction keeps its validation boundary. The walks
/// cost O(N + T) for N instructions and T branch-table targets.
pub fn blocks(f: &mut Func, arity: &Arity) -> bool {
    blocks_with(f, arity, || {})
}

/// One open label of [`no_op_controls`]: the operand height under it, the
/// values its end leaves, what opened it, and whether the code after the
/// last stack-polymorphic instruction in it is unreachable.
struct HeightFrame {
    base: usize,
    results: usize,
    opener: LabelKind,
    unreachable: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LabelKind {
    Block,
    Loop,
    If,
}

/// Rule 18: control instructions that only fall through.
///
/// A `br 0` straight before the `end` or `else` of a block or an `if`
/// jumps where falling through goes. It goes when the stack holds no more
/// than the label's values there: exactly them in reachable code, and in
/// unreachable code the polymorphic stack supplies the rest. A `br 0` that
/// discards values under them, and a loop's back edge, stay. An `else`
/// straight before the `end` of a void `if` goes too.
///
/// One forward walk, O(N); a body with an instruction whose stack effect
/// this table does not know keeps every instruction.
fn no_op_controls(f: &Func, arity: &Arity, visit: &mut impl FnMut()) -> Vec<usize> {
    let block_results = |t: &wasm_encoder::BlockType| match t {
        wasm_encoder::BlockType::Empty => Some(0),
        wasm_encoder::BlockType::Result(_) => Some(1),
        wasm_encoder::BlockType::FunctionType(_) => None,
    };
    // Pops `n` operands; an unreachable label's stack is polymorphic.
    let pop = |height: &mut usize, frame: &HeightFrame, n: usize| {
        let remaining = height.checked_sub(n).filter(|h| *h >= frame.base);
        *height = if frame.unreachable {
            remaining.unwrap_or(frame.base)
        } else {
            // ruled out: Func contains typed, balanced Wasm operations;
            // unknown effects and block signatures leave the walk before
            // this pop. A reachable known-effect operation cannot underflow.
            crate::emit::required(remaining, "known stack effects fit their control frame")
        };
    };
    let mut walk = || -> Option<Vec<usize>> {
        // The function's own label: a `br 0` to it returns.
        let mut frames = vec![HeightFrame {
            base: 0,
            results: f.results.len(),
            opener: LabelKind::Loop,
            unreachable: false,
        }];
        let mut height = 0usize;
        let mut found = Vec::new();
        for (at, ins) in f.body.iter().enumerate() {
            visit();
            let top = frames.last_mut()?;
            let next_ends = matches!(f.body.get(at + 1), Some(Ins::Op(I::End)));
            match ins {
                Ins::Op(op @ (I::Block(t) | I::Loop(t) | I::If(t))) => {
                    let opener = match op {
                        I::Block(_) => LabelKind::Block,
                        I::Loop(_) => LabelKind::Loop,
                        _ => {
                            pop(&mut height, top, 1);
                            LabelKind::If
                        }
                    };
                    frames.push(HeightFrame {
                        base: height,
                        results: block_results(t)?,
                        opener,
                        unreachable: false,
                    });
                }
                Ins::Op(I::Else) => {
                    if next_ends && top.opener == LabelKind::If && top.results == 0 {
                        found.push(at);
                    }
                    height = top.base;
                    top.unreachable = false;
                }
                Ins::Op(I::End) => {
                    // ruled out: Func.body stores only nested End operators;
                    // module serialization appends the function's final End.
                    let done = crate::emit::required(
                        frames.pop().filter(|_| !frames.is_empty()),
                        "End closes a nested control frame",
                    );
                    height = done.base + done.results;
                }
                Ins::Op(I::Br(depth)) => {
                    let falls = *depth == 0
                        && top.opener != LabelKind::Loop
                        && matches!(f.body.get(at + 1), Some(Ins::Op(I::End | I::Else)))
                        && height <= top.base + top.results;
                    if falls {
                        found.push(at);
                    } else {
                        height = top.base;
                        top.unreachable = true;
                    }
                }
                Ins::Op(I::BrIf(_)) => pop(&mut height, top, 1),
                Ins::Op(I::BrTable(..) | I::Return | I::Unreachable) => {
                    height = top.base;
                    top.unreachable = true;
                }
                Ins::Static(_) => {
                    height += 1;
                }
                Ins::DrainGlue(_) => {}
                other => {
                    let (pops, pushes) = effect(other, arity, &f.symbols)?;
                    pop(&mut height, top, pops);
                    height += pushes;
                }
            }
        }
        (frames.len() == 1).then_some(found)
    };
    walk().unwrap_or_default()
}

struct BlockFrame {
    keep: bool,
    kept_depth: u32,
}

fn branch_frame(stack: &[usize], depth: u32) -> Option<usize> {
    let offset = (depth as usize).checked_add(1)?;
    stack.get(stack.len().checked_sub(offset)?).copied()
}

/// Collect labels before editing, including the implicit function label.
/// Invalid control depths leave the body for ordinary Wasm validation.
fn block_frames(f: &Func, visit: &mut impl FnMut()) -> Option<(Vec<BlockFrame>, Vec<usize>)> {
    let mut frames = vec![BlockFrame {
        keep: true,
        kept_depth: 1,
    }];
    let mut ids = vec![0; f.body.len()];
    let mut stack = vec![0];
    for (i, ins) in f.body.iter().enumerate() {
        visit();
        let barrier = match ins {
            Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => {
                let id = frames.len();
                ids[i] = id;
                frames.push(BlockFrame {
                    keep: !matches!(ins, Ins::Op(I::Block(wasm_encoder::BlockType::Empty))),
                    kept_depth: 0,
                });
                stack.push(id);
                false
            }
            Ins::Op(I::End) if stack.len() > 1 => {
                ids[i] = stack.pop()?;
                false
            }
            Ins::Op(I::Br(depth) | I::BrIf(depth)) => {
                frames[branch_frame(&stack, *depth)?].keep = true;
                matches!(ins, Ins::Op(I::Br(_)))
            }
            Ins::Op(I::BrTable(table, default)) => {
                for &depth in f.label_list(*table).iter().chain(std::iter::once(default)) {
                    visit();
                    frames[branch_frame(&stack, depth)?].keep = true;
                }
                true
            }
            Ins::Op(I::Return | I::Unreachable) => true,
            _ => false,
        };
        if barrier {
            frames[*stack.last()?].keep = true;
        }
    }
    (stack.len() == 1).then_some((frames, ids))
}

fn blocks_with(f: &mut Func, arity: &Arity, mut visit: impl FnMut()) -> bool {
    let falls = no_op_controls(f, arity, &mut visit);
    if !falls.is_empty() {
        let mut origins = Remap::new(f.origins.as_deref());
        let body = std::mem::take(&mut f.body);
        let mut out = Vec::with_capacity(body.len() - falls.len());
        let mut next = falls.iter().copied().peekable();
        for (at, ins) in body.into_iter().enumerate() {
            if next.next_if_eq(&at).is_none() {
                out.push(ins);
                origins.copy(at..at + 1);
            }
        }
        f.body = out;
        origins.finish(f);
    }
    let Some((mut frames, ids)) = block_frames(f, &mut visit) else {
        return !falls.is_empty();
    };
    if frames.iter().all(|frame| frame.keep) {
        return !falls.is_empty();
    }
    let mut origins = Remap::new(f.origins.as_deref());
    let body = std::mem::take(&mut f.body);
    let mut out = Vec::with_capacity(body.len());
    let mut stack = vec![0];
    let mut kept = 1;
    for (i, ins) in body.into_iter().enumerate() {
        visit();
        // The first walk validated every depth and kept every target.
        let depth = |d: u32, frames: &[BlockFrame]| {
            let target = stack[stack.len() - 1 - d as usize];
            kept - frames[target].kept_depth
        };
        let rewritten = match ins {
            Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => {
                let id = ids[i];
                stack.push(id);
                kept += u32::from(frames[id].keep);
                frames[id].kept_depth = kept;
                frames[id].keep.then_some(ins)
            }
            Ins::Op(I::End) if stack.len() > 1 => {
                let id = ids[i];
                stack.pop();
                kept -= u32::from(frames[id].keep);
                frames[id].keep.then_some(ins)
            }
            Ins::Op(I::Br(d)) => Some(Ins::Op(I::Br(depth(d, &frames)))),
            Ins::Op(I::BrIf(d)) => Some(Ins::Op(I::BrIf(depth(d, &frames)))),
            Ins::Op(I::BrTable(table, default)) => {
                let targets = f
                    .label_list(table)
                    .iter()
                    .map(|&d| {
                        visit();
                        depth(d, &frames)
                    })
                    .collect();
                let default = depth(default, &frames);
                Some(Ins::Op(I::BrTable(f.intern_labels(targets), default)))
            }
            _ => Some(ins),
        };
        if let Some(ins) = rewritten {
            out.push(ins);
            origins.copy(i..i + 1);
        }
    }
    f.body = out;
    origins.finish(f);
    true
}

/// Rules 5, 6 and 19: rewrites each `if` of those shapes. Each round reads
/// the body's `if`s once and rewrites every site of one rule in one
/// rebuild: rule 5's branch-only arms, then rule 6, then rule 19
/// ([`chains::run`]). A rewrite can make a new site (the inner `&&` of `a
/// && b && c` feeds the branch once the outer one does), so rounds repeat
/// until none rewrites.
///
/// Rule 5 needs no other form: [`branch_shapes::run`] takes every site no
/// branch targets, and in one that a branch targets, the branch is in the
/// kept arm (the other is a lone `br` out of the `if`), which no rewrite
/// that drops the `if` keeps.
fn shapes(f: &mut Func, np: u32, arity: &Arity) -> bool {
    let mut changed = false;
    loop {
        let sites = ifs(&f.body);
        if branch_shapes::run(f, &sites)
            || branch_shapes::value_arms(f, np, &sites)
            || chains::run(f, &sites, arity)
        {
            changed = true;
            continue;
        }
        break;
    }
    changed
}

/// K of a short-circuit value's `else` arm `i32.const K`, K 0 or 1.
fn short_circuit(arm: &[Ins]) -> Option<u32> {
    match arm {
        [Ins::Op(I::I32Const(k @ (0 | 1)))] => Some(k.cast_unsigned()),
        _ => None,
    }
}

/// How far [`operand_start`] walks back.
const OPERAND_REACH: usize = 128;

/// The start of the run of instructions before `at`, in `at`'s block,
/// that pushes the one operand `at` pops and reaches no stack value
/// below: the place a block can open around both. Nested blocks count as
/// one instruction each; a branch at the run's own level ends the walk.
fn operand_start(b: &[Ins], at: usize, arity: &Arity, symbols: &Symbols) -> Option<usize> {
    use wasm_encoder::BlockType;
    let results = |bt: &BlockType| match bt {
        BlockType::Empty => Some(0),
        BlockType::Result(_) => Some(1),
        BlockType::FunctionType(_) => None,
    };
    let mut need = 1usize;
    let mut k = at;
    let mut steps = 0;
    while need > 0 {
        steps += 1;
        if steps > OPERAND_REACH {
            return None;
        }
        k = k.checked_sub(1)?;
        let (pops, pushes) = match &b[k] {
            Ins::Op(I::End) => {
                // Back to the block's start, an instruction of `results`;
                // an `if` pops its test too.
                let mut nested = 0u32;
                loop {
                    steps += 1;
                    if steps > OPERAND_REACH {
                        return None;
                    }
                    k = k.checked_sub(1)?;
                    if let Ins::Op(I::End) = b[k] {
                        nested += 1;
                    } else if let Ins::Op(I::Block(bt) | I::Loop(bt) | I::If(bt)) = b[k] {
                        if nested == 0 {
                            let test = usize::from(matches!(b[k], Ins::Op(I::If(_))));
                            break (test, results(&bt)?);
                        }
                        nested -= 1;
                    }
                }
            }
            i => effect(i, arity, symbols)?,
        };
        if pushes > need {
            return None;
        }
        need = need - pushes + pops;
    }
    Some(k)
}

/// Each `if` of `b`: its own index, its `else`'s and its `end`'s.
fn ifs(b: &[Ins]) -> Vec<(usize, Option<usize>, usize)> {
    let mut open: Vec<(usize, bool, Option<usize>)> = Vec::new();
    let mut out = Vec::new();
    for (k, i) in b.iter().enumerate() {
        crate::work::control(1);
        match i {
            Ins::Op(I::If(_)) => open.push((k, true, None)),
            Ins::Op(I::Block(_) | I::Loop(_)) => open.push((k, false, None)),
            Ins::Op(I::Else) => {
                if let Some(top) = open.last_mut() {
                    top.2 = Some(k);
                }
            }
            Ins::Op(I::End) => {
                if let Some((at, is_if, els)) = open.pop()
                    && is_if
                {
                    out.push((at, els, k));
                }
            }
            _ => {}
        }
    }
    out
}

/// Instructions `r` moved into `by` more blocks, or out of `-by` blocks:
/// each branch out of `r` names `by` labels more. `None` when `r`
/// branches to a block it moves out of.
fn shifted(r: &[Ins], by: i32, labels: &mut Vec<Vec<u32>>) -> Option<Vec<Ins>> {
    let mut d = 0u32;
    let mut out = Vec::with_capacity(r.len());
    for i in r {
        // A branch to a block the run moves out of has no target.
        let moved = |l: u32| match l.checked_add_signed(by) {
            _ if l < d => Some(l),
            Some(t) if t >= d => Some(t),
            _ => None,
        };
        out.push(match i {
            Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => {
                d += 1;
                *i
            }
            Ins::Op(I::End) => {
                d = d.saturating_sub(1);
                *i
            }
            Ins::Op(I::Br(l)) => Ins::Op(I::Br(moved(*l)?)),
            Ins::Op(I::BrIf(l)) => Ins::Op(I::BrIf(moved(*l)?)),
            Ins::Op(I::BrTable(ls, def)) => {
                let targets = labels[ls.0 as usize]
                    .iter()
                    .map(|l| moved(*l))
                    .collect::<Option<_>>()?;
                let id = LabelListId(u32::try_from(labels.len()).expect("branch tables"));
                labels.push(targets);
                Ins::Op(I::BrTable(id, moved(*def)?))
            }
            other => *other,
        });
    }
    Some(out)
}

/// Whether `i` may change what pure push `p` pushes.
fn clobbers(p: &Ins, i: &Ins) -> bool {
    match (p, i) {
        (Ins::Op(I::LocalGet(a)), Ins::Op(I::LocalSet(b) | I::LocalTee(b))) => a == b,
        (Ins::GlobalGet(g), Ins::GlobalSet(h)) => g == h,
        (Ins::GlobalGet(_), Ins::Call(_) | Ins::CallIndirect(..)) => true,
        _ => false,
    }
}

/// Whether `i` ends a straight-line run: a branch, or a block's edge.
fn ends_run(i: &Ins) -> bool {
    matches!(
        i,
        Ins::Op(
            I::Block(_)
                | I::Loop(_)
                | I::If(_)
                | I::Else
                | I::End
                | I::Br(_)
                | I::BrIf(_)
                | I::BrTable(..)
                | I::Return
                | I::Unreachable
        )
    )
}

/// Each local's reads and writes; a `local.tee` is a write.
fn counts(f: &Func) -> (Vec<u32>, Vec<u32>) {
    counts_with(f, &mut || {})
}

fn counts_with(f: &Func, visit: &mut impl FnMut()) -> (Vec<u32>, Vec<u32>) {
    let n = f.params.len() + f.locals.len();
    let (mut reads, mut writes) = (vec![0u32; n], vec![0u32; n]);
    for i in &f.body {
        visit();
        match i {
            Ins::Op(I::LocalGet(l)) => reads[*l as usize] += 1,
            Ins::Op(I::LocalSet(l) | I::LocalTee(l)) => writes[*l as usize] += 1,
            _ => {}
        }
    }
    (reads, writes)
}

/// Drops the locals past the parameters that no instruction names, and
/// numbers the others again in order.
pub(crate) fn renumber(f: &mut Func) {
    let np = f.params.len();
    let mut used = vec![false; f.locals.len()];
    for i in &f.body {
        if let Ins::Op(I::LocalGet(l) | I::LocalSet(l) | I::LocalTee(l)) = i
            && let Some(at) = (*l as usize).checked_sub(np)
        {
            used[at] = true;
        }
    }
    let mut map = Vec::with_capacity(f.locals.len());
    let mut kept = Vec::new();
    for (j, v) in f.locals.iter().enumerate() {
        map.push(u32::try_from(np + kept.len()).expect("locals"));
        if used[j] {
            kept.push(*v);
        }
    }
    let to = |l: u32| -> u32 {
        match (l as usize).checked_sub(np) {
            Some(at) => map[at],
            None => l,
        }
    };
    for i in &mut f.body {
        match i {
            Ins::Op(I::LocalGet(l)) => *l = to(*l),
            Ins::Op(I::LocalSet(l)) => *l = to(*l),
            Ins::Op(I::LocalTee(l)) => *l = to(*l),
            _ => {}
        }
    }
    if let Some(names) = &mut f.local_names {
        *names = hd_base::btree_map(
            std::mem::take(names)
                .into_iter()
                .filter(|(l, _)| (*l as usize).checked_sub(np).is_none_or(|at| used[at]))
                .map(|(l, n)| (to(l), n)),
        );
    }
    f.locals = kept;
}

/// Rule 16: removes a count whose every operand is a constant (#910). A
/// constant address is null or static data, which has no count: neither
/// the page-table path (`address < heap`) nor the known path (a count of
/// 0) changes it, in any runtime. An operand is a constant when it comes
/// from a constant push, possibly through tees, or from a local whose
/// every write stores such a value: such a local holds only constants
/// and its initial 0. A local proven from itself never qualifies, so a
/// loop counter stays unproven. A void call drops each operand; a
/// collection copy leaves its input as its result. Gives whether
/// anything changed.
pub fn static_counts(f: &mut Func, retain: Hash128, arity: &Arity, static_params: &[bool]) -> bool {
    let rewrite = |ins: &Ins| -> Option<(usize, usize)> {
        let Ins::Call(id) = ins else {
            return None;
        };
        let (operands, results) = (*arity.get(id.idx())?)?;
        match (&*f.symbols.get(*id), operands, results) {
            (Sym::Helper(Helper::CollCopy), 1, 1) => Some((1, 0)),
            (Sym::Inst(k), n, 0) if *k == retain => Some((n, n)),
            (
                Sym::Helper(
                    Helper::ReleaseRef
                    | Helper::RetainKnown
                    | Helper::ReleaseKnown
                    | Helper::Retain { .. }
                    | Helper::Release { .. },
                ),
                n,
                0,
            ) => Some((n, n)),
            _ => None,
        }
    };
    if !f.body.iter().any(|i| rewrite(i).is_some()) {
        return false;
    }
    let joins = constant_joins(&f.body);
    let fixed = fixed_locals(f, static_params, &[], arity, &joins);
    let gone: Vec<(usize, usize)> = f
        .body
        .iter()
        .enumerate()
        .filter_map(|(k, ins)| {
            let (operands, drops) = rewrite(ins)?;
            (0..operands)
                .all(|d| constant_before(f, k, d, &fixed, &[], arity, &joins))
                .then_some((k, drops))
        })
        .collect();
    if gone.is_empty() {
        return false;
    }
    let mut origins = Remap::new(f.origins.as_deref());
    let body = std::mem::take(&mut f.body);
    let mut next = gone.iter().peekable();
    for (k, ins) in body.into_iter().enumerate() {
        if let Some(&&(at, n)) = next.peek()
            && at == k
        {
            next.next();
            f.body.extend(std::iter::repeat_n(Ins::Op(I::Drop), n));
            origins.merged(k..k + 1, n);
        } else {
            f.body.push(ins);
            origins.copy(k..k + 1);
        }
    }
    origins.finish(f);
    true
}

/// The instruction and its result index that produced the value `depth`
/// places below the stack top before `at`, within one straight-line run.
const PRODUCER_REACH: usize = 128;

fn producer(
    body: &[Ins],
    at: usize,
    depth: usize,
    arity: &Arity,
    symbols: &Symbols,
) -> Option<(usize, usize)> {
    let mut above = depth;
    for j in (at.saturating_sub(PRODUCER_REACH)..at).rev() {
        let (pops, pushes) = effect(&body[j], arity, symbols)?;
        if above >= pushes {
            above = above - pushes + pops;
        } else if matches!(body[j], Ins::Op(I::LocalTee(_))) {
            if above != 0 {
                return None;
            }
            above = 0;
        } else {
            return Some((j, pushes - 1 - above));
        }
    }
    None
}

fn constant_at(ins: &Ins, result: usize, fixed: &[bool], results: &[Vec<bool>]) -> bool {
    match ins {
        Ins::Op(I::I32Const(_) | I::I64Const(_) | I::F32Const(_) | I::F64Const(_))
        | Ins::Static(_)
        | Ins::TableSlot(_)
        | Ins::Glue(_)
        | Ins::VTable(_) => result == 0,
        Ins::Op(I::LocalGet(l)) => result == 0 && fixed[*l as usize],
        Ins::Call(id) => results
            .get(id.idx())
            .and_then(|r| r.get(result))
            .copied()
            .unwrap_or(false),
        _ => false,
    }
}

// A scalar if with an else and no branch/return into or out of its arms
// yields one of those arms' values. Loops and arbitrary block exits remain
// unproved. The map records the else instruction at each qualifying end.
fn constant_joins(body: &[Ins]) -> Vec<Option<usize>> {
    use wasm_encoder::BlockType;
    if !body
        .iter()
        .any(|i| matches!(i, Ins::Op(I::If(BlockType::Result(_)))))
    {
        return Vec::new();
    }
    let mut joins = vec![None; body.len()];
    let mut stack = Vec::new();
    let mut exits = 0usize;
    for (at, ins) in body.iter().enumerate() {
        match ins {
            Ins::Op(I::If(bt)) => stack.push((matches!(bt, BlockType::Result(_)), None, exits)),
            Ins::Op(I::Block(_) | I::Loop(_)) => stack.push((false, None, exits)),
            Ins::Op(I::Else) => {
                if let Some((_, otherwise, _)) = stack.last_mut() {
                    *otherwise = Some(at);
                }
            }
            Ins::Op(I::End) => {
                if let Some((true, Some(otherwise), before)) = stack.pop()
                    && before == exits
                {
                    joins[at] = Some(otherwise);
                }
            }
            Ins::Op(I::Br(_) | I::BrIf(_) | I::BrTable(..) | I::Return | I::Unreachable) => {
                exits += 1;
            }
            _ => {}
        }
    }
    joins
}

fn constant_before(
    f: &Func,
    at: usize,
    depth: usize,
    fixed: &[bool],
    results: &[Vec<bool>],
    arity: &Arity,
    joins: &[Option<usize>],
) -> bool {
    if let Some((p, result)) = producer(&f.body, at, depth, arity, &f.symbols) {
        return constant_at(&f.body[p], result, fixed, results);
    }
    if joins.is_empty() {
        return false;
    }
    let mut work = vec![(at, depth)];
    while let Some((at, depth)) = work.pop() {
        if let Some((p, result)) = producer(&f.body, at, depth, arity, &f.symbols) {
            if !constant_at(&f.body[p], result, fixed, results) {
                return false;
            }
            continue;
        }
        let mut above = depth;
        let mut joined = false;
        for p in (at.saturating_sub(PRODUCER_REACH)..at).rev() {
            if above == 0
                && let Some(Some(otherwise)) = joins.get(p)
            {
                work.extend([(*otherwise, 0), (p, 0)]);
                joined = true;
                break;
            }
            let Some((pops, pushes)) = effect(&f.body[p], arity, &f.symbols) else {
                return false;
            };
            if above >= pushes {
                above = above - pushes + pops;
            } else if matches!(f.body[p], Ins::Op(I::LocalTee(_))) {
                above = 0;
            } else {
                return false;
            }
        }
        if !joined {
            return false;
        }
    }
    true
}

type LocalWrite = (usize, usize, Option<(usize, usize)>);

fn fixed_locals(
    f: &Func,
    params: &[bool],
    results: &[Vec<bool>],
    arity: &Arity,
    joins: &[Option<usize>],
) -> Vec<bool> {
    let np = f.params.len();
    let mut fixed = vec![false; np + f.locals.len()];
    fixed[..np].copy_from_slice(&params[..np]);
    let mut writes: Vec<LocalWrite> = Vec::new();
    for (k, ins) in f.body.iter().enumerate() {
        if let Ins::Op(I::LocalSet(l) | I::LocalTee(l)) = ins {
            writes.push((*l as usize, k, producer(&f.body, k, 0, arity, &f.symbols)));
        }
    }
    let mut grew = true;
    while grew {
        let mut open = vec![true; fixed.len()];
        for &(l, at, from) in &writes {
            let constant = from.map_or_else(
                || constant_before(f, at, 0, &fixed, results, arity, joins),
                |(p, result)| constant_at(&f.body[p], result, &fixed, results),
            );
            if !constant {
                open[l] = false;
            }
        }
        for l in 0..np {
            if !open[l] {
                fixed[l] = false;
            }
        }
        grew = false;
        for l in np..fixed.len() {
            if open[l] && !fixed[l] {
                fixed[l] = true;
                grew = true;
            }
        }
    }
    fixed
}

/// Makes each direct call of a runtime helper with no effect and no result
/// the `drop` of its arguments. Such a helper is one whose body only reads
/// and writes its locals and branches forward, or calls another such
/// helper. A release helper whose counted part folded away is one. A body with a loop, a store, a
/// trap or any other call keeps its calls. Gives, per function, whether
/// any of its calls went.
pub fn effect_free_calls(funcs: &mut [Option<Func>], symbols: &Symbols) -> Vec<bool> {
    let mut free: Vec<Option<usize>> = vec![None; funcs.len()];
    loop {
        let mut grew = false;
        for (k, f) in funcs.iter().enumerate() {
            let Some(f) = f else { continue };
            if free[k].is_some()
                || f.raw.is_some()
                || !f.results.is_empty()
                || !matches!(
                    &*symbols.get(SymId(u32::try_from(k).expect("symbols"))),
                    Sym::Helper(_)
                )
            {
                continue;
            }
            let quiet = f.body.iter().all(|ins| match ins {
                Ins::Call(id) => free.get(id.idx()).is_some_and(Option::is_some),
                ins => local_only(ins),
            });
            if quiet {
                free[k] = Some(f.params.len());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    let mut changed = vec![false; funcs.len()];
    if free.iter().all(Option::is_none) {
        return changed;
    }
    for (k, f) in funcs.iter_mut().enumerate() {
        let Some(f) = f else { continue };
        let gone = |ins: &Ins| match ins {
            Ins::Call(id) => free.get(id.idx()).copied().flatten(),
            _ => None,
        };
        if f.raw.is_some() || !f.body.iter().any(|ins| gone(ins).is_some()) {
            continue;
        }
        let mut origins = Remap::new(f.origins.as_deref());
        let mut body = Vec::with_capacity(f.body.len());
        for (at, ins) in std::mem::take(&mut f.body).into_iter().enumerate() {
            if let Some(params) = gone(&ins) {
                let origin = origins.first(at..at + 1);
                origins.repeat(origin, params);
                body.extend(std::iter::repeat_n(Ins::Op(I::Drop), params));
            } else {
                origins.copy(at..at + 1);
                body.push(ins);
            }
        }
        f.body = body;
        origins.finish(f);
        f.rebuild_callees();
        changed[k] = true;
    }
    changed
}

/// Folds code that a constant decides (#655): an `if` whose condition is
/// an `i32` constant becomes the arm that runs (or goes, with no `else`
/// and no result), and an `i32` local whose every write stores 0 is read
/// as 0, so the flag of a branch folded away goes with it. An arm that
/// branches to the `if` itself stays in a `block`, which keeps the
/// label depth of the `if`; any other arm takes the `if`'s place, each
/// branch past it naming one label fewer, so the straight-line runs the
/// local rules work on continue through it. Gives whether anything
/// changed.
pub fn fold_constant_branches(f: &mut Func) -> bool {
    use crate::module::Val;
    use wasm_encoder::BlockType;
    let mut changed = false;
    loop {
        let mut again = false;
        // Constant conditions.
        let mut i = 0;
        while i + 1 < f.body.len() {
            let (Ins::Op(I::I32Const(c)), Ins::Op(I::If(bt))) = (&f.body[i], &f.body[i + 1]) else {
                i += 1;
                continue;
            };
            let (c, bt) = (*c, *bt);
            // The `if`'s `else` (if any) and `end`.
            let (mut depth, mut els, mut end) = (0usize, None, None);
            for (k, ins) in f.body.iter().enumerate().skip(i + 2) {
                match ins {
                    Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => depth += 1,
                    Ins::Op(I::Else) if depth == 0 => els = Some(k),
                    Ins::Op(I::End) if depth == 0 => {
                        end = Some(k);
                        break;
                    }
                    Ins::Op(I::End) => depth -= 1,
                    _ => {}
                }
            }
            let Some(end) = end else {
                i += 1;
                continue;
            };
            let then = i + 2..els.unwrap_or(end);
            let other = els.map_or(end..end, |e| e + 1..end);
            let selected = if c != 0 { then } else { other };
            let arm: Vec<Ins> = f.body[selected.clone()].to_vec();
            let mut origins = Remap::new(f.origins.as_deref());
            origins.copy(0..i);
            let mut out: Vec<Ins> = Vec::new();
            if let Some(moved) = shifted(&arm, -1, &mut f.labels) {
                out = moved;
                origins.copy(selected);
            } else if !(arm.is_empty() && matches!(bt, BlockType::Empty)) {
                out.push(Ins::Op(I::Block(bt)));
                out.extend(arm);
                out.push(Ins::Op(I::End));
                origins.merged(i..i + 2, 1);
                origins.copy(selected);
                origins.copy(end..end + 1);
            }
            origins.copy(end + 1..f.body.len());
            f.body.splice(i..=end, out);
            origins.finish(f);
            again = true;
        }
        // Locals that only ever hold 0.
        let np = f.params.len();
        let mut zero: Vec<bool> = f.locals.iter().map(|v| *v == Val::I32).collect();
        for (k, ins) in f.body.iter().enumerate() {
            if let Ins::Op(I::LocalSet(l) | I::LocalTee(l)) = ins
                && let Some(at) = (*l as usize).checked_sub(np)
                && !(k > 0 && matches!(f.body[k - 1], Ins::Op(I::I32Const(0))))
            {
                zero[at] = false;
            }
        }
        let np32 = u32::try_from(np).expect("params");
        if zero.iter().any(|z| *z)
            && f.body.iter().any(|ins| {
                matches!(ins, Ins::Op(I::LocalGet(l) | I::LocalSet(l) | I::LocalTee(l))
                    if *l >= np32 && zero[(*l - np32) as usize])
            })
        {
            let mut origins = Remap::new(f.origins.as_deref());
            let body = std::mem::take(&mut f.body);
            f.origins.as_mut().map(Vec::clear);
            for (position, ins) in body.into_iter().enumerate() {
                match ins {
                    Ins::Op(I::LocalGet(l)) if l >= np32 && zero[(l - np32) as usize] => {
                        f.op(I::I32Const(0));
                        origins.copy(position..position + 1);
                    }
                    // Every write stores a constant 0 just pushed: a set
                    // takes it back, and a tee leaves it.
                    Ins::Op(I::LocalSet(l)) if l >= np32 && zero[(l - np32) as usize] => {
                        f.body.pop();
                        if let Some(spans) = &mut f.origins {
                            spans.pop();
                        }
                        origins.pop();
                    }
                    Ins::Op(I::LocalTee(l)) if l >= np32 && zero[(l - np32) as usize] => {}
                    other => {
                        f.push(other);
                        origins.copy(position..position + 1);
                    }
                }
            }
            origins.finish(f);
            again = true;
        }
        if !again {
            return changed;
        }
        changed = true;
    }
}

/// Inlines the fast paths of the counts on known block starts
/// ([`Helper::RetainKnown`], [`Helper::ReleaseKnown`]; §23.30): a retain
/// adds one to the header's count unless the address is null or a static
/// object of count 0, and a release that leaves a count above zero takes
/// one off. A last count, an object that takes part in cycle collection
/// (so its decrement reaches `suspect`), or a release while the collector
/// traces (`Tracing`) still calls the helper, which does all the rest.
/// `acyclic` drops the two collector tests, as no object takes part.
/// Gives whether anything changed.
pub fn inline_counts(f: &mut Func, acyclic: bool) -> bool {
    use crate::module::{HEAP, Val, mem};
    use wasm_encoder::BlockType;
    if !f.body.iter().any(|i| {
        matches!(i, Ins::Call(id) if matches!(&*f.symbol(*id), Sym::Helper(Helper::RetainKnown | Helper::ReleaseKnown)))
    }) {
        return false;
    }
    let (t, b, c) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    let body = std::mem::take(&mut f.body);
    let origins = f.origins.as_mut().map(std::mem::take);
    let outer = f.origin_at;
    // Leaves the header's address in `b` and its count in `c`, branching
    // out of the enclosing block for null.
    let header = |f: &mut Func| {
        f.set(t);
        f.op(I::Block(BlockType::Empty));
        f.get(t);
        f.op(I::I32Eqz);
        f.op(I::BrIf(0));
        f.get(t);
        f.i32(8);
        f.op(I::I32Sub);
        f.op(I::LocalTee(b));
        f.op(I::I32Load(mem(HEAP, 0, 2)));
        f.op(I::LocalTee(c));
    };
    for (position, ins) in body.into_iter().enumerate() {
        f.origin_at = origins.as_ref().and_then(|o| o[position]);
        let known = match ins {
            Ins::Call(id) => match &*f.symbol(id) {
                Sym::Helper(Helper::RetainKnown) => Some(true),
                Sym::Helper(Helper::ReleaseKnown) => Some(false),
                _ => None,
            },
            _ => None,
        };
        match known {
            Some(true) => {
                header(f);
                f.op(I::I32Eqz);
                f.op(I::BrIf(0));
                f.get(b);
                f.get(c);
                f.i32(1);
                f.op(I::I32Add);
                f.op(I::I32Store(mem(HEAP, 0, 2)));
                f.op(I::End);
            }
            Some(false) => {
                header(f);
                f.i32(1);
                f.op(I::I32GtU);
                if !acyclic {
                    f.global_get(Global::Tracing);
                    f.op(I::I32Eqz);
                    f.op(I::I32And);
                    f.get(b);
                    f.op(I::I32Load(mem(HEAP, 4, 2)));
                    f.i32(0);
                    f.op(I::I32GeS);
                    f.op(I::I32And);
                }
                f.op(I::If(BlockType::Empty));
                f.get(b);
                f.get(c);
                f.i32(1);
                f.op(I::I32Sub);
                f.op(I::I32Store(mem(HEAP, 0, 2)));
                f.op(I::Else);
                // A count of 0 is a static object's, except while the
                // collector traces: a gray object's count may have reached
                // 0, and `visit` must still see the reference.
                f.get(c);
                if !acyclic {
                    f.global_get(Global::Tracing);
                    f.op(I::I32Or);
                }
                f.op(I::If(BlockType::Empty));
                f.get(t);
                f.call(Sym::Helper(Helper::ReleaseKnown));
                f.op(I::End);
                f.op(I::End);
                f.op(I::End);
            }
            None => f.push(ins),
        }
    }
    f.origin_at = outer;
    true
}

#[cfg(test)]
mod origin_tests {
    use super::*;
    use crate::module::{HEAP, Val, mem};
    use wasm_encoder::BlockType;

    fn batch_branch_shapes(f: &mut Func) -> bool {
        let sites = ifs(&f.body);
        branch_shapes::run(f, &sites)
    }

    #[test]
    fn an_ungenerated_helper_has_enough_shape_to_remove_fallthrough_controls() {
        let symbols = Symbols::default();
        let target = symbols.intern(Sym::Helper(Helper::NewFixed { size: 4 }));
        let descriptor = symbols.intern(Sym::Helper(Helper::FreeFrame));
        let mut function = Func::default();
        function.symbols = symbols.clone();
        function.body = vec![
            Ins::Op(I::Block(BlockType::Empty)),
            Ins::Op(I::I32Const(4)),
            Ins::Descriptor(descriptor, crate::module::DescriptorKey::default()),
            Ins::Call(target),
            Ins::Op(I::Drop),
            Ins::Op(I::Br(0)),
            Ins::Op(I::End),
        ];
        let mut unknown = function.clone();
        blocks(&mut unknown, &vec![]);
        assert!(
            unknown
                .body
                .iter()
                .any(|instruction| matches!(instruction, Ins::Op(I::Br(0))))
        );
        let arities = symbols.call_arities(&[]);
        assert_eq!(arities[target.idx()], Some((2, 1)));
        assert!(blocks(&mut function, &arities));
        assert!(
            function
                .body
                .iter()
                .any(|instruction| matches!(instruction, Ins::Call(id) if *id == target))
        );
        assert!(
            !function
                .body
                .iter()
                .any(|instruction| matches!(instruction, Ins::Op(I::Br(_) | I::Block(_) | I::End)))
        );
    }

    fn call(symbols: &Symbols, symbol: Sym) -> Ins {
        Ins::Call(symbols.intern(symbol))
    }

    fn tracked(symbols: &Symbols, params: usize, locals: usize, body: Vec<Ins>) -> Func {
        let mut f =
            crate::module::Module::default().func("origins", vec![Val::I32; params], vec![]);
        f.symbols = symbols.clone();
        f.locals = vec![Val::I32; locals];
        f.origins = Some(
            (0..body.len())
                .map(|i| Some(10 + u32::try_from(i).unwrap()))
                .collect(),
        );
        f.body = body;
        f
    }

    fn run(mut f: Func, pass: impl Fn(&mut Func)) -> Func {
        let mut plain = f.clone();
        plain.origins = None;
        pass(&mut plain);
        pass(&mut f);
        assert_eq!(format!("{:?}", f.body), format!("{:?}", plain.body));
        assert_eq!(f.origins.as_ref().unwrap().len(), f.body.len());
        f
    }

    fn spans(f: &Func) -> Vec<u32> {
        f.origins
            .as_ref()
            .unwrap()
            .iter()
            .map(|s| s.expect("retained source"))
            .collect()
    }

    #[test]
    fn a_constant_bulk_width_survives_empty_inlining_blocks() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        for bulk in [
            I::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
            I::MemoryFill(0),
        ] {
            let f = run(
                tracked(
                    &symbols,
                    2,
                    1,
                    vec![
                        op(I::I32Const(1)),
                        op(I::LocalSet(2)),
                        op(I::Block(wasm_encoder::BlockType::Empty)),
                        op(I::Block(wasm_encoder::BlockType::Empty)),
                        op(I::LocalGet(0)),
                        op(I::LocalGet(1)),
                        op(I::LocalGet(2)),
                        op(bulk),
                        op(I::End),
                        op(I::End),
                    ],
                ),
                |f| {
                    assert!(forward(f, 2));
                    assert!(crate::bulk::lower(f, |_, _| None));
                },
            );
            assert!(f.body.iter().any(|i| matches!(i, Ins::Op(I::I32Store8(_)))));
            assert!(!f.body.iter().any(|i| matches!(
                i,
                Ins::Call(_) | Ins::Op(I::MemoryCopy { .. } | I::MemoryFill(_))
            )));
        }
    }

    #[test]
    fn forwarding_work_is_linear_at_width_1000_depth_20() -> Result<(), String> {
        let symbols = Symbols::default();
        let op = Ins::Op;
        for width in [1usize, 1000] {
            for depth in [1usize, 20] {
                let local = |lane: usize, level: usize| {
                    u32::try_from(1 + lane * (depth + 1) + level).map_err(|e| e.to_string())
                };
                let mut body = Vec::new();
                for lane in 0..width {
                    for level in 0..=depth {
                        let source = if level == 0 {
                            0
                        } else {
                            local(lane, level - 1)?
                        };
                        body.push(op(I::LocalGet(source)));
                        body.push(op(I::LocalSet(local(lane, level)?)));
                    }
                }
                for lane in 0..width {
                    body.push(op(I::LocalGet(local(lane, depth)?)));
                    body.push(op(I::Drop));
                }
                let n = body.len();
                let mut f = tracked(&symbols, 1, width * (depth + 1), body);
                let mut work = 0;
                assert!(forward_with(&mut f, 1, || work += 1));
                // Four instruction walks and at most one query per position.
                // Long-distance uses and copy-chain depth add no rescans.
                assert!(
                    work <= 5 * n,
                    "{width} x {depth}: {work} visits / {n} instructions"
                );
                assert_eq!(f.body.len(), 2 * width);
                for chunk in f.body.chunks_exact(2) {
                    assert!(matches!(chunk, [Ins::Op(I::LocalGet(0)), Ins::Op(I::Drop)]));
                }
                let origins = spans(&f);
                for lane in 0..width {
                    assert_eq!(
                        origins[2 * lane],
                        10 + u32::try_from(2 * lane * (depth + 1)).map_err(|e| e.to_string())?
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn forwarding_relinks_newly_adjacent_copies() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let f = run(
            tracked(
                &symbols,
                1,
                3,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Const(3)),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(1)),
                    op(I::LocalSet(2)),
                    op(I::LocalSet(3)),
                    op(I::LocalGet(2)),
                    op(I::Drop),
                    op(I::LocalGet(3)),
                    op(I::Drop),
                ],
            ),
            |f| {
                assert!(forward(f, 1));
            },
        );
        assert!(matches!(
            f.body.as_slice(),
            [
                Ins::Op(I::I32Const(3)),
                Ins::Op(I::Drop),
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::Drop)
            ]
        ));
        assert_eq!(spans(&f), [11, 17, 10, 19]);
    }

    #[test]
    fn forwarding_preserves_local_and_global_clobbers() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let global = symbols.intern_global(Global::HashSeed);
        let other = symbols.intern_global(Global::Forbid);
        let effect = call(
            &symbols,
            Sym::Import(crate::module::Import::new("test", "change", &[], &[])),
        );
        for (source, between, allowed) in [
            (
                op(I::LocalGet(0)),
                vec![op(I::I32Const(9)), op(I::LocalSet(0))],
                false,
            ),
            (
                op(I::LocalGet(0)),
                vec![op(I::I32Const(9)), op(I::LocalSet(2))],
                true,
            ),
            (
                Ins::GlobalGet(global),
                vec![op(I::I32Const(9)), Ins::GlobalSet(global)],
                false,
            ),
            (
                Ins::GlobalGet(global),
                vec![op(I::I32Const(9)), Ins::GlobalSet(other)],
                true,
            ),
            (Ins::GlobalGet(global), vec![effect], false),
        ] {
            let mut body = vec![source, op(I::LocalSet(1))];
            body.extend(between);
            body.extend([op(I::LocalGet(1)), op(I::Drop)]);
            let original = format!("{body:?}");
            let mut f = tracked(&symbols, 1, 2, body);
            assert_eq!(forward(&mut f, 1), allowed);
            if allowed {
                assert!(matches!(f.body.last(), Some(Ins::Op(I::Drop))));
                assert_eq!(
                    format!("{:?}", f.body[f.body.len() - 2]),
                    format!("{source:?}")
                );
            } else {
                assert_eq!(format!("{:?}", f.body), original);
            }
        }
    }

    #[test]
    fn an_ordinary_store_keeps_its_constant_outside_the_block() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::I32Const(1 << 30)),
                    op(I::LocalSet(1)),
                    op(I::Block(wasm_encoder::BlockType::Empty)),
                    op(I::Block(wasm_encoder::BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::LocalGet(1)),
                    op(I::I32Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    })),
                    op(I::End),
                    op(I::End),
                ],
            ),
            |f| {
                assert!(!forward(f, 1));
            },
        );
        assert_eq!(spans(&f), (10..19).collect::<Vec<_>>());
    }

    #[test]
    fn constant_forwarding_still_stops_at_conditional_and_loop_edges() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        for (edge, conditional, produces) in [
            (I::If(wasm_encoder::BlockType::Empty), true, false),
            (I::Loop(wasm_encoder::BlockType::Empty), false, false),
            (
                I::Block(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
                false,
                true,
            ),
        ] {
            let mut body = vec![op(I::I32Const(1)), op(I::LocalSet(1))];
            if conditional {
                body.push(op(I::LocalGet(0)));
            }
            body.extend([op(edge), op(I::LocalGet(1))]);
            if !produces {
                body.push(op(I::Drop));
            }
            body.push(op(I::End));
            if produces {
                body.push(op(I::Drop));
            }
            let mut f = tracked(&symbols, 1, 1, body);
            assert!(!forward(&mut f, 1));
        }
        // A read after the end of a conditional store may still see zero.
        let mut f = tracked(
            &symbols,
            1,
            1,
            vec![
                op(I::LocalGet(0)),
                op(I::If(wasm_encoder::BlockType::Empty)),
                op(I::I32Const(1)),
                op(I::LocalSet(1)),
                op(I::End),
                op(I::LocalGet(1)),
                op(I::Drop),
            ],
        );
        assert!(!forward(&mut f, 1));
    }

    #[test]
    fn origins_follow_local_cleanup_forwarding_and_scheduling() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![op(I::LocalGet(0)), op(I::LocalSet(1)), op(I::LocalGet(1))],
            ),
            |f| locals(f, &Arity::new()),
        );
        assert_eq!(spans(&f), [10]);
        let f = run(
            tracked(&symbols, 0, 1, vec![op(I::I32Const(7)), op(I::LocalSet(0))]),
            |f| locals(f, &Arity::new()),
        );
        assert!(spans(&f).is_empty());
        let f = run(
            tracked(&symbols, 0, 1, vec![op(I::I32Const(7)), op(I::LocalTee(0))]),
            |f| locals(f, &Arity::new()),
        );
        assert_eq!(spans(&f), [10]);
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::I32Const(1)),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(0)),
                    op(I::I32Eqz),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(1)),
                    op(I::LocalGet(1)),
                    op(I::I32Add),
                ],
            ),
            |f| locals(f, &Arity::new()),
        );
        assert!(
            f.body
                .iter()
                .zip(f.origins.as_ref().unwrap())
                .any(|(i, s)| matches!(i, Ins::Op(I::LocalTee(_))) && *s == Some(14))
        );
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    op(I::LocalSet(1)),
                    op(I::I32Const(2)),
                    op(I::Drop),
                    op(I::LocalGet(1)),
                ],
            ),
            |f| {
                assert!(forward(f, 1));
            },
        );
        assert_eq!(spans(&f), [12, 13, 10]);
        let f = run(
            tracked(
                &symbols,
                1,
                0,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Const(4)),
                    op(I::I32Add),
                    op(I::I32Const(9)),
                    op(I::I32Store(mem(HEAP, 0, 2))),
                ],
            ),
            |f| {
                assert!(schedule(f, 1, &Arity::new()));
            },
        );
        assert_eq!(spans(&f), [10, 13, 11]);
        let f = run(
            tracked(
                &symbols,
                0,
                2,
                vec![
                    op(I::I32Const(1)),
                    op(I::LocalSet(0)),
                    op(I::I32Const(2)),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(0)),
                    op(I::LocalGet(1)),
                    op(I::I32Add),
                ],
            ),
            |f| {
                assert!(schedule(f, 0, &Arity::new()));
            },
        );
        // Rule 9 takes precedence here, moving the second value before its producer.
        assert_eq!(spans(&f), [10, 11, 14, 12, 16]);
        let f = run(
            tracked(
                &symbols,
                0,
                1,
                vec![
                    op(I::I32Const(3)),
                    op(I::LocalSet(0)),
                    op(I::I32Const(5)),
                    op(I::LocalGet(0)),
                    op(I::I32Sub),
                ],
            ),
            |f| {
                assert!(schedule(f, 0, &Arity::new()));
            },
        );
        assert_eq!(spans(&f), [12, 10, 14]);
    }

    fn validate_void_block_fixture(f: &Func) {
        use wasm_encoder::{CodeSection, Function, FunctionSection, Module, TypeSection, ValType};
        let mut types = TypeSection::new();
        types.ty().function(vec![ValType::I32; f.params.len()], []);
        types.ty().function([ValType::I32], [ValType::I32]);
        let mut functions = FunctionSection::new();
        functions.function(0);
        let mut code =
            Function::new_with_locals_types(std::iter::repeat_n(ValType::I32, f.locals.len()));
        for ins in &f.body {
            if let Ins::Op(op) = ins {
                code.instruction(&op.to_wasm(&f.labels));
            }
        }
        code.instruction(&wasm_encoder::Instruction::End);
        let mut codes = CodeSection::new();
        codes.function(&code);
        let mut module = Module::new();
        module.section(&types).section(&functions).section(&codes);
        let validation = wasmparser::Validator::new().validate_all(&module.finish());
        assert!(validation.is_ok(), "{:?}", validation.err());
    }

    #[test]
    fn void_blocks_rewrite_nested_branches_and_shared_tables() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let mut f = tracked(
            &symbols,
            1,
            0,
            vec![
                op(I::Block(BlockType::Empty)),
                op(I::Block(BlockType::Empty)),
                op(I::LocalGet(0)),
                op(I::BrIf(1)),
                op(I::Block(BlockType::Empty)),
                op(I::LocalGet(0)),
                op(I::BrIf(2)),
                op(I::End),
                op(I::End),
                op(I::End),
            ],
        );
        validate_void_block_fixture(&f);
        assert!(blocks(&mut f, &vec![]));
        validate_void_block_fixture(&f);
        assert_eq!(spans(&f), [10, 12, 13, 15, 16, 19]);
        assert!(matches!(f.body[2], Ins::Op(I::BrIf(0))));
        assert!(matches!(f.body[4], Ins::Op(I::BrIf(0))));

        let mut f = tracked(
            &symbols,
            1,
            0,
            vec![
                op(I::Block(BlockType::Empty)),
                op(I::Block(BlockType::Empty)),
                op(I::LocalGet(0)),
                op(I::BrIf(2)),
                op(I::End),
                op(I::End),
            ],
        );
        validate_void_block_fixture(&f);
        assert!(blocks(&mut f, &vec![]));
        validate_void_block_fixture(&f);
        assert!(matches!(
            f.body.as_slice(),
            [Ins::Op(I::LocalGet(0)), Ins::Op(I::BrIf(0))]
        ));

        let mut f = tracked(
            &symbols,
            0,
            0,
            vec![
                op(I::Block(BlockType::Empty)),
                op(I::Block(BlockType::Empty)),
                op(I::Block(BlockType::Empty)),
                op(I::Br(2)),
                op(I::End),
                op(I::End),
                op(I::End),
            ],
        );
        validate_void_block_fixture(&f);
        assert!(blocks(&mut f, &vec![]));
        validate_void_block_fixture(&f);
        assert!(matches!(f.body[2], Ins::Op(I::Br(1))));

        let mut body = Vec::new();
        for keep_middle in [false, true] {
            body.extend([
                op(I::Block(BlockType::Empty)),
                op(I::Block(BlockType::Empty)),
            ]);
            if keep_middle {
                body.extend([op(I::LocalGet(0)), op(I::BrIf(0))]);
            }
            body.extend([
                op(I::Loop(BlockType::Empty)),
                op(I::Block(BlockType::Empty)),
                op(I::LocalGet(0)),
                op(I::BrTable(LabelListId(0), 3)),
                op(I::End),
                op(I::End),
                op(I::End),
                op(I::End),
            ]);
        }
        let mut f = tracked(&symbols, 1, 0, body);
        f.labels.push(vec![1, 3]);
        validate_void_block_fixture(&f);
        assert!(blocks(&mut f, &vec![]));
        validate_void_block_fixture(&f);
        assert_eq!(f.label_list(LabelListId(0)), [1, 3]);
        let tables: Vec<_> = f
            .body
            .iter()
            .filter_map(|ins| match ins {
                Ins::Op(I::BrTable(table, default)) => Some((f.label_list(*table), *default)),
                _ => None,
            })
            .collect();
        assert_eq!(
            tables,
            [(vec![1, 2].as_slice(), 2), (vec![1, 3].as_slice(), 3)]
        );
    }

    #[test]
    fn void_blocks_keep_typed_parameters_and_polymorphic_boundaries() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let f = tracked(
            &symbols,
            0,
            0,
            vec![
                op(I::Block(BlockType::Result(wasm_encoder::ValType::I32))),
                op(I::I32Const(1)),
                op(I::End),
                op(I::Drop),
                op(I::I32Const(1)),
                op(I::Block(BlockType::FunctionType(1))),
                op(I::I32Const(2)),
                op(I::I32Add),
                op(I::End),
                op(I::Drop),
                op(I::Block(BlockType::Empty)),
                op(I::Unreachable),
                op(I::End),
                op(I::Block(BlockType::Empty)),
                op(I::I32Const(3)),
                op(I::Drop),
                op(I::End),
            ],
        );
        validate_void_block_fixture(&f);
        let f = run(f, |f| {
            assert!(blocks(f, &vec![]));
        });
        validate_void_block_fixture(&f);
        assert_eq!(
            f.body
                .iter()
                .filter(|ins| matches!(ins, Ins::Op(I::Block(_))))
                .count(),
            3
        );
        assert_eq!(spans(&f), (10..23).chain(24..26).collect::<Vec<_>>());
    }

    #[test]
    fn void_blocks_have_linear_work_at_width_and_depth() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        for (width, depth) in [(1000, 1), (1000, 20), (1, 10_000)] {
            let mut body = Vec::new();
            for _ in 0..width {
                body.extend(std::iter::repeat_n(op(I::Block(BlockType::Empty)), depth));
                body.extend([op(I::LocalGet(0)), op(I::Drop)]);
                body.extend(std::iter::repeat_n(op(I::End), depth));
            }
            let n = body.len();
            let mut f = tracked(&symbols, 1, 0, body);
            validate_void_block_fixture(&f);
            let mut visits = 0;
            assert!(blocks_with(&mut f, &vec![], || visits += 1));
            assert_eq!(f.body.len(), 2 * width);
            assert_eq!(visits, 3 * n);
            assert_eq!(f.origins.as_ref().map(Vec::len), Some(2 * width));
            validate_void_block_fixture(&f);
            eprintln!(
                "void blocks width {width}, depth {depth}: {visits} visits for {n} instructions"
            );
        }
    }

    /// Rule 18: a `br 0` that only falls through goes, and a void block it
    /// alone targeted goes with it, as does a void `if`'s empty `else`; a
    /// loop's back edge, a branch that leaves values under its label's,
    /// and a typed block stay.
    #[test]
    fn a_branch_to_the_next_end_goes_when_falling_through_matches_it() {
        use wasm_encoder::ValType;
        let symbols = Symbols::default();
        let op = Ins::Op;
        let cases = [
            (
                // A void block's only branch, then the block.
                vec![
                    op(I::Block(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::Drop),
                    op(I::Br(0)),
                    op(I::End),
                ],
                vec![op(I::LocalGet(0)), op(I::Drop)],
            ),
            (
                // A typed block keeps its label; its value is the stack's.
                vec![
                    op(I::Block(BlockType::Result(ValType::I32))),
                    op(I::I32Const(1)),
                    op(I::Br(0)),
                    op(I::End),
                    op(I::Drop),
                ],
                vec![
                    op(I::Block(BlockType::Result(ValType::I32))),
                    op(I::I32Const(1)),
                    op(I::End),
                    op(I::Drop),
                ],
            ),
            (
                // The branch discards the 1 under its value; falling through cannot.
                vec![
                    op(I::Block(BlockType::Result(ValType::I32))),
                    op(I::I32Const(1)),
                    op(I::I32Const(2)),
                    op(I::Br(0)),
                    op(I::End),
                    op(I::Drop),
                ],
                vec![],
            ),
            (
                // A loop's `br 0` is its back edge.
                vec![
                    op(I::Block(BlockType::Empty)),
                    op(I::Loop(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::BrIf(1)),
                    op(I::Br(0)),
                    op(I::End),
                    op(I::End),
                ],
                vec![],
            ),
            (
                // A then arm's branch to its `if` before the `else`.
                vec![
                    op(I::LocalGet(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::Br(0)),
                    op(I::Else),
                    op(I::LocalGet(0)),
                    op(I::Drop),
                    op(I::End),
                ],
                vec![
                    op(I::LocalGet(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::Else),
                    op(I::LocalGet(0)),
                    op(I::Drop),
                    op(I::End),
                ],
            ),
            (
                // After `unreachable` the stack is polymorphic: the branch
                // goes, and the block stays around the polymorphic boundary.
                vec![
                    op(I::Block(BlockType::Result(ValType::I32))),
                    op(I::Unreachable),
                    op(I::Br(0)),
                    op(I::End),
                    op(I::Drop),
                ],
                vec![
                    op(I::Block(BlockType::Result(ValType::I32))),
                    op(I::Unreachable),
                    op(I::End),
                    op(I::Drop),
                ],
            ),
            (
                // A void `if`'s empty `else`, after its then arm's branch.
                vec![
                    op(I::LocalGet(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::Drop),
                    op(I::Br(0)),
                    op(I::Else),
                    op(I::End),
                ],
                vec![
                    op(I::LocalGet(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::Drop),
                    op(I::End),
                ],
            ),
            (
                // A value pushed after `unreachable` is still one too many
                // for a void label to fall through with.
                vec![
                    op(I::Block(BlockType::Empty)),
                    op(I::Unreachable),
                    op(I::I32Const(1)),
                    op(I::Br(0)),
                    op(I::End),
                ],
                vec![],
            ),
        ];
        for (body, want) in cases {
            let f = tracked(&symbols, 1, 0, body.clone());
            validate_void_block_fixture(&f);
            let f = run(f, |f| {
                blocks(f, &vec![]);
            });
            validate_void_block_fixture(&f);
            let want = if want.is_empty() { body } else { want };
            assert_eq!(format!("{:?}", f.body), format!("{want:?}"));
        }
    }

    /// A body whose labels do not balance, or that branches past its
    /// outermost label, keeps its blocks: Wasm validation reports it as
    /// written.
    #[test]
    fn void_blocks_leave_an_invalid_body_as_written() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        for body in [
            vec![
                op(I::Block(BlockType::Empty)),
                op(I::I32Const(1)),
                op(I::Drop),
            ],
            vec![op(I::Block(BlockType::Empty)), op(I::Br(2)), op(I::End)],
        ] {
            let mut f = tracked(&symbols, 0, 0, body.clone());
            assert!(!blocks(&mut f, &vec![]));
            assert_eq!(format!("{:?}", f.body), format!("{body:?}"));
            assert_eq!(spans(&f), (10..13).collect::<Vec<_>>());
        }
    }

    /// A constant address addend folds into a store's offset only while
    /// the offset fits 32 bits; past that the store keeps its `i32.add`.
    #[test]
    fn a_store_offset_folds_only_while_it_fits_32_bits() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let f = run(
            tracked(
                &symbols,
                1,
                0,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Const(32)),
                    op(I::I32Add),
                    op(I::I32Const(9)),
                    op(I::I32Store(mem(HEAP, 0xFFFF_FFF0, 2))),
                    op(I::LocalGet(0)),
                    op(I::I32Const(4)),
                    op(I::I32Add),
                    op(I::I32Const(9)),
                    op(I::I32Store(mem(HEAP, 0, 2))),
                ],
            ),
            |f| {
                assert!(schedule(f, 1, &Arity::new()));
            },
        );
        assert!(matches!(f.body[2], Ins::Op(I::I32Add)));
        assert!(matches!(f.body[4], Ins::Op(I::I32Store(m)) if m.offset == 0xFFFF_FFF0));
        assert!(matches!(f.body[7], Ins::Op(I::I32Store(m)) if m.offset == 4));
        assert_eq!(f.body.len(), 8);
        assert_eq!(spans(&f), [10, 11, 12, 13, 14, 15, 18, 16]);
    }

    #[test]
    fn scheduling_visits_are_linear_at_width_and_depth() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        for (width, depth) in [(256u32, 1u32), (1024, 1), (1, 1024)] {
            let mut body = Vec::new();
            for w in 0..width {
                body.extend([
                    op(I::LocalGet(0)),
                    op(I::I32Const(4)),
                    op(I::I32Add),
                    op(I::I32Const(9)),
                    op(I::I32Store(mem(HEAP, 0, 2))),
                    op(I::LocalGet(0)),
                ]);
                for d in 0..depth {
                    let local = 1 + w * depth + d;
                    body.extend([
                        op(I::LocalSet(local)),
                        op(I::I32Const(5)),
                        op(I::LocalGet(local)),
                        op(I::I32Sub),
                    ]);
                }
                body.push(op(I::Drop));
            }
            let before = body.len();
            let mut f = tracked(&symbols, 1, (width * depth) as usize, body);
            let mut visits = 0;
            assert!(schedule_with(&mut f, 1, &Arity::new(), || visits += 1));
            assert_eq!(f.body.len(), (width * (5 + 2 * depth)) as usize);
            assert_eq!(f.origins.as_ref().map(Vec::len), Some(f.body.len()));
            assert!(
                visits <= 9 * before,
                "{visits} scheduling visits for {before} instructions"
            );
            eprintln!(
                "scheduling width {width}, depth {depth}: {visits} visits for {before} instructions"
            );
            let mut settled = 0;
            assert!(!schedule_with(&mut f, 1, &Arity::new(), || settled += 1));
            assert!(settled <= 5 * f.body.len());
            for chunk in f.body.chunks_exact((5 + 2 * depth) as usize) {
                assert!(matches!(chunk[2], Ins::Op(I::I32Store(m)) if m.offset == 4));
                assert!(
                    chunk[3..3 + depth as usize]
                        .iter()
                        .all(|i| matches!(i, Ins::Op(I::I32Const(5))))
                );
                assert!(matches!(chunk[3 + depth as usize], Ins::Op(I::LocalGet(0))));
            }
        }
    }

    #[test]
    fn scheduling_indexes_nested_store_regions_once() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        for depth in [1000, 4000] {
            let mut body = Vec::new();
            for _ in 0..depth {
                body.extend([op(I::LocalGet(0)), op(I::I32Const(4)), op(I::I32Add)]);
            }
            for _ in 0..depth {
                body.extend([op(I::I32Const(9)), op(I::I32Store(mem(HEAP, 0, 2)))]);
            }
            let n = body.len();
            let mut f = tracked(&symbols, 1, 0, body);
            let mut visits = 0;
            assert!(schedule_with(&mut f, 1, &Arity::new(), || visits += 1));
            assert_eq!(f.body.len(), 3 * depth);
            assert!(visits <= 9 * n, "{visits} visits for {n} instructions");
            assert!(
                f.body[..depth]
                    .iter()
                    .all(|i| matches!(i, Ins::Op(I::LocalGet(0))))
            );
            assert!(f.body[depth..].chunks_exact(2).all(|pair| matches!(pair, [Ins::Op(I::I32Const(9)), Ins::Op(I::I32Store(m))] if m.offset == 4)));
            eprintln!("scheduling nested stores {depth}: {visits} visits for {n} instructions");
        }
    }

    #[test]
    fn scheduling_keeps_global_reads_after_calls_and_writes() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let global = symbols.intern_global(Global::HeapTop);
        let call = call(
            &symbols,
            Sym::Import(crate::module::Import::new("fixture", "effect", &[], &[])),
        );
        for effect in [vec![call], vec![op(I::I32Const(2)), Ins::GlobalSet(global)]] {
            let mut body = vec![op(I::I32Const(1))];
            body.extend(effect);
            body.extend([
                op(I::LocalSet(0)),
                Ins::GlobalGet(global),
                op(I::LocalGet(0)),
                op(I::I32Sub),
                op(I::Drop),
            ]);
            let mut f = tracked(&symbols, 0, 1, body);
            let before = spans(&f);
            assert!(!schedule(&mut f, 0, &Arity::new()));
            assert_eq!(spans(&f), before);
        }
    }

    #[test]
    fn scheduling_respects_clobbers_and_multi_result_boundaries() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let multi = call(
            &symbols,
            Sym::Import(crate::module::Import {
                module: "fixture".to_owned(),
                name: "multi".to_owned(),
                params: vec![],
                results: vec![Val::I32; 2],
            }),
        );
        let mut f = tracked(
            &symbols,
            1,
            2,
            vec![
                op(I::I32Const(1)),
                op(I::LocalTee(0)),
                op(I::LocalSet(1)),
                op(I::LocalGet(0)),
                op(I::LocalGet(1)),
                op(I::I32Sub),
                op(I::Drop),
                multi,
                op(I::LocalSet(2)),
                op(I::I32Const(2)),
                op(I::LocalGet(2)),
                op(I::I32Sub),
                op(I::Drop),
                op(I::Drop),
            ],
        );
        assert!(!schedule(&mut f, 1, &Arity::new()));
        assert_eq!(spans(&f), (10..24).collect::<Vec<_>>());
    }

    #[test]
    fn fold_clears_nested_empty_blocks_in_one_walk() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let depth = 4096;
        let mut body = vec![op(I::LocalGet(0)), op(I::I32Const(1)), op(I::I32Mul)];
        body.extend(std::iter::repeat_n(op(I::Block(BlockType::Empty)), depth));
        body.push(op(I::Br(0)));
        body.extend(std::iter::repeat_n(op(I::End), depth));
        // Keep a real branch to the enclosing block, and keep a loop's
        // self-branch: it repeats instead of exiting.
        body.extend([
            op(I::Block(BlockType::Empty)),
            op(I::Block(BlockType::Empty)),
            op(I::Br(1)),
            op(I::End),
            op(I::End),
            op(I::Loop(BlockType::Empty)),
            op(I::Br(0)),
            op(I::End),
        ]);
        let mut f = tracked(&symbols, 1, 0, body);
        assert!(fold(&mut f));
        assert_eq!(f.body.len(), 9);
        assert!(matches!(f.body[0], Ins::Op(I::LocalGet(0))));
        assert!(matches!(f.body[3], Ins::Op(I::Br(1))));
        assert!(matches!(f.body[6], Ins::Op(I::Loop(BlockType::Empty))));
        assert!(matches!(f.body[7], Ins::Op(I::Br(0))));
        assert_eq!(spans(&f)[0], 10);
        assert!(!fold(&mut f));
    }

    #[test]
    fn origins_follow_folds_and_both_control_shapes() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let mut f = tracked(
            &symbols,
            1,
            0,
            vec![op(I::LocalGet(0)), op(I::I32Const(0)), op(I::I32Eq)],
        );
        f.origins.as_mut().unwrap()[1] = None;
        let f = run(f, |f| {
            assert!(fold(f));
        });
        assert_eq!(spans(&f), [10, 12]);
        let f = run(
            tracked(
                &symbols,
                1,
                0,
                vec![
                    op(I::LocalGet(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::I32Const(7)),
                    op(I::Drop),
                    op(I::Else),
                    op(I::Br(1)),
                    op(I::End),
                ],
            ),
            |f| {
                assert!(shapes(f, 1, &Vec::new()));
            },
        );
        assert_eq!(spans(&f), [10, 11, 11, 12, 13]);
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::I32Const(7)),
                    op(I::LocalSet(1)),
                    op(I::Else),
                    op(I::I32Const(8)),
                    op(I::LocalSet(1)),
                    op(I::End),
                    op(I::LocalGet(1)),
                ],
            ),
            |f| {
                assert!(shapes(f, 1, &Vec::new()));
            },
        );
        assert_eq!(spans(&f), [10, 11, 12, 14, 15, 17]);
        let f = run(
            tracked(
                &symbols,
                0,
                0,
                vec![
                    op(I::I32Const(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::I32Const(10)),
                    op(I::Drop),
                    op(I::Else),
                    op(I::I32Const(20)),
                    op(I::Drop),
                    op(I::End),
                ],
            ),
            |f| {
                assert!(fold_constant_branches(f));
            },
        );
        assert_eq!(spans(&f), [15, 16]);
        // An arm takes its `if`'s place, one label fewer past it, unless
        // it branches to the `if` itself.
        for (target, kept) in [(1, false), (0, true)] {
            let mut f = tracked(
                &symbols,
                0,
                0,
                vec![
                    op(I::Block(BlockType::Empty)),
                    op(I::I32Const(1)),
                    op(I::If(BlockType::Empty)),
                    op(I::Br(target)),
                    op(I::End),
                    op(I::End),
                ],
            );
            assert!(fold_constant_branches(&mut f));
            let body: Vec<String> = f.body.iter().map(|i| format!("{i:?}")).collect();
            let want = if kept {
                ["Block", "Block", "Br(0)", "End", "End"].as_slice()
            } else {
                ["Block", "Br(0)", "End"].as_slice()
            };
            assert_eq!(body.len(), want.len(), "{body:?}");
            assert!(
                body.iter().zip(want).all(|(b, w)| b.contains(w)),
                "{body:?}"
            );
        }
        let f = run(
            tracked(
                &symbols,
                0,
                1,
                vec![op(I::I32Const(0)), op(I::LocalSet(0)), op(I::LocalGet(0))],
            ),
            |f| {
                assert!(fold_constant_branches(f));
            },
        );
        assert_eq!(spans(&f), [12]);
    }

    #[test]
    fn static_counts_meet_scalar_if_values_and_reject_unknown_arms_or_exits() {
        for (dynamic, exit) in [(false, false), (true, false), (false, true)] {
            let symbols = Symbols::default();
            let retain = Hash128(7);
            let count = call(&symbols, Sym::Inst(retain));
            let arity = vec![Some((1, 0)); symbols.len()];
            let mut body = vec![
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::If(BlockType::Result(wasm_encoder::ValType::I32))),
                Ins::Op(I::I32Const(56)),
            ];
            if exit {
                body.extend([Ins::Op(I::I32Const(1)), Ins::Op(I::BrIf(0))]);
            }
            body.extend([
                Ins::Op(I::Else),
                Ins::Op(if dynamic {
                    I::LocalGet(1)
                } else {
                    I::I32Const(72)
                }),
                Ins::Op(I::End),
                Ins::Op(I::LocalSet(2)),
                Ins::Op(I::LocalGet(2)),
                count,
            ]);
            let f = run(tracked(&symbols, 2, 1, body), |f| {
                assert_eq!(
                    static_counts(f, retain, &arity, &[false, false]),
                    !dynamic && !exit
                );
            });
            assert_eq!(
                f.body.iter().filter(|i| matches!(i, Ins::Call(_))).count(),
                usize::from(dynamic || exit)
            );
        }
    }

    #[test]
    fn static_counts_go_only_on_constant_operands() {
        let symbols = Symbols::default();
        let retain = Hash128(7);
        let counts = [
            call(&symbols, Sym::Inst(retain)),
            call(&symbols, Sym::Helper(Helper::RetainKnown)),
            call(&symbols, Sym::Helper(Helper::ReleaseRef)),
        ];
        let mut arity = vec![None; symbols.len()];
        for c in &counts {
            let Ins::Call(id) = c else { unreachable!() };
            arity[id.idx()] = Some((1, 0));
        }
        let op = Ins::Op;
        // Local 1 holds only constants, through a tee and a copy; local 2
        // only ever adds to itself; local 0 is a parameter.
        let f = run(
            tracked(
                &symbols,
                1,
                2,
                vec![
                    op(I::I32Const(56)),
                    op(I::LocalTee(1)),
                    counts[0],
                    op(I::I32Const(72)),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(1)),
                    counts[1],
                    op(I::LocalGet(2)),
                    op(I::I32Const(8)),
                    op(I::I32Add),
                    op(I::LocalTee(2)),
                    counts[2],
                    op(I::LocalGet(0)),
                    counts[0],
                ],
            ),
            |f| {
                assert!(static_counts(f, retain, &arity, &[false]));
            },
        );
        let left: Vec<String> = f
            .body
            .iter()
            .filter(|i| matches!(i, Ins::Call(_)))
            .map(|i| format!("{i:?}"))
            .collect();
        assert_eq!(
            left,
            [format!("{:?}", counts[2]), format!("{:?}", counts[0])]
        );
        assert!(matches!(f.body[2], Ins::Op(I::Drop)));
        assert_eq!(spans(&f)[2], 12);
    }

    #[test]
    fn static_values_require_every_closed_world_call_to_be_constant() {
        let symbols = Symbols::default();
        let callee = symbols.intern(Sym::Inst(Hash128(7)));
        let source = symbols.intern(Sym::Inst(Hash128(8)));
        let branched = symbols.intern(Sym::Inst(Hash128(9)));
        let arity = vec![Some((1, 0)), Some((0, 1)), Some((0, 1))];
        let target = tracked(&symbols, 1, 0, vec![]);
        let constant_caller = tracked(
            &symbols,
            0,
            0,
            vec![Ins::Op(I::I32Const(1)), Ins::Call(callee)],
        );
        let facts = static_values(
            &[Some(target.clone()), Some(constant_caller.clone())],
            [],
            &arity,
        );
        assert_eq!(facts[0], [true]);

        let mut constant_source =
            crate::module::Module::default().func("constant source", vec![], vec![Val::I32]);
        constant_source.symbols = symbols.clone();
        constant_source.body = vec![Ins::Op(I::I32Const(2))];
        let result_caller = tracked(&symbols, 0, 0, vec![Ins::Call(source), Ins::Call(callee)]);
        let facts = static_values(
            &[
                Some(target.clone()),
                Some(constant_source),
                Some(result_caller),
            ],
            [],
            &arity,
        );
        assert_eq!(facts[0], [true]);

        let mut branched_source =
            crate::module::Module::default().func("branched source", vec![], vec![Val::I32]);
        branched_source.symbols = symbols.clone();
        branched_source.body = vec![
            Ins::Op(I::I32Const(0)),
            Ins::Op(I::If(BlockType::Empty)),
            Ins::Op(I::I32Const(2)),
            Ins::Op(I::Return),
            Ins::Op(I::End),
            Ins::Op(I::I32Const(1)),
        ];
        let branched_caller = tracked(&symbols, 0, 0, vec![Ins::Call(branched), Ins::Call(callee)]);
        let facts = static_values(
            &[
                Some(target.clone()),
                None,
                Some(branched_source),
                Some(branched_caller),
            ],
            [],
            &arity,
        );
        assert_eq!(facts[0], [false]);

        let unknown_caller = tracked(
            &symbols,
            1,
            0,
            vec![Ins::Op(I::LocalGet(0)), Ins::Call(callee)],
        );
        let facts = static_values(
            &[
                Some(target.clone()),
                Some(constant_caller),
                Some(unknown_caller),
            ],
            [],
            &arity,
        );
        assert_eq!(facts[0], [false]);

        let facts = static_values(&[Some(target)], [callee], &arity);
        assert_eq!(facts[0], [false]);
    }

    // #1912: the proof went in rounds over every function, and a chain of
    // calls that pass a constant on proved one level per round: O(n^2)
    // instructions read for a chain of n functions. Now a function is
    // looked at again only when a fact it reads was proved.
    #[test]
    fn static_values_read_a_call_chain_a_constant_number_of_times() {
        let chain = |n: usize| {
            let symbols = Symbols::default();
            let ids: Vec<SymId> = (0..=n)
                .map(|k| symbols.intern(Sym::Inst(Hash128(100 + k as u128))))
                .collect();
            let mut arity = vec![Some((0, 0))];
            arity.extend((1..=n).map(|_| Some((1, 0))));
            // `f0` passes a constant to `f1`; each `f_i` passes its own
            // parameter on to `f_(i+1)`.
            let mut funcs = vec![Some(tracked(
                &symbols,
                0,
                0,
                vec![Ins::Op(I::I32Const(1)), Ins::Call(ids[1])],
            ))];
            for k in 1..=n {
                let body = if k < n {
                    vec![Ins::Op(I::LocalGet(0)), Ins::Call(ids[k + 1])]
                } else {
                    vec![]
                };
                funcs.push(Some(tracked(&symbols, 1, 0, body)));
            }
            let (facts, work) =
                crate::work::count_static_looks(|| static_values(&funcs, [ids[0]], &arity));
            assert!(facts[1..].iter().all(|p| p == &[true]), "{n}: {facts:?}");
            work
        };
        let (small, large) = (chain(20), chain(200));
        assert!(
            small > 0 && large <= 11 * small,
            "instructions read for chains of 20 and 200 calls: {small} and {large}"
        );
    }

    fn releases(f: &Func) -> usize {
        f.body
            .iter()
            .filter(|i| {
                matches!(i, Ins::Call(id) if matches!(&*f.symbol(*id),
            Sym::Helper(Helper::ReleaseRef | Helper::ReleaseKnown | Helper::Release { .. })))
            })
            .count()
    }

    #[test]
    fn zero_facts_remove_initial_and_moved_slots_but_keep_live_values() {
        let symbols = Symbols::default();
        let f = tracked(
            &symbols,
            1,
            2,
            vec![
                Ins::Op(I::LocalGet(1)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::I32Const(0)),
                call(&symbols, Sym::Helper(Helper::ReleaseKnown)),
                Ins::Op(I::LocalGet(0)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::I32Const(7)),
                Ins::Op(I::LocalSet(2)),
                Ins::Op(I::LocalGet(2)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::I32Const(0)),
                Ins::Op(I::LocalSet(2)),
                Ins::Op(I::LocalGet(2)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::I32Const(0)),
                Ins::Op(I::LocalSet(2)),
            ],
        );
        let f = run(f, |f| {
            zero_releases(f);
        });
        assert_eq!(releases(&f), 2);
        assert_eq!(
            f.body
                .iter()
                .filter(|i| matches!(i, Ins::Op(I::LocalSet(2))))
                .count(),
            2
        );
    }

    #[test]
    fn zero_facts_meet_both_if_arms_and_exclude_returned_paths() {
        let symbols = Symbols::default();
        let branch = |live: bool| {
            tracked(
                &symbols,
                2,
                1,
                vec![
                    Ins::Op(I::LocalGet(0)),
                    Ins::Op(I::If(BlockType::Empty)),
                    Ins::Op(if live { I::LocalGet(1) } else { I::I32Const(0) }),
                    Ins::Op(I::LocalSet(2)),
                    Ins::Op(I::Else),
                    Ins::Op(I::I32Const(0)),
                    Ins::Op(I::LocalSet(2)),
                    Ins::Op(I::End),
                    Ins::Op(I::LocalGet(2)),
                    call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                ],
            )
        };
        assert_eq!(
            releases(&run(branch(false), |f| {
                zero_releases(f);
            })),
            0
        );
        assert_eq!(
            releases(&run(branch(true), |f| {
                zero_releases(f);
            })),
            1
        );
        let mut exits = branch(true);
        exits.body.insert(4, Ins::Op(I::Return));
        exits.origins = Some(vec![Some(17); exits.body.len()]);
        assert_eq!(
            releases(&run(exits, |f| {
                zero_releases(f);
            })),
            0
        );
    }

    #[test]
    fn zero_facts_include_loop_backedges_and_branch_table_exits() {
        let symbols = Symbols::default();
        let loop_body = |reset: bool| {
            let mut body = vec![
                Ins::Op(I::Loop(BlockType::Empty)),
                Ins::Op(I::LocalGet(1)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::LocalSet(1)),
            ];
            if reset {
                body.extend([Ins::Op(I::I32Const(0)), Ins::Op(I::LocalSet(1))]);
            }
            body.extend([
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::BrIf(0)),
                Ins::Op(I::End),
            ]);
            tracked(&symbols, 1, 1, body)
        };
        assert_eq!(
            releases(&run(loop_body(false), |f| {
                zero_releases(f);
            })),
            1
        );
        assert_eq!(
            releases(&run(loop_body(true), |f| {
                zero_releases(f);
            })),
            0
        );
        let mut table = tracked(
            &symbols,
            1,
            1,
            vec![
                Ins::Op(I::Block(BlockType::Empty)),
                Ins::Op(I::Block(BlockType::Empty)),
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::BrTable(LabelListId(0), 1)),
                Ins::Op(I::End),
                Ins::Op(I::I32Const(7)),
                Ins::Op(I::LocalSet(1)),
                Ins::Op(I::End),
                Ins::Op(I::LocalGet(1)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
            ],
        );
        table.labels.push(vec![0]);
        assert_eq!(
            releases(&run(table, |f| {
                zero_releases(f);
            })),
            1
        );
    }

    #[test]
    fn dead_stores_go_unless_a_release_reads_them_on_some_path() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let release = call(&symbols, Sym::Helper(Helper::ReleaseRef));
        let zeros = |f: &Func| {
            f.body
                .windows(2)
                .filter(|w| matches!(w, [Ins::Op(I::I32Const(0)), Ins::Op(I::LocalSet(1))]))
                .count()
        };
        // A loop that moves local 1 out and zeroes it: the zero stays when
        // the release after the loop, or the release at the loop head on
        // the backedge, reads it, and goes when the next iteration
        // overwrites it first.
        let body = |head_release: bool, exit_release: bool| {
            let mut b = vec![op(I::Loop(BlockType::Empty))];
            if head_release {
                b.extend([op(I::LocalGet(1)), release]);
            }
            b.extend([
                op(I::LocalGet(0)),
                op(I::LocalSet(1)),
                op(I::LocalGet(1)),
                op(I::Drop),
                op(I::I32Const(0)),
                op(I::LocalSet(1)),
                op(I::LocalGet(0)),
                op(I::BrIf(0)),
                op(I::End),
            ]);
            if exit_release {
                b.extend([op(I::LocalGet(1)), release]);
            }
            tracked(&symbols, 1, 1, b)
        };
        for (head, exit, kept) in [(false, false, 0), (true, false, 1), (false, true, 1)] {
            let f = run(body(head, exit), |f| {
                live_rules(f, &Arity::new());
            });
            assert_eq!(zeros(&f), kept, "{:?}", f.body);
        }
        // A dead set of a constant drops it, and rule 15 takes the push back.
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    release,
                    op(I::I32Const(5)),
                    op(I::LocalSet(1)),
                ],
            ),
            |f| {
                assert!(live_rules(f, &Arity::new()));
                assert!(drops(f));
            },
        );
        assert_eq!(f.body.len(), 2);
        assert_eq!(spans(&f), [10, 11]);
    }

    #[test]
    fn single_uses_stay_on_the_stack_by_liveness() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let guard = [
            op(I::If(BlockType::Empty)),
            call(&symbols, Sym::Helper(Helper::ReleaseRef)),
            op(I::Unreachable),
            op(I::End),
        ];
        // Local 2 is written twice, so the count rules leave it; each
        // write has one reader, and the loop head writes it again.
        let mut body = vec![op(I::Loop(BlockType::Empty))];
        body.extend([op(I::LocalGet(0)), op(I::LocalSet(2))]);
        body.extend([op(I::LocalGet(1)), op(I::I32Eqz)]);
        body.extend(guard);
        body.extend([op(I::LocalGet(2)), op(I::Drop)]);
        body.extend([op(I::LocalGet(1)), op(I::I32Const(1)), op(I::I32Add)]);
        body.extend([op(I::LocalSet(2)), op(I::LocalGet(1)), op(I::LocalSet(3))]);
        body.extend([
            op(I::LocalGet(2)),
            op(I::LocalGet(3)),
            op(I::BrIf(0)),
            op(I::Drop),
        ]);
        body.push(op(I::End));
        let f = run(tracked(&symbols, 2, 2, body), |f| {
            while live_rules(f, &Arity::new()) {}
        });
        let shown: Vec<String> = f.body.iter().map(|i| format!("{i:?}")).collect();
        // The copy of 0 moves past the guard to its read, the copy of 1
        // moves to its read, and the sum stays on the stack under it.
        assert!(
            !shown
                .iter()
                .any(|i| i.contains("Local") && i.contains("(2)")),
            "{shown:?}"
        );
        assert!(!shown.iter().any(|i| i.contains("(3)")), "{shown:?}");
        // A loop test evaluated before the loop and again at its head:
        // each value stays on the stack, written by a tee for the head.
        let f = run(
            tracked(
                &symbols,
                0,
                2,
                vec![
                    op(I::I32Const(0)),
                    op(I::LocalSet(0)),
                    op(I::I32Const(3)),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(0)),
                    op(I::LocalGet(1)),
                    op(I::I32LtU),
                    op(I::Drop),
                    op(I::Loop(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::LocalGet(1)),
                    op(I::I32LtU),
                    op(I::BrIf(0)),
                    op(I::End),
                ],
            ),
            |f| {
                while live_rules(f, &Arity::new()) {}
            },
        );
        let shown: Vec<String> = f.body.iter().map(|i| format!("{i:?}")).collect();
        assert_eq!(
            shown.iter().filter(|i| i.contains("LocalTee")).count(),
            2,
            "{shown:?}"
        );
        assert!(!shown.iter().any(|i| i.contains("LocalSet")), "{shown:?}");
        // A run that consumes the value keeps its local.
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Eqz),
                    op(I::LocalSet(1)),
                    op(I::Drop),
                    op(I::LocalGet(1)),
                ],
            ),
            |f| {
                assert!(!live_rules(f, &Arity::new()));
            },
        );
        assert_eq!(f.body.len(), 5);
    }

    #[test]
    fn a_branch_to_the_exit_after_its_values_returns_them() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        // The first branch sets both exit values and returns; the second
        // sets one only and keeps its branch.
        let f = run(
            tracked(
                &symbols,
                1,
                2,
                vec![
                    op(I::Block(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::I32Const(1)),
                    op(I::I32Const(2)),
                    op(I::LocalSet(2)),
                    op(I::LocalSet(1)),
                    op(I::Br(1)),
                    op(I::End),
                    op(I::I32Const(3)),
                    op(I::LocalSet(2)),
                    op(I::Br(0)),
                    op(I::End),
                    op(I::LocalGet(1)),
                    op(I::LocalGet(2)),
                ],
            ),
            |f| {
                assert!(exits(f));
            },
        );
        assert!(matches!(f.body[5], Ins::Op(I::Return)), "{:?}", f.body);
        assert!(matches!(f.body[9], Ins::Op(I::Br(0))), "{:?}", f.body);
        assert_eq!(spans(&f)[5], 17);
    }

    #[test]
    fn local_cleanup_rounds_are_bounded_at_width_1000_and_depth_20() {
        let symbols = Symbols::default();
        let opaque = call(&symbols, Sym::Inst(Hash128(1882)));
        let arity = vec![Some((1, 0)); symbols.len()];
        let op = Ins::Op;
        for width in [1, 1000, 10000] {
            for depth in [1, 20] {
                let mut body = vec![op(I::Block(BlockType::Empty)); depth];
                for _ in 0..width {
                    // The first write becomes dead when pairs forwards the
                    // second one. Early liveness exposes that fact within
                    // this sweep rather than waiting for another fixpoint.
                    body.extend([
                        op(I::I32Const(1)),
                        op(I::LocalSet(0)),
                        op(I::I32Const(42)),
                        op(I::LocalSet(0)),
                        op(I::LocalGet(0)),
                        opaque,
                    ]);
                }
                body.extend(vec![op(I::End); depth]);
                let input = body.len();
                let mut f = tracked(&symbols, 0, 1, body);
                let expected_origins: Vec<_> = f
                    .body
                    .iter()
                    .zip(spans(&f))
                    .filter_map(|(ins, origin)| matches!(ins, Ins::Call(_)).then_some(origin))
                    .collect();
                let mut plain = f.clone();
                plain.origins = None;
                locals(&mut plain, &arity);
                let mut rounds = Vec::new();
                locals_with_work(&mut f, &arity, &mut |n| rounds.push(n));
                assert_eq!(rounds.len(), 2, "{width} x {depth}: {rounds:?}");
                assert!(rounds.iter().sum::<usize>() <= 2 * input);
                assert_eq!(format!("{:?}", f.body), format!("{:?}", plain.body));
                assert!(f.locals.is_empty());
                let retained: Vec<_> = f
                    .body
                    .iter()
                    .filter(|ins| !matches!(ins, Ins::Op(I::Block(_) | I::End)))
                    .copied()
                    .collect();
                assert_eq!(retained.len(), 2 * width);
                for pair in retained.chunks_exact(2) {
                    assert!(matches!(pair, [Ins::Op(I::I32Const(42)), Ins::Call(_)]));
                }
                let actual_origins: Vec<_> = f
                    .body
                    .iter()
                    .zip(spans(&f))
                    .filter_map(|(ins, origin)| matches!(ins, Ins::Call(_)).then_some(origin))
                    .collect();
                assert_eq!(actual_origins, expected_origins);
            }
        }
    }

    #[test]
    fn a_rule_that_found_nothing_runs_again_only_after_an_edit() {
        let mut f = Func::default();
        let mut settled = Settled::new();
        let runs = std::cell::Cell::new([0; 3]);
        let pass = |k: usize, edits: bool| {
            let mut now = runs.get();
            now[k] += 1;
            runs.set(now);
            edits
        };
        assert!(!settled.run(&mut f, Rule::Pairs, |_| pass(0, false)));
        assert!(!settled.run(&mut f, Rule::Pairs, |_| pass(0, false)));
        assert_eq!(runs.get()[0], 1, "nothing changed since it found nothing");
        assert!(settled.run(&mut f, Rule::Forward, |_| pass(1, true)));
        assert!(!settled.run(&mut f, Rule::Pairs, |_| pass(0, false)));
        assert_eq!(runs.get()[0], 2, "another rule edited the function");
        // A rule that repeats itself to its own fixed point has seen its
        // result; a single sweep has not.
        assert!(settled.run(&mut f, Rule::Live, |_| pass(2, true)));
        assert!(!settled.run(&mut f, Rule::Live, |_| pass(2, false)));
        assert_eq!(runs.get()[2], 1);
        assert!(settled.run(&mut f, Rule::Forward, |_| pass(1, true)));
        assert!(!settled.run(&mut f, Rule::Forward, |_| pass(1, false)));
        assert_eq!(runs.get()[1], 3);
    }

    #[test]
    fn liveness_revisits_only_the_blocks_a_backedge_reaches() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        // A long run of forward blocks, then a loop that reads local 1
        // before it writes it, so the read reaches the loop's head again
        // over the backedge.
        for prefix in [16usize, 256, 1024] {
            let mut body = Vec::new();
            for _ in 0..prefix {
                body.extend([
                    op(I::Block(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::BrIf(0)),
                    op(I::End),
                ]);
            }
            body.extend([
                op(I::Loop(BlockType::Empty)),
                op(I::LocalGet(1)),
                op(I::Drop),
                op(I::LocalGet(0)),
                op(I::LocalSet(1)),
                op(I::LocalGet(0)),
                op(I::BrIf(0)),
                op(I::End),
            ]);
            let f = tracked(&symbols, 1, 1, body);
            let mut live = Live::blocks(&f, Sets::default()).expect("blocks");
            assert!(!live.forward);
            let blocks = live.starts.len();
            let mut visits = 0;
            live.settle_with(&f, || visits += 1);
            // The backedge sends only the loop's blocks and the one
            // before it round again, not every block once more.
            assert!(visits <= blocks + 4, "{visits} visits of {blocks} blocks");
            // Local 1 is live into every block before the loop: its
            // first read is of the zero it starts with.
            assert!(
                live.into[..blocks - 2]
                    .iter()
                    .all(|&row| live.sets.contains(row, 1))
            );
            assert!(live.sets.contains(live.into[0], 0));
        }
    }

    /// Rule 13's rounds carry their dead accesses: after each round, the
    /// carried ones are what a new liveness pass finds. The bodies hold
    /// each shape a round can leave: two moves of one local whose spans
    /// cross or nest, with and without a later read of it, a read of the
    /// moved local inside a span, a span across a panic guard's blocks, a
    /// tee's push read again, a moved constant, a dead tee, and a loop.
    #[test]
    fn rule_13_rounds_carry_the_dead_accesses_a_new_pass_finds() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let (get, set, tee) = (
            |l| op(I::LocalGet(l)),
            |l| op(I::LocalSet(l)),
            |l| op(I::LocalTee(l)),
        );
        let add = op(I::I32Add);
        let drop = op(I::Drop);
        let crossing = vec![get(0), set(1), get(0), set(2), get(1), get(2), add, drop];
        let nested = vec![get(0), set(1), get(0), set(2), get(2), get(1), add, drop];
        let read_inside = vec![get(0), set(1), get(0), drop, get(1), drop];
        let guard = vec![
            get(0),
            set(1),
            op(I::I32Const(1)),
            op(I::If(BlockType::Empty)),
            op(I::Unreachable),
            op(I::End),
            get(1),
            drop,
        ];
        let teed = vec![get(0), tee(1), drop, get(1), drop];
        let constant = vec![op(I::I32Const(7)), set(1), get(1), drop];
        let dead_tee = vec![get(0), tee(2), drop];
        let mut cases = Vec::new();
        for body in [
            crossing,
            nested,
            read_inside,
            guard,
            teed,
            constant,
            dead_tee,
        ] {
            let mut later = body.clone();
            later.extend([get(0), drop]);
            let mut looped = vec![op(I::Loop(BlockType::Empty))];
            looped.extend(later.iter().copied());
            looped.extend([get(0), op(I::BrIf(0)), op(I::End)]);
            cases.extend([body, later, looped]);
        }
        for body in cases {
            let mut f = tracked(&symbols, 1, 2, body.clone());
            let mut dead = Live::dead_accesses(&f).expect("analysed");
            let mut rounds = 0;
            while let Some(next) = live_once(&mut f, &Arity::new(), &dead) {
                assert_eq!(
                    Some(&next),
                    Live::dead_accesses(&f).as_ref(),
                    "{body:?} -> {:?}",
                    f.body
                );
                dead = next;
                rounds += 1;
            }
            assert!(rounds > 0, "{body:?} rewrites");
        }
    }

    /// Rule 11's fixed point settles a loop before the blocks after it.
    /// The loop copies a local its last round wrote, so its exit gets two
    /// facts; taken first in first out, each of the `n` blocks after it
    /// was visited once per fact, and now once.
    #[test]
    fn zero_facts_settle_a_loop_before_the_blocks_after_it() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        for n in [16usize, 256] {
            let mut body = vec![
                op(I::Loop(BlockType::Empty)),
                op(I::LocalGet(1)),
                op(I::LocalSet(2)),
                op(I::LocalGet(0)),
                op(I::LocalSet(1)),
                op(I::LocalGet(0)),
                op(I::BrIf(0)),
                op(I::End),
            ];
            for _ in 0..n {
                body.extend([
                    op(I::Block(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::BrIf(0)),
                    op(I::End),
                ]);
            }
            body.extend([op(I::I32Const(0)), op(I::LocalSet(2))]);
            let mut f = tracked(&symbols, 1, 2, body);
            let blocks = zero_edges(&f.body, &f.labels)
                .map(|edges| basic_blocks(&edges, f.body.len()).1.len())
                .expect("edges");
            let mut visits = 0;
            zero_releases_with(&mut f, &mut || visits += 1);
            assert!(visits <= blocks + 4, "{visits} visits of {blocks} blocks");
        }
    }

    /// A copy shares its source's local only where no other value of the
    /// copy reaches: a branch around the copy brings the copy's older
    /// value to the join, so the two stay apart. Without the branch they
    /// share and the copy goes.
    #[test]
    fn a_copy_joined_by_a_branch_around_it_keeps_its_own_local() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let release = call(&symbols, Sym::Helper(Helper::ReleaseRef));
        for branch in [true, false] {
            let mut body = vec![
                op(I::I32Const(7)),
                op(I::LocalSet(1)),
                op(I::I32Const(1)),
                op(I::LocalSet(2)),
                op(I::Block(BlockType::Empty)),
            ];
            if branch {
                body.extend([op(I::LocalGet(0)), op(I::BrIf(0))]);
            }
            body.extend([
                op(I::LocalGet(1)),
                op(I::LocalSet(2)),
                op(I::End),
                op(I::LocalGet(2)),
                release,
                op(I::LocalGet(1)),
                release,
            ]);
            let f = run(tracked(&symbols, 1, 2, body), coalesce);
            assert_eq!(f.locals.len(), if branch { 2 } else { 1 }, "{:?}", f.body);
        }
    }

    /// Each set of a multi-value copy pairs with its get from the end of
    /// the run of gets, a tee ends the run, and a set whose source an
    /// earlier set of the run wrote copies nothing.
    #[test]
    fn copy_sources_pair_each_set_of_a_run_with_its_get() {
        let op = Ins::Op;
        let body = [
            op(I::LocalGet(1)),
            op(I::LocalGet(2)),
            op(I::LocalSet(1)),
            op(I::LocalSet(2)),
            op(I::LocalGet(3)),
            op(I::LocalTee(4)),
            op(I::I32Const(0)),
            op(I::LocalSet(5)),
            op(I::LocalGet(4)),
            op(I::LocalGet(3)),
            op(I::LocalSet(6)),
            op(I::LocalTee(7)),
        ];
        assert_eq!(
            copy_sources(&body, 8),
            [
                None,
                None,
                Some((2, 1)),
                None,
                None,
                Some((3, 4)),
                None,
                None,
                None,
                None,
                Some((3, 9)),
                Some((4, 8)),
            ]
        );
    }

    #[test]
    fn coalescing_keeps_apart_locals_live_over_a_backedge_or_from_entry() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let release = call(&symbols, Sym::Helper(Helper::ReleaseRef));
        let locals = |f: &Func| f.locals.len();
        // Local 1 is read in the loop before local 2 is written there, but
        // the backedge reads 1 again: they never share.
        let looped = vec![
            op(I::I32Const(5)),
            op(I::LocalSet(1)),
            op(I::Loop(BlockType::Empty)),
            op(I::LocalGet(1)),
            release,
            op(I::I32Const(7)),
            op(I::LocalSet(2)),
            op(I::LocalGet(2)),
            release,
            op(I::LocalGet(0)),
            op(I::BrIf(0)),
            op(I::End),
        ];
        assert_eq!(locals(&run(tracked(&symbols, 1, 2, looped), coalesce)), 2);
        // Local 2 is written in one arm only, so the read after the `if`
        // may read its zero: it is live from the entry, and does not take
        // local 1's, whose value is dead by then.
        let one_arm = vec![
            op(I::I32Const(9)),
            op(I::LocalSet(1)),
            op(I::LocalGet(1)),
            release,
            op(I::LocalGet(0)),
            op(I::If(BlockType::Empty)),
            op(I::I32Const(3)),
            op(I::LocalSet(2)),
            op(I::End),
            op(I::LocalGet(2)),
            release,
        ];
        assert_eq!(locals(&run(tracked(&symbols, 1, 2, one_arm), coalesce)), 2);
        // With both arms writing it, local 2 is not live at entry and
        // shares a local with 1 or the parameter, both dead by then.
        let both_arms = vec![
            op(I::I32Const(9)),
            op(I::LocalSet(1)),
            op(I::LocalGet(1)),
            release,
            op(I::LocalGet(0)),
            op(I::If(BlockType::Empty)),
            op(I::I32Const(3)),
            op(I::LocalSet(2)),
            op(I::Else),
            op(I::I32Const(4)),
            op(I::LocalSet(2)),
            op(I::End),
            op(I::LocalGet(2)),
            release,
        ];
        assert_eq!(
            locals(&run(tracked(&symbols, 1, 2, both_arms), coalesce)),
            1
        );
        // A copy of a running total whose source is written again only
        // after the copy's last read shares the total's local, and the
        // copy goes, in and out of a loop.
        for looping in [false, true] {
            let mut body = vec![op(I::I32Const(1)), op(I::LocalSet(1))];
            if looping {
                body.push(op(I::Loop(BlockType::Empty)));
            }
            body.extend([
                op(I::LocalGet(1)),
                op(I::LocalSet(2)),
                op(I::LocalGet(0)),
                release,
                op(I::LocalGet(2)),
                op(I::I32Const(1)),
                op(I::I32Add),
                op(I::LocalSet(1)),
            ]);
            if looping {
                body.extend([op(I::LocalGet(0)), op(I::BrIf(0)), op(I::End)]);
            }
            body.extend([op(I::LocalGet(1)), release]);
            let f = run(tracked(&symbols, 1, 2, body), coalesce);
            assert_eq!(locals(&f), 1, "{looping}: {:?}", f.body);
            let copies = f.body.windows(2).filter(
                |w| matches!(w, [Ins::Op(I::LocalGet(a)), Ins::Op(I::LocalSet(b))] if a == b),
            );
            assert_eq!(copies.count(), 0, "{:?}", f.body);
        }
        // A copy whose source is written while the copy is still live
        // keeps a local of its own (the parameter, read last, keeps its).
        let clobbered = vec![
            op(I::I32Const(1)),
            op(I::LocalSet(1)),
            op(I::LocalGet(1)),
            op(I::LocalSet(2)),
            op(I::I32Const(2)),
            op(I::LocalSet(1)),
            op(I::LocalGet(2)),
            op(I::LocalGet(1)),
            op(I::I32Add),
            release,
            op(I::LocalGet(0)),
            release,
        ];
        assert_eq!(
            locals(&run(tracked(&symbols, 1, 2, clobbered), coalesce)),
            2
        );
    }

    #[test]
    fn coalesced_locals_never_hold_two_live_values() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let release = call(&symbols, Sym::Helper(Helper::ReleaseRef));
        // Locals 1 and 2 live one after the other: one local. Local 3
        // overlaps local 4, and local 5 is read before any write, so it
        // keeps its own zero while 4 is live.
        let body = vec![
            op(I::LocalGet(0)),
            op(I::LocalSet(1)),
            op(I::LocalGet(1)),
            release,
            op(I::LocalGet(0)),
            op(I::I32Eqz),
            op(I::LocalSet(2)),
            op(I::LocalGet(2)),
            release,
            op(I::I32Const(7)),
            op(I::LocalSet(3)),
            op(I::I32Const(8)),
            op(I::LocalSet(4)),
            op(I::LocalGet(3)),
            op(I::LocalGet(4)),
            op(I::I32Add),
            release,
            op(I::LocalGet(5)),
            release,
        ];
        let mut f = run(tracked(&symbols, 1, 5, body), coalesce);
        let named: Vec<u32> = f
            .body
            .iter()
            .filter_map(|i| match i {
                Ins::Op(I::LocalGet(l) | I::LocalSet(l)) => Some(*l),
                _ => None,
            })
            .collect();
        // 1 and 2 take the parameter's local, whose value is dead after
        // its last read; 3 joins them too. 4 and 5 stay apart.
        assert_eq!(named, [0, 0, 0, 0, 0, 1, 0, 1, 2], "{:?}", f.body);
        assert_eq!(f.locals.len(), 2);
        // A copy whose source keeps its value shares the source's local
        // and goes.
        f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(0)),
                    release,
                    op(I::LocalGet(1)),
                    release,
                ],
            ),
            coalesce,
        );
        assert_eq!(f.body.len(), 4);
        assert!(f.locals.is_empty());
        // A two-value copy shares each source's local and goes, though
        // first fit would put its first destination in the parameter's
        // class.
        f = run(
            tracked(
                &symbols,
                1,
                4,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Eqz),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(0)),
                    op(I::I32Eqz),
                    op(I::LocalSet(2)),
                    op(I::LocalGet(1)),
                    op(I::LocalGet(2)),
                    op(I::LocalSet(4)),
                    op(I::LocalSet(3)),
                    op(I::LocalGet(3)),
                    release,
                    op(I::LocalGet(4)),
                    release,
                ],
            ),
            coalesce,
        );
        assert_eq!(f.body.len(), 10, "{:?}", f.body);
    }

    #[test]
    fn zero_facts_without_locals_distinguish_reachable_and_unreachable_releases() {
        let symbols = Symbols::default();
        let f = run(
            tracked(
                &symbols,
                0,
                0,
                vec![
                    Ins::Op(I::I32Const(0)),
                    call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                    Ins::Op(I::Return),
                    Ins::Op(I::I32Const(0)),
                    call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                ],
            ),
            |f| {
                assert!(zero_releases(f));
            },
        );
        assert!(matches!(
            f.body.as_slice(),
            [Ins::Op(I::Return), Ins::Op(I::I32Const(0)), Ins::Call(_)]
        ));
        assert_eq!(releases(&f), 1);
    }

    #[test]
    fn zero_facts_meet_multiple_words_without_losing_other_high_locals() {
        let symbols = Symbols::default();
        let f = run(
            tracked(
                &symbols,
                1,
                129,
                vec![
                    Ins::Op(I::LocalGet(0)),
                    Ins::Op(I::If(BlockType::Empty)),
                    Ins::Op(I::LocalGet(0)),
                    Ins::Op(I::LocalSet(65)),
                    Ins::Op(I::Else),
                    Ins::Op(I::I32Const(0)),
                    Ins::Op(I::LocalSet(65)),
                    Ins::Op(I::End),
                    Ins::Op(I::LocalGet(65)),
                    call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                    Ins::Op(I::LocalGet(129)),
                    call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                ],
            ),
            |f| {
                assert!(zero_releases(f));
            },
        );
        assert_eq!(releases(&f), 1);
        assert!(f.body.iter().any(|i| matches!(i, Ins::Op(I::LocalGet(65)))));
        assert!(
            !f.body
                .iter()
                .any(|i| matches!(i, Ins::Op(I::LocalGet(129))))
        );
    }

    #[test]
    fn zero_facts_preserve_source_origins() {
        let symbols = Symbols::default();
        let f = tracked(
            &symbols,
            1,
            0,
            vec![
                Ins::Op(I::I32Const(0)),
                Ins::Anchor(99),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::LocalGet(0)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
            ],
        );
        let f = run(f, |f| {
            zero_releases(f);
        });
        assert_eq!(spans(&f), [11, 13, 14]);
        let value = tracked(
            &symbols,
            0,
            2,
            vec![
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::LocalGet(1)),
                call(
                    &symbols,
                    Sym::Helper(Helper::Release {
                        vals: vec![Val::I32, Val::I32],
                        walk: crate::layout::Walk::switch(
                            crate::layout::Discr {
                                leaf: crate::layout::Leaf {
                                    offset: 0,
                                    val: Val::I32,
                                    bytes: 4,
                                    signed: false,
                                },
                                value: 0,
                                enc: crate::layout::Enc::Direct,
                            },
                            vec![None, Some(crate::layout::Walk::Ref(1))],
                        ),
                    }),
                ),
            ],
        );
        assert_eq!(
            releases(&run(value, |f| {
                zero_releases(f);
            })),
            0
        );
    }

    #[test]
    fn an_if_with_nothing_in_it_drops_its_test() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let empty = wasm_encoder::BlockType::Empty;
        // A pure test goes, past an anchor; a call's stays and its result
        // is dropped; an `if` with something in it stays.
        let f = run(
            tracked(
                &symbols,
                1,
                0,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Load(mem(HEAP, 8, 2))),
                    op(I::I32Eqz),
                    op(I::If(empty)),
                    Ins::Anchor(0),
                    op(I::End),
                    call(&symbols, Sym::Helper(Helper::CollCopy)),
                    op(I::If(empty)),
                    op(I::End),
                    op(I::LocalGet(0)),
                    op(I::If(empty)),
                    op(I::Unreachable),
                    op(I::End),
                ],
            ),
            |f| {
                drops(f);
            },
        );
        assert_eq!(
            format!("{:?}", f.body),
            format!(
                "{:?}",
                [
                    Ins::Anchor(0),
                    call(&symbols, Sym::Helper(Helper::CollCopy)),
                    op(I::Drop),
                    op(I::LocalGet(0)),
                    op(I::If(empty)),
                    op(I::Unreachable),
                    op(I::End),
                ]
            )
        );
    }

    #[test]
    fn a_drop_takes_back_what_pushed_its_value() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        // A discarded field read, past an anchor; a tee that is dropped;
        // an addition of a read and a constant; a call's result stays.
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Load(mem(HEAP, 8, 2))),
                    Ins::Anchor(0),
                    op(I::Drop),
                    op(I::I32Const(3)),
                    op(I::LocalTee(1)),
                    op(I::Drop),
                    op(I::LocalGet(0)),
                    op(I::I32Load(mem(HEAP, 0, 2))),
                    op(I::I32Const(4)),
                    op(I::I32Add),
                    op(I::Drop),
                    call(&symbols, Sym::Helper(Helper::CollCopy)),
                    op(I::Drop),
                ],
            ),
            |f| {
                drops(f);
            },
        );
        assert_eq!(
            format!("{:?}", f.body),
            format!(
                "{:?}",
                [
                    Ins::Anchor(0),
                    op(I::I32Const(3)),
                    op(I::LocalSet(1)),
                    call(&symbols, Sym::Helper(Helper::CollCopy)),
                    op(I::Drop),
                ]
            )
        );
        assert_eq!(spans(&f), [12, 14, 16, 22, 23]);
        // A division can trap, so it stays and its result is dropped.
        let f = run(
            tracked(
                &symbols,
                1,
                0,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Const(2)),
                    op(I::I32DivS),
                    op(I::Drop),
                ],
            ),
            |f| {
                drops(f);
            },
        );
        assert_eq!(f.body.len(), 4);
    }

    /// Rule 19 where its forms do not apply: a branch that carries a
    /// value, a value that branches to its own `if`, a test with two
    /// empty arms, and a test whose operand has no start in reach.
    #[test]
    fn a_short_circuit_value_stays_where_no_branch_chain_holds_it() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let i32 = BlockType::Result(wasm_encoder::ValType::I32);
        let value = |x: Vec<Ins>, k: i32| {
            let mut v = vec![op(I::LocalGet(0)), op(I::If(i32))];
            v.extend(x);
            v.extend([op(I::Else), op(I::I32Const(k)), op(I::End)]);
            v
        };
        let two = Ins::Call(symbols.intern(Sym::Inst(Hash128(7))));
        let arity = vec![Some((0, 2)); symbols.len()];
        let shaped = |params: usize, results: Vec<Val>, body: Vec<Ins>| {
            let mut f = tracked(&symbols, params, 0, body);
            f.results = results;
            let changed = shapes(&mut f, u32::try_from(params).unwrap(), &arity);
            (changed, f.body)
        };
        // `&&` falls through its `br_if`; the label takes the value under
        // the test, which a new `if` would hide.
        for (label, stays) in [(i32, true), (BlockType::Empty, false)] {
            let mut body = vec![op(I::Block(label)), op(I::I32Const(5))];
            body.extend(value(vec![op(I::LocalGet(1))], 0));
            body.extend([op(I::BrIf(0)), op(I::End)]);
            assert_eq!(shaped(2, vec![], body).0, !stays);
        }
        for (label, stays) in [
            (BlockType::FunctionType(1), true),
            (BlockType::Empty, false),
        ] {
            let mut body = vec![op(I::Loop(label))];
            body.extend(value(vec![op(I::LocalGet(1))], 0));
            body.extend([op(I::BrIf(0)), op(I::End)]);
            assert_eq!(shaped(2, vec![], body).0, !stays);
        }
        for (results, stays) in [(vec![Val::I32], true), (vec![], false)] {
            let mut body = value(vec![op(I::LocalGet(1))], 0);
            body.push(op(I::BrIf(0)));
            assert_eq!(shaped(2, results, body).0, !stays);
        }
        // `X` branches to its own `if` with the value.
        let mut body = vec![op(I::Block(BlockType::Empty))];
        body.extend(value(
            vec![op(I::LocalGet(1)), op(I::LocalGet(1)), op(I::BrIf(0))],
            0,
        ));
        body.extend([op(I::BrIf(0)), op(I::End)]);
        assert!(!shaped(2, vec![], body).0);
        // An `if` with no code on either arm.
        let mut body = value(vec![op(I::LocalGet(1))], 1);
        body.extend([op(I::If(BlockType::Empty)), op(I::End)]);
        assert!(!shaped(2, vec![], body).0);
        // `||` into an `if` needs a block around its operand: none past
        // a block of unknown type, nor past the walk's reach.
        let arms = [
            op(I::If(BlockType::Empty)),
            op(I::LocalGet(1)),
            op(I::Drop),
            op(I::End),
        ];
        let mut body = vec![op(I::Block(BlockType::FunctionType(1))), op(I::End)];
        body.extend(value(vec![op(I::LocalGet(1))], 1));
        body.extend(arms);
        body[2] = op(I::I32Eqz);
        assert!(!shaped(2, vec![], body).0);
        let mut body = vec![op(I::LocalGet(0))];
        body.extend((0..OPERAND_REACH).map(|_| op(I::I32Eqz)));
        body.extend(value(vec![op(I::LocalGet(1))], 1).into_iter().skip(1));
        body.extend(arms);
        assert!(!shaped(2, vec![], body).0);
        let mut body = vec![op(I::LocalGet(0)), op(I::Block(BlockType::Empty))];
        body.extend((0..OPERAND_REACH).map(|_| op(I::I32Eqz)));
        body.extend([op(I::End), op(I::I32Eqz)]);
        body.extend(value(vec![op(I::LocalGet(1))], 1).into_iter().skip(1));
        body.extend(arms);
        assert!(!shaped(2, vec![], body).0);
        // A call that pushes a value under the test as well.
        let mut body = vec![two];
        body.extend(value(vec![op(I::LocalGet(1))], 1).into_iter().skip(1));
        body.extend(arms);
        body.push(op(I::Drop));
        assert!(!shaped(2, vec![], body).0);
        // Within reach, the same chain is one block, around a void block.
        let mut body = vec![
            op(I::LocalGet(0)),
            op(I::Block(BlockType::Empty)),
            op(I::End),
        ];
        body.push(op(I::I32Eqz));
        body.extend(value(vec![op(I::LocalGet(1))], 1).into_iter().skip(1));
        body.extend(arms);
        let (changed, body) = shaped(2, vec![], body);
        assert!(changed);
        assert!(matches!(body[0], Ins::Op(I::Block(BlockType::Empty))));
    }
    #[test]
    fn branch_only_shapes_compact_at_width_1000_depth_20_and_depth_50000() {
        let symbols = Symbols::default();
        let mut baseline = 0;
        for (width, depth) in [(100usize, 20u32), (1000, 20), (1, 50_000)] {
            let mut body = Vec::new();
            for _ in 0..width {
                body.push(Ins::Op(I::Block(BlockType::Empty)));
                for level in 1..=depth {
                    body.extend([
                        Ins::Op(I::LocalGet(0)),
                        Ins::Op(I::If(BlockType::Empty)),
                        Ins::Op(I::Br(level)),
                        Ins::Op(I::Else),
                    ]);
                }
                body.extend(std::iter::repeat_n(Ins::Op(I::End), depth as usize + 1));
            }
            let size = body.len();
            let mut f = tracked(&symbols, 1, 0, body);
            let (changed, work) = crate::work::count_control(|| batch_branch_shapes(&mut f));
            assert!(changed);
            assert!(
                work <= 4 * size as u64,
                "{width} x {depth}: {work} for {size} instructions"
            );
            if width == 100 {
                baseline = work;
            }
            if width == 1000 {
                assert!(work <= 10 * baseline);
            }
            assert_eq!(f.body.len(), width * (2 + 2 * depth as usize));
            assert!(
                !f.body
                    .iter()
                    .any(|ins| matches!(ins, Ins::Op(I::If(_) | I::Else)))
            );
            validate_void_block_fixture(&f);
            assert!(!batch_branch_shapes(&mut f));
        }
    }
    #[test]
    fn branch_only_compaction_preserves_a_targeted_if_label() {
        let symbols = Symbols::default();
        let mut f = tracked(
            &symbols,
            1,
            0,
            vec![
                Ins::Op(I::Block(BlockType::Empty)),
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::If(BlockType::Empty)),
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::BrIf(0)),
                Ins::Op(I::Else),
                Ins::Op(I::Br(1)),
                Ins::Op(I::End),
                Ins::Op(I::End),
            ],
        );
        validate_void_block_fixture(&f);
        assert!(!batch_branch_shapes(&mut f));
        assert!(matches!(f.body[2], Ins::Op(I::If(BlockType::Empty))));
        assert!(matches!(f.body[4], Ins::Op(I::BrIf(0))));
        assert!(matches!(f.body[6], Ins::Op(I::Br(1))));
        validate_void_block_fixture(&f);
    }
    #[test]
    fn branch_only_compaction_remaps_every_branch_table_target() {
        let symbols = Symbols::default();
        let mut f = tracked(&symbols, 1, 0, Vec::new());
        let table = f.intern_labels(vec![1, 1]);
        f.body = vec![
            Ins::Op(I::Block(BlockType::Empty)),
            Ins::Op(I::LocalGet(0)),
            Ins::Op(I::If(BlockType::Empty)),
            Ins::Op(I::Br(1)),
            Ins::Op(I::Else),
            Ins::Op(I::LocalGet(0)),
            Ins::Op(I::BrTable(table, 1)),
            Ins::Op(I::End),
            Ins::Op(I::End),
        ];
        f.origins = None;
        validate_void_block_fixture(&f);
        assert!(batch_branch_shapes(&mut f));
        let mapped = f
            .body
            .iter()
            .find_map(|ins| match ins {
                Ins::Op(I::BrTable(id, default)) => Some((*id, *default)),
                _ => None,
            })
            .expect("the table remains");
        assert_eq!(f.label_list(mapped.0), [0, 0]);
        assert_eq!(mapped.1, 0);
        validate_void_block_fixture(&f);
    }
}
