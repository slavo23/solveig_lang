//! Small constructors for readable primitive type schemes.

use super::*;

pub(super) fn prim_spec(
    short_name: &'static str,
    builtin: BuiltinId,
    arity: usize,
    scheme: Scheme,
) -> PrimitiveBuiltinSpec {
    PrimitiveBuiltinSpec {
        short_name,
        builtin,
        arity,
        scheme,
    }
}

pub(super) fn unary(arg: Ty, ret: Ty) -> Scheme {
    arr(&[arg], ret)
}

pub(super) fn binary(arg: Ty, ret: Ty) -> Scheme {
    arr(&[arg.clone(), arg], ret)
}

pub(super) fn ternary(a: Ty, b: Ty, c: Ty, ret: Ty) -> Scheme {
    arr(&[a, b, c], ret)
}

pub(super) fn arr(args: &[Ty], ret: Ty) -> Scheme {
    Scheme::mono(arr_ty(args, ret))
}

fn arr_ty(args: &[Ty], ret: Ty) -> Ty {
    args.iter()
        .rev()
        .cloned()
        .fold(ret, |body, arg| Ty::make_arr(arg, body))
}

pub(super) fn poly_arr(vars: &[TyVarId], args: &[Ty], ret: Ty) -> Scheme {
    poly(vars, arr_ty(args, ret))
}

pub(super) fn poly(vars: &[TyVarId], ty: Ty) -> Scheme {
    Scheme {
        quantifiers: vars.iter().map(|id| (*id, Kind::Star)).collect(),
        predicates: Vec::new(),
        ty,
    }
}

pub(super) fn io_of(error: Ty, value: Ty) -> Ty {
    Ty::make_app(Ty::make_app(Ty::make_con_id(PRIM_TYS.io), error), value)
}
