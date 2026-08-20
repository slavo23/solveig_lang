//! Runtime IO action representation and execution.
//! The current action stays in one register while suspended binds share the VM's
//! continuation stack. Blocking actions park the process until an IO worker
//! returns plain Rust data for scheduler-thread conversion.

use super::channel::{ChannelOutcome, bounded_channel_capacity};
use super::io_os::{EnvironmentOutcome, FilesystemOutcome};
use super::io_worker::IoRequest;
use super::*;
use crate::builtins::VmCtx;
use crate::bytecode::BytecodeProgram;
use crate::handle::HandleRequest;
use crate::heap::IoAction;
use crate::values::{Continuation, Value};

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

        if self.timer_result.take().is_some() {
            self.io_current = None;
            if let Some(value) = self.resume_io_value(program, Value::Unit)? {
                self.continuations.clear();
                return Ok(value);
            }
        }

        if let Some(result) = self.io_result.take() {
            self.io_current = None;
            if let Some(value) = self.resume_worker_result(program, result)? {
                self.continuations.clear();
                return Ok(value);
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
                IoAction::RaiseDefect { defect } => {
                    let recover_heap = self.defect_requires_heap_recovery(defect);
                    self.resume_io_defect_value(program, defect, recover_heap, None)?;
                }
                IoAction::Finally { action, cleanup } => {
                    self.continuations.push(Continuation::IoFinally(cleanup));
                    self.io_current = Some(action);
                }
                IoAction::Exit { status } => {
                    self.resume_io_exit(program, ExitStatus::new(status))?;
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
                IoAction::Environment(action) => match self.perform_environment_action(action)? {
                    EnvironmentOutcome::Value(value) => {
                        if let Some(value) = self.resume_io_value(program, value)? {
                            return Ok(value);
                        }
                    }
                    EnvironmentOutcome::Failure(failure) => {
                        self.io_current = Some(self.alloc_io_failure_action(failure)?);
                    }
                },
                IoAction::Filesystem(action) => match self.prepare_filesystem_action(action)? {
                    FilesystemOutcome::Value(value) => {
                        if let Some(value) = self.resume_io_value(program, value)? {
                            return Ok(value);
                        }
                    }
                    FilesystemOutcome::Failure(failure) => {
                        self.io_current = Some(self.alloc_io_failure_action(failure)?);
                    }
                    FilesystemOutcome::Request(request) => {
                        if let Some(interruption) = self.accept_pending_interruption_at_block() {
                            self.resume_io_interruption(program, interruption)?;
                            continue;
                        }
                        return self.block_on_io(IoRequest::Filesystem(request));
                    }
                },
                IoAction::RunProcess {
                    program: child_program,
                    arguments,
                    maximum,
                } => {
                    let request =
                        self.prepare_process_request(child_program, arguments, maximum)?;
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::Process(request));
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
                IoAction::Sleep { milliseconds } => {
                    if milliseconds <= 0 {
                        if let Some(value) = self.resume_io_value(program, Value::Unit)? {
                            return Ok(value);
                        }
                        continue;
                    }
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    let Ok(milliseconds) = u64::try_from(milliseconds) else {
                        solveig_base::ice!("positive sleep duration did not fit u64")
                    };
                    let deadline = self.timers.register(self.id, milliseconds);
                    self.status = ProcStatus::Blocked(BlockReason::Timer(deadline));
                    return Err(StepOutcome::Blocked);
                }
                IoAction::GetMonotonicTime => {
                    let nanoseconds = self.timers.monotonic_nanos();
                    if let Some(value) = self.resume_io_value(program, Value::Int(nanoseconds))? {
                        return Ok(value);
                    }
                    continue;
                }
                IoAction::GetSystemTime => {
                    let nanoseconds = self.timers.system_nanos();
                    if let Some(value) = self.resume_io_value(program, Value::Int(nanoseconds))? {
                        return Ok(value);
                    }
                    continue;
                }
                IoAction::NewStdGen => {
                    let generator = self.random_seeder.next_generator();
                    if let Some(value) = self.resume_io_value(program, Value::StdGen(generator))? {
                        return Ok(value);
                    }
                    continue;
                }
                IoAction::ResolveHost { host } => {
                    let host = self.expect_string_value(host, "resolveHost")?;
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::ResolveHost(host));
                }
                IoAction::Connect { host, port } => {
                    let host = self.expect_string_value(host, "connect")?;
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::Connect { host, port });
                }
                IoAction::Listen { port } => {
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::Listen { port });
                }
                IoAction::Accept { socket } => {
                    let socket = self.expect_socket_id(socket, "accept")?;
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::Accept(socket));
                }
                IoAction::SendBytes { socket, bytes } => {
                    let socket = self.expect_socket_id(socket, "sendBytes")?;
                    let bytes = self.expect_bytes_value(bytes, "sendBytes")?;
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::SendBytes { socket, bytes });
                }
                IoAction::RecvBytes { socket, maximum } => {
                    let socket = self.expect_socket_id(socket, "recvBytes")?;
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::RecvBytes { socket, maximum });
                }
                IoAction::CloseSocket { socket } => {
                    let socket = self.expect_socket_id(socket, "closeSocket")?;
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::CloseSocket(socket));
                }
                IoAction::ReadFile { path, maximum } => {
                    let builtin = if maximum.is_some() {
                        "readFileLimited"
                    } else {
                        "readFile"
                    };
                    let path = self.expect_string_value(path, builtin)?;
                    let resolved = self.io.resolve_path(&path);
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::ReadFile {
                        path: resolved,
                        maximum,
                    });
                }
                IoAction::ReadFileBytes { path, maximum } => {
                    let builtin = if maximum.is_some() {
                        "readFileBytesLimited"
                    } else {
                        "readFileBytes"
                    };
                    let path = self.expect_string_value(path, builtin)?;
                    let resolved = self.io.resolve_path(&path);
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::ReadFileBytes {
                        path: resolved,
                        maximum,
                    });
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
                IoAction::WriteFileBytes { path, contents } => {
                    let path = self.expect_string_value(path, "writeFileBytes")?;
                    let contents = self.expect_bytes_value(contents, "writeFileBytes")?;
                    let resolved = self.io.resolve_path(&path);
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::WriteFileBytes {
                        path: resolved,
                        contents,
                    });
                }
                IoAction::OpenFile { path, mode } => {
                    let path = self.expect_string_value(path, "openFile")?;
                    let resolved = self.io.resolve_path(&path);
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    let request_id = self.io.issue_open_file(self.id, resolved, mode)?;
                    self.status = ProcStatus::Blocked(BlockReason::Io(request_id));
                    return Err(StepOutcome::Blocked);
                }
                IoAction::CloseHandle { handle } => {
                    let handle = self.expect_handle_id(handle, "hClose")?;
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::Handle(HandleRequest::Close(handle)));
                }
                IoAction::FlushHandle { handle } => {
                    let handle = self.expect_handle_id(handle, "hFlush")?;
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::Handle(HandleRequest::Flush(handle)));
                }
                IoAction::GetHandleLine { handle } => {
                    let handle = self.expect_handle_id(handle, "hGetLine")?;
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::Handle(HandleRequest::GetLine(handle)));
                }
                IoAction::GetHandleContents { handle } => {
                    let handle = self.expect_handle_id(handle, "hGetContents")?;
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::Handle(HandleRequest::GetContents(handle)));
                }
                IoAction::GetHandleBytes { handle, maximum } => {
                    let handle = self.expect_handle_id(handle, "hGetBytes")?;
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::Handle(HandleRequest::GetBytes {
                        handle,
                        maximum,
                    }));
                }
                IoAction::PutHandle {
                    handle,
                    text,
                    newline,
                } => {
                    let handle = self.expect_handle_id(handle, "hPutStr")?;
                    let text = self.expect_string_value(text, "hPutStr")?;
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::Handle(HandleRequest::Put {
                        handle,
                        text,
                        newline,
                    }));
                }
                IoAction::PutHandleBytes { handle, bytes } => {
                    let handle = self.expect_handle_id(handle, "hPutBytes")?;
                    let bytes = self.expect_bytes_value(bytes, "hPutBytes")?;
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self
                        .block_on_io(IoRequest::Handle(HandleRequest::PutBytes { handle, bytes }));
                }
                IoAction::IsHandleEof { handle } => {
                    let handle = self.expect_handle_id(handle, "hIsEOF")?;
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::Handle(HandleRequest::IsEof(handle)));
                }
                IoAction::SetHandleBuffering { handle, buffering } => {
                    let handle = self.expect_handle_id(handle, "hSetBuffering")?;
                    if let Some(interruption) = self.accept_pending_interruption_at_block() {
                        self.resume_io_interruption(program, interruption)?;
                        continue;
                    }
                    return self.block_on_io(IoRequest::Handle(HandleRequest::SetBuffering {
                        handle,
                        buffering,
                    }));
                }
            }
        }
    }

    fn block_on_io(&mut self, request: IoRequest) -> Result<Value, StepOutcome> {
        let request_id = self.io.issue(self.id, request)?;
        self.status = ProcStatus::Blocked(BlockReason::Io(request_id));
        Err(StepOutcome::Blocked)
    }

    pub(super) fn resume_io_value(
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
                Continuation::IoRestoreExit { status, mask_depth } => {
                    self.mask_depth = mask_depth;
                    self.resume_io_exit(program, status)?;
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
                | Continuation::IoRestoreInterruption { mask_depth, .. }
                | Continuation::IoRestoreExit { mask_depth, .. } => {
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
}
