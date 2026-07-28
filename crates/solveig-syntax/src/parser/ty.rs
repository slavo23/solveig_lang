use super::*;

impl Parser<'_> {
    pub(super) fn parse_qualified_type(&mut self) -> Result<S<SurfaceTy>, ParseError> {
        let checkpoint = self.pos;

        if let Ok(preds) = self.parse_predicate_list()
            && self.at(&Token::FatArrow)
        {
            self.advance();
            let body = self.parse_type_arrow()?;
            let start = preds
                .first()
                .map(span_start)
                .unwrap_or_else(|| span_start(&body));
            let end = span_end(&body);
            return Ok(sp(SurfaceTy::Qualified(preds, Box::new(body)), start, end));
        }

        self.pos = checkpoint;
        self.parse_type_arrow()
    }

    pub(super) fn parse_type(&mut self) -> Result<S<SurfaceTy>, ParseError> {
        let start = self.current_start();

        if self.at(&Token::Forall) {
            self.advance();
            let mut vars = Vec::new();
            while matches!(self.current_kind(), Token::Ident(_)) {
                let (v, vs, ve) = self.expect_ident()?;
                vars.push(sp(v, vs, ve));
            }
            self.expect(&Token::Dot)?;
            let body = self.parse_type()?;
            let end = span_end(&body);
            return Ok(sp(SurfaceTy::ForAll(vars, Box::new(body)), start, end));
        }

        self.parse_qualified_type()
    }

    pub(super) fn parse_type_arrow(&mut self) -> Result<S<SurfaceTy>, ParseError> {
        let lhs = self.parse_type_app()?;
        if self.at(&Token::Arrow) {
            self.advance();
            let rhs = self.parse_type()?;
            let (start, end) = merge_span(&lhs, &rhs);
            Ok(sp(SurfaceTy::Arr(Box::new(lhs), Box::new(rhs)), start, end))
        } else {
            Ok(lhs)
        }
    }

    pub(super) fn parse_type_app(&mut self) -> Result<S<SurfaceTy>, ParseError> {
        let mut ty = self.parse_type_atom()?;
        while self.is_type_atom_start() {
            let arg = self.parse_type_atom()?;
            let (start, end) = merge_span(&ty, &arg);
            ty = sp(SurfaceTy::App(Box::new(ty), Box::new(arg)), start, end);
        }
        Ok(ty)
    }

    pub(super) fn is_type_atom_start(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::Ident(_) | Token::UpperIdent(_) | Token::LParen | Token::LBrack
        )
    }

    pub(super) fn parse_type_atom(&mut self) -> Result<S<SurfaceTy>, ParseError> {
        let start = self.current_start();
        match self.current_kind() {
            Token::Ident(_) | Token::UpperIdent(_) => {
                let name_ref = self.parse_name_path()?;
                let end = span_end(&name_ref);
                if is_ctor_like(&name_ref.0.last_text(self.session)) {
                    Ok(sp(SurfaceTy::Con(name_ref), start, end))
                } else if name_ref.0.segments.len() == 1 {
                    Ok(sp(SurfaceTy::Var(name_ref.0.last()), start, end))
                } else {
                    Err(self.error(format!(
                        "qualified type variable is not allowed: {}",
                        name_ref.0.display(self.session)
                    )))
                }
            }
            Token::LParen => {
                self.advance();
                if self.at(&Token::RParen) {
                    let end = self.advance().end;
                    return Ok(sp(SurfaceTy::Unit, start, end));
                }
                let inner = self.parse_type()?;
                let end = self.expect(&Token::RParen)?.end;
                Ok(sp(inner.0, start, end))
            }
            Token::LBrack => {
                self.advance();
                let elem = self.parse_type()?;
                let end = self.expect(&Token::RBrack)?.end;
                Ok(list_ty(self.session, elem, start, end))
            }
            _ => Err(self.error(format!("expected type, got {}", self.describe_current()))),
        }
    }
}
