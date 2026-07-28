use super::*;

fn is_open_primitive_type(ty: &Ty) -> bool {
    type_head_tycon(ty).is_some_and(|id| PRIM_TYS.open_primitive_tycon_ids().contains(&id))
}

#[derive(Clone, Copy)]
pub(super) struct DataEnv<'a> {
    pub(super) names: &'a Names,
    pub(super) tycon_table: &'a TyConTable,
    pub(super) ctor_table: &'a CtorTable,
}

impl DataEnv<'_> {
    pub(super) fn constructors_for_type(&self, ty: &Ty) -> Option<Vec<CtorId>> {
        if is_open_primitive_type(ty) {
            return None;
        }

        let (tycon, _) = type_head_and_args(ty)?;
        let data_def = self.data_definition(tycon)?;

        Some(
            data_def
                .constructors
                .iter()
                .map(|constructor| constructor.id)
                .collect(),
        )
    }

    pub(super) fn field_types_for_constructor(
        &self,
        constructor_id: CtorId,
        scrutinee_type: &Ty,
    ) -> Option<Vec<Ty>> {
        let ctor = self.ctor_table.get(constructor_id)?;
        let (tycon, ty_args) = type_head_and_args(scrutinee_type)?;

        if ctor.tycon != tycon {
            return None;
        }

        let data_def = self.data_definition(ctor.tycon)?;
        Some(instantiate_ctor_fields(data_def, ctor, &ty_args))
    }

    fn data_definition(&self, tycon: TyConId) -> Option<&DataDefinition> {
        let tycon_info = self.tycon_table.get(tycon)?;

        match &tycon_info.definition {
            TyConDefinition::Data(data_def) => Some(data_def),
            TyConDefinition::Primitive => None,
        }
    }
}

pub(super) fn normalize_type(ty: &Ty) -> Ty {
    let mut current = ty;

    while let Ty::TyForAll(_, _, body) = current {
        current = body;
    }

    current.clone()
}

fn type_head_tycon(ty: &Ty) -> Option<TyConId> {
    type_head_and_args(ty).map(|(tycon, _)| tycon)
}

fn type_head_and_args(ty: &Ty) -> Option<(TyConId, Vec<Ty>)> {
    let mut current = normalize_type(ty);
    let mut args = Vec::new();

    loop {
        match current {
            Ty::TyCon(id) => {
                args.reverse();
                return Some((id, args));
            }

            Ty::TyApp(lhs, rhs) => {
                args.push(normalize_type(&rhs));
                current = normalize_type(&lhs);
            }

            Ty::TyUnit
            | Ty::TyVar(_)
            | Ty::TyExVar(_)
            | Ty::TyArr(_, _)
            | Ty::TyForAll(_, _, _) => return None,
        }
    }
}

pub(super) fn instantiate_ctor_fields(
    data_def: &DataDefinition,
    ctor: &CtorInfo,
    ty_args: &[Ty],
) -> Vec<Ty> {
    let subst = data_def
        .ty_params
        .iter()
        .zip(ty_args)
        .map(|((param_id, _), arg)| (*param_id, arg.clone()))
        .collect::<HashMap<_, _>>();

    ctor.fields
        .iter()
        .map(|field_type| subst_type_vars(field_type, &subst))
        .collect()
}

fn subst_type_vars(ty: &Ty, subst: &HashMap<TyVarId, Ty>) -> Ty {
    match ty {
        Ty::TyVar(id) => subst.get(id).cloned().unwrap_or(Ty::TyVar(*id)),

        Ty::TyUnit => Ty::TyUnit,

        Ty::TyCon(id) => Ty::TyCon(*id),

        Ty::TyExVar(id) => Ty::TyExVar(*id),

        Ty::TyArr(lhs, rhs) => {
            Ty::make_arr(subst_type_vars(lhs, subst), subst_type_vars(rhs, subst))
        }

        Ty::TyApp(lhs, rhs) => {
            Ty::make_app(subst_type_vars(lhs, subst), subst_type_vars(rhs, subst))
        }

        Ty::TyForAll(binders, predicates, body) => {
            let mut scoped_subst = subst.clone();
            for (name, _) in binders {
                scoped_subst.remove(name);
            }

            Ty::make_forall_group(
                binders.clone(),
                predicates
                    .iter()
                    .map(|pred| Predicate {
                        class_id: pred.class_id,
                        args: pred
                            .args
                            .iter()
                            .map(|arg| subst_type_vars(arg, &scoped_subst))
                            .collect(),
                    })
                    .collect(),
                subst_type_vars(body, &scoped_subst),
            )
        }
    }
}
