//! Source file storage, file identifiers, and byte spans.
//! Compiler stages use these ids to preserve locations without owning source text.

use std::ops::Range;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub fn from_usize_range(range: Range<usize>) -> Self {
        let start =
            u32::try_from(range.start).expect("ICE: source span start exceeded u32 capacity");
        let end = u32::try_from(range.end).expect("ICE: source span end exceeded u32 capacity");

        Self { start, end }
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub fn to_usize_range(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileSpan {
    pub file: FileId,
    pub span: Span,
}

impl FileSpan {
    pub fn new(file: FileId, span: Span) -> Self {
        Self { file, span }
    }

    pub fn is_empty(self) -> bool {
        self.span.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct SourceFile {
    pub path: PathBuf,
    pub src: String,
    pub line_starts: Vec<u32>,
}

impl SourceFile {
    pub fn new(path: impl Into<PathBuf>, src: impl Into<String>) -> Self {
        let src = src.into();
        let line_starts = line_starts(&src);

        Self {
            path: path.into(),
            src,
            line_starts,
        }
    }

    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let max_offset = u32::try_from(self.src.len()).unwrap_or(u32::MAX);
        let offset = offset.min(max_offset);
        let line = self.line_starts.partition_point(|start| *start <= offset) - 1;
        let line_start = self.line_starts[line];

        (line as u32, offset - line_start)
    }
}

#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_file(&mut self, path: impl Into<PathBuf>, src: impl Into<String>) -> FileId {
        let id = u32::try_from(self.files.len())
            .expect("ICE: source map file table exceeded u32 capacity");

        self.files.push(SourceFile::new(path, src));
        FileId(id)
    }

    pub fn lookup(&self, file: FileId) -> &SourceFile {
        self.files
            .get(file.0 as usize)
            .expect("ICE: FileId not present in SourceMap")
    }

    pub fn line_col(&self, file: FileId, offset: u32) -> (u32, u32) {
        self.lookup(file).line_col(offset)
    }

    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

fn line_starts(src: &str) -> Vec<u32> {
    let mut starts = vec![0];

    for (idx, byte) in src.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(u32::try_from(idx + 1).unwrap_or(u32::MAX));
        }
    }

    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_handles_multiline_sources_and_eof() {
        let mut source_map = SourceMap::new();
        let file = source_map.add_file("main.sv", "first\nsecond\nthird");

        assert_eq!(source_map.line_col(file, 0), (0, 0));
        assert_eq!(source_map.line_col(file, 5), (0, 5));
        assert_eq!(source_map.line_col(file, 6), (1, 0));
        assert_eq!(source_map.line_col(file, 12), (1, 6));
        assert_eq!(source_map.line_col(file, 13), (2, 0));
        assert_eq!(source_map.line_col(file, 18), (2, 5));
    }

    #[test]
    fn line_col_handles_trailing_newline_eof() {
        let mut source_map = SourceMap::new();
        let file = source_map.add_file("main.sv", "x\n");

        assert_eq!(source_map.line_col(file, 0), (0, 0));
        assert_eq!(source_map.line_col(file, 1), (0, 1));
        assert_eq!(source_map.line_col(file, 2), (1, 0));
    }
}
