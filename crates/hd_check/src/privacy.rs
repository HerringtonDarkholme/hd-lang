//! Member visibility (spec 03 "Visibility", "Field Lookup", "Methods Not
//! Found"; spec 08 "Field Visibility"). Lookup considers every own member
//! whatever its visibility (`names.members.uniform`); these checks decide
//! only whether the body's module may use the member that lookup selected.

use hd_base::DefId;
use hd_diag::Code;
use hd_resolve::{Field, ItemData};
use hd_syntax::NodeRef;
use hd_types::{Ty, TyData};

use crate::body::Ck;

impl<'a> Ck<'a, '_> {
    /// The data declaration of `t` and its fields.
    fn data_decl(&self, t: Ty) -> Option<(DefId, &'a [Field])> {
        let pool = self.pool();
        let TyData::Adt { def, .. } = pool.get(self.strip_mut(t)) else {
            return None;
        };
        match &self.cx.lookup.item(def)?.data {
            ItemData::Data(fields) => Some((def, fields)),
            _ => None,
        }
    }

    /// Whether `d` is declared in the body's module, which for a body of a
    /// REPL input is any module of its session ([`BodyCx::session`]).
    ///
    /// [`BodyCx::session`]: crate::body::BodyCx::session
    fn declared_here(&self, d: DefId) -> bool {
        let names = &self.cx.names;
        let session = self.cx.session;
        names.declared_in(d, self.module)
            || (session.contains(&self.module) && session.contains(&names.module_node(d)))
    }

    /// Whether the body's module may use a member of `owner`: the member
    /// is `pub` or `owner` is declared there (`names.visible.field-method`).
    fn member_visible(&self, owner: DefId, public: bool) -> bool {
        public || self.declared_here(owner)
    }

    /// Whether the body's module may call the inherent method `method`.
    pub(crate) fn method_visible(&self, method: DefId) -> bool {
        self.declared_here(method) || self.cx.lookup.item(method).is_some_and(|i| i.public)
    }

    /// A use of the own field `name` of `t`: an error when the field is
    /// not visible here (`names.field-lookup.private`,
    /// `data.vis.private-fields`). An embedded field is always public
    /// (`data.vis.embedded-public`).
    pub(crate) fn check_field_visible(&mut self, t: Ty, name: &str, n: NodeRef<'_>) {
        let Some((def, fields)) = self.data_decl(t) else {
            return;
        };
        let sym = self.cx.names.syms.intern(name);
        let Some(f) = fields.iter().find(|f| f.name == sym) else {
            return;
        };
        if self.member_visible(def, f.public || f.embedded) {
            return;
        }
        let msg = format!(
            "the field `{name}` of {} is private to its module",
            self.show(t)
        );
        self.err(Code::PrivateMember, n, &msg);
    }

    /// A data literal of the data type `def`, a copy-update included: in
    /// another module, every field of the type must be public
    /// (`data.vis.literal`, `data.vis.private-fields`). Reports the first
    /// private field.
    pub(crate) fn check_literal_visible(&mut self, def: DefId, n: NodeRef<'_>) {
        let Some(ItemData::Data(fields)) = self.cx.lookup.item(def).map(|i| &i.data) else {
            return;
        };
        let Some(f) = fields
            .iter()
            .find(|f| !self.member_visible(def, f.public || f.embedded))
        else {
            return;
        };
        let msg = format!(
            "`{}` has the private field `{}`, so only its module can construct or copy it",
            self.cx.names.display_name(def),
            self.cx.names.text(f.name)
        );
        self.err(Code::PrivateMember, n, &msg);
    }
}
