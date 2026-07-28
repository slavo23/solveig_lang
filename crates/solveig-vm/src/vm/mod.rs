//! Stack virtual machine state and execution entry points.
//! Child modules own call handling, dispatch, garbage collection, and IO actions.

use crate::bytecode::{BytecodeProgram, lower_to_bytecode};
use crate::display::DisplayValue;
use crate::heap::{Heap, HeapObject};
use crate::values::{BoolCache, Continuation, GlobalSlot, IoFrame, NullaryCtorCache, Value};
use solveig_ast::ast::CtorRuntimeInfo;
use solveig_lower::closure_ir::CCProgram;
use std::fmt;
use std::io::{BufRead, Cursor, Write};
use std::path::Path;

mod call;
mod dispatch;
mod gc;
mod io;
#[cfg(test)]
mod tests;

use io::IoRuntime;

struct Vm {
    chunk: usize,
    pc: usize,
    frame_base: usize,

    stack: Vec<Value>,
    continuations: Vec<Continuation>,
    binding_buffer: Vec<Value>,
    temporary_roots: Vec<Value>,
    io_current: Option<Value>,
    io_stack: Vec<IoFrame>,

    heap: Heap,
    globals: Vec<GlobalSlot>,
    nullary_ctors: NullaryCtorCache,
    bools: BoolCache,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InvokeMode {
    Call,
    HaltCall,
    TailCall,
}

impl Vm {
    fn new(program: &BytecodeProgram) -> Self {
        let max_tag = program
            .constructor_arities
            .iter()
            .map(|info| info.tag as usize)
            .max()
            .unwrap_or(0);

        let mut vm = Vm {
            chunk: 0,
            pc: 0,
            frame_base: 0,
            stack: Vec::with_capacity(4096),
            continuations: Vec::with_capacity(512),
            binding_buffer: Vec::with_capacity(8),
            temporary_roots: Vec::with_capacity(8),
            io_current: None,
            io_stack: Vec::with_capacity(512),
            heap: Heap::default(),
            globals: program
                .globals
                .iter()
                .map(|g| GlobalSlot::Thunk { chunk: g.chunk })
                .collect(),
            nullary_ctors: NullaryCtorCache::new(max_tag),
            bools: BoolCache {
                true_val: Value::Unit,
                false_val: Value::Unit,
            },
        };

        vm.init_nullary(&program.constructor_arities);
        vm
    }
}

pub struct EvalResult {
    root: Value,
    heap: Heap,
    constructor_arities: Vec<CtorRuntimeInfo>,
}

pub struct IoEvalResult {
    pub result: EvalResult,
    pub stdout: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    pub code: u16,
    pub message: String,
}

impl RuntimeError {
    fn trap(code: u16) -> Self {
        Self {
            code,
            message: crate::bytecode::trap_message(code),
        }
    }

    pub(crate) fn builtin(message: String) -> Self {
        Self { code: 2, message }
    }

    fn cyclic_global(name: &str) -> Self {
        Self {
            code: 3,
            message: format!("cyclic global `{name}`"),
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RuntimeError {}

pub struct Machine;

impl Machine {
    pub fn exec(program: &CCProgram, entry_name: &str) -> Result<EvalResult, RuntimeError> {
        let bytecode = lower_to_bytecode(program);
        Self::exec_compiled(&bytecode, entry_name)
    }

    pub fn exec_compiled(
        program: &BytecodeProgram,
        entry_name: &str,
    ) -> Result<EvalResult, RuntimeError> {
        let Some(entry) = program.global_index_by_name(entry_name) else {
            return Err(RuntimeError {
                code: 0,
                message: format!("entry '{entry_name}' not found"),
            });
        };

        let mut vm = Vm::new(program);
        let root = vm.eval_global(program, entry)?;

        Ok(EvalResult {
            root,
            heap: vm.heap,
            constructor_arities: program.constructor_arities.clone(),
        })
    }

    pub fn exec_io(
        program: &CCProgram,
        entry_name: &str,
        stdin: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<IoEvalResult, RuntimeError> {
        let bytecode = lower_to_bytecode(program);
        Self::exec_compiled_io(&bytecode, entry_name, stdin, args, cwd)
    }

    pub fn exec_compiled_io(
        program: &BytecodeProgram,
        entry_name: &str,
        stdin: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<IoEvalResult, RuntimeError> {
        let mut input = Cursor::new(stdin.as_bytes());
        let mut stdout = Vec::new();
        let result = Self::exec_compiled_io_with_streams(
            program,
            entry_name,
            &mut input,
            &mut stdout,
            args,
            cwd,
        )?;
        let stdout = String::from_utf8(stdout).map_err(|err| {
            RuntimeError::builtin(format!("captured IO output was not valid UTF-8: {err}"))
        })?;

        Ok(IoEvalResult { result, stdout })
    }

    pub fn exec_io_with_streams(
        program: &CCProgram,
        entry_name: &str,
        stdin: &mut dyn BufRead,
        stdout: &mut dyn Write,
        args: &[String],
        cwd: &Path,
    ) -> Result<EvalResult, RuntimeError> {
        let bytecode = lower_to_bytecode(program);
        Self::exec_compiled_io_with_streams(&bytecode, entry_name, stdin, stdout, args, cwd)
    }

    pub fn exec_compiled_io_with_streams(
        program: &BytecodeProgram,
        entry_name: &str,
        stdin: &mut dyn BufRead,
        stdout: &mut dyn Write,
        args: &[String],
        cwd: &Path,
    ) -> Result<EvalResult, RuntimeError> {
        let Some(entry) = program.global_index_by_name(entry_name) else {
            return Err(RuntimeError {
                code: 0,
                message: format!("entry '{entry_name}' not found"),
            });
        };

        let mut vm = Vm::new(program);
        let mut io = IoRuntime::new(stdin, stdout, args, cwd);
        let action = vm.eval_global(program, entry)?;
        let root = vm.perform_io(program, action, &mut io)?;

        Ok(EvalResult {
            root,
            heap: vm.heap,
            constructor_arities: program.constructor_arities.clone(),
        })
    }

    pub fn dump_bytecode<P: AsRef<std::path::Path>>(
        program: &CCProgram,
        path: P,
    ) -> std::io::Result<()> {
        let bytecode = lower_to_bytecode(program);
        bytecode.dump_to_file(path)
    }
}

impl EvalResult {
    pub fn display(&self) -> DisplayValue<'_> {
        DisplayValue {
            root: self.root,
            heap: &self.heap,
            arities: &self.constructor_arities,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        let Value::Obj(id) = self.root else {
            return None;
        };

        match self.heap.get(id) {
            HeapObject::Str { bytes } => Some(self.heap.str_value(*bytes)),
            _ => None,
        }
    }
}
