//! VM execution regression tests.
//! These cases cover bytecode dispatch, closures, thunks, constructors, and IO.

use super::*;
use crate::bytecode::Instr;
use crate::heap::{Chan, ChanId, IoAction, PayloadStore};
use crate::vm::io_worker::IoFailure;
use solveig_ast::ast::{
    BUILTIN_BIND_IO, BUILTIN_CHARS_TO_STRING, BUILTIN_PURE_IO, BUILTIN_STRING_TO_CHARS, BinOp,
    BuiltinId, CtorId, CtorRuntimeRole, PRIM_TYS,
};
use solveig_base::{FileId, FileSpan, Span};
use solveig_lower::closure_ir::{CCExpr, CCFunction, CCGlobalDef};
use solveig_lower::{LowerCaseArm, LowerPattern};
use std::rc::Rc;

const TRUE_TAG: u16 = 0;
const FALSE_TAG: u16 = 1;
const NIL_TAG: u16 = 2;
const CONS_TAG: u16 = 3;
const PRELUDE_NIL_TAG: u16 = 4;
const PRELUDE_CONS_TAG: u16 = 5;
const PRELUDE_PAIR_TAG: u16 = 6;
const USER_PAIR_TAG: u16 = 7;

#[test]
fn bytecode_offsets_retain_nested_source_locations() {
    let outer = FileSpan::new(FileId(7), Span::new(10, 19));
    let left = FileSpan::new(FileId(7), Span::new(10, 12));
    let right = FileSpan::new(FileId(7), Span::new(17, 19));
    let body = located(
        outer,
        bin(BinOp::Plus, located(left, int(1)), located(right, int(2))),
    );
    let bytecode = lower_to_bytecode(&program(vec![global("main", body)], Vec::new(), &[]));
    let chunk = bytecode.chunk(bytecode.globals[0].chunk);

    assert_eq!(chunk.source_spans.len(), chunk.code.len());
    assert_eq!(chunk.source_span(0), Some(left));
    assert_eq!(chunk.source_span(1), Some(right));
    assert_eq!(chunk.source_span(2), Some(outer));
    assert_eq!(chunk.source_span(3), None);
}

#[test]
fn tail_recursive_countdown_uses_bounded_vm_stack() {
    let countdown = function(
        0,
        "countdown",
        2,
        if_then_else(
            bin(BinOp::Equal, local(1), int(0)),
            local(0),
            call_known(
                0,
                vec![
                    bin(BinOp::Minus, local(1), int(1)),
                    bin(BinOp::Plus, local(0), int(1)),
                ],
            ),
        ),
    );

    let program = program(
        vec![global("main", call_known(0, vec![int(5_000_000), int(0)]))],
        vec![countdown],
        &[],
    );

    let bytecode = lower_to_bytecode(&program);
    let countdown_chunk = bytecode
        .chunks
        .iter()
        .find(|chunk| chunk.name == "countdown")
        .expect("countdown chunk should be lowered");

    assert!(
        countdown_chunk
            .code
            .iter()
            .any(|instr| matches!(instr, Instr::TailCallKnown { .. })),
        "countdown must lower its recursive call as TailCallKnown"
    );
    assert!(
        !countdown_chunk
            .code
            .iter()
            .any(|instr| matches!(instr, Instr::CallKnown { .. })),
        "countdown should not keep a caller continuation for the recursive call"
    );

    assert_eq!(run_main(&program), "5000000");
}

#[test]
fn call_safe_points_yield_and_resume_with_consistent_process_state() {
    let countdown = function(
        0,
        "countdown",
        2,
        if_then_else(
            bin(BinOp::Equal, local(1), int(0)),
            local(0),
            call_known(
                0,
                vec![
                    bin(BinOp::Minus, local(1), int(1)),
                    bin(BinOp::Plus, local(0), int(1)),
                ],
            ),
        ),
    );
    let program = program(
        vec![global("main", call_known(0, vec![int(4), int(0)]))],
        vec![countdown],
        &[],
    );
    let mut runtime = runtime_with_reduction_budget(lower_to_bytecode(&program), 1);
    let process_id = runtime.allocate_process();
    runtime.run_queue.clear();
    start_process_global(&mut runtime, process_id, 0);

    let mut yields = 0;
    let mut observed_caller_frame = false;
    let result = loop {
        match run_process(&mut runtime, process_id) {
            StepOutcome::Yielded => {
                yields += 1;
                let process = &runtime.processes[process_id];
                assert_eq!(process.status, ProcStatus::Runnable);
                let frames = process.frames().collect::<Vec<_>>();
                assert!(!frames.is_empty());
                observed_caller_frame |= frames.len() > 1;
                for frame in frames {
                    assert!(frame.pc <= runtime.program.chunk(frame.chunk).code.len());
                    assert!(frame.frame_base <= process.stack.len());
                }
                assert_eq!(runtime.run_queue.pop_front(), Some(process_id));
            }
            StepOutcome::Finished(value) => break value,
            StepOutcome::Blocked | StepOutcome::Failed(_) | StepOutcome::Interrupted(_) => {
                solveig_base::ice!("countdown process did not remain runnable")
            }
        }
    };

    assert_eq!(result, Value::Int(4));
    assert_eq!(yields, 5);
    assert!(observed_caller_frame);
}

#[test]
fn allocation_safe_point_yields_after_publishing_the_allocated_root() {
    let program = program(
        vec![global("main", pack(CONS_TAG, 2, vec![int(1), int(2)]))],
        Vec::new(),
        &[("Cons", CONS_TAG, 2)],
    );
    let mut runtime = runtime_with_reduction_budget(lower_to_bytecode(&program), 1);
    let process_id = runtime.allocate_process();
    runtime.run_queue.clear();
    start_process_global(&mut runtime, process_id, 0);

    let outcome = run_process(&mut runtime, process_id);

    assert_eq!(outcome, StepOutcome::Yielded);
    assert!(matches!(
        runtime.processes[process_id].stack.last(),
        Some(Value::Obj(_))
    ));
    assert_eq!(runtime.run_queue.pop_front(), Some(process_id));
    assert!(matches!(
        run_process(&mut runtime, process_id),
        StepOutcome::Finished(Value::Obj(_))
    ));
}

#[test]
fn backward_jump_safe_point_restarts_at_the_branch_target() {
    let program = program(vec![global("main", int(0))], Vec::new(), &[]);
    let mut bytecode = lower_to_bytecode(&program);
    let chunk = bytecode.globals[0].chunk;
    bytecode.chunks[chunk].code = vec![Instr::Jump(0)];
    bytecode.chunks[chunk].source_spans = vec![None];
    let mut runtime = runtime_with_reduction_budget(bytecode, 1);
    let process_id = runtime.allocate_process();
    runtime.run_queue.clear();
    start_process_global(&mut runtime, process_id, 0);

    assert_eq!(run_process(&mut runtime, process_id), StepOutcome::Yielded);
    assert_eq!(runtime.processes[process_id].pc, 0);
    assert_eq!(runtime.run_queue.pop_front(), Some(process_id));
    assert_eq!(run_process(&mut runtime, process_id), StepOutcome::Yielded);
    assert_eq!(runtime.processes[process_id].pc, 0);
}

#[test]
fn yielded_processes_rejoin_the_fifo_run_queue() {
    let countdown = function(0, "identity", 1, local(0));
    let program = program(
        vec![
            global("first", call_known(0, vec![int(1)])),
            global("second", call_known(0, vec![int(2)])),
        ],
        vec![countdown],
        &[],
    );
    let mut runtime = runtime_with_reduction_budget(lower_to_bytecode(&program), 1);
    let first = runtime.allocate_process();
    let second = runtime.allocate_process();
    runtime.run_queue.clear();
    start_process_global(&mut runtime, first, 0);
    start_process_global(&mut runtime, second, 1);

    assert_eq!(run_process(&mut runtime, first), StepOutcome::Yielded);
    assert_eq!(run_process(&mut runtime, second), StepOutcome::Yielded);

    assert_eq!(runtime.run_queue.pop_front(), Some(first));
    assert_eq!(runtime.run_queue.pop_front(), Some(second));
}

#[test]
fn io_callback_yields_and_resumes_through_the_same_safe_point() {
    let continuation = function(
        0,
        "continuation",
        1,
        builtin(BUILTIN_PURE_IO, vec![bin(BinOp::Plus, local(0), int(1))]),
    );
    let action = builtin(
        BUILTIN_BIND_IO,
        vec![builtin(BUILTIN_PURE_IO, vec![int(41)]), function_ref(0, 1)],
    );
    let program = program(vec![global("main", action)], vec![continuation], &[]);
    let bytecode = lower_to_bytecode(&program);
    let entry = bytecode
        .global_index_by_name("main")
        .expect("IO safe-point fixture should contain main");
    let mut runtime = runtime_with_reduction_budget(bytecode, 1);
    let (process_id, outcome) = evaluate_entry(&mut runtime, entry);
    let StepOutcome::Finished(action) = outcome else {
        solveig_base::ice!("IO safe-point fixture did not produce an action");
    };
    let mut yields = 0;
    let result = loop {
        match perform_process_io(&mut runtime, process_id, action) {
            StepOutcome::Yielded => {
                yields += 1;
                assert_eq!(runtime.run_queue.pop_front(), Some(process_id));
            }
            StepOutcome::Finished(value) => break value,
            StepOutcome::Blocked | StepOutcome::Failed(_) | StepOutcome::Interrupted(_) => {
                solveig_base::ice!("IO callback did not remain resumable")
            }
        }
    };

    assert_eq!(result, Value::Int(42));
    assert!(yields >= 2);
}

#[test]
fn typed_io_failure_survives_collection_and_skips_bind_continuations() {
    let program = program(vec![global("main", int(0))], Vec::new(), &[]);
    let mut runtime = Runtime::new(lower_to_bytecode(&program));
    let process = runtime.allocate_process();
    assert_eq!(runtime.run_queue.pop_front(), Some(process));
    let error = runtime.alloc_str_for_test("typed failure");
    let failure = runtime
        .heap
        .alloc(HeapObject::IoAction(IoAction::Fail(error)));
    let action = runtime.heap.alloc(HeapObject::IoAction(IoAction::Bind {
        action: failure,
        continuation: Value::Int(99),
    }));
    runtime.processes[process].io_current = Some(action);
    runtime.collect_all_process_roots_for_test();

    let outcome = perform_process_io(&mut runtime, process, Value::Unit);

    let StepOutcome::Finished(Value::Obj(id)) = outcome else {
        solveig_base::ice!("typed IO failure did not finish as an inert failure action");
    };
    let HeapObject::IoAction(IoAction::Fail(Value::Obj(error))) = runtime.heap.get(id) else {
        solveig_base::ice!("typed IO failure lost its error value");
    };
    let HeapObject::Str { bytes } = runtime.heap.get(*error) else {
        solveig_base::ice!("typed IO failure error was not a string");
    };
    assert_eq!(runtime.heap.str_value(*error, *bytes), "typed failure");
}

#[test]
fn host_io_failure_becomes_the_matching_typed_error_constructor() {
    const STDOUT_ERROR_TAG: u16 = 2;
    const STDIN_ERROR_TAG: u16 = 3;
    let program = program_with_ctors(
        vec![global("main", int(0))],
        Vec::new(),
        &[
            ctor_spec(
                CtorId(300),
                CtorRuntimeRole::PreludeIoErrorStdout,
                "prelude.builtins.StdoutError",
                "StdoutError",
                STDOUT_ERROR_TAG,
                1,
            ),
            ctor_spec(
                CtorId(301),
                CtorRuntimeRole::PreludeIoErrorStdin,
                "prelude.builtins.StdinError",
                "StdinError",
                STDIN_ERROR_TAG,
                1,
            ),
        ],
    );
    let mut runtime = Runtime::new(lower_to_bytecode(&program));
    let process_id = runtime.allocate_process();
    assert_eq!(runtime.run_queue.pop_front(), Some(process_id));
    let mut process = runtime.take_process(process_id);
    let action = Interpreter::new(&mut runtime, &mut process)
        .alloc_io_failure_action(IoFailure::Print(std::io::Error::other("closed")))
        .expect("stdout failure should become a typed IO action");
    runtime.restore_process(process);

    let Value::Obj(action) = action else {
        solveig_base::ice!("typed stdout failure was not heap allocated");
    };
    let HeapObject::IoAction(IoAction::Fail(Value::Obj(error))) = runtime.heap.get(action) else {
        solveig_base::ice!("stdout failure did not become a failed IO action");
    };
    let HeapObject::Data1 { tag, field } = runtime.heap.get(*error) else {
        solveig_base::ice!("stdout failure did not use its unary IOError constructor");
    };
    assert_eq!(*tag, STDOUT_ERROR_TAG);
    let Value::Obj(message) = field else {
        solveig_base::ice!("stdout error message was not a heap string");
    };
    let HeapObject::Str { bytes } = runtime.heap.get(*message) else {
        solveig_base::ice!("stdout error message was not represented as String");
    };
    assert_eq!(runtime.heap.str_value(*message, *bytes), "closed");

    let mut process = runtime.take_process(process_id);
    let action = Interpreter::new(&mut runtime, &mut process)
        .alloc_io_failure_action(IoFailure::ReadLine(std::io::Error::other("unreadable")))
        .expect("stdin failure should become a typed IO action");
    runtime.restore_process(process);
    let Value::Obj(action) = action else {
        solveig_base::ice!("typed stdin failure was not heap allocated");
    };
    let HeapObject::IoAction(IoAction::Fail(Value::Obj(error))) = runtime.heap.get(action) else {
        solveig_base::ice!("stdin failure did not become a failed IO action");
    };
    let HeapObject::Data1 { tag, .. } = runtime.heap.get(*error) else {
        solveig_base::ice!("stdin failure did not use its unary IOError constructor");
    };
    assert_eq!(*tag, STDIN_ERROR_TAG);
}

#[test]
fn typed_io_handler_frames_keep_their_values_live() {
    let program = program(vec![global("main", int(0))], Vec::new(), &[]);
    let mut runtime = Runtime::new(lower_to_bytecode(&program));
    let process = runtime.allocate_process();
    assert_eq!(runtime.run_queue.pop_front(), Some(process));
    let caught = runtime.alloc_str_for_test("catch");
    let cleanup = runtime.alloc_str_for_test("finally");
    let restored = runtime.alloc_str_for_test("restore");
    let defect_handler = runtime.alloc_str_for_test("defect-catch");
    let restored_defect = runtime.alloc_str_for_test("defect-restore");
    runtime.processes[process].continuations.extend([
        Continuation::IoCatch(caught),
        Continuation::IoFinally(cleanup),
        Continuation::IoRestore(restored),
        Continuation::IoCatchDefect(defect_handler),
        Continuation::IoRestoreDefect(restored_defect),
    ]);

    runtime.collect_all_process_roots_for_test();

    let values = runtime.processes[process]
        .continuations
        .iter()
        .map(|continuation| match continuation {
            Continuation::IoCatch(value)
            | Continuation::IoCatchDefect(value)
            | Continuation::IoFinally(value)
            | Continuation::IoRestore(value)
            | Continuation::IoRestoreDefect(value) => *value,
            Continuation::Halt
            | Continuation::CallerFrame { .. }
            | Continuation::PublishGlobalThenHalt { .. }
            | Continuation::PublishGlobalThenResume { .. }
            | Continuation::IoBind(_)
            | Continuation::IoRestoreMask(_)
            | Continuation::IoRestoreInterruption { .. } => {
                solveig_base::ice!("typed IO root test found an unrelated continuation")
            }
        })
        .collect::<Vec<_>>();
    let strings = values
        .iter()
        .map(|value| {
            let Value::Obj(id) = value else {
                solveig_base::ice!("typed IO continuation payload was not heap allocated");
            };
            let HeapObject::Str { bytes } = runtime.heap.get(*id) else {
                solveig_base::ice!("typed IO continuation payload was not a string");
            };
            runtime.heap.str_value(*id, *bytes)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        strings,
        [
            "catch",
            "finally",
            "restore",
            "defect-catch",
            "defect-restore"
        ]
    );
}

#[test]
fn scheduler_waits_for_worker_io_and_wakes_the_blocked_process() {
    let program = program(vec![global("main", int(0))], vec![], &[]);
    let bytecode = lower_to_bytecode(&program);
    let mut input = Cursor::new(b"from worker\n".to_vec());
    let mut output = Vec::new();
    let input = Mutex::new(&mut input as &mut (dyn BufRead + Send));
    let output = Mutex::new(&mut output as &mut (dyn Write + Send));

    thread::scope(|scope| {
        let io = IoSubsystem::start(scope, &input, &output, &[], Path::new("."))
            .expect("IO workers should start");
        let mut runtime = Runtime::new_with_io(bytecode, io);
        let process_id = runtime.allocate_process();
        assert_eq!(runtime.run_queue.pop_front(), Some(process_id));
        let request = runtime
            .io
            .issue(process_id, io_worker::IoRequest::ReadLine)
            .expect("readLine request should enter the worker pool");
        runtime.processes[process_id].status = ProcStatus::Blocked(BlockReason::Io(request));

        assert_eq!(scheduler::next_runnable(&mut runtime), Ok(process_id));
        assert_eq!(runtime.processes[process_id].status, ProcStatus::Runnable);
        assert!(matches!(
            runtime.processes[process_id].io_result.take(),
            Some(IoResult::Text(line)) if line == "from worker"
        ));
    });
}

#[test]
fn scheduler_runs_another_process_while_worker_io_is_blocked() {
    struct GatedRead {
        release: Option<std::sync::mpsc::Receiver<()>>,
        contents: Cursor<Vec<u8>>,
    }

    impl std::io::Read for GatedRead {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if let Some(release) = self.release.take() {
                release.recv().map_err(|disconnected| {
                    std::io::Error::other(format!("read gate disconnected: {disconnected}"))
                })?;
            }
            std::io::Read::read(&mut self.contents, buffer)
        }
    }

    let program = program(vec![global("main", int(0))], vec![], &[]);
    let bytecode = lower_to_bytecode(&program);
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let mut input = std::io::BufReader::new(GatedRead {
        release: Some(release_rx),
        contents: Cursor::new(b"eventually\n".to_vec()),
    });
    let mut output = Vec::new();
    let input = Mutex::new(&mut input as &mut (dyn BufRead + Send));
    let output = Mutex::new(&mut output as &mut (dyn Write + Send));

    thread::scope(|scope| {
        let io = IoSubsystem::start(scope, &input, &output, &[], Path::new("."))
            .expect("IO workers should start");
        let mut runtime = Runtime::new_with_io(bytecode, io);
        let blocked = runtime.allocate_process();
        assert_eq!(runtime.run_queue.pop_front(), Some(blocked));
        let request = runtime
            .io
            .issue(blocked, io_worker::IoRequest::ReadLine)
            .expect("readLine request should enter the worker pool");
        runtime.processes[blocked].status = ProcStatus::Blocked(BlockReason::Io(request));
        let runnable = runtime.allocate_process();

        assert_eq!(scheduler::next_runnable(&mut runtime), Ok(runnable));
        assert_eq!(
            runtime.processes[blocked].status,
            ProcStatus::Blocked(BlockReason::Io(request))
        );

        release_tx
            .send(())
            .expect("blocked test reader should still be connected");
    });
}

#[test]
fn interruption_cancels_a_blocked_worker_io_registration() {
    struct GatedRead {
        release: Option<std::sync::mpsc::Receiver<()>>,
        contents: Cursor<Vec<u8>>,
    }

    impl std::io::Read for GatedRead {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if let Some(release) = self.release.take() {
                release.recv().map_err(|disconnected| {
                    std::io::Error::other(format!("read gate disconnected: {disconnected}"))
                })?;
            }
            std::io::Read::read(&mut self.contents, buffer)
        }
    }

    let program = program(vec![global("main", int(0))], vec![], &[]);
    let bytecode = lower_to_bytecode(&program);
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let mut input = std::io::BufReader::new(GatedRead {
        release: Some(release_rx),
        contents: Cursor::new(b"discarded\n".to_vec()),
    });
    let mut output = Vec::new();
    let input = Mutex::new(&mut input as &mut (dyn BufRead + Send));
    let output = Mutex::new(&mut output as &mut (dyn Write + Send));

    thread::scope(|scope| {
        let io = IoSubsystem::start(scope, &input, &output, &[], Path::new("."))
            .expect("IO workers should start");
        let mut runtime = Runtime::new_with_io(bytecode, io);
        let blocked = runtime.allocate_process();
        assert_eq!(runtime.run_queue.pop_front(), Some(blocked));
        let request = runtime
            .io
            .issue(blocked, io_worker::IoRequest::ReadLine)
            .expect("readLine request should enter the worker pool");
        runtime.processes[blocked].status = ProcStatus::Blocked(BlockReason::Io(request));

        let killer = runtime.allocate_process();
        assert_eq!(runtime.run_queue.pop_front(), Some(killer));
        let mut killer_process = runtime.take_process(killer);
        {
            let mut interpreter = Interpreter::new(&mut runtime, &mut killer_process);
            interpreter
                .interrupt_thread(Value::ThreadId(blocked.into_raw().into_u32()))
                .expect("valid thread interruption should succeed");
        }
        runtime.restore_process(killer_process);
        assert!(!runtime.io.has_wakeable_request());
        assert_eq!(runtime.run_queue.pop_front(), Some(blocked));
        assert_eq!(
            perform_process_io(&mut runtime, blocked, Value::Unit),
            StepOutcome::Interrupted(Interruption::Killed)
        );

        release_tx
            .send(())
            .expect("blocked test reader should still be connected");
        let error = scheduler::next_runnable(&mut runtime)
            .expect_err("a cancelled completion must not wake its former process");
        assert_eq!(error.message, "all threads are blocked forever");
    });
}

#[test]
fn nested_interruption_masks_restore_one_depth_at_a_time() {
    let program = program(vec![global("main", int(0))], Vec::new(), &[]);
    let mut runtime = Runtime::new(lower_to_bytecode(&program));
    let process = runtime.allocate_process();
    runtime.run_queue.clear();
    let mut process = runtime.take_process(process);

    {
        let mut interpreter = Interpreter::new(&mut runtime, &mut process);
        assert_eq!(interpreter.enter_mask(), Ok(0));
        assert_eq!(interpreter.enter_mask(), Ok(1));
        assert_eq!(interpreter.mask_depth, 2);
        assert_eq!(interpreter.enter_restore_mask(), Ok(2));
        assert_eq!(interpreter.mask_depth, 1);
    }
}

#[test]
fn virtual_timers_wake_in_deadline_order_without_host_waiting() {
    let program = program(vec![global("main", int(0))], Vec::new(), &[]);
    let mut runtime = Runtime::new(lower_to_bytecode(&program));
    runtime.timers = TimerQueue::virtual_time();
    let late = runtime.allocate_process();
    let early = runtime.allocate_process();
    runtime.run_queue.clear();
    let late_deadline = runtime.timers.register(late, 30);
    let early_deadline = runtime.timers.register(early, 10);
    runtime.processes[late].status = ProcStatus::Blocked(BlockReason::Timer(late_deadline));
    runtime.processes[early].status = ProcStatus::Blocked(BlockReason::Timer(early_deadline));

    assert_eq!(scheduler::next_runnable(&mut runtime), Ok(early));
    assert_eq!(runtime.processes[early].timer_result, Some(early_deadline));
    assert_eq!(scheduler::next_runnable(&mut runtime), Ok(late));
    assert_eq!(runtime.processes[late].timer_result, Some(late_deadline));
}

#[test]
fn timer_cancellation_removes_the_exact_registration() {
    let program = program(vec![global("main", int(0))], Vec::new(), &[]);
    let mut runtime = Runtime::new(lower_to_bytecode(&program));
    runtime.timers = TimerQueue::virtual_time();
    let process = runtime.allocate_process();
    runtime.run_queue.clear();
    let deadline = runtime.timers.register(process, 10);
    runtime.processes[process].status = ProcStatus::Blocked(BlockReason::Timer(deadline));

    runtime.timers.cancel(process, deadline);

    assert!(!runtime.timers.advance_virtual_to_next());
}

#[test]
fn simultaneous_virtual_timers_wake_in_process_order() {
    let program = program(vec![global("main", int(0))], Vec::new(), &[]);
    let mut runtime = Runtime::new(lower_to_bytecode(&program));
    runtime.timers = TimerQueue::virtual_time();
    let first = runtime.allocate_process();
    let second = runtime.allocate_process();
    runtime.run_queue.clear();
    let first_deadline = runtime.timers.register(first, 10);
    let second_deadline = runtime.timers.register(second, 10);
    assert_eq!(first_deadline, second_deadline);
    runtime.processes[first].status = ProcStatus::Blocked(BlockReason::Timer(first_deadline));
    runtime.processes[second].status = ProcStatus::Blocked(BlockReason::Timer(second_deadline));

    assert_eq!(scheduler::next_runnable(&mut runtime), Ok(first));
    assert_eq!(scheduler::next_runnable(&mut runtime), Ok(second));
}

#[test]
fn scheduler_reports_deadlock_when_no_process_can_make_progress() {
    let program = program(vec![global("main", int(0))], vec![], &[]);
    let mut runtime = Runtime::new(lower_to_bytecode(&program));
    let process_id = runtime.allocate_process();
    assert_eq!(runtime.run_queue.pop_front(), Some(process_id));
    runtime.processes[process_id].status =
        ProcStatus::Blocked(BlockReason::Blackhole(GlobalIndex(0)));

    let error = scheduler::next_runnable(&mut runtime)
        .expect_err("an empty run queue without external work should deadlock");
    assert_eq!(error.message, "all threads are blocked forever");
}

#[test]
fn partial_application_applies_remaining_arguments() {
    let add3 = function(
        0,
        "add3",
        3,
        bin(BinOp::Plus, bin(BinOp::Plus, local(2), local(1)), local(0)),
    );

    let program = program(
        vec![global(
            "main",
            call(call_known(0, vec![int(10), int(20)]), vec![int(12)]),
        )],
        vec![add3],
        &[],
    );

    assert_eq!(run_main(&program), "42");
}

#[test]
fn gc_preserves_list_across_build_and_fold() {
    assert_eq!(run_main(&list_build_and_sum_program(40_000)), "800020000");
}

#[test]
fn generational_gc_runs_many_minors_and_a_major() {
    let bytecode = lower_to_bytecode(&list_build_and_sum_program(200));
    let entry = bytecode
        .global_index_by_name("main")
        .expect("generational GC fixture should contain main");
    let mut runtime = Runtime::new_with_collection_limits_for_test(bytecode, 8, 16);

    let (_, outcome) = evaluate_entry(&mut runtime, entry);

    assert_eq!(outcome, StepOutcome::Finished(Value::Int(20_100)));
    assert!(runtime.heap.minor_collections >= 20);
    assert!(runtime.heap.major_collections >= 1);
    assert!(runtime.heap.nursery.len() < runtime.heap.nursery_slot_count);
}

fn list_build_and_sum_program(count: i64) -> CCProgram {
    let build = function(
        0,
        "build",
        2,
        if_then_else(
            bin(BinOp::Equal, local(1), int(0)),
            local(0),
            call_known(
                0,
                vec![
                    bin(BinOp::Minus, local(1), int(1)),
                    pack(CONS_TAG, 2, vec![local(1), local(0)]),
                ],
            ),
        ),
    );

    let fold_sum = function(
        1,
        "fold_sum",
        2,
        case(
            local(1),
            vec![
                arm(LowerPattern::Con(NIL_TAG, vec![]), local(0)),
                arm(
                    LowerPattern::Con(CONS_TAG, vec![LowerPattern::Var, LowerPattern::Var]),
                    call_known(1, vec![local(0), bin(BinOp::Plus, local(2), local(1))]),
                ),
            ],
        ),
    );

    program(
        vec![global(
            "main",
            let_in(
                call_known(0, vec![int(count), pack(NIL_TAG, 0, Vec::new())]),
                call_known(1, vec![local(0), int(0)]),
            ),
        )],
        vec![build, fold_sum],
        &[("Nil", NIL_TAG, 0), ("Cons", CONS_TAG, 2)],
    )
}

#[test]
fn gc_reclaims_short_lived_objects() {
    let churn = function(
        0,
        "churn",
        1,
        if_then_else(
            bin(BinOp::Equal, local(0), int(0)),
            int(42),
            let_in(
                pack(CONS_TAG, 2, vec![local(0), pack(NIL_TAG, 0, Vec::new())]),
                call_known(0, vec![bin(BinOp::Minus, local(1), int(1))]),
            ),
        ),
    );

    let program = program(
        vec![global("main", call_known(0, vec![int(500_000)]))],
        vec![churn],
        &[("Nil", NIL_TAG, 0), ("Cons", CONS_TAG, 2)],
    );

    assert_eq!(run_main(&program), "42");
}

#[test]
fn bulk_builtin_allocation_keeps_roots_relocatable() {
    let input = "copying-gc".repeat(100);
    let round_trip = builtin(
        BUILTIN_CHARS_TO_STRING,
        vec![builtin(
            BUILTIN_STRING_TO_CHARS,
            vec![expr(CCExpr::StrConst(0))],
        )],
    );
    let mut program = program_with_ctors(
        vec![global("main", round_trip)],
        Vec::new(),
        &[
            ctor_spec(
                CtorId(300),
                CtorRuntimeRole::PreludeListNil,
                "prelude.List.Nil",
                "Nil",
                NIL_TAG,
                0,
            ),
            ctor_spec(
                CtorId(301),
                CtorRuntimeRole::PreludeListCons,
                "prelude.List.Cons",
                "Cons",
                CONS_TAG,
                2,
            ),
        ],
    );
    program.string_constants.push(input.clone());

    let bytecode = lower_to_bytecode(&program);
    let entry = bytecode
        .global_index_by_name("main")
        .expect("bulk builtin fixture should contain main");
    let mut runtime = Runtime::new_with_collection_limits_for_test(bytecode, 8, 16);
    let (_, outcome) = evaluate_entry(&mut runtime, entry);
    let StepOutcome::Finished(Value::Obj(id)) = outcome else {
        solveig_base::ice!("bulk builtin fixture did not return a heap string");
    };
    let HeapObject::Str { bytes } = runtime.heap.get(id) else {
        solveig_base::ice!("bulk builtin fixture returned a non-string heap object");
    };
    assert_eq!(runtime.heap.str_value(id, *bytes), input);
    assert!(runtime.heap.minor_collections > 100);
}

#[test]
fn global_thunks_resume_dependency_order() {
    let program = program(
        vec![
            global("main", bin(BinOp::Plus, global_ref(1), global_ref(2))),
            global("branch", bin(BinOp::Plus, global_ref(3), global_ref(2))),
            global("leaf", int(10)),
            global("root", bin(BinOp::Plus, global_ref(2), int(1))),
        ],
        Vec::new(),
        &[],
    );

    assert_eq!(run_main(&program), "31");
}

#[test]
fn blackhole_waiters_resume_with_the_published_value() {
    let program = program(
        vec![
            global("shared", int(42)),
            global("first_waiter", bin(BinOp::Plus, global_ref(0), int(1))),
            global("second_waiter", bin(BinOp::Plus, global_ref(0), int(2))),
        ],
        Vec::new(),
        &[],
    );
    let mut runtime = Runtime::new(lower_to_bytecode(&program));
    let owner_id = runtime.allocate_process();
    let first_waiter_id = runtime.allocate_process();
    let second_waiter_id = runtime.allocate_process();
    runtime.run_queue.clear();
    start_process_global(&mut runtime, owner_id, 0);
    start_process_global(&mut runtime, first_waiter_id, 1);
    start_process_global(&mut runtime, second_waiter_id, 2);

    assert_eq!(
        run_process(&mut runtime, first_waiter_id),
        StepOutcome::Blocked
    );
    assert_eq!(
        run_process(&mut runtime, second_waiter_id),
        StepOutcome::Blocked
    );
    assert_eq!(
        runtime.processes[first_waiter_id].status,
        ProcStatus::Blocked(BlockReason::Blackhole(GlobalIndex(0)))
    );
    assert_eq!(
        runtime.processes[second_waiter_id].status,
        ProcStatus::Blocked(BlockReason::Blackhole(GlobalIndex(0)))
    );

    let owner_outcome = run_process(&mut runtime, owner_id);

    assert_eq!(owner_outcome, StepOutcome::Finished(Value::Int(42)));
    assert_eq!(
        runtime.processes[first_waiter_id].status,
        ProcStatus::Runnable
    );
    assert_eq!(
        runtime.processes[second_waiter_id].status,
        ProcStatus::Runnable
    );
    assert_eq!(
        runtime.processes[first_waiter_id].stack.last(),
        Some(&Value::Int(42))
    );
    assert_eq!(
        runtime.processes[second_waiter_id].stack.last(),
        Some(&Value::Int(42))
    );
    assert_eq!(runtime.run_queue.pop_front(), Some(first_waiter_id));
    assert_eq!(runtime.run_queue.pop_front(), Some(second_waiter_id));

    assert_eq!(
        run_process(&mut runtime, first_waiter_id),
        StepOutcome::Finished(Value::Int(43))
    );
    assert_eq!(
        run_process(&mut runtime, second_waiter_id),
        StepOutcome::Finished(Value::Int(44))
    );
    assert!(matches!(
        runtime.globals.get(1),
        Some(GlobalSlot::Evaluated(Value::Int(43)))
    ));
    assert!(matches!(
        runtime.globals.get(2),
        Some(GlobalSlot::Evaluated(Value::Int(44)))
    ));
}

#[test]
fn interrupted_global_owner_restores_thunk_and_retries_waiters() {
    let program = program(
        vec![global("shared", int(42)), global("waiter", global_ref(0))],
        Vec::new(),
        &[],
    );
    let mut runtime = Runtime::new(lower_to_bytecode(&program));
    let owner = runtime.allocate_process();
    let waiter = runtime.allocate_process();
    runtime.run_queue.clear();
    start_process_global(&mut runtime, owner, 0);
    start_process_global(&mut runtime, waiter, 1);
    assert_eq!(run_process(&mut runtime, waiter), StepOutcome::Blocked);
    let retry_pc = runtime.processes[waiter].pc;

    let mut owner_process = runtime.take_process(owner);
    let outcome = {
        let mut interpreter = Interpreter::new(&mut runtime, &mut owner_process);
        let program = interpreter.program;
        interpreter.resume_io_interruption(program, Interruption::Killed)
    };
    runtime.restore_process(owner_process);

    assert_eq!(outcome, Err(StepOutcome::Interrupted(Interruption::Killed)));
    assert!(matches!(
        runtime.globals.first(),
        Some(GlobalSlot::Thunk { .. })
    ));
    assert_eq!(runtime.processes[waiter].status, ProcStatus::Runnable);
    assert_eq!(runtime.processes[waiter].pc + 1, retry_pc);
    assert_eq!(runtime.run_queue.pop_front(), Some(waiter));
    assert_eq!(
        run_process(&mut runtime, waiter),
        StepOutcome::Finished(Value::Int(42))
    );
}

#[test]
fn blackhole_owner_failure_reaches_waiters_and_their_owned_globals() {
    let program = program(
        vec![
            global("failing", case(int(0), Vec::new())),
            global("waiter", global_ref(0)),
        ],
        Vec::new(),
        &[],
    );
    let mut runtime = Runtime::new(lower_to_bytecode(&program));
    let owner_id = runtime.allocate_process();
    let waiter_id = runtime.allocate_process();
    runtime.run_queue.clear();
    start_process_global(&mut runtime, owner_id, 0);
    start_process_global(&mut runtime, waiter_id, 1);

    assert_eq!(run_process(&mut runtime, waiter_id), StepOutcome::Blocked);

    let owner_outcome = run_process(&mut runtime, owner_id);
    let StepOutcome::Failed(error) = owner_outcome else {
        solveig_base::ice!("blackhole owner fixture did not fail");
    };

    assert_eq!(
        runtime.processes[waiter_id].status,
        ProcStatus::Failed(error.clone())
    );
    assert!(matches!(
        runtime.globals.first(),
        Some(GlobalSlot::Failed(global_error)) if global_error == &error
    ));
    assert!(matches!(
        runtime.globals.get(1),
        Some(GlobalSlot::Failed(global_error)) if global_error == &error
    ));
    assert!(runtime.run_queue.is_empty());
}

#[test]
fn global_owner_reentry_is_a_cycle_not_a_waiter() {
    let program = program(vec![global("loop", global_ref(0))], Vec::new(), &[]);
    let mut runtime = Runtime::new(lower_to_bytecode(&program));

    let (id, outcome) = evaluate_entry(&mut runtime, 0);

    let StepOutcome::Failed(error) = outcome else {
        solveig_base::ice!("cyclic global fixture did not fail");
    };
    assert_eq!(error, RuntimeError::cyclic_global("loop"));
    assert_eq!(
        runtime.processes[id].status,
        ProcStatus::Failed(error.clone())
    );
    assert!(matches!(
        runtime.globals.first(),
        Some(GlobalSlot::Failed(global_error)) if global_error == &error
    ));
}

fn start_process_global(runtime: &mut Runtime, process_id: ProcessId, global_index: usize) {
    let mut process = runtime.take_process(process_id);
    process.status = ProcStatus::Running;
    let started = {
        let mut interpreter = Interpreter::new(runtime, &mut process);
        let program = interpreter.program;
        interpreter.start_global_eval(
            program,
            global_index,
            Continuation::PublishGlobalThenHalt { global_index },
        )
    };
    assert!(matches!(started, Ok(GlobalEval::Entered)));
    runtime.restore_process(process);
}

fn runtime_with_reduction_budget(program: BytecodeProgram, reduction_budget: u32) -> Runtime {
    Runtime::new_with_heap_and_reduction_budget(
        program,
        Heap::default(),
        reduction_budget,
        IoSubsystem::inactive(),
        TimerQueue::real_time(),
    )
}

fn run_process(runtime: &mut Runtime, process_id: ProcessId) -> StepOutcome {
    let mut process = runtime.take_process(process_id);
    let outcome = run(runtime, &mut process);
    runtime.restore_process(process);
    outcome
}

#[test]
fn trap_returns_runtime_error() {
    let program = program(
        vec![global("main", case(int(0), Vec::new()))],
        Vec::new(),
        &[],
    );

    let err = match Machine::exec(&program, "main") {
        Ok(_) => solveig_base::ice!("trap test unexpectedly evaluated successfully"),
        Err(err) => err,
    };

    assert_eq!(err.code, 1);
    assert_eq!(err.message, "non-exhaustive case");
}

#[test]
fn bool_cache_uses_ctor_ids_not_names() {
    let program = program_with_bool_names(
        vec![global(
            "main",
            if_then_else(bin(BinOp::Less, int(1), int(2)), int(42), int(0)),
        )],
        Vec::new(),
        &[],
        "DefinitelyTrue",
        "DefinitelyFalse",
    );

    assert_eq!(run_main(&program), "42");
}

#[test]
fn display_fallback_prints_structural_constructors_for_prelude_roles() {
    let list = pack(
        PRELUDE_CONS_TAG,
        2,
        vec![
            int(1),
            pack(
                PRELUDE_CONS_TAG,
                2,
                vec![int(2), pack(PRELUDE_NIL_TAG, 0, Vec::new())],
            ),
        ],
    );
    let pair = pack(PRELUDE_PAIR_TAG, 2, vec![int(1), int(2)]);

    let program = program_with_ctors(
        vec![global("listMain", list), global("pairMain", pair)],
        Vec::new(),
        &[
            ctor_spec(
                CtorId(100),
                CtorRuntimeRole::PreludeListNil,
                "renamed.list.Empty",
                "Empty",
                PRELUDE_NIL_TAG,
                0,
            ),
            ctor_spec(
                CtorId(101),
                CtorRuntimeRole::PreludeListCons,
                "renamed.list.Link",
                "Link",
                PRELUDE_CONS_TAG,
                2,
            ),
            ctor_spec(
                CtorId(102),
                CtorRuntimeRole::PreludePair,
                "renamed.pair.Tuple",
                "Tuple",
                PRELUDE_PAIR_TAG,
                2,
            ),
        ],
    );

    assert_eq!(run_entry(&program, "listMain"), "Link 1 (Link 2 Empty)");
    assert_eq!(run_entry(&program, "pairMain"), "Tuple 1 2");
}

#[test]
fn display_does_not_tuple_user_pair_by_name() {
    let program = program_with_ctors(
        vec![global("main", pack(USER_PAIR_TAG, 2, vec![int(1), int(2)]))],
        Vec::new(),
        &[ctor_spec(
            CtorId(200),
            CtorRuntimeRole::Ordinary,
            "main.MkPair",
            "MkPair",
            USER_PAIR_TAG,
            2,
        )],
    );

    assert_eq!(run_main(&program), "MkPair 1 2");
}

#[test]
fn runtime_step_finishes_and_records_process_status() {
    let program = program(vec![global("main", int(42))], Vec::new(), &[]);
    let bytecode = lower_to_bytecode(&program);
    let entry = bytecode
        .global_index_by_name("main")
        .expect("runtime step fixture should contain main");
    let mut runtime = Runtime::new(bytecode);

    let (id, outcome) = evaluate_entry(&mut runtime, entry);

    assert_eq!(outcome, StepOutcome::Finished(Value::Int(42)));
    assert_eq!(runtime.processes[id].id, id);
    assert_eq!(
        runtime.processes[id].status,
        ProcStatus::Finished(Value::Int(42))
    );
    assert!(runtime.run_queue.is_empty());
}

#[test]
fn gc_traces_roots_in_every_inactive_process() {
    let program = program(vec![global("main", int(0))], Vec::new(), &[]);
    let mut runtime = Runtime::new(lower_to_bytecode(&program));
    let rooted = runtime.alloc_str_for_test("rooted");
    let process = runtime.allocate_process();
    runtime.processes[process].status = ProcStatus::Finished(rooted);
    let unrooted = runtime.alloc_str_for_test("unrooted");
    let Value::Obj(unrooted_id) = unrooted else {
        solveig_base::ice!("heap string allocation did not return an object id");
    };

    runtime.collect_all_process_roots_for_test();

    let ProcStatus::Finished(Value::Obj(promoted_id)) = runtime.processes[process].status else {
        solveig_base::ice!("finished process root was not rewritten to an object id");
    };
    assert!(runtime.heap.is_live(promoted_id));
    assert!(!runtime.heap.is_nursery_id(promoted_id));
    assert!(!runtime.heap.is_live(unrooted_id));
    assert!(runtime.heap.nursery.is_empty());
}

#[test]
fn channel_write_barrier_preserves_fifo_values_across_minor_collections() {
    let program = program(vec![global("main", int(0))], Vec::new(), &[]);
    let mut runtime =
        Runtime::new_with_collection_limits_for_test(lower_to_bytecode(&program), 8, 64);
    let root_process = runtime.allocate_process();
    runtime.run_queue.clear();
    let channel = runtime.heap.alloc(HeapObject::Chan(Chan {
        queue: PayloadStore::empty(),
        capacity: None,
        closed: false,
    }));
    runtime.processes[root_process]
        .temporary_roots
        .push(channel);
    runtime.collect_all_process_roots_for_test();

    for index in 0..50 {
        let value = runtime.alloc_str_for_test(&format!("message-{index}"));
        let Value::Obj(channel) = runtime.processes[root_process].temporary_roots[0] else {
            solveig_base::ice!("channel root changed value kind during collection")
        };
        runtime.heap.push_channel(ChanId(channel), value);
    }

    runtime.collect_all_process_roots_for_test();
    let Value::Obj(channel) = runtime.processes[root_process].temporary_roots[0] else {
        solveig_base::ice!("channel root changed value kind during collection")
    };
    let channel = ChanId(channel);

    for index in 0..50 {
        let Some(Value::Obj(value)) = runtime.heap.pop_channel(channel) else {
            solveig_base::ice!("channel lost a queued string during collection")
        };
        let HeapObject::Str { bytes } = runtime.heap.get(value) else {
            solveig_base::ice!("channel queue returned a non-string value")
        };
        assert_eq!(
            runtime.heap.str_value(value, *bytes),
            format!("message-{index}")
        );
    }
    assert!(runtime.heap.pop_channel(channel).is_none());
    assert!(runtime.heap.minor_collections >= 2);
}

#[test]
fn gc_traces_a_parked_channel_sender_value() {
    let program = program(vec![global("main", int(0))], Vec::new(), &[]);
    let mut runtime =
        Runtime::new_with_collection_limits_for_test(lower_to_bytecode(&program), 8, 64);
    let root_process = runtime.allocate_process();
    runtime.run_queue.clear();
    let channel = runtime.heap.alloc(HeapObject::Chan(Chan {
        queue: PayloadStore::empty(),
        capacity: Some(1),
        closed: false,
    }));
    runtime.processes[root_process]
        .temporary_roots
        .push(channel);
    runtime.collect_all_process_roots_for_test();
    let Value::Obj(channel) = runtime.processes[root_process].temporary_roots[0] else {
        solveig_base::ice!("channel root changed value kind during collection")
    };
    let channel = ChanId(channel);
    runtime.heap.push_channel(channel, Value::Int(1));

    let pending = runtime.alloc_str_for_test("parked");
    let sender = runtime.allocate_process();
    runtime.run_queue.clear();
    let mut sender_process = runtime.take_process(sender);
    {
        let mut interpreter = Interpreter::new(&mut runtime, &mut sender_process);
        assert!(matches!(
            interpreter.send_channel(channel, pending),
            super::channel::ChannelOutcome::Blocked
        ));
    }
    runtime.restore_process(sender_process);

    runtime.collect_all_process_roots_for_test();

    let receiver = runtime.allocate_process();
    runtime.run_queue.clear();
    let mut receiver_process = runtime.take_process(receiver);
    {
        let mut interpreter = Interpreter::new(&mut runtime, &mut receiver_process);
        assert!(matches!(
            interpreter.recv_channel(channel, false),
            super::channel::ChannelOutcome::Ready(Value::Int(1))
        ));
    }
    runtime.restore_process(receiver_process);

    let Some(Value::Obj(value)) = runtime.heap.pop_channel(channel) else {
        solveig_base::ice!("parked sender value was not admitted to the channel")
    };
    let HeapObject::Str { bytes } = runtime.heap.get(value) else {
        solveig_base::ice!("parked sender resumed with a non-string value")
    };
    assert_eq!(runtime.heap.str_value(value, *bytes), "parked");
    assert_eq!(runtime.processes[sender].status, ProcStatus::Runnable);
}

#[test]
fn scheduler_completes_ten_thousand_threads() {
    const THREADS: usize = 10_000;

    let program = program(vec![global("main", int(0))], Vec::new(), &[]);
    let mut runtime = Runtime::new(lower_to_bytecode(&program));
    let main = runtime.allocate_process();
    runtime.run_queue.clear();
    let action = runtime
        .heap
        .alloc(HeapObject::IoAction(IoAction::Pure(Value::Unit)));

    let mut main_process = runtime.take_process(main);
    {
        let mut interpreter = Interpreter::new(&mut runtime, &mut main_process);
        for expected_id in 1..=THREADS {
            assert_eq!(
                interpreter.spawn_io_process(action),
                Value::ThreadId(
                    u32::try_from(expected_id).expect("thread test identity should fit u32")
                )
            );
        }
    }
    runtime.restore_process(main_process);

    let mut stderr = Vec::new();

    assert_eq!(
        scheduler::schedule_io(&mut runtime, main, action, &mut stderr),
        Ok(Value::Unit)
    );
    assert_eq!(
        runtime
            .processes
            .iter()
            .filter(|(id, process)| {
                *id != main && process.status == ProcStatus::Finished(Value::Unit)
            })
            .count(),
        THREADS
    );
}

fn run_main(program: &CCProgram) -> String {
    run_entry(program, "main")
}

fn run_entry(program: &CCProgram, entry_name: &str) -> String {
    Machine::exec(program, entry_name)
        .expect("VM test program should evaluate successfully")
        .display()
        .to_string()
}

fn program(
    globals: Vec<CCGlobalDef>,
    functions: Vec<CCFunction>,
    extra_constructors: &[(&str, u16, u16)],
) -> CCProgram {
    program_with_bool_names(globals, functions, extra_constructors, "True", "False")
}

fn program_with_bool_names(
    globals: Vec<CCGlobalDef>,
    functions: Vec<CCFunction>,
    extra_constructors: &[(&str, u16, u16)],
    true_name: &'static str,
    false_name: &'static str,
) -> CCProgram {
    let extra_constructors = extra_constructors
        .iter()
        .enumerate()
        .map(|(index, (name, tag, arity))| {
            let constructor_offset =
                u32::try_from(index).expect("VM test constructor count should fit in u32");
            ctor_spec(
                CtorId(1_000 + constructor_offset),
                CtorRuntimeRole::Ordinary,
                name,
                name,
                *tag,
                *arity,
            )
        })
        .collect::<Vec<_>>();

    program_with_bool_ctor_specs(
        globals,
        functions,
        &extra_constructors,
        true_name,
        false_name,
    )
}

fn program_with_ctors(
    globals: Vec<CCGlobalDef>,
    functions: Vec<CCFunction>,
    extra_constructors: &[TestCtorSpec],
) -> CCProgram {
    program_with_bool_ctor_specs(globals, functions, extra_constructors, "True", "False")
}

fn program_with_bool_ctor_specs(
    globals: Vec<CCGlobalDef>,
    functions: Vec<CCFunction>,
    extra_constructors: &[TestCtorSpec],
    true_name: &'static str,
    false_name: &'static str,
) -> CCProgram {
    let true_spec = ctor_spec(
        PRIM_TYS.true_ctor,
        CtorRuntimeRole::Ordinary,
        true_name,
        true_name,
        TRUE_TAG,
        0,
    );
    let false_spec = ctor_spec(
        PRIM_TYS.false_ctor,
        CtorRuntimeRole::Ordinary,
        false_name,
        false_name,
        FALSE_TAG,
        0,
    );
    let mut constructor_arities = vec![
        ctor_runtime_info(&true_spec),
        ctor_runtime_info(&false_spec),
    ];

    constructor_arities.extend(extra_constructors.iter().map(ctor_runtime_info));

    CCProgram {
        globals,
        functions,
        constructor_arities,
        string_constants: Vec::new(),
        string_literals: Vec::new(),
    }
}

struct TestCtorSpec {
    ctor: CtorId,
    role: CtorRuntimeRole,
    name: String,
    short_name: String,
    tag: u16,
    arity: u16,
}

fn ctor_spec(
    ctor: CtorId,
    role: CtorRuntimeRole,
    name: &str,
    short_name: &str,
    tag: u16,
    arity: u16,
) -> TestCtorSpec {
    TestCtorSpec {
        ctor,
        role,
        name: name.to_string(),
        short_name: short_name.to_string(),
        tag,
        arity,
    }
}

fn ctor_runtime_info(spec: &TestCtorSpec) -> CtorRuntimeInfo {
    CtorRuntimeInfo {
        ctor: spec.ctor,
        role: spec.role,
        name: spec.name.clone(),
        short_name: spec.short_name.clone(),
        tag: spec.tag,
        arity: spec.arity,
    }
}

fn function(id: usize, name: &str, arity: usize, body: Rc<CCExpr>) -> CCFunction {
    CCFunction {
        id,
        name: name.to_string(),
        arity,
        capture_count: 0,
        body,
    }
}

fn global(name: &str, body: Rc<CCExpr>) -> CCGlobalDef {
    CCGlobalDef {
        name: name.to_string(),
        body,
    }
}

fn expr(expr: CCExpr) -> Rc<CCExpr> {
    Rc::new(expr)
}

fn located(source: FileSpan, inner: Rc<CCExpr>) -> Rc<CCExpr> {
    expr(CCExpr::Located(source, inner))
}

fn int(n: i64) -> Rc<CCExpr> {
    expr(CCExpr::Int(n))
}

fn local(index: usize) -> Rc<CCExpr> {
    expr(CCExpr::Local(index))
}

fn global_ref(index: usize) -> Rc<CCExpr> {
    expr(CCExpr::Global(index))
}

fn bin(op: BinOp, lhs: Rc<CCExpr>, rhs: Rc<CCExpr>) -> Rc<CCExpr> {
    expr(CCExpr::BinaryOp(op, lhs, rhs))
}

fn call(callee: Rc<CCExpr>, args: Vec<Rc<CCExpr>>) -> Rc<CCExpr> {
    expr(CCExpr::Call { callee, args })
}

fn call_known(func: usize, args: Vec<Rc<CCExpr>>) -> Rc<CCExpr> {
    expr(CCExpr::CallKnown { func, args })
}

fn function_ref(func: usize, arity: usize) -> Rc<CCExpr> {
    expr(CCExpr::FunctionRef { func, arity })
}

fn builtin(id: BuiltinId, args: Vec<Rc<CCExpr>>) -> Rc<CCExpr> {
    expr(CCExpr::CallBuiltin { id, args })
}

fn let_in(binder: Rc<CCExpr>, body: Rc<CCExpr>) -> Rc<CCExpr> {
    expr(CCExpr::Let(binder, body))
}

fn pack(tag: u16, arity: u16, fields: Vec<Rc<CCExpr>>) -> Rc<CCExpr> {
    expr(CCExpr::Pack { tag, arity, fields })
}

fn case(scrutinee: Rc<CCExpr>, arms: Vec<LowerCaseArm<CCExpr>>) -> Rc<CCExpr> {
    expr(CCExpr::Case { scrutinee, arms })
}

fn arm(pattern: LowerPattern, body: Rc<CCExpr>) -> LowerCaseArm<CCExpr> {
    LowerCaseArm { pattern, body }
}

fn if_then_else(cond: Rc<CCExpr>, then_br: Rc<CCExpr>, else_br: Rc<CCExpr>) -> Rc<CCExpr> {
    expr(CCExpr::IfThenElse {
        cond,
        then_br,
        else_br,
    })
}
