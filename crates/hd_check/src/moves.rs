//! Moves (`ref.move`, #1137 and #1138, slices M1 and M2 of #1130): a
//! `&mut T` value is not copied, nor is a value of a type that holds one
//! ([`Ck::copies`]). A read of a whole such binding moves it where no `&mut`
//! type is expected: a `:=` or untyped `let`, a generic argument, a tuple
//! or optional part, a return. A read of a field that is not copied
//! moves the field out the same way, and the binding is then partly
//! moved: its other fields stay usable. A pattern that binds a payload
//! reads it as a field of its variant, so a match moves the payload out
//! the same way (`ref.move.pattern`). A site that expects a declared `&mut`
//! reborrows a `&mut` instead: a parameter, a typed `let`, an assignment,
//! a field declared `&mut`. A use of the binding after a move on some path
//! is `use-of-moved-value`, until an assignment gives it a value again. The
//! paths are the forward walk over the structured control flow that the
//! ownership passes use (`hd_tir::own::flow`).

use hd_base::{HashMap, HashSet, LocalId, SliceSort as _};
use hd_diag::Code;
use hd_resolve::{Item, ItemData};
use hd_syntax::SyntaxKind;
use hd_tir::Callee;
use hd_tir::ir::{Body, NONE, PlaceRoot, Proj, Ref, Role, Tag, Topology};
use hd_tir::own::flow::{BitArena, Flow, Forward, Suites, forward};
use hd_types::{CopyDecl, ParamRef, Ty, TyData, Types};

use crate::body::Ck;

mod out_of_place;

/// What [`Ck::check_moves`] finds for the exclusivity rules.
#[derive(Default)]
pub(crate) struct MoveFacts {
    /// The reads of a whole `&mut` binding or field that a typed `let`,
    /// an assignment or a field declared `&mut` reborrows, each a loan
    /// like `&mut *c` (`ref.move.reborrow.excl`).
    pub(crate) reborrows: HashSet<u32>,
    /// The reads that move a binding or a part of it (`ref.move.sites`,
    /// `ref.move.field`, `ref.move.pattern`), which `ref.excl.move` checks.
    pub(crate) moves: HashSet<u32>,
}

/// An instruction index as a word.
fn w(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// Whether `t`, under any `mut`, is `&mut`: a local reference, since a
/// `*mut` is copied (`ref.move.mut`).
pub(crate) fn is_mut_ref(pool: Types<'_>, t: Ty) -> bool {
    match pool.get(t) {
        TyData::Ref {
            mutable, counted, ..
        } => mutable && !counted,
        _ => false,
    }
}

/// A member type's own answer ([`blocker`]): the blocker it holds, if
/// any, and whether that answer depends on the body (`open`).
#[derive(Clone, Copy)]
pub struct CopyAnswer {
    pub blocker: Option<Ty>,
    pub open: bool,
}

type CopyContext<'w, 'p> = (
    Types<'p>,
    &'w dyn Fn(hd_base::DefId) -> Option<&'p Item>,
    &'w [Ty],
    bool, // compiler ownership queries reborrow local-reference bits
);

/// The checker's Copy rule for compiler ownership after checking. Local
/// references are reborrowed bits here; the checker already checked their
/// exclusive moves. Nominal `!Copy` and open parameter rules remain shared.
/// Callers share the checker's published closed answers and cache each newly
/// answered member. The same iterative walk handles instantiated types not
/// visited while checking a generic body; no second ownership rule is needed.
pub fn copy_answer<'p>(
    pool: Types<'p>,
    item: &dyn Fn(hd_base::DefId) -> Option<&'p Item>,
    t: Ty,
    normalize: &mut dyn FnMut(Ty) -> Ty,
    known: &mut dyn FnMut(Ty) -> Option<CopyAnswer>,
    done: &mut dyn FnMut(Ty, CopyAnswer),
) -> CopyAnswer {
    known(t).unwrap_or_else(|| blocker((pool, item, &[], true), t, normalize, known, done))
}

/// One type the walk of [`blocker`] has entered and not finished: its
/// member types not yet asked, its answer so far, and the shallowest
/// stack level a member on the stack (a cycle) reached.
struct CopyFrame {
    t: Ty,
    members: Vec<Ty>,
    next: usize,
    answer: CopyAnswer,
    low: usize,
}

/// What `t` holds by itself, before its member types: a blocker, or the
/// member types to ask in field order. A `*mut T` and a `&T` copy.
fn copy_step(cx: CopyContext<'_, '_>, t: Ty) -> Result<Option<Ty>, Vec<Ty>> {
    let (pool, find, relaxed, reborrow) = cx;
    match pool.get(t) {
        TyData::Ref {
            mutable, counted, ..
        } => Ok((mutable && !counted && !reborrow).then_some(t)),
        TyData::Tuple { elems, rest } => {
            Err(pool.list_items(elems).iter().copied().chain(rest).collect())
        }
        TyData::Union(ms) => Err(pool.list_items(ms).to_vec()),
        TyData::Param(_) if relaxed.contains(&t) => Ok(Some(t)),
        TyData::Param(p) => Ok((!param_copies((pool, find), p)).then_some(t)),
        TyData::Assoc { assoc, .. } => {
            let copies = find(assoc).is_some_and(|item| match &item.data {
                ItemData::AssocType { bounds, .. } => bounds.iter().any(|&bound| {
                    pool.trait_value(bound).is_some_and(|(trait_, _)| {
                        find(trait_).is_some_and(hd_resolve::Item::requires_copy)
                    })
                }),
                _ => false,
            });
            Ok((!copies).then_some(t))
        }
        TyData::Adt { def, args } => {
            let Some(item) = find(def) else {
                return Ok(None);
            };
            match item.copy_decl() {
                CopyDecl::Copies => Ok(None),
                CopyDecl::NotCopy(_) => Ok(Some(t)),
                CopyDecl::Members(ms) => {
                    let argv = pool.list_items(args);
                    Err(ms
                        .iter()
                        .map(|&m| {
                            pool.subst(m, &|p| {
                                (p.owner == def)
                                    .then(|| argv.get(p.index as usize).copied())
                                    .flatten()
                            })
                        })
                        .collect())
                }
            }
        }
        // Values that copy, or hold no value of their own here.
        TyData::Prim(_)
        | TyData::Never
        | TyData::Poison
        | TyData::Fn { .. }
        | TyData::TraitValue { .. }
        | TyData::Opaque { .. }
        | TyData::Const(_)
        | TyData::Infer(_)
        | TyData::Canon(_)
        | TyData::Row(_)
        | TyData::Context(_) => Ok(None),
    }
}

/// The part of `t` that keeps a value of it from being copied
/// (`ref.move.holds`, `ref.copy.structure`): a `&mut`, a type declared
/// `!Copy`, or a type parameter that is not `Copy`, held by value in a
/// struct, enum, tuple, optional or union: the first one in field order.
/// `relaxed` lists the body's local functions' `?Copy` parameters.
///
/// An iterative post-order over the member types, as rustc's
/// `is_copy_raw` query answers each type once and caches it: each type
/// entered gets its own answer, its first member's blocker in field order
/// or none, and `done` receives it, clear or blocked, so every level of a
/// nest is answered by one walk and a later question about any of them is
/// one probe. `known` answers a member type asked before, and a known
/// answer is not walked again. A type's answer is `open` when it met a
/// type parameter or a projection, whose answer depends on the body. A
/// member already on the stack (a type that holds itself, which the
/// value rule rejects) counts as clear, as a depth-first walk's visited
/// set did; the types of that cycle do not reach `done`, since their
/// answer assumed it. Work: one step per distinct member type entered,
/// at any depth, with no native recursion.
fn blocker(
    cx: CopyContext<'_, '_>,
    t: Ty,
    normalize: &mut dyn FnMut(Ty) -> Ty,
    known: &mut dyn FnMut(Ty) -> Option<CopyAnswer>,
    done: &mut dyn FnMut(Ty, CopyAnswer),
) -> CopyAnswer {
    let mut walk = CopyWalk {
        cx,
        normalize,
        known,
        done,
        finished: HashMap::default(),
        on_stack: HashMap::default(),
        stack: Vec::new(),
    };
    match walk.enter(t) {
        Some((a, _)) => a,
        None => walk.run(),
    }
}

/// The state of one [`blocker`] walk.
struct CopyWalk<'w, 'p> {
    cx: CopyContext<'w, 'p>,
    normalize: &'w mut dyn FnMut(Ty) -> Ty,
    known: &'w mut dyn FnMut(Ty) -> Option<CopyAnswer>,
    done: &'w mut dyn FnMut(Ty, CopyAnswer),
    /// Each type answered by this walk, for a type met twice, with the
    /// shallowest stack level its answer assumed (`usize::MAX`: none).
    finished: HashMap<Ty, (CopyAnswer, usize)>,
    /// Each type entered and not finished, by its stack level.
    on_stack: HashMap<Ty, usize>,
    stack: Vec<CopyFrame>,
}

impl CopyWalk<'_, '_> {
    /// Asks member type `u`: its answer and the shallowest stack level it
    /// assumed, or `None` when it was pushed to be walked.
    fn enter(&mut self, u: Ty) -> Option<(CopyAnswer, usize)> {
        crate::work::scan_work(1);
        let pool = self.cx.0;
        let mut open = false;
        let u = if matches!(pool.get(u), TyData::Assoc { .. }) {
            open = true;
            (self.normalize)(u)
        } else {
            u
        };
        let depth = self.stack.len();
        let answered = if let Some(&d) = self.on_stack.get(&u) {
            Some((
                CopyAnswer {
                    blocker: None,
                    open,
                },
                d,
            ))
        } else if let Some(&a) = self.finished.get(&u) {
            Some(a)
        } else if depth > 0
            && let Some(a) = (self.known)(u)
        {
            Some((a, usize::MAX))
        } else {
            None
        };
        if let Some((a, low)) = answered {
            let open = a.open || open;
            return Some((
                CopyAnswer {
                    blocker: a.blocker,
                    open,
                },
                low,
            ));
        }
        // Count actual member expansion, after all memo/cycle answers.
        // Both checker and ownership queries now use this shared walk.
        hd_types::work::type_node();
        open |= matches!(pool.get(u), TyData::Param(_) | TyData::Assoc { .. });
        match copy_step(self.cx, u) {
            Ok(blocker) => {
                let a = CopyAnswer { blocker, open };
                self.finished.insert(u, (a, usize::MAX));
                (self.done)(u, a);
                Some((a, usize::MAX))
            }
            Err(members) => {
                self.on_stack.insert(u, depth);
                self.stack.push(CopyFrame {
                    t: u,
                    members,
                    next: 0,
                    answer: CopyAnswer {
                        blocker: None,
                        open,
                    },
                    low: depth,
                });
                None
            }
        }
    }

    /// Walks the frames on the stack to the root's answer: each frame
    /// takes its member's answer, then asks its next member or finishes.
    fn run(&mut self) -> CopyAnswer {
        let mut result: Option<(CopyAnswer, usize)> = None;
        let mut root = CopyAnswer {
            blocker: None,
            open: false,
        };
        while let Some(mut top) = self.stack.pop() {
            if let Some((a, low)) = result.take() {
                top.answer.open |= a.open;
                top.low = top.low.min(low);
                if a.blocker.is_some() {
                    top.answer.blocker = a.blocker;
                    // An ownership query keeps looking past a ?Copy blocker:
                    // a concrete !Copy part makes every instance noncopyable.
                    if !self.cx.3
                        || a.blocker.is_some_and(|t| {
                            !matches!(self.cx.0.get(t), TyData::Param(_) | TyData::Assoc { .. })
                        })
                    {
                        top.next = top.members.len();
                    }
                }
            }
            if top.next < top.members.len() {
                let u = top.members[top.next];
                top.next += 1;
                self.stack.push(top);
                result = self.enter(u);
                continue;
            }
            self.on_stack.remove(&top.t);
            // A clear answer that assumed a type still on the stack is
            // clear only if that type is: it stays out of the memo.
            let low = if top.low >= self.stack.len() || top.answer.blocker.is_some() {
                (self.done)(top.t, top.answer);
                usize::MAX
            } else {
                top.low
            };
            self.finished.insert(top.t, (top.answer, low));
            root = top.answer;
            result = Some((top.answer, low));
        }
        root
    }
}

/// Whether type parameter `p` is `Copy`: it has a bound on a trait that
/// implies it, as every parameter has unless declared `?Copy`
/// (`ref.copy.generic.default`, `ref.copy.trait.default`). A trait's
/// `Self` is `Copy` unless the trait is declared `< ?Copy`.
fn param_copies<'p>(
    (pool, find): (Types<'p>, &dyn Fn(hd_base::DefId) -> Option<&'p Item>),
    p: ParamRef,
) -> bool {
    let Some(item) = find(p.owner) else {
        return true;
    };
    if let ItemData::Trait(t) = &item.data
        && p.index == 0
    {
        return !t.relaxed_copy;
    }
    let generics = item.sig().map_or(&item.generics[..], |s| &s.generics[..]);
    let at = usize::from(p.index) - usize::from(matches!(item.data, ItemData::Trait(_)));
    generics.get(at).is_none_or(|g| {
        g.bounds.iter().any(|&b| {
            pool.trait_value(b)
                .is_some_and(|(d, _)| find(d).is_some_and(hd_resolve::Item::requires_copy))
        })
    })
}

/// The local and whether the place is the whole local, for a place word
/// rooted at a local.
fn local_of(b: &Body, word: u32) -> Option<(LocalId, bool)> {
    let p = b.decode_place(word)?;
    match p.root {
        PlaceRoot::Local(l) => Some((l, p.proj.is_empty())),
        _ => None,
    }
}

/// What a consuming use does with a value it is given
/// (`Ck::consume_moves`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Consumed {
    Moves,
    /// A store or construction that reborrows a `&mut`.
    Stays,
    /// A store to a whole hidden local: its reads decide.
    Hidden(LocalId),
}

/// What [`Ck::moves`] follows a value through: the facts of one body
/// that both move checks read, built once per body ([`Ck::move_base`]).
/// The users and hidden reads wait for a check that follows a value.
pub(crate) struct Uses {
    q: CopyQuery,
    copied: Vec<bool>,
    topo: Topology,
    /// Each value's users, with the role of each use.
    users: Vec<Vec<(u32, Role)>>,
    hidden: Vec<bool>,
    hidden_reads: HashMap<LocalId, Vec<u32>>,
    /// Whether a sub-body's result moves its value: the caller takes it.
    /// Use-after-move needs no such move, since nothing runs after it.
    root_result_moves: bool,
}

impl Uses {
    fn follow(&mut self, b: &Body, root_result_moves: bool) {
        self.root_result_moves = root_result_moves;
        if self.users.len() != b.len() {
            self.users = out_of_place::users_of(b);
            self.hidden_reads = out_of_place::hidden_reads_of(b, &self.hidden);
        }
    }
}

/// The binding a place reaches and the field path to it: a read of
/// `h.a.b` is a read of `h` and two field reads, each rooted at the last.
struct Access {
    local: LocalId,
    path: Vec<u32>,
    /// Whether the place is the field itself, not a place behind a
    /// dereference or an element of it.
    exact: bool,
}

/// The transfer of the move walk: one bit per binding or field path that
/// some read moves (`ref.move.field`), set while it may have moved.
struct Walk<'x> {
    acc: &'x [Option<Access>],
    /// The reads that only lead to a field read of the same binding.
    link: &'x [bool],
    /// The bit each moving read sets, or `NONE`.
    move_bit: &'x [u32],
    /// The bits each crossing closure's creation sets.
    closure_moves: &'x HashMap<u32, Vec<u32>>,
    /// The binding and path of each bit.
    bits: &'x [(LocalId, Vec<u32>)],
    of_local: &'x HashMap<LocalId, Vec<u32>>,
    /// The first use after a move of each binding, and the bit of the
    /// move it reaches.
    found: &'x mut HashMap<LocalId, (u32, u32)>,
}

impl Walk<'_> {
    /// A use of `path` of `l` conflicts with a moved path that contains
    /// it or lies inside it; with `strict`, only with one that contains
    /// it, as an assignment to the path needs.
    fn use_of(&mut self, l: LocalId, path: &[u32], strict: bool, x: u32, s: &BitArena, st: usize) {
        for &k in self.of_local.get(&l).into_iter().flatten() {
            let p = &self.bits[k as usize].1;
            let hit = if strict {
                p.len() < path.len() && path.starts_with(p)
            } else {
                path.starts_with(p) || p.starts_with(path)
            };
            // A loop's later round may reach an earlier use.
            if hit && s.get(st, k) {
                let at = self.found.entry(l).or_insert((x, k));
                if x < at.0 {
                    *at = (x, k);
                }
            }
        }
    }
}

impl Forward for Walk<'_> {
    const SUITES: Suites = Suites::FromExits;

    fn stmt(&mut self, b: &Body, x: u32, s: &mut BitArena, st: usize) {
        let i = x as usize;
        match b.tags[i] {
            tag @ (Tag::Read | Tag::Ref | Tag::Store) => {
                let Some(a) = &self.acc[i] else {
                    return;
                };
                if self.link[i] {
                    return;
                }
                // `ref.move.reassign`: storing a whole value at a moved
                // path makes it usable again.
                if tag == Tag::Store && a.exact {
                    self.use_of(a.local, &a.path, true, x, s, st);
                    for &k in self.of_local.get(&a.local).into_iter().flatten() {
                        if self.bits[k as usize].1.starts_with(&a.path) {
                            s.set(st, k, false);
                        }
                    }
                    return;
                }
                self.use_of(a.local, &a.path, false, x, s, st);
                if self.move_bit[i] != NONE {
                    s.set(st, self.move_bit[i], true);
                }
            }
            // A closure's captures use the locals it captures.
            Tag::Closure if !b.is_instantiation(i) => {
                if let [start, len, ..] = *b.record(b.data[i][1]) {
                    for c in start..start.saturating_add(len) {
                        if let Some(&l) = b.cap_local.get(c as usize) {
                            self.use_of(l, &[], false, x, s, st);
                        }
                    }
                }
                for &k in self.closure_moves.get(&x).into_iter().flatten() {
                    s.set(st, k, true);
                }
            }
            Tag::ItemRef
            | Tag::ProviderGet
            | Tag::Hole
            | Tag::Poison
            | Tag::Prim
            | Tag::And
            | Tag::Or
            | Tag::Call
            | Tag::CallValue
            | Tag::CallDyn
            | Tag::Is
            | Tag::CallHost
            | Tag::Intrinsic
            | Tag::DefaultCall
            | Tag::Coerce
            | Tag::Await
            | Tag::AwaitValue
            | Tag::AwaitAll
            | Tag::AwaitRace
            | Tag::Hook
            | Tag::NewStruct
            | Tag::CopyStruct
            | Tag::NewVariant
            | Tag::NewTuple
            | Tag::NewList
            | Tag::NewMap
            | Tag::Closure
            | Tag::RefDyn
            | Tag::RefIdentity
            | Tag::Drop
            | Tag::IsUnique
            | Tag::FlagSet
            | Tag::FlagClear
            | Tag::Box
            | Tag::With
            | Tag::ContextNew
            | Tag::ContextFor
            | Tag::Block
            | Tag::Scope
            | Tag::Defer
            | Tag::If
            | Tag::Loop
            | Tag::Break
            | Tag::Continue
            | Tag::Return
            | Tag::Unreachable
            | Tag::Match
            | Tag::SwitchTag
            | Tag::SwitchInt
            | Tag::SwitchChar
            | Tag::SwitchStr
            | Tag::Guard
            | Tag::ToArm
            | Tag::LoanEnd
            | Tag::Retain
            | Tag::Release
            | Tag::MakeUnique
            | Tag::LoanBegin
            | Tag::LoanClose
            | Tag::OwnLoans => {}
        }
    }

    fn enter(&mut self, _: &Body, _: u32, _: &mut BitArena, _: usize) {}
}

/// The mark of a variant step in a field path, beside its index: a
/// match arm's payload is field `j` of variant `k`, the path `[k | VARIANT, j]`.
const VARIANT: u32 = 1 << 31;

/// The leading field and variant projections of `proj`, and whether they
/// are all.
fn field_prefix(proj: &[Proj]) -> (Vec<u32>, bool) {
    let mut path = Vec::new();
    for p in proj {
        match *p {
            Proj::Field(k) => path.push(k),
            Proj::Variant(k) => path.push(k | VARIANT),
            _ => return (path, false),
        }
    }
    (path, true)
}

/// One pass's copy questions ([`Ck::copy_query`]): the body's `?Copy`
/// local parameters, gathered once, and each type's answer, so a pass
/// that asks about every value walks each distinct type once. Valid while
/// the body's local functions and inference stay as they are.
pub(crate) struct CopyQuery {
    relaxed: Vec<Ty>,
    /// Each type's answer in this body, and whether it depends on the
    /// body ([`CopyAnswer`]).
    memo: HashMap<Ty, CopyAnswer>,
    /// The value rule's decisions under this body's lookup alone
    /// (`crate::value::holds_itself`).
    inline: crate::value::InlineDecls,
}

impl Ck<'_, '_> {
    /// Resolve associated projections before the immutable move walk.
    /// A substituted field can name a concrete associated type even when
    /// the enclosing type's argument list contains no projection.
    ///
    /// Cost: one memo probe per value; one walk per distinct type.
    fn copy_facts(&mut self, q: &mut CopyQuery) -> Vec<bool> {
        (0..self.b.body().len())
            .map(|i| {
                let b = self.b.body();
                let (tag, ty) = (b.tags[i], b.ty[i]);
                (tag == Tag::Read || tag == Tag::Coerce && b.row_adaptation(i).is_none())
                    && self.copies_in(q, ty)
            })
            .collect()
    }

    /// A query for one pass over the body's values ([`CopyQuery`]).
    pub(crate) fn copy_query(&self) -> CopyQuery {
        CopyQuery {
            relaxed: self.relaxed_local_params(),
            memo: HashMap::default(),
            inline: crate::value::InlineDecls::default(),
        }
    }

    /// Whether a value of `t` is copied ([`blocker`]).
    pub(crate) fn copies(&mut self, t: Ty) -> bool {
        let mut q = self.copy_query();
        self.copies_in(&mut q, t)
    }

    /// Whether a value of `t` is copied, as one question of `q`'s pass.
    pub(crate) fn copies_in(&mut self, q: &mut CopyQuery, t: Ty) -> bool {
        self.not_copied_in(q, t).is_none()
    }

    /// The part of `t` that keeps it from being copied ([`blocker`]).
    fn not_copied_by(&mut self, t: Ty) -> Option<Ty> {
        let mut q = self.copy_query();
        self.not_copied_in(&mut q, t)
    }

    /// The part of `t` that keeps it from being copied, from `q`'s memo,
    /// then the run's memo of closed types, then one walk ([`blocker`]).
    /// Every type the walk answers enters `q`'s memo, and one whose
    /// answer met no parameter or projection, with no inference variable
    /// or body-local part, is published to the run's memo, so a wide or
    /// deep type is walked once per run, not once per body or per level.
    fn not_copied_in(&mut self, q: &mut CopyQuery, t: Ty) -> Option<Ty> {
        if let Some(a) = q.memo.get(&t) {
            return a.blocker;
        }
        let pool = self.pool();
        let closed_ty = |u: Ty| !u.is_local() && !pool.has_infer(u) && !pool.has_canon(u);
        if closed_ty(t)
            && let Some(blocker) = self.cx.global.copy_blocker(t)
        {
            q.memo.insert(
                t,
                CopyAnswer {
                    blocker,
                    open: false,
                },
            );
            return blocker;
        }
        let global = self.cx.global;
        let CopyQuery {
            relaxed,
            memo,
            inline,
        } = q;
        let lookup = self.cx.lookup;
        let mut answered: Vec<(Ty, CopyAnswer)> = Vec::new();
        let a = blocker(
            (pool, &|d| lookup.item(d), relaxed, false),
            t,
            &mut |ty| self.norm_ty(ty),
            &mut |u| {
                memo.get(&u)
                    .copied()
                    .or_else(|| {
                        closed_ty(u)
                            .then(|| global.copy_blocker(u))
                            .flatten()
                            .map(|blocker| CopyAnswer {
                                blocker,
                                open: false,
                            })
                    })
                    // An instance of a declaration the value rule rejects
                    // is not walked: its members may grow without end
                    // (`crate::value::holds_itself`), and it is reported.
                    .or_else(|| {
                        matches!(pool.get(u), TyData::Adt { def, .. }
                            if crate::value::holds_itself(global, inline, pool, lookup, def))
                        .then_some(CopyAnswer {
                            blocker: None,
                            open: false,
                        })
                    })
            },
            &mut |u, a| answered.push((u, a)),
        );
        // In this body each answer holds whatever it met; in every body
        // when it met no parameter or projection.
        memo.extend(answered.iter().copied());
        memo.insert(t, a);
        global.publish_copy_blockers(
            (answered.iter().copied().chain([(t, a)]))
                .filter(|&(u, a)| !a.open && closed_ty(u))
                .map(|(u, a)| (u, a.blocker)),
        );
        a.blocker
    }

    /// The type parameters of the body's generic local functions declared
    /// `?Copy`: their bounds hold no trait that implies `Copy`
    /// (`ref.copy.generic.relaxed`).
    fn relaxed_local_params(&self) -> Vec<Ty> {
        let pool = self.pool();
        let lookup = self.cx.lookup;
        let copies = |b: &Ty| {
            pool.trait_value(*b)
                .is_some_and(|(d, _)| lookup.item(d).is_some_and(hd_resolve::Item::requires_copy))
        };
        (self.local_generics.values())
            .flat_map(|g| g.params.iter().zip(&g.generics))
            .filter(|(_, generic)| !generic.row && !generic.bounds.iter().any(copies))
            .map(|(&p, _)| p)
            .collect()
    }

    /// Why a value of `t` is not copied, for a message: the `&mut`, the
    /// `!Copy` type or the `?Copy` parameter it holds.
    pub(crate) fn why_not_copied(&mut self, t: Ty) -> String {
        let pool = self.pool();
        let Some(part) = self.not_copied_by(t) else {
            return "its type is not copied".to_string();
        };
        let shown = hd_resolve::show_ty(&self.cx.names, part);
        match pool.get(part) {
            TyData::Adt { .. } if part == t => {
                format!("its type `{shown}` is declared `impl !Copy`, so it is not copied")
            }
            TyData::Adt { .. } => {
                format!("its type holds `{shown}`, declared `impl !Copy`, so it is not copied")
            }
            TyData::Param(p)
                if p.index == 0
                    && self
                        .cx
                        .lookup
                        .item(p.owner)
                        .is_some_and(|i| matches!(i.data, ItemData::Trait(_))) =>
            {
                "its type holds the `Self` of a trait declared `< ?Copy`, so it is not copied"
                    .to_string()
            }
            TyData::Param(_) => {
                format!("its type holds `{shown}`, declared `?Copy`, so it is not copied")
            }
            TyData::Assoc { .. } => {
                format!("its associated type `{shown}` has no `Copy` bound, so it is not copied")
            }
            _ => "its type holds a `&mut`, so it is not copied".to_string(),
        }
    }

    /// The body's [`Uses`]; `None` for a body with an error, whose
    /// poison values say nothing about moves.
    pub(crate) fn move_base(&mut self) -> Option<Uses> {
        if self.b.body().tags.contains(&Tag::Poison) {
            return None;
        }
        let mut q = self.copy_query();
        Some(Uses {
            copied: self.copy_facts(&mut q),
            q,
            topo: Topology::new(self.b.body()),
            users: Vec::new(),
            hidden: self.hidden_locals(self.b.body()),
            hidden_reads: HashMap::default(),
            root_result_moves: false,
        })
    }

    /// `ref.move`: each use of a binding that is not copied, after its
    /// value moved on some path, is `use-of-moved-value`, reported once
    /// per binding. Returns the reads that move ([`MoveFacts`]).
    pub(crate) fn check_moves(&mut self, root: Ref, cx: &mut Uses) -> MoveFacts {
        let pool = self.pool();
        let mut facts = MoveFacts::default();
        // A crossing parameter is called at most once: its call moves it
        // (`fn.crossing.once`).
        let crossing: Vec<LocalId> = self.crossing_params.iter().map(|&(l, _)| l).collect();
        let held: Vec<bool> = (0..self.b.body().local_ty.len())
            .map(|i| {
                let ty = self.b.body().local_ty[i];
                crossing.iter().any(|l| l.idx() == i) || !self.copies_in(&mut cx.q, ty)
            })
            .collect();
        if !(0..held.len()).any(|l| held[l] && !cx.hidden[l]) {
            return facts;
        }
        let b = self.b.body();
        let n = b.len();
        cx.follow(b, false);
        let tracked = |l: LocalId| held[l.idx()] && !cx.hidden[l.idx()];
        // The binding and field path each place reaches.
        let mut acc: Vec<Option<Access>> = Vec::with_capacity(n);
        for i in 0..n {
            let a = (matches!(b.tags[i], Tag::Read | Tag::Ref | Tag::Store))
                .then(|| b.decode_place(b.data[i][0]))
                .flatten()
                .and_then(|p| {
                    let (more, exact) = field_prefix(&p.proj);
                    let (local, mut path) = match p.root {
                        PlaceRoot::Local(l) if tracked(l) => (l, Vec::new()),
                        PlaceRoot::Value(r) => match acc.get(r.0 as usize) {
                            Some(Some(a)) if a.exact && b.tags[r.0 as usize] == Tag::Read => {
                                (a.local, a.path.clone())
                            }
                            _ => return None,
                        },
                        _ => return None,
                    };
                    path.extend(more);
                    Some(Access { local, path, exact })
                });
            acc.push(a);
        }
        // A read whose every use is a place rooted at it is not a use of
        // its own; the field reads at the ends are.
        let link: Vec<bool> = (0..n)
            .map(|i| {
                b.tags[i] == Tag::Read
                    && acc[i].is_some()
                    && !cx.users[i].is_empty()
                    && cx.users[i].iter().all(|&(u, _)| {
                        acc[u as usize].is_some()
                            && b.decode_place(b.data[u as usize][0])
                                .is_some_and(|p| p.root == PlaceRoot::Value(Ref(w(i))))
                    })
            })
            .collect();
        // A read of the binding, or of a field of it that is not copied,
        // moves it or the field where it moves (`ref.move.field`).
        let mut bits: Vec<(LocalId, Vec<u32>)> = Vec::new();
        let mut bit_of: HashMap<(LocalId, Vec<u32>), u32> = HashMap::default();
        let mut of_local: HashMap<LocalId, Vec<u32>> = HashMap::default();
        let mut move_bit = vec![NONE; n];
        for i in 0..n {
            let Some(a) = &acc[i] else { continue };
            let field_copies = !a.path.is_empty() && cx.copied[i];
            let called = crossing.contains(&a.local)
                && cx.users[i].iter().any(|&(u, _)| {
                    b.tags[u as usize] == Tag::CallValue && b.data[u as usize][0] == w(i)
                });
            if b.tags[i] != Tag::Read
                || link[i]
                || !a.exact
                || field_copies
                || !(called || self.moves(cx, w(i)))
            {
                continue;
            }
            let key = (a.local, a.path.clone());
            let k = *bit_of.entry(key).or_insert_with_key(|key| {
                bits.push(key.clone());
                of_local.entry(a.local).or_default().push(w(bits.len() - 1));
                w(bits.len() - 1)
            });
            move_bit[i] = k;
        }
        for i in 0..n {
            if b.tags[i] == Tag::Read
                && acc[i].as_ref().is_some_and(|a| a.exact)
                && !link[i]
                && is_mut_ref(pool, b.ty[i])
                && self.reborrows(cx, w(i))
            {
                facts.reborrows.insert(w(i));
            }
        }
        // A crossing closure moves each capture that is not copied
        // (`fn.crossing.mode.move`): its source binding ends there.
        let mut closure_moves: HashMap<u32, Vec<u32>> = HashMap::default();
        for &(i, _) in &self.crossing_closures {
            if let [start, len, ..] = *b.record(b.data[i as usize][1]) {
                for c in start..start.saturating_add(len) {
                    let Some(&l) = b.cap_local.get(c as usize) else {
                        continue;
                    };
                    if !tracked(l) {
                        continue;
                    }
                    let k = *bit_of.entry((l, Vec::new())).or_insert_with_key(|key| {
                        bits.push(key.clone());
                        of_local.entry(l).or_default().push(w(bits.len() - 1));
                        w(bits.len() - 1)
                    });
                    closure_moves.entry(i).or_default().push(k);
                }
            }
        }
        if bits.is_empty() {
            return facts;
        }
        facts.moves = (0..n).filter(|&i| move_bit[i] != NONE).map(w).collect();
        let mut found = HashMap::default();
        let mut flow = Flow::default();
        flow.arena.words = bits.len().div_ceil(64);
        // The body's root is not yet in `sub_root`, which holds the
        // closures' roots.
        let roots = std::iter::once(root.0).chain(b.sub_root.iter().copied());
        for root in roots.filter(|&r| (r as usize) < n && b.tags[r as usize] == Tag::Block) {
            let st = flow.arena.alloc(None);
            let mut walk = Walk {
                acc: &acc,
                link: &link,
                closure_moves: &closure_moves,
                move_bit: &move_bit,
                bits: &bits,
                of_local: &of_local,
                found: &mut found,
            };
            forward(b, &cx.topo, &mut flow, &mut walk, root, st);
            flow.arena.free(st);
        }
        let mut found: Vec<(LocalId, (u32, u32))> = found.into_iter().collect();
        found.sort_stable_by_key(|&(_, (x, _))| x);
        for (l, (x, k)) in found {
            let path = &bits[k as usize].1;
            let b = self.b.body();
            let at = b.syn[x as usize];
            if at == hd_base::NodeIdx::NONE {
                continue;
            }
            let l = l.idx();
            let name = self.cx.names.text(b.local_name[l]).to_string();
            let msg = if path.iter().any(|&p| p & VARIANT != 0) {
                format!(
                    "a payload of `{name}` was moved out by a pattern before this use; assign the matched value again to use `{name}` whole"
                )
            } else if !path.is_empty() {
                format!(
                    "a field of `{name}` was moved out before this use; assign the field again to use `{name}` whole"
                )
            } else if crossing.iter().any(|c| c.idx() == l) {
                format!(
                    "`{name}` is a crossing parameter, which runs once; an earlier call moved it"
                )
            } else if is_mut_ref(self.pool(), b.local_ty[l]) {
                format!(
                    "`{name}` was moved before this use; pass `&mut *{name}` where it moves to keep using it"
                )
            } else {
                let why = self.why_not_copied(b.local_ty[l]);
                format!("`{name}` was moved before this use; {why}")
            };
            let node = self.cx.src.parse.tree.node(at);
            self.err(Code::UseOfMovedValue, node, &msg);
        }
        facts
    }

    /// Whether `&mut` value `v` reaches a site that reborrows it as
    /// `&mut` with a binding, a place or a field to hold it: a typed
    /// `let`, an assignment, or a field or payload declared `&mut`
    /// (`ref.move.reborrow.typed`). A call's reborrow ends when it
    /// returns, and a hidden local's value is its reads'.
    ///
    /// A work stack over the coercions `v` passes through, each value
    /// once: a body's value graph is as deep as its program, so a depth
    /// cap answered "does not reborrow" past it (#1777).
    fn reborrows(&self, cx: &Uses, v: u32) -> bool {
        let (pool, b) = (self.pool(), self.b.body());
        let mut stack: Vec<u32> = Vec::new();
        let mut seen: HashSet<u32> = HashSet::default();
        let mut next = Some(v);
        while let Some(v) = next.take().or_else(|| stack.pop()) {
            for &(u, role) in &cx.users[v as usize] {
                let i = u as usize;
                let found = match (b.tags[i], role) {
                    (Tag::Coerce, _) if b.row_adaptation(i).is_none() => {
                        if is_mut_ref(pool, b.ty[i]) && seen.insert(u) {
                            stack.push(u);
                        }
                        false
                    }
                    (Tag::Store, Role::Consume)
                        if local_of(b, b.data[i][0])
                            .is_some_and(|(h, whole)| whole && cx.hidden[h.idx()]) =>
                    {
                        false
                    }
                    (
                        Tag::Store | Tag::NewStruct | Tag::NewVariant | Tag::CopyStruct,
                        Role::Consume,
                    ) => self.consume_moves(cx, i, v) == Consumed::Stays,
                    _ => false,
                };
                if found {
                    return true;
                }
            }
        }
        false
    }

    /// Whether value `v`, which is not copied, moves where it is used
    /// (`ref.move.sites`, `ref.move.reborrow`).
    ///
    /// A work stack over the values `v` passes on to (coercions, a
    /// block's owner, a hidden local's reads), each once, which also ends
    /// a cycle through a hidden local: a body's value graph is as deep as
    /// its program, so a depth cap answered "does not move" past it, and
    /// a moved value's later use went unreported (#1777).
    fn moves(&self, cx: &Uses, v: u32) -> bool {
        let b = self.b.body();
        let mut stack: Vec<u32> = Vec::new();
        let mut seen: HashSet<u32> = HashSet::default();
        let mut pass = |u: u32, stack: &mut Vec<u32>| {
            if seen.insert(u) {
                stack.push(u);
            }
        };
        let mut next = Some(v);
        while let Some(v) = next.take().or_else(|| stack.pop()) {
            for &(u, role) in &cx.users[v as usize] {
                let i = u as usize;
                // A row adapter consumes its source into a new capture, rather
                // than passing the same value on to the wrapper's users.
                if b.tags[i] == Tag::Coerce && b.row_adaptation(i).is_none() {
                    // A coercion to `&T` reborrows; one that keeps a type
                    // that is not copied passes the value on.
                    if !cx.copied[i] {
                        pass(u, &mut stack);
                    }
                    continue;
                }
                let found = match role {
                    Role::Consume => match self.consume_moves(cx, i, v) {
                        Consumed::Moves => true,
                        Consumed::Stays => false,
                        Consumed::Hidden(h) => {
                            for &r in cx.hidden_reads.get(&h).into_iter().flatten() {
                                pass(r, &mut stack);
                            }
                            false
                        }
                    },
                    Role::Arg(k) => !self.param_is_mut(b, i, k as usize),
                    // A block's value is its owner's. A sub-body's root block
                    // gives its result to the caller ([`Uses::root_result_moves`]).
                    Role::Tail => {
                        let owner = cx.topo.owner[i];
                        if owner == NONE {
                            cx.root_result_moves
                        } else {
                            pass(owner, &mut stack);
                            false
                        }
                    }
                    _ => false,
                };
                if found {
                    return true;
                }
            }
        }
        false
    }

    /// Whether instruction `i`, which consumes value `v`, moves it: a
    /// store or a construction reborrows where a `&mut` type is expected
    /// (`ref.move.sites`, `ref.move.reborrow.typed`). A store to a whole
    /// hidden local passes the value on to its reads (`Hidden`).
    fn consume_moves(&self, cx: &Uses, i: usize, v: u32) -> Consumed {
        let (pool, b) = (self.pool(), self.b.body());
        let tag = b.tags[i];
        let moves = |m: bool| if m { Consumed::Moves } else { Consumed::Stays };
        match tag {
            Tag::Store => {
                let Some(p) = b.decode_place(b.data[i][0]) else {
                    return Consumed::Moves;
                };
                let (PlaceRoot::Local(h), true) = (p.root, p.proj.is_empty()) else {
                    return moves(!self.place_is_mut(b, &p));
                };
                // A hidden local passes the value on to its reads.
                if cx.hidden[h.idx()] {
                    return Consumed::Hidden(h);
                }
                // An assignment, or a `let` that writes the type, expects
                // the binding's `&mut` type; `:=` and a bare `let` do not.
                // Only a `&mut` reborrows (`ref.move.reborrow.typed`).
                if !is_mut_ref(pool, b.local_ty[h.idx()]) {
                    return Consumed::Moves;
                }
                let at = b.syn[i];
                if at == hd_base::NodeIdx::NONE {
                    return Consumed::Moves;
                }
                let n = self.cx.src.parse.tree.node(at);
                moves(
                    !(n.kind() == SyntaxKind::AssignmentStmt
                        || n.kind() == SyntaxKind::LetStmt
                            && n.children().any(|c| c.kind().is_type())),
                )
            }
            Tag::NewStruct | Tag::NewVariant | Tag::CopyStruct => {
                let rec = b.record(b.data[i][1]);
                let j = match tag {
                    Tag::CopyStruct => rec.chunks(2).find(|c| c.get(1) == Some(&v)).map(|c| c[0]),
                    _ => rec.iter().position(|&x| x == v).map(w),
                };
                let variant = (tag == Tag::NewVariant).then(|| b.data[i][0]);
                moves(
                    !(j.and_then(|j| self.field_decl(b.ty[i], variant, j)))
                        .is_some_and(|t| is_mut_ref(pool, t)),
                )
            }
            _ => Consumed::Moves,
        }
    }

    /// The declared type of field `j` of `t`, of variant `variant` for an
    /// enum: a struct's or an enum's, not a tuple's or an optional's.
    fn field_decl(&self, t: Ty, variant: Option<u32>, j: u32) -> Option<Ty> {
        let TyData::Adt { def, .. } = self.pool().get(t) else {
            return None;
        };
        let fields = match (&self.cx.lookup.item(def)?.data, variant) {
            (ItemData::Struct(fs, _), None) => fs,
            (
                ItemData::Enum {
                    shared, variants, ..
                },
                Some(k),
            ) if shared.is_empty() => &variants.get(k as usize)?.fields,
            _ => return None,
        };
        Some(fields.get(j as usize)?.ty)
    }

    /// Whether place `p`'s declared type is `&mut`: a dereference's
    /// referent or a struct field written `&mut`. An element is a type
    /// argument's; a field of a field is not typed here, and reborrows.
    fn place_is_mut(&self, b: &Body, p: &hd_tir::ir::Place) -> bool {
        let pool = self.pool();
        let mut t = match p.root {
            PlaceRoot::Local(l) => b.local_ty[l.idx()],
            PlaceRoot::Value(r) if (r.0 as usize) < b.len() => b.ty[r.0 as usize],
            _ => return true,
        };
        let mut variant = None;
        for (k, proj) in p.proj.iter().enumerate() {
            let last = k + 1 == p.proj.len();
            t = match (*proj, pool.get(t)) {
                (Proj::Deref, TyData::Ref { inner, .. }) => inner,
                (Proj::Variant(x), _) => {
                    variant = Some(x);
                    continue;
                }
                (Proj::Field(j), _) if last => {
                    return self
                        .field_decl(t, variant, j)
                        .is_some_and(|d| is_mut_ref(pool, d));
                }
                (Proj::Elem(_), _) => return !last,
                _ => return true,
            };
        }
        is_mut_ref(pool, t)
    }

    /// Whether call `i` declares argument `k`'s parameter `&mut`, so the
    /// argument is reborrowed (`ref.move.reborrow`). An unknown callee
    /// reborrows.
    fn param_is_mut(&self, b: &Body, i: usize, k: usize) -> bool {
        let pool = self.pool();
        let sig = |def| {
            self.sig_of(def)
                .map(|s| s.params.iter().map(|p| p.1).collect())
        };
        let params: Option<Vec<Ty>> = match b.tags[i] {
            Tag::Call | Tag::Await => match Callee::from_words(b.record(b.data[i][0])) {
                Some(Callee::Item { def, .. } | Callee::TraitMethod { method: def, .. }) => {
                    sig(def)
                }
                None => None,
            },
            // A dynamic call's arguments follow its receiver.
            Tag::CallDyn => (b.record(b.data[i][1]).first())
                .and_then(|&m| sig(hd_base::DefId::from_raw(m)))
                .map(|ps: Vec<Ty>| ps.into_iter().skip(1).collect()),
            Tag::CallValue => match b.ty.get(b.data[i][0] as usize).map(|&t| pool.get(t)) {
                Some(TyData::Fn { params, .. }) => Some(pool.list_items(params).to_vec()),
                _ => None,
            },
            _ => None,
        };
        // A variadic parameter takes the arguments past the last.
        params.is_none_or(|ps| ps.get(k).or(ps.last()).is_none_or(|&t| is_mut_ref(pool, t)))
    }
}
