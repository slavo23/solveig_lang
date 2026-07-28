//! Compiler-known primitive builtin API construction.
//! These specs seed the virtual `prelude.prim` module before resolution and checking.

use super::*;

pub(super) fn driver_builtin_api(
    session: &Session,
    names: &mut Names,
    modules: &ModuleTable,
) -> BuiltinApi {
    let mut api = compiler_builtin_api(session, modules);
    seed_primitive_builtin_terms(session, names, modules, &mut api);
    api
}

fn seed_primitive_builtin_terms(
    session: &Session,
    names: &mut Names,
    modules: &ModuleTable,
    api: &mut BuiltinApi,
) {
    let prim_module = compiler_prelude_prim_module(session);
    let Some(module_id) = modules.id(&prim_module) else {
        return;
    };

    for spec in primitive_builtin_specs(names) {
        let symbol = session.intern(spec.short_name);
        let id = DefId {
            module: module_id,
            name: symbol,
        };
        let full_name = format!("{COMPILER_PRELUDE_PRIM_MODULE}.{}", spec.short_name);

        names.register_symbol(symbol, spec.short_name);
        names.register_def(id, &full_name);

        api.builtin_defs.push(BuiltinDefinition {
            id,
            name: full_name,
            short_name: spec.short_name.to_string(),
            builtin: spec.builtin,
            arity: spec.arity,
            scheme: spec.scheme,
        });
    }
}

struct PrimitiveBuiltinSpec {
    short_name: &'static str,
    builtin: BuiltinId,
    arity: usize,
    scheme: Scheme,
}

fn primitive_builtin_specs(names: &mut Names) -> Vec<PrimitiveBuiltinSpec> {
    let int = Ty::make_con_id(PRIM_TYS.int);
    let float = Ty::make_con_id(PRIM_TYS.float);
    let char_ty = Ty::make_con_id(PRIM_TYS.char);
    let string = Ty::make_con_id(PRIM_TYS.string);
    let bool_ty = Ty::make_con_id(PRIM_TYS.bool);
    let ordering = Ty::make_con_id(prelude_ordering_tycon_id(names));
    let list = Ty::make_con_id(prelude_list_tycon_id(names));
    let maybe_char = Ty::make_app(
        Ty::make_con_id(prelude_maybe_tycon_id(names)),
        char_ty.clone(),
    );
    let char_list = Ty::make_app(list.clone(), char_ty.clone());
    let string_list = Ty::make_app(list, string.clone());
    let a_id = names.fresh_tyvar("a");
    let b_id = names.fresh_tyvar("b");
    let a = Ty::TyVar(a_id);
    let b = Ty::TyVar(b_id);

    vec![
        prim_spec(
            "intAdd",
            BUILTIN_INT_ADD,
            2,
            binary(int.clone(), int.clone()),
        ),
        prim_spec(
            "intSub",
            BUILTIN_INT_SUB,
            2,
            binary(int.clone(), int.clone()),
        ),
        prim_spec(
            "intMul",
            BUILTIN_INT_MUL,
            2,
            binary(int.clone(), int.clone()),
        ),
        prim_spec(
            "intDiv",
            BUILTIN_INT_DIV,
            2,
            binary(int.clone(), int.clone()),
        ),
        prim_spec(
            "intMod",
            BUILTIN_INT_MOD,
            2,
            binary(int.clone(), int.clone()),
        ),
        prim_spec(
            "intNeg",
            BUILTIN_INT_NEG,
            1,
            unary(int.clone(), int.clone()),
        ),
        prim_spec(
            "intEq",
            BUILTIN_INT_EQ,
            2,
            binary(int.clone(), bool_ty.clone()),
        ),
        prim_spec(
            "intLt",
            BUILTIN_INT_LT,
            2,
            binary(int.clone(), bool_ty.clone()),
        ),
        prim_spec(
            "intLe",
            BUILTIN_INT_LE,
            2,
            binary(int.clone(), bool_ty.clone()),
        ),
        prim_spec(
            "intGt",
            BUILTIN_INT_GT,
            2,
            binary(int.clone(), bool_ty.clone()),
        ),
        prim_spec(
            "intGe",
            BUILTIN_INT_GE,
            2,
            binary(int.clone(), bool_ty.clone()),
        ),
        prim_spec(
            "intCompare",
            BUILTIN_INT_COMPARE,
            2,
            binary(int.clone(), ordering.clone()),
        ),
        prim_spec(
            "floatAdd",
            BUILTIN_FLOAT_ADD,
            2,
            binary(float.clone(), float.clone()),
        ),
        prim_spec(
            "floatSub",
            BUILTIN_FLOAT_SUB,
            2,
            binary(float.clone(), float.clone()),
        ),
        prim_spec(
            "floatMul",
            BUILTIN_FLOAT_MUL,
            2,
            binary(float.clone(), float.clone()),
        ),
        prim_spec(
            "floatDiv",
            BUILTIN_FLOAT_DIV,
            2,
            binary(float.clone(), float.clone()),
        ),
        prim_spec(
            "floatNeg",
            BUILTIN_FLOAT_NEG,
            1,
            unary(float.clone(), float.clone()),
        ),
        prim_spec(
            "floatEq",
            BUILTIN_FLOAT_EQ,
            2,
            binary(float.clone(), bool_ty.clone()),
        ),
        prim_spec(
            "floatLt",
            BUILTIN_FLOAT_LT,
            2,
            binary(float.clone(), bool_ty.clone()),
        ),
        prim_spec(
            "floatLe",
            BUILTIN_FLOAT_LE,
            2,
            binary(float.clone(), bool_ty.clone()),
        ),
        prim_spec(
            "floatGt",
            BUILTIN_FLOAT_GT,
            2,
            binary(float.clone(), bool_ty.clone()),
        ),
        prim_spec(
            "floatGe",
            BUILTIN_FLOAT_GE,
            2,
            binary(float.clone(), bool_ty.clone()),
        ),
        prim_spec(
            "intToFloat",
            BUILTIN_INT_TO_FLOAT,
            1,
            unary(int.clone(), float.clone()),
        ),
        prim_spec(
            "floatToInt",
            BUILTIN_FLOAT_TO_INT,
            1,
            unary(float.clone(), int.clone()),
        ),
        prim_spec(
            "intToString",
            BUILTIN_INT_TO_STRING,
            1,
            unary(int.clone(), string.clone()),
        ),
        prim_spec(
            "floatToString",
            BUILTIN_FLOAT_TO_STRING,
            1,
            unary(float, string.clone()),
        ),
        prim_spec(
            "charToString",
            BUILTIN_CHAR_TO_STRING,
            1,
            unary(char_ty.clone(), string.clone()),
        ),
        prim_spec(
            "stringLength",
            BUILTIN_STRING_LENGTH,
            1,
            unary(string.clone(), int.clone()),
        ),
        prim_spec(
            "stringAppend",
            BUILTIN_STRING_APPEND,
            2,
            binary(string.clone(), string.clone()),
        ),
        prim_spec(
            "stringSlice",
            BUILTIN_STRING_SLICE,
            3,
            ternary(string.clone(), int.clone(), int.clone(), string.clone()),
        ),
        prim_spec(
            "stringCharAt",
            BUILTIN_STRING_CHAR_AT,
            2,
            arr(&[string.clone(), int], maybe_char),
        ),
        prim_spec(
            "stringEq",
            BUILTIN_STRING_EQ,
            2,
            binary(string.clone(), bool_ty.clone()),
        ),
        prim_spec(
            "stringCompare",
            BUILTIN_STRING_COMPARE,
            2,
            binary(string.clone(), ordering.clone()),
        ),
        prim_spec(
            "charEq",
            BUILTIN_CHAR_EQ,
            2,
            binary(char_ty.clone(), bool_ty),
        ),
        prim_spec(
            "charCompare",
            BUILTIN_CHAR_COMPARE,
            2,
            binary(char_ty, ordering),
        ),
        prim_spec(
            "stringToChars",
            BUILTIN_STRING_TO_CHARS,
            1,
            unary(string.clone(), char_list.clone()),
        ),
        prim_spec(
            "charsToString",
            BUILTIN_CHARS_TO_STRING,
            1,
            unary(char_list, string.clone()),
        ),
        prim_spec(
            "pureIO",
            BUILTIN_PURE_IO,
            1,
            poly_arr(&[a_id], std::slice::from_ref(&a), io_of(a.clone())),
        ),
        prim_spec(
            "bindIO",
            BUILTIN_BIND_IO,
            2,
            poly_arr(
                &[a_id, b_id],
                &[io_of(a.clone()), Ty::make_arr(a.clone(), io_of(b.clone()))],
                io_of(b),
            ),
        ),
        prim_spec(
            "printLine",
            BUILTIN_PRINT_LINE,
            1,
            unary(string.clone(), io_of(Ty::TyUnit)),
        ),
        prim_spec(
            "printStr",
            BUILTIN_PRINT_STR,
            1,
            unary(string.clone(), io_of(Ty::TyUnit)),
        ),
        prim_spec(
            "readLine",
            BUILTIN_READ_LINE,
            0,
            Scheme::mono(io_of(string.clone())),
        ),
        prim_spec(
            "getArgs",
            BUILTIN_GET_ARGS,
            0,
            Scheme::mono(io_of(string_list)),
        ),
        prim_spec(
            "readFile",
            BUILTIN_READ_FILE,
            1,
            unary(string.clone(), io_of(string.clone())),
        ),
        prim_spec(
            "writeFile",
            BUILTIN_WRITE_FILE,
            2,
            arr(&[string.clone(), string.clone()], io_of(Ty::TyUnit)),
        ),
        prim_spec(
            "appendFile",
            BUILTIN_APPEND_FILE,
            2,
            arr(&[string.clone(), string], io_of(Ty::TyUnit)),
        ),
    ]
}

fn prim_spec(
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

fn unary(arg: Ty, ret: Ty) -> Scheme {
    arr(&[arg], ret)
}

fn binary(arg: Ty, ret: Ty) -> Scheme {
    arr(&[arg.clone(), arg], ret)
}

fn ternary(a: Ty, b: Ty, c: Ty, ret: Ty) -> Scheme {
    arr(&[a, b, c], ret)
}

fn arr(args: &[Ty], ret: Ty) -> Scheme {
    Scheme::mono(arr_ty(args, ret))
}

fn arr_ty(args: &[Ty], ret: Ty) -> Ty {
    args.iter()
        .rev()
        .cloned()
        .fold(ret, |body, arg| Ty::make_arr(arg, body))
}

fn poly_arr(vars: &[TyVarId], args: &[Ty], ret: Ty) -> Scheme {
    poly(vars, arr_ty(args, ret))
}

fn poly(vars: &[TyVarId], ty: Ty) -> Scheme {
    Scheme {
        quantifiers: vars.iter().map(|id| (*id, Kind::Star)).collect(),
        predicates: Vec::new(),
        ty,
    }
}

fn io_of(value: Ty) -> Ty {
    Ty::make_app(Ty::make_con_id(PRIM_TYS.io), value)
}
