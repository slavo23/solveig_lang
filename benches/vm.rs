//! Measures the bytecode interpreter with compilation removed from the sample.
//! Programs are small hand-authored bytecode kernels with deterministic results.

use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use solveig_ast::ast::{BinOp, CtorRuntimeInfo, CtorRuntimeRole, PRIM_TYS};
use solveig_vm::bytecode::{BytecodeProgram, Chunk, ChunkKind, CompiledGlobal, Instr};
use solveig_vm::vm::Machine;

const ARITHMETIC_ITERATIONS: i64 = 250_000;
const ALLOCATION_ITERATIONS: i64 = 100_000;
const GLOBAL_LOAD_ITERATIONS: i64 = 250_000;

fn benchmark_vm(c: &mut Criterion) {
    let arithmetic = arithmetic_program();
    validate(&arithmetic, "31250125000");
    c.bench_function("vm/arithmetic_dispatch", |b| {
        b.iter(|| {
            let _result = black_box(Machine::exec_compiled(&arithmetic, "main"));
        });
    });

    let allocation = allocation_program();
    validate(&allocation, "0");
    c.bench_function("vm/allocation_gc", |b| {
        b.iter(|| {
            let _result = black_box(Machine::exec_compiled(&allocation, "main"));
        });
    });

    let global_load = global_load_program();
    validate(&global_load, "1750000");
    c.bench_function("vm/global_load", |b| {
        b.iter(|| {
            let _result = black_box(Machine::exec_compiled(&global_load, "main"));
        });
    });
}

fn arithmetic_program() -> BytecodeProgram {
    program(
        2,
        vec![
            Instr::PushInt(ARITHMETIC_ITERATIONS),
            Instr::StoreLocal(0),
            Instr::PushInt(0),
            Instr::StoreLocal(1),
            Instr::BinaryLocalInt {
                op: BinOp::Equal,
                local: 0,
                rhs: 0,
            },
            Instr::JumpIfFalse(8),
            Instr::LoadLocal(1),
            Instr::Return,
            Instr::BinaryLocals {
                op: BinOp::Plus,
                lhs: 1,
                rhs: 0,
            },
            Instr::StoreLocal(1),
            Instr::BinaryLocalInt {
                op: BinOp::Minus,
                local: 0,
                rhs: 1,
            },
            Instr::StoreLocal(0),
            Instr::Jump(4),
        ],
    )
}

fn allocation_program() -> BytecodeProgram {
    program(
        2,
        vec![
            Instr::PushInt(ALLOCATION_ITERATIONS),
            Instr::StoreLocal(0),
            Instr::PushUnit,
            Instr::StoreLocal(1),
            Instr::LoadLocal(0),
            Instr::PushInt(0),
            Instr::Binary(BinOp::Equal),
            Instr::JumpIfFalse(10),
            Instr::LoadLocal(0),
            Instr::Return,
            Instr::LoadLocal(1),
            Instr::LoadLocal(0),
            Instr::Pack {
                tag: 2,
                arity: 2,
                field_count: 2,
            },
            Instr::StoreLocal(1),
            Instr::LoadLocal(0),
            Instr::PushInt(1),
            Instr::Binary(BinOp::Minus),
            Instr::StoreLocal(0),
            Instr::Jump(4),
        ],
    )
}

fn global_load_program() -> BytecodeProgram {
    let main_code = vec![
        Instr::PushInt(GLOBAL_LOAD_ITERATIONS),
        Instr::StoreLocal(0),
        Instr::PushInt(0),
        Instr::StoreLocal(1),
        Instr::LoadLocal(0),
        Instr::PushInt(0),
        Instr::Binary(BinOp::Equal),
        Instr::JumpIfFalse(10),
        Instr::LoadLocal(1),
        Instr::Return,
        Instr::LoadLocal(1),
        Instr::LoadGlobal(1),
        Instr::Binary(BinOp::Plus),
        Instr::StoreLocal(1),
        Instr::LoadLocal(0),
        Instr::PushInt(1),
        Instr::Binary(BinOp::Minus),
        Instr::StoreLocal(0),
        Instr::Jump(4),
    ];
    let shared_code = vec![Instr::PushInt(7), Instr::Return];

    BytecodeProgram {
        chunks: vec![
            chunk(0, "main", 2, main_code),
            chunk(1, "shared", 0, shared_code),
        ],
        globals: vec![
            CompiledGlobal {
                name: "main".to_string(),
                chunk: 0,
            },
            CompiledGlobal {
                name: "shared".to_string(),
                chunk: 1,
            },
        ],
        constructor_arities: bool_constructors(),
        string_constants: Vec::new(),
        string_literals: Vec::new(),
    }
}

fn validate(program: &BytecodeProgram, expected: &str) {
    let output = Machine::exec_compiled(program, "main")
        .map(|result| result.display().to_string())
        .map_err(|error| error.to_string());

    assert_eq!(output.as_deref(), Ok(expected));
}

fn program(slot_count: u16, code: Vec<Instr>) -> BytecodeProgram {
    BytecodeProgram {
        chunks: vec![chunk(0, "main", slot_count, code)],
        globals: vec![CompiledGlobal {
            name: "main".to_string(),
            chunk: 0,
        }],
        constructor_arities: bool_constructors(),
        string_constants: Vec::new(),
        string_literals: Vec::new(),
    }
}

fn chunk(id: usize, name: &str, slot_count: u16, code: Vec<Instr>) -> Chunk {
    let source_spans = vec![None; code.len()];
    Chunk {
        id,
        name: name.to_string(),
        kind: ChunkKind::GlobalThunk,
        slot_count,
        code,
        source_spans,
        match_tables: Vec::new(),
    }
}

fn bool_constructors() -> Vec<CtorRuntimeInfo> {
    vec![
        CtorRuntimeInfo {
            ctor: PRIM_TYS.true_ctor,
            role: CtorRuntimeRole::Ordinary,
            name: "True".to_string(),
            short_name: "True".to_string(),
            tag: 0,
            arity: 0,
        },
        CtorRuntimeInfo {
            ctor: PRIM_TYS.false_ctor,
            role: CtorRuntimeRole::Ordinary,
            name: "False".to_string(),
            short_name: "False".to_string(),
            tag: 1,
            arity: 0,
        },
    ]
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .sample_size(20);
    targets = benchmark_vm
}
criterion_main!(benches);
