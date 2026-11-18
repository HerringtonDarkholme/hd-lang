//! `HD_WAT`: `hd build` writes the module's text beside its `.wasm`, every
//! function by hd's own name, runtime helpers included (`call $rt:retain`),
//! one instruction per line, so `scripts/ir-audit.py DIR` reads the bench
//! programs as it reads the stage baselines (#1973). Off by default: the
//! build then records its instruction origins, which bypasses the stored
//! artifact.

use std::collections::BTreeMap;

/// Whether this process writes `NAME.wat` beside each built module.
pub fn requested() -> bool {
    std::env::var_os("HD_WAT").is_some()
}

/// `wasm`'s text, its function names those of `names` (index, name) over
/// the module's own name section. One pass over the module's sections,
/// then the printer's: O(module size + functions log functions).
pub fn text(wasm: &[u8], names: &[(u32, &str)]) -> Result<String, String> {
    let module = renamed(wasm, names)?;
    wasmprinter::print_bytes(module).map_err(|e| e.to_string())
}

/// `wasm` with one `name` section: function names from `names`, then the
/// module's own for any function `names` leaves out, then an import's
/// module and field; its other name subsections (locals, globals, ...)
/// kept as they were.
fn renamed(wasm: &[u8], names: &[(u32, &str)]) -> Result<Vec<u8>, String> {
    let bytes_at = |r: std::ops::Range<u64>| {
        let at = usize::try_from(r.start).ok()?..usize::try_from(r.end).ok()?;
        wasm.get(at)
    };
    let short = || "the module ends early".to_owned();
    // The magic number and version.
    let mut out = wasm.get(..8).ok_or_else(short)?.to_vec();
    out.reserve(wasm.len() + names.len() * 16);
    let mut functions: BTreeMap<u32, String> = BTreeMap::new();
    let mut kept = Vec::new();
    // The defined functions' indices, after the imported ones.
    let (mut imported, mut unnamed) = (0, 0..0);
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        let payload = payload.map_err(|e| e.to_string())?;
        // An imported function's name: its module without `hd:`, then its
        // field (`rt:panic`), unless a name section names it.
        if let wasmparser::Payload::FunctionSection(defined) = &payload {
            unnamed = imported..imported + defined.count();
        }
        if let wasmparser::Payload::ImportSection(imports) = &payload {
            let mut index = 0;
            for import in imports.clone().into_imports() {
                let import = import.map_err(|e| e.to_string())?;
                if let wasmparser::TypeRef::Func(_) | wasmparser::TypeRef::FuncExact(_) = import.ty
                {
                    let module = import.module.strip_prefix("hd:").unwrap_or(import.module);
                    functions.insert(index, format!("{module}:{}", import.name));
                    index += 1;
                }
            }
            imported = index;
        }
        if let wasmparser::Payload::CustomSection(c) = &payload
            && c.name() == "name"
        {
            // Each subsection: its id, its size, its contents.
            let mut r = wasmparser::BinaryReader::new(c.data(), c.data_offset());
            let names = |r: &mut wasmparser::BinaryReader<'_>,
                         functions: &mut BTreeMap<u32, String>,
                         kept: &mut Vec<u8>|
             -> wasmparser::Result<()> {
                let id = r.read_u8()?;
                let size = r.read_var_u32()? as usize;
                let at = r.original_position();
                let bytes = r.read_bytes(size)?;
                if id == 1 {
                    let map = wasmparser::NameMap::new(wasmparser::BinaryReader::new(bytes, at))?;
                    for naming in map {
                        let naming = naming?;
                        functions.insert(naming.index, naming.name.to_owned());
                    }
                } else {
                    kept.push(id);
                    leb(kept, size);
                    kept.extend_from_slice(bytes);
                }
                Ok(())
            };
            while !r.eof() {
                names(&mut r, &mut functions, &mut kept).map_err(|e| e.to_string())?;
            }
            continue;
        }
        if let Some((id, range)) = payload.as_section() {
            // Ruled out: a range past `wasm`, from which the parser read it.
            let bytes = bytes_at(range).ok_or_else(short)?;
            out.push(id);
            leb(&mut out, bytes.len());
            out.extend_from_slice(bytes);
        }
    }
    for &(index, name) in names {
        functions.insert(index, name.to_owned());
    }
    // A function no origin and no name section names (a generated
    // helper): by its index, so every call names its callee.
    for index in unnamed {
        functions
            .entry(index)
            .or_insert_with(|| format!("func{index}"));
    }
    let mut map = Vec::new();
    leb(&mut map, functions.len());
    for (index, name) in &functions {
        leb(&mut map, *index as usize);
        leb(&mut map, name.len());
        map.extend_from_slice(name.as_bytes());
    }
    let mut section = Vec::new();
    leb(&mut section, "name".len());
    section.extend_from_slice(b"name");
    section.push(1);
    leb(&mut section, map.len());
    section.extend_from_slice(&map);
    section.extend_from_slice(&kept);
    out.push(0);
    leb(&mut out, section.len());
    out.extend_from_slice(&section);
    Ok(out)
}

/// Appends `n` as unsigned LEB128.
fn leb(out: &mut Vec<u8>, n: usize) {
    hd_base::leb::put(out, n as u64);
}

#[cfg(test)]
mod tests {
    /// A module of an imported `hd:rt.panic` (0) and three functions of
    /// type `(param i32)`: 1 named `own` with its local 0 named `x` by its
    /// name section, 2 and 3 unnamed; 1 calls 0.
    fn module() -> Vec<u8> {
        let mut m = b"\0asm\x01\0\0\0".to_vec();
        m.extend_from_slice(&[1, 5, 1, 0x60, 1, 0x7f, 0]);
        m.extend_from_slice(&[2, 15, 1, 5]);
        m.extend_from_slice(b"hd:rt\x05panic\x00\x00");
        m.extend_from_slice(&[3, 4, 3, 0, 0, 0]);
        m.extend_from_slice(&[
            10, 14, 3, 6, 0, 0x20, 0, 0x10, 0, 0x0b, 2, 0, 0x0b, 2, 0, 0x0b,
        ]);
        m.extend_from_slice(&[0, 21, 4]);
        m.extend_from_slice(b"name");
        m.extend_from_slice(&[1, 6, 1, 1, 3]);
        m.extend_from_slice(b"own");
        m.extend_from_slice(&[2, 6, 1, 1, 1, 0, 1]);
        m.extend_from_slice(b"x");
        m
    }

    /// Every function prints by a name: an origin's, the name section's,
    /// an import's module and field, or its index; local names stay.
    #[test]
    fn every_function_prints_by_name() {
        let text = super::text(&module(), &[(2, "origin")]).expect("prints");
        for want in [
            "(import \"hd:rt\" \"panic\" (func $rt:panic",
            "(func $own (;1;) (type 0) (param $x i32)",
            "call $rt:panic",
            "(func $origin (;2;)",
            "(func $func3 (;3;)",
        ] {
            assert!(text.contains(want), "{want} not in\n{text}");
        }
    }

    /// An origin's name wins over the name section's.
    #[test]
    fn an_origin_renames_a_named_function() {
        let text = super::text(&module(), &[(1, "origin")]).expect("prints");
        assert!(
            text.contains("(func $origin (;1;) (type 0) (param $x i32)"),
            "{text}"
        );
        assert!(!text.contains("$own"), "{text}");
    }

    /// A module cut short is an error, not a panic.
    #[test]
    fn a_cut_module_is_an_error() {
        assert!(super::text(b"\0asm", &[]).is_err());
        let mut cut = module();
        cut.truncate(20);
        assert!(super::text(&cut, &[]).is_err());
    }
}
