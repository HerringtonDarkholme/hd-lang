//! Call-site locations (spec/lang/07-functions.md "Call-Site Locations").
//!
//! A `@track_caller` function's body has one hidden parameter after its
//! written ones, `$site`, a `string`: the call site's record. Each direct
//! call passes it: a constant the checker writes from the call's syntax, or,
//! inside another `@track_caller` function, that function's own `$site`
//! (`fn.caller.chain`). `Location::caller()` and `Location::caller_args()`
//! read `$site` through ordinary `std.debug` decoders.
//!
//! The record's fields are separated by U+001F: the root-package bit, the
//! line, the column and the file, then seven fields per argument (start line
//! and column, end line and column, the name of a named argument or empty,
//! text, and the function a positional argument names or empty, for `dbg`)
//! when the callee reads the argument texts (`fn.caller.record.args`). A string constant is one
//! pointer to static data, so a call pays one argument and no allocation;
//! only an accessor decodes the record. The driver clears the root-package
//! bit in fetched and std code (`crate::caller` users in `hd_driver`).

use std::fmt::Write as _;

use hd_base::DefId;
use hd_diag::Code;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::ir::{Callee, NONE, Providers, Ref, TirSink};
use hd_types::{Ty, TyList};

use crate::body::Ck;

/// The hidden parameter's name, which no source identifier can spell.
pub(crate) const SITE: &str = "$site";

/// The record's field separator.
pub const SEPARATOR: char = '\u{1f}';

impl Ck<'_, '_> {
    /// The attributes of `def` when it is a `@track_caller` function:
    /// whether its records hold argument texts.
    pub(crate) fn tracked_caller(&self, def: DefId) -> Option<bool> {
        let a = self.cx.lookup.item(def)?.fn_attrs()?;
        a.track_caller.then_some(a.reads_args)
    }

    /// The 1-based line and column, in characters, of byte `at`.
    fn line_column(&self, at: u32) -> (usize, usize) {
        let text = self.cx.src.text.as_bytes();
        let before = &text[..(at as usize).min(text.len())];
        let line = before.split(|&b| b == b'\n').count();
        let start = before
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1);
        let column = String::from_utf8_lossy(&before[start..]).chars().count() + 1;
        (line, column)
    }

    /// The arguments of call `n` in source order, each with its name when
    /// it is a named one, when it has no spread and no trailing block
    /// (`fn.caller.accessor.args.complete`).
    fn site_args<'t>(&self, n: NodeRef<'t>) -> Vec<(Option<&str>, NodeRef<'t>)> {
        if n.kind() != SyntaxKind::CallExpr || self.trailing_heads.contains(&n.index()) {
            return Vec::new();
        }
        let Some(al) = n.children().find(|c| c.kind() == SyntaxKind::ArgumentList) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        // Only argument nodes count; punctuation tokens sit between them.
        let args = al
            .children()
            .filter(|c| matches!(c.kind(), SyntaxKind::Argument | SyntaxKind::NamedArgument));
        for c in args {
            match c.children().next() {
                Some(e) if c.kind() == SyntaxKind::NamedArgument => {
                    let name = self.cx.src.first_ident(c).map(|t| self.cx.src.text(t));
                    out.push((name, e));
                }
                Some(e) if e.kind() != SyntaxKind::SpreadExpr => out.push((None, e)),
                // A spread or an empty argument: texts unknown.
                Some(_) | None => return Vec::new(),
            }
        }
        out
    }

    /// The record of call site `n` (`fn.caller.record`): its location,
    /// and its arguments when `args`.
    fn site_record(&self, n: NodeRef<'_>, args: bool) -> String {
        let span = self.cx.src.span(n);
        let (line, column) = self.line_column(span.lo);
        let s = SEPARATOR;
        let mut out = format!("1{s}{line}{s}{column}{s}{}", self.cx.file_name);
        if args {
            for (name, e) in self.site_args(n) {
                let span = self.cx.src.span(e);
                let (sl, sc) = self.line_column(span.lo);
                let (el, ec) = self.line_column(span.hi);
                let text = self
                    .cx
                    .src
                    .text
                    .get(span.lo as usize..span.hi as usize)
                    .unwrap_or("")
                    .replace(s, "");
                // The function a positional argument names
                // (`module.dbg.value.function`), keyed by the span of the
                // expression inside any parentheses.
                let function = match name {
                    Some(_) => "",
                    None => self.named_fn(e),
                };
                let name = name.unwrap_or("");
                let _ = write!(
                    out,
                    "{s}{sl}{s}{sc}{s}{el}{s}{ec}{s}{name}{s}{text}{s}{function}"
                );
            }
        }
        out
    }

    /// The name of the function expression `e` refers to, if it names one,
    /// else empty.
    fn named_fn(&self, mut e: NodeRef<'_>) -> &str {
        while e.kind() == SyntaxKind::ParenExpr
            && let Some(inner) = e.children().next()
        {
            e = inner;
        }
        let span = self.cx.src.span(e);
        self.named_fns
            .get(&(span.lo, span.hi))
            .and_then(|def| self.cx.lookup.item(*def))
            .map_or("", |item| self.cx.names.text(item.name))
    }

    /// The enclosing `@track_caller` function's `$site`, read here.
    fn own_site(&mut self, n: NodeRef<'_>) -> Option<Ref> {
        let sym = self.cx.names.syms.intern(SITE);
        let (l, depth) = self.find_local(sym)?;
        Some(self.read_local(l, depth, n).0)
    }

    /// The hidden argument of a call at `n` of a `@track_caller` function
    /// whose records hold argument texts when `reads_args`: the enclosing
    /// one's `$site` (`fn.caller.chain`), or this site's record. A function
    /// value binds the place that creates it, with no argument texts
    /// (`fn.caller.value`).
    pub(crate) fn site_arg(&mut self, reads_args: bool, n: NodeRef<'_>) -> Ref {
        if !self.site_value
            && let Some(r) = self.own_site(n)
        {
            return r;
        }
        let record = self.site_record(n, reads_args && !self.site_value);
        self.b.const_str(&record)
    }

    /// `Location::caller()` or `Location::caller_args()` at `n`: the
    /// decoder of `key` applied to the enclosing `$site`
    /// (`fn.caller.accessor.inside`).
    pub(crate) fn caller_access(&mut self, key: &str, ret: Ty, n: NodeRef<'_>) -> (Ref, Ty) {
        let Some(site) = self.own_site(n) else {
            let name = if key == "caller_args" {
                "Location::caller_args()"
            } else {
                "Location::caller()"
            };
            let msg = format!(
                "`{name}` is valid only inside a `@track_caller` function, which has a caller"
            );
            self.err(Code::CallerOutsideTrackCaller, n, &msg);
            return (Ref(NONE), Ty::POISON);
        };
        let known = &self.cx.names.known;
        let decoder = if key == "caller_args" {
            known.site_args
        } else {
            known.site_location
        };
        let c = Callee::Item {
            def: decoder,
            targs: TyList::EMPTY,
        };
        (
            self.b.call(&c, &[site], Providers::None, ret, n.index()),
            ret,
        )
    }
}
