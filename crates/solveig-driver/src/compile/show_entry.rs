//! Show-wrapper synthesis for non-IO `main` entries.
//! The wrapper is checked as ordinary core before lowering.

use super::*;

pub(super) fn entry_returns_io(module: &CheckedModule, names: &Names, entry_name: &str) -> bool {
    entry_def(module, names, entry_name).is_some_and(|(_, scheme)| scheme_returns_io(scheme))
}

fn scheme_returns_io(scheme: &Scheme) -> bool {
    let (_, _, ty) = scheme.ty.peel_scheme_like();
    matches!(
        &ty,
        Ty::TyApp(func, _)
            if matches!(func.as_ref(), Ty::TyApp(io, _)
                if matches!(io.as_ref(), Ty::TyCon(id) if *id == PRIM_TYS.io))
    )
}

const SHOW_ENTRY_NAME: &str = "main.$solveig_show_main";
const SHOW_ENTRY_SHORT_NAME: &str = "$solveig_show_main";

pub(super) fn add_show_entry_wrapper(
    module: &mut CheckedModule,
    checked_project: &mut CheckedProject,
    entry_name: &str,
) -> Option<String> {
    let (entry_id, _) = entry_def(module, &checked_project.names, entry_name)?;
    let show_method_id = show_method_id(module)?;
    let string_ty = Ty::make_con_id(PRIM_TYS.string);
    let expr = show_entry_expr(show_method_id, entry_id, entry_expr_span(module, entry_id));
    let file = checked_project
        .module_files
        .get(&entry_id.module)
        .copied()
        .unwrap_or(FileId(0));
    let elaborated = match check_and_elab_expr_in_checked_module(
        module,
        file,
        &expr,
        &string_ty,
        &checked_project.names,
    ) {
        Ok(Some(expr)) => expr,
        Ok(None) | Err(_) => return None,
    };
    let id = DefId {
        module: entry_id.module,
        name: checked_project.names.symbol(SHOW_ENTRY_SHORT_NAME),
    };
    let scheme = Scheme::mono(string_ty);

    checked_project.names.register_def(id, SHOW_ENTRY_NAME);
    module.func_schemes.insert(id, scheme.clone());
    module.func_defs.push(TermDefinition {
        id,
        name: SHOW_ENTRY_NAME.to_string(),
        short_name: SHOW_ENTRY_SHORT_NAME.to_string(),
        signature: Some(scheme),
        body: elaborated,
    });

    Some(SHOW_ENTRY_NAME.to_string())
}

fn entry_def<'a>(
    module: &'a CheckedModule,
    names: &Names,
    entry_name: &str,
) -> Option<(DefId, &'a Scheme)> {
    module
        .func_schemes
        .iter()
        .find(|(def_id, _)| names.def_display_name(**def_id) == entry_name)
        .map(|(def_id, scheme)| (*def_id, scheme))
}

fn show_method_id(module: &CheckedModule) -> Option<DefId> {
    module
        .class_defs
        .iter()
        .find(|class_def| {
            class_def.name == "prelude.typeclasses.Show" || class_def.short_name == "Show"
        })
        .and_then(|class_def| {
            class_def
                .methods
                .iter()
                .find(|method| method.short_name == "show")
                .map(|method| method.id)
        })
}

fn entry_expr_span(module: &CheckedModule, entry_id: DefId) -> Range<usize> {
    module
        .func_defs
        .iter()
        .find(|func| func.id == entry_id)
        .map(|func| func.body.anno.span.clone())
        .filter(|span| span.start < span.end)
        .unwrap_or(0..1)
}

fn show_entry_expr(
    show_method_id: DefId,
    entry_id: DefId,
    span: Range<usize>,
) -> TermExprNode<Range<usize>> {
    TermExprNode::new(
        TermExpr::App(
            Box::new(TermExprNode::new(
                TermExpr::Var(VarRef::Global(show_method_id)),
                span.clone(),
            )),
            Box::new(TermExprNode::new(
                TermExpr::Var(VarRef::Global(entry_id)),
                span.clone(),
            )),
        ),
        span,
    )
}
