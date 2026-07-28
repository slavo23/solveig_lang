//! Typeclass predicates and polymorphic type schemes.
//! Schemes are the only core type nodes that own quantifiers and predicates together.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Predicate {
    pub class_id: ClassId,
    pub args: Vec<Ty>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scheme {
    pub quantifiers: Vec<(TyVarId, Kind)>,
    pub predicates: Vec<Predicate>,
    pub ty: Ty,
}

impl Predicate {
    pub fn pretty(&self) -> String {
        let args = self
            .args
            .iter()
            .map(|arg| {
                let s = arg.pretty();
                match arg {
                    Ty::TyArr(_, _) | Ty::TyForAll(_, _, _) => format!("({s})"),
                    _ => s,
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        if args.is_empty() {
            class_fallback_name(self.class_id)
        } else {
            format!("{} {args}", class_fallback_name(self.class_id))
        }
    }

    pub fn named(names: &mut Names, class_name: String, args: Vec<Ty>) -> Self {
        Self {
            class_id: names.class_id(&class_name),
            args,
        }
    }

    pub fn class_name(&self) -> String {
        class_fallback_name(self.class_id)
    }
}

impl Scheme {
    pub fn to_ty(&self) -> Ty {
        Ty::make_forall_group(
            self.quantifiers.clone(),
            self.predicates.clone(),
            self.ty.clone(),
        )
    }

    pub fn pretty(&self) -> String {
        self.to_ty().pretty()
    }

    pub fn mono(ty: Ty) -> Self {
        Self {
            quantifiers: Vec::new(),
            predicates: Vec::new(),
            ty,
        }
    }
}
