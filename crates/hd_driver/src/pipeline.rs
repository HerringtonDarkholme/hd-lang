//! The pipeline key of each (target, level) (`crates/hd_opt/DESIGN.md` (optimization pipelines) §3.3): the
//! hash of `hd_opt`'s passes and knobs. It hashes
//! content, never the target's name, so targets whose passes, knobs and
//! code-generation settings are equal share every cache entry. The table is validated once
//! per process (§3.2): `hd_opt::passes::validate` runs checks 2 and 3 on
//! each row, the total `match` in `hd_opt::passes::pipeline` is check 1,
//! and [`check_distinct`] is check 4.

use std::sync::OnceLock;

use hd_base::{EngineTarget, Hash128, OptLevel, StableHasher};
use hd_opt::passes::{Pipeline, pipeline, validate};

/// The key of a pipeline: what every program and
/// prelude key holds as its pipeline part.
pub fn content_key(passes: &Pipeline) -> Result<Hash128, String> {
    let mut h = StableHasher::new("pipeline-key");
    h.hash(validate(passes)?);
    Ok(h.finish())
}

/// One row of the table: its passes and key.
pub struct Row {
    pub at: (EngineTarget, OptLevel),
    pub passes: &'static Pipeline,
    pub key: Hash128,
}

/// Validator check 4: two rows have equal keys exactly when their content
/// is equal.
pub fn check_distinct(rows: &[Row]) -> Result<(), String> {
    for (i, a) in rows.iter().enumerate() {
        for b in &rows[i + 1..] {
            let same = a.passes.same_content(b.passes);
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
pub fn validate_table() -> Result<Vec<Row>, String> {
    validate_table_of(pipeline)
}

/// Every row of `table`, validated: [`validate_table`] on the real table,
/// or on a retuned one in tests.
pub fn validate_table_of(
    table: fn(EngineTarget, OptLevel) -> &'static Pipeline,
) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    for target in EngineTarget::ALL {
        for level in OptLevel::ALL {
            let passes = table(target, level);
            let key = content_key(passes).map_err(|e| format!("{target:?} {level:?}: {e}"))?;
            rows.push(Row {
                at: (target, level),
                passes,
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
pub fn key(target: EngineTarget, level: OptLevel) -> Hash128 {
    static TABLE: OnceLock<Vec<Row>> = OnceLock::new();
    let rows = TABLE
        .get_or_init(|| validate_table().unwrap_or_else(|e| panic!("invalid pipeline table: {e}")));
    rows.iter()
        .find(|r| r.at == (target, level))
        .map(|r| r.key)
        .expect("the table has every (target, level)")
}
