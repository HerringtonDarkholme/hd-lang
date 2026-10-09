//! Copy-update helpers (spec 05 "Copy-Update Expressions", spec 08
//! "Copy-Update Literals" and "Parts And Copies"; checking-and-tir.md
//! `CopyData`). A copy reads every field the literal does not replace
//! through the source's access view. An embedded part is copied as a
//! copy-update of its own, made when the spread is evaluated
//! (`data.part.copy-time`), so the `CopyData` replacements carry it.

use hd_base::DefId;
use hd_diag::Code;
use hd_resolve::ItemData;
use hd_syntax::NodeRef;
use hd_tir::ir::{NONE, Ref, Tag, TirSink};
use hd_types::{ParamRef, Ty, TyData, TyList};

use crate::body::Ck;

impl Ck<'_, '_> {
    /// `T { ...src }` for an embedded-field copy marker (`data.part.construct`):
    /// the copy of `src`, whose own embedded parts are copied in turn
    /// (`data.part.copy-update`). The copy is `mut` when nothing mutable is
    /// read through a readonly view to make it.
    pub(crate) fn copy_value(&mut self, src: (Ref, Ty), at: NodeRef<'_>) -> (Ref, Ty) {
        let pool = self.pool();
        let (sr, st) = src;
        let st = self.infer.resolve(pool, st);
        let Some(n) = self.data_field_count(st) else {
            return (sr, st);
        };
        let skip = vec![false; n];
        let mut vals = vec![Ref(NONE); n];
        let fresh_parts = self.copy_parts((sr, st), &skip, &mut vals, at);
        let fresh = fresh_parts && !self.reads_readonly_edge(st, &skip);
        let bare = self.strip_mut(st);
        let ty = if fresh {
            pool.intern_ty(&TyData::Mut(bare))
        } else {
            bare
        };
        let rec = self.copy_record(&vals);
        (self.b.emit(Tag::CopyData, sr.0, rec, ty, at.index()), ty)
    }

    /// `expr.update.exact-type`: the spread source has exactly the data
    /// type being constructed. Reports `type-mismatch` otherwise.
    pub(crate) fn copy_source_fits(&mut self, src: Ty, target: Ty, at: NodeRef<'_>) -> bool {
        let pool = self.pool();
        let bare = self.strip_mut(src);
        let snap = self.infer.snapshot();
        if self.infer.unify(pool, bare, target).is_ok() {
            return true;
        }
        self.infer.rollback(snap);
        let msg = format!(
            "in copy-update source: expected {}, found {}",
            self.show(target),
            self.show(src)
        );
        self.err(Code::TypeMismatch, at, &msg);
        false
    }

    /// Copies the embedded parts of `src` that `skip` does not replace,
    /// now, into `vals`. Returns whether every copy is `mut`.
    pub(crate) fn copy_parts(
        &mut self,
        src: (Ref, Ty),
        skip: &[bool],
        vals: &mut [Ref],
        at: NodeRef<'_>,
    ) -> bool {
        let pool = self.pool();
        let (sr, st) = src;
        let mut fresh = true;
        for (i, name) in self.embedded_fields(st) {
            if skip.get(i).copied().unwrap_or(true) {
                continue;
            }
            let Some((idx, ft)) = self.field_of(st, &name) else {
                continue;
            };
            let ft = self.field_access(st, &name, ft);
            let read = self.b.emit(Tag::Field, sr.0, idx, ft, at.index());
            let (r, t) = self.copy_value((read, ft), at);
            fresh &= matches!(pool.get(self.infer.resolve(pool, t)), TyData::Mut(_));
            vals[i] = r;
        }
        fresh
    }

    /// A `CopyData` replacement record: a `(field, value)` pair for each
    /// value set in `vals`.
    pub(crate) fn copy_record(&mut self, vals: &[Ref]) -> u32 {
        let mut words = Vec::new();
        for (i, v) in vals.iter().enumerate() {
            if v.0 != NONE {
                words.push(Ref(u32::try_from(i).unwrap_or(0)));
                words.push(*v);
            }
        }
        self.b.refs_record(&words)
    }

    /// Whether a copy of a `src` value that keeps the fields `skip` leaves
    /// out of the copy reads a direct `mut U` field through a readonly
    /// view, so the copy is readonly (`expr.update.readonly-source`,
    /// `data.update.mutable-result`).
    pub(crate) fn reads_readonly_edge(&self, src: Ty, skip: &[bool]) -> bool {
        let pool = self.pool();
        let src = self.infer.resolve(pool, src);
        if self.has_mut_access(src) {
            return false;
        }
        let TyData::Adt { def, args } = pool.get(src) else {
            return false;
        };
        let Some(item) = self.cx.lookup.item(def) else {
            return false;
        };
        let ItemData::Data(fields) = &item.data else {
            return false;
        };
        fields.iter().enumerate().any(|(i, f)| {
            !skip.get(i).copied().unwrap_or(false)
                && !f.embedded
                && matches!(pool.get(f.ty), TyData::Mut(_))
                && self.is_composite(self.subst_args(def, args, f.ty))
        })
    }

    /// A field type of `def` with the data type's arguments applied.
    fn subst_args(&self, def: DefId, args: TyList, ty: Ty) -> Ty {
        let pool = self.pool();
        let actual = pool.list_items(args);
        pool.subst(ty, &|p: ParamRef| {
            (p.owner == def)
                .then(|| actual.get(p.index as usize).copied())
                .flatten()
        })
    }

    /// The number of fields of a data type.
    pub(crate) fn data_field_count(&self, t: Ty) -> Option<usize> {
        let pool = self.pool();
        let TyData::Adt { def, .. } = pool.get(self.strip_mut(t)) else {
            return None;
        };
        match &self.cx.lookup.item(def)?.data {
            ItemData::Data(fields) => Some(fields.len()),
            _ => None,
        }
    }

    /// The embedded fields of a data type: index and name.
    fn embedded_fields(&self, t: Ty) -> Vec<(usize, String)> {
        let pool = self.pool();
        let TyData::Adt { def, .. } = pool.get(self.strip_mut(t)) else {
            return Vec::new();
        };
        let Some(ItemData::Data(fields)) = self.cx.lookup.item(def).map(|i| &i.data) else {
            return Vec::new();
        };
        fields
            .iter()
            .enumerate()
            .filter(|(_, f)| f.embedded)
            .map(|(i, f)| (i, self.cx.names.text(f.name).to_owned()))
            .collect()
    }
}
