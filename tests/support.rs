use std::ffi::OsStr;
use std::ops::Deref;
use std::path::Path;

use tempfile::{Builder, TempDir};

pub struct TestRoot {
    dir: TempDir,
}

impl TestRoot {
    pub fn path(&self) -> &Path {
        self.dir.path()
    }
}

impl Deref for TestRoot {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        self.path()
    }
}

impl AsRef<Path> for TestRoot {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

impl AsRef<OsStr> for TestRoot {
    fn as_ref(&self) -> &OsStr {
        self.path().as_os_str()
    }
}

pub fn temp_root(prefix: &str, name: &str) -> TestRoot {
    let dir = Builder::new()
        .prefix(&format!("{prefix}_{name}_"))
        .tempdir()
        .expect("test temp directory should be creatable");
    TestRoot { dir }
}
