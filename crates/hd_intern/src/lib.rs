#![forbid(unsafe_code)]

use std::hash::BuildHasher;
use std::ops::Range;

use hd_base::HashMap;

use hd_base::Symbol;

pub mod paths;
pub use paths::{ImplSeg, PathKind, PathTable, ShardedInterner, StablePath};

#[derive(Clone, Debug, Default)]
pub struct Interner {
    by_hash: HashMap<u64, Symbol>,
    arena: String,
    text: Vec<InternedText>,
}

#[derive(Clone, Debug)]
struct InternedText {
    range: Range<usize>,
    next: Option<Symbol>,
}

impl Interner {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, text: &str) -> Symbol {
        self.intern_hashed(text, self.by_hash.hasher().hash_one(text))
    }

    fn intern_hashed(&mut self, text: &str, hash: u64) -> Symbol {
        let head = self.by_hash.get(&hash).copied();
        let mut current = head;
        while let Some(symbol) = current {
            let entry = &self.text[symbol.idx()];
            if &self.arena[entry.range.clone()] == text {
                return symbol;
            }
            current = entry.next;
        }
        let symbol = Symbol::from_raw(
            self.text
                .len()
                .try_into()
                .expect("symbol table exceeds u32"),
        );
        let start = self.arena.len();
        self.arena.push_str(text);
        self.text.push(InternedText {
            range: start..self.arena.len(),
            next: head,
        });
        self.by_hash.insert(hash, symbol);
        symbol
    }

    #[must_use]
    pub fn resolve(&self, symbol: Symbol) -> Option<&str> {
        self.text
            .get(symbol.idx())
            .map(|entry| &self.arena[entry.range.clone()])
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.text.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::Interner;
    use hd_base::Symbol;

    #[test]
    fn equal_text_has_equal_identity() {
        let mut interner = Interner::new();
        let first = interner.intern("answer");
        let second = interner.intern("answer");
        assert_eq!(first, second);
        assert_eq!(interner.resolve(first), Some("answer"));
    }

    #[test]
    fn invalid_symbol_is_not_resolved() {
        let mut interner = Interner::new();
        assert!(interner.is_empty());
        assert_eq!(interner.resolve(Symbol::from_raw(0)), None);
        interner.intern("one");
        assert!(!interner.is_empty());
        assert_eq!(interner.resolve(Symbol::from_raw(1)), None);
        assert_eq!(interner.resolve(Symbol::from_raw(u32::MAX - 1)), None);
    }

    #[test]
    fn hash_collisions_keep_distinct_text_and_reuse_symbols() {
        let mut interner = Interner::new();
        let empty = interner.intern_hashed("", 0);
        let first = interner.intern_hashed("first", 0);
        let second = interner.intern_hashed("éλ😀", 0);
        assert_ne!(first, second);
        assert_eq!(interner.intern_hashed("first", 0), first);
        assert_eq!(interner.intern_hashed("éλ😀", 0), second);
        assert_eq!(interner.intern_hashed("", 0), empty);
        assert_eq!(interner.resolve(first), Some("first"));
        assert_eq!(interner.resolve(second), Some("éλ😀"));
        assert_eq!(interner.resolve(empty), Some(""));
        assert_eq!(interner.len(), 3);
    }

    #[test]
    fn arena_growth_keeps_empty_and_unicode_symbols_valid() {
        let mut interner = Interner::new();
        let empty = interner.intern("");
        let unicode = interner.intern("éλ😀");
        for index in 0..1024 {
            interner.intern(&format!("name_{index}"));
        }
        assert_eq!(interner.resolve(empty), Some(""));
        assert_eq!(interner.resolve(unicode), Some("éλ😀"));
        assert_eq!(interner.intern(""), empty);
        assert_eq!(interner.intern("éλ😀"), unicode);
        assert_eq!(interner.len(), 1026);
    }

    #[test]
    fn clones_preserve_symbols_and_own_their_arena() {
        let mut original = Interner::new();
        let shared = original.intern("shared");
        let mut cloned = original.clone();
        original.intern("original");
        cloned.intern("cloned");
        assert_eq!(original.resolve(shared), Some("shared"));
        assert_eq!(cloned.resolve(shared), Some("shared"));
        assert_eq!(cloned.intern("shared"), shared);
        assert_eq!(original.arena, "sharedoriginal");
        assert_eq!(cloned.arena, "sharedcloned");
    }
}
