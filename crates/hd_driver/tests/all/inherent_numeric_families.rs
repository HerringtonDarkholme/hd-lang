//! A sealed family contributes only its primitive heads to the member index.
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::SourceSet as _;
use hd_sched::SerialOrder;

fn comparisons(width: usize, leading_copy: bool) -> Result<u64, String> {
    let family = include_str!("programs/inherent_numeric_families/family.hd");
    let family = if leading_copy {
        family.to_owned()
    } else {
        family.replace("Copy & Integer", "Integer & Copy")
    };
    let mut source = String::new();
    for index in 0..width {
        source.push('\n');
        source.push_str(
            &include_str!("programs/inherent_numeric_families/unrelated.hd")
                .replace("LocalReceiver", &format!("Local{index}")),
        );
    }
    let mut sources = hd_driver::std_sources();
    let numeric = sources
        .read("num.hd")
        .ok_or("embedded std.num is missing")?;
    let numeric = std::str::from_utf8(&numeric).map_err(|error| error.to_string())?;
    sources.insert("num.hd", &format!("{family}\n{numeric}"));
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
    for leading_copy in [false, true] {
        let fixed = comparisons(0, leading_copy)?;
        for width in [100, 1_000] {
            let work = comparisons(width, leading_copy)?;
            println!(
                "leading Copy {leading_copy}, width {width}: {work} visits, fixed {fixed}, delta {}",
                work - fixed
            );
            assert_eq!(
                work - fixed,
                width as u64,
                "leading Copy {leading_copy}, width {width}: {work}, base {fixed}"
            );
        }
    }
    Ok(())
}
