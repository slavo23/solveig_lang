use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use solveig_ast::ast::{
    ClassDefinition, ClassId, ClassTable, CtorId, CtorTable, DefId, Ev, InstanceId, ModuleId,
    PrettyTables, Scheme, TermExpr, TermExprNode, TyConId, TyConTable, TypeExpr, def_display_name,
    symbol_display_name,
};
use solveig_base::{FileId, FileSpan, IntoDiagnostic, Session, SourceFile};
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
    artifacts: Rc<RefCell<Option<CompilationArtifacts>>>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CompileOptions {
    pub dump_bytecode: bool,
    pub dump_evidence: bool,
}

#[derive(Clone, Debug)]
struct CompilationArtifacts {
    checked: HashMap<ModuleId, CheckedModule>,
    module_files: HashMap<ModuleId, FileId>,
    source_files: Vec<SourceFile>,
    tycon_table: TyConTable,
    class_table: ClassTable,
}

struct CheckedProject {
    modules: Vec<CheckedModule>,
    checked: HashMap<ModuleId, CheckedModule>,
    module_files: HashMap<ModuleId, FileId>,
    source_files: Vec<SourceFile>,
}

impl CompilationArtifacts {
    fn from_checked_project(project: &CheckedProject, merged: &CheckedModule) -> Self {
        Self {
            checked: project.checked.clone(),
            module_files: project.module_files.clone(),
            source_files: project.source_files.clone(),
            tycon_table: merged.tycon_table.clone(),
            class_table: merged.class_table.clone(),
        }
    }
}

impl Compilation {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            artifacts: Rc::new(RefCell::new(None)),
        }
    }

    pub fn compile_and_run_project(
        &self,
        entry_name: &str,
        dump_bytecode: bool,
    ) -> Result<(), String> {
        let output = self.compile_project_capture(
            entry_name,
            CompileOptions {
                dump_bytecode,
                dump_evidence: false,
            },
        )?;
        println!("{output}");
        Ok(())
    }

    pub fn compile_and_run_project_with_options(
        &self,
        entry_name: &str,
        options: CompileOptions,
    ) -> Result<(), String> {
        let output = self.compile_project_capture(entry_name, options)?;
        println!("{output}");
        Ok(())
    }

    pub fn run_project_capture(&self, entry_name: &str) -> Result<String, String> {
        self.compile_project_capture(entry_name, CompileOptions::default())
    }

    pub fn checked_project_schemes(&self) -> Result<BTreeMap<String, String>, String> {
        let merged = self.check_and_merge_project()?;
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

    pub fn hover(&self, file: impl AsRef<Path>, offset: usize) -> Option<String> {
        let artifacts = self.artifacts.borrow();
        hover_in_artifacts(artifacts.as_ref()?, file.as_ref(), offset)
    }

    fn compile_project_capture(
        &self,
        entry_name: &str,
        options: CompileOptions,
    ) -> Result<String, String> {
        let merged = self.check_and_merge_project()?;

        if options.dump_evidence {
            dump_solved_evidence(&merged);
        }

        let ir = lower_module(&merged);
        let cc = lower_to_closure_ir(&ir);

        if options.dump_bytecode {
            Machine::dump_bytecode(&cc, "bytecode.txt")
                .map_err(|e| format!("failed to dump bytecode: {e}"))?;
        }

        let entry_fq = format!("main.{entry_name}");
        let result =
            Machine::exec(&cc, &entry_fq).map_err(|err| format!("runtime error: {err}"))?;
        Ok(result.display().to_string())
    }

    fn check_and_merge_project(&self) -> Result<CheckedModule, String> {
        self.artifacts.replace(None);
        let checked_project = check_project_modules(&self.project_root)?;
        let merged = merge_checked_modules(checked_project.modules.clone())?;

        self.artifacts
            .replace(Some(CompilationArtifacts::from_checked_project(
                &checked_project,
                &merged,
            )));

        Ok(merged)
    }
}

#[derive(Clone, Debug)]
struct HoverCandidate {
    span: Range<usize>,
    ty: String,
}

fn hover_in_artifacts(
    artifacts: &CompilationArtifacts,
    file: &Path,
    offset: usize,
) -> Option<String> {
    let file_id = source_file_id(artifacts, file)?;
    let pretty_tables = PrettyTables {
        tycons: &artifacts.tycon_table,
        classes: &artifacts.class_table,
    };
    let mut best = None;

    for (module_id, checked) in &artifacts.checked {
        if artifacts.module_files.get(module_id) != Some(&file_id) {
            continue;
        }

        for func_def in &checked.func_defs {
            hover_expr(&func_def.body, pretty_tables, offset, &mut best);
        }

        for instance in &checked.instance_defs {
            for method in &instance.methods {
                hover_expr(&method.body, pretty_tables, offset, &mut best);
            }
        }
    }

    best.map(|candidate| candidate.ty)
}

fn source_file_id(artifacts: &CompilationArtifacts, file: &Path) -> Option<FileId> {
    artifacts
        .source_files
        .iter()
        .enumerate()
        .find(|(_, source)| source.path == file)
        .or_else(|| {
            artifacts
                .source_files
                .iter()
                .enumerate()
                .find(|(_, source)| file.is_relative() && source.path.ends_with(file))
        })
        .map(|(index, _)| FileId(index as u32))
}

fn hover_expr(
    expr: &TermExprNode<TcMetadata, Range<usize>>,
    pretty_tables: PrettyTables<'_>,
    offset: usize,
    best: &mut Option<HoverCandidate>,
) {
    if !span_contains_offset(&expr.anno.span, offset) {
        return;
    }

    consider_hover_candidate(
        best,
        expr.anno.span.clone(),
        expr.anno.inferred_type.pretty_with_tables(pretty_tables),
    );

    match &expr.node {
        TermExpr::Unit
        | TermExpr::Var(_)
        | TermExpr::Lit(_)
        | TermExpr::Hole
        | TermExpr::Con(_) => {}
        TermExpr::App(func, arg) => {
            hover_expr(func, pretty_tables, offset, best);
            hover_expr(arg, pretty_tables, offset, best);
        }
        TermExpr::Abs(_, _, body)
        | TermExpr::TyAbs(_, _, body)
        | TermExpr::TyApp(body, _)
        | TermExpr::EvAbs(_, _, body)
        | TermExpr::EvApp(body, _)
        | TermExpr::Anno(body, _)
        | TermExpr::UnaryOperator(_, body) => hover_expr(body, pretty_tables, offset, best),
        TermExpr::Case(scrutinee, arms) => {
            hover_expr(scrutinee, pretty_tables, offset, best);
            for arm in arms {
                hover_expr(&arm.node.expr, pretty_tables, offset, best);
            }
        }
        TermExpr::IfThenElse(cond, then_branch, else_branch) => {
            hover_expr(cond, pretty_tables, offset, best);
            hover_expr(then_branch, pretty_tables, offset, best);
            hover_expr(else_branch, pretty_tables, offset, best);
        }
        TermExpr::BinaryOperator(_, lhs, rhs) => {
            hover_expr(lhs, pretty_tables, offset, best);
            hover_expr(rhs, pretty_tables, offset, best);
        }
        TermExpr::Let(_, binder, in_expr) => {
            hover_expr(binder, pretty_tables, offset, best);
            hover_expr(in_expr, pretty_tables, offset, best);
        }
    }
}

fn span_contains_offset(span: &Range<usize>, offset: usize) -> bool {
    if span.start == span.end {
        offset == span.start
    } else {
        span.start <= offset && offset < span.end
    }
}

fn consider_hover_candidate(best: &mut Option<HoverCandidate>, span: Range<usize>, ty: String) {
    let replace = best
        .as_ref()
        .is_none_or(|current| span_len(&span) <= span_len(&current.span));

    if replace {
        *best = Some(HoverCandidate { span, ty });
    }
}

fn span_len(span: &Range<usize>) -> usize {
    span.end.saturating_sub(span.start)
}

fn dump_solved_evidence(module: &CheckedModule) {
    let tables = PrettyTables {
        tycons: &module.tycon_table,
        classes: &module.class_table,
    };
    let mut printed_any = false;

    eprintln!("=== Evidence ===");

    for func in &module.func_defs {
        let mut lines = Vec::new();
        collect_evidence_lines(&func.body, tables, &mut lines);
        printed_any |= print_evidence_block(&func.name, &lines);
    }

    for inst in &module.instance_defs {
        let owner = format!(
            "instance#{} {}",
            inst.id.0,
            inst.head.pretty_with_tables(tables)
        );
        let mut lines = inst
            .context
            .iter()
            .zip(inst.context_evidence.iter())
            .enumerate()
            .map(|(index, (pred, evvar))| {
                format!(
                    "ctx[{index}] %ev{} : {}",
                    evvar.0,
                    pred.pretty_with_tables(tables)
                )
            })
            .collect::<Vec<_>>();
        lines.extend(
            inst.superclass_evidence
                .iter()
                .enumerate()
                .map(|(index, ev)| format!("super[{index}] = {}", format_ev(ev, tables))),
        );
        printed_any |= print_evidence_block(&owner, &lines);

        for method in &inst.methods {
            let mut lines = Vec::new();
            collect_evidence_lines(&method.body, tables, &mut lines);
            printed_any |=
                print_evidence_block(&format!("instance#{}::{}", inst.id.0, method.name), &lines);
        }
    }

    if !printed_any {
        eprintln!("  (none)");
    }
}

fn print_evidence_block(owner: &str, lines: &[String]) -> bool {
    if lines.is_empty() {
        return false;
    }

    eprintln!("{owner}:");
    for line in lines {
        eprintln!("  {line}");
    }
    true
}

fn collect_evidence_lines(
    expr: &TermExprNode<TcMetadata, Range<usize>>,
    tables: PrettyTables<'_>,
    out: &mut Vec<String>,
) {
    match &expr.node {
        TermExpr::EvAbs(evvar, pred, body) => {
            out.push(format!(
                "lambda %ev{} : {}",
                evvar.0,
                pred.pretty_with_tables(tables)
            ));
            collect_evidence_lines(body, tables, out);
        }
        TermExpr::EvApp(body, ev) => {
            out.push(format!("apply {}", format_ev(ev, tables)));
            collect_evidence_lines(body, tables, out);
        }
        TermExpr::App(func, arg) => {
            collect_evidence_lines(func, tables, out);
            collect_evidence_lines(arg, tables, out);
        }
        TermExpr::Abs(_, _, body)
        | TermExpr::TyAbs(_, _, body)
        | TermExpr::TyApp(body, _)
        | TermExpr::Anno(body, _)
        | TermExpr::UnaryOperator(_, body) => collect_evidence_lines(body, tables, out),
        TermExpr::Case(scrutinee, arms) => {
            collect_evidence_lines(scrutinee, tables, out);
            for arm in arms {
                collect_evidence_lines(&arm.node.expr, tables, out);
            }
        }
        TermExpr::IfThenElse(cond, then_branch, else_branch) => {
            collect_evidence_lines(cond, tables, out);
            collect_evidence_lines(then_branch, tables, out);
            collect_evidence_lines(else_branch, tables, out);
        }
        TermExpr::BinaryOperator(_, lhs, rhs) => {
            collect_evidence_lines(lhs, tables, out);
            collect_evidence_lines(rhs, tables, out);
        }
        TermExpr::Let(_, binder, in_expr) => {
            collect_evidence_lines(binder, tables, out);
            collect_evidence_lines(in_expr, tables, out);
        }
        TermExpr::Unit
        | TermExpr::Var(_)
        | TermExpr::Lit(_)
        | TermExpr::Hole
        | TermExpr::Con(_) => {}
    }
}

fn format_ev(ev: &Ev, tables: PrettyTables<'_>) -> String {
    match ev {
        Ev::Var(evvar) => format!("%ev{}", evvar.0),
        Ev::Placeholder(evvar) => format!("<placeholder %ev{}>", evvar.0),
        Ev::Inst(instance_id, ty_args, sub_evidence) => {
            let ty_args = if ty_args.is_empty() {
                String::new()
            } else {
                format!(
                    " @{}",
                    ty_args
                        .iter()
                        .map(|ty| ty.pretty_with_tables(tables))
                        .collect::<Vec<_>>()
                        .join(" @")
                )
            };
            let sub_evidence = if sub_evidence.is_empty() {
                String::new()
            } else {
                format!(
                    " ({})",
                    sub_evidence
                        .iter()
                        .map(|ev| format_ev(ev, tables))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };

            format!("inst#{}{}{}", instance_id.0, ty_args, sub_evidence)
        }
        Ev::Super(root, class_id, index) => {
            format!(
                "super({}).class#{}.{}",
                format_ev(root, tables),
                class_id.0,
                index
            )
        }
    }
}

pub fn compile_and_run_project(
    project_root: &Path,
    entry_name: &str,
    dump_bytecode: bool,
) -> Result<(), String> {
    Compilation::new(project_root).compile_and_run_project(entry_name, dump_bytecode)
}

pub fn compile_and_run_project_with_options(
    project_root: &Path,
    entry_name: &str,
    options: CompileOptions,
) -> Result<(), String> {
    Compilation::new(project_root).compile_and_run_project_with_options(entry_name, options)
}

pub fn run_project_capture(project_root: &Path, entry_name: &str) -> Result<String, String> {
    Compilation::new(project_root).run_project_capture(entry_name)
}

pub fn checked_project_schemes(project_root: &Path) -> Result<BTreeMap<String, String>, String> {
    Compilation::new(project_root).checked_project_schemes()
}

fn check_project_modules(project_root: &Path) -> Result<CheckedProject, String> {
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

    if !session.diagnostics().is_empty() {
        emit_session_diagnostics(&session)?;
    }

    let source_files = session.source_map().files().to_vec();
    Ok(state.into_checked_project(source_files))
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

struct FrontendState<'a> {
    session: &'a Session,
    project: &'a Project,
    parsed: &'a ParsedProject,
    builtins: Rc<BuiltinApi>,
    apis: HashMap<ModuleName, SharedModuleApi>,
    typed_apis: HashMap<ModuleName, SharedTypedModuleApi>,
    resolutions: HashMap<ModuleId, Vec<(FileSpan, DefId)>>,
    checked_modules: Vec<CheckedModule>,
    checked: HashMap<ModuleId, CheckedModule>,
    module_files: HashMap<ModuleId, FileId>,
    next_instance_id: u32,
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
            checked: HashMap::new(),
            module_files: HashMap::new(),
            next_instance_id: 0,
        })
    }

    fn into_checked_project(self, source_files: Vec<SourceFile>) -> CheckedProject {
        CheckedProject {
            modules: self.checked_modules,
            checked: self.checked,
            module_files: self.module_files,
            source_files,
        }
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
            self.module_files.insert(module_id, parsed.file);
            self.checked.insert(module_id, checked.clone());
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
        let mut resolved = resolved_info.module;
        self.stamp_instance_ids(&mut resolved);

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
        check_env.instance_defs.extend(
            imported_instance_defs
                .into_iter()
                .map(|inst| inst.map_anno(&mut |meta| meta.span, &mut |span| span)),
        );

        let checked_raw = match check_and_elab_module(&resolved, &check_env, file) {
            Ok(c) => c,
            Err(errors) => {
                for error in errors {
                    self.session.diagnostics().push(error.into_diagnostic(()));
                }
                return Err(FrontendPhase::Type);
            }
        };

        for diagnostic in &checked_raw.diagnostics {
            self.session
                .diagnostics()
                .push(diagnostic.into_diagnostic(()));
        }

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

    fn stamp_instance_ids(&mut self, module: &mut solveig_ast::ast::ResolvedModule<Range<usize>>) {
        for inst in &mut module.instance_defs {
            let id = self.next_instance_id;
            self.next_instance_id = self
                .next_instance_id
                .checked_add(1)
                .expect("ICE: instance id space exhausted");
            inst.id = InstanceId(id);
        }
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
    instance_defs: Vec<solveig_ast::ast::InstanceDefinition<TcMetadata, Range<usize>>>,
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
