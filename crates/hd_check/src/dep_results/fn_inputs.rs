//! A function value where a declared function type stands for it
//! (`fn.type.lifetime.input`): what the type lets a caller pass at each
//! input must outlive what the value needs there. The type keeps no
//! lifetime (`fn.type.lifetime.identity`), so the need comes from the
//! value ([`Ck::written_inputs`]) and the allowance from the declaration
//! that types the place: a named call's parameter, a struct literal's
//! field, or the function's result. Cost: one trace per function-typed
//! argument, field and returned value; a local's part is traced once
//! ([`FnValues`]).

use super::{
    ANON, ANY, Body, BodyKind, Callee, Ck, Code, DepSig, FnValues, HashMap, ItemData, Layouts,
    NodeIdx, NodeRef, Part, Proj, STATIC, STATIC_SLOT, Spot, Tag, Ty, TyData, Var, fn_spot,
    own_inputs, returned, source_arg, w,
};

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
            for (k, need) in self.written_inputs(ls, b, (v, path), values) {
                let need = match need {
                    Var::Own(l) => Some(l),
                    Var::Life(STATIC) => Some(STATIC),
                    _ => None,
                };
                let fits = match (allows(k), need) {
                    (Allows::Instance | Allows::Own(STATIC), _) => true,
                    (Allows::Any, _) => false,
                    (Allows::Own(have), Some(need)) => have == need,
                    // A call's lifetime is checked where the value is
                    // called, against what that call passed.
                    (Allows::Own(_), None) => true,
                };
                if !fits {
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
            } else if b.tags[i] == Tag::NewStruct {
                let TyData::Adt { def, .. } = pool.get(b.ty[i]) else {
                    continue;
                };
                let Some(ItemData::Struct(fields, _)) = self.cx.lookup.item(def).map(|it| &it.data)
                else {
                    continue;
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
        out.extend(self.fn_field_paths(ls, t));
        out
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
