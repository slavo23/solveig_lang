//! Parser event stream and concrete syntax kind vocabulary.
//! H3 consumes these events to build Solveig's hand-rolled lossless CST.

use solveig_base::Diagnostic;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxKind {
    Module,
    ImportList,
    ImportDecl,
    DefinitionList,
    FunctionOrSignature,
    FixityDef,
    DataDef,
    TypeAliasDef,
    Constructor,
    ClassDef,
    InstanceDef,
    PredicateList,
    Predicate,
    NamePath,
    Expr,
    OpChain,
    OpOperand,
    OperatorSection,
    BacktickOperator,
    AppExpr,
    AtomExpr,
    LambdaExpr,
    LetExpr,
    IfExpr,
    DoExpr,
    DoStmt,
    CaseExpr,
    CaseArm,
    Pattern,
    PatternHead,
    PatternArg,
    Type,
    QualifiedType,
    TypeArrow,
    TypeApp,
    TypeAtom,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    StartNode(SyntaxKind),
    Token(usize),
    FinishNode,
    Error(Diagnostic),
}
