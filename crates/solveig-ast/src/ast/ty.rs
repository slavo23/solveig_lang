//! Checked/core type syntax and type substitution operations.
//! Type constructor and variable identities are table-backed IDs, not strings.

use super::*;

#[derive(Clone, PartialEq, Debug, Eq, Hash)]
pub enum Ty {
    TyUnit,
    TyVar(TyVarId),
    TyExVar(EvarId),
    TyArr(Box<Ty>, Box<Ty>),
    TyForAll(Vec<(TyVarId, Kind)>, Vec<Predicate>, Box<Ty>),
    TyCon(TyConId),
    TyApp(Box<Ty>, Box<Ty>),
}

pub fn evar_display_name(id: EvarId) -> String {
    format!("a^{}", id.0)
}

pub fn tyvar_fallback_name(id: TyVarId) -> String {
    format!("t{}", id.0)
}

pub fn generated_tyvar_name(base: &str, id: TyVarId) -> String {
    format!("{}{}", base, id.0)
}

pub fn tyvar_display_name(id: TyVarId) -> String {
    tyvar_fallback_name(id)
}

impl Ty {
    pub fn is_mono(&self) -> bool {
        match self {
            Ty::TyForAll(_, _, _) => false,
            Ty::TyArr(lhs, rhs) | Ty::TyApp(lhs, rhs) => lhs.is_mono() && rhs.is_mono(),
            _ => true,
        }
    }

    pub fn occurs_check(&self, evar_id: EvarId) -> bool {
        match self {
            Self::TyExVar(id) if *id == evar_id => true,

            Self::TyArr(lhs, rhs) | Self::TyApp(lhs, rhs) => {
                lhs.occurs_check(evar_id) || rhs.occurs_check(evar_id)
            }

            Self::TyForAll(_, preds, body) => {
                preds
                    .iter()
                    .any(|pred| pred.args.iter().any(|arg| arg.occurs_check(evar_id)))
                    || body.occurs_check(evar_id)
            }

            _ => false,
        }
    }

    pub fn peel_scheme_like(&self) -> (Vec<(TyVarId, Kind)>, Vec<Predicate>, Ty) {
        let mut current_ty = self.clone();
        let mut quants_with_kinds = vec![];
        let mut predicates = vec![];
        while let Ty::TyForAll(mut quants, mut preds, body) = current_ty {
            quants_with_kinds.append(&mut quants);
            predicates.append(&mut preds);
            current_ty = *body;
        }

        (quants_with_kinds, predicates, current_ty)
    }

    pub fn peel_arrow(&self) -> Option<(Ty, Ty)> {
        match self {
            Self::TyArr(fun_ty, arg_ty) => Some((*fun_ty.clone(), *arg_ty.clone())),
            _ => None,
        }
    }

    pub fn make_arr(lhs: Ty, rhs: Ty) -> Ty {
        Ty::TyArr(Box::new(lhs), Box::new(rhs))
    }

    pub fn make_app(lhs: Ty, rhs: Ty) -> Ty {
        Ty::TyApp(Box::new(lhs), Box::new(rhs))
    }

    pub fn make_evar(id: EvarId) -> Ty {
        Self::TyExVar(id)
    }

    pub fn make_named_con(names: &mut Names, name: String) -> Ty {
        Self::TyCon(names.tycon_id(&name))
    }

    pub fn make_con_id(id: TyConId) -> Ty {
        Self::TyCon(id)
    }

    pub fn make_forall_id(quant_id: TyVarId, kind: Kind, body: Ty) -> Ty {
        Self::make_forall_group(vec![(quant_id, kind)], Vec::new(), body)
    }

    pub fn make_qualified(predicates: Vec<Predicate>, body: Ty) -> Ty {
        Self::make_forall_group(Vec::new(), predicates, body)
    }

    pub fn make_forall_group(
        mut binders: Vec<(TyVarId, Kind)>,
        mut predicates: Vec<Predicate>,
        body: Ty,
    ) -> Ty {
        if binders.is_empty() && predicates.is_empty() {
            return body;
        }

        match body {
            Ty::TyForAll(mut body_binders, mut body_predicates, body_body) => {
                binders.append(&mut body_binders);
                predicates.append(&mut body_predicates);
                Ty::TyForAll(binders, predicates, body_body)
            }
            body => Ty::TyForAll(binders, predicates, Box::new(body)),
        }
    }

    pub fn substitute(&self, var_id: TyVarId, with_type: &Ty) -> Ty {
        match self {
            Self::TyVar(sub_var_id) => {
                if *sub_var_id == var_id {
                    with_type.clone()
                } else {
                    self.clone()
                }
            }

            Self::TyCon(_) | Self::TyExVar(_) | Self::TyUnit => self.clone(),

            Self::TyArr(lhs, rhs) => Self::make_arr(
                lhs.substitute(var_id, with_type),
                rhs.substitute(var_id, with_type),
            ),

            Self::TyApp(lhs, rhs) => Self::make_app(
                lhs.substitute(var_id, with_type),
                rhs.substitute(var_id, with_type),
            ),

            Self::TyForAll(binders, preds, body) => {
                if binders.iter().any(|(quantifier, _)| *quantifier == var_id) {
                    Self::make_forall_group(binders.clone(), preds.clone(), (**body).clone())
                } else {
                    let new_preds = preds
                        .iter()
                        .map(|pred| Predicate {
                            class_id: pred.class_id,
                            args: pred
                                .args
                                .iter()
                                .map(|arg| arg.substitute(var_id, with_type))
                                .collect(),
                        })
                        .collect();
                    Self::make_forall_group(
                        binders.clone(),
                        new_preds,
                        body.substitute(var_id, with_type),
                    )
                }
            }
        }
    }

    pub fn substitute_evar(&self, evar_id: EvarId, with_type: &Ty) -> Ty {
        match self {
            Self::TyExVar(id) => {
                if *id == evar_id {
                    with_type.clone()
                } else {
                    self.clone()
                }
            }

            Self::TyVar(_) | Self::TyCon(_) | Self::TyUnit => self.clone(),

            Self::TyArr(lhs, rhs) => Self::make_arr(
                lhs.substitute_evar(evar_id, with_type),
                rhs.substitute_evar(evar_id, with_type),
            ),

            Self::TyApp(lhs, rhs) => Self::make_app(
                lhs.substitute_evar(evar_id, with_type),
                rhs.substitute_evar(evar_id, with_type),
            ),

            Self::TyForAll(binders, preds, body) => {
                let new_preds = preds
                    .iter()
                    .map(|pred| Predicate {
                        class_id: pred.class_id,
                        args: pred
                            .args
                            .iter()
                            .map(|arg| arg.substitute_evar(evar_id, with_type))
                            .collect(),
                    })
                    .collect();

                Self::make_forall_group(
                    binders.clone(),
                    new_preds,
                    body.substitute_evar(evar_id, with_type),
                )
            }
        }
    }
}
