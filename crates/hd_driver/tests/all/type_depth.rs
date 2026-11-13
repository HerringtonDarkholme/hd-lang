//! `types.limit.type-size`: a body value whose type nests deeper than
//! `hd_base::limits::TYPE_DEPTH` is one `type-too-large`, on the first
//! such value, never a type walk that stops short and answers silently.
//! A written type cannot reach the limit under the parser's nesting
//! limit, so these grow one by inference: `x1 := [x0]`, ... adds a
//! `List` level per binding.

use hd_base::limits::TYPE_DEPTH;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use std::fmt::Write as _;

/// A body whose last binding's type is `levels` lists around `i32`.
fn nested(levels: u32) -> String {
    let mut s = String::from("fn run() -> usize:\n    x0 := 1\n");
    for i in 1..=levels {
        let _ = writeln!(s, "    x{i} := [x{}]", i - 1);
    }
    let _ = writeln!(s, "    x{levels}.len()");
    s
}

fn codes(text: &str) -> Vec<String> {
    let mut src = MemorySources::default();
    src.insert("main.hd", text);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Pool(2),
    };
    let out = build(&host, "app", &Goal::Analyze);
    out.diags
        .code
        .iter()
        .map(|c| c.as_str().to_owned())
        .collect()
}

#[test]
fn a_type_at_the_depth_limit_checks_and_one_level_more_is_too_large() {
    assert_eq!(codes(&nested(TYPE_DEPTH)), Vec::<String>::new());
    let past = codes(&nested(TYPE_DEPTH + 1));
    assert_eq!(past, ["type-too-large"]);
}
