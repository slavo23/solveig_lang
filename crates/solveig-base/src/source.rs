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

/// Names the code-unit encoding used for editor position columns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PositionEncoding {
    /// Counts position characters as UTF-8 code units.
    Utf8,
    /// Counts position characters as UTF-16 code units.
    #[default]
    Utf16,
}

/// Stores a zero-based source position for editor and LSP-facing APIs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourcePosition {
    /// Stores the zero-based line number.
    pub line: u32,
    /// Stores the zero-based character offset in the selected encoding.
    pub character: u32,
}

impl SourcePosition {
    /// Creates a zero-based source position.
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
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

    /// Converts a byte offset into a line/character position using the selected encoding.
    pub fn position(&self, offset: u32, encoding: PositionEncoding) -> SourcePosition {
        let offset = self.clamp_offset(offset);
        let line = self.line_index(offset);
        let text_end = self.line_text_end(line);
        let offset = offset.min(text_end);
        let character = character_for_offset(&self.src, self.line_start(line), offset, encoding);

        SourcePosition::new(u32::try_from(line).unwrap_or(u32::MAX), character)
    }

    /// Converts a line/character position into a byte offset using the selected encoding.
    pub fn offset(&self, position: SourcePosition, encoding: PositionEncoding) -> u32 {
        let line = usize::try_from(position.line)
            .unwrap_or(usize::MAX)
            .min(self.line_starts.len() - 1);
        let offset = offset_for_character(
            &self.src,
            self.line_start(line),
            self.line_text_end(line),
            position.character,
            encoding,
        );

        u32::try_from(offset).unwrap_or(u32::MAX)
    }

    fn clamp_offset(&self, offset: u32) -> usize {
        usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(self.src.len())
    }

    fn line_index(&self, offset: usize) -> usize {
        let offset = u32::try_from(offset).unwrap_or(u32::MAX);
        self.line_starts.partition_point(|start| *start <= offset) - 1
    }

    fn line_start(&self, line: usize) -> usize {
        self.line_starts[line] as usize
    }

    fn line_text_end(&self, line: usize) -> usize {
        let start = self.line_start(line);
        let mut end = self
            .line_starts
            .get(line + 1)
            .map_or(self.src.len(), |next| *next as usize);
        let bytes = self.src.as_bytes();

        if start < end && bytes[end - 1] == b'\n' {
            end -= 1;
            if start < end && bytes[end - 1] == b'\r' {
                end -= 1;
            }
        }

        end
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

    /// Converts a file byte offset into a line/character position.
    pub fn position(
        &self,
        file: FileId,
        offset: u32,
        encoding: PositionEncoding,
    ) -> SourcePosition {
        self.lookup(file).position(offset, encoding)
    }

    /// Converts a file line/character position into a byte offset.
    pub fn offset(
        &self,
        file: FileId,
        position: SourcePosition,
        encoding: PositionEncoding,
    ) -> u32 {
        self.lookup(file).offset(position, encoding)
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

fn character_for_offset(src: &str, start: usize, offset: usize, encoding: PositionEncoding) -> u32 {
    match encoding {
        PositionEncoding::Utf8 => u32::try_from(offset.saturating_sub(start)).unwrap_or(u32::MAX),
        PositionEncoding::Utf16 => {
            let mut units = 0u32;

            // LSP 3.17 `PositionEncodingKind` defines UTF-16 positions in code units.
            for (rel, ch) in src[start..].char_indices() {
                let char_end = start + rel + ch.len_utf8();
                if offset < char_end {
                    break;
                }
                units = units.saturating_add(ch.len_utf16() as u32);
            }

            units
        }
    }
}

fn offset_for_character(
    src: &str,
    start: usize,
    end: usize,
    character: u32,
    encoding: PositionEncoding,
) -> usize {
    match encoding {
        PositionEncoding::Utf8 => start.saturating_add(character as usize).min(end),
        PositionEncoding::Utf16 => {
            let mut units = 0u32;

            // LSP 3.17 `PositionEncodingKind` defines UTF-16 positions in code units.
            for (rel, ch) in src[start..end].char_indices() {
                let char_start = start + rel;
                let char_end = char_start + ch.len_utf8();
                let next_units = units.saturating_add(ch.len_utf16() as u32);

                if character < next_units {
                    return char_start;
                }
                if character == next_units {
                    return char_end;
                }

                units = next_units;
            }

            end
        }
    }
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

    #[test]
    fn positions_count_ascii_offsets_the_same_in_both_encodings() {
        let mut source_map = SourceMap::new();
        let file = source_map.add_file("main.sv", "abc");

        assert_eq!(
            source_map.position(file, 2, PositionEncoding::Utf8),
            SourcePosition::new(0, 2)
        );
        assert_eq!(
            source_map.position(file, 2, PositionEncoding::Utf16),
            SourcePosition::new(0, 2)
        );
        assert_eq!(
            source_map.offset(file, SourcePosition::new(0, 2), PositionEncoding::Utf8),
            2
        );
        assert_eq!(
            source_map.offset(file, SourcePosition::new(0, 2), PositionEncoding::Utf16),
            2
        );
    }

    #[test]
    fn positions_count_unicode_units_per_encoding() {
        let mut source_map = SourceMap::new();
        let file = source_map.add_file("unicode.sv", "aé中🎉z");

        assert_eq!(
            source_map.position(file, 3, PositionEncoding::Utf8),
            SourcePosition::new(0, 3)
        );
        assert_eq!(
            source_map.position(file, 3, PositionEncoding::Utf16),
            SourcePosition::new(0, 2)
        );
        assert_eq!(
            source_map.position(file, 6, PositionEncoding::Utf16),
            SourcePosition::new(0, 3)
        );
        assert_eq!(
            source_map.position(file, 10, PositionEncoding::Utf16),
            SourcePosition::new(0, 5)
        );
        assert_eq!(
            source_map.position(file, 11, PositionEncoding::Utf16),
            SourcePosition::new(0, 6)
        );
        assert_eq!(
            source_map.offset(file, SourcePosition::new(0, 5), PositionEncoding::Utf16),
            10
        );
        assert_eq!(
            source_map.offset(file, SourcePosition::new(0, 11), PositionEncoding::Utf8),
            11
        );
    }

    #[test]
    fn positions_handle_line_and_file_edges() {
        let mut source_map = SourceMap::new();
        let multiline = source_map.add_file("multiline.sv", "ab\nc");
        let trailing_newline = source_map.add_file("trailing.sv", "ab\n");
        let empty = source_map.add_file("empty.sv", "");

        assert_eq!(
            source_map.position(multiline, 2, PositionEncoding::Utf16),
            SourcePosition::new(0, 2)
        );
        assert_eq!(
            source_map.position(multiline, 3, PositionEncoding::Utf16),
            SourcePosition::new(1, 0)
        );
        assert_eq!(
            source_map.position(multiline, 4, PositionEncoding::Utf16),
            SourcePosition::new(1, 1)
        );
        assert_eq!(
            source_map.position(trailing_newline, 3, PositionEncoding::Utf16),
            SourcePosition::new(1, 0)
        );
        assert_eq!(
            source_map.position(empty, 0, PositionEncoding::Utf16),
            SourcePosition::new(0, 0)
        );
        assert_eq!(
            source_map.offset(
                multiline,
                SourcePosition::new(99, 99),
                PositionEncoding::Utf16
            ),
            4
        );
        assert_eq!(
            source_map.offset(empty, SourcePosition::new(99, 99), PositionEncoding::Utf16),
            0
        );
    }

    #[test]
    fn positions_treat_crlf_as_one_line_separator() {
        let mut source_map = SourceMap::new();
        let file = source_map.add_file("crlf.sv", "ab\r\nc");

        assert_eq!(
            source_map.position(file, 2, PositionEncoding::Utf16),
            SourcePosition::new(0, 2)
        );
        assert_eq!(
            source_map.position(file, 3, PositionEncoding::Utf16),
            SourcePosition::new(0, 2)
        );
        assert_eq!(
            source_map.position(file, 4, PositionEncoding::Utf16),
            SourcePosition::new(1, 0)
        );
        assert_eq!(
            source_map.offset(file, SourcePosition::new(0, 2), PositionEncoding::Utf16),
            2
        );
        assert_eq!(
            source_map.offset(file, SourcePosition::new(0, 99), PositionEncoding::Utf16),
            2
        );
        assert_eq!(
            source_map.offset(file, SourcePosition::new(1, 0), PositionEncoding::Utf16),
            4
        );
    }

    #[test]
    fn positions_round_trip_character_boundary_offsets() {
        let mut source_map = SourceMap::new();
        let src = "aé\n中🎉z";
        let file = source_map.add_file("roundtrip.sv", src);

        for encoding in [PositionEncoding::Utf8, PositionEncoding::Utf16] {
            for offset in src
                .char_indices()
                .map(|(offset, _)| offset)
                .chain(std::iter::once(src.len()))
            {
                let offset = u32::try_from(offset).expect("test fixture offset fits in u32");
                let position = source_map.position(file, offset, encoding);

                assert_eq!(
                    source_map.offset(file, position, encoding),
                    offset,
                    "round-trip failed for {encoding:?} at {position:?}"
                );
            }
        }
    }
}
