//! A `dbg` call written out at its call site (spec/lang/10-modules.md,
//! "Debug Printing", "Debug Lines"). The declaration stays the plain
//! `dbg[Args < Tuple](values...: Args) -> void`; what no ordinary hd
//! parameter can supply, the call's location and each argument's source
//! text, the checker supplies by calling `std.format`'s support functions
//! with them. Collection supplies `dbg_printer` at each argument's concrete
//! type, selecting an ordinary hd printer through `Debug` or Structure.
//!
//! - A direct call with only positional arguments writes one line for each
//!   argument, as `dbg_one(head, value, dbg_printer[T]())`, with the location and
//!   the argument's text in `head` (`module.dbg.line`).
//! - Any other direct call knows only the location: each element of the
//!   collected tuple prints after it (`module.dbg.line.bare`).
//! - A `dbg` used as a function value is the ordinary function, whose
//!   supplied body prints each element alone (`module.dbg.body.value`).

use hd_base::{DefId, StageResult};
use hd_syntax::NodeRef;
use hd_tir::ir::{Callee, NONE, Providers, Ref, Tag, TirSink};
use hd_types::{Ty, TyData};

use crate::body::Ck;
use crate::call::Args;

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

    /// `FILE:LINE:COLUMN` where the node begins (`module.dbg.location`):
    /// the file as diagnostics name it, and the 1-based line and column
    /// that counts characters, as they do.
    fn dbg_location(&self, n: NodeRef<'_>) -> String {
        let at = self.cx.src.span(n).lo as usize;
        let text = self.cx.src.text.as_bytes();
        let before = &text[..at.min(text.len())];
        let line = before.split(|&b| b == b'\n').count();
        let start = before
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1);
        let column = String::from_utf8_lossy(&before[start..]).chars().count() + 1;
        format!("{}:{line}:{column}", self.cx.file_name)
    }

    /// An argument's source text, its line breaks and the indentation after
    /// them collapsed to one space.
    fn dbg_source(&self, e: NodeRef<'_>) -> String {
        let span = self.cx.src.span(e);
        let text = self
            .cx
            .src
            .text
            .get(span.lo as usize..span.hi as usize)
            .unwrap_or("");
        text.split('\n')
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A call of `dbg_printer` at `ty`: the printer of a value, as a
    /// function value.
    fn dbg_printer_ref(&mut self, ty: Ty, n: NodeRef<'_>) -> Ref {
        let pool = self.pool();
        let writer = pool.intern_ty(&TyData::Adt {
            def: self.format_fn("DebugWriter"),
            args: hd_types::TyList::EMPTY,
        });
        let writer = pool.intern_ty(&TyData::Ref {
            inner: writer,
            mutable: true,
            counted: true,
        });
        let ft = pool.intern_ty(&TyData::Fn {
            params: pool.list(&[ty, writer]),
            result: Ty::VOID,
            row: hd_types::RowId::EMPTY,
            suspends: false,
            inputs: hd_types::Inputs::Fixed,
            deps: hd_types::FnDeps::NONE,
        });
        self.dbg_support("dbg_printer", &[ty], &[], ft, n)
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

    /// The text of `r`, rendered by ordinary std code,
    /// of type `t`, as `dbg` prints it after `width` columns of its line.
    pub(crate) fn dbg_render_value(&mut self, r: Ref, t: Ty, width: usize, n: NodeRef<'_>) -> Ref {
        if let Some(text) = self.function_text(r, t) {
            return self.b.const_str(&text);
        }
        let usize = Ty::prim(hd_types::Prim::Usize);
        let width = self
            .b
            .const_value(usize, u64::try_from(width).unwrap_or(u64::MAX));
        let show = self.dbg_printer_ref(t, n);
        self.dbg_support("dbg_layout", &[t], &[width, r, show], Ty::STRING, n)
    }

    /// The value line of a REPL input that is one expression
    /// (`cli.repl.value`): `r` as `dbg` prints it, then ` : ` and its
    /// type as diagnostics show types, which counts against the line's
    /// 80 columns. It is written after the input's `dbg` lines, to the
    /// same debug output (`cli.dbg.repl`). A `void` or `never` expression
    /// has no value to show.
    pub(crate) fn repl_value(&mut self, r: Ref, t: Ty, n: NodeRef<'_>) {
        let t = self.zonk(t);
        let t = self.strip_mut(t);
        if t == Ty::VOID || t == Ty::NEVER {
            return;
        }
        let ty = self.show(t);
        let text = self.dbg_render_value(r, t, ty.chars().count() + 3, n);
        let ty = self.b.const_str(&ty);
        self.dbg_support("repl_value", &[], &[text, ty], Ty::VOID, n);
    }

    /// A direct `dbg` call with only positional arguments.
    pub(crate) fn dbg_call(&mut self, args: &Args<'_>, n: NodeRef<'_>) -> StageResult<(Ref, Ty)> {
        if args.positional.is_empty() {
            // `dbg()` writes nothing (`module.dbg.line`): no support
            // call, just an unused constant as the void value.
            let r = self.b.const_str("");
            return Ok((r, Ty::VOID));
        }
        let location = self.dbg_location(n);
        let mut printed = Vec::new();
        let mut types = Vec::new();
        for e in &args.positional {
            let (r, t) = self.arg_value(*e, None)?;
            let t = self.strip_mut(t);
            let head = format!("{location}: {}", self.dbg_source(*e));
            let head = self.b.const_str(&head);
            let line = if let Some(text) = self.function_text(r, t) {
                let text = self.b.const_str(&text);
                self.dbg_support("dbg_opaque", &[t], &[head, r, text], t, n)
            } else {
                let show = self.dbg_printer_ref(t, n);
                self.dbg_support("dbg_one", &[t], &[head, r, show], t, n)
            };
            printed.push(line);
            types.push(t);
        }
        // What the call ends in: the printed values, which it then drops.
        let (done, done_ty) = if let [one] = printed.as_slice() {
            (*one, types[0])
        } else {
            let pool = self.pool();
            let tuple = pool.intern_ty(&TyData::Tuple {
                elems: pool.list(&types),
                rest: None,
            });
            let rec = self.b.refs_record(&printed);
            (
                self.b.emit(Tag::NewTuple, NONE, rec, tuple, n.index()),
                tuple,
            )
        };
        let r = self.dbg_support("dbg_done", &[done_ty], &[done], Ty::VOID, n);
        Ok((r, Ty::VOID))
    }

    /// A direct `dbg` call that knows only its location: each element of
    /// the collected tuple prints after it. `None` when the tuple's shape is
    /// not known here, and the call stays the ordinary one.
    pub(crate) fn dbg_bare(&mut self, tuple: Ref, args_ty: Ty, n: NodeRef<'_>) -> Option<Ref> {
        let pool = self.pool();
        let resolved = self.infer.resolve(pool, args_ty);
        let TyData::Tuple { elems, rest: None } = pool.get(resolved) else {
            return None;
        };
        let elems: Vec<Ty> = pool.list_items(elems).to_vec();
        let prefix = format!("{}: ", self.dbg_location(n));
        let mut last = None;
        for (k, e) in elems.iter().enumerate() {
            let e = self.strip_mut(*e);
            let v = self
                .b
                .field(tuple, u32::try_from(k).unwrap_or(0), e, n.index());
            let head = self.b.const_str(&prefix);
            let show = self.dbg_printer_ref(e, n);
            last = Some(self.dbg_support("dbg_bare_one", &[e], &[head, v, show], Ty::VOID, n));
        }
        if last.is_none() {
            // An empty tuple prints nothing (`module.dbg.line`): the
            // already-evaluated tuple is the call's value.
            last = Some(tuple);
        }
        last
    }
}

/// Whether the call is one of the forms that print one line for each
/// argument, with its source text (`module.dbg.body.site`).
pub(crate) fn is_direct(args: &Args<'_>, explicit: &[Ty]) -> bool {
    explicit.is_empty()
        && args.spread.is_none()
        && args.after_spread.is_empty()
        && args.named.is_empty()
        && args.trailing.is_none()
}
