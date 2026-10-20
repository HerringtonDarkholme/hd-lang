//! Copy-update helpers (spec 05 "Copy-Update Expressions", spec 08
//! "Copy-Update Literals"; `crates/hd_tir/DESIGN.md` `CopyData`). A copy
//! reads every field the literal does not replace; the copied fields are
//! read when the spread is evaluated if an explicit field expression may
//! run code (`expr.update.copy-time`).

use hd_diag::Code;
use hd_resolve::ItemData;
use hd_syntax::NodeRef;
use hd_tir::ir::{NONE, Ref, Tag, TirSink};
use hd_types::{Ty, TyData};

use crate::body::Ck;

impl Ck<'_, '_> {
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

    /// Reads the ordinary fields of `src` that `skip` does not replace,
    /// now, into `vals` (`expr.update.copy-time`), so an explicit field
    /// expression that changes the source does not show in the copy.
    pub(crate) fn copy_fields_now(
        &mut self,
        src: (Ref, Ty),
        skip: &[bool],
        vals: &mut [Ref],
        at: NodeRef<'_>,
    ) {
        let pool = self.pool();
        let (sr, st) = src;
        let TyData::Adt { def, .. } = pool.get(self.strip_mut(self.infer.resolve(pool, st))) else {
            return;
        };
        let Some(ItemData::Data(fields)) = self.cx.lookup.item(def).map(|i| &i.data) else {
            return;
        };
        let names: Vec<(usize, String)> = fields
            .iter()
            .enumerate()
            .filter(|(i, f)| !f.embedded && !skip.get(*i).copied().unwrap_or(true))
            .map(|(i, f)| (i, self.cx.names.text(f.name).to_owned()))
            .collect();
        for (i, name) in names {
            let Some((idx, ft)) = self.field_of(st, &name) else {
                continue;
            };
            let ft = self.field_access(st, &name, ft);
            vals[i] = self.b.emit(Tag::Field, sr.0, idx, ft, at.index());
        }
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
}
