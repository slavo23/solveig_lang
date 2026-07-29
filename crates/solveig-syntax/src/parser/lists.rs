//! List syntax desugaring for parser-owned surface AST nodes.

use super::*;

fn qref(
    session: &Session,
    segments: &[&str],
    start: usize,
    end: usize,
    cst_id: CstId,
) -> S<NameRef> {
    sp(
        NameRef {
            segments: segments
                .iter()
                .map(|segment| session.intern(segment))
                .collect(),
        },
        start,
        end,
        cst_id,
    )
}

pub(super) fn list_nil_pattern(
    session: &Session,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> S<SurfacePattern> {
    sp(
        SurfacePattern::Con(
            qref(session, &["prelude", "list", "Nil"], start, end, cst_id),
            vec![],
        ),
        start,
        end,
        cst_id,
    )
}

pub(super) fn list_nil_expr(
    session: &Session,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> S<SurfaceExpr> {
    sp(
        SurfaceExpr::Con(qref(
            session,
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
    session: &Session,
    elem_ty: S<SurfaceTy>,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> S<SurfaceTy> {
    let list_con = sp(
        SurfaceTy::Con(qref(
            session,
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

pub(super) fn int_pattern(
    value: i64,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> S<SurfacePattern> {
    sp(
        SurfacePattern::Lit(SurfaceLiteral::Int(value)),
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
) -> S<SurfacePattern> {
    sp(SurfacePattern::Lit(literal), start, end, cst_id)
}

pub(super) fn cons_pattern(
    session: &Session,
    head: S<SurfacePattern>,
    tail: S<SurfacePattern>,
    cons_start: usize,
    cons_end: usize,
    cst_id: CstId,
) -> S<SurfacePattern> {
    let (start, end) = merge_span(&head, &tail);
    sp(
        SurfacePattern::Con(
            qref(
                session,
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
    session: &Session,
    elems: Vec<S<SurfacePattern>>,
    _start: usize,
    end: usize,
    cst_id: CstId,
) -> S<SurfacePattern> {
    elems
        .into_iter()
        .rev()
        .fold(list_nil_pattern(session, end, end, cst_id), |acc, elem| {
            let generated = span_start(&elem);
            cons_pattern(session, elem, acc, generated, generated, cst_id)
        })
}

pub(super) fn cons_expr(
    session: &Session,
    head: S<SurfaceExpr>,
    tail: S<SurfaceExpr>,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> S<SurfaceExpr> {
    let cons = sp(
        SurfaceExpr::Con(qref(
            session,
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
    session: &Session,
    elems: Vec<S<SurfaceExpr>>,
    start: usize,
    end: usize,
    cst_id: CstId,
) -> S<SurfaceExpr> {
    let expr =
        elems
            .into_iter()
            .rev()
            .fold(list_nil_expr(session, end, end, cst_id), |acc, elem| {
                let start = span_start(&elem);
                let end = span_end(&acc);
                cons_expr(session, elem, acc, start, end, cst_id)
            });

    sp(expr.0, start, end, cst_id)
}
