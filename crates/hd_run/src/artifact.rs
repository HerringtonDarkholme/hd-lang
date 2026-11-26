//! Native artifacts keep hd's metadata after the engine's native image.
//! The fixed tail locates the metadata without reading the native code.

use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::Path;

use hd_base::Hash128;

const MAGIC: &[u8; 8] = b"HDCWASM1";
const TAIL: usize = 32;

/// An open native image and the hd metadata that describes that same file.
/// Holding the file also lets a running installation survive replacement.
pub struct Native {
    file: File,
    pub metadata: Vec<u8>,
    pub key: Hash128,
}

impl Native {
    /// Reads a fixed tail and the metadata, never the native image.
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt as _;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_GENERIC_EXECUTE, FILE_GENERIC_READ,
            };
            options.access_mode(FILE_GENERIC_READ | FILE_GENERIC_EXECUTE);
        }
        let file = options.open(path).map_err(|e| e.to_string())?;
        Self::from_file(file)
    }

    pub fn from_file(mut file: File) -> Result<Self, String> {
        let size = file.metadata().map_err(|e| e.to_string())?.len();
        let tail_size = u64::try_from(TAIL).map_err(|e| e.to_string())?;
        if size < tail_size {
            return Err("not an hd native artifact".into());
        }
        file.seek(SeekFrom::Start(size - tail_size))
            .map_err(|e| e.to_string())?;
        let mut tail = [0; TAIL];
        file.read_exact(&mut tail).map_err(|e| e.to_string())?;
        if tail.get(24..) != Some(MAGIC.as_slice()) {
            return Err("not an hd native artifact".into());
        }
        let key = Hash128(u128::from_le_bytes(
            tail[..16].try_into().map_err(|_| "invalid native key")?,
        ));
        let len = u64::from_le_bytes(
            tail[16..24]
                .try_into()
                .map_err(|_| "invalid native metadata size")?,
        );
        let start = (size - tail_size)
            .checked_sub(len)
            .ok_or("native metadata extends before the image")?;
        if start == 0 {
            return Err("native artifact has no code image".into());
        }
        let len = usize::try_from(len).map_err(|e| e.to_string())?;
        let mut metadata = Vec::new();
        metadata.try_reserve_exact(len).map_err(|e| e.to_string())?;
        metadata.resize(len, 0);
        file.seek(SeekFrom::Start(start))
            .and_then(|_| file.read_exact(&mut metadata))
            .map_err(|e| e.to_string())?;
        crate::module_imports(&metadata)?;
        crate::module_exports(&metadata)?;
        Ok(Self {
            file,
            metadata,
            key,
        })
    }

    /// A handle to the same image that supplied the metadata.
    pub fn file(&self) -> Result<File, String> {
        self.file.try_clone().map_err(|e| e.to_string())
    }
}

/// Adds the existing Wasm metadata projection and its build-input key.
pub fn encode(mut native: Vec<u8>, wasm: &[u8], key: Hash128) -> Result<Vec<u8>, String> {
    let metadata = crate::metadata_projection(wasm)?;
    native.extend_from_slice(&metadata);
    native.extend_from_slice(&key.0.to_le_bytes());
    native.extend_from_slice(
        &u64::try_from(metadata.len())
            .map_err(|e| e.to_string())?
            .to_le_bytes(),
    );
    native.extend_from_slice(MAGIC);
    Ok(native)
}

/// A program image. Native images carry only the metadata needed by hd;
/// the execution engine maps their machine code from the open file.
#[derive(Clone, Copy)]
pub enum Image<'a> {
    Wasm(&'a [u8]),
    Native(&'a Native),
}
