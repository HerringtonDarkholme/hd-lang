//! Requirement rows in bodies (type-checking.md §5.2 to §5.6): the
//! available-keys stack, closure rows, row subsumption, and the least
//! solution of a callee's row parameters.

use hd_diag::Code;
use hd_resolve::{FnSig, HeadKind, Src};
use hd_syntax::{NodeRef, SyntaxKind};
use hd_types::{RowData, RowId, RowParamRef, Ty, TyData, VarKind};

use crate::body::{Ck, RowFrame};

/// One requirement a body needs: a key or a row parameter.
#[derive(Clone, Copy)]
enum Need {
    Key(Ty),
    Param(RowParamRef),
}

impl Ck<'_, '_> {
    /// Whether the listed key `have` supplies the needed key `want`: the
    /// same key, or a key whose trait has `want`'s trait as a supertrait.
    fn key_supplies(&self, have: Ty, want: Ty) -> bool {
        if have == want {
            return true;
        }
        let pool = self.pool();
        match (pool.get(have), pool.get(want)) {
            (
                TyData::TraitValue {
                    def: d1,
                    args: a1,
                    bindings: b1,
                },
                TyData::TraitValue {
                    def: d2,
                    args: a2,
                    bindings: b2,
                },
            ) => {
                (d1 == d2 && a1 == a2 && (b1.is_empty() || b2.is_empty() || b1 == b2))
                    || (d1 != d2 && self.trait_extends(d1, d2, 0))
            }
            _ => false,
        }
    }

    /// `key_supplies`, solving type variables inside a generic key: the
    /// pattern `Repo[T]` against `Repo[User]` fixes `T`.
    fn key_fits(&mut self, have: Ty, want: Ty) -> bool {
        if self.key_supplies(have, want) {
            return true;
        }
        let pool = self.pool();
        let same_trait = matches!(
            (pool.get(have), pool.get(want)),
            (TyData::TraitValue { def: a, .. }, TyData::TraitValue { def: b, .. }) if a == b
        );
        if !same_trait || !(pool.has_infer(have) || pool.has_infer(want)) {
            return false;
        }
        let snap = self.infer.snapshot();
        if self.infer.unify(pool, have, want).is_ok() {
            return true;
        }
        self.infer.rollback(snap);
        false
    }

    fn row_supplies(&self, row: RowId, need: Need) -> bool {
        let d = self.pool().row_data(row);
        match need {
            Need::Key(k) => d.keys.iter().any(|h| self.key_supplies(*h, k)),
            Need::Param(p) => d.params.contains(&p),
        }
    }

    /// Finds `need` in the available-keys stack (`req.row.entail`); a
    /// closure that infers its row takes the key into it instead.
    fn available(&mut self, need: Need) -> bool {
        for i in (0..self.rows.len()).rev() {
            let supplied = match &self.rows[i] {
                RowFrame::With(r) => {
                    if self.row_supplies(*r, need) {
                        return true;
                    }
                    continue;
                }
                RowFrame::Any => return true,
                RowFrame::Profile { row, .. } => {
                    let row = *row;
                    let have = match need {
                        Need::Key(k) => self
                            .pool()
                            .row_data(row)
                            .keys
                            .into_iter()
                            .find(|h| self.key_supplies(*h, k)),
                        Need::Param(_) => None,
                    };
                    if let (Some(h), RowFrame::Profile { used, .. }) = (have, &mut self.rows[i]) {
                        used.keys.push(h);
                    }
                    have.is_some()
                }
                RowFrame::Declared(r)
                | RowFrame::Closure {
                    written: Some(r), ..
                } => self.row_supplies(*r, need),
                RowFrame::Closure { written: None, .. } => {
                    if let RowFrame::Closure { used, .. } = &mut self.rows[i] {
                        match need {
                            Need::Key(k) => used.keys.push(k),
                            Need::Param(p) => used.params.push(p),
                        }
                    }
                    true
                }
            };
            return supplied;
        }
        false
    }

    /// A key the body uses must be available (`req.row.set.call`):
    /// `missing-requirement` otherwise.
    pub(crate) fn require_key(&mut self, key: Ty, n: NodeRef<'_>) {
        if !self.available(Need::Key(key)) {
            let shown = hd_resolve::show_ty_in(&self.cx.names, self.pool(), key);
            // `module.testing.unit-row.test-runner`,
            // `module.testing.integration-row.unbound.error`.
            let msg = if matches!(self.rows.first(), Some(RowFrame::Profile { .. })) {
                format!(
                    "this needs `$ {shown}`, which the runner does not bind for this test case: bind it with $.with({shown}=...)"
                )
            } else {
                let note = self.expanded_row_note().unwrap_or_default();
                format!(
                    "this needs `$ {shown}`, which the enclosing function's row does not name{note}"
                )
            };
            self.err(Code::MissingRequirement, n, &msg);
        }
    }

    /// When the function's own row decides a missing key and its header
    /// writes the row with an alias: the row as written and its expanded
    /// keys (`req.row.alias.diagnostics`,
    /// `req.row.alias.diagnostics.expanded`).
    fn expanded_row_note(&self) -> Option<String> {
        let decided = self
            .rows
            .iter()
            .rev()
            .find(|f| !matches!(f, RowFrame::With(_)))?;
        let RowFrame::Declared(row) = decided else {
            return None;
        };
        let written = self.cx.src.parse.tree.node(self.written_row?);
        let uses_alias = written
            .children()
            .filter(|c| c.kind() == SyntaxKind::NamedType)
            .any(|c| {
                self.resolve_path(&self.segments(c))
                    .is_some_and(|d| self.kind_of_item(d) == Some(HeadKind::Alias))
            });
        if !uses_alias {
            return None;
        }
        let span = self.cx.src.span(written);
        let text = self.cx.src.text.get(span.lo as usize..span.hi as usize)?;
        let d = self.pool().row_data(*row);
        let mut keys: Vec<String> = d
            .keys
            .iter()
            .map(|k| hd_resolve::show_ty_in(&self.cx.names, self.pool(), *k))
            .collect();
        keys.sort();
        let mut params: Vec<String> = d
            .params
            .iter()
            .filter_map(|p| self.row_gens.iter().find(|(_, g)| g == p))
            .map(|(s, _)| self.cx.names.text(*s).to_owned())
            .collect();
        params.sort();
        keys.extend(params);
        let expanded = if keys.is_empty() {
            "$()".to_owned()
        } else {
            format!("$ {}", keys.join(" + "))
        };
        Some(format!("; `{text}` is `{expanded}`"))
    }

    /// Every key and row parameter of a callee's row, as seen from this
    /// call, must be available here.
    pub(crate) fn check_row(&mut self, row: RowId, n: NodeRef<'_>) {
        let pool = self.pool();
        let row = self.infer.resolve_row(pool, row);
        let mut d = pool.row_data(row);
        // Diagnostics in content order (scheduler.md §6.5), not by the
        // order this run interned the keys.
        let mut keys: Vec<(String, Ty)> = d
            .keys
            .into_iter()
            .filter(|k| matches!(pool.get(*k), TyData::TraitValue { .. }))
            .map(|k| (hd_resolve::show_ty_in(&self.cx.names, self.pool(), k), k))
            .collect();
        keys.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, k) in keys {
            self.require_key(k, n);
        }
        let gens = self.row_gens.clone();
        let name_of = |p: &RowParamRef| {
            gens.iter()
                .find(|(_, g)| g == p)
                .map_or_else(String::new, |(s, _)| self.cx.names.text(*s).to_owned())
        };
        d.params.sort_by_cached_key(&name_of);
        for p in d.params {
            // Only this body's own row parameters reach here substituted;
            // another owner's are a callee's that was not instantiated.
            if !self.row_gens.iter().any(|(_, g)| *g == p) {
                continue;
            }
            if !self.available(Need::Param(p)) {
                let name = self
                    .row_gens
                    .iter()
                    .find(|(_, g)| *g == p)
                    .map_or_else(String::new, |(s, _)| self.cx.names.text(*s).to_owned());
                let msg = format!(
                    "this needs the row `$ {name}`, which the enclosing function's row does not name"
                );
                self.err(Code::MissingRequirement, n, &msg);
            }
        }
    }

    /// A function value of row `got` checked against an expected function
    /// type of row `want`. A row variable still open in `want` takes the
    /// least solution (`req.row.least.solution`); otherwise `want` must
    /// entail every part of `got` (`req.row.subsume`), else `type-mismatch`.
    pub(crate) fn fit_row(&mut self, got: RowId, want: RowId, n: NodeRef<'_>, what: &str) {
        let pool = self.pool();
        let got = self.infer.resolve_row(pool, got);
        let want = self.infer.resolve_row(pool, want);
        if got == want {
            return;
        }
        let (g, w) = (pool.row_data(got), pool.row_data(want));
        let open: Vec<Ty> = w
            .keys
            .iter()
            .copied()
            .filter(|k| matches!(pool.get(*k), TyData::Infer(_)))
            .collect();
        let concrete: Vec<Ty> = w
            .keys
            .iter()
            .copied()
            .filter(|k| !matches!(pool.get(*k), TyData::Infer(_)))
            .collect();
        if let Some((first, rest)) = open.split_first() {
            // The least solution: what `got` lists beyond the pattern.
            let mut beyond = Vec::new();
            for k in &g.keys {
                if !concrete.iter().any(|c| self.key_fits(*c, *k)) {
                    beyond.push(*k);
                }
            }
            let sol = RowData {
                keys: beyond,
                params: g
                    .params
                    .iter()
                    .copied()
                    .filter(|p| !w.params.contains(p))
                    .collect(),
            };
            let sol = pool.intern_ty(&TyData::Row(pool.row(&sol)));
            let _ = self.infer.unify(pool, *first, sol);
            let empty = pool.intern_ty(&TyData::Row(RowId::EMPTY));
            for v in rest {
                let _ = self.infer.unify(pool, *v, empty);
            }
            return;
        }
        let mut missing = Vec::new();
        for k in &g.keys {
            if matches!(pool.get(*k), TyData::Infer(_)) {
                // An open row variable in the value's own row: the least
                // row that fits, the empty one.
                let empty = pool.intern_ty(&TyData::Row(RowId::EMPTY));
                let _ = self.infer.unify(pool, *k, empty);
                continue;
            }
            if !concrete.iter().any(|c| self.key_fits(*c, *k)) {
                missing.push(hd_resolve::show_ty_in(&self.cx.names, self.pool(), *k));
            }
        }
        for p in &g.params {
            if !w.params.contains(p) {
                let name = self.row_gens.iter().find(|(_, x)| x == p).map_or_else(
                    || "R".to_owned(),
                    |(s, _)| self.cx.names.text(*s).to_owned(),
                );
                missing.push(name);
            }
        }
        missing.sort();
        if let Some(k) = missing.first() {
            let msg = format!(
                "in {what}: the function's row lists `{k}`, which the expected row does not"
            );
            self.err(Code::TypeMismatch, n, &msg);
        }
    }

    /// The type a least-common-type site (`types.lct.sites`) joins its
    /// values into: the expected type, or else a fresh variable whose
    /// values take the union of their function rows. A variable not yet
    /// solved, such as a generic argument's, is no expected type: the site
    /// joins on its own and the caller solves the variable from the result.
    pub(crate) fn join_target(&mut self, want: Option<Ty>) -> Ty {
        let pool = self.pool();
        if let Some(w) = want
            && (!matches!(pool.get(self.infer.shallow(pool, w)), TyData::Infer(_))
                || self.joins.contains(&w))
        {
            return w;
        }
        let v = self.infer.fresh(pool, VarKind::General);
        self.joins.push(v);
        v
    }

    /// A value of type `got` joined the site variable `var`, whose type is
    /// already solved. When both are function types, the site's row
    /// becomes the union of the two rows (`req.row.union.sites.type`),
    /// and each value fits it by row subsumption. A value that only holds
    /// function values keeps its type (`req.row.union.sites.direct`).
    pub(crate) fn join_row(&mut self, var: Ty, got: Ty) {
        let pool = self.pool();
        let cur = self.infer.shallow(pool, var);
        let got = self.strip_mut(self.infer.shallow(pool, got));
        let (
            TyData::Fn {
                params,
                result,
                suspends,
                vararg,
                row,
            },
            TyData::Fn { row: more, .. },
        ) = (pool.get(cur), pool.get(got))
        else {
            return;
        };
        if !self.joins.contains(&var) {
            return;
        }
        let row = self.infer.resolve_row(pool, row);
        let mut union = pool.row_data(row);
        let more = pool.row_data(self.infer.resolve_row(pool, more));
        union.keys.extend(more.keys);
        union.params.extend(more.params);
        let union = pool.row(&union);
        if union != row {
            let widened = pool.intern_ty(&TyData::Fn {
                params,
                result,
                suspends,
                vararg,
                row: union,
            });
            self.infer.rebind(pool, var, widened);
        }
    }

    /// The callee's row as seen from this call, its row parameters
    /// replaced by the call's row variables; a row variable that no
    /// argument constrained is the empty row (`req.row.least.examples`).
    pub(crate) fn call_row(
        &mut self,
        def: hd_base::DefId,
        sig: &FnSig,
        vars: &[Ty],
        skip: usize,
    ) -> RowId {
        let pool = self.pool();
        let empty = pool.intern_ty(&TyData::Row(RowId::EMPTY));
        for (i, g) in sig.generics.iter().enumerate() {
            if g.row
                && let Some(v) = vars.get(i + skip)
                && matches!(pool.get(self.infer.shallow(pool, *v)), TyData::Infer(_))
            {
                let _ = self.infer.unify(pool, *v, empty);
            }
        }
        let row = pool.subst_row(sig.row, &|p| {
            (p.owner == def)
                .then(|| vars.get(p.index as usize + skip).copied())
                .flatten()
        });
        self.infer.resolve_row(pool, row)
    }

    /// `req.row.least.ambiguous`: a parameter's row pattern may list at most
    /// one row parameter that no other parameter's pattern fixes.
    pub(crate) fn check_row_patterns(&mut self, sig: &FnSig, node: NodeRef<'_>) {
        let pool = self.pool();
        let patterns: Vec<Vec<RowParamRef>> = sig
            .params
            .iter()
            .map(|(_, t)| match pool.get(*t) {
                TyData::Fn { row, .. } => pool.row_data(row).params,
                _ => Vec::new(),
            })
            .collect();
        let mut fixed: Vec<RowParamRef> = Vec::new();
        loop {
            let before = fixed.len();
            for p in &patterns {
                let open: Vec<RowParamRef> =
                    p.iter().copied().filter(|r| !fixed.contains(r)).collect();
                if let [one] = open.as_slice() {
                    fixed.push(*one);
                }
            }
            if fixed.len() == before {
                break;
            }
        }
        let params: Vec<NodeRef<'_>> = Src::child(node, SyntaxKind::ParameterList)
            .map(|pl| {
                pl.children()
                    .filter(|c| c.kind() == SyntaxKind::Parameter)
                    .collect()
            })
            .unwrap_or_default();
        for (i, p) in patterns.iter().enumerate() {
            if p.iter().filter(|r| !fixed.contains(r)).count() >= 2 {
                let at = params.get(i).copied().unwrap_or(node);
                self.err(
                    Code::AmbiguousRowPattern,
                    at,
                    "this row pattern lists two row parameters that no other parameter fixes",
                );
            }
        }
    }
}
