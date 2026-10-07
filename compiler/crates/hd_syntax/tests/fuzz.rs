use std::panic::{AssertUnwindSafe, catch_unwind};

use hd_syntax::parse;

#[test]
fn fixed_budget_parser_fuzz_has_no_panic() {
    let mut state = 0x4844_5f53_594e_5441_u64;
    for case in 0..4_096 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let len = usize::from(state.to_le_bytes()[0]);
        let mut source = Vec::with_capacity(len);
        for _ in 0..len {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            source.push(state.to_le_bytes()[0]);
        }
        let result = catch_unwind(AssertUnwindSafe(|| parse(&source)));
        assert!(
            result.is_ok(),
            "parser panicked for fuzz case {case}: {source:?}"
        );
    }
}
