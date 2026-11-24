mod tests {
    use hd_base::SliceSort as _;
    use hd_wasm::testing::opt::live_set::{Set, Sets};

    fn keys(sets: &Sets, s: Set) -> Vec<u32> {
        let mut out = Vec::new();
        sets.members(s, &mut out);
        out
    }

    #[test]
    fn a_set_of_sorted_keys_is_the_set_their_inserts_make() {
        let mut sets = Sets::default();
        for ks in [
            &[][..],
            &[7],
            &[0, 1, 2, 3, 4],
            &[3, 9, 10, 64, 65, 1000, u32::MAX - 1],
        ] {
            let bulk = sets.of_sorted(ks);
            let one_by_one = ks.iter().fold(Set::EMPTY, |s, &k| sets.insert(s, k));
            assert_eq!(bulk, one_by_one, "{ks:?}");
            let mut got = keys(&sets, bulk);
            got.sort_stable();
            assert_eq!(got, ks.to_vec());
        }
        let mut fresh = Sets::default();
        let all: Vec<u32> = (100..1100).collect();
        fresh.of_sorted(&all);
        assert_eq!(fresh.made(), 2 * all.len() - 1, "each node once");
        assert_eq!(fresh.work(), fresh.made(), "each asked for once");
    }

    #[test]
    fn an_unconsed_arena_builds_the_same_keys_with_no_table() {
        let mut sets = Sets::unconsed();
        let a = sets.of_sorted(&[1, 4, 9, 30]);
        let b = sets.of_sorted(&[1, 4, 9, 30]);
        assert_ne!(a, b, "two nodes of one content");
        let c = sets.insert(a, 12);
        let d = sets.remove(b, 4);
        let meet = sets.intersection(c, d);
        let mut got = keys(&sets, meet);
        got.sort_stable();
        assert_eq!(got, [1, 9, 30]);
        let equal = sets.intersection(a, b);
        assert_eq!(sets.len(equal), 4, "a meet of equals keeps their size");
        assert!(sets.made.is_empty(), "no node was hashed");
    }

    #[test]
    fn sets_insert_remove_and_join_like_sorted_lists() {
        let mut sets = Sets::default();
        // Keys that differ in low, high and middle bits, the top bit too.
        let all = [5u32, 0, 7, 1 << 31, 6, 64, 65, u32::MAX - 1, 3];
        let mut s = Set::EMPTY;
        for &k in &all {
            s = sets.insert(s, k);
            assert!(sets.contains(s, k));
        }
        let mut sorted = all.to_vec();
        sorted.sort_stable();
        assert_eq!(keys(&sets, s), sorted);
        assert_eq!(sets.len(s), all.len());
        // Inserting a member or removing a stranger makes nothing.
        let made = sets.made();
        assert_eq!(sets.insert(s, 64), s);
        assert_eq!(sets.remove(s, 2), s);
        assert_eq!(sets.remove(s, 1 << 30), s);
        assert_eq!(sets.made(), made);
        let t = sets.remove(s, 64);
        assert!(!sets.contains(t, 64) && sets.contains(s, 64));
        assert_eq!(sets.len(t), all.len() - 1);
        let mut u = Set::EMPTY;
        for k in [2u32, 64, 1 << 20, 7] {
            u = sets.insert(u, k);
        }
        let both = sets.union(t, u);
        let mut want: Vec<u32> = sorted.iter().copied().chain([2, 1 << 20]).collect();
        want.sort_stable();
        assert_eq!(keys(&sets, both), want);
        assert_eq!(sets.len(both), want.len());
        let other = sets.union(u, t);
        assert_eq!(keys(&sets, other), want);
        // A union with a subset or with itself is the larger set as it is.
        assert_eq!(sets.union(s, t), s);
        assert_eq!(sets.union(t, s), s);
        assert_eq!(sets.union(both, Set::EMPTY), both);
        assert_eq!(sets.union(Set::EMPTY, both), both);
        // Emptying a set one key at a time.
        let mut e = both;
        for k in want {
            e = sets.remove(e, k);
        }
        assert_eq!(e, Set::EMPTY);
    }

    #[test]
    fn sets_of_one_content_are_one_node_and_a_difference_stops_at_its_limit() {
        let mut sets = Sets::default();
        let (mut up, mut down) = (Set::EMPTY, Set::EMPTY);
        for k in 0..100u32 {
            up = sets.insert(up, k);
            down = sets.insert(down, 99 - k);
        }
        assert_eq!(up, down);
        let fewer = sets.remove(up, 50);
        let mut out = Vec::new();
        sets.difference_upto(up, fewer, &mut out, 0);
        assert!(out.is_empty());
        sets.difference(up, fewer, &mut out);
        assert_eq!(out, [50]);
        // Against a lone key, the rest of the other set, up to the limit.
        let one = sets.insert(Set::EMPTY, 3);
        out.clear();
        sets.difference_upto(up, one, &mut out, 5);
        assert_eq!(out, [0, 1, 2, 4, 5]);
        out.clear();
        sets.difference_upto(fewer, Set::EMPTY, &mut out, 2);
        assert_eq!(out, [0, 1]);
        // The work of a difference of sets one key apart is a path.
        let work = sets.work();
        out.clear();
        sets.difference(fewer, up, &mut out);
        assert!(out.is_empty() && sets.work() - work <= 2 * 32);
    }

    #[test]
    fn an_intersection_keeps_the_shared_keys_and_shares_what_agrees() {
        let mut sets = Sets::default();
        let mut base = Set::EMPTY;
        for k in 0..64u32 {
            base = sets.insert(base, k);
        }
        let a = sets.remove(base, 5);
        let a = sets.insert(a, 1 << 20);
        let b = sets.remove(base, 40);
        let both = sets.intersection(a, b);
        let want: Vec<u32> = (0..64).filter(|&k| k != 5 && k != 40).collect();
        assert_eq!(keys(&sets, both), want);
        assert_eq!(sets.intersection(b, a), both);
        // With itself, a subset, a stranger, a lone key, or nothing.
        assert_eq!(sets.intersection(a, a), a);
        assert_eq!(sets.intersection(base, b), b);
        let far = sets.insert(Set::EMPTY, 1 << 30);
        assert_eq!(sets.intersection(base, far), Set::EMPTY);
        let one = sets.insert(Set::EMPTY, 7);
        assert_eq!(sets.intersection(base, one), one);
        assert_eq!(sets.intersection(one, base), one);
        assert_eq!(sets.intersection(far, one), Set::EMPTY);
        assert_eq!(sets.intersection(Set::EMPTY, base), Set::EMPTY);
        // Disjoint halves under one branch, and a set under one side.
        let mut low = Set::EMPTY;
        for k in 0..8u32 {
            low = sets.insert(low, k);
        }
        let mut high = Set::EMPTY;
        for k in 32..40u32 {
            high = sets.insert(high, k);
        }
        assert_eq!(sets.intersection(low, high), Set::EMPTY);
        assert_eq!(sets.intersection(base, high), high);
        assert_eq!(sets.intersection(high, base), high);
    }

    #[test]
    fn a_union_of_two_sets_from_one_base_makes_only_their_differing_paths() {
        for n in [1_000u32, 10_000] {
            let mut sets = Sets::default();
            let mut base = Set::EMPTY;
            for k in 0..n {
                base = sets.insert(base, k);
            }
            let a = sets.remove(base, 3);
            let a = sets.insert(a, n + 1);
            let b = sets.remove(base, n / 2);
            let made = sets.made();
            // The table holds an index and a control byte per slot, not
            // a copy of each node: at most 8 slots per 7 entries, doubled
            // by the last growth.
            let table = sets.made.allocation_size();
            assert!(
                table <= made * 5 * 8 * 2 / 7 + 64,
                "{n}: {table} table bytes for {made} nodes"
            );
            let both = sets.union(a, b);
            assert_eq!(sets.len(both), n as usize + 1);
            // About two paths of `log2 n` nodes, not `n`.
            assert!(
                sets.made() - made <= 64,
                "{n}: {} nodes",
                sets.made() - made
            );
        }
    }
}
