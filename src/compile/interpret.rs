use core::ast::{BinOp, UnOp};
use std::fmt;
use std::rc::Rc;

use super::lower_ast::{IRCaseArm, IRExpr, IRPattern, IRProgram};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Addr(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Value {
    Unit,
    Int(i64),
    Heap(Addr),
}

#[derive(Debug, Clone)]
enum Env {
    Empty,
    Extend { value: Value, parent: Rc<Env> },
}

impl Env {
    #[inline]
    fn empty() -> Self {
        Env::Empty
    }

    #[inline]
    fn extend(&self, value: Value) -> Self {
        Env::Extend {
            value,
            parent: Rc::new(self.clone()),
        }
    }

    fn extend_many(&self, values: &[Value]) -> Self {
        let mut env = self.clone();

        // Keep forward order.
        // If values are [x, y], then y must become the most recent binder,
        // so that BVar(0) still resolves to y.
        for &value in values {
            env = env.extend(value);
        }

        env
    }

    #[inline]
    fn lookup(&self, index: usize) -> Value {
        let mut current = self;
        let mut remaining = index;

        loop {
            match current {
                Env::Empty => {
                    panic!("unbound variable at index {}", index);
                }
                Env::Extend { value, parent } => {
                    if remaining == 0 {
                        return *value;
                    }
                    remaining -= 1;
                    current = parent.as_ref();
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
enum HeapObject {
    Closure {
        env: Env,
        body: Rc<IRExpr>,
    },
    Constructor {
        tag: u16,
        arity: u16,
        fields: Box<[Value]>,
    },
}

#[derive(Debug, Default)]
struct Store {
    objects: Vec<HeapObject>,
}

impl Store {
    #[inline]
    fn alloc(&mut self, object: HeapObject) -> Addr {
        let addr = Addr(self.objects.len());
        self.objects.push(object);
        addr
    }

    #[inline]
    fn get(&self, addr: Addr) -> &HeapObject {
        self.objects
            .get(addr.0)
            .unwrap_or_else(|| panic!("invalid store address {}", addr.0))
    }

    #[inline]
    fn len(&self) -> usize {
        self.objects.len()
    }
}

pub struct EvalResult {
    root: Value,
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
    root: Value,
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

fn base_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

fn constructor_name_by_tag<'a>(program: &'a IRProgram, tag: u16) -> &'a str {
    program
        .constructor_name(tag)
        .unwrap_or("<unknown-constructor>")
}

fn constructor_has_base_name(program: &IRProgram, tag: u16, expected: &str) -> bool {
    base_name(constructor_name_by_tag(program, tag)) == expected
}

fn constructor_tag_by_full_name(program: &IRProgram, expected: &str) -> Option<u16> {
    program.constructor_tag(expected)
}

fn constructor_tag_by_base_name(program: &IRProgram, expected: &str) -> Option<u16> {
    let mut found: Option<u16> = None;

    for (name, tag, _) in &program.constructor_arities {
        if base_name(name) == expected {
            if let Some(prev) = found {
                if prev != *tag {
                    panic!(
                        "ambiguous constructor base name '{}': multiple qualified constructors share it",
                        expected
                    );
                }
            } else {
                found = Some(*tag);
            }
        }
    }

    found
}

struct Renderer<'a> {
    store: &'a Store,
    program: &'a IRProgram,
}

impl Renderer<'_> {
    fn render(&self, value: Value) -> String {
        if let Some(rendered) = self.try_render_nat(value) {
            return rendered;
        }

        if let Some(rendered) = self.try_render_list(value) {
            return rendered;
        }

        if let Some(rendered) = self.try_render_pair(value) {
            return rendered;
        }

        match value {
            Value::Unit => "()".to_string(),
            Value::Int(n) => n.to_string(),
            Value::Heap(addr) => match self.store.get(addr) {
                HeapObject::Closure { .. } => "<closure>".to_string(),
                HeapObject::Constructor { tag, arity, fields } => {
                    let name = base_name(constructor_name_by_tag(self.program, *tag));

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
            },
        }
    }

    fn render_atomic(&self, value: Value) -> String {
        if self.try_render_nat(value).is_some()
            || self.try_render_list(value).is_some()
            || self.try_render_pair(value).is_some()
        {
            return self.render(value);
        }

        match value {
            Value::Unit | Value::Int(_) => self.render(value),
            Value::Heap(addr) => match self.store.get(addr) {
                HeapObject::Closure { .. } => self.render(value),
                HeapObject::Constructor { fields, .. } if fields.is_empty() => self.render(value),
                _ => format!("({})", self.render(value)),
            },
        }
    }

    fn try_render_nat(&self, value: Value) -> Option<String> {
        let mut n: usize = 0;
        let mut current = value;

        loop {
            match current {
                Value::Heap(addr) => match self.store.get(addr) {
                    HeapObject::Constructor { tag, arity, fields }
                        if constructor_has_base_name(self.program, *tag, "Z")
                            && *arity == 0
                            && fields.is_empty() =>
                    {
                        return Some(n.to_string());
                    }

                    HeapObject::Constructor { tag, arity, fields }
                        if constructor_has_base_name(self.program, *tag, "S")
                            && *arity == 1
                            && fields.len() == 1 =>
                    {
                        n += 1;
                        current = fields[0];
                    }

                    _ => return None,
                },
                _ => return None,
            }
        }
    }

    fn try_render_list(&self, value: Value) -> Option<String> {
        let mut items = Vec::new();
        let mut current = value;

        loop {
            match current {
                Value::Heap(addr) => match self.store.get(addr) {
                    HeapObject::Constructor { tag, arity, fields }
                        if constructor_has_base_name(self.program, *tag, "Nil")
                            && *arity == 0
                            && fields.is_empty() =>
                    {
                        return Some(format!("[{}]", items.join(", ")));
                    }

                    HeapObject::Constructor { tag, arity, fields }
                        if constructor_has_base_name(self.program, *tag, "Cons")
                            && *arity == 2
                            && fields.len() == 2 =>
                    {
                        items.push(self.render(fields[0]));
                        current = fields[1];
                    }

                    _ => return None,
                },
                _ => return None,
            }
        }
    }

    fn try_render_pair(&self, value: Value) -> Option<String> {
        match value {
            Value::Heap(addr) => match self.store.get(addr) {
                HeapObject::Constructor { tag, arity, fields }
                    if *arity == 2
                        && (constructor_has_base_name(self.program, *tag, "Pair")
                            || constructor_has_base_name(self.program, *tag, "MkPair"))
                        && fields.len() == 2 =>
                {
                    Some(format!(
                        "({}, {})",
                        self.render(fields[0]),
                        self.render(fields[1])
                    ))
                }
                _ => None,
            },
            _ => None,
        }
    }
}

#[derive(Clone)]
enum GlobalEntry {
    Value(Value),
    Expr(Rc<IRExpr>),
    BlackHole,
}

enum Frame {
    EvalArg {
        arg: Rc<IRExpr>,
        env: Env,
    },
    Apply(Value),
    EvalBinaryR {
        op: BinOp,
        rhs: Rc<IRExpr>,
        env: Env,
    },
    ApplyBinary {
        op: BinOp,
        lhs: Value,
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
    nullary: Vec<Option<Value>>,
    true_value: Option<Value>,
    false_value: Option<Value>,
}

impl ConstructorCache {
    fn new(size: usize) -> Self {
        Self {
            nullary: vec![None; size],
            true_value: None,
            false_value: None,
        }
    }

    fn set_nullary(&mut self, tag: u16, value: Value) {
        self.nullary[tag as usize] = Some(value);
    }

    fn nullary_value(&self, tag: u16) -> Value {
        self.nullary[tag as usize]
            .unwrap_or_else(|| panic!("missing nullary constructor cache for tag {}", tag))
    }

    fn true_value(&self) -> Value {
        self.true_value
            .unwrap_or_else(|| panic!("missing cached True constructor"))
    }

    fn false_value(&self) -> Value {
        self.false_value
            .unwrap_or_else(|| panic!("missing cached False constructor"))
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

        let true_tag = constructor_tag_by_full_name(program, "prelude.builtins.True")
            .or_else(|| constructor_tag_by_base_name(program, "True"))
            .unwrap_or_else(|| panic!("constructor 'prelude.builtins.True' not found"));

        let false_tag = constructor_tag_by_full_name(program, "prelude.builtins.False")
            .or_else(|| constructor_tag_by_base_name(program, "False"))
            .unwrap_or_else(|| panic!("constructor 'prelude.builtins.False' not found"));

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
                let value = self.alloc_heap(HeapObject::Constructor {
                    tag: *tag,
                    arity: *arity,
                    fields: Vec::<Value>::new().into_boxed_slice(),
                });
                self.ctors.set_nullary(*tag, value);
            }
        }

        self.ctors.true_value = Some(self.ctors.nullary_value(true_tag));
        self.ctors.false_value = Some(self.ctors.nullary_value(false_tag));
    }

    fn init_globals(&mut self, program: &IRProgram) {
        for global_def in &program.globals {
            match global_def.body.as_ref() {
                IRExpr::Abs(body) => {
                    let value = self.alloc_heap(HeapObject::Closure {
                        env: Env::empty(),
                        body: body.clone(),
                    });
                    self.globals.push(GlobalEntry::Value(value));
                }
                _ => self
                    .globals
                    .push(GlobalEntry::Expr(global_def.body.clone())),
            }
        }
    }

    #[inline]
    fn alloc_heap(&mut self, object: HeapObject) -> Value {
        let addr = self.store.alloc(object);
        self.maybe_collect();
        Value::Heap(addr)
    }

    fn maybe_collect(&mut self) {
        // Placeholder hook for a future GC.
        // Right now this only tracks growth and gives you one place to insert collection later.
        if self.store.len() < self.next_gc_threshold {
            return;
        }

        self.next_gc_threshold = (self.next_gc_threshold.max(1024) * 2).max(self.store.len() + 1);
    }

    fn run(&mut self) -> Value {
        loop {
            let value = self.eval_current();
            match self.resume(value) {
                Some(result) => return result,
                None => continue,
            }
        }
    }

    fn eval_current(&mut self) -> Value {
        loop {
            match self.control.as_ref() {
                IRExpr::Unit => return Value::Unit,
                IRExpr::Int(n) => return Value::Int(*n),
                IRExpr::BVar(index) => return self.env.lookup(*index),
                IRExpr::FVar(index) => return self.force_global(*index),
                IRExpr::Con { tag, arity } => return self.eval_constructor(*tag, *arity),

                IRExpr::Abs(body) => {
                    return self.alloc_heap(HeapObject::Closure {
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

    fn resume(&mut self, mut value: Value) -> Option<Value> {
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
                        let mut bindings = Vec::with_capacity(arm.pattern.binder_count());
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

                    if value == self.ctors.true_value() {
                        self.control = then_br;
                        return None;
                    }

                    if value == self.ctors.false_value() {
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

    fn force_global(&mut self, index: usize) -> Value {
        match self.globals[index].clone() {
            GlobalEntry::Value(value) => value,
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

    #[inline]
    fn eval_constructor(&mut self, tag: u16, arity: u16) -> Value {
        if arity == 0 {
            return self.ctors.nullary_value(tag);
        }

        self.alloc_heap(HeapObject::Constructor {
            tag,
            arity,
            fields: Vec::<Value>::new().into_boxed_slice(),
        })
    }

    fn apply(&mut self, function: Value, argument: Value) -> Option<Value> {
        let addr = match function {
            Value::Heap(addr) => addr,
            _ => panic!("cannot apply non-function"),
        };

        match self.store.get(addr) {
            HeapObject::Closure { env, body } => {
                self.env = env.extend(argument);
                self.control = body.clone();
                None
            }

            HeapObject::Constructor { tag, arity, fields } => {
                if fields.len() >= *arity as usize {
                    panic!("cannot over-apply constructor tag {}", tag);
                }

                let mut new_fields = Vec::with_capacity(fields.len() + 1);
                new_fields.extend_from_slice(fields);
                new_fields.push(argument);

                Some(self.alloc_heap(HeapObject::Constructor {
                    tag: *tag,
                    arity: *arity,
                    fields: new_fields.into_boxed_slice(),
                }))
            }
        }
    }

    #[inline]
    fn expect_int(&self, value: Value) -> i64 {
        match value {
            Value::Int(n) => n,
            _ => panic!("expected Int value"),
        }
    }

    #[inline]
    fn eval_unary_prim(&mut self, op: &UnOp, rhs: Value) -> Value {
        let rhs = self.expect_int(rhs);

        match op {
            UnOp::Neg => Value::Int(-rhs),
        }
    }

    #[inline]
    fn eval_binary_prim(&mut self, op: &BinOp, lhs: Value, rhs: Value) -> Value {
        let lhs = self.expect_int(lhs);
        let rhs = self.expect_int(rhs);

        match op {
            BinOp::Plus => Value::Int(lhs + rhs),
            BinOp::Minus => Value::Int(lhs - rhs),
            BinOp::Mult => Value::Int(lhs * rhs),
            BinOp::Div => Value::Int(lhs / rhs),
            BinOp::Mod => Value::Int(lhs % rhs),

            BinOp::Less => self.bool_value(lhs < rhs),
            BinOp::Greater => self.bool_value(lhs > rhs),
            BinOp::LessOrEqual => self.bool_value(lhs <= rhs),
            BinOp::GreaterOrEqual => self.bool_value(lhs >= rhs),
            BinOp::Equal => self.bool_value(lhs == rhs),
        }
    }

    #[inline]
    fn bool_value(&self, value: bool) -> Value {
        if value {
            self.ctors.true_value()
        } else {
            self.ctors.false_value()
        }
    }

    fn match_pattern(&self, pattern: &IRPattern, value: Value, bindings: &mut Vec<Value>) -> bool {
        let mut work: Vec<(&IRPattern, Value)> = vec![(pattern, value)];

        while let Some((pat, value)) = work.pop() {
            match pat {
                IRPattern::Var => bindings.push(value),
                IRPattern::Wild => {}
                IRPattern::Con(expected_tag, sub_patterns) => {
                    let addr = match value {
                        Value::Heap(addr) => addr,
                        _ => return false,
                    };

                    match self.store.get(addr) {
                        HeapObject::Constructor { tag, fields, .. } => {
                            if *expected_tag != *tag || sub_patterns.len() != fields.len() {
                                return false;
                            }

                            for (sub_pat, &field_value) in
                                sub_patterns.iter().zip(fields.iter()).rev()
                            {
                                work.push((sub_pat, field_value));
                            }
                        }
                        HeapObject::Closure { .. } => return false,
                    }
                }
            }
        }

        true
    }
}
