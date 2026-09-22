use super::*;
use solveig_ast::ast::{
    BUILTIN_BIG_INT_ABS, BUILTIN_BIG_INT_ADD, BUILTIN_BIG_INT_COMPARE, BUILTIN_BIG_INT_DIV,
    BUILTIN_BIG_INT_EQ, BUILTIN_BIG_INT_FROM_INT, BUILTIN_BIG_INT_MOD, BUILTIN_BIG_INT_MUL,
    BUILTIN_BIG_INT_NEG, BUILTIN_BIG_INT_PARSE, BUILTIN_BIG_INT_PARSE_BASE, BUILTIN_BIG_INT_POW,
    BUILTIN_BIG_INT_QUOT, BUILTIN_BIG_INT_REM, BUILTIN_BIG_INT_SIGNUM, BUILTIN_BIG_INT_SUB,
    BUILTIN_BIG_INT_TO_FLOAT, BUILTIN_BIG_INT_TO_INT, BUILTIN_BIG_INT_TO_STRING,
    BUILTIN_BIG_INT_TO_STRING_BASE, BUILTIN_FLOAT_ABS, BUILTIN_FLOAT_ACOS, BUILTIN_FLOAT_ASIN,
    BUILTIN_FLOAT_ATAN, BUILTIN_FLOAT_ATAN2, BUILTIN_FLOAT_CBRT, BUILTIN_FLOAT_CEIL,
    BUILTIN_FLOAT_COPY_SIGN, BUILTIN_FLOAT_COS, BUILTIN_FLOAT_COSH, BUILTIN_FLOAT_EXP,
    BUILTIN_FLOAT_EXP2, BUILTIN_FLOAT_FLOOR, BUILTIN_FLOAT_HYPOT, BUILTIN_FLOAT_IS_FINITE,
    BUILTIN_FLOAT_IS_INFINITE, BUILTIN_FLOAT_IS_NAN, BUILTIN_FLOAT_LOG, BUILTIN_FLOAT_LOG2,
    BUILTIN_FLOAT_LOG10, BUILTIN_FLOAT_POW, BUILTIN_FLOAT_ROUND, BUILTIN_FLOAT_SIGNUM,
    BUILTIN_FLOAT_SIN, BUILTIN_FLOAT_SINH, BUILTIN_FLOAT_SQRT, BUILTIN_FLOAT_TAN,
    BUILTIN_FLOAT_TANH, BUILTIN_FLOAT_TO_BIG_INT, BUILTIN_FLOAT_TO_INT_MAYBE,
    BUILTIN_FLOAT_TRUNCATE, BUILTIN_INT_ABS, BUILTIN_INT_MIX_HASH, BUILTIN_INT_POW,
    BUILTIN_INT_QUOT, BUILTIN_INT_REM, BUILTIN_INT_SIGNUM,
};

pub(super) fn numeric_builtin_specs(
    names: &mut Names,
    int: &Ty,
    float: &Ty,
    bool_ty: &Ty,
    ordering: &Ty,
) -> Vec<PrimitiveBuiltinSpec> {
    let big_int = Ty::make_con_id(PRIM_TYS.big_int);
    let maybe_int = Ty::make_app(Ty::make_con_id(prelude_maybe_tycon_id(names)), int.clone());
    let maybe_big_int = Ty::make_app(
        Ty::make_con_id(prelude_maybe_tycon_id(names)),
        big_int.clone(),
    );
    let string = Ty::make_con_id(PRIM_TYS.string);
    let maybe_string = Ty::make_app(
        Ty::make_con_id(prelude_maybe_tycon_id(names)),
        string.clone(),
    );
    let mut specs = vec![
        prim_spec(
            "intQuot",
            BUILTIN_INT_QUOT,
            2,
            binary(int.clone(), int.clone()),
        ),
        prim_spec(
            "intRem",
            BUILTIN_INT_REM,
            2,
            binary(int.clone(), int.clone()),
        ),
        prim_spec(
            "intAbs",
            BUILTIN_INT_ABS,
            1,
            unary(int.clone(), int.clone()),
        ),
        prim_spec(
            "intSignum",
            BUILTIN_INT_SIGNUM,
            1,
            unary(int.clone(), int.clone()),
        ),
        prim_spec(
            "intPow",
            BUILTIN_INT_POW,
            2,
            arr(&[int.clone(), int.clone()], int.clone()),
        ),
        prim_spec(
            "intMixHash",
            BUILTIN_INT_MIX_HASH,
            2,
            binary(int.clone(), int.clone()),
        ),
        prim_spec(
            "bigIntFromInt",
            BUILTIN_BIG_INT_FROM_INT,
            1,
            unary(int.clone(), big_int.clone()),
        ),
        prim_spec(
            "bigIntToInt",
            BUILTIN_BIG_INT_TO_INT,
            1,
            unary(big_int.clone(), maybe_int),
        ),
        prim_spec(
            "bigIntToFloat",
            BUILTIN_BIG_INT_TO_FLOAT,
            1,
            unary(big_int.clone(), float.clone()),
        ),
        prim_spec(
            "bigIntToString",
            BUILTIN_BIG_INT_TO_STRING,
            1,
            unary(big_int.clone(), string.clone()),
        ),
        prim_spec(
            "bigIntParse",
            BUILTIN_BIG_INT_PARSE,
            1,
            unary(string.clone(), maybe_big_int.clone()),
        ),
        prim_spec(
            "bigIntParseBase",
            BUILTIN_BIG_INT_PARSE_BASE,
            2,
            arr(&[int.clone(), string], maybe_big_int),
        ),
        prim_spec(
            "bigIntToStringBase",
            BUILTIN_BIG_INT_TO_STRING_BASE,
            2,
            arr(&[int.clone(), big_int.clone()], maybe_string),
        ),
        prim_spec(
            "floatToIntMaybe",
            BUILTIN_FLOAT_TO_INT_MAYBE,
            1,
            unary(
                float.clone(),
                Ty::make_app(Ty::make_con_id(prelude_maybe_tycon_id(names)), int.clone()),
            ),
        ),
        prim_spec(
            "floatToBigInt",
            BUILTIN_FLOAT_TO_BIG_INT,
            1,
            unary(
                float.clone(),
                Ty::make_app(
                    Ty::make_con_id(prelude_maybe_tycon_id(names)),
                    big_int.clone(),
                ),
            ),
        ),
    ];

    for (name, builtin) in [
        ("bigIntAdd", BUILTIN_BIG_INT_ADD),
        ("bigIntSub", BUILTIN_BIG_INT_SUB),
        ("bigIntMul", BUILTIN_BIG_INT_MUL),
        ("bigIntDiv", BUILTIN_BIG_INT_DIV),
        ("bigIntMod", BUILTIN_BIG_INT_MOD),
        ("bigIntQuot", BUILTIN_BIG_INT_QUOT),
        ("bigIntRem", BUILTIN_BIG_INT_REM),
    ] {
        specs.push(prim_spec(
            name,
            builtin,
            2,
            binary(big_int.clone(), big_int.clone()),
        ));
    }
    for (name, builtin) in [
        ("bigIntNeg", BUILTIN_BIG_INT_NEG),
        ("bigIntAbs", BUILTIN_BIG_INT_ABS),
        ("bigIntSignum", BUILTIN_BIG_INT_SIGNUM),
    ] {
        specs.push(prim_spec(
            name,
            builtin,
            1,
            unary(big_int.clone(), big_int.clone()),
        ));
    }
    specs.extend([
        prim_spec(
            "bigIntPow",
            BUILTIN_BIG_INT_POW,
            2,
            arr(&[big_int.clone(), int.clone()], big_int.clone()),
        ),
        prim_spec(
            "bigIntEq",
            BUILTIN_BIG_INT_EQ,
            2,
            binary(big_int.clone(), bool_ty.clone()),
        ),
        prim_spec(
            "bigIntCompare",
            BUILTIN_BIG_INT_COMPARE,
            2,
            binary(big_int, ordering.clone()),
        ),
    ]);

    for (name, builtin) in [
        ("floatAbs", BUILTIN_FLOAT_ABS),
        ("floatSignum", BUILTIN_FLOAT_SIGNUM),
        ("floatSqrt", BUILTIN_FLOAT_SQRT),
        ("floatCbrt", BUILTIN_FLOAT_CBRT),
        ("floatExp", BUILTIN_FLOAT_EXP),
        ("floatExp2", BUILTIN_FLOAT_EXP2),
        ("floatLog", BUILTIN_FLOAT_LOG),
        ("floatLog2", BUILTIN_FLOAT_LOG2),
        ("floatLog10", BUILTIN_FLOAT_LOG10),
        ("floatSin", BUILTIN_FLOAT_SIN),
        ("floatCos", BUILTIN_FLOAT_COS),
        ("floatTan", BUILTIN_FLOAT_TAN),
        ("floatAsin", BUILTIN_FLOAT_ASIN),
        ("floatAcos", BUILTIN_FLOAT_ACOS),
        ("floatAtan", BUILTIN_FLOAT_ATAN),
        ("floatSinh", BUILTIN_FLOAT_SINH),
        ("floatCosh", BUILTIN_FLOAT_COSH),
        ("floatTanh", BUILTIN_FLOAT_TANH),
        ("floatFloor", BUILTIN_FLOAT_FLOOR),
        ("floatCeil", BUILTIN_FLOAT_CEIL),
        ("floatTruncate", BUILTIN_FLOAT_TRUNCATE),
        ("floatRound", BUILTIN_FLOAT_ROUND),
    ] {
        specs.push(prim_spec(
            name,
            builtin,
            1,
            unary(float.clone(), float.clone()),
        ));
    }
    for (name, builtin) in [
        ("floatPow", BUILTIN_FLOAT_POW),
        ("floatHypot", BUILTIN_FLOAT_HYPOT),
        ("floatAtan2", BUILTIN_FLOAT_ATAN2),
        ("floatCopySign", BUILTIN_FLOAT_COPY_SIGN),
    ] {
        specs.push(prim_spec(
            name,
            builtin,
            2,
            binary(float.clone(), float.clone()),
        ));
    }
    for (name, builtin) in [
        ("floatIsNaN", BUILTIN_FLOAT_IS_NAN),
        ("floatIsInfinite", BUILTIN_FLOAT_IS_INFINITE),
        ("floatIsFinite", BUILTIN_FLOAT_IS_FINITE),
    ] {
        specs.push(prim_spec(
            name,
            builtin,
            1,
            unary(float.clone(), bool_ty.clone()),
        ));
    }

    specs
}
