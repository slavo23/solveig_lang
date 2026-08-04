#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use solveig_base::Session;
use solveig_syntax::lowering::parse_source;

#[derive(Arbitrary, Debug)]
struct ParserInput {
    fragments: Vec<SourceFragment>,
}

#[derive(Arbitrary, Debug)]
enum SourceFragment {
    Ident(IdentSeed),
    Upper(IdentSeed),
    Int(u16),
    Keyword(Keyword),
    Operator(Operator),
    Whitespace(Whitespace),
    Char(char),
}

#[derive(Arbitrary, Debug)]
struct IdentSeed {
    bytes: Vec<u8>,
}

#[derive(Arbitrary, Debug)]
enum Keyword {
    Import,
    Data,
    Type,
    Class,
    Instance,
    Where,
    Let,
    In,
    If,
    Then,
    Else,
    Case,
    Of,
    Do,
    Forall,
}

#[derive(Arbitrary, Debug)]
enum Operator {
    Equals,
    Colon,
    Arrow,
    FatArrow,
    Pipe,
    Plus,
    Minus,
    Times,
    Slash,
    Cons,
    Bind,
    Dot,
    ParenOpen,
    ParenClose,
    BracketOpen,
    BracketClose,
    Comma,
}

#[derive(Arbitrary, Debug)]
enum Whitespace {
    Space,
    Newline,
    Indent,
}

fuzz_target!(|input: ParserInput| {
    let source = input.to_source();
    let session = Session::new();
    let file = session.add_file("<fuzz>", source.clone());
    let parsed = parse_source(&session, file, &source);

    assert_eq!(parsed.cst.reconstruct_text(&source), source);
});

impl ParserInput {
    fn to_source(&self) -> String {
        let mut out = String::new();

        for fragment in self.fragments.iter().take(512) {
            fragment.push_source(&mut out);
            if out.len() > 4096 {
                out.truncate(4096);
                break;
            }
        }

        out
    }
}

impl SourceFragment {
    fn push_source(&self, out: &mut String) {
        match self {
            Self::Ident(seed) => seed.push_ident(out, false),
            Self::Upper(seed) => seed.push_ident(out, true),
            Self::Int(value) => out.push_str(&value.to_string()),
            Self::Keyword(keyword) => out.push_str(keyword.text()),
            Self::Operator(operator) => out.push_str(operator.text()),
            Self::Whitespace(whitespace) => out.push_str(whitespace.text()),
            Self::Char(ch) if !ch.is_control() => out.push(*ch),
            Self::Char(_) => out.push(' '),
        }
    }
}

impl IdentSeed {
    fn push_ident(&self, out: &mut String, upper: bool) {
        out.push(if upper { 'A' } else { 'a' });

        for byte in self.bytes.iter().take(16) {
            let ch = match byte % 38 {
                n @ 0..=25 => char::from(b'a' + n),
                n @ 26..=35 => char::from(b'0' + (n - 26)),
                36 => '_',
                _ => '\'',
            };
            out.push(ch);
        }
    }
}

impl Keyword {
    fn text(&self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Data => "data",
            Self::Type => "type",
            Self::Class => "class",
            Self::Instance => "instance",
            Self::Where => "where",
            Self::Let => "let",
            Self::In => "in",
            Self::If => "if",
            Self::Then => "then",
            Self::Else => "else",
            Self::Case => "case",
            Self::Of => "of",
            Self::Do => "do",
            Self::Forall => "forall",
        }
    }
}

impl Operator {
    fn text(&self) -> &'static str {
        match self {
            Self::Equals => "=",
            Self::Colon => ":",
            Self::Arrow => "->",
            Self::FatArrow => "=>",
            Self::Pipe => "|",
            Self::Plus => "+",
            Self::Minus => "-",
            Self::Times => "*",
            Self::Slash => "/",
            Self::Cons => "::",
            Self::Bind => "<-",
            Self::Dot => ".",
            Self::ParenOpen => "(",
            Self::ParenClose => ")",
            Self::BracketOpen => "[",
            Self::BracketClose => "]",
            Self::Comma => ",",
        }
    }
}

impl Whitespace {
    fn text(&self) -> &'static str {
        match self {
            Self::Space => " ",
            Self::Newline => "\n",
            Self::Indent => "\n    ",
        }
    }
}
