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
        // The snapshots an update run recorded: a line for each, its text
        // as a JSON string (`host/cases.ts`).
        let snapshots = match object.get("updates") {
            None => Vec::new(),
            Some(v) => v
                .as_str()
                .ok_or("test result updates must be a string")?
                .split('\n')
                .map(|line| {
                    let actual = serde_json::from_str(line)
                        .map_err(|_| "a test result update is malformed".to_owned())?;
                    Ok(crate::testing::Recorded { text: actual })
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
                .map(|r| serde_json::to_string(&r.text).expect("serialize string"))
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
