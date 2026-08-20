//! Tokenization for Solveig source text.
//! The parser consumes the significant tokens; later CST phases keep trivia.

use logos::Logos;
use text_size::{TextRange, TextSize};

use solveig_base::{FileId, FileSpan, Span, ice};

#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
enum RawToken {
    // Keywords (must appear before identifiers).
    #[token("let")]
    KwLet,
    #[token("in")]
    KwIn,
    #[token("do")]
    KwDo,
    #[token("fun")]
    KwFun,
    #[token("forall")]
    KwForall,
    #[token("case")]
    KwCase,
    #[token("of")]
    KwOf,
    #[token("if")]
    KwIf,
    #[token("then")]
    KwThen,
    #[token("else")]
    KwElse,
    #[token("data")]
    KwData,
    #[token("type")]
    KwType,
    #[token("sig")]
    KwSig,
    #[token("class")]
    KwClass,
    #[token("instance")]
    KwInstance,
    #[token("where")]
    KwWhere,
    #[token("default")]
    KwDefault,
    #[token("import")]
    KwImport,
    #[token("as")]
    KwAs,
    #[token("infixl")]
    KwInfixl,
    #[token("infixr")]
    KwInfixr,
    #[token("infix")]
    KwInfix,

    // Single-character punctuation.
    #[token(";")]
    Semicolon,
    #[token(",")]
    Comma,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBrack,
    #[token("]")]
    RBrack,
    #[token("_", priority = 3)]
    Underscore,
    #[token("`")]
    Backtick,

    #[regex(r"[0-9]+((\.[0-9]+)([eE][+-]?[0-9]+)?|[eE][+-]?[0-9]+)")]
    Float,

    #[regex(r#"'([^'\\\n]|\\(n|t|\\|'|"|u\{[0-9a-fA-F]{1,6}\}))'"#)]
    Char,

    #[regex(r#""([^"\\\n]|\\(n|t|\\|'|"|u\{[0-9a-fA-F]{1,6}\}))*""#)]
    Str,

    #[regex(r"[0-9]+")]
    Int,

    #[regex(r"[A-Z][a-zA-Z0-9_']*")]
    UpperIdent,

    #[regex(r"[a-z_][a-zA-Z0-9_']*")]
    Ident,

    #[regex(r"[!#$%&*+\./<=>?@\\\^\|\-~:]+")]
    Operator,
}

// -- Positioned tokens ------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PosToken {
    pub kind: Token,
    pub range: TextRange,
    pub line: u32,
    pub col: u32,
    synthetic: bool,
}

impl PosToken {
    fn new(kind: Token, start: usize, end: usize, line: u32, col: u32) -> Self {
        Self {
            kind,
            range: text_range(start, end),
            line,
            col,
            synthetic: false,
        }
    }

    fn virtual_at(kind: Token, anchor: &Self) -> Self {
        let anchor_start = anchor.range.start();
        Self {
            kind,
            range: TextRange::new(anchor_start, anchor_start),
            line: anchor.line,
            col: anchor.col,
            synthetic: true,
        }
    }

    pub fn is_synthetic(&self) -> bool {
        self.synthetic
    }

    pub fn start(&self) -> usize {
        text_size_to_usize(self.range.start())
    }

    pub fn end(&self) -> usize {
        text_size_to_usize(self.range.end())
    }

    pub fn source_text<'a>(&self, source: &'a str) -> &'a str {
        if self.synthetic {
            ""
        } else {
            source
                .get(self.start()..self.end())
                .expect("ICE: token span must split source at UTF-8 boundaries")
        }
    }
}

fn text_range(start: usize, end: usize) -> TextRange {
    TextRange::new(text_size(start), text_size(end))
}

fn text_size(offset: usize) -> TextSize {
    let offset = u32::try_from(offset).expect("ICE: token offset exceeded u32 capacity");
    TextSize::from(offset)
}

fn text_size_to_usize(offset: TextSize) -> usize {
    u32::from(offset) as usize
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Int,
    Float,
    Char,
    Str,
    Ident,
    UpperIdent,

    Whitespace,
    Newline,
    LineComment,
    BlockComment,

    Let,
    In,
    Do,
    Fun,
    Forall,
    Case,
    Of,
    If,
    Then,
    Else,
    Data,
    Type,
    Sig,
    Class,
    Instance,
    Where,
    Default,
    Import,
    As,
    Infixl,
    Infixr,
    Infix,

    Eq,
    Colon,
    Arrow,
    BindArrow,
    FatArrow,
    Cons,
    Pipe,
    Dot,
    Comma,
    LParen,
    RParen,
    LBrack,
    RBrack,
    Minus,
    Hole,
    Underscore,
    Semicolon,
    Backtick,
    Operator,
    ReservedOp,

    VOpen,
    VClose,
    VSemi,

    Eof,
}

impl Token {
    pub fn is_trivia(&self) -> bool {
        matches!(
            self,
            Token::Whitespace | Token::Newline | Token::LineComment | Token::BlockComment
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub span: FileSpan,
    pub line: u32,
    pub col: u32,
    pub message: String,
}

fn compute_line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

fn offset_to_line_col(line_starts: &[usize], offset: usize) -> (u32, u32) {
    let line_ix = line_starts
        .partition_point(|&start| start <= offset)
        .saturating_sub(1);
    let line_start = *line_starts
        .get(line_ix)
        .expect("ICE: lexer line table must contain current line");
    let col = offset - line_start;
    (
        u32::try_from(line_ix).expect("ICE: lexer line index exceeded u32 capacity"),
        u32::try_from(col).expect("ICE: lexer column exceeded u32 capacity"),
    )
}

fn lex_error(
    source: &str,
    file: FileId,
    offset: usize,
    line: u32,
    col: u32,
    end: usize,
) -> LexError {
    LexError {
        span: FileSpan::new(file, Span::from_usize_range(offset..end)),
        line,
        col,
        message: format!(
            "unexpected character '{}'",
            source
                .get(offset..end)
                .expect("ICE: lexer error span must split source at UTF-8 boundaries")
        ),
    }
}

fn trivia_at(source: &str, offset: usize) -> Option<(Token, usize)> {
    let bytes = source.as_bytes();
    let byte = *bytes.get(offset)?;

    match byte {
        b' ' | b'\t' | b'\r' => {
            let mut end = offset + 1;
            while bytes
                .get(end)
                .is_some_and(|byte| matches!(*byte, b' ' | b'\t' | b'\r'))
            {
                end += 1;
            }
            Some((Token::Whitespace, end))
        }
        b'\n' => {
            let mut end = offset + 1;
            while bytes.get(end).is_some_and(|byte| *byte == b'\n') {
                end += 1;
            }
            Some((Token::Newline, end))
        }
        b'-' if bytes.get(offset + 1) == Some(&b'-') => {
            let end = source
                .get(offset..)?
                .find('\n')
                .map_or(bytes.len(), |line_end| offset + line_end);
            Some((Token::LineComment, end))
        }
        b'{' if bytes.get(offset + 1) == Some(&b'-') => {
            Some((Token::BlockComment, block_comment_end(source, offset)))
        }
        _ => None,
    }
}

fn block_comment_end(source: &str, start: usize) -> usize {
    let bytes = source.as_bytes();
    let mut depth = 1u32;
    let mut offset = start + 2;

    while offset + 1 < bytes.len() {
        let current = bytes
            .get(offset)
            .expect("ICE: block comment scan offset must be in bounds");
        let next = bytes
            .get(offset + 1)
            .expect("ICE: block comment lookahead must be in bounds");
        if *current == b'{' && *next == b'-' {
            depth += 1;
            offset += 2;
        } else if *current == b'-' && *next == b'}' {
            depth -= 1;
            offset += 2;
            if depth == 0 {
                return offset;
            }
        } else {
            offset += 1;
        }
    }

    bytes.len()
}

fn map_raw_token(raw: RawToken, slice: &str) -> Token {
    match raw {
        RawToken::Int => Token::Int,
        RawToken::Float => Token::Float,
        RawToken::Char => Token::Char,
        RawToken::Str => Token::Str,
        RawToken::Ident => Token::Ident,
        RawToken::UpperIdent => Token::UpperIdent,

        RawToken::KwLet => Token::Let,
        RawToken::KwIn => Token::In,
        RawToken::KwDo => Token::Do,
        RawToken::KwFun => Token::Fun,
        RawToken::KwForall => Token::Forall,
        RawToken::KwCase => Token::Case,
        RawToken::KwOf => Token::Of,
        RawToken::KwIf => Token::If,
        RawToken::KwThen => Token::Then,
        RawToken::KwElse => Token::Else,
        RawToken::KwData => Token::Data,
        RawToken::KwType => Token::Type,
        RawToken::KwSig => Token::Sig,
        RawToken::KwClass => Token::Class,
        RawToken::KwInstance => Token::Instance,
        RawToken::KwWhere => Token::Where,
        RawToken::KwDefault => Token::Default,
        RawToken::KwImport => Token::Import,
        RawToken::KwAs => Token::As,
        RawToken::KwInfixl => Token::Infixl,
        RawToken::KwInfixr => Token::Infixr,
        RawToken::KwInfix => Token::Infix,

        RawToken::Comma => Token::Comma,
        RawToken::LParen => Token::LParen,
        RawToken::RParen => Token::RParen,
        RawToken::LBrack => Token::LBrack,
        RawToken::RBrack => Token::RBrack,
        RawToken::Underscore => Token::Underscore,
        RawToken::Semicolon => Token::Semicolon,
        RawToken::Backtick => Token::Backtick,
        RawToken::Operator => match slice {
            "->" => Token::Arrow,
            "<-" => Token::BindArrow,
            "=>" => Token::FatArrow,
            "::" => Token::Cons,
            "??" => Token::Hole,
            "=" => Token::Eq,
            ":" => Token::Colon,
            "." => Token::Dot,
            "|" => Token::Pipe,
            "-" => Token::Minus,
            _ => {
                if is_reserved_operator(slice) {
                    Token::ReservedOp
                } else {
                    Token::Operator
                }
            }
        },
    }
}

fn is_reserved_operator(slice: &str) -> bool {
    matches!(
        slice,
        ".." | ":" | "::" | "=" | "\\" | "|" | "<-" | "->" | "@" | "~" | "=>"
    )
}

fn raw_lex(file: FileId, source: &str) -> Result<Vec<PosToken>, LexError> {
    let line_starts = compute_line_starts(source);
    let mut out = Vec::new();
    let mut offset = 0usize;

    while offset < source.len() {
        let (line, col) = offset_to_line_col(&line_starts, offset);

        if let Some((kind, end)) = trivia_at(source, offset) {
            out.push(PosToken::new(kind, offset, end, line, col));
            offset = end;
            continue;
        }

        let source_tail = source
            .get(offset..)
            .expect("ICE: lexer offset must split source at a UTF-8 boundary");
        let mut lexer = RawToken::lexer(source_tail);
        let Some(result) = lexer.next() else {
            break;
        };
        let span = lexer.span();
        let start = offset + span.start;
        let end = offset + span.end;

        let raw = match result {
            Ok(token) if span.start == 0 => token,
            Ok(_) => ice!("lexer skipped non-trivia input before token"),
            Err(()) => return Err(lex_error(source, file, start, line, col, end)),
        };

        let slice = source
            .get(start..end)
            .expect("ICE: raw token span must split source at UTF-8 boundaries");
        let kind = map_raw_token(raw, slice);
        out.push(PosToken::new(kind, start, end, line, col));
        offset = end;
    }

    if offset != source.len() {
        let (line, col) = offset_to_line_col(&line_starts, offset);
        return Err(lex_error(source, file, offset, line, col, source.len()));
    }

    let eof_offset = source.len();
    let (line, col) = offset_to_line_col(&line_starts, eof_offset);
    out.push(PosToken::new(Token::Eof, eof_offset, eof_offset, line, col));

    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayoutContext {
    Implicit(u32),
}

fn opens_layout(tok: &Token) -> bool {
    matches!(tok, Token::Of | Token::Where | Token::Do)
}

fn is_layout_continuation(tok: &Token) -> bool {
    matches!(
        tok,
        Token::Then
            | Token::Else
            | Token::In
            | Token::Comma
            | Token::RParen
            | Token::RBrack
            | Token::Pipe
            | Token::VClose
            | Token::Eof
    )
}

fn effective_layout_col(tokens: &[PosToken], index: usize) -> u32 {
    let token = tokens
        .get(index)
        .expect("ICE: layout token index must be in bounds");

    if matches!(token.kind, Token::Pipe)
        && let Some(next) = tokens
            .get(index + 1..)
            .expect("ICE: layout token tail range must be in bounds")
            .iter()
            .find(|token| !token.kind.is_trivia())
        && next.line == token.line
    {
        return next.col;
    }

    token.col
}

fn push_virtual(out: &mut Vec<PosToken>, kind: Token, anchor: &PosToken) {
    out.push(PosToken::virtual_at(kind, anchor));
}

pub fn insert_layout(tokens: &[PosToken]) -> Vec<PosToken> {
    let mut out = Vec::new();
    let mut stack: Vec<LayoutContext> = Vec::new();
    let mut pending_layout = false;
    let mut prev_line: Option<u32> = None;
    let mut need_top_level_block = true;

    for (index, token) in tokens.iter().enumerate() {
        if token.kind.is_trivia() {
            out.push(*token);
            continue;
        }

        let col = effective_layout_col(tokens, index);

        if need_top_level_block && token.kind != Token::Eof {
            need_top_level_block = false;
            stack.push(LayoutContext::Implicit(col));
            push_virtual(&mut out, Token::VOpen, token);
        }

        if pending_layout {
            pending_layout = false;

            if token.kind != Token::Eof {
                let ref_col = col;
                let enclosing_col = stack.last().copied();

                push_virtual(&mut out, Token::VOpen, token);

                let block_is_indented = match enclosing_col {
                    Some(LayoutContext::Implicit(parent_col)) => ref_col > parent_col,
                    None => true,
                };

                if block_is_indented {
                    stack.push(LayoutContext::Implicit(ref_col));
                } else {
                    push_virtual(&mut out, Token::VClose, token);
                }
            }
        }

        if let Some(previous_line) = prev_line
            && token.line > previous_line
            && token.kind != Token::Eof
        {
            while let Some(LayoutContext::Implicit(ref_col)) = stack.last().copied() {
                if col < ref_col {
                    stack.pop();
                    push_virtual(&mut out, Token::VClose, token);
                } else {
                    break;
                }
            }

            if !is_layout_continuation(&token.kind)
                && let Some(LayoutContext::Implicit(ref_col)) = stack.last().copied()
                && col == ref_col
            {
                push_virtual(&mut out, Token::VSemi, token);
            }
        }

        if opens_layout(&token.kind) {
            pending_layout = true;
        }

        if token.kind == Token::Eof {
            while stack.pop().is_some() {
                push_virtual(&mut out, Token::VClose, token);
            }
        }

        out.push(*token);
        prev_line = Some(token.line);
    }

    out
}

pub fn lex(file: FileId, source: &str) -> Result<Vec<PosToken>, LexError> {
    raw_lex(file, source).map(|tokens| insert_layout(&tokens))
}

pub fn parser_tokens(tokens: Vec<PosToken>) -> Vec<PosToken> {
    tokens
        .into_iter()
        .filter(|token| !token.kind.is_trivia())
        .collect()
}

#[cfg(test)]
mod tests;
