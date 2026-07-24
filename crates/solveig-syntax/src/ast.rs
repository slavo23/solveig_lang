use solveig_base::{FileId, Session, Span, Symbol};

pub type Spanned<T> = (T, Span);

pub type SpannedWrapper<T> = Spanned<T>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NameRef {
    pub segments: Vec<Symbol>,
}

impl NameRef {
    pub fn last(&self) -> Symbol {
        *self
            .segments
            .last()
            .expect("ICE: NameRef must have at least one segment")
    }

    pub fn last_text(&self, session: &Session) -> &'static str {
        session.str(self.last())
    }

    pub fn prefix(&self) -> &[Symbol] {
        &self.segments[..self.segments.len().saturating_sub(1)]
    }

    pub fn display(&self, session: &Session) -> String {
        self.segments
            .iter()
            .map(|segment| session.str(*segment))
            .collect::<Vec<_>>()
            .join(".")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModuleName(pub Vec<Symbol>);

impl ModuleName {
    pub fn as_string(&self, session: &Session) -> String {
        self.0
            .iter()
            .map(|segment| session.str(*segment))
            .collect::<Vec<_>>()
            .join(".")
    }

    pub fn default_alias(&self) -> Symbol {
        *self
            .0
            .last()
            .expect("ICE: ModuleName must have at least one segment")
    }
}

#[derive(Debug, Clone)]
pub enum ImportSpec {
    All,
    Only(Vec<Symbol>),
}

#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub module: ModuleName,
    pub alias: Symbol,
    pub spec: ImportSpec,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ParsedModule {
    pub file: FileId,
    pub imports: Vec<ImportDecl>,
    pub defs: SurfaceProgram,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SurfacePredicate {
    pub class_name: Spanned<NameRef>,
    pub args: Vec<Spanned<SurfaceTy>>,
}

#[derive(Debug, Clone)]
pub struct ClassMethod {
    pub name: Spanned<Symbol>,
    pub ty: Spanned<SurfaceTy>,
}

#[derive(Debug, Clone)]
pub struct InstanceMethod {
    pub name: Spanned<Symbol>,
    pub params: Vec<Spanned<Symbol>>,
    pub body: Spanned<SurfaceExpr>,
}

#[derive(Debug, Clone)]
pub enum SurfaceDefinition {
    FunDef {
        name: Spanned<Symbol>,
        body: Spanned<SurfaceExpr>,
        params: Vec<Spanned<Symbol>>,
    },
    SigDef {
        name: Spanned<Symbol>,
        ty: Spanned<SurfaceTy>,
    },
    DataDef {
        name: Spanned<Symbol>,
        ty_params: Vec<Spanned<Symbol>>,
        constructors: Vec<Spanned<SufraceConstructor>>,
    },
    ClassDef {
        name: Spanned<Symbol>,
        ty_params: Vec<Spanned<Symbol>>,
        superclasses: Vec<Spanned<SurfacePredicate>>,
        methods: Vec<ClassMethod>,
        defaults: Vec<InstanceMethod>,
    },
    InstanceDef {
        context: Vec<Spanned<SurfacePredicate>>,
        class_name: Spanned<Symbol>,
        args: Vec<Spanned<SurfaceTy>>,
        methods: Vec<InstanceMethod>,
    },
}

#[derive(Debug, Clone)]
pub struct SufraceConstructor {
    pub name: Spanned<Symbol>,
    pub params: Vec<Spanned<SurfaceTy>>,
}

pub type SurfaceProgram = Vec<Spanned<SurfaceDefinition>>;

#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceTy {
    Unit,
    Var(Symbol),
    Arr(Box<Spanned<SurfaceTy>>, Box<Spanned<SurfaceTy>>),
    ForAll(Vec<Spanned<Symbol>>, Box<Spanned<SurfaceTy>>),
    Qualified(Vec<Spanned<SurfacePredicate>>, Box<Spanned<SurfaceTy>>),
    Con(Spanned<NameRef>),
    App(Box<Spanned<SurfaceTy>>, Box<Spanned<SurfaceTy>>),
}
#[derive(Clone, Debug, PartialEq)]
pub enum SurfacePattern {
    Var(Symbol),
    Lit(SurfaceLiteral),
    Con(Spanned<NameRef>, Vec<Spanned<SurfacePattern>>),
    Wild,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceMatchArm {
    pub pattern: Spanned<SurfacePattern>,
    pub expr: Spanned<SurfaceExpr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceBinOp {
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

#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceUnOp {
    Neg,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceLiteral {
    Int(i64),
    Float(f64),
    Char(char),
    Str(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceExpr {
    Unit,
    Literal(SurfaceLiteral),
    Hole,
    Var(Spanned<NameRef>),
    App(Box<Spanned<SurfaceExpr>>, Box<Spanned<SurfaceExpr>>),
    Abs(
        Vec<(Spanned<Symbol>, Option<Spanned<SurfaceTy>>)>,
        Box<Spanned<SurfaceExpr>>,
    ),
    Ann(Box<Spanned<SurfaceExpr>>, Spanned<SurfaceTy>),
    Let(
        Spanned<Symbol>,
        Box<Spanned<SurfaceExpr>>,
        Box<Spanned<SurfaceExpr>>,
    ),
    Con(Spanned<NameRef>),
    Case(Box<Spanned<SurfaceExpr>>, Vec<Spanned<SurfaceMatchArm>>),
    IfThenElse(
        Box<Spanned<SurfaceExpr>>,
        Box<Spanned<SurfaceExpr>>,
        Box<Spanned<SurfaceExpr>>,
    ),
    BinaryOperator(
        SurfaceBinOp,
        Box<Spanned<SurfaceExpr>>,
        Box<Spanned<SurfaceExpr>>,
    ),
    UnaryOperator(SurfaceUnOp, Box<Spanned<SurfaceExpr>>),
}
