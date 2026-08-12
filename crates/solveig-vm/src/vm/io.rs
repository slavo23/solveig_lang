//! Runtime IO action representation and execution.
//! The current action stays in one register while suspended binds share the VM's
//! continuation stack. Blocking actions park the process until an IO worker
//! returns plain Rust data for scheduler-thread conversion.

use super::channel::{ChannelOutcome, bounded_channel_capacity};
use super::io_worker::{IoFailure, IoRequest, IoResult};
use super::*;
use crate::builtins::VmCtx;
use crate::bytecode::BytecodeProgram;
use crate::heap::{HeapObject, IoAction};
use crate::values::{Continuation, Value};
use solveig_ast::ast::CtorRuntimeRole;

impl Interpreter<'_> {
    pub(super) fn perform_io(
        &mut self,
        program: &BytecodeProgram,
        action: Value,
    ) -> Result<Value, StepOutcome> {
        self.deliver_pending_interruption(program)?;

        if let Some(error) = self.pending_defect.take() {
            self.resume_io_defect(program, error)?;
        }

        if let Some(result) = self.channel_result.take() {
            self.io_current = None;
            match result {
                ChannelResult::Value(value) => {
                    if let Some(value) = self.resume_io_value(program, value)? {
                        self.continuations.clear();
                        return Ok(value);
                    }
                }
                ChannelResult::Closed => {
                    self.io_current = Some(self.alloc_channel_closed_action()?);
                }
            }
        }

        if let Some(result) = self.io_result.take() {
            self.io_current = None;
            match result {
                IoResult::Unit => {
                    if let Some(value) = self.resume_io_value(program, Value::Unit)? {
                        self.continuations.clear();
                        return Ok(value);
                    }
                }
                IoResult::Text(text) => {
                    self.safe_point();
                    let value = self.heap.alloc_str(&text);
                    if let Some(value) = self.resume_io_value(program, value)? {
                        self.continuations.clear();
                        return Ok(value);
                    }
                }
                IoResult::Failed(failure) => {
                    self.io_current = Some(self.alloc_io_failure_action(failure)?);
                }
            }
        }

        if self.io_current.is_none() {
            if self.continuations.is_empty() {
                self.io_current = Some(action);
            } else {
                match self.dispatch(program) {
                    Ok(next) => self.io_current = Some(next),
                    Err(StepOutcome::Failed(error)) if error.defect_kind().is_some() => {
                        self.resume_io_defect(program, error)?;
                    }
                    Err(outcome) => return Err(outcome),
                }
            }
        }

        let result = self.perform_io_loop(program);

        if result.is_ok() {
            self.io_current = None;
            self.continuations.clear();
        }
        result
    }

    fn perform_io_loop(&mut self, program: &BytecodeProgram) -> Result<Value, StepOutcome> {
        // Awkward Squad section 2 gives IO bind the sequencing contract; AOSA's
        // GHC RTS chapter keeps resumable runtime state explicit at safe points.
        loop {
            self.deliver_pending_interruption(program)?;
            if let Some(error) = self.resource_defect() {
                self.resume_io_defect(program, error)?;
                continue;
            }
            let current = self
                .io_current
                .expect("ICE: IO machine missing current action");

            match self.expect_io_action(current)? {
                IoAction::Pure(value) => {
                    if let Some(value) = self.resume_io_value(program, value)? {
                        return Ok(value);
                    }
                }
                IoAction::Fail(error) => {
                    if let Some(value) = self.resume_io_failure(program, current, error)? {
                        return Ok(value);
                    }
                }
                IoAction::Bind {
                    action,
                    continuation,
                } => {
                    self.continuations.push(Continuation::IoBind(continuation));
                    self.io_current = Some(action);
                }
                IoAction::Catch { action, handler } => {
                    self.continuations.push(Continuation::IoCatch(handler));
                    self.io_current = Some(action);
                }
                IoAction::CatchDefect { action, handler } => {
                    self.continuations
                        .push(Continuation::IoCatchDefect(handler));
                    self.io_current = Some(action);
                }
                IoAction::Finally { action, cleanup } => {
                    self.continuations.push(Continuation::IoFinally(cleanup));
                    self.io_current = Some(action);
                }
                IoAction::Print { text, newline } => {
                    let mut text = self.expect_string_value(text, "IO print")?;
                    if newline {
                        text.push('\n');
                    }
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::Print(text));
                }
                IoAction::ReadLine => {
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::ReadLine);
                }
                IoAction::GetArgs => {
                    let checkpoint = self.temporary_roots.len();
                    let arg_count = self.io.args().len();
                    for index in 0..arg_count {
                        let Some(arg) = self.io.args().get(index).cloned() else {
                            solveig_base::ice!("IO argument index disappeared during allocation");
                        };
                        self.safe_point();
                        let value = self.heap.alloc_str(&arg);
                        self.temporary_roots.push(value);
                    }

                    let mut ctx = VmCtx::new(self);
                    let value = ctx.alloc_list_from_root_range(checkpoint, arg_count, "getArgs");
                    self.temporary_roots.truncate(checkpoint);
                    let value = value?;
                    if let Some(value) = self.resume_io_value(program, value)? {
                        return Ok(value);
                    }
                }
                IoAction::Spawn { action } => {
                    let thread_id = self.spawn_io_process(action);
                    if let Some(value) = self.resume_io_value(program, thread_id)? {
                        return Ok(value);
                    }
                }
                IoAction::NewChan { capacity } => {
                    let capacity = match capacity {
                        Some(capacity) => match bounded_channel_capacity(capacity) {
                            Ok(capacity) => Some(capacity),
                            Err(error) => {
                                self.resume_io_defect(program, error)?;
                                continue;
                            }
                        },
                        None => None,
                    };
                    let channel = self.alloc_channel(capacity);
                    if let Some(value) = self.resume_io_value(program, channel)? {
                        return Ok(value);
                    }
                }
                IoAction::Send { channel, value } => {
                    let id = self.expect_channel_id(channel, "send")?;
                    match self.send_channel(id, value) {
                        ChannelOutcome::Ready(value) => {
                            if let Some(value) = self.resume_io_value(program, value)? {
                                return Ok(value);
                            }
                        }
                        ChannelOutcome::Closed => {
                            self.io_current = Some(self.alloc_channel_closed_action()?);
                        }
                        ChannelOutcome::Blocked => {
                            if let Some(interruption) = self.accept_pending_interruption_at_block()
                            {
                                self.remove_channel_waiter(id, self.id);
                                self.status = ProcStatus::Running;
                                self.resume_io_interruption(program, interruption)?;
                            } else {
                                return Err(StepOutcome::Blocked);
                            }
                        }
                        ChannelOutcome::Empty => {
                            solveig_base::ice!("send returned the receive-only empty outcome")
                        }
                    }
                }
                IoAction::Recv { channel } => {
                    let id = self.expect_channel_id(channel, "recv")?;
                    match self.recv_channel(id, true) {
                        ChannelOutcome::Ready(value) => {
                            if let Some(value) = self.resume_io_value(program, value)? {
                                return Ok(value);
                            }
                        }
                        ChannelOutcome::Closed => {
                            self.io_current = Some(self.alloc_channel_closed_action()?);
                        }
                        ChannelOutcome::Blocked => {
                            if let Some(interruption) = self.accept_pending_interruption_at_block()
                            {
                                self.remove_channel_waiter(id, self.id);
                                self.status = ProcStatus::Running;
                                self.resume_io_interruption(program, interruption)?;
                            } else {
                                return Err(StepOutcome::Blocked);
                            }
                        }
                        ChannelOutcome::Empty => {
                            solveig_base::ice!("blocking recv returned an empty outcome")
                        }
                    }
                }
                IoAction::TryRecv { channel } => {
                    let id = self.expect_channel_id(channel, "tryRecv")?;
                    let value = match self.recv_channel(id, false) {
                        ChannelOutcome::Ready(value) => self.alloc_maybe_value(Some(value))?,
                        ChannelOutcome::Empty => self.alloc_maybe_value(None)?,
                        ChannelOutcome::Closed => {
                            self.io_current = Some(self.alloc_channel_closed_action()?);
                            continue;
                        }
                        ChannelOutcome::Blocked => {
                            solveig_base::ice!("tryRecv blocked")
                        }
                    };
                    if let Some(value) = self.resume_io_value(program, value)? {
                        return Ok(value);
                    }
                }
                IoAction::CloseChan { channel } => {
                    let id = self.expect_channel_id(channel, "closeChan")?;
                    match self.close_channel_value(id) {
                        ChannelOutcome::Ready(value) => {
                            if let Some(value) = self.resume_io_value(program, value)? {
                                return Ok(value);
                            }
                        }
                        ChannelOutcome::Empty
                        | ChannelOutcome::Closed
                        | ChannelOutcome::Blocked => {
                            solveig_base::ice!("closeChan returned an impossible outcome")
                        }
                    }
                }
                IoAction::KillThread { thread } => {
                    self.interrupt_thread(thread)?;
                    self.deliver_pending_interruption(program)?;
                    if let Some(value) = self.resume_io_value(program, Value::Unit)? {
                        return Ok(value);
                    }
                }
                IoAction::Mask { action } => {
                    let previous = self.enter_mask()?;
                    self.continuations
                        .push(Continuation::IoRestoreMask(previous));
                    self.io_current = Some(action);
                }
                IoAction::RestoreMask { action } => {
                    let previous = self.enter_restore_mask()?;
                    self.continuations
                        .push(Continuation::IoRestoreMask(previous));
                    self.io_current = Some(action);
                }
                IoAction::ReadFile { path } => {
                    let path = self.expect_string_value(path, "readFile")?;
                    let resolved = self.io.resolve_path(&path);
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::ReadFile(resolved));
                }
                IoAction::WriteFile {
                    path,
                    contents,
                    append,
                } => {
                    let builtin = if append { "appendFile" } else { "writeFile" };
                    let path = self.expect_string_value(path, builtin)?;
                    let contents = self.expect_string_value(contents, builtin)?;
                    let resolved = self.io.resolve_path(&path);
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::WriteFile {
                        path: resolved,
                        contents,
                        append,
                    });
                }
            }
        }
    }

    fn block_on_io(&mut self, request: IoRequest) -> Result<Value, StepOutcome> {
        let request_id = self.io.issue(self.id, request)?;
        self.status = ProcStatus::Blocked(BlockReason::Io(request_id));
        Err(StepOutcome::Blocked)
    }

    fn resume_io_value(
        &mut self,
        program: &BytecodeProgram,
        value: Value,
    ) -> Result<Option<Value>, StepOutcome> {
        loop {
            let Some(frame) = self.continuations.pop() else {
                return Ok(Some(value));
            };

            match frame {
                Continuation::IoBind(continuation) => {
                    self.io_current = None;
                    match self.call_value_for_result(program, continuation, &[value]) {
                        Ok(next) => self.io_current = Some(next),
                        Err(StepOutcome::Failed(error)) if error.defect_kind().is_some() => {
                            self.resume_io_defect(program, error)?;
                        }
                        Err(outcome) => return Err(outcome),
                    }
                    return Ok(None);
                }
                Continuation::IoCatch(_) | Continuation::IoCatchDefect(_) => {}
                Continuation::IoFinally(cleanup) => {
                    let outcome = self.alloc_io_success_action(value);
                    self.continuations.push(Continuation::IoRestore(outcome));
                    self.io_current = Some(cleanup);
                    return Ok(None);
                }
                Continuation::IoRestore(outcome) => {
                    self.io_current = Some(outcome);
                    return Ok(None);
                }
                Continuation::IoRestoreDefect(defect) => {
                    self.resume_io_defect_value(program, defect, false, None)?;
                    return Ok(None);
                }
                Continuation::IoRestoreMask(mask_depth) => {
                    self.mask_depth = mask_depth;
                    self.io_current = None;
                    self.deliver_pending_interruption(program)?;
                    if self.io_current.is_some() {
                        return Ok(None);
                    }
                }
                Continuation::IoRestoreInterruption {
                    interruption,
                    mask_depth,
                } => {
                    self.mask_depth = mask_depth;
                    self.resume_io_interruption(program, interruption)?;
                    return Ok(None);
                }
                Continuation::Halt
                | Continuation::CallerFrame { .. }
                | Continuation::PublishGlobalThenHalt { .. }
                | Continuation::PublishGlobalThenResume { .. } => {
                    solveig_base::ice!("non-IO continuation reached the IO machine")
                }
            }
        }
    }

    fn resume_io_failure(
        &mut self,
        program: &BytecodeProgram,
        action: Value,
        error: Value,
    ) -> Result<Option<Value>, StepOutcome> {
        loop {
            let Some(frame) = self.continuations.pop() else {
                return Ok(Some(action));
            };

            match frame {
                Continuation::IoBind(_)
                | Continuation::IoCatchDefect(_)
                | Continuation::IoRestore(_)
                | Continuation::IoRestoreDefect(_) => {}
                Continuation::IoCatch(handler) => {
                    self.io_current = None;
                    match self.call_value_for_result(program, handler, &[error]) {
                        Ok(next) => self.io_current = Some(next),
                        Err(StepOutcome::Failed(error)) if error.defect_kind().is_some() => {
                            self.resume_io_defect(program, error)?;
                        }
                        Err(outcome) => return Err(outcome),
                    }
                    return Ok(None);
                }
                Continuation::IoFinally(cleanup) => {
                    self.continuations.push(Continuation::IoRestore(action));
                    self.io_current = Some(cleanup);
                    return Ok(None);
                }
                Continuation::IoRestoreMask(mask_depth)
                | Continuation::IoRestoreInterruption { mask_depth, .. } => {
                    self.mask_depth = mask_depth;
                }
                Continuation::Halt
                | Continuation::CallerFrame { .. }
                | Continuation::PublishGlobalThenHalt { .. }
                | Continuation::PublishGlobalThenResume { .. } => {
                    solveig_base::ice!("non-IO continuation reached the IO machine")
                }
            }
        }
    }

    fn resume_io_defect(
        &mut self,
        program: &BytecodeProgram,
        error: RuntimeError,
    ) -> Result<(), StepOutcome> {
        let Some(kind) = error.defect_kind().cloned() else {
            return Err(StepOutcome::Failed(error));
        };
        let recover_heap = matches!(kind, DefectKind::HeapExhausted);
        let defect = self.alloc_defect(kind)?;
        self.resume_io_defect_value(program, defect, recover_heap, Some(error))
    }

    fn resume_io_defect_value(
        &mut self,
        program: &BytecodeProgram,
        defect: Value,
        recover_heap: bool,
        mut boundary_error: Option<RuntimeError>,
    ) -> Result<(), StepOutcome> {
        loop {
            let frame_base = self.frame_base;
            self.stack.truncate(frame_base);
            let Some(frame) = self.continuations.pop() else {
                let error = boundary_error.unwrap_or_else(|| self.render_uncaught_defect(defect));
                return Err(StepOutcome::Failed(error));
            };

            match frame {
                Continuation::CallerFrame {
                    chunk,
                    pc,
                    frame_base,
                } => {
                    self.chunk = chunk;
                    self.pc = pc;
                    self.frame_base = frame_base;
                }
                Continuation::PublishGlobalThenResume {
                    global_index,
                    chunk,
                    pc,
                    frame_base,
                } => {
                    let error = boundary_error
                        .get_or_insert_with(|| self.render_uncaught_defect(defect))
                        .clone();
                    self.publish_global_defect(global_index, &error);
                    self.chunk = chunk;
                    self.pc = pc;
                    self.frame_base = frame_base;
                }
                Continuation::PublishGlobalThenHalt { global_index } => {
                    let error = boundary_error
                        .get_or_insert_with(|| self.render_uncaught_defect(defect))
                        .clone();
                    self.publish_global_defect(global_index, &error);
                    self.frame_base = self.stack.len();
                }
                Continuation::Halt => {
                    self.frame_base = self.stack.len();
                }
                Continuation::IoBind(_)
                | Continuation::IoCatch(_)
                | Continuation::IoRestore(_)
                | Continuation::IoRestoreDefect(_) => {}
                Continuation::IoFinally(cleanup) => {
                    self.continuations
                        .push(Continuation::IoRestoreDefect(defect));
                    let cleanup = self.root_across_defect_recovery(cleanup, recover_heap);
                    self.io_current = Some(cleanup);
                    return Ok(());
                }
                Continuation::IoRestoreMask(mask_depth)
                | Continuation::IoRestoreInterruption { mask_depth, .. } => {
                    self.mask_depth = mask_depth;
                }
                Continuation::IoCatchDefect(handler) => {
                    let checkpoint = self.temporary_roots.len();
                    self.temporary_roots.extend([handler, defect]);
                    if recover_heap {
                        self.collect_for_defect_recovery();
                    }
                    let handler = *self
                        .temporary_roots
                        .get(checkpoint)
                        .expect("ICE: defect handler root disappeared during recovery");
                    let defect = *self
                        .temporary_roots
                        .get(checkpoint + 1)
                        .expect("ICE: defect value root disappeared during recovery");
                    self.temporary_roots.truncate(checkpoint);
                    self.io_current = None;
                    match self.call_value_for_result(program, handler, &[defect]) {
                        Ok(next) => self.io_current = Some(next),
                        Err(StepOutcome::Failed(error)) if error.defect_kind().is_some() => {
                            return self.resume_io_defect(program, error);
                        }
                        Err(outcome) => return Err(outcome),
                    }
                    return Ok(());
                }
            }
        }
    }

    fn root_across_defect_recovery(&mut self, value: Value, recover_heap: bool) -> Value {
        if !recover_heap {
            return value;
        }
        let checkpoint = self.temporary_roots.len();
        self.temporary_roots.push(value);
        self.collect_for_defect_recovery();
        let value = *self
            .temporary_roots
            .get(checkpoint)
            .expect("ICE: defect recovery root disappeared during collection");
        self.temporary_roots.truncate(checkpoint);
        value
    }

    fn alloc_defect(&mut self, defect: DefectKind) -> Result<Value, RuntimeError> {
        match defect {
            DefectKind::Error(message) => {
                self.alloc_string_ctor(CtorRuntimeRole::PreludeDefectError, &message, "error")
            }
            DefectKind::DivisionByZero => VmCtx::new(self)
                .nullary_ctor(CtorRuntimeRole::PreludeDefectDivisionByZero, "division"),
            DefectKind::StackExhausted => VmCtx::new(self).nullary_ctor(
                CtorRuntimeRole::PreludeDefectStackExhausted,
                "stack exhaustion",
            ),
            DefectKind::HeapExhausted => VmCtx::new(self).nullary_ctor(
                CtorRuntimeRole::PreludeDefectHeapExhausted,
                "heap exhaustion",
            ),
        }
    }

    fn render_uncaught_defect(&self, defect: Value) -> RuntimeError {
        let rendered = DisplayValue {
            root: defect,
            heap: &*self.heap,
            arities: &self.program.constructor_arities,
        }
        .to_string();
        RuntimeError::restored_defect(&rendered)
    }

    fn publish_global_defect(&mut self, global_index: usize, error: &RuntimeError) {
        let process_id = self.id;
        let Some(slot) = self.globals.get_mut(global_index) else {
            solveig_base::ice!("defect unwind reached an out-of-bounds global index");
        };
        let waiters = match slot {
            GlobalSlot::Evaluating { owner, waiters } if *owner == process_id => {
                std::mem::take(waiters)
            }
            GlobalSlot::Evaluating { .. }
            | GlobalSlot::Thunk { .. }
            | GlobalSlot::Evaluated(_)
            | GlobalSlot::Failed(_) => {
                solveig_base::ice!("defect unwind lost ownership of its global blackhole")
            }
        };
        *slot = GlobalSlot::Failed(error.clone());

        for waiter_id in waiters {
            let waiter = &mut self.inactive_processes[waiter_id];
            if waiter.status
                != ProcStatus::Blocked(BlockReason::Blackhole(GlobalIndex(global_index)))
            {
                solveig_base::ice!("defected global waiter has a mismatched process status");
            }
            waiter.pending_defect = Some(error.clone());
            waiter.status = ProcStatus::Runnable;
            self.run_queue.push_back(waiter_id);
        }
    }

    fn alloc_io_success_action(&mut self, value: Value) -> Value {
        let root = self.temporary_roots.len();
        self.temporary_roots.push(value);
        self.safe_point();
        let value = *self
            .temporary_roots
            .get(root)
            .expect("ICE: IO success root disappeared during allocation");
        let action = self.heap.alloc(HeapObject::IoAction(IoAction::Pure(value)));
        self.temporary_roots.truncate(root);
        action
    }

    pub(super) fn alloc_io_failure_action(
        &mut self,
        failure: IoFailure,
    ) -> Result<Value, RuntimeError> {
        let error = self.alloc_io_error(failure)?;
        self.alloc_typed_failure_action(error)
    }

    fn alloc_channel_closed_action(&mut self) -> Result<Value, RuntimeError> {
        let error = VmCtx::new(self).nullary_ctor(
            CtorRuntimeRole::PreludeIoErrorChannelClosed,
            "channel operation",
        )?;
        self.alloc_typed_failure_action(error)
    }

    fn alloc_typed_failure_action(&mut self, error: Value) -> Result<Value, RuntimeError> {
        let root = self.temporary_roots.len();
        self.temporary_roots.push(error);
        self.safe_point();
        let error = *self
            .temporary_roots
            .get(root)
            .expect("ICE: IO error root disappeared during allocation");
        let action = self.heap.alloc(HeapObject::IoAction(IoAction::Fail(error)));
        self.temporary_roots.truncate(root);
        Ok(action)
    }

    fn alloc_maybe_value(&mut self, value: Option<Value>) -> Result<Value, RuntimeError> {
        let Some(value) = value else {
            return VmCtx::new(self).nullary_ctor(CtorRuntimeRole::PreludeMaybeNothing, "tryRecv");
        };

        let root = self.temporary_roots.len();
        self.temporary_roots.push(value);
        self.safe_point();
        let value = *self
            .temporary_roots
            .get(root)
            .expect("ICE: tryRecv value root disappeared during allocation");
        let result =
            VmCtx::new(self).alloc_data1_ctor(CtorRuntimeRole::PreludeMaybeJust, value, "tryRecv");
        self.temporary_roots.truncate(root);
        result
    }

    fn alloc_io_error(&mut self, failure: IoFailure) -> Result<Value, RuntimeError> {
        match failure {
            IoFailure::Print(error) => self.alloc_string_ctor(
                CtorRuntimeRole::PreludeIoErrorStdout,
                &error.to_string(),
                "print",
            ),
            IoFailure::ReadLine(error) => self.alloc_string_ctor(
                CtorRuntimeRole::PreludeIoErrorStdin,
                &error.to_string(),
                "readLine",
            ),
            IoFailure::ReadFile { path, source } => self.alloc_io_error2(
                CtorRuntimeRole::PreludeIoErrorReadFile,
                &path.display().to_string(),
                &source.to_string(),
                "readFile",
            ),
            IoFailure::WriteFile {
                path,
                append,
                source,
            } => {
                let (role, builtin) = if append {
                    (CtorRuntimeRole::PreludeIoErrorAppendFile, "appendFile")
                } else {
                    (CtorRuntimeRole::PreludeIoErrorWriteFile, "writeFile")
                };
                self.alloc_io_error2(
                    role,
                    &path.display().to_string(),
                    &source.to_string(),
                    builtin,
                )
            }
        }
    }

    fn alloc_string_ctor(
        &mut self,
        role: CtorRuntimeRole,
        message: &str,
        builtin: &str,
    ) -> Result<Value, RuntimeError> {
        self.safe_point();
        let root = self.temporary_roots.len();
        let message = self.heap.alloc_str(message);
        self.temporary_roots.push(message);
        self.safe_point();
        let message = *self
            .temporary_roots
            .get(root)
            .expect("ICE: IO error message root disappeared during allocation");
        let result = VmCtx::new(self).alloc_data1_ctor(role, message, builtin);
        self.temporary_roots.truncate(root);
        result
    }

    fn alloc_io_error2(
        &mut self,
        role: CtorRuntimeRole,
        first: &str,
        second: &str,
        builtin: &str,
    ) -> Result<Value, RuntimeError> {
        self.safe_point();
        let root = self.temporary_roots.len();
        let first = self.heap.alloc_str(first);
        self.temporary_roots.push(first);
        self.safe_point();
        let second = self.heap.alloc_str(second);
        self.temporary_roots.push(second);
        self.safe_point();
        let first = *self
            .temporary_roots
            .get(root)
            .expect("ICE: first IO error field root disappeared during allocation");
        let second = *self
            .temporary_roots
            .get(root + 1)
            .expect("ICE: second IO error field root disappeared during allocation");
        let result = VmCtx::new(self).alloc_data2_ctor(role, first, second, builtin);
        self.temporary_roots.truncate(root);
        result
    }

    pub(super) fn expect_io_action(&self, value: Value) -> Result<IoAction, RuntimeError> {
        match value {
            Value::Obj(id) => match self.heap.get(id) {
                HeapObject::IoAction(action) => Ok(*action),
                _ => Err(RuntimeError::builtin(
                    "main returned a non-IO heap value while performing IO".to_string(),
                )),
            },
            _ => Err(RuntimeError::builtin(
                "main returned a non-IO value while performing IO".to_string(),
            )),
        }
    }

    pub(super) fn expect_string_value(
        &self,
        value: Value,
        context: &str,
    ) -> Result<String, RuntimeError> {
        match value {
            Value::Obj(id) => match self.heap.get(id) {
                HeapObject::Str { bytes } => Ok(self.heap.str_value(id, *bytes).to_string()),
                _ => Err(RuntimeError::builtin(format!("{context} expected String"))),
            },
            _ => Err(RuntimeError::builtin(format!("{context} expected String"))),
        }
    }
}
