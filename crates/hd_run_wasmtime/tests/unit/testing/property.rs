mod tests {
    use hd_run_wasmtime::internals::testing::property::{PlanKind, Property, Trial, shrink};

    #[test]
    fn examples_saved_stream_and_fresh_plans_have_consecutive_seeds() {
        let mut property = Property::new(2, 10, 1, 20, vec![vec![8, 3]]);
        let example = property.next().expect("plan").expect("example");
        assert_eq!(
            (example.kind, example.example, example.seed, example.size),
            (PlanKind::Example, Some(0), 20, 0)
        );
        property.pass(example.kind);
        let saved = property.next().expect("plan").expect("saved");
        assert_eq!(
            (saved.kind, saved.seed, saved.size, saved.replay),
            (PlanKind::Saved, 21, 0, vec![8, 3])
        );
        property.pass(saved.kind);
        let first = property.next().expect("plan").expect("fresh");
        assert_eq!(
            (first.kind, first.seed, first.size),
            (PlanKind::Fresh, 22, 0)
        );
        // A discarded generated case consumes a seed but not a size.
        let second = property.next().expect("plan").expect("fresh");
        assert_eq!((second.seed, second.size), (23, 0));
        property.pass(second.kind);
        let last = property.next().expect("plan").expect("fresh");
        assert_eq!((last.seed, last.size), (24, 1));
        property.pass(last.kind);
        assert!(!property.more());
        assert!(property.next().expect("done").is_none());
    }

    #[test]
    fn shrinking_keeps_failing_recorded_streams_and_respects_budget() {
        let mut tried = Vec::new();
        let best = shrink(vec![10_i64], 100, Vec::as_slice, |stream| {
            tried.push(stream.to_vec());
            Ok(if stream.len() == 1 && stream[0] >= 3 {
                Trial::Candidate(stream.to_vec())
            } else {
                Trial::NotFailed
            })
        })
        .expect("shrink");
        assert_eq!(best, [3]);
        assert!(tried.len() <= 100);
        assert_eq!(tried.first(), Some(&Vec::new()));
        let mut attempts = 0;
        let unchanged = shrink(vec![9_i64; 4], 2, Vec::as_slice, |_| {
            attempts += 1;
            Ok(Trial::NotFailed)
        })
        .expect("bounded shrink");
        assert_eq!(attempts, 2);
        assert_eq!(unchanged, [9; 4]);
        assert_eq!(
            shrink(vec![5_i64], 0, Vec::as_slice, |_| panic!(
                "zero budget never runs"
            ))
            .expect("zero"),
            [5]
        );
    }

    #[test]
    fn seed_overflow_is_not_silently_wrapped() {
        let mut property = Property::new(2, 0, 0, i64::MAX, Vec::new());
        let first = property.next().expect("plan").expect("fresh");
        assert_eq!(first.seed, i64::MAX);
        property.pass(first.kind);
        assert!(property.next().is_err());
    }
}
