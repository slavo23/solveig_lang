//! Conversion from driver semantic tokens into LSP semantic-token payloads.

use async_lsp::lsp_types::{
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokensLegend,
};
use solveig_base::{PositionEncoding, SourceFile, SourceMap};
use solveig_ide::{SourceSemanticToken, SourceSemanticTokenKind, SourceSemanticTokenModifier};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AbsoluteToken {
    line: u32,
    start: u32,
    end: u32,
    token_type: u32,
    token_modifiers_bitset: u32,
}

pub(crate) fn semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::NAMESPACE,
            SemanticTokenType::TYPE,
            SemanticTokenType::CLASS,
            SemanticTokenType::TYPE_PARAMETER,
            SemanticTokenType::PARAMETER,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::FUNCTION,
            SemanticTokenType::ENUM_MEMBER,
            SemanticTokenType::METHOD,
            SemanticTokenType::KEYWORD,
            SemanticTokenType::OPERATOR,
            SemanticTokenType::STRING,
            SemanticTokenType::NUMBER,
            SemanticTokenType::COMMENT,
        ],
        token_modifiers: vec![
            SemanticTokenModifier::DECLARATION,
            SemanticTokenModifier::DEFINITION,
            SemanticTokenModifier::READONLY,
            SemanticTokenModifier::DEFAULT_LIBRARY,
        ],
    }
}

pub(crate) fn lsp_semantic_tokens(
    tokens: &[SourceSemanticToken],
    source_map: &SourceMap,
    encoding: PositionEncoding,
) -> Vec<SemanticToken> {
    let mut absolute = Vec::new();

    for token in tokens {
        push_absolute_tokens(*token, source_map, encoding, &mut absolute);
    }

    absolute.sort_by_key(|token| {
        (
            token.line,
            token.start,
            token.end,
            token.token_type,
            token.token_modifiers_bitset,
        )
    });
    encode_relative_tokens(&absolute)
}

fn push_absolute_tokens(
    token: SourceSemanticToken,
    source_map: &SourceMap,
    encoding: PositionEncoding,
    out: &mut Vec<AbsoluteToken>,
) {
    let span = token.file_span();
    let Some(source_file) = source_map.files().get(span.file.0 as usize) else {
        return;
    };
    let start = (span.span.start as usize).min(source_file.src.len());
    let end = (span.span.end as usize).min(source_file.src.len());
    let mut offset = start;

    while offset < end {
        let line = line_index(source_file, offset);
        let line_end = line_text_end(source_file, line);
        let segment_end = end.min(line_end);

        if segment_end > offset {
            push_absolute_segment(token, source_file, offset, segment_end, encoding, out);
        }

        if segment_end >= end {
            break;
        }

        let Some(next_line_start) = source_file.line_starts.get(line + 1) else {
            break;
        };
        offset = (*next_line_start as usize).min(end);
    }
}

fn push_absolute_segment(
    token: SourceSemanticToken,
    source_file: &SourceFile,
    start_offset: usize,
    end_offset: usize,
    encoding: PositionEncoding,
    out: &mut Vec<AbsoluteToken>,
) {
    let start = source_file.position(start_offset as u32, encoding);
    let end = source_file.position(end_offset as u32, encoding);

    if start.line != end.line || end.character <= start.character {
        return;
    }

    out.push(AbsoluteToken {
        line: start.line,
        start: start.character,
        end: end.character,
        token_type: token_type_index(token.kind()),
        token_modifiers_bitset: modifier_bitset(token),
    });
}

fn encode_relative_tokens(tokens: &[AbsoluteToken]) -> Vec<SemanticToken> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut previous_line = 0u32;
    let mut previous_start = 0u32;

    for (index, token) in tokens.iter().enumerate() {
        let delta_line = if index == 0 {
            token.line
        } else {
            token.line.saturating_sub(previous_line)
        };
        let delta_start = if index == 0 || delta_line > 0 {
            token.start
        } else {
            token.start.saturating_sub(previous_start)
        };

        out.push(SemanticToken {
            delta_line,
            delta_start,
            length: token.end.saturating_sub(token.start),
            token_type: token.token_type,
            token_modifiers_bitset: token.token_modifiers_bitset,
        });

        previous_line = token.line;
        previous_start = token.start;
    }

    out
}

fn token_type_index(kind: SourceSemanticTokenKind) -> u32 {
    match kind {
        SourceSemanticTokenKind::Namespace => 0,
        SourceSemanticTokenKind::Type => 1,
        SourceSemanticTokenKind::Class => 2,
        SourceSemanticTokenKind::TypeParameter => 3,
        SourceSemanticTokenKind::Parameter => 4,
        SourceSemanticTokenKind::Variable => 5,
        SourceSemanticTokenKind::Function => 6,
        SourceSemanticTokenKind::EnumMember => 7,
        SourceSemanticTokenKind::Method => 8,
        SourceSemanticTokenKind::Keyword => 9,
        SourceSemanticTokenKind::Operator => 10,
        SourceSemanticTokenKind::String => 11,
        SourceSemanticTokenKind::Number => 12,
        SourceSemanticTokenKind::Comment => 13,
    }
}

fn modifier_bitset(token: SourceSemanticToken) -> u32 {
    token
        .modifiers()
        .iter()
        .map(modifier_index)
        .fold(0u32, |bits, index| bits | (1 << index))
}

fn modifier_index(modifier: SourceSemanticTokenModifier) -> u32 {
    match modifier {
        SourceSemanticTokenModifier::Declaration => 0,
        SourceSemanticTokenModifier::Definition => 1,
        SourceSemanticTokenModifier::Readonly => 2,
        SourceSemanticTokenModifier::DefaultLibrary => 3,
    }
}

fn line_index(source_file: &SourceFile, offset: usize) -> usize {
    source_file
        .line_starts
        .partition_point(|start| (*start as usize) <= offset)
        .saturating_sub(1)
}

fn line_text_end(source_file: &SourceFile, line: usize) -> usize {
    let start = source_file.line_starts[line] as usize;
    let mut end = source_file
        .line_starts
        .get(line + 1)
        .map_or(source_file.src.len(), |next| *next as usize);
    let bytes = source_file.src.as_bytes();

    if start < end && bytes[end - 1] == b'\n' {
        end -= 1;
        if start < end && bytes[end - 1] == b'\r' {
            end -= 1;
        }
    }

    end
}

#[cfg(test)]
mod tests {
    use super::*;
    use solveig_base::{FileSpan, SourceMap, Span};
    use solveig_ide::SourceSemanticTokenModifiers;

    #[test]
    fn lsp_tokens_are_relative_and_split_by_line() {
        let mut source_map = SourceMap::new();
        let file = source_map.add_file("main.sv", "one\n/*a\nb*/\nthree");
        let tokens = vec![SourceSemanticToken::new(
            FileSpan::new(file, Span::new(4, 11)),
            SourceSemanticTokenKind::Comment,
            SourceSemanticTokenModifiers::empty(),
        )];

        let encoded = lsp_semantic_tokens(&tokens, &source_map, PositionEncoding::Utf16);

        assert_eq!(encoded.len(), 2);
        assert_eq!(encoded[0].delta_line, 1);
        assert_eq!(encoded[0].delta_start, 0);
        assert_eq!(encoded[0].length, 3);
        assert_eq!(encoded[1].delta_line, 1);
        assert_eq!(encoded[1].delta_start, 0);
        assert_eq!(encoded[1].length, 3);
    }
}
