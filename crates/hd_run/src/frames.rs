//! A panic's frames (`crates/hd_wasm/DESIGN.md` §15.9): the engine's backtrace, as
//! function index and module byte offset per frame, mapped through the
//! module's `hd.lines` rows to an anchor, a TIR instruction in the body of
//! the item the function compiles. The caller resolves anchors to
//! `FILE:LINE` against the current sources (`cli.test.report.frame`).
//!
//! The table is read only on a trap, never on a clean run. A function's
//! name comes from the `name` section, or from `hd.names` in a release
//! build, which omits `name`.
//!
//! A module that `hd build` wrote stands alone (`crates/hd_mono/DESIGN.md`
//! "Positions"): its `hd.positions` section gives each anchor's file, line
//! and printed function name, resolved when it was written, so `hd
//! FILE.wasm` prints a panic's frames without the compiler's cache.

use hd_base::HashMap;

use crate::imports::{Bytes, all_sections, import_kinds};

/// A frame of an engine's backtrace: function index, module byte offset.
pub type RawFrame = (u32, u32);

/// A frame that lands on an anchored call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchored {
    /// The stable path of the item whose body holds the anchor: the
    /// function's name (`path#sub`) without its `#sub` suffix.
    pub item: String,
    /// The anchor: the TIR instruction's index in the item's body.
    pub inst: u32,
}

/// A module's line table, parsed once and shared by every case of a test
/// program (§15.9, Q2).
#[derive(Clone, Debug, Default)]
pub struct LineTable {
    /// The number of imported functions: the index of the first function
    /// with a body.
    imports: u32,
    /// The module offset of each function body, by defined function.
    starts: Vec<u32>,
    names: HashMap<u32, String>,
    /// Per function index, its rows: offset in its body, anchor; sorted.
    rows: HashMap<u32, Vec<(u32, u32)>>,
    /// A written module's resolved rows: per function index, its file and
    /// printed name, and per row of its `hd.lines` (in that order) the
    /// line, 0 for a row the build could not resolve.
    resolved: HashMap<u32, (String, String, Vec<u32>)>,
}

impl LineTable {
    /// Reads the table of a module; a module without `hd.lines` has no rows.
    pub fn parse(wasm: &[u8]) -> Result<LineTable, String> {
        let sections = all_sections(wasm)?;
        let imports = u32::try_from(import_kinds(wasm)?.iter().filter(|i| i.2 == 0).count())
            .map_err(|e| e.to_string())?;
        let mut t = LineTable {
            imports,
            ..LineTable::default()
        };
        for (id, start, body) in sections {
            match id {
                10 => {
                    let mut r = Bytes { b: body, at: 0 };
                    for _ in 0..r.len()? {
                        let size = r.len()?;
                        t.starts
                            .push(u32::try_from(start + r.at).map_err(|e| e.to_string())?);
                        r.at = r.at.checked_add(size).ok_or("a body is too long")?;
                    }
                }
                0 => {
                    let mut r = Bytes { b: body, at: 0 };
                    match r.name()?.as_str() {
                        "name" => t.names_section(&mut r)?,
                        "hd.names" => {
                            for _ in 0..r.len()? {
                                let f = num(&mut r)?;
                                t.names.insert(f, r.name()?);
                            }
                        }
                        "hd.lines" => {
                            for _ in 0..r.len()? {
                                let f = num(&mut r)?;
                                let mut rows = Vec::new();
                                let mut off = 0u32;
                                for _ in 0..r.len()? {
                                    off = off
                                        .checked_add(num(&mut r)?)
                                        .ok_or("a row offset is too large")?;
                                    rows.push((off, num(&mut r)?));
                                }
                                t.rows.insert(f, rows);
                            }
                        }
                        "hd.positions" => {
                            let mut strings = Vec::new();
                            for _ in 0..r.len()? {
                                strings.push(r.name()?);
                            }
                            let string = |i: u32| {
                                strings
                                    .get(i as usize)
                                    .cloned()
                                    .ok_or("a string index out of range")
                            };
                            for _ in 0..r.len()? {
                                let f = num(&mut r)?;
                                let file = string(num(&mut r)?)?;
                                let name = string(num(&mut r)?)?;
                                let mut lines = Vec::new();
                                let mut line = 0i64;
                                for _ in 0..r.len()? {
                                    line += zigzag(num(&mut r)?);
                                    lines.push(u32::try_from(line).map_err(|e| e.to_string())?);
                                }
                                t.resolved.insert(f, (file, name, lines));
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Ok(t)
    }

    /// Each function with rows, in index order: its index, the stable
    /// path of its item, and its rows' anchors in `hd.lines` order, which
    /// `hd build` resolves into `hd.positions`.
    #[must_use]
    pub fn functions(&self) -> Vec<(u32, String, Vec<u32>)> {
        let mut fs: Vec<&u32> = self.rows.keys().collect();
        fs.sort_unstable();
        fs.into_iter()
            .filter_map(|f| {
                let name = self.names.get(f)?;
                let item = name
                    .rsplit_once('#')
                    .map_or(name.as_str(), |(item, _)| item);
                let insts = self.rows[f].iter().map(|&(_, inst)| inst).collect();
                Some((*f, item.to_owned(), insts))
            })
            .collect()
    }

    /// The row index of each frame that lands on a row, as `anchor` finds
    /// it: function index and row.
    fn rows_of(&self, frames: &[RawFrame]) -> Vec<(u32, usize)> {
        frames
            .iter()
            .filter_map(|&(f, addr)| {
                let rows = self.rows.get(&f)?;
                let start = *self.starts.get(f.checked_sub(self.imports)? as usize)?;
                let at = addr.checked_sub(start)?;
                let k = rows.partition_point(|&(off, _)| off <= at).checked_sub(1)?;
                Some((f, k))
            })
            .collect()
    }

    /// A written module's frames as `FILE:LINE FUNCTION` lines, panic site
    /// first, from its `hd.positions` section; empty when it has none.
    #[must_use]
    pub fn resolved_lines(&self, frames: &[RawFrame]) -> Vec<String> {
        self.rows_of(frames)
            .into_iter()
            .filter_map(|(f, k)| {
                let (file, name, lines) = self.resolved.get(&f)?;
                let line = *lines.get(k).filter(|l| **l != 0)?;
                Some(format!("{file}:{line} {name}"))
            })
            .collect()
    }

    /// The function names of a `name` section (subsection 1).
    fn names_section(&mut self, r: &mut Bytes<'_>) -> Result<(), String> {
        while r.at < r.b.len() {
            let id = r.byte()?;
            let size = r.len()?;
            let end = r.at.checked_add(size).ok_or("a subsection is too long")?;
            if id == 1 {
                for _ in 0..r.len()? {
                    let f = num(r)?;
                    self.names.insert(f, r.name()?);
                }
            }
            r.at = end;
        }
        Ok(())
    }

    /// The frames that land on an anchored call, innermost first. A frame
    /// belongs to the row with the greatest offset at or below its address
    /// (§15.9, Q1). A frame in a function without rows, such as a runtime
    /// helper or a panic stub, is not reported, so an explicit panic's
    /// first frame is the call of its stub.
    #[must_use]
    pub fn anchor(&self, frames: &[RawFrame]) -> Vec<Anchored> {
        frames
            .iter()
            .filter_map(|&(f, addr)| {
                let rows = self.rows.get(&f)?;
                let start = *self.starts.get(f.checked_sub(self.imports)? as usize)?;
                let at = addr.checked_sub(start)?;
                let k = rows.partition_point(|&(off, _)| off <= at).checked_sub(1)?;
                let name = self.names.get(&f)?;
                let item = name
                    .rsplit_once('#')
                    .map_or(name.as_str(), |(item, _)| item);
                Some(Anchored {
                    item: item.to_owned(),
                    inst: rows[k].1,
                })
            })
            .collect()
    }
}

fn num(r: &mut Bytes<'_>) -> Result<u32, String> {
    u32::try_from(r.leb()?).map_err(|e| e.to_string())
}

/// A zigzag-encoded delta: 0, -1, 1, -2, … as 0, 1, 2, 3, ….
fn zigzag(v: u32) -> i64 {
    let v = i64::from(v);
    if v & 1 == 0 { v >> 1 } else { -((v + 1) >> 1) }
}

/// The `hd.positions` custom section, ready to append to a module: a
/// table of the file paths and printed names, each once; then per function
/// (its index, file and name as table indices), the line of each of its
/// `hd.lines` rows in order, as zigzag deltas from the row before (0 is a
/// row the build could not resolve). Each entry is (function index, file,
/// printed name, lines).
#[must_use]
pub fn positions_section(funcs: &[(u32, String, String, Vec<u32>)]) -> Vec<u8> {
    fn leb(out: &mut Vec<u8>, mut v: usize) {
        loop {
            let b = u8::try_from(v & 0x7f).unwrap_or(0);
            v >>= 7;
            if v == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }
    fn name(out: &mut Vec<u8>, s: &str) {
        leb(out, s.len());
        out.extend_from_slice(s.as_bytes());
    }
    fn index<'s>(s: &'s str, strings: &mut Vec<&'s str>) -> usize {
        strings.iter().position(|x| *x == s).unwrap_or_else(|| {
            strings.push(s);
            strings.len() - 1
        })
    }
    let mut strings: Vec<&str> = Vec::new();
    let mut rows = Vec::new();
    for (f, file, fname, lines) in funcs {
        let (fi, ni) = (index(file, &mut strings), index(fname, &mut strings));
        rows.push((*f, fi, ni, lines));
    }
    let mut body = Vec::new();
    name(&mut body, "hd.positions");
    leb(&mut body, strings.len());
    for s in &strings {
        name(&mut body, s);
    }
    leb(&mut body, rows.len());
    for (f, fi, ni, lines) in rows {
        leb(&mut body, f as usize);
        leb(&mut body, fi);
        leb(&mut body, ni);
        leb(&mut body, lines.len());
        let mut last = 0i64;
        for &line in lines {
            let d = i64::from(line) - last;
            last = i64::from(line);
            let z = if d >= 0 { d << 1 } else { ((-d) << 1) - 1 };
            leb(&mut body, usize::try_from(z).unwrap_or(0));
        }
    }
    let mut out = vec![0];
    leb(&mut out, body.len());
    out.extend(body);
    out
}

#[cfg(test)]
mod tests {
    use super::{Anchored, LineTable};

    fn leb(out: &mut Vec<u8>, mut v: u32) {
        loop {
            let b = u8::try_from(v & 0x7f).expect("seven bits");
            v >>= 7;
            if v == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }

    fn section(m: &mut Vec<u8>, id: u8, body: &[u8]) {
        m.push(id);
        leb(m, u32::try_from(body.len()).expect("len"));
        m.extend(body);
    }

    fn custom(m: &mut Vec<u8>, name: &str, data: &[u8]) {
        let mut body = Vec::new();
        leb(&mut body, u32::try_from(name.len()).expect("len"));
        body.extend(name.bytes());
        body.extend(data);
        section(m, 0, &body);
    }

    /// One imported function (index 0) and two bodies (1 and 2) of ten
    /// bytes each; function 2 has rows at offsets 3 and 7.
    fn module(names: &str) -> Vec<u8> {
        let mut m = b"\0asm\x01\0\0\0".to_vec();
        section(&mut m, 2, &[1, 1, b'm', 1, b'f', 0, 0]);
        let mut code = vec![2];
        for _ in 0..2 {
            code.push(10);
            code.extend([0; 10]);
        }
        section(&mut m, 10, &code);
        custom(&mut m, "hd.lines", &[1, 2, 2, 3, 5, 4, 9]);
        let mut entries = vec![2, 1, 1, b'g', 2];
        entries.push(u8::try_from("app/main/f#0".len()).expect("len"));
        entries.extend(b"app/main/f#0");
        if names == "name" {
            let mut sub = vec![1];
            leb(&mut sub, u32::try_from(entries.len()).expect("len"));
            sub.extend(&entries);
            custom(&mut m, "name", &sub);
        } else {
            custom(&mut m, "hd.names", &entries);
        }
        m
    }

    #[test]
    fn frames_map_to_the_greatest_row_at_or_below_them() {
        for names in ["name", "hd.names"] {
            let m = module(names);
            let t = LineTable::parse(&m).expect("parses");
            // Function 2's body starts after function 1's ten bytes.
            let start = t.starts[1];
            let at = |inst| Anchored {
                item: "app/main/f".into(),
                inst,
            };
            assert_eq!(
                t.anchor(&[
                    (0, 0),
                    (1, t.starts[0] + 4),
                    (2, start + 2),
                    (2, start + 3),
                    (2, start + 6),
                    (2, start + 9),
                ]),
                [at(5), at(5), at(9)]
            );
        }
    }

    /// A written module's `hd.positions` gives each row's file, line and
    /// function, and `functions` lists what `hd build` resolves.
    #[test]
    fn a_written_module_prints_its_resolved_frames() {
        let mut m = module("name");
        let t = LineTable::parse(&m).expect("parses");
        assert_eq!(t.functions(), [(2, "app/main/f".to_owned(), vec![5, 9])]);
        let start = t.starts[1];
        assert!(t.resolved_lines(&[(2, start + 3)]).is_empty());
        m.extend(super::positions_section(&[(
            2,
            "src/main.hd".into(),
            "app.main.f".into(),
            vec![4, 7],
        )]));
        let t = LineTable::parse(&m).expect("parses with positions");
        assert_eq!(
            t.resolved_lines(&[(2, start + 8), (2, start + 3)]),
            ["src/main.hd:7 app.main.f", "src/main.hd:4 app.main.f"]
        );
    }

    #[test]
    fn a_module_without_a_table_maps_no_frame() {
        let mut m = b"\0asm\x01\0\0\0".to_vec();
        section(&mut m, 10, &[1, 2, 0, 0x0b]);
        let t = LineTable::parse(&m).expect("parses");
        assert!(t.anchor(&[(0, 9)]).is_empty());
        assert!(LineTable::parse(b"nope").is_err());
    }
}
