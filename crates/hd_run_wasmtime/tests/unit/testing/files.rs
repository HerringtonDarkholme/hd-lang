mod tests {
    use hd_run_wasmtime::internals::testing::files::{Files, load_regression, save_regression};
    use hd_run_wasmtime::internals::testing::{Snapshot, TestCase};
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
        crate::test_fifo::make(&path);
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
