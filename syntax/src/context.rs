use core::core_type::Ty;

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
        return Context { entries: entries };
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
        for i in 0..new_entries.len() {
            if new_entries[i] == *entry {
                new_entries[i] = replacement.clone();
            }
        }

        Context {
            entries: new_entries,
        }
    }

    pub fn split_2(
        &self,
        on_entry: &ContextEntry,
    ) -> (Vec<ContextEntry>, &ContextEntry, Vec<ContextEntry>) {
        let found_ix = self.entries.iter().position(|ent| ent == on_entry);
        match found_ix {
            Some(ix) => {
                let middle = &self.entries[ix];
                let entries_copy = self.entries.clone();

                let left = entries_copy.get(0..ix).unwrap_or_default().to_vec();
                let right = &entries_copy.get(ix + 1..).unwrap_or_default().to_vec();

                (left.clone(), middle, right.clone())
            }
            None => {
                let err = format!("{:?}", on_entry);
                panic!("{} not found in the context", err);
            }
        }
    }

    pub fn from_2(entries_1: Vec<ContextEntry>, entries_2: Vec<ContextEntry>) -> Context {
        let mut left_ctx = entries_1.clone();
        left_ctx.extend(entries_2);
        Self { entries: left_ctx }
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

    pub fn get_typed_var(&self, var_name: String) -> Option<&ContextEntry> {
        self.entries.iter().find(
            |ent| matches!(
                ent,
                ContextEntry::TypedVar { var_name: typed_var_name, ty: var_type } if var_name == *typed_var_name )
            )
    }
}
