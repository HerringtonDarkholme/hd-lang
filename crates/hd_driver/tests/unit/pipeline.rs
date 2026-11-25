use hd_driver::testing::pipeline::{
    Row, check_distinct, content_key, key, validate_table, validate_table_of,
};

mod tests {
    use super::{Row, check_distinct, content_key, key, validate_table, validate_table_of};
    use hd_base::{EngineTarget, OptLevel};
    use hd_opt::passes::{Knob, OPTIMIZED, Pipeline, pipeline};

    /// `OPTIMIZED` with another `max_insts`.
    static TUNED: Pipeline = Pipeline {
        name: "optimized",
        tir: OPTIMIZED.tir,
        link: OPTIMIZED.link,
        knobs: &[
            Knob {
                pass: "closure_spec",
                name: "max_insts",
                value: 32,
            },
            Knob {
                pass: "closure_spec",
                name: "max_depth",
                value: 2,
            },
            Knob {
                pass: "sroa",
                name: "cold_spill",
                value: 1,
            },
        ],
    };

    /// The real table with `TUNED` as `V8`'s optimized row.
    fn v8_retuned(target: EngineTarget, level: OptLevel) -> &'static Pipeline {
        match (target, level) {
            (EngineTarget::V8, OptLevel::Optimized) => &TUNED,
            _ => pipeline(target, level),
        }
    }

    /// A knob change moves only the key of the row that holds it: every
    /// other row, `Browser` included, keeps its key.
    #[test]
    fn a_knob_changes_only_its_own_pipelines_key() {
        let changed = (EngineTarget::V8, OptLevel::Optimized);
        assert!(!TUNED.same_content(pipeline(changed.0, changed.1)));
        let before = validate_table().expect("valid table");
        let after = validate_table_of(v8_retuned).expect("valid retuned table");
        assert_eq!(before.len(), after.len());
        for (a, b) in before.iter().zip(&after) {
            assert_eq!(a.at, b.at);
            assert_eq!(a.key == b.key, a.at != changed, "{:?}", a.at);
        }
    }

    #[test]
    fn the_table_validates() {
        let rows = validate_table().expect("valid table");
        assert_eq!(rows.len(), EngineTarget::ALL.len() * OptLevel::ALL.len());
        assert_ne!(
            key(EngineTarget::V8, OptLevel::Dev),
            key(EngineTarget::V8, OptLevel::Optimized)
        );
    }

    /// `V8` and `Browser` share keys while their content is equal.
    /// `Wasmtime` shares their TIR passes, but its link inlines the count
    /// fast paths (#644) and its `sroa` spills nothing around a cold call,
    /// which split its key. A knob splits a key too.
    #[test]
    fn equal_content_shares_a_key_and_a_knob_splits_it() {
        for level in OptLevel::ALL {
            assert_eq!(
                key(EngineTarget::V8, level),
                key(EngineTarget::Browser, level),
                "{level:?}"
            );
            assert_ne!(
                key(EngineTarget::V8, level),
                key(EngineTarget::Wasmtime, level),
                "{level:?}"
            );
            let (v8, wasmtime) = (
                pipeline(EngineTarget::V8, level),
                pipeline(EngineTarget::Wasmtime, level),
            );
            assert_eq!(v8.tir, wasmtime.tir);
            assert!(!v8.link.contains(&"inline_counts"));
            assert!(wasmtime.link.contains(&"inline_counts"));
        }
        let tuned = content_key(&TUNED).expect("valid");
        assert_ne!(tuned, key(EngineTarget::V8, OptLevel::Optimized));
        assert_ne!(tuned, key(EngineTarget::Wasmtime, OptLevel::Optimized));
        let row = |at, passes: &'static Pipeline, key| Row { at, passes, key };
        let v8 = (EngineTarget::V8, OptLevel::Optimized);
        let wasmtime = (EngineTarget::Wasmtime, OptLevel::Optimized);
        let optimized = pipeline(v8.0, v8.1);
        // The passes tell the rows apart.
        let v8_key = content_key(optimized).expect("valid");
        assert!(
            check_distinct(&[row(v8, optimized, v8_key), row(wasmtime, &TUNED, tuned)]).is_ok()
        );
        // Check 4 refuses equal keys for different content, and different
        // keys for equal content.
        assert!(
            check_distinct(&[row(v8, optimized, v8_key), row(wasmtime, &TUNED, v8_key)]).is_err()
        );
        assert!(
            check_distinct(&[row(v8, optimized, v8_key), row(wasmtime, optimized, tuned)]).is_err()
        );
    }
}
