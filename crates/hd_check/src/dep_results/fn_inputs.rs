//! A function value where a declared function type stands for it
//! (`fn.type.lifetime.input`): what the type lets a caller pass at each
//! input must outlive what the value needs there. The type keeps no
//! lifetime (`fn.type.lifetime.identity`), so the need comes from the
//! value ([`Ck::written_inputs`]) and the allowance from the declaration
//! that types the place: a named call's parameter, a parameter of a
//! function value a parameter holds, a struct literal's or a variant's
//! field, a place a parameter's reference reaches, or the function's
//! result. A value a call returned needs what that call passed at the
//! callee's lifetime. Cost: one trace per function-typed argument, field,
//! store and returned value; a local's part is traced once ([`FnValues`]),
//! and a call's lifetime once per check.

use super::{
    ANON, ANY, Body, BodyKind, Callee, Ck, Code, DefId, DepSig, FnValues, HashMap, HashSet,
    ItemData, Layouts, NodeIdx, NodeRef, Part, PlaceRoot, Proj, STATIC, STATIC_SLOT, Spot, Tag, Ty,
    TyData, Var, fn_spot, own_inputs, returned, source_arg, w,
};
use hd_resolve::FnSig;
use hd_types::ParamRef;

/// What a declared function type lets a caller pass at one input.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Allows {
    /// No lifetime written: a reference of any lifetime.
    Any,
    /// A lifetime each use instantiates, a callee's or a struct's: the
    /// value's need goes with it (`fn.type.lifetime.call.written`).
    Instance,
    /// The body's own lifetime, `'static` among them.
    Own(u32),
}

impl Ck<'_, '_> {
    /// The function values of `b` that stand where a declared function
    /// type lets a caller pass a reference shorter than an input of the
    /// value needs: one `type-mismatch` each, at the value.
    pub(super) fn input_fits(
        &self,
        ls: &Layouts<'_>,
        b: &Body,
        values: &FnValues<'_>,
        (root, fun): (u32, Option<NodeRef<'_>>),
        errs: &mut Vec<(NodeIdx, Code, String)>,
    ) {
        let pool = ls.pool;
        let mut check = |v: u32, path: Vec<Part>, allows: &dyn Fn(usize) -> Allows| {
            for (k, var) in self.written_inputs(ls, b, (v, path), values) {
                let mut needs = Vec::new();
                self.needs_of(ls, b, values, var, &mut needs, &mut HashSet::default());
                let fits = |need: Option<u32>| match (allows(k), need) {
                    (Allows::Instance | Allows::Own(STATIC), _) => true,
                    (Allows::Own(have), Some(need)) => have == need,
                    (Allows::Any | Allows::Own(_), _) => false,
                };
                if let Some(&need) = needs.iter().find(|&&n| !fits(n)) {
                    let at = b.syn.get(v as usize).copied().unwrap_or(NodeIdx::NONE);
                    errs.push((
                        at,
                        Code::TypeMismatch,
                        self.input_fit_message(k, need, allows(k), fun),
                    ));
                    return;
                }
            }
        };
        let own = self.cx.lookup.item(b.item).and_then(|it| it.sig());
        for i in 0..b.tags.len() {
            if matches!(b.tags[i], Tag::Call | Tag::Await) {
                let Some(sig) = Callee::from_words(b.record(b.data[i][0]))
                    .map(|c| match c {
                        Callee::Item { def, .. } => def,
                        Callee::TraitMethod { method, .. } => method,
                    })
                    .and_then(|def| self.cx.lookup.item(def)?.sig())
                else {
                    continue;
                };
                for (p, &(_, pt)) in sig.params.iter().enumerate() {
                    let paths = self.fn_paths(ls, pt);
                    let Some(a) = (!paths.is_empty()).then(|| source_arg(b, i, p)).flatten() else {
                        continue;
                    };
                    let rec = sig.fn_lifetimes.get(p).map_or(&[][..], Vec::as_slice);
                    for path in paths {
                        let (by, rest) = self.declared_allows(ls, pt, &path, rec, None);
                        check(a, path, &|k| by.get(&k).copied().unwrap_or(rest));
                    }
                }
            } else if b.tags[i] == Tag::CallValue {
                let mut sources = Vec::new();
                self.callee_sources(
                    b,
                    own,
                    values,
                    b.data[i][0],
                    &mut sources,
                    &mut HashSet::default(),
                );
                for source in sources {
                    let pts: Vec<Ty> = match source {
                        Source::Item(sig) => sig.params.iter().map(|&(_, t)| t).collect(),
                        Source::Param(ct, _) | Source::Declared(ct, _) => {
                            let TyData::Fn { params, .. } = pool.get(ct) else {
                                continue;
                            };
                            pool.list_items(params).to_vec()
                        }
                    };
                    let mut at = 0;
                    for (p, &pt) in pts.iter().enumerate() {
                        let n = pool.fn_written_slots(pt);
                        // What the source's declaration writes in this
                        // input's type, and whose lifetimes they are.
                        let (rec, own_lifetimes) = match source {
                            Source::Param(_, rec) => (rec.get(at..at + n).unwrap_or(&[]), true),
                            Source::Item(sig) => (
                                sig.fn_lifetimes.get(p).map_or(&[][..], Vec::as_slice),
                                false,
                            ),
                            Source::Declared(_, rec) => (rec.get(at..at + n).unwrap_or(&[]), false),
                        };
                        at += n;
                        let Some(a) = source_arg(b, i, p) else {
                            continue;
                        };
                        let outs = own_lifetimes.then_some(&[][..]);
                        for path in self.fn_paths(ls, pt) {
                            let (by, rest) = self.declared_allows(ls, pt, &path, rec, outs);
                            check(a, path, &|k| by.get(&k).copied().unwrap_or(rest));
                        }
                    }
                }
            } else if b.tags[i] == Tag::Store {
                // A place a parameter's reference reaches is the caller's,
                // typed by the parameter's declared type, reached directly
                // or through a local holding that reference or a reborrow.
                let Some(place) = b.decode_place(b.data[i][0]) else {
                    continue;
                };
                let l = match place.root {
                    PlaceRoot::Local(l) => Some(l.raw() as usize),
                    PlaceRoot::Value(r) => whole_local_read(b, r.0),
                    PlaceRoot::Global(..) | PlaceRoot::Frame(_) | PlaceRoot::Ptr(_) => None,
                };
                let (Some(l), Some(Proj::Deref)) = (l, place.proj.first()) else {
                    continue;
                };
                let (Some(sig), Some(mine)) = (own.filter(|_| b.kind == BodyKind::Fn), values.mine)
                else {
                    continue;
                };
                let v = b.data[i][1];
                let Some(vt) = b.ty.get(v as usize).copied() else {
                    continue;
                };
                let mut places = Vec::new();
                param_places(
                    b,
                    values,
                    (l, place.proj.clone()),
                    &mut places,
                    &mut HashSet::default(),
                );
                for (p, proj) in places {
                    let (Some(&(_, pt)), Some((_, ids))) = (sig.params.get(p), mine.params.get(p))
                    else {
                        continue;
                    };
                    let rec = sig.fn_lifetimes.get(p).map_or(&[][..], Vec::as_slice);
                    let ids: Vec<Option<u32>> = ids.iter().map(|&id| Some(id)).collect();
                    for sub in self.fn_paths(ls, vt) {
                        let full: Vec<Part> = (proj.iter().map(|&q| Part::Proj(q)))
                            .chain(sub.iter().copied())
                            .collect();
                        let (by, rest) = self.declared_allows(ls, pt, &full, rec, Some(&ids));
                        check(v, sub, &|k| by.get(&k).copied().unwrap_or(rest));
                    }
                }
            } else if matches!(b.tags[i], Tag::NewStruct | Tag::NewVariant) {
                let TyData::Adt { def, .. } = pool.get(b.ty[i]) else {
                    continue;
                };
                let fields = match (self.cx.lookup.item(def).map(|it| &it.data), b.tags[i]) {
                    (Some(ItemData::Struct(fields, _)), Tag::NewStruct) => fields,
                    (Some(ItemData::Enum { variants, .. }), Tag::NewVariant) => {
                        match variants.get(b.data[i][0] as usize) {
                            Some(v) => &v.fields,
                            None => continue,
                        }
                    }
                    _ => continue,
                };
                for (f, &v) in fields.iter().zip(b.record(b.data[i][1])) {
                    if !matches!(pool.get(f.ty), TyData::Fn { .. }) {
                        continue;
                    }
                    let rec = own_inputs(pool, f.ty, &f.fn_lifetimes);
                    check(v, Vec::new(), &|k| match rec.get(k) {
                        Some(&l) if l != 0 => Allows::Instance,
                        _ => Allows::Any,
                    });
                }
            }
        }
        // The function's result (`ref.dep.return`).
        let Some(sig) = own.filter(|_| b.kind == BodyKind::Fn && fun.is_some()) else {
            return;
        };
        let rec = sig.fn_lifetimes.get(sig.params.len());
        let rec = rec.map_or(&[][..], Vec::as_slice);
        let is_self = |s| self.cx.names.text(s) == "self";
        let outs = DepSig::new(ls, &sig.params, is_self, sig.ret, &sig.lifetimes).outs;
        let paths = self.fn_paths(ls, sig.ret);
        let returned = if paths.is_empty() {
            Vec::new()
        } else {
            returned(b, root)
        };
        for path in paths {
            let (by, rest) = self.declared_allows(ls, sig.ret, &path, rec, Some(&outs));
            for &v in &returned {
                check(v, path.clone(), &|k| by.get(&k).copied().unwrap_or(rest));
            }
        }
    }

    /// The parts of a value of type `t` that hold a function type whose
    /// inputs take references: `t` itself, a tuple element, or a struct
    /// field [`Ck::fn_field_paths`] finds.
    fn fn_paths(&self, ls: &Layouts<'_>, t: Ty) -> Vec<Vec<Part>> {
        let pool = ls.pool;
        let takes =
            |t: Ty| matches!(pool.get(t), TyData::Fn { .. }) && pool.fn_written_slots(t) != 0;
        let mut out = Vec::new();
        if takes(t) {
            out.push(Vec::new());
        } else if let TyData::Tuple { elems, .. } = pool.get(t) {
            for (j, &e) in pool.list_items(elems).iter().enumerate() {
                let at = Part::Proj(Proj::Field(w(j)));
                if takes(e) {
                    out.push(vec![at]);
                }
                for p in self.fn_field_paths(ls, e) {
                    out.push(std::iter::once(at).chain(p).collect());
                }
            }
        }
        // A variant's field whose function type writes a lifetime.
        if let TyData::Adt { def, .. } = pool.get(t)
            && let Some(ItemData::Enum { variants, .. }) =
                self.cx.lookup.item(def).map(|it| &it.data)
        {
            for (k, v) in variants.iter().enumerate() {
                for (j, f) in v.fields.iter().enumerate() {
                    if takes(f.ty) && f.fn_lifetimes.iter().any(|&l| l != 0) {
                        out.push(vec![
                            Part::Proj(Proj::Variant(w(k))),
                            Part::Proj(Proj::Field(w(j))),
                        ]);
                    }
                }
            }
        }
        out.extend(self.fn_field_paths(ls, t));
        out
    }

    /// The sources of the declared type of function value `v`, into
    /// `out`: a parameter read whole, a named function, or a closure
    /// literal, through copies into locals and coercions. A value of
    /// another kind adds none.
    fn callee_sources<'s>(
        &self,
        b: &Body,
        own: Option<&'s FnSig>,
        values: &FnValues<'_>,
        v: u32,
        out: &mut Vec<Source<'s>>,
        seen: &mut HashSet<usize>,
    ) where
        Self: 's,
    {
        let Some(&tag) = b.tags.get(v as usize) else {
            return;
        };
        if let Some(l) = whole_local_read(b, v) {
            let param = b
                .local_flags
                .get(l)
                .is_some_and(|f| f & hd_tir::ir::local_flags::PARAM != 0);
            match own.filter(|_| param && b.kind == BodyKind::Fn) {
                Some(sig) => {
                    if let Some(&(_, t)) = sig.params.get(l) {
                        let rec = sig.fn_lifetimes.get(l).map_or(&[][..], Vec::as_slice);
                        out.push(Source::Param(t, rec));
                    }
                }
                None if seen.insert(l) => {
                    for &s in values.stores(w(l)) {
                        if b.decode_place(b.data[s as usize][0])
                            .is_some_and(|p| p.proj.is_empty())
                        {
                            self.callee_sources(b, own, values, b.data[s as usize][1], out, seen);
                        }
                    }
                }
                None => {}
            }
        } else if tag == Tag::Coerce {
            self.callee_sources(b, own, values, b.data[v as usize][0], out, seen);
        } else if tag == Tag::Closure {
            out.push(Source::Declared(b.ty[v as usize], &[]));
        } else if matches!(tag, Tag::Call | Tag::Await)
            && let Some(sig) = Callee::from_words(b.record(b.data[v as usize][0]))
                .map(|c| match c {
                    Callee::Item { def, .. } => def,
                    Callee::TraitMethod { method, .. } => method,
                })
                .and_then(|def| self.cx.lookup.item(def)?.sig())
            && matches!(self.pool().get(sig.ret), TyData::Fn { .. })
        {
            // A call's result: the callee's declared result type.
            let rec = sig.fn_lifetimes.get(sig.params.len());
            out.push(Source::Declared(
                b.ty[v as usize],
                rec.map_or(&[][..], Vec::as_slice),
            ));
        } else if let Some(f) = self.read_field(b, v)
            && matches!(self.pool().get(f.ty), TyData::Fn { .. })
        {
            // A struct field's declared function type.
            out.push(Source::Declared(b.ty[v as usize], &f.fn_lifetimes));
        } else if tag == Tag::ItemRef
            && let Some(sig) = (b.record(b.data[v as usize][0]).first())
                .and_then(|&d| self.cx.lookup.item(DefId::from_raw(d))?.sig())
        {
            out.push(Source::Item(sig));
        }
    }

    /// The struct field value `v` reads, through dereferences and fields
    /// of its place.
    fn read_field<'s>(&self, b: &Body, v: u32) -> Option<&'s hd_resolve::Field>
    where
        Self: 's,
    {
        let pool = self.pool();
        if b.tags.get(v as usize) != Some(&Tag::Read) {
            return None;
        }
        let place = b.decode_place(b.data[v as usize][0])?;
        let mut t = match place.root {
            PlaceRoot::Local(l) => *b.local_ty.get(l.idx())?,
            PlaceRoot::Value(r) => *b.ty.get(r.0 as usize)?,
            PlaceRoot::Global(_, t) => t,
            PlaceRoot::Frame(_) | PlaceRoot::Ptr(_) => return None,
        };
        let (&Proj::Field(j), init) = place.proj.split_last()? else {
            return None;
        };
        let field = |t: Ty, k: u32| {
            let TyData::Adt { def, args } = pool.get(t) else {
                return None;
            };
            let Some(ItemData::Struct(fields, _)) = self.cx.lookup.item(def).map(|i| &i.data)
            else {
                return None;
            };
            Some((fields.get(k as usize)?, def, args))
        };
        for &q in init {
            t = match q {
                Proj::Deref => match pool.get(t) {
                    TyData::Ref { inner, .. } => inner,
                    _ => return None,
                },
                Proj::Field(k) => {
                    let (f, def, args) = field(t, k)?;
                    let args = pool.list_items(args);
                    pool.subst(f.ty, &|p: ParamRef| {
                        (p.owner == def).then(|| args.get(usize::from(p.index)).copied())?
                    })
                }
                Proj::Variant(_) | Proj::Elem(_) => return None,
            };
        }
        if let TyData::Ref { inner, .. } = pool.get(t) {
            t = inner;
        }
        Some(field(t, j)?.0)
    }

    /// The lifetimes a function value's written input `var` needs a
    /// caller's reference to outlive, into `out`; `None` for one that is
    /// no lifetime of the body's. A call's lifetime is what the call passed
    /// there: the needs of a function value it took at that lifetime, and
    /// the lifetime of a parameter or a top-level binding it took a
    /// reference to (`fn.type.lifetime.call.written`).
    fn needs_of(
        &self,
        ls: &Layouts<'_>,
        b: &Body,
        values: &FnValues<'_>,
        var: Var,
        out: &mut Vec<Option<u32>>,
        seen: &mut HashSet<(u32, u32)>,
    ) {
        let (c, l) = match var {
            Var::Own(l) => return out.push(Some(l)),
            Var::Life(STATIC) => return out.push(Some(STATIC)),
            Var::At(c, l) => (c, l),
            Var::Life(_) | Var::Param(..) | Var::Held => return out.push(None),
        };
        if !seen.insert((c, l)) {
            return;
        }
        let Some(sig) = Callee::from_words(b.record(b.data[c as usize][0]))
            .map(|x| match x {
                Callee::Item { def, .. } => def,
                Callee::TraitMethod { method, .. } => method,
            })
            .and_then(|def| self.cx.lookup.item(def)?.sig())
        else {
            return out.push(None);
        };
        let dep = self.call_dep(ls, sig);
        for (p, (pt, ids)) in dep.params.iter().enumerate() {
            let Some(a) = source_arg(b, c as usize, p) else {
                continue;
            };
            // A reference passed at `l`: what it lives for.
            if ids.contains(&l) {
                let param = whole_local_read(b, a).filter(|&x| {
                    b.kind == BodyKind::Fn
                        && b.local_flags
                            .get(x)
                            .is_some_and(|f| f & hd_tir::ir::local_flags::PARAM != 0)
                });
                let lived = param.and_then(|x| {
                    let mine = &values.mine?.params.get(x)?.1;
                    let at = ids.iter().position(|&id| id == l)?;
                    mine.get(at).copied().filter(|&id| id < ANON)
                });
                let global = b.tags.get(a as usize) == Some(&Tag::Ref)
                    && b.decode_place(b.data[a as usize][0])
                        .is_some_and(|p| matches!(p.root, PlaceRoot::Global(..)));
                out.push(if global { Some(STATIC) } else { lived });
            }
            // A function value passed with `l` at its inputs.
            let rec = sig.fn_lifetimes.get(p).map_or(&[][..], Vec::as_slice);
            let rec = own_inputs(ls.pool, *pt, rec);
            if rec.iter().any(|&x| u32::from(x) == l) {
                for (k, var) in self.written_inputs(ls, b, (a, Vec::new()), values) {
                    if rec.get(k).is_some_and(|&x| u32::from(x) == l) {
                        self.needs_of(ls, b, values, var, out, seen);
                    }
                }
            }
        }
    }

    /// What the declared type `t`, whose function types write `rec`
    /// ([`hd_resolve::FnSig::fn_lifetimes`]), lets a caller pass at each
    /// input of the function type at `path`, by input, and at the inputs
    /// it does not list. `outs`, a result's slot lifetimes, makes a
    /// written lifetime the body's own; without it a parameter's is the
    /// callee's.
    fn declared_allows(
        &self,
        ls: &Layouts<'_>,
        t: Ty,
        path: &[Part],
        rec: &[u16],
        outs: Option<&[Option<u32>]>,
    ) -> (HashMap<usize, Allows>, Allows) {
        let written = |l: u32| match outs {
            _ if l == STATIC => Allows::Own(STATIC),
            Some(_) => Allows::Own(l),
            None => Allows::Instance,
        };
        let mut by_input: HashMap<usize, Allows> = HashMap::default();
        match fn_spot(self.cx, ls.pool, t, path) {
            Some(Spot::Fn(lo, n)) => {
                for (k, &l) in rec.iter().skip(lo).take(n).enumerate() {
                    if l != 0 {
                        by_input.insert(k, written(u32::from(l)));
                    }
                }
            }
            Some(Spot::Field(at)) => {
                for (k, slot) in self.field_inputs(ls, t, path, at).unwrap_or_default() {
                    let l = match (slot, outs) {
                        (STATIC_SLOT, _) => Some(STATIC),
                        (_, Some(outs)) => outs.get(slot).copied().flatten(),
                        (_, None) => Some(ANY),
                    };
                    let allows = match l {
                        Some(STATIC) => Allows::Own(STATIC),
                        Some(l) if outs.is_some() && l < ANON => Allows::Own(l),
                        _ => Allows::Instance,
                    };
                    by_input.insert(k, allows);
                }
            }
            // A result's own function type's fields are typed by that
            // function type, which a caller of the value instantiates.
            Some(Spot::ResultField { .. }) | None => return (by_input, Allows::Instance),
        }
        (by_input, Allows::Any)
    }

    /// `fn.type.lifetime.input`'s message for input `k`.
    fn input_fit_message(
        &self,
        k: usize,
        need: Option<u32>,
        allows: Allows,
        fun: Option<NodeRef<'_>>,
    ) -> String {
        let lifetimes = fun.map(|f| self.lifetime_names(f)).unwrap_or_default();
        let name = |l: u32| match l {
            STATIC => "`'static`".to_owned(),
            l => (lifetimes.get((l as usize).wrapping_sub(1)))
                .map_or_else(|| "a lifetime of its own".to_owned(), |n| format!("`{n}`")),
        };
        let need = need.map_or_else(|| "a lifetime of its own".to_owned(), name);
        let have = match allows {
            Allows::Own(l) => format!("one that lives only for {}", name(l)),
            _ => "one of any lifetime".to_owned(),
        };
        format!(
            "this function value needs the reference at its input {} to live for {need}, but its type here lets a caller pass {have}",
            k + 1
        )
    }
}

/// The local value `v` reads whole, if it does.
fn whole_local_read(b: &Body, v: u32) -> Option<usize> {
    if b.tags.get(v as usize) != Some(&Tag::Read) {
        return None;
    }
    let place = b.decode_place(b.data[v as usize][0])?;
    match place.root {
        PlaceRoot::Local(l) if place.proj.is_empty() => Some(l.raw() as usize),
        PlaceRoot::Local(_)
        | PlaceRoot::Value(_)
        | PlaceRoot::Global(..)
        | PlaceRoot::Frame(_)
        | PlaceRoot::Ptr(_) => None,
    }
}

/// Where the declared type of a called function value comes from, which
/// writes the lifetimes of its inputs' function types: a parameter's
/// declared type and its record ([`FnSig::fn_lifetimes`]), the body's
/// own lifetimes; a named function's signature, the callee's; or a
/// declared function type whose lifetimes each use instantiates: a call's
/// result or a struct field, or a closure literal, whose annotations
/// write none (`ref.lifetime.not-yet.body`).
#[derive(Clone, Copy)]
enum Source<'s> {
    Param(Ty, &'s [u16]),
    Item(&'s FnSig),
    /// A declared function type and its record, whose written lifetimes
    /// each use instantiates: a call's result, a struct field, or a
    /// closure literal's, which writes none.
    Declared(Ty, &'s [u16]),
}

/// The parameter places the place rooted at local `l` with projection
/// `proj`, which starts with a dereference, reaches: `l` itself when it
/// is a parameter, or through each value stored into `l`, a parameter's
/// reference copied whole or a reborrow of a place a parameter roots.
fn param_places(
    b: &Body,
    values: &FnValues<'_>,
    (l, proj): (usize, Vec<Proj>),
    out: &mut Vec<(usize, Vec<Proj>)>,
    seen: &mut HashSet<usize>,
) {
    if b.local_flags
        .get(l)
        .is_some_and(|f| f & hd_tir::ir::local_flags::PARAM != 0)
    {
        return out.push((l, proj));
    }
    if !seen.insert(l) {
        return;
    }
    for &s in values.stores(w(l)) {
        let whole = b
            .decode_place(b.data[s as usize][0])
            .is_some_and(|p| p.proj.is_empty());
        let v = b.data[s as usize][1];
        if !whole {
            continue;
        }
        if let Some(m) = whole_local_read(b, v) {
            param_places(b, values, (m, proj.clone()), out, seen);
        } else if b.tags.get(v as usize) == Some(&Tag::Ref)
            && let Some(q) = b.decode_place(b.data[v as usize][0])
        {
            let m = match q.root {
                PlaceRoot::Local(m) => Some(m.raw() as usize),
                PlaceRoot::Value(r) => whole_local_read(b, r.0),
                PlaceRoot::Global(..) | PlaceRoot::Frame(_) | PlaceRoot::Ptr(_) => None,
            };
            let full: Vec<Proj> = q.proj.iter().chain(&proj[1..]).copied().collect();
            if let Some(m) = m.filter(|_| q.proj.first() == Some(&Proj::Deref)) {
                param_places(b, values, (m, full), out, seen);
            }
        }
    }
}
