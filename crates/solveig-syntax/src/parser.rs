use std::cell::Ref;

use solveig_base::{Diagnostic, FileId, FileSpan, IntoDiagnostic, Label, Session, Span, Symbol};

use crate::ast::*;
use crate::lexer::{self, LexError, PosToken, Token};

type S<T> = (T, Span);

type NameSpan = (Symbol, usize, usize);

type LambdaParam = (S<Symbol>, Option<S<SurfaceTy>>);

fn mk_span(start: usize, end: usize) -> Span {
    Span::from_usize_range(start..end)
}

fn sp<T>(value: T, start: usize, end: usize) -> S<T> {
    (value, mk_span(start, end))
}

fn span_start<T>(spanned: &S<T>) -> usize {
    spanned.1.start as usize
}

fn span_end<T>(spanned: &S<T>) -> usize {
    spanned.1.end as usize
}

fn merge_span<T, U>(start: &S<T>, end: &S<U>) -> (usize, usize) {
    (span_start(start), span_end(end))
}

fn is_ctor_like(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

fn qref(session: &Session, segments: &[&str], start: usize, end: usize) -> S<NameRef> {
    sp(
        NameRef {
            segments: segments
                .iter()
                .map(|segment| session.intern(segment))
                .collect(),
        },
        start,
        end,
    )
}

fn list_nil_pattern(session: &Session, start: usize, end: usize) -> S<SurfacePattern> {
    sp(
        SurfacePattern::Con(
            qref(session, &["prelude", "list", "Nil"], start, end),
            vec![],
        ),
        start,
        end,
    )
}

fn list_nil_expr(session: &Session, start: usize, end: usize) -> S<SurfaceExpr> {
    sp(
        SurfaceExpr::Con(qref(session, &["prelude", "list", "Nil"], start, end)),
        start,
        end,
    )
}

fn list_ty(session: &Session, elem_ty: S<SurfaceTy>, start: usize, end: usize) -> S<SurfaceTy> {
    let list_con = sp(
        SurfaceTy::Con(qref(session, &["prelude", "list", "List"], start, start)),
        start,
        start,
    );
    sp(
        SurfaceTy::App(Box::new(list_con), Box::new(elem_ty)),
        start,
        end,
    )
}

fn int_pattern(value: i64, start: usize, end: usize) -> S<SurfacePattern> {
    sp(SurfacePattern::Lit(SurfaceLiteral::Int(value)), start, end)
}

fn literal_pattern(literal: SurfaceLiteral, start: usize, end: usize) -> S<SurfacePattern> {
    sp(SurfacePattern::Lit(literal), start, end)
}

fn cons_pattern(
    session: &Session,
    head: S<SurfacePattern>,
    tail: S<SurfacePattern>,
) -> S<SurfacePattern> {
    let (start, end) = merge_span(&head, &tail);
    sp(
        SurfacePattern::Con(
            qref(session, &["prelude", "list", "Cons"], start, start),
            vec![head, tail],
        ),
        start,
        end,
    )
}

fn list_pattern(
    session: &Session,
    elems: Vec<S<SurfacePattern>>,
    _start: usize,
    end: usize,
) -> S<SurfacePattern> {
    elems
        .into_iter()
        .rev()
        .fold(list_nil_pattern(session, end, end), |acc, elem| {
            cons_pattern(session, elem, acc)
        })
}

fn cons_expr(
    session: &Session,
    head: S<SurfaceExpr>,
    tail: S<SurfaceExpr>,
    start: usize,
    end: usize,
) -> S<SurfaceExpr> {
    let cons = sp(
        SurfaceExpr::Con(qref(session, &["prelude", "list", "Cons"], start, start)),
        start,
        start,
    );
    let app = sp(SurfaceExpr::App(Box::new(cons), Box::new(head)), start, end);
    sp(SurfaceExpr::App(Box::new(app), Box::new(tail)), start, end)
}

fn list_expr(
    session: &Session,
    elems: Vec<S<SurfaceExpr>>,
    _start: usize,
    end: usize,
) -> S<SurfaceExpr> {
    elems
        .into_iter()
        .rev()
        .fold(list_nil_expr(session, end, end), |acc, elem| {
            let start = span_start(&elem);
            let end = span_end(&acc);
            cons_expr(session, elem, acc, start, end)
        })
}

#[derive(Debug, Clone)]
pub struct ParseError {
    pub span: FileSpan,
    pub message: String,
}

impl From<LexError> for ParseError {
    fn from(error: LexError) -> Self {
        Self {
            span: error.span,
            message: error.message,
        }
    }
}

impl ParseError {
    pub fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic::error("PAR0001", "parse error")
            .with_label(Label::primary(self.span, self.message.clone()))
    }
}

impl IntoDiagnostic for ParseError {
    fn into_diagnostic(self, (): ()) -> Diagnostic {
        self.to_diagnostic()
    }
}

impl IntoDiagnostic for &ParseError {
    fn into_diagnostic(self, (): ()) -> Diagnostic {
        self.to_diagnostic()
    }
}

pub struct Parser<'a> {
    session: &'a Session,
    file: FileId,
    tokens: Vec<PosToken>,
    pos: usize,
}

impl<'a> Parser<'a> {
    pub fn new(session: &'a Session, file: FileId, tokens: Vec<PosToken>) -> Self {
        Self {
            session,
            file,
            tokens,
            pos: 0,
        }
    }

    fn current(&self) -> &PosToken {
        &self.tokens[self.pos.min(self.tokens.len().saturating_sub(1))]
    }

    fn current_kind(&self) -> &Token {
        &self.current().kind
    }

    fn current_start(&self) -> usize {
        self.current().start
    }

    fn at(&self, token: &Token) -> bool {
        std::mem::discriminant(self.current_kind()) == std::mem::discriminant(token)
    }

    fn at_eof(&self) -> bool {
        matches!(self.current_kind(), Token::Eof)
    }

    fn advance(&mut self) -> &PosToken {
        let token = &self.tokens[self.pos];
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        token
    }

    fn current_span(&self) -> FileSpan {
        let token = self.current();
        FileSpan::new(self.file, mk_span(token.start, token.end))
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            span: self.current_span(),
            message: message.into(),
        }
    }

    fn report_error(&self, error: ParseError) {
        self.session.diagnostics().push(error.to_diagnostic());
    }

    fn token_text(&self, symbol: Symbol) -> Ref<'_, str> {
        self.session.str(symbol)
    }

    fn describe_current(&self) -> String {
        match self.current_kind() {
            Token::Ident(symbol) => format!("identifier `{}`", self.token_text(*symbol)),
            Token::UpperIdent(symbol) => format!("constructor `{}`", self.token_text(*symbol)),
            Token::Operator(symbol) => format!("operator `{}`", self.token_text(*symbol)),
            Token::ReservedOp(symbol) => {
                format!("reserved operator `{}`", self.token_text(*symbol))
            }
            Token::Int(value) => format!("integer literal `{value}`"),
            Token::Float(value) => format!("float literal `{value}`"),
            Token::Char(value) => format!("char literal `{value}`"),
            Token::Str(_) => "string literal".to_string(),
            other => format!("{other:?}"),
        }
    }

    fn expect(&mut self, token: &Token) -> Result<&PosToken, ParseError> {
        if self.at(token) {
            Ok(self.advance())
        } else {
            Err(self.error(format!(
                "expected {:?}, got {}",
                token,
                self.describe_current()
            )))
        }
    }

    fn expect_ident(&mut self) -> Result<NameSpan, ParseError> {
        match self.current_kind() {
            Token::Ident(name) => {
                let name = *name;
                let token = self.advance();
                Ok((name, token.start, token.end))
            }
            _ => Err(self.error(format!(
                "expected identifier, got {}",
                self.describe_current()
            ))),
        }
    }

    fn expect_upper(&mut self) -> Result<NameSpan, ParseError> {
        match self.current_kind() {
            Token::UpperIdent(name) => {
                let name = *name;
                let token = self.advance();
                Ok((name, token.start, token.end))
            }
            _ => Err(self.error(format!(
                "expected constructor, got {}",
                self.describe_current()
            ))),
        }
    }

    fn expect_module_name_segment(&mut self) -> Result<Symbol, ParseError> {
        match self.current_kind() {
            Token::Ident(name) | Token::UpperIdent(name) => {
                let name = *name;
                self.advance();
                Ok(name)
            }
            _ => Err(self.error(format!(
                "expected module name, got {}",
                self.describe_current()
            ))),
        }
    }

    fn fixed_operator_text(token: &Token) -> Option<&'static str> {
        match token {
            Token::Plus => Some("+"),
            Token::Minus => Some("-"),
            Token::Star => Some("*"),
            Token::Slash => Some("/"),
            Token::Percent => Some("%"),
            Token::EqEq => Some("=="),
            Token::Less => Some("<"),
            Token::Greater => Some(">"),
            Token::LessEq => Some("<="),
            Token::GreaterEq => Some(">="),
            Token::Cons => Some("::"),
            Token::Dot => Some("."),
            _ => None,
        }
    }

    fn current_operator_symbol(&self) -> Option<(Symbol, bool)> {
        match self.current_kind() {
            Token::Operator(symbol) => Some((*symbol, false)),
            token => Self::fixed_operator_text(token)
                .map(|text| (self.session.intern(text), matches!(token, Token::Cons))),
        }
    }

    fn is_infix_operator(&self) -> bool {
        self.current_operator_symbol().is_some()
    }

    fn expect_operator_symbol(&mut self, allow_cons_syntax: bool) -> Result<NameSpan, ParseError> {
        let Some((symbol, is_cons_syntax)) = self.current_operator_symbol() else {
            return Err(self.error(format!(
                "expected operator, got {}",
                self.describe_current()
            )));
        };

        if is_cons_syntax && !allow_cons_syntax {
            return Err(self.error("list cons `::` is reserved syntax, not an operator name"));
        }

        let is_reserved_constructor_operator = {
            let text = self.token_text(symbol);
            text.starts_with(':') && !is_cons_syntax
        };

        if is_reserved_constructor_operator {
            return Err(self.error(
                "operators starting with `:` are reserved for future constructor operators",
            ));
        }

        let token = self.advance();
        Ok((symbol, token.start, token.end))
    }

    fn parse_parenthesized_operator_name(&mut self) -> Result<NameSpan, ParseError> {
        let start = self.expect(&Token::LParen)?.start;
        let (symbol, _, _) = self.expect_operator_symbol(false)?;
        let end = self.expect(&Token::RParen)?.end;
        Ok((symbol, start, end))
    }

    fn starts_parenthesized_operator_name(&self) -> bool {
        if !self.at(&Token::LParen) {
            return false;
        }

        self.tokens
            .get(self.pos + 1)
            .is_some_and(|token| match &token.kind {
                Token::Operator(_) => true,
                other => Self::fixed_operator_text(other)
                    .is_some_and(|text| text != "::" && !text.starts_with(':')),
            })
            && self
                .tokens
                .get(self.pos + 2)
                .is_some_and(|token| matches!(token.kind, Token::RParen))
    }

    fn expect_value_name(&mut self) -> Result<NameSpan, ParseError> {
        if self.starts_parenthesized_operator_name() {
            self.parse_parenthesized_operator_name()
        } else {
            self.expect_ident()
        }
    }

    fn parse_import_name(&mut self) -> Result<Symbol, ParseError> {
        if self.starts_parenthesized_operator_name() {
            Ok(self.parse_parenthesized_operator_name()?.0)
        } else {
            self.expect_module_name_segment()
        }
    }

    fn expect_binder(&mut self) -> Result<NameSpan, ParseError> {
        match self.current_kind() {
            Token::Ident(name) => {
                let name = *name;
                let token = self.advance();
                Ok((name, token.start, token.end))
            }
            Token::Underscore => {
                let underscore = self.session.intern("_");
                let token = self.advance();
                Ok((underscore, token.start, token.end))
            }
            _ => Err(self.error(format!("expected binder, got {}", self.describe_current()))),
        }
    }

    fn skip_separators(&mut self) {
        while matches!(self.current_kind(), Token::VSemi | Token::Semicolon) {
            self.advance();
        }
    }

    fn is_final_module_vclose(&self) -> bool {
        if !self.at(&Token::VClose) {
            return false;
        }

        let mut index = self.pos;
        while index < self.tokens.len()
            && matches!(
                self.tokens[index].kind,
                Token::VClose | Token::VSemi | Token::Semicolon
            )
        {
            index += 1;
        }

        index >= self.tokens.len() || matches!(self.tokens[index].kind, Token::Eof)
    }

    fn skip_top_level_junk(&mut self) {
        loop {
            self.skip_separators();
            if self.at(&Token::VClose) && !self.is_final_module_vclose() {
                self.advance();
                continue;
            }
            break;
        }
    }

    fn layout_depth_before_current(&self) -> usize {
        self.tokens[..self.pos.min(self.tokens.len())]
            .iter()
            .fold(0usize, |depth, token| match token.kind {
                Token::VOpen => depth + 1,
                Token::VClose => depth.saturating_sub(1),
                _ => depth,
            })
    }

    fn is_top_level_sync_keyword(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::Data
                | Token::Sig
                | Token::Class
                | Token::Instance
                | Token::Import
                | Token::Infixl
                | Token::Infixr
                | Token::Infix
        )
    }

    fn recover_top_level(&mut self) {
        let mut depth = self.layout_depth_before_current();
        let start_pos = self.pos;

        while !self.at_eof() {
            match self.current_kind() {
                Token::VOpen => {
                    depth += 1;
                    self.advance();
                }
                Token::VClose => {
                    if depth <= 1 {
                        return;
                    }

                    depth -= 1;
                    self.advance();
                }
                Token::VSemi | Token::Semicolon if depth <= 1 => {
                    self.advance();
                    return;
                }
                _ if self.pos != start_pos && depth <= 1 && self.is_top_level_sync_keyword() => {
                    return;
                }
                _ => {
                    self.advance();
                }
            }
        }
    }

    fn parse_module(&mut self) -> ParsedModule {
        if self.at(&Token::VOpen) {
            self.advance();
        }

        let imports = self.parse_imports();
        let defs = self.parse_definitions();

        if self.at(&Token::VClose) {
            self.advance();
        }

        ParsedModule {
            file: self.file,
            imports,
            defs,
        }
    }

    fn parse_imports(&mut self) -> Vec<ImportDecl> {
        let mut imports = Vec::new();
        while self.at(&Token::Import) {
            match self.parse_import() {
                Ok(import) => imports.push(import),
                Err(error) => {
                    self.report_error(error);
                    self.recover_top_level();
                }
            }
            self.skip_separators();
        }
        imports
    }

    fn parse_import(&mut self) -> Result<ImportDecl, ParseError> {
        let start = self.current_start();
        self.expect(&Token::Import)?;

        let mut segments = vec![self.expect_module_name_segment()?];
        while self.at(&Token::Dot) {
            self.advance();
            segments.push(self.expect_module_name_segment()?);
        }

        let module = ModuleName(segments);
        let alias = if self.at(&Token::As) {
            self.advance();
            self.expect_upper()?.0
        } else {
            module.default_alias()
        };

        let spec = if self.at(&Token::LParen) {
            self.advance();
            let mut names = Vec::new();
            while !self.at(&Token::RParen) {
                names.push(self.parse_import_name()?);
                if !self.at(&Token::Comma) {
                    break;
                }
                self.advance();
            }
            self.expect(&Token::RParen)?;
            ImportSpec::Only(names)
        } else {
            ImportSpec::All
        };
        let end = self.current_start();

        Ok(ImportDecl {
            module,
            alias,
            spec,
            span: mk_span(start, end),
        })
    }

    fn parse_definitions(&mut self) -> SurfaceProgram {
        let mut defs = Vec::new();

        loop {
            self.skip_top_level_junk();
            if self.at_eof() || self.is_final_module_vclose() {
                break;
            }

            let start = self.current_start();
            match self.parse_definition() {
                Ok(def) => {
                    let end = self.current_start();
                    defs.push(sp(def, start, end));
                }
                Err(error) => {
                    self.report_error(error);
                    self.recover_top_level();
                }
            }
        }

        defs
    }

    fn parse_definition(&mut self) -> Result<SurfaceDefinition, ParseError> {
        match self.current_kind() {
            Token::Data => self.parse_data_def(),
            Token::Class => self.parse_class_def(),
            Token::Instance => self.parse_instance_def(),
            Token::Infixl | Token::Infixr | Token::Infix => self.parse_fixity_def(),
            _ => self.parse_func_or_sig(),
        }
    }

    fn parse_func_or_sig(&mut self) -> Result<SurfaceDefinition, ParseError> {
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

    fn parse_fixity_def(&mut self) -> Result<SurfaceDefinition, ParseError> {
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

    fn parse_data_def(&mut self) -> Result<SurfaceDefinition, ParseError> {
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

    fn parse_constructor(&mut self) -> Result<SufraceConstructor, ParseError> {
        let (name, start, end) = self.expect_upper()?;
        let mut params = Vec::new();
        while self.is_type_atom_start() {
            params.push(self.parse_type_atom()?);
        }
        Ok(SufraceConstructor {
            name: sp(name, start, end),
            params,
        })
    }

    fn parse_predicate(&mut self) -> Result<S<SurfacePredicate>, ParseError> {
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

        let end = args
            .last()
            .map(span_end)
            .unwrap_or_else(|| span_end(&class_name));

        Ok(sp(SurfacePredicate { class_name, args }, start, end))
    }

    fn parse_predicate_list(&mut self) -> Result<Vec<S<SurfacePredicate>>, ParseError> {
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

    fn parse_class_def(&mut self) -> Result<SurfaceDefinition, ParseError> {
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

    fn parse_instance_def(&mut self) -> Result<SurfaceDefinition, ParseError> {
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

    fn parse_qualified_type(&mut self) -> Result<S<SurfaceTy>, ParseError> {
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

    fn parse_type(&mut self) -> Result<S<SurfaceTy>, ParseError> {
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

    fn parse_type_arrow(&mut self) -> Result<S<SurfaceTy>, ParseError> {
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

    fn parse_type_app(&mut self) -> Result<S<SurfaceTy>, ParseError> {
        let mut ty = self.parse_type_atom()?;
        while self.is_type_atom_start() {
            let arg = self.parse_type_atom()?;
            let (start, end) = merge_span(&ty, &arg);
            ty = sp(SurfaceTy::App(Box::new(ty), Box::new(arg)), start, end);
        }
        Ok(ty)
    }

    fn is_type_atom_start(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::Ident(_) | Token::UpperIdent(_) | Token::LParen | Token::LBrack
        )
    }

    fn parse_name_path(&mut self) -> Result<S<NameRef>, ParseError> {
        let start = self.current_start();

        let first = match self.current_kind() {
            Token::Ident(name) | Token::UpperIdent(name) => {
                let name = *name;
                self.advance();
                name
            }
            _ => return Err(self.error(format!("expected name, got {}", self.describe_current()))),
        };

        let mut segments = vec![first];
        while self.current_dot_is_qualified_separator() {
            match self.tokens.get(self.pos + 1).map(|token| &token.kind) {
                Some(Token::Ident(name)) | Some(Token::UpperIdent(name)) => {
                    let name = *name;
                    self.advance();
                    self.advance();
                    segments.push(name);
                }
                _ => break,
            }
        }

        Ok(sp(
            NameRef { segments },
            start,
            self.tokens[self.pos.saturating_sub(1)].end,
        ))
    }

    fn current_dot_is_qualified_separator(&self) -> bool {
        if !self.at(&Token::Dot) || self.pos == 0 {
            return false;
        }

        let previous = &self.tokens[self.pos - 1];
        let dot = self.current();
        let Some(next) = self.tokens.get(self.pos + 1) else {
            return false;
        };

        previous.end == dot.start
            && dot.end == next.start
            && matches!(next.kind, Token::Ident(_) | Token::UpperIdent(_))
    }

    fn is_case_block_follower(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::RParen
                | Token::RBrack
                | Token::Comma
                | Token::Then
                | Token::Else
                | Token::In
                | Token::BindArrow
                | Token::Semicolon
                | Token::VSemi
                | Token::VClose
                | Token::Eof
        )
    }

    fn parse_type_atom(&mut self) -> Result<S<SurfaceTy>, ParseError> {
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

    fn parse_pattern(&mut self) -> Result<S<SurfacePattern>, ParseError> {
        let head = self.parse_pattern_head()?;
        if self.at(&Token::Cons) {
            self.advance();
            let tail = self.parse_pattern()?;
            Ok(cons_pattern(self.session, head, tail))
        } else {
            Ok(head)
        }
    }

    fn parse_pattern_head(&mut self) -> Result<S<SurfacePattern>, ParseError> {
        if matches!(self.current_kind(), Token::UpperIdent(_)) {
            let start = self.current_start();
            let name_ref = self.parse_name_path()?;
            let mut args = Vec::new();
            while self.is_pattern_arg_start() {
                args.push(self.parse_pattern_arg()?);
            }
            let end = args
                .last()
                .map(span_end)
                .unwrap_or_else(|| span_end(&name_ref));
            Ok(sp(SurfacePattern::Con(name_ref, args), start, end))
        } else {
            self.parse_pattern_arg()
        }
    }

    fn parse_pattern_arg(&mut self) -> Result<S<SurfacePattern>, ParseError> {
        let start = self.current_start();

        match self.current_kind() {
            Token::Int(value) => {
                let value = *value;
                let token = self.advance();
                Ok(int_pattern(value, token.start, token.end))
            }

            Token::Float(value) => {
                let value = *value;
                let token = self.advance();
                Ok(literal_pattern(
                    SurfaceLiteral::Float(value),
                    token.start,
                    token.end,
                ))
            }

            Token::Char(value) => {
                let value = *value;
                let token = self.advance();
                Ok(literal_pattern(
                    SurfaceLiteral::Char(value),
                    token.start,
                    token.end,
                ))
            }

            Token::Str(value) => {
                let value = value.clone();
                let token = self.advance();
                Ok(literal_pattern(
                    SurfaceLiteral::Str(value),
                    token.start,
                    token.end,
                ))
            }

            Token::Minus => {
                // fix mutable borrow later
                let minus = self.advance().clone();
                match self.current_kind() {
                    Token::Int(value) => {
                        let value = value
                            .checked_neg()
                            .ok_or_else(|| self.error("integer literal pattern is too small"))?;

                        let int_token = self.advance();
                        Ok(int_pattern(value, minus.start, int_token.end))
                    }

                    Token::Float(value) => {
                        let value = -*value;
                        let float_token = self.advance();
                        Ok(literal_pattern(
                            SurfaceLiteral::Float(value),
                            minus.start,
                            float_token.end,
                        ))
                    }

                    _ => Err(self.error(format!(
                        "expected numeric literal after `-` in pattern, got {}",
                        self.describe_current()
                    ))),
                }
            }

            Token::Underscore => {
                let token = self.advance();
                Ok(sp(SurfacePattern::Wild, token.start, token.end))
            }

            Token::Ident(_) => {
                let name_ref = self.parse_name_path()?;
                let end = span_end(&name_ref);

                if name_ref.0.segments.len() == 1 {
                    Ok(sp(SurfacePattern::Var(name_ref.0.last()), start, end))
                } else {
                    Err(self.error(format!(
                        "qualified variable patterns are not supported: {}",
                        name_ref.0.display(self.session)
                    )))
                }
            }

            Token::UpperIdent(_) => {
                let name_ref = self.parse_name_path()?;
                let end = span_end(&name_ref);
                Ok(sp(SurfacePattern::Con(name_ref, vec![]), start, end))
            }

            Token::LParen => {
                self.advance();

                if self.at(&Token::RParen) {
                    return Err(self.error("unit pattern `()` is not supported yet"));
                }

                let inner = self.parse_pattern()?;
                let end = self.expect(&Token::RParen)?.end;
                Ok(sp(inner.0, start, end))
            }

            Token::LBrack => {
                self.advance();

                if self.at(&Token::RBrack) {
                    let end = self.advance().end;
                    return Ok(list_nil_pattern(self.session, start, end));
                }

                let mut elems = vec![self.parse_pattern()?];

                while self.at(&Token::Comma) {
                    self.advance();
                    elems.push(self.parse_pattern()?);
                }

                let end = self.expect(&Token::RBrack)?.end;
                Ok(list_pattern(self.session, elems, start, end))
            }

            _ => Err(self.error(format!("expected pattern, got {}", self.describe_current()))),
        }
    }

    fn is_pattern_arg_start(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::Int(_)
                | Token::Float(_)
                | Token::Char(_)
                | Token::Str(_)
                | Token::Minus
                | Token::Ident(_)
                | Token::UpperIdent(_)
                | Token::LParen
                | Token::LBrack
                | Token::Underscore
        )
    }

    fn parse_expr(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let start = self.current_start();

        let expr = match self.current_kind() {
            Token::Fun => return self.parse_lambda(),
            Token::Let => return self.parse_let(),
            Token::Do => return self.parse_do(),
            Token::If => return self.parse_if(),
            Token::Case => return self.parse_case(),
            _ => self.parse_op_chain()?,
        };

        if self.at(&Token::Colon) {
            self.advance();
            let ty = self.parse_type()?;
            let end = span_end(&ty);
            return Ok(sp(SurfaceExpr::Ann(Box::new(expr), ty), start, end));
        }

        Ok(expr)
    }

    fn parse_op_chain(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let first = self.parse_op_operand()?;
        let mut segments = Vec::new();

        while self.is_infix_operator() {
            let (op, start, end) = self.expect_operator_symbol(true)?;
            let rhs = self.parse_op_operand()?;
            segments.push(SurfaceOpSegment {
                op: sp(op, start, end),
                rhs,
            });
        }

        if segments.is_empty() && first.prefix_minuses.is_empty() {
            Ok(first.expr)
        } else {
            let start = first
                .prefix_minuses
                .first()
                .map(|span| span.start as usize)
                .unwrap_or_else(|| span_start(&first.expr));
            let end = segments
                .last()
                .map(|segment| span_end(&segment.rhs.expr))
                .unwrap_or_else(|| span_end(&first.expr));
            Ok(sp(
                SurfaceExpr::OpSeq(Box::new(first), segments),
                start,
                end,
            ))
        }
    }

    fn parse_op_operand(&mut self) -> Result<SurfaceOpOperand, ParseError> {
        let mut prefix_minuses = Vec::new();
        while self.at(&Token::Minus) {
            let token = self.advance();
            prefix_minuses.push(mk_span(token.start, token.end));
        }

        let expr = self.parse_app()?;
        Ok(SurfaceOpOperand {
            prefix_minuses,
            expr,
        })
    }

    fn parse_app(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let mut func = self.parse_atom()?;
        while self.is_atom_start() {
            let arg = self.parse_atom()?;
            let (start, end) = merge_span(&func, &arg);
            func = sp(SurfaceExpr::App(Box::new(func), Box::new(arg)), start, end);
        }
        Ok(func)
    }

    fn is_atom_start(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::Int(_)
                | Token::Float(_)
                | Token::Char(_)
                | Token::Str(_)
                | Token::Ident(_)
                | Token::UpperIdent(_)
                | Token::LParen
                | Token::LBrack
                | Token::Hole
                | Token::Underscore
        )
    }

    fn parse_atom(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let start = self.current_start();
        match self.current_kind() {
            Token::Int(value) => {
                let value = *value;
                let token = self.advance();
                Ok(sp(
                    SurfaceExpr::Literal(SurfaceLiteral::Int(value)),
                    token.start,
                    token.end,
                ))
            }
            Token::Float(value) => {
                let value = *value;
                let token = self.advance();
                Ok(sp(
                    SurfaceExpr::Literal(SurfaceLiteral::Float(value)),
                    token.start,
                    token.end,
                ))
            }
            Token::Char(value) => {
                let value = *value;
                let token = self.advance();
                Ok(sp(
                    SurfaceExpr::Literal(SurfaceLiteral::Char(value)),
                    token.start,
                    token.end,
                ))
            }
            Token::Str(value) => {
                let value = value.clone();
                let token = self.advance();
                Ok(sp(
                    SurfaceExpr::Literal(SurfaceLiteral::Str(value)),
                    token.start,
                    token.end,
                ))
            }
            Token::Hole | Token::Underscore => {
                let token = self.advance();
                Ok(sp(SurfaceExpr::Hole, token.start, token.end))
            }
            Token::Ident(_) | Token::UpperIdent(_) => {
                let name_ref = self.parse_name_path()?;
                let end = span_end(&name_ref);
                let expr = if is_ctor_like(&name_ref.0.last_text(self.session)) {
                    SurfaceExpr::Con(name_ref)
                } else {
                    SurfaceExpr::Var(name_ref)
                };
                Ok(sp(expr, start, end))
            }
            Token::LParen => {
                if self.starts_parenthesized_operator_name() {
                    let (symbol, start, end) = self.parse_parenthesized_operator_name()?;
                    let name_ref = sp(
                        NameRef {
                            segments: vec![symbol],
                        },
                        start,
                        end,
                    );
                    return Ok(sp(SurfaceExpr::Var(name_ref), start, end));
                }

                self.advance();
                if self.at(&Token::RParen) {
                    let end = self.advance().end;
                    return Ok(sp(SurfaceExpr::Unit, start, end));
                }
                let inner = self.parse_expr()?;
                let end = self.expect(&Token::RParen)?.end;
                Ok(sp(inner.0, start, end))
            }
            Token::LBrack => {
                self.advance();
                if self.at(&Token::RBrack) {
                    let end = self.advance().end;
                    return Ok(list_nil_expr(self.session, start, end));
                }
                let mut elems = vec![self.parse_expr()?];
                while self.at(&Token::Comma) {
                    self.advance();
                    elems.push(self.parse_expr()?);
                }
                let end = self.expect(&Token::RBrack)?.end;
                Ok(list_expr(self.session, elems, start, end))
            }
            _ => Err(self.error(format!(
                "expected expression, got {}",
                self.describe_current()
            ))),
        }
    }

    fn parse_lambda(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let start = self.expect(&Token::Fun)?.start;
        let mut params: Vec<LambdaParam> = Vec::new();

        while !self.at(&Token::FatArrow) {
            if self.at(&Token::LParen) {
                self.advance();
                let (name, start, end) = self.expect_binder()?;
                self.expect(&Token::Colon)?;
                let ty = self.parse_type()?;
                self.expect(&Token::RParen)?;
                params.push((sp(name, start, end), Some(ty)));
            } else {
                let (name, start, end) = self.expect_binder()?;
                params.push((sp(name, start, end), None));
            }
        }

        self.expect(&Token::FatArrow)?;
        let body = self.parse_expr()?;
        let end = span_end(&body);
        Ok(sp(SurfaceExpr::Abs(params, Box::new(body)), start, end))
    }

    fn parse_let(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let start = self.expect(&Token::Let)?.start;
        let (name, name_start, name_end) = self.expect_ident()?;
        self.expect(&Token::Eq)?;
        let binding = self.parse_expr()?;
        self.expect(&Token::In)?;
        let body = self.parse_expr()?;
        let end = span_end(&body);
        Ok(sp(
            SurfaceExpr::Let(
                sp(name, name_start, name_end),
                Box::new(binding),
                Box::new(body),
            ),
            start,
            end,
        ))
    }

    fn parse_if(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let start = self.expect(&Token::If)?.start;
        let cond = self.parse_expr()?;
        self.expect(&Token::Then)?;
        let then_branch = self.parse_expr()?;
        self.expect(&Token::Else)?;
        let else_branch = self.parse_expr()?;
        let end = span_end(&else_branch);
        Ok(sp(
            SurfaceExpr::IfThenElse(Box::new(cond), Box::new(then_branch), Box::new(else_branch)),
            start,
            end,
        ))
    }

    fn parse_do(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let start = self.expect(&Token::Do)?.start;

        let implicit_layout = if self.at(&Token::VOpen) {
            self.advance();
            true
        } else {
            false
        };

        let mut statements = Vec::new();
        loop {
            self.skip_separators();

            if self.at(&Token::VClose) {
                if implicit_layout {
                    self.advance();
                }
                break;
            }

            if self.at_eof() || (implicit_layout && self.is_case_block_follower()) {
                break;
            }

            let stmt_start = self.current_start();
            let stmt = self.parse_do_statement()?;
            let stmt_end = match &stmt {
                SurfaceDoStmt::Bind { expr, .. }
                | SurfaceDoStmt::Let { expr, .. }
                | SurfaceDoStmt::Expr(expr) => span_end(expr),
            };
            statements.push(sp(stmt, stmt_start, stmt_end));
        }

        if statements.is_empty() {
            return Err(self.error("do block must contain at least one statement"));
        }

        let last = statements
            .last()
            .expect("ICE: checked non-empty do block before reading final statement");
        if !matches!(last.0, SurfaceDoStmt::Expr(_)) {
            return Err(ParseError {
                span: FileSpan::new(self.file, last.1),
                message: "final do statement must be an expression".to_string(),
            });
        }

        let end = span_end(last);
        Ok(sp(SurfaceExpr::Do(statements), start, end))
    }

    fn parse_do_statement(&mut self) -> Result<SurfaceDoStmt, ParseError> {
        if self.at(&Token::Let) {
            self.advance();
            let (name, start, end) = self.expect_ident()?;
            self.expect(&Token::Eq)?;
            let expr = self.parse_expr()?;
            return Ok(SurfaceDoStmt::Let {
                binder: sp(name, start, end),
                expr,
            });
        }

        if matches!(self.current_kind(), Token::Ident(_) | Token::Underscore)
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|token| matches!(token.kind, Token::BindArrow))
        {
            let (name, start, end) = self.expect_binder()?;
            self.expect(&Token::BindArrow)?;
            let expr = self.parse_expr()?;
            return Ok(SurfaceDoStmt::Bind {
                binder: sp(name, start, end),
                expr,
            });
        }

        Ok(SurfaceDoStmt::Expr(self.parse_expr()?))
    }

    fn parse_case(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let start = self.expect(&Token::Case)?.start;
        let scrutinee = self.parse_expr()?;
        self.expect(&Token::Of)?;

        let implicit_layout = if self.at(&Token::VOpen) {
            self.advance();
            true
        } else {
            false
        };

        let mut arms = Vec::new();
        loop {
            self.skip_separators();

            if self.at(&Token::VClose) {
                if implicit_layout {
                    self.advance();
                }
                break;
            }

            if self.at_eof() || (implicit_layout && self.is_case_block_follower()) {
                break;
            }

            if self.at(&Token::Pipe) {
                self.advance();
            }

            let start = self.current_start();
            let pattern = self.parse_pattern()?;
            self.expect(&Token::FatArrow)?;
            let expr = self.parse_expr()?;
            let end = span_end(&expr);
            arms.push(sp(SurfaceMatchArm { pattern, expr }, start, end));
        }

        let end = arms
            .last()
            .map(span_end)
            .unwrap_or_else(|| span_end(&scrutinee));
        Ok(sp(SurfaceExpr::Case(Box::new(scrutinee), arms), start, end))
    }
}

pub fn parse_source(session: &Session, file: FileId, source: &str) -> ParsedModule {
    let tokens = match lexer::lex(session, file, source) {
        Ok(tokens) => tokens,
        Err(error) => {
            let error = ParseError::from(error);
            session.diagnostics().push(error.to_diagnostic());

            return ParsedModule {
                file,
                imports: Vec::new(),
                defs: Vec::new(),
            };
        }
    };

    Parser::new(session, file, tokens).parse_module()
}
