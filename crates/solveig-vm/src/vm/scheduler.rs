//! Deterministic single-threaded scheduling for lightweight Solveig processes.

use super::*;

pub(super) fn run(runtime: &mut Runtime, process: &mut Process) -> StepOutcome {
    if process.reductions_left == 0 {
        process.reductions_left = runtime.reduction_budget;
    }
    process.status = ProcStatus::Running;
    let result = {
        let mut interpreter = Interpreter::new(runtime, process);
        let program = interpreter.program;
        interpreter.dispatch(program)
    };
    let outcome = match result {
        Ok(value) => StepOutcome::Finished(value),
        Err(outcome) => outcome,
    };
    runtime.record_process_outcome(process, &outcome);
    outcome
}

pub(super) fn evaluate_entry(runtime: &mut Runtime, entry: usize) -> (ProcessId, StepOutcome) {
    let id = runtime.allocate_process();
    let Some(queued) = runtime.run_queue.pop_front() else {
        solveig_base::ice!("new process was not placed on the run queue");
    };
    if queued != id {
        solveig_base::ice!("run queue changed process creation order");
    }

    // The active process leaves the arena for the duration of a step so the
    // interpreter can borrow shared runtime state without interior mutability.
    let mut process = runtime.take_process(id);
    let started = {
        let mut interpreter = Interpreter::new(runtime, &mut process);
        let program = interpreter.program;
        interpreter.start_global_eval(
            program,
            entry,
            Continuation::PublishGlobalThenHalt {
                global_index: entry,
            },
        )
    };
    let outcome = match started {
        Ok(GlobalEval::Evaluated(value)) => {
            let outcome = StepOutcome::Finished(value);
            runtime.record_process_outcome(&mut process, &outcome);
            outcome
        }
        Ok(GlobalEval::Entered) => loop {
            let outcome = run(runtime, &mut process);
            if outcome != StepOutcome::Yielded {
                break outcome;
            }
            let Some(queued) = runtime.run_queue.pop_front() else {
                solveig_base::ice!("yielded process was not returned to the run queue");
            };
            if queued != id {
                solveig_base::ice!("single-process evaluation lost FIFO run-queue order");
            }
        },
        Ok(GlobalEval::Blocked) => {
            let outcome = StepOutcome::Blocked;
            runtime.record_process_outcome(&mut process, &outcome);
            outcome
        }
        Err(error) => {
            let outcome = StepOutcome::Failed(error);
            runtime.record_process_outcome(&mut process, &outcome);
            outcome
        }
    };
    runtime.restore_process(process);
    (id, outcome)
}

pub(super) fn perform_process_io(
    runtime: &mut Runtime,
    id: ProcessId,
    action: Value,
    io: &mut IoRuntime<'_>,
) -> StepOutcome {
    let mut process = runtime.take_process(id);
    if process.reductions_left == 0 {
        process.reductions_left = runtime.reduction_budget;
    }
    process.status = ProcStatus::Running;
    let result = {
        let mut interpreter = Interpreter::new(runtime, &mut process);
        let program = interpreter.program;
        interpreter.perform_io(program, action, io)
    };
    let outcome = match result {
        Ok(value) => StepOutcome::Finished(value),
        Err(outcome) => outcome,
    };
    runtime.record_process_outcome(&mut process, &outcome);
    runtime.restore_process(process);
    outcome
}

impl Interpreter<'_> {
    pub(super) fn spawn_io_process(&mut self, action: Value) -> Value {
        let id = allocate_process_in(
            self.inactive_processes,
            self.run_queue,
            self.reduction_budget,
            Some(action),
        );
        Value::ThreadId(id.into_raw().into_u32())
    }
}

/// Runs runnable processes until the main process completes or fails.
///
/// This follows the lightweight-thread scheduler model described by Marlow and
/// Peyton Jones in the AOSA GHC runtime chapter and by Peyton Jones, Gordon,
/// and Finne in "Concurrent Haskell": one OS thread repeatedly executes a
/// bounded process step and handles its explicit return reason.
pub(super) fn schedule_io(
    runtime: &mut Runtime,
    main: ProcessId,
    action: Value,
    io: &mut IoRuntime<'_>,
) -> Result<Value, RuntimeError> {
    let main_process = &mut runtime.processes[main];
    main_process.io_current = Some(action);
    main_process.status = ProcStatus::Runnable;
    runtime.run_queue.push_back(main);

    loop {
        let Some(id) = runtime.run_queue.pop_front() else {
            return Err(RuntimeError::builtin(
                "scheduler stopped with the main thread blocked".to_string(),
            ));
        };
        if runtime.processes[id].status != ProcStatus::Runnable {
            solveig_base::ice!("run queue contained a non-runnable process");
        }

        let outcome = perform_process_io(runtime, id, Value::Unit, io);
        match outcome {
            StepOutcome::Finished(value) if id == main => return Ok(value),
            StepOutcome::Yielded | StepOutcome::Blocked | StepOutcome::Finished(_) => {}
            StepOutcome::Failed(error) if id == main => return Err(error),
            StepOutcome::Failed(error) => {
                io.report_thread_failure(id.into_raw().into_u32(), &error)?;
            }
        }
    }
}
