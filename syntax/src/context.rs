use core::core_type::Ty;
use std::iter::successors;

#[derive(Clone, PartialEq, Debug)]
pub enum ContextEntry {
    ExVar(String),
    MonoVar(String),
    TypedVar { var_name: String, ty: Ty },
    SolvedEVar { lhs: String, rhs: Ty },
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
        Context { entries: entries }
    }

    fn apply_once(&self, ty: &Ty) -> Ty {
        match ty {
            Ty::TyVar(_) | Ty::TyCon(_) => ty.clone(),
            Ty::TyUnit => ty.clone(),
            Ty::TyArr(lhs, rhs) => {
                let app_lhs = self.apply(lhs);
                let app_rhs = self.apply(rhs);
                Ty::make_arr(app_lhs, app_rhs)
            }
            Ty::TyApp(lhs, rhs) => {
                let app_lhs = self.apply(lhs);
                let app_rhs = self.apply(rhs);
                Ty::make_app(app_lhs, app_rhs)
            }
            Ty::TyExVar(ex_var_name) => {
                if let Some(ContextEntry::SolvedEVar {
                    lhs: _,
                    rhs: solved_type,
                }) = self.find_solved_evar(ex_var_name.clone())
                {
                    return self.apply(&solved_type);
                };

                ty.clone()
            }
            Ty::TyForAll(quant_var, body) => Ty::make_forall(quant_var.clone(), self.apply(body)),
        }
    }

    pub fn apply(&self, ty: &Ty) -> Ty {
        successors(Some(ty.clone()), |prev| {
            let next = self.apply_once(prev);
            if next != *prev { Some(next) } else { None }
        })
        .last()
        .unwrap()
    }

    pub fn add_entry(self, entry: ContextEntry) -> Context {
        let mut new_ctx_entries = self.entries.clone();
        new_ctx_entries.push(entry);
        Context {
            entries: new_ctx_entries,
        }
    }

    pub fn find_solved_evar(&self, evar_name: String) -> Option<ContextEntry> {
        self.entries
            .iter()
            .find(|ent| {
                matches!(
                    ent, ContextEntry::SolvedEVar { lhs, rhs } if *lhs == evar_name
                )
            })
            .cloned()
    }

    pub fn has_solved_evar(&self, evar_name: String) -> bool {
        self.find_solved_evar(evar_name).is_some()
    }

    pub fn until(&self, entry: &ContextEntry) -> Context {
        let (left, _, _) = self.split_2(entry);
        Context { entries: left }
    }

    pub fn before(&self, entry_a: &ContextEntry, entry_b: &ContextEntry) -> bool {
        let found_a = self.entries.iter().position(|ent| ent == entry_a);
        let found_b = self.entries.iter().position(|ent| ent == entry_b);

        match (found_a, found_b) {
            (Some(a_ix), Some(b_ix)) => a_ix < b_ix,
            _ => false,
        }
    }

    pub fn replace(&self, entry: &ContextEntry, replacement: &ContextEntry) -> Context {
        let mut new_entries = self.entries.clone();
        if let Some(i) = new_entries.iter().rposition(|ent| ent == entry) {
            new_entries[i] = replacement.clone();
        } else {
            panic!("{:?} not found in the context", entry);
        }
        Context {
            entries: new_entries,
        }
    }

    pub fn split_2(
        &self,
        on_entry: &ContextEntry,
    ) -> (Vec<ContextEntry>, &ContextEntry, Vec<ContextEntry>) {
        let found_ix = self.entries.iter().rposition(|ent| ent == on_entry);
        match found_ix {
            Some(ix) => {
                let middle = &self.entries[ix];
                let left = self.entries[..ix].to_vec();
                let right = self.entries[ix + 1..].to_vec();
                (left, middle, right)
            }
            None => {
                panic!("{:?} not found in the context", on_entry);
            }
        }
    }

    pub fn from_2(entries_1: Vec<ContextEntry>, entries_2: Vec<ContextEntry>) -> Context {
        let mut left_ctx = entries_1.clone();
        left_ctx.extend(entries_2);
        Self { entries: left_ctx }
    }

    pub fn remove_in_place(&self, on_entry: &ContextEntry) -> Context {
        let (left_ctx, _, right_ctx) = self.split_2(on_entry);

        Self::from_2(left_ctx, right_ctx)
    }

    pub fn has_evar(&self, evar_name: String) -> bool {
        self.entries
            .iter()
            .find(|ent| {
                matches!(
                    ent, ContextEntry::ExVar(found_evar_name) if *found_evar_name == evar_name
                )
            })
            .is_some()
    }

    pub fn has_var(&self, var_name: String) -> bool {
        self.entries
            .iter()
            .find(|ent| {
                matches!(
                    ent, ContextEntry::MonoVar(found_var_name) if *found_var_name == var_name
                )
            })
            .is_some()
    }

    fn evar_index(&self, name: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| matches!(e, ContextEntry::ExVar(n) if n == name))
    }

    pub fn evar_before(&self, a: &str, b: &str) -> bool {
        match (self.evar_index(a), self.evar_index(b)) {
            (Some(i), Some(j)) => i < j,
            _ => false,
        }
    }

    pub fn get_typed_var(&self, var_name: String) -> Option<&ContextEntry> {
        self.entries.iter().find(
            |ent| matches!(
                ent,
                ContextEntry::TypedVar { var_name: typed_var_name, ty: var_type } if var_name == *typed_var_name )
            )
    }
}
