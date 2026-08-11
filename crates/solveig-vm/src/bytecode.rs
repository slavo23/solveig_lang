//! Bytecode instruction definitions and closure IR compilation.
//! The VM executes chunks produced here from lowered Solveig programs.

use solveig_ast::ast::{BinOp, BuiltinId, CtorRuntimeInfo, Literal, StrLitId, UnOp};
use solveig_base::{FileSpan, FxHashMap};
use std::collections::BTreeSet;
use std::path::Path;
use std::rc::Rc;

use solveig_lower::LowerPattern;
use solveig_lower::closure_ir::{CCExpr, CCFunction, CCGlobalDef, CCProgram};

const NON_EXHAUSTIVE_CASE_TRAP: u16 = 1;

#[derive(Debug, Clone, Copy)]
pub enum Instr {
    PushUnit,
    PushInt(i64),
    PushFloat(f64),
    PushChar(char),
    PushStr(u32),
    LoadLocal(u16),
    LoadGlobal(u32),
    PushCtorFn {
        tag: u16,
        arity: u16,
    },
    MakeClosure {
        chunk: u32,
        capture_count: u16,
    },
    PushFunctionRef {
        chunk: u32,
        arity: u16,
    },
    Unary(UnOp),
    Binary(BinOp),
    CallBuiltin {
        id: BuiltinId,
        arg_count: u16,
    },
    Pack {
        tag: u16,
        arity: u16,
        field_count: u16,
    },
    StoreLocal(u16),
    CallN {
        arg_count: u16,
    },
    CallKnown {
        chunk: u32,
        arg_count: u16,
    },
    TailCallN {
        arg_count: u16,
    },
    TailCallKnown {
        chunk: u32,
        arg_count: u16,
    },
    Return,
    Jump(u32),
    JumpIfFalse(u32),
    Match(u16),
    Trap(u16),
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: LowerPattern,
    pub bind_slots: Box<[u16]>,
    pub target: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkKind {
    GlobalThunk,
    Function { capture_count: u16, arity: u16 },
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub id: usize,
    pub name: String,
    pub kind: ChunkKind,
    pub slot_count: u16,
    pub code: Vec<Instr>,
    /// Source location for each bytecode offset; synthetic instructions use `None`.
    pub source_spans: Vec<Option<FileSpan>>,
    pub match_tables: Vec<MatchTable>,
}

impl Chunk {
    /// Returns the source location associated with a bytecode offset.
    pub fn source_span(&self, pc: usize) -> Option<FileSpan> {
        self.source_spans.get(pc).copied().flatten()
    }
}

#[derive(Debug, Clone)]
pub struct MatchTable {
    pub arms: Box<[MatchArm]>,
    pub else_target: u32,
}

#[derive(Debug, Clone)]
pub struct CompiledGlobal {
    pub name: String,
    pub chunk: usize,
}

#[derive(Debug, Clone)]
pub struct BytecodeProgram {
    pub chunks: Vec<Chunk>,
    pub globals: Vec<CompiledGlobal>,
    pub constructor_arities: Vec<CtorRuntimeInfo>,
    pub string_constants: Vec<String>,
    pub string_literals: Vec<String>,
}

impl BytecodeProgram {
    pub fn constructor_name(&self, tag: u16) -> Option<String> {
        self.constructor_arities
            .iter()
            .find(|info| info.tag == tag)
            .map(|info| info.name.clone())
    }

    pub fn constructor_arity(&self, tag: u16) -> Option<u16> {
        self.constructor_arities
            .iter()
            .find(|info| info.tag == tag)
            .map(|info| info.arity)
    }

    pub fn chunk(&self, id: usize) -> &Chunk {
        self.chunks
            .get(id)
            .expect("ICE: bytecode chunk id out of bounds")
    }

    pub fn global_index_by_name(&self, name: &str) -> Option<usize> {
        self.globals.iter().position(|g| g.name == name)
    }

    pub fn string_literal_value(&self, id: StrLitId) -> Option<&str> {
        self.string_literals.get(id.0 as usize).map(String::as_str)
    }
}

include!("bytecode/lower.rs");
include!("bytecode/dump.rs");
