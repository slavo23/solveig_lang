use solveig_ast::ast::{BinOp, CtorRuntimeInfo, Literal, UnOp, ctor_display_name};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::{collections::HashMap, path::Path};

use solveig_lower::closure_ir::{CCExpr, CCFunction, CCGlobalDef, CCPattern, CCProgram};

const NON_EXHAUSTIVE_CASE_TRAP: u16 = 1;

#[derive(Debug, Clone, Copy)]
pub enum Instr {
    PushUnit,
    PushInt(i64),
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
    pub pattern: BytePattern,
    pub bind_slots: Box<[u16]>,
    pub target: u32,
}

#[derive(Debug, Clone)]
pub enum BytePattern {
    Var,
    Wild,
    Con(u16, Box<[BytePattern]>),
    Lit(Literal),
}

impl BytePattern {
    pub fn binder_count(&self) -> usize {
        match self {
            BytePattern::Var => 1,
            BytePattern::Wild => 0,
            BytePattern::Lit(_) => 0,
            BytePattern::Con(_, sub_patterns) => {
                sub_patterns.iter().map(|p| p.binder_count()).sum()
            }
        }
    }
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
    pub match_tables: Vec<MatchTable>,
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
}

impl BytecodeProgram {
    pub fn constructor_name(&self, tag: u16) -> Option<String> {
        self.constructor_arities
            .iter()
            .find(|info| info.tag == tag)
            .map(|info| ctor_display_name(info.ctor))
    }

    pub fn constructor_tag(&self, name: &str) -> Option<u16> {
        self.constructor_arities
            .iter()
            .find(|info| ctor_display_name(info.ctor) == name)
            .map(|info| info.tag)
    }

    pub fn constructor_arity(&self, tag: u16) -> Option<u16> {
        self.constructor_arities
            .iter()
            .find(|info| info.tag == tag)
            .map(|info| info.arity)
    }

    pub fn chunk(&self, id: usize) -> &Chunk {
        &self.chunks[id]
    }

    pub fn global_index_by_name(&self, name: &str) -> Option<usize> {
        self.globals.iter().position(|g| g.name == name)
    }

    pub fn dump_to_string(&self) -> String {
        let mut out = String::new();

        out.push_str("=== Globals ===\n");
        for (idx, global) in self.globals.iter().enumerate() {
            out.push_str(&format!(
                "[{idx}] {} -> chunk {}\n",
                global.name, global.chunk
            ));
        }

        out.push_str("\n=== Constructors ===\n");
        for info in &self.constructor_arities {
            out.push_str(&format!(
                "tag {:>3}  arity {:>2}  {}\n",
                info.tag,
                info.arity,
                ctor_display_name(info.ctor)
            ));
        }

        out.push_str("\n=== Chunks ===\n");
        for chunk in &self.chunks {
            out.push_str(&format!("\n--- chunk {}: {} ---\n", chunk.id, chunk.name));
            out.push_str(&format!(
                "kind: {}\nslot_count: {}\n",
                format_chunk_kind(chunk.kind),
                chunk.slot_count
            ));

            let labels = collect_chunk_labels(chunk);

            for (pc, instr) in chunk.code.iter().enumerate() {
                let pc_u32 = pc as u32;

                if labels.contains(&pc_u32) {
                    out.push_str(&format_label_line(pc_u32));
                }

                match instr {
                    Instr::Match(table_ix) => {
                        let table_ix = *table_ix as usize;
                        let table = chunk.match_tables.get(table_ix).unwrap_or_else(|| {
                            panic!(
                                "chunk '{}' references missing match table {}",
                                chunk.name, table_ix
                            )
                        });

                        out.push_str(&format_code_line(
                            pc,
                            format!(
                                "Match table={} else_label={}",
                                table_ix,
                                format_target(table.else_target)
                            ),
                        ));

                        for (arm_ix, arm) in table.arms.iter().enumerate() {
                            out.push_str(&format_detail_line(format!(
                                "arm[{arm_ix}] target_label={} binds={:?} pattern={}",
                                format_target(arm.target),
                                arm.bind_slots,
                                format_pattern(&arm.pattern),
                            )));
                        }
                    }

                    _ => {
                        out.push_str(&format_code_line(pc, format_instr(instr)));
                    }
                }
            }
        }

        out
    }

    pub fn dump_to_file<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        std::fs::write(path, self.dump_to_string())
    }
}

pub fn lower_to_bytecode(program: &CCProgram) -> BytecodeProgram {
    BytecodeLowerer::new(program).lower_program(program)
}

struct BytecodeLowerer {
    function_chunk_ids: HashMap<usize, usize>,
    chunks: Vec<Chunk>,
}

impl BytecodeLowerer {
    fn new(program: &CCProgram) -> Self {
        let mut function_chunk_ids = HashMap::new();

        // Reserve dense chunk ids for functions first.
        for func in &program.functions {
            function_chunk_ids.insert(func.id, function_chunk_ids.len());
        }

        Self {
            function_chunk_ids,
            chunks: Vec::new(),
        }
    }

    fn lower_program(mut self, program: &CCProgram) -> BytecodeProgram {
        for func in &program.functions {
            let chunk = self.lower_function(func);
            self.push_chunk(chunk);
        }

        let globals = program
            .globals
            .iter()
            .map(|global| self.lower_global(global))
            .collect();

        BytecodeProgram {
            chunks: self.chunks,
            globals,
            constructor_arities: program.constructor_arities.clone(),
        }
    }

    fn push_chunk(&mut self, chunk: Chunk) {
        let expected_id = self.chunks.len();
        assert_eq!(chunk.id, expected_id, "chunk ids must be dense");
        self.chunks.push(chunk);
    }

    fn lower_function(&self, func: &CCFunction) -> Chunk {
        let capture_count = u16::try_from(func.capture_count)
            .unwrap_or_else(|_| panic!("too many captures in function '{}'", func.name));

        let arity = u16::try_from(func.arity)
            .unwrap_or_else(|_| panic!("arity too large in function '{}'", func.name));

        let mut builder = ChunkBuilder::new_function(
            self.function_chunk_ids[&func.id],
            func.name.clone(),
            capture_count,
            arity,
        );

        let mut env = LocalEnv::default();

        // Slot layout:
        //   0 .. capture_count-1              = captures
        //   capture_count .. + arity - 1      = arguments (oldest .. newest)
        for offset in 0..arity {
            env.locals.push(capture_count + offset);
        }

        self.compile_tail_expr(&mut builder, &env, &func.body);

        builder.finish()
    }

    fn lower_global(&mut self, global: &CCGlobalDef) -> CompiledGlobal {
        let chunk_id = self.chunks.len();
        let mut builder = ChunkBuilder::new_global(chunk_id, global.name.clone());
        let env = LocalEnv::default();

        self.compile_expr(&mut builder, &env, &global.body);
        builder.emit(Instr::Return);

        self.push_chunk(builder.finish());

        CompiledGlobal {
            name: global.name.clone(),
            chunk: chunk_id,
        }
    }

    fn compile_tail_expr(&self, builder: &mut ChunkBuilder, env: &LocalEnv, expr: &Rc<CCExpr>) {
        match expr.as_ref() {
            CCExpr::Call { callee, args } => {
                self.compile_expr(builder, env, callee);

                for arg in args {
                    self.compile_expr(builder, env, arg);
                }

                let arg_count = u16::try_from(args.len())
                    .unwrap_or_else(|_| panic!("arg count {} does not fit in u16", args.len()));

                builder.emit(Instr::TailCallN { arg_count });
            }

            CCExpr::CallKnown { func, args } => {
                for arg in args {
                    self.compile_expr(builder, env, arg);
                }

                let chunk = self.function_chunk_ids[func];
                let chunk = u32::try_from(chunk)
                    .unwrap_or_else(|_| panic!("chunk id {} does not fit in u32", chunk));
                let arg_count = u16::try_from(args.len())
                    .unwrap_or_else(|_| panic!("arg count {} does not fit in u16", args.len()));

                builder.emit(Instr::TailCallKnown { chunk, arg_count });
            }

            CCExpr::Let(binder, body) => {
                self.compile_expr(builder, env, binder);

                let slot = builder.alloc_local_slot();
                builder.emit(Instr::StoreLocal(slot));

                let mut body_env = env.clone();
                body_env.locals.push(slot);

                self.compile_tail_expr(builder, &body_env, body);
            }

            CCExpr::IfThenElse {
                cond,
                then_br,
                else_br,
            } => {
                self.compile_expr(builder, env, cond);

                let jump_if_false = builder.emit_placeholder_jump_if_false();
                self.compile_tail_expr(builder, env, then_br);

                let else_target = builder.current_pc();
                builder.patch_jump_if_false(jump_if_false, else_target);

                self.compile_tail_expr(builder, env, else_br);
            }

            CCExpr::Case { scrutinee, arms } => {
                self.compile_expr(builder, env, scrutinee);

                let match_pos = builder.emit_placeholder_match();
                let mut compiled_arms = Vec::with_capacity(arms.len());

                for arm in arms {
                    let bind_slots = builder.alloc_pattern_slots(arm.pattern.binder_count());
                    let target = builder.current_pc();

                    let mut arm_env = env.clone();
                    arm_env.locals.extend(bind_slots.iter().copied());

                    self.compile_tail_expr(builder, &arm_env, &arm.body);

                    compiled_arms.push(MatchArm {
                        pattern: lower_pattern(&arm.pattern),
                        bind_slots: bind_slots.into_boxed_slice(),
                        target,
                    });
                }

                let else_target = builder.current_pc();
                builder.emit(Instr::Trap(NON_EXHAUSTIVE_CASE_TRAP));

                builder.patch_match(match_pos, compiled_arms.into_boxed_slice(), else_target);
            }

            _ => {
                self.compile_expr(builder, env, expr);
                builder.emit(Instr::Return);
            }
        }
    }

    fn compile_expr(&self, builder: &mut ChunkBuilder, env: &LocalEnv, expr: &Rc<CCExpr>) {
        match expr.as_ref() {
            CCExpr::Unit => builder.emit(Instr::PushUnit),
            CCExpr::Int(n) => builder.emit(Instr::PushInt(*n)),

            CCExpr::Local(index) => {
                let slot = env.lookup(*index);
                builder.emit(Instr::LoadLocal(slot));
            }

            CCExpr::Capture(index) => {
                let slot = u16::try_from(*index)
                    .unwrap_or_else(|_| panic!("capture index {} does not fit in u16", index));
                builder.emit(Instr::LoadLocal(slot));
            }

            CCExpr::Global(index) => {
                let idx = u32::try_from(*index)
                    .unwrap_or_else(|_| panic!("global index {} does not fit in u32", index));
                builder.emit(Instr::LoadGlobal(idx));
            }

            CCExpr::CtorFn { tag, arity } => {
                builder.emit(Instr::PushCtorFn {
                    tag: *tag,
                    arity: *arity,
                });
            }

            CCExpr::UnaryOp(op, rhs) => {
                self.compile_expr(builder, env, rhs);
                builder.emit(Instr::Unary(*op));
            }

            CCExpr::BinaryOp(op, lhs, rhs) => {
                self.compile_expr(builder, env, lhs);
                self.compile_expr(builder, env, rhs);
                builder.emit(Instr::Binary(*op));
            }

            CCExpr::MakeClosure { func, captures } => {
                for capture in captures {
                    self.compile_expr(builder, env, capture);
                }

                let chunk = self.function_chunk_ids[func];
                let chunk = u32::try_from(chunk)
                    .unwrap_or_else(|_| panic!("chunk id {} does not fit in u32", chunk));
                let capture_count = u16::try_from(captures.len()).unwrap_or_else(|_| {
                    panic!("capture count {} does not fit in u16", captures.len())
                });

                builder.emit(Instr::MakeClosure {
                    chunk,
                    capture_count,
                });
            }
            CCExpr::FunctionRef { func, arity } => {
                let chunk = self.function_chunk_ids[func];
                let chunk = u32::try_from(chunk)
                    .unwrap_or_else(|_| panic!("chunk id {} does not fit in u32", chunk));
                let arity = u16::try_from(*arity)
                    .unwrap_or_else(|_| panic!("arity {} does not fit in u16", arity));

                builder.emit(Instr::PushFunctionRef { chunk, arity });
            }

            CCExpr::Call { callee, args } => {
                self.compile_expr(builder, env, callee);

                for arg in args {
                    self.compile_expr(builder, env, arg);
                }

                let arg_count = u16::try_from(args.len())
                    .unwrap_or_else(|_| panic!("arg count {} does not fit in u16", args.len()));

                builder.emit(Instr::CallN { arg_count });
            }

            CCExpr::CallKnown { func, args } => {
                for arg in args {
                    self.compile_expr(builder, env, arg);
                }

                let chunk = self.function_chunk_ids[func];
                let chunk = u32::try_from(chunk)
                    .unwrap_or_else(|_| panic!("chunk id {} does not fit in u32", chunk));
                let arg_count = u16::try_from(args.len())
                    .unwrap_or_else(|_| panic!("arg count {} does not fit in u16", args.len()));

                builder.emit(Instr::CallKnown { chunk, arg_count });
            }

            CCExpr::Let(binder, body) => {
                self.compile_expr(builder, env, binder);

                let slot = builder.alloc_local_slot();
                builder.emit(Instr::StoreLocal(slot));

                let mut body_env = env.clone();
                body_env.locals.push(slot);

                self.compile_expr(builder, &body_env, body);
            }

            CCExpr::Pack { tag, arity, fields } => {
                for field in fields {
                    self.compile_expr(builder, env, field);
                }

                let field_count = u16::try_from(fields.len())
                    .unwrap_or_else(|_| panic!("field count {} does not fit in u16", fields.len()));

                builder.emit(Instr::Pack {
                    tag: *tag,
                    arity: *arity,
                    field_count,
                });
            }

            CCExpr::IfThenElse {
                cond,
                then_br,
                else_br,
            } => {
                self.compile_expr(builder, env, cond);

                let jump_if_false = builder.emit_placeholder_jump_if_false();
                self.compile_expr(builder, env, then_br);
                let jump_end = builder.emit_placeholder_jump();

                let else_target = builder.current_pc();
                builder.patch_jump_if_false(jump_if_false, else_target);

                self.compile_expr(builder, env, else_br);

                let end_target = builder.current_pc();
                builder.patch_jump(jump_end, end_target);
            }

            CCExpr::Case { scrutinee, arms } => {
                self.compile_expr(builder, env, scrutinee);

                let match_pos = builder.emit_placeholder_match();
                let mut compiled_arms = Vec::with_capacity(arms.len());
                let mut arm_jumps_to_end = Vec::with_capacity(arms.len());

                for arm in arms {
                    let bind_slots = builder.alloc_pattern_slots(arm.pattern.binder_count());
                    let target = builder.current_pc();

                    let mut arm_env = env.clone();
                    arm_env.locals.extend(bind_slots.iter().copied());

                    self.compile_expr(builder, &arm_env, &arm.body);

                    let jump_to_end = builder.emit_placeholder_jump();
                    arm_jumps_to_end.push(jump_to_end);

                    compiled_arms.push(MatchArm {
                        pattern: lower_pattern(&arm.pattern),
                        bind_slots: bind_slots.into_boxed_slice(),
                        target,
                    });
                }

                let else_target = builder.current_pc();
                builder.emit(Instr::Trap(NON_EXHAUSTIVE_CASE_TRAP));

                let end_target = builder.current_pc();
                for jump in arm_jumps_to_end {
                    builder.patch_jump(jump, end_target);
                }

                builder.patch_match(match_pos, compiled_arms.into_boxed_slice(), else_target);
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
struct LocalEnv {
    locals: Vec<u16>,
}

impl LocalEnv {
    fn lookup(&self, index: usize) -> u16 {
        let pos = self
            .locals
            .len()
            .checked_sub(index + 1)
            .unwrap_or_else(|| panic!("bad de Bruijn local index {}", index));
        self.locals[pos]
    }
}

struct ChunkBuilder {
    id: usize,
    name: String,
    kind: ChunkKind,
    code: Vec<Instr>,

    next_slot: u16,
    max_slot_exclusive: u16,
    match_tables: Vec<MatchTable>,
}

impl ChunkBuilder {
    fn new_global(id: usize, name: String) -> Self {
        Self {
            id,
            name,
            kind: ChunkKind::GlobalThunk,
            code: Vec::new(),
            match_tables: Vec::new(),
            next_slot: 0,
            max_slot_exclusive: 0,
        }
    }

    fn new_function(id: usize, name: String, capture_count: u16, arity: u16) -> Self {
        let next_slot = capture_count
            .checked_add(arity)
            .unwrap_or_else(|| panic!("slot overflow in function '{}'", name));

        Self {
            id,
            name,
            kind: ChunkKind::Function {
                capture_count,
                arity,
            },
            code: Vec::new(),
            match_tables: Vec::new(),
            next_slot,
            max_slot_exclusive: next_slot,
        }
    }

    fn emit(&mut self, instr: Instr) {
        self.code.push(instr);
    }

    fn current_pc(&self) -> u32 {
        u32::try_from(self.code.len()).unwrap_or_else(|_| panic!("program counter overflow"))
    }

    fn emit_placeholder_jump(&mut self) -> usize {
        let pos = self.code.len();
        self.code.push(Instr::Jump(u32::MAX));
        pos
    }

    fn patch_jump(&mut self, pos: usize, target: u32) {
        match &mut self.code[pos] {
            Instr::Jump(slot) => *slot = target,
            other => panic!("expected Jump at patch site, found {:?}", other),
        }
    }

    fn emit_placeholder_jump_if_false(&mut self) -> usize {
        let pos = self.code.len();
        self.code.push(Instr::JumpIfFalse(u32::MAX));
        pos
    }

    fn patch_jump_if_false(&mut self, pos: usize, target: u32) {
        match &mut self.code[pos] {
            Instr::JumpIfFalse(slot) => *slot = target,
            other => panic!("expected JumpIfFalse at patch site, found {:?}", other),
        }
    }

    fn emit_placeholder_match(&mut self) -> usize {
        let table_idx = self.match_tables.len();
        self.match_tables.push(MatchTable {
            arms: Box::new([]),
            else_target: u32::MAX,
        });
        self.code.push(Instr::Match(table_idx as u16));
        table_idx
    }

    fn patch_match(&mut self, table_idx: usize, arms: Box<[MatchArm]>, else_target: u32) {
        self.match_tables[table_idx] = MatchTable { arms, else_target };
    }

    fn alloc_local_slot(&mut self) -> u16 {
        let slot = self.next_slot;
        self.next_slot = self
            .next_slot
            .checked_add(1)
            .unwrap_or_else(|| panic!("slot overflow in chunk '{}'", self.name));
        self.max_slot_exclusive = self.max_slot_exclusive.max(self.next_slot);
        slot
    }

    fn alloc_pattern_slots(&mut self, count: usize) -> Vec<u16> {
        (0..count).map(|_| self.alloc_local_slot()).collect()
    }

    fn finish(self) -> Chunk {
        Chunk {
            id: self.id,
            name: self.name,
            kind: self.kind,
            slot_count: self.max_slot_exclusive,
            code: self.code,
            match_tables: self.match_tables,
        }
    }
}

const LABEL_COL_WIDTH: usize = 8;

fn format_label_def(pc: u32) -> String {
    format!("L{pc:04}:")
}

fn format_target(pc: u32) -> String {
    format!("L{pc:04}")
}

fn format_label_line(pc: u32) -> String {
    let label = format_label_def(pc);
    format!("{label:<LABEL_COL_WIDTH$}\n")
}

fn format_code_line(pc: usize, body: impl std::fmt::Display) -> String {
    format!("{:LABEL_COL_WIDTH$} {pc:04} {body}\n", "")
}

fn format_detail_line(body: impl std::fmt::Display) -> String {
    format!("{:LABEL_COL_WIDTH$}      {body}\n", "")
}

fn collect_chunk_labels(chunk: &Chunk) -> BTreeSet<u32> {
    let mut labels = BTreeSet::new();

    for instr in &chunk.code {
        match instr {
            Instr::Jump(target) | Instr::JumpIfFalse(target) => {
                labels.insert(*target);
            }
            Instr::Match(table_ix) => {
                let table = &chunk.match_tables[*table_ix as usize];
                labels.insert(table.else_target);
                for arm in &table.arms {
                    labels.insert(arm.target);
                }
            }
            _ => {}
        }
    }

    labels
}

fn format_chunk_kind(kind: ChunkKind) -> String {
    match kind {
        ChunkKind::GlobalThunk => "GlobalThunk".to_string(),
        ChunkKind::Function {
            capture_count,
            arity,
        } => {
            format!("Function(captures={capture_count}, arity={arity})")
        }
    }
}

fn format_instr(instr: &Instr) -> String {
    match instr {
        Instr::PushUnit => "PushUnit".to_string(),
        Instr::PushInt(n) => format!("PushInt {n}"),
        Instr::LoadLocal(slot) => format!("LoadLocal {slot}"),
        Instr::LoadGlobal(idx) => format!("LoadGlobal {idx}"),
        Instr::PushCtorFn { tag, arity } => format!("PushCtorFn tag={tag} arity={arity}"),
        Instr::MakeClosure {
            chunk,
            capture_count,
        } => format!("MakeClosure chunk={chunk} capture_count={capture_count}"),
        Instr::PushFunctionRef { chunk, arity } => {
            format!("PushFunctionRef chunk={chunk} arity={arity}")
        }
        Instr::Unary(op) => format!("Unary {:?}", op),
        Instr::Binary(op) => format!("Binary {:?}", op),
        Instr::Pack {
            tag,
            arity,
            field_count,
        } => format!("Pack tag={tag} arity={arity} field_count={field_count}"),
        Instr::StoreLocal(slot) => format!("StoreLocal {slot}"),
        Instr::CallN { arg_count } => format!("CallN arg_count={arg_count}"),
        Instr::CallKnown { chunk, arg_count } => {
            format!("CallKnown chunk={chunk} arg_count={arg_count}")
        }
        Instr::Return => "Return".to_string(),
        Instr::Jump(target) => format!("Jump {}", format_target(*target)),
        Instr::JumpIfFalse(target) => format!("JumpIfFalse {}", format_target(*target)),
        Instr::Match(table_ix) => format!("Match table={table_ix}"),
        Instr::TailCallN { arg_count } => format!("TailCallN arg_count={arg_count}"),
        Instr::TailCallKnown { chunk, arg_count } => {
            format!("TailCallKnown chunk={chunk} arg_count={arg_count}")
        }
        Instr::Trap(code) => format!("Trap {}", format_trap(*code)),
    }
}

fn format_trap(code: u16) -> String {
    match code {
        NON_EXHAUSTIVE_CASE_TRAP => "\"non-exhaustive case\"".to_string(),
        other => format!("code={other}"),
    }
}

fn format_pattern(pattern: &BytePattern) -> String {
    match pattern {
        BytePattern::Var => "Var".to_string(),
        BytePattern::Wild => "_".to_string(),
        BytePattern::Lit(lit_val) => match lit_val {
            Literal::Int(i_val) => format!("Int({})", i_val.to_string()),
        },
        BytePattern::Con(tag, sub_patterns) => {
            if sub_patterns.is_empty() {
                format!("Con({tag})")
            } else {
                let rendered = sub_patterns
                    .iter()
                    .map(format_pattern)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Con({tag}; [{rendered}])")
            }
        }
    }
}

fn lower_pattern(pattern: &CCPattern) -> BytePattern {
    match pattern {
        CCPattern::Var => BytePattern::Var,
        CCPattern::Wild => BytePattern::Wild,
        CCPattern::Lit(lit_val) => match lit_val {
            Literal::Int(i_val) => BytePattern::Lit(Literal::Int(*i_val)),
        },
        CCPattern::Con(tag, sub_patterns) => BytePattern::Con(
            *tag,
            sub_patterns
                .iter()
                .map(lower_pattern)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
    }
}
