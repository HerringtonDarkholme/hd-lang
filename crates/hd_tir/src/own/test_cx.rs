//! Shared defaults for ownership test contexts. Each test keeps only its
//! policy differences; production `OwnCx` implementations remain explicit.
/// Delegate a test context's unchanged policies to its shared base.
#[macro_export]
macro_rules! own_cx {
    ($ty:ty, [$($method:ident),* $(,)?], {$($overrides:item)*}) => {
        $crate::own_cx!($ty, [$($method),*], $crate::own::tests::Cx, {$($overrides)*});
    };
    ($ty:ty, [$($method:ident),* $(,)?], $base:expr, {$($overrides:item)*}) => {
        impl $crate::own::OwnCx for $ty {
            $($crate::own_cx!(@ $method, $base);)*
            $($overrides)*
        }
    };
    (@ temporary_owner_name, $base:expr) => {
        fn temporary_owner_name(&self) -> hd_base::Symbol {
            $crate::own::OwnCx::temporary_owner_name(&$base)
        }
    };
    (@ counted, $base:expr) => {
        fn counted(&self, ty: hd_types::Ty) -> bool {
            $crate::own::OwnCx::counted(&$base, ty)
        }
    };
    (@ pass, $base:expr) => {
        fn pass(&self, callee: &$crate::ir::Callee, k: usize, n: usize) -> $crate::own::Pass {
            $crate::own::OwnCx::pass(&$base, callee, k, n)
        }
    };
    (@ pass_value, $base:expr) => {
        fn pass_value(&self, ty: hd_types::Ty, k: usize, n: usize) -> $crate::own::Pass {
            $crate::own::OwnCx::pass_value(&$base, ty, k, n)
        }
    };
    (@ may_write, $base:expr) => {
        fn may_write(&self, callee: &$crate::ir::Callee) -> bool {
            $crate::own::OwnCx::may_write(&$base, callee)
        }
    };
    (@ parts, $base:expr) => {
        fn parts(&self, ty: hd_types::Ty, variant: Option<u32>, out: &mut Vec<hd_types::Ty>) -> bool {
            $crate::own::OwnCx::parts(&$base, ty, variant, out)
        }
    };
    (@ received, $base:expr) => {
        fn received(&self, item: hd_base::DefId, k: usize, n: usize) -> $crate::own::Pass {
            $crate::own::OwnCx::received(&$base, item, k, n)
        }
    };
    (@ holds_coll, $base:expr) => {
        fn holds_coll(&self, ty: hd_types::Ty) -> bool {
            $crate::own::OwnCx::holds_coll(&$base, ty)
        }
    };
}
pub(crate) use own_cx;
