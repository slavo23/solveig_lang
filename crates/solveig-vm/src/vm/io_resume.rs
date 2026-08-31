//! Resumes typed IO success and failure through the single continuation stack.
//! Cleanup and mask restoration remain part of the existing IO machine.

use super::*;

impl Interpreter<'_> {
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

    pub(super) fn resume_io_failure(
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
