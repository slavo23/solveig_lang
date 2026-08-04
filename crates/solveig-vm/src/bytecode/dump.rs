impl BytecodeProgram {
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
                info.tag, info.arity, info.name
            ));
        }

        out.push_str("\n=== String Constants ===\n");
        for (idx, value) in self.string_constants.iter().enumerate() {
            out.push_str(&format!("[{idx}] \"{}\"\n", escape_string_literal(value)));
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
                let pc_u32 = u32::try_from(pc)
                    .unwrap_or_else(|_| solveig_base::ice!("program counter {pc} does not fit in u32"));

                if labels.contains(&pc_u32) {
                    out.push_str(&format_label_line(pc_u32));
                }

                match instr {
                    Instr::Match(table_ix) => {
                        let table_ix = *table_ix as usize;
                        let table = chunk.match_tables.get(table_ix).unwrap_or_else(|| {
                            solveig_base::ice!(
                                "chunk '{}' references missing match table {}",
                                chunk.name,
                                table_ix
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
                let table = chunk
                    .match_tables
                    .get(*table_ix as usize)
                    .expect("ICE: bytecode match instruction references missing table");
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
        Instr::PushFloat(n) => format!("PushFloat {n}"),
        Instr::PushChar(ch) => format!("PushChar '{}'", escape_char_literal(*ch)),
        Instr::PushStr(index) => format!("PushStr const#{index}"),
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
        Instr::Unary(op) => format!("Unary {op:?}"),
        Instr::Binary(op) => format!("Binary {op:?}"),
        Instr::CallBuiltin { id, arg_count } => {
            format!("CallBuiltin id={} arg_count={arg_count}", id.0)
        }
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
        NON_EXHAUSTIVE_CASE_TRAP => format!("{:?}", trap_message(code)),
        other => format!("code={other}"),
    }
}

pub fn trap_message(code: u16) -> String {
    match code {
        NON_EXHAUSTIVE_CASE_TRAP => "non-exhaustive case".to_string(),
        other => format!("runtime trap code {other}"),
    }
}

fn format_pattern(pattern: &LowerPattern) -> String {
    match pattern {
        LowerPattern::Var => "Var".to_string(),
        LowerPattern::Wild => "_".to_string(),
        LowerPattern::Lit(lit_val) => match lit_val {
            Literal::Int(i_val) => format!("Int({i_val})"),
            Literal::Float(value) => format!("Float({value})"),
            Literal::Char(value) => format!("Char('{}')", escape_char_literal(*value)),
            Literal::Str(id) => format!("StrLit({})", id.0),
        },
        LowerPattern::Con(tag, sub_patterns) => {
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

fn escape_char_literal(ch: char) -> String {
    match ch {
        '\n' => "\\n".to_string(),
        '\t' => "\\t".to_string(),
        '\\' => "\\\\".to_string(),
        '\'' => "\\'".to_string(),
        '"' => "\\\"".to_string(),
        ch if ch.is_control() => ch.escape_unicode().to_string(),
        ch => ch.to_string(),
    }
}

fn escape_string_literal(value: &str) -> String {
    let mut out = String::new();

    for ch in value.chars() {
        out.push_str(&match ch {
            '\n' => "\\n".to_string(),
            '\t' => "\\t".to_string(),
            '\\' => "\\\\".to_string(),
            '"' => "\\\"".to_string(),
            ch if ch.is_control() => ch.escape_unicode().to_string(),
            ch => ch.to_string(),
        });
    }

    out
}
