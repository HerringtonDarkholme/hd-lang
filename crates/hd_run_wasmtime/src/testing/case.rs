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
        }
    }
}
