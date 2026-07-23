use std::collections::HashMap;
use std::ops::Range;

use solveig_ast::ast::{
    CaseArm, CaseArmNode, ConstructorDef, DataDefinition, InstanceDefinition, Kind, Pattern,
    PatternNode, Predicate, PrettyTables, ResolvedModule, Scheme, TermDefinition, TermExpr,
    TermExprNode, TyVarId, TypeExpr, VarRef, class_display_name, legacy_ctor_id, legacy_def_id,
    legacy_tycon_id, legacy_tyvar_id, symbol_display_name, tycon_display_name, tyvar_display_name,
    var_display_name,
};

use crate::inference::type_infer::TcMetadata;

type Span = Range<usize>;

#[derive(Clone, Debug)]
struct MethodRuntimeInfo {
    selector_name: String,
    class_head_pred: Predicate,
    method_body_ty: TypeExpr,
}

#[derive(Clone, Debug)]
struct ClassRuntimeInfo {
    dict_tycon: String,
    dict_ctor: String,

    params: Vec<(String, Kind)>,
    superclasses: Vec<Predicate>,
    field_count: usize,

    methods: HashMap<String, MethodRuntimeInfo>,
}

#[derive(Clone, Debug)]
struct ConstrainedFuncInfo {
    predicates: Vec<Predicate>,
    body_ty: TypeExpr,
}

#[derive(Clone, Debug, Default)]
struct EvidenceEnv {
    by_pred: Vec<(Predicate, TermExprNode<TcMetadata, Span>)>,
}

impl EvidenceEnv {
    fn lookup(&self, pred: &Predicate) -> Option<TermExprNode<TcMetadata, Span>> {
        // Givens are already in the current type scope, so evidence lookup should be exact.
        // Pattern matching is reserved for instance heads.
        self.by_pred
            .iter()
            .find(|(p, _)| p == pred)
            .map(|(_, expr)| expr.clone())
    }

    fn extend(&mut self, pred: Predicate, expr: TermExprNode<TcMetadata, Span>) {
        self.by_pred.push((pred, expr));
    }
}

fn pretty_tables_for_module(module: &ResolvedModule<TcMetadata, Span>) -> PrettyTables<'_> {
    PrettyTables {
        tycons: &module.tycon_table,
        classes: &module.class_table,
    }
}

include!("names.rs");
include!("dict_synth.rs");
include!("rewrite.rs");
