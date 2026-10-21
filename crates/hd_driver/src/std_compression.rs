//! Build-time dictionary for the browser's embedded std text. Codes below
//! 128 are literal bytes, 128..254 select words, and 255 escapes a byte.
//! Decoding runs once per browser instance; native embedding is unchanged.

#[cfg(not(target_family = "wasm"))]
#[derive(Debug, PartialEq, Eq)]
pub struct Packed {
    pub words: Vec<Vec<u8>>,
    pub bytes: Vec<u8>,
}

/// Non-overlapping words, indentation and literal bytes. The same chunks
/// supply frequency counts and encoding, so all savings are exact.
#[cfg(not(target_family = "wasm"))]
fn chunks(mut text: &[u8]) -> impl Iterator<Item = &[u8]> {
    std::iter::from_fn(move || {
        let (&first, tail) = text.split_first()?;
        let extra = if first.is_ascii_alphabetic() || first == b'_' {
            tail.iter()
                .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
                .count()
        } else if first == b'\n' || first == b' ' {
            tail.iter().take_while(|b| **b == b' ').count()
        } else {
            0
        };
        let (word, rest) = text.split_at(extra + 1);
        text = rest;
        Some(word)
    })
}

/// Pick the 127 most useful chunks, deterministically, then substitute
/// one-byte codes. Literal high bytes use the escape, including UTF-8.
#[cfg(not(target_family = "wasm"))]
pub fn compress(text: &[u8]) -> Packed {
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<&[u8], usize> = BTreeMap::new();
    for word in chunks(text).filter(|w| w.len() > 1) {
        *counts.entry(word).or_default() += 1;
    }
    let mut candidates: Vec<_> = counts
        .into_iter()
        .filter_map(|(word, count)| {
            ((word.len() - 1) * count)
                .checked_sub(word.len() + 8)
                .filter(|gain| *gain > 0)
                .map(|gain| (word, gain))
        })
        .collect();
    candidates.sort_unstable_by(|(a, ga), (b, gb)| gb.cmp(ga).then_with(|| a.cmp(b)));
    let words: Vec<Vec<u8>> = candidates
        .iter()
        .take(127)
        .map(|(w, _)| w.to_vec())
        .collect();
    let codes: BTreeMap<&[u8], u8> = words
        .iter()
        .enumerate()
        .map(|(i, w)| {
            (
                w.as_slice(),
                u8::try_from(128 + i).expect("dictionary code"),
            )
        })
        .collect();
    let mut bytes = Vec::with_capacity(text.len());
    for word in chunks(text) {
        if let Some(&code) = codes.get(word) {
            bytes.push(code);
        } else {
            for &b in word {
                if b >= 128 {
                    bytes.push(255);
                }
                bytes.push(b);
            }
        }
    }
    Packed { words, bytes }
}

/// Recover the exact source bytes. Input and dictionary are generated
/// together by `build.rs`, rather than read from an external archive.
#[cfg(any(test, target_family = "wasm"))]
pub fn decompress(mut bytes: &[u8], words: &[&[u8]], len: usize) -> Vec<u8> {
    let mut text = Vec::with_capacity(len);
    while !bytes.is_empty() {
        let literals = bytes.iter().position(|b| *b >= 128).unwrap_or(bytes.len());
        text.extend_from_slice(&bytes[..literals]);
        bytes = &bytes[literals..];
        let Some((&code, tail)) = bytes.split_first() else {
            break;
        };
        if code == 255 {
            text.push(tail[0]);
            bytes = &tail[1..];
        } else {
            text.extend_from_slice(words[usize::from(code - 128)]);
            bytes = tail;
        }
    }
    assert_eq!(text.len(), len, "embedded std text length");
    text
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests {
    use super::{compress, decompress};

    #[test]
    fn every_browser_std_file_round_trips_exactly() {
        let files: Vec<_> = crate::STD_FILES_WITHOUT_DOCS
            .iter()
            .filter(|(p, _)| !matches!(*p, "rt/checked.hd" | "rt/checked_report.hd"))
            .collect();
        let text: Vec<u8> = files
            .iter()
            .flat_map(|(_, s)| s.as_bytes())
            .copied()
            .collect();
        let packed = compress(&text);
        let words: Vec<_> = packed.words.iter().map(Vec::as_slice).collect();
        let decoded = decompress(&packed.bytes, &words, text.len());
        let mut offset = 0;
        for (path, original) in files {
            let end = offset + original.len();
            assert_eq!(&decoded[offset..end], original.as_bytes(), "{path}");
            offset = end;
        }
        assert!(packed.bytes.len() + packed.words.iter().map(Vec::len).sum::<usize>() < text.len());
    }

    #[test]
    fn literal_bytes_unicode_and_dictionary_codes_stay_distinct() {
        let mut text: Vec<u8> = (0..=255).collect();
        for i in 0..200 {
            let line = format!("\n    word_{i} λ 🙂 # x\n");
            for _ in 0..20 {
                text.extend_from_slice(line.as_bytes());
            }
        }
        let packed = compress(&text);
        assert_eq!(packed.words.len(), 127);
        let words: Vec<_> = packed.words.iter().map(Vec::as_slice).collect();
        assert_eq!(decompress(&packed.bytes, &words, text.len()), text);
        assert_eq!(
            compress(&text),
            packed,
            "frequency ties have a stable byte order"
        );
    }
}
