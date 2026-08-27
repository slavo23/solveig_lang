//! Type schemes for runtime environment and filesystem primitives.

use super::*;

pub(super) fn os_builtin_specs(
    names: &mut Names,
    int: &Ty,
    string: &Ty,
    bytes: &Ty,
    bool_ty: &Ty,
    host_error: &Ty,
) -> Vec<PrimitiveBuiltinSpec> {
    let task = |value| io_of(host_error.clone(), value);
    let maybe_string = Ty::make_app(
        Ty::make_con_id(prelude_maybe_tycon_id(names)),
        string.clone(),
    );
    let string_pair = Ty::make_app(
        Ty::make_app(
            Ty::make_con_id(prelude_tuple_tycon_id(names, 2)),
            string.clone(),
        ),
        string.clone(),
    );
    let string_list = Ty::make_app(
        Ty::make_con_id(prelude_list_tycon_id(names)),
        string.clone(),
    );
    let environment = Ty::make_app(Ty::make_con_id(prelude_list_tycon_id(names)), string_pair);
    let maybe_int = Ty::make_app(Ty::make_con_id(prelude_maybe_tycon_id(names)), int.clone());
    let process_result = Ty::make_con_id(std_process_result_tycon_id(names));
    let process_config = tuple(
        names,
        &[
            string.clone(),
            bool_ty.clone(),
            environment.clone(),
            string_list.clone(),
            bytes.clone(),
            bool_ty.clone(),
            bool_ty.clone(),
            maybe_int,
        ],
    );
    let process_request = tuple(
        names,
        &[string.clone(), string_list.clone(), process_config],
    );
    let a_id = names.fresh_tyvar("a");
    let a = Ty::TyVar(a_id);

    vec![
        prim_spec(
            "getEnv",
            BUILTIN_GET_ENV,
            1,
            unary(string.clone(), task(maybe_string.clone())),
        ),
        prim_spec(
            "setEnv",
            BUILTIN_SET_ENV,
            2,
            arr(&[string.clone(), string.clone()], task(Ty::TyUnit)),
        ),
        prim_spec(
            "getEnvironment",
            BUILTIN_GET_ENVIRONMENT,
            0,
            Scheme::mono(task(environment)),
        ),
        prim_spec(
            "getProgName",
            BUILTIN_GET_PROG_NAME,
            0,
            Scheme::mono(task(string.clone())),
        ),
        prim_spec(
            "getCurrentDirectory",
            BUILTIN_GET_CURRENT_DIRECTORY,
            0,
            Scheme::mono(task(string.clone())),
        ),
        prim_spec(
            "setCurrentDirectory",
            BUILTIN_SET_CURRENT_DIRECTORY,
            1,
            unary(string.clone(), task(Ty::TyUnit)),
        ),
        prim_spec(
            "doesFileExist",
            BUILTIN_DOES_FILE_EXIST,
            1,
            unary(string.clone(), task(bool_ty.clone())),
        ),
        prim_spec(
            "doesDirectoryExist",
            BUILTIN_DOES_DIRECTORY_EXIST,
            1,
            unary(string.clone(), task(bool_ty.clone())),
        ),
        prim_spec(
            "listDirectory",
            BUILTIN_LIST_DIRECTORY,
            1,
            unary(string.clone(), task(string_list)),
        ),
        prim_spec(
            "createDirectory",
            BUILTIN_CREATE_DIRECTORY,
            1,
            unary(string.clone(), task(Ty::TyUnit)),
        ),
        prim_spec(
            "removeFile",
            BUILTIN_REMOVE_FILE,
            1,
            unary(string.clone(), task(Ty::TyUnit)),
        ),
        prim_spec(
            "renameFile",
            BUILTIN_RENAME_FILE,
            2,
            arr(&[string.clone(), string.clone()], task(Ty::TyUnit)),
        ),
        prim_spec(
            "getFileSize",
            BUILTIN_GET_FILE_SIZE,
            1,
            unary(string.clone(), task(int.clone())),
        ),
        prim_spec(
            "getModificationTime",
            BUILTIN_GET_MODIFICATION_TIME,
            1,
            unary(string.clone(), task(int.clone())),
        ),
        prim_spec(
            "pathJoin",
            BUILTIN_PATH_JOIN,
            2,
            arr(&[string.clone(), string.clone()], string.clone()),
        ),
        prim_spec(
            "pathParent",
            BUILTIN_PATH_PARENT,
            1,
            unary(string.clone(), maybe_string.clone()),
        ),
        prim_spec(
            "pathFileName",
            BUILTIN_PATH_FILE_NAME,
            1,
            unary(string.clone(), maybe_string.clone()),
        ),
        prim_spec(
            "pathExtension",
            BUILTIN_PATH_EXTENSION,
            1,
            unary(string.clone(), maybe_string),
        ),
        prim_spec(
            "pathNormalise",
            BUILTIN_PATH_NORMALISE,
            1,
            unary(string.clone(), string.clone()),
        ),
        prim_spec(
            "pathIsAbsolute",
            BUILTIN_PATH_IS_ABSOLUTE,
            1,
            unary(string.clone(), bool_ty.clone()),
        ),
        prim_spec(
            "runProcessConfigured",
            BUILTIN_RUN_PROCESS_CONFIGURED,
            1,
            unary(process_request, task(process_result)),
        ),
        prim_spec(
            "exitWith",
            BUILTIN_EXIT_WITH,
            1,
            poly_arr(&[a_id], std::slice::from_ref(int), task(a)),
        ),
    ]
}
