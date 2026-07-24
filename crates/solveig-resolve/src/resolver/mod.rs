use std::collections::{HashMap, HashSet};
use std::env;
use std::ops::Range;

use solveig_ast::ast::{
    BinOp, BuiltinDefinition, COMPILER_PRELUDE_BUILTINS_MODULE, CaseArm, CaseArmNode,
    ClassDefinition, ClassId, ClassMethodDef, ConstructorDef, CtorId, DataDefinition, DefId,
    InstanceDefinition, InstanceMethodDef, Kind, KindMetaId, Literal, ModuleId, PRIM_TYS, Pattern,
    PatternNode, Predicate, ResolvedModule, Scheme, TermDefinition, TermExpr, TermExprNode,
    TyConId, TypeExpr, UnOp, VarRef, compiler_bool_constructor_types,
    compiler_bool_data_definition, compiler_primitive_tycon_kinds, intern_string_literal,
    legacy_class_id, legacy_ctor_id, legacy_tycon_id, prelude_list_cons_ctor_id,
    register_def_display_name, register_symbol_display_name, tyvar_display_name,
};
use solveig_base::{FileSpan, Session, Span as SourceSpan, Symbol};
use solveig_check::data_kind_infer::{
    RawConstructorDef, RawDataDef, infer_all_data_kinds_with_env,
};
use solveig_check::inference::class_kind_infer::ClassKindInferencer;
use solveig_check::signature_infer::{KindError, SignatureKindInferencer};
use solveig_syntax::ast::{
    Assoc, Fixity, ModuleName, NameRef, ParsedModule, SpannedWrapper, SurfaceBinOp,
    SurfaceDefinition, SurfaceExpr, SurfaceLiteral, SurfaceMatchArm, SurfacePattern,
    SurfacePredicate, SurfaceTy, SurfaceUnOp,
};
use solveig_syntax::fixity::{FixityError, resolve_op_seq};

use crate::project::{ModuleContext, ModuleTable, ResolveError, ResolveNameKind};

type Span = Range<usize>;

fn to_core_span(span: SourceSpan) -> Span {
    span.to_usize_range()
}

fn resolver_trace_enabled() -> bool {
    match env::var("SOLVEIG_RESOLVER_TRACE") {
        Ok(value) => {
            let value = value.trim().to_ascii_lowercase();
            !value.is_empty() && value != "0" && value != "false" && value != "off"
        }
        Err(_) => false,
    }
}

fn resolve_literal(lit: &SurfaceLiteral) -> Literal {
    match lit {
        SurfaceLiteral::Int(value) => Literal::Int(*value),
        SurfaceLiteral::Float(value) => Literal::Float(*value),
        SurfaceLiteral::Char(value) => Literal::Char(*value),
        SurfaceLiteral::Str(value) => Literal::Str(intern_string_literal(value)),
    }
}

fn trace(session: &Session, module: &ModuleName, message: impl AsRef<str>) {
    if resolver_trace_enabled() {
        eprintln!(
            "[resolver:{}] {}",
            module.as_string(session),
            message.as_ref()
        );
    }
}

fn trace_predicates(session: &Session, module: &ModuleName, label: &str, predicates: &[Predicate]) {
    if resolver_trace_enabled() && !predicates.is_empty() {
        let rendered = predicates
            .iter()
            .map(|p| format!("{:?}", p))
            .collect::<Vec<_>>()
            .join(", ");

        trace(session, module, format!("{label}: {rendered}"));
    }
}

#[derive(Debug, Clone, Default)]
pub struct ResolverImports {
    pub tycon_kind_ids: HashMap<TyConId, Kind>,
    pub class_param_kind_ids: HashMap<ClassId, Vec<Kind>>,
}

#[derive(Debug)]
pub struct ResolvedModuleInfo<A> {
    pub module: ResolvedModule<A>,
    pub resolutions: Vec<(FileSpan, DefId)>,
}

#[derive(Debug)]
pub enum ResolverError {
    Resolve(Vec<ResolveError>),
    Kind(KindError),
}

impl From<KindError> for ResolverError {
    fn from(error: KindError) -> Self {
        Self::Kind(error)
    }
}

#[derive(Debug)]
struct LocalResolverEnv {
    /// Surface class name -> canonical class id for classes defined in this module.
    ///
    /// This is deliberately separate from `ModuleContext.local_api`: while resolving a
    /// module, class predicates in signatures/methods/instances must canonicalize
    /// against the source declarations currently being resolved. Relying only on the
    /// already-built API made local class names like `Functor` leak through as raw
    /// predicate names in some paths.
    classes: HashMap<Symbol, DefId>,
}

impl LocalResolverEnv {
    fn from_parsed_module(module_id: ModuleId, parsed: &ParsedModule) -> Self {
        let classes = parsed
            .defs
            .iter()
            .filter_map(|defn| match &defn.0 {
                SurfaceDefinition::ClassDef { name, .. } => Some((
                    name.0,
                    DefId {
                        module: module_id,
                        name: name.0,
                    },
                )),
                _ => None,
            })
            .collect();

        Self { classes }
    }
}

pub struct Resolver<'a> {
    session: &'a Session,
    modules: &'a ModuleTable,
    errors: Vec<ResolveError>,
    resolutions: Vec<(FileSpan, DefId)>,
}

impl<'a> Resolver<'a> {
    pub fn new(session: &'a Session, modules: &'a ModuleTable) -> Self {
        Self {
            session,
            modules,
            errors: Vec::new(),
            resolutions: Vec::new(),
        }
    }
    fn symbol_text(&self, symbol: Symbol) -> &'static str {
        self.session.str(symbol)
    }
    fn file_span(&self, cx: &ModuleContext, span: SourceSpan) -> FileSpan {
        FileSpan::new(cx.file, span)
    }
    fn record_resolution(&mut self, span: FileSpan, def_id: DefId) {
        self.resolutions.push((span, def_id));
    }
    fn qualified_def_name(&self, def_id: DefId) -> String {
        let name = self.modules.def_path(def_id).render(self.session);
        register_def_display_name(def_id, &name);
        name
    }
    fn local_def_id(&self, cx: &ModuleContext, name: Symbol) -> DefId {
        DefId {
            module: cx.module_id,
            name,
        }
    }
    fn unresolved_name(&self, r: &NameRef) -> String {
        r.display(self.session)
    }
    fn imported_by_alias_or_module<'cx>(
        &self,
        r: &NameRef,
        cx: &'cx ModuleContext,
    ) -> Option<&'cx crate::project::ImportedApi> {
        let [first, ..] = r.segments.as_slice() else {
            return None;
        };

        cx.imported_apis
            .iter()
            .find(|import| import.alias == *first)
            .or_else(|| {
                cx.imported_apis
                    .iter()
                    .find(|import| import.module.0.as_slice() == r.prefix())
            })
    }
    fn report_unknown_name(
        &mut self,
        span: FileSpan,
        kind: ResolveNameKind,
        name: impl Into<String>,
    ) {
        self.errors.push(ResolveError::UnknownName {
            span,
            kind,
            name: name.into(),
        });
    }
    fn report_ambiguous_import(
        &mut self,
        span: FileSpan,
        kind: ResolveNameKind,
        name: Symbol,
        candidates: Vec<DefId>,
    ) {
        self.errors.push(ResolveError::AmbiguousImport {
            span,
            kind,
            name,
            candidates,
        });
    }
    fn report_unknown_module(&mut self, span: FileSpan, module: ModuleName) {
        self.errors
            .push(ResolveError::UnknownModule { span, module });
    }
    fn take_errors(&mut self) -> Option<Vec<ResolveError>> {
        if self.errors.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.errors))
        }
    }
}

include!("terms.rs");
include!("types.rs");
include!("constructors.rs");
include!("classes.rs");
include!("module.rs");
