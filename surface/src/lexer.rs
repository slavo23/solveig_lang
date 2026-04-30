use logos::Logos;

pub type Symbol = String;

pub fn intern(text: &str) -> Symbol {
    text.to_string()
}

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r]+")]
enum RawToken {
    #[token("\n")]
    Newline,

    #[regex(r"--[^\n]*", logos::skip, allow_greedy = true)]
    LineComment,

    // Keywords (must appear before identifiers).
    #[token("let")]
    KwLet,
    #[token("in")]
    KwIn,
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
    #[token("sig")]
    KwSig,
    #[token("class")]
    KwClass,
    #[token("instance")]
    KwInstance,
    #[token("where")]
    KwWhere,
    #[token("import")]
    KwImport,
    #[token("as")]
    KwAs,

    // Multi-character operators (longest first).
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,
    #[token("::")]
    Cons,
    #[token("==")]
    EqEq,
    #[token(">=")]
    GreaterEq,
    #[token("<=")]
    LessEq,
    #[token("??")]
    Hole,

    // Single-character punctuation/operators.
    #[token("=")]
    Eq,
    #[token(":")]
    Colon,
    #[token(";")]
    Semicolon,
    #[token(".")]
    Dot,
    #[token(",")]
    Comma,
    #[token("|")]
    Pipe,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token(">")]
    Greater,
    #[token("<")]
    Less,
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

    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    Int(i64),

    #[regex(r"[A-Z][a-zA-Z0-9_']*")]
    UpperIdent,

    #[regex(r"[a-z_][a-zA-Z0-9_']*")]
    Ident,
}

// -- Positioned tokens ------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosToken<'src> {
    pub kind: Token<'src>,
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub col: u32,
}

impl<'src> PosToken<'src> {
    fn new(kind: Token<'src>, start: usize, end: usize, line: u32, col: u32) -> Self {
        Self {
            kind,
            start,
            end,
            line,
            col,
        }
    }

    fn virtual_at(kind: Token<'src>, anchor: &Self) -> Self {
        Self::new(kind, anchor.start, anchor.start, anchor.line, anchor.col)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token<'src> {
    Int(i64),
    Ident(&'src str),
    UpperIdent(&'src str),

    Let,
    In,
    Fun,
    Forall,
    Case,
    Of,
    If,
    Then,
    Else,
    Data,
    Sig,
    Class,
    Instance,
    Where,
    Import,
    As,

    Eq,
    Colon,
    Arrow,
    FatArrow,
    Cons,
    Pipe,
    Dot,
    Comma,
    LParen,
    RParen,
    LBrack,
    RBrack,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    Less,
    Greater,
    LessEq,
    GreaterEq,
    Hole,
    Underscore,
    Semicolon,

    VOpen,
    VClose,
    VSemi,

    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub offset: usize,
    pub line: u32,
    pub col: u32,
    pub message: String,
}

fn strip_block_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let mut depth = 0u32;

    while i < bytes.len() {
        let starts_comment = i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'-';
        let ends_comment = i + 1 < bytes.len() && bytes[i] == b'-' && bytes[i + 1] == b'}';

        if starts_comment {
            depth += 1;
            out.extend_from_slice(b"  ");
            i += 2;
            continue;
        }

        if depth > 0 && ends_comment {
            depth -= 1;
            out.extend_from_slice(b"  ");
            i += 2;
            continue;
        }

        if depth > 0 {
            out.push(if bytes[i] == b'\n' { b'\n' } else { b' ' });
        } else {
            out.push(bytes[i]);
        }

        i += 1;
    }

    String::from_utf8(out).expect("block comment stripping broke UTF-8")
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
    let col = offset - line_starts[line_ix];
    (line_ix as u32, col as u32)
}

fn lex_error(source: &str, offset: usize, line: u32, col: u32, end: usize) -> LexError {
    LexError {
        offset,
        line,
        col,
        message: format!("unexpected character '{}'", &source[offset..end]),
    }
}

fn map_raw_token<'src>(raw: RawToken, slice: &'src str) -> Option<Token<'src>> {
    let token = match raw {
        RawToken::Newline | RawToken::LineComment => return None,
        RawToken::Int(n) => Token::Int(n),
        RawToken::Ident => Token::Ident(slice),
        RawToken::UpperIdent => Token::UpperIdent(slice),

        RawToken::KwLet => Token::Let,
        RawToken::KwIn => Token::In,
        RawToken::KwFun => Token::Fun,
        RawToken::KwForall => Token::Forall,
        RawToken::KwCase => Token::Case,
        RawToken::KwOf => Token::Of,
        RawToken::KwIf => Token::If,
        RawToken::KwThen => Token::Then,
        RawToken::KwElse => Token::Else,
        RawToken::KwData => Token::Data,
        RawToken::KwSig => Token::Sig,
        RawToken::KwClass => Token::Class,
        RawToken::KwInstance => Token::Instance,
        RawToken::KwWhere => Token::Where,
        RawToken::KwImport => Token::Import,
        RawToken::KwAs => Token::As,

        RawToken::Arrow => Token::Arrow,
        RawToken::FatArrow => Token::FatArrow,
        RawToken::Cons => Token::Cons,
        RawToken::EqEq => Token::EqEq,
        RawToken::GreaterEq => Token::GreaterEq,
        RawToken::LessEq => Token::LessEq,
        RawToken::Hole => Token::Hole,

        RawToken::Eq => Token::Eq,
        RawToken::Colon => Token::Colon,
        RawToken::Dot => Token::Dot,
        RawToken::Comma => Token::Comma,
        RawToken::Pipe => Token::Pipe,
        RawToken::Plus => Token::Plus,
        RawToken::Minus => Token::Minus,
        RawToken::Star => Token::Star,
        RawToken::Slash => Token::Slash,
        RawToken::Percent => Token::Percent,
        RawToken::Greater => Token::Greater,
        RawToken::Less => Token::Less,
        RawToken::LParen => Token::LParen,
        RawToken::RParen => Token::RParen,
        RawToken::LBrack => Token::LBrack,
        RawToken::RBrack => Token::RBrack,
        RawToken::Underscore => Token::Underscore,
        RawToken::Semicolon => Token::Semicolon,
    };

    Some(token)
}

fn raw_lex<'src>(source: &'src str) -> Result<Vec<PosToken<'src>>, LexError> {
    let cleaned = strip_block_comments(source);
    let line_starts = compute_line_starts(source);
    let mut out = Vec::new();

    let mut lexer = RawToken::lexer(cleaned.as_str());
    while let Some(result) = lexer.next() {
        let span = lexer.span();
        let (line, col) = offset_to_line_col(&line_starts, span.start);

        let raw = match result {
            Ok(token) => token,
            Err(()) => return Err(lex_error(source, span.start, line, col, span.end)),
        };

        let slice = &source[span.start..span.end];
        let Some(kind) = map_raw_token(raw, slice) else {
            continue;
        };

        out.push(PosToken::new(kind, span.start, span.end, line, col));
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

fn opens_layout(tok: &Token<'_>) -> bool {
    matches!(tok, Token::Of | Token::Where)
}

fn is_layout_continuation(tok: &Token<'_>) -> bool {
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

fn effective_layout_col<'src>(tokens: &[PosToken<'src>], index: usize) -> u32 {
    let token = &tokens[index];

    if matches!(token.kind, Token::Pipe) {
        if let Some(next) = tokens.get(index + 1) {
            if next.line == token.line {
                return next.col;
            }
        }
    }

    token.col
}

fn push_virtual<'src>(out: &mut Vec<PosToken<'src>>, kind: Token<'src>, anchor: &PosToken<'src>) {
    out.push(PosToken::virtual_at(kind, anchor));
}

pub fn insert_layout<'src>(tokens: Vec<PosToken<'src>>) -> Vec<PosToken<'src>> {
    let mut out = Vec::new();
    let mut stack: Vec<LayoutContext> = Vec::new();
    let mut pending_layout = false;
    let mut prev_line: Option<u32> = None;
    let mut need_top_level_block = true;

    for (index, token) in tokens.iter().enumerate() {
        let col = effective_layout_col(&tokens, index);

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

        if let Some(previous_line) = prev_line {
            if token.line > previous_line && token.kind != Token::Eof {
                while let Some(LayoutContext::Implicit(ref_col)) = stack.last().copied() {
                    if col < ref_col {
                        stack.pop();
                        push_virtual(&mut out, Token::VClose, token);
                    } else {
                        break;
                    }
                }

                if !is_layout_continuation(&token.kind) {
                    if let Some(LayoutContext::Implicit(ref_col)) = stack.last().copied() {
                        if col == ref_col {
                            push_virtual(&mut out, Token::VSemi, token);
                        }
                    }
                }
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

        out.push(token.clone());
        prev_line = Some(token.line);
    }

    out
}

pub fn lex<'src>(source: &'src str) -> Result<Vec<PosToken<'src>>, LexError> {
    raw_lex(source).map(insert_layout)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Left,
    Right,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fixity {
    pub prec: u8,
    pub assoc: Assoc,
}

pub fn builtin_fixity(tok: &Token<'_>) -> Option<Fixity> {
    match tok {
        Token::EqEq | Token::Less | Token::Greater | Token::LessEq | Token::GreaterEq => {
            Some(Fixity {
                prec: 4,
                assoc: Assoc::None,
            })
        }
        Token::Cons => Some(Fixity {
            prec: 5,
            assoc: Assoc::Right,
        }),
        Token::Plus | Token::Minus => Some(Fixity {
            prec: 6,
            assoc: Assoc::Left,
        }),
        Token::Star | Token::Slash | Token::Percent => Some(Fixity {
            prec: 7,
            assoc: Assoc::Left,
        }),
        _ => None,
    }
}
