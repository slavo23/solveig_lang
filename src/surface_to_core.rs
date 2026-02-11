use core::{
    core_expr::{CoreDefinition, CoreExpr},
    core_type::Ty,
};
use std::collections::HashMap;
use surface::ast::{SpannedWrapper, SurfaceDefinition, SurfaceExpr, SurfaceProgram, SurfaceTy};

pub fn convert_surface_to_core_ty<'src>(sf_ty: &SpannedWrapper<SurfaceTy<'src>>) -> Ty {
    match &**sf_ty {
        SurfaceTy::Var(var_name) => Ty::TyVar(var_name.to_string()),
        SurfaceTy::Arr(lhs, rhs) => Ty::make_arr(
            convert_surface_to_core_ty(&**lhs),
            convert_surface_to_core_ty(&**rhs),
        ),
        SurfaceTy::ForAll(quants, body) => quants
            .iter()
            .rev()
            .fold(convert_surface_to_core_ty(&**body), |acc, quant| {
                Ty::make_forall((*quant).to_string(), acc)
            }),
        SurfaceTy::Unit => Ty::TyUnit,
        SurfaceTy::Con(ctor_name) => Ty::TyCon(ctor_name.to_string()),
        SurfaceTy::App(lhs, rhs) => Ty::make_app(
            convert_surface_to_core_ty(&**lhs),
            convert_surface_to_core_ty(&**rhs),
        ),
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
        SurfaceExpr::Con(ctor_name) => CoreExpr::Con(ctor_name.to_string()),
    }
}

pub fn convert_to_core_program<'src>(sf_prog: SurfaceProgram<'src>) -> Vec<CoreDefinition> {
    for data_def in sf_prog.iter() {
        if let SurfaceDefinition::DataDef {
            name,
            ty_params,
            constructors,
        } = &**data_def
        {
            
        } else {
            continue;
        }
    }

    let signatures = sf_prog.iter().fold(
        HashMap::new(),
        |mut sig_map: HashMap<String, Ty>, curr| match &**curr {
            SurfaceDefinition::SigDef { name, ty } => {
                sig_map.insert((*name).to_string(), convert_surface_to_core_ty(ty));
                sig_map
            }
            _ => sig_map,
        },
    );

    let mut func_defs = vec![];

    for defn in sf_prog.iter() {
        match &**defn {
            SurfaceDefinition::FunDef { name, body, params } => {
                if let Some(func_signature) = signatures.get(**name) {
                    let mut core_body = convert_to_core_expr(body);
                    for param in params.iter().rev() {
                        core_body = CoreExpr::Abs((*param).to_string(), Box::new(core_body))
                    }

                    func_defs.push(CoreDefinition::FunDef {
                        name: (**name).to_string(),
                        signature: func_signature.clone(),
                        body: core_body,
                    });
                }
            }
            _ => continue,
        }
    }

    func_defs
}
