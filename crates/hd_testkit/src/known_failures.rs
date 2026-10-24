//! The root `KNOWN_FAILURES.tsv`: the conformance cases the Rust compiler
//! may fail. Every indexed case of the driver tier
//! (`hd_driver/tests/conformance.rs`) and the CLI tier
//! (`hd_cli/tests/cli_conformance.rs`) must pass unless a row lists it,
//! and a listed case that passes is an error too, so the list stays exact.
//!
//! The file is tab-separated with the columns `path`, `issue`, `engine`,
//! `reason`.
//! `path` is the case path from `spec/conformance/cases.tsv`, or `cli/NAME`
//! for a CLI case. `issue` is the GitHub issue `#N` that tracks the fix.
//! `engine` is empty for every engine, or one engine name (`node`,
//! `wasmtime`). `reason` says briefly why the case fails.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

/// The header line of `KNOWN_FAILURES.tsv`.
pub const HEADER: &str = "path\tissue\tengine\treason";

/// The engines a row may name.
pub const ENGINES: [&str; 2] = ["node", "wasmtime"];

/// One row of `KNOWN_FAILURES.tsv`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownFailure {
    pub path: String,
    pub issue: String,
    /// `None` for every engine.
    pub engine: Option<String>,
    pub reason: String,
}

impl KnownFailure {
    /// Whether the row applies to a run on `engine`.
    #[must_use]
    pub fn applies_to(&self, engine: &str) -> bool {
        self.engine.as_deref().is_none_or(|own| own == engine)
    }
}

/// Parses the text of `KNOWN_FAILURES.tsv`.
///
/// # Errors
/// On a wrong header, a row without four columns, an issue that is not
/// `#N`, an unknown engine, an empty path or reason, or a repeated path
/// and engine.
pub fn parse(text: &str) -> Result<Vec<KnownFailure>, String> {
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err(format!(
            "KNOWN_FAILURES.tsv must start with the header `{HEADER}`"
        ));
    }
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        let [path, issue, engine, reason] = fields[..] else {
            return Err(format!("KNOWN_FAILURES.tsv row needs four columns: {line}"));
        };
        let number = issue.strip_prefix('#').unwrap_or_default();
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(format!("KNOWN_FAILURES.tsv issue must be #N: {line}"));
        }
        if !engine.is_empty() && !ENGINES.contains(&engine) {
            return Err(format!(
                "KNOWN_FAILURES.tsv engine must be empty or one of {ENGINES:?}: {line}"
            ));
        }
        if path.is_empty() || reason.is_empty() {
            return Err(format!(
                "KNOWN_FAILURES.tsv row needs a path and a reason: {line}"
            ));
        }
        if !seen.insert((path, engine)) {
            return Err(format!("KNOWN_FAILURES.tsv repeats a row: {line}"));
        }
        rows.push(KnownFailure {
            path: path.to_owned(),
            issue: issue.to_owned(),
            engine: (!engine.is_empty()).then(|| engine.to_owned()),
            reason: reason.to_owned(),
        });
    }
    Ok(rows)
}

/// Reads and parses `KNOWN_FAILURES.tsv` at the repository root.
///
/// # Panics
/// If the file cannot be read or does not parse.
#[must_use]
pub fn load(root: &Path) -> Vec<KnownFailure> {
    let text =
        std::fs::read_to_string(root.join("KNOWN_FAILURES.tsv")).expect("KNOWN_FAILURES.tsv");
    parse(&text).unwrap_or_else(|error| panic!("{error}"))
}

/// One case's result: its path and, unless it passed, its bucket (the
/// failure or unsupported reason).
pub struct Outcome<'a> {
    pub path: &'a str,
    pub bucket: Option<String>,
}

/// Applies the rule to one tier's full run on `engine`. `pipeline` names
/// the optimization pipeline of every build when the tier fixes one, for
/// the summary line. `in_tier` picks the rows that belong to the tier. Returns the summary to print and the
/// errors: a case that does not pass and has no row, a listed case that
/// passes, and a row that names no case of the tier.
#[must_use]
pub fn judge(
    tier: &str,
    engine: &str,
    pipeline: Option<&str>,
    known: &[KnownFailure],
    in_tier: impl Fn(&str) -> bool,
    outcomes: &[Outcome<'_>],
) -> (String, Vec<String>) {
    let listed: BTreeMap<&str, &KnownFailure> = known
        .iter()
        .filter(|row| in_tier(&row.path) && row.applies_to(engine))
        .map(|row| (row.path.as_str(), row))
        .collect();
    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();
    let (mut pass, mut known_count, mut fail, mut unsupported) = (0, 0, 0, 0);
    let mut buckets: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for outcome in outcomes {
        let row = listed.get(outcome.path);
        if row.is_some() {
            seen.insert(outcome.path);
        }
        match (&outcome.bucket, row) {
            (None, None) => pass += 1,
            (None, Some(_)) => {
                pass += 1;
                errors.push(format!(
                    "{} passes now: remove its row from KNOWN_FAILURES.tsv",
                    outcome.path
                ));
            }
            (Some(bucket), row) => {
                let mut entry = outcome.path.to_owned();
                if let Some(row) = row {
                    known_count += 1;
                    write!(entry, " (known, {})", row.issue).expect("write summary");
                } else {
                    if bucket.starts_with("unsupported") {
                        unsupported += 1;
                    } else {
                        fail += 1;
                    }
                    errors.push(format!(
                        "{}: {bucket}: fix it or list it in KNOWN_FAILURES.tsv with its issue",
                        outcome.path
                    ));
                }
                buckets.entry(bucket).or_default().push(entry);
            }
        }
    }
    for path in listed.keys() {
        if !seen.contains(path) {
            errors.push(format!(
                "{path}: no such {tier} case: remove its row from KNOWN_FAILURES.tsv"
            ));
        }
    }
    let on = pipeline.map_or_else(
        || engine.to_owned(),
        |pipeline| format!("{engine}, {pipeline} pipeline"),
    );
    let mut summary = format!(
        "\n{tier} conformance on {on}: pass {pass}, known {known_count}, fail {fail}, unsupported {unsupported}, total {}\n",
        outcomes.len()
    );
    for (bucket, paths) in buckets {
        writeln!(summary, "  {bucket} ({}):", paths.len()).expect("write summary");
        for path in paths {
            writeln!(summary, "    {path}").expect("write summary");
        }
    }
    (summary, errors)
}

/// Judges only the cases that a filtered run selected. Known rows outside
/// that selection are not missing cases; selected rows keep the full run's
/// exception and unexpected-pass rules.
#[must_use]
pub fn judge_selection(
    tier: &str,
    engine: &str,
    pipeline: Option<&str>,
    known: &[KnownFailure],
    outcomes: &[Outcome<'_>],
) -> (String, Vec<String>) {
    let selected: BTreeSet<&str> = outcomes.iter().map(|outcome| outcome.path).collect();
    judge(
        tier,
        engine,
        pipeline,
        known,
        |path| selected.contains(path),
        outcomes,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<KnownFailure> {
        parse("path\tissue\tengine\treason\na.hd\t#9\t\tunsupported\nb.hd\t#2\twasmtime\ttrap\n")
            .expect("parses")
    }

    #[test]
    fn rows_parse_and_bad_rows_are_refused() {
        let rows = rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].engine.as_deref(), Some("wasmtime"));
        assert!(rows[0].applies_to("node") && !rows[1].applies_to("node"));
        assert!(parse("path\texpected\n").is_err());
        assert!(parse(&format!("{HEADER}\na.hd\t9\t\twhy\n")).is_err());
        assert!(parse(&format!("{HEADER}\na.hd\t#9\tv8\twhy\n")).is_err());
        assert!(parse(&format!("{HEADER}\na.hd\t#9\t\t\n")).is_err());
        assert!(parse(&format!("{HEADER}\na.hd\t#9\t\twhy\na.hd\t#9\t\twhy\n")).is_err());
    }

    #[test]
    fn only_unlisted_failures_listed_passes_and_missing_cases_are_errors() {
        let rows = rows();
        let outcomes = [
            Outcome {
                path: "a.hd",
                bucket: Some("unsupported:Link".to_owned()),
            },
            Outcome {
                path: "b.hd",
                bucket: None,
            },
        ];
        let (summary, errors) = judge("driver", "node", None, &rows, |_| true, &outcomes);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(summary.contains(
            "driver conformance on node: pass 1, known 1, fail 0, unsupported 0, total 2"
        ));
        let (summary, _) = judge(
            "driver",
            "node",
            Some("optimized"),
            &rows,
            |_| true,
            &outcomes,
        );
        assert!(summary.contains("driver conformance on node, optimized pipeline: pass 1"));
        let (_, errors) = judge("driver", "wasmtime", None, &rows, |_| true, &outcomes);
        assert_eq!(
            errors,
            ["b.hd passes now: remove its row from KNOWN_FAILURES.tsv"]
        );
        let (_, errors) = judge("driver", "node", None, &rows, |_| true, &outcomes[1..]);
        assert_eq!(
            errors,
            ["a.hd: no such driver case: remove its row from KNOWN_FAILURES.tsv"]
        );
        let (_, errors) = judge("driver", "node", None, &[], |_| true, &outcomes);
        assert_eq!(
            errors,
            ["a.hd: unsupported:Link: fix it or list it in KNOWN_FAILURES.tsv with its issue"]
        );
    }

    #[test]
    fn selected_runs_reject_failures_and_unsupported_cases_but_accept_passes() {
        for bucket in [None, Some("fail:mismatch"), Some("unsupported:runtime")] {
            let outcomes = [Outcome {
                path: "selected.hd",
                bucket: bucket.map(str::to_owned),
            }];
            let (_, errors) = judge_selection("driver", "node", None, &[], &outcomes);
            assert_eq!(
                errors.is_empty(),
                bucket.is_none(),
                "{bucket:?}: {errors:?}"
            );
        }
    }

    #[test]
    fn selected_runs_apply_only_the_selected_known_failure_rows() {
        let known = rows();
        let mut outcomes = [Outcome {
            path: "b.hd",
            bucket: Some("fail:trap".to_owned()),
        }];
        let (summary, errors) = judge_selection("driver", "wasmtime", None, &known, &outcomes);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(summary.contains("pass 0, known 1, fail 0, unsupported 0, total 1"));
        let (_, errors) = judge_selection("driver", "node", None, &known, &outcomes);
        assert_eq!(
            errors.len(),
            1,
            "an engine-specific row does not hide another engine's failure"
        );
        outcomes[0].bucket = None;
        let (_, errors) = judge_selection("driver", "wasmtime", None, &known, &outcomes);
        assert_eq!(
            errors,
            ["b.hd passes now: remove its row from KNOWN_FAILURES.tsv"]
        );
        let (_, errors) = judge_selection("driver", "node", None, &known, &outcomes);
        assert!(
            errors.is_empty(),
            "unselected known rows do not make missing-case errors"
        );
    }
}
