// Untyped and typed API projections for package visibility and re-exports.

impl ModuleApi {
    pub fn definition(&self, name: Symbol) -> Option<DefId> {
        let mut matches = self
            .terms
            .get(&name)
            .into_iter()
            .chain(self.types.get(&name))
            .chain(self.type_aliases.get(&name))
            .chain(self.classes.get(&name))
            .copied();
        let first = matches.next()?;
        matches.all(|candidate| candidate == first).then_some(first)
    }

    pub fn visible_to(&self, package: PackageId) -> Self {
        if package == self.package {
            self.without(&self.private)
        } else {
            self.only(&self.external)
        }
    }

    /// Returns the defining module's names without bindings published by re-export.
    pub fn local_view(&self) -> Self {
        self.without(&self.reexported)
    }

    fn without(&self, names: &ApiNames) -> Self {
        let mut visible = self.clone();
        visible
            .terms
            .retain(|name, _| !names.terms.contains(name));
        visible
            .types
            .retain(|name, _| !names.types.contains(name));
        visible
            .type_aliases
            .retain(|name, _| !names.type_aliases.contains(name));
        visible
            .constructors
            .retain(|name, _| !names.constructors.contains(name));
        visible
            .classes
            .retain(|name, _| !names.classes.contains(name));
        visible.retain_metadata();
        visible
    }

    fn only(&self, names: &ApiNames) -> Self {
        let mut visible = self.clone();
        visible.terms.retain(|name, _| names.terms.contains(name));
        visible.types.retain(|name, _| names.types.contains(name));
        visible
            .type_aliases
            .retain(|name, _| names.type_aliases.contains(name));
        visible
            .constructors
            .retain(|name, _| names.constructors.contains(name));
        visible
            .classes
            .retain(|name, _| names.classes.contains(name));
        visible.retain_metadata();
        visible
    }

    fn retain_metadata(&mut self) {
        self.fixities
            .retain(|name, _| self.terms.contains_key(name));
        self.class_methods.retain(|name, methods| {
            if !self.classes.contains_key(name) {
                return false;
            }
            methods.retain(|method| self.terms.contains_key(method));
            true
        });
        self.data_constructors.retain(|name, constructors| {
            if !self.types.contains_key(name) {
                return false;
            }
            constructors.retain(|constructor| self.constructors.contains_key(constructor));
            true
        });
        let definitions = self
            .terms
            .values()
            .chain(self.types.values())
            .chain(self.type_aliases.values())
            .chain(self.constructors.values())
            .chain(self.classes.values())
            .copied()
            .collect::<FxHashSet<_>>();
        self.docs.retain(|id, _| definitions.contains(id));
    }
}

impl TypedModuleApi {
    pub fn visible_to(&self, module_api: &ModuleApi, package: PackageId) -> Self {
        let visible = module_api.visible_to(package);
        self.retain_for_api(&visible)
    }

    fn retain_for_api(&self, visible: &ModuleApi) -> Self {
        let mut projected = self.clone();
        projected
            .term_schemes
            .retain(|id, _| visible.terms.values().any(|candidate| candidate == id));
        projected
            .type_kinds
            .retain(|id, _| visible.types.values().any(|candidate| candidate == id));
        projected
            .type_ids
            .retain(|id, _| projected.type_kinds.contains_key(id));
        projected.type_aliases.retain(|id, _| {
            visible
                .type_aliases
                .values()
                .any(|candidate| candidate == id)
        });
        projected.constructor_types.retain(|id, _| {
            visible
                .constructors
                .values()
                .any(|candidate| candidate == id)
        });
        projected
            .constructor_ids
            .retain(|id, _| projected.constructor_types.contains_key(id));

        let mut visible_classes = projected
            .class_defs
            .iter()
            .filter_map(|(id, class)| {
                visible
                    .classes
                    .values()
                    .any(|candidate| candidate == id)
                    .then_some(class.id)
            })
            .collect::<FxHashSet<_>>();
        loop {
            let previous = visible_classes.len();
            for class in projected.class_defs.values() {
                if visible_classes.contains(&class.id) {
                    visible_classes.extend(
                        class
                            .superclasses
                            .iter()
                            .map(|superclass| superclass.class_id),
                    );
                }
            }
            if visible_classes.len() == previous {
                break;
            }
        }
        projected
            .class_defs
            .retain(|_, class| visible_classes.contains(&class.id));
        let visible_docs = projected
            .term_schemes
            .keys()
            .chain(projected.type_kinds.keys())
            .chain(projected.type_aliases.keys())
            .chain(projected.constructor_types.keys())
            .chain(projected.class_defs.keys())
            .copied()
            .collect::<FxHashSet<_>>();
        projected.docs.retain(|id, _| visible_docs.contains(id));
        projected
    }

    /// Selects definitions whose stable IDs occur in an already projected API.
    pub fn selected_by_api(&self, selected: &ModuleApi) -> Self {
        self.retain_for_api(selected)
    }

    /// Projects typed definitions through the exact untyped name selection.
    pub fn imported_view(
        &self,
        module_api: &ModuleApi,
        package: PackageId,
        spec: &ImportSpec,
    ) -> Self {
        let selected = module_api.visible_to(package).filtered(spec);
        self.retain_for_api(&selected)
    }

    /// Adds typed definitions republished by `module_api` from dependency APIs.
    pub fn include_reexports<'a>(
        &mut self,
        module_api: &ModuleApi,
        dependencies: impl IntoIterator<Item = &'a TypedModuleApi>,
    ) {
        let exported_ids = module_api
            .external
            .terms
            .iter()
            .filter_map(|name| module_api.terms.get(name))
            .chain(
                module_api
                    .external
                    .types
                    .iter()
                    .filter_map(|name| module_api.types.get(name)),
            )
            .chain(
                module_api
                    .external
                    .type_aliases
                    .iter()
                    .filter_map(|name| module_api.type_aliases.get(name)),
            )
            .chain(
                module_api
                    .external
                    .constructors
                    .iter()
                    .filter_map(|name| module_api.constructors.get(name)),
            )
            .chain(
                module_api
                    .external
                    .classes
                    .iter()
                    .filter_map(|name| module_api.classes.get(name)),
            )
            .copied()
            .collect::<FxHashSet<_>>();

        for dependency in dependencies {
            self.term_schemes.extend(
                dependency
                    .term_schemes
                    .iter()
                    .filter(|(id, _)| exported_ids.contains(id))
                    .map(|(id, scheme)| (*id, scheme.clone())),
            );
            self.type_kinds.extend(
                dependency
                    .type_kinds
                    .iter()
                    .filter(|(id, _)| exported_ids.contains(id))
                    .map(|(id, kind)| (*id, kind.clone())),
            );
            self.type_ids.extend(
                dependency
                    .type_ids
                    .iter()
                    .filter(|(id, _)| exported_ids.contains(id))
                    .map(|(id, tycon)| (*id, *tycon)),
            );
            self.type_aliases.extend(
                dependency
                    .type_aliases
                    .iter()
                    .filter(|(id, _)| exported_ids.contains(id))
                    .map(|(id, alias)| (*id, alias.clone())),
            );
            self.constructor_types.extend(
                dependency
                    .constructor_types
                    .iter()
                    .filter(|(id, _)| exported_ids.contains(id))
                    .map(|(id, ty)| (*id, ty.clone())),
            );
            self.constructor_ids.extend(
                dependency
                    .constructor_ids
                    .iter()
                    .filter(|(id, _)| exported_ids.contains(id))
                    .map(|(id, ctor)| (*id, *ctor)),
            );
            let exported_classes = dependency
                .class_defs
                .keys()
                .filter(|id| exported_ids.contains(id))
                .copied()
                .collect::<Vec<_>>();
            for class in exported_classes {
                dependency.include_class_with_superclasses(self, &class);
            }
            self.docs.extend(
                dependency
                    .docs
                    .iter()
                    .filter(|(id, _)| exported_ids.contains(id))
                    .map(|(id, docs)| (*id, docs.clone())),
            );
            for (id, info) in dependency.tycon_table.iter() {
                self.tycon_table.insert_with_id(id, info.clone());
            }
            for (id, info) in dependency.ctor_table.iter() {
                self.ctor_table.insert_with_id(id, info.clone());
            }
            for (id, info) in dependency.class_table.iter() {
                self.class_table.insert_with_id(id, info.clone());
            }
            for instance in &dependency.instance_defs {
                if !self
                    .instance_defs
                    .iter()
                    .any(|existing| existing.id == instance.id)
                {
                    self.instance_defs.push(instance.clone());
                }
            }
        }
    }
}
