//! Pretty-printing for kinds, types, predicates, and schemes.
//! Table-backed rendering uses `Names`, `TyConTable`, and `ClassTable` explicitly.

use super::*;

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
        fn traverse_helper(t: &Ty, parent_precedence: u8) -> String {
            let curr_precedence = Ty::get_precedence(t);

            let type_fmt = match t {
                Ty::TyUnit => "()".into(),
                Ty::TyVar(id) => tyvar_display_name(*id),
                Ty::TyExVar(id) => evar_display_name(*id),
                Ty::TyCon(id) => tycon_fallback_name(*id),

                Ty::TyArr(lhs, rhs) => {
                    let lhs_fmt = traverse_helper(lhs, PRECEDENCE_APP);
                    let rhs_fmt = traverse_helper(rhs, PRECEDENCE_ARR);
                    format!("{lhs_fmt} -> {rhs_fmt}")
                }

                Ty::TyForAll(_, _, _) => {
                    let (kinded_quants, preds, rest) = t.peel_scheme_like();

                    let peeled_quants = kinded_quants
                        .iter()
                        .map(|(id, kind)| {
                            format!("({} : {})", tyvar_display_name(*id), kind.pretty())
                        })
                        .collect::<Vec<_>>();

                    let prefix = if peeled_quants.is_empty() {
                        String::new()
                    } else {
                        format!("∀ {}. ", peeled_quants.join(" "))
                    };

                    if preds.is_empty() {
                        format!("{prefix}{}", traverse_helper(&rest, PRECEDENCE_LOOSE))
                    } else {
                        let preds_fmt = if preds.len() == 1 {
                            preds[0].pretty()
                        } else {
                            format!(
                                "({})",
                                preds
                                    .iter()
                                    .map(|p| p.pretty())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        };

                        format!(
                            "{prefix}{preds_fmt} => {}",
                            traverse_helper(&rest, PRECEDENCE_LOOSE)
                        )
                    }
                }

                Ty::TyApp(f, x) => {
                    let f_s = traverse_helper(f, PRECEDENCE_APP);
                    let x_s = traverse_helper(x, PRECEDENCE_ATOM);
                    format!("{f_s} {x_s}")
                }
            };

            if curr_precedence < parent_precedence {
                format!("({type_fmt})")
            } else {
                type_fmt
            }
        }

        traverse_helper(self, PRECEDENCE_LOOSE)
    }
}

#[derive(Clone, Copy)]
pub struct PrettyTables<'a> {
    pub names: &'a Names,
    pub tycons: &'a TyConTable,
    pub classes: &'a ClassTable,
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

impl Ty {
    pub fn pretty_with_tables(&self, tables: PrettyTables<'_>) -> String {
        fn traverse_helper(t: &Ty, parent_precedence: u8, tables: PrettyTables<'_>) -> String {
            let curr_precedence = Ty::get_precedence(t);

            let type_fmt = match t {
                Ty::TyUnit => "()".into(),
                Ty::TyVar(id) => tables.names.tyvar_display_name(*id),
                Ty::TyExVar(id) => evar_display_name(*id),
                Ty::TyCon(id) => tycon_pretty_name(*id, tables),

                Ty::TyArr(lhs, rhs) => {
                    let lhs_fmt = traverse_helper(lhs, PRECEDENCE_APP, tables);
                    let rhs_fmt = traverse_helper(rhs, PRECEDENCE_ARR, tables);
                    format!("{lhs_fmt} -> {rhs_fmt}")
                }

                Ty::TyForAll(_, _, _) => {
                    let (kinded_quants, preds, rest) = t.peel_scheme_like();

                    let peeled_quants = kinded_quants
                        .iter()
                        .map(|(id, kind)| {
                            format!(
                                "({} : {})",
                                tables.names.tyvar_display_name(*id),
                                kind.pretty()
                            )
                        })
                        .collect::<Vec<_>>();

                    let prefix = if peeled_quants.is_empty() {
                        String::new()
                    } else {
                        format!("∀ {}. ", peeled_quants.join(" "))
                    };

                    if preds.is_empty() {
                        format!(
                            "{prefix}{}",
                            traverse_helper(&rest, PRECEDENCE_LOOSE, tables)
                        )
                    } else {
                        let preds_fmt = if preds.len() == 1 {
                            preds[0].pretty_with_tables(tables)
                        } else {
                            format!(
                                "({})",
                                preds
                                    .iter()
                                    .map(|p| p.pretty_with_tables(tables))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        };

                        format!(
                            "{prefix}{preds_fmt} => {}",
                            traverse_helper(&rest, PRECEDENCE_LOOSE, tables)
                        )
                    }
                }

                Ty::TyApp(f, x) => {
                    let f_s = traverse_helper(f, PRECEDENCE_APP, tables);
                    let x_s = traverse_helper(x, PRECEDENCE_ATOM, tables);
                    format!("{f_s} {x_s}")
                }
            };

            if curr_precedence < parent_precedence {
                format!("({type_fmt})")
            } else {
                type_fmt
            }
        }

        traverse_helper(self, PRECEDENCE_LOOSE, tables)
    }
}

impl Predicate {
    pub fn pretty_with_tables(&self, tables: PrettyTables<'_>) -> String {
        let class_name = class_pretty_name(self.class_id, tables);

        let args = self
            .args
            .iter()
            .map(|arg| {
                let s = arg.pretty_with_tables(tables);
                match arg {
                    Ty::TyArr(_, _) | Ty::TyForAll(_, _, _) => format!("({s})"),
                    _ => s,
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
}

impl Scheme {
    pub fn pretty_with_tables(&self, tables: PrettyTables<'_>) -> String {
        self.to_ty().pretty_with_tables(tables)
    }
}
