//! `hd_iface`: folder interfaces and the canonical bytes every key and entry
//! uses (resolution-and-interfaces.md §4.10, §4.11; cache.md §5.3). Content
//! only: stable paths and canonical types, never a run ID.

use std::collections::{BTreeSet, HashMap};

use hd_base::Hash128;

/// The content form of a type: stable paths, no IDs. Interfaces, entries and
/// keys use it (`canon(T)`, codegen.md §13.3).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CTy {
    Void,
    Never,
    Bool,
    I32,
    Param(u32),
    SelfTy,
    Adt(String),
    ClassRef,
}

impl CTy {
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            CTy::Void => out.push(0),
            CTy::Never => out.push(1),
            CTy::Bool => out.push(2),
            CTy::I32 => out.push(3),
            CTy::Param(i) => {
                out.push(4);
                out.extend_from_slice(&i.to_le_bytes());
            }
            CTy::SelfTy => out.push(5),
            CTy::Adt(path) => {
                out.push(6);
                put_str(out, path);
            }
            CTy::ClassRef => out.push(7),
        }
    }
    pub fn decode(r: &mut Reader<'_>) -> CTy {
        match r.u8() {
            0 => CTy::Void,
            1 => CTy::Never,
            2 => CTy::Bool,
            3 => CTy::I32,
            4 => CTy::Param(r.u32()),
            5 => CTy::SelfTy,
            6 => CTy::Adt(r.str()),
            7 => CTy::ClassRef,
            t => panic!("bad type tag {t}"),
        }
    }
}


// ---- canonical bytes and hashing (cache.md §5.3: H(kind tag, fields...)) ----

pub fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&u32::try_from(s.len()).expect("len").to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}
pub fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
pub fn put_hash(out: &mut Vec<u8>, h: Hash128) {
    out.extend_from_slice(&h.0.to_le_bytes());
}

pub struct Reader<'a> {
    pub bytes: &'a [u8],
    pub pos: usize,
}
impl<'a> Reader<'a> {
    #[must_use]
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    pub fn u8(&mut self) -> u8 {
        let v = self.bytes[self.pos];
        self.pos += 1;
        v
    }
    pub fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.bytes[self.pos..self.pos + 4].try_into().expect("u32"));
        self.pos += 4;
        v
    }
    pub fn u64(&mut self) -> u64 {
        let v = u64::from_le_bytes(self.bytes[self.pos..self.pos + 8].try_into().expect("u64"));
        self.pos += 8;
        v
    }
    pub fn hash(&mut self) -> Hash128 {
        let v = u128::from_le_bytes(self.bytes[self.pos..self.pos + 16].try_into().expect("h"));
        self.pos += 16;
        Hash128(v)
    }
    pub fn str(&mut self) -> String {
        let n = self.u32() as usize;
        let s = std::str::from_utf8(&self.bytes[self.pos..self.pos + n]).expect("utf8").to_owned();
        self.pos += n;
        s
    }
    #[must_use]
    pub fn done(&self) -> bool {
        self.pos >= self.bytes.len()
    }
}

/// `H(tag, fields...)`: the hasher every key uses.
pub struct KeyHasher(Vec<u8>);
impl KeyHasher {
    #[must_use]
    pub fn new(tag: &str) -> Self {
        let mut v = Vec::new();
        put_str(&mut v, tag);
        Self(v)
    }
    #[must_use]
    pub fn str(mut self, s: &str) -> Self {
        put_str(&mut self.0, s);
        self
    }
    #[must_use]
    pub fn hash(mut self, h: Hash128) -> Self {
        put_hash(&mut self.0, h);
        self
    }
    #[must_use]
    pub fn bytes(mut self, b: &[u8]) -> Self {
        put_u32(&mut self.0, u32::try_from(b.len()).expect("len"));
        self.0.extend_from_slice(b);
        self
    }
    #[must_use]
    pub fn finish(self) -> Hash128 {
        hd_base::hash128(&self.0)
    }
}

// ------------------------------------------------------------- header items

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CSig {
    pub generics: Vec<(String, Option<String>)>,
    pub params: Vec<(String, CTy)>,
    pub ret: CTy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CItem {
    Fn(CSig),
    Data(Vec<(String, CTy)>),
    Trait(Vec<(String, CSig)>),
    Impl { trait_: String, target: CTy, methods: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderItem {
    pub path: String,
    pub public: bool,
    pub item: CItem,
}

/// Stable path of item `name` in module `module`.
#[must_use]
pub fn item_path(module: &str, name: &str) -> String {
    format!("{module}::{name}")
}
#[must_use]
pub fn module_of(path: &str) -> &str {
    path.split("::").next().unwrap_or(path)
}
#[must_use]
pub fn folder_of_module(module: &str) -> &str {
    module.rsplit_once('.').map_or(module, |(f, _)| f)
}
#[must_use]
pub fn folder_of(path: &str) -> &str {
    folder_of_module(module_of(path))
}

// ------------------------------------------------------------------- blob

fn put_sig(out: &mut Vec<u8>, s: &CSig) {
    put_u32(out, u32::try_from(s.generics.len()).expect("n"));
    for (g, b) in &s.generics {
        put_str(out, g);
        put_str(out, b.as_deref().unwrap_or(""));
    }
    put_u32(out, u32::try_from(s.params.len()).expect("n"));
    for (p, t) in &s.params {
        put_str(out, p);
        t.encode(out);
    }
    s.ret.encode(out);
}
fn get_sig(r: &mut Reader<'_>) -> CSig {
    let ng = r.u32();
    let generics = (0..ng)
        .map(|_| {
            let g = r.str();
            let b = r.str();
            (g, if b.is_empty() { None } else { Some(b) })
        })
        .collect();
    let np = r.u32();
    let params = (0..np).map(|_| (r.str(), CTy::decode(r))).collect();
    CSig { generics, params, ret: CTy::decode(r) }
}

pub fn encode_item(out: &mut Vec<u8>, h: &HeaderItem) {
    put_str(out, &h.path);
    out.push(u8::from(h.public));
    match &h.item {
        CItem::Fn(s) => {
            out.push(0);
            put_sig(out, s);
        }
        CItem::Data(fields) => {
            out.push(1);
            put_u32(out, u32::try_from(fields.len()).expect("n"));
            for (f, t) in fields {
                put_str(out, f);
                t.encode(out);
            }
        }
        CItem::Trait(ms) => {
            out.push(2);
            put_u32(out, u32::try_from(ms.len()).expect("n"));
            for (m, s) in ms {
                put_str(out, m);
                put_sig(out, s);
            }
        }
        CItem::Impl { trait_, target, methods } => {
            out.push(3);
            put_str(out, trait_);
            target.encode(out);
            put_u32(out, u32::try_from(methods.len()).expect("n"));
            for m in methods {
                put_str(out, m);
            }
        }
    }
}
pub fn decode_item(r: &mut Reader<'_>) -> HeaderItem {
    let path = r.str();
    let public = r.u8() != 0;
    let item = match r.u8() {
        0 => CItem::Fn(get_sig(r)),
        1 => {
            let n = r.u32();
            CItem::Data((0..n).map(|_| (r.str(), CTy::decode(r))).collect())
        }
        2 => {
            let n = r.u32();
            CItem::Trait((0..n).map(|_| (r.str(), get_sig(r))).collect())
        }
        _ => {
            let trait_ = r.str();
            let target = CTy::decode(r);
            let n = r.u32();
            CItem::Impl { trait_, target, methods: (0..n).map(|_| r.str()).collect() }
        }
    };
    HeaderItem { path, public, item }
}

/// A folder interface (§4.10): the blob is the canonical form; `items` is a
/// decoded view of it (the design reads the blob in place).
#[derive(Clone, Debug)]
pub struct FolderIface {
    pub folder: String,
    pub blob: Vec<u8>,
    pub items: Vec<HeaderItem>,
    pub item_hashes: Vec<Hash128>,
    /// Stable path to its index in `items`: the import lookup's index
    /// (syntax.md §4.6 `by_name`), so resolving N imports is linear.
    pub by_path: HashMap<String, usize>,
    pub api_hash: Hash128,
    pub deep_hash: Hash128,
}

impl FolderIface {
    /// The public item at `path`, if this interface exports one.
    #[must_use]
    pub fn public_item(&self, path: &str) -> Option<&HeaderItem> {
        self.by_path.get(path).map(|&i| &self.items[i]).filter(|h| h.public)
    }
}

/// The blob: `deep_hash` first (it needs the dependencies' deep hashes, so it
/// is stored, not recomputed by a reader), then every interface item.
#[must_use]
pub fn build_iface(
    folder: &str,
    items: &[HeaderItem],
    deps: &HashMap<String, FolderIface>,
) -> FolderIface {
    let mut api = Vec::new();
    for h in items {
        encode_item(&mut api, h);
    }
    let api_hash = KeyHasher::new("api").bytes(&api).finish();
    // mentions(F): folders whose items F's api names (§4.11.3).
    let mut mentions = BTreeSet::new();
    let mut note = |c: &CTy| {
        if let CTy::Adt(p) = c {
            mentions.insert(folder_of(p).to_owned());
        }
    };
    for h in items {
        match &h.item {
            CItem::Fn(s) => {
                s.params.iter().for_each(|p| note(&p.1));
                note(&s.ret);
                for (_, b) in &s.generics {
                    if let Some(b) = b {
                        note(&CTy::Adt(b.clone()));
                    }
                }
            }
            CItem::Data(f) => f.iter().for_each(|p| note(&p.1)),
            CItem::Trait(ms) => ms.iter().for_each(|(_, s)| {
                s.params.iter().for_each(|p| note(&p.1));
                note(&s.ret);
            }),
            CItem::Impl { trait_, target, .. } => {
                note(&CTy::Adt(trait_.clone()));
                note(target);
            }
        }
    }
    mentions.remove(folder);
    let mut deep = KeyHasher::new("deep").hash(api_hash);
    for m in &mentions {
        let d = deps.get(m).map_or(Hash128(0), |i| i.deep_hash);
        deep = deep.str(m).hash(d);
    }
    let deep_hash = deep.finish();
    let mut blob = Vec::new();
    put_str(&mut blob, folder);
    put_hash(&mut blob, api_hash);
    put_hash(&mut blob, deep_hash);
    blob.extend_from_slice(&api);
    decode_iface(&blob)
}

#[must_use]
pub fn decode_iface(blob: &[u8]) -> FolderIface {
    let mut r = Reader::new(blob);
    let folder = r.str();
    let api_hash = r.hash();
    let deep_hash = r.hash();
    let mut items = Vec::new();
    let mut item_hashes = Vec::new();
    while !r.done() {
        let start = r.pos;
        items.push(decode_item(&mut r));
        item_hashes.push(KeyHasher::new("item").bytes(&blob[start..r.pos]).finish());
    }
    let by_path = items.iter().enumerate().map(|(i, h)| (h.path.clone(), i)).collect();
    FolderIface { folder, blob: blob.to_vec(), items, item_hashes, by_path, api_hash, deep_hash }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fn_item(path: &str, public: bool, params: Vec<(String, CTy)>, ret: CTy) -> HeaderItem {
        let sig = CSig { generics: vec![], params, ret };
        HeaderItem { path: path.into(), public, item: CItem::Fn(sig) }
    }

    #[test]
    fn blob_round_trips_and_indexes_public_items() {
        let x = vec![("x".to_owned(), CTy::I32)];
        let items =
            vec![fn_item("pkg.geo.a::f", true, x.clone(), CTy::Bool), fn_item("pkg.geo.a::g", false, x, CTy::Bool)];
        let iface = build_iface("pkg.geo", &items, &HashMap::new());
        let again = decode_iface(&iface.blob);
        assert_eq!(again.items, iface.items);
        assert_eq!(again.deep_hash, iface.deep_hash);
        assert!(again.public_item("pkg.geo.a::f").is_some());
        assert!(again.public_item("pkg.geo.a::g").is_none());
        assert!(again.public_item("pkg.geo.a::h").is_none());
    }

    #[test]
    fn deep_hash_follows_a_mentioned_folder() {
        let dep = |ret: CTy| build_iface("pkg.b", &[fn_item("pkg.b.m::f", true, vec![], ret)], &HashMap::new());
        let user = |b: FolderIface| {
            let p = vec![("p".to_owned(), CTy::Adt("pkg.b.m::P".into()))];
            let items = vec![fn_item("pkg.a.m::g", true, p, CTy::Void)];
            build_iface("pkg.a", &items, &HashMap::from([("pkg.b".to_owned(), b)])).deep_hash
        };
        assert_ne!(user(dep(CTy::I32)), user(dep(CTy::Bool)));
    }
}
