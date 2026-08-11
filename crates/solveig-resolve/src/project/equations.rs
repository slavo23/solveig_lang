use super::*;

pub fn function_equation_group_indices(
    parsed: &ParsedModule,
) -> Result<Vec<Vec<usize>>, ResolveError> {
    let mut groups = Vec::<Vec<usize>>::new();
    let mut seen_terms = FxHashMap::<Symbol, (FileSpan, usize)>::default();
    let mut active_group: Option<(Symbol, usize, usize, FileSpan)> = None;

    for (index, defn) in parsed.defs.iter().enumerate() {
        let SurfaceDefinition::FunDef { name, params, .. } = &defn.0 else {
            active_group = None;
            continue;
        };

        let current = FileSpan::new(parsed.file, name.1);
        let arity = params.len();

        if let Some((active_name, active_arity, group_index, active_span)) = &active_group
            && *active_name == name.0
        {
            if *active_arity == 0 || arity == 0 {
                return Err(ResolveError::DuplicateDefinition {
                    span: current,
                    previous: Some(*active_span),
                    kind: ResolveNameKind::Term,
                    name: name.0,
                });
            }

            if arity != *active_arity {
                return Err(ResolveError::EquationArityMismatch {
                    span: current,
                    previous: *active_span,
                    name: name.0,
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

        if let Some((previous, previous_arity)) = seen_terms.get(&name.0).copied() {
            if previous_arity > 0 && arity > 0 {
                return Err(ResolveError::NonContiguousEquationGroup {
                    span: current,
                    previous,
                    name: name.0,
                });
            }

            return Err(ResolveError::DuplicateDefinition {
                span: current,
                previous: Some(previous),
                kind: ResolveNameKind::Term,
                name: name.0,
            });
        }

        let group_index = groups.len();
        groups.push(vec![index]);
        seen_terms.insert(name.0, (current, arity));
        active_group = Some((name.0, arity, group_index, current));
    }

    Ok(groups)
}
