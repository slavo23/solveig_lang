//! Symbol identifiers and the shared string interner.
//!
//! The interner gives semantic code compact copyable ids while keeping source
//! names available for diagnostics and pretty-printing.

use crate::FxHashMap;

/// Interned name identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol(pub u32);

/// Bidirectional string-to-symbol table for a single compiler session.
#[derive(Clone, Debug, Default)]
pub struct Interner {
    strings: Vec<String>,
    symbols: FxHashMap<String, Symbol>,
}

impl Interner {
    /// Create an empty interner.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the symbol for `text`, inserting it when needed.
    pub fn intern(&mut self, text: &str) -> Symbol {
        if let Some(symbol) = self.symbols.get(text) {
            return *symbol;
        }

        let id = u32::try_from(self.strings.len())
            .expect("ICE: interned symbol table exceeded u32 capacity");
        let symbol = Symbol(id);

        self.strings.push(text.to_owned());
        self.symbols.insert(text.to_owned(), symbol);

        symbol
    }

    /// Resolve a symbol back to its interned text.
    pub fn str(&self, symbol: Symbol) -> &str {
        self.strings
            .get(symbol.0 as usize)
            .map(String::as_str)
            .expect("ICE: symbol not present in interner")
    }

    /// Number of distinct interned strings.
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Whether the interner contains no strings.
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_round_trips_and_deduplicates() {
        let mut interner = Interner::new();

        let alpha = interner.intern("alpha");
        let beta = interner.intern("beta");
        let alpha_again = interner.intern("alpha");

        assert_eq!(alpha, alpha_again);
        assert_ne!(alpha, beta);
        assert_eq!(interner.str(alpha), "alpha");
        assert_eq!(interner.str(beta), "beta");
        assert_eq!(interner.len(), 2);
    }

    #[test]
    fn interner_owns_many_strings() {
        let mut interner = Interner::new();
        let mut symbols = Vec::new();

        for index in 0..100_000 {
            symbols.push(interner.intern(&format!("symbol_{index}")));
        }

        assert_eq!(interner.len(), 100_000);
        assert_eq!(interner.str(symbols[0]), "symbol_0");
        assert_eq!(interner.str(symbols[99_999]), "symbol_99999");
    }
}
