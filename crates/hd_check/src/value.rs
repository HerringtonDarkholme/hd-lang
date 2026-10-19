//! Struct values (spec 08 "Struct Declarations", spec 04 "Value Types").
//! A struct value has no identity, so a field write changes only the
//! writable place it names: a `let mut` local or top-level binding, a
//! field of a data value with mutable access, or a struct field of one of
//! these (`types.value.writable-forms`).
//!
//! The Wasm GC backend holds a struct value as an object that is never
//! written after it is built (`crates/hd_wasm/DESIGN.md` §15.2, "Struct
//! values"). So `a.b.x = v` is lowered as `a = A { ...a, b: B { ...a.b,
//! x: v } }`: each struct on the path is rebuilt with `CopyData`, and the
//! place's root is stored. Since no struct object changes, two places may
//! hold one object, and a copy of a struct value copies one reference.

use hd_base::{DefId, LocalId, StageResult};
use hd_diag::Code;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{NONE, Ref, Tag, TirSink, local_flags};
use hd_types::{ParamRef, Ty, TyData};

use crate::body::{Ck, ExpectPosition};

/// Where a writable struct place starts.
#[derive(Clone, Copy)]
enum Root {
    /// A `let mut` local, with its scope depth.
    Local(LocalId, usize),
    /// A `let mut` top-level binding, with its type.
    Global(DefId, Ty),
    /// Field `idx` of the data value `base`, with the field's type.
    Field(Ref, u32, Ty),
    /// The value the reference `r`, a `&mut`, refers to, with `r`'s type
    /// (`ref.write.field`).
    Deref(Ref, Ty),
    /// Element `index` of the writable list `list`, with the element type
    /// (`types.coll.element-place`).
    Elem(Ref, Ref, Ty),
}

/// A writable struct place: its root, then the struct fields read from it
/// in order, each with the type it reads.
struct Place {
    root: Root,
    path: Vec<(u32, Ty)>,
}

impl Ck<'_, '_> {
    /// `base.name = rhs`, `base.name op= rhs` or `base.name ...= rhs`
    /// where `base`, already checked as `(br, bt)`, has a struct type
    /// (`types.value.writable-forms`, `types.value.field-write`).
    pub(crate) fn struct_field_assign(
        &mut self,
        (br, bt): (Ref, Ty),
        (base, lhs, rhs, s): (NodeRef<'_>, NodeRef<'_>, NodeRef<'_>, NodeRef<'_>),
        name: &str,
        compound: Option<TokenKind>,
        copy: bool,
    ) -> StageResult<()> {
        let Some((idx, ft)) = self.field_of(bt, name) else {
            let msg = format!("no field `{name}` on {}", self.show(bt));
            self.err(Code::UnknownField, lhs, &msg);
            return Ok(());
        };
        self.check_field_visible(bt, name, lhs);
        // A struct's field, embedded or not, is assigned with `=`
        // (`data.struct.embed.no-marker`).
        if copy {
            let msg = format!("`{name}` of a struct is assigned with `=`, not `...=`");
            self.err(Code::CopyIntoOrdinaryField, lhs, &msg);
            return Ok(());
        }
        let Some(place) = self.value_place(br, base, (lhs, s)) else {
            return Ok(());
        };
        let v = match compound {
            None => {
                let (r, t) = self.expr(rhs, Some(ft))?;
                self.coerce(r, t, ft, rhs, "assignment", ExpectPosition::General)
            }
            Some(k) => {
                let cur = self.b.emit(Tag::Field, br.0, idx, ft, lhs.index());
                self.compound(k, cur, ft, rhs, s)?
            }
        };
        self.store_field(&place, idx, v, (base, s));
        Ok(())
    }

    /// The writable place that `br`, a struct value read by `base`,
    /// reads; reports why it is not writable otherwise.
    fn value_place(
        &mut self,
        br: Ref,
        base: NodeRef<'_>,
        (at, stmt): (NodeRef<'_>, NodeRef<'_>),
    ) -> Option<Place> {
        let mut path = Vec::new();
        let mut cur = br;
        let mut node = base;
        while let Some(i) = cur.as_inst() {
            let (tag, data) = {
                let body = self.b.body_mut();
                (body.tags[i.0 as usize], body.data[i.0 as usize])
            };
            let inner_node = if node.kind() == SyntaxKind::FieldExpr {
                node.children().next().unwrap_or(node)
            } else {
                node
            };
            match tag {
                Tag::Field => {
                    let inner = Ref(data[0]);
                    let it = self.b.ty_of(inner);
                    if self.is_struct_ty(it) {
                        path.push((data[1], self.b.ty_of(cur)));
                        cur = inner;
                        node = inner_node;
                        continue;
                    }
                    // A field of a data value: the store needs mutable
                    // access to the data value (`types.path.mutation`).
                    if !self.has_mut_access(it) {
                        self.check_store_target(inner_node, (inner, it), at);
                        return None;
                    }
                    path.reverse();
                    let root = Root::Field(inner, data[1], self.b.ty_of(cur));
                    return Some(Place { root, path });
                }
                // A list element is a place of a writable list; a map value
                // is no place (`types.coll.element-place`,
                // `types.coll.map-value`).
                Tag::Intrinsic
                    if matches!(
                        hd_tir::ir::IntrinsicOp::from_u32(data[0]),
                        Some(
                            hd_tir::ir::IntrinsicOp::ListIndex
                                | hd_tir::ir::IntrinsicOp::ListIndexMut
                                | hd_tir::ir::IntrinsicOp::MapIndex
                                | hd_tir::ir::IntrinsicOp::MapIndexMut
                        )
                    ) =>
                {
                    let args = self.b.body_mut().record(data[1]).to_vec();
                    let (list, index) = (Ref(args[0]), Ref(args[1]));
                    if matches!(
                        hd_tir::ir::IntrinsicOp::from_u32(data[0]),
                        Some(
                            hd_tir::ir::IntrinsicOp::MapIndex
                                | hd_tir::ir::IntrinsicOp::MapIndexMut
                        )
                    ) {
                        self.map_value_write(at, stmt, node);
                        return None;
                    }
                    let lt = self.b.ty_of(list);
                    let list_node = node.children().next().unwrap_or(node);
                    if !self.coll_change(list, lt, "this field write", list_node) {
                        return None;
                    }
                    path.reverse();
                    return Some(Place {
                        root: Root::Elem(list, index, self.b.ty_of(cur)),
                        path,
                    });
                }
                // A read through a reference (`refs.rs`): the place is the
                // referenced value, writable through `&mut` only.
                Tag::CallValue if let Some((r, m)) = self.deref_source(Ref(data[0])) => {
                    let rt = self.b.ty_of(r);
                    if !m {
                        let msg = format!(
                            "this writes through a read-only reference {}; take the reference with `&mut` to write through it",
                            self.show(rt)
                        );
                        self.err(Code::ReadonlyRoot, at, &msg);
                        return None;
                    }
                    path.reverse();
                    return Some(Place {
                        root: Root::Deref(r, rt),
                        path,
                    });
                }
                Tag::LocalGet => {
                    let l = LocalId::from_raw(data[0]);
                    if !self.local_writable(l, at) {
                        return None;
                    }
                    let depth = self
                        .scopes
                        .iter()
                        .rposition(|m| m.values().any(|x| *x == l))
                        .unwrap_or(0);
                    path.reverse();
                    return Some(Place {
                        root: Root::Local(l, depth),
                        path,
                    });
                }
                Tag::GlobalGet if node.kind() == SyntaxKind::NameExpr => {
                    let name = self.sym_of(node);
                    let global = {
                        let init = self.cx.init.borrow();
                        init.globals
                            .get(&name)
                            .map(crate::init::Global::binding)
                            .or_else(|| init.earlier.get(&name).copied())
                    };
                    let Some(g) = global else { break };
                    if !g.mutable {
                        let text = self.cx.names.text(name).to_owned();
                        self.not_writable(&text, g.short, false, at);
                        return None;
                    }
                    path.reverse();
                    return Some(Place {
                        root: Root::Global(g.def, g.ty),
                        path,
                    });
                }
                _ => break,
            }
        }
        // `types.value.write.other-value`
        let msg = "this struct value is not a variable or a field of one, so its fields cannot change; copy it into a `let mut` first";
        self.err(Code::ReadonlyRoot, at, msg);
        None
    }

    /// Whether the local `l` is writable: bound with `let mut`. Reports
    /// why not otherwise.
    fn local_writable(&mut self, l: LocalId, at: NodeRef<'_>) -> bool {
        if self.mut_locals.contains(&l) {
            return true;
        }
        let name = self
            .cx
            .names
            .text(self.b.body_mut().local_name[l.idx()])
            .to_owned();
        let flags = self.b.body_mut().local_flags[l.idx()];
        let short = flags & local_flags::SHORT != 0 || self.using_locals.contains(&l);
        let param = flags & local_flags::PARAM != 0;
        self.not_writable(&name, short, param, at);
        // `fn.method.self-struct-write`: the fix takes `&mut self`.
        if param && name == "self" {
            let syn = self.b.body_mut().local_syn[l.idx()];
            self.writable_binding_fix("self", false, true, syn);
        }
        false
    }

    /// Reports a field write through the binding `name`, which is not
    /// `let mut` (`types.value.write.short`, `types.value.write.plain-param`,
    /// `types.value.write.let`).
    fn not_writable(&mut self, name: &str, short: bool, param: bool, at: NodeRef<'_>) {
        if param {
            let msg = if name == "self" {
                "`self` is the method's own read-only copy of the receiver; take `&mut self` to change the caller's value".to_owned()
            } else {
                format!(
                    "parameter `{name}` is the function's own copy, which never changes; write `mut {name}` to change the copy"
                )
            };
            self.err(Code::NonReassignableParameterBinding, at, &msg);
        } else if short {
            let msg = format!("`{name}` never changes; bind it with `let mut` to change a field");
            self.err(Code::NonReassignableBinding, at, &msg);
        } else {
            let msg = format!(
                "`{name}` is not `let mut`, so it may only be reassigned as a whole; bind it with `let mut` to change a field"
            );
            self.err(Code::ReadonlyRoot, at, &msg);
        }
    }

    /// Stores `v` into field `idx` of the struct at `place`: reads the
    /// place again, rebuilds each struct on its path, and stores the root.
    fn store_field(
        &mut self,
        place: &Place,
        idx: u32,
        v: Ref,
        (node, s): (NodeRef<'_>, NodeRef<'_>),
    ) {
        let at = s.index();
        let root = match place.root {
            Root::Local(l, d) => self.read_local(l, d, node).0,
            Root::Global(def, ty) => {
                let a = self.b.refs_record(&[Ref(def.raw())]);
                self.b.emit(Tag::GlobalGet, a, NONE, ty, at)
            }
            Root::Field(base, i, ty) => self.b.emit(Tag::Field, base.0, i, ty, at),
            Root::Deref(r, rt) => self.emit_deref(r, rt, at).0,
            // The element is read after its list's buffer is made unique,
            // so the rebuilt element shares nothing with another list's
            // (`types.coll.nested`).
            Root::Elem(list, index, ty) => {
                let rec = self.b.refs_record(&[list, index]);
                let op = hd_tir::ir::IntrinsicOp::ListIndexMut as u32;
                self.b.emit(Tag::Intrinsic, op, rec, ty, at)
            }
        };
        let mut vals = vec![root];
        for &(i, ty) in &place.path {
            let prev = vals[vals.len() - 1];
            vals.push(self.b.emit(Tag::Field, prev.0, i, ty, at));
        }
        let mut new = v;
        let mut field = idx;
        for (k, val) in vals.iter().enumerate().rev() {
            let ty = self.b.ty_of(*val);
            let rec = self.b.refs_record(&[Ref(field), new]);
            new = self.b.emit(Tag::CopyData, val.0, rec, ty, at);
            if k > 0 {
                field = place.path[k - 1].0;
            }
        }
        match place.root {
            Root::Local(l, d) => {
                self.note_write(l, d);
                if self.mut_params.contains(&l) {
                    self.param_writes.push((l, at));
                }
                self.set_local(l, new, at);
            }
            Root::Global(def, _) => {
                let a = self.b.refs_record(&[Ref(def.raw())]);
                self.b.emit(Tag::GlobalSet, a, new.0, Ty::VOID, at);
            }
            Root::Field(base, i, _) => {
                let rec = self.b.refs_record(&[Ref(i), new]);
                self.b.emit(Tag::FieldSet, base.0, rec, Ty::VOID, at);
            }
            Root::Deref(r, rt) => self.emit_ref_store(r, rt, new, at),
            Root::Elem(list, index, _) => {
                let rec = self.b.refs_record(&[list, index, new]);
                let op = hd_tir::ir::IntrinsicOp::ListSet as u32;
                self.b.emit(Tag::Intrinsic, op, rec, Ty::VOID, at);
            }
        }
    }

    /// `types.coll.map-value`: a write into a map value's field. A map
    /// value moves when the map rehashes, so it is no place; the fix-it
    /// reads the value, changes it and stores it back.
    fn map_value_write(&mut self, at: NodeRef<'_>, stmt: NodeRef<'_>, value: NodeRef<'_>) {
        self.err(
            Code::MapValueNotAddressable,
            at,
            "a map value is no place, so its fields cannot change in the map; read it, change it and store it back, or keep `&mut` values in the map",
        );
        // `m[k].x = v` becomes `let mut entry = m[k]`, `entry.x = v`,
        // `m[k] = entry`, each on its own line at the statement's indent.
        let src = self.cx.src;
        let (sp, vp) = (src.span(stmt), src.span(value));
        let text: &str = src.text;
        let (lo, hi, vlo, vhi) = (
            sp.lo as usize,
            sp.hi as usize,
            vp.lo as usize,
            vp.hi as usize,
        );
        if vlo != lo || vhi > hi || hi > text.len() {
            return;
        }
        let line = text[..lo].rfind('\n').map_or(0, |p| p + 1);
        let indent = &text[line..lo];
        if !indent.chars().all(|c| c == ' ') {
            return;
        }
        let place = &text[vlo..vhi];
        let rest = &text[vhi..hi];
        let new = format!("let mut entry = {place}\n{indent}entry{rest}\n{indent}{place} = entry");
        self.diags.add_fix(
            "read the value, change it and store it back",
            hd_diag::FixSafety::Suggestion,
            &[(sp, new.as_str())],
        );
    }
}

impl Ck<'_, '_> {
    /// Marks the `mut` parameters of the function `f`: each is its own
    /// changeable copy, so it may be reassigned and, for a struct, its
    /// fields are writable places (`fn.param.mut`). `mut self` on a data
    /// type keeps its `mut` type and is not marked.
    pub(crate) fn mark_mut_params(&mut self, f: NodeRef<'_>) {
        let Some(list) = hd_resolve::Src::child(f, SyntaxKind::ParameterList) else {
            return;
        };
        let toks = &self.cx.src.parse.tokens;
        let marked: Vec<_> = list
            .children()
            .filter(|p| p.kind() == SyntaxKind::Parameter)
            .filter(|p| {
                // `&mut self` is a reference, not a changeable copy.
                p.direct_token(toks, TokenKind::KwMut).is_some()
                    && p.direct_token(toks, TokenKind::Amp).is_none()
            })
            .map(|p| {
                if p.direct_token(toks, TokenKind::KwSelfValue).is_some() {
                    "self".to_owned()
                } else {
                    p.name(toks)
                        .map_or_else(String::new, |t| self.cx.src.text(t).to_owned())
                }
            })
            .collect();
        for name in marked {
            let sym = self.cx.names.syms.intern(&name);
            let Some(&l) = self.scopes[0].get(&sym) else {
                continue;
            };
            // A data type's `mut self` keeps its `mut` type. A rejected
            // `mut` view of a struct is marked, so one error is reported.
            let t = self.b.local_ty(l);
            if matches!(self.pool().get(t), TyData::Mut(_)) && !self.is_struct_ty(t) {
                continue;
            }
            self.mut_params.push(l);
            self.mut_locals.push(l);
        }
    }

    /// `lost-write` (`fn.param.mut.lost-write`): a statement of the body
    /// that writes a `mut` parameter, or a struct field of one, when no
    /// later statement of the body names the parameter, and no closure or
    /// `defer` in the body does. The write changes the callee's copy and is
    /// never seen. Only a statement directly in the body is judged, so a
    /// write the analysis cannot place stays silent.
    pub(crate) fn lost_writes(&mut self, body: NodeRef<'_>) {
        let mut writes = std::mem::take(&mut self.param_writes);
        writes.dedup();
        let stmts: Vec<_> = body.children().collect();
        for (l, at) in writes {
            let Some(i) = stmts.iter().position(|s| s.index() == at) else {
                continue;
            };
            let name = self
                .cx
                .names
                .text(self.b.body_mut().local_name[l.idx()])
                .to_owned();
            let named = |n: NodeRef<'_>| self.names_param(n, &name);
            let deferred = stmts.iter().any(|s| self.captures_param(*s, &name));
            if deferred || stmts[i + 1..].iter().any(|s| named(*s)) {
                continue;
            }
            let (what, help) = if name == "self" {
                ("`self`", "take `&mut self` to change the caller's value")
            } else {
                (
                    "the parameter",
                    "take `&mut T` to change the caller's value",
                )
            };
            let msg = if name == "self" {
                format!(
                    "this write to {what} is lost: the copy is never read afterwards or returned; {help}"
                )
            } else {
                format!(
                    "this write to {what} `{name}` is lost: the copy is never read afterwards or returned; {help}"
                )
            };
            let span = self.cx.src.span(stmts[i]);
            self.diags.push(
                Code::LostWrite,
                hd_diag::Severity::Warning,
                span,
                &msg,
                None,
            );
        }
    }

    /// Whether `n` names the parameter `name` anywhere in its tokens.
    fn names_param(&self, n: NodeRef<'_>, name: &str) -> bool {
        let src = self.cx.src;
        n.direct_tokens().any(|t| {
            matches!(
                src.tkind(t),
                Some(TokenKind::Ident | TokenKind::RawIdent | TokenKind::KwSelfValue)
            ) && src.text(t) == name
        }) || n.children().any(|c| self.names_param(c, name))
    }

    /// Whether a closure or a `defer` within `n` names the parameter, so it
    /// may read the copy after the write.
    fn captures_param(&self, n: NodeRef<'_>, name: &str) -> bool {
        match n.kind() {
            SyntaxKind::ClosureExpr | SyntaxKind::DeferStmt => self.names_param(n, name),
            _ => n.children().any(|c| self.captures_param(c, name)),
        }
    }
}

/// Whether a value of type `t` holds a value of the struct or enum
/// `target` inline: through struct fields, enum payloads, tuple elements,
/// optionals and newtypes, but not through a reference, a data type, a
/// collection, a trait value or a function (`data.struct.contains`,
/// `data.struct.no-recursion`, `data.enum.no-recursion`). `seen` holds the
/// types already searched.
pub(crate) fn holds_value(
    pool: hd_types::Types<'_>,
    lookup: &hd_resolve::Lookup<'_>,
    t: Ty,
    target: DefId,
    seen: &mut Vec<Ty>,
) -> bool {
    walk_values(pool, lookup, t, seen, &mut |d| d == target)
}

/// The declarations whose values a value of type `t` holds inline, as
/// [`holds_value`] searches them, each once, in the order first reached.
pub(crate) fn held_values(
    pool: hd_types::Types<'_>,
    lookup: &hd_resolve::Lookup<'_>,
    t: Ty,
    seen: &mut Vec<Ty>,
    out: &mut Vec<DefId>,
) {
    walk_values(pool, lookup, t, seen, &mut |d| {
        if !out.contains(&d) {
            out.push(d);
        }
        false
    });
}

/// Visits each declaration whose value `t` holds inline, stopping at the
/// first for which `found` is true; whether one was.
fn walk_values(
    pool: hd_types::Types<'_>,
    lookup: &hd_resolve::Lookup<'_>,
    t: Ty,
    seen: &mut Vec<Ty>,
    found: &mut dyn FnMut(DefId) -> bool,
) -> bool {
    if seen.contains(&t) || seen.len() > 256 {
        return false;
    }
    seen.push(t);
    let field_tys = |def: DefId, args: hd_types::TyList, tys: &mut Vec<Ty>| {
        let argv = pool.list_items(args);
        let sub = |f: Ty| {
            pool.subst(f, &|p: ParamRef| {
                (p.owner == def)
                    .then(|| argv.get(usize::from(p.index)).copied())
                    .flatten()
            })
        };
        let Some(item) = lookup.item(def) else {
            return;
        };
        match &item.data {
            hd_resolve::ItemData::Data(fs) if item.is_struct => {
                tys.extend(fs.iter().map(|f| sub(f.ty)));
            }
            hd_resolve::ItemData::Enum { shared, variants } => {
                tys.extend(
                    shared
                        .iter()
                        .chain(variants.iter().flat_map(|v| &v.fields))
                        .map(|f| sub(f.ty)),
                );
            }
            hd_resolve::ItemData::Newtype(base) => tys.push(sub(*base)),
            _ => {}
        }
    };
    let mut inner = Vec::new();
    match pool.get(t) {
        TyData::Adt { def, args } => {
            if found(def) {
                return true;
            }
            field_tys(def, args, &mut inner);
        }
        TyData::Option(i) | TyData::Mut(i) => inner.push(i),
        TyData::Tuple { elems, .. } => inner.extend(pool.list_items(elems).iter().copied()),
        _ => {}
    }
    inner
        .into_iter()
        .any(|i| walk_values(pool, lookup, i, seen, found))
}
