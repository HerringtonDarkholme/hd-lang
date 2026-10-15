//! The pipeline key of each (target, level) (`crates/hd_opt/DESIGN.md` (optimization pipelines) §3.3): the
//! hash of `hd_opt`'s pass hash and `hd_wasm`'s lowering hash. It hashes
//! content, never the target's name, so targets whose passes, knobs and
//! lowering are equal share every cache entry. The table is validated once
//! per process (§3.2): `hd_opt::passes::validate` runs checks 2 and 3 on
//! each row, the total `match` in `hd_opt::passes::pipeline` is check 1,
//! and [`check_distinct`] is check 4.

use std::sync::OnceLock;

use hd_base::{EngineTarget, Hash128, OptLevel, StableHasher};
use hd_opt::passes::{Pipeline, pipeline, validate};
use hd_wasm::Lowering;

/// The key of a pipeline with `lowering`: what every code, program and
/// prelude key holds as its pipeline part.
pub(crate) fn content_key(passes: &Pipeline, lowering: Lowering) -> Result<Hash128, String> {
    let mut h = StableHasher::new("pipeline-key");
    h.hash(validate(passes)?);
    h.hash(lowering.hash());
    Ok(h.finish())
}

/// One row of the table: its passes, lowering and key.
struct Row {
    at: (EngineTarget, OptLevel),
    passes: &'static Pipeline,
    lowering: Lowering,
    key: Hash128,
}

/// Validator check 4: two rows have equal keys exactly when their content
/// is equal.
fn check_distinct(rows: &[Row]) -> Result<(), String> {
    for (i, a) in rows.iter().enumerate() {
        for b in &rows[i + 1..] {
            let same = a.passes.same_content(b.passes) && a.lowering == b.lowering;
            if same != (a.key == b.key) {
                return Err(format!(
                    "{:?} and {:?}: content {} but keys {}",
                    a.at,
                    b.at,
                    if same { "equal" } else { "differs" },
                    if a.key == b.key { "equal" } else { "differ" },
                ));
            }
        }
    }
    Ok(())
}

/// Every row of the table, validated.
fn validate_table() -> Result<Vec<Row>, String> {
    validate_table_of(pipeline)
}

/// Every row of `table`, validated: [`validate_table`] on the real table,
/// or on a retuned one in tests.
fn validate_table_of(
    table: fn(EngineTarget, OptLevel) -> &'static Pipeline,
) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    for target in EngineTarget::ALL {
        for level in OptLevel::ALL {
            let passes = table(target, level);
            let lowering = Lowering::of(target);
            let key =
                content_key(passes, lowering).map_err(|e| format!("{target:?} {level:?}: {e}"))?;
            rows.push(Row {
                at: (target, level),
                passes,
                lowering,
                key,
            });
        }
    }
    check_distinct(&rows)?;
    Ok(rows)
}

/// The pipeline key of (`target`, `level`).
///
/// # Panics
///
/// If the table fails validation, a compiler bug the unit tests rule out.
pub(crate) fn key(target: EngineTarget, level: OptLevel) -> Hash128 {
    static TABLE: OnceLock<Vec<Row>> = OnceLock::new();
    let rows = TABLE
        .get_or_init(|| validate_table().unwrap_or_else(|e| panic!("invalid pipeline table: {e}")));
    rows.iter()
        .find(|r| r.at == (target, level))
        .map(|r| r.key)
        .expect("the table has every (target, level)")
}

#[cfg(test)]
mod tests {
    use super::{Row, check_distinct, content_key, key, validate_table, validate_table_of};
    use hd_base::{EngineTarget, OptLevel};
    use hd_opt::passes::{Knob, OPTIMIZED, Pipeline, pipeline};
    use hd_wasm::Lowering;

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

    /// `V8` and `Browser` share keys while their content is equal; `V8`
    /// and `Wasmtime` differ in lowering (closure code), so their keys
    /// differ; a knob splits a key too.
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
            // Equal passes, so the lowering alone splits the key.
            assert!(
                pipeline(EngineTarget::V8, level)
                    .same_content(pipeline(EngineTarget::Wasmtime, level))
            );
        }
        let lowering = Lowering::of(EngineTarget::Wasmtime);
        let tuned = content_key(&TUNED, lowering).expect("valid");
        assert_ne!(tuned, key(EngineTarget::V8, OptLevel::Optimized));
        assert_ne!(tuned, key(EngineTarget::Wasmtime, OptLevel::Optimized));
        let row = |at, passes: &'static Pipeline, key| Row {
            at,
            passes,
            lowering,
            key,
        };
        let v8 = (EngineTarget::V8, OptLevel::Optimized);
        let wasmtime = (EngineTarget::Wasmtime, OptLevel::Optimized);
        let optimized = pipeline(v8.0, v8.1);
        // Both rows carry one lowering, so only the passes tell them apart.
        let v8_key = content_key(optimized, lowering).expect("valid");
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
