//! The `CacheStore` interface, entry kinds, entry framing, the memory and
//! disk stores, the stat manifest record and eviction (cache.md §5.2,
//! §5.4, §5.5, §5.7, §5.8; data-structures.md §3.20).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use hd_base::{Hash128, NotImplemented, Stage, StageResult};

/// Entry kinds (§5.2). `Code` exists only inside packs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum EntryKind {
    Iface,
    Check,
    CheckTest,
    Graph,
    Pkgres,
    Depfiles,
    Codepack,
    Clpack,
    Packhint,
    Link,
    Cwasm,
    Code,
}

impl EntryKind {
    pub const ALL: [EntryKind; 12] = [
        EntryKind::Iface,
        EntryKind::Check,
        EntryKind::CheckTest,
        EntryKind::Graph,
        EntryKind::Pkgres,
        EntryKind::Depfiles,
        EntryKind::Codepack,
        EntryKind::Clpack,
        EntryKind::Packhint,
        EntryKind::Link,
        EntryKind::Cwasm,
        EntryKind::Code,
    ];
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            EntryKind::Iface => "iface",
            EntryKind::Check => "check",
            EntryKind::CheckTest => "check-test",
            EntryKind::Graph => "graph",
            EntryKind::Pkgres => "pkgres",
            EntryKind::Depfiles => "depfiles",
            EntryKind::Codepack => "codepack",
            EntryKind::Clpack => "clpack",
            EntryKind::Packhint => "packhint",
            EntryKind::Link => "link",
            EntryKind::Cwasm => "cwasm",
            EntryKind::Code => "code",
        }
    }
}

/// The store interface (§5.8). The core never awaits storage.
pub trait CacheStore: Sync {
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>>;
    fn put(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]);
    fn touch(&self, kind: EntryKind, key: &Hash128);
}

type Entries = Mutex<HashMap<(EntryKind, Hash128), Arc<[u8]>>>;

/// The memory store: the browser's store and tests (§5.8). New entries
/// are kept so the JS host can drain and write them back.
#[derive(Default)]
pub struct MemoryStore {
    entries: Entries,
    new: Mutex<Vec<(EntryKind, Hash128)>>,
}

impl MemoryStore {
    /// The entries written since the last drain, in write order.
    pub fn drain_new(&self) -> Vec<(EntryKind, Hash128)> {
        std::mem::take(&mut *self.new.lock().expect("new"))
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.lock().expect("entries").len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl CacheStore for MemoryStore {
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
        self.entries
            .lock()
            .expect("entries")
            .get(&(kind, *key))
            .cloned()
    }
    fn put(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]) {
        self.entries
            .lock()
            .expect("entries")
            .insert((kind, *key), Arc::from(bytes));
        self.new.lock().expect("new").push((kind, *key));
    }
    fn touch(&self, _: EntryKind, _: &Hash128) {}
}

/// The 64-byte entry header (data-structures.md §3.20), little-endian.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryHeader {
    pub kind: EntryKind,
    pub layout: u16,
    pub key: Hash128,
    pub toolchain: Hash128,
    pub payload_len: u64,
    pub checksum: u64,
    pub nsections: u32,
    pub flags: u32,
}

pub const MAGIC: &[u8; 4] = b"HDCE";
pub const HEADER_LEN: usize = 64;
pub const SECTION_LEN: usize = 16;
pub const FLAG_HAS_DIAGNOSTICS: u32 = 1;
pub const FLAG_HAS_TIR: u32 = 2;

/// A section table row: kind, flags, rows and the payload offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionEntry {
    pub kind: u16,
    pub flags: u16,
    pub rows: u32,
    pub offset: u64,
}

fn checksum(bytes: &[u8]) -> u64 {
    let h = hd_base::hash128(bytes).0;
    u64::try_from(h & u128::from(u64::MAX)).expect("low half")
}

/// Frames sections into one entry: header, section table, payloads.
#[must_use]
pub fn encode_entry(
    kind: EntryKind,
    layout: u16,
    key: Hash128,
    toolchain: Hash128,
    flags: u32,
    sections: &[(u16, u32, &[u8])],
) -> Vec<u8> {
    let mut body = Vec::new();
    let table_len = sections.len() * SECTION_LEN;
    let mut payload = Vec::new();
    for &(skind, rows, bytes) in sections {
        let offset = (HEADER_LEN + table_len + payload.len()) as u64;
        body.extend_from_slice(&skind.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(&rows.to_le_bytes());
        body.extend_from_slice(&offset.to_le_bytes());
        payload.extend_from_slice(&u32::try_from(bytes.len()).expect("section").to_le_bytes());
        payload.extend_from_slice(bytes);
    }
    body.extend_from_slice(&payload);
    let mut out = Vec::with_capacity(HEADER_LEN + body.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(kind as u16).to_le_bytes());
    out.extend_from_slice(&layout.to_le_bytes());
    out.extend_from_slice(&key.0.to_le_bytes());
    out.extend_from_slice(&toolchain.0.to_le_bytes());
    out.extend_from_slice(&(body.len() as u64).to_le_bytes());
    out.extend_from_slice(&checksum(&body).to_le_bytes());
    out.extend_from_slice(
        &u32::try_from(sections.len())
            .expect("sections")
            .to_le_bytes(),
    );
    out.extend_from_slice(&flags.to_le_bytes());
    debug_assert_eq!(out.len(), HEADER_LEN);
    out.extend_from_slice(&body);
    out
}

/// Why an entry was rejected; a rejected entry is a miss (§5.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryError {
    Short,
    Magic,
    Kind,
    Checksum,
    Section,
}

fn le<const N: usize>(b: &[u8], at: usize) -> Result<[u8; N], EntryError> {
    b.get(at..at + N)
        .and_then(|s| s.try_into().ok())
        .ok_or(EntryError::Short)
}

/// Checks and splits an entry into its header and section payloads.
pub type Sections<'a> = Vec<(SectionEntry, &'a [u8])>;

/// Checks and splits an entry.
pub fn decode_entry(bytes: &[u8]) -> Result<(EntryHeader, Sections<'_>), EntryError> {
    if bytes.len() < HEADER_LEN {
        return Err(EntryError::Short);
    }
    if &bytes[..4] != MAGIC {
        return Err(EntryError::Magic);
    }
    let kind_n = u16::from_le_bytes(le(bytes, 4)?);
    let kind = *EntryKind::ALL
        .get(kind_n as usize)
        .ok_or(EntryError::Kind)?;
    let h = EntryHeader {
        kind,
        layout: u16::from_le_bytes(le(bytes, 6)?),
        key: Hash128(u128::from_le_bytes(le(bytes, 8)?)),
        toolchain: Hash128(u128::from_le_bytes(le(bytes, 24)?)),
        payload_len: u64::from_le_bytes(le(bytes, 40)?),
        checksum: u64::from_le_bytes(le(bytes, 48)?),
        nsections: u32::from_le_bytes(le(bytes, 56)?),
        flags: u32::from_le_bytes(le(bytes, 60)?),
    };
    let body = &bytes[HEADER_LEN..];
    if body.len() as u64 != h.payload_len || checksum(body) != h.checksum {
        return Err(EntryError::Checksum);
    }
    let mut out = Vec::new();
    for i in 0..h.nsections as usize {
        let at = HEADER_LEN + i * SECTION_LEN;
        let s = SectionEntry {
            kind: u16::from_le_bytes(le(bytes, at)?),
            flags: u16::from_le_bytes(le(bytes, at + 2)?),
            rows: u32::from_le_bytes(le(bytes, at + 4)?),
            offset: u64::from_le_bytes(le(bytes, at + 8)?),
        };
        let off = usize::try_from(s.offset).map_err(|_| EntryError::Section)?;
        let len = u32::from_le_bytes(le(bytes, off)?) as usize;
        let payload = bytes
            .get(off + 4..off + 4 + len)
            .ok_or(EntryError::Section)?;
        out.push((s, payload));
    }
    Ok((h, out))
}

/// One stat manifest record (§5.5; data-structures.md §3.20), 88 bytes on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestRecord {
    pub path: String,
    pub size: u64,
    pub mtime_ns: i64,
    pub ctime_ns: i64,
    pub inode: u64,
    pub source_hash: Hash128,
    pub api_text_hash: Hash128,
    pub uses: Vec<String>,
    pub role: u8,
}

impl ManifestRecord {
    /// A file is unchanged when its stat fields all match (§5.5).
    #[must_use]
    pub fn same_stat(&self, size: u64, mtime_ns: i64, ctime_ns: i64, inode: u64) -> bool {
        self.size == size
            && self.mtime_ns == mtime_ns
            && self.ctime_ns == ctime_ns
            && self.inode == inode
    }
}

/// The eviction shard of a key: its first byte (§5.7, 256 shards).
#[must_use]
pub fn shard_of(key: Hash128) -> u8 {
    u8::try_from(key.0 >> 120).expect("top byte")
}

/// One entry as eviction sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryStat {
    pub kind: EntryKind,
    pub key: Hash128,
    pub bytes: u64,
    pub mtime_s: i64,
}

/// Plans one shard's eviction (§5.7): oldest first until the shard is at
/// most 90% of its budget. Ties break by kind and key, so the plan is
/// deterministic.
#[must_use]
pub fn plan_eviction(entries: &[EntryStat], budget: u64) -> Vec<EntryStat> {
    let total: u64 = entries.iter().map(|e| e.bytes).sum();
    if total <= budget {
        return Vec::new();
    }
    let target = budget / 10 * 9;
    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|e| (e.mtime_s, e.kind, e.key));
    let mut left = total;
    let mut out = Vec::new();
    for e in sorted {
        if left <= target {
            break;
        }
        left -= e.bytes;
        out.push(e);
    }
    out
}

/// The disk store's interface (feature `disk`): `<root>/<kind>/<shard>/<key>`,
/// published by write-then-rename (§5.4). Only the disk feature touches
/// the file system (design-overview.md §2.2 rule 3).
pub struct DiskStore {
    pub root: std::path::PathBuf,
}

impl DiskStore {
    #[must_use]
    pub fn path(&self, kind: EntryKind, key: &Hash128) -> std::path::PathBuf {
        self.root
            .join(kind.as_str())
            .join(format!("{:02x}", shard_of(*key)))
            .join(format!("{:032x}", key.0))
    }
}

#[cfg(feature = "disk")]
impl CacheStore for DiskStore {
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
        std::fs::read(self.path(kind, key)).ok().map(Arc::from)
    }
    fn put(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]) {
        let path = self.path(kind, key);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
    fn touch(&self, _: EntryKind, _: &Hash128) {}
}

/// Verify mode (§5.6): recompute an entry and compare it with the stored one.
pub fn verify_entry(stored: &[u8], recomputed: &[u8]) -> StageResult<bool> {
    let (a, sa) = decode_entry(stored)
        .map_err(|e| NotImplemented::new(Stage::PackageResult, format!("verify: {e:?}")))?;
    let (b, sb) = decode_entry(recomputed)
        .map_err(|e| NotImplemented::new(Stage::PackageResult, format!("verify: {e:?}")))?;
    Ok(a.key == b.key && sa.len() == sb.len() && sa.iter().zip(&sb).all(|(x, y)| x.1 == y.1))
}

#[cfg(test)]
mod tests {
    use super::{
        CacheStore, EntryKind, EntryStat, MemoryStore, decode_entry, encode_entry, plan_eviction,
        shard_of,
    };
    use hd_base::Hash128;

    #[test]
    fn entry_round_trip_and_corruption_is_a_miss() {
        let e = encode_entry(
            EntryKind::Check,
            1,
            Hash128(9),
            Hash128(3),
            0,
            &[(1, 2, b"ab"), (2, 0, b"")],
        );
        let (h, secs) = decode_entry(&e).expect("decodes");
        assert_eq!(h.kind, EntryKind::Check);
        assert_eq!(h.key, Hash128(9));
        assert_eq!(secs.len(), 2);
        assert_eq!(secs[0].1, b"ab");
        let mut bad = e.clone();
        *bad.last_mut().expect("byte") ^= 1;
        assert!(decode_entry(&bad).is_err());
    }

    #[test]
    fn memory_store_drains_new_entries() {
        let s = MemoryStore::default();
        s.put(EntryKind::Iface, &Hash128(1), b"x");
        assert_eq!(
            s.get(EntryKind::Iface, &Hash128(1)).as_deref(),
            Some(&b"x"[..])
        );
        assert_eq!(s.drain_new().len(), 1);
        assert!(s.drain_new().is_empty());
    }

    #[test]
    fn eviction_takes_oldest_until_ninety_percent() {
        let e = |m: i64, b: u64| EntryStat {
            kind: EntryKind::Link,
            key: Hash128(u128::from(m.unsigned_abs())),
            bytes: b,
            mtime_s: m,
        };
        let plan = plan_eviction(&[e(3, 40), e(1, 40), e(2, 40)], 100);
        assert_eq!(plan.iter().map(|x| x.mtime_s).collect::<Vec<_>>(), [1]);
        assert!(plan_eviction(&[e(1, 10)], 100).is_empty());
        assert_eq!(shard_of(Hash128(0xab << 120)), 0xab);
    }
}
