// Stable module identities and definition-path rendering.

#[derive(Debug, Clone)]
pub struct ModuleTable {
    names: Vec<ModuleName>,
    packages: Vec<PackageId>,
    ids: FxHashMap<ModuleName, ModuleId>,
}

impl ModuleTable {
    pub fn new(
        session: &Session,
        modules: impl IntoIterator<Item = (ModuleName, PackageId)>,
    ) -> Self {
        let mut entries = modules.into_iter().collect::<Vec<_>>();
        entries.sort_by_key(|(module, _)| module.as_string(session));
        entries.dedup_by(|(left, _), (right, _)| left == right);

        let names = entries
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        let packages = entries
            .iter()
            .map(|(_, package)| *package)
            .collect::<Vec<_>>();
        let ids = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let id = ModuleId(
                    u32::try_from(index).expect("ICE: module table exceeded u32 capacity"),
                );
                (name.clone(), id)
            })
            .collect();

        Self {
            names,
            packages,
            ids,
        }
    }

    pub fn id(&self, module: &ModuleName) -> Option<ModuleId> {
        self.ids.get(module).copied()
    }

    pub fn name(&self, id: ModuleId) -> &ModuleName {
        self.names
            .get(id.0 as usize)
            .expect("ICE: ModuleId not present in ModuleTable")
    }

    pub fn package(&self, id: ModuleId) -> PackageId {
        *self
            .packages
            .get(id.0 as usize)
            .expect("ICE: ModuleId package not present in ModuleTable")
    }

    pub fn package_for_name(&self, module: &ModuleName) -> Option<PackageId> {
        self.id(module).map(|id| self.package(id))
    }

    pub fn def_path(&self, def: DefId) -> DefPath<'_> {
        DefPath {
            module: self.name(def.module),
            name: def.name,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DefPath<'a> {
    module: &'a ModuleName,
    name: Symbol,
}

impl DefPath<'_> {
    pub fn render(self, session: &Session) -> String {
        if self.module.0.is_empty() {
            return session.str(self.name).to_string();
        }

        let mut rendered = self.module.as_string(session);
        rendered.push('.');
        rendered.push_str(&session.str(self.name));
        rendered
    }
}
