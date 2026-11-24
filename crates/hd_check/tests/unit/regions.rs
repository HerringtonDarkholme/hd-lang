use hd_check::testing::{Category, Constraints, Edge, Loan, Region, RegionTable, Solution};

fn loans(s: &Solution, r: Region) -> Vec<Loan> {
    s.loans(r).collect()
}

fn flow(c: &mut Constraints, a: Region, b: Region) {
    c.outlives(a, b, 0, Category::Flow);
}

#[test]
fn empty_graph() {
    let s = Constraints::default().solve(0);
    assert_eq!(s.components(), 0);
    let s = Constraints::default().solve(3);
    assert!((0..3).all(|r| s.loans(r).next().is_none() && !s.same(r, (r + 1) % 3)));
}

#[test]
fn region_table_keys_and_fresh() {
    let mut t = RegionTable::default();
    let a = t.push(2);
    let hub = t.fresh(1);
    let b = t.push(0);
    let c = t.push(3);
    assert_eq!(
        (t.slots(a), hub, t.slots(b), t.slots(c)),
        (0..2, 2..3, 3..3, 3..6)
    );
    assert_eq!((t.region(c, 2), t.len()), (5, 6));
}

#[test]
fn chain_reaches_the_end_only_forward() {
    let mut c = Constraints::default();
    for r in 0..4 {
        flow(&mut c, r, r + 1);
        c.seed(r, r);
    }
    let s = c.solve(5);
    assert_eq!(loans(&s, 4), [0, 1, 2, 3]);
    assert_eq!(loans(&s, 2), [0, 1, 2]);
    assert_eq!(loans(&s, 0), [0]);
}

#[test]
fn edges_given_backwards_still_solve_in_one_pass() {
    // Edges listed sink first, so Tarjan visits the end of the chain first.
    let mut c = Constraints::default();
    for r in (0..4).rev() {
        flow(&mut c, r, r + 1);
    }
    c.seed(0, 7);
    let s = c.solve(5);
    assert!((0..5).all(|r| loans(&s, r) == [7]));
}

#[test]
fn diamond_unions_both_arms() {
    // 0 -> 1 -> 3, 0 -> 2 -> 3: an `if` whose arms both flow to the result.
    let mut c = Constraints::default();
    for (a, b) in [(0, 1), (0, 2), (1, 3), (2, 3)] {
        flow(&mut c, a, b);
    }
    c.seed(0, 0);
    c.seed(1, 1);
    c.seed(2, 2);
    let s = c.solve(4);
    assert_eq!(loans(&s, 3), [0, 1, 2]);
    assert_eq!((loans(&s, 1), loans(&s, 2)), (vec![0, 1], vec![0, 2]));
}

#[test]
fn cycle_collapses_into_one_value() {
    // 0 -> 1 -> 2 -> 0 with a tail 2 -> 3 and an entry 4 -> 1.
    let mut c = Constraints::default();
    for (a, b) in [(0, 1), (1, 2), (2, 0), (2, 3), (4, 1)] {
        flow(&mut c, a, b);
    }
    c.seed(0, 0);
    c.seed(4, 4);
    c.seed(3, 3);
    let s = c.solve(5);
    assert!(s.same(0, 1) && s.same(1, 2) && !s.same(2, 3) && !s.same(4, 0));
    assert!((0..3).all(|r| loans(&s, r) == [0, 4]));
    assert_eq!((loans(&s, 3), loans(&s, 4)), (vec![0, 3, 4], vec![4]));
}

#[test]
fn equal_and_self_loops() {
    let mut c = Constraints::default();
    flow(&mut c, 0, 0);
    c.equal(1, 2, 0);
    c.outlives(2, 2, 0, Category::Result);
    c.seed(0, 1);
    c.seed(2, 2);
    let s = c.solve(3);
    assert!(s.same(1, 2) && !s.same(0, 1));
    assert_eq!((loans(&s, 0), loans(&s, 1)), (vec![1], vec![2]));
}

#[test]
fn loan_sets_cost_their_members() {
    let mut c = Constraints::default();
    flow(&mut c, 0, 2);
    flow(&mut c, 1, 2);
    for l in [0, 63, 64, 200] {
        c.seed(0, l);
    }
    c.seed(1, 130);
    c.seed(1, 100_000);
    let s = c.solve(3);
    assert_eq!(loans(&s, 2), [0, 63, 64, 130, 200, 100_000]);
    assert!(s.holds(2, 200) && !s.holds(2, 65));
    assert!(!s.holds(1, 1000) && s.holds(1, 100_000));
    // A far loan costs half a word, not the 1,562 words below it.
    assert_eq!((s.words(0), s.words(1), s.words(2)), (2, 1, 3));
}

#[test]
fn deep_chain_needs_no_recursion() {
    let n = 100_000;
    let mut c = Constraints::default();
    for r in 0..n - 1 {
        flow(&mut c, r, r + 1);
    }
    flow(&mut c, n - 1, 0);
    c.seed(n / 2, 5);
    let s = c.solve(n);
    assert!(s.same(0, n - 1) && loans(&s, 0) == [5]);
}

#[test]
fn checked_edge_reports_the_first_instruction() {
    // Loans 0 and 1 are placeholders of the universal region 5, loan 2 a
    // local. Local 2 reaches region 3 at instruction 4; a return at 9 and
    // a store into a caller's place at 7 both carry it on.
    let mut c = Constraints::default();
    c.seed(0, 0);
    c.seed(1, 2);
    c.seed(5, 1);
    c.outlives(0, 3, 2, Category::Flow);
    c.outlives(1, 3, 4, Category::Flow);
    c.outlives(3, 5, 9, Category::Result);
    c.outlives(3, 5, 7, Category::CallerPlace);
    c.outlives(0, 5, 1, Category::Result);
    let s = c.solve(6);
    let allowed = |e: &Edge, l: Loan| e.to == 5 && l < 2;
    let v = s.violations(&c, allowed);
    let got: Vec<_> = v
        .iter()
        .map(|v| (v.edge.inst, v.edge.cat, v.loan))
        .collect();
    assert_eq!(
        got,
        [(7, Category::CallerPlace, 2), (9, Category::Result, 2)]
    );
    // Checked edges propagate too, and a flow edge is never reported.
    assert_eq!(loans(&s, 5), [0, 1, 2]);
    assert!(s.violations(&c, |_, l| l != 1).is_empty());
}
