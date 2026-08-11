//! Stack virtual machine state and execution entry points.
//! Child modules own call handling, dispatch, garbage collection, and IO actions.

use crate::bytecode::{BytecodeProgram, lower_to_bytecode};
use crate::display::DisplayValue;
use crate::heap::{Heap, HeapObject};
use crate::values::{BoolCache, Continuation, NullaryCtorCache, Value};
use la_arena::{Arena, Idx, RawIdx};
use solveig_ast::ast::CtorRuntimeInfo;
use solveig_lower::closure_ir::CCProgram;
use std::collections::VecDeque;
use std::fmt;
use std::io::{BufRead, Cursor, Write};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::sync::Mutex;
use std::thread;

mod call;
mod dispatch;
mod gc;
mod io;
mod io_worker;
mod scheduler;
#[cfg(test)]
mod tests;

use io_worker::{IoResult, IoSubsystem};
use scheduler::evaluate_entry;
#[cfg(test)]
use scheduler::{perform_process_io, run};

const DEFAULT_REDUCTIONS: u32 = 4_096;
/// Identifies one process in a runtime's process arena.
pub type ProcessId = Idx<Process>;

/// One green thread's instruction position and private execution stacks.
pub struct Process {
    pub id: ProcessId,
    chunk: usize,
    pc: usize,
    frame_base: usize,
    stack: Vec<Value>,
    continuations: Vec<Continuation>,
    binding_buffer: Vec<Value>,
    pub(crate) temporary_roots: Vec<Value>,
    io_current: Option<Value>,
    io_result: Option<IoResult>,
    status: ProcStatus,
    reductions_left: u32,
}

/// A resumable bytecode frame exposed in current-to-oldest-caller order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessFrame {
    pub chunk: usize,
    pub pc: usize,
    pub frame_base: usize,
}

/// Shared state that outlives every individual interpreter step.
pub struct Runtime {
    program: BytecodeProgram,
    heap: Heap,
    globals: Vec<GlobalSlot>,
    nullary_ctors: NullaryCtorCache,
    bools: BoolCache,
    processes: Arena<Process>,
    run_queue: VecDeque<ProcessId>,
    io: IoSubsystem,
    reduction_budget: u32,
}

enum GlobalSlot {
    Thunk {
        chunk: usize,
    },
    Evaluating {
        owner: ProcessId,
        waiters: Vec<ProcessId>,
    },
    Evaluated(Value),
    Failed(RuntimeError),
}
/// Describes whether a process may run or why it has stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcStatus {
    Runnable,
    Running,
    Blocked(BlockReason),
    Finished(Value),
    Failed(RuntimeError),
}

/// Describes the resource that prevents a process from running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    Io(RequestId),
    Blackhole(GlobalIndex),
}

/// Identifies one request submitted to the runtime IO subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(u32);

/// Identifies one global slot in the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlobalIndex(usize);

enum GlobalEval {
    Evaluated(Value),
    Entered,
    Blocked,
}

/// Reports why one interpreter step returned to its scheduler.
///
/// This mirrors GHC's scheduler return reasons; see the AOSA GHC runtime
/// chapter and the header of GHC's `rts/Schedule.c`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    Yielded,
    Blocked,
    Finished(Value),
    Failed(RuntimeError),
}

pub(crate) struct Interpreter<'a> {
    pub(crate) process: &'a mut Process,
    pub(crate) program: &'a BytecodeProgram,
    pub(crate) heap: &'a mut Heap,
    globals: &'a mut Vec<GlobalSlot>,
    pub(crate) nullary_ctors: &'a mut NullaryCtorCache,
    pub(crate) bools: &'a mut BoolCache,
    inactive_processes: &'a mut Arena<Process>,
    run_queue: &'a mut VecDeque<ProcessId>,
    io: &'a mut IoSubsystem,
    reduction_budget: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InvokeMode {
    Call,
    HaltCall,
    TailCall,
}

impl Process {
    fn new(id: ProcessId, reduction_budget: u32) -> Self {
        Self {
            id,
            chunk: 0,
            pc: 0,
            frame_base: 0,
            stack: Vec::with_capacity(4096),
            continuations: Vec::with_capacity(512),
            binding_buffer: Vec::with_capacity(8),
            temporary_roots: Vec::with_capacity(8),
            io_current: None,
            io_result: None,
            status: ProcStatus::Runnable,
            reductions_left: reduction_budget,
        }
    }

    fn placeholder(id: ProcessId) -> Self {
        Self {
            id,
            chunk: 0,
            pc: 0,
            frame_base: 0,
            stack: Vec::new(),
            continuations: Vec::new(),
            binding_buffer: Vec::new(),
            temporary_roots: Vec::new(),
            io_current: None,
            io_result: None,
            status: ProcStatus::Running,
            reductions_left: 0,
        }
    }

    fn record_outcome(&mut self, outcome: &StepOutcome) {
        self.status = match outcome {
            StepOutcome::Yielded => ProcStatus::Runnable,
            StepOutcome::Blocked => match &self.status {
                ProcStatus::Blocked(reason) => ProcStatus::Blocked(*reason),
                ProcStatus::Runnable
                | ProcStatus::Running
                | ProcStatus::Finished(_)
                | ProcStatus::Failed(_) => {
                    solveig_base::ice!("blocked step returned without a block reason")
                }
            },
            StepOutcome::Finished(value) => ProcStatus::Finished(*value),
            StepOutcome::Failed(error) => ProcStatus::Failed(error.clone()),
        };
    }

    /// Walks the suspended process's active bytecode frames without mutating it.
    pub fn frames(&self) -> impl Iterator<Item = ProcessFrame> + '_ {
        let current = ProcessFrame {
            chunk: self.chunk,
            pc: self.pc,
            frame_base: self.frame_base,
        };

        std::iter::once(current).chain(self.continuations.iter().rev().filter_map(|continuation| {
            match continuation {
                Continuation::CallerFrame {
                    chunk,
                    pc,
                    frame_base,
                }
                | Continuation::PublishGlobalThenResume {
                    chunk,
                    pc,
                    frame_base,
                    ..
                } => Some(ProcessFrame {
                    chunk: *chunk,
                    pc: *pc,
                    frame_base: *frame_base,
                }),
                Continuation::Halt
                | Continuation::PublishGlobalThenHalt { .. }
                | Continuation::IoBind(_)
                | Continuation::IoCatch(_)
                | Continuation::IoFinally(_)
                | Continuation::IoRestore(_) => None,
            }
        }))
    }
}

impl Runtime {
    fn new(program: BytecodeProgram) -> Self {
        Self::new_with_heap_and_reduction_budget(
            program,
            Heap::default(),
            reduction_budget_from_env(),
            IoSubsystem::inactive(),
        )
    }

    fn new_with_io(program: BytecodeProgram, io: IoSubsystem) -> Self {
        Self::new_with_heap_and_reduction_budget(
            program,
            Heap::default(),
            reduction_budget_from_env(),
            io,
        )
    }

    fn new_with_heap_and_reduction_budget(
        program: BytecodeProgram,
        heap: Heap,
        reduction_budget: u32,
        io: IoSubsystem,
    ) -> Self {
        let max_tag = program
            .constructor_arities
            .iter()
            .map(|info| info.tag as usize)
            .max()
            .unwrap_or(0);

        let globals = program
            .globals
            .iter()
            .map(|global| GlobalSlot::Thunk {
                chunk: global.chunk,
            })
            .collect();
        let mut runtime = Self {
            program,
            heap,
            globals,
            nullary_ctors: NullaryCtorCache::new(max_tag),
            bools: BoolCache {
                true_val: Value::Unit,
                false_val: Value::Unit,
            },
            processes: Arena::new(),
            run_queue: VecDeque::new(),
            io,
            reduction_budget,
        };
        runtime.init_nullary();
        runtime
    }

    #[cfg(test)]
    fn new_with_collection_limits_for_test(
        program: BytecodeProgram,
        nursery_slot_count: usize,
        next_major_at: usize,
    ) -> Self {
        let mut heap = Heap::default();
        heap.set_collection_limits_for_test(nursery_slot_count, next_major_at);
        Self::new_with_heap_and_reduction_budget(
            program,
            heap,
            reduction_budget_from_env(),
            IoSubsystem::inactive(),
        )
    }

    fn allocate_process(&mut self) -> ProcessId {
        allocate_process_in(
            &mut self.processes,
            &mut self.run_queue,
            self.reduction_budget,
            None,
        )
    }

    fn take_process(&mut self, id: ProcessId) -> Process {
        std::mem::replace(&mut self.processes[id], Process::placeholder(id))
    }

    fn restore_process(&mut self, process: Process) {
        let id = process.id;
        self.processes[id] = process;
    }

    fn record_process_outcome(&mut self, process: &mut Process, outcome: &StepOutcome) {
        process.record_outcome(outcome);
        match outcome {
            StepOutcome::Yielded => self.run_queue.push_back(process.id),
            StepOutcome::Failed(error) => {
                self.fail_owned_blackholes(process, error);
                process.io_current = None;
                process.io_result = None;
                process.continuations.clear();
            }
            StepOutcome::Blocked | StepOutcome::Finished(_) => {}
        }
    }

    fn fail_owned_blackholes(&mut self, owner: &Process, error: &RuntimeError) {
        let mut owners = vec![(owner.id, owned_globals(owner))];

        while let Some((owner_id, globals)) = owners.pop() {
            for global_index in globals {
                let Some(slot) = self.globals.get_mut(global_index.0) else {
                    solveig_base::ice!("failed process owns an out-of-bounds global blackhole");
                };
                let waiters = match slot {
                    GlobalSlot::Evaluating { owner, waiters } if *owner == owner_id => {
                        std::mem::take(waiters)
                    }
                    GlobalSlot::Evaluating { .. }
                    | GlobalSlot::Thunk { .. }
                    | GlobalSlot::Evaluated(_)
                    | GlobalSlot::Failed(_) => {
                        solveig_base::ice!("failed process lost ownership of its global blackhole")
                    }
                };
                *slot = GlobalSlot::Failed(error.clone());

                for waiter_id in waiters {
                    let waiter = &mut self.processes[waiter_id];
                    if waiter.status != ProcStatus::Blocked(BlockReason::Blackhole(global_index)) {
                        solveig_base::ice!(
                            "global blackhole waiter has a mismatched process status"
                        );
                    }
                    owners.push((waiter_id, owned_globals(waiter)));
                    waiter.io_current = None;
                    waiter.io_result = None;
                    waiter.continuations.clear();
                    waiter.status = ProcStatus::Failed(error.clone());
                }
            }
        }
    }

    fn into_eval_result(self, root: Value) -> EvalResult {
        EvalResult {
            root,
            heap: self.heap,
            constructor_arities: self.program.constructor_arities,
        }
    }
}

impl<'a> Interpreter<'a> {
    fn new(runtime: &'a mut Runtime, process: &'a mut Process) -> Self {
        let Runtime {
            program,
            heap,
            globals,
            nullary_ctors,
            bools,
            processes,
            run_queue,
            io,
            reduction_budget,
        } = runtime;
        Self {
            process,
            program,
            heap,
            globals,
            nullary_ctors,
            bools,
            inactive_processes: processes,
            run_queue,
            io,
            reduction_budget: *reduction_budget,
        }
    }
}

fn allocate_process_in(
    processes: &mut Arena<Process>,
    run_queue: &mut VecDeque<ProcessId>,
    reduction_budget: u32,
    io_action: Option<Value>,
) -> ProcessId {
    let raw = u32::try_from(processes.len())
        .unwrap_or_else(|_| solveig_base::ice!("process arena exhausted u32 identities"));
    let id = ProcessId::from_raw(RawIdx::from(raw));
    let mut process = Process::new(id, reduction_budget);
    process.io_current = io_action;
    let allocated = processes.alloc(process);
    if allocated != id {
        solveig_base::ice!("process arena returned a non-sequential identity");
    }
    run_queue.push_back(id);
    id
}

impl Deref for Interpreter<'_> {
    type Target = Process;

    fn deref(&self) -> &Self::Target {
        self.process
    }
}

impl DerefMut for Interpreter<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.process
    }
}

impl From<RuntimeError> for StepOutcome {
    fn from(error: RuntimeError) -> Self {
        Self::Failed(error)
    }
}

fn owned_globals(process: &Process) -> Vec<GlobalIndex> {
    process
        .continuations
        .iter()
        .filter_map(|continuation| match continuation {
            Continuation::PublishGlobalThenHalt { global_index }
            | Continuation::PublishGlobalThenResume { global_index, .. } => {
                Some(GlobalIndex(*global_index))
            }
            Continuation::Halt
            | Continuation::CallerFrame { .. }
            | Continuation::IoBind(_)
            | Continuation::IoCatch(_)
            | Continuation::IoFinally(_)
            | Continuation::IoRestore(_) => None,
        })
        .collect()
}

fn outcome_value(outcome: StepOutcome) -> Result<Value, RuntimeError> {
    match outcome {
        StepOutcome::Finished(value) => Ok(value),
        StepOutcome::Failed(error) => Err(error),
        StepOutcome::Yielded | StepOutcome::Blocked => {
            solveig_base::ice!("single-process execution stopped before producing a value")
        }
    }
}

fn reduction_budget_from_env() -> u32 {
    std::env::var("SOLVEIG_REDUCTIONS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|budget| *budget > 0)
        .unwrap_or(DEFAULT_REDUCTIONS)
}

pub struct EvalResult {
    root: Value,
    heap: Heap,
    constructor_arities: Vec<CtorRuntimeInfo>,
}

pub struct IoEvalResult {
    pub result: EvalResult,
    pub stdout: String,
    pub stderr: String,
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
        Self::exec_owned(bytecode, entry_name)
    }

    pub fn exec_compiled(
        program: &BytecodeProgram,
        entry_name: &str,
    ) -> Result<EvalResult, RuntimeError> {
        Self::exec_owned(program.clone(), entry_name)
    }

    fn exec_owned(program: BytecodeProgram, entry_name: &str) -> Result<EvalResult, RuntimeError> {
        let Some(entry) = program.global_index_by_name(entry_name) else {
            return Err(RuntimeError {
                code: 0,
                message: format!("entry '{entry_name}' not found"),
            });
        };

        let mut runtime = Runtime::new(program);
        let (_, outcome) = evaluate_entry(&mut runtime, entry);
        let root = outcome_value(outcome)?;

        Ok(runtime.into_eval_result(root))
    }

    pub fn exec_io(
        program: &CCProgram,
        entry_name: &str,
        stdin: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<IoEvalResult, RuntimeError> {
        let bytecode = lower_to_bytecode(program);
        Self::exec_owned_io(bytecode, entry_name, stdin, args, cwd)
    }

    pub fn exec_compiled_io(
        program: &BytecodeProgram,
        entry_name: &str,
        stdin: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<IoEvalResult, RuntimeError> {
        Self::exec_owned_io(program.clone(), entry_name, stdin, args, cwd)
    }

    fn exec_owned_io(
        program: BytecodeProgram,
        entry_name: &str,
        stdin: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<IoEvalResult, RuntimeError> {
        let mut input = Cursor::new(stdin.as_bytes());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let result = Self::exec_owned_io_with_streams(
            program,
            entry_name,
            &mut input,
            &mut stdout,
            &mut stderr,
            args,
            cwd,
        )?;
        let stdout = String::from_utf8(stdout).map_err(|err| {
            RuntimeError::builtin(format!("captured IO output was not valid UTF-8: {err}"))
        })?;
        let stderr = String::from_utf8(stderr).map_err(|err| {
            RuntimeError::builtin(format!(
                "captured IO error output was not valid UTF-8: {err}"
            ))
        })?;

        Ok(IoEvalResult {
            result,
            stdout,
            stderr,
        })
    }

    pub fn exec_io_with_streams(
        program: &CCProgram,
        entry_name: &str,
        stdin: &mut (dyn BufRead + Send),
        stdout: &mut (dyn Write + Send),
        args: &[String],
        cwd: &Path,
    ) -> Result<EvalResult, RuntimeError> {
        let bytecode = lower_to_bytecode(program);
        let stderr = std::io::stderr();
        let mut stderr = stderr.lock();
        Self::exec_owned_io_with_streams(
            bytecode,
            entry_name,
            stdin,
            stdout,
            &mut stderr,
            args,
            cwd,
        )
    }

    pub fn exec_compiled_io_with_streams(
        program: &BytecodeProgram,
        entry_name: &str,
        stdin: &mut (dyn BufRead + Send),
        stdout: &mut (dyn Write + Send),
        args: &[String],
        cwd: &Path,
    ) -> Result<EvalResult, RuntimeError> {
        let stderr = std::io::stderr();
        let mut stderr = stderr.lock();
        Self::exec_owned_io_with_streams(
            program.clone(),
            entry_name,
            stdin,
            stdout,
            &mut stderr,
            args,
            cwd,
        )
    }

    fn exec_owned_io_with_streams(
        program: BytecodeProgram,
        entry_name: &str,
        stdin: &mut (dyn BufRead + Send),
        stdout: &mut (dyn Write + Send),
        stderr: &mut dyn Write,
        args: &[String],
        cwd: &Path,
    ) -> Result<EvalResult, RuntimeError> {
        let Some(entry) = program.global_index_by_name(entry_name) else {
            return Err(RuntimeError {
                code: 0,
                message: format!("entry '{entry_name}' not found"),
            });
        };

        let stdin = Mutex::new(stdin);
        let stdout = Mutex::new(stdout);
        thread::scope(|scope| {
            let io = IoSubsystem::start(scope, &stdin, &stdout, args, cwd)?;
            let mut runtime = Runtime::new_with_io(program, io);
            let (id, outcome) = evaluate_entry(&mut runtime, entry);
            let action = outcome_value(outcome)?;
            let root = scheduler::schedule_io(&mut runtime, id, action, stderr)?;

            Ok(runtime.into_eval_result(root))
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
            HeapObject::Str { bytes } => Some(self.heap.str_value(id, *bytes)),
            _ => None,
        }
    }
}
