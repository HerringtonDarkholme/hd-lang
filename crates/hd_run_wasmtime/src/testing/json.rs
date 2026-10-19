//! The existing Node runner's JSON-line protocol, without time in parity.

use serde_json::Value;

use crate::testing::CaseRun;

impl CaseRun {
    pub fn from_json_line(line: &str) -> Result<Self, String> {
        let value: Value =
            serde_json::from_str(line).map_err(|e| format!("test result JSON: {e}"))?;
        let object = value
            .as_object()
            .ok_or("test result must be a JSON object")?;
        let integer = |name: &str| {
            object
                .get(name)
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("test result {name} must be a nonnegative integer"))
        };
        let small = |name: &str| {
            integer(name).and_then(|n| {
                u32::try_from(n).map_err(|_| format!("test result {name} exceeds u32"))
            })
        };
        let string = |name: &str| {
            object
                .get(name)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("test result {name} must be a string"))
        };
        let test = small("test")?;
        let row = if object.contains_key("row") || object.contains_key("rows") {
            Some((small("row")?, small("rows")?))
        } else {
            None
        };
        if let Some((row, rows)) = row {
            if (rows == 0 && row != 0) || (rows != 0 && row >= rows) {
                return Err("test result row is outside its table".into());
            }
            if rows == 0 {
                return Ok(Self {
                    test,
                    row: Some((0, 0)),
                    ..Self::default()
                });
            }
        }
        let status = object
            .get("status")
            .and_then(Value::as_i64)
            .ok_or("test result status must be an integer")?;
        let trapped = object
            .get("trapped")
            .and_then(Value::as_bool)
            .ok_or("test result trapped must be a boolean")?;
        let input = if object.contains_key("input") {
            Some(string("input")?)
        } else {
            None
        };
        // A panicked run's `category` and `message`, which the runner reads
        // instead of the report on standard error.
        let panic = if object.contains_key("category") {
            Some(crate::testing::PanicInfo {
                category: string("category")?,
                message: string("message")?,
                frames: Vec::new(),
            })
        } else {
            None
        };
        let frames = match object.get("frames") {
            None => Vec::new(),
            Some(v) => v
                .as_array()
                .ok_or("test result frames must be an array")?
                .iter()
                .map(|pair| {
                    let n = |i: usize| {
                        pair.get(i)
                            .and_then(Value::as_u64)
                            .and_then(|n| u32::try_from(n).ok())
                    };
                    n(0).zip(n(1))
                        .ok_or("a test result frame must be two u32 numbers")
                })
                .collect::<Result<_, _>>()?,
        };
        // The snapshots an update run recorded: a line for each, the call's
        // frames as `function:offset` pairs, a tab, and the text as a JSON
        // string (`host/cases.mjs`).
        let snapshots = match object.get("updates") {
            None => Vec::new(),
            Some(v) => v
                .as_str()
                .ok_or("test result updates must be a string")?
                .split('\n')
                .map(|line| {
                    let bad = || "a test result update is malformed".to_owned();
                    let (frames, quoted) = line.split_once('\t').ok_or_else(bad)?;
                    let frames = frames
                        .split(',')
                        .filter(|f| !f.is_empty())
                        .map(|f| {
                            let (func, at) = f.split_once(':')?;
                            Some((func.parse().ok()?, at.parse().ok()?))
                        })
                        .collect::<Option<Vec<_>>>()
                        .ok_or_else(bad)?;
                    let actual = serde_json::from_str(quoted).map_err(|_| bad())?;
                    Ok(crate::testing::Recorded {
                        text: actual,
                        frames,
                    })
                })
                .collect::<Result<_, String>>()?,
        };
        Ok(Self {
            test,
            status,
            trapped,
            stdout: string("stdout")?,
            stderr: string("stderr")?,
            us: integer("us")?,
            row,
            failure: if object.contains_key("failure") {
                Some(string("failure")?)
            } else {
                None
            },
            input,
            panic,
            frames,
            snapshots,
        })
    }

    #[must_use]
    pub fn to_json_line(&self) -> String {
        use std::fmt::Write as _;
        if self.row == Some((0, 0)) {
            return format!("{{\"test\":{},\"row\":0,\"rows\":0}}", self.test);
        }
        let stdout = serde_json::to_string(&self.stdout).expect("serialize string");
        let stderr = serde_json::to_string(&self.stderr).expect("serialize string");
        let mut line = format!(
            "{{\"test\":{},\"status\":{},\"trapped\":{},\"stdout\":{stdout},\"stderr\":{stderr},\"us\":{}",
            self.test, self.status, self.trapped, self.us
        );
        if let Some((row, rows)) = self.row {
            write!(line, ",\"row\":{row},\"rows\":{rows}").expect("write string");
        }
        if let Some(panic) = &self.panic {
            write!(
                line,
                ",\"category\":{},\"message\":{}",
                serde_json::to_string(&panic.category).expect("serialize string"),
                serde_json::to_string(&panic.message).expect("serialize string")
            )
            .expect("write string");
        }
        if let Some(failure) = &self.failure {
            write!(
                line,
                ",\"failure\":{}",
                serde_json::to_string(failure).expect("serialize string")
            )
            .expect("write result");
        }
        if let Some(input) = &self.input {
            write!(
                line,
                ",\"input\":{}",
                serde_json::to_string(input).expect("serialize string")
            )
            .expect("write string");
        }
        if !self.frames.is_empty() {
            let frames: Vec<String> = self
                .frames
                .iter()
                .map(|(f, off)| format!("[{f},{off}]"))
                .collect();
            write!(line, ",\"frames\":[{}]", frames.join(",")).expect("write string");
        }
        if !self.snapshots.is_empty() {
            let records: Vec<String> = self
                .snapshots
                .iter()
                .map(|r| {
                    let frames: Vec<String> = r
                        .frames
                        .iter()
                        .map(|(f, off)| format!("{f}:{off}"))
                        .collect();
                    let text = serde_json::to_string(&r.text).expect("serialize string");
                    format!("{}\t{text}", frames.join(","))
                })
                .collect();
            write!(
                line,
                ",\"updates\":{}",
                serde_json::to_string(&records.join("\n")).expect("serialize string")
            )
            .expect("write string");
        }
        line.push('}');
        line
    }

    /// All JSON-observable fields except measured microseconds and raw
    /// frames, whose depth differs by engine on an exhausted stack.
    #[must_use]
    pub fn equivalent(&self, other: &Self) -> bool {
        let panic = |run: &Self| {
            run.panic
                .as_ref()
                .map(|p| (p.category.clone(), p.message.clone()))
        };
        self.failure == other.failure
            && panic(self) == panic(other)
            && self.test == other.test
            && self.status == other.status
            && self.trapped == other.trapped
            && self.stdout == other.stdout
            && self.stderr == other.stderr
            && self.row == other.row
            && self.input == other.input
            && self
                .snapshots
                .iter()
                .map(|r| &r.text)
                .eq(other.snapshots.iter().map(|r| &r.text))
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::CaseRun;

    #[test]
    fn result_protocol_round_trips_and_only_time_is_ignored() {
        let result = CaseRun {
            test: 7,
            status: 3,
            trapped: true,
            stdout: "hi\n\"\\\u{0}é".into(),
            stderr: "panic".into(),
            us: 123,
            row: Some((1, 2)),
            input: Some("x".into()),
            failure: Some("state left by an earlier case".into()),
            panic: Some(crate::testing::PanicInfo {
                category: "explicit-panic".into(),
                message: "a\npanic: integer-overflow: \"b\"".into(),
                frames: Vec::new(),
            }),
            frames: vec![(4, 120), (9, 77)],
            snapshots: vec![crate::testing::Recorded {
                text: "a\tb\n\"".into(),
                frames: vec![(3, 40), (8, 9)],
            }],
        };
        assert_eq!(
            CaseRun::from_json_line(&result.to_json_line()).expect("round trip"),
            result
        );
        let mut different = result.clone();
        different.us += 1;
        different.frames.clear();
        assert!(result.equivalent(&different));
        different.input = None;
        assert!(!result.equivalent(&different));
        let mut different = result.clone();
        different.panic = None;
        assert!(!result.equivalent(&different));
        let empty = CaseRun {
            test: 2,
            row: Some((0, 0)),
            ..CaseRun::default()
        };
        assert_eq!(
            CaseRun::from_json_line(&empty.to_json_line()).expect("empty table"),
            empty
        );
        assert_eq!(empty.to_json_line(), "{\"test\":2,\"row\":0,\"rows\":0}");
    }

    #[test]
    fn malformed_result_lines_are_errors_not_fallbacks() {
        for line in [
            "not JSON",
            "[]",
            "{}",
            "{\"test\":-1}",
            "{\"test\":0,\"row\":0}",
            "{\"test\":0,\"row\":2,\"rows\":2}",
            "{\"test\":0,\"status\":0,\"trapped\":false,\"stdout\":0,\"stderr\":\"\",\"us\":0}",
        ] {
            assert!(CaseRun::from_json_line(line).is_err(), "{line}");
        }
    }
}
