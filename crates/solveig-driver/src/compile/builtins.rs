//! Compiler-known primitive builtin API construction.
//! These specs seed the virtual `prelude.prim` module before resolution and checking.

use super::*;

mod handles;
mod os;
mod schemes;

use schemes::*;

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

pub(super) struct PrimitiveBuiltinSpec {
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
    let bytes = Ty::make_con_id(PRIM_TYS.bytes);
    let bool_ty = Ty::make_con_id(PRIM_TYS.bool);
    let thread_id = Ty::make_con_id(PRIM_TYS.thread_id);
    let socket = Ty::make_con_id(PRIM_TYS.socket);
    let defect = Ty::make_con_id(prelude_defect_tycon_id(names));
    let io_error = Ty::make_con_id(prelude_io_error_tycon_id(names));
    let ordering = Ty::make_con_id(prelude_ordering_tycon_id(names));
    let list = Ty::make_con_id(prelude_list_tycon_id(names));
    let maybe_char = Ty::make_app(
        Ty::make_con_id(prelude_maybe_tycon_id(names)),
        char_ty.clone(),
    );
    let char_list = Ty::make_app(list.clone(), char_ty.clone());
    let string_list = Ty::make_app(list, string.clone());
    let e_id = names.fresh_tyvar("e");
    let f_id = names.fresh_tyvar("f");
    let a_id = names.fresh_tyvar("a");
    let b_id = names.fresh_tyvar("b");
    let e = Ty::TyVar(e_id);
    let f = Ty::TyVar(f_id);
    let a = Ty::TyVar(a_id);
    let b = Ty::TyVar(b_id);
    let chan_a = Ty::make_app(Ty::make_con_id(PRIM_TYS.chan), a.clone());
    let maybe_a = Ty::make_app(Ty::make_con_id(prelude_maybe_tycon_id(names)), a.clone());
    let maybe_int = Ty::make_app(Ty::make_con_id(prelude_maybe_tycon_id(names)), int.clone());
    let result_io_string = Ty::make_app(
        Ty::make_app(
            Ty::make_con_id(prelude_result_tycon_id(names)),
            io_error.clone(),
        ),
        string.clone(),
    );
    let resolve_host = unary(string.clone(), io_of(io_error.clone(), string.clone()));
    let listener_result = Ty::make_app(
        Ty::make_app(
            Ty::make_con_id(prelude_pair_tycon_id(names)),
            socket.clone(),
        ),
        int.clone(),
    );
    let handle_specs =
        handles::handle_builtin_specs(names, &int, &string, &bytes, &bool_ty, &io_error);
    let os_specs = os::os_builtin_specs(names, &int, &string, &bool_ty, &io_error);

    let mut specs = vec![
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
            arr(&[string.clone(), int.clone()], maybe_char),
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
            "stringToBytes",
            BUILTIN_STRING_TO_BYTES,
            1,
            unary(string.clone(), bytes.clone()),
        ),
        prim_spec(
            "bytesToString",
            BUILTIN_BYTES_TO_STRING,
            1,
            unary(bytes.clone(), result_io_string),
        ),
        prim_spec(
            "bytesLength",
            BUILTIN_BYTES_LENGTH,
            1,
            unary(bytes.clone(), int.clone()),
        ),
        prim_spec(
            "bytesIndex",
            BUILTIN_BYTES_INDEX,
            2,
            arr(&[bytes.clone(), int.clone()], maybe_int),
        ),
        prim_spec(
            "bytesSlice",
            BUILTIN_BYTES_SLICE,
            3,
            ternary(bytes.clone(), int.clone(), int.clone(), bytes.clone()),
        ),
        prim_spec(
            "bytesAppend",
            BUILTIN_BYTES_APPEND,
            2,
            binary(bytes, Ty::make_con_id(PRIM_TYS.bytes)),
        ),
        prim_spec(
            "connect",
            BUILTIN_CONNECT,
            2,
            arr(
                &[string.clone(), int.clone()],
                io_of(io_error.clone(), socket.clone()),
            ),
        ),
        prim_spec("resolveHost", BUILTIN_RESOLVE_HOST, 1, resolve_host),
        prim_spec(
            "listen",
            BUILTIN_LISTEN,
            1,
            unary(int.clone(), io_of(io_error.clone(), listener_result)),
        ),
        prim_spec(
            "accept",
            BUILTIN_ACCEPT,
            1,
            unary(socket.clone(), io_of(io_error.clone(), socket.clone())),
        ),
        prim_spec(
            "sendBytes",
            BUILTIN_SEND_BYTES,
            2,
            arr(
                &[socket.clone(), Ty::make_con_id(PRIM_TYS.bytes)],
                io_of(io_error.clone(), Ty::TyUnit),
            ),
        ),
        prim_spec(
            "recvBytes",
            BUILTIN_RECV_BYTES,
            2,
            arr(
                &[socket.clone(), int.clone()],
                io_of(io_error.clone(), Ty::make_con_id(PRIM_TYS.bytes)),
            ),
        ),
        prim_spec(
            "closeSocket",
            BUILTIN_CLOSE_SOCKET,
            1,
            unary(socket, io_of(io_error.clone(), Ty::TyUnit)),
        ),
        prim_spec(
            "pureIO",
            BUILTIN_PURE_IO,
            1,
            poly_arr(
                &[e_id, a_id],
                std::slice::from_ref(&a),
                io_of(e.clone(), a.clone()),
            ),
        ),
        prim_spec(
            "bindIO",
            BUILTIN_BIND_IO,
            2,
            poly_arr(
                &[e_id, a_id, b_id],
                &[
                    io_of(e.clone(), a.clone()),
                    Ty::make_arr(a.clone(), io_of(e.clone(), b.clone())),
                ],
                io_of(e.clone(), b.clone()),
            ),
        ),
        prim_spec(
            "failIO",
            BUILTIN_FAIL_IO,
            1,
            poly_arr(
                &[e_id, a_id],
                std::slice::from_ref(&e),
                io_of(e.clone(), a.clone()),
            ),
        ),
        prim_spec(
            "error",
            BUILTIN_ERROR,
            1,
            poly_arr(
                std::slice::from_ref(&a_id),
                std::slice::from_ref(&string),
                a.clone(),
            ),
        ),
        prim_spec(
            "catchIO",
            BUILTIN_CATCH_IO,
            2,
            poly_arr(
                &[e_id, f_id, a_id],
                &[
                    io_of(e.clone(), a.clone()),
                    Ty::make_arr(e.clone(), io_of(f.clone(), a.clone())),
                ],
                io_of(f.clone(), a.clone()),
            ),
        ),
        prim_spec(
            "finallyIO",
            BUILTIN_FINALLY_IO,
            2,
            poly_arr(
                &[e_id, a_id, b_id],
                &[io_of(e.clone(), a.clone()), io_of(e.clone(), b)],
                io_of(e.clone(), a.clone()),
            ),
        ),
        prim_spec(
            "catchDefectIO",
            BUILTIN_CATCH_DEFECT_IO,
            2,
            poly_arr(
                &[e_id, a_id],
                &[
                    io_of(e.clone(), a.clone()),
                    Ty::make_arr(defect.clone(), io_of(e.clone(), a.clone())),
                ],
                io_of(e.clone(), a.clone()),
            ),
        ),
        prim_spec(
            "raiseDefectIO",
            BUILTIN_RAISE_DEFECT_IO,
            1,
            poly_arr(
                &[e_id, a_id],
                std::slice::from_ref(&defect),
                io_of(e.clone(), a.clone()),
            ),
        ),
        prim_spec(
            "printLine",
            BUILTIN_PRINT_LINE,
            1,
            arr(
                std::slice::from_ref(&string),
                io_of(io_error.clone(), Ty::TyUnit),
            ),
        ),
        prim_spec(
            "printStr",
            BUILTIN_PRINT_STR,
            1,
            arr(
                std::slice::from_ref(&string),
                io_of(io_error.clone(), Ty::TyUnit),
            ),
        ),
        prim_spec(
            "readLine",
            BUILTIN_READ_LINE,
            0,
            Scheme::mono(io_of(io_error.clone(), string.clone())),
        ),
        prim_spec(
            "getArgs",
            BUILTIN_GET_ARGS,
            0,
            poly(&[e_id], io_of(e.clone(), string_list)),
        ),
        prim_spec(
            "readFile",
            BUILTIN_READ_FILE,
            1,
            unary(string.clone(), io_of(io_error.clone(), string.clone())),
        ),
        prim_spec(
            "writeFile",
            BUILTIN_WRITE_FILE,
            2,
            arr(
                &[string.clone(), string.clone()],
                io_of(io_error.clone(), Ty::TyUnit),
            ),
        ),
        prim_spec(
            "appendFile",
            BUILTIN_APPEND_FILE,
            2,
            arr(
                &[string.clone(), string],
                io_of(io_error.clone(), Ty::TyUnit),
            ),
        ),
        prim_spec(
            "spawn",
            BUILTIN_SPAWN,
            1,
            poly_arr(
                &[e_id, f_id],
                &[io_of(e.clone(), Ty::TyUnit)],
                io_of(f, thread_id.clone()),
            ),
        ),
        prim_spec(
            "newChan",
            BUILTIN_NEW_CHAN,
            0,
            poly(&[e_id, a_id], io_of(e.clone(), chan_a.clone())),
        ),
        prim_spec(
            "newBoundedChan",
            BUILTIN_NEW_BOUNDED_CHAN,
            1,
            poly_arr(
                &[e_id, a_id],
                std::slice::from_ref(&int),
                io_of(e.clone(), chan_a.clone()),
            ),
        ),
        prim_spec(
            "send",
            BUILTIN_SEND_CHAN,
            2,
            poly_arr(
                std::slice::from_ref(&a_id),
                &[chan_a.clone(), a.clone()],
                io_of(io_error.clone(), Ty::TyUnit),
            ),
        ),
        prim_spec(
            "recv",
            BUILTIN_RECV_CHAN,
            1,
            poly_arr(
                std::slice::from_ref(&a_id),
                std::slice::from_ref(&chan_a),
                io_of(io_error.clone(), a.clone()),
            ),
        ),
        prim_spec(
            "tryRecv",
            BUILTIN_TRY_RECV_CHAN,
            1,
            poly_arr(
                std::slice::from_ref(&a_id),
                std::slice::from_ref(&chan_a),
                io_of(io_error.clone(), maybe_a),
            ),
        ),
        prim_spec(
            "closeChan",
            BUILTIN_CLOSE_CHAN,
            1,
            poly_arr(
                std::slice::from_ref(&a_id),
                std::slice::from_ref(&chan_a),
                io_of(io_error.clone(), Ty::TyUnit),
            ),
        ),
        prim_spec(
            "killThread",
            BUILTIN_KILL_THREAD,
            1,
            unary(thread_id, io_of(io_error.clone(), Ty::TyUnit)),
        ),
        prim_spec(
            "getMonotonicTime",
            BUILTIN_GET_MONOTONIC_TIME,
            0,
            poly(&[e_id], io_of(e.clone(), int.clone())),
        ),
        prim_spec(
            "getSystemTime",
            BUILTIN_GET_SYSTEM_TIME,
            0,
            poly(&[e_id], io_of(e.clone(), int.clone())),
        ),
        prim_spec(
            "sleep",
            BUILTIN_SLEEP,
            1,
            unary(int.clone(), io_of(io_error, Ty::TyUnit)),
        ),
        prim_spec(
            "maskIO",
            BUILTIN_MASK_IO,
            1,
            poly_arr(
                &[e_id, a_id],
                &[io_of(e.clone(), a.clone())],
                io_of(e.clone(), a.clone()),
            ),
        ),
        prim_spec(
            "restoreMaskIO",
            BUILTIN_RESTORE_MASK_IO,
            1,
            poly_arr(&[e_id, a_id], &[io_of(e.clone(), a.clone())], io_of(e, a)),
        ),
    ];
    specs.extend(handle_specs);
    specs.extend(os_specs);
    specs
}
