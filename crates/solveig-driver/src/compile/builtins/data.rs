//! Type schemes for packed text, character, and byte-string primitives.

use super::*;

pub(super) fn data_builtin_specs(names: &mut Names) -> Vec<PrimitiveBuiltinSpec> {
    let int = Ty::make_con_id(PRIM_TYS.int);
    let float = Ty::make_con_id(PRIM_TYS.float);
    let char_ty = Ty::make_con_id(PRIM_TYS.char);
    let string = Ty::make_con_id(PRIM_TYS.string);
    let bytes = Ty::make_con_id(PRIM_TYS.bytes);
    let bool_ty = Ty::make_con_id(PRIM_TYS.bool);
    let ordering = Ty::make_con_id(prelude_ordering_tycon_id(names));
    let list = Ty::make_con_id(prelude_list_tycon_id(names));
    let maybe = Ty::make_con_id(prelude_maybe_tycon_id(names));
    let maybe_char = Ty::make_app(maybe.clone(), char_ty.clone());
    let maybe_int = Ty::make_app(maybe.clone(), int.clone());
    let maybe_string = Ty::make_app(maybe, string.clone());
    let char_list = Ty::make_app(list.clone(), char_ty.clone());
    let string_list = Ty::make_app(list.clone(), string.clone());
    let bytes_list = Ty::make_app(list, bytes.clone());
    let utf8_error = Ty::make_con_id(prelude_utf8_error_tycon_id(names));
    let either_utf8_string = Ty::make_app(
        Ty::make_app(Ty::make_con_id(prelude_either_tycon_id(names)), utf8_error),
        string.clone(),
    );

    vec![
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
            "stringByteLength",
            BUILTIN_STRING_BYTE_LENGTH,
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
            "stringCharAtByte",
            BUILTIN_STRING_CHAR_AT_BYTE,
            2,
            arr(
                &[string.clone(), int.clone()],
                Ty::make_app(
                    Ty::make_con_id(prelude_maybe_tycon_id(names)),
                    char_ty.clone(),
                ),
            ),
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
            "stringStartsWith",
            BUILTIN_STRING_STARTS_WITH,
            2,
            binary(string.clone(), bool_ty.clone()),
        ),
        prim_spec(
            "stringFindCharFrom",
            BUILTIN_STRING_FIND_CHAR_FROM,
            3,
            arr(&[string.clone(), int.clone(), char_ty.clone()], int.clone()),
        ),
        prim_spec(
            "stringByteSlice",
            BUILTIN_STRING_BYTE_SLICE,
            3,
            arr(
                &[string.clone(), int.clone(), int.clone()],
                maybe_string.clone(),
            ),
        ),
        prim_spec(
            "stringTrimAscii",
            BUILTIN_STRING_TRIM_ASCII,
            1,
            unary(string.clone(), string.clone()),
        ),
        prim_spec(
            "stringsConcat",
            BUILTIN_STRINGS_CONCAT,
            1,
            unary(string_list, string.clone()),
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
            binary(char_ty.clone(), ordering),
        ),
        prim_spec(
            "charToCodePoint",
            BUILTIN_CHAR_TO_CODE_POINT,
            1,
            unary(char_ty, int.clone()),
        ),
        prim_spec(
            "codePointToString",
            BUILTIN_CODE_POINT_TO_STRING,
            1,
            unary(int.clone(), maybe_string),
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
            unary(bytes.clone(), either_utf8_string.clone()),
        ),
        prim_spec(
            "bytesToStringReplacing",
            BUILTIN_BYTES_TO_STRING_REPLACING,
            1,
            unary(bytes.clone(), string),
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
            binary(bytes.clone(), bytes.clone()),
        ),
        prim_spec(
            "bytesConcat",
            BUILTIN_BYTES_CONCAT,
            1,
            unary(bytes_list, bytes.clone()),
        ),
        prim_spec(
            "bytesSpanAny",
            BUILTIN_BYTES_SPAN_ANY,
            3,
            arr(&[bytes.clone(), int.clone(), bytes.clone()], int.clone()),
        ),
        prim_spec(
            "bytesFindAny",
            BUILTIN_BYTES_FIND_ANY,
            3,
            arr(&[bytes.clone(), int.clone(), bytes.clone()], int.clone()),
        ),
        prim_spec(
            "bytesFindBelow",
            BUILTIN_BYTES_FIND_BELOW,
            3,
            arr(&[bytes.clone(), int.clone(), int.clone()], int.clone()),
        ),
        prim_spec(
            "bytesSpanRange",
            BUILTIN_BYTES_SPAN_RANGE,
            4,
            arr(
                &[bytes.clone(), int.clone(), int.clone(), int.clone()],
                int.clone(),
            ),
        ),
        prim_spec(
            "bytesSliceToString",
            BUILTIN_BYTES_SLICE_TO_STRING,
            3,
            arr(&[bytes, int.clone(), int], either_utf8_string),
        ),
    ]
}
