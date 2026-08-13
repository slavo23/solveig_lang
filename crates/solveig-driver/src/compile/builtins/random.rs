//! Type schemes for immutable random-generator primitives.

use super::*;

pub(super) fn random_builtin_specs(names: &mut Names, int: &Ty) -> Vec<PrimitiveBuiltinSpec> {
    let std_gen = Ty::make_con_id(PRIM_TYS.std_gen);
    let pair_int_gen = Ty::make_app(
        Ty::make_app(Ty::make_con_id(prelude_pair_tycon_id(names)), int.clone()),
        std_gen.clone(),
    );
    let e_id = names.fresh_tyvar("e");

    vec![
        prim_spec(
            "mkStdGen",
            BUILTIN_MK_STD_GEN,
            1,
            unary(int.clone(), std_gen.clone()),
        ),
        prim_spec(
            "newStdGen",
            BUILTIN_NEW_STD_GEN,
            0,
            poly(&[e_id], io_of(Ty::TyVar(e_id), std_gen.clone())),
        ),
        prim_spec(
            "randomInt",
            BUILTIN_RANDOM_INT,
            1,
            unary(std_gen.clone(), pair_int_gen.clone()),
        ),
        prim_spec(
            "randomRange",
            BUILTIN_RANDOM_RANGE,
            3,
            arr(&[int.clone(), int.clone(), std_gen], pair_int_gen),
        ),
    ]
}
