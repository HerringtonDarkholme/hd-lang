//! What the checker keeps of `dbg` (spec/lang/10-modules.md, "Debug
//! Printing"). `dbg` is ordinary hd in `std.format`, a `@track_caller`
//! function whose call-site record holds each argument's source text; the
//! checker only notes each reference for the release check
//! (`module.dbg.release`) and writes the REPL's value line, which lays a
//! value out as a `dbg` line does (`cli.repl.value`).

use hd_base::DefId;
use hd_syntax::NodeRef;
use hd_tir::ir::{Callee, Providers, Ref, Tag, TirSink};
use hd_types::{Ty, TyData};

use crate::body::Ck;

/// A reference to `dbg` in source: the call, or the path used as a value
/// (`module.dbg.release`). It holds the same in every build profile, so
/// the profile decides what it means, not the checker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DbgRef {
    pub span: hd_base::Span,
    /// A call, whose fix-it deletes the statement.
    pub call: bool,
}

const FORMAT: &str = "std.format";

impl Ck<'_, '_> {
    /// Notes a reference to `dbg` at `n`; a body checked twice notes it once.
    pub(crate) fn note_dbg(&self, n: NodeRef<'_>, call: bool) {
        let r = DbgRef {
            span: self.cx.src.span(n),
            call,
        };
        let mut refs = self.cx.dbg_refs.borrow_mut();
        if !refs.contains(&r) {
            refs.push(r);
        }
    }

    fn format_fn(&self, name: &str) -> DefId {
        self.cx.names.item(FORMAT, name)
    }

    /// A call of a `std.format` support function.
    fn dbg_support(
        &mut self,
        name: &str,
        targs: &[Ty],
        args: &[Ref],
        ret: Ty,
        n: NodeRef<'_>,
    ) -> Ref {
        let c = Callee::Item {
            def: self.format_fn(name),
            targs: self.pool().list(targs),
        };
        self.b.call(&c, args, Providers::None, ret, n.index())
    }

    /// The function an argument names, if it is a reference to one
    /// (`module.dbg.value.function`): its name.
    fn named_function(&mut self, r: Ref) -> Option<String> {
        let body = self.b.body_mut();
        let i = r.as_inst()?.0 as usize;
        if body.tags.get(i) != Some(&Tag::ItemRef) {
            return None;
        }
        let def = *body.record(body.data[i][0]).first()?;
        let item = self.cx.lookup.item(DefId::from_raw(def))?;
        Some(self.cx.names.text(item.name).to_owned())
    }

    /// How `dbg` prints a reference to a named function
    /// (`module.dbg.value.function`): `<fn name(..) -> ..>`.
    fn function_text(&mut self, r: Ref, t: Ty) -> Option<String> {
        let name = self.named_function(r)?;
        if !matches!(self.pool().get(t), TyData::Fn { .. }) {
            return None;
        }
        let shown = self.show(t);
        Some(format!(
            "<fn {name}{}>",
            shown.strip_prefix("fn").unwrap_or(&shown)
        ))
    }

    /// The value line of a REPL input that is one expression
    /// (`cli.repl.value`): `r` as `dbg` prints it, then ` : ` and its
    /// type as diagnostics show types, which counts against the line's
    /// 80 columns. It is written after the input's `dbg` lines, to the
    /// same debug output (`cli.dbg.repl`). A `void` or `never` expression
    /// has no value to show.
    pub(crate) fn repl_value(&mut self, r: Ref, t: Ty, n: NodeRef<'_>) {
        let t = self.zonk(t);
        let t = self.resolved(t);
        if t == Ty::VOID || t == Ty::NEVER {
            return;
        }
        let ty = self.show(t);
        let text = if let Some(text) = self.function_text(r, t) {
            self.b.const_str(&text)
        } else {
            let width = ty.chars().count() + 3;
            let width = self.b.const_value(
                Ty::prim(hd_types::Prim::Usize),
                u64::try_from(width).unwrap_or(u64::MAX),
            );
            self.dbg_support("repl_text", &[t], &[r, width], Ty::STRING, n)
        };
        let ty = self.b.const_str(&ty);
        self.dbg_support("repl_value", &[], &[text, ty], Ty::VOID, n);
    }
}
