use hd_check::testing::Frames;

#[test]
fn the_base_frame_stays_through_pops_and_truncation() {
    let mut f = Frames::new(0);
    assert_eq!((*f.top(), f.len(), f.depth()), (0, 1, 0));
    f.push(1);
    f.push(2);
    assert_eq!((*f.top(), *f.base(), f.len(), f.depth()), (2, 0, 3, 2));
    assert_eq!(f.iter().copied().collect::<Vec<_>>(), [0, 1, 2]);
    assert_eq!(
        f.iter_rev().map(|(d, &x)| (d, x)).collect::<Vec<_>>(),
        [(2, 2), (1, 1), (0, 0)]
    );
    *f.top_mut() = 5;
    *f.base_mut() = 7;
    assert_eq!(f.pop(), Some(5));
    f.truncate(0);
    assert_eq!((*f.top(), f.len()), (7, 1));
    assert_eq!(f.pop(), None);
    *f.top_mut() = 9;
    assert_eq!(*f.base(), 9);
}
