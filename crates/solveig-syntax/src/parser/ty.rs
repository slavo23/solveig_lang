//! Type and predicate parser for annotations, data fields, and class heads.
//! Kind inference consumes these syntax nodes after name resolution.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_qualified_type(&mut self) -> Result<S<SurfaceTy>, ParseError> {
        self.node(SyntaxKind::QualifiedType, |this| {
            let checkpoint = this.checkpoint();

            if let Ok(preds) = this.parse_predicate_list()
                && this.at(&Token::FatArrow)
            {
                this.advance();
                let body = this.parse_type_arrow()?;
                let start = preds.first().map_or_else(|| span_start(&body), span_start);
                let end = span_end(&body);
                return Ok(this.sp(SurfaceTy::Qualified(preds, Box::new(body)), start, end));
            }

            this.restore(checkpoint);
            this.parse_type_arrow()
        })
    }

    pub(super) fn parse_type(&mut self) -> Result<S<SurfaceTy>, ParseError> {
        self.node(SyntaxKind::Type, |this| {
            let start = this.current_start();

            if this.at(&Token::Forall) {
                this.advance();
                let mut vars = Vec::new();
                while matches!(this.current_kind(), Token::Ident) {
                    let (v, vs, ve) = this.expect_ident()?;
                    vars.push(this.sp(v, vs, ve));
                }
                this.expect(&Token::Dot)?;
                let body = this.parse_type()?;
                let end = span_end(&body);
                return Ok(this.sp(SurfaceTy::ForAll(vars, Box::new(body)), start, end));
            }

            this.parse_qualified_type()
        })
    }

    pub(super) fn parse_type_arrow(&mut self) -> Result<S<SurfaceTy>, ParseError> {
        self.node(SyntaxKind::TypeArrow, |this| {
            let lhs = this.parse_type_app()?;
            if this.at(&Token::Arrow) {
                this.advance();
                let rhs = this.parse_type()?;
                let (start, end) = merge_span(&lhs, &rhs);
                Ok(this.sp(SurfaceTy::Arr(Box::new(lhs), Box::new(rhs)), start, end))
            } else {
                Ok(lhs)
            }
        })
    }

    pub(super) fn parse_type_app(&mut self) -> Result<S<SurfaceTy>, ParseError> {
        self.node(SyntaxKind::TypeApp, |this| {
            let mut ty = this.parse_type_atom()?;
            while this.is_type_atom_start() {
                let arg = this.parse_type_atom()?;
                let (start, end) = merge_span(&ty, &arg);
                ty = this.sp(SurfaceTy::App(Box::new(ty), Box::new(arg)), start, end);
            }
            Ok(ty)
        })
    }

    pub(super) fn is_type_atom_start(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::Ident | Token::UpperIdent | Token::LParen | Token::LBrack
        )
    }

    pub(super) fn parse_type_atom(&mut self) -> Result<S<SurfaceTy>, ParseError> {
        self.node(SyntaxKind::TypeAtom, |this| {
            let start = this.current_start();
            match this.current_kind() {
                Token::Ident | Token::UpperIdent => {
                    let name_ref = this.parse_name_path()?;
                    let end = span_end(&name_ref);
                    if is_ctor_like(this.name_ref_last_source_text(&name_ref)) {
                        Ok(this.sp(SurfaceTy::Con(name_ref), start, end))
                    } else if name_ref.0.segments.len() == 1 {
                        Ok(this.sp(SurfaceTy::Var(name_ref.0.last()), start, end))
                    } else {
                        Err(this.error(format!(
                            "qualified type variable is not allowed: {}",
                            this.name_ref_source_text(&name_ref)
                        )))
                    }
                }
                Token::LParen => {
                    this.advance();
                    if this.at(&Token::RParen) {
                        let end = this.advance().end;
                        return Ok(this.sp(SurfaceTy::Unit, start, end));
                    }
                    let inner = this.parse_type()?;
                    let end = this.expect(&Token::RParen)?.end;
                    Ok(this.sp(inner.0, start, end))
                }
                Token::LBrack => {
                    this.advance();
                    let elem = this.parse_type()?;
                    let end = this.expect(&Token::RBrack)?.end;
                    Ok(list_ty(this, elem, start, end, this.current_cst_id()))
                }
                _ => Err(this.error(format!("expected type, got {}", this.describe_current()))),
            }
        })
    }
}
