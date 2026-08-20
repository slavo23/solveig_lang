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
const PRELUDE_FUNCTION_SRC: &str = include_str!("../../../lib/prelude/src/prelude/function.sv");
const PRELUDE_CONTROL_SRC: &str = include_str!("../../../lib/prelude/src/prelude/control.sv");
const PRELUDE_SHOW_SRC: &str = include_str!("../../../lib/prelude/src/prelude/show.sv");
const PRELUDE_INSTANCES_SRC: &str = include_str!("../../../lib/prelude/src/prelude/instances.sv");
const STD_BYTES_SRC: &str = include_str!("../../../lib/std/src/std/bytes.sv");
const STD_CHANNEL_SRC: &str = include_str!("../../../lib/std/src/std/channel.sv");
const STD_ENV_SRC: &str = include_str!("../../../lib/std/src/std/env.sv");
const STD_FS_SRC: &str = include_str!("../../../lib/std/src/std/fs.sv");
const STD_IO_SRC: &str = include_str!("../../../lib/std/src/std/io.sv");
const STD_LIST_SRC: &str = include_str!("../../../lib/std/src/std/list.sv");
const STD_NET_SRC: &str = include_str!("../../../lib/std/src/std/net.sv");
const STD_PARSER_SRC: &str = include_str!("../../../lib/std/src/std/parser.sv");
const STD_PROCESS_SRC: &str = include_str!("../../../lib/std/src/std/process.sv");
const STD_RANDOM_SRC: &str = include_str!("../../../lib/std/src/std/random.sv");
const STD_TASK_SRC: &str = include_str!("../../../lib/std/src/std/task.sv");
const STD_TEXT_SRC: &str = include_str!("../../../lib/std/src/std/text.sv");
const STD_THREAD_SRC: &str = include_str!("../../../lib/std/src/std/thread.sv");
const STD_TIME_SRC: &str = include_str!("../../../lib/std/src/std/time.sv");
const PRELUDE_MANIFEST: &str = include_str!("../../../lib/prelude/solveig.toml");
const STD_MANIFEST: &str = include_str!("../../../lib/std/solveig.toml");

/// Resolves the library location associated with this compiler build.
pub fn compiler_library_location() -> Result<LibraryLocation, DriverError> {
    resolve_library_location(Utf8Path::new(EMBEDDED_LIBRARY_ROOT)).map_err(DriverError::from)
}

pub(crate) fn embedded_library_source(module: &str) -> Option<&'static str> {
    match module {
        "prelude.builtins" => Some(PRELUDE_BUILTINS_SRC),
        "prelude.list" => Some(PRELUDE_LIST_SRC),
        "prelude.prim" => Some(PRELUDE_PRIM_SRC),
        "prelude.typeclasses" => Some(PRELUDE_TYPECLASSES_SRC),
        "prelude.function" => Some(PRELUDE_FUNCTION_SRC),
        "prelude.control" => Some(PRELUDE_CONTROL_SRC),
        "prelude.show" => Some(PRELUDE_SHOW_SRC),
        "prelude.instances" => Some(PRELUDE_INSTANCES_SRC),
        "std.bytes" => Some(STD_BYTES_SRC),
        "std.channel" => Some(STD_CHANNEL_SRC),
        "std.env" => Some(STD_ENV_SRC),
        "std.fs" => Some(STD_FS_SRC),
        "std.io" => Some(STD_IO_SRC),
        "std.list" => Some(STD_LIST_SRC),
        "std.net" => Some(STD_NET_SRC),
        "std.parser" => Some(STD_PARSER_SRC),
        "std.process" => Some(STD_PROCESS_SRC),
        "std.random" => Some(STD_RANDOM_SRC),
        "std.task" => Some(STD_TASK_SRC),
        "std.text" => Some(STD_TEXT_SRC),
        "std.thread" => Some(STD_THREAD_SRC),
        "std.time" => Some(STD_TIME_SRC),
        _ => None,
    }
}

pub(crate) fn embedded_library_manifest(package: &str) -> Option<&'static str> {
    match package {
        "prelude" => Some(PRELUDE_MANIFEST),
        "std" => Some(STD_MANIFEST),
        _ => None,
    }
}

pub(crate) fn embedded_library_modules(package: &str) -> &'static [&'static str] {
    match package {
        "prelude" => &[
            "prelude/builtins.sv",
            "prelude/list.sv",
            "prelude/prim.sv",
            "prelude/typeclasses.sv",
            "prelude/function.sv",
            "prelude/control.sv",
            "prelude/show.sv",
            "prelude/instances.sv",
        ],
        "std" => &[
            "std/bytes.sv",
            "std/channel.sv",
            "std/env.sv",
            "std/fs.sv",
            "std/io.sv",
            "std/list.sv",
            "std/net.sv",
            "std/parser.sv",
            "std/process.sv",
            "std/random.sv",
            "std/task.sv",
            "std/text.sv",
            "std/thread.sv",
            "std/time.sv",
        ],
        _ => &[],
    }
}
