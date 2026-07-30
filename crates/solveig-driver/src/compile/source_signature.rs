//! User-facing source signature rendering for editor edits and hints.

use super::*;

const PRECEDENCE_LOOSE: u8 = 0;
const PRECEDENCE_ARR: u8 = 1;
const PRECEDENCE_APP: u8 = 2;
const PRECEDENCE_ATOM: u8 = 3;

pub(super) fn pretty_source_scheme(scheme: &Scheme, tables: PrettyTables<'_>) -> String {
    let mut printer = SourceSignaturePrinter::new(tables);
    printer.bind_quantifiers(&scheme.quantifiers, &scheme.predicates);
    printer.scheme_parts(&scheme.quantifiers, &scheme.predicates, &scheme.ty)
}

pub(super) fn pretty_source_ty(ty: &Ty, tables: PrettyTables<'_>) -> String {
    let mut printer = SourceSignaturePrinter::new(tables);
    printer.ty(ty, PRECEDENCE_LOOSE)
}

struct SourceSignaturePrinter<'a> {
    tables: PrettyTables<'a>,
    tyvars: FxHashMap<TyVarId, String>,
    used: FxHashSet<String>,
    star_index: usize,
    higher_index: usize,
    fallback_index: usize,
}

impl<'a> SourceSignaturePrinter<'a> {
    fn new(tables: PrettyTables<'a>) -> Self {
        Self {
            tables,
            tyvars: FxHashMap::default(),
            used: FxHashSet::default(),
            star_index: 0,
            higher_index: 0,
            fallback_index: 0,
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
                .and_then(|name| self.claim_name(&name))
                .or_else(|| hints.get(id).and_then(|hint| self.claim_name(hint)))
                .unwrap_or_else(|| self.next_name_for_kind(kind));
            self.tyvars.insert(*id, preferred);
        }
    }

    fn source_tyvar_name(&self, id: TyVarId) -> Option<String> {
        let name = self.tables.names.tyvar_display_name(id);
        if is_generated_tyvar_name(&name) || !is_source_tyvar_name(&name) {
            None
        } else {
            Some(name)
        }
    }

    fn quantifier_hints(&self, predicates: &[Predicate]) -> FxHashMap<TyVarId, &'static str> {
        let mut hints = FxHashMap::default();

        for pred in predicates {
            let Some(Ty::TyVar(id)) = pred.args.first() else {
                continue;
            };
            let class_name = source_class_name(pred.class_id, self.tables);
            let hint = match class_name.as_str() {
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

    fn claim_name(&mut self, name: &str) -> Option<String> {
        if self.used.insert(name.to_string()) {
            Some(name.to_string())
        } else {
            None
        }
    }

    fn next_name_for_kind(&mut self, kind: &Kind) -> String {
        if matches!(kind, Kind::Star) {
            loop {
                let name = star_tyvar_name(self.star_index);
                self.star_index += 1;
                if self.used.insert(name.clone()) {
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
            if self.used.insert(name.clone()) {
                return name;
            }
        }
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

    fn ty(&mut self, ty: &Ty, parent_precedence: u8) -> String {
        let current_precedence = type_precedence(ty);
        let text = match ty {
            Ty::TyUnit => "()".to_string(),
            Ty::TyVar(id) => self.tyvar_name(*id),
            Ty::TyExVar(_) => "_".to_string(),
            Ty::TyCon(id) => source_tycon_name(*id, self.tables),
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

        if current_precedence < parent_precedence {
            format!("({text})")
        } else {
            text
        }
    }

    fn predicate(&mut self, pred: &Predicate) -> String {
        let class = source_class_name(pred.class_id, self.tables);
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
            .collect::<Vec<_>>();

        if args.is_empty() {
            class
        } else {
            format!("{class} {}", args.join(" "))
        }
    }

    fn tyvar_name(&self, id: TyVarId) -> String {
        self.tyvars
            .get(&id)
            .cloned()
            .unwrap_or_else(|| self.tables.names.tyvar_display_name(id))
    }
}

fn type_precedence(ty: &Ty) -> u8 {
    match ty {
        Ty::TyForAll(_, _, _) => PRECEDENCE_LOOSE,
        Ty::TyArr(_, _) => PRECEDENCE_ARR,
        Ty::TyApp(_, _) => PRECEDENCE_APP,
        Ty::TyUnit | Ty::TyVar(_) | Ty::TyExVar(_) | Ty::TyCon(_) => PRECEDENCE_ATOM,
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

fn source_tycon_name(id: TyConId, tables: PrettyTables<'_>) -> String {
    tables.tycons.get(id).map_or_else(
        || tables.names.tycon_display_name(id),
        |info| tables.names.symbol_display_name(info.short_name),
    )
}

fn source_class_name(id: ClassId, tables: PrettyTables<'_>) -> String {
    tables.classes.get(id).map_or_else(
        || tables.names.class_display_name(id),
        |info| tables.names.symbol_display_name(info.short_name),
    )
}
