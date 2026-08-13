impl<'a> FrontendState<'a> {
    fn capture_package_cache(&self) -> PackageCache {
        let mut packages = FxHashMap::<PackageId, CachedPackage>::default();

        for (module_name, module_file) in &self.project.modules {
            let module_id = self
                .project
                .module_table
                .id(module_name)
                .expect("ICE: cache capture referenced unknown module");

            if !self.checked.contains_key(&module_id) {
                continue;
            }

            let package = packages.entry(module_file.package).or_default();
            package.module_ids.insert(module_name.clone(), module_id);
            package
                .source_hashes
                .insert(module_name.clone(), module_file.source_hash);
        }

        PackageCache {
            packages,
            names: self.names.clone(),
            apis: self.apis.clone(),
            typed_apis: self.typed_apis.clone(),
            checked: self.checked.clone(),
            module_files: self.module_files.clone(),
            resolutions: self.resolutions.clone(),
            non_reusable_modules: self.non_reusable_modules.clone(),
            next_instance_id: self.next_instance_id,
            verified_core: self.verify_core,
        }
    }

}

fn rebase_resolutions_file(
    resolutions: &[(FileSpan, DefId)],
    file: FileId,
) -> Vec<(FileSpan, DefId)> {
    resolutions
        .iter()
        .map(|(span, def_id)| (FileSpan::new(file, span.span), *def_id))
        .collect()
}
