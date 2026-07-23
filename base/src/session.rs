use std::cell::{Ref, RefCell};
use std::path::PathBuf;

use crate::source::{FileId, SourceMap};
use crate::symbol::{Interner, Symbol};
use crate::utils::diagnostics::DiagnosticSink;

#[derive(Debug, Default)]
pub struct Session {
    interner: RefCell<Interner>,
    source_map: RefCell<SourceMap>,
    diagnostics: DiagnosticSink,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&self, text: &str) -> Symbol {
        self.interner.borrow_mut().intern(text)
    }

    pub fn str(&self, symbol: Symbol) -> &'static str {
        self.interner.borrow().str(symbol)
    }

    pub fn add_file(&self, path: impl Into<PathBuf>, src: impl Into<String>) -> FileId {
        self.source_map.borrow_mut().add_file(path, src)
    }

    pub fn line_col(&self, file: FileId, offset: u32) -> (u32, u32) {
        self.source_map.borrow().line_col(file, offset)
    }

    pub fn source_map(&self) -> Ref<'_, SourceMap> {
        self.source_map.borrow()
    }

    pub fn diagnostics(&self) -> &DiagnosticSink {
        &self.diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_interns_with_shared_reference() {
        let session = Session::new();

        let one = session.intern("main");
        let two = session.intern("main");

        assert_eq!(one, two);
        assert_eq!(session.str(one), "main");
    }
}
