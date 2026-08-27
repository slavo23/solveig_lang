//! Compiler-known primitive builtin API construction.
//! These specs seed the virtual `prelude.prim` module before resolution and checking.

use super::*;

mod data;
mod handles;
mod os;
mod random;
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
    let string = Ty::make_con_id(PRIM_TYS.string);
    let bytes = Ty::make_con_id(PRIM_TYS.bytes);
    let bool_ty = Ty::make_con_id(PRIM_TYS.bool);
    let thread_id = Ty::make_con_id(PRIM_TYS.thread_id);
    let socket = Ty::make_con_id(PRIM_TYS.socket);
    let defect = Ty::make_con_id(prelude_defect_tycon_id(names));
    let host_error = Ty::make_con_id(prelude_host_error_tycon_id(names));
    let ordering = Ty::make_con_id(prelude_ordering_tycon_id(names));
    let list = Ty::make_con_id(prelude_list_tycon_id(names));
    let string_list = Ty::make_app(list.clone(), string.clone());
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
    let resolve_host = unary(string.clone(), io_of(host_error.clone(), string.clone()));
    let listener_result = tuple(names, &[socket.clone(), int.clone()]);
    let header = tuple(names, &[string.clone(), string.clone()]);
    let headers = Ty::make_app(list, header);
    let maybe_bytes = Ty::make_app(
        Ty::make_con_id(prelude_maybe_tycon_id(names)),
        bytes.clone(),
    );
    let http_request = tuple(
        names,
        &[string.clone(), string.clone(), headers.clone(), maybe_bytes],
    );
    let http_limits = tuple(names, &[int.clone(), int.clone(), int.clone(), int.clone()]);
    let http_response = tuple(names, &[int.clone(), headers, bytes.clone()]);
    let handle_specs =
        handles::handle_builtin_specs(names, &int, &string, &bytes, &bool_ty, &host_error);
    let os_specs = os::os_builtin_specs(names, &int, &string, &bytes, &bool_ty, &host_error);
    let random_specs = random::random_builtin_specs(names, &int);
    let data_specs = data::data_builtin_specs(names);

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
            "floatTotalCompare",
            BUILTIN_FLOAT_TOTAL_COMPARE,
            2,
            binary(float.clone(), ordering),
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
            unary(float, int.clone()),
        ),
    ];
    specs.extend(data_specs);
    specs.extend([
        prim_spec(
            "connect",
            BUILTIN_CONNECT,
            2,
            arr(
                &[string.clone(), int.clone()],
                io_of(host_error.clone(), socket.clone()),
            ),
        ),
        prim_spec("resolveHost", BUILTIN_RESOLVE_HOST, 1, resolve_host),
        prim_spec(
            "httpRequestRaw",
            BUILTIN_HTTP_REQUEST_RAW,
            2,
            arr(
                &[http_request, http_limits],
                io_of(host_error.clone(), http_response),
            ),
        ),
        prim_spec(
            "listen",
            BUILTIN_LISTEN,
            1,
            unary(int.clone(), io_of(host_error.clone(), listener_result)),
        ),
        prim_spec(
            "accept",
            BUILTIN_ACCEPT,
            1,
            unary(socket.clone(), io_of(host_error.clone(), socket.clone())),
        ),
        prim_spec(
            "sendBytes",
            BUILTIN_SEND_BYTES,
            2,
            arr(
                &[socket.clone(), Ty::make_con_id(PRIM_TYS.bytes)],
                io_of(host_error.clone(), Ty::TyUnit),
            ),
        ),
        prim_spec(
            "recvBytes",
            BUILTIN_RECV_BYTES,
            2,
            arr(
                &[socket.clone(), int.clone()],
                io_of(host_error.clone(), Ty::make_con_id(PRIM_TYS.bytes)),
            ),
        ),
        prim_spec(
            "closeSocket",
            BUILTIN_CLOSE_SOCKET,
            1,
            unary(socket, io_of(host_error.clone(), Ty::TyUnit)),
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
                io_of(host_error.clone(), Ty::TyUnit),
            ),
        ),
        prim_spec(
            "printStr",
            BUILTIN_PRINT_STR,
            1,
            arr(
                std::slice::from_ref(&string),
                io_of(host_error.clone(), Ty::TyUnit),
            ),
        ),
        prim_spec(
            "readLine",
            BUILTIN_READ_LINE,
            0,
            Scheme::mono(io_of(host_error.clone(), string.clone())),
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
            unary(string.clone(), io_of(host_error.clone(), string.clone())),
        ),
        prim_spec(
            "readFileLimited",
            BUILTIN_READ_FILE_LIMITED,
            2,
            arr(
                &[string.clone(), int.clone()],
                io_of(host_error.clone(), string.clone()),
            ),
        ),
        prim_spec(
            "writeFile",
            BUILTIN_WRITE_FILE,
            2,
            arr(
                &[string.clone(), string.clone()],
                io_of(host_error.clone(), Ty::TyUnit),
            ),
        ),
        prim_spec(
            "appendFile",
            BUILTIN_APPEND_FILE,
            2,
            arr(
                &[string.clone(), string],
                io_of(host_error.clone(), Ty::TyUnit),
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
                io_of(host_error.clone(), Ty::TyUnit),
            ),
        ),
        prim_spec(
            "recv",
            BUILTIN_RECV_CHAN,
            1,
            poly_arr(
                std::slice::from_ref(&a_id),
                std::slice::from_ref(&chan_a),
                io_of(host_error.clone(), a.clone()),
            ),
        ),
        prim_spec(
            "tryRecv",
            BUILTIN_TRY_RECV_CHAN,
            1,
            poly_arr(
                std::slice::from_ref(&a_id),
                std::slice::from_ref(&chan_a),
                io_of(host_error.clone(), maybe_a),
            ),
        ),
        prim_spec(
            "closeChan",
            BUILTIN_CLOSE_CHAN,
            1,
            poly_arr(
                std::slice::from_ref(&a_id),
                std::slice::from_ref(&chan_a),
                io_of(host_error.clone(), Ty::TyUnit),
            ),
        ),
        prim_spec(
            "killThread",
            BUILTIN_KILL_THREAD,
            1,
            unary(thread_id, io_of(host_error.clone(), Ty::TyUnit)),
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
            unary(int.clone(), io_of(host_error, Ty::TyUnit)),
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
    ]);
    specs.extend(handle_specs);
    specs.extend(os_specs);
    specs.extend(random_specs);
    specs
}
