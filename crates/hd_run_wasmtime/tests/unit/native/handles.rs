mod tests {
    use hd_host_abi::Handle;
    use hd_run_wasmtime::internals::native::handles::*;
    use hd_run_wasmtime::internals::native::{Answer, Dispatched};
    use std::any::Any;

    #[allow(clippy::unnecessary_wraps)]
    fn none(_: &mut dyn Any, _: usize, _: &[u8], _: &mut Handles) -> Dispatched {
        Ok(Answer::Ready(Vec::new()))
    }

    #[test]
    fn a_released_number_never_resolves_again_after_its_slot_is_reused() {
        let mut h = Handles::default();
        let a = h.insert(0, none, Box::new(1u8)).unwrap();
        h.release(a, 0).unwrap();
        let b = h.insert(0, none, Box::new(2u8)).unwrap();
        assert_ne!(a, b, "the reused slot issues a new number");
        assert_eq!(
            Handle(a - 1).slot(),
            Handle(b - 1).slot(),
            "the slot is reused"
        );
        assert!(h.lend(a, 0).is_err());
        assert!(h.release(a, 0).is_err());
        let lent = h.lend(b, 0).unwrap();
        assert_eq!(lent.value().downcast_ref::<u8>(), Some(&2));
        h.restore(lent);
        assert_eq!(h.live(), 1);
    }

    #[test]
    fn a_forged_zero_or_wrong_trait_number_resolves_nothing() {
        let mut h = Handles::default();
        let a = h.insert(3, none, Box::new(())).unwrap();
        for forged in [0, a + 1, u32::MAX, a + (1 << Handle::SLOT_BITS)] {
            assert_eq!(
                h.lend(forged, 3).err(),
                Some(fail("call of an invalid handle"))
            );
        }
        assert_eq!(
            h.release(a, 4).err(),
            Some(fail("release of an invalid handle"))
        );
        // A lent handle is not callable again until restored.
        let lent = h.lend(a, 3).unwrap();
        assert!(h.lend(a, 3).is_err());
        h.restore(lent);
        h.release(a, 3).unwrap();
        assert_eq!(h.live(), 0);
    }

    #[test]
    fn a_slot_whose_generations_are_spent_is_retired() {
        let mut h = Handles::default();
        let mut seen = hd_base::HashSet::default();
        for _ in 0..GENERATIONS {
            let n = h.insert(0, none, Box::new(())).unwrap();
            assert!(n > 0 && i32::try_from(n).is_ok());
            assert!(seen.insert(n), "number {n} issued twice");
            h.release(n, 0).unwrap();
        }
        assert_eq!(h.slot_count(), 1);
        let n = h.insert(0, none, Box::new(())).unwrap();
        assert_eq!(h.slot_count(), 2, "the spent slot is not reused");
        assert!(seen.insert(n));
    }

    #[test]
    fn the_table_is_bounded_by_its_slots() {
        let mut h = Handles::default();
        h.fill_vacant(SLOTS as usize, GENERATIONS - 1);
        assert_eq!(
            h.insert(0, none, Box::new(())).err(),
            Some(fail("handle space exhausted"))
        );
        // The largest number fits a positive i32.
        h.free_slot(SLOTS - 1);
        let n = h.insert(0, none, Box::new(())).unwrap();
        assert_eq!(n, i32::MAX as u32);
    }
}
