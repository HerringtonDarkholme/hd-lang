#[cfg(windows)]
mod tests {
    use crate::fs::test_support::error;
    use hd_run_wasmtime::internals::fs::api::{Failure, failure};

    #[test]
    fn device_namespace_detection_preserves_ordinary_files() {
        for path in [
            r"\\.\pipe\hd-test",
            r"\\?\PIPE\hd-test",
            "//./pipe/hd-test",
            r"\\localhost\pipe\hd-test",
            r"\\?\UNC\host\pipe\hd-test",
            r"\\?\GLOBALROOT\Device\NamedPipe\hd-test",
            r"\\.\NUL",
        ] {
            assert!(
                hd_run_wasmtime::internals::fs::io::device_path(path),
                "{path}"
            );
        }
        for path in [
            r"C:\pipe\hd-test",
            r"\\host\share\pipe\hd-test",
            r"\\host\pipeline\hd-test",
            r"\\?\C:\pipe\hd-test",
            "pipe/hd-test",
        ] {
            assert!(
                !hd_run_wasmtime::internals::fs::io::device_path(path),
                "{path}"
            );
        }
    }
    #[test]
    fn windows_errors_use_rust_error_categories_and_native_messages() {
        use std::io::ErrorKind;
        use windows_sys::Win32::Foundation::{
            ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_DIRECTORY,
            ERROR_DIRECTORY_NOT_SUPPORTED, ERROR_FILE_NOT_FOUND, ERROR_INVALID_NAME, ERROR_NO_DATA,
            ERROR_SHARING_VIOLATION,
        };
        // Rust std's Windows decode_error_kind, not libuv's translation.
        for (code, kind, variant) in [
            (ERROR_ACCESS_DENIED, ErrorKind::PermissionDenied, 1),
            (ERROR_ALREADY_EXISTS, ErrorKind::AlreadyExists, 3),
            (ERROR_FILE_NOT_FOUND, ErrorKind::NotFound, 0),
            (ERROR_DIRECTORY, ErrorKind::NotADirectory, 4),
            (ERROR_DIRECTORY_NOT_SUPPORTED, ErrorKind::IsADirectory, 5),
            (ERROR_INVALID_NAME, ErrorKind::InvalidFilename, 7),
            (ERROR_NO_DATA, ErrorKind::BrokenPipe, 7),
        ] {
            let native = std::io::Error::from_raw_os_error(i32::try_from(code).unwrap());
            assert_eq!(native.kind(), kind);
            let message = format!("open \"to\": {native}");
            let refused = Failure::new("open", native);
            assert_eq!(
                failure("to", "read_text", "to", &refused).done(),
                error(if variant == 7 { &message } else { "to" }, variant).done()
            );
        }
        // Unclassified platform errors remain informative Other values.
        let native =
            std::io::Error::from_raw_os_error(i32::try_from(ERROR_SHARING_VIOLATION).unwrap());
        let message = format!("open \"to\": {native}");
        let refused = Failure::new("open", native);
        assert_eq!(
            failure("to", "read_text", "to", &refused).done(),
            error(&message, 7).done()
        );
    }
}
