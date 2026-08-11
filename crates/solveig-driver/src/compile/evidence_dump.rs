//! Evidence debug dumping for checked modules.
//! The dump walks elaborated core and renders dictionaries with explicit tables.

use super::*;

pub(super) fn dump_solved_evidence(module: &CheckedModule, names: &Names) {
    let tables = PrettyTables::new(names, &module.tycon_table, &module.class_table);
    let mut printed_any = false;

    eprintln!("=== Evidence ===");

    for func in &module.func_defs {
        let mut lines = Vec::new();
        collect_evidence_lines(&func.body, tables, &mut lines);
        printed_any |= print_evidence_block(&func.name, &lines);
    }

    for inst in &module.instance_defs {
        let owner = format!(
            "instance#{} {}",
            inst.id.0,
            inst.head.pretty_with_tables(tables)
        );
        let mut lines = inst
            .context
            .iter()
            .zip(inst.context_evidence.iter())
            .enumerate()
            .map(|(index, (pred, evvar))| {
                format!(
                    "ctx[{index}] %ev{} : {}",
                    evvar.0,
                    pred.pretty_with_tables(tables)
                )
            })
            .collect::<Vec<_>>();
        lines.extend(
            inst.superclass_evidence
                .iter()
                .enumerate()
                .map(|(index, ev)| format!("super[{index}] = {}", format_ev(ev, tables))),
        );
        printed_any |= print_evidence_block(&owner, &lines);

        for method in &inst.methods {
            let mut lines = Vec::new();
            collect_evidence_lines(&method.body, tables, &mut lines);
            printed_any |=
                print_evidence_block(&format!("instance#{}::{}", inst.id.0, method.name), &lines);
        }
    }

    if !printed_any {
        eprintln!("  (none)");
    }
}

fn print_evidence_block(owner: &str, lines: &[String]) -> bool {
    if lines.is_empty() {
        return false;
    }

    eprintln!("{owner}:");
    for line in lines {
        eprintln!("  {line}");
    }
    true
}

fn collect_evidence_lines(
    expr: &TermExprNode<TcMetadata, Range<usize>>,
    tables: PrettyTables<'_>,
    out: &mut Vec<String>,
) {
    match &expr.node {
        TermExpr::EvAbs(evvar, pred, body) => {
            out.push(format!(
                "lambda %ev{} : {}",
                evvar.0,
                pred.pretty_with_tables(tables)
            ));
            collect_evidence_lines(body, tables, out);
        }
        TermExpr::EvApp(body, ev) => {
            out.push(format!("apply {}", format_ev(ev, tables)));
            collect_evidence_lines(body, tables, out);
        }
        TermExpr::App(func, arg) => {
            collect_evidence_lines(func, tables, out);
            collect_evidence_lines(arg, tables, out);
        }
        TermExpr::Abs(_, _, body)
        | TermExpr::TyAbs(_, _, body)
        | TermExpr::TyApp(body, _)
        | TermExpr::Anno(body, _) => collect_evidence_lines(body, tables, out),
        TermExpr::Case(scrutinee, arms) => {
            collect_evidence_lines(scrutinee, tables, out);
            for arm in arms {
                collect_evidence_lines(&arm.node.expr, tables, out);
            }
        }
        TermExpr::Match(scrutinees, arms) => {
            for scrutinee in scrutinees {
                collect_evidence_lines(scrutinee, tables, out);
            }
            for arm in arms {
                collect_evidence_lines(&arm.node.expr, tables, out);
            }
        }
        TermExpr::IfThenElse(cond, then_branch, else_branch) => {
            collect_evidence_lines(cond, tables, out);
            collect_evidence_lines(then_branch, tables, out);
            collect_evidence_lines(else_branch, tables, out);
        }
        TermExpr::Let(_, binder, in_expr) => {
            collect_evidence_lines(binder, tables, out);
            collect_evidence_lines(in_expr, tables, out);
        }
        TermExpr::Unit
        | TermExpr::Var(_)
        | TermExpr::Lit(_)
        | TermExpr::Hole
        | TermExpr::Con(_) => {}
    }
}

fn format_ev(ev: &Ev, tables: PrettyTables<'_>) -> String {
    match ev {
        Ev::Var(evvar) => format!("%ev{}", evvar.0),
        Ev::Placeholder(evvar) => format!("<placeholder %ev{}>", evvar.0),
        Ev::Inst(instance_id, ty_args, sub_evidence) => {
            let ty_args = if ty_args.is_empty() {
                String::new()
            } else {
                format!(
                    " @{}",
                    ty_args
                        .iter()
                        .map(|ty| ty.pretty_with_tables(tables))
                        .collect::<Vec<_>>()
                        .join(" @")
                )
            };
            let sub_evidence = if sub_evidence.is_empty() {
                String::new()
            } else {
                format!(
                    " ({})",
                    sub_evidence
                        .iter()
                        .map(|ev| format_ev(ev, tables))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };

            format!("inst#{}{}{}", instance_id.0, ty_args, sub_evidence)
        }
        Ev::Super(root, class_id, index) => {
            format!(
                "super({}).class#{}.{}",
                format_ev(root, tables),
                class_id.0,
                index
            )
        }
    }
}
