use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use base::utils::diagnostics::emit_type_errors;
use core::ast::{ClassDefinition, Scheme};
use lowering::closure_lower::lower_to_closure_ir;
use lowering::lowering::lower_module;
use runtime::vm::Machine;
use surface::ast::ModuleName;
use syntax::inference::holes::emit_holes;
use syntax::inference::type_infer::{check_and_elab_module, CheckedModule, TypeCheckEnv};

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
    let elaborated = syntax::inference::class_elab::elaborate_typeclasses(&merged);
    let ir = lower_module(&elaborated);
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

fn erase_tc_expr_to_span_expr(
    expr: core::ast::TermExprNode<
        syntax::inference::type_infer::TcMetadata,
        std::ops::Range<usize>,
    >,
) -> core::ast::TermExprNode<std::ops::Range<usize>, std::ops::Range<usize>> {
    let anno = expr.anno.span.clone();

    let node = match expr.node {
        core::ast::TermExpr::Var(name) => core::ast::TermExpr::Var(name),
        core::ast::TermExpr::Con(name) => core::ast::TermExpr::Con(name),
        core::ast::TermExpr::Unit => core::ast::TermExpr::Unit,
        core::ast::TermExpr::Lit(lit) => core::ast::TermExpr::Lit(lit),
        core::ast::TermExpr::Hole => core::ast::TermExpr::Hole,

        core::ast::TermExpr::Abs(param, ann, body) => {
            core::ast::TermExpr::Abs(param, ann, Box::new(erase_tc_expr_to_span_expr(*body)))
        }

        core::ast::TermExpr::App(f, x) => core::ast::TermExpr::App(
            Box::new(erase_tc_expr_to_span_expr(*f)),
            Box::new(erase_tc_expr_to_span_expr(*x)),
        ),

        core::ast::TermExpr::TyAbs(name, kind, body) => {
            core::ast::TermExpr::TyAbs(name, kind, Box::new(erase_tc_expr_to_span_expr(*body)))
        }

        core::ast::TermExpr::TyApp(inner, ty) => {
            core::ast::TermExpr::TyApp(Box::new(erase_tc_expr_to_span_expr(*inner)), ty)
        }

        core::ast::TermExpr::Anno(inner, ty) => {
            core::ast::TermExpr::Anno(Box::new(erase_tc_expr_to_span_expr(*inner)), ty)
        }

        core::ast::TermExpr::Case(scrutinee, arms) => core::ast::TermExpr::Case(
            Box::new(erase_tc_expr_to_span_expr(*scrutinee)),
            arms.into_iter()
                .map(|arm| {
                    core::ast::CaseArmNode::new(
                        core::ast::CaseArm {
                            pattern: arm.node.pattern,
                            expr: erase_tc_expr_to_span_expr(arm.node.expr),
                        },
                        arm.anno,
                    )
                })
                .collect(),
        ),

        core::ast::TermExpr::IfThenElse(c, t, e) => core::ast::TermExpr::IfThenElse(
            Box::new(erase_tc_expr_to_span_expr(*c)),
            Box::new(erase_tc_expr_to_span_expr(*t)),
            Box::new(erase_tc_expr_to_span_expr(*e)),
        ),

        core::ast::TermExpr::BinaryOperator(op, l, r) => core::ast::TermExpr::BinaryOperator(
            op,
            Box::new(erase_tc_expr_to_span_expr(*l)),
            Box::new(erase_tc_expr_to_span_expr(*r)),
        ),

        core::ast::TermExpr::UnaryOperator(op, rhs) => {
            core::ast::TermExpr::UnaryOperator(op, Box::new(erase_tc_expr_to_span_expr(*rhs)))
        }

        core::ast::TermExpr::Let(name, binder, in_expr) => core::ast::TermExpr::Let(
            name,
            Box::new(erase_tc_expr_to_span_expr(*binder)),
            Box::new(erase_tc_expr_to_span_expr(*in_expr)),
        ),
    };

    core::ast::TermExprNode::new(node, anno)
}

fn erase_checked_instance_defs_to_resolved(
    defs: Vec<
        core::ast::InstanceDefinition<
            syntax::inference::type_infer::TcMetadata,
            std::ops::Range<usize>,
        >,
    >,
) -> Vec<core::ast::InstanceDefinition<std::ops::Range<usize>, std::ops::Range<usize>>> {
    defs.into_iter()
        .map(|inst| core::ast::InstanceDefinition {
            context: inst.context,
            head: inst.head,
            methods: inst
                .methods
                .into_iter()
                .map(|m| core::ast::InstanceMethodDef {
                    name: m.name,
                    body: erase_tc_expr_to_span_expr(m.body),
                })
                .collect(),
        })
        .collect()
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

    fn collect_imported_class_param_kinds(
        &self,
        imports: &[crate::project::EffectiveImport],
    ) -> HashMap<String, Vec<core::ast::Kind>> {
        let mut out = HashMap::new();

        for imp in imports {
            if let Some(api) = self.typed_apis.get(&imp.module) {
                for (sym, class_def) in &api.filtered(&imp.spec).class_defs {
                    out.insert(
                        sym.display(),
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
            class_param_kinds: self.collect_imported_class_param_kinds(&imports),
        };

        let resolver = Resolver::new();
        let resolved = resolver
            .resolve_module(module_name, parsed, &cx, &resolver_imports)
            .map_err(|errs| {
                format!(
                    "resolve error in {}:\n{errs:#?}",
                    module_file.path.display()
                )
            })?;

        let imported_typed = self.build_imported_typed_apis(&imports);
        let (
            func_schemes,
            tycon_kinds,
            constructor_types,
            imported_class_defs,
            imported_instance_defs,
        ) = collect_imported_type_info(&imported_typed);

        let mut check_env = TypeCheckEnv::from_module(&resolved);

        check_env.func_schemes.extend(func_schemes);
        check_env.tycon_kinds.extend(tycon_kinds);
        check_env.constructor_types.extend(constructor_types);
        check_env
            .class_defs
            .extend(imported_class_defs.into_values());
        check_env
            .instance_defs
            .extend(erase_checked_instance_defs_to_resolved(
                imported_instance_defs,
            ));

        let checked_raw = match check_and_elab_module(
            &resolved,
            &check_env,
            module_file.path.display().to_string(),
        ) {
            Ok(c) => c,
            Err(errors) => {
                emit_type_errors(&errors, &module_file.source)
                    .map_err(|err| format!("failed to print type errors: {err}"))?;
                return Err("type error".to_string());
            }
        };

        if !checked_raw.holes.is_empty() {
            emit_holes(
                &checked_raw.holes,
                &module_file.source,
                &check_env.constructor_types,
            );
            return Err("Program has unfilled holes. Aborting compilation".to_string());
        }

        Ok(checked_raw.module)
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
    HashMap<String, Scheme>,
    HashMap<String, core::ast::Kind>,
    HashMap<String, core::ast::TypeExpr>,
    HashMap<String, ClassDefinition>,
    Vec<
        core::ast::InstanceDefinition<
            syntax::inference::type_infer::TcMetadata,
            std::ops::Range<usize>,
        >,
    >,
) {
    let mut func_schemes = HashMap::new();
    let mut tycon_kinds = HashMap::new();
    let mut constructor_types = HashMap::new();
    let mut class_defs = HashMap::new();
    let mut instance_defs = Vec::new();

    for imp in imports {
        imp.api.filtered(&imp.spec).extend_display_maps(
            &mut func_schemes,
            &mut tycon_kinds,
            &mut constructor_types,
            &mut class_defs,
            &mut instance_defs,
        );
    }

    (
        func_schemes,
        tycon_kinds,
        constructor_types,
        class_defs,
        instance_defs,
    )
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
        .func_schemes
        .retain(|name, _| name.starts_with(&prefix));

    checked
        .class_defs
        .retain(|class_def| class_def.name.starts_with(&prefix));

    checked
        .instance_defs
        .retain(|inst_def| inst_def.head.class_name.starts_with(&prefix));

    checked
        .func_defs
        .retain(|def| def.name.starts_with(&prefix));

    checked
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
        data_defs.extend(m.data_defs);
        class_defs.extend(m.class_defs);
        instance_defs.extend(m.instance_defs);
        extend_unique(&mut tycon_kinds, m.tycon_kinds, "type constructor")?;
        extend_unique(&mut constructor_types, m.constructor_types, "constructor")?;
        extend_unique(&mut func_schemes, m.func_schemes, "function")?;
        func_defs.extend(m.func_defs);
    }

    Ok(core::ast::ResolvedModule {
        data_defs,
        class_defs,
        instance_defs,
        tycon_kinds,
        constructor_types,
        func_schemes,
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
