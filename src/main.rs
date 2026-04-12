mod project;
mod resolver;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use base::utils::diagnostics::emit_type_errors;
use clap::Parser;
use resolver::{Resolver, ResolverImports};
use syntax::inference::type_infer::{check_and_elab_module, CheckedModule};

pub mod compile;
use compile::{closure_lower::lower_to_closure_ir, lowering::lower_module, vm::Machine};

use project::{
    build_module_api, build_module_graph, build_typed_module_api, ensure_acyclic_modules,
    gather_imported_type_info, load_project, module_order, BuiltinApi, ImportedApi,
    ImportedTypedApi, ModuleContext, SharedModuleApi, SharedTypedModuleApi,
};

use surface::ast::ModuleName;

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(long, default_value = ".")]
    project_root: PathBuf,

    #[arg(short, long, default_value = "main")]
    entry: String,

    #[arg(long, default_value = "false")]
    dump_bytecode: bool,
}

fn main() {
    let cli = Cli::parse();

    if let Err(err) = compile_and_run_project(&cli.project_root, &cli.entry, cli.dump_bytecode) {
        eprintln!("{err}");
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
    let mut tycon_kinds = HashMap::new();
    let mut constructor_types = HashMap::new();
    let mut func_types = HashMap::new();
    let mut func_defs = Vec::new();

    for m in mods {
        data_defs.extend(m.data_defs);

        for (k, v) in m.tycon_kinds {
            if tycon_kinds.insert(k.clone(), v).is_some() {
                return Err(format!("duplicate type constructor {}", k));
            }
        }

        for (k, v) in m.constructor_types {
            if constructor_types.insert(k.clone(), v).is_some() {
                return Err(format!("duplicate constructor {}", k));
            }
        }

        for (k, v) in m.func_types {
            if func_types.insert(k.clone(), v).is_some() {
                return Err(format!("duplicate function {}", k));
            }
        }

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

fn compile_and_run_project(
    project_root: &Path,
    entry_name: &str,
    dump_bytecode: bool,
) -> Result<(), String> {
    let entry_file = find_entry_file(project_root)?;

    let (_root, project) = load_project(&entry_file)?;
    let graph = build_module_graph(&project)?;
    ensure_acyclic_modules(&graph)?;
    let order = module_order(&graph)?;

    let builtins = Rc::new(BuiltinApi::default());

    let mut apis: HashMap<ModuleName, SharedModuleApi> = HashMap::new();
    for module_name in &order {
        let module_file = &project.modules[module_name];
        let parsed = project::parse_module(module_file)?;
        let api: SharedModuleApi = Rc::new(build_module_api(module_name, &parsed)?);
        apis.insert(module_name.clone(), api);
    }

    let mut typed_apis: HashMap<ModuleName, SharedTypedModuleApi> = HashMap::new();
    let mut checked_modules: Vec<CheckedModule> = Vec::new();

    for module_name in &order {
        let module_file = &project.modules[module_name];
        let parsed = project::parse_module(module_file)?;

        let mut imported_apis: Vec<ImportedApi> = parsed
            .imports
            .iter()
            .map(|imp| ImportedApi {
                alias: imp.alias.clone(),
                api: Rc::new(project::filter_api(&apis[&imp.module], &imp.spec)),
                spec: imp.spec.clone(),
            })
            .collect();

        if !module_name.as_string().starts_with("prelude.") {
            for implicit in [
                ModuleName(vec!["prelude".into(), "builtins".into()]),
                ModuleName(vec!["prelude".into(), "list".into()]),
            ] {
                imported_apis.push(ImportedApi {
                    alias: implicit.0.last().unwrap().clone(),
                    api: apis[&implicit].clone(),
                    spec: surface::ast::ImportSpec::All,
                });
            }
        }

        let cx = ModuleContext {
            module: module_name.clone(),
            local_api: apis[module_name].clone(),
            imported_apis,
            builtins: builtins.clone(),
        };

        let mut imported_tycon_kinds: HashMap<String, core::ast::Kind> = parsed
            .imports
            .iter()
            .flat_map(|imp| {
                typed_apis[&imp.module]
                    .type_kinds
                    .iter()
                    .map(|(sym, kind)| (sym.display(), kind.clone()))
            })
            .collect();

        if !module_name.as_string().starts_with("prelude.") {
            for implicit in [
                ModuleName(vec!["prelude".into(), "builtins".into()]),
                ModuleName(vec!["prelude".into(), "list".into()]),
            ] {
                for (sym, kind) in &typed_apis[&implicit].type_kinds {
                    imported_tycon_kinds.insert(sym.display(), kind.clone());
                }
            }
        }

        let resolver = Resolver::new();
        let mut resolved = resolver
            .resolve_module(
                module_name,
                &parsed,
                &cx,
                &ResolverImports {
                    tycon_kinds: imported_tycon_kinds,
                },
            )
            .map_err(|errs| {
                format!(
                    "resolve error in {}:\n{errs:#?}",
                    module_file.path.display()
                )
            })?;

        let mut imported_typed: Vec<ImportedTypedApi> = parsed
            .imports
            .iter()
            .map(|imp| ImportedTypedApi {
                alias: imp.alias.clone(),
                api: typed_apis[&imp.module].clone(),
                spec: imp.spec.clone(),
            })
            .collect();

        if !module_name.as_string().starts_with("prelude.") {
            for implicit in [
                ModuleName(vec!["prelude".into(), "builtins".into()]),
                ModuleName(vec!["prelude".into(), "list".into()]),
            ] {
                imported_typed.push(ImportedTypedApi {
                    alias: implicit.0.last().unwrap().clone(),
                    api: typed_apis[&implicit].clone(),
                    spec: surface::ast::ImportSpec::All,
                });
            }
        }

        let (func_types, tycon_kinds, constructor_types) =
            gather_imported_type_info(&imported_typed);

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

        let checked = retain_only_local_symbols(module_name, checked_raw);

        let typed_api: SharedTypedModuleApi =
            Rc::new(build_typed_module_api(module_name, &checked));
        typed_apis.insert(module_name.clone(), typed_api);

        checked_modules.push(checked);
    }

    let merged = merge_checked_modules(checked_modules)?;
    let ir = lower_module(&merged);
    let cc = lower_to_closure_ir(&ir);

    let entry_fq = format!("main.{}", entry_name);
    if dump_bytecode {
        Machine::dump_bytecode(&cc, "bytecode.txt").expect("failed to dump bytecode");
    }

    let result = Machine::exec(&cc, &entry_fq);

    println!("{}", result.display());
    Ok(())
}
