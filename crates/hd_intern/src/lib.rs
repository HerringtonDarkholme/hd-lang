#![forbid(unsafe_code)]

use std::collections::HashMap;

use hd_base::Symbol;

pub mod paths;
pub use paths::{ImplSeg, PathKind, PathTable, ShardedInterner, StablePath};

#[derive(Clone, Debug, Default)]
pub struct Interner {
    by_text: HashMap<String, Symbol>,
    text: Vec<String>,
}

impl Interner {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, text: &str) -> Symbol {
        if let Some(symbol) = self.by_text.get(text) {
            return *symbol;
        }
        let symbol = Symbol::from_raw(
            self.text
                .len()
                .try_into()
                .expect("symbol table exceeds u32"),
        );
        let owned = text.to_owned();
        self.text.push(owned.clone());
        self.by_text.insert(owned, symbol);
        symbol
    }

    #[must_use]
    pub fn resolve(&self, symbol: Symbol) -> Option<&str> {
        self.text.get(symbol.idx()).map(String::as_str)
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

    #[test]
    fn equal_text_has_equal_identity() {
        let mut interner = Interner::new();
        let first = interner.intern("answer");
        let second = interner.intern("answer");
        assert_eq!(first, second);
        assert_eq!(interner.resolve(first), Some("answer"));
    }
}
