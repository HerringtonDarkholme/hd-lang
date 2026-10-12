//! Member promotion through embedded parts (spec 03 "Depths And Promoted
//! Members", "Promoted Member Access"). A promoted member is its explicit
//! path through the embedded fields, so lookup only finds the path; the
//! callers then walk it like written field reads. Conflicts are
//! declaration errors (`hd_resolve`), so the first hit at the smallest
//! depth is the one member.

use hd_resolve::ItemData;
use hd_syntax::NodeRef;
use hd_tir::ir::{Ref, Tag, TirSink};
use hd_types::{Ty, TyData};

use crate::body::Ck;

/// The deepest chain of embedded parts (`data.embed.depth`).
const MAX_DEPTH: usize = 3;

impl Ck<'_, '_> {
    /// The embedded fields of a data type: name and substituted type.
    fn embedded_parts(&self, t: Ty) -> Vec<(String, Ty)> {
        let pool = self.pool();
        let t = self.strip_mut(t);
        let TyData::Adt { def, .. } = pool.get(t) else {
            return Vec::new();
        };
        let Some(ItemData::Data(fields)) = self.cx.lookup.item(def).map(|i| &i.data) else {
            return Vec::new();
        };
        fields
            .iter()
            .filter(|f| f.embedded)
            .filter_map(|f| {
                let name = self.cx.names.text(f.name).to_owned();
                let (_, ft) = self.field_of(t, &name)?;
                Some((name, ft))
            })
            .collect()
    }

    /// The embedded-field path to the shallowest part that `hit` accepts.
    fn promoted_path(
        &mut self,
        t: Ty,
        mut hit: impl FnMut(&mut Self, Ty) -> bool,
    ) -> Option<Vec<String>> {
        let mut level = vec![(t, Vec::<String>::new())];
        for _ in 0..MAX_DEPTH {
            let mut next = Vec::new();
            for (ty, path) in &level {
                for (name, part) in self.embedded_parts(*ty) {
                    let mut p = path.clone();
                    p.push(name);
                    next.push((part, p));
                }
            }
            for (part, path) in &next {
                if hit(self, *part) {
                    return Some(path.clone());
                }
            }
            if next.is_empty() {
                break;
            }
            level = next;
        }
        None
    }

    /// The path to the part whose `pub` field is `name`. An embedded field
    /// of a part counts as a member of that part.
    pub(crate) fn promoted_field_path(&mut self, t: Ty, name: &str) -> Option<Vec<String>> {
        let sym = self.cx.names.syms.intern(name);
        self.promoted_path(t, |ck, part| {
            let pool = ck.pool();
            let TyData::Adt { def, .. } = pool.get(ck.strip_mut(part)) else {
                return false;
            };
            matches!(
                ck.cx.lookup.item(def).map(|i| &i.data),
                Some(ItemData::Data(fields))
                    if fields.iter().any(|f| f.name == sym && (f.public || f.embedded))
            )
        })
    }

    /// The path to the part with a `pub` inherent method `name`.
    pub(crate) fn promoted_method_path(&mut self, t: Ty, name: &str) -> Option<Vec<String>> {
        self.promoted_path(t, |ck, part| {
            let Some((method, _, _)) = ck.find_inherent(part, name) else {
                return false;
            };
            ck.cx.lookup.item(method).is_some_and(|i| i.public)
        })
    }

    /// Reads the embedded fields of `path` in turn, each following its
    /// container's access (`names.promoted.path`).
    pub(crate) fn walk_path(
        &mut self,
        mut r: Ref,
        mut t: Ty,
        path: &[String],
        n: NodeRef<'_>,
    ) -> (Ref, Ty) {
        for name in path {
            let Some((idx, ft)) = self.field_of(t, name) else {
                break;
            };
            let ft = self.field_access(t, name, ft);
            r = self.b.emit(Tag::Field, r.0, idx, ft, n.index());
            t = ft;
        }
        (r, t)
    }

    /// The base to read or assign the field `name` of: `r` itself when
    /// the type has the field, else the part that promotes it.
    pub(crate) fn promote_base(&mut self, r: Ref, t: Ty, name: &str, n: NodeRef<'_>) -> (Ref, Ty) {
        if self.field_of(t, name).is_some() {
            return (r, t);
        }
        match self.promoted_field_path(t, name) {
            Some(path) => self.walk_path(r, t, &path, n),
            None => (r, t),
        }
    }
}
