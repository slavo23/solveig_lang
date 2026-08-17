//! Dictionary layout planning for typeclass evidence.
//! Lowering uses these maps to turn solved instances into runtime values.

use solveig_base::FxHashMap;

use solveig_ast::ast::{
    ClassId, CtorId, CtorRuntimeInfo, CtorRuntimeRole, DefId, InstanceId, ResolvedModule,
};

#[derive(Clone, Debug)]
pub struct DictMethodLayout {
    pub id: DefId,
    pub name: String,
    pub short_name: String,
    pub field_index: usize,
}

#[derive(Clone, Debug)]
pub struct DictLayout {
    pub class_id: ClassId,
    pub name: String,
    pub ctor: CtorId,
    pub ctor_name: String,
    pub short_ctor_name: String,
    pub tag: u16,
    pub arity: u16,
    pub superclass_count: usize,
    pub methods: Vec<DictMethodLayout>,
}

#[derive(Clone, Debug, Default)]
pub struct DictLayouts {
    classes: FxHashMap<ClassId, DictLayout>,
    method_classes: FxHashMap<DefId, ClassId>,
    instance_globals: FxHashMap<InstanceId, String>,
    instance_method_globals: FxHashMap<(InstanceId, DefId), String>,
    runtime_constructors: Vec<CtorRuntimeInfo>,
}

impl DictLayouts {
    pub fn build<A, P>(
        module: &ResolvedModule<A, P>,
        mut next_ctor_id: u32,
        mut next_tag: u16,
    ) -> Self {
        let mut out = Self::default();

        for class_def in &module.class_defs {
            let superclass_count = class_def.superclasses.len();
            let methods = class_def
                .methods
                .iter()
                .enumerate()
                .map(|(method_index, method)| {
                    let field_index = superclass_count + method_index;
                    out.method_classes.insert(method.id, class_def.id);
                    DictMethodLayout {
                        id: method.id,
                        name: method.name.clone(),
                        short_name: method.short_name.clone(),
                        field_index,
                    }
                })
                .collect::<Vec<_>>();

            let arity = u16::try_from(superclass_count + methods.len())
                .expect("ICE: dictionary constructor arity does not fit in u16");
            let ctor = CtorId(next_ctor_id);
            next_ctor_id = next_ctor_id
                .checked_add(1)
                .expect("ICE: dictionary constructor id space exhausted");

            let name = format!("$dict.{}", class_def.name);
            let ctor_name = format!("{name}.Mk");
            let short_ctor_name = format!("Mk{}Dict", class_def.short_name);
            let layout = DictLayout {
                class_id: class_def.id,
                name: name.clone(),
                ctor,
                ctor_name: ctor_name.clone(),
                short_ctor_name: short_ctor_name.clone(),
                tag: next_tag,
                arity,
                superclass_count,
                methods,
            };

            next_tag = next_tag
                .checked_add(1)
                .expect("ICE: constructor tag space exhausted while adding dictionaries");

            out.runtime_constructors.push(CtorRuntimeInfo {
                ctor,
                role: CtorRuntimeRole::Ordinary,
                name: ctor_name,
                short_name: short_ctor_name,
                tag: layout.tag,
                arity,
            });
            out.classes.insert(class_def.id, layout);
        }

        for inst in &module.instance_defs {
            out.instance_globals
                .insert(inst.id, instance_global_name(inst.id));

            let layout = out.classes.get(&inst.head.class_id).unwrap_or_else(|| {
                solveig_base::ice!(
                    "missing dictionary layout for instance head {:?}",
                    inst.head.class_id
                )
            });
            for method in &layout.methods {
                out.instance_method_globals.insert(
                    (inst.id, method.id),
                    instance_method_global_name(inst.id, method.field_index),
                );
            }
        }

        out
    }

    pub fn class(&self, id: ClassId) -> Option<&DictLayout> {
        self.classes.get(&id)
    }

    pub fn method_field(&self, id: DefId) -> Option<(ClassId, usize)> {
        let class = *self.method_classes.get(&id)?;
        let field = self
            .classes
            .get(&class)?
            .methods
            .iter()
            .find(|method| method.id == id)?
            .field_index;
        Some((class, field))
    }

    pub fn instance_global(&self, id: InstanceId) -> Option<&str> {
        self.instance_globals.get(&id).map(String::as_str)
    }

    pub fn instance_method_global(&self, instance: InstanceId, method: DefId) -> Option<&str> {
        self.instance_method_globals
            .get(&(instance, method))
            .map(String::as_str)
    }

    pub fn runtime_constructors(&self) -> &[CtorRuntimeInfo] {
        &self.runtime_constructors
    }
}

pub fn instance_global_name(id: InstanceId) -> String {
    format!("$dict.instance.{}", id.0)
}

pub fn instance_method_global_name(id: InstanceId, field_index: usize) -> String {
    format!("$dict.instance.{}.method.{field_index}", id.0)
}
