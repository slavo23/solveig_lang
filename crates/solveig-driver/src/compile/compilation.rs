//! Public driver API and retained compilation state.
//! This module owns `Compilation` and the run/check entry points used by the CLI and tests.

use super::*;

pub struct Compilation {
    pub(super) project_root: PathBuf,
    pub(super) entry_file: Option<PathBuf>,
    pub(super) session: Session,
    pub(super) names: Names,
    pub(super) source_provider: SourceProvider,
    pub(super) artifacts: Option<CompilationArtifacts>,
    pub(super) checked_project: Option<CheckedProject>,
    pub(super) semantic_tokens: Vec<SourceSemanticToken>,
    pub(super) code_actions: Vec<SourceCodeAction>,
    pub(super) prelude_cache: Option<PreludeCache>,
    pub(super) incremental_cache: Option<IncrementalCache>,
    check_result: CheckResult,
}

#[derive(Clone, Debug, Default)]
pub struct CheckResult {
    diagnostics: Vec<Diagnostic>,
    modules: Vec<CheckedModule>,
    source_map: SourceMap,
    freshly_checked_modules: Vec<String>,
    reused_modules: Vec<String>,
    success: bool,
    failure_message: Option<&'static str>,
}

pub type ProjectSchemes = BTreeMap<String, String>;
pub type ProjectDocs = BTreeMap<String, String>;

/// Semantic hover payload retained by the driver after a successful check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoverInfo {
    file: FileId,
    span: Range<usize>,
    signature: String,
    documentation: Option<String>,
}

impl HoverInfo {
    /// Creates a semantic hover for a byte span in one source file.
    pub fn new(
        file: FileId,
        span: Range<usize>,
        signature: impl Into<String>,
        documentation: Option<String>,
    ) -> Self {
        Self {
            file,
            span,
            signature: signature.into(),
            documentation,
        }
    }

    /// Returns this hover's span with its source file id attached.
    pub fn file_span(&self) -> FileSpan {
        FileSpan::new(
            self.file,
            solveig_base::Span::from_usize_range(self.span.clone()),
        )
    }

    /// Returns this hover's byte span inside its source file.
    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }

    /// Returns the Solveig signature, type, or module path heading to display.
    pub fn signature(&self) -> &str {
        &self.signature
    }

    /// Returns documentation text attached to the hovered semantic item, when available.
    pub fn documentation(&self) -> Option<&str> {
        self.documentation.as_deref()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CompileOptions {
    pub dump_bytecode: bool,
    pub dump_evidence: bool,
    pub verify_core: bool,
}

impl CompileOptions {
    pub fn test_support() -> Self {
        Self {
            verify_core: true,
            ..Self::default()
        }
    }
}

impl CheckResult {
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn modules(&self) -> &[CheckedModule] {
        &self.modules
    }

    pub fn source_map(&self) -> &SourceMap {
        &self.source_map
    }

    pub fn freshly_checked_modules(&self) -> &[String] {
        &self.freshly_checked_modules
    }

    pub fn reused_modules(&self) -> &[String] {
        &self.reused_modules
    }

    pub fn success(&self) -> bool {
        self.success
    }

    pub fn failure_message(&self) -> Option<&'static str> {
        self.failure_message
    }
}

#[derive(Clone, Debug)]
pub(super) struct CompilationArtifacts {
    pub(super) names: Names,
    pub(super) checked: FxHashMap<ModuleId, CheckedModule>,
    pub(super) module_files: FxHashMap<ModuleId, FileId>,
    pub(super) source_files: Vec<SourceFile>,
    pub(super) docs: FxHashMap<DefId, DocComment>,
    pub(super) resolutions: FxHashMap<ModuleId, Vec<(FileSpan, DefId)>>,
    pub(super) declaration_spans: Vec<(FileSpan, DefId)>,
    pub(super) module_display_names: FxHashMap<ModuleId, String>,
    pub(super) module_paths: FxHashMap<ModuleId, PathBuf>,
    pub(super) module_imports: Vec<(FileSpan, ModuleId)>,
    pub(super) source_symbols: Vec<SourceSymbol>,
    pub(super) tycon_table: TyConTable,
    pub(super) ctor_table: CtorTable,
    pub(super) class_table: ClassTable,
}

#[derive(Clone, Debug)]
pub(super) struct CheckedProject {
    pub(super) names: Names,
    pub(super) modules: Vec<CheckedModule>,
    pub(super) checked: FxHashMap<ModuleId, CheckedModule>,
    pub(super) module_files: FxHashMap<ModuleId, FileId>,
    pub(super) source_files: Vec<SourceFile>,
    pub(super) docs: FxHashMap<DefId, DocComment>,
    pub(super) resolutions: FxHashMap<ModuleId, Vec<(FileSpan, DefId)>>,
    pub(super) declaration_spans: Vec<(FileSpan, DefId)>,
    pub(super) module_display_names: FxHashMap<ModuleId, String>,
    pub(super) module_paths: FxHashMap<ModuleId, PathBuf>,
    pub(super) module_imports: Vec<(FileSpan, ModuleId)>,
    pub(super) source_symbols: Vec<SourceSymbol>,
}

#[derive(Clone, Debug)]
pub(super) struct PreludeCache {
    pub(super) project_modules: FxHashMap<ModuleName, ModuleFile>,
    pub(super) module_ids: FxHashMap<ModuleName, ModuleId>,
    pub(super) names: Names,
    pub(super) builtins: Arc<BuiltinApi>,
    pub(super) apis: FxHashMap<ModuleName, SharedModuleApi>,
    pub(super) typed_apis: FxHashMap<ModuleName, SharedTypedModuleApi>,
    pub(super) checked: FxHashMap<ModuleId, CheckedModule>,
    pub(super) module_files: FxHashMap<ModuleId, FileId>,
    pub(super) resolutions: FxHashMap<ModuleId, Vec<(FileSpan, DefId)>>,
    pub(super) next_instance_id: u32,
    pub(super) verified_core: bool,
}

struct CapturedRun {
    output: String,
}

struct PreparedRun {
    cc: CCProgram,
    entry_fq: String,
    entry_is_io: bool,
    show_entry: Option<String>,
}

impl CompilationArtifacts {
    fn from_checked_project(project: &CheckedProject) -> Self {
        let (tycon_table, ctor_table, class_table) = checked_project_tables(&project.modules);

        Self {
            names: project.names.clone(),
            checked: project.checked.clone(),
            module_files: project.module_files.clone(),
            source_files: project.source_files.clone(),
            docs: project.docs.clone(),
            resolutions: project.resolutions.clone(),
            declaration_spans: project.declaration_spans.clone(),
            module_display_names: project.module_display_names.clone(),
            module_paths: project.module_paths.clone(),
            module_imports: project.module_imports.clone(),
            source_symbols: project.source_symbols.clone(),
            tycon_table,
            ctor_table,
            class_table,
        }
    }
}

impl PreludeCache {
    pub(super) fn can_reuse(&self, modules: &ModuleTable, verify_core: bool) -> bool {
        if verify_core && !self.verified_core {
            return false;
        }

        self.module_ids
            .iter()
            .all(|(module, cached_id)| modules.id(module) == Some(*cached_id))
    }
}

impl Compilation {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            entry_file: None,
            session: Session::new(),
            names: Names::new(),
            source_provider: SourceProvider::default(),
            artifacts: None,
            checked_project: None,
            semantic_tokens: Vec::new(),
            code_actions: Vec::new(),
            prelude_cache: None,
            incremental_cache: None,
            check_result: CheckResult::default(),
        }
    }

    pub fn for_source_file(source_file: impl Into<PathBuf>) -> Self {
        let source_file = source_file.into();
        let (project_root, entry_file) = discover_project_for_source_file(&source_file);
        let mut compilation = Self::new(project_root);
        compilation.entry_file = entry_file;
        compilation
    }

    pub fn compile_and_run_project(
        &mut self,
        entry_name: &str,
        dump_bytecode: bool,
    ) -> Result<(), DriverError> {
        self.compile_and_run_project_with_options(
            entry_name,
            CompileOptions {
                dump_bytecode,
                dump_evidence: false,
                verify_core: false,
            },
        )
    }

    pub fn compile_and_run_project_with_options(
        &mut self,
        entry_name: &str,
        options: CompileOptions,
    ) -> Result<(), DriverError> {
        self.compile_and_run_project_with_options_and_args(entry_name, options, &[])
    }

    /// Sets editor-provided source text for a project path, shadowing disk on the next check.
    pub fn set_overlay(&mut self, path: impl AsRef<Path>, text: impl Into<String>) {
        let path = self.project_source_path(path.as_ref());
        self.source_provider.set_overlay(path, text);
        self.clear_retained_project();
    }

    /// Clears editor-provided source text for a project path, exposing disk again on the next check.
    pub fn clear_overlay(&mut self, path: impl AsRef<Path>) {
        let path = self.project_source_path(path.as_ref());
        self.source_provider.clear_overlay(path);
        self.clear_retained_project();
    }

    pub fn compile_and_run_project_with_options_and_args(
        &mut self,
        entry_name: &str,
        options: CompileOptions,
        args: &[String],
    ) -> Result<(), DriverError> {
        let stdin = io::stdin();
        let mut stdin = stdin.lock();
        let stdout = io::stdout();
        let mut stdout = stdout.lock();

        self.run_project_with_streams(entry_name, options, &mut stdin, &mut stdout, args)
    }

    pub fn run_project_with_streams(
        &mut self,
        entry_name: &str,
        options: CompileOptions,
        stdin: &mut dyn BufRead,
        stdout: &mut dyn Write,
        args: &[String],
    ) -> Result<(), DriverError> {
        let prepared = self.prepare_project_run(entry_name, options)?;

        if prepared.entry_is_io {
            Machine::exec_io_with_streams(
                &prepared.cc,
                &prepared.entry_fq,
                stdin,
                stdout,
                args,
                &self.project_root,
            )
            .map_err(|err| DriverError::runtime(err.to_string()))?;
        } else if let Some(show_entry) = prepared.show_entry {
            let result = Machine::exec(&prepared.cc, &show_entry)
                .map_err(|err| DriverError::runtime(err.to_string()))?;
            let output = result
                .as_string()
                .ok_or_else(|| DriverError::runtime("Show main did not return String"))?;
            writeln!(stdout, "{output}").map_err(|source| DriverError::WriteOutput { source })?;
        } else {
            let result = Machine::exec(&prepared.cc, &prepared.entry_fq)
                .map_err(|err| DriverError::runtime(err.to_string()))?;
            writeln!(stdout, "{}", result.display())
                .map_err(|source| DriverError::WriteOutput { source })?;
        }

        Ok(())
    }

    pub fn run_project_capture(&mut self, entry_name: &str) -> Result<String, DriverError> {
        self.compile_project_capture(entry_name, CompileOptions::test_support())
    }

    pub fn run_project_capture_with_stdin(
        &mut self,
        entry_name: &str,
        stdin: &str,
    ) -> Result<String, DriverError> {
        self.run_project_capture_with_stdin_and_args(entry_name, stdin, &[])
    }

    pub fn run_project_capture_with_stdin_and_args(
        &mut self,
        entry_name: &str,
        stdin: &str,
        args: &[String],
    ) -> Result<String, DriverError> {
        self.compile_project_capture_inner(entry_name, CompileOptions::test_support(), stdin, args)
            .map(|capture| capture.output)
    }

    pub fn checked_project_schemes(&mut self) -> Result<ProjectSchemes, DriverError> {
        let failure_message = self.check(true)?.failure_message();
        if let Some(message) = failure_message {
            return Err(DriverError::compilation_failed(message));
        }

        let checked_project = self
            .checked_project
            .as_ref()
            .expect("ICE: successful check did not retain checked project");
        let (tycon_table, _ctor_table, class_table) =
            checked_project_tables(&checked_project.modules);
        let pretty_tables = PrettyTables {
            names: &checked_project.names,
            tycons: &tycon_table,
            classes: &class_table,
        };
        let mut schemes = BTreeMap::new();

        for module in &checked_project.modules {
            for (id, scheme) in &module.func_schemes {
                schemes.insert(
                    checked_project.names.def_display_name(*id),
                    scheme.pretty_with_tables(pretty_tables),
                );
            }
        }

        Ok(schemes)
    }

    pub fn checked_project_docs(&mut self) -> Result<ProjectDocs, DriverError> {
        let failure_message = self.check(true)?.failure_message();
        if let Some(message) = failure_message {
            return Err(DriverError::compilation_failed(message));
        }

        let checked_project = self
            .checked_project
            .as_ref()
            .expect("ICE: successful check did not retain checked project");
        let mut docs = BTreeMap::new();

        for (id, doc) in &checked_project.docs {
            docs.insert(
                checked_project.names.def_display_name(*id),
                doc.text.clone(),
            );
        }

        Ok(docs)
    }

    pub fn check(&mut self, verify_core: bool) -> Result<&CheckResult, DriverError> {
        self.artifacts = None;
        self.checked_project = None;
        self.semantic_tokens = Vec::new();
        self.code_actions = Vec::new();
        self.session.diagnostics().drain();
        self.names = Names::new();

        let outcome = self.check_project_modules(verify_core)?;
        let diagnostics = self.session.diagnostics().sorted_by_file_and_span();
        let modules = outcome
            .checked_project
            .as_ref()
            .map(|project| project.modules.clone())
            .unwrap_or_default();
        let source_map = self.session.source_map().clone();
        let success = outcome.phase.is_none()
            && !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Error);

        if let Some(checked_project) = outcome.checked_project {
            self.artifacts = Some(CompilationArtifacts::from_checked_project(&checked_project));
            self.checked_project = Some(checked_project);
        }
        self.semantic_tokens = outcome.semantic_tokens;
        self.code_actions = outcome.code_actions;

        self.check_result = CheckResult {
            diagnostics,
            modules,
            source_map,
            freshly_checked_modules: outcome.freshly_checked_modules,
            reused_modules: outcome.reused_modules,
            success,
            failure_message: outcome.phase.map(FrontendPhase::error_message),
        };

        Ok(&self.check_result)
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        self.check_result.diagnostics()
    }

    pub fn source_map(&self) -> &SourceMap {
        self.check_result.source_map()
    }

    fn compile_project_capture(
        &mut self,
        entry_name: &str,
        options: CompileOptions,
    ) -> Result<String, DriverError> {
        self.compile_project_capture_inner(entry_name, options, "", &[])
            .map(|capture| capture.output)
    }

    fn compile_project_capture_inner(
        &mut self,
        entry_name: &str,
        options: CompileOptions,
        stdin: &str,
        args: &[String],
    ) -> Result<CapturedRun, DriverError> {
        let prepared = self.prepare_project_run(entry_name, options)?;

        if prepared.entry_is_io {
            let result = Machine::exec_io(
                &prepared.cc,
                &prepared.entry_fq,
                stdin,
                args,
                &self.project_root,
            )
            .map_err(|err| DriverError::runtime(err.to_string()))?;
            Ok(CapturedRun {
                output: result.stdout,
            })
        } else if let Some(show_entry) = prepared.show_entry {
            let result = Machine::exec(&prepared.cc, &show_entry)
                .map_err(|err| DriverError::runtime(err.to_string()))?;
            let output = result
                .as_string()
                .ok_or_else(|| DriverError::runtime("Show main did not return String"))?
                .to_string();
            Ok(CapturedRun { output })
        } else {
            let result = Machine::exec(&prepared.cc, &prepared.entry_fq)
                .map_err(|err| DriverError::runtime(err.to_string()))?;
            Ok(CapturedRun {
                output: result.display().to_string(),
            })
        }
    }

    fn prepare_project_run(
        &mut self,
        entry_name: &str,
        options: CompileOptions,
    ) -> Result<PreparedRun, DriverError> {
        let failure_message = self.check(options.verify_core)?.failure_message();
        if let Some(message) = failure_message {
            return Err(DriverError::compilation_failed(message));
        }

        let mut checked_project = self
            .checked_project
            .as_ref()
            .expect("ICE: successful check did not retain checked project")
            .clone();
        let mut merged =
            merge_checked_modules(checked_project.modules.clone(), &mut checked_project.names)?;

        let entry_fq = format!("main.{entry_name}");
        let entry_is_io = entry_returns_io(&merged, &checked_project.names, &entry_fq);
        let show_entry = if entry_is_io {
            None
        } else {
            add_show_entry_wrapper(&mut merged, &mut checked_project, &entry_fq)
        };

        if options.dump_evidence {
            dump_solved_evidence(&merged, &checked_project.names);
        }

        if options.verify_core {
            let diagnostics = verify_lowering_core(&merged, &checked_project);
            if !diagnostics.is_empty() {
                self.record_post_check_failure(FrontendPhase::Core, diagnostics, &checked_project);
                return Err(DriverError::compilation_failed(
                    FrontendPhase::Core.error_message(),
                ));
            }
        }

        let ir = lower_module(&merged, &checked_project.names);
        let cc = lower_to_closure_ir(&ir);

        if options.dump_bytecode {
            Machine::dump_bytecode(&cc, "bytecode.txt")
                .map_err(|source| DriverError::DumpBytecode { source })?;
        }

        Ok(PreparedRun {
            cc,
            entry_fq,
            entry_is_io,
            show_entry,
        })
    }

    fn record_post_check_failure(
        &mut self,
        phase: FrontendPhase,
        diagnostics: Vec<Diagnostic>,
        checked_project: &CheckedProject,
    ) {
        for diagnostic in diagnostics {
            self.session.diagnostics().push(diagnostic);
        }

        let diagnostics = self.session.diagnostics().sorted_by_file_and_span();
        self.check_result = CheckResult {
            diagnostics,
            modules: checked_project.modules.clone(),
            source_map: self.session.source_map().clone(),
            freshly_checked_modules: Vec::new(),
            reused_modules: Vec::new(),
            success: false,
            failure_message: Some(phase.error_message()),
        };
    }

    fn project_source_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.project_root.join(path)
        }
    }

    fn clear_retained_project(&mut self) {
        self.artifacts = None;
        self.checked_project = None;
        self.semantic_tokens = Vec::new();
        self.code_actions = Vec::new();
        self.check_result = CheckResult::default();
    }
}

fn discover_project_for_source_file(source_file: &Path) -> (PathBuf, Option<PathBuf>) {
    let source_dir = source_file.parent().unwrap_or_else(|| Path::new("."));

    for candidate in source_dir.ancestors() {
        if candidate.join("src").join("main.sv").is_file() {
            return (candidate.to_path_buf(), None);
        }
    }

    (source_dir.to_path_buf(), Some(source_file.to_path_buf()))
}

fn checked_project_tables(modules: &[CheckedModule]) -> (TyConTable, CtorTable, ClassTable) {
    let mut tycon_table = TyConTable::new();
    let mut ctor_table = CtorTable::new();
    let mut class_table = ClassTable::new();

    for module in modules {
        for (id, info) in module.tycon_table.iter() {
            tycon_table.insert_with_id(id, info.clone());
        }

        for (id, info) in module.ctor_table.iter() {
            ctor_table.insert_with_id(id, info.clone());
        }

        for (id, info) in module.class_table.iter() {
            class_table.insert_with_id(id, info.clone());
        }
    }

    (tycon_table, ctor_table, class_table)
}

pub fn compile_and_run_project(
    project_root: &Path,
    entry_name: &str,
    dump_bytecode: bool,
) -> Result<(), DriverError> {
    Compilation::new(project_root).compile_and_run_project(entry_name, dump_bytecode)
}

pub fn compile_and_run_project_with_options(
    project_root: &Path,
    entry_name: &str,
    options: CompileOptions,
) -> Result<(), DriverError> {
    Compilation::new(project_root).compile_and_run_project_with_options(entry_name, options)
}

pub fn compile_and_run_project_with_options_and_args(
    project_root: &Path,
    entry_name: &str,
    options: CompileOptions,
    args: &[String],
) -> Result<(), DriverError> {
    Compilation::new(project_root)
        .compile_and_run_project_with_options_and_args(entry_name, options, args)
}

pub fn run_project_capture(project_root: &Path, entry_name: &str) -> Result<String, DriverError> {
    Compilation::new(project_root).run_project_capture(entry_name)
}

pub fn run_project_capture_with_stdin(
    project_root: &Path,
    entry_name: &str,
    stdin: &str,
) -> Result<String, DriverError> {
    Compilation::new(project_root).run_project_capture_with_stdin(entry_name, stdin)
}

pub fn run_project_capture_with_stdin_and_args(
    project_root: &Path,
    entry_name: &str,
    stdin: &str,
    args: &[String],
) -> Result<String, DriverError> {
    Compilation::new(project_root).run_project_capture_with_stdin_and_args(entry_name, stdin, args)
}

pub fn checked_project_schemes(project_root: &Path) -> Result<ProjectSchemes, DriverError> {
    Compilation::new(project_root).checked_project_schemes()
}

pub fn checked_project_docs(project_root: &Path) -> Result<ProjectDocs, DriverError> {
    Compilation::new(project_root).checked_project_docs()
}
