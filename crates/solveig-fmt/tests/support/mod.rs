use solveig_base::{Session, Symbol};
use solveig_syntax::ast::{
    ClassMethod, EntityExposure, ImportDecl, InstanceMethod, ModuleEntity, ModuleSelection,
    NameRef, ParsedModule, SurfaceConstructor, SurfaceDefinition, SurfaceDoStmt, SurfaceExpr,
    SurfaceLiteral, SurfaceMatchArm, SurfaceOpOperand, SurfaceOpSegment, SurfacePattern,
    SurfacePredicate, SurfaceTy,
};

pub(super) fn normalized_ast(session: &Session, module: &ParsedModule) -> String {
    format!(
        "module(imports=[{}],defs=[{}])",
        joined(
            module
                .imports
                .iter()
                .map(|import| import_shape(session, import))
        ),
        joined(
            module
                .defs
                .iter()
                .map(|definition| definition_shape(session, &definition.0))
        )
    )
}

fn joined(items: impl IntoIterator<Item = String>) -> String {
    items.into_iter().collect::<Vec<_>>().join(",")
}

fn symbol(session: &Session, value: Symbol) -> String {
    session.str(value).to_string()
}

fn name(session: &Session, value: &NameRef) -> String {
    value
        .segments
        .iter()
        .map(|segment| symbol(session, *segment))
        .collect::<Vec<_>>()
        .join(".")
}

fn import_shape(session: &Session, import: &ImportDecl) -> String {
    let selection = match &import.selection {
        ModuleSelection::All => "all".to_string(),
        ModuleSelection::Only(entities) => format!(
            "only({})",
            joined(
                entities
                    .iter()
                    .map(|entity| import_entity_shape(session, entity))
            )
        ),
    };
    let alias = import
        .alias
        .map_or_else(|| "default".to_string(), |alias| symbol(session, alias.0));
    format!(
        "import({} as {alias} {selection})",
        joined(
            import
                .module
                .0
                .iter()
                .map(|segment| symbol(session, *segment))
        )
    )
}

fn import_entity_shape(session: &Session, entity: &ModuleEntity) -> String {
    let exposure = match entity.exposure {
        EntityExposure::Opaque => "",
        EntityExposure::Constructors => "(..)",
    };
    let rename = entity.rename.map_or_else(String::new, |rename| {
        format!(" as {}", symbol(session, rename.0))
    });
    format!("{}{exposure}{rename}", symbol(session, entity.name.0))
}

fn definition_shape(session: &Session, definition: &SurfaceDefinition) -> String {
    match definition {
        SurfaceDefinition::FunDef {
            name, body, params, ..
        } => format!(
            "fun({} [{}] {})",
            symbol(session, name.0),
            joined(params.iter().map(|param| pattern_shape(session, &param.0))),
            expr_shape(session, &body.0)
        ),
        SurfaceDefinition::SigDef { name, ty, .. } => format!(
            "sig({} {})",
            symbol(session, name.0),
            ty_shape(session, &ty.0)
        ),
        SurfaceDefinition::FixityDef { fixity, ops, .. } => format!(
            "fixity({fixity:?} [{}])",
            joined(ops.iter().map(|op| symbol(session, op.0)))
        ),
        SurfaceDefinition::DataDef {
            name,
            ty_params,
            constructors,
            ..
        } => format!(
            "data({} [{}] [{}])",
            symbol(session, name.0),
            joined(ty_params.iter().map(|param| symbol(session, param.0))),
            joined(
                constructors
                    .iter()
                    .map(|constructor| constructor_shape(session, &constructor.0))
            )
        ),
        SurfaceDefinition::TypeAliasDef {
            name,
            ty_params,
            ty,
            ..
        } => format!(
            "alias({} [{}] {})",
            symbol(session, name.0),
            joined(ty_params.iter().map(|param| symbol(session, param.0))),
            ty_shape(session, &ty.0)
        ),
        SurfaceDefinition::ClassDef {
            name,
            ty_params,
            superclasses,
            methods,
            defaults,
            ..
        } => format!(
            "class({} [{}] [{}] [{}] [{}])",
            symbol(session, name.0),
            joined(ty_params.iter().map(|param| symbol(session, param.0))),
            joined(
                superclasses
                    .iter()
                    .map(|predicate| predicate_shape(session, &predicate.0))
            ),
            joined(
                methods
                    .iter()
                    .map(|method| class_method_shape(session, method))
            ),
            joined(
                defaults
                    .iter()
                    .map(|method| instance_method_shape(session, method))
            )
        ),
        SurfaceDefinition::InstanceDef {
            context,
            class_name,
            args,
            methods,
            ..
        } => format!(
            "instance([{}] {} [{}] [{}])",
            joined(
                context
                    .iter()
                    .map(|predicate| predicate_shape(session, &predicate.0))
            ),
            symbol(session, class_name.0),
            joined(args.iter().map(|arg| ty_shape(session, &arg.0))),
            joined(
                methods
                    .iter()
                    .map(|method| instance_method_shape(session, method))
            )
        ),
    }
}

fn constructor_shape(session: &Session, constructor: &SurfaceConstructor) -> String {
    format!(
        "ctor({} [{}])",
        symbol(session, constructor.name.0),
        joined(
            constructor
                .params
                .iter()
                .map(|param| ty_shape(session, &param.0))
        )
    )
}

fn class_method_shape(session: &Session, method: &ClassMethod) -> String {
    format!(
        "method({} {})",
        symbol(session, method.name.0),
        ty_shape(session, &method.ty.0)
    )
}

fn instance_method_shape(session: &Session, method: &InstanceMethod) -> String {
    format!(
        "method({} [{}] {})",
        symbol(session, method.name.0),
        joined(
            method
                .params
                .iter()
                .map(|param| pattern_shape(session, &param.0))
        ),
        expr_shape(session, &method.body.0)
    )
}

fn predicate_shape(session: &Session, predicate: &SurfacePredicate) -> String {
    format!(
        "pred({} [{}])",
        name(session, &predicate.class_name.0),
        joined(predicate.args.iter().map(|arg| ty_shape(session, &arg.0)))
    )
}

fn ty_shape(session: &Session, ty: &SurfaceTy) -> String {
    match ty {
        SurfaceTy::Unit => "unit".to_string(),
        SurfaceTy::Var(name) => format!("var({})", symbol(session, *name)),
        SurfaceTy::Arr(from, to) => format!(
            "arr({} {})",
            ty_shape(session, &from.0),
            ty_shape(session, &to.0)
        ),
        SurfaceTy::ForAll(params, body) => format!(
            "forall([{}] {})",
            joined(params.iter().map(|param| symbol(session, param.0))),
            ty_shape(session, &body.0)
        ),
        SurfaceTy::Qualified(predicates, body) => format!(
            "qualified([{}] {})",
            joined(
                predicates
                    .iter()
                    .map(|predicate| predicate_shape(session, &predicate.0))
            ),
            ty_shape(session, &body.0)
        ),
        SurfaceTy::Con(name_ref) => format!("con({})", name(session, &name_ref.0)),
        SurfaceTy::App(function, argument) => format!(
            "app({} {})",
            ty_shape(session, &function.0),
            ty_shape(session, &argument.0)
        ),
    }
}

fn pattern_shape(session: &Session, pattern: &SurfacePattern) -> String {
    match pattern {
        SurfacePattern::Var(name) => format!("var({})", symbol(session, *name)),
        SurfacePattern::Lit(literal) => literal_shape(literal),
        SurfacePattern::Con(name_ref, args) => format!(
            "con({} [{}])",
            name(session, &name_ref.0),
            joined(args.iter().map(|arg| pattern_shape(session, &arg.0)))
        ),
        SurfacePattern::Wild => "wild".to_string(),
    }
}

fn literal_shape(literal: &SurfaceLiteral) -> String {
    match literal {
        SurfaceLiteral::Int(value) => format!("int({value})"),
        SurfaceLiteral::BigInt(value) => format!("bigint({value})"),
        SurfaceLiteral::Float(value) => format!("float({:x})", value.to_bits()),
        SurfaceLiteral::Char(value) => format!("char({value:?})"),
        SurfaceLiteral::Str(value) => format!("string({value:?})"),
    }
}

fn match_arm_shape(session: &Session, arm: &SurfaceMatchArm) -> String {
    format!(
        "arm({} {})",
        pattern_shape(session, &arm.pattern.0),
        expr_shape(session, &arm.expr.0)
    )
}

fn do_statement_shape(session: &Session, statement: &SurfaceDoStmt) -> String {
    match statement {
        SurfaceDoStmt::Bind { pattern, expr } => format!(
            "bind({} {})",
            pattern_shape(session, &pattern.0),
            expr_shape(session, &expr.0)
        ),
        SurfaceDoStmt::Let { pattern, expr } => format!(
            "let({} {})",
            pattern_shape(session, &pattern.0),
            expr_shape(session, &expr.0)
        ),
        SurfaceDoStmt::Expr(expr) => format!("expr({})", expr_shape(session, &expr.0)),
    }
}

fn operand_shape(session: &Session, operand: &SurfaceOpOperand) -> String {
    format!(
        "operand({} {})",
        operand.prefix_minuses.len(),
        expr_shape(session, &operand.expr.0)
    )
}

fn segment_shape(session: &Session, segment: &SurfaceOpSegment) -> String {
    format!(
        "segment({} {})",
        symbol(session, segment.op.0),
        operand_shape(session, &segment.rhs)
    )
}

fn expr_shape(session: &Session, expr: &SurfaceExpr) -> String {
    match expr {
        SurfaceExpr::Unit => "unit".to_string(),
        SurfaceExpr::Literal(literal) => literal_shape(literal),
        SurfaceExpr::Hole => "hole".to_string(),
        SurfaceExpr::Var(name_ref) => format!("var({})", name(session, &name_ref.0)),
        SurfaceExpr::App(function, argument) => format!(
            "app({} {})",
            expr_shape(session, &function.0),
            expr_shape(session, &argument.0)
        ),
        SurfaceExpr::Abs(params, body) => format!(
            "abs([{}] {})",
            joined(params.iter().map(|(pattern, annotation)| {
                format!(
                    "param({} {})",
                    pattern_shape(session, &pattern.0),
                    annotation
                        .as_ref()
                        .map_or_else(|| "none".to_string(), |ty| ty_shape(session, &ty.0))
                )
            })),
            expr_shape(session, &body.0)
        ),
        SurfaceExpr::Ann(value, ty) => format!(
            "ann({} {})",
            expr_shape(session, &value.0),
            ty_shape(session, &ty.0)
        ),
        SurfaceExpr::Let(pattern, value, body) => format!(
            "let({} {} {})",
            pattern_shape(session, &pattern.0),
            expr_shape(session, &value.0),
            expr_shape(session, &body.0)
        ),
        SurfaceExpr::Con(name_ref) => format!("con({})", name(session, &name_ref.0)),
        SurfaceExpr::Case(scrutinee, arms) => format!(
            "case({} [{}])",
            expr_shape(session, &scrutinee.0),
            joined(arms.iter().map(|arm| match_arm_shape(session, &arm.0)))
        ),
        SurfaceExpr::Do(statements) => format!(
            "do([{}])",
            joined(
                statements
                    .iter()
                    .map(|statement| do_statement_shape(session, &statement.0))
            )
        ),
        SurfaceExpr::IfThenElse(condition, then_branch, else_branch) => format!(
            "if({} {} {})",
            expr_shape(session, &condition.0),
            expr_shape(session, &then_branch.0),
            expr_shape(session, &else_branch.0)
        ),
        SurfaceExpr::UnaryOperator(operator, value) => {
            format!("unary({operator:?} {})", expr_shape(session, &value.0))
        }
        SurfaceExpr::OperatorApply(left, operator, right) => format!(
            "operator({} {} {})",
            expr_shape(session, &left.0),
            symbol(session, operator.0),
            expr_shape(session, &right.0)
        ),
        SurfaceExpr::OperatorLeftSection(value, operator) => format!(
            "left-section({} {})",
            expr_shape(session, &value.0),
            symbol(session, operator.0)
        ),
        SurfaceExpr::OperatorRightSection(operator, value) => format!(
            "right-section({} {})",
            symbol(session, operator.0),
            expr_shape(session, &value.0)
        ),
        SurfaceExpr::OpSeq(first, rest) => format!(
            "opseq({} [{}])",
            operand_shape(session, first),
            joined(rest.iter().map(|segment| segment_shape(session, segment)))
        ),
    }
}
