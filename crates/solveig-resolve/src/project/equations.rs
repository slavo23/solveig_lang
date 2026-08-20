use super::*;

pub fn function_equation_group_indices(
    parsed: &ParsedModule,
) -> Result<Vec<Vec<usize>>, ResolveError> {
    equation_group_indices(parsed.defs.iter().enumerate().map(|(index, defn)| {
        let SurfaceDefinition::FunDef { name, params, .. } = &defn.0 else {
            return None;
        };

        Some((
            index,
            name.0,
            params.len(),
            FileSpan::new(parsed.file, name.1),
        ))
    }))
}

pub fn instance_method_equation_group_indices(
    file: FileId,
    methods: &[InstanceMethod],
) -> Result<Vec<Vec<usize>>, ResolveError> {
    equation_group_indices(methods.iter().enumerate().map(|(index, method)| {
        Some((
            index,
            method.name.0,
            method.params.len(),
            FileSpan::new(file, method.name.1),
        ))
    }))
}

fn equation_group_indices(
    equations: impl IntoIterator<Item = Option<(usize, Symbol, usize, FileSpan)>>,
) -> Result<Vec<Vec<usize>>, ResolveError> {
    let mut groups = Vec::<Vec<usize>>::new();
    let mut seen_terms = FxHashMap::<Symbol, (FileSpan, usize)>::default();
    let mut active_group: Option<(Symbol, usize, usize, FileSpan)> = None;

    for equation in equations {
        let Some((index, name, arity, current)) = equation else {
            active_group = None;
            continue;
        };

        if let Some((active_name, active_arity, group_index, active_span)) = &active_group
            && *active_name == name
        {
            if *active_arity == 0 || arity == 0 {
                return Err(ResolveError::DuplicateDefinition {
                    span: current,
                    previous: Some(*active_span),
                    kind: ResolveNameKind::Term,
                    name,
                });
            }

            if arity != *active_arity {
                return Err(ResolveError::EquationArityMismatch {
                    span: current,
                    previous: *active_span,
                    name,
                    expected: *active_arity,
                    actual: arity,
                });
            }

            groups
                .get_mut(*group_index)
                .expect("ICE: active equation group index out of bounds")
                .push(index);
            continue;
        }

        if let Some((previous, previous_arity)) = seen_terms.get(&name).copied() {
            if previous_arity > 0 && arity > 0 {
                return Err(ResolveError::NonContiguousEquationGroup {
                    span: current,
                    previous,
                    name,
                });
            }

            return Err(ResolveError::DuplicateDefinition {
                span: current,
                previous: Some(previous),
                kind: ResolveNameKind::Term,
                name,
            });
        }

        let group_index = groups.len();
        groups.push(vec![index]);
        seen_terms.insert(name, (current, arity));
        active_group = Some((name, arity, group_index, current));
    }

    Ok(groups)
}
