// The raw corruption corpus supplies independent facts for malformed records.
// Checked bodies obtain these relationships when their records are constructed.
use hd_base::DefId;
use hd_base::SliceSort as _;
use hd_types::Ty;
use ir::{Body, Callee, ChoiceKind, NONE, Ref, Tag, VerifyError, verify_checked};

/// Independent synthetic facts for raw records that checked bodies never emit
/// (`crates/hd_tir/DESIGN.md` §4.13.11).
pub trait VerifyCx {
    /// How many variants `ty` has, an enum or an optional; `None` for any
    /// other type (invariant 11).
    fn variant_count(&self, ty: Ty) -> Option<u32>;
    /// The sealed trait whose rows the compiler-supplied implementation
    /// `index` (a `BuiltinImpl` as `u32`) answers (invariant 13).
    fn builtin_trait(&self, index: u32) -> Option<DefId>;
    /// How many method-level bounds trait method `method` has
    /// (invariant 17).
    fn method_bounds(&self, method: DefId) -> Option<u32>;
    /// How many row keys a call of `callee` supplies providers for, when
    /// the callee's row is closed (invariant 8).
    fn row_keys(&self, callee: &Callee) -> Option<u32>;
}

/// [`verify_checked`], then relationships against independent raw-corpus facts:
/// - 8: a call's provider pairs, one per key of the callee's closed row,
///   and no `Pending` providers after `finish`;
/// - 11: a `SwitchTag` with no default has a case for every variant, and
///   `SwitchInt`, `SwitchChar` and `SwitchStr` have a default;
/// - 13: a `Builtin` choice that names a `BuiltinImpl` (its value is the
///   impl plus one) names one of the callee's trait;
/// - 17: a `CallDyn` has one evidence operand per method-level bound.
pub fn verify_in(b: &Body, cx: &dyn VerifyCx) -> Vec<VerifyError> {
    let mut errs = verify_checked(b);
    let ty_of = |v: u32| {
        if v & Ref::CONST_BIT != 0 {
            b.consts.get((v & !Ref::CONST_BIT) as usize).map(|c| c.0)
        } else {
            b.ty.get(v as usize).copied()
        }
    };
    for (i, (&tag, &[a, w])) in b.tags.iter().zip(&b.data).enumerate() {
        let inst = u32::try_from(i).expect("i");
        let mut err = |invariant: u8, what: String| {
            errs.push(VerifyError {
                invariant,
                inst,
                what,
            });
        };
        match tag {
            Tag::SwitchTag => {
                let r = b.record(w);
                let (cases, default) = r.split_at(r.len().saturating_sub(1));
                if default.first().is_some_and(|d| *d == NONE) {
                    let mut seen: Vec<u32> = cases.iter().step_by(2).copied().collect();
                    seen.sort_stable();
                    seen.dedup();
                    let have = u32::try_from(seen.len()).expect("cases");
                    if let Some(n) = ty_of(a).and_then(|t| cx.variant_count(t))
                        && have < n
                    {
                        err(11, format!("{have} of {n} variants and no default"));
                    }
                }
            }
            Tag::Call | Tag::Await => {
                let callee = Callee::from_words(b.record(a));
                if let Some(Callee::TraitMethod {
                    trait_,
                    choice: (ChoiceKind::Builtin, v),
                    ..
                }) = callee
                    && (1..=u32::from(u8::MAX)).contains(&v)
                    && cx.builtin_trait(v - 1) != Some(trait_)
                {
                    err(
                        13,
                        format!("a builtin choice {} not of the callee's trait", v - 1),
                    );
                }
                let r = b.record(w);
                if let [1, _, len] = r[r.len().saturating_sub(3)..]
                    && let Some(n) = callee.as_ref().and_then(|c| cx.row_keys(c))
                    && n != len
                {
                    err(8, format!("{len} provider pairs for {n} row keys"));
                }
            }
            Tag::CallDyn => {
                // `[method, type arguments, evidence count, evidence...,
                // arguments, providers]`.
                if let [method, _, count, ..] = *b.record(w)
                    && let Some(n) = cx.method_bounds(DefId::from_raw(method))
                    && n != count
                {
                    err(17, format!("{count} evidence operands for {n} bounds"));
                }
            }
            _ => {}
        }
    }
    errs
}
