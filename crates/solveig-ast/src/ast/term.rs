//! Checked/core term syntax, patterns, and per-module term containers.
//! Term annotations are generic so the checker can preserve span and type metadata.

use solveig_base::FxHashMap;

use solveig_base::Symbol;

use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Ev {
    Var(EvVarId),
    Placeholder(EvVarId),
    Inst(InstanceId, Vec<Ty>, Vec<Ev>),
    Super(Box<Ev>, ClassId, usize),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum VarRef {
    Local(Symbol),
    Global(DefId),
}

impl VarRef {
    pub fn named_local(names: &mut Names, name: &str) -> Self {
        Self::Local(names.symbol(name))
    }

    pub fn named_global(names: &mut Names, name: &str) -> Self {
        Self::Global(names.def_id(name))
    }

    pub fn display_name(&self, names: &Names) -> String {
        names.var_display_name(self)
    }
}

#[derive(Clone, Debug)]
pub struct ExprNode<T, A> {
    pub node: T,
    pub anno: A,
}

impl<T, A> ExprNode<T, A> {
    pub fn new(node: T, anno: A) -> Self {
        Self { node, anno }
    }
}

pub type TermExprNode<A, P = A> = ExprNode<TermExpr<A, P>, A>;
pub type PatternNode<A> = ExprNode<Pattern<A>, A>;
pub type CaseArmNode<A, P = A> = ExprNode<CaseArm<A, P>, P>;
pub type MatchArmNode<A, P = A> = ExprNode<MatchArm<A, P>, P>;

impl<A, P> ExprNode<TermExpr<A, P>, A> {
    pub fn map_anno<B, Q, FE, FP>(
        self,
        expr_anno: &mut FE,
        pattern_anno: &mut FP,
    ) -> ExprNode<TermExpr<B, Q>, B>
    where
        FE: FnMut(A) -> B,
        FP: FnMut(P) -> Q,
    {
        let anno = expr_anno(self.anno);
        let node = match self.node {
            TermExpr::Unit => TermExpr::Unit,
            TermExpr::Var(var) => TermExpr::Var(var),
            TermExpr::Lit(lit) => TermExpr::Lit(lit),
            TermExpr::Hole => TermExpr::Hole,
            TermExpr::App(func, arg) => TermExpr::App(
                Box::new(func.map_anno(expr_anno, pattern_anno)),
                Box::new(arg.map_anno(expr_anno, pattern_anno)),
            ),
            TermExpr::Abs(param, annotation, body) => TermExpr::Abs(
                param,
                annotation,
                Box::new(body.map_anno(expr_anno, pattern_anno)),
            ),
            TermExpr::TyAbs(param, kind, body) => TermExpr::TyAbs(
                param,
                kind,
                Box::new(body.map_anno(expr_anno, pattern_anno)),
            ),
            TermExpr::TyApp(inner, ty) => {
                TermExpr::TyApp(Box::new(inner.map_anno(expr_anno, pattern_anno)), ty)
            }
            TermExpr::EvAbs(ev, pred, body) => {
                TermExpr::EvAbs(ev, pred, Box::new(body.map_anno(expr_anno, pattern_anno)))
            }
            TermExpr::EvApp(inner, ev) => {
                TermExpr::EvApp(Box::new(inner.map_anno(expr_anno, pattern_anno)), ev)
            }
            TermExpr::Anno(inner, ty) => {
                TermExpr::Anno(Box::new(inner.map_anno(expr_anno, pattern_anno)), ty)
            }
            TermExpr::Con(id) => TermExpr::Con(id),
            TermExpr::Case(scrutinee, arms) => TermExpr::Case(
                Box::new(scrutinee.map_anno(expr_anno, pattern_anno)),
                arms.into_iter()
                    .map(|arm| arm.map_anno(expr_anno, pattern_anno))
                    .collect(),
            ),
            TermExpr::Match(scrutinees, arms) => TermExpr::Match(
                scrutinees
                    .into_iter()
                    .map(|scrutinee| scrutinee.map_anno(expr_anno, pattern_anno))
                    .collect(),
                arms.into_iter()
                    .map(|arm| arm.map_anno(expr_anno, pattern_anno))
                    .collect(),
            ),
            TermExpr::IfThenElse(cond, then_branch, else_branch) => TermExpr::IfThenElse(
                Box::new(cond.map_anno(expr_anno, pattern_anno)),
                Box::new(then_branch.map_anno(expr_anno, pattern_anno)),
                Box::new(else_branch.map_anno(expr_anno, pattern_anno)),
            ),
            TermExpr::Let(name, binder, in_expr) => TermExpr::Let(
                name,
                Box::new(binder.map_anno(expr_anno, pattern_anno)),
                Box::new(in_expr.map_anno(expr_anno, pattern_anno)),
            ),
        };

        ExprNode::new(node, anno)
    }
}

impl<A> ExprNode<Pattern<A>, A> {
    pub fn map_anno<B, F>(self, anno: &mut F) -> ExprNode<Pattern<B>, B>
    where
        F: FnMut(A) -> B,
    {
        let mapped_anno = anno(self.anno);
        let node = match self.node {
            Pattern::Con(id, args) => {
                Pattern::Con(id, args.into_iter().map(|arg| arg.map_anno(anno)).collect())
            }
            Pattern::Lit(lit) => Pattern::Lit(lit),
            Pattern::Var(name) => Pattern::Var(name),
            Pattern::Wild => Pattern::Wild,
        };

        ExprNode::new(node, mapped_anno)
    }
}

impl<A, P> ExprNode<CaseArm<A, P>, P> {
    pub fn map_anno<B, Q, FE, FP>(
        self,
        expr_anno: &mut FE,
        pattern_anno: &mut FP,
    ) -> ExprNode<CaseArm<B, Q>, Q>
    where
        FE: FnMut(A) -> B,
        FP: FnMut(P) -> Q,
    {
        let anno = pattern_anno(self.anno);
        let node = CaseArm {
            pattern: self.node.pattern.map_anno(pattern_anno),
            expr: self.node.expr.map_anno(expr_anno, pattern_anno),
        };

        ExprNode::new(node, anno)
    }
}

impl<A, P> ExprNode<MatchArm<A, P>, P> {
    pub fn map_anno<B, Q, FE, FP>(
        self,
        expr_anno: &mut FE,
        pattern_anno: &mut FP,
    ) -> ExprNode<MatchArm<B, Q>, Q>
    where
        FE: FnMut(A) -> B,
        FP: FnMut(P) -> Q,
    {
        let anno = pattern_anno(self.anno);
        let node = MatchArm {
            patterns: self
                .node
                .patterns
                .into_iter()
                .map(|pattern| pattern.map_anno(pattern_anno))
                .collect(),
            expr: self.node.expr.map_anno(expr_anno, pattern_anno),
        };

        ExprNode::new(node, anno)
    }
}

#[derive(Clone, Debug)]
pub enum Literal {
    Int(i64),
    Float(f64),
    Char(char),
    Str(StrLitId),
}

#[derive(Clone, Debug, Copy)]
pub enum BinOp {
    Plus,
    Minus,
    Mult,
    Div,
    Mod,

    Less,
    Greater,
    LessOrEqual,
    GreaterOrEqual,
    Equal,
}

#[derive(Clone, Debug, Copy)]
pub enum UnOp {
    Neg,
}

#[derive(Clone, Debug)]
pub enum TermExpr<A, P = A> {
    Unit,
    Var(VarRef),
    Lit(Literal),
    Hole,
    App(Box<TermExprNode<A, P>>, Box<TermExprNode<A, P>>),
    Abs(String, Option<Ty>, Box<TermExprNode<A, P>>),
    TyAbs(String, Kind, Box<TermExprNode<A, P>>),
    TyApp(Box<TermExprNode<A, P>>, Ty),
    EvAbs(EvVarId, Predicate, Box<TermExprNode<A, P>>),
    EvApp(Box<TermExprNode<A, P>>, Ev),
    Anno(Box<TermExprNode<A, P>>, Ty),
    Con(CtorId),
    Case(Box<TermExprNode<A, P>>, Vec<CaseArmNode<A, P>>),
    Match(Vec<TermExprNode<A, P>>, Vec<MatchArmNode<A, P>>),
    IfThenElse(
        Box<TermExprNode<A, P>>,
        Box<TermExprNode<A, P>>,
        Box<TermExprNode<A, P>>,
    ),
    Let(String, Box<TermExprNode<A, P>>, Box<TermExprNode<A, P>>),
}

#[derive(Clone, Debug)]
pub enum Pattern<A> {
    Con(CtorId, Vec<PatternNode<A>>),
    Lit(Literal),
    Var(String),
    Wild,
}

#[derive(Clone, Debug)]
pub struct CaseArm<A, P = A> {
    pub pattern: PatternNode<P>,
    pub expr: TermExprNode<A, P>,
}

#[derive(Clone, Debug)]
pub struct MatchArm<A, P = A> {
    pub patterns: Vec<PatternNode<P>>,
    pub expr: TermExprNode<A, P>,
}

#[derive(Clone, Debug)]
pub struct TermDefinition<A, P = A> {
    pub id: DefId,
    pub name: String,
    pub short_name: String,
    pub signature: Option<Scheme>,
    pub body: TermExprNode<A, P>,
}

#[derive(Clone, Debug)]
pub struct BuiltinDefinition {
    pub id: DefId,
    pub name: String,
    pub short_name: String,
    pub builtin: BuiltinId,
    pub arity: usize,
    pub scheme: Scheme,
}

impl<A, P> TermDefinition<A, P> {
    pub fn map_anno<B, Q, FE, FP>(
        self,
        expr_anno: &mut FE,
        pattern_anno: &mut FP,
    ) -> TermDefinition<B, Q>
    where
        FE: FnMut(A) -> B,
        FP: FnMut(P) -> Q,
    {
        TermDefinition {
            id: self.id,
            name: self.name,
            short_name: self.short_name,
            signature: self.signature,
            body: self.body.map_anno(expr_anno, pattern_anno),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedModule<A, P = A> {
    pub data_defs: Vec<DataDefinition>,
    pub class_defs: Vec<ClassDefinition>,
    pub instance_defs: Vec<InstanceDefinition<A, P>>,
    pub builtin_defs: Vec<BuiltinDefinition>,
    pub tycon_table: TyConTable,
    pub ctor_table: CtorTable,
    pub class_table: ClassTable,
    pub tycon_kinds: FxHashMap<TyConId, Kind>,
    pub constructor_types: FxHashMap<CtorId, Ty>,
    pub func_schemes: FxHashMap<DefId, Scheme>,
    pub func_defs: Vec<TermDefinition<A, P>>,
}

impl<A, P> ResolvedModule<A, P> {
    pub fn rebuild_identity_tables(&mut self, names: &mut Names) {
        let (tycon_table, ctor_table, class_table) =
            identity_tables_from_defs(names, &self.data_defs, &self.class_defs);

        self.tycon_table = tycon_table;
        self.ctor_table = ctor_table;
        self.class_table = class_table;
    }

    pub fn map_anno<B, Q, FE, FP>(
        self,
        expr_anno: &mut FE,
        pattern_anno: &mut FP,
    ) -> ResolvedModule<B, Q>
    where
        FE: FnMut(A) -> B,
        FP: FnMut(P) -> Q,
    {
        ResolvedModule {
            data_defs: self.data_defs,
            class_defs: self.class_defs,
            instance_defs: self
                .instance_defs
                .into_iter()
                .map(|inst| inst.map_anno(expr_anno, pattern_anno))
                .collect(),
            builtin_defs: self.builtin_defs,
            tycon_table: self.tycon_table,
            ctor_table: self.ctor_table,
            class_table: self.class_table,
            tycon_kinds: self.tycon_kinds,
            constructor_types: self.constructor_types,
            func_schemes: self.func_schemes,
            func_defs: self
                .func_defs
                .into_iter()
                .map(|def| def.map_anno(expr_anno, pattern_anno))
                .collect(),
        }
    }
}
