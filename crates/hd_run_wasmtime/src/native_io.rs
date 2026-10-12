//! Native I/O categories used by filesystem and process providers.

use std::io::{self, ErrorKind};

/// Rust classifies most native failures directly. Preserve access-refusal
/// categories for Windows errors which std currently leaves unclassified;
/// do not translate platform messages or reproduce another host's errno names.
pub(crate) fn error_kind(error: &io::Error) -> ErrorKind {
    #[cfg(windows)]
    if error.raw_os_error().is_some_and(|code| {
        use windows_sys::Win32::Foundation::{
            ERROR_CANT_ACCESS_FILE, ERROR_ELEVATION_REQUIRED, ERROR_PRIVILEGE_NOT_HELD,
        };
        u32::try_from(code).is_ok_and(|code| {
            matches!(
                code,
                ERROR_CANT_ACCESS_FILE | ERROR_ELEVATION_REQUIRED | ERROR_PRIVILEGE_NOT_HELD
            )
        })
    }) {
        return ErrorKind::PermissionDenied;
    }
    error.kind()
}

#[cfg(test)]
mod tests {
    use super::*;

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
