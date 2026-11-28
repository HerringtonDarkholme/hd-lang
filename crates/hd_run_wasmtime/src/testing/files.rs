//! Per-body snapshot and temporary-file resources, independent of Wasmtime.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::testing::{Snapshot, TestCase};

pub struct Files {
    base: Option<PathBuf>,
    snapshot: Option<Snapshot>,
    ordinal: u64,
    row: Option<u32>,
    snapshots: u64,
    made: Option<PathBuf>,
    pub(crate) deadline: Option<Instant>,
}

impl Files {
    #[must_use]
    pub fn new(case: &TestCase) -> Self {
        Self {
            base: case.temp_dir.clone(),
            snapshot: case.snapshot.clone(),
            ordinal: 0,
            row: None,
            snapshots: 0,
            made: None,
            deadline: None,
        }
    }

    pub fn begin(&mut self, ordinal: u64, row: Option<u32>) {
        self.cleanup();
        self.ordinal = ordinal;
        self.row = row;
        self.snapshots = 0;
        self.deadline = None;
    }

    pub(crate) fn set_row(&mut self, row: u32) {
        self.row = Some(row);
    }

    pub fn temp_dir(&mut self) -> Result<String, String> {
        if self.made.is_none() {
            let path = if let Some(base) = &self.base {
                let path = if self.ordinal == 0 {
                    base.clone()
                } else {
                    base.join(self.ordinal.to_string())
                };
                std::fs::create_dir_all(&path).map_err(|e| file_error(&path, &e))?;
                path
            } else {
                static NEXT: AtomicU64 = AtomicU64::new(0);
                loop {
                    let next = NEXT.fetch_add(1, Ordering::Relaxed);
                    let path =
                        std::env::temp_dir().join(format!("hd-case-{}-{next}", std::process::id()));
                    match std::fs::create_dir(&path) {
                        Ok(()) => break path,
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(e) => return Err(file_error(&path, &e)),
                    }
                }
            };
            self.made = Some(path);
        }
        Ok(self
            .made
            .as_ref()
            .expect("created temporary directory")
            .to_string_lossy()
            .into_owned())
    }

    pub fn snapshot_check(&mut self, actual: &str) -> Result<String, String> {
        self.snapshots += 1;
        let Some(snapshot) = &self.snapshot else {
            return Ok("no snapshot file: this run names no snapshot directory".into());
        };
        let row = self.row.map_or_else(String::new, |row| format!(".{row}"));
        let name = format!("{}{row}-{}.snap", snapshot.slug, self.snapshots);
        let file = snapshot.dir.join(&name);
        let shown = snapshot.shown.as_ref().map_or_else(
            || file.to_string_lossy().into_owned(),
            |dir| format!("{dir}/{name}"),
        );
        if file.exists() {
            let bytes = crate::fs::read_file_until(&file, self.deadline)
                .map_err(|e| file_error(&file, &e))?;
            let expected = String::from_utf8_lossy(&bytes);
            if expected == actual {
                return Ok(String::new());
            }
            if !snapshot.update {
                let expected = serde_json::to_string(&expected).map_err(|e| e.to_string())?;
                let actual = serde_json::to_string(actual).map_err(|e| e.to_string())?;
                return Ok(format!(
                    "the text differs from the snapshot file {shown}: expected {expected}, actual {actual}; run `hd test --update` to record it"
                ));
            }
        } else if !snapshot.update {
            return Ok(format!(
                "the snapshot file {shown} is missing; run `hd test --update` to record it"
            ));
        }
        std::fs::create_dir_all(&snapshot.dir).map_err(|e| file_error(&snapshot.dir, &e))?;
        crate::fs::write_file_until(&file, actual.as_bytes(), self.deadline)
            .map_err(|e| file_error(&file, &e))?;
        Ok(String::new())
    }

    pub(crate) fn cleanup(&mut self) {
        if let Some(path) = self.made.take() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        self.cleanup();
    }
}

pub(crate) fn file_error(path: &Path, error: &std::io::Error) -> String {
    format!("{}: {error}", path.display())
}

pub fn load_regression(
    path: Option<&Path>,
    deadline: Option<Instant>,
) -> Result<Vec<Vec<i64>>, String> {
    let Some(path) = path.filter(|path| path.exists()) else {
        return Ok(Vec::new());
    };
    let bytes = crate::fs::read_file_until(path, deadline).map_err(|e| file_error(path, &e))?;
    let text = String::from_utf8_lossy(&bytes);
    let stream = text
        .lines()
        .map(|line| line.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}'))
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.parse::<i64>()
                .map_err(|e| format!("{}: invalid regression draw {line:?}: {e}", path.display()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(vec![stream])
}

pub fn save_regression(path: &Path, stream: &[i64]) -> Result<(), String> {
    use std::fmt::Write as _;
    use std::io::Write as _;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| file_error(parent, &e))?;
    let mut text = String::new();
    for draw in stream {
        writeln!(text, "{draw}").expect("write to string");
    }
    // Persistence is runner bookkeeping after guest execution, not a new
    // guest body under its expired deadline. Write a regular file and rename
    // it: never open an existing FIFO, or truncate the last saved stream on
    // an error. Ordinary blocked filesystem syscalls retain the OS limitation.
    let (temporary, mut file) = loop {
        let next = NEXT.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".hd-regression-{}-{next}.tmp", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (RegressionTemp(temporary), file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(file_error(path, &error)),
        }
    };
    file.write_all(text.as_bytes())
        .map_err(|error| file_error(path, &error))?;
    drop(file);
    std::fs::rename(&temporary.0, path).map_err(|error| file_error(path, &error))
}

struct RegressionTemp(PathBuf);

impl Drop for RegressionTemp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
