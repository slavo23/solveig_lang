//! Public driver API and retained compilation state.
//! This module owns `Compilation` and the run/check entry points used by the CLI and tests.

use super::*;

pub struct Compilation {
    pub(super) project_root: PathBuf,
    pub(super) entry_file: Option<PathBuf>,
    pub(super) session: Session,
    pub(super) names: Names,
    pub(super) source_provider: SourceProvider,
    pub(super) snapshot: Option<CompilationSnapshot>,
    pub(super) checked_project: Option<CheckedProject>,
    pub(super) prelude_cache: Option<PreludeCache>,
    pub(super) incremental_cache: Option<IncrementalCache>,
    pub(super) retained_verify_core: Option<bool>,
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

#[derive(Clone, Debug)]
pub struct ModuleResolveErrors {
    pub module: ModuleName,
    pub errors: Vec<ResolveError>,
}

/// Retains compiler and project facts from the last frontend check.
#[derive(Clone, Debug)]
pub struct CompilationSnapshot {
    pub source_root: PathBuf,
    pub project: Project,
    pub parsed: ParsedProject,
    pub names: Names,
    pub apis: FxHashMap<ModuleName, SharedModuleApi>,
    pub typed_apis: FxHashMap<ModuleName, SharedTypedModuleApi>,
    pub modules: Vec<CheckedModule>,
    pub checked: FxHashMap<ModuleId, CheckedModule>,
    pub module_files: FxHashMap<ModuleId, FileId>,
    pub source_files: Vec<SourceFile>,
    pub resolutions: FxHashMap<ModuleId, Vec<(FileSpan, DefId)>>,
    pub resolve_errors: Vec<ModuleResolveErrors>,
    pub type_errors: Vec<TypeError>,
    pub holes: Vec<HoleInfo>,
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
pub(super) struct CheckedProject {
    pub(super) names: Names,
    pub(super) modules: Vec<CheckedModule>,
    pub(super) module_files: FxHashMap<ModuleId, FileId>,
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

impl CheckedProject {
    fn from_snapshot(snapshot: &CompilationSnapshot) -> Self {
        Self {
            names: snapshot.names.clone(),
            modules: snapshot.modules.clone(),
            module_files: snapshot.module_files.clone(),
        }
    }
}

impl PreludeCache {
    pub(super) fn can_reuse(&self, project: &Project, verify_core: bool) -> bool {
        if verify_core && !self.verified_core {
            return false;
        }

        self.module_ids.iter().all(|(module, cached_id)| {
            project.module_table.id(module) == Some(*cached_id)
                && self
                    .project_modules
                    .get(module)
                    .zip(project.modules.get(module))
                    .is_some_and(|(cached, current)| cached.source_hash == current.source_hash)
        })
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
            snapshot: None,
            checked_project: None,
            prelude_cache: None,
            incremental_cache: None,
            retained_verify_core: None,
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
        let mut stdin = io::BufReader::new(io::stdin());
        let mut stdout = io::stdout();

        self.run_project_with_streams(entry_name, options, &mut stdin, &mut stdout, args)
    }

    pub fn run_project_with_streams(
        &mut self,
        entry_name: &str,
        options: CompileOptions,
        stdin: &mut (dyn BufRead + Send),
        stdout: &mut (dyn Write + Send),
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

        let snapshot = self
            .snapshot
            .as_ref()
            .expect("ICE: successful check did not retain compilation snapshot");
        let (tycon_table, _ctor_table, class_table) = checked_project_tables(&snapshot.modules);
        let pretty_tables = PrettyTables::new(&snapshot.names, &tycon_table, &class_table);
        let mut schemes = BTreeMap::new();

        for module in &snapshot.modules {
            for (id, scheme) in &module.func_schemes {
                schemes.insert(
                    snapshot.names.def_display_name(*id),
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

        let snapshot = self
            .snapshot
            .as_ref()
            .expect("ICE: successful check did not retain compilation snapshot");
        let mut docs = BTreeMap::new();

        for api in snapshot.typed_apis.values() {
            for (id, doc) in &api.docs {
                docs.insert(snapshot.names.def_display_name(*id), doc.text.clone());
            }
        }

        Ok(docs)
    }

    pub fn check(&mut self, verify_core: bool) -> Result<&CheckResult, DriverError> {
        self.snapshot = None;
        self.checked_project = None;
        self.retained_verify_core = None;
        self.session.diagnostics().drain();
        self.names = Names::new();

        let outcome = self.check_project_modules(verify_core)?;
        let diagnostics = self.session.diagnostics().sorted_by_file_and_span();
        let modules = outcome
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.modules.clone())
            .unwrap_or_default();
        let source_map = self.session.source_map().clone();
        let success = outcome.phase.is_none()
            && !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Error);

        if let Some(snapshot) = outcome.snapshot {
            self.checked_project = Some(CheckedProject::from_snapshot(&snapshot));
            self.snapshot = Some(snapshot);
        }

        self.check_result = CheckResult {
            diagnostics,
            modules,
            source_map,
            freshly_checked_modules: outcome.freshly_checked_modules,
            reused_modules: outcome.reused_modules,
            success,
            failure_message: outcome.phase.map(FrontendPhase::error_message),
        };
        self.retained_verify_core = Some(verify_core);

        Ok(&self.check_result)
    }

    pub fn ensure_checked(&mut self, verify_core: bool) -> Result<&CheckResult, DriverError> {
        let reusable_verify_core = self
            .retained_verify_core
            .is_some_and(|retained| retained || !verify_core);
        if reusable_verify_core && !self.check_result.source_map().is_empty() {
            return Ok(&self.check_result);
        }

        self.check(verify_core)
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        self.check_result.diagnostics()
    }

    pub fn source_map(&self) -> &SourceMap {
        self.check_result.source_map()
    }

    pub fn snapshot(&self) -> Option<&CompilationSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    pub fn source_provider(&self) -> &SourceProvider {
        &self.source_provider
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
        self.snapshot = None;
        self.checked_project = None;
        self.retained_verify_core = None;
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

pub(super) fn checked_project_tables(
    modules: &[CheckedModule],
) -> (TyConTable, CtorTable, ClassTable) {
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
