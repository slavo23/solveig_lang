//! Type schemes for compiler-known file-handle primitives.

use super::*;

pub(super) fn handle_builtin_specs(
    names: &mut Names,
    int: &Ty,
    string: &Ty,
    bytes: &Ty,
    bool_ty: &Ty,
    io_error: &Ty,
) -> Vec<PrimitiveBuiltinSpec> {
    let handle = Ty::make_con_id(PRIM_TYS.handle);
    let io_mode = Ty::make_con_id(prelude_io_mode_tycon_id(names));
    let buffer_mode = Ty::make_con_id(prelude_buffer_mode_tycon_id(names));
    let task = |value| io_of(io_error.clone(), value);

    vec![
        prim_spec(
            "openFile",
            BUILTIN_OPEN_FILE,
            2,
            arr(&[string.clone(), io_mode], task(handle.clone())),
        ),
        prim_spec(
            "hClose",
            BUILTIN_H_CLOSE,
            1,
            unary(handle.clone(), task(Ty::TyUnit)),
        ),
        prim_spec(
            "hFlush",
            BUILTIN_H_FLUSH,
            1,
            unary(handle.clone(), task(Ty::TyUnit)),
        ),
        prim_spec(
            "hGetLine",
            BUILTIN_H_GET_LINE,
            1,
            unary(handle.clone(), task(string.clone())),
        ),
        prim_spec(
            "hGetContents",
            BUILTIN_H_GET_CONTENTS,
            1,
            unary(handle.clone(), task(string.clone())),
        ),
        prim_spec(
            "hGetBytes",
            BUILTIN_H_GET_BYTES,
            2,
            arr(&[handle.clone(), int.clone()], task(bytes.clone())),
        ),
        prim_spec(
            "hPutStr",
            BUILTIN_H_PUT_STR,
            2,
            arr(&[handle.clone(), string.clone()], task(Ty::TyUnit)),
        ),
        prim_spec(
            "hPutStrLn",
            BUILTIN_H_PUT_STR_LN,
            2,
            arr(&[handle.clone(), string.clone()], task(Ty::TyUnit)),
        ),
        prim_spec(
            "hPutBytes",
            BUILTIN_H_PUT_BYTES,
            2,
            arr(&[handle.clone(), bytes.clone()], task(Ty::TyUnit)),
        ),
        prim_spec(
            "hIsEOF",
            BUILTIN_H_IS_EOF,
            1,
            unary(handle.clone(), task(bool_ty.clone())),
        ),
        prim_spec(
            "hSetBuffering",
            BUILTIN_H_SET_BUFFERING,
            2,
            arr(&[handle.clone(), buffer_mode], task(Ty::TyUnit)),
        ),
        prim_spec("stdin", BUILTIN_STDIN, 0, Scheme::mono(handle.clone())),
        prim_spec("stdout", BUILTIN_STDOUT, 0, Scheme::mono(handle.clone())),
        prim_spec("stderr", BUILTIN_STDERR, 0, Scheme::mono(handle)),
        prim_spec(
            "readFileBytes",
            BUILTIN_READ_FILE_BYTES,
            1,
            unary(string.clone(), task(bytes.clone())),
        ),
        prim_spec(
            "writeFileBytes",
            BUILTIN_WRITE_FILE_BYTES,
            2,
            arr(&[string.clone(), bytes.clone()], task(Ty::TyUnit)),
        ),
    ]
}
