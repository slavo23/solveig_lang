//! Kind syntax and formatting for checked type constructors.
//! Kinds are stored in the core AST and in kind-inference tables.

use super::*;

#[derive(Clone, PartialEq, Debug, Eq, Hash)]
pub enum Kind {
    Meta(KindMetaId),
    Star,
    Arrow(Box<Kind>, Box<Kind>),
}

impl Kind {
    pub fn make_star() -> Self {
        Kind::Star
    }

    pub fn make_meta(id: KindMetaId) -> Self {
        Kind::Meta(id)
    }

    pub fn make_arrow(lhs: Kind, rhs: Kind) -> Self {
        Kind::Arrow(Box::new(lhs), Box::new(rhs))
    }
}

impl Pretty<Kind> for Kind {
    fn get_precedence(kind: &Kind) -> u8 {
        match kind {
            Kind::Star | Kind::Meta(_) => PRECEDENCE_ATOM,
            Kind::Arrow(_, _) => PRECEDENCE_ARR,
        }
    }

    fn pretty(&self) -> String {
        fn traverse_helper(kind: &Kind, parent_precedence: u8) -> String {
            let curr_precedence = Kind::get_precedence(kind);
            let kind_fmt = match kind {
                Kind::Star => "*".to_string(),
                Kind::Meta(id) => format!("?k{}", id.0),
                Kind::Arrow(lhs, rhs) => {
                    let lhs_fmt = traverse_helper(lhs, PRECEDENCE_ATOM);
                    let rhs_fmt = traverse_helper(rhs, PRECEDENCE_ARR);
                    format!("{lhs_fmt} -> {rhs_fmt}")
                }
            };

            if curr_precedence < parent_precedence {
                format!("({kind_fmt})")
            } else {
                kind_fmt
            }
        }

        traverse_helper(self, PRECEDENCE_ARR)
    }
}
