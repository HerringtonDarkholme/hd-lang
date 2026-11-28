use hd_check::testing::join_work;
use hd_check::work::count_join_work;

#[test]
fn join_work_counts_only_inside_a_measurement_and_restores_the_outer_one() {
    join_work(5);
    let ((), outer) = count_join_work(|| {
        join_work(1);
        let ((), inner) = count_join_work(|| join_work(2));
        assert_eq!(inner, 2);
        join_work(3);
    });
    assert_eq!(outer, 4);
}
