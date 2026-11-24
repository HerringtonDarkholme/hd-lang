//! Host-only formatting of the checked collector's descriptor counters.
use hd_base::SliceSort as _;
use std::collections::BTreeSet;

use crate::host::Host;
use wasmtime::{Instance, Store};

type SiteKey = (String, String, u32, String);
pub type PairTotals = std::collections::BTreeMap<SiteKey, u128>;

#[derive(Clone, Debug)]
pub struct Report(Vec<(u32, u8)>, pub Vec<CountSite>);

#[derive(Clone, Debug)]
pub struct CountSite {
    id: u32,
    function: String,
    file: String,
    line: u32,
    reason: String,
}

#[must_use]
pub fn count_sites(data: &[u8]) -> Option<Vec<CountSite>> {
    let mut r = hd_base::wire::Reader::new(data);
    if r.u8() != 1 {
        return None;
    }
    let mut seen = BTreeSet::new();
    let mut sites = Vec::new();
    while r.ok() && !r.at_end() {
        let id = r.u32();
        if id == 0 || !seen.insert(id) {
            return None;
        }
        sites.push(CountSite {
            id,
            function: r.str().to_owned(),
            file: r.str().to_owned(),
            line: r.u32(),
            reason: r.str().to_owned(),
        });
    }
    r.ok().then_some(sites)
}

fn leb(bytes: &mut &[u8]) -> Option<usize> {
    usize::try_from(hd_base::leb::take(bytes, 32)?).ok()
}

impl Report {
    #[must_use]
    pub fn read(wasm: &[u8]) -> Option<Self> {
        let mut bytes = wasm.get(8..)?;
        let mut found = None;
        let mut sites = Vec::new();
        while !bytes.is_empty() {
            let (&id, rest) = bytes.split_first()?;
            bytes = rest;
            let length = leb(&mut bytes)?;
            let (mut section, rest) = bytes.split_at_checked(length)?;
            bytes = rest;
            if id != 0 {
                continue;
            }
            let length = leb(&mut section)?;
            let (name, data) = section.split_at_checked(length)?;
            if name == b"hd.count.sites" {
                sites = count_sites(data)?;
            }
            if name != b"hd.cycles" {
                continue;
            }
            if found.is_some() {
                return None;
            }
            found = Some(Self::decode(data)?);
        }
        let mut report = found?;
        report.1 = sites;
        Some(report)
    }

    #[must_use]
    pub fn decode(data: &[u8]) -> Option<Self> {
        let (&version, data) = data.split_first()?;
        if version != 1 || !data.len().is_multiple_of(5) {
            return None;
        }
        let mut seen = BTreeSet::new();
        let mut entries = Vec::new();
        for row in data.chunks_exact(5) {
            let id = u32::from_le_bytes(row[..4].try_into().ok()?);
            let kind = row[4];
            if id == 0 || kind > 6 || !seen.insert(id) {
                return None;
            }
            entries.push((id, kind));
        }
        Some(Self(entries, Vec::new()))
    }

    pub(crate) fn summary(
        &self,
        instance: Instance,
        store: &mut Store<Host>,
        previous: &mut PairTotals,
    ) -> Option<String> {
        if let Some(path) = std::env::var_os("HD_COUNT_AUDIT_REPORT")
            && let Some(rows) = self.pairs(instance, store, previous)
        {
            use std::io::Write as _;
            let record = serde_json::json!({"program":store.data().program, "sites":rows});
            let mut line = serde_json::to_vec(&record).ok()?;
            line.push(b'\n');
            let result = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut file| file.write_all(&line));
            if let Err(error) = result {
                store
                    .data_mut()
                    .err(&format!("count audit report: {error}\n"));
            }
        }
        let pointer = instance
            .get_typed_func::<(), i32>(&mut *store, "hd.cycles.pointer")
            .ok()?
            .call(&mut *store, ())
            .ok()?
            .cast_unsigned() as usize;
        let length = instance
            .get_typed_func::<(), i32>(&mut *store, "hd.cycles.length")
            .ok()?
            .call(&mut *store, ())
            .ok()?
            .cast_unsigned() as usize;
        let memory = instance.get_memory(&mut *store, "hd.heap")?;
        let heap = memory.data(&*store);
        let counts = heap.get(pointer..pointer.checked_add(length.checked_mul(8)?)?)?;
        self.format(counts)
    }

    fn pairs(
        &self,
        instance: Instance,
        store: &mut Store<Host>,
        previous: &mut PairTotals,
    ) -> Option<Vec<serde_json::Value>> {
        let pointer = instance
            .get_typed_func::<(), i32>(&mut *store, "hd.count.pairs.pointer")
            .ok()?
            .call(&mut *store, ())
            .ok()?
            .cast_unsigned() as usize;
        let length = instance
            .get_typed_func::<(), i32>(&mut *store, "hd.count.pairs.length")
            .ok()?
            .call(&mut *store, ())
            .ok()?
            .cast_unsigned() as usize;
        let memory = instance.get_memory(&mut *store, "hd.heap")?;
        let heap = memory.data(&*store);
        let counts = heap.get(pointer..pointer.checked_add(length.checked_mul(8)?)?)?;
        self.pair_rows(counts, previous)
    }

    pub fn pair_rows(
        &self,
        counts: &[u8],
        previous: &mut PairTotals,
    ) -> Option<Vec<serde_json::Value>> {
        let mut totals = std::collections::BTreeMap::new();
        let missing = (
            "<unattributed>".to_owned(),
            "<runtime>".to_owned(),
            0,
            "instrumentation gap".to_owned(),
        );
        if let Some(zero) = counts.get(..8) {
            let count = u64::from_le_bytes(zero.try_into().ok()?);
            if count != 0 {
                totals.insert(
                    (&missing.0, &missing.1, missing.2, &missing.3),
                    u128::from(count),
                );
            }
        }
        for site in &self.1 {
            let at = site.id as usize * 8;
            if at >= counts.len() {
                continue;
            }
            let count = u64::from_le_bytes(counts.get(at..at + 8)?.try_into().ok()?);
            if count == 0 {
                continue;
            }
            let key = (&site.function, &site.file, site.line, &site.reason);
            *totals.entry(key).or_insert(0u128) += u128::from(count);
        }
        let mut current = std::collections::BTreeMap::new();
        let mut rows = Vec::new();
        for (key, count) in totals {
            let key = (key.0.clone(), key.1.clone(), key.2, key.3.clone());
            let before = previous.get(&key).copied().unwrap_or(0);
            let delta = if count >= before {
                count - before
            } else {
                count
            };
            current.insert(key.clone(), count);
            if delta != 0 {
                rows.push((key, delta));
            }
        }
        *previous = current;
        rows.sort_stable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Some(rows.into_iter().map(|((function,file,line,reason),count)| serde_json::json!({"function":function,"file":file,"line":line,"reason":reason,"count":count.to_string()})).collect())
    }

    #[must_use]
    pub fn format(&self, counts: &[u8]) -> Option<String> {
        let mut totals = [0u64; 7];
        for &(id, kind) in &self.0 {
            let at = id as usize * 8;
            if at >= counts.len() {
                continue;
            }
            let count = u64::from_le_bytes(counts.get(at..at + 8)?.try_into().ok()?);
            totals[kind as usize] = totals[kind as usize].checked_add(count)?;
        }
        Some(format!(
            "cycles freed: {} closure envs, {} local cells, {} frames, {} waker/all nodes, {} list/map buffers, {} list/map headers, {} objects\n",
            totals[1], totals[2], totals[3], totals[4], totals[5], totals[6], totals[0]
        ))
    }
}
