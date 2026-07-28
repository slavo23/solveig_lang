//! Declaration parsing for modules, data types, classes, instances, and fixities.
//! Parsed declarations feed fixity resolution and project API construction.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_func_or_sig(&mut self) -> Result<SurfaceDefinition, ParseError> {
        let (name, start, end) = self.expect_value_name()?;
        let name = sp(name, start, end);

        if self.at(&Token::Colon) {
            self.advance();
            let ty = self.parse_type()?;
            return Ok(SurfaceDefinition::SigDef { name, ty });
        }

        let mut params = Vec::new();
        while matches!(self.current_kind(), Token::Ident(_) | Token::Underscore) {
            let (param, start, end) = self.expect_binder()?;
            params.push(sp(param, start, end));
        }

        self.expect(&Token::Eq)?;
        let body = self.parse_expr()?;

        Ok(SurfaceDefinition::FunDef { name, body, params })
    }

    pub(super) fn parse_fixity_def(&mut self) -> Result<SurfaceDefinition, ParseError> {
        let assoc = match self.current_kind() {
            Token::Infixl => Assoc::Left,
            Token::Infixr => Assoc::Right,
            Token::Infix => Assoc::None,
            _ => {
                return Err(self.error(format!(
                    "expected fixity declaration, got {}",
                    self.describe_current()
                )));
            }
        };
        self.advance();

        let prec = match self.current_kind() {
            Token::Int(value) => {
                let value = *value;
                self.advance();
                u8::try_from(value)
                    .ok()
                    .filter(|value| *value <= 9)
                    .ok_or_else(|| self.error("fixity precedence must be in the range 0..9"))?
            }
            _ => Fixity::DEFAULT.prec,
        };

        let mut ops = Vec::new();
        loop {
            let (op, start, end) = if self.starts_parenthesized_operator_name() {
                self.parse_parenthesized_operator_name()?
            } else {
                self.expect_operator_symbol(false)?
            };
            ops.push(sp(op, start, end));

            if !self.at(&Token::Comma) {
                break;
            }
            self.advance();
        }

        Ok(SurfaceDefinition::FixityDef {
            fixity: Fixity { prec, assoc },
            ops,
        })
    }

    pub(super) fn parse_data_def(&mut self) -> Result<SurfaceDefinition, ParseError> {
        self.expect(&Token::Data)?;
        let (name, start, end) = self.expect_upper()?;
        let name = sp(name, start, end);

        let mut ty_params = Vec::new();
        while matches!(self.current_kind(), Token::Ident(_)) {
            let (param, start, end) = self.expect_ident()?;
            ty_params.push(sp(param, start, end));
        }

        self.expect(&Token::Eq)?;

        let mut constructors = Vec::new();
        loop {
            if self.at(&Token::Pipe) {
                self.advance();
            }

            let start = self.current_start();
            let ctor = self.parse_constructor()?;
            let end = self.current_start();
            constructors.push(sp(ctor, start, end));

            if !self.at(&Token::Pipe) {
                break;
            }
        }

        Ok(SurfaceDefinition::DataDef {
            name,
            ty_params,
            constructors,
        })
    }

    pub(super) fn parse_constructor(&mut self) -> Result<SurfaceConstructor, ParseError> {
        let (name, start, end) = self.expect_upper()?;
        let mut params = Vec::new();
        while self.is_type_atom_start() {
            params.push(self.parse_type_atom()?);
        }
        Ok(SurfaceConstructor {
            name: sp(name, start, end),
            params,
        })
    }

    pub(super) fn parse_predicate(&mut self) -> Result<S<SurfacePredicate>, ParseError> {
        let start = self.current_start();

        let class_name = self.parse_name_path()?;
        if !is_ctor_like(&class_name.0.last_text(self.session)) {
            return Err(self.error(format!(
                "expected class name, got {}",
                class_name.0.display(self.session)
            )));
        }

        let mut args = Vec::new();
        while self.is_type_atom_start() {
            args.push(self.parse_type_atom()?);
        }

        if args.is_empty() {
            return Err(self.error("expected at least one type argument in predicate".to_string()));
        }

        let end = args.last().map_or_else(|| span_end(&class_name), span_end);

        Ok(sp(SurfacePredicate { class_name, args }, start, end))
    }

    pub(super) fn parse_predicate_list(&mut self) -> Result<Vec<S<SurfacePredicate>>, ParseError> {
        if self.at(&Token::LParen) {
            self.advance();

            let mut preds = vec![self.parse_predicate()?];
            while self.at(&Token::Comma) {
                self.advance();
                preds.push(self.parse_predicate()?);
            }

            self.expect(&Token::RParen)?;
            Ok(preds)
        } else {
            Ok(vec![self.parse_predicate()?])
        }
    }

    pub(super) fn parse_class_def(&mut self) -> Result<SurfaceDefinition, ParseError> {
        self.expect(&Token::Class)?;

        let checkpoint = self.pos;
        let superclasses = if let Ok(preds) = self.parse_predicate_list() {
            if self.at(&Token::FatArrow) {
                self.advance();
                preds
            } else {
                self.pos = checkpoint;
                Vec::new()
            }
        } else {
            self.pos = checkpoint;
            Vec::new()
        };

        let (name, ns, ne) = self.expect_upper()?;

        let mut ty_params = Vec::new();
        while matches!(self.current_kind(), Token::Ident(_)) {
            let (p, ps, pe) = self.expect_ident()?;
            ty_params.push(sp(p, ps, pe));
        }

        if ty_params.is_empty() {
            return Err(
                self.error("class declaration needs at least one type parameter".to_string())
            );
        }

        self.expect(&Token::Where)?;

        if self.at(&Token::VOpen) {
            self.advance();
        }

        let mut methods = Vec::new();
        let mut defaults = Vec::new();
        loop {
            self.skip_separators();
            if self.at(&Token::VClose) || self.at_eof() {
                break;
            }

            if self.at(&Token::Default) {
                self.advance();
                let (mname, ms, me) = self.expect_value_name()?;
                let mut mparams = Vec::new();
                while matches!(self.current_kind(), Token::Ident(_) | Token::Underscore) {
                    let (p, ps, pe) = self.expect_binder()?;
                    mparams.push(sp(p, ps, pe));
                }
                self.expect(&Token::Eq)?;
                let mbody = self.parse_expr()?;
                defaults.push(InstanceMethod {
                    name: sp(mname, ms, me),
                    params: mparams,
                    body: mbody,
                });
                continue;
            }

            let (mname, ms, me) = self.expect_value_name()?;
            self.expect(&Token::Colon)?;
            let mty = self.parse_type()?;
            methods.push(ClassMethod {
                name: sp(mname, ms, me),
                ty: mty,
            });
        }

        if self.at(&Token::VClose) {
            self.advance();
        }

        Ok(SurfaceDefinition::ClassDef {
            name: sp(name, ns, ne),
            ty_params,
            superclasses,
            methods,
            defaults,
        })
    }

    pub(super) fn parse_instance_def(&mut self) -> Result<SurfaceDefinition, ParseError> {
        self.expect(&Token::Instance)?;

        let checkpoint = self.pos;
        let context = if let Ok(preds) = self.parse_predicate_list() {
            if self.at(&Token::FatArrow) {
                self.advance();
                preds
            } else {
                self.pos = checkpoint;
                Vec::new()
            }
        } else {
            self.pos = checkpoint;
            Vec::new()
        };

        let (cname, cs, ce) = self.expect_upper()?;

        let mut args = Vec::new();
        while self.is_type_atom_start() {
            args.push(self.parse_type_atom()?);
        }

        if args.is_empty() {
            return Err(self.error("instance head needs at least one type argument".to_string()));
        }

        self.expect(&Token::Where)?;

        if self.at(&Token::VOpen) {
            self.advance();
        }

        let mut methods = Vec::new();
        loop {
            self.skip_separators();
            if self.at(&Token::VClose) || self.at_eof() {
                break;
            }
            let (mname, ms, me) = self.expect_value_name()?;
            let mut mparams = Vec::new();
            while matches!(self.current_kind(), Token::Ident(_) | Token::Underscore) {
                let (p, ps, pe) = self.expect_binder()?;
                mparams.push(sp(p, ps, pe));
            }
            self.expect(&Token::Eq)?;
            let mbody = self.parse_expr()?;
            methods.push(InstanceMethod {
                name: sp(mname, ms, me),
                params: mparams,
                body: mbody,
            });
        }

        if self.at(&Token::VClose) {
            self.advance();
        }

        Ok(SurfaceDefinition::InstanceDef {
            context,
            class_name: sp(cname, cs, ce),
            args,
            methods,
        })
    }
}
