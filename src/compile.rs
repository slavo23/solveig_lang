use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use base::utils::diagnostics::emit_type_errors;
use lowering::closure_lower::lower_to_closure_ir;
use lowering::lowering::lower_module;
use runtime::vm::Machine;
use surface::ast::ModuleName;
use syntax::inference::type_infer::{check_and_elab_module, CheckedModule};

use crate::project::{
    build_module_api, build_typed_module_api, effective_imports, load_project,
    module_order_checked, parse_project, BuiltinApi, ImportedApi, ImportedTypedApi, ModuleContext,
    ParsedProject, Project, SharedModuleApi, SharedTypedModuleApi,
};
use crate::resolver::{Resolver, ResolverImports};

pub fn compile_and_run_project(
    project_root: &Path,
    entry_name: &str,
    dump_bytecode: bool,
) -> Result<(), String> {
    let entry_file = find_entry_file(project_root)?;
    let (_root, project) = load_project(&entry_file)?;
    let order = module_order_checked(&project)?;
    let parsed = parse_project(&project)?;

    let mut state = FrontendState::new(&project, &parsed, &order)?;
    state.check_all(&order)?;

    let merged = merge_checked_modules(state.checked_modules)?;
    let ir = lower_module(&merged);
    let cc = lower_to_closure_ir(&ir);

    if dump_bytecode {
        Machine::dump_bytecode(&cc, "bytecode.txt")
            .map_err(|e| format!("failed to dump bytecode: {e}"))?;
    }

    let entry_fq = format!("main.{entry_name}");
    let result = Machine::exec(&cc, &entry_fq);
    println!("{}", result.display());
    Ok(())
}

struct FrontendState<'a> {
    project: &'a Project,
    parsed: &'a ParsedProject<'a>,
    builtins: Rc<BuiltinApi>,
    apis: HashMap<ModuleName, SharedModuleApi>,
    typed_apis: HashMap<ModuleName, SharedTypedModuleApi>,
    checked_modules: Vec<CheckedModule>,
}

impl<'a> FrontendState<'a> {
    fn new(
        project: &'a Project,
        parsed: &'a ParsedProject<'a>,
        order: &[ModuleName],
    ) -> Result<Self, String> {
        let mut apis = HashMap::new();

        for module_name in order {
            let api = build_module_api(module_name, parsed.get(module_name));
            apis.insert(module_name.clone(), Rc::new(api));
        }

        Ok(Self {
            project,
            parsed,
            builtins: Rc::new(BuiltinApi::default()),
            apis,
            typed_apis: HashMap::new(),
            checked_modules: Vec::new(),
        })
    }

    fn check_all(&mut self, order: &[ModuleName]) -> Result<(), String> {
        for module_name in order {
            let checked = self.check_module(module_name)?;
            let typed_api = Rc::new(build_typed_module_api(module_name, &checked));

            self.typed_apis.insert(module_name.clone(), typed_api);
            self.checked_modules.push(checked);
        }

        Ok(())
    }

    fn check_module(&self, module_name: &ModuleName) -> Result<CheckedModule, String> {
        let module_file = &self.project.modules[module_name];
        let parsed = self.parsed.get(module_name);
        let imports = effective_imports(module_name, &parsed.imports);

        let imported_apis = self.build_imported_apis(&imports);
        let cx = ModuleContext {
            module: module_name.clone(),
            local_api: self.apis[module_name].clone(),
            imported_apis,
            builtins: self.builtins.clone(),
        };

        let resolver_imports = ResolverImports {
            tycon_kinds: self.collect_imported_kinds(&imports),
        };

        let resolver = Resolver::new();
        let mut resolved = resolver
            .resolve_module(module_name, parsed, &cx, &resolver_imports)
            .map_err(|errs| {
                format!(
                    "resolve error in {}:\n{errs:#?}",
                    module_file.path.display()
                )
            })?;

        let imported_typed = self.build_imported_typed_apis(&imports);
        let (func_types, tycon_kinds, constructor_types) =
            collect_imported_type_info(&imported_typed);

        resolved.func_types.extend(func_types);
        resolved.tycon_kinds.extend(tycon_kinds);
        resolved.constructor_types.extend(constructor_types);

        let checked_raw =
            match check_and_elab_module(&resolved, module_file.path.display().to_string()) {
                Ok(c) => c,
                Err(errors) => {
                    emit_type_errors(&errors, &module_file.source)
                        .map_err(|err| format!("failed to print type errors: {err}"))?;
                    return Err("type error".to_string());
                }
            };

        Ok(retain_only_local_symbols(module_name, checked_raw))
    }

    fn build_imported_apis(&self, imports: &[crate::project::EffectiveImport]) -> Vec<ImportedApi> {
        imports
            .iter()
            .map(|imp| ImportedApi {
                alias: imp.alias.clone(),
                api: Rc::new(self.apis[&imp.module].filtered(&imp.spec)),
                spec: imp.spec.clone(),
            })
            .collect()
    }

    fn build_imported_typed_apis(
        &self,
        imports: &[crate::project::EffectiveImport],
    ) -> Vec<ImportedTypedApi> {
        imports
            .iter()
            .filter_map(|imp| {
                self.typed_apis
                    .get(&imp.module)
                    .map(|api| ImportedTypedApi {
                        alias: imp.alias.clone(),
                        api: api.clone(),
                        spec: imp.spec.clone(),
                    })
            })
            .collect()
    }

    fn collect_imported_kinds(
        &self,
        imports: &[crate::project::EffectiveImport],
    ) -> HashMap<String, core::ast::Kind> {
        let mut out = HashMap::new();

        for imp in imports {
            if let Some(api) = self.typed_apis.get(&imp.module) {
                for (sym, kind) in &api.filtered(&imp.spec).type_kinds {
                    out.insert(sym.display(), kind.clone());
                }
            }
        }

        out
    }
}

fn collect_imported_type_info(
    imports: &[ImportedTypedApi],
) -> (
    HashMap<String, core::ast::TypeExpr>,
    HashMap<String, core::ast::Kind>,
    HashMap<String, core::ast::TypeExpr>,
) {
    let mut func_types = HashMap::new();
    let mut tycon_kinds = HashMap::new();
    let mut constructor_types = HashMap::new();

    for imp in imports {
        imp.api.filtered(&imp.spec).extend_display_maps(
            &mut func_types,
            &mut tycon_kinds,
            &mut constructor_types,
        );
    }

    (func_types, tycon_kinds, constructor_types)
}

fn find_entry_file(project_root: &Path) -> Result<PathBuf, String> {
    let entry = project_root.join("src").join("main.sv");
    if entry.is_file() {
        Ok(entry)
    } else {
        Err(format!("could not find entry file at {}", entry.display()))
    }
}

fn retain_only_local_symbols(
    module_name: &ModuleName,
    mut checked: CheckedModule,
) -> CheckedModule {
    let prefix = module_name.as_string() + ".";

    checked
        .tycon_kinds
        .retain(|name, _| name.starts_with(&prefix));
    checked
        .constructor_types
        .retain(|name, _| name.starts_with(&prefix));
    checked
        .func_types
        .retain(|name, _| name.starts_with(&prefix));

    checked
}

fn merge_checked_modules(mods: Vec<CheckedModule>) -> Result<CheckedModule, String> {
    let mut data_defs = Vec::new();
    let mut tycon_kinds = HashMap::new();
    let mut constructor_types = HashMap::new();
    let mut func_types = HashMap::new();
    let mut func_defs = Vec::new();

    for m in mods {
        data_defs.extend(m.data_defs);
        extend_unique(&mut tycon_kinds, m.tycon_kinds, "type constructor")?;
        extend_unique(&mut constructor_types, m.constructor_types, "constructor")?;
        extend_unique(&mut func_types, m.func_types, "function")?;
        func_defs.extend(m.func_defs);
    }

    Ok(core::ast::ResolvedModule {
        data_defs,
        tycon_kinds,
        constructor_types,
        func_types,
        func_defs,
    })
}

fn extend_unique<T>(
    dst: &mut HashMap<String, T>,
    src: HashMap<String, T>,
    label: &str,
) -> Result<(), String> {
    for (k, v) in src {
        if dst.insert(k.clone(), v).is_some() {
            return Err(format!("duplicate {label} {k}"));
        }
    }
    Ok(())
}
