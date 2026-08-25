//! Retained per-compilation checked-module cache for editor-speed rechecks.

use super::*;

#[derive(Clone, Debug)]
pub(super) struct PackageCache {
    pub(super) packages: FxHashMap<PackageId, CachedPackage>,
    pub(super) names: Names,
    pub(super) apis: FxHashMap<ModuleName, SharedModuleApi>,
    pub(super) typed_apis: FxHashMap<ModuleName, SharedTypedModuleApi>,
    pub(super) checked: FxHashMap<ModuleId, CheckedModule>,
    pub(super) module_files: FxHashMap<ModuleId, FileId>,
    pub(super) resolutions: FxHashMap<ModuleId, Vec<(FileSpan, DefId)>>,
    pub(super) non_reusable_modules: FxHashSet<ModuleName>,
    pub(super) instance_modules: FxHashSet<ModuleName>,
    pub(super) next_instance_id: u32,
    pub(super) verified_core: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct CachedPackage {
    pub(super) module_ids: FxHashMap<ModuleName, ModuleId>,
    pub(super) source_hashes: FxHashMap<ModuleName, u64>,
}

impl PackageCache {
    pub(super) fn reusable_modules(
        &self,
        project: &Project,
        verify_core: bool,
    ) -> FxHashSet<ModuleName> {
        if verify_core && !self.verified_core {
            return FxHashSet::default();
        }

        let invalidated = self.invalidated_modules(project);
        let mut reusable = FxHashSet::default();

        for module_name in project.modules.keys() {
            if invalidated.contains(module_name)
                || self.non_reusable_modules.contains(module_name)
                || !self.has_current_artifacts(project, module_name)
            {
                continue;
            }

            reusable.insert(module_name.clone());
        }

        reusable
    }

    fn invalidated_modules(&self, project: &Project) -> FxHashSet<ModuleName> {
        let mut invalidated = FxHashSet::default();
        let mut reverse_imports = FxHashMap::<ModuleName, Vec<ModuleName>>::default();

        for (module_name, module_file) in &project.modules {
            let module_id = project
                .module_table
                .id(module_name)
                .expect("ICE: invalidation referenced unknown module");

            let cached_package = self.packages.get(&module_file.package);
            if cached_package.and_then(|package| package.module_ids.get(module_name))
                != Some(&module_id)
                || cached_package.and_then(|package| package.source_hashes.get(module_name))
                    != Some(&module_file.source_hash)
                || !self.has_current_artifacts(project, module_name)
            {
                invalidated.insert(module_name.clone());
            }

            let imports = effective_imports(
                project,
                module_name,
                &module_file.imports,
                module_file.parsed.file,
            );
            for import in imports {
                reverse_imports
                    .entry(import.module)
                    .or_default()
                    .push(module_name.clone());
            }
        }

        let mut stack = invalidated.iter().cloned().collect::<Vec<_>>();
        while let Some(module_name) = stack.pop() {
            let Some(dependents) = reverse_imports.get(&module_name) else {
                continue;
            };

            for dependent in dependents {
                if invalidated.insert(dependent.clone()) {
                    stack.push(dependent.clone());
                }
            }
        }

        let current_instance_modules = project
            .modules
            .iter()
            .filter(|(_, module)| parsed_module_defines_instances(&module.parsed))
            .map(|(name, _)| name.clone());
        let instance_graph_changed = current_instance_modules
            .chain(self.instance_modules.iter().cloned())
            .any(|module| invalidated.contains(&module));
        if instance_graph_changed {
            invalidated.extend(project.modules.keys().cloned());
        }

        invalidated
    }

    fn has_current_artifacts(&self, project: &Project, module_name: &ModuleName) -> bool {
        let Some(module_id) = project.module_table.id(module_name) else {
            return false;
        };

        self.checked.contains_key(&module_id)
            && self.module_files.contains_key(&module_id)
            && self.resolutions.contains_key(&module_id)
            && self.apis.contains_key(module_name)
            && self.typed_apis.contains_key(module_name)
    }
}

pub(super) fn parsed_module_defines_instances(parsed: &ParsedModule) -> bool {
    parsed.defs.iter().any(|definition| {
        matches!(
            &definition.0,
            solveig_syntax::ast::SurfaceDefinition::InstanceDef { .. }
        )
    })
}
