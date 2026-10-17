use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct TestCase {
    pub test: u32,
    pub init: u32,
    pub kind: String,
    pub temp_dir: Option<PathBuf>,
    pub seed: Option<i64>,
    pub snapshot: Option<Snapshot>,
    pub regression: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub dir: PathBuf,
    pub shown: Option<String>,
    pub slug: String,
    pub update: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PanicFrame {
    pub file: String,
    pub line: u32,
    pub function: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PanicInfo {
    pub category: String,
    pub message: String,
    pub frames: Vec<PanicFrame>,
}

impl PanicInfo {
    /// The panic of a host's recorded report, the text after `panic: `:
    /// `CATEGORY: MESSAGE` and a newline, or `CATEGORY` alone. The
    /// category is the compiler's or the host's, never program text, so
    /// the first `: ` ends it; the message is the rest, whatever it holds.
    #[must_use]
    pub fn from_report(report: &str) -> Self {
        let report = report.strip_suffix('\n').unwrap_or(report);
        let (category, message) = report.split_once(": ").unwrap_or((report, ""));
        Self {
            category: category.to_owned(),
            message: message.to_owned(),
            frames: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct BodyResult {
    pub status: i64,
    pub trapped: bool,
    pub ended: bool,
    pub stdout: String,
    pub stderr: String,
    pub us: u64,
    pub panic: Option<PanicInfo>,
    /// A Wasm trap's frames, innermost first: function index and module
    /// byte offset (`crates/hd_wasm/DESIGN.md` §15.9).
    pub frames: Vec<(u32, u32)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaseRun {
    pub test: u32,
    pub status: i64,
    pub trapped: bool,
    pub stdout: String,
    pub stderr: String,
    pub us: u64,
    pub row: Option<(u32, u32)>,
    pub input: Option<String>,
    pub panic: Option<PanicInfo>,
    /// A Wasm trap's frames, innermost first: function index and module
    /// byte offset (`crates/hd_wasm/DESIGN.md` §15.9).
    pub frames: Vec<(u32, u32)>,
    /// The snapshots an update run recorded, in call order
    /// (`cli.test.snapshot.update`).
    pub snapshots: Vec<Recorded>,
}

/// A failing `snapshot` call that an update run recorded instead of
/// failing at: the text it compared and the Wasm frames of the call,
/// innermost first, as a trap's are.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recorded {
    pub text: String,
    pub frames: Vec<(u32, u32)>,
}

impl CaseRun {
    #[must_use]
    pub fn last(&self) -> bool {
        self.row.is_none_or(|(row, rows)| row + 1 >= rows)
    }

    pub(crate) fn from_body(test: u32, body: BodyResult) -> Self {
        Self {
            test,
            status: body.status,
            trapped: body.trapped,
            stdout: body.stdout,
            stderr: body.stderr,
            us: body.us,
            row: None,
            input: None,
            panic: body.panic,
            frames: body.frames,
            snapshots: Vec::new(),
        }
    }
}
