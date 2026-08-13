//! Compiler-library location and embedded fallback sources.
//!
//! Package layout is selected by `solveig-package`; this module supplies the
//! compiler build's fallback root and the source bytes embedded in the binary.

use camino::Utf8Path;
use solveig_package::{LibraryLocation, resolve_library_location};

use crate::DriverError;

const EMBEDDED_LIBRARY_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../lib");
const PRELUDE_BUILTINS_SRC: &str = include_str!("../../../lib/prelude/src/prelude/builtins.sv");
const PRELUDE_LIST_SRC: &str = include_str!("../../../lib/prelude/src/prelude/list.sv");
const PRELUDE_PRIM_SRC: &str = include_str!("../../../lib/prelude/src/prelude/prim.sv");
const PRELUDE_TYPECLASSES_SRC: &str =
    include_str!("../../../lib/prelude/src/prelude/typeclasses.sv");

/// Resolves the library location associated with this compiler build.
pub fn compiler_library_location() -> Result<LibraryLocation, DriverError> {
    resolve_library_location(Utf8Path::new(EMBEDDED_LIBRARY_ROOT)).map_err(DriverError::from)
}

pub(crate) fn embedded_prelude_source(module: &str) -> Option<&'static str> {
    match module {
        "prelude.builtins" => Some(PRELUDE_BUILTINS_SRC),
        "prelude.list" => Some(PRELUDE_LIST_SRC),
        "prelude.prim" => Some(PRELUDE_PRIM_SRC),
        "prelude.typeclasses" => Some(PRELUDE_TYPECLASSES_SRC),
        _ => None,
    }
}
