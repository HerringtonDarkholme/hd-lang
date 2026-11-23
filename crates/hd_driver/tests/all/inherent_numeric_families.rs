//! A sealed family contributes only its primitive heads to the member index.
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_sched::SerialOrder;
use std::fmt::Write as _;

fn comparisons(width: usize) -> Result<u64, String> {
    let mut source = String::from(
        "use std.num.Integer\nimpl[N < Integer] N:\n    fn family_probe(self) -> usize: 0\n",
    );
    for index in 0..width {
        writeln!(
            source,
            "struct Local{index}: pass\nimpl Local{index}:\n    fn family_probe(self) -> usize: 0",
        )
        .map_err(|error| error.to_string())?;
    }
    let mut sources = hd_driver::std_sources();
    sources.insert("rt_tests/stage_case.hd", &source);
    let store = hd_cache::MemoryStore::default();
    let host = Host {
        sources: &sources,
        store: &store,
        render_tir: &[],
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let (out, comparisons) = hd_resolve::work::count_member_pairs(|| {
        build_packages(
            &host,
            "std",
            &Packages {
                std_leaf: Some("rt_tests/stage_case.hd"),
                ..Packages::default()
            },
            &Goal::Analyze,
        )
    });
    if out.diags.has_errors() {
        return Err(out.render());
    }
    Ok(comparisons)
}

#[test]
fn unrelated_heads_do_not_compare_with_a_sealed_numeric_family() -> Result<(), String> {
    // Each measurement deliberately builds std cold in a fresh store,
    // keeping the fixed std contribution equal in all three runs.
    // The only source-sized work is one candidate-index visit per local type.
    let fixed = comparisons(0)?;
    for width in [100, 1_000] {
        let work = comparisons(width)?;
        println!(
            "width {width}: {work} visits, fixed {fixed}, delta {}",
            work - fixed
        );
        assert_eq!(
            work - fixed,
            width as u64,
            "width {width}: {work}, base {fixed}"
        );
    }
    Ok(())
}
