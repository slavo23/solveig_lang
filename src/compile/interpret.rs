use std::fmt;
use std::rc::Rc;

use super::lower_ast::{IRCaseArm, IRExpr, IRPattern, IRProgram};

#[derive(Clone, Debug)]
pub enum Value {
    Unit,
    Closure {
        env: Rc<Env>,
        body: Rc<IRExpr>,
    },
    PartialCon {
        tag: u16,
        arity: u16,
        fields: Vec<Value>,
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
                .map(|field| format_atomic(field, program))
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
                current = &fields[0];
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
                items.push(format_value(&fields[0], program));
                current = &fields[1];
            }

            _ => return None,
        }
    }
}

fn try_format_pair(value: &Value, program: &IRProgram) -> Option<String> {
    match value {
        Value::PartialCon { tag, arity, fields }
            if (*arity == 2)
                && (program.constructor_name(*tag) == Some("Pair")
                    || program.constructor_name(*tag) == Some("MkPair"))
                && fields.len() == 2 =>
        {
            Some(format!(
                "({}, {})",
                format_value(&fields[0], program),
                format_value(&fields[1], program)
            ))
        }

        _ => None,
    }
}

#[derive(Clone, Debug)]
pub enum Env {
    Empty,
    Cons(Value, Rc<Env>),
}

impl Env {
    fn extend(&self, value: Value) -> Env {
        Env::Cons(value, Rc::new(self.clone()))
    }

    fn extend_many(&self, values: &[Value]) -> Env {
        let mut env = self.clone();
        for value in values {
            env = env.extend(value.clone());
        }
        env
    }

    fn lookup(&self, index: usize) -> &Value {
        let mut current = self;
        let mut remaining = index;

        loop {
            match current {
                Env::Empty => panic!("unbound variable at index {}", index),
                Env::Cons(value, parent) => {
                    if remaining == 0 {
                        return value;
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
        env: Env,
    },
    Apply(Value),
    CaseOf {
        arms: Vec<IRCaseArm>,
        env: Env,
    },
    IfBranch {
        then_br: Rc<IRExpr>,
        else_br: Rc<IRExpr>,
        env: Env,
    },
    LetBody {
        body: Rc<IRExpr>,
        env: Env,
    },
}

pub struct Machine {
    control: Rc<IRExpr>,
    env: Env,
    continuation: Vec<Frame>,
    globals: Vec<Rc<IRExpr>>,
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

        let globals: Vec<Rc<IRExpr>> = program
            .globals
            .iter()
            .map(|global_def| global_def.body.clone())
            .collect();

        let mut machine = Machine {
            control: Rc::new(IRExpr::FVar(entry_index)),
            env: Env::Empty,
            continuation: Vec::new(),
            globals,
            true_tag,
            false_tag,
        };

        machine.run()
    }

    fn run(&mut self) -> Value {
        loop {
            let value = self.step();
            match self.return_value(value) {
                Some(result) => return result,
                None => continue,
            }
        }
    }

    fn step(&mut self) -> Value {
        loop {
            match self.control.as_ref() {
                IRExpr::Unit => return Value::Unit,

                IRExpr::BVar(index) => return self.env.lookup(*index).clone(),

                IRExpr::FVar(index) => {
                    let expr = self.globals[*index].clone();

                    match expr.as_ref() {
                        IRExpr::Abs(body) => {
                            return Value::Closure {
                                env: Rc::new(Env::Empty),
                                body: body.clone(),
                            };
                        }
                        _ => {
                            self.control = expr;
                            self.env = Env::Empty;
                            continue;
                        }
                    }
                }

                IRExpr::Con { tag, arity } => {
                    return Value::PartialCon {
                        tag: *tag,
                        arity: *arity,
                        fields: Vec::new(),
                    };
                }

                IRExpr::Abs(body) => {
                    return Value::Closure {
                        env: Rc::new(self.env.clone()),
                        body: body.clone(),
                    };
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
                        arms: arms.clone(),
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

    fn return_value(&mut self, value: Value) -> Option<Value> {
        match self.continuation.pop() {
            None => Some(value),

            Some(Frame::EvalArg { arg, env }) => {
                self.continuation.push(Frame::Apply(value));
                self.control = arg;
                self.env = env;
                None
            }

            Some(Frame::Apply(function)) => self.apply(function, value),

            Some(Frame::LetBody { body, env }) => {
                self.env = env.extend(value);
                self.control = body;
                None
            }

            Some(Frame::CaseOf { arms, env }) => {
                for arm in &arms {
                    let mut bindings = Vec::new();
                    if match_pattern(&arm.pattern, &value, &mut bindings) {
                        self.env = env.extend_many(&bindings);
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
                self.control = match &value {
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
                None
            }
        }
    }

    fn apply(&mut self, function: Value, argument: Value) -> Option<Value> {
        match function {
            Value::Closure { env, body } => {
                self.env = env.extend(argument);
                self.control = body;
                None
            }

            Value::PartialCon {
                tag,
                arity,
                mut fields,
            } => {
                if fields.len() >= arity as usize {
                    panic!("cannot over-apply constructor tag {}", tag);
                }

                fields.push(argument);

                let result = Value::PartialCon { tag, arity, fields };
                self.return_value(result)
            }

            _ => panic!("cannot apply non-function"),
        }
    }
}

fn match_pattern(pattern: &IRPattern, value: &Value, bindings: &mut Vec<Value>) -> bool {
    match (pattern, value) {
        (IRPattern::Var, _) => {
            bindings.push(value.clone());
            true
        }

        (IRPattern::Wild, _) => true,

        (IRPattern::Con(expected_tag, sub_patterns), Value::PartialCon { tag, fields, .. }) => {
            if *expected_tag != *tag || sub_patterns.len() != fields.len() {
                return false;
            }

            for (sub_pattern, field_value) in sub_patterns.iter().zip(fields.iter()) {
                if !match_pattern(sub_pattern, field_value, bindings) {
                    return false;
                }
            }

            true
        }

        _ => false,
    }
}