use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use solveig_ast::ast::{
    ClassDefinition, ClassId, ClassTable, CtorId, CtorTable, DefId, ModuleId, PrettyTables, Scheme,
    TyConId, TyConTable, TypeExpr, def_display_name, symbol_display_name,
};
use solveig_base::{FileSpan, IntoDiagnostic, Session};
use solveig_check::inference::holes::hole_diagnostics;
use solveig_check::inference::type_infer::{
    CheckedModule, TcMetadata, TypeCheckEnv, check_and_elab_module,
};
use solveig_lower::closure_lower::lower_to_closure_ir;
use solveig_lower::lowering::lower_module;
use solveig_report::emit_diagnostics;
use solveig_resolve::project::{
    BuiltinApi, EffectiveImport, ImportedApi, ImportedTypedApi, ModuleContext, ModuleTable,
    ParsedProject, Project, ResolveError, SharedModuleApi, SharedTypedModuleApi, TypedDisplayMaps,
    build_module_api, build_typed_module_api, compiler_builtin_api, effective_imports,
    module_order_checked, seed_compiler_prelude_module_api,
};
use solveig_resolve::resolver::{Resolver, ResolverError, ResolverImports};
use solveig_syntax::ast::ModuleName;
use solveig_vm::vm::Machine;

use crate::project::{ProjectError, load_project, parse_project};

#[derive(Debug, Clone)]
pub struct Compilation {
    project_root: PathBuf,
}

impl Compilation {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
        }
    }

    pub fn compile_and_run_project(
        &self,
        entry_name: &str,
        dump_bytecode: bool,
    ) -> Result<(), String> {
        let output = compile_project_capture(&self.project_root, entry_name, dump_bytecode)?;
        println!("{output}");
        Ok(())
    }

    pub fn run_project_capture(&self, entry_name: &str) -> Result<String, String> {
        compile_project_capture(&self.project_root, entry_name, false)
    }

    pub fn checked_project_schemes(&self) -> Result<BTreeMap<String, String>, String> {
        checked_project_schemes_impl(&self.project_root)
    }
}

pub fn compile_and_run_project(
    project_root: &Path,
    entry_name: &str,
    dump_bytecode: bool,
) -> Result<(), String> {
    Compilation::new(project_root).compile_and_run_project(entry_name, dump_bytecode)
}

pub fn run_project_capture(project_root: &Path, entry_name: &str) -> Result<String, String> {
    Compilation::new(project_root).run_project_capture(entry_name)
}

pub fn checked_project_schemes(project_root: &Path) -> Result<BTreeMap<String, String>, String> {
    Compilation::new(project_root).checked_project_schemes()
}

fn checked_project_schemes_impl(project_root: &Path) -> Result<BTreeMap<String, String>, String> {
    let checked_modules = check_project_modules(project_root)?;
    let merged = merge_checked_modules(checked_modules)?;
    let pretty_tables = PrettyTables {
        tycons: &merged.tycon_table,
        classes: &merged.class_table,
    };
    let mut schemes = BTreeMap::new();

    for (id, scheme) in &merged.func_schemes {
        schemes.insert(
            def_display_name(*id),
            scheme.pretty_with_tables(pretty_tables),
        );
    }

    Ok(schemes)
}

fn compile_project_capture(
    project_root: &Path,
    entry_name: &str,
    dump_bytecode: bool,
) -> Result<String, String> {
    let checked_modules = check_project_modules(project_root)?;
    let merged = merge_checked_modules(checked_modules)?;
    let elaborated = solveig_check::inference::class_elab::elaborate_typeclasses(&merged);
    let ir = lower_module(&elaborated);
    let cc = lower_to_closure_ir(&ir);

    if dump_bytecode {
        Machine::dump_bytecode(&cc, "bytecode.txt")
            .map_err(|e| format!("failed to dump bytecode: {e}"))?;
    }

    let entry_fq = format!("main.{entry_name}");
    let result = Machine::exec(&cc, &entry_fq).map_err(|err| format!("runtime error: {err}"))?;
    Ok(result.display().to_string())
}

fn check_project_modules(project_root: &Path) -> Result<Vec<CheckedModule>, String> {
    let session = Session::new();
    let entry_file = find_entry_file(project_root)?;
    let (_root, project) = match load_project(&session, &entry_file) {
        Ok(project) => project,
        Err(ProjectError::Message(message)) => return Err(message),
        Err(ProjectError::Resolve(error)) => {
            push_resolve_errors(&session, None, vec![error]);
            emit_session_diagnostics(&session)?;
            return Err("resolve error".to_string());
        }
    };

    if session.diagnostics().has_errors() {
        emit_session_diagnostics(&session)?;
        return Err("parse error".to_string());
    }

    let order = match module_order_checked(&session, &project) {
        Ok(order) => order,
        Err(error) => {
            push_resolve_errors(&session, Some(&project.module_table), vec![error]);
            emit_session_diagnostics(&session)?;
            return Err("resolve error".to_string());
        }
    };
    let parsed = parse_project(&project);

    let mut state = match FrontendState::new(&session, &project, &parsed, &order) {
        Ok(state) => state,
        Err(errors) => {
            push_resolve_errors(&session, Some(&project.module_table), errors);
            emit_session_diagnostics(&session)?;
            return Err("resolve error".to_string());
        }
    };
    state.check_all(&order)?;

    Ok(state.checked_modules)
}

fn emit_session_diagnostics(session: &Session) -> Result<(), String> {
    let diagnostics = session.diagnostics().sorted_by_file_and_span();
    let source_map = session.source_map();

    emit_diagnostics(&diagnostics, &source_map)
        .map_err(|err| format!("failed to print diagnostics: {err}"))
}

fn push_resolve_errors(
    session: &Session,
    modules: Option<&ModuleTable>,
    errors: Vec<ResolveError>,
) {
    for error in errors {
        session
            .diagnostics()
            .push(error.into_diagnostic((session, modules)));
    }
}

fn erase_tc_expr_to_span_expr(
    expr: solveig_ast::ast::TermExprNode<
        solveig_check::inference::type_infer::TcMetadata,
        std::ops::Range<usize>,
    >,
) -> solveig_ast::ast::TermExprNode<std::ops::Range<usize>, std::ops::Range<usize>> {
    let anno = expr.anno.span.clone();

    let node = match expr.node {
        solveig_ast::ast::TermExpr::Var(name) => solveig_ast::ast::TermExpr::Var(name),
        solveig_ast::ast::TermExpr::Con(name) => solveig_ast::ast::TermExpr::Con(name),
        solveig_ast::ast::TermExpr::Unit => solveig_ast::ast::TermExpr::Unit,
        solveig_ast::ast::TermExpr::Lit(lit) => solveig_ast::ast::TermExpr::Lit(lit),
        solveig_ast::ast::TermExpr::Hole => solveig_ast::ast::TermExpr::Hole,

        solveig_ast::ast::TermExpr::Abs(param, ann, body) => {
            solveig_ast::ast::TermExpr::Abs(param, ann, Box::new(erase_tc_expr_to_span_expr(*body)))
        }

        solveig_ast::ast::TermExpr::App(f, x) => solveig_ast::ast::TermExpr::App(
            Box::new(erase_tc_expr_to_span_expr(*f)),
            Box::new(erase_tc_expr_to_span_expr(*x)),
        ),

        solveig_ast::ast::TermExpr::TyAbs(name, kind, body) => solveig_ast::ast::TermExpr::TyAbs(
            name,
            kind,
            Box::new(erase_tc_expr_to_span_expr(*body)),
        ),

        solveig_ast::ast::TermExpr::TyApp(inner, ty) => {
            solveig_ast::ast::TermExpr::TyApp(Box::new(erase_tc_expr_to_span_expr(*inner)), ty)
        }

        solveig_ast::ast::TermExpr::Anno(inner, ty) => {
            solveig_ast::ast::TermExpr::Anno(Box::new(erase_tc_expr_to_span_expr(*inner)), ty)
        }

        solveig_ast::ast::TermExpr::Case(scrutinee, arms) => solveig_ast::ast::TermExpr::Case(
            Box::new(erase_tc_expr_to_span_expr(*scrutinee)),
            arms.into_iter()
                .map(|arm| {
                    solveig_ast::ast::CaseArmNode::new(
                        solveig_ast::ast::CaseArm {
                            pattern: arm.node.pattern,
                            expr: erase_tc_expr_to_span_expr(arm.node.expr),
                        },
                        arm.anno,
                    )
                })
                .collect(),
        ),

        solveig_ast::ast::TermExpr::IfThenElse(c, t, e) => solveig_ast::ast::TermExpr::IfThenElse(
            Box::new(erase_tc_expr_to_span_expr(*c)),
            Box::new(erase_tc_expr_to_span_expr(*t)),
            Box::new(erase_tc_expr_to_span_expr(*e)),
        ),

        solveig_ast::ast::TermExpr::BinaryOperator(op, l, r) => {
            solveig_ast::ast::TermExpr::BinaryOperator(
                op,
                Box::new(erase_tc_expr_to_span_expr(*l)),
                Box::new(erase_tc_expr_to_span_expr(*r)),
            )
        }

        solveig_ast::ast::TermExpr::UnaryOperator(op, rhs) => {
            solveig_ast::ast::TermExpr::UnaryOperator(
                op,
                Box::new(erase_tc_expr_to_span_expr(*rhs)),
            )
        }

        solveig_ast::ast::TermExpr::Let(name, binder, in_expr) => solveig_ast::ast::TermExpr::Let(
            name,
            Box::new(erase_tc_expr_to_span_expr(*binder)),
            Box::new(erase_tc_expr_to_span_expr(*in_expr)),
        ),
    };

    solveig_ast::ast::TermExprNode::new(node, anno)
}

type CheckedInstanceDef = solveig_ast::ast::InstanceDefinition<TcMetadata, std::ops::Range<usize>>;
type ResolvedInstanceDef =
    solveig_ast::ast::InstanceDefinition<std::ops::Range<usize>, std::ops::Range<usize>>;

fn erase_checked_instance_defs_to_resolved(
    defs: Vec<CheckedInstanceDef>,
) -> Vec<ResolvedInstanceDef> {
    defs.into_iter()
        .map(|inst| solveig_ast::ast::InstanceDefinition {
            context: inst.context,
            head: inst.head,
            methods: inst
                .methods
                .into_iter()
                .map(|m| solveig_ast::ast::InstanceMethodDef {
                    name: m.name,
                    body: erase_tc_expr_to_span_expr(m.body),
                })
                .collect(),
        })
        .collect()
}

struct FrontendState<'a> {
    session: &'a Session,
    project: &'a Project,
    parsed: &'a ParsedProject,
    builtins: Rc<BuiltinApi>,
    apis: HashMap<ModuleName, SharedModuleApi>,
    typed_apis: HashMap<ModuleName, SharedTypedModuleApi>,
    resolutions: HashMap<ModuleId, Vec<(FileSpan, DefId)>>,
    checked_modules: Vec<CheckedModule>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum FrontendPhase {
    Resolve,
    Kind,
    Type,
    Hole,
}

impl FrontendPhase {
    fn error_message(self) -> &'static str {
        match self {
            Self::Resolve => "resolve error",
            Self::Kind => "kind error",
            Self::Type => "type error",
            Self::Hole => "Program has unfilled holes. Aborting compilation",
        }
    }
}

impl<'a> FrontendState<'a> {
    fn new(
        session: &'a Session,
        project: &'a Project,
        parsed: &'a ParsedProject,
        order: &[ModuleName],
    ) -> Result<Self, Vec<ResolveError>> {
        let mut apis = HashMap::new();
        let mut errors = Vec::new();
        let builtins = Rc::new(compiler_builtin_api(session, &project.module_table));

        for module_name in order {
            let module_id = project
                .module_table
                .id(module_name)
                .expect("ICE: checked module order referenced unknown module");
            match build_module_api(module_id, parsed.get(session, module_name)) {
                Ok(mut api) => {
                    seed_compiler_prelude_module_api(session, module_name, module_id, &mut api);
                    apis.insert(module_name.clone(), Rc::new(api));
                }
                Err(error) => errors.push(error),
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(Self {
            session,
            project,
            parsed,
            builtins,
            apis,
            typed_apis: HashMap::new(),
            resolutions: HashMap::new(),
            checked_modules: Vec::new(),
        })
    }

    fn check_all(&mut self, order: &[ModuleName]) -> Result<(), String> {
        let mut failed_modules = HashSet::new();
        let mut first_failure: Option<FrontendPhase> = None;

        for module_name in order {
            let parsed = self.parsed.get(self.session, module_name);
            let imports =
                effective_imports(self.session, module_name, &parsed.imports, parsed.file);

            if imports
                .iter()
                .any(|import| failed_modules.contains(&import.module))
            {
                failed_modules.insert(module_name.clone());
                continue;
            }

            let checked = match self.check_module(module_name) {
                Ok(checked) => checked,
                Err(phase) => {
                    failed_modules.insert(module_name.clone());
                    first_failure = Some(first_failure.map_or(phase, |current| current.min(phase)));
                    continue;
                }
            };

            let module_id = self
                .project
                .module_table
                .id(module_name)
                .expect("ICE: checked module order referenced unknown module");
            let typed_api = Rc::new(build_typed_module_api(
                self.session,
                module_id,
                module_name,
                &checked,
            ));

            self.typed_apis.insert(module_name.clone(), typed_api);
            self.checked_modules.push(checked);
        }

        if let Some(phase) = first_failure {
            emit_session_diagnostics(self.session)?;
            return Err(phase.error_message().to_string());
        }

        Ok(())
    }

    fn collect_imported_class_param_kind_ids(
        &self,
        imports: &[EffectiveImport],
    ) -> HashMap<ClassId, Vec<solveig_ast::ast::Kind>> {
        let mut out = HashMap::new();

        for imp in imports {
            if let Some(api) = self.typed_apis.get(&imp.module) {
                for class_def in api.filtered(&imp.spec).class_defs.values() {
                    out.insert(
                        class_def.id,
                        class_def
                            .params
                            .iter()
                            .map(|(_, kind)| kind.clone())
                            .collect(),
                    );
                }
            }
        }

        out
    }

    fn check_module(&mut self, module_name: &ModuleName) -> Result<CheckedModule, FrontendPhase> {
        let parsed = self.parsed.get(self.session, module_name);
        let file = parsed.file;
        let module_id = self
            .project
            .module_table
            .id(module_name)
            .expect("ICE: module check referenced unknown module");
        let imports = effective_imports(self.session, module_name, &parsed.imports, file);

        let imported_apis = self.build_imported_apis(&imports);
        let cx = ModuleContext {
            module: module_name.clone(),
            module_id,
            file,
            local_api: self.apis[module_name].clone(),
            imported_apis,
            builtins: self.builtins.clone(),
        };

        let resolver_imports = ResolverImports {
            tycon_kind_ids: self.collect_imported_kind_ids(&imports),
            class_param_kind_ids: self.collect_imported_class_param_kind_ids(&imports),
        };

        let resolver = Resolver::new(self.session, &self.project.module_table);
        let resolved_info =
            match resolver.resolve_module(module_name, parsed, &cx, &resolver_imports) {
                Ok(resolved) => resolved,
                Err(ResolverError::Resolve(errors)) => {
                    push_resolve_errors(self.session, Some(&self.project.module_table), errors);
                    return Err(FrontendPhase::Resolve);
                }
                Err(ResolverError::Kind(err)) => {
                    self.session.diagnostics().push(err.into_diagnostic(()));
                    return Err(FrontendPhase::Kind);
                }
            };
        self.resolutions
            .insert(module_id, resolved_info.resolutions.clone());
        let resolved = resolved_info.module;

        let imported_typed = self.build_imported_typed_apis(&imports);
        let ImportedTypeInfo {
            func_schemes,
            tycon_kinds,
            constructor_types,
            class_defs: imported_class_defs,
            instance_defs: imported_instance_defs,
            tycon_table: imported_tycon_table,
            ctor_table: imported_ctor_table,
            class_table: imported_class_table,
        } = collect_imported_type_info(&imported_typed);

        let mut check_env = TypeCheckEnv::from_module(&resolved);

        check_env.func_schemes.extend(func_schemes);
        check_env.tycon_kinds.extend(tycon_kinds);
        check_env.constructor_types.extend(constructor_types);
        check_env.class_defs.extend(imported_class_defs);
        for (id, info) in imported_tycon_table.iter() {
            check_env.tycon_table.insert_with_id(id, info.clone());
        }
        for (id, info) in imported_ctor_table.iter() {
            check_env.ctor_table.insert_with_id(id, info.clone());
        }
        for (id, info) in imported_class_table.iter() {
            check_env.class_table.insert_with_id(id, info.clone());
        }
        check_env
            .instance_defs
            .extend(erase_checked_instance_defs_to_resolved(
                imported_instance_defs,
            ));

        let checked_raw = match check_and_elab_module(&resolved, &check_env, file) {
            Ok(c) => c,
            Err(errors) => {
                for error in errors {
                    self.session.diagnostics().push(error.into_diagnostic(()));
                }
                return Err(FrontendPhase::Type);
            }
        };

        if !checked_raw.holes.is_empty() {
            let diagnostics = hole_diagnostics(
                &checked_raw.holes,
                &check_env.constructor_types,
                &check_env.ctor_table,
                check_env.pretty_tables(),
            );

            for diagnostic in diagnostics {
                self.session.diagnostics().push(diagnostic);
            }

            return Err(FrontendPhase::Hole);
        }

        Ok(checked_raw.module)
    }

    fn build_imported_apis(&self, imports: &[EffectiveImport]) -> Vec<ImportedApi> {
        imports
            .iter()
            .map(|imp| ImportedApi {
                alias: imp.alias,
                module: imp.module.clone(),
                api: Rc::new(self.apis[&imp.module].filtered(&imp.spec)),
                spec: imp.spec.clone(),
            })
            .collect()
    }

    fn build_imported_typed_apis(&self, imports: &[EffectiveImport]) -> Vec<ImportedTypedApi> {
        imports
            .iter()
            .filter_map(|imp| {
                self.typed_apis
                    .get(&imp.module)
                    .map(|api| ImportedTypedApi {
                        alias: imp.alias,
                        module: imp.module.clone(),
                        api: api.clone(),
                        spec: imp.spec.clone(),
                    })
            })
            .collect()
    }

    fn collect_imported_kind_ids(
        &self,
        imports: &[EffectiveImport],
    ) -> HashMap<TyConId, solveig_ast::ast::Kind> {
        let mut out = HashMap::new();

        for imp in imports {
            if let Some(api) = self.typed_apis.get(&imp.module) {
                let filtered = api.filtered(&imp.spec);
                for (sym, kind) in &filtered.type_kinds {
                    let id = filtered
                        .type_ids
                        .get(sym)
                        .copied()
                        .expect("ICE: imported type kind missing typed API TyConId");
                    out.insert(id, kind.clone());
                }
            }
        }

        out
    }
}

struct ImportedTypeInfo {
    func_schemes: HashMap<DefId, Scheme>,
    tycon_kinds: HashMap<TyConId, solveig_ast::ast::Kind>,
    constructor_types: HashMap<CtorId, solveig_ast::ast::TypeExpr>,
    class_defs: HashMap<ClassId, ClassDefinition>,
    instance_defs: Vec<CheckedInstanceDef>,
    tycon_table: TyConTable,
    ctor_table: CtorTable,
    class_table: ClassTable,
}

fn collect_imported_type_info(imports: &[ImportedTypedApi]) -> ImportedTypeInfo {
    let mut func_schemes = HashMap::new();
    let mut tycon_kinds = HashMap::new();
    let mut constructor_types = HashMap::new();
    let mut class_defs = HashMap::new();
    let mut instance_defs = Vec::new();
    let mut tycon_table = TyConTable::new();
    let mut ctor_table = CtorTable::new();
    let mut class_table = ClassTable::new();

    for imp in imports {
        let mut display_maps = TypedDisplayMaps {
            func_schemes: &mut func_schemes,
            tycon_kinds: &mut tycon_kinds,
            constructor_types: &mut constructor_types,
            class_defs: &mut class_defs,
            tycon_table: &mut tycon_table,
            ctor_table: &mut ctor_table,
            class_table: &mut class_table,
            instance_defs: &mut instance_defs,
        };

        imp.api
            .filtered(&imp.spec)
            .extend_display_maps(&mut display_maps);
    }

    ImportedTypeInfo {
        func_schemes,
        tycon_kinds,
        constructor_types,
        class_defs,
        instance_defs,
        tycon_table,
        ctor_table,
        class_table,
    }
}

fn find_entry_file(project_root: &Path) -> Result<PathBuf, String> {
    let entry = project_root.join("src").join("main.sv");
    if entry.is_file() {
        Ok(entry)
    } else {
        Err(format!("could not find entry file at {}", entry.display()))
    }
}

fn merge_checked_modules(mods: Vec<CheckedModule>) -> Result<CheckedModule, String> {
    let mut data_defs = Vec::new();
    let mut class_defs = Vec::new();
    let mut instance_defs = Vec::new();
    let mut tycon_kinds = HashMap::new();
    let mut constructor_types = HashMap::new();
    let mut func_schemes = HashMap::new();
    let mut func_defs = Vec::new();

    for m in mods {
        let CheckedModule {
            data_defs: module_data_defs,
            class_defs: module_class_defs,
            instance_defs: module_instance_defs,
            tycon_table: module_tycon_table,
            ctor_table: module_ctor_table,
            class_table: _,
            tycon_kinds: module_tycon_kinds,
            constructor_types: module_constructor_types,
            func_schemes: module_func_schemes,
            func_defs: module_func_defs,
        } = m;

        data_defs.extend(module_data_defs);
        class_defs.extend(module_class_defs);
        instance_defs.extend(module_instance_defs);
        extend_unique_tycon(&mut tycon_kinds, module_tycon_kinds, &module_tycon_table)?;
        extend_unique_ctor(
            &mut constructor_types,
            module_constructor_types,
            &module_ctor_table,
        )?;
        extend_unique_def(&mut func_schemes, module_func_schemes, "function")?;
        func_defs.extend(module_func_defs);
    }

    let mut module = solveig_ast::ast::ResolvedModule {
        data_defs,
        class_defs,
        instance_defs,
        tycon_table: Default::default(),
        ctor_table: Default::default(),
        class_table: Default::default(),
        tycon_kinds,
        constructor_types,
        func_schemes,
        func_defs,
    };
    module.rebuild_identity_tables();

    Ok(module)
}

fn extend_unique_def<T>(
    dst: &mut HashMap<DefId, T>,
    src: HashMap<DefId, T>,
    label: &str,
) -> Result<(), String> {
    for (k, v) in src {
        if dst.insert(k, v).is_some() {
            return Err(format!("duplicate {label} {}", def_display_name(k)));
        }
    }
    Ok(())
}

fn extend_unique_tycon(
    dst: &mut HashMap<TyConId, solveig_ast::ast::Kind>,
    src: HashMap<TyConId, solveig_ast::ast::Kind>,
    names: &TyConTable,
) -> Result<(), String> {
    for (k, v) in src {
        if dst.insert(k, v).is_some() {
            return Err(format!(
                "duplicate type constructor {}",
                tycon_name_for_error(k, names)
            ));
        }
    }
    Ok(())
}

fn extend_unique_ctor(
    dst: &mut HashMap<CtorId, TypeExpr>,
    src: HashMap<CtorId, TypeExpr>,
    names: &CtorTable,
) -> Result<(), String> {
    for (k, v) in src {
        if dst.insert(k, v).is_some() {
            return Err(format!(
                "duplicate constructor {}",
                ctor_name_for_error(k, names)
            ));
        }
    }
    Ok(())
}

fn tycon_name_for_error(id: TyConId, names: &TyConTable) -> String {
    names
        .get(id)
        .map(|info| symbol_display_name(info.name))
        .unwrap_or_else(|| format!("T{}", id.0))
}

fn ctor_name_for_error(id: CtorId, names: &CtorTable) -> String {
    names
        .get(id)
        .map(|info| symbol_display_name(info.name))
        .unwrap_or_else(|| format!("C{}", id.0))
}
