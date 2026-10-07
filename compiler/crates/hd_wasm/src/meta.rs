//! The `hd.runtime` custom section (runtime-and-host.md §16.4) and the
//! `hd FILE.wasm` import check.

use hd_base::Hash128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EntryKind {
    Main,
    MainBang,
    Tests,
    ReplInput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TestKind {
    It,
    ItEach,
    ItProp,
    DocTest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestMeta {
    pub export: u32,
    pub name: String,
    pub kind: TestKind,
    pub file: u32,
    pub line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeMeta {
    pub format: u16,
    pub compiler: Hash128,
    pub entry: EntryKind,
    pub tests: Vec<TestMeta>,
    pub exchange: Option<String>,
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&u32::try_from(s.len()).expect("str").to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

struct R<'a>(&'a [u8], usize);

impl R<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.0.get(self.1..self.1 + n)?;
        self.1 += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes)
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        self.take(n).and_then(|b| String::from_utf8(b.to_vec()).ok())
    }
}

impl RuntimeMeta {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Vec::new();
        o.extend_from_slice(&self.format.to_le_bytes());
        o.extend_from_slice(&self.compiler.0.to_le_bytes());
        o.push(self.entry as u8);
        o.extend_from_slice(&u32::try_from(self.tests.len()).expect("tests").to_le_bytes());
        for t in &self.tests {
            o.extend_from_slice(&t.export.to_le_bytes());
            put_str(&mut o, &t.name);
            o.push(t.kind as u8);
            o.extend_from_slice(&t.file.to_le_bytes());
            o.extend_from_slice(&t.line.to_le_bytes());
        }
        match &self.exchange {
            Some(e) => {
                o.push(1);
                put_str(&mut o, e);
            }
            None => o.push(0),
        }
        o
    }

    /// Decodes the section; `None` means "not built by hd" (§16.4).
    #[must_use]
    pub fn decode(b: &[u8]) -> Option<RuntimeMeta> {
        let mut r = R(b, 0);
        let format = u16::from_le_bytes(r.take(2)?.try_into().ok()?);
        let compiler = Hash128(u128::from_le_bytes(r.take(16)?.try_into().ok()?));
        let entry = [EntryKind::Main, EntryKind::MainBang, EntryKind::Tests, EntryKind::ReplInput].get(r.u8()? as usize).copied()?;
        let n = r.u32()?;
        let mut tests = Vec::new();
        for _ in 0..n {
            let export = r.u32()?;
            let name = r.str()?;
            let kind = [TestKind::It, TestKind::ItEach, TestKind::ItProp, TestKind::DocTest].get(r.u8()? as usize).copied()?;
            tests.push(TestMeta { export, name, kind, file: r.u32()?, line: r.u32()? });
        }
        let exchange = if r.u8()? == 1 { Some(r.str()?) } else { None };
        (r.1 == b.len()).then_some(RuntimeMeta { format, compiler, entry, tests, exchange })
    }
}

#[cfg(test)]
mod tests {
    use super::{EntryKind, RuntimeMeta, TestKind, TestMeta};
    use hd_base::Hash128;

    #[test]
    fn runtime_meta_round_trips() {
        let m = RuntimeMeta {
            format: 1,
            compiler: Hash128(5),
            entry: EntryKind::Tests,
            tests: vec![TestMeta { export: 3, name: "adds".into(), kind: TestKind::ItEach, file: 0, line: 9 }],
            exchange: Some("hd.exchange".into()),
        };
        assert_eq!(RuntimeMeta::decode(&m.encode()), Some(m));
        assert_eq!(RuntimeMeta::decode(b"xx"), None);
    }
}
