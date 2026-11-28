mod capture_origin_tests {
    use hd_wasm::testing::emit::borrow::*;

    use hd_tir::ir::{Body, PlaceRoot, Proj};

    use hd_types::Ty;

    use hd_base::{DefId, LocalId, SubId};

    #[test]
    fn pointer_key_interning_is_independent_of_other_keys() {
        for (width, depth) in [(100_u32, 20), (1000, 20), (20, 1000)] {
            let mut borrows = Borrows::default();
            let keys: Vec<_> = (0..width)
                .map(|local| {
                    (
                        PlaceRoot::Local(LocalId::from_raw(local)),
                        vec![Proj::Field(0); depth],
                        ANY_FRAME,
                    )
                })
                .collect();
            for (expected, key) in keys.iter().enumerate() {
                assert_eq!(borrows.intern_key(key.clone()), expected);
            }
            for _ in 0..10 {
                for (expected, key) in keys.iter().enumerate() {
                    let before = borrows.key_work;
                    assert_eq!(borrows.intern_key(key.clone()), expected);
                    assert_eq!(borrows.key_work - before, depth + 1);
                }
            }
            assert_eq!(u32::try_from(borrows.keys.len()), Ok(width));
            assert_eq!(borrows.key_work, 12 * keys.len() * (depth + 1));
        }
    }

    #[test]
    fn pointer_keys_normalize_capture_origins_before_interning() {
        let body = body(3, &[(1, 0), (2, 1)]);
        let mut borrows = Borrows::default();
        borrows.capture_origins = CaptureOrigins::new(&body, &[], |_| true);
        for local in [2, 0, 1] {
            assert_eq!(
                borrows.intern_key((
                    PlaceRoot::Local(LocalId::from_raw(local)),
                    vec![],
                    ANY_FRAME
                )),
                0
            );
        }
        assert_eq!(borrows.keys.len(), 1);
        assert_eq!(borrows.key_index.len(), 1);
        assert_eq!(borrows.keys[0].0.0, PlaceRoot::Local(LocalId::from_raw(0)));
    }

    fn body(locals: usize, edges: &[(u32, u32)]) -> Body {
        let mut b = Body::new(DefId::from_raw(0), hd_tir::ir::BodyKind::Fn);
        b.local_ty = vec![Ty::VOID; locals];
        b.inlined_capture_origins = edges
            .iter()
            .map(|&(s, c)| {
                (
                    SubId::from_raw(0),
                    LocalId::from_raw(s),
                    LocalId::from_raw(c),
                )
            })
            .collect();
        b
    }

    #[test]
    fn only_whole_pointer_capture_keys_inherit_creator_context() {
        let b = body(5, &[(1, 0), (2, 1), (3, 0)]);
        let origins = CaptureOrigins::new(&b, &[], |l| l.raw() != 3);
        let root = PlaceRoot::Local(LocalId::from_raw(2));
        assert_eq!(
            origins.key((root, vec![], ANY_FRAME)).0,
            PlaceRoot::Local(LocalId::from_raw(0))
        );
        let field = (root, vec![Proj::Field(0)], ANY_FRAME);
        assert_eq!(origins.key(field.clone()), field);
        let cell = (root, vec![], 0);
        assert_eq!(origins.key(cell.clone()), cell);
        assert_eq!(origins.local(3), 3, "non-pointer capture has no exemption");
        assert_eq!(origins.local(4), 4, "ordinary aliases have no provenance");
    }

    #[test]
    fn capture_provenance_chains_have_linear_counted_work() {
        for width in [100u32, 1_000] {
            let depth = 20;
            let mut edges = Vec::new();
            for layer in 0..depth {
                for column in 0..width {
                    let snapshot = 1 + layer * width + column;
                    let creator = if layer == 0 { 0 } else { snapshot - width };
                    edges.push((snapshot, creator));
                }
            }
            let n = 1 + (width * depth) as usize;
            let b = body(n, &edges);
            let origins = CaptureOrigins::new(&b, &[], |_| true);
            assert_eq!(origins.work, 2 * n + edges.len());
            for l in 0..n {
                assert_eq!(origins.local(l), 0);
            }
        }
    }
}
