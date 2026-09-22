//! List syntax desugaring for lowered surface AST nodes.

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

pub(super) fn list_nil_pattern(
    lowerer: &CstToAst<'_>,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> Spanned<SurfacePattern> {
    sp(
        SurfacePattern::Con(
            qref(lowerer, &["prelude", "list", "Nil"], start, end, cst_id),
            vec![],
        ),
        start,
        end,
        cst_id,
    )
}

pub(super) fn list_nil_expr(
    lowerer: &CstToAst<'_>,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> Spanned<SurfaceExpr> {
    sp(
        SurfaceExpr::Con(qref(
            lowerer,
            &["prelude", "list", "Nil"],
            start,
            end,
            cst_id,
        )),
        start,
        end,
        cst_id,
    )
}

pub(super) fn list_ty(
    lowerer: &CstToAst<'_>,
    elem_ty: Spanned<SurfaceTy>,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> Spanned<SurfaceTy> {
    let list_con = sp(
        SurfaceTy::Con(qref(
            lowerer,
            &["prelude", "list", "List"],
            start,
            start,
            cst_id,
        )),
        start,
        start,
        cst_id,
    );
    sp(
        SurfaceTy::App(Box::new(list_con), Box::new(elem_ty)),
        start,
        end,
        cst_id,
    )
}

pub(super) fn literal_pattern(
    literal: SurfaceLiteral,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> Spanned<SurfacePattern> {
    sp(SurfacePattern::Lit(literal), start, end, cst_id)
}

pub(super) fn cons_pattern(
    lowerer: &CstToAst<'_>,
    head: Spanned<SurfacePattern>,
    tail: Spanned<SurfacePattern>,
    cons_start: usize,
    cons_end: usize,
    cst_id: CstId,
) -> Spanned<SurfacePattern> {
    let (start, end) = merge_span(&head, &tail);
    sp(
        SurfacePattern::Con(
            qref(
                lowerer,
                &["prelude", "list", "Cons"],
                cons_start,
                cons_end,
                cst_id,
            ),
            vec![head, tail],
        ),
        start,
        end,
        cst_id,
    )
}

pub(super) fn list_pattern(
    lowerer: &CstToAst<'_>,
    elems: Vec<Spanned<SurfacePattern>>,
    _start: usize,
    end: usize,
    cst_id: CstId,
) -> Spanned<SurfacePattern> {
    elems
        .into_iter()
        .rev()
        .fold(list_nil_pattern(lowerer, end, end, cst_id), |acc, elem| {
            let generated = span_start(&elem);
            cons_pattern(lowerer, elem, acc, generated, generated, cst_id)
        })
}

pub(super) fn cons_expr(
    lowerer: &CstToAst<'_>,
    head: Spanned<SurfaceExpr>,
    tail: Spanned<SurfaceExpr>,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> Spanned<SurfaceExpr> {
    let cons = sp(
        SurfaceExpr::Con(qref(
            lowerer,
            &["prelude", "list", "Cons"],
            start,
            start,
            cst_id,
        )),
        start,
        start,
        cst_id,
    );
    let app = sp(
        SurfaceExpr::App(Box::new(cons), Box::new(head)),
        start,
        end,
        cst_id,
    );
    sp(
        SurfaceExpr::App(Box::new(app), Box::new(tail)),
        start,
        end,
        cst_id,
    )
}

pub(super) fn list_expr(
    lowerer: &CstToAst<'_>,
    elems: Vec<Spanned<SurfaceExpr>>,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> Spanned<SurfaceExpr> {
    let expr =
        elems
            .into_iter()
            .rev()
            .fold(list_nil_expr(lowerer, end, end, cst_id), |acc, elem| {
                let start = span_start(&elem);
                let end = span_end(&acc);
                cons_expr(lowerer, elem, acc, start, end, cst_id)
            });

    sp(expr.0, start, end, cst_id)
}
