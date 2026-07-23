use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol(pub u32);

#[derive(Debug, Default)]
pub struct Interner {
    strings: Vec<&'static str>,
    symbols: HashMap<&'static str, Symbol>,
}

impl Interner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, text: &str) -> Symbol {
        if let Some(symbol) = self.symbols.get(text) {
            return *symbol;
        }

        let id = u32::try_from(self.strings.len())
            .expect("ICE: interned symbol table exceeded u32 capacity");
        let interned: &'static str = Box::leak(text.to_owned().into_boxed_str());
        let symbol = Symbol(id);

        self.strings.push(interned);
        self.symbols.insert(interned, symbol);

        symbol
    }

    pub fn str(&self, symbol: Symbol) -> &'static str {
        self.strings
            .get(symbol.0 as usize)
            .copied()
            .expect("ICE: symbol not present in interner")
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

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
}
