use core::core_expr::CoreExpr;
use core::core_type::Ty;
use surface::ast::{SpannedWrapper, SurfaceExpr, SurfaceTy};

pub fn convert_surface_to_core_ty<'src>(sf_ty: &SpannedWrapper<SurfaceTy<'src>>) -> Ty {
    match &**sf_ty {
        SurfaceTy::Var(var_name) => Ty::TyVar(var_name.to_string()),
        SurfaceTy::Arr(lhs, rhs) => Ty::make_arr(
            convert_surface_to_core_ty(&**lhs),
            convert_surface_to_core_ty(&**rhs),
        ),
        SurfaceTy::ForAll(quants, body) => {
            // TODO: handle multiple args

            Ty::TyForAll(
                (*quants[0]).into(),
                Box::new(convert_surface_to_core_ty(&**body)),
            )
        }
        SurfaceTy::Unit => Ty::TyUnit,
    }
}

pub fn convert_to_core_expr<'src>(sf_expr: &SpannedWrapper<SurfaceExpr<'src>>) -> CoreExpr {
    match &**sf_expr {
        SurfaceExpr::Var(var_name) => CoreExpr::Var(var_name.to_string()),
        SurfaceExpr::Abs(arg_name, body) => CoreExpr::Abs(
            arg_name.to_string(),
            Box::new(convert_to_core_expr(&**body)),
        ),
        SurfaceExpr::Ann(ann_expr, ann_ty) => CoreExpr::Anno(
            Box::new(convert_to_core_expr(&**ann_expr)),
            convert_surface_to_core_ty(ann_ty),
        ),
        SurfaceExpr::App(lhs, rhs) => {
            CoreExpr::app(convert_to_core_expr(lhs), convert_to_core_expr(rhs))
        }
        SurfaceExpr::Unit => CoreExpr::Unit,
        SurfaceExpr::Let(bind_name, body, then) => CoreExpr::Let(
            bind_name.to_string(),
            Box::new(convert_to_core_expr(&**body)),
            Box::new(convert_to_core_expr(&**then)),
        ),
    }
}

pub fn convert_to_core_program<'src>(
    sf_prog: Vec<SpannedWrapper<SurfaceExpr<'src>>>,
) -> Vec<CoreExpr> {
    sf_prog
        .iter()
        .map(|prog_entry| convert_to_core_expr(prog_entry))
        .collect()
}
