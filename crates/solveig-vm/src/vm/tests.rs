//! VM execution regression tests.
//! These cases cover bytecode dispatch, closures, thunks, constructors, and IO.

use super::*;
use crate::bytecode::Instr;
use solveig_ast::ast::{
    BUILTIN_CHARS_TO_STRING, BUILTIN_STRING_TO_CHARS, BinOp, BuiltinId, CtorId, CtorRuntimeRole,
    PRIM_TYS,
};
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
