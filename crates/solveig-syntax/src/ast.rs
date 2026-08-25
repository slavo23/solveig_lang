//! Surface syntax tree for parsed Solveig modules.
//! The parser produces these nodes before fixity resolution and name resolution.

use std::cell::Ref;

use solveig_base::{FileId, Session, Span, Symbol};

use crate::cst::{CstId, SyntaxTree};

pub type Spanned<T> = (T, Span, CstId);

pub type SpannedWrapper<T> = Spanned<T>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Left,
    Right,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fixity {
    pub prec: u8,
    pub assoc: Assoc,
}

impl Fixity {
    pub const DEFAULT: Self = Self {
        prec: 9,
        assoc: Assoc::Left,
    };
}

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

    pub fn last_text<'a>(&self, session: &'a Session) -> Ref<'a, str> {
        session.str(self.last())
    }

    pub fn prefix(&self) -> &[Symbol] {
        self.segments
            .get(..self.segments.len().saturating_sub(1))
            .expect("ICE: NameRef prefix range must be in bounds")
    }

    pub fn display(&self, session: &Session) -> String {
        self.segments
            .iter()
            .map(|segment| session.str(*segment).to_string())
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
            .map(|segment| session.str(*segment).to_string())
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
pub struct ImportDecl {
    pub module: ModuleName,
    pub alias: Option<Spanned<Symbol>>,
    pub selection: ModuleSelection,
    pub span: Span,
    pub cst_id: CstId,
}

/// Controls whether a selected data type exposes its constructors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityExposure {
    Opaque,
    Constructors,
}

/// Names one entity in a module directive or selective import.
#[derive(Debug, Clone)]
pub struct ModuleEntity {
    pub name: Spanned<Symbol>,
    pub exposure: EntityExposure,
    pub rename: Option<Spanned<Symbol>>,
    pub cst_id: CstId,
}

/// Selects every visible entity or a precise entity list.
#[derive(Debug, Clone)]
pub enum ModuleSelection {
    All,
    Only(Vec<ModuleEntity>),
}

/// Retains one export or private directive with its source identity.
#[derive(Debug, Clone)]
pub struct VisibilityDecl {
    pub entities: Vec<ModuleEntity>,
    pub span: Span,
    pub cst_id: CstId,
}

/// Republishes selected entities from one source module.
#[derive(Debug, Clone)]
pub struct ReexportDecl {
    pub module: ModuleName,
    pub selection: ModuleSelection,
    pub span: Span,
    pub cst_id: CstId,
}

/// Retains the centralized visibility directives from the initial module header.
#[derive(Debug, Clone, Default)]
pub struct ModuleDirectives {
    pub exports: Vec<VisibilityDecl>,
    pub private: Vec<VisibilityDecl>,
    pub reexports: Vec<ReexportDecl>,
}

/// Documentation text attached to a surface declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocComment {
    /// Normalized comment text without the `-- |` or `-- ^` marker.
    pub text: String,
}

impl DocComment {
    /// Creates a documentation comment from normalized text.
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

#[derive(Debug, Clone)]
pub struct ParsedModule {
    pub file: FileId,
    pub cst: SyntaxTree,
    pub directives: ModuleDirectives,
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
    pub docs: Option<DocComment>,
    pub cst_id: CstId,
}

#[derive(Debug, Clone)]
pub struct InstanceMethod {
    pub name: Spanned<Symbol>,
    pub params: Vec<Spanned<SurfacePattern>>,
    pub body: Spanned<SurfaceExpr>,
    pub cst_id: CstId,
}

#[derive(Debug, Clone)]
pub enum SurfaceDefinition {
    FunDef {
        name: Spanned<Symbol>,
        body: Spanned<SurfaceExpr>,
        params: Vec<Spanned<SurfacePattern>>,
        docs: Option<DocComment>,
    },
    SigDef {
        name: Spanned<Symbol>,
        ty: Spanned<SurfaceTy>,
        docs: Option<DocComment>,
    },
    FixityDef {
        fixity: Fixity,
        ops: Vec<Spanned<Symbol>>,
        docs: Option<DocComment>,
    },
    DataDef {
        name: Spanned<Symbol>,
        ty_params: Vec<Spanned<Symbol>>,
        constructors: Vec<Spanned<SurfaceConstructor>>,
        docs: Option<DocComment>,
    },
    TypeAliasDef {
        name: Spanned<Symbol>,
        ty_params: Vec<Spanned<Symbol>>,
        ty: Spanned<SurfaceTy>,
        docs: Option<DocComment>,
    },
    ClassDef {
        name: Spanned<Symbol>,
        ty_params: Vec<Spanned<Symbol>>,
        superclasses: Vec<Spanned<SurfacePredicate>>,
        methods: Vec<ClassMethod>,
        defaults: Vec<InstanceMethod>,
        docs: Option<DocComment>,
    },
    InstanceDef {
        context: Vec<Spanned<SurfacePredicate>>,
        class_name: Spanned<Symbol>,
        args: Vec<Spanned<SurfaceTy>>,
        methods: Vec<InstanceMethod>,
        docs: Option<DocComment>,
    },
}

impl SurfaceDefinition {
    /// Returns the documentation comment attached to this definition.
    pub fn docs(&self) -> Option<&DocComment> {
        match self {
            Self::FunDef { docs, .. }
            | Self::SigDef { docs, .. }
            | Self::FixityDef { docs, .. }
            | Self::DataDef { docs, .. }
            | Self::TypeAliasDef { docs, .. }
            | Self::ClassDef { docs, .. }
            | Self::InstanceDef { docs, .. } => docs.as_ref(),
        }
    }

    /// Replaces the documentation comment attached to this definition.
    pub fn set_docs(&mut self, new_docs: Option<DocComment>) {
        let docs = match self {
            Self::FunDef { docs, .. }
            | Self::SigDef { docs, .. }
            | Self::FixityDef { docs, .. }
            | Self::DataDef { docs, .. }
            | Self::TypeAliasDef { docs, .. }
            | Self::ClassDef { docs, .. }
            | Self::InstanceDef { docs, .. } => docs,
        };
        *docs = new_docs;
    }
}

#[derive(Debug, Clone)]
pub struct SurfaceConstructor {
    pub name: Spanned<Symbol>,
    pub params: Vec<Spanned<SurfaceTy>>,
    pub docs: Option<DocComment>,
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

#[derive(Clone, Debug, PartialEq)]
pub enum SurfaceDoStmt {
    Bind {
        pattern: Spanned<SurfacePattern>,
        expr: Spanned<SurfaceExpr>,
    },
    Let {
        pattern: Spanned<SurfacePattern>,
        expr: Spanned<SurfaceExpr>,
    },
    Expr(Spanned<SurfaceExpr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceOpOperand {
    pub prefix_minuses: Vec<Span>,
    pub expr: Spanned<SurfaceExpr>,
    pub cst_id: CstId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceOpSegment {
    pub op: Spanned<Symbol>,
    pub rhs: SurfaceOpOperand,
    pub cst_id: CstId,
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
        Vec<(Spanned<SurfacePattern>, Option<Spanned<SurfaceTy>>)>,
        Box<Spanned<SurfaceExpr>>,
    ),
    Ann(Box<Spanned<SurfaceExpr>>, Spanned<SurfaceTy>),
    Let(
        Spanned<SurfacePattern>,
        Box<Spanned<SurfaceExpr>>,
        Box<Spanned<SurfaceExpr>>,
    ),
    Con(Spanned<NameRef>),
    Case(Box<Spanned<SurfaceExpr>>, Vec<Spanned<SurfaceMatchArm>>),
    Do(Vec<Spanned<SurfaceDoStmt>>),
    IfThenElse(
        Box<Spanned<SurfaceExpr>>,
        Box<Spanned<SurfaceExpr>>,
        Box<Spanned<SurfaceExpr>>,
    ),
    UnaryOperator(SurfaceUnOp, Box<Spanned<SurfaceExpr>>),
    OperatorApply(
        Box<Spanned<SurfaceExpr>>,
        Spanned<Symbol>,
        Box<Spanned<SurfaceExpr>>,
    ),
    OperatorLeftSection(Box<Spanned<SurfaceExpr>>, Spanned<Symbol>),
    OperatorRightSection(Spanned<Symbol>, Box<Spanned<SurfaceExpr>>),
    OpSeq(Box<SurfaceOpOperand>, Vec<SurfaceOpSegment>),
}
