//! Pretty-printing for kinds, types, predicates, and schemes.
//! Table-backed rendering uses `Names`, `TyConTable`, and `ClassTable` explicitly.

use super::*;
use solveig_base::{FxHashMap, FxHashSet};

impl Pretty<Ty> for Ty {
    fn get_precedence(t: &Ty) -> u8 {
        match t {
            Ty::TyForAll(_, _, _) => PRECEDENCE_LOOSE,
            Ty::TyArr(_, _) => PRECEDENCE_ARR,
            Ty::TyApp(_, _) => PRECEDENCE_APP,
            Ty::TyUnit | Ty::TyVar(_) | Ty::TyExVar(_) | Ty::TyCon(_) => PRECEDENCE_ATOM,
        }
    }

    fn pretty(&self) -> String {
        TypePrinter::without_tables().ty(self, PRECEDENCE_LOOSE)
    }
}

#[derive(Clone, Copy)]
pub struct PrettyTables<'a> {
    pub names: &'a Names,
    pub tycons: &'a TyConTable,
    pub classes: &'a ClassTable,
    name_scratch: Option<&'a NameScratch>,
}

impl<'a> PrettyTables<'a> {
    /// Creates table-backed pretty-printing state without speculative names.
    pub fn new(names: &'a Names, tycons: &'a TyConTable, classes: &'a ClassTable) -> Self {
        Self {
            names,
            tycons,
            classes,
            name_scratch: None,
        }
    }

    /// Returns a view that consults speculative type-variable names first.
    pub fn with_name_scratch(mut self, name_scratch: &'a NameScratch) -> Self {
        self.name_scratch = Some(name_scratch);
        self
    }

    fn tyvar_display_name(self, id: TyVarId) -> String {
        self.name_scratch.map_or_else(
            || self.names.tyvar_display_name(id),
            |scratch| scratch.tyvar_display_name(self.names, id),
        )
    }
}

fn tycon_pretty_name(id: TyConId, tables: PrettyTables<'_>) -> String {
    tables.tycons.get(id).map_or_else(
        || tables.names.tycon_display_name(id),
        |info| tables.names.symbol_display_name(info.short_name),
    )
}

fn class_pretty_name(id: ClassId, tables: PrettyTables<'_>) -> String {
    tables.classes.get(id).map_or_else(
        || tables.names.class_display_name(id),
        |info| tables.names.symbol_display_name(info.short_name),
    )
}

struct TypePrinter<'a> {
    tables: Option<PrettyTables<'a>>,
    tyvars: FxHashMap<TyVarId, String>,
    used_tyvars: FxHashSet<String>,
    star_index: usize,
    higher_index: usize,
    fallback_index: usize,
}

impl<'a> TypePrinter<'a> {
    fn without_tables() -> Self {
        Self::new(None)
    }

    fn with_tables(tables: PrettyTables<'a>) -> Self {
        Self::new(Some(tables))
    }

    fn new(tables: Option<PrettyTables<'a>>) -> Self {
        Self {
            tables,
            tyvars: FxHashMap::default(),
            used_tyvars: FxHashSet::default(),
            star_index: 0,
            higher_index: 0,
            fallback_index: 0,
        }
    }

    fn ty(&mut self, ty: &Ty, parent_precedence: u8) -> String {
        if let Some(list_arg) = self.list_arg(ty) {
            let text = format!("[{}]", self.ty(list_arg, PRECEDENCE_LOOSE));
            return self.parenthesize(PRECEDENCE_ATOM, parent_precedence, text);
        }

        let current_precedence = Ty::get_precedence(ty);
        let text = match ty {
            Ty::TyUnit => "()".to_string(),
            Ty::TyVar(id) => self.tyvar_name(*id),
            Ty::TyExVar(id) => evar_display_name(*id),
            Ty::TyCon(id) => self.tycon_name(*id),
            Ty::TyArr(lhs, rhs) => {
                let lhs = self.ty(lhs, PRECEDENCE_APP);
                let rhs = self.ty(rhs, PRECEDENCE_ARR);
                format!("{lhs} -> {rhs}")
            }
            Ty::TyForAll(_, _, _) => {
                let (quantifiers, predicates, body) = ty.peel_scheme_like();
                self.bind_quantifiers(&quantifiers, &predicates);
                self.scheme_parts(&quantifiers, &predicates, &body)
            }
            Ty::TyApp(func, arg) => {
                let func = self.ty(func, PRECEDENCE_APP);
                let arg = self.ty(arg, PRECEDENCE_ATOM);
                format!("{func} {arg}")
            }
        };

        self.parenthesize(current_precedence, parent_precedence, text)
    }

    fn scheme(&mut self, scheme: &Scheme) -> String {
        self.bind_quantifiers(&scheme.quantifiers, &scheme.predicates);
        self.scheme_parts(&scheme.quantifiers, &scheme.predicates, &scheme.ty)
    }

    fn scheme_parts(
        &mut self,
        quantifiers: &[(TyVarId, Kind)],
        predicates: &[Predicate],
        body: &Ty,
    ) -> String {
        let prefix = if quantifiers.is_empty() {
            String::new()
        } else {
            let names = quantifiers
                .iter()
                .map(|(id, _)| self.tyvar_name(*id))
                .collect::<Vec<_>>();
            format!("forall {}. ", names.join(" "))
        };

        if predicates.is_empty() {
            format!("{prefix}{}", self.ty(body, PRECEDENCE_LOOSE))
        } else {
            let predicates = if predicates.len() == 1 {
                self.predicate(&predicates[0])
            } else {
                format!(
                    "({})",
                    predicates
                        .iter()
                        .map(|pred| self.predicate(pred))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };

            format!(
                "{prefix}{predicates} => {}",
                self.ty(body, PRECEDENCE_LOOSE)
            )
        }
    }

    fn predicate(&mut self, pred: &Predicate) -> String {
        let class_name = self.class_name(pred.class_id);
        let args = pred
            .args
            .iter()
            .map(|arg| {
                let text = self.ty(arg, PRECEDENCE_LOOSE);
                match arg {
                    Ty::TyArr(_, _) | Ty::TyForAll(_, _, _) => format!("({text})"),
                    _ => text,
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        if args.is_empty() {
            class_name
        } else {
            format!("{class_name} {args}")
        }
    }

    fn bind_quantifiers(&mut self, quantifiers: &[(TyVarId, Kind)], predicates: &[Predicate]) {
        let hints = self.quantifier_hints(predicates);
        for (id, kind) in quantifiers {
            if self.tyvars.contains_key(id) {
                continue;
            }

            let preferred = self
                .source_tyvar_name(*id)
                .and_then(|name| self.claim_tyvar_name(&name))
                .or_else(|| hints.get(id).and_then(|hint| self.claim_tyvar_name(hint)))
                .unwrap_or_else(|| self.next_tyvar_name(kind));
            self.tyvars.insert(*id, preferred);
        }
    }

    fn quantifier_hints(&self, predicates: &[Predicate]) -> FxHashMap<TyVarId, &'static str> {
        let mut hints = FxHashMap::default();

        for predicate in predicates {
            let Some(Ty::TyVar(id)) = predicate.args.first() else {
                continue;
            };
            let hint = match self.class_name(predicate.class_id).as_str() {
                "Monad" => Some("m"),
                "Functor" | "Applicative" | "Alternative" | "Traversable" => Some("f"),
                _ => None,
            };
            if let Some(hint) = hint {
                hints.entry(*id).or_insert(hint);
            }
        }

        hints
    }

    fn tyvar_name(&mut self, id: TyVarId) -> String {
        if let Some(name) = self.tyvars.get(&id) {
            return name.clone();
        }

        let name = self
            .source_tyvar_name(id)
            .and_then(|name| self.claim_tyvar_name(&name))
            .unwrap_or_else(|| self.next_tyvar_name(&Kind::Star));
        self.tyvars.insert(id, name.clone());
        name
    }

    fn source_tyvar_name(&self, id: TyVarId) -> Option<String> {
        let name = self.tables.map_or_else(
            || tyvar_display_name(id),
            |tables| tables.tyvar_display_name(id),
        );

        if is_generated_tyvar_name(&name) || !is_source_tyvar_name(&name) {
            None
        } else {
            Some(name)
        }
    }

    fn claim_tyvar_name(&mut self, name: &str) -> Option<String> {
        if self.used_tyvars.insert(name.to_string()) {
            Some(name.to_string())
        } else {
            None
        }
    }

    fn next_tyvar_name(&mut self, kind: &Kind) -> String {
        if matches!(kind, Kind::Star) {
            loop {
                let name = star_tyvar_name(self.star_index);
                self.star_index += 1;
                if self.used_tyvars.insert(name.clone()) {
                    return name;
                }
            }
        }

        loop {
            let name = higher_tyvar_name(self.higher_index)
                .unwrap_or_else(|| fallback_tyvar_name(self.fallback_index));
            if self.higher_index < HIGHER_TYVAR_NAMES.len() {
                self.higher_index += 1;
            } else {
                self.fallback_index += 1;
            }
            if self.used_tyvars.insert(name.clone()) {
                return name;
            }
        }
    }

    fn list_arg<'b>(&self, ty: &'b Ty) -> Option<&'b Ty> {
        let Ty::TyApp(func, arg) = ty else {
            return None;
        };

        self.is_prelude_list_tycon(func).then_some(arg)
    }

    fn is_prelude_list_tycon(&self, ty: &Ty) -> bool {
        let Ty::TyCon(id) = ty else {
            return false;
        };

        self.tables.is_some_and(|tables| {
            tables.tycons.get(*id).is_some_and(|info| {
                tables.names.symbol_display_name(info.name) == PRELUDE_LIST_NAME
            }) || tables.names.tycon_display_name(*id) == PRELUDE_LIST_NAME
        })
    }

    fn tycon_name(&self, id: TyConId) -> String {
        self.tables.map_or_else(
            || tycon_fallback_name(id),
            |tables| tycon_pretty_name(id, tables),
        )
    }

    fn class_name(&self, id: ClassId) -> String {
        self.tables.map_or_else(
            || class_fallback_name(id),
            |tables| class_pretty_name(id, tables),
        )
    }

    fn parenthesize(&self, current_precedence: u8, parent_precedence: u8, text: String) -> String {
        if current_precedence < parent_precedence {
            format!("({text})")
        } else {
            text
        }
    }
}

fn is_generated_tyvar_name(name: &str) -> bool {
    name.strip_prefix('t')
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit()))
}

fn is_source_tyvar_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    first.is_ascii_lowercase()
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '\'')
}

fn star_tyvar_name(index: usize) -> String {
    const LETTERS: &[u8; 26] = b"abcdefghijklmnopqrstuvwxyz";
    LETTERS.get(index).map_or_else(
        || format!("t{}", index - LETTERS.len()),
        |letter| char::from(*letter).to_string(),
    )
}

const HIGHER_TYVAR_NAMES: &[&str] = &["f", "g", "h", "m", "n"];

fn higher_tyvar_name(index: usize) -> Option<String> {
    HIGHER_TYVAR_NAMES
        .get(index)
        .map(|name| (*name).to_string())
}

fn fallback_tyvar_name(index: usize) -> String {
    format!("t{index}")
}

impl Ty {
    pub fn pretty_with_tables(&self, tables: PrettyTables<'_>) -> String {
        TypePrinter::with_tables(tables).ty(self, PRECEDENCE_LOOSE)
    }
}

impl Predicate {
    pub fn pretty_with_tables(&self, tables: PrettyTables<'_>) -> String {
        TypePrinter::with_tables(tables).predicate(self)
    }
}

impl Scheme {
    pub fn pretty_with_tables(&self, tables: PrettyTables<'_>) -> String {
        TypePrinter::with_tables(tables).scheme(self)
    }
}
