//! A module's import list (`cli.cap.total.needs`): read from its import
//! section alone, so it holds alike for a program built from source and for
//! a prebuilt module (`cli.cap.total.any-module`).

/// A reader over a module's bytes.
struct Bytes<'a> {
    b: &'a [u8],
    at: usize,
}

impl Bytes<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let v = *self.b.get(self.at).ok_or("the module ends early")?;
        self.at += 1;
        Ok(v)
    }

    /// An unsigned or signed LEB128 number; only its length matters for a
    /// signed one, which is skipped.
    fn leb(&mut self) -> Result<u64, String> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            v |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("a number is too long".to_owned())
    }

    fn len(&mut self) -> Result<usize, String> {
        usize::try_from(self.leb()?).map_err(|e| e.to_string())
    }

    fn name(&mut self) -> Result<String, String> {
        let n = self.len()?;
        let end = self.at.checked_add(n).ok_or("a name is too long")?;
        let s = self.b.get(self.at..end).ok_or("the module ends early")?;
        self.at = end;
        String::from_utf8(s.to_vec()).map_err(|e| e.to_string())
    }

    /// A value type: one byte, or a reference type `(ref null? ht)`.
    fn val_type(&mut self) -> Result<(), String> {
        if matches!(self.byte()?, 0x63 | 0x64) {
            self.leb()?;
        }
        Ok(())
    }

    fn limits(&mut self) -> Result<(), String> {
        let flags = self.byte()?;
        self.leb()?;
        if flags & 1 != 0 {
            self.leb()?;
        }
        Ok(())
    }
}

/// Each import of a Wasm module, as (module, name), in order.
pub fn module_imports(wasm: &[u8]) -> Result<Vec<(String, String)>, String> {
    if wasm.get(..8) != Some(b"\0asm\x01\0\0\0") {
        return Err("not a Wasm module".to_owned());
    }
    let mut r = Bytes { b: wasm, at: 8 };
    while r.at < wasm.len() {
        let id = r.byte()?;
        let size = r.len()?;
        let end = r.at.checked_add(size).ok_or("a section is too long")?;
        if id != 2 {
            r.at = end;
            continue;
        }
        let mut out = Vec::new();
        for _ in 0..r.len()? {
            let module = r.name()?;
            let name = r.name()?;
            match r.byte()? {
                // A function or a tag: a type index (a tag's after its attribute).
                0x00 => {
                    r.leb()?;
                }
                0x04 => {
                    r.byte()?;
                    r.leb()?;
                }
                0x01 => {
                    r.val_type()?;
                    r.limits()?;
                }
                0x02 => r.limits()?,
                0x03 => {
                    r.val_type()?;
                    r.byte()?;
                }
                k => return Err(format!("an import of unknown kind {k}")),
            }
            out.push((module, name));
        }
        return Ok(out);
    }
    Ok(Vec::new())
}

/// The host capability traits a module needs: those whose methods its
/// imports name (`cli.cap.total.needs`), each once, in table order.
#[must_use]
pub fn needs(imports: &[(String, String)]) -> Vec<&'static str> {
    hd_host_abi::TABLE
        .iter()
        .map(|t| t.key)
        .filter(|key| {
            let module = format!("hd:{key}");
            imports.iter().any(|(m, _)| *m == module)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{module_imports, needs};

    /// A module that imports `hd:Console.write_line.start` (a function), a
    /// memory and a global, by hand.
    #[test]
    fn reads_the_import_section() {
        let mut m = b"\0asm\x01\0\0\0".to_vec();
        // A custom section first, skipped.
        m.extend([0, 3, 1, b'x', 9]);
        let mut body = vec![3];
        for (module, name, desc) in [
            ("hd:Console", "write_line.start", &[0u8, 0][..]),
            ("env", "mem", &[2, 1, 1, 2][..]),
            ("env", "g", &[3, 0x64, 0x6e, 0][..]),
        ] {
            body.push(u8::try_from(module.len()).expect("len"));
            body.extend(module.bytes());
            body.push(u8::try_from(name.len()).expect("len"));
            body.extend(name.bytes());
            body.extend(desc);
        }
        m.push(2);
        m.push(u8::try_from(body.len()).expect("len"));
        m.extend(body);
        let imports = module_imports(&m).expect("imports");
        assert_eq!(imports.len(), 3);
        assert_eq!(imports[0].0, "hd:Console");
        assert_eq!(needs(&imports), ["Console"]);
        assert!(module_imports(b"not wasm").is_err());
    }
}
