//! Tuple syntax desugaring for lowered surface AST nodes.

use super::*;

fn qref(
    lowerer: &CstToAst<'_>,
    segments: &[&str],
    start: usize,
    end: usize,
    cst_id: CstId,
) -> Spanned<NameRef> {
    sp(
        NameRef {
            segments: segments
                .iter()
                .map(|segment| lowerer.intern(segment))
                .collect(),
        },
        start,
        end,
        cst_id,
    )
}

fn tuple_type_segments(arity: usize) -> &'static [&'static str] {
    match arity {
        2 => &["prelude", "builtins", "Tuple2"],
        3 => &["prelude", "builtins", "Tuple3"],
        4 => &["prelude", "builtins", "Tuple4"],
        5 => &["prelude", "builtins", "Tuple5"],
        6 => &["prelude", "builtins", "Tuple6"],
        7 => &["prelude", "builtins", "Tuple7"],
        8 => &["prelude", "builtins", "Tuple8"],
        _ => solveig_base::ice!("tuple arity reached lowering outside the parser's 2..=8 bound"),
    }
}

fn tuple_ctor_segments(arity: usize) -> &'static [&'static str] {
    match arity {
        2 => &["prelude", "builtins", "MkTuple2"],
        3 => &["prelude", "builtins", "MkTuple3"],
        4 => &["prelude", "builtins", "MkTuple4"],
        5 => &["prelude", "builtins", "MkTuple5"],
        6 => &["prelude", "builtins", "MkTuple6"],
        7 => &["prelude", "builtins", "MkTuple7"],
        8 => &["prelude", "builtins", "MkTuple8"],
        _ => solveig_base::ice!("tuple arity reached lowering outside the parser's 2..=8 bound"),
    }
}

pub(super) fn tuple_constructor_expr(
    lowerer: &CstToAst<'_>,
    arity: usize,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> Spanned<SurfaceExpr> {
    sp(
        SurfaceExpr::Con(qref(
            lowerer,
            tuple_ctor_segments(arity),
            start,
            end,
            cst_id,
        )),
        start,
        end,
        cst_id,
    )
}

pub(super) fn tuple_expr(
    lowerer: &CstToAst<'_>,
    elems: Vec<Spanned<SurfaceExpr>>,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> Spanned<SurfaceExpr> {
    let constructor = tuple_constructor_expr(lowerer, elems.len(), start, start, cst_id);
    let expr = elems.into_iter().fold(constructor, |function, element| {
        let element_end = span_end(&element);
        sp(
            SurfaceExpr::App(Box::new(function), Box::new(element)),
            start,
            element_end,
            cst_id,
        )
    });
    sp(expr.0, start, end, cst_id)
}

pub(super) fn tuple_pattern(
    lowerer: &CstToAst<'_>,
    elems: Vec<Spanned<SurfacePattern>>,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> Spanned<SurfacePattern> {
    sp(
        SurfacePattern::Con(
            qref(
                lowerer,
                tuple_ctor_segments(elems.len()),
                start,
                end,
                cst_id,
            ),
            elems,
        ),
        start,
        end,
        cst_id,
    )
}

pub(super) fn tuple_constructor_pattern(
    lowerer: &CstToAst<'_>,
    arity: usize,
    elems: Vec<Spanned<SurfacePattern>>,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> Spanned<SurfacePattern> {
    sp(
        SurfacePattern::Con(
            qref(lowerer, tuple_ctor_segments(arity), start, end, cst_id),
            elems,
        ),
        start,
        end,
        cst_id,
    )
}

pub(super) fn tuple_constructor_ty(
    lowerer: &CstToAst<'_>,
    arity: usize,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> Spanned<SurfaceTy> {
    sp(
        SurfaceTy::Con(qref(
            lowerer,
            tuple_type_segments(arity),
            start,
            end,
            cst_id,
        )),
        start,
        end,
        cst_id,
    )
}

pub(super) fn tuple_ty(
    lowerer: &CstToAst<'_>,
    elems: Vec<Spanned<SurfaceTy>>,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> Spanned<SurfaceTy> {
    let constructor = tuple_constructor_ty(lowerer, elems.len(), start, start, cst_id);
    let ty = elems.into_iter().fold(constructor, |function, element| {
        let element_end = span_end(&element);
        sp(
            SurfaceTy::App(Box::new(function), Box::new(element)),
            start,
            element_end,
            cst_id,
        )
    });
    sp(ty.0, start, end, cst_id)
}
