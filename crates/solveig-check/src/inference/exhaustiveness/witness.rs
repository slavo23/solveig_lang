//! Witness pattern construction for missing case alternatives.
//! Exhaustiveness diagnostics render these rows back into user-facing patterns.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum LitPat {
    Int(i64),
    Float(u64),
    Char(char),
    Str(solveig_ast::ast::StrLitId),
}

impl LitPat {
    pub(super) fn from_literal(lit: &Literal) -> Self {
        match lit {
            Literal::Int(value) => LitPat::Int(*value),
            Literal::Float(value) => LitPat::Float(value.to_bits()),
            Literal::Char(value) => LitPat::Char(*value),
            Literal::Str(id) => LitPat::Str(*id),
        }
    }

    pub(super) fn pretty(&self, names: &Names) -> String {
        match self {
            LitPat::Int(value) => value.to_string(),
            LitPat::Float(bits) => f64::from_bits(*bits).to_string(),
            LitPat::Char(value) => format!("'{}'", escape_char_literal(*value)),
            LitPat::Str(id) => {
                format!(
                    "\"{}\"",
                    escape_string_literal(names.string_literal_value(*id))
                )
            }
        }
    }
}

fn escape_char_literal(ch: char) -> String {
    match ch {
        '\n' => "\\n".to_string(),
        '\t' => "\\t".to_string(),
        '\\' => "\\\\".to_string(),
        '\'' => "\\'".to_string(),
        '"' => "\\\"".to_string(),
        ch if ch.is_control() => ch.escape_unicode().to_string(),
        ch => ch.to_string(),
    }
}

fn escape_string_literal(value: &str) -> String {
    let mut out = String::new();

    for ch in value.chars() {
        out.push_str(&match ch {
            '\n' => "\\n".to_string(),
            '\t' => "\\t".to_string(),
            '\\' => "\\\\".to_string(),
            '"' => "\\\"".to_string(),
            ch if ch.is_control() => ch.escape_unicode().to_string(),
            ch => ch.to_string(),
        });
    }

    out
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Head {
    Constructor(CtorId),
    Literal(LitPat),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Pat {
    Wild,
    Head(Head, Vec<Pat>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Witness {
    Wild,
    Head(Head, Vec<Witness>),
}

impl Witness {
    pub(super) fn pretty(&self, names: &Names, ctor_table: &CtorTable) -> String {
        match self {
            Witness::Wild => "_".to_string(),

            Witness::Head(Head::Constructor(id), args) => {
                let name = ctor_table_short_name(names, *id, ctor_table);

                if name == "Nil" && args.is_empty() {
                    return "[]".to_string();
                }

                if name == "Cons"
                    && let [head, tail] = args.as_slice()
                {
                    return format!(
                        "{} :: {}",
                        head.pretty_atom(names, ctor_table),
                        tail.pretty(names, ctor_table)
                    );
                }

                if args.is_empty() {
                    name
                } else {
                    format!(
                        "{} {}",
                        name,
                        args.iter()
                            .map(|arg| arg.pretty_atom(names, ctor_table))
                            .collect::<Vec<_>>()
                            .join(" ")
                    )
                }
            }

            Witness::Head(Head::Literal(lit), args) => {
                debug_assert!(args.is_empty());
                lit.pretty(names)
            }
        }
    }

    pub(super) fn pretty_atom(&self, names: &Names, ctor_table: &CtorTable) -> String {
        match self {
            Witness::Wild => self.pretty(names, ctor_table),
            Witness::Head(_, args) if args.is_empty() => self.pretty(names, ctor_table),
            Witness::Head(_, _) => format!("({})", self.pretty(names, ctor_table)),
        }
    }
}

pub(super) fn ctor_table_short_name(names: &Names, id: CtorId, ctor_table: &CtorTable) -> String {
    ctor_table.get(id).map_or_else(
        || solveig_base::ice!("exhaustiveness constructor table missing CtorId {id:?}"),
        |info| names.symbol_display_name(info.short_name),
    )
}

pub(super) fn witness_to_pattern_row(witness: &[Witness]) -> Vec<Pat> {
    witness.iter().map(witness_to_pattern).collect()
}

fn witness_to_pattern(witness: &Witness) -> Pat {
    match witness {
        Witness::Wild => Pat::Wild,

        Witness::Head(head, args) => {
            Pat::Head(head.clone(), args.iter().map(witness_to_pattern).collect())
        }
    }
}
