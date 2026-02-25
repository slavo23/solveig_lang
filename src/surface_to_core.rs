use chumsky::input::Input;
use core::{
    core_expr::{CoreConstructor, CoreDataDef, CoreDefinition, CoreExpr},
    core_type::{Kind, Ty},
};
use std::collections::HashMap;
use surface::ast::{
    SpannedWrapper, SufraceConstructor, SurfaceDefinition, SurfaceExpr, SurfaceProgram, SurfaceTy,
};

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
        SurfaceExpr::Con(ctor_name, args) => CoreExpr::Con(
            ctor_name.to_string(),
            args.iter().map(|e| convert_to_core_expr(e)).collect(),
        ),
    }
}

fn infer_data_def_type(name: &SpannedWrapper<&str>, ty_params: &Vec<SpannedWrapper<&str>>) -> Ty {
    if ty_params.is_empty() {
        Ty::TyCon((*name).to_string())
    } else {
        ty_params
            .iter()
            .fold(Ty::TyCon((*name).to_string()), |acc, param| {
                Ty::make_app(acc, Ty::make_var((*param).to_string()))
            })
    }
}

fn infer_data_def_kind(ty_params: &Vec<SpannedWrapper<&str>>) -> Kind {
    ty_params
        .iter()
        .fold(Kind::KStar, |acc, _| Kind::make_arrow(Kind::KStar, acc))
}

fn quantify_surface_constructor(
    ty_params: &Vec<SpannedWrapper<&str>>,
    constructor: &SpannedWrapper<SufraceConstructor>,
    result_type: &Ty,
) -> Ty {
    let variant_type = constructor
        .params
        .iter()
        .map(|p| convert_surface_to_core_ty(p))
        .rev()
        .fold(result_type.clone(), |acc, ty| Ty::make_arr(ty, acc));

    ty_params.iter().rev().fold(variant_type, |acc, param| {
        Ty::make_forall((*param).to_string(), acc)
    })
}

fn convert_to_core_data_def(
    name: &SpannedWrapper<&str>,
    ty_params: &Vec<SpannedWrapper<&str>>,
    constructors: &Vec<SpannedWrapper<SufraceConstructor>>,
) -> CoreDataDef {
    let ctor_kind = infer_data_def_kind(ty_params);
    let data_def_ty = infer_data_def_type(name, ty_params);

    let core_ctors = constructors
        .iter()
        .map(|ctor| {
            let ctor_ty = quantify_surface_constructor(ty_params, ctor, &data_def_ty);
            CoreConstructor {
                name: ctor.name.to_string(),
                ty: ctor_ty,
            }
        })
        .collect::<Vec<_>>();

    CoreDataDef {
        name: (*name).to_string(),
        kind: ctor_kind,
        ctors: core_ctors,
    }
}

pub fn convert_to_core_program<'src>(sf_prog: SurfaceProgram<'src>) -> Vec<CoreDefinition> {
    let mut data_defs = vec![];

    for data_def in sf_prog.iter() {
        if let SurfaceDefinition::DataDef {
            name,
            ty_params,
            constructors,
        } = &**data_def
        {
            data_defs.push(convert_to_core_data_def(name, ty_params, constructors));
        } else {
            continue;
        }
    }

    println!("data defs {:#?}", data_defs);

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
                        core_body = CoreExpr::Abs((*param).to_string(), Box::new(core_body));
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
