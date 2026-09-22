pub fn lower_to_bytecode(program: &CCProgram) -> BytecodeProgram {
    BytecodeLowerer::new(program).lower_program(program)
}

struct BytecodeLowerer {
    function_chunk_ids: FxHashMap<usize, usize>,
    chunks: Vec<Chunk>,
}

impl BytecodeLowerer {
    fn new(program: &CCProgram) -> Self {
        let mut function_chunk_ids = FxHashMap::default();

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
            string_constants: program.string_constants.clone(),
            big_int_constants: program
                .big_int_constants
                .iter()
                .map(|value| {
                    value.parse::<num_bigint::BigInt>().unwrap_or_else(|_error| {
                        solveig_base::ice!("lowering produced an invalid BigInt constant `{value}`")
                    })
                })
                .collect(),
            string_literals: program.string_literals.clone(),
        }
    }

    fn push_chunk(&mut self, chunk: Chunk) {
        let expected_id = self.chunks.len();
        assert_eq!(chunk.id, expected_id, "chunk ids must be dense");
        self.chunks.push(chunk);
    }

    fn function_chunk_id(&self, function_id: usize) -> usize {
        *self
            .function_chunk_ids
            .get(&function_id)
            .expect("ICE: closure function id missing bytecode chunk")
    }

    fn lower_function(&self, func: &CCFunction) -> Chunk {
        let capture_count = u16::try_from(func.capture_count)
            .unwrap_or_else(|_| solveig_base::ice!("too many captures in function '{}'", func.name));

        let arity = u16::try_from(func.arity)
            .unwrap_or_else(|_| solveig_base::ice!("arity too large in function '{}'", func.name));

        let mut builder = ChunkBuilder::new_function(
            self.function_chunk_id(func.id),
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
            CCExpr::Located(source, inner) => {
                let previous = builder.set_source(Some(*source));
                self.compile_tail_expr(builder, env, inner);
                builder.set_source(previous);
            }
            CCExpr::Call { callee, args } => {
                self.compile_expr(builder, env, callee);

                for arg in args {
                    self.compile_expr(builder, env, arg);
                }

                let arg_count = u16::try_from(args.len()).unwrap_or_else(|_| {
                    solveig_base::ice!("arg count {} does not fit in u16", args.len())
                });

                builder.emit(Instr::TailCallN { arg_count });
            }

            CCExpr::CallKnown { func, args } => {
                for arg in args {
                    self.compile_expr(builder, env, arg);
                }

                let chunk = self.function_chunk_id(*func);
                let chunk = u32::try_from(chunk)
                    .unwrap_or_else(|_| solveig_base::ice!("chunk id {chunk} does not fit in u32"));
                let arg_count = u16::try_from(args.len()).unwrap_or_else(|_| {
                    solveig_base::ice!("arg count {} does not fit in u16", args.len())
                });

                builder.emit(Instr::TailCallKnown { chunk, arg_count });
            }

            CCExpr::CallBuiltin { id, args } => {
                for arg in args {
                    self.compile_expr(builder, env, arg);
                }

                let arg_count = u16::try_from(args.len()).unwrap_or_else(|_| {
                    solveig_base::ice!("arg count {} does not fit in u16", args.len())
                });

                builder.emit(Instr::CallBuiltin { id: *id, arg_count });
                builder.emit(Instr::Return);
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
                        pattern: arm.pattern.clone(),
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
            CCExpr::Located(source, inner) => {
                let previous = builder.set_source(Some(*source));
                self.compile_expr(builder, env, inner);
                builder.set_source(previous);
            }
            CCExpr::Unit => builder.emit(Instr::PushUnit),
            CCExpr::Int(n) => builder.emit(Instr::PushInt(*n)),
            CCExpr::BigIntConst(index) => builder.emit(Instr::PushBigInt(*index)),
            CCExpr::Float(n) => builder.emit(Instr::PushFloat(*n)),
            CCExpr::Char(ch) => builder.emit(Instr::PushChar(*ch)),
            CCExpr::StrConst(index) => builder.emit(Instr::PushStr(*index)),

            CCExpr::Local(_) | CCExpr::Capture(_) => {
                let Some(slot) = expression_local_slot(expr, env) else {
                    solveig_base::ice!("local bytecode expression lost its stack slot");
                };
                builder.emit(Instr::LoadLocal(slot));
            }

            CCExpr::Global(index) => {
                let idx = u32::try_from(*index)
                    .unwrap_or_else(|_| solveig_base::ice!("global index {index} does not fit in u32"));
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
                // Ertl and Gregg's superinstruction technique is applied only
                // to the operand shapes selected by O5's dynamic profile.
                if let Some(local) = expression_local_slot(lhs, env)
                    && let CCExpr::Int(rhs) = unlocated_expression(rhs)
                {
                    builder.emit(Instr::BinaryLocalInt {
                        op: *op,
                        local,
                        rhs: *rhs,
                    });
                } else if let (Some(lhs), Some(rhs)) = (
                    expression_local_slot(lhs, env),
                    expression_local_slot(rhs, env),
                ) {
                    builder.emit(Instr::BinaryLocals { op: *op, lhs, rhs });
                } else {
                    self.compile_expr(builder, env, lhs);
                    self.compile_expr(builder, env, rhs);
                    builder.emit(Instr::Binary(*op));
                }
            }

            CCExpr::MakeClosure { func, captures } => {
                for capture in captures {
                    self.compile_expr(builder, env, capture);
                }

                let chunk = self.function_chunk_id(*func);
                let chunk = u32::try_from(chunk)
                    .unwrap_or_else(|_| solveig_base::ice!("chunk id {chunk} does not fit in u32"));
                let capture_count = u16::try_from(captures.len()).unwrap_or_else(|_| {
                    solveig_base::ice!("capture count {} does not fit in u16", captures.len())
                });

                builder.emit(Instr::MakeClosure {
                    chunk,
                    capture_count,
                });
            }
            CCExpr::FunctionRef { func, arity } => {
                let chunk = self.function_chunk_id(*func);
                let chunk = u32::try_from(chunk)
                    .unwrap_or_else(|_| solveig_base::ice!("chunk id {chunk} does not fit in u32"));
                let arity = u16::try_from(*arity)
                    .unwrap_or_else(|_| solveig_base::ice!("arity {arity} does not fit in u16"));

                builder.emit(Instr::PushFunctionRef { chunk, arity });
            }

            CCExpr::Call { callee, args } => {
                self.compile_expr(builder, env, callee);

                for arg in args {
                    self.compile_expr(builder, env, arg);
                }

                let arg_count = u16::try_from(args.len()).unwrap_or_else(|_| {
                    solveig_base::ice!("arg count {} does not fit in u16", args.len())
                });

                builder.emit(Instr::CallN { arg_count });
            }

            CCExpr::CallKnown { func, args } => {
                let chunk = self.function_chunk_id(*func);
                let chunk = u32::try_from(chunk)
                    .unwrap_or_else(|_| solveig_base::ice!("chunk id {chunk} does not fit in u32"));
                let arg_count = u16::try_from(args.len()).unwrap_or_else(|_| {
                    solveig_base::ice!("arg count {} does not fit in u16", args.len())
                });

                // The same O5 profile selected one- and two-local suffixes of
                // known calls. Earlier arguments retain their evaluation order.
                if let Some((last, prefix)) = args.split_last()
                    && let Some(local) = expression_local_slot(last, env)
                {
                    if let Some((penultimate, earlier)) = prefix.split_last()
                        && let Some(first) = expression_local_slot(penultimate, env)
                    {
                        for arg in earlier {
                            self.compile_expr(builder, env, arg);
                        }
                        builder.emit(Instr::CallKnownLocals {
                            chunk,
                            arg_count,
                            first,
                            second: local,
                        });
                    } else {
                        for arg in prefix {
                            self.compile_expr(builder, env, arg);
                        }
                        builder.emit(Instr::CallKnownLocal {
                            chunk,
                            arg_count,
                            local,
                        });
                    }
                } else {
                    for arg in args {
                        self.compile_expr(builder, env, arg);
                    }
                    builder.emit(Instr::CallKnown { chunk, arg_count });
                }
            }

            CCExpr::CallBuiltin { id, args } => {
                for arg in args {
                    self.compile_expr(builder, env, arg);
                }

                let arg_count = u16::try_from(args.len()).unwrap_or_else(|_| {
                    solveig_base::ice!("arg count {} does not fit in u16", args.len())
                });

                builder.emit(Instr::CallBuiltin { id: *id, arg_count });
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

                let field_count = u16::try_from(fields.len()).unwrap_or_else(|_| {
                    solveig_base::ice!("field count {} does not fit in u16", fields.len())
                });

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
                        pattern: arm.pattern.clone(),
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

fn unlocated_expression(mut expr: &CCExpr) -> &CCExpr {
    while let CCExpr::Located(_, inner) = expr {
        expr = inner;
    }
    expr
}

fn expression_local_slot(expr: &CCExpr, env: &LocalEnv) -> Option<u16> {
    match unlocated_expression(expr) {
        CCExpr::Local(index) => Some(env.lookup(*index)),
        CCExpr::Capture(index) => Some(u16::try_from(*index).unwrap_or_else(|_| {
            solveig_base::ice!("capture index {index} does not fit in u16")
        })),
        CCExpr::Located(..)
        | CCExpr::Unit
        | CCExpr::Int(_)
        | CCExpr::BigIntConst(_)
        | CCExpr::Float(_)
        | CCExpr::Char(_)
        | CCExpr::StrConst(_)
        | CCExpr::Global(_)
        | CCExpr::CtorFn { .. }
        | CCExpr::CallBuiltin { .. }
        | CCExpr::UnaryOp(..)
        | CCExpr::BinaryOp(..)
        | CCExpr::MakeClosure { .. }
        | CCExpr::FunctionRef { .. }
        | CCExpr::Call { .. }
        | CCExpr::CallKnown { .. }
        | CCExpr::Let(..)
        | CCExpr::Pack { .. }
        | CCExpr::Case { .. }
        | CCExpr::IfThenElse { .. } => None,
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
            .unwrap_or_else(|| solveig_base::ice!("bad de Bruijn local index {index}"));
        *self
            .locals
            .get(pos)
            .expect("ICE: de Bruijn local slot position out of bounds")
    }
}

struct ChunkBuilder {
    id: usize,
    name: String,
    kind: ChunkKind,
    code: Vec<Instr>,
    source_spans: Vec<Option<FileSpan>>,
    current_source: Option<FileSpan>,

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
            source_spans: Vec::new(),
            current_source: None,
            match_tables: Vec::new(),
            next_slot: 0,
            max_slot_exclusive: 0,
        }
    }

    fn new_function(id: usize, name: String, capture_count: u16, arity: u16) -> Self {
        let next_slot = capture_count
            .checked_add(arity)
            .unwrap_or_else(|| solveig_base::ice!("slot overflow in function '{name}'"));

        Self {
            id,
            name,
            kind: ChunkKind::Function {
                capture_count,
                arity,
            },
            code: Vec::new(),
            source_spans: Vec::new(),
            current_source: None,
            match_tables: Vec::new(),
            next_slot,
            max_slot_exclusive: next_slot,
        }
    }

    fn emit(&mut self, instr: Instr) {
        self.code.push(instr);
        self.source_spans.push(self.current_source);
    }

    fn set_source(&mut self, source: Option<FileSpan>) -> Option<FileSpan> {
        std::mem::replace(&mut self.current_source, source)
    }

    fn current_pc(&self) -> u32 {
        u32::try_from(self.code.len()).unwrap_or_else(|_| solveig_base::ice!("program counter overflow"))
    }

    fn emit_placeholder_jump(&mut self) -> usize {
        let pos = self.code.len();
        self.emit(Instr::Jump(u32::MAX));
        pos
    }

    fn patch_jump(&mut self, pos: usize, target: u32) {
        match self
            .code
            .get_mut(pos)
            .expect("ICE: jump patch position out of bounds")
        {
            Instr::Jump(slot) => *slot = target,
            other => solveig_base::ice!("expected Jump at patch site, found {other:?}"),
        }
    }

    fn emit_placeholder_jump_if_false(&mut self) -> usize {
        let pos = self.code.len();
        self.emit(Instr::JumpIfFalse(u32::MAX));
        pos
    }

    fn patch_jump_if_false(&mut self, pos: usize, target: u32) {
        match self
            .code
            .get_mut(pos)
            .expect("ICE: conditional jump patch position out of bounds")
        {
            Instr::JumpIfFalse(slot) => *slot = target,
            other => solveig_base::ice!("expected JumpIfFalse at patch site, found {other:?}"),
        }
    }

    fn emit_placeholder_match(&mut self) -> usize {
        let table_idx = self.match_tables.len();
        self.match_tables.push(MatchTable {
            arms: Box::new([]),
            else_target: u32::MAX,
        });
        let table_idx_u16 = u16::try_from(table_idx)
            .unwrap_or_else(|_| solveig_base::ice!("match table index {table_idx} does not fit in u16"));
        self.emit(Instr::Match(table_idx_u16));
        table_idx
    }

    fn patch_match(&mut self, table_idx: usize, arms: Box<[MatchArm]>, else_target: u32) {
        *self
            .match_tables
            .get_mut(table_idx)
            .expect("ICE: match table patch index out of bounds") = MatchTable { arms, else_target };
    }

    fn alloc_local_slot(&mut self) -> u16 {
        let slot = self.next_slot;
        self.next_slot = self
            .next_slot
            .checked_add(1)
            .unwrap_or_else(|| solveig_base::ice!("slot overflow in chunk '{}'", self.name));
        self.max_slot_exclusive = self.max_slot_exclusive.max(self.next_slot);
        slot
    }

    fn alloc_pattern_slots(&mut self, count: usize) -> Vec<u16> {
        (0..count).map(|_| self.alloc_local_slot()).collect()
    }

    fn finish(self) -> Chunk {
        assert_eq!(
            self.code.len(),
            self.source_spans.len(),
            "ICE: bytecode source table must contain one entry per instruction"
        );
        Chunk {
            id: self.id,
            name: self.name,
            kind: self.kind,
            slot_count: self.max_slot_exclusive,
            code: self.code,
            source_spans: self.source_spans,
            match_tables: self.match_tables,
        }
    }
}
