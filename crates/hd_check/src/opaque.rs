//! Opens each declared opaque return occurrence independently while checking
//! its defining body. Callers retain the opaque descriptors in the header.

use crate::body::Ck;
use hd_base::StageResult;
use hd_resolve::view::Src;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_types::{Ty, TyData, VarKind};

#[derive(Clone, Copy)]
enum Mode {
    Open,
    Recursive,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ReturnQuery {
    Strict(Ty),
    Pair(Ty, Ty),
}

/// Read raw strict-slot occurrences, stopping at opaque and union boundaries.
/// Child queries are evaluated in order and stop at the first true answer.
fn strict_query(
    pool: hd_types::Types<'_>,
    table: &hd_types::InferTable,
    classes: &hd_base::HashSet<u32>,
    memo: &mut crate::type_query::TypeQueries<ReturnQuery>,
    ty: Ty,
) -> bool {
    enum Work {
        Enter(Ty),
        Next(Ty, std::vec::IntoIter<Ty>),
    }
    let mut todo = vec![Work::Enter(ty)];
    let mut answer = false;
    while let Some(work) = todo.pop() {
        match work {
            Work::Enter(ty) => {
                let key = ReturnQuery::Strict(ty);
                if let Some(cached) = memo.cached(key) {
                    answer = cached;
                    continue;
                }
                hd_types::work::type_node();
                if let Some(class) = table.class_of(pool, ty) {
                    memo.watch_class(key, class);
                    answer = classes.contains(&class);
                    memo.finish(key, answer);
                    continue;
                }
                if !pool.has_infer(ty)
                    || matches!(pool.get(ty), TyData::Opaque { .. } | TyData::Union(_))
                {
                    answer = false;
                    memo.finish(key, false);
                    continue;
                }
                let mut children = Vec::new();
                if let TyData::Fn {
                    params,
                    result,
                    row,
                    ..
                } = pool.get(ty)
                {
                    children.extend_from_slice(pool.list_items(params));
                    children.push(result);
                    children.push(pool.intern_ty(&TyData::Row(row)));
                } else {
                    pool.children(ty, &mut children);
                }
                answer = false;
                todo.push(Work::Next(ty, children.into_iter()));
            }
            Work::Next(ty, mut children) => {
                let key = ReturnQuery::Strict(ty);
                if !answer && let Some(child) = children.next() {
                    memo.link(ReturnQuery::Strict(child), key);
                    todo.push(Work::Next(ty, children));
                    todo.push(Work::Enter(child));
                } else {
                    memo.finish(key, answer);
                }
            }
        }
    }
    answer
}

impl Ck<'_, '_> {
    pub(crate) fn open_opaque_return(&mut self, ty: Ty) -> Ty {
        self.map_opaque_return(ty, Mode::Open)
    }

    pub(crate) fn recursive_opaque_return(&mut self, ty: Ty) -> Ty {
        self.map_opaque_return(ty, Mode::Recursive)
    }

    fn opaque_return_row(&mut self, row: hd_types::RowId, mode: Mode) -> hd_types::RowId {
        let pool = self.pool();
        let mut row = pool.row_data(row);
        row.keys = row
            .keys
            .into_iter()
            .map(|ty| self.map_opaque_return(ty, mode))
            .collect();
        pool.row(&row)
    }

    fn map_opaque_return(&mut self, ty: Ty, mode: Mode) -> Ty {
        let pool = self.pool();
        if !pool.has_opaque(ty) {
            return ty;
        }
        match pool.get(ty) {
            TyData::Opaque { bound, .. } => {
                if let Some(&(_, _, result)) = self.opaque_rets.iter().find(|&&(t, _, result)| {
                    t == ty && (matches!(mode, Mode::Open) || self.strict_rets.contains(&result))
                }) {
                    if matches!(mode, Mode::Recursive) {
                        self.recursive_returns.push(result);
                    }
                    return result;
                }
                if !matches!(mode, Mode::Open) {
                    return ty;
                }
                let result = self.infer.fresh(pool, VarKind::General);
                self.strict_rets.push(result);
                self.opaque_rets.push((ty, bound, result));
                result
            }
            _ => pool.map_children(ty, &mut OpaqueMap { ck: self, mode }),
        }
    }
    pub(crate) fn strict_opaque_slot(&self, ty: Ty) -> bool {
        if self.strict_rets.is_empty() {
            return false;
        }
        let pool = self.pool();
        let Some(class) = self.infer.class_of(pool, ty) else {
            return false;
        };
        self.strict_rets
            .iter()
            .any(|&slot| self.infer.class_of(pool, slot) == Some(class))
    }

    pub(crate) fn strict_opaque_return(&self, ty: Ty) -> bool {
        let pool = self.pool();
        if self.strict_rets.is_empty() || !pool.has_infer(ty) {
            return false;
        }
        if matches!(pool.get(ty), TyData::Infer(_)) {
            return self.strict_opaque_slot(ty);
        }
        let mut slots = self.strict_rets.iter().filter_map(|&slot| {
            hd_types::work::type_node();
            self.infer.class_of(pool, slot)
        });
        let mut classes = hd_base::HashSet::default();
        let mut seen = hd_base::HashSet::default();
        !hd_base::stack::all(ty, |ty, stack| {
            if !pool.has_infer(ty) || !seen.insert(ty) {
                return true;
            }
            hd_types::work::type_node();
            if let Some(class) = self.infer.class_of(pool, ty) {
                if classes.contains(&class) {
                    return false;
                }
                // Index lazily: an early match still reads only the slots
                // before it, and a false query reads each slot at most once.
                for candidate in slots.by_ref() {
                    if candidate == class {
                        return false;
                    }
                    classes.insert(candidate);
                }
            }
            let start = stack.len();
            match pool.get(ty) {
                TyData::Opaque { .. } | TyData::Union(_) => return true,
                _ => pool.children(ty, stack),
            }
            stack[start..].reverse();
            true
        })
    }

    /// Keep the chosen hidden types invariant inside ordinary outer
    /// conversions (for example an optional injection around the result).
    pub(crate) fn constrain_opaque_return(&mut self, got: Ty, want: Ty, node: NodeRef<'_>) -> bool {
        enum Work {
            Pair(Ty, Ty, Option<ReturnQuery>),
            Finish(ReturnQuery),
        }
        if self.strict_rets.is_empty() || !self.pool().has_infer(want) {
            return true;
        }
        let pool = self.pool();
        let mut classes: hd_base::HashSet<_> = self
            .strict_rets
            .iter()
            .filter_map(|&slot| self.infer.class_of(pool, slot))
            .collect();
        let mut cursor = self.infer.snapshot();
        let mut memo =
            crate::type_query::TypeQueries::<ReturnQuery>::new(&self.infer, self.env.changes());
        let mut layers = crate::type_query::TypeQueries::new(&self.infer, self.env.changes());
        let mut todo = vec![Work::Pair(got, want, None)];
        while let Some(work) = todo.pop() {
            // A merge can make its winning class strict without changing that
            // class's value. Wake its prior false strictness readers as well.
            for change in self.infer.changes_since(cursor) {
                if let hd_types::unify::ClassChange::Merged { from, into } = change
                    && classes.remove(&from)
                    && classes.insert(into)
                {
                    memo.changed_class(into);
                }
            }
            cursor = self.infer.snapshot();
            memo.sync(&self.infer, self.env.changes());
            let (got, want, parent) = match work {
                Work::Finish(key) => {
                    memo.finish(key, true);
                    continue;
                }
                Work::Pair(got, want, parent) => (got, want, parent),
            };
            let key = ReturnQuery::Pair(got, want);
            if let Some(parent) = parent {
                memo.link(key, parent);
            }
            if memo.cached(key) == Some(true) {
                continue;
            }
            hd_types::work::type_node();
            memo.link(ReturnQuery::Strict(want), key);
            if !strict_query(pool, &self.infer, &classes, &mut memo, want) {
                memo.finish(key, true);
                continue;
            }
            memo.observe(pool, &self.infer, got, key);
            memo.observe(pool, &self.infer, want, key);
            let got = self.infer.shallow(pool, got);
            if got == Ty::NEVER {
                memo.finish(key, true);
                continue;
            }
            if self
                .infer
                .class_of(pool, want)
                .is_some_and(|c| classes.contains(&c))
            {
                let got = self.norm_ty(got);
                let wanted = self.norm_ty(want);
                let snapshot = self.infer.snapshot();
                if self.infer.unify(pool, got, wanted).is_err() {
                    self.infer.rollback(snapshot);
                    let (expected, found) = self.show_pair(wanted, got);
                    self.err(hd_diag::Code::TypeMismatch, node,
                        &format!("all returns of `impl Trait` need one concrete type: expected {expected}, found {found}"));
                    return false;
                }
                todo.push(Work::Finish(key));
                continue;
            }
            let wanted = self.expected_shape(want);
            let incoming = self.expected_shape(got);
            todo.push(Work::Finish(key));
            if let Some(inner) = self.option_arg(wanted)
                && self.option_layers_in(incoming, &mut layers) + 1
                    == self.option_layers_in(wanted, &mut layers)
            {
                todo.push(Work::Pair(got, inner, Some(key)));
                continue;
            }
            let mut children = Vec::new();
            let mut list = |got, want| {
                children.extend(
                    pool.list_items(got)
                        .iter()
                        .zip(pool.list_items(want))
                        .map(|(&g, &w)| (g, w)),
                );
            };
            match (pool.get(incoming), pool.get(wanted)) {
                (TyData::Ref { inner: g, .. }, TyData::Ref { inner: w, .. }) => {
                    children.push((g, w));
                }
                (TyData::Adt { def: a, args: g }, TyData::Adt { def: b, args: w }) if a == b => {
                    list(g, w);
                }
                (TyData::Tuple { elems: g, rest: a }, TyData::Tuple { elems: w, rest: b }) => {
                    list(g, w);
                    if let (Some(g), Some(w)) = (a, b) {
                        children.push((g, w));
                    }
                }
                (
                    TyData::TraitValue {
                        def: a,
                        args: g,
                        bindings: ga,
                    },
                    TyData::TraitValue {
                        def: b,
                        args: w,
                        bindings: wa,
                    },
                ) if a == b => {
                    list(g, w);
                    children.extend(
                        ga.into_iter()
                            .zip(wa)
                            .filter_map(|((a, g), (b, w))| (a == b).then_some((g, w))),
                    );
                }
                _ => continue,
            }
            todo.extend(
                children
                    .into_iter()
                    .rev()
                    .map(|(g, w)| Work::Pair(g, w, Some(key))),
            );
        }
        true
    }

    pub(crate) fn local_return_annotation(
        &mut self,
        annotation: NodeRef<'_>,
        function: NodeRef<'_>,
        borrows: bool,
    ) -> StageResult<Ty> {
        let before = self
            .opaque_annotation
            .replace((function.index().raw() + 1, borrows));
        let result = self.ty_node(annotation);
        self.opaque_annotation = before;
        result
    }

    pub(crate) fn declared_opaque_annotation(&mut self, n: NodeRef<'_>) -> StageResult<Ty> {
        let Some((site, borrows)) = self.opaque_annotation else {
            self.err(
                hd_diag::Code::TypeMismatch,
                n,
                "`impl Trait` needs a named function's return annotation",
            );
            return Ok(Ty::POISON);
        };
        // An opaque type in the bound's arguments borrows nothing.
        self.opaque_annotation = Some((site + 1, false));
        let bound = match Src::child(n, SyntaxKind::NamedType) {
            Some(t) => self.ty_node_as(t, crate::ty::NameRole::Trait),
            None => Ok(Ty::POISON),
        };
        if let Some((s, _)) = self.opaque_annotation {
            self.opaque_annotation = Some((s, borrows));
        }
        let bound = bound?;
        let pool = self.pool();
        let args: Vec<Ty> = self
            .gens
            .iter()
            .map(|(name, ty)| {
                self.row_gens
                    .iter()
                    .find(|(s, _)| s == name)
                    .map_or(*ty, |(_, row)| {
                        pool.intern_ty(&TyData::Row(pool.row(&hd_types::RowData {
                            keys: vec![],
                            params: vec![*row],
                        })))
                    })
            })
            .collect();
        let owner = self.b.body_mut().item;
        let args = if let Some(hd_resolve::ItemData::Method { owner: parent, .. }) =
            self.cx.lookup.item(owner).map(|item| &item.data)
            && matches!(
                self.cx.lookup.item(*parent).map(|item| &item.data),
                Some(hd_resolve::ItemData::Trait(_))
            ) {
            self.self_ty.into_iter().chain(args).collect()
        } else {
            args
        };
        Ok(pool.intern_ty(&TyData::Opaque {
            owner,
            site,
            borrows,
            args: pool.list(&args),
            bound,
        }))
    }

    pub(crate) fn check_opaque_occurrences(
        &mut self,
        range: std::ops::Range<usize>,
        node: NodeRef<'_>,
    ) -> StageResult<()> {
        for index in range {
            let (_, bound, result) = self.opaque_rets[index];
            self.check_opaque_result(result, bound, node)?;
        }
        Ok(())
    }
}

/// [`Ck::map_opaque_return`] applied to each child of a type, and to each
/// row's keys.
struct OpaqueMap<'k, 'a, 'c> {
    ck: &'k mut Ck<'a, 'c>,
    mode: Mode,
}

impl hd_types::TyMap for OpaqueMap<'_, '_, '_> {
    fn ty(&mut self, t: Ty) -> Ty {
        self.ck.map_opaque_return(t, self.mode)
    }
    fn row(&mut self, r: hd_types::RowId) -> hd_types::RowId {
        self.ck.opaque_return_row(r, self.mode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::with_checker;

    #[test]
    fn opaque_constraints_share_mutating_subgraphs_at_depth_1000() {
        std::thread::Builder::new()
            // cost: one 96 KiB thread for generated return constraints.
            .stack_size(96 << 10)
            .spawn(|| {
                for depth in [100, 1_000] {
                    with_checker(|ck| {
                        let p = ck.pool();
                        let strict = ck.infer.fresh(p, VarKind::General);
                        ck.strict_rets.push(strict);
                        let (mut got, mut want) = (Ty::I32, strict);
                        for _ in 0..depth {
                            got = p.intern_ty(&TyData::Tuple {
                                elems: p.list(&[got, got]),
                                rest: None,
                            });
                            want = p.intern_ty(&TyData::Tuple {
                                elems: p.list(&[want, want]),
                                rest: None,
                            });
                        }
                        let node = ck.cx.src.parse.tree.root();
                        let (answer, work) = hd_types::work::count_type_nodes(|| {
                            ck.constrain_opaque_return(got, want, node)
                        });
                        assert!(answer);
                        assert_eq!(ck.infer.resolve(p, strict), Ty::I32);
                        assert!(
                            work <= 30 * depth + 30,
                            "linear affected graph work: {depth}: {work}"
                        );
                    });
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn opaque_constraints_follow_aliases_and_revisit_optional_paths() {
        with_checker(|ck| {
            let p = ck.pool();
            let strict = ck.infer.fresh(p, VarKind::General);
            let later = ck.infer.fresh(p, VarKind::General);
            let incoming = ck.infer.fresh(p, VarKind::General);
            ck.strict_rets.push(strict);
            let def = ck.cx.names.item("app.main", "Holder");
            let wrap = |t| {
                p.intern_ty(&TyData::Adt {
                    def,
                    args: p.list(&[t]),
                })
            };
            let tuple = |xs: &[Ty]| {
                p.intern_ty(&TyData::Tuple {
                    elems: p.list(xs),
                    rest: None,
                })
            };
            let got = tuple(&[wrap(incoming), later, wrap(incoming)]);
            let want = tuple(&[wrap(later), strict, wrap(later)]);
            let node = ck.cx.src.parse.tree.root();
            assert!(ck.constrain_opaque_return(got, want, node));
            assert_eq!(
                ck.infer.class_of(p, incoming),
                ck.infer.class_of(p, strict),
                "a later alias makes the repeated subtree strict"
            );
        });
        with_checker(|ck| {
            let p = ck.pool();
            let strict = ck.infer.fresh(p, VarKind::General);
            let other_strict = ck.infer.fresh(p, VarKind::General);
            let later = ck.infer.fresh(p, VarKind::General);
            let incoming = ck.infer.fresh(p, VarKind::General);
            ck.strict_rets.extend([strict, other_strict]);
            let option = |t| {
                p.intern_ty(&TyData::Adt {
                    def: ck.cx.names.known.option,
                    args: p.list(&[t]),
                })
            };
            ck.infer.unify(p, other_strict, option(Ty::I32)).unwrap();
            let tuple = |xs: &[Ty]| {
                p.intern_ty(&TyData::Tuple {
                    elems: p.list(xs),
                    rest: None,
                })
            };
            let got = tuple(&[option(incoming), incoming, later, option(incoming)]);
            let want = tuple(&[
                option(option(later)),
                other_strict,
                strict,
                option(option(later)),
            ]);
            let node = ck.cx.src.parse.tree.root();
            assert!(ck.constrain_opaque_return(got, want, node));
            assert_eq!(
                ck.infer.resolve(p, strict),
                Ty::I32,
                "defer optional alignment until that subtree is strict"
            );
        });
    }

    #[test]
    fn strict_returns_at_depth_1000_fit_a_small_stack() {
        std::thread::Builder::new()
            // cost: one 96 KiB thread for this test.
            .stack_size(96 << 10)
            .spawn(|| {
                with_checker(|ck| {
                    let pool = ck.pool();
                    let strict = ck.infer.fresh(pool, VarKind::General);
                    let other = ck.infer.fresh(pool, VarKind::General);
                    ck.strict_rets.push(strict);
                    for depth in [100, 1_000] {
                        let mut chain = [strict, other];
                        for _ in 0..depth {
                            for t in &mut chain {
                                *t = pool.intern_ty(&TyData::Tuple {
                                    elems: pool.list(&[*t, *t]),
                                    rest: None,
                                });
                            }
                        }
                        for (t, expected) in [(chain[0], true), (chain[1], false)] {
                            let (answer, work) =
                                hd_types::work::count_type_nodes(|| ck.strict_opaque_return(t));
                            assert_eq!(answer, expected);
                            assert_eq!(work, depth + 2, "one graph walk and one strict-slot read");
                        }
                    }
                    let owner = ck.cx.names.item("app.main", "Shape");
                    let row = pool.row(&hd_types::RowData {
                        keys: vec![strict],
                        params: vec![],
                    });
                    for t in [
                        pool.intern_ty(&TyData::Adt {
                            def: owner,
                            args: pool.list(&[strict]),
                        }),
                        pool.intern_ty(&TyData::Tuple {
                            elems: hd_types::TyList::EMPTY,
                            rest: Some(strict),
                        }),
                        pool.intern_ty(&TyData::Ref {
                            inner: strict,
                            mutable: false,
                            counted: false,
                        }),
                        pool.intern_ty(&TyData::Fn {
                            params: hd_types::TyList::EMPTY,
                            result: Ty::I32,
                            row,
                            suspends: false,
                            inputs: hd_types::Inputs::Fixed,
                            deps: hd_types::FnDeps::NONE,
                        }),
                        pool.intern_ty(&TyData::TraitValue {
                            def: owner,
                            args: hd_types::TyList::EMPTY,
                            bindings: vec![(owner, strict)],
                        }),
                        pool.intern_ty(&TyData::Assoc {
                            assoc: owner,
                            trait_: owner,
                            self_ty: strict,
                            args: hd_types::TyList::EMPTY,
                        }),
                        pool.intern_ty(&TyData::Row(row)),
                        pool.intern_ty(&TyData::Context(row)),
                    ] {
                        assert!(
                            ck.strict_opaque_return(t),
                            "the original holder forms are traversed"
                        );
                    }
                    let opaque = pool.intern_ty(&TyData::Opaque {
                        owner,
                        site: 0,
                        args: pool.list(&[strict]),
                        bound: strict,
                        borrows: false,
                    });
                    let union = pool.intern_ty(&TyData::Union(pool.list(&[strict, Ty::BOOL])));
                    for t in [opaque, union] {
                        assert!(
                            !ck.strict_opaque_return(t),
                            "the original opaque/union boundaries stay leaves"
                        );
                    }
                    let root = pool.intern_ty(&TyData::Tuple {
                        elems: pool.list(&[other]),
                        rest: None,
                    });
                    assert!(!ck.strict_opaque_return(root));
                    let mark = ck.infer.snapshot();
                    ck.infer.unify(pool, strict, other).unwrap();
                    assert!(ck.strict_opaque_return(root));
                    ck.infer.rollback(mark);
                    assert!(!ck.strict_opaque_return(root));
                    ck.infer.unify(pool, strict, other).unwrap();
                    assert!(
                        ck.strict_opaque_return(root),
                        "a new query sees merged classes"
                    );
                    ck.infer.unify(pool, other, Ty::I32).unwrap();
                    assert!(
                        ck.strict_opaque_return(root),
                        "a solved occurrence stays strict"
                    );
                });
            })
            .expect("thread")
            .join()
            .expect("no stack overflow");
    }

    #[test]
    fn strict_return_slot_index_is_lazy_and_linear() {
        for width in [100, 1_000] {
            with_checker(|ck| {
                let pool = ck.pool();
                for _ in 0..width {
                    let slot = ck.infer.fresh(pool, VarKind::General);
                    ck.strict_rets.push(slot);
                }
                let others: Vec<_> = (0..width)
                    .map(|_| ck.infer.fresh(pool, VarKind::General))
                    .collect();
                let root = pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&others),
                    rest: None,
                });
                let (answer, work) =
                    hd_types::work::count_type_nodes(|| ck.strict_opaque_return(root));
                assert!(!answer);
                assert_eq!(work, 2 * width + 1);
                let first = pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&[ck.strict_rets[0]]),
                    rest: None,
                });
                let (answer, work) =
                    hd_types::work::count_type_nodes(|| ck.strict_opaque_return(first));
                assert!(answer);
                assert_eq!(work, 3, "the first match does not index unrelated slots");
            });
        }
    }
}
