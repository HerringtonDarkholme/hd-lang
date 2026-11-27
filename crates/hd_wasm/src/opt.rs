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
//!    `c` more offset; through a later `R; i32.add` (`x + c + y` is
//!    `x + y + c`) the constant reaches a store or a load of the sum;
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

pub(crate) mod bounds;
pub mod branch_shapes;
mod chains;

/// Select a checked or range-proven copy of an innermost counted loop.
pub(crate) fn version_loop_bounds(f: &mut Func) -> bool {
    bounds::run(f)
}
pub(crate) mod constant;
pub(crate) mod empty_loops;
pub(crate) mod live_set;

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
/// read sees the constant. Whether it folded any binding.
pub fn constants(
    funcs: &mut [Option<Func>],
    symbols: &Symbols,
    inits: &[Hash128],
    arity: &Arity,
) -> bool {
    // The init keys, indexed once: every key the caller and the entry
    // helpers name goes in one set, and each function asks it once, so
    // the pass costs the keys named plus the functions, not their product.
    let mut keys: hd_base::HashSet<Hash128> = inits.iter().copied().collect();
    let mut named = inits.len();
    for (id, _) in funcs.iter().enumerate().filter(|(_, f)| f.is_some()) {
        match &*symbols.get(SymId(u32::try_from(id).expect("symbols"))) {
            Sym::Helper(Helper::EntryInit { inits: ks }) => {
                named += ks.len();
                keys.extend(ks.iter().map(|k| k.0));
            }
            Sym::Helper(Helper::TestInit { inits: ks }) => {
                named += ks.iter().map(Vec::len).sum::<usize>();
                keys.extend(ks.iter().flatten().map(|k| k.0));
            }
            Sym::Helper(Helper::EntryPoll { then, .. }) => {
                named += then.len();
                keys.extend(then.iter().map(|k| k.0));
            }
            _ => {}
        }
    }
    crate::work::init_probes(named);
    let init_of = |id: SymId| {
        crate::work::init_probes(1);
        matches!(&*symbols.get(id), Sym::Inst(key) if keys.contains(key))
    };
    // A value held across the old storage's release reaches its global
    // through a temporary. Expose those constants before looking for the
    // single initializing write, rather than only cleaning locals afterward.
    for (id, f) in funcs
        .iter_mut()
        .enumerate()
        .filter_map(|(id, f)| Some((id, f.as_mut()?)))
    {
        // An index past u32 names no symbol, so it is no init.
        if u32::try_from(id).is_ok_and(|id| init_of(SymId(id))) {
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
        let init = init_of(id);
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
        return false;
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
    true
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
        )
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
    // A function's facts read only its body and the arities, so they are
    // built on the task's threads (#2392).
    let mut facts: Vec<Facts> = hd_sched::par_map(funcs, |f| {
        f.as_ref().map_or_else(Facts::default, |f| {
            Facts::new(f, arity, &constant_joins(&f.body))
        })
    });
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
            cf.fixed = fixed_locals(f, &params[c], &results, &cf.writes);
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
    /// The local writes' producers, which every look reads.
    writes: Writes,
    fixed: Vec<bool>,
}

impl Facts {
    fn new(f: &Func, arity: &Arity, joins: &[Option<usize>]) -> Self {
        let produced = |at: usize, depth: usize| {
            let at = producer(&f.body, at, depth, arity, &f.symbols);
            let from = at.and_then(|(p, _)| match f.body[p] {
                Ins::Call(id, _) => Some(id.idx()),
                _ => None,
            });
            Produced {
                at,
                from,
                proved: false,
            }
        };
        let mut facts = Facts {
            writes: Writes::new(f, arity, joins),
            ..Facts::default()
        };
        for (at, ins) in f.body.iter().enumerate() {
            if let Ins::Op(I::LocalSet(_) | I::LocalTee(_)) = ins {
                facts.writes_read.extend(produced(at, 0).from);
                continue;
            }
            let Ins::Call(id, _) = ins else { continue };
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

pub fn locals_with_work(f: &mut Func, arity: &Arity, visit_round: &mut impl FnMut(usize)) {
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
        // Zero's blocks serve Live when Zero leaves the body as it is.
        let mut cfg = None;
        changed |= settled.run(f, Rule::Zero, |f| zero_releases(f, &mut cfg, &mut || {}));
        changed |= settled.run(f, Rule::Live, |f| live_rules(f, arity, cfg.take()));
        if !changed {
            break;
        }
    }
    renumber(f);
}

/// The rule groups of [`locals`], each one call of its pass.
#[derive(Clone, Copy)]
pub enum Rule {
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
pub struct Settled {
    /// How many times a rule has edited the function.
    edits: usize,
    /// `clean[rule]`: the edit count at which that rule last found
    /// nothing to do, or `usize::MAX`.
    clean: [usize; 9],
}

impl Settled {
    #[must_use]
    #[cfg_attr(
        feature = "testing",
        expect(
            clippy::new_without_default,
            reason = "a fresh record is always built with `new`"
        )
    )]
    pub fn new() -> Self {
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
    pub fn run(&mut self, f: &mut Func, rule: Rule, pass: impl FnOnce(&mut Func) -> bool) -> bool {
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
pub enum ZeroEdges {
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
#[must_use]
pub fn zero_edges(body: &[Ins], label_lists: &[Vec<u32>]) -> Option<Vec<ZeroEdges>> {
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
}

/// What [`decide_block`] reads and writes of rule 11's facts as it walks
/// a block: [`Zeros`]' persistent sets, or a [`ZeroRow`].
trait ZeroFacts {
    fn has(&self, local: u32) -> bool;

    /// Records a write of `local`, of zero bits when `zero`.
    fn write(&mut self, local: u32, zero: bool);

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
        self.write(*l, zero);
    }
}

impl ZeroFacts for Zeros {
    fn has(&self, local: u32) -> bool {
        match self.wrote.get(local as usize) {
            Some(1) => true,
            Some(2) => false,
            _ => self.sets.contains(self.base, local),
        }
    }

    fn write(&mut self, local: u32, zero: bool) {
        let Some(slot) = self.wrote.get_mut(local as usize) else {
            return;
        };
        if *slot == 0 {
            self.touched.push(local);
        }
        *slot = if zero { 1 } else { 2 };
    }
}

/// The locals known zero at one point, a bit each ([`zero_rows`]).
struct ZeroRow(Vec<u64>);

impl ZeroFacts for ZeroRow {
    fn has(&self, local: u32) -> bool {
        let l = local as usize;
        self.0.get(l / 64).is_some_and(|w| w >> (l % 64) & 1 != 0)
    }

    fn write(&mut self, local: u32, zero: bool) {
        let l = local as usize;
        let Some(w) = self.0.get_mut(l / 64) else {
            return;
        };
        let bit = 1u64 << (l % 64);
        *w = if zero { *w | bit } else { *w & !bit };
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
/// every incoming path. Facts are dense rows ([`zero_rows`]) or, above
/// [`row_words`], persistent sets ([`Zeros`]) kept only at basic-block
/// entries; one working set carries facts through each block.
/// It calls `visit` at each block its fixed point visits, reads the body's
/// blocks from `cfg`, or makes them there, and empties `cfg` when it edits
/// the body.
pub fn zero_releases(f: &mut Func, cfg: &mut Option<Cfg>, visit: &mut dyn FnMut()) -> bool {
    let symbols = f.symbols.clone();
    let release_width = |ins: &Ins| {
        let Ins::Call(id, _) = ins else {
            return None;
        };
        match &*symbols.get(*id) {
            Sym::Helper(Helper::ReleaseRef | Helper::ReleaseKnown) => Some(1),
            Sym::Shim(_, false, width, false) => usize::try_from(*width).ok(),
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
    if f.body.is_empty() {
        return false;
    }
    let Some(g) = Cfg::of(f, cfg) else {
        return false;
    };
    let n = f.params.len() + f.locals.len();
    let gone = if let Some(words) = row_words(g.starts.len(), n, f.body.len()) {
        zero_rows(f, g, words, &release_width, visit)
    } else {
        zero_fixpoint(f, g, &release_width, visit)
    };
    if !gone.iter().any(|g| *g) {
        return false;
    }
    *cfg = None;
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

/// [`zero_releases`]' decisions on dense rows of known-zero locals,
/// `words` u64 per block ([`row_words`]): [`zero_fixpoint`]'s worklist,
/// lowest block first, where a write sets or clears one bit and a meet
/// is `words` of `and` that changed the facts when it cleared a bit, not
/// a path of `log2 n` nodes. A body that only branches forward visits
/// each block once.
fn zero_rows(
    f: &Func,
    Cfg {
        edges,
        point_at,
        starts,
        ..
    }: &Cfg,
    words: usize,
    release_width: &dyn Fn(&Ins) -> Option<usize>,
    visit: &mut dyn FnMut(),
) -> Vec<bool> {
    let (len, blocks) = (f.body.len(), starts.len());
    let mut facts = vec![0u64; blocks * words];
    // Every local but a parameter starts zero.
    for l in f.params.len()..f.params.len() + f.locals.len() {
        facts[l / 64] |= 1 << (l % 64);
    }
    let mut reached = vec![false; blocks];
    reached[0] = true;
    let mut queued = reached.clone();
    let mut pending = BinaryHeap::from([Reverse(0)]);
    let mut row = ZeroRow(vec![0; words]);
    let mut work = 0;
    while let Some(Reverse(p)) = pending.pop() {
        visit();
        queued[p] = false;
        let end = starts.get(p + 1).copied().unwrap_or(len);
        row.0.copy_from_slice(&facts[p * words..][..words]);
        for at in starts[p]..end {
            row.after(&f.body, at);
        }
        edges[end - 1].each(end, |to| {
            let Some(&q) = point_at.get(to) else {
                return;
            };
            work += words;
            let into = &mut facts[q * words..][..words];
            let changed = if std::mem::replace(&mut reached[q], true) {
                let mut cut = false;
                for (x, &y) in into.iter_mut().zip(&row.0) {
                    cut |= *x & !y != 0;
                    *x &= y;
                }
                cut
            } else {
                into.copy_from_slice(&row.0);
                true
            };
            if changed && !queued[q] {
                queued[q] = true;
                pending.push(Reverse(q));
            }
        });
    }
    crate::work::zero_facts(work);
    let mut gone = vec![false; len];
    for p in (0..blocks).filter(|&p| reached[p]) {
        row.0.copy_from_slice(&facts[p * words..][..words]);
        let end = starts.get(p + 1).copied().unwrap_or(len);
        decide_block(f, starts[p]..end, &mut row, &mut gone, release_width);
    }
    gone
}

/// The words of a dense row of `locals` bits, when a row per block of
/// `blocks` takes at most four words per instruction of a body of `len`:
/// storage and a pass's work linear in the body, where persistent sets
/// pay `log2 n` nodes per access. Above it the persistent sets keep a
/// huge body from holding `n²` facts.
fn row_words(blocks: usize, locals: usize, len: usize) -> Option<usize> {
    let words = locals.div_ceil(64);
    (blocks.checked_mul(words)? <= 4 * len).then_some(words)
}

/// A body's basic blocks and their edges ([`zero_edges`],
/// [`basic_blocks`]), made once per sweep of [`locals`] for Zero and
/// Live while neither rule has edited the body.
pub struct Cfg {
    edges: Vec<ZeroEdges>,
    point_at: Vec<usize>,
    starts: Vec<usize>,
    /// Every edge leads to a later block ([`forward_only`]).
    forward: bool,
}

impl Cfg {
    /// `f`'s blocks, from `cfg` or made there; `None` when its labels
    /// do not nest.
    fn of<'a>(f: &Func, cfg: &'a mut Option<Self>) -> Option<&'a Self> {
        if cfg.is_none() {
            let edges = zero_edges(&f.body, &f.labels)?;
            let (point_at, starts) = basic_blocks(&edges, f.body.len());
            let forward = forward_only(&edges, &point_at, &starts, f.body.len());
            *cfg = Some(Self {
                edges,
                point_at,
                starts,
                forward,
            });
        }
        cfg.as_ref()
    }
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

/// [`zero_releases`]' decisions on persistent sets ([`Zeros`]), for a
/// body whose dense rows would not fit: the facts at each block's entry
/// to a fixed point, then the decisions.
///
/// The worklist hands out the lowest queued block first, as clang's
/// `ForwardDataflowWorklist` hands out blocks in reverse post-order.
/// Blocks are numbered in body order, where every edge but a loop's
/// backedge leads forward, so a loop settles before the blocks after it
/// are visited, and each of those is visited once, not once more per
/// round of the loops before it: `O(V log B)` heap steps for `V` visits.
fn zero_fixpoint(
    f: &Func,
    Cfg {
        edges,
        point_at,
        starts,
        ..
    }: &Cfg,
    release_width: &dyn Fn(&Ins) -> Option<usize>,
    visit: &mut dyn FnMut(),
) -> Vec<bool> {
    let n = f.params.len() + f.locals.len();
    let zeros = &mut Zeros::new(n);
    // Every local but a parameter starts zero: one set made in one pass.
    // ruled out: a local index past u32; Wasm counts locals in u32.
    let all: Vec<u32> = (f.params.len()..n)
        .map(|l| u32::try_from(l).expect("locals"))
        .collect();
    // `None`: not reached yet. A function without locals has only empty
    // sets but can still have removable releases.
    let mut facts: Vec<Option<Set>> = vec![None; starts.len()];
    facts[0] = Some(zeros.sets.of_sorted(&all));
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
        zeros.enter(known);
        decide_block(f, start..end, zeros, &mut gone, release_width);
    }
    gone
}

/// Marks in `gone` the releases and stores of `block` that its entry
/// facts, entered in `zeros`, prove redundant, carrying the facts
/// through it ([`Zeros::leave`] makes its exit set).
fn decide_block(
    f: &Func,
    block: std::ops::Range<usize>,
    zeros: &mut impl ZeroFacts,
    gone: &mut [bool],
    release_width: &dyn Fn(&Ins) -> Option<usize>,
) {
    for at in block {
        let ins = &f.body[at];
        if let Some(n) = release_width(ins) {
            let mut args = Vec::new();
            for p in (0..at).rev() {
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
            && let Some(p) = at.checked_sub(1)
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
#[must_use]
pub fn basic_blocks(edges: &[ZeroEdges], len: usize) -> (Vec<usize>, Vec<usize>) {
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
/// marks kills within a block. [`Live::dead_accesses`] solves the
/// same equations on dense bit rows ([`Live::rows`]) when they fit.
pub struct Live {
    edges: Vec<ZeroEdges>,
    point_at: Vec<usize>,
    pub starts: Vec<usize>,
    /// Every edge leads to a later block, so one backward pass settles.
    pub forward: bool,
    pub sets: Sets,
    pub into: Vec<Set>,
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
    pub fn new(f: &Func, sets: Sets) -> Option<Self> {
        let mut me = Self::blocks(f, None, sets)?;
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
    pub fn settle_with(&mut self, f: &Func, mut visit: impl FnMut()) {
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
    pub fn blocks(f: &Func, mut cfg: Option<Cfg>, sets: Sets) -> Option<Self> {
        let n = f.params.len() + f.locals.len();
        if n == 0 || f.body.is_empty() {
            return None;
        }
        Cfg::of(f, &mut cfg)?;
        let Cfg {
            edges,
            point_at,
            starts,
            forward,
        } = cfg?;
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
            forward,
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
    ///
    /// The live-out sets are dense rows ([`Live::rows`]) when they fit,
    /// and persistent sets above that.
    #[must_use]
    pub fn dead_accesses(f: &Func, cfg: Option<Cfg>) -> Option<Vec<bool>> {
        let body = &f.body;
        let mut me = Self::blocks(f, cfg, Sets::unconsed())?;
        let rows = me.rows(f);
        if rows.is_none() {
            me.settle(f);
        }
        let live_out = |p: usize, l: u32| match &rows {
            Some(rows) => rows.has(p, l),
            None => me.sets.contains(me.outs[p], l),
        };
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
                    !live_out(p, l)
                };
                later[i] = p;
                next_writes[i] = write;
            }
        }
        Some(dead)
    }

    /// The locals live out of each block as dense bit rows, when the rows
    /// take at most four words per instruction of the body, as the live
    /// sets of production allocators are bit rows. The pass is
    /// regalloc2's `compute_liveness`: a FIFO worklist seeded in
    /// postorder (here the blocks last to first, as every edge but a
    /// backedge leads forward), each visit unioning the block's live-in
    /// row into its predecessors' live-out rows and queueing a
    /// predecessor whose row grew. In a body that only branches forward
    /// each block is visited once. A visit costs `W` words per
    /// predecessor and one bit per first access, with no allocation, so
    /// the pass is `O(V W)` words within the `4 L` the threshold allows,
    /// where a persistent set pays a path of `log2 n` nodes per access.
    fn rows(&self, f: &Func) -> Option<Rows> {
        let len = f.body.len();
        let blocks = self.starts.len();
        let words = row_words(blocks, f.params.len() + f.locals.len(), len)?;
        let (first, preds) = self.predecessors(len);
        let mut bits = vec![0u64; blocks * words];
        let mut queue: std::collections::VecDeque<usize> = (0..blocks).rev().collect();
        let mut queued = vec![true; blocks];
        let mut live = vec![0u64; words];
        let mut work = 0;
        while let Some(p) = queue.pop_front() {
            queued[p] = false;
            live.copy_from_slice(&bits[p * words..][..words]);
            for &(l, reads) in &self.firsts[self.first[p]..self.first[p + 1]] {
                let (w, bit) = (l as usize / 64, 1u64 << (l % 64));
                live[w] = if reads { live[w] | bit } else { live[w] & !bit };
            }
            for &q in &preds[first[p]..first[p + 1]] {
                work += words;
                let mut grew = false;
                for (out, &x) in bits[q * words..][..words].iter_mut().zip(&live) {
                    grew |= x & !*out != 0;
                    *out |= x;
                }
                if grew && !queued[q] {
                    queued[q] = true;
                    queue.push_back(q);
                }
            }
        }
        crate::work::liveness(work);
        Some(Rows { words, bits })
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

/// Each block's live-out locals, `words` u64 per block ([`Live::rows`]).
struct Rows {
    words: usize,
    bits: Vec<u64>,
}

impl Rows {
    fn has(&self, p: usize, l: u32) -> bool {
        let l = l as usize;
        self.bits[p * self.words + l / 64] >> (l % 64) & 1 != 0
    }
}

/// The locals of suspending body `f` live just after its instruction at
/// `at`, ascending, where a `Save`, which returns at once, reads none:
/// what a run from there may read before it writes. `None` when the
/// body's branches are not analysed.
///
/// One query needs no live set per block. A local is live after `at`
/// exactly when some block reachable from there reads it first while
/// some path to that block leaves it unwritten. So one forward pass from
/// `at`, blocks in body order, carries what every path to a block wrote
/// (a must-write set, met by intersection), and a block's first reads
/// outside that set are live. Structured Wasm is reducible and every
/// edge but a loop's backedge leads forward, so a block's forward
/// predecessors are final when it is visited. A backedge to a loop
/// entered at its header after `at` brings nothing: the run through the
/// loop wrote at least what the header's entry facts hold, and a meet
/// with a superset changes nothing (a "fast" framework, Graham and
/// Wegman 1976: a loop's closure of a write-only transfer is the
/// identity). Only a loop holding `at` re-enters at its header with new
/// facts, what every path to its backedges wrote; it is walked once
/// more from its header, innermost first, each header once.
///
/// So the query costs one pass over the blocks after `at` and one per
/// loop holding `at`, with no depth factor for the loops after it,
/// where the full backward fixed point ([`Live::settle`]) visits a
/// loop's blocks once more per level of loops around them (#2401), as
/// regalloc2's `compute_liveness` does: a FIFO worklist seeded in
/// postorder, as many rounds as the loops nest. The SSA loop-forest
/// pass (Brandner et al., INRIA RR-7503) settles in two passes, but
/// only because an SSA value live into a header is live in its whole
/// loop; a Wasm local written inside the loop is not.
#[must_use]
pub fn live_after(f: &Func, at: usize) -> Option<Vec<u32>> {
    let edges = zero_edges(&f.body, &f.labels)?;
    let len = f.body.len();
    let (point_at, starts) = basic_blocks(&edges, len);
    let blocks = starts.len();
    let end = |p: usize| starts.get(p + 1).copied().unwrap_or(len);
    let mut sets = Sets::unconsed();
    let meet = |sets: &mut Sets, slot: &mut Option<Set>, w: Set| {
        *slot = Some(slot.map_or(w, |known| sets.intersection(known, w)));
    };
    let mut live = vec![false; f.params.len() + f.locals.len()];
    // What every path from the source to a block's entry wrote; `None`:
    // not reached from the source.
    let mut written: Vec<Option<Set>> = vec![None; blocks];
    // A loop holding the source: what every path to its backedges
    // wrote, by header block, and the headers still to walk from.
    let mut seeds: Vec<Option<Set>> = vec![None; blocks];
    let mut headers = std::collections::BinaryHeap::new();
    let mut work = 0;
    let mut source = Some((*point_at.get(at)?, at + 1, Set::EMPTY));
    while let Some((first, from, entry)) = source {
        written[first..].fill(None);
        written[first] = Some(entry);
        for p in first..blocks {
            let Some(mut w) = written[p] else {
                continue;
            };
            let start = if p == first { from } else { starts[p] };
            work += 1 + end(p) - start;
            for ins in &f.body[start..end(p)] {
                if let Ins::Op(I::LocalGet(l)) = *ins
                    && !live[l as usize]
                {
                    live[l as usize] = !sets.contains(w, l);
                } else if let Ins::Op(I::LocalSet(l) | I::LocalTee(l)) = *ins {
                    w = sets.insert(w, l);
                }
            }
            edges[end(p) - 1].each(end(p), |to| {
                let Some(&t) = point_at.get(to) else {
                    return; // The function's exit.
                };
                if t > p {
                    meet(&mut sets, &mut written[t], w);
                } else if t < first || (t == first && from != starts[first]) {
                    // A backedge of a loop holding the source.
                    if seeds[t].is_none() {
                        headers.push(t);
                    }
                    meet(&mut sets, &mut seeds[t], w);
                }
            });
        }
        // A pass from header `t` seeds only the loops around it, whose
        // headers come before it: the latest header first sees each of
        // its seeds final.
        source = headers
            .pop()
            .and_then(|t| seeds[t].map(|seed| (t, starts[t], seed)));
    }
    crate::work::liveness(work + sets.work());
    Some(
        (0u32..)
            .zip(live)
            .filter_map(|(l, x)| x.then_some(l))
            .collect(),
    )
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
#[must_use]
pub fn copy_sources(body: &[Ins], locals: usize) -> Vec<Option<(u32, usize)>> {
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
pub fn live_rules(f: &mut Func, arity: &Arity, cfg: Option<Cfg>) -> bool {
    let Some(mut dead) = Live::dead_accesses(f, cfg) else {
        return false;
    };
    let mut any = false;
    let mut changed: Option<Vec<usize>> = None;
    loop {
        // A whole round, uncounted, to check the round over changed spans.
        #[cfg(debug_assertions)]
        let whole = changed.as_ref().map(|_| {
            crate::work::count_liveness(|| {
                let mut g = f.clone();
                let d = live_once(&mut g, arity, &dead, None).map(|r| r.0);
                (d, format!("{:?}", g.body))
            })
            .0
        });
        let Some((next, edits)) = live_once(f, arity, &dead, changed.as_deref()) else {
            #[cfg(debug_assertions)]
            debug_assert!(
                whole.is_none_or(|(d, _)| d.is_none()),
                "rule 13's round over the changed spans missed a rewrite"
            );
            break;
        };
        #[cfg(debug_assertions)]
        debug_assert!(
            whole.is_none_or(|(d, b)| d.as_ref() == Some(&next) && b == format!("{:?}", f.body)),
            "rule 13's round over the changed spans differs from a whole round"
        );
        debug_assert!(
            Live::dead_accesses(f, None).is_none_or(|solved| solved == next),
            "rule 13's carried dead accesses differ from a new liveness pass"
        );
        dead = next;
        changed = Some(edits);
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
///
/// `changed`, from the round before, lists in order the positions that
/// round touched: a new or rewritten instruction, the first one after a
/// removed run (the body's length for its end), and one whose dead flag
/// moved. A candidate reads only its window, from its push or the start
/// of its row of sets to one past its get, and the round's state changes
/// only where it accepts. So a candidate whose window holds no touched
/// position reads what it read last round, where it was refused, and a
/// refusal that came from the state names an accepted position in the
/// window: it is refused again without its walks. A dead write sits at a
/// touched position too, and the new body is copied in runs between the
/// edits. A nest of `N` pairs, peeled one per round, then costs `N`
/// rounds of a few array passes, not `N` rounds of every candidate's
/// walks and a per-instruction rebuild.
pub fn live_once(
    f: &mut Func,
    arity: &Arity,
    dead: &[bool],
    changed: Option<&[usize]>,
) -> Option<(Vec<bool>, Vec<usize>)> {
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
    // The positions the rewrite edits, and the gets that take a push.
    let mut edits: Vec<usize> = Vec::new();
    let mut takers: Vec<usize> = Vec::new();
    let mut seen = vec![NONE; f.params.len() + f.locals.len()];
    for (at, ins) in body.iter().enumerate().rev() {
        if let Ins::Op(I::LocalSet(l) | I::LocalTee(l) | I::LocalGet(l)) = ins {
            next[at] = seen[*l as usize];
            seen[*l as usize] = u32::try_from(at).expect("body");
        }
    }
    // The start of the row of sets that ends at `k`, as `matching_push`
    // walks it.
    let mut row = 0;
    for k in 0..body.len() {
        let tee = match body[k] {
            Ins::Op(I::LocalSet(_)) => false,
            Ins::Op(I::LocalTee(_)) => true,
            _ => continue,
        };
        if tee || k == 0 || !matches!(body[k - 1], Ins::Op(I::LocalSet(_))) {
            row = k;
        }
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
        if let Some(changed) = changed {
            // The window's first position: the push a tee keeps, or the
            // first instruction `matching_push` reads.
            let lo = if tee {
                k.saturating_sub(1)
            } else {
                row.checked_sub(1 + (k - row))
                    .unwrap_or_else(|| row.saturating_sub(1))
            };
            let t = changed.partition_point(|&x| x < lo);
            if changed.get(t).is_none_or(|&x| x > j + 1) {
                continue;
            }
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
            if !tee {
                edits.push(p);
            }
            moved[j] = u32::try_from(p).expect("body");
            edits.push(j);
            takers.push(j);
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
            edits.push(j);
            open.push((k, j));
            if !last {
                teed[k] = true;
                edits.push(k);
                continue;
            }
        } else {
            continue;
        }
        gone[k] = true;
        edits.push(k);
    }
    // A dead write of the round before's body sits where it touched, as
    // the doc comment says; a whole round looks at every position.
    match changed {
        Some(changed) => edits.extend(
            changed
                .iter()
                .copied()
                .filter(|&at| at < body.len() && dead_write(at)),
        ),
        None => edits.extend((0..body.len()).filter(|&at| dead_write(at))),
    }
    if edits.is_empty() {
        return None;
    }
    edits.sort_stable();
    edits.dedup();
    // The dead accesses after the rewrites, by old position: a moved get
    // that reads no local is no access, and each union of one local's
    // move spans is settled as the doc comment says.
    let mut after = dead.to_vec();
    // The positions `after` may differ from `dead` at.
    let mut flipped: Vec<usize> = Vec::new();
    for &(_, _, j) in &reads_moved {
        after[j] = false;
        flipped.push(j);
    }
    for &at in &takers {
        if !matches!(body[moved[at] as usize], Ins::Op(I::LocalGet(_))) {
            after[at] = false;
            flipped.push(at);
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
            flipped.push(r as usize);
            r = next[r as usize];
        }
        after[hi] = last;
        flipped.push(hi);
    }
    flipped.sort_stable();
    flipped.dedup();
    flipped.retain(|&at| after[at] != dead[at]);
    let mut dead_after = Vec::with_capacity(body.len());
    // The new positions the next round reads again, as the doc comment
    // says: `out.len()` stands for the body's end.
    let mut touched: Vec<usize> = Vec::new();
    let mut flips = flipped.iter().copied().peekable();
    let mut gap = false;
    let mut start = 0;
    for at in edits.iter().copied().chain(std::iter::once(body.len())) {
        // A run the round left alone is copied whole: its dead accesses
        // are `after`'s, as none of it is a dead write.
        let base = out.len();
        out.extend_from_slice(&body[start..at]);
        origins.copy(start..at);
        dead_after.extend_from_slice(&after[start..at]);
        if gap && at > start {
            touched.push(base);
            gap = false;
        }
        while let Some(x) = flips.next_if(|&x| x < at) {
            if x >= start {
                touched.push(base + x - start);
            }
        }
        start = at + 1;
        let Some(ins) = body.get(at) else {
            break;
        };
        if gone[at] {
            gap = true;
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
        } else if matches!(ins, Ins::Op(I::LocalTee(_))) {
            // A dead tee leaves its value; a dead set drops it, and rule
            // 15 takes back a pure push just before it.
            dead_after.pop();
            gap = true;
            continue;
        } else {
            out.push(Ins::Op(I::Drop));
            origins.copy(at..at + 1);
        }
        touched.push(out.len() - 1);
        gap = false;
    }
    if gap {
        touched.push(out.len());
    }
    touched.dedup();
    f.body = out;
    origins.finish(f);
    Some((dead_after, touched))
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
            Ins::Op(I::Unreachable | I::I32Const(_) | I::I64Const(_)) | Ins::Call(_, _) => {}
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
pub fn forward(f: &mut Func, np: u32) -> bool {
    forward_with(f, np, || {})
}

/// O(instructions + locals + globals): index uses once, carry the next
/// clobber/control edge while walking backwards, then compact once.
pub fn forward_with(f: &mut Func, np: u32, mut visit: impl FnMut()) -> bool {
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
        if matches!(p, Ins::Call(_, _) | Ins::CallIndirect(_, _)) {
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
pub fn fold(f: &mut Func) -> bool {
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
pub fn drops(f: &mut Func) -> bool {
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
            .then_some(k + 1)
            .filter(|&e| matches!(b.get(e), Some(Ins::Op(I::End))));
        if end.is_some() {
            empty_end = end;
            changed = true;
        } else if !matches!(ins, Ins::Op(I::Drop)) {
            out.push(*ins);
            origins.copy(k..k + 1);
            continue;
        }
        // The values still to drop, the last one on top.
        let mut n = 1usize;
        while n > 0 {
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
    let traps_or_writes = matches!(
        i,
        I::I32DivS
            | I::I32DivU
            | I::I32RemS
            | I::I32RemU
            | I::I64DivS
            | I::I64DivU
            | I::I64RemS
            | I::I64RemU
            | I::LocalTee(_)
            | I::MemoryGrow(_)
    );
    match op_effect(i)? {
        (pops, 1) if !traps_or_writes => Some(pops),
        _ => None,
    }
}

/// Rules 8 and 9 operate on straight-line stack effects. Schedule the
/// single-use locals first so the store walk sees the resulting stack order.
pub fn schedule(f: &mut Func, np: u32, arity: &Arity) -> bool {
    schedule_with(f, np, arity, || {})
}

pub fn schedule_with(f: &mut Func, np: u32, arity: &Arity, mut visit: impl FnMut()) -> bool {
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
        } else if matches!(f.body[i], Ins::Call(_, _) | Ins::CallIndirect(_, _)) {
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
        // The top value's candidate: a load's address, or an addition's
        // right operand.
        let top = (pops > 0)
            .then(|| {
                active
                    .last()
                    .filter(|&&(d, _)| d == depth)
                    .map(|&(_, id)| id)
            })
            .flatten();
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
            && with_offset(&op, 0).is_some()
        {
            let mut address = if is_load(&op) { top } else { address };
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
        } else if matches!(f.body[i], Ins::Op(I::I32Add))
            && let Some(id) = address.or(top)
        {
            // `x + c + y` is `x + y + c`: the constant goes on to the load
            // or store of the sum.
            active.push((depth, id));
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
        Ins::GlobalSet(_) => (1, 0),
        Ins::Call(id, _) => match &*symbols.get(*id) {
            Sym::Import(im) => (im.params.len(), im.results.len()),
            _ => (*arity.get(id.idx())?)?,
        },
        Ins::CallIndirect(id, _) => {
            let signature = symbols.signature(*id);
            (signature.0.len() + 1, signature.1.len())
        }
        Ins::Op(op) => op_effect(op)?,
        _ => return None,
    })
}

/// What plain instruction `op` pops and pushes; `None` for control and
/// for anything this table does not know.
fn op_effect(op: &I) -> Option<(usize, usize)> {
    Some(match op {
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
pub fn exits(f: &mut Func) -> bool {
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

pub fn blocks_with(f: &mut Func, arity: &Arity, mut visit: impl FnMut()) -> bool {
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
pub fn shapes(f: &mut Func, np: u32, arity: &Arity) -> bool {
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
pub const OPERAND_REACH: usize = 128;

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
#[must_use]
pub fn ifs(b: &[Ins]) -> Vec<(usize, Option<usize>, usize)> {
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
        (Ins::GlobalGet(_), Ins::Call(_, _) | Ins::CallIndirect(..)) => true,
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
        let Ins::Call(id, _) = ins else {
            return None;
        };
        let (operands, results) = (*arity.get(id.idx())?)?;
        match (&*f.symbols.get(*id), operands, results) {
            (Sym::Inst(k), n, 0) if *k == retain => Some((n, n)),
            (
                Sym::Helper(
                    Helper::ReleaseRef
                    | Helper::RetainKnown
                    | Helper::ReleaseKnown
                    | Helper::RetainColl,
                )
                | Sym::Shim(_, _, _, false),
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
    let fixed = fixed_locals(f, static_params, &[], &Writes::new(f, arity, &joins));
    let mut found = Vec::new();
    let gone: Vec<(usize, usize)> = f
        .body
        .iter()
        .enumerate()
        .filter_map(|(k, ins)| {
            let (operands, drops) = rewrite(ins)?;
            (0..operands)
                .all(|d| {
                    found.clear();
                    sources(f, (k, d), arity, &joins, &mut found)
                        && all_constant(f, &found, &fixed, &[])
                })
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
    producer_with(body, at, depth, arity, symbols, |_, _| false)
}

/// One backward walk shared by straight-line queries and scalar joins.
/// A join callback stops the walk after recording the arms to visit.
fn producer_with(
    body: &[Ins],
    at: usize,
    depth: usize,
    arity: &Arity,
    symbols: &Symbols,
    mut join: impl FnMut(usize, usize) -> bool,
) -> Option<(usize, usize)> {
    let mut above = depth;
    for j in (at.saturating_sub(PRODUCER_REACH)..at).rev() {
        if join(j, above) {
            return None;
        }
        let (pops, pushes) = effect(&body[j], arity, symbols)?;
        if above >= pushes {
            above = above - pushes + pops;
        } else if matches!(body[j], Ins::Op(I::LocalTee(_))) {
            // ruled out: a nonzero depth here; LocalTee pushes one and
            // this branch requires above < pushes.
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
        Ins::Call(id, _) => results
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

/// Appends to `out` the producers the value `depth` places below the
/// stack top before `at` comes from: its one producer, or every arm's of
/// the scalar `if`s it joins (`constant_joins`). False, with `out` as it
/// was, where the walk cannot tell. It reads no fact, so a fixed point
/// over the facts walks each value once, as SCCP builds its def-use
/// graph once and iterates only the lattice.
fn sources(
    f: &Func,
    (at, depth): (usize, usize),
    arity: &Arity,
    joins: &[Option<usize>],
    out: &mut Vec<(usize, usize)>,
) -> bool {
    let start = out.len();
    let mut work = vec![(at, depth)];
    while let Some((at, depth)) = work.pop() {
        let mut joined = false;
        let found = producer_with(&f.body, at, depth, arity, &f.symbols, |p, above| {
            if above == 0
                && let Some(Some(otherwise)) = joins.get(p)
            {
                work.extend([(*otherwise, 0), (p, 0)]);
                joined = true;
            }
            joined
        });
        if let Some(found) = found {
            out.push(found);
        } else if !joined {
            out.truncate(start);
            return false;
        }
    }
    true
}

/// Whether every producer in `sources` gives a constant.
fn all_constant(
    f: &Func,
    sources: &[(usize, usize)],
    fixed: &[bool],
    results: &[Vec<bool>],
) -> bool {
    (sources.iter()).all(|&(p, result)| constant_at(&f.body[p], result, fixed, results))
}

/// Each local write's local and the range of [`Writes::sources`] its
/// value comes from, `None` where unknown ([`sources`]).
#[derive(Default)]
struct Writes {
    writes: Vec<(usize, Option<std::ops::Range<usize>>)>,
    sources: Vec<(usize, usize)>,
}

impl Writes {
    fn new(f: &Func, arity: &Arity, joins: &[Option<usize>]) -> Self {
        let mut me = Self::default();
        for (k, ins) in f.body.iter().enumerate() {
            if let Ins::Op(I::LocalSet(l) | I::LocalTee(l)) = ins {
                let start = me.sources.len();
                let known = sources(f, (k, 0), arity, joins, &mut me.sources);
                me.writes
                    .push((*l as usize, known.then_some(start..me.sources.len())));
            }
        }
        me
    }
}

fn fixed_locals(f: &Func, params: &[bool], results: &[Vec<bool>], writes: &Writes) -> Vec<bool> {
    let np = f.params.len();
    let mut fixed = vec![false; np + f.locals.len()];
    fixed[..np].copy_from_slice(&params[..np]);
    let mut grew = true;
    while grew {
        let mut open = vec![true; fixed.len()];
        for (l, from) in &writes.writes {
            let constant = from
                .clone()
                .is_some_and(|r| all_constant(f, &writes.sources[r], &fixed, results));
            if !constant {
                open[*l] = false;
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

/// Makes each direct call of a runtime helper or count shim with no effect
/// and no result the `drop` of its arguments. Such a function is one whose
/// body only reads and writes its locals and branches forward, or calls
/// another such function. A release helper or shim whose counted part
/// folded away is one. A body with a loop, a store, a
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
                    Sym::Helper(_) | Sym::Shim(..) | Sym::CellDrop(_)
                )
            {
                continue;
            }
            let quiet = f.body.iter().all(|ins| match ins {
                Ins::Call(id, _) => free.get(id.idx()).is_some_and(Option::is_some),
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
            Ins::Call(id, _) => free.get(id.idx()).copied().flatten(),
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
        matches!(i, Ins::Call(id, _) if matches!(&*f.symbol(*id), Sym::Helper(Helper::RetainKnown | Helper::ReleaseKnown)))
    }) {
        return false;
    }
    let (t, b, c) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    let body = std::mem::take(&mut f.body);
    let origins = f.origins.as_mut().map(std::mem::take);
    let outer = f.origin_at;
    let outer_anchor = f.at;
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
        f.at = ins.call_anchor();
        let known = match ins {
            Ins::Call(id, _) => match &*f.symbol(id) {
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
    f.at = outer_anchor;
    true
}
