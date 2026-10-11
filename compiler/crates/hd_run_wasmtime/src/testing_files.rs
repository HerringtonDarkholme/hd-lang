//! Per-body snapshot and temporary-file resources, independent of Wasmtime.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::testing::{Snapshot, TestCase};

pub(crate) struct Files {
    base: Option<PathBuf>,
    snapshot: Option<Snapshot>,
    ordinal: u64,
    row: Option<u32>,
    snapshots: u64,
    made: Option<PathBuf>,
    pub(crate) deadline: Option<Instant>,
}

impl Files {
    pub(crate) fn new(case: &TestCase) -> Self {
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

    pub(crate) fn begin(&mut self, ordinal: u64, row: Option<u32>) {
        self.cleanup();
        self.ordinal = ordinal;
        self.row = row;
        self.snapshots = 0;
        self.deadline = None;
    }

    pub(crate) fn set_row(&mut self, row: u32) {
        self.row = Some(row);
    }

    pub(crate) fn temp_dir(&mut self) -> Result<String, String> {
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

    pub(crate) fn snapshot_check(&mut self, actual: &str) -> Result<String, String> {
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

pub(crate) fn load_regression(
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

pub(crate) fn save_regression(path: &Path, stream: &[i64]) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::{Files, load_regression, save_regression};
    use crate::testing::{Snapshot, TestCase};
    use std::path::PathBuf;

    #[test]
    fn lazy_directories_are_unique_per_body_and_removed_on_cleanup() {
        let mut files = Files::new(&TestCase::default());
        let first = PathBuf::from(files.temp_dir().expect("temp directory"));
        assert!(first.is_absolute());
        assert!(first.is_dir());
        assert_eq!(
            PathBuf::from(files.temp_dir().expect("same directory")),
            first
        );
        files.begin(1, None);
        assert!(!first.exists());
        let second = PathBuf::from(files.temp_dir().expect("new body"));
        assert_ne!(first, second);
        drop(files);
        assert!(!second.exists());
    }

    #[test]
    fn snapshot_names_messages_updates_and_lossy_utf8_match_node() {
        let mut owner = Files::new(&TestCase::default());
        let root = PathBuf::from(owner.temp_dir().expect("root"));
        let snapshot = Snapshot {
            dir: root.join("snapshots"),
            shown: Some("__snapshots__/pkg".into()),
            slug: "a-test".into(),
            update: false,
        };
        let mut files = Files::new(&TestCase {
            snapshot: Some(snapshot.clone()),
            ..TestCase::default()
        });
        assert_eq!(
            files.snapshot_check("hi").expect("missing"),
            "the snapshot file __snapshots__/pkg/a-test-1.snap is missing; run `hd test --update` to record it"
        );
        let mut update = snapshot.clone();
        update.update = true;
        let mut files = Files::new(&TestCase {
            snapshot: Some(update),
            ..TestCase::default()
        });
        files.begin(0, Some(2));
        assert_eq!(files.snapshot_check("hi\n").expect("record"), "");
        assert_eq!(
            std::fs::read(root.join("snapshots/a-test.2-1.snap")).expect("saved"),
            b"hi\n"
        );
        let mut files = Files::new(&TestCase {
            snapshot: Some(snapshot),
            ..TestCase::default()
        });
        files.begin(0, Some(2));
        assert_eq!(files.snapshot_check("hi\n").expect("match"), "");
        files.begin(1, Some(2));
        assert_eq!(
            files.snapshot_check("other").expect("mismatch"),
            "the text differs from the snapshot file __snapshots__/pkg/a-test.2-1.snap: expected \"hi\\n\", actual \"other\"; run `hd test --update` to record it"
        );
        std::fs::write(root.join("snapshots/a-test.2-1.snap"), [0xff]).expect("malformed UTF-8");
        files.begin(2, Some(2));
        assert_eq!(files.snapshot_check("\u{fffd}").expect("lossy UTF-8"), "");
        let mut missing = Files::new(&TestCase::default());
        assert_eq!(
            missing.snapshot_check("x").expect("no configuration"),
            "no snapshot file: this run names no snapshot directory"
        );
    }

    #[test]
    fn regression_files_round_trip_empty_streams_and_reject_malformed_draws() {
        let mut owner = Files::new(&TestCase::default());
        let root = PathBuf::from(owner.temp_dir().expect("root"));
        let path = root.join("regressions/a-test");
        assert!(
            load_regression(Some(&path), None)
                .expect("absent")
                .is_empty()
        );
        save_regression(&path, &[0, 7, i64::MAX]).expect("save");
        assert_eq!(
            std::fs::read_to_string(&path).expect("format"),
            format!("0\n7\n{}\n", i64::MAX)
        );
        assert_eq!(
            load_regression(Some(&path), None).expect("load"),
            [vec![0, 7, i64::MAX]]
        );
        save_regression(&path, &[]).expect("empty stream");
        assert_eq!(
            load_regression(Some(&path), None).expect("empty replay"),
            [Vec::<i64>::new()]
        );
        for text in ["bad\n", "9223372036854775808\n", "1.0\n"] {
            std::fs::write(&path, text).expect("bad fixture");
            assert!(load_regression(Some(&path), None).is_err());
        }
        std::fs::write(&path, "\u{feff} 1 \r\n\n 2\n").expect("whitespace");
        assert_eq!(
            load_regression(Some(&path), None).expect("trimmed replay"),
            [vec![1, 2]]
        );
    }

    #[cfg(unix)]
    #[test]
    fn regression_persistence_replaces_a_fifo_without_opening_it() {
        let mut owner = Files::new(&TestCase::default());
        let path = PathBuf::from(owner.temp_dir().unwrap()).join("regression");
        nix::unistd::mkfifo(
            &path,
            nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
        )
        .unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            send.send(save_regression(&path, &[8])).unwrap();
        });
        let result = receive
            .recv_timeout(std::time::Duration::from_secs(3))
            .expect("regression persistence must not open the FIFO");
        worker.join().unwrap();
        result.unwrap();
        let path = PathBuf::from(owner.temp_dir().unwrap()).join("regression");
        assert!(path.is_file());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "8\n");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn failed_regression_rename_preserves_the_target_and_cleans_the_temporary() {
        let mut owner = Files::new(&TestCase::default());
        let root = PathBuf::from(owner.temp_dir().unwrap());
        let path = root.join("not-a-file");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("kept"), "original").unwrap();
        assert!(save_regression(&path, &[8]).is_err());
        assert_eq!(
            std::fs::read_to_string(path.join("kept")).unwrap(),
            "original"
        );
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
    }
}
