//! Source text access for driver project loading.
//! A compilation owns overlays so editor buffers can shadow or supply files without touching disk.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub(crate) struct SourceProvider {
    overlays: BTreeMap<PathBuf, String>,
}

impl SourceProvider {
    pub(crate) fn read_to_string(&self, path: &Path) -> io::Result<String> {
        if let Some(source) = self.overlays.get(&source_key(path)) {
            return Ok(source.clone());
        }

        fs::read_to_string(path)
    }

    pub(crate) fn has_source(&self, path: &Path) -> bool {
        self.overlays.contains_key(&source_key(path)) || path.is_file()
    }

    pub(crate) fn set_overlay(&mut self, path: impl AsRef<Path>, text: impl Into<String>) {
        self.overlays.insert(source_key(path.as_ref()), text.into());
    }

    pub(crate) fn clear_overlay(&mut self, path: impl AsRef<Path>) {
        self.overlays.remove(&source_key(path.as_ref()));
    }
}

fn source_key(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::ParentDir) | None => out.push(".."),
                Some(Component::Prefix(_) | Component::RootDir | Component::CurDir) => {}
            },
            other => out.push(other.as_os_str()),
        }
    }

    out
}
