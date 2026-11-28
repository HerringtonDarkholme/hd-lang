use hd_base::{DefId, Stage, Symbol};
use hd_check::testing::resolved_callable;
use hd_resolve::{CallAttrs, FnAttrs, FnSig, Item, ItemData, TypeAttrs};
use hd_types::Ty;

#[test]
fn functions_and_abstract_or_concrete_methods_borrow_their_signature() {
    let signature = || FnSig::simple(vec![], vec![], Ty::I32);
    let method = |has_body| ItemData::Method {
        owner: DefId::from_raw(2),
        sig: signature(),
        has_body,
        call: CallAttrs::default(),
    };
    for data in [
        ItemData::Fn(signature(), FnAttrs::default()),
        method(false),
        method(true),
    ] {
        let item = Item::new(DefId::from_raw(1), Symbol::from_raw(1), true, data);
        let (header, sig) = resolved_callable(Some(&item)).expect("callable");
        assert!(std::ptr::eq(header, &raw const item));
        assert!(std::ptr::eq(sig, item.sig().expect("signature")));
    }
}

#[test]
fn absent_or_non_callable_headers_return_internal_errors() {
    let builtin = Item::new(
        DefId::from_raw(1),
        Symbol::from_raw(1),
        false,
        ItemData::Builtin,
    );
    let data = Item::new(
        DefId::from_raw(2),
        Symbol::from_raw(2),
        false,
        ItemData::Struct(vec![], TypeAttrs::default()),
    );
    for item in [None, Some(&builtin), Some(&data)] {
        let error = resolved_callable(item).expect_err("invalid header");
        assert_eq!(error.stage, Stage::Body);
        assert!(error.bug);
        assert!(!error.out_of_fuel);
    }
}
