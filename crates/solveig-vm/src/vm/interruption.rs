//! Asynchronous interruption delivery and scoped masking.
//! The scheduler cancels blocked registrations before waking a target, while
//! the IO continuation stack owns cleanup and mask restoration during unwind.

use super::*;
use crate::bytecode::BytecodeProgram;
use crate::values::Continuation;

impl Interpreter<'_> {
    /// Requests delivery at the target's next safe point or interruptible wait.
    ///
    /// Marlow et al., "Asynchronous Exceptions in Haskell" (PLDI 2001),
    /// sections 5.2-5.3 and 8, require scoped masking, deferred runnable
    /// delivery, and immediate wakeup from operations that can wait forever.
    pub(super) fn interrupt_thread(&mut self, thread: Value) -> Result<(), RuntimeError> {
        let Value::ThreadId(raw) = thread else {
            return Err(RuntimeError::builtin(
                "killThread expected a ThreadId".to_string(),
            ));
        };
        let target = ProcessId::from_raw(RawIdx::from(raw));
        if target == self.id {
            self.interrupted.get_or_insert(Interruption::Killed);
            return Ok(());
        }

        if raw as usize >= self.inactive_processes.len() {
            return Ok(());
        }
        let status = self.inactive_processes[target].status.clone();
        match status {
            ProcStatus::Runnable => {
                self.inactive_processes[target]
                    .interrupted
                    .get_or_insert(Interruption::Killed);
            }
            ProcStatus::Blocked(reason) => {
                match reason {
                    BlockReason::Chan(channel) => self.remove_channel_waiter(channel, target),
                    BlockReason::Io(request) => self.io.cancel(request, target),
                    BlockReason::Blackhole(global) => self.remove_blackhole_waiter(global, target),
                    BlockReason::Timer(deadline) => self.timers.cancel(target, deadline),
                }
                let process = &mut self.inactive_processes[target];
                process.interrupted.get_or_insert(Interruption::Killed);
                // Blocking operations are interruptible even inside `mask`;
                // accepting the interruption abandons the masked scope.
                process.mask_depth = 0;
                process.status = ProcStatus::Runnable;
                self.run_queue.push_back(target);
            }
            ProcStatus::Finished(_) | ProcStatus::Failed(_) | ProcStatus::Interrupted(_) => {}
            ProcStatus::Running => {
                solveig_base::ice!("inactive process table contained a running process")
            }
        }
        Ok(())
    }

    pub(super) fn enter_mask(&mut self) -> Result<u32, RuntimeError> {
        let previous = self.mask_depth;
        self.mask_depth = previous
            .checked_add(1)
            .ok_or_else(|| RuntimeError::defect(DefectKind::StackExhausted))?;
        Ok(previous)
    }

    pub(super) fn enter_restore_mask(&mut self) -> Result<u32, RuntimeError> {
        let previous = self.mask_depth;
        let Some(restored) = previous.checked_sub(1) else {
            return Err(RuntimeError::builtin(
                "restoreMaskIO executed outside mask".to_string(),
            ));
        };
        self.mask_depth = restored;
        Ok(previous)
    }

    pub(super) fn deliver_pending_interruption(
        &mut self,
        program: &BytecodeProgram,
    ) -> Result<(), StepOutcome> {
        if self.mask_depth != 0 {
            return Ok(());
        }
        if let Some(interruption) = self.interrupted.take() {
            self.resume_io_interruption(program, interruption)?;
        }
        Ok(())
    }

    pub(super) fn accept_pending_interruption_at_block(&mut self) -> Option<Interruption> {
        let interruption = self.interrupted.take()?;
        self.mask_depth = 0;
        Some(interruption)
    }

    pub(super) fn resume_io_interruption(
        &mut self,
        program: &BytecodeProgram,
        interruption: Interruption,
    ) -> Result<(), StepOutcome> {
        self.io_current = None;
        loop {
            let frame_base = self.frame_base;
            self.stack.truncate(frame_base);
            let Some(frame) = self.continuations.pop() else {
                return Err(StepOutcome::Interrupted(interruption));
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
                    self.abandon_global_eval(program, GlobalIndex(global_index));
                    self.chunk = chunk;
                    self.pc = pc;
                    self.frame_base = frame_base;
                }
                Continuation::PublishGlobalThenHalt { global_index } => {
                    self.abandon_global_eval(program, GlobalIndex(global_index));
                    self.frame_base = self.stack.len();
                }
                Continuation::Halt => {
                    self.frame_base = self.stack.len();
                }
                Continuation::IoFinally(cleanup) => {
                    let mask_depth = self.mask_depth;
                    self.mask_depth = mask_depth.checked_add(1).unwrap_or_else(|| {
                        solveig_base::ice!("cleanup mask depth overflowed before stack limit")
                    });
                    self.continuations
                        .push(Continuation::IoRestoreInterruption {
                            interruption,
                            mask_depth,
                        });
                    self.io_current = Some(cleanup);
                    return Ok(());
                }
                Continuation::IoRestoreMask(mask_depth)
                | Continuation::IoRestoreInterruption { mask_depth, .. } => {
                    self.mask_depth = mask_depth;
                }
                Continuation::IoBind(_)
                | Continuation::IoCatch(_)
                | Continuation::IoCatchDefect(_)
                | Continuation::IoRestore(_)
                | Continuation::IoRestoreDefect(_) => {}
            }
        }
    }

    fn remove_blackhole_waiter(&mut self, global: GlobalIndex, process: ProcessId) {
        let Some(GlobalSlot::Evaluating { waiters, .. }) = self.globals.get_mut(global.0) else {
            solveig_base::ice!("blocked blackhole process referenced a non-evaluating global")
        };
        let Some(index) = waiters.iter().position(|waiter| *waiter == process) else {
            solveig_base::ice!("blocked blackhole process was absent from its waiter queue")
        };
        waiters.remove(index);
    }

    fn abandon_global_eval(&mut self, program: &BytecodeProgram, global: GlobalIndex) {
        let Some(definition) = program.globals.get(global.0) else {
            solveig_base::ice!("interrupted process owned an absent global")
        };
        let process = self.id;
        let Some(slot) = self.globals.get_mut(global.0) else {
            solveig_base::ice!("interrupted process owned an out-of-bounds global")
        };
        let waiters = match slot {
            GlobalSlot::Evaluating { owner, waiters } if *owner == process => {
                std::mem::take(waiters)
            }
            GlobalSlot::Evaluating { .. }
            | GlobalSlot::Thunk { .. }
            | GlobalSlot::Evaluated(_)
            | GlobalSlot::Failed(_) => {
                solveig_base::ice!("interrupted process lost ownership of its global")
            }
        };
        *slot = GlobalSlot::Thunk {
            chunk: definition.chunk,
        };

        for waiter_id in waiters {
            let waiter = &mut self.inactive_processes[waiter_id];
            if waiter.status != ProcStatus::Blocked(BlockReason::Blackhole(global)) {
                solveig_base::ice!("abandoned global waiter had a mismatched status")
            }
            waiter.pc = waiter
                .pc
                .checked_sub(1)
                .unwrap_or_else(|| solveig_base::ice!("global waiter had no load to retry"));
            waiter.status = ProcStatus::Runnable;
            self.run_queue.push_back(waiter_id);
        }
    }
}
