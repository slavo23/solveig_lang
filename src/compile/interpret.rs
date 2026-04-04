use core::ast::{BinOp, UnOp};
use std::fmt;
use std::rc::Rc;

use super::lower_ast::{IRCaseArm, IRExpr, IRPattern, IRProgram};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Addr(usize);

#[derive(Debug, Clone)]
struct Env(Rc<[Addr]>);

impl Env {
    fn empty() -> Self {
        Self(Rc::from(Vec::<Addr>::new().into_boxed_slice()))
    }

    fn extend(&self, value: Addr) -> Self {
        let mut values = Vec::with_capacity(self.0.len() + 1);
        values.extend_from_slice(&self.0);
        values.push(value);
        Self(Rc::from(values.into_boxed_slice()))
    }

    fn extend_many(&self, values: &[Addr]) -> Self {
        let mut out = Vec::with_capacity(self.0.len() + values.len());
        out.extend_from_slice(&self.0);
        out.extend_from_slice(values);
        Self(Rc::from(out.into_boxed_slice()))
    }

    fn lookup(&self, index: usize) -> Addr {
        let rev_index = self
            .0
            .len()
            .checked_sub(index + 1)
            .unwrap_or_else(|| panic!("unbound variable at index {}", index));
        self.0[rev_index]
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Unit,
    Int(i64),
    Closure {
        env: Env,
        body: Rc<IRExpr>,
    },
    Constructor {
        tag: u16,
        arity: u16,
        fields: Box<[Addr]>,
    },
}

#[derive(Debug, Default)]
pub struct Store {
    objects: Vec<Value>,
}

impl Store {
    fn alloc(&mut self, value: Value) -> Addr {
        let addr = Addr(self.objects.len());
        self.objects.push(value);
        addr
    }

    fn get(&self, addr: Addr) -> &Value {
        self.objects
            .get(addr.0)
            .unwrap_or_else(|| panic!("invalid store address {}", addr.0))
    }

    fn len(&self) -> usize {
        self.objects.len()
    }
}

pub struct EvalResult {
    root: Addr,
    store: Store,
}

impl EvalResult {
    pub fn display<'a>(&'a self, program: &'a IRProgram) -> DisplayValue<'a> {
        DisplayValue {
            root: self.root,
            store: &self.store,
            program,
        }
    }
}

pub struct DisplayValue<'a> {
    root: Addr,
    store: &'a Store,
    program: &'a IRProgram,
}

impl fmt::Display for DisplayValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let renderer = Renderer {
            store: self.store,
            program: self.program,
        };
        write!(f, "{}", renderer.render(self.root))
    }
}

struct Renderer<'a> {
    store: &'a Store,
    program: &'a IRProgram,
}

impl Renderer<'_> {
    fn render(&self, addr: Addr) -> String {
        if let Some(rendered) = self.try_render_nat(addr) {
            return rendered;
        }

        if let Some(rendered) = self.try_render_list(addr) {
            return rendered;
        }

        if let Some(rendered) = self.try_render_pair(addr) {
            return rendered;
        }

        match self.store.get(addr) {
            Value::Unit => "()".to_string(),
            Value::Int(n) => n.to_string(),
            Value::Closure { .. } => "<closure>".to_string(),
            Value::Constructor { tag, arity, fields } => {
                let name = self
                    .program
                    .constructor_name(*tag)
                    .unwrap_or("<unknown-constructor>");

                if fields.is_empty() {
                    return name.to_string();
                }

                let rendered_args = fields
                    .iter()
                    .map(|&field| self.render_atomic(field))
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

    fn render_atomic(&self, addr: Addr) -> String {
        if self.try_render_nat(addr).is_some()
            || self.try_render_list(addr).is_some()
            || self.try_render_pair(addr).is_some()
        {
            return self.render(addr);
        }

        match self.store.get(addr) {
            Value::Unit => self.render(addr),
            Value::Int(_) => self.render(addr),
            Value::Closure { .. } => self.render(addr),
            Value::Constructor { fields, .. } if fields.is_empty() => self.render(addr),
            _ => format!("({})", self.render(addr)),
        }
    }

    fn try_render_nat(&self, addr: Addr) -> Option<String> {
        let mut n: usize = 0;
        let mut current = addr;

        loop {
            match self.store.get(current) {
                Value::Constructor { tag, arity, fields }
                    if self.program.constructor_name(*tag) == Some("Z")
                        && *arity == 0
                        && fields.is_empty() =>
                {
                    return Some(n.to_string());
                }

                Value::Constructor { tag, arity, fields }
                    if self.program.constructor_name(*tag) == Some("S")
                        && *arity == 1
                        && fields.len() == 1 =>
                {
                    n += 1;
                    current = fields[0];
                }

                _ => return None,
            }
        }
    }

    fn try_render_list(&self, addr: Addr) -> Option<String> {
        let mut items = Vec::new();
        let mut current = addr;

        loop {
            match self.store.get(current) {
                Value::Constructor { tag, arity, fields }
                    if self.program.constructor_name(*tag) == Some("Nil")
                        && *arity == 0
                        && fields.is_empty() =>
                {
                    return Some(format!("[{}]", items.join(", ")));
                }

                Value::Constructor { tag, arity, fields }
                    if self.program.constructor_name(*tag) == Some("Cons")
                        && *arity == 2
                        && fields.len() == 2 =>
                {
                    items.push(self.render(fields[0]));
                    current = fields[1];
                }

                _ => return None,
            }
        }
    }

    fn try_render_pair(&self, addr: Addr) -> Option<String> {
        match self.store.get(addr) {
            Value::Constructor { tag, arity, fields }
                if *arity == 2
                    && (self.program.constructor_name(*tag) == Some("Pair")
                        || self.program.constructor_name(*tag) == Some("MkPair"))
                    && fields.len() == 2 =>
            {
                Some(format!(
                    "({}, {})",
                    self.render(fields[0]),
                    self.render(fields[1])
                ))
            }
            _ => None,
        }
    }
}

#[derive(Clone)]
enum GlobalEntry {
    Value(Addr),
    Expr(Rc<IRExpr>),
    BlackHole,
}

enum Frame {
    EvalArg {
        arg: Rc<IRExpr>,
        env: Env,
    },
    Apply(Addr),
    EvalBinaryR {
        op: BinOp,
        rhs: Rc<IRExpr>,
        env: Env,
    },
    ApplyBinary {
        op: BinOp,
        lhs: Addr,
    },
    ApplyUnary(UnOp),
    CaseOf {
        arms: Rc<[IRCaseArm]>,
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
    UpdateGlobal(usize),
}

struct ConstructorCache {
    nullary: Vec<Option<Addr>>,
    true_addr: Addr,
    false_addr: Addr,
}

impl ConstructorCache {
    fn new(size: usize) -> Self {
        Self {
            nullary: vec![None; size],
            true_addr: Addr(0),
            false_addr: Addr(0),
        }
    }

    fn set_nullary(&mut self, tag: u16, addr: Addr) {
        self.nullary[tag as usize] = Some(addr);
    }

    fn nullary_addr(&self, tag: u16) -> Addr {
        self.nullary[tag as usize]
            .unwrap_or_else(|| panic!("missing nullary constructor cache for tag {}", tag))
    }
}

pub struct Machine {
    control: Rc<IRExpr>,
    env: Env,
    continuation: Vec<Frame>,
    store: Store,
    globals: Vec<GlobalEntry>,
    ctors: ConstructorCache,
    next_gc_threshold: usize,
}

impl Machine {
    pub fn exec(program: &IRProgram, entry_name: &str) -> EvalResult {
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

        let max_tag = program
            .constructor_arities
            .iter()
            .map(|(_, tag, _)| *tag as usize)
            .max()
            .unwrap_or(0);

        let mut machine = Self {
            control: Rc::new(IRExpr::FVar(entry_index)),
            env: Env::empty(),
            continuation: Vec::new(),
            store: Store::default(),
            globals: Vec::with_capacity(program.globals.len()),
            ctors: ConstructorCache::new(max_tag + 1),
            next_gc_threshold: 1024,
        };

        machine.init_constructors(program, true_tag, false_tag);
        machine.init_globals(program);

        let root = machine.run();

        EvalResult {
            root,
            store: machine.store,
        }
    }

    fn init_constructors(&mut self, program: &IRProgram, true_tag: u16, false_tag: u16) {
        for (_, tag, arity) in &program.constructor_arities {
            if *arity == 0 && self.ctors.nullary[*tag as usize].is_none() {
                let addr = self.alloc(Value::Constructor {
                    tag: *tag,
                    arity: *arity,
                    fields: Vec::<Addr>::new().into_boxed_slice(),
                });
                self.ctors.set_nullary(*tag, addr);
            }
        }

        self.ctors.true_addr = self.ctors.nullary_addr(true_tag);
        self.ctors.false_addr = self.ctors.nullary_addr(false_tag);
    }

    fn init_globals(&mut self, program: &IRProgram) {
        for global_def in &program.globals {
            match global_def.body.as_ref() {
                IRExpr::Abs(body) => {
                    let addr = self.alloc(Value::Closure {
                        env: Env::empty(),
                        body: body.clone(),
                    });
                    self.globals.push(GlobalEntry::Value(addr));
                }
                _ => self
                    .globals
                    .push(GlobalEntry::Expr(global_def.body.clone())),
            }
        }
    }

    fn alloc(&mut self, value: Value) -> Addr {
        let addr = self.store.alloc(value);
        self.maybe_collect();
        addr
    }

    fn maybe_collect(&mut self) {
        if self.store.len() < self.next_gc_threshold {
            return;
        }

        self.next_gc_threshold = (self.next_gc_threshold.max(1024) * 2).max(self.store.len() + 1);
    }

    fn run(&mut self) -> Addr {
        loop {
            let value = self.eval_current();
            match self.resume(value) {
                Some(result) => return result,
                None => continue,
            }
        }
    }

    fn eval_current(&mut self) -> Addr {
        loop {
            match self.control.as_ref() {
                IRExpr::Unit => return self.alloc(Value::Unit),

                IRExpr::Int(n) => return self.alloc(Value::Int(*n)),

                IRExpr::BVar(index) => return self.env.lookup(*index),

                IRExpr::FVar(index) => return self.force_global(*index),

                IRExpr::Con { tag, arity } => return self.eval_constructor(*tag, *arity),

                IRExpr::Abs(body) => {
                    return self.alloc(Value::Closure {
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
                }

                IRExpr::Let(binding, body) => {
                    self.continuation.push(Frame::LetBody {
                        body: body.clone(),
                        env: self.env.clone(),
                    });
                    self.control = binding.clone();
                }

                IRExpr::Case { scrutinee, arms } => {
                    self.continuation.push(Frame::CaseOf {
                        arms: arms.clone().into(),
                        env: self.env.clone(),
                    });
                    self.control = scrutinee.clone();
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
                }

                IRExpr::BinaryOp(op, lhs, rhs) => {
                    self.continuation.push(Frame::EvalBinaryR {
                        op: op.clone(),
                        rhs: rhs.clone(),
                        env: self.env.clone(),
                    });
                    self.control = lhs.clone();
                }

                IRExpr::UnaryOp(op, rhs) => {
                    self.continuation.push(Frame::ApplyUnary(op.clone()));
                    self.control = rhs.clone();
                }
            }
        }
    }

    fn resume(&mut self, mut value: Addr) -> Option<Addr> {
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
                    Some(next_value) => value = next_value,
                    None => return None,
                },

                Some(Frame::EvalBinaryR { op, rhs, env }) => {
                    self.continuation
                        .push(Frame::ApplyBinary { op, lhs: value });
                    self.control = rhs;
                    self.env = env;
                    return None;
                }

                Some(Frame::ApplyBinary { op, lhs }) => {
                    value = self.eval_binary_prim(&op, lhs, value);
                }

                Some(Frame::ApplyUnary(op)) => {
                    value = self.eval_unary_prim(&op, value);
                }

                Some(Frame::LetBody { body, env }) => {
                    self.env = env.extend(value);
                    self.control = body;
                    return None;
                }

                Some(Frame::CaseOf { arms, env }) => {
                    for arm in arms.iter() {
                        let mut bindings = Vec::new();
                        if self.match_pattern(&arm.pattern, value, &mut bindings) {
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

                    if value == self.ctors.true_addr {
                        self.control = then_br;
                        return None;
                    }

                    if value == self.ctors.false_addr {
                        self.control = else_br;
                        return None;
                    }

                    panic!("if-condition is not a boolean");
                }

                Some(Frame::UpdateGlobal(index)) => {
                    self.globals[index] = GlobalEntry::Value(value);
                }
            }
        }
    }

    fn force_global(&mut self, index: usize) -> Addr {
        match self.globals[index].clone() {
            GlobalEntry::Value(addr) => addr,
            GlobalEntry::Expr(expr) => {
                self.globals[index] = GlobalEntry::BlackHole;
                self.continuation.push(Frame::UpdateGlobal(index));
                self.control = expr;
                self.env = Env::empty();
                self.eval_current()
            }
            GlobalEntry::BlackHole => {
                panic!("cyclic evaluation of global at index {}", index)
            }
        }
    }

    fn eval_constructor(&mut self, tag: u16, arity: u16) -> Addr {
        if arity == 0 {
            return self.ctors.nullary_addr(tag);
        }

        self.alloc(Value::Constructor {
            tag,
            arity,
            fields: Vec::<Addr>::new().into_boxed_slice(),
        })
    }

    fn apply(&mut self, function: Addr, argument: Addr) -> Option<Addr> {
        match self.store.get(function) {
            Value::Closure { env, body } => {
                self.env = env.extend(argument);
                self.control = body.clone();
                None
            }

            Value::Constructor { tag, arity, fields } => {
                if fields.len() >= *arity as usize {
                    panic!("cannot over-apply constructor tag {}", tag);
                }

                let mut new_fields = Vec::with_capacity(fields.len() + 1);
                new_fields.extend_from_slice(fields);
                new_fields.push(argument);

                Some(self.alloc(Value::Constructor {
                    tag: *tag,
                    arity: *arity,
                    fields: new_fields.into_boxed_slice(),
                }))
            }

            _ => panic!("cannot apply non-function"),
        }
    }

    fn expect_int(&self, addr: Addr) -> i64 {
        match self.store.get(addr) {
            Value::Int(n) => *n,
            _ => panic!("expected Int value"),
        }
    }

    fn eval_unary_prim(&mut self, op: &UnOp, rhs: Addr) -> Addr {
        let rhs = self.expect_int(rhs);

        match op {
            UnOp::Neg => self.alloc(Value::Int(-rhs)),
        }
    }

    fn eval_binary_prim(&mut self, op: &BinOp, lhs: Addr, rhs: Addr) -> Addr {
        let lhs = self.expect_int(lhs);
        let rhs = self.expect_int(rhs);

        match op {
            BinOp::Plus => self.alloc(Value::Int(lhs + rhs)),
            BinOp::Minus => self.alloc(Value::Int(lhs - rhs)),
            BinOp::Mult => self.alloc(Value::Int(lhs * rhs)),
            BinOp::Div => self.alloc(Value::Int(lhs / rhs)),

            BinOp::Less => self.bool_addr(lhs < rhs),
            BinOp::Greater => self.bool_addr(lhs > rhs),
            BinOp::LessOrEqual => self.bool_addr(lhs <= rhs),
            BinOp::GreaterOrEqual => self.bool_addr(lhs >= rhs),
            BinOp::Equal => self.bool_addr(lhs == rhs),
        }
    }

    fn bool_addr(&self, value: bool) -> Addr {
        if value {
            self.ctors.true_addr
        } else {
            self.ctors.false_addr
        }
    }

    fn match_pattern(&self, pattern: &IRPattern, value: Addr, bindings: &mut Vec<Addr>) -> bool {
        let mut work: Vec<(&IRPattern, Addr)> = vec![(pattern, value)];

        while let Some((pat, addr)) = work.pop() {
            match (pat, self.store.get(addr)) {
                (IRPattern::Var, _) => bindings.push(addr),

                (IRPattern::Wild, _) => {}

                (
                    IRPattern::Con(expected_tag, sub_patterns),
                    Value::Constructor { tag, fields, .. },
                ) => {
                    if *expected_tag != *tag || sub_patterns.len() != fields.len() {
                        return false;
                    }

                    for (sub_pat, &field_addr) in sub_patterns.iter().zip(fields.iter()).rev() {
                        work.push((sub_pat, field_addr));
                    }
                }

                _ => return false,
            }
        }

        true
    }
}
