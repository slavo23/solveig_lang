use std::path::Path;

use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::span::SimpleSpan;

use crate::ast::*;
use crate::lexer::{self, Assoc, Fixity, LexError, PosToken, Token, builtin_fixity};

type Span = SimpleSpan;
type S<T> = (T, Span);

type NameSpan<'src> = (&'src str, usize, usize);

type LambdaParam<'src> = (S<&'src str>, Option<S<SurfaceTy<'src>>>);

type BinOpInfo<'src> = (Token<'src>, usize, usize);

type StackedOp<'src> = (Token<'src>, usize, usize, Fixity);

fn mk_span(start: usize, end: usize) -> Span {
    (start..end).into()
}

fn sp<T>(value: T, start: usize, end: usize) -> S<T> {
    (value, mk_span(start, end))
}

fn span_start<T>(spanned: &S<T>) -> usize {
    spanned.1.start
}

fn span_end<T>(spanned: &S<T>) -> usize {
    spanned.1.end
}

fn merge_span<T, U>(start: &S<T>, end: &S<U>) -> (usize, usize) {
    (span_start(start), span_end(end))
}

fn is_ctor_like(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

fn qref<'src>(segments: &[&'src str], start: usize, end: usize) -> S<NameRef<'src>> {
    sp(
        NameRef {
            segments: segments.to_vec(),
        },
        start,
        end,
    )
}

fn list_nil_pattern<'src>(start: usize, end: usize) -> S<SurfacePattern<'src>> {
    sp(
        SurfacePattern::Con(qref(&["prelude", "list", "Nil"], start, end), vec![]),
        start,
        end,
    )
}

fn list_nil_expr<'src>(start: usize, end: usize) -> S<SurfaceExpr<'src>> {
    sp(
        SurfaceExpr::Con(qref(&["prelude", "list", "Nil"], start, end)),
        start,
        end,
    )
}

fn list_ty<'src>(elem_ty: S<SurfaceTy<'src>>, start: usize, end: usize) -> S<SurfaceTy<'src>> {
    let list_con = sp(
        SurfaceTy::Con(qref(&["prelude", "list", "List"], start, start)),
        start,
        start,
    );
    sp(
        SurfaceTy::App(Box::new(list_con), Box::new(elem_ty)),
        start,
        end,
    )
}

fn cons_pattern<'src>(
    head: S<SurfacePattern<'src>>,
    tail: S<SurfacePattern<'src>>,
) -> S<SurfacePattern<'src>> {
    let (start, end) = merge_span(&head, &tail);
    sp(
        SurfacePattern::Con(
            qref(&["prelude", "list", "Cons"], start, start),
            vec![head, tail],
        ),
        start,
        end,
    )
}

fn list_pattern<'src>(
    elems: Vec<S<SurfacePattern<'src>>>,
    _start: usize,
    end: usize,
) -> S<SurfacePattern<'src>> {
    elems
        .into_iter()
        .rev()
        .fold(list_nil_pattern(end, end), |acc, elem| {
            cons_pattern(elem, acc)
        })
}

fn cons_expr<'src>(
    head: S<SurfaceExpr<'src>>,
    tail: S<SurfaceExpr<'src>>,
    start: usize,
    end: usize,
) -> S<SurfaceExpr<'src>> {
    let cons = sp(
        SurfaceExpr::Con(qref(&["prelude", "list", "Cons"], start, start)),
        start,
        start,
    );
    let app = sp(SurfaceExpr::App(Box::new(cons), Box::new(head)), start, end);
    sp(SurfaceExpr::App(Box::new(app), Box::new(tail)), start, end)
}

fn list_expr<'src>(
    elems: Vec<S<SurfaceExpr<'src>>>,
    _start: usize,
    end: usize,
) -> S<SurfaceExpr<'src>> {
    elems
        .into_iter()
        .rev()
        .fold(list_nil_expr(end, end), |acc, elem| {
            let start = span_start(&elem);
            let end = span_end(&acc);
            cons_expr(elem, acc, start, end)
        })
}

#[derive(Debug, Clone)]
pub struct ParseError {
    pub offset: usize,
    pub line: u32,
    pub col: u32,
    pub message: String,
}

impl From<LexError> for ParseError {
    fn from(error: LexError) -> Self {
        Self {
            offset: error.offset,
            line: error.line,
            col: error.col,
            message: error.message,
        }
    }
}

pub struct Parser<'src> {
    tokens: Vec<PosToken<'src>>,
    pos: usize,
}

impl<'src> Parser<'src> {
    pub fn new(tokens: Vec<PosToken<'src>>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn current(&self) -> &PosToken<'src> {
        &self.tokens[self.pos.min(self.tokens.len().saturating_sub(1))]
    }

    fn current_kind(&self) -> &Token<'src> {
        &self.current().kind
    }

    fn current_start(&self) -> usize {
        self.current().start
    }

    fn at(&self, token: &Token<'src>) -> bool {
        std::mem::discriminant(self.current_kind()) == std::mem::discriminant(token)
    }

    fn at_eof(&self) -> bool {
        matches!(self.current_kind(), Token::Eof)
    }

    fn advance(&mut self) -> &PosToken<'src> {
        let token = &self.tokens[self.pos];
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        token
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        let token = self.current();
        ParseError {
            offset: token.start,
            line: token.line,
            col: token.col,
            message: message.into(),
        }
    }

    fn expect(&mut self, token: &Token<'src>) -> Result<&PosToken<'src>, ParseError> {
        if self.at(token) {
            Ok(self.advance())
        } else {
            Err(self.error(format!(
                "expected {:?}, got {:?}",
                token,
                self.current_kind()
            )))
        }
    }

    fn expect_ident(&mut self) -> Result<NameSpan<'src>, ParseError> {
        match self.current_kind() {
            Token::Ident(name) => {
                let name = *name;
                let token = self.advance();
                Ok((name, token.start, token.end))
            }
            _ => Err(self.error(format!(
                "expected identifier, got {:?}",
                self.current_kind()
            ))),
        }
    }

    fn expect_upper(&mut self) -> Result<NameSpan<'src>, ParseError> {
        match self.current_kind() {
            Token::UpperIdent(name) => {
                let name = *name;
                let token = self.advance();
                Ok((name, token.start, token.end))
            }
            _ => Err(self.error(format!(
                "expected constructor, got {:?}",
                self.current_kind()
            ))),
        }
    }

    fn expect_module_name_segment(&mut self) -> Result<&'src str, ParseError> {
        match self.current_kind() {
            Token::Ident(name) | Token::UpperIdent(name) => {
                let name = *name;
                self.advance();
                Ok(name)
            }
            _ => Err(self.error(format!(
                "expected module name, got {:?}",
                self.current_kind()
            ))),
        }
    }

    fn expect_binder(&mut self) -> Result<NameSpan<'src>, ParseError> {
        match self.current_kind() {
            Token::Ident(name) => {
                let name = *name;
                let token = self.advance();
                Ok((name, token.start, token.end))
            }
            Token::Underscore => {
                let token = self.advance();
                Ok(("_", token.start, token.end))
            }
            _ => Err(self.error(format!("expected binder, got {:?}", self.current_kind()))),
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

    fn parse_module(&mut self) -> Result<ParsedModule<'src>, ParseError> {
        if self.at(&Token::VOpen) {
            self.advance();
        }

        let imports = self.parse_imports()?;
        let defs = self.parse_definitions()?;

        if self.at(&Token::VClose) {
            self.advance();
        }

        Ok(ParsedModule { imports, defs })
    }

    fn parse_imports(&mut self) -> Result<Vec<ImportDecl>, ParseError> {
        let mut imports = Vec::new();
        while self.at(&Token::Import) {
            imports.push(self.parse_import()?);
            self.skip_separators();
        }
        Ok(imports)
    }

    fn parse_import(&mut self) -> Result<ImportDecl, ParseError> {
        self.expect(&Token::Import)?;

        let mut segments = vec![self.expect_module_name_segment()?.to_string()];
        while self.at(&Token::Dot) {
            self.advance();
            segments.push(self.expect_module_name_segment()?.to_string());
        }

        let module = ModuleName(segments);
        let alias = if self.at(&Token::As) {
            self.advance();
            self.expect_upper()?.0.to_string()
        } else {
            module.default_alias()
        };

        let spec = if self.at(&Token::LParen) {
            self.advance();
            let mut names = Vec::new();
            while !self.at(&Token::RParen) {
                names.push(self.expect_module_name_segment()?.to_string());
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

        Ok(ImportDecl {
            module,
            alias,
            spec,
        })
    }

    fn parse_definitions(&mut self) -> Result<SurfaceProgram<'src>, ParseError> {
        let mut defs = Vec::new();

        loop {
            self.skip_top_level_junk();
            if self.at_eof() || self.is_final_module_vclose() {
                break;
            }

            let start = self.current_start();
            let def = self.parse_definition()?;
            let end = self.current_start();
            defs.push(sp(def, start, end));
        }

        Ok(defs)
    }

    fn parse_definition(&mut self) -> Result<SurfaceDefinition<'src>, ParseError> {
        match self.current_kind() {
            Token::Data => self.parse_data_def(),
            Token::Class => self.parse_class_def(),
            Token::Instance => self.parse_instance_def(),
            _ => self.parse_func_or_sig(),
        }
    }

    fn parse_func_or_sig(&mut self) -> Result<SurfaceDefinition<'src>, ParseError> {
        let (name, start, end) = self.expect_ident()?;
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

    fn parse_data_def(&mut self) -> Result<SurfaceDefinition<'src>, ParseError> {
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

    fn parse_constructor(&mut self) -> Result<SufraceConstructor<'src>, ParseError> {
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

    fn parse_predicate(&mut self) -> Result<S<SurfacePredicate<'src>>, ParseError> {
        let start = self.current_start();

        let class_name = self.parse_name_path()?;
        if !is_ctor_like(class_name.0.last()) {
            return Err(self.error(format!(
                "expected class name, got {}",
                class_name.0.segments.join(".")
            )));
        }

        let mut args = Vec::new();
        while self.is_type_atom_start() {
            args.push(self.parse_type_atom()?);
        }

        if args.is_empty() {
            return Err(self.error("expected at least one type argument in predicate".to_string()));
        }

        let end = args.last().map(|a| a.1.end).unwrap_or(class_name.1.end);

        Ok(sp(SurfacePredicate { class_name, args }, start, end))
    }

    fn parse_predicate_list(&mut self) -> Result<Vec<S<SurfacePredicate<'src>>>, ParseError> {
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

    fn parse_class_def(&mut self) -> Result<SurfaceDefinition<'src>, ParseError> {
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
        loop {
            self.skip_separators();
            if self.at(&Token::VClose) || self.at_eof() {
                break;
            }
            let (mname, ms, me) = self.expect_ident()?;
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
        })
    }

    fn parse_instance_def(&mut self) -> Result<SurfaceDefinition<'src>, ParseError> {
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
            let (mname, ms, me) = self.expect_ident()?;
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

    fn parse_layout_block<T>(
        &mut self,
        mut parse_item: impl FnMut(&mut Self) -> Result<T, ParseError>,
    ) -> Result<Vec<T>, ParseError> {
        if self.at(&Token::VOpen) {
            self.advance();
        }

        let mut items = Vec::new();
        loop {
            self.skip_separators();
            if self.at(&Token::VClose) || self.at_eof() {
                break;
            }
            items.push(parse_item(self)?);
        }

        if self.at(&Token::VClose) {
            self.advance();
        }

        Ok(items)
    }

    fn parse_qualified_type(&mut self) -> Result<S<SurfaceTy<'src>>, ParseError> {
        let checkpoint = self.pos;

        if let Ok(preds) = self.parse_predicate_list() {
            if self.at(&Token::FatArrow) {
                self.advance();
                let body = self.parse_type_arrow()?;
                let start = preds.first().map(|p| p.1.start).unwrap_or(body.1.start);
                let end = body.1.end;
                return Ok(sp(SurfaceTy::Qualified(preds, Box::new(body)), start, end));
            }
        }

        self.pos = checkpoint;
        self.parse_type_arrow()
    }

    fn parse_type(&mut self) -> Result<S<SurfaceTy<'src>>, ParseError> {
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
            let end = body.1.end;
            return Ok(sp(SurfaceTy::ForAll(vars, Box::new(body)), start, end));
        }

        self.parse_qualified_type()
    }

    fn parse_type_arrow(&mut self) -> Result<S<SurfaceTy<'src>>, ParseError> {
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

    fn parse_type_app(&mut self) -> Result<S<SurfaceTy<'src>>, ParseError> {
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

    fn parse_name_path(&mut self) -> Result<S<NameRef<'src>>, ParseError> {
        let start = self.current_start();

        let first = match self.current_kind() {
            Token::Ident(name) | Token::UpperIdent(name) => {
                let name = *name;
                self.advance();
                name
            }
            _ => return Err(self.error(format!("expected name, got {:?}", self.current_kind()))),
        };

        let mut segments = vec![first];
        while self.at(&Token::Dot) {
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

    fn is_case_block_follower(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::RParen
                | Token::RBrack
                | Token::Comma
                | Token::Then
                | Token::Else
                | Token::In
                | Token::Semicolon
                | Token::VSemi
                | Token::VClose
                | Token::Eof
        )
    }

    fn parse_type_atom(&mut self) -> Result<S<SurfaceTy<'src>>, ParseError> {
        let start = self.current_start();
        match self.current_kind() {
            Token::Ident(_) | Token::UpperIdent(_) => {
                let name_ref = self.parse_name_path()?;
                let end = span_end(&name_ref);
                if is_ctor_like(name_ref.0.last()) {
                    Ok(sp(SurfaceTy::Con(name_ref), start, end))
                } else if name_ref.0.segments.len() == 1 {
                    Ok(sp(SurfaceTy::Var(name_ref.0.last()), start, end))
                } else {
                    Err(self.error(format!(
                        "qualified type variable is not allowed: {}",
                        name_ref.0.segments.join(".")
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
                Ok(list_ty(elem, start, end))
            }
            _ => Err(self.error(format!("expected type, got {:?}", self.current_kind()))),
        }
    }

    fn parse_pattern(&mut self) -> Result<S<SurfacePattern<'src>>, ParseError> {
        let head = self.parse_pattern_head()?;
        if self.at(&Token::Cons) {
            self.advance();
            let tail = self.parse_pattern()?;
            Ok(cons_pattern(head, tail))
        } else {
            Ok(head)
        }
    }

    fn parse_pattern_head(&mut self) -> Result<S<SurfacePattern<'src>>, ParseError> {
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

    fn parse_pattern_arg(&mut self) -> Result<S<SurfacePattern<'src>>, ParseError> {
        let start = self.current_start();
        match self.current_kind() {
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
                        name_ref.0.segments.join(".")
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
                    return Ok(list_nil_pattern(start, end));
                }
                let mut elems = vec![self.parse_pattern()?];
                while self.at(&Token::Comma) {
                    self.advance();
                    elems.push(self.parse_pattern()?);
                }
                let end = self.expect(&Token::RBrack)?.end;
                Ok(list_pattern(elems, start, end))
            }
            _ => Err(self.error(format!("expected pattern, got {:?}", self.current_kind()))),
        }
    }

    fn is_pattern_arg_start(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::Ident(_)
                | Token::UpperIdent(_)
                | Token::LParen
                | Token::LBrack
                | Token::Underscore
        )
    }

    fn parse_expr(&mut self) -> Result<S<SurfaceExpr<'src>>, ParseError> {
        let start = self.current_start();

        let expr = match self.current_kind() {
            Token::Fun => return self.parse_lambda(),
            Token::Let => return self.parse_let(),
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

    fn parse_op_chain(&mut self) -> Result<S<SurfaceExpr<'src>>, ParseError> {
        let mut items = vec![self.parse_unary()?];
        let mut ops = Vec::new();

        while builtin_fixity(self.current_kind()).is_some() {
            let token = self.advance();
            ops.push((token.kind.clone(), token.start, token.end));
            items.push(self.parse_unary()?);
        }

        if ops.is_empty() {
            Ok(items
                .pop()
                .expect("operator chain always has one expression"))
        } else {
            rearrange_ops(items, ops)
        }
    }

    fn parse_unary(&mut self) -> Result<S<SurfaceExpr<'src>>, ParseError> {
        if self.at(&Token::Minus) {
            let start = self.advance().start;
            let operand = self.parse_app()?;
            let end = span_end(&operand);
            Ok(sp(
                SurfaceExpr::UnaryOperator(SurfaceUnOp::Neg, Box::new(operand)),
                start,
                end,
            ))
        } else {
            self.parse_app()
        }
    }

    fn parse_app(&mut self) -> Result<S<SurfaceExpr<'src>>, ParseError> {
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
                | Token::Ident(_)
                | Token::UpperIdent(_)
                | Token::LParen
                | Token::LBrack
                | Token::Hole
                | Token::Underscore
        )
    }

    fn parse_atom(&mut self) -> Result<S<SurfaceExpr<'src>>, ParseError> {
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
            Token::Hole | Token::Underscore => {
                let token = self.advance();
                Ok(sp(SurfaceExpr::Hole, token.start, token.end))
            }
            Token::Ident(_) | Token::UpperIdent(_) => {
                let name_ref = self.parse_name_path()?;
                let end = span_end(&name_ref);
                let expr = if is_ctor_like(name_ref.0.last()) {
                    SurfaceExpr::Con(name_ref)
                } else {
                    SurfaceExpr::Var(name_ref)
                };
                Ok(sp(expr, start, end))
            }
            Token::LParen => {
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
                    return Ok(list_nil_expr(start, end));
                }
                let mut elems = vec![self.parse_expr()?];
                while self.at(&Token::Comma) {
                    self.advance();
                    elems.push(self.parse_expr()?);
                }
                let end = self.expect(&Token::RBrack)?.end;
                Ok(list_expr(elems, start, end))
            }
            _ => Err(self.error(format!(
                "expected expression, got {:?}",
                self.current_kind()
            ))),
        }
    }

    fn parse_lambda(&mut self) -> Result<S<SurfaceExpr<'src>>, ParseError> {
        let start = self.expect(&Token::Fun)?.start;
        let mut params: Vec<LambdaParam<'src>> = Vec::new();

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

    fn parse_let(&mut self) -> Result<S<SurfaceExpr<'src>>, ParseError> {
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

    fn parse_if(&mut self) -> Result<S<SurfaceExpr<'src>>, ParseError> {
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

    fn parse_case(&mut self) -> Result<S<SurfaceExpr<'src>>, ParseError> {
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

fn rearrange_ops<'src>(
    mut items: Vec<S<SurfaceExpr<'src>>>,
    ops: Vec<BinOpInfo<'src>>,
) -> Result<S<SurfaceExpr<'src>>, ParseError> {
    let mut expr_stack = vec![items.remove(0)];
    let mut op_stack: Vec<StackedOp<'src>> = Vec::new();

    for (op, rhs) in ops.into_iter().zip(items) {
        let fixity = builtin_fixity(&op.0).expect("operator fixity must exist");

        while let Some((_, _, _, top_fixity)) = op_stack.last() {
            let should_pop = top_fixity.prec > fixity.prec
                || (top_fixity.prec == fixity.prec && matches!(top_fixity.assoc, Assoc::Left));
            if !should_pop {
                break;
            }

            let (top_op, start, end, _) = op_stack.pop().expect("operator stack is non-empty");
            let right = expr_stack.pop().expect("rhs expression exists");
            let left = expr_stack.pop().expect("lhs expression exists");
            expr_stack.push(make_binop(top_op, left, right, start, end));
        }

        op_stack.push((op.0, op.1, op.2, fixity));
        expr_stack.push(rhs);
    }

    while let Some((op, start, end, _)) = op_stack.pop() {
        let right = expr_stack.pop().expect("rhs expression exists");
        let left = expr_stack.pop().expect("lhs expression exists");
        expr_stack.push(make_binop(op, left, right, start, end));
    }

    Ok(expr_stack.pop().expect("final expression exists"))
}

fn make_binop<'src>(
    op: Token<'src>,
    lhs: S<SurfaceExpr<'src>>,
    rhs: S<SurfaceExpr<'src>>,
    _op_start: usize,
    _op_end: usize,
) -> S<SurfaceExpr<'src>> {
    let (start, end) = merge_span(&lhs, &rhs);

    if matches!(op, Token::Cons) {
        return cons_expr(lhs, rhs, start, end);
    }

    let op = match op {
        Token::Plus => SurfaceBinOp::Plus,
        Token::Minus => SurfaceBinOp::Minus,
        Token::Star => SurfaceBinOp::Mult,
        Token::Slash => SurfaceBinOp::Div,
        Token::Percent => SurfaceBinOp::Mod,
        Token::EqEq => SurfaceBinOp::Equal,
        Token::Less => SurfaceBinOp::Less,
        Token::Greater => SurfaceBinOp::Greater,
        Token::LessEq => SurfaceBinOp::LessOrEqual,
        Token::GreaterEq => SurfaceBinOp::GreaterOrEqual,
        _ => unreachable!("unknown operator in rearrangement"),
    };

    sp(
        SurfaceExpr::BinaryOperator(op, Box::new(lhs), Box::new(rhs)),
        start,
        end,
    )
}

pub fn parse_source<'src>(source: &'src str) -> Result<ParsedModule<'src>, Vec<ParseError>> {
    let tokens = lexer::lex(source).map_err(|error| vec![ParseError::from(error)])?;
    Parser::new(tokens)
        .parse_module()
        .map_err(|error| vec![error])
}

pub fn render_errors(path: &Path, source: &str, errs: &[ParseError]) -> String {
    let file_id = path.display().to_string();
    let mut out = Vec::new();

    for error in errs {
        let range = error.offset..error.offset + 1;
        let _ = Report::build(ReportKind::Error, (file_id.clone(), range.clone()))
            .with_message("parse error")
            .with_label(
                Label::new((file_id.clone(), range))
                    .with_color(Color::Red)
                    .with_message(&error.message),
            )
            .finish()
            .write((file_id.clone(), Source::from(source)), &mut out);
        out.extend_from_slice(b"\n");
    }

    String::from_utf8_lossy(&out).into_owned()
}
