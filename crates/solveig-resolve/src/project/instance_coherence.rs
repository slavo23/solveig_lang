//! Graph-wide instance ownership and overlap validation.
//! Coherence is checked once from all resolved modules before typechecking begins.

use std::ops::Range;

use solveig_ast::ast::{
    ClassTable, InstanceDefinition, InstanceId, Names, PrettyTables, ResolvedModule, TyConTable,
    instance_overlaps,
};
use solveig_base::{FileId, FileSpan, FxHashMap, Span};

use super::*;

/// Validates instance ownership and overlap across the complete visible package graph.
pub fn validate_instance_coherence(
    names: &Names,
    modules: &ModuleTable,
    resolved_modules: &[(FileId, &ResolvedModule<Range<usize>>)],
) -> Vec<ResolveError> {
    let mut tycons = TyConTable::new();
    let mut classes = ClassTable::new();
    let mut instances = Vec::new();
    let mut by_id = FxHashMap::<InstanceId, (FileId, &InstanceDefinition<Range<usize>>)>::default();

    for (file, module) in resolved_modules {
        for (id, info) in module.tycon_table.iter() {
            tycons.insert_with_id(id, info.clone());
        }
        for (id, info) in module.class_table.iter() {
            classes.insert_with_id(id, info.clone());
        }
        for instance in &module.instance_defs {
            instances.push((*file, instance));
            by_id.insert(instance.id, (*file, instance));
        }
    }

    let pretty = PrettyTables::new(names, &tycons, &classes);
    let mut errors = instances
        .iter()
        .filter_map(|(file, instance)| {
            let defining_package = modules.package(instance.def_module);
            let owns_class = instance
                .class_module
                .is_some_and(|module| modules.package(module) == defining_package);
            let owns_type = instance
                .type_module
                .is_some_and(|module| modules.package(module) == defining_package);

            (!owns_class && !owns_type).then(|| ResolveError::OrphanInstance {
                span: instance_file_span(*file, instance),
                class_name: class_name(names, &classes, instance.head.class_id),
                instance_type: instance_type(instance, pretty),
            })
        })
        .collect::<Vec<_>>();

    for overlap in instance_overlaps(instances.iter().map(|(_, instance)| *instance)) {
        let Some((first_file, first)) = by_id.get(&overlap.first) else {
            solveig_base::ice!("instance overlap referenced a missing first instance");
        };
        let Some((second_file, second)) = by_id.get(&overlap.second) else {
            solveig_base::ice!("instance overlap referenced a missing second instance");
        };

        errors.push(ResolveError::OverlappingInstances {
            span: instance_file_span(*second_file, second),
            previous: instance_file_span(*first_file, first),
            class_name: class_name(names, &classes, overlap.class),
            instance_type: overlap
                .instance_args
                .iter()
                .map(|arg| arg.pretty_with_tables(pretty))
                .collect::<Vec<_>>()
                .join(" "),
        });
    }

    errors
}

fn instance_file_span(file: FileId, instance: &InstanceDefinition<Range<usize>>) -> FileSpan {
    FileSpan::new(file, Span::from_usize_range(instance.span.clone()))
}

fn class_name(names: &Names, classes: &ClassTable, id: solveig_ast::ast::ClassId) -> String {
    classes.get(id).map_or_else(
        || names.class_display_name(id),
        |info| names.symbol_display_name(info.short_name),
    )
}

fn instance_type(instance: &InstanceDefinition<Range<usize>>, pretty: PrettyTables<'_>) -> String {
    instance
        .head
        .args
        .iter()
        .map(|arg| arg.pretty_with_tables(pretty))
        .collect::<Vec<_>>()
        .join(" ")
}
