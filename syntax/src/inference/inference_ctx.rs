use core::ast::{Kind, Pretty, TypeExpr};
use std::collections::HashMap;
use std::iter::successors;

#[derive(Clone, PartialEq, Debug)]
pub enum ContextEntry {
    ExVar(String, Kind),
    UniVar(String, Kind),
    TypedVar { var_name: String, ty: TypeExpr },
    SolvedEVar { lhs: String, rhs: TypeExpr },
    Marker { var_name: String },
}

#[derive(Clone, Debug)]
pub struct Context {
    entries: Vec<ContextEntry>,
}

impl Context {
    pub fn new() -> Context {
        Context { entries: vec![] }
    }

    pub fn from_entries(entries: Vec<ContextEntry>) -> Context {
        Context { entries }
    }

    fn apply_once(&self, ty: &TypeExpr) -> TypeExpr {
        match ty {
            TypeExpr::TyVar(_) | TypeExpr::TyCon(_) | TypeExpr::TyUnit => ty.clone(),
            TypeExpr::TyArr(lhs, rhs) => TypeExpr::make_arr(self.apply(lhs), self.apply(rhs)),
            TypeExpr::TyApp(lhs, rhs) => TypeExpr::make_app(self.apply(lhs), self.apply(rhs)),
            TypeExpr::TyExVar(evar_name) => {
                if let Some(ContextEntry::SolvedEVar { rhs: solved, .. }) =
                    self.find_solved_evar(evar_name)
                {
                    return self.apply(&solved);
                }
                ty.clone()
            }
            TypeExpr::TyForAll(quantifier, kind, body) => {
                TypeExpr::make_forall(quantifier.clone(), kind.clone(), self.apply(body))
            }
        }
    }

    pub fn apply(&self, ty: &TypeExpr) -> TypeExpr {
        successors(Some(ty.clone()), |prev| {
            let next = self.apply_once(prev);
            if next != *prev { Some(next) } else { None }
        })
        .last()
        .unwrap()
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
            panic!("replace: {:?} not found in context", target);
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
                panic!("split_at: {:?} not found in context", on_entry);
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

    pub fn find_solved_evar(&self, evar_name: &str) -> Option<ContextEntry> {
        self.entries
            .iter()
            .find(|ent| matches!(ent, ContextEntry::SolvedEVar { lhs, .. } if lhs == evar_name))
            .cloned()
    }

    pub fn has_solved_evar(&self, evar_name: &str) -> bool {
        self.find_solved_evar(evar_name).is_some()
    }

    pub fn has_unsolved_evar(&self, evar_name: &str) -> bool {
        self.entries
            .iter()
            .any(|ent| matches!(ent, ContextEntry::ExVar(name, _) if name == evar_name))
    }

    pub fn has_universal(&self, var_name: &str) -> bool {
        self.entries
            .iter()
            .any(|ent| matches!(ent, ContextEntry::UniVar(name, _) if name == var_name))
    }

    pub fn evar_kind(&self, evar_name: &str) -> Option<Kind> {
        self.entries.iter().find_map(|ent| match ent {
            ContextEntry::ExVar(name, kind) if name == evar_name => Some(kind.clone()),
            _ => None,
        })
    }

    pub fn universal_kind(&self, var_name: &str) -> Option<Kind> {
        self.entries.iter().find_map(|ent| match ent {
            ContextEntry::UniVar(name, kind) if name == var_name => Some(kind.clone()),
            _ => None,
        })
    }

    pub fn evar_before(&self, earlier: &str, later: &str) -> bool {
        let position_a = self
            .entries
            .iter()
            .position(|ent| matches!(ent, ContextEntry::ExVar(name, _) if name == earlier));
        let position_b = self
            .entries
            .iter()
            .position(|ent| matches!(ent, ContextEntry::ExVar(name, _) if name == later));
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
        tycon_kinds: &HashMap<String, Kind>,
    ) -> Result<Kind, String> {
        let ty = self.apply(ty);
        match &ty {
            TypeExpr::TyUnit => Ok(Kind::Star),
            TypeExpr::TyCon(name) => tycon_kinds
                .get(name)
                .cloned()
                .ok_or_else(|| format!("unknown type constructor: {}", name)),
            TypeExpr::TyVar(name) => self
                .universal_kind(name)
                .ok_or_else(|| format!("unbound type variable: {}", name)),
            TypeExpr::TyExVar(name) => self
                .evar_kind(name)
                .ok_or_else(|| format!("unknown existential: {}", name)),
            TypeExpr::TyArr(_, _) => Ok(Kind::Star),
            TypeExpr::TyForAll(_, _, _) => Ok(Kind::Star),
            TypeExpr::TyApp(func, arg) => {
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
            TypeExpr::TyVar(name) => self.has_universal(name),
            TypeExpr::TyExVar(name) => self.has_unsolved_evar(name) || self.has_solved_evar(name),
            TypeExpr::TyArr(lhs, rhs) | TypeExpr::TyApp(lhs, rhs) => {
                self.type_well_formed(lhs) && self.type_well_formed(rhs)
            }
            TypeExpr::TyForAll(quantifier, kind, body) => {
                let extended = self
                    .clone()
                    .add_entry(ContextEntry::UniVar(quantifier.clone(), kind.clone()));
                extended.type_well_formed(body)
            }
        }
    }
}
