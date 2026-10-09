//! A `dbg` call written out at its call site (spec/lang/10-modules.md,
//! "Debug Printing", "Debug Lines"). The declaration stays the plain
//! `dbg[Args < Tuple](values...: Args) -> void`; what no ordinary hd
//! parameter can supply, the call's location and each argument's source
//! text, the checker supplies by calling `std.format`'s support functions
//! with them. Collection supplies `dbg_show` at each argument's concrete
//! type, which prints through `Debug` or structurally.
//!
//! - A direct call with only positional arguments writes one line for each
//!   argument, as `dbg_one(head, value, dbg_show)`, with the location and
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

const FORMAT: &str = "std.format";

impl Ck<'_, '_> {
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

    /// A reference to `dbg_show` at `ty`: the printer of a value, as a
    /// function value.
    fn dbg_show_ref(&mut self, ty: Ty, n: NodeRef<'_>) -> Ref {
        let pool = self.pool();
        let writer = pool.intern_ty(&TyData::Adt {
            def: self.format_fn("DebugWriter"),
            args: hd_types::TyList::EMPTY,
        });
        let writer = pool.intern_ty(&TyData::Mut(writer));
        let ft = pool.intern_ty(&TyData::Fn {
            params: pool.list(&[ty, writer]),
            result: Ty::VOID,
            row: hd_types::RowId::EMPTY,
            suspends: false,
        });
        let show = self.format_fn("dbg_show");
        let a = self.b.refs_record(&[Ref(show.raw())]);
        let l = pool.list(&[ty]);
        let bw = self.b.refs_record(&[Ref(l.0)]);
        self.b.emit(Tag::ItemRef, a, bw, ft, n.index())
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

    /// A direct `dbg` call with only positional arguments.
    pub(crate) fn dbg_call(&mut self, args: &Args<'_>, n: NodeRef<'_>) -> StageResult<(Ref, Ty)> {
        let location = self.dbg_location(n);
        if args.positional.is_empty() {
            let head = self.b.const_str(&location);
            let r = self.dbg_support("dbg_here", &[], &[head], Ty::VOID, n);
            return Ok((r, Ty::VOID));
        }
        let mut printed = Vec::new();
        let mut types = Vec::new();
        for e in &args.positional {
            let (r, t) = self.arg_value(*e, None)?;
            let t = self.strip_mut(t);
            let head = format!("{location}: {}", self.dbg_source(*e));
            let head = self.b.const_str(&head);
            let line = match self.named_function(r) {
                Some(name) if matches!(self.pool().get(t), TyData::Fn { .. }) => {
                    let shown = self.show(t);
                    let text = format!("<fn {name}{}>", shown.strip_prefix("fn").unwrap_or(&shown));
                    let text = self.b.const_str(&text);
                    self.dbg_support("dbg_opaque", &[t], &[head, r, text], t, n)
                }
                _ => {
                    let show = self.dbg_show_ref(t, n);
                    self.dbg_support("dbg_one", &[t], &[head, r, show], t, n)
                }
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
            let v = self.b.emit(
                Tag::TupleGet,
                tuple.0,
                u32::try_from(k).unwrap_or(0),
                e,
                n.index(),
            );
            let head = self.b.const_str(&prefix);
            let show = self.dbg_show_ref(e, n);
            last = Some(self.dbg_support("dbg_bare_one", &[e], &[head, v, show], Ty::VOID, n));
        }
        if last.is_none() {
            let head = self.b.const_str(prefix.trim_end_matches(": "));
            last = Some(self.dbg_support("dbg_here", &[], &[head], Ty::VOID, n));
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
