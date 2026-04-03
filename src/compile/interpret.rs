use std::fmt;
use std::rc::Rc;

use super::lower_ast::{IRCaseArm, IRExpr, IRPattern, IRProgram};

type ValueRef = Rc<Value>;
type SharedEnv = Rc<Env>;

#[derive(Debug, Clone)]
pub enum Value {
    Unit,
    Closure {
        env: SharedEnv,
        body: Rc<IRExpr>,
    },
    PartialCon {
        tag: u16,
        arity: u16,
        fields: Vec<ValueRef>,
    },
}

impl Value {
    pub fn display<'a>(&'a self, program: &'a IRProgram) -> DisplayValue<'a> {
        DisplayValue {
            value: self,
            program,
        }
    }
}

pub struct DisplayValue<'a> {
    value: &'a Value,
    program: &'a IRProgram,
}

impl fmt::Display for DisplayValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", format_value(self.value, self.program))
    }
}

fn format_value(value: &Value, program: &IRProgram) -> String {
    if let Some(rendered) = try_format_nat(value, program) {
        return rendered;
    }

    if let Some(rendered) = try_format_list(value, program) {
        return rendered;
    }

    if let Some(rendered) = try_format_pair(value, program) {
        return rendered;
    }

    match value {
        Value::Unit => "()".to_string(),
        Value::Closure { .. } => "<closure>".to_string(),
        Value::PartialCon { tag, arity, fields } => {
            let name = program
                .constructor_name(*tag)
                .unwrap_or("<unknown-constructor>");

            if fields.is_empty() {
                return name.to_string();
            }

            let rendered_args = fields
                .iter()
                .map(|field| format_atomic(field.as_ref(), program))
                .collect::<Vec<_>>()
                .join(" ");

            if fields.len() < *arity as usize {
                format!("<partial {name} {rendered_args}>")
            } else {
                format!("{name} {rendered_args}")
            }
        }
    }
}

fn format_atomic(value: &Value, program: &IRProgram) -> String {
    if try_format_nat(value, program).is_some()
        || try_format_list(value, program).is_some()
        || try_format_pair(value, program).is_some()
    {
        return format_value(value, program);
    }

    match value {
        Value::Unit => format_value(value, program),
        Value::Closure { .. } => format_value(value, program),
        Value::PartialCon { fields, .. } if fields.is_empty() => format_value(value, program),
        _ => format!("({})", format_value(value, program)),
    }
}

fn try_format_nat(value: &Value, program: &IRProgram) -> Option<String> {
    let mut n: usize = 0;
    let mut current = value;

    loop {
        match current {
            Value::PartialCon { tag, arity, fields }
                if program.constructor_name(*tag) == Some("Z")
                    && *arity == 0
                    && fields.is_empty() =>
            {
                return Some(n.to_string());
            }

            Value::PartialCon { tag, arity, fields }
                if program.constructor_name(*tag) == Some("S")
                    && *arity == 1
                    && fields.len() == 1 =>
            {
                n += 1;
                current = fields[0].as_ref();
            }

            _ => return None,
        }
    }
}

fn try_format_list(value: &Value, program: &IRProgram) -> Option<String> {
    let mut items = Vec::new();
    let mut current = value;

    loop {
        match current {
            Value::PartialCon { tag, arity, fields }
                if program.constructor_name(*tag) == Some("Nil")
                    && *arity == 0
                    && fields.is_empty() =>
            {
                return Some(format!("[{}]", items.join(", ")));
            }

            Value::PartialCon { tag, arity, fields }
                if program.constructor_name(*tag) == Some("Cons")
                    && *arity == 2
                    && fields.len() == 2 =>
            {
                items.push(format_value(fields[0].as_ref(), program));
                current = fields[1].as_ref();
            }

            _ => return None,
        }
    }
}

fn try_format_pair(value: &Value, program: &IRProgram) -> Option<String> {
    match value {
        Value::PartialCon { tag, arity, fields }
            if *arity == 2
                && (program.constructor_name(*tag) == Some("Pair")
                    || program.constructor_name(*tag) == Some("MkPair"))
                && fields.len() == 2 =>
        {
            Some(format!(
                "({}, {})",
                format_value(fields[0].as_ref(), program),
                format_value(fields[1].as_ref(), program)
            ))
        }
        _ => None,
    }
}

#[derive(Debug)]
pub enum Env {
    Empty,
    Cons(ValueRef, SharedEnv),
}

impl Env {
    fn empty() -> SharedEnv {
        Rc::new(Env::Empty)
    }

    fn extend(env: &SharedEnv, value: ValueRef) -> SharedEnv {
        Rc::new(Env::Cons(value, env.clone()))
    }

    fn extend_many(env: &SharedEnv, values: &[ValueRef]) -> SharedEnv {
        let mut current = env.clone();
        for value in values {
            current = Self::extend(&current, value.clone());
        }
        current
    }

    fn lookup(env: &SharedEnv, index: usize) -> ValueRef {
        let mut current = env.as_ref();
        let mut remaining = index;

        loop {
            match current {
                Env::Empty => panic!("unbound variable at index {}", index),
                Env::Cons(value, parent) => {
                    if remaining == 0 {
                        return value.clone();
                    }
                    remaining -= 1;
                    current = parent.as_ref();
                }
            }
        }
    }
}

enum Frame {
    EvalArg {
        arg: Rc<IRExpr>,
        env: SharedEnv,
    },
    Apply(ValueRef),
    CaseOf {
        arms: Rc<[IRCaseArm]>,
        env: SharedEnv,
    },
    IfBranch {
        then_br: Rc<IRExpr>,
        else_br: Rc<IRExpr>,
        env: SharedEnv,
    },
    LetBody {
        body: Rc<IRExpr>,
        env: SharedEnv,
    },
}

enum GlobalEntry {
    Closure(ValueRef),
    Expr(Rc<IRExpr>),
}

pub struct Machine {
    control: Rc<IRExpr>,
    env: SharedEnv,
    continuation: Vec<Frame>,
    globals: Vec<GlobalEntry>,
    true_tag: u16,
    false_tag: u16,
}

impl Machine {
    pub fn exec(program: &IRProgram, entry_name: &str) -> Value {
        let entry_index = program
            .globals
            .iter()
            .position(|global_def| global_def.name == entry_name)
            .unwrap_or_else(|| panic!("entry point '{}' not found", entry_name));

        let true_tag = program
            .constructor_tag("True")
            .unwrap_or_else(|| panic!("constructor 'True' not found"));
        let false_tag = program
            .constructor_tag("False")
            .unwrap_or_else(|| panic!("constructor 'False' not found"));

        let globals = program
            .globals
            .iter()
            .map(|global_def| match global_def.body.as_ref() {
                IRExpr::Abs(body) => GlobalEntry::Closure(Rc::new(Value::Closure {
                    env: Env::empty(),
                    body: body.clone(),
                })),
                _ => GlobalEntry::Expr(global_def.body.clone()),
            })
            .collect();

        let mut machine = Machine {
            control: Rc::new(IRExpr::FVar(entry_index)),
            env: Env::empty(),
            continuation: Vec::new(),
            globals,
            true_tag,
            false_tag,
        };

        Rc::try_unwrap(machine.run()).unwrap_or_else(|rc| (*rc).clone())
    }

    fn run(&mut self) -> ValueRef {
        loop {
            let value = self.step();
            match self.return_value(value) {
                Some(result) => return result,
                None => continue,
            }
        }
    }

    fn step(&mut self) -> ValueRef {
        loop {
            match self.control.as_ref() {
                IRExpr::Unit => return Rc::new(Value::Unit),

                IRExpr::BVar(index) => return Env::lookup(&self.env, *index),

                IRExpr::FVar(index) => match &self.globals[*index] {
                    GlobalEntry::Closure(value) => return value.clone(),
                    GlobalEntry::Expr(expr) => {
                        self.control = expr.clone();
                        self.env = Env::empty();
                        continue;
                    }
                },

                IRExpr::Con { tag, arity } => {
                    return Rc::new(Value::PartialCon {
                        tag: *tag,
                        arity: *arity,
                        fields: Vec::new(),
                    });
                }

                IRExpr::Abs(body) => {
                    return Rc::new(Value::Closure {
                        env: self.env.clone(),
                        body: body.clone(),
                    });
                }

                IRExpr::App(func, arg) => {
                    self.continuation.push(Frame::EvalArg {
                        arg: arg.clone(),
                        env: self.env.clone(),
                    });
                    self.control = func.clone();
                    continue;
                }

                IRExpr::Let(binding, body) => {
                    self.continuation.push(Frame::LetBody {
                        body: body.clone(),
                        env: self.env.clone(),
                    });
                    self.control = binding.clone();
                    continue;
                }

                IRExpr::Case { scrutinee, arms } => {
                    self.continuation.push(Frame::CaseOf {
                        arms: arms.clone().into(),
                        env: self.env.clone(),
                    });
                    self.control = scrutinee.clone();
                    continue;
                }

                IRExpr::IfThenElse {
                    cond,
                    then_br,
                    else_br,
                } => {
                    self.continuation.push(Frame::IfBranch {
                        then_br: then_br.clone(),
                        else_br: else_br.clone(),
                        env: self.env.clone(),
                    });
                    self.control = cond.clone();
                    continue;
                }
            }
        }
    }

    fn return_value(&mut self, mut value: ValueRef) -> Option<ValueRef> {
        loop {
            match self.continuation.pop() {
                None => return Some(value),

                Some(Frame::EvalArg { arg, env }) => {
                    self.continuation.push(Frame::Apply(value));
                    self.control = arg;
                    self.env = env;
                    return None;
                }

                Some(Frame::Apply(function)) => match self.apply(function, value) {
                    Some(next_value) => {
                        value = next_value;
                        continue;
                    }
                    None => return None,
                },
                Some(Frame::LetBody { body, env }) => {
                    self.env = Env::extend(&env, value);
                    self.control = body;
                    return None;
                }

                Some(Frame::CaseOf { arms, env }) => {
                    for arm in arms.iter() {
                        let mut bindings = Vec::new();
                        if match_pattern(&arm.pattern, &value, &mut bindings) {
                            self.env = Env::extend_many(&env, &bindings);
                            self.control = arm.body.clone();
                            return None;
                        }
                    }
                    panic!("non-exhaustive case");
                }

                Some(Frame::IfBranch {
                    then_br,
                    else_br,
                    env,
                }) => {
                    self.env = env;
                    self.control = match value.as_ref() {
                        Value::PartialCon {
                            tag,
                            arity: 0,
                            fields,
                        } if *tag == self.true_tag && fields.is_empty() => then_br,
                        Value::PartialCon {
                            tag,
                            arity: 0,
                            fields,
                        } if *tag == self.false_tag && fields.is_empty() => else_br,
                        _ => panic!("if-condition is not a boolean"),
                    };
                    return None;
                }
            }
        }
    }

    fn apply(&mut self, function: ValueRef, argument: ValueRef) -> Option<ValueRef> {
        match function.as_ref() {
            Value::Closure { env, body } => {
                self.env = Env::extend(env, argument);
                self.control = body.clone();
                None
            }

            Value::PartialCon { tag, arity, fields } => {
                if fields.len() >= *arity as usize {
                    panic!("cannot over-apply constructor tag {}", tag);
                }

                let mut new_fields = Vec::with_capacity(fields.len() + 1);
                new_fields.extend(fields.iter().cloned());
                new_fields.push(argument);

                Some(Rc::new(Value::PartialCon {
                    tag: *tag,
                    arity: *arity,
                    fields: new_fields,
                }))
            }

            _ => panic!("cannot apply non-function"),
        }
    }
}

fn match_pattern(pattern: &IRPattern, value: &ValueRef, bindings: &mut Vec<ValueRef>) -> bool {
    let mut work: Vec<(&IRPattern, &ValueRef)> = vec![(pattern, value)];

    while let Some((pat, val)) = work.pop() {
        match (pat, val.as_ref()) {
            (IRPattern::Var, _) => {
                bindings.push(val.clone());
            }
            (IRPattern::Wild, _) => {}
            (IRPattern::Con(expected_tag, sub_patterns), Value::PartialCon { tag, fields, .. }) => {
                if *expected_tag != *tag || sub_patterns.len() != fields.len() {
                    return false;
                }

                for (sub_pat, field_val) in sub_patterns.iter().zip(fields.iter()).rev() {
                    work.push((sub_pat, field_val));
                }
            }

            _ => return false,
        }
    }

    true
}
