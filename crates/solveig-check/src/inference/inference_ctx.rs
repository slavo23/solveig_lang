use solveig_ast::ast::{
    EvarId, Kind, Predicate, Pretty, TyConId, TyVarId, TypeExpr, tyvar_display_name,
};
use std::collections::HashMap;
use std::iter::successors;

#[derive(Clone, PartialEq, Debug)]
pub enum ContextEntry {
    ExVar(EvarId, Kind),
    UniVar {
        id: TyVarId,
        name: String,
        kind: Kind,
    },
    TypedVar {
        var_name: String,
        ty: TypeExpr,
    },
    SolvedEVar {
        lhs: EvarId,
        rhs: TypeExpr,
    },
    Marker {
        var_name: String,
    },
}

#[derive(Clone, Debug)]
pub struct Context {
    entries: Vec<ContextEntry>,
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Context {
    pub fn new() -> Context {
        Context { entries: vec![] }
    }

    pub fn from_entries(entries: Vec<ContextEntry>) -> Context {
        Context { entries }
    }

    pub fn get_entries(&self) -> Vec<ContextEntry> {
        self.entries.clone()
    }

    fn apply_predicate(&self, pred: &Predicate) -> Predicate {
        Predicate {
            class_id: pred.class_id,
            args: pred.args.iter().map(|arg| self.apply(arg)).collect(),
        }
    }

    fn apply_once(&self, ty: &TypeExpr) -> TypeExpr {
        match ty {
            TypeExpr::TyVar(_) | TypeExpr::TyCon(_) | TypeExpr::TyUnit => ty.clone(),

            TypeExpr::TyArr(lhs, rhs) => TypeExpr::make_arr(self.apply(lhs), self.apply(rhs)),

            TypeExpr::TyApp(lhs, rhs) => TypeExpr::make_app(self.apply(lhs), self.apply(rhs)),

            TypeExpr::TyExVar(evar_id) => {
                if let Some(ContextEntry::SolvedEVar { rhs: solved, .. }) =
                    self.find_solved_evar_id(*evar_id)
                {
                    return self.apply(&solved);
                }
                ty.clone()
            }

            TypeExpr::TyForAll(binders, preds, body) => TypeExpr::make_forall_group(
                binders.clone(),
                preds.iter().map(|p| self.apply_predicate(p)).collect(),
                self.apply(body),
            ),
        }
    }

    pub fn apply(&self, ty: &TypeExpr) -> TypeExpr {
        successors(Some(ty.clone()), |prev| {
            let next = self.apply_once(prev);
            if next != *prev { Some(next) } else { None }
        })
        .last()
        .expect("ICE: context application iterator is never empty")
    }

    pub fn add_entry(self, entry: ContextEntry) -> Context {
        let mut new_entries = self.entries;
        new_entries.push(entry);
        Context {
            entries: new_entries,
        }
    }

    pub fn until(&self, entry: &ContextEntry) -> Context {
        let (left, _, _) = self.split_at(entry);
        Context { entries: left }
    }

    pub fn replace(&self, target: &ContextEntry, replacement: &ContextEntry) -> Context {
        let mut new_entries = self.entries.clone();
        if let Some(position) = new_entries.iter().rposition(|ent| ent == target) {
            new_entries[position] = replacement.clone();
        } else {
            panic!("ICE: replace target {:?} not found in context", target);
        }
        Context {
            entries: new_entries,
        }
    }

    pub fn split_at(
        &self,
        on_entry: &ContextEntry,
    ) -> (Vec<ContextEntry>, &ContextEntry, Vec<ContextEntry>) {
        let found_position = self.entries.iter().rposition(|ent| ent == on_entry);
        match found_position {
            Some(position) => {
                let middle = &self.entries[position];
                let left = self.entries[..position].to_vec();
                let right = self.entries[position + 1..].to_vec();
                (left, middle, right)
            }
            None => {
                panic!("ICE: split_at target {:?} not found in context", on_entry);
            }
        }
    }

    pub fn from_halves(left: Vec<ContextEntry>, right: Vec<ContextEntry>) -> Context {
        let mut entries = left;
        entries.extend(right);
        Context { entries }
    }

    pub fn remove_entry(&self, target: &ContextEntry) -> Context {
        let (left, _, right) = self.split_at(target);
        Self::from_halves(left, right)
    }

    pub fn find_solved_evar_id(&self, evar_id: EvarId) -> Option<ContextEntry> {
        self.entries
            .iter()
            .find(|ent| matches!(ent, ContextEntry::SolvedEVar { lhs, .. } if *lhs == evar_id))
            .cloned()
    }

    pub fn has_solved_evar_id(&self, evar_id: EvarId) -> bool {
        self.find_solved_evar_id(evar_id).is_some()
    }

    pub fn has_unsolved_evar_id(&self, evar_id: EvarId) -> bool {
        self.entries
            .iter()
            .any(|ent| matches!(ent, ContextEntry::ExVar(id, _) if *id == evar_id))
    }

    pub fn has_universal(&self, var_name: &str) -> bool {
        self.entries
            .iter()
            .any(|ent| matches!(ent, ContextEntry::UniVar { name, .. } if name == var_name))
    }

    pub fn has_universal_id(&self, var_id: TyVarId) -> bool {
        self.entries
            .iter()
            .any(|ent| matches!(ent, ContextEntry::UniVar { id, .. } if *id == var_id))
    }

    pub fn evar_kind_id(&self, evar_id: EvarId) -> Option<Kind> {
        self.entries.iter().find_map(|ent| match ent {
            ContextEntry::ExVar(id, kind) if *id == evar_id => Some(kind.clone()),
            _ => None,
        })
    }

    pub fn universal_kind(&self, var_name: &str) -> Option<Kind> {
        self.entries.iter().find_map(|ent| match ent {
            ContextEntry::UniVar { name, kind, .. } if name == var_name => Some(kind.clone()),
            _ => None,
        })
    }

    pub fn universal_kind_id(&self, var_id: TyVarId) -> Option<Kind> {
        self.entries.iter().find_map(|ent| match ent {
            ContextEntry::UniVar { id, kind, .. } if *id == var_id => Some(kind.clone()),
            _ => None,
        })
    }

    pub fn evar_before_id(&self, earlier: EvarId, later: EvarId) -> bool {
        let position_a = self
            .entries
            .iter()
            .position(|ent| matches!(ent, ContextEntry::ExVar(id, _) if *id == earlier));
        let position_b = self
            .entries
            .iter()
            .position(|ent| matches!(ent, ContextEntry::ExVar(id, _) if *id == later));
        match (position_a, position_b) {
            (Some(pos_a), Some(pos_b)) => pos_a < pos_b,
            _ => false,
        }
    }

    pub fn lookup_term_var(&self, name: &str) -> Option<TypeExpr> {
        self.entries.iter().rev().find_map(|ent| match ent {
            ContextEntry::TypedVar { var_name, ty } if var_name == name => Some(ty.clone()),
            _ => None,
        })
    }

    pub fn kind_of_type(
        &self,
        ty: &TypeExpr,
        tycon_kinds: &HashMap<TyConId, Kind>,
    ) -> Result<Kind, String> {
        let ty = self.apply(ty);
        match &ty {
            TypeExpr::TyUnit => Ok(Kind::Star),

            TypeExpr::TyCon(id) => tycon_kinds
                .get(id)
                .cloned()
                .ok_or_else(|| format!("unknown type constructor: T{}", id.0)),

            TypeExpr::TyVar(id) => self
                .universal_kind_id(*id)
                .ok_or_else(|| format!("unbound type variable: {}", tyvar_display_name(*id))),

            TypeExpr::TyExVar(id) => self
                .evar_kind_id(*id)
                .ok_or_else(|| format!("unknown existential: ?{}", id.0)),

            TypeExpr::TyArr(_, _) => Ok(Kind::Star),

            TypeExpr::TyForAll(binders, preds, body) => {
                let mut extended = self.clone();
                for (id, kind) in binders {
                    extended = extended.add_entry(ContextEntry::UniVar {
                        id: *id,
                        name: tyvar_display_name(*id),
                        kind: kind.clone(),
                    });
                }

                for pred in preds {
                    for arg in &pred.args {
                        let arg_kind = extended.kind_of_type(arg, tycon_kinds)?;
                        if arg_kind != Kind::Star {
                            return Err(format!(
                                "predicate argument must have kind *, got {}",
                                arg_kind.pretty()
                            ));
                        }
                    }
                }

                let body_kind = extended.kind_of_type(body, tycon_kinds)?;
                if body_kind == Kind::Star {
                    Ok(Kind::Star)
                } else {
                    Err(format!(
                        "scheme type body must have kind *, got {}",
                        body_kind.pretty()
                    ))
                }
            }

            TypeExpr::TyApp(func, arg) => {
                if matches!(**func, TypeExpr::TyForAll(_, _, _))
                    || matches!(**arg, TypeExpr::TyForAll(_, _, _))
                {
                    return Err(
                        "scheme type cannot appear directly in type application".to_string()
                    );
                }

                let func_kind = self.kind_of_type(func, tycon_kinds)?;
                let arg_kind = self.kind_of_type(arg, tycon_kinds)?;
                match func_kind {
                    Kind::Arrow(domain, codomain) if *domain == arg_kind => Ok(*codomain),
                    Kind::Arrow(domain, _) => Err(format!(
                        "type application kind mismatch: expected {}, got {}",
                        domain.pretty(),
                        arg_kind.pretty()
                    )),
                    other => Err(format!("cannot apply non-arrow kind: {}", other.pretty())),
                }
            }
        }
    }

    pub fn type_well_formed(&self, ty: &TypeExpr) -> bool {
        match ty {
            TypeExpr::TyUnit | TypeExpr::TyCon(_) => true,
            TypeExpr::TyVar(id) => self.has_universal_id(*id),
            TypeExpr::TyExVar(id) => self.has_unsolved_evar_id(*id) || self.has_solved_evar_id(*id),
            TypeExpr::TyArr(lhs, rhs) => self.type_well_formed(lhs) && self.type_well_formed(rhs),
            TypeExpr::TyApp(lhs, rhs) => {
                !matches!(**lhs, TypeExpr::TyForAll(_, _, _))
                    && !matches!(**rhs, TypeExpr::TyForAll(_, _, _))
                    && self.type_well_formed(lhs)
                    && self.type_well_formed(rhs)
            }
            TypeExpr::TyForAll(binders, preds, body) => {
                let mut extended = self.clone();
                for (id, kind) in binders {
                    extended = extended.add_entry(ContextEntry::UniVar {
                        id: *id,
                        name: tyvar_display_name(*id),
                        kind: kind.clone(),
                    });
                }
                preds
                    .iter()
                    .all(|pred| pred.args.iter().all(|arg| extended.type_well_formed(arg)))
                    && extended.type_well_formed(body)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evar_context_entries_are_keyed_by_id() {
        let evar = EvarId(7);
        let unsolved = ContextEntry::ExVar(evar, Kind::Star);
        let ctx = Context::new().add_entry(unsolved.clone());

        assert!(ctx.has_unsolved_evar_id(evar));
        assert!(!ctx.has_unsolved_evar_id(EvarId(8)));

        let solved = ContextEntry::SolvedEVar {
            lhs: evar,
            rhs: TypeExpr::TyUnit,
        };
        let ctx = ctx.replace(&unsolved, &solved);

        assert!(ctx.has_solved_evar_id(evar));
        assert_eq!(ctx.apply(&TypeExpr::make_evar(evar)), TypeExpr::TyUnit);
    }

    #[test]
    fn universal_context_entries_are_keyed_by_id() {
        let tyvar = TyVarId(3);
        let ctx = Context::new().add_entry(ContextEntry::UniVar {
            id: tyvar,
            name: "a".to_string(),
            kind: Kind::Star,
        });

        assert!(ctx.has_universal("a"));
        assert!(ctx.has_universal_id(tyvar));
        assert_eq!(ctx.universal_kind("a"), Some(Kind::Star));
        assert_eq!(ctx.universal_kind_id(tyvar), Some(Kind::Star));
    }

    #[test]
    fn type_constructor_kinds_are_keyed_by_id() {
        let list_id = TyConId(7);
        let other_id = TyConId(8);
        let mut tycon_kinds = HashMap::new();
        tycon_kinds.insert(list_id, Kind::make_arrow(Kind::Star, Kind::Star));

        let ctx = Context::new();
        let list_unit = TypeExpr::make_app(TypeExpr::make_con_id(list_id), TypeExpr::TyUnit);

        assert_eq!(ctx.kind_of_type(&list_unit, &tycon_kinds), Ok(Kind::Star));
        assert!(
            ctx.kind_of_type(&TypeExpr::make_con_id(other_id), &tycon_kinds)
                .is_err()
        );
    }
}
