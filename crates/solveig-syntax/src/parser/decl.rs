//! Declaration parsing for modules, data types, classes, instances, and fixities.
//! Parsed declarations feed fixity resolution and project API construction.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_func_or_sig(&mut self) -> Result<SurfaceDefinition, ParseError> {
        self.node(SyntaxKind::FunctionOrSignature, |this| {
            let (name, start, end) = this.expect_value_name()?;
            let name = this.sp(name, start, end);

            if this.at(&Token::Colon) {
                this.advance();
                let ty = this.parse_type()?;
                return Ok(SurfaceDefinition::SigDef { name, ty });
            }

            let mut params = Vec::new();
            while matches!(this.current_kind(), Token::Ident(_) | Token::Underscore) {
                let (param, start, end) = this.expect_binder()?;
                params.push(this.sp(param, start, end));
            }

            this.expect(&Token::Eq)?;
            let body = this.parse_expr()?;

            Ok(SurfaceDefinition::FunDef { name, body, params })
        })
    }

    pub(super) fn parse_fixity_def(&mut self) -> Result<SurfaceDefinition, ParseError> {
        self.node(SyntaxKind::FixityDef, |this| {
            let assoc = match this.current_kind() {
                Token::Infixl => Assoc::Left,
                Token::Infixr => Assoc::Right,
                Token::Infix => Assoc::None,
                _ => {
                    return Err(this.error(format!(
                        "expected fixity declaration, got {}",
                        this.describe_current()
                    )));
                }
            };
            this.advance();

            let prec = match this.current_kind() {
                Token::Int(value) => {
                    let value = *value;
                    this.advance();
                    u8::try_from(value)
                        .ok()
                        .filter(|value| *value <= 9)
                        .ok_or_else(|| this.error("fixity precedence must be in the range 0..9"))?
                }
                _ => Fixity::DEFAULT.prec,
            };

            let mut ops = Vec::new();
            loop {
                let (op, start, end) = if this.starts_parenthesized_operator_name() {
                    this.parse_parenthesized_operator_name()?
                } else {
                    this.expect_operator_symbol(false)?
                };
                ops.push(this.sp(op, start, end));

                if !this.at(&Token::Comma) {
                    break;
                }
                this.advance();
            }

            Ok(SurfaceDefinition::FixityDef {
                fixity: Fixity { prec, assoc },
                ops,
            })
        })
    }

    pub(super) fn parse_data_def(&mut self) -> Result<SurfaceDefinition, ParseError> {
        self.node(SyntaxKind::DataDef, |this| {
            this.expect(&Token::Data)?;
            let (name, start, end) = this.expect_upper()?;
            let name = this.sp(name, start, end);

            let mut ty_params = Vec::new();
            while matches!(this.current_kind(), Token::Ident(_)) {
                let (param, start, end) = this.expect_ident()?;
                ty_params.push(this.sp(param, start, end));
            }

            this.expect(&Token::Eq)?;

            let mut constructors = Vec::new();
            loop {
                if this.at(&Token::Pipe) {
                    this.advance();
                }

                let start = this.current_start();
                let ctor = this.parse_constructor()?;
                let end = this.current_start();
                constructors.push(sp(ctor, start, end, this.completed_cst_id()));

                if !this.at(&Token::Pipe) {
                    break;
                }
            }

            Ok(SurfaceDefinition::DataDef {
                name,
                ty_params,
                constructors,
            })
        })
    }

    pub(super) fn parse_type_alias_def(&mut self) -> Result<SurfaceDefinition, ParseError> {
        self.node(SyntaxKind::TypeAliasDef, |this| {
            this.expect(&Token::Type)?;
            let (name, start, end) = this.expect_upper()?;
            let name = this.sp(name, start, end);

            let mut ty_params = Vec::new();
            while matches!(this.current_kind(), Token::Ident(_)) {
                let (param, start, end) = this.expect_ident()?;
                ty_params.push(this.sp(param, start, end));
            }

            this.expect(&Token::Eq)?;
            let ty = this.parse_type()?;

            Ok(SurfaceDefinition::TypeAliasDef {
                name,
                ty_params,
                ty,
            })
        })
    }

    pub(super) fn parse_constructor(&mut self) -> Result<SurfaceConstructor, ParseError> {
        self.node(SyntaxKind::Constructor, |this| {
            let (name, start, end) = this.expect_upper()?;
            let mut params = Vec::new();
            while this.is_type_atom_start() {
                params.push(this.parse_type_atom()?);
            }
            Ok(SurfaceConstructor {
                name: this.sp(name, start, end),
                params,
            })
        })
    }

    pub(super) fn parse_predicate(&mut self) -> Result<S<SurfacePredicate>, ParseError> {
        self.node(SyntaxKind::Predicate, |this| {
            let start = this.current_start();

            let class_name = this.parse_name_path()?;
            if !is_ctor_like(&class_name.0.last_text(this.session)) {
                return Err(this.error(format!(
                    "expected class name, got {}",
                    class_name.0.display(this.session)
                )));
            }

            let mut args = Vec::new();
            while this.is_type_atom_start() {
                args.push(this.parse_type_atom()?);
            }

            if args.is_empty() {
                return Err(
                    this.error("expected at least one type argument in predicate".to_string())
                );
            }

            let end = args.last().map_or_else(|| span_end(&class_name), span_end);

            Ok(this.sp(SurfacePredicate { class_name, args }, start, end))
        })
    }

    pub(super) fn parse_predicate_list(&mut self) -> Result<Vec<S<SurfacePredicate>>, ParseError> {
        self.node(SyntaxKind::PredicateList, |this| {
            if this.at(&Token::LParen) {
                this.advance();

                let mut preds = vec![this.parse_predicate()?];
                while this.at(&Token::Comma) {
                    this.advance();
                    preds.push(this.parse_predicate()?);
                }

                this.expect(&Token::RParen)?;
                Ok(preds)
            } else {
                Ok(vec![this.parse_predicate()?])
            }
        })
    }

    pub(super) fn parse_class_def(&mut self) -> Result<SurfaceDefinition, ParseError> {
        self.node(SyntaxKind::ClassDef, |this| {
            this.expect(&Token::Class)?;

            let checkpoint = this.checkpoint();
            let superclasses = if let Ok(preds) = this.parse_predicate_list() {
                if this.at(&Token::FatArrow) {
                    this.advance();
                    preds
                } else {
                    this.restore(checkpoint);
                    Vec::new()
                }
            } else {
                this.restore(checkpoint);
                Vec::new()
            };

            let (name, ns, ne) = this.expect_upper()?;

            let mut ty_params = Vec::new();
            while matches!(this.current_kind(), Token::Ident(_)) {
                let (p, ps, pe) = this.expect_ident()?;
                ty_params.push(this.sp(p, ps, pe));
            }

            if ty_params.is_empty() {
                return Err(
                    this.error("class declaration needs at least one type parameter".to_string())
                );
            }

            this.expect(&Token::Where)?;

            if this.at(&Token::VOpen) {
                this.advance();
            }

            let mut methods = Vec::new();
            let mut defaults = Vec::new();
            loop {
                this.skip_separators();
                if this.at(&Token::VClose) || this.at_eof() {
                    break;
                }

                if this.at(&Token::Default) {
                    this.advance();
                    let (mname, ms, me) = this.expect_value_name()?;
                    let mut mparams = Vec::new();
                    while matches!(this.current_kind(), Token::Ident(_) | Token::Underscore) {
                        let (p, ps, pe) = this.expect_binder()?;
                        mparams.push(this.sp(p, ps, pe));
                    }
                    this.expect(&Token::Eq)?;
                    let mbody = this.parse_expr()?;
                    defaults.push(InstanceMethod {
                        name: this.sp(mname, ms, me),
                        params: mparams,
                        body: mbody,
                        cst_id: this.current_cst_id(),
                    });
                    continue;
                }

                let (mname, ms, me) = this.expect_value_name()?;
                this.expect(&Token::Colon)?;
                let mty = this.parse_type()?;
                methods.push(ClassMethod {
                    name: this.sp(mname, ms, me),
                    ty: mty,
                    cst_id: this.current_cst_id(),
                });
            }

            if this.at(&Token::VClose) {
                this.advance();
            }

            Ok(SurfaceDefinition::ClassDef {
                name: this.sp(name, ns, ne),
                ty_params,
                superclasses,
                methods,
                defaults,
            })
        })
    }

    pub(super) fn parse_instance_def(&mut self) -> Result<SurfaceDefinition, ParseError> {
        self.node(SyntaxKind::InstanceDef, |this| {
            this.expect(&Token::Instance)?;

            let checkpoint = this.checkpoint();
            let context = if let Ok(preds) = this.parse_predicate_list() {
                if this.at(&Token::FatArrow) {
                    this.advance();
                    preds
                } else {
                    this.restore(checkpoint);
                    Vec::new()
                }
            } else {
                this.restore(checkpoint);
                Vec::new()
            };

            let (cname, cs, ce) = this.expect_upper()?;

            let mut args = Vec::new();
            while this.is_type_atom_start() {
                args.push(this.parse_type_atom()?);
            }

            if args.is_empty() {
                return Err(
                    this.error("instance head needs at least one type argument".to_string())
                );
            }

            this.expect(&Token::Where)?;

            if this.at(&Token::VOpen) {
                this.advance();
            }

            let mut methods = Vec::new();
            loop {
                this.skip_separators();
                if this.at(&Token::VClose) || this.at_eof() {
                    break;
                }
                let (mname, ms, me) = this.expect_value_name()?;
                let mut mparams = Vec::new();
                while matches!(this.current_kind(), Token::Ident(_) | Token::Underscore) {
                    let (p, ps, pe) = this.expect_binder()?;
                    mparams.push(this.sp(p, ps, pe));
                }
                this.expect(&Token::Eq)?;
                let mbody = this.parse_expr()?;
                methods.push(InstanceMethod {
                    name: this.sp(mname, ms, me),
                    params: mparams,
                    body: mbody,
                    cst_id: this.current_cst_id(),
                });
            }

            if this.at(&Token::VClose) {
                this.advance();
            }

            Ok(SurfaceDefinition::InstanceDef {
                context,
                class_name: this.sp(cname, cs, ce),
                args,
                methods,
            })
        })
    }
}
