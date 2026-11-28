mod tests {
    use hd_run_wasmtime::internals::runtime::native_io::*;
    use std::io::{self, ErrorKind};

    #[test]
    fn ordinary_categories_remain_native() {
        for kind in [
            ErrorKind::NotFound,
            ErrorKind::PermissionDenied,
            ErrorKind::AlreadyExists,
            ErrorKind::NotADirectory,
            ErrorKind::IsADirectory,
            ErrorKind::InvalidInput,
        ] {
            assert_eq!(error_kind(&io::Error::from(kind)), kind);
        }
    }

    #[cfg(windows)]
    #[test]
    fn access_refusal_is_permission_denied_even_when_std_leaves_it_unclassified() {
        use windows_sys::Win32::Foundation::{
            ERROR_CANT_ACCESS_FILE, ERROR_ELEVATION_REQUIRED, ERROR_PRIVILEGE_NOT_HELD,
        };
        for code in [
            ERROR_CANT_ACCESS_FILE,
            ERROR_ELEVATION_REQUIRED,
            ERROR_PRIVILEGE_NOT_HELD,
        ] {
            let native = io::Error::from_raw_os_error(i32::try_from(code).unwrap());
            assert_eq!(error_kind(&native), ErrorKind::PermissionDenied);
        }
    }
}
