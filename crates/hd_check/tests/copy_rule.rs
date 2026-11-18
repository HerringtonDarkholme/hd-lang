use hd_base::{DefId, HashMap, Symbol};
use hd_check::{CopyAnswer, copy_answer};
use hd_resolve::{Generic, Item, ItemData, TypeAttrs};
use hd_types::{InternPool, ParamRef, Ty, TyData, TyList};

#[test]
fn an_ownership_reborrow_does_not_hide_a_later_resource_after_an_open_parameter() {
    let pool = InternPool::new();
    let resource = Item::new(
        DefId::from_raw(1),
        Symbol::NONE,
        true,
        ItemData::Struct(
            vec![],
            TypeAttrs {
                not_copy: true,
                ..TypeAttrs::default()
            },
        ),
    );
    let mut generic = Item::new(
        DefId::from_raw(2),
        Symbol::NONE,
        true,
        ItemData::Struct(vec![], TypeAttrs::default()),
    );
    generic.generics.push(Generic::plain(Symbol::NONE));
    let parameter = pool.intern_ty(&TyData::Param(ParamRef {
        owner: generic.def,
        index: 0,
    }));
    let reference = pool.intern_ty(&TyData::Ref {
        inner: Ty::I32,
        mutable: true,
        counted: false,
    });
    let resource_ty = pool.intern_ty(&TyData::Adt {
        def: resource.def,
        args: TyList::EMPTY,
    });
    let tuple = pool.intern_ty(&TyData::Tuple {
        elems: pool.list(&[reference, parameter, resource_ty]),
        rest: None,
    });
    let answer = copy_answer(
        pool.types(),
        &|d| {
            if d == resource.def {
                Some(&resource)
            } else if d == generic.def {
                Some(&generic)
            } else {
                None
            }
        },
        tuple,
        &mut |t| t,
        &mut |_| None,
        &mut |_, _| {},
    );
    assert_eq!(answer.blocker, Some(resource_ty));
}

#[test]
fn repeated_reborrow_members_cache_each_type_at_width_thousand_depth_twenty() {
    let pool = InternPool::new();
    let reference = pool.intern_ty(&TyData::Ref {
        inner: Ty::I32,
        mutable: true,
        counted: false,
    });
    for width in [100, 1_000] {
        let mut ty = reference;
        for _ in 0..20 {
            ty = pool.intern_ty(&TyData::Tuple {
                elems: pool.list(&vec![ty; width]),
                rest: None,
            });
        }
        let mut memo = HashMap::<Ty, CopyAnswer>::default();
        let mut done = Vec::new();
        let answer = copy_answer(
            pool.types(),
            &|_| None,
            ty,
            &mut |t| t,
            &mut |t| memo.get(&t).copied(),
            &mut |t, a| done.push((t, a)),
        );
        assert_eq!(answer.blocker, None);
        assert_eq!(done.len(), 21);
        memo.extend(done);
        let answer = copy_answer(
            pool.types(),
            &|_| None,
            ty,
            &mut |t| t,
            &mut |t| memo.get(&t).copied(),
            &mut |_, _| panic!("a warm type query must not walk a member"),
        );
        assert_eq!(answer.blocker, None);
    }
}

#[test]
fn an_associated_member_uses_the_instances_normal_form() {
    let pool = InternPool::new();
    let projection = pool.intern_ty(&TyData::Assoc {
        assoc: DefId::from_raw(1),
        trait_: DefId::from_raw(2),
        self_ty: Ty::I32,
        args: TyList::EMPTY,
    });
    let tuple = pool.intern_ty(&TyData::Tuple {
        elems: pool.list(&[projection]),
        rest: None,
    });
    let mut normalized = Vec::new();
    let answer = copy_answer(
        pool.types(),
        &|_| None,
        tuple,
        &mut |ty| {
            normalized.push(ty);
            Ty::I32
        },
        &mut |_| None,
        &mut |_, _| {},
    );
    assert_eq!(normalized, [projection]);
    assert_eq!(answer.blocker, None);
}
