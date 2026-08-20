//! Runtime implementations of Solveig primitive operations.
//! The VM dispatches builtin ids here for arithmetic, strings, files, and IO.

use std::cmp::Ordering;

use solveig_ast::ast::{
    BUILTIN_ACCEPT, BUILTIN_APPEND_FILE, BUILTIN_BIND_IO, BUILTIN_BYTES_APPEND,
    BUILTIN_BYTES_INDEX, BUILTIN_BYTES_LENGTH, BUILTIN_BYTES_SLICE, BUILTIN_BYTES_TO_STRING,
    BUILTIN_CATCH_DEFECT_IO, BUILTIN_CATCH_IO, BUILTIN_CHAR_COMPARE, BUILTIN_CHAR_EQ,
    BUILTIN_CHAR_TO_STRING, BUILTIN_CHARS_TO_STRING, BUILTIN_CLOSE_CHAN, BUILTIN_CLOSE_SOCKET,
    BUILTIN_CONNECT, BUILTIN_CREATE_DIRECTORY, BUILTIN_DOES_DIRECTORY_EXIST,
    BUILTIN_DOES_FILE_EXIST, BUILTIN_ERROR, BUILTIN_EXIT_WITH, BUILTIN_FAIL_IO, BUILTIN_FINALLY_IO,
    BUILTIN_FLOAT_ADD, BUILTIN_FLOAT_DIV, BUILTIN_FLOAT_EQ, BUILTIN_FLOAT_GE, BUILTIN_FLOAT_GT,
    BUILTIN_FLOAT_LE, BUILTIN_FLOAT_LT, BUILTIN_FLOAT_MUL, BUILTIN_FLOAT_NEG, BUILTIN_FLOAT_SUB,
    BUILTIN_FLOAT_TO_INT, BUILTIN_FLOAT_TO_STRING, BUILTIN_FLOAT_TOTAL_COMPARE, BUILTIN_GET_ARGS,
    BUILTIN_GET_CURRENT_DIRECTORY, BUILTIN_GET_ENV, BUILTIN_GET_ENVIRONMENT, BUILTIN_GET_FILE_SIZE,
    BUILTIN_GET_MODIFICATION_TIME, BUILTIN_GET_MONOTONIC_TIME, BUILTIN_GET_PROG_NAME,
    BUILTIN_GET_SYSTEM_TIME, BUILTIN_H_CLOSE, BUILTIN_H_FLUSH, BUILTIN_H_GET_BYTES,
    BUILTIN_H_GET_CONTENTS, BUILTIN_H_GET_LINE, BUILTIN_H_IS_EOF, BUILTIN_H_PUT_BYTES,
    BUILTIN_H_PUT_STR, BUILTIN_H_PUT_STR_LN, BUILTIN_H_SET_BUFFERING, BUILTIN_INT_ADD,
    BUILTIN_INT_COMPARE, BUILTIN_INT_DIV, BUILTIN_INT_EQ, BUILTIN_INT_GE, BUILTIN_INT_GT,
    BUILTIN_INT_LE, BUILTIN_INT_LT, BUILTIN_INT_MOD, BUILTIN_INT_MUL, BUILTIN_INT_NEG,
    BUILTIN_INT_SUB, BUILTIN_INT_TO_FLOAT, BUILTIN_INT_TO_STRING, BUILTIN_KILL_THREAD,
    BUILTIN_LIST_DIRECTORY, BUILTIN_LISTEN, BUILTIN_MASK_IO, BUILTIN_MK_STD_GEN,
    BUILTIN_NEW_BOUNDED_CHAN, BUILTIN_NEW_CHAN, BUILTIN_NEW_STD_GEN, BUILTIN_OPEN_FILE,
    BUILTIN_PRINT_LINE, BUILTIN_PRINT_STR, BUILTIN_PURE_IO, BUILTIN_RAISE_DEFECT_IO,
    BUILTIN_RANDOM_INT, BUILTIN_RANDOM_RANGE, BUILTIN_READ_FILE, BUILTIN_READ_FILE_BYTES,
    BUILTIN_READ_FILE_BYTES_LIMITED, BUILTIN_READ_FILE_LIMITED, BUILTIN_READ_LINE,
    BUILTIN_RECV_BYTES, BUILTIN_RECV_CHAN, BUILTIN_REMOVE_FILE, BUILTIN_RENAME_FILE,
    BUILTIN_RESOLVE_HOST, BUILTIN_RESTORE_MASK_IO, BUILTIN_RUN_PROCESS,
    BUILTIN_RUN_PROCESS_LIMITED, BUILTIN_SEND_BYTES, BUILTIN_SEND_CHAN,
    BUILTIN_SET_CURRENT_DIRECTORY, BUILTIN_SET_ENV, BUILTIN_SLEEP, BUILTIN_SPAWN, BUILTIN_STDERR,
    BUILTIN_STDIN, BUILTIN_STDOUT, BUILTIN_STRING_APPEND, BUILTIN_STRING_CHAR_AT,
    BUILTIN_STRING_COMPARE, BUILTIN_STRING_EQ, BUILTIN_STRING_LENGTH, BUILTIN_STRING_SLICE,
    BUILTIN_STRING_TO_BYTES, BUILTIN_STRING_TO_CHARS, BUILTIN_TRY_RECV_CHAN, BUILTIN_WRITE_FILE,
    BUILTIN_WRITE_FILE_BYTES, BuiltinId, CtorRuntimeInfo, CtorRuntimeRole,
};

use crate::heap::{HeapObject, IoAction};
use crate::values::Value;
use crate::vm::{DefectKind, Interpreter, RuntimeError};

mod handles;
mod io_actions;
mod network;
mod os;
mod random;
mod text;

use handles::*;
use io_actions::*;
use network::*;
use os::*;
use random::*;
use text::*;

pub type BuiltinRun = for<'context, 'runtime> fn(
    &mut VmCtx<'context, 'runtime>,
    &[Value],
) -> Result<Value, RuntimeError>;

pub struct Builtin {
    pub id: BuiltinId,
    pub name: &'static str,
    pub arity: usize,
    pub run: BuiltinRun,
}

pub struct VmCtx<'context, 'runtime> {
    interpreter: &'context mut Interpreter<'runtime>,
}

impl<'context, 'runtime> VmCtx<'context, 'runtime> {
    pub(crate) fn new(interpreter: &'context mut Interpreter<'runtime>) -> Self {
        Self { interpreter }
    }

    fn bool_value(&self, value: bool) -> Value {
        if value {
            self.interpreter.bools.true_val
        } else {
            self.interpreter.bools.false_val
        }
    }

    fn alloc_str(&mut self, value: &str) -> Value {
        self.interpreter.heap.alloc_str(value)
    }

    fn alloc_bytes(&mut self, value: &[u8]) -> Value {
        self.interpreter.heap.alloc_bytes(value)
    }

    fn alloc_io_action(&mut self, action: IoAction) -> Value {
        self.interpreter.heap.alloc(HeapObject::IoAction(action))
    }

    fn ctor_info(
        &self,
        role: CtorRuntimeRole,
        builtin: &str,
    ) -> Result<&CtorRuntimeInfo, RuntimeError> {
        self.interpreter
            .program
            .constructor_arities
            .iter()
            .find(|info| info.role == role)
            .ok_or_else(|| {
                RuntimeError::builtin(format!(
                    "{builtin} runtime constructor metadata missing for role {role:?}"
                ))
            })
    }

    pub(crate) fn ctor_tag_with_arity(
        &self,
        role: CtorRuntimeRole,
        arity: u16,
        builtin: &str,
    ) -> Result<u16, RuntimeError> {
        let info = self.ctor_info(role, builtin)?;
        if info.arity != arity {
            return Err(RuntimeError::builtin(format!(
                "{builtin} runtime constructor `{}` expected arity {}, got {}",
                info.short_name, arity, info.arity
            )));
        }
        Ok(info.tag)
    }

    pub(crate) fn nullary_ctor(
        &self,
        role: CtorRuntimeRole,
        builtin: &str,
    ) -> Result<Value, RuntimeError> {
        let tag = self.ctor_tag_with_arity(role, 0, builtin)?;
        self.interpreter.nullary_ctors.try_get(tag).ok_or_else(|| {
            RuntimeError::builtin(format!(
                "{builtin} runtime nullary constructor cache missing tag {tag}"
            ))
        })
    }

    pub(crate) fn alloc_data1_ctor(
        &mut self,
        role: CtorRuntimeRole,
        field: Value,
        builtin: &str,
    ) -> Result<Value, RuntimeError> {
        let tag = self.ctor_tag_with_arity(role, 1, builtin)?;
        Ok(self
            .interpreter
            .heap
            .alloc(HeapObject::Data1 { tag, field }))
    }

    pub(crate) fn alloc_data2_ctor(
        &mut self,
        role: CtorRuntimeRole,
        field0: Value,
        field1: Value,
        builtin: &str,
    ) -> Result<Value, RuntimeError> {
        let tag = self.ctor_tag_with_arity(role, 2, builtin)?;
        Ok(self.interpreter.heap.alloc(HeapObject::Data2 {
            tag,
            field0,
            field1,
        }))
    }

    pub(crate) fn alloc_data3_ctor(
        &mut self,
        role: CtorRuntimeRole,
        field0: Value,
        field1: Value,
        field2: Value,
        builtin: &str,
    ) -> Result<Value, RuntimeError> {
        let tag = self.ctor_tag_with_arity(role, 3, builtin)?;
        let mut fields = vec![field0, field1, field2];
        let fields = self
            .interpreter
            .heap
            .alloc_payload_from_stack_tail(&mut fields, 3);
        Ok(self
            .interpreter
            .heap
            .alloc(HeapObject::DataN { tag, fields }))
    }

    pub(crate) fn alloc_list_from_values(
        &mut self,
        values: &[Value],
        builtin: &str,
    ) -> Result<Value, RuntimeError> {
        let checkpoint = self.interpreter.process.temporary_roots.len();
        self.interpreter
            .process
            .temporary_roots
            .extend_from_slice(values);
        let result = self.alloc_list_from_root_range(checkpoint, values.len(), builtin);
        self.interpreter
            .process
            .temporary_roots
            .truncate(checkpoint);
        result
    }

    pub(crate) fn alloc_list_from_root_range(
        &mut self,
        root_start: usize,
        value_count: usize,
        builtin: &str,
    ) -> Result<Value, RuntimeError> {
        let cons_tag = self.ctor_tag_with_arity(CtorRuntimeRole::PreludeListCons, 2, builtin)?;
        let nil = self.nullary_ctor(CtorRuntimeRole::PreludeListNil, builtin)?;
        let result_root = self.interpreter.process.temporary_roots.len();
        self.interpreter.process.temporary_roots.push(nil);

        for offset in (0..value_count).rev() {
            self.interpreter.safe_point();
            let value = *self
                .interpreter
                .process
                .temporary_roots
                .get(root_start + offset)
                .expect("ICE: rooted list element index out of bounds");
            let result = *self
                .interpreter
                .process
                .temporary_roots
                .get(result_root)
                .expect("ICE: rooted list result index out of bounds");
            let result = self.interpreter.heap.alloc(HeapObject::Data2 {
                tag: cons_tag,
                field0: value,
                field1: result,
            });
            *self
                .interpreter
                .process
                .temporary_roots
                .get_mut(result_root)
                .expect("ICE: rooted list result index out of bounds") = result;
        }

        let result = self
            .interpreter
            .process
            .temporary_roots
            .pop()
            .expect("ICE: rooted list result disappeared");
        Ok(result)
    }
}

pub fn lookup(id: BuiltinId) -> Option<&'static Builtin> {
    ALL_BUILTINS.iter().find(|builtin| builtin.id == id)
}

macro_rules! builtin {
    ($id:expr, $name:literal, $arity:expr, $run:ident) => {
        Builtin {
            id: $id,
            name: concat!("prelude.prim.", $name),
            arity: $arity,
            run: $run,
        }
    };
}

static ALL_BUILTINS: &[Builtin] = &[
    builtin!(BUILTIN_INT_ADD, "intAdd", 2, int_add),
    builtin!(BUILTIN_INT_SUB, "intSub", 2, int_sub),
    builtin!(BUILTIN_INT_MUL, "intMul", 2, int_mul),
    builtin!(BUILTIN_INT_DIV, "intDiv", 2, int_div),
    builtin!(BUILTIN_INT_MOD, "intMod", 2, int_mod),
    builtin!(BUILTIN_INT_NEG, "intNeg", 1, int_neg),
    builtin!(BUILTIN_INT_EQ, "intEq", 2, int_eq),
    builtin!(BUILTIN_INT_LT, "intLt", 2, int_lt),
    builtin!(BUILTIN_INT_LE, "intLe", 2, int_le),
    builtin!(BUILTIN_INT_GT, "intGt", 2, int_gt),
    builtin!(BUILTIN_INT_GE, "intGe", 2, int_ge),
    builtin!(BUILTIN_INT_COMPARE, "intCompare", 2, int_compare),
    builtin!(BUILTIN_FLOAT_ADD, "floatAdd", 2, float_add),
    builtin!(BUILTIN_FLOAT_SUB, "floatSub", 2, float_sub),
    builtin!(BUILTIN_FLOAT_MUL, "floatMul", 2, float_mul),
    builtin!(BUILTIN_FLOAT_DIV, "floatDiv", 2, float_div),
    builtin!(BUILTIN_FLOAT_NEG, "floatNeg", 1, float_neg),
    builtin!(BUILTIN_FLOAT_EQ, "floatEq", 2, float_eq),
    builtin!(BUILTIN_FLOAT_LT, "floatLt", 2, float_lt),
    builtin!(BUILTIN_FLOAT_LE, "floatLe", 2, float_le),
    builtin!(BUILTIN_FLOAT_GT, "floatGt", 2, float_gt),
    builtin!(BUILTIN_FLOAT_GE, "floatGe", 2, float_ge),
    builtin!(
        BUILTIN_FLOAT_TOTAL_COMPARE,
        "floatTotalCompare",
        2,
        float_total_compare
    ),
    builtin!(BUILTIN_INT_TO_FLOAT, "intToFloat", 1, int_to_float),
    builtin!(BUILTIN_FLOAT_TO_INT, "floatToInt", 1, float_to_int),
    builtin!(BUILTIN_INT_TO_STRING, "intToString", 1, int_to_string),
    builtin!(BUILTIN_FLOAT_TO_STRING, "floatToString", 1, float_to_string),
    builtin!(BUILTIN_CHAR_TO_STRING, "charToString", 1, char_to_string),
    builtin!(BUILTIN_STRING_LENGTH, "stringLength", 1, string_length),
    builtin!(BUILTIN_STRING_APPEND, "stringAppend", 2, string_append),
    builtin!(BUILTIN_STRING_SLICE, "stringSlice", 3, string_slice),
    builtin!(BUILTIN_STRING_CHAR_AT, "stringCharAt", 2, string_char_at),
    builtin!(BUILTIN_STRING_EQ, "stringEq", 2, string_eq),
    builtin!(BUILTIN_STRING_COMPARE, "stringCompare", 2, string_compare),
    builtin!(BUILTIN_CHAR_EQ, "charEq", 2, char_eq),
    builtin!(BUILTIN_CHAR_COMPARE, "charCompare", 2, char_compare),
    builtin!(BUILTIN_STRING_TO_CHARS, "stringToChars", 1, string_to_chars),
    builtin!(BUILTIN_CHARS_TO_STRING, "charsToString", 1, chars_to_string),
    builtin!(BUILTIN_STRING_TO_BYTES, "stringToBytes", 1, string_to_bytes),
    builtin!(BUILTIN_BYTES_TO_STRING, "bytesToString", 1, bytes_to_string),
    builtin!(BUILTIN_BYTES_LENGTH, "bytesLength", 1, bytes_length),
    builtin!(BUILTIN_BYTES_INDEX, "bytesIndex", 2, bytes_index),
    builtin!(BUILTIN_BYTES_SLICE, "bytesSlice", 3, bytes_slice),
    builtin!(BUILTIN_BYTES_APPEND, "bytesAppend", 2, bytes_append),
    builtin!(BUILTIN_CONNECT, "connect", 2, connect),
    builtin!(BUILTIN_RESOLVE_HOST, "resolveHost", 1, resolve_host),
    builtin!(BUILTIN_LISTEN, "listen", 1, listen),
    builtin!(BUILTIN_ACCEPT, "accept", 1, accept),
    builtin!(BUILTIN_SEND_BYTES, "sendBytes", 2, send_bytes),
    builtin!(BUILTIN_RECV_BYTES, "recvBytes", 2, recv_bytes),
    builtin!(BUILTIN_CLOSE_SOCKET, "closeSocket", 1, close_socket),
    builtin!(BUILTIN_OPEN_FILE, "openFile", 2, open_file),
    builtin!(BUILTIN_H_CLOSE, "hClose", 1, h_close),
    builtin!(BUILTIN_H_FLUSH, "hFlush", 1, h_flush),
    builtin!(BUILTIN_H_GET_LINE, "hGetLine", 1, h_get_line),
    builtin!(BUILTIN_H_GET_CONTENTS, "hGetContents", 1, h_get_contents),
    builtin!(BUILTIN_H_GET_BYTES, "hGetBytes", 2, h_get_bytes),
    builtin!(BUILTIN_H_PUT_STR, "hPutStr", 2, h_put_str),
    builtin!(BUILTIN_H_PUT_STR_LN, "hPutStrLn", 2, h_put_str_ln),
    builtin!(BUILTIN_H_PUT_BYTES, "hPutBytes", 2, h_put_bytes),
    builtin!(BUILTIN_H_IS_EOF, "hIsEOF", 1, h_is_eof),
    builtin!(BUILTIN_H_SET_BUFFERING, "hSetBuffering", 2, h_set_buffering),
    builtin!(BUILTIN_STDIN, "stdin", 0, stdin),
    builtin!(BUILTIN_STDOUT, "stdout", 0, stdout),
    builtin!(BUILTIN_STDERR, "stderr", 0, stderr),
    builtin!(BUILTIN_PURE_IO, "pureIO", 1, pure_io),
    builtin!(BUILTIN_BIND_IO, "bindIO", 2, bind_io),
    builtin!(BUILTIN_FAIL_IO, "failIO", 1, fail_io),
    builtin!(BUILTIN_CATCH_IO, "catchIO", 2, catch_io),
    builtin!(BUILTIN_FINALLY_IO, "finallyIO", 2, finally_io),
    builtin!(BUILTIN_ERROR, "error", 1, raise_error),
    builtin!(BUILTIN_CATCH_DEFECT_IO, "catchDefectIO", 2, catch_defect_io),
    builtin!(BUILTIN_RAISE_DEFECT_IO, "raiseDefectIO", 1, raise_defect_io),
    builtin!(BUILTIN_PRINT_LINE, "printLine", 1, print_line),
    builtin!(BUILTIN_PRINT_STR, "printStr", 1, print_str),
    builtin!(BUILTIN_READ_LINE, "readLine", 0, read_line),
    builtin!(BUILTIN_GET_ARGS, "getArgs", 0, get_args),
    builtin!(BUILTIN_READ_FILE, "readFile", 1, read_file),
    builtin!(
        BUILTIN_READ_FILE_LIMITED,
        "readFileLimited",
        2,
        read_file_limited
    ),
    builtin!(BUILTIN_READ_FILE_BYTES, "readFileBytes", 1, read_file_bytes),
    builtin!(
        BUILTIN_READ_FILE_BYTES_LIMITED,
        "readFileBytesLimited",
        2,
        read_file_bytes_limited
    ),
    builtin!(BUILTIN_WRITE_FILE, "writeFile", 2, write_file),
    builtin!(
        BUILTIN_WRITE_FILE_BYTES,
        "writeFileBytes",
        2,
        write_file_bytes
    ),
    builtin!(BUILTIN_APPEND_FILE, "appendFile", 2, append_file),
    builtin!(BUILTIN_GET_ENV, "getEnv", 1, get_env),
    builtin!(BUILTIN_SET_ENV, "setEnv", 2, set_env),
    builtin!(
        BUILTIN_GET_ENVIRONMENT,
        "getEnvironment",
        0,
        get_environment
    ),
    builtin!(BUILTIN_GET_PROG_NAME, "getProgName", 0, get_prog_name),
    builtin!(
        BUILTIN_GET_CURRENT_DIRECTORY,
        "getCurrentDirectory",
        0,
        get_current_directory
    ),
    builtin!(
        BUILTIN_SET_CURRENT_DIRECTORY,
        "setCurrentDirectory",
        1,
        set_current_directory
    ),
    builtin!(BUILTIN_DOES_FILE_EXIST, "doesFileExist", 1, does_file_exist),
    builtin!(
        BUILTIN_DOES_DIRECTORY_EXIST,
        "doesDirectoryExist",
        1,
        does_directory_exist
    ),
    builtin!(BUILTIN_LIST_DIRECTORY, "listDirectory", 1, list_directory),
    builtin!(
        BUILTIN_CREATE_DIRECTORY,
        "createDirectory",
        1,
        create_directory
    ),
    builtin!(BUILTIN_REMOVE_FILE, "removeFile", 1, remove_file),
    builtin!(BUILTIN_RENAME_FILE, "renameFile", 2, rename_file),
    builtin!(BUILTIN_GET_FILE_SIZE, "getFileSize", 1, get_file_size),
    builtin!(
        BUILTIN_GET_MODIFICATION_TIME,
        "getModificationTime",
        1,
        get_modification_time
    ),
    builtin!(BUILTIN_RUN_PROCESS, "runProcess", 2, run_process_action),
    builtin!(
        BUILTIN_RUN_PROCESS_LIMITED,
        "runProcessLimited",
        3,
        run_process_limited_action
    ),
    builtin!(BUILTIN_EXIT_WITH, "exitWith", 1, exit_with),
    builtin!(
        BUILTIN_GET_MONOTONIC_TIME,
        "getMonotonicTime",
        0,
        get_monotonic_time
    ),
    builtin!(BUILTIN_GET_SYSTEM_TIME, "getSystemTime", 0, get_system_time),
    builtin!(BUILTIN_MK_STD_GEN, "mkStdGen", 1, mk_std_gen),
    builtin!(BUILTIN_NEW_STD_GEN, "newStdGen", 0, new_std_gen),
    builtin!(BUILTIN_RANDOM_INT, "randomInt", 1, random_int),
    builtin!(BUILTIN_RANDOM_RANGE, "randomRange", 3, random_range),
    builtin!(BUILTIN_SPAWN, "spawn", 1, spawn),
    builtin!(BUILTIN_NEW_CHAN, "newChan", 0, new_chan),
    builtin!(
        BUILTIN_NEW_BOUNDED_CHAN,
        "newBoundedChan",
        1,
        new_bounded_chan
    ),
    builtin!(BUILTIN_SEND_CHAN, "send", 2, send_chan),
    builtin!(BUILTIN_RECV_CHAN, "recv", 1, recv_chan),
    builtin!(BUILTIN_TRY_RECV_CHAN, "tryRecv", 1, try_recv_chan),
    builtin!(BUILTIN_CLOSE_CHAN, "closeChan", 1, close_chan),
    builtin!(BUILTIN_KILL_THREAD, "killThread", 1, kill_thread),
    builtin!(BUILTIN_MASK_IO, "maskIO", 1, mask_io),
    builtin!(BUILTIN_RESTORE_MASK_IO, "restoreMaskIO", 1, restore_mask_io),
    builtin!(BUILTIN_SLEEP, "sleep", 1, sleep),
];

// D3 primitive semantics:
// - Int add/sub/mul/neg wrap on i64 overflow. Division and modulo defect on zero;
//   i64::MIN divided/modded by -1 follows wrapping i64 behavior.
// - Float arithmetic and comparisons delegate to f64, preserving IEEE-754 NaN
//   and infinity behavior for arithmetic and relational predicates.
// - String length, indexing, slicing, and char bridges use Unicode scalar values.
//   Slicing clamps negative/out-of-range bounds instead of trapping.

pub(crate) fn int_div_or_defect(lhs: i64, rhs: i64) -> Result<i64, RuntimeError> {
    if rhs == 0 {
        return Err(RuntimeError::defect(DefectKind::DivisionByZero));
    }
    if lhs == i64::MIN && rhs == -1 {
        return Ok(i64::MIN);
    }
    Ok(lhs / rhs)
}

pub(crate) fn int_mod_or_defect(lhs: i64, rhs: i64) -> Result<i64, RuntimeError> {
    if rhs == 0 {
        return Err(RuntimeError::defect(DefectKind::DivisionByZero));
    }
    if lhs == i64::MIN && rhs == -1 {
        return Ok(0);
    }
    Ok(lhs % rhs)
}

fn int_add(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intAdd")?;
    Ok(Value::Int(lhs.wrapping_add(rhs)))
}

fn int_sub(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intSub")?;
    Ok(Value::Int(lhs.wrapping_sub(rhs)))
}

fn int_mul(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intMul")?;
    Ok(Value::Int(lhs.wrapping_mul(rhs)))
}

fn int_div(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intDiv")?;
    Ok(Value::Int(int_div_or_defect(lhs, rhs)?))
}

fn int_mod(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intMod")?;
    Ok(Value::Int(int_mod_or_defect(lhs, rhs)?))
}

fn int_neg(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_int(args, 0, "intNeg")?;
    Ok(Value::Int(value.wrapping_neg()))
}

fn int_eq(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intEq")?;
    Ok(ctx.bool_value(lhs == rhs))
}

fn int_lt(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intLt")?;
    Ok(ctx.bool_value(lhs < rhs))
}

fn int_le(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intLe")?;
    Ok(ctx.bool_value(lhs <= rhs))
}

fn int_gt(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intGt")?;
    Ok(ctx.bool_value(lhs > rhs))
}

fn int_ge(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intGe")?;
    Ok(ctx.bool_value(lhs >= rhs))
}

fn int_compare(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_int2(args, "intCompare")?;
    ordering_value(ctx, lhs.cmp(&rhs), "intCompare")
}

fn float_add(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatAdd")?;
    Ok(Value::Float(lhs + rhs))
}

fn float_sub(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatSub")?;
    Ok(Value::Float(lhs - rhs))
}

fn float_mul(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatMul")?;
    Ok(Value::Float(lhs * rhs))
}

fn float_div(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatDiv")?;
    Ok(Value::Float(lhs / rhs))
}

fn float_neg(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_float(args, 0, "floatNeg")?;
    Ok(Value::Float(-value))
}

fn float_eq(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatEq")?;
    Ok(ctx.bool_value(lhs == rhs))
}

fn float_lt(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatLt")?;
    Ok(ctx.bool_value(lhs < rhs))
}

fn float_le(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatLe")?;
    Ok(ctx.bool_value(lhs <= rhs))
}

fn float_gt(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatGt")?;
    Ok(ctx.bool_value(lhs > rhs))
}

fn float_ge(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatGe")?;
    Ok(ctx.bool_value(lhs >= rhs))
}

fn float_total_compare(ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let (lhs, rhs) = expect_float2(args, "floatTotalCompare")?;
    ordering_value(ctx, lhs.total_cmp(&rhs), "floatTotalCompare")
}

fn int_to_float(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_int(args, 0, "intToFloat")?;
    Ok(Value::Float(value as f64))
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "Solveig floatToInt is defined as Rust's saturating truncating f64-to-i64 conversion"
)]
fn float_to_int(_ctx: &mut VmCtx<'_, '_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = expect_float(args, 0, "floatToInt")?;
    // D3: truncate toward zero. Rust's f64 -> i64 cast is saturating and keeps
    // this conversion total for NaN and out-of-range values.
    Ok(Value::Int(value.trunc() as i64))
}

fn ordering_value(
    ctx: &VmCtx<'_, '_>,
    ordering: Ordering,
    builtin: &str,
) -> Result<Value, RuntimeError> {
    let ctor = match ordering {
        Ordering::Less => CtorRuntimeRole::PreludeOrderingLt,
        Ordering::Equal => CtorRuntimeRole::PreludeOrderingEq,
        Ordering::Greater => CtorRuntimeRole::PreludeOrderingGt,
    };
    ctx.nullary_ctor(ctor, builtin)
}

fn expect_value(args: &[Value], index: usize, builtin: &str) -> Result<Value, RuntimeError> {
    args.get(index).copied().ok_or_else(|| {
        RuntimeError::builtin(format!(
            "{builtin} expected argument at position {}",
            index + 1
        ))
    })
}

fn expect_int(args: &[Value], index: usize, builtin: &str) -> Result<i64, RuntimeError> {
    match expect_value(args, index, builtin)? {
        Value::Int(value) => Ok(value),
        _ => Err(RuntimeError::builtin(format!(
            "{builtin} expected Int argument at position {}",
            index + 1
        ))),
    }
}

fn expect_int2(args: &[Value], builtin: &str) -> Result<(i64, i64), RuntimeError> {
    Ok((expect_int(args, 0, builtin)?, expect_int(args, 1, builtin)?))
}

fn expect_float(args: &[Value], index: usize, builtin: &str) -> Result<f64, RuntimeError> {
    match expect_value(args, index, builtin)? {
        Value::Float(value) => Ok(value),
        _ => Err(RuntimeError::builtin(format!(
            "{builtin} expected Float argument at position {}",
            index + 1
        ))),
    }
}

fn expect_float2(args: &[Value], builtin: &str) -> Result<(f64, f64), RuntimeError> {
    Ok((
        expect_float(args, 0, builtin)?,
        expect_float(args, 1, builtin)?,
    ))
}

fn expect_char(args: &[Value], index: usize, builtin: &str) -> Result<char, RuntimeError> {
    match expect_value(args, index, builtin)? {
        Value::Char(value) => Ok(value),
        _ => Err(RuntimeError::builtin(format!(
            "{builtin} expected Char argument at position {}",
            index + 1
        ))),
    }
}

fn expect_char2(args: &[Value], builtin: &str) -> Result<(char, char), RuntimeError> {
    Ok((
        expect_char(args, 0, builtin)?,
        expect_char(args, 1, builtin)?,
    ))
}
