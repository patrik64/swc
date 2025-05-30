use std::fmt::Write;

use either::Either;
use swc_atoms::atom;
use swc_common::Spanned;

use super::*;
use crate::{parser::class_and_fn::IsSimpleParameterList, token::Keyword};

impl<I: Tokens> Parser<I> {
    /// `tsNextTokenCanFollowModifier`
    fn ts_next_token_can_follow_modifier(&mut self) -> PResult<bool> {
        debug_assert!(self.input.syntax().typescript());

        // Note: TypeScript's implementation is much more complicated because
        // more things are considered modifiers there.
        // This implementation only handles modifiers not handled by @babel/parser
        // itself. And "static". TODO: Would be nice to avoid lookahead. Want a
        // hasLineBreakUpNext() method...
        bump!(self);
        Ok(!self.input.had_line_break_before_cur()
            && is_one_of!(self, '[', '{', '*', "...", '#', IdentName, Str, Num, BigInt))
    }

    /// Parses a modifier matching one the given modifier names.
    ///
    /// `tsParseModifier`
    pub(super) fn parse_ts_modifier(
        &mut self,
        allowed_modifiers: &[&'static str],
        stop_on_start_of_class_static_blocks: bool,
    ) -> PResult<Option<&'static str>> {
        if !self.input.syntax().typescript() {
            return Ok(None);
        }

        let pos = {
            let modifier = match *cur!(self, true) {
                Token::Word(ref w @ Word::Ident(..))
                | Token::Word(ref w @ Word::Keyword(Keyword::In | Keyword::Const)) => w.cow(),

                _ => return Ok(None),
            };

            allowed_modifiers.iter().position(|s| **s == **modifier)
        };

        if let Some(pos) = pos {
            if stop_on_start_of_class_static_blocks && is!(self, "static") && peeked_is!(self, '{')
            {
                return Ok(None);
            }
            if self.try_parse_ts_bool(|p| p.ts_next_token_can_follow_modifier().map(Some))? {
                return Ok(Some(allowed_modifiers[pos]));
            }
        }

        Ok(None)
    }

    /// `tsIsListTerminator`
    fn is_ts_list_terminator(&mut self, kind: ParsingContext) -> PResult<bool> {
        debug_assert!(self.input.syntax().typescript());

        Ok(match kind {
            ParsingContext::EnumMembers | ParsingContext::TypeMembers => is!(self, '}'),
            ParsingContext::HeritageClauseElement { .. } => {
                is!(self, '{') || is!(self, "implements") || is!(self, "extends")
            }
            ParsingContext::TupleElementTypes => is!(self, ']'),
            ParsingContext::TypeParametersOrArguments => is!(self, '>'),
        })
    }

    /// `tsParseList`
    fn parse_ts_list<T, F>(&mut self, kind: ParsingContext, mut parse_element: F) -> PResult<Vec<T>>
    where
        F: FnMut(&mut Self) -> PResult<T>,
    {
        debug_assert!(self.input.syntax().typescript());

        let mut buf = Vec::with_capacity(8);
        while !self.is_ts_list_terminator(kind)? {
            // Skipping "parseListElement" from the TS source since that's just for error
            // handling.
            buf.push(parse_element(self)?);
        }
        Ok(buf)
    }

    /// `tsParseDelimitedList`
    fn parse_ts_delimited_list<T, F>(
        &mut self,
        kind: ParsingContext,
        mut parse_element: F,
    ) -> PResult<Vec<T>>
    where
        F: FnMut(&mut Self) -> PResult<T>,
    {
        self.parse_ts_delimited_list_inner(kind, |p| {
            let start = p.input.cur_pos();

            Ok((start, parse_element(p)?))
        })
    }

    /// `tsParseDelimitedList`
    fn parse_ts_delimited_list_inner<T, F>(
        &mut self,
        kind: ParsingContext,
        mut parse_element: F,
    ) -> PResult<Vec<T>>
    where
        F: FnMut(&mut Self) -> PResult<(BytePos, T)>,
    {
        debug_assert!(self.input.syntax().typescript());

        let mut buf = Vec::new();

        loop {
            trace_cur!(self, parse_ts_delimited_list_inner__element);

            if self.is_ts_list_terminator(kind)? {
                break;
            }
            let (_, element) = parse_element(self)?;
            buf.push(element);

            if eat!(self, ',') {
                continue;
            }

            if self.is_ts_list_terminator(kind)? {
                break;
            }

            if kind == ParsingContext::EnumMembers {
                const TOKEN: &Token = &Token::Comma;
                let cur = match cur!(self, false).ok() {
                    Some(tok) => format!("{:?}", tok),
                    None => "EOF".to_string(),
                };
                self.emit_err(self.input.cur_span(), SyntaxError::Expected(TOKEN, cur));
                continue;
            }
            // This will fail with an error about a missing comma
            expect!(self, ',');
        }

        Ok(buf)
    }

    fn parse_ts_bracketed_list<T, F>(
        &mut self,
        kind: ParsingContext,
        parse_element: F,
        bracket: bool,
        skip_first_token: bool,
    ) -> PResult<Vec<T>>
    where
        F: FnMut(&mut Self) -> PResult<T>,
    {
        debug_assert!(self.input.syntax().typescript());

        if !skip_first_token {
            if bracket {
                expect!(self, '[');
            } else {
                expect!(self, '<');
            }
        }

        let result = self.parse_ts_delimited_list(kind, parse_element)?;

        if bracket {
            expect!(self, ']');
        } else {
            expect!(self, '>');
        }

        Ok(result)
    }

    /// `tsParseEntityName`
    fn parse_ts_entity_name(&mut self, allow_reserved_words: bool) -> PResult<TsEntityName> {
        debug_assert!(self.input.syntax().typescript());
        trace_cur!(self, parse_ts_entity_name);
        let start = cur_pos!(self);

        let init = self.parse_ident_name()?;
        if &*init.sym == "void" {
            let dot_start = cur_pos!(self);
            let dot_span = span!(self, dot_start);
            self.emit_err(dot_span, SyntaxError::TS1005)
        }
        let mut entity = TsEntityName::Ident(init.into());
        while eat!(self, '.') {
            let dot_start = cur_pos!(self);
            if !is!(self, '#') && !is!(self, IdentName) {
                self.emit_err(Span::new(dot_start, dot_start), SyntaxError::TS1003);
                return Ok(entity);
            }

            let left = entity;
            let right = if allow_reserved_words {
                self.parse_ident_name()?
            } else {
                self.parse_ident(false, false)?.into()
            };
            let span = span!(self, start);
            entity = TsEntityName::TsQualifiedName(Box::new(TsQualifiedName { span, left, right }));
        }

        Ok(entity)
    }

    /// `tsParseTypeReference`
    fn parse_ts_type_ref(&mut self) -> PResult<TsTypeRef> {
        trace_cur!(self, parse_ts_type_ref);
        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);

        let has_modifier = self.eat_any_ts_modifier()?;

        let type_name = self.parse_ts_entity_name(/* allow_reserved_words */ true)?;
        trace_cur!(self, parse_ts_type_ref__type_args);
        let type_params = if !self.input.had_line_break_before_cur() && is!(self, '<') {
            Some(
                self.with_ctx(self.ctx() & !Context::ShouldNotLexLtOrGtAsType)
                    .parse_ts_type_args()?,
            )
        } else {
            None
        };

        if has_modifier {
            self.emit_err(span!(self, start), SyntaxError::TS2369);
        }

        Ok(TsTypeRef {
            span: span!(self, start),
            type_name,
            type_params,
        })
    }

    /// `tsParseThisTypePredicate`
    fn parse_ts_this_type_predicate(
        &mut self,
        start: BytePos,
        has_asserts_keyword: bool,
        lhs: TsThisType,
    ) -> PResult<TsTypePredicate> {
        debug_assert!(self.input.syntax().typescript());

        let param_name = TsThisTypeOrIdent::TsThisType(lhs);
        let type_ann = if eat!(self, "is") {
            let cur_pos = cur_pos!(self);
            Some(self.parse_ts_type_ann(
                // eat_colon
                false, cur_pos,
            )?)
        } else {
            None
        };

        Ok(TsTypePredicate {
            span: span!(self, start),
            asserts: has_asserts_keyword,
            param_name,
            type_ann,
        })
    }

    /// `tsParseThisTypeNode`
    fn parse_ts_this_type_node(&mut self) -> PResult<TsThisType> {
        debug_assert!(self.input.syntax().typescript());

        expect!(self, "this");

        Ok(TsThisType {
            span: self.input.prev_span(),
        })
    }

    /// `tsParseImportType`
    fn parse_ts_import_type(&mut self) -> PResult<TsImportType> {
        let start = cur_pos!(self);
        assert_and_bump!(self, "import");

        expect!(self, '(');

        let _ = cur!(self, false);

        let arg_span = self.input.cur_span();

        let arg = match cur!(self, true) {
            Token::Str { .. } => match bump!(self) {
                Token::Str { value, raw } => Str {
                    span: arg_span,
                    value,
                    raw: Some(raw),
                },
                _ => unreachable!(),
            },
            _ => {
                bump!(self);
                self.emit_err(arg_span, SyntaxError::TS1141);
                Str {
                    span: arg_span,
                    value: "".into(),
                    raw: Some("\"\"".into()),
                }
            }
        };

        // the "assert" keyword is deprecated and this syntax is niche, so
        // don't support it
        let attributes =
            if eat!(self, ',') && self.input.syntax().import_attributes() && is!(self, '{') {
                Some(self.parse_ts_call_options()?)
            } else {
                None
            };

        expect!(self, ')');

        let qualifier = if eat!(self, '.') {
            self.parse_ts_entity_name(false).map(Some)?
        } else {
            None
        };

        let type_args = if is!(self, '<') {
            self.with_ctx(self.ctx() & !Context::ShouldNotLexLtOrGtAsType)
                .parse_ts_type_args()
                .map(Some)?
        } else {
            None
        };

        Ok(TsImportType {
            span: span!(self, start),
            arg,
            qualifier,
            type_args,
            attributes,
        })
    }

    fn parse_ts_call_options(&mut self) -> PResult<TsImportCallOptions> {
        debug_assert!(self.input.syntax().typescript());
        let start = cur_pos!(self);
        assert_and_bump!(self, '{');

        expect!(self, "with");
        expect!(self, ':');

        let value = match self.parse_object::<Expr>()? {
            Expr::Object(v) => v,
            _ => unreachable!(),
        };
        eat!(self, ',');
        expect!(self, '}');
        Ok(TsImportCallOptions {
            span: span!(self, start),
            with: Box::new(value),
        })
    }

    /// `tsParseTypeQuery`
    fn parse_ts_type_query(&mut self) -> PResult<TsTypeQuery> {
        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);
        expect!(self, "typeof");
        let expr_name = if is!(self, "import") {
            self.parse_ts_import_type().map(From::from)?
        } else {
            self.parse_ts_entity_name(
                // allow_reserved_word
                true,
            )
            .map(From::from)?
        };

        let type_args = if !self.input.had_line_break_before_cur() && is!(self, '<') {
            Some(
                self.with_ctx(self.ctx() & !Context::ShouldNotLexLtOrGtAsType)
                    .parse_ts_type_args()?,
            )
        } else {
            None
        };

        Ok(TsTypeQuery {
            span: span!(self, start),
            expr_name,
            type_args,
        })
    }

    /// `tsParseTypeParameter`
    fn parse_ts_type_param(
        &mut self,
        permit_in_out: bool,
        permit_const: bool,
    ) -> PResult<TsTypeParam> {
        debug_assert!(self.input.syntax().typescript());

        let mut is_in = false;
        let mut is_out = false;
        let mut is_const = false;

        let start = cur_pos!(self);

        while let Some(modifer) = self.parse_ts_modifier(
            &[
                "public",
                "private",
                "protected",
                "readonly",
                "abstract",
                "const",
                "override",
                "in",
                "out",
            ],
            false,
        )? {
            match modifer {
                "const" => {
                    is_const = true;
                    if !permit_const {
                        self.emit_err(self.input.prev_span(), SyntaxError::TS1277("const".into()));
                    }
                }
                "in" => {
                    if !permit_in_out {
                        self.emit_err(self.input.prev_span(), SyntaxError::TS1274("in".into()));
                    } else if is_in {
                        self.emit_err(self.input.prev_span(), SyntaxError::TS1030("in".into()));
                    } else if is_out {
                        self.emit_err(
                            self.input.prev_span(),
                            SyntaxError::TS1029("in".into(), "out".into()),
                        );
                    }
                    is_in = true;
                }
                "out" => {
                    if !permit_in_out {
                        self.emit_err(self.input.prev_span(), SyntaxError::TS1274("out".into()));
                    } else if is_out {
                        self.emit_err(self.input.prev_span(), SyntaxError::TS1030("out".into()));
                    }
                    is_out = true;
                }
                other => self.emit_err(self.input.prev_span(), SyntaxError::TS1273(other.into())),
            };
        }

        let name = self.in_type().parse_ident_name()?.into();
        let constraint = self.eat_then_parse_ts_type(&tok!("extends"))?;
        let default = self.eat_then_parse_ts_type(&tok!('='))?;

        Ok(TsTypeParam {
            span: span!(self, start),
            name,
            is_in,
            is_out,
            is_const,
            constraint,
            default,
        })
    }

    /// `tsParseTypeParameter`
    pub(super) fn parse_ts_type_params(
        &mut self,
        permit_in_out: bool,
        permit_const: bool,
    ) -> PResult<Box<TsTypeParamDecl>> {
        self.in_type().parse_with(|p| {
            p.ts_in_no_context(|p| {
                let start = cur_pos!(p);

                if !is!(p, '<') && !is!(p, JSXTagStart) {
                    unexpected!(p, "< (jsx tag start)")
                }
                bump!(p); // '<'

                let params = p.parse_ts_bracketed_list(
                    ParsingContext::TypeParametersOrArguments,
                    |p| p.parse_ts_type_param(permit_in_out, permit_const), // bracket
                    false,
                    // skip_first_token
                    true,
                )?;

                Ok(Box::new(TsTypeParamDecl {
                    span: span!(p, start),
                    params,
                }))
            })
        })
    }

    /// `tsParseTypeOrTypePredicateAnnotation`
    pub(super) fn parse_ts_type_or_type_predicate_ann(
        &mut self,
        return_token: &'static Token,
    ) -> PResult<Box<TsTypeAnn>> {
        debug_assert!(self.input.syntax().typescript());

        self.in_type().parse_with(|p| {
            let return_token_start = cur_pos!(p);
            if !p.input.eat(return_token) {
                let cur = format!("{:?}", cur!(p, false).ok());
                let span = p.input.cur_span();
                syntax_error!(p, span, SyntaxError::Expected(return_token, cur))
            }

            let type_pred_start = cur_pos!(p);
            let has_type_pred_asserts = is!(p, "asserts") && peeked_is!(p, IdentRef);
            if has_type_pred_asserts {
                assert_and_bump!(p, "asserts");
                cur!(p, false)?;
            }

            let has_type_pred_is = is!(p, IdentRef)
                && peeked_is!(p, "is")
                && !p.input.has_linebreak_between_cur_and_peeked();
            let is_type_predicate = has_type_pred_asserts || has_type_pred_is;
            if !is_type_predicate {
                return p.parse_ts_type_ann(
                    // eat_colon
                    false,
                    return_token_start,
                );
            }

            let type_pred_var = p.parse_ident_name()?;
            let type_ann = if has_type_pred_is {
                assert_and_bump!(p, "is");
                let pos = cur_pos!(p);
                Some(p.parse_ts_type_ann(
                    // eat_colon
                    false, pos,
                )?)
            } else {
                None
            };

            let node = Box::new(TsType::TsTypePredicate(TsTypePredicate {
                span: span!(p, type_pred_start),
                asserts: has_type_pred_asserts,
                param_name: TsThisTypeOrIdent::Ident(type_pred_var.into()),
                type_ann,
            }));

            Ok(Box::new(TsTypeAnn {
                span: span!(p, return_token_start),
                type_ann: node,
            }))
        })
    }

    /// `tsTryParse`
    fn try_parse_ts_bool<F>(&mut self, op: F) -> PResult<bool>
    where
        F: FnOnce(&mut Self) -> PResult<Option<bool>>,
    {
        if !self.input.syntax().typescript() {
            return Ok(false);
        }
        let prev_ignore_error = self.input.get_ctx().contains(Context::IgnoreError);
        let mut cloned = self.clone();
        cloned.set_ctx(self.ctx() | Context::IgnoreError);
        let res = op(&mut cloned);
        match res {
            Ok(Some(res)) if res => {
                *self = cloned;
                let mut ctx = self.ctx();
                ctx.set(Context::IgnoreError, prev_ignore_error);
                self.input.set_ctx(ctx);
                Ok(res)
            }
            Err(..) => Ok(false),
            _ => Ok(false),
        }
    }

    #[cfg_attr(feature = "tracing-spans", tracing::instrument(skip_all))]
    pub(super) fn try_parse_ts_type_args(&mut self) -> Option<Box<TsTypeParamInstantiation>> {
        trace_cur!(self, try_parse_ts_type_args);
        debug_assert!(self.input.syntax().typescript());

        self.try_parse_ts(|p| {
            let type_args = p.parse_ts_type_args()?;

            if is_one_of!(
                p, '<', // invalid syntax
                '>', '=', ">>", ">=", '+', '-', // becomes relational expression
                /* these should be type arguments in function call or template,
                 * not instantiation expression */
                '(', '`'
            ) {
                Ok(None)
            } else if p.input.had_line_break_before_cur()
                || matches!(cur!(p, false), Ok(Token::BinOp(..)))
                || !p.is_start_of_expr()?
            {
                Ok(Some(type_args))
            } else {
                Ok(None)
            }
        })
    }

    /// `tsTryParse`
    pub(super) fn try_parse_ts<T, F>(&mut self, op: F) -> Option<T>
    where
        F: FnOnce(&mut Self) -> PResult<Option<T>>,
    {
        if !self.input.syntax().typescript() {
            return None;
        }
        let _tracing = debug_tracing!(self, "try_parse_ts");

        trace_cur!(self, try_parse_ts);

        let prev_ignore_error = self.input.get_ctx().contains(Context::IgnoreError);
        let mut cloned = self.clone();
        cloned.set_ctx(self.ctx() | Context::IgnoreError);
        let res = op(&mut cloned);
        match res {
            Ok(Some(res)) => {
                *self = cloned;
                trace_cur!(self, try_parse_ts__success_value);
                let mut ctx = self.ctx();
                ctx.set(Context::IgnoreError, prev_ignore_error);
                self.input.set_ctx(ctx);

                Some(res)
            }
            Ok(None) => {
                trace_cur!(self, try_parse_ts__success_no_value);

                None
            }
            Err(..) => {
                trace_cur!(self, try_parse_ts__fail);

                None
            }
        }
    }

    #[cfg_attr(feature = "tracing-spans", tracing::instrument(skip_all))]
    pub(super) fn parse_ts_type_ann(
        &mut self,
        eat_colon: bool,
        start: BytePos,
    ) -> PResult<Box<TsTypeAnn>> {
        trace_cur!(self, parse_ts_type_ann);

        debug_assert!(self.input.syntax().typescript());

        self.in_type().parse_with(|p| {
            if eat_colon {
                assert_and_bump!(p, ':');
            }

            trace_cur!(p, parse_ts_type_ann__after_colon);

            let type_ann = p.parse_ts_type()?;

            Ok(Box::new(TsTypeAnn {
                span: span!(p, start),
                type_ann,
            }))
        })
    }

    /// `tsEatThenParseType`
    fn eat_then_parse_ts_type(
        &mut self,
        token_to_eat: &'static Token,
    ) -> PResult<Option<Box<TsType>>> {
        if !cfg!(feature = "typescript") {
            return Ok(Default::default());
        }

        self.in_type().parse_with(|p| {
            if !p.input.eat(token_to_eat) {
                return Ok(None);
            }

            p.parse_ts_type().map(Some)
        })
    }

    /// `tsExpectThenParseType`
    fn expect_then_parse_ts_type(
        &mut self,
        token: &'static Token,
        token_str: &'static str,
    ) -> PResult<Box<TsType>> {
        debug_assert!(self.input.syntax().typescript());

        self.in_type().parse_with(|p| {
            if !p.input.eat(token) {
                let got = format!("{:?}", cur!(p, false).ok());
                syntax_error!(
                    p,
                    p.input.cur_span(),
                    SyntaxError::Unexpected {
                        got,
                        expected: token_str
                    }
                );
            }

            p.parse_ts_type()
        })
    }

    /// `tsNextThenParseType`
    pub(super) fn next_then_parse_ts_type(&mut self) -> PResult<Box<TsType>> {
        debug_assert!(self.input.syntax().typescript());

        let result = self.in_type().parse_with(|p| {
            bump!(p);

            p.parse_ts_type()
        });

        if !self.ctx().contains(Context::InType) && is_one_of!(self, '>', '<') {
            self.input.merge_lt_gt();
        }

        result
    }

    /// `tsParseEnumMember`
    fn parse_ts_enum_member(&mut self) -> PResult<TsEnumMember> {
        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);
        // Computed property names are grammar errors in an enum, so accept just string
        // literal or identifier.
        let id = match *cur!(self, true) {
            Token::Str { .. } => self.parse_lit().map(|lit| match lit {
                Lit::Str(s) => TsEnumMemberId::Str(s),
                _ => unreachable!(),
            })?,
            Token::Num { value, ref raw } => {
                let mut new_raw = String::new();

                new_raw.push('"');
                new_raw.push_str(raw);
                new_raw.push('"');

                bump!(self);

                let span = span!(self, start);

                // Recover from error
                self.emit_err(span, SyntaxError::TS2452);

                TsEnumMemberId::Str(Str {
                    span,
                    value: value.to_string().into(),
                    raw: Some(new_raw.into()),
                })
            }
            Token::LBracket => {
                assert_and_bump!(self, '[');
                let _ = self.parse_expr()?;

                self.emit_err(span!(self, start), SyntaxError::TS1164);

                expect!(self, ']');

                TsEnumMemberId::Ident(Ident::new_no_ctxt(atom!(""), span!(self, start)))
            }
            _ => self
                .parse_ident_name()
                .map(Ident::from)
                .map(TsEnumMemberId::from)?,
        };

        let init = if eat!(self, '=') {
            Some(self.parse_assignment_expr()?)
        } else if is!(self, ',') || is!(self, '}') {
            None
        } else {
            let start = cur_pos!(self);
            bump!(self);
            store!(self, ',');
            self.emit_err(Span::new(start, start), SyntaxError::TS1005);
            None
        };

        Ok(TsEnumMember {
            span: span!(self, start),
            id,
            init,
        })
    }

    /// `tsParseEnumDeclaration`
    pub(super) fn parse_ts_enum_decl(
        &mut self,
        start: BytePos,
        is_const: bool,
    ) -> PResult<Box<TsEnumDecl>> {
        debug_assert!(self.input.syntax().typescript());

        let id = self.parse_ident_name()?;
        expect!(self, '{');
        let members = self
            .parse_ts_delimited_list(ParsingContext::EnumMembers, |p| p.parse_ts_enum_member())?;
        expect!(self, '}');

        Ok(Box::new(TsEnumDecl {
            span: span!(self, start),
            declare: false,
            is_const,
            id: id.into(),
            members,
        }))
    }

    /// `tsParseModuleBlock`
    fn parse_ts_module_block(&mut self) -> PResult<TsModuleBlock> {
        trace_cur!(self, parse_ts_module_block);

        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);
        expect!(self, '{');
        // Inside of a module block is considered "top-level", meaning it can have
        // imports and exports.
        let body = self
            .with_ctx(self.ctx() | Context::TopLevel)
            .parse_with(|p| {
                p.parse_block_body(/* directives */ false, /* end */ Some(&tok!('}')))
            })?;

        Ok(TsModuleBlock {
            span: span!(self, start),
            body,
        })
    }

    /// `tsParseModuleOrNamespaceDeclaration`
    fn parse_ts_module_or_ns_decl(
        &mut self,
        start: BytePos,
        namespace: bool,
    ) -> PResult<Box<TsModuleDecl>> {
        debug_assert!(self.input.syntax().typescript());

        let id = self.parse_ident_name()?;
        let body: TsNamespaceBody = if eat!(self, '.') {
            let inner_start = cur_pos!(self);
            let inner = self.parse_ts_module_or_ns_decl(inner_start, namespace)?;
            let inner = TsNamespaceDecl {
                span: inner.span,
                id: match inner.id {
                    TsModuleName::Ident(i) => i,
                    _ => unreachable!(),
                },
                body: Box::new(inner.body.unwrap()),
                declare: inner.declare,
                global: inner.global,
            };
            inner.into()
        } else {
            self.parse_ts_module_block().map(From::from)?
        };

        Ok(Box::new(TsModuleDecl {
            span: span!(self, start),
            declare: false,
            id: TsModuleName::Ident(id.into()),
            body: Some(body),
            global: false,
            namespace,
        }))
    }

    /// `tsParseAmbientExternalModuleDeclaration`
    fn parse_ts_ambient_external_module_decl(
        &mut self,
        start: BytePos,
    ) -> PResult<Box<TsModuleDecl>> {
        debug_assert!(self.input.syntax().typescript());

        let (global, id) = if is!(self, "global") {
            let id = self.parse_ident_name()?;
            (true, TsModuleName::Ident(id.into()))
        } else if matches!(*cur!(self, true), Token::Str { .. }) {
            let id = self.parse_lit().map(|lit| match lit {
                Lit::Str(s) => TsModuleName::Str(s),
                _ => unreachable!(),
            })?;
            (false, id)
        } else {
            unexpected!(self, "global or a string literal");
        };

        let body = if is!(self, '{') {
            Some(self.parse_ts_module_block().map(TsNamespaceBody::from)?)
        } else {
            expect!(self, ';');
            None
        };

        Ok(Box::new(TsModuleDecl {
            span: span!(self, start),
            declare: false,
            id,
            global,
            body,
            namespace: false,
        }))
    }

    pub fn parse_type(&mut self) -> PResult<Box<TsType>> {
        debug_assert!(self.input.syntax().typescript());

        self.in_type().parse_ts_type()
    }

    /// Be sure to be in a type context before calling self.
    ///
    /// `tsParseType`
    pub(super) fn parse_ts_type(&mut self) -> PResult<Box<TsType>> {
        trace_cur!(self, parse_ts_type);

        debug_assert!(self.input.syntax().typescript());

        // Need to set `state.inType` so that we don't parse JSX in a type context.
        debug_assert!(self.ctx().contains(Context::InType));

        let start = cur_pos!(self);

        let ctx = self.ctx() & !Context::DisallowConditionalTypes;
        self.with_ctx(ctx).parse_with(|p| {
            let ty = p.parse_ts_non_conditional_type()?;
            if p.input.had_line_break_before_cur() || !eat!(p, "extends") {
                return Ok(ty);
            }

            let check_type = ty;
            let extends_type = {
                p.with_ctx(p.ctx() | Context::DisallowConditionalTypes)
                    .parse_ts_non_conditional_type()?
            };

            expect!(p, '?');

            let true_type = p.parse_ts_type()?;

            expect!(p, ':');

            let false_type = p.parse_ts_type()?;

            Ok(Box::new(TsType::TsConditionalType(TsConditionalType {
                span: span!(p, start),
                check_type,
                extends_type,
                true_type,
                false_type,
            })))
        })
    }

    /// `tsParseNonConditionalType`
    fn parse_ts_non_conditional_type(&mut self) -> PResult<Box<TsType>> {
        trace_cur!(self, parse_ts_non_conditional_type);

        debug_assert!(self.input.syntax().typescript());

        if self.is_ts_start_of_fn_type()? {
            return self
                .parse_ts_fn_or_constructor_type(true)
                .map(TsType::from)
                .map(Box::new);
        }
        if (is!(self, "abstract") && peeked_is!(self, "new")) || is!(self, "new") {
            // As in `new () => Date`
            return self
                .parse_ts_fn_or_constructor_type(false)
                .map(TsType::from)
                .map(Box::new);
        }

        self.parse_ts_union_type_or_higher()
    }

    fn is_ts_start_of_fn_type(&mut self) -> PResult<bool> {
        debug_assert!(self.input.syntax().typescript());

        if is!(self, '<') {
            return Ok(true);
        }

        Ok(is!(self, '(') && self.ts_look_ahead(|p| p.is_ts_unambiguously_start_of_fn_type())?)
    }

    /// `tsParseTypeAssertion`
    pub(super) fn parse_ts_type_assertion(&mut self, start: BytePos) -> PResult<TsTypeAssertion> {
        debug_assert!(self.input.syntax().typescript());

        if self.input.syntax().disallow_ambiguous_jsx_like() {
            self.emit_err(span!(self, start), SyntaxError::ReservedTypeAssertion);
        }

        // Not actually necessary to set state.inType because we never reach here if JSX
        // plugin is enabled, but need `tsInType` to satisfy the assertion in
        // `tsParseType`.
        let type_ann = self.in_type().parse_with(|p| p.parse_ts_type())?;
        expect!(self, '>');
        let expr = self.parse_unary_expr()?;
        Ok(TsTypeAssertion {
            span: span!(self, start),
            type_ann,
            expr,
        })
    }

    /// `tsParseHeritageClause`
    pub(super) fn parse_ts_heritage_clause(&mut self) -> PResult<Vec<TsExprWithTypeArgs>> {
        debug_assert!(self.input.syntax().typescript());

        self.parse_ts_delimited_list(ParsingContext::HeritageClauseElement, |p| {
            p.parse_ts_heritage_clause_element()
        })
    }

    fn parse_ts_heritage_clause_element(&mut self) -> PResult<TsExprWithTypeArgs> {
        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);
        // Note: TS uses parseLeftHandSideExpressionOrHigher,
        // then has grammar errors later if it's not an EntityName.

        let ident = self.parse_ident_name()?.into();
        let expr = self.parse_subscripts(Callee::Expr(ident), true, true)?;
        if !matches!(
            &*expr,
            Expr::Ident(..) | Expr::Member(..) | Expr::TsInstantiation(..)
        ) {
            self.emit_err(span!(self, start), SyntaxError::TS2499);
        }

        match *expr {
            Expr::TsInstantiation(v) => Ok(TsExprWithTypeArgs {
                span: v.span,
                expr: v.expr,
                type_args: Some(v.type_args),
            }),
            _ => {
                let type_args = if is!(self, '<') {
                    Some(self.parse_ts_type_args()?)
                } else {
                    None
                };

                Ok(TsExprWithTypeArgs {
                    span: span!(self, start),
                    expr,
                    type_args,
                })
            }
        }
    }

    /// `tsParseInterfaceDeclaration`
    pub(super) fn parse_ts_interface_decl(
        &mut self,
        start: BytePos,
    ) -> PResult<Box<TsInterfaceDecl>> {
        debug_assert!(self.input.syntax().typescript());

        let id = self.parse_ident_name()?;
        match &*id.sym {
            "string" | "null" | "number" | "object" | "any" | "unknown" | "boolean" | "bigint"
            | "symbol" | "void" | "never" | "intrinsic" => {
                self.emit_err(id.span, SyntaxError::TS2427);
            }
            _ => {}
        }

        let type_params = self.try_parse_ts_type_params(true, false)?;

        let extends = if eat!(self, "extends") {
            self.parse_ts_heritage_clause()?
        } else {
            Vec::new()
        };

        // Recover from
        //
        //     interface I extends A extends B {}
        if is!(self, "extends") {
            self.emit_err(self.input.cur_span(), SyntaxError::TS1172);

            while !eof!(self) && !is!(self, '{') {
                bump!(self);
            }
        }

        let body_start = cur_pos!(self);
        let body = self
            .in_type()
            .parse_with(|p| p.parse_ts_object_type_members())?;
        let body = TsInterfaceBody {
            span: span!(self, body_start),
            body,
        };
        Ok(Box::new(TsInterfaceDecl {
            span: span!(self, start),
            declare: false,
            id: id.into(),
            type_params,
            extends,
            body,
        }))
    }

    /// `tsParseTypeAliasDeclaration`
    pub(super) fn parse_ts_type_alias_decl(
        &mut self,
        start: BytePos,
    ) -> PResult<Box<TsTypeAliasDecl>> {
        debug_assert!(self.input.syntax().typescript());

        let id = self.parse_ident_name()?;
        let type_params = self.try_parse_ts_type_params(true, false)?;
        let type_ann = self.expect_then_parse_ts_type(&tok!('='), "=")?;
        expect!(self, ';');
        Ok(Box::new(TsTypeAliasDecl {
            declare: false,
            span: span!(self, start),
            id: id.into(),
            type_params,
            type_ann,
        }))
    }

    /// `tsParseImportEqualsDeclaration`
    pub(super) fn parse_ts_import_equals_decl(
        &mut self,
        start: BytePos,
        id: Ident,
        is_export: bool,
        is_type_only: bool,
    ) -> PResult<Box<TsImportEqualsDecl>> {
        debug_assert!(self.input.syntax().typescript());

        expect!(self, '=');

        let module_ref = self.parse_ts_module_ref()?;
        expect!(self, ';');
        Ok(Box::new(TsImportEqualsDecl {
            span: span!(self, start),
            id,
            is_export,
            is_type_only,
            module_ref,
        }))
    }

    /// `tsIsExternalModuleReference`
    fn is_ts_external_module_ref(&mut self) -> PResult<bool> {
        debug_assert!(self.input.syntax().typescript());

        Ok(is!(self, "require") && peeked_is!(self, '('))
    }

    /// `tsParseModuleReference`
    fn parse_ts_module_ref(&mut self) -> PResult<TsModuleRef> {
        debug_assert!(self.input.syntax().typescript());

        if self.is_ts_external_module_ref()? {
            self.parse_ts_external_module_ref().map(From::from)
        } else {
            self.parse_ts_entity_name(/* allow_reserved_words */ false)
                .map(From::from)
        }
    }

    /// `tsParseExternalModuleReference`
    fn parse_ts_external_module_ref(&mut self) -> PResult<TsExternalModuleRef> {
        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);
        expect!(self, "require");
        expect!(self, '(');
        match *cur!(self, true) {
            Token::Str { .. } => {}
            _ => unexpected!(self, "a string literal"),
        }
        let expr = match self.parse_lit()? {
            Lit::Str(s) => s,
            _ => unreachable!(),
        };
        expect!(self, ')');
        Ok(TsExternalModuleRef {
            span: span!(self, start),
            expr,
        })
    }

    pub(super) fn ts_look_ahead<T, F>(&mut self, op: F) -> PResult<T>
    where
        F: FnOnce(&mut Self) -> PResult<T>,
    {
        debug_assert!(self.input.syntax().typescript());

        let mut cloned = self.clone();
        cloned.set_ctx(self.ctx() | Context::IgnoreError);
        op(&mut cloned)
    }

    /// `tsIsUnambiguouslyStartOfFunctionType`
    fn is_ts_unambiguously_start_of_fn_type(&mut self) -> PResult<bool> {
        debug_assert!(self.input.syntax().typescript());

        assert_and_bump!(self, '(');
        if is_one_of!(self, ')', "...") {
            // ( )
            // ( ...
            return Ok(true);
        }
        if self.skip_ts_parameter_start()? {
            if is_one_of!(self, ':', ',', '?', '=') {
                // ( xxx :
                // ( xxx ,
                // ( xxx ?
                // ( xxx =
                return Ok(true);
            }
            if eat!(self, ')') && is!(self, "=>") {
                // ( xxx ) =>
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// `tsSkipParameterStart`
    fn skip_ts_parameter_start(&mut self) -> PResult<bool> {
        debug_assert!(self.input.syntax().typescript());

        let _ = self.eat_any_ts_modifier()?;

        if is_one_of!(self, IdentName, "this") {
            bump!(self);
            return Ok(true);
        }

        if (is!(self, '{') || is!(self, '[')) && self.parse_binding_pat_or_ident(false).is_ok() {
            return Ok(true);
        }

        Ok(false)
    }

    /// `tsParseTypeMemberSemicolon`
    fn parse_ts_type_member_semicolon(&mut self) -> PResult<()> {
        debug_assert!(self.input.syntax().typescript());

        if !eat!(self, ',') {
            expect!(self, ';');
        }

        Ok(())
    }

    /// `tsParseSignatureMember`
    fn parse_ts_signature_member(
        &mut self,
        kind: SignatureParsingMode,
    ) -> PResult<Either<TsCallSignatureDecl, TsConstructSignatureDecl>> {
        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);

        if kind == SignatureParsingMode::TSConstructSignatureDeclaration {
            expect!(self, "new");
        }

        // ----- inlined self.tsFillSignature(tt.colon, node);
        let type_params = self.try_parse_ts_type_params(false, true)?;
        expect!(self, '(');
        let params = self.parse_ts_binding_list_for_signature()?;
        let type_ann = if is!(self, ':') {
            Some(self.parse_ts_type_or_type_predicate_ann(&tok!(':'))?)
        } else {
            None
        };
        // -----

        self.parse_ts_type_member_semicolon()?;

        match kind {
            SignatureParsingMode::TSCallSignatureDeclaration => {
                Ok(Either::Left(TsCallSignatureDecl {
                    span: span!(self, start),
                    params,
                    type_ann,
                    type_params,
                }))
            }
            SignatureParsingMode::TSConstructSignatureDeclaration => {
                Ok(Either::Right(TsConstructSignatureDecl {
                    span: span!(self, start),
                    params,
                    type_ann,
                    type_params,
                }))
            }
        }
    }

    /// `tsIsUnambiguouslyIndexSignature`
    fn is_ts_unambiguously_index_signature(&mut self) -> PResult<bool> {
        debug_assert!(self.input.syntax().typescript());

        // Note: babel's comment is wrong
        assert_and_bump!(self, '['); // Skip '['

        // ',' is for error recovery
        Ok(eat!(self, IdentRef) && is_one_of!(self, ':', ','))
    }

    /// `tsTryParseIndexSignature`
    pub(super) fn try_parse_ts_index_signature(
        &mut self,
        index_signature_start: BytePos,
        readonly: bool,
        is_static: bool,
    ) -> PResult<Option<TsIndexSignature>> {
        if !cfg!(feature = "typescript") {
            return Ok(Default::default());
        }

        if !(is!(self, '[') && self.ts_look_ahead(|p| p.is_ts_unambiguously_index_signature())?) {
            return Ok(None);
        }

        expect!(self, '[');

        let ident_start = cur_pos!(self);
        let mut id = self.parse_ident_name().map(BindingIdent::from)?;
        let type_ann_start = cur_pos!(self);

        if eat!(self, ',') {
            self.emit_err(id.span, SyntaxError::TS1096);
        } else {
            expect!(self, ':');
        }

        let type_ann = self.parse_ts_type_ann(/* eat_colon */ false, type_ann_start)?;
        id.span = span!(self, ident_start);
        id.type_ann = Some(type_ann);

        expect!(self, ']');
        let params = vec![TsFnParam::Ident(id)];

        let ty = self.try_parse_ts_type_ann()?;
        let type_ann = ty;

        self.parse_ts_type_member_semicolon()?;
        Ok(Some(TsIndexSignature {
            span: span!(self, index_signature_start),
            readonly,
            is_static,
            params,
            type_ann,
        }))
    }

    /// `parsePropertyName` in babel.
    ///
    /// Returns `(computed, key)`.
    fn parse_ts_property_name(&mut self) -> PResult<(bool, Box<Expr>)> {
        let (computed, key) = if eat!(self, '[') {
            let key = self.parse_assignment_expr()?;
            expect!(self, ']');
            (true, key)
        } else {
            let ctx = self.ctx() | Context::InPropertyName;
            self.with_ctx(ctx).parse_with(|p| {
                // We check if it's valid for it to be a private name when we push it.
                let key = match *cur!(p, true) {
                    Token::Num { .. } | Token::Str { .. } => p.parse_new_expr(),
                    _ => p.parse_maybe_private_name().map(|e| match e {
                        Either::Left(e) => {
                            p.emit_err(e.span(), SyntaxError::PrivateNameInInterface);

                            e.into()
                        }
                        Either::Right(e) => e.into(),
                    }),
                };

                key.map(|key| (false, key))
            })?
        };

        Ok((computed, key))
    }

    /// `tsParsePropertyOrMethodSignature`
    fn parse_ts_property_or_method_signature(
        &mut self,
        start: BytePos,
        readonly: bool,
    ) -> PResult<Either<TsPropertySignature, TsMethodSignature>> {
        debug_assert!(self.input.syntax().typescript());

        let (computed, key) = self.parse_ts_property_name()?;

        let optional = eat!(self, '?');

        if is_one_of!(self, '(', '<') {
            if readonly {
                syntax_error!(self, SyntaxError::ReadOnlyMethod)
            }

            let type_params = self.try_parse_ts_type_params(false, true)?;
            expect!(self, '(');
            let params = self.parse_ts_binding_list_for_signature()?;
            let type_ann = if is!(self, ':') {
                self.parse_ts_type_or_type_predicate_ann(&tok!(':'))
                    .map(Some)?
            } else {
                None
            };
            // -----

            self.parse_ts_type_member_semicolon()?;
            Ok(Either::Right(TsMethodSignature {
                span: span!(self, start),
                computed,
                key,
                optional,
                type_params,
                params,
                type_ann,
            }))
        } else {
            let type_ann = self.try_parse_ts_type_ann()?;

            self.parse_ts_type_member_semicolon()?;
            Ok(Either::Left(TsPropertySignature {
                span: span!(self, start),
                computed,
                readonly,
                key,
                optional,
                type_ann,
            }))
        }
    }

    /// `tsParseTypeMember`
    fn parse_ts_type_member(&mut self) -> PResult<TsTypeElement> {
        debug_assert!(self.input.syntax().typescript());

        fn into_type_elem(
            e: Either<TsCallSignatureDecl, TsConstructSignatureDecl>,
        ) -> TsTypeElement {
            match e {
                Either::Left(e) => e.into(),
                Either::Right(e) => e.into(),
            }
        }
        if is_one_of!(self, '(', '<') {
            return self
                .parse_ts_signature_member(SignatureParsingMode::TSCallSignatureDeclaration)
                .map(into_type_elem);
        }
        if is!(self, "new") && self.ts_look_ahead(|p| p.is_ts_start_of_construct_signature())? {
            return self
                .parse_ts_signature_member(SignatureParsingMode::TSConstructSignatureDeclaration)
                .map(into_type_elem);
        }
        // Instead of fullStart, we create a node here.
        let start = cur_pos!(self);
        let readonly = self.parse_ts_modifier(&["readonly"], false)?.is_some();

        let idx = self.try_parse_ts_index_signature(start, readonly, false)?;
        if let Some(idx) = idx {
            return Ok(idx.into());
        }

        if let Some(v) = self.try_parse_ts(|p| {
            let start = p.input.cur_pos();

            if readonly {
                syntax_error!(p, SyntaxError::GetterSetterCannotBeReadonly)
            }

            let is_get = if eat!(p, "get") {
                true
            } else {
                expect!(p, "set");
                false
            };

            let (computed, key) = p.parse_ts_property_name()?;

            if is_get {
                expect!(p, '(');
                expect!(p, ')');
                let type_ann = p.try_parse_ts_type_ann()?;

                p.parse_ts_type_member_semicolon()?;

                Ok(Some(TsTypeElement::TsGetterSignature(TsGetterSignature {
                    span: span!(p, start),
                    key,
                    computed,
                    type_ann,
                })))
            } else {
                expect!(p, '(');
                let params = p.parse_ts_binding_list_for_signature()?;
                if params.is_empty() {
                    syntax_error!(p, SyntaxError::SetterParamRequired)
                }
                let param = params.into_iter().next().unwrap();

                p.parse_ts_type_member_semicolon()?;

                Ok(Some(TsTypeElement::TsSetterSignature(TsSetterSignature {
                    span: span!(p, start),
                    key,
                    computed,
                    param,
                })))
            }
        }) {
            return Ok(v);
        }

        self.parse_ts_property_or_method_signature(start, readonly)
            .map(|e| match e {
                Either::Left(e) => e.into(),
                Either::Right(e) => e.into(),
            })
    }

    /// `tsIsStartOfConstructSignature`
    fn is_ts_start_of_construct_signature(&mut self) -> PResult<bool> {
        debug_assert!(self.input.syntax().typescript());

        bump!(self);

        Ok(is!(self, '(') || is!(self, '<'))
    }

    /// `tsParseTypeLiteral`
    fn parse_ts_type_lit(&mut self) -> PResult<TsTypeLit> {
        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);
        let members = self.parse_ts_object_type_members()?;
        Ok(TsTypeLit {
            span: span!(self, start),
            members,
        })
    }

    /// `tsParseObjectTypeMembers`
    fn parse_ts_object_type_members(&mut self) -> PResult<Vec<TsTypeElement>> {
        debug_assert!(self.input.syntax().typescript());

        expect!(self, '{');
        let members =
            self.parse_ts_list(ParsingContext::TypeMembers, |p| p.parse_ts_type_member())?;
        expect!(self, '}');
        Ok(members)
    }

    /// `tsIsStartOfMappedType`
    fn is_ts_start_of_mapped_type(&mut self) -> PResult<bool> {
        debug_assert!(self.input.syntax().typescript());

        bump!(self);
        if eat!(self, '+') || eat!(self, '-') {
            return Ok(is!(self, "readonly"));
        }
        if is!(self, "readonly") {
            bump!(self);
        }
        if !is!(self, '[') {
            return Ok(false);
        }
        bump!(self);
        if !is!(self, IdentRef) {
            return Ok(false);
        }
        bump!(self);

        Ok(is!(self, "in"))
    }

    /// `tsParseMappedTypeParameter`
    fn parse_ts_mapped_type_param(&mut self) -> PResult<TsTypeParam> {
        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);
        let name = self.parse_ident_name()?;
        let constraint = Some(self.expect_then_parse_ts_type(&tok!("in"), "in")?);

        Ok(TsTypeParam {
            span: span!(self, start),
            name: name.into(),
            is_in: false,
            is_out: false,
            is_const: false,
            constraint,
            default: None,
        })
    }

    /// `tsParseMappedType`
    fn parse_ts_mapped_type(&mut self) -> PResult<TsMappedType> {
        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);
        expect!(self, '{');
        let mut readonly = None;
        if is_one_of!(self, '+', '-') {
            readonly = Some(if is!(self, '+') {
                TruePlusMinus::Plus
            } else {
                TruePlusMinus::Minus
            });
            bump!(self);
            expect!(self, "readonly")
        } else if eat!(self, "readonly") {
            readonly = Some(TruePlusMinus::True);
        }

        expect!(self, '[');
        let type_param = self.parse_ts_mapped_type_param()?;
        let name_type = if eat!(self, "as") {
            Some(self.parse_ts_type()?)
        } else {
            None
        };
        expect!(self, ']');

        let mut optional = None;
        if is_one_of!(self, '+', '-') {
            optional = Some(if is!(self, '+') {
                TruePlusMinus::Plus
            } else {
                TruePlusMinus::Minus
            });
            bump!(self); // +, -
            expect!(self, '?');
        } else if eat!(self, '?') {
            optional = Some(TruePlusMinus::True);
        }

        let type_ann = self.try_parse_ts_type()?;
        expect!(self, ';');
        expect!(self, '}');

        Ok(TsMappedType {
            span: span!(self, start),
            readonly,
            optional,
            type_param,
            name_type,
            type_ann,
        })
    }

    /// `tsParseTupleType`
    fn parse_ts_tuple_type(&mut self) -> PResult<TsTupleType> {
        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);
        let elems = self.parse_ts_bracketed_list(
            ParsingContext::TupleElementTypes,
            |p| p.parse_ts_tuple_element_type(),
            /* bracket */ true,
            /* skipFirstToken */ false,
        )?;

        // Validate the elementTypes to ensure:
        //   No mandatory elements may follow optional elements
        //   If there's a rest element, it must be at the end of the tuple

        let mut seen_optional_element = false;

        for elem in elems.iter() {
            match *elem.ty {
                TsType::TsRestType(..) => {}
                TsType::TsOptionalType(..) => {
                    seen_optional_element = true;
                }
                _ if seen_optional_element => {
                    syntax_error!(
                        self,
                        span!(self, start),
                        SyntaxError::TsRequiredAfterOptional
                    )
                }
                _ => {}
            }
        }

        Ok(TsTupleType {
            span: span!(self, start),
            elem_types: elems,
        })
    }

    fn try_parse_ts_tuple_element_name(&mut self) -> Option<Pat> {
        if !cfg!(feature = "typescript") {
            return Default::default();
        }

        self.try_parse_ts(|p| {
            let start = cur_pos!(p);

            let rest = if eat!(p, "...") {
                Some(p.input.prev_span())
            } else {
                None
            };

            let mut ident = p.parse_ident_name().map(Ident::from)?;
            if eat!(p, '?') {
                ident.optional = true;
                ident.span = ident.span.with_hi(p.input.prev_span().hi);
            }
            expect!(p, ':');

            Ok(Some(if let Some(dot3_token) = rest {
                RestPat {
                    span: span!(p, start),
                    dot3_token,
                    arg: ident.into(),
                    type_ann: None,
                }
                .into()
            } else {
                ident.into()
            }))
        })
    }

    /// `tsParseTupleElementType`
    fn parse_ts_tuple_element_type(&mut self) -> PResult<TsTupleElement> {
        debug_assert!(self.input.syntax().typescript());

        // parses `...TsType[]`
        let start = cur_pos!(self);

        let label = self.try_parse_ts_tuple_element_name();

        if eat!(self, "...") {
            let type_ann = self.parse_ts_type()?;
            return Ok(TsTupleElement {
                span: span!(self, start),
                label,
                ty: Box::new(TsType::TsRestType(TsRestType {
                    span: span!(self, start),
                    type_ann,
                })),
            });
        }

        let ty = self.parse_ts_type()?;
        // parses `TsType?`
        if eat!(self, '?') {
            let type_ann = ty;
            return Ok(TsTupleElement {
                span: span!(self, start),
                label,
                ty: Box::new(TsType::TsOptionalType(TsOptionalType {
                    span: span!(self, start),
                    type_ann,
                })),
            });
        }

        Ok(TsTupleElement {
            span: span!(self, start),
            label,
            ty,
        })
    }

    /// `tsParseParenthesizedType`
    fn parse_ts_parenthesized_type(&mut self) -> PResult<TsParenthesizedType> {
        debug_assert!(self.input.syntax().typescript());
        trace_cur!(self, parse_ts_parenthesized_type);

        let start = cur_pos!(self);
        expect!(self, '(');
        let type_ann = self.parse_ts_type()?;
        expect!(self, ')');
        Ok(TsParenthesizedType {
            span: span!(self, start),
            type_ann,
        })
    }

    /// `tsParseFunctionOrConstructorType`
    fn parse_ts_fn_or_constructor_type(
        &mut self,
        is_fn_type: bool,
    ) -> PResult<TsFnOrConstructorType> {
        trace_cur!(self, parse_ts_fn_or_constructor_type);

        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);
        let is_abstract = if !is_fn_type {
            eat!(self, "abstract")
        } else {
            false
        };
        if !is_fn_type {
            expect!(self, "new");
        }

        // ----- inlined `self.tsFillSignature(tt.arrow, node)`
        let type_params = self.try_parse_ts_type_params(false, true)?;
        expect!(self, '(');
        let params = self.parse_ts_binding_list_for_signature()?;
        let type_ann = self.parse_ts_type_or_type_predicate_ann(&tok!("=>"))?;
        // ----- end

        Ok(if is_fn_type {
            TsFnOrConstructorType::TsFnType(TsFnType {
                span: span!(self, start),
                type_params,
                params,
                type_ann,
            })
        } else {
            TsFnOrConstructorType::TsConstructorType(TsConstructorType {
                span: span!(self, start),
                type_params,
                params,
                type_ann,
                is_abstract,
            })
        })
    }

    /// `tsParseLiteralTypeNode`
    fn parse_ts_lit_type_node(&mut self) -> PResult<TsLitType> {
        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);

        let lit = if is!(self, '`') {
            let tpl = self.parse_ts_tpl_lit_type()?;

            TsLit::Tpl(tpl)
        } else {
            match self.parse_lit()? {
                Lit::BigInt(n) => TsLit::BigInt(n),
                Lit::Bool(n) => TsLit::Bool(n),
                Lit::Num(n) => TsLit::Number(n),
                Lit::Str(n) => TsLit::Str(n),
                _ => unreachable!(),
            }
        };

        Ok(TsLitType {
            span: span!(self, start),
            lit,
        })
    }

    /// `tsParseTemplateLiteralType`
    fn parse_ts_tpl_lit_type(&mut self) -> PResult<TsTplLitType> {
        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);

        assert_and_bump!(self, '`');

        let (types, quasis) = self.parse_ts_tpl_type_elements()?;

        expect!(self, '`');

        Ok(TsTplLitType {
            span: span!(self, start),
            types,
            quasis,
        })
    }

    fn parse_ts_tpl_type_elements(&mut self) -> PResult<(Vec<Box<TsType>>, Vec<TplElement>)> {
        if !cfg!(feature = "typescript") {
            return Ok(Default::default());
        }

        trace_cur!(self, parse_tpl_elements);

        let mut types = Vec::new();

        let cur_elem = self.parse_tpl_element(false)?;
        let mut is_tail = cur_elem.tail;
        let mut quasis = vec![cur_elem];

        while !is_tail {
            expect!(self, "${");
            types.push(self.parse_ts_type()?);
            expect!(self, '}');
            let elem = self.parse_tpl_element(false)?;
            is_tail = elem.tail;
            quasis.push(elem);
        }

        Ok((types, quasis))
    }

    /// `tsParseBindingListForSignature`
    ///
    /// Eats ')` at the end but does not eat `(` at start.
    fn parse_ts_binding_list_for_signature(&mut self) -> PResult<Vec<TsFnParam>> {
        if !cfg!(feature = "typescript") {
            return Ok(Default::default());
        }

        debug_assert!(self.input.syntax().typescript());

        let params = self.parse_formal_params()?;
        let mut list = Vec::with_capacity(4);

        for param in params {
            let item = match param.pat {
                Pat::Ident(pat) => TsFnParam::Ident(pat),
                Pat::Array(pat) => TsFnParam::Array(pat),
                Pat::Object(pat) => TsFnParam::Object(pat),
                Pat::Rest(pat) => TsFnParam::Rest(pat),
                _ => unexpected!(
                    self,
                    "an identifier, [ for an array pattern, { for an object patter or ... for a \
                     rest pattern"
                ),
            };
            list.push(item);
        }
        expect!(self, ')');
        Ok(list)
    }

    /// `tsTryParseTypeOrTypePredicateAnnotation`
    ///
    /// Used for parsing return types.
    fn try_parse_ts_type_or_type_predicate_ann(&mut self) -> PResult<Option<Box<TsTypeAnn>>> {
        if !cfg!(feature = "typescript") {
            return Ok(None);
        }

        if is!(self, ':') {
            self.parse_ts_type_or_type_predicate_ann(&tok!(':'))
                .map(Some)
        } else {
            Ok(None)
        }
    }

    /// `tsTryParseTypeAnnotation`
    #[cfg_attr(feature = "tracing-spans", tracing::instrument(skip_all))]
    pub(super) fn try_parse_ts_type_ann(&mut self) -> PResult<Option<Box<TsTypeAnn>>> {
        if !cfg!(feature = "typescript") {
            return Ok(None);
        }

        if is!(self, ':') {
            let pos = cur_pos!(self);
            return self.parse_ts_type_ann(/* eat_colon */ true, pos).map(Some);
        }

        Ok(None)
    }

    /// `tsTryParseType`
    fn try_parse_ts_type(&mut self) -> PResult<Option<Box<TsType>>> {
        if !cfg!(feature = "typescript") {
            return Ok(None);
        }

        self.eat_then_parse_ts_type(&tok!(':'))
    }

    /// `tsTryParseTypeParameters`
    pub(super) fn try_parse_ts_type_params(
        &mut self,
        permit_in_out: bool,
        permit_const: bool,
    ) -> PResult<Option<Box<TsTypeParamDecl>>> {
        if !cfg!(feature = "typescript") {
            return Ok(None);
        }

        if is!(self, '<') {
            return self
                .parse_ts_type_params(permit_in_out, permit_const)
                .map(Some);
        }
        Ok(None)
    }

    /// `tsParseNonArrayType`
    fn parse_ts_non_array_type(&mut self) -> PResult<Box<TsType>> {
        if !cfg!(feature = "typescript") {
            unreachable!()
        }
        trace_cur!(self, parse_ts_non_array_type);
        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);

        match *cur!(self, true) {
            Token::Word(Word::Ident(..))
            | tok!("void")
            | tok!("yield")
            | tok!("null")
            | tok!("await")
            | tok!("break") => {
                if is!(self, "asserts") && peeked_is!(self, "this") {
                    bump!(self);
                    let this_keyword = self.parse_ts_this_type_node()?;
                    return self
                        .parse_ts_this_type_predicate(start, true, this_keyword)
                        .map(TsType::from)
                        .map(Box::new);
                }

                let kind = if is!(self, "void") {
                    Some(TsKeywordTypeKind::TsVoidKeyword)
                } else if is!(self, "null") {
                    Some(TsKeywordTypeKind::TsNullKeyword)
                } else if is!(self, "any") {
                    Some(TsKeywordTypeKind::TsAnyKeyword)
                } else if is!(self, "boolean") {
                    Some(TsKeywordTypeKind::TsBooleanKeyword)
                } else if is!(self, "bigint") {
                    Some(TsKeywordTypeKind::TsBigIntKeyword)
                } else if is!(self, "never") {
                    Some(TsKeywordTypeKind::TsNeverKeyword)
                } else if is!(self, "number") {
                    Some(TsKeywordTypeKind::TsNumberKeyword)
                } else if is!(self, "object") {
                    Some(TsKeywordTypeKind::TsObjectKeyword)
                } else if is!(self, "string") {
                    Some(TsKeywordTypeKind::TsStringKeyword)
                } else if is!(self, "symbol") {
                    Some(TsKeywordTypeKind::TsSymbolKeyword)
                } else if is!(self, "unknown") {
                    Some(TsKeywordTypeKind::TsUnknownKeyword)
                } else if is!(self, "undefined") {
                    Some(TsKeywordTypeKind::TsUndefinedKeyword)
                } else if is!(self, "intrinsic") {
                    Some(TsKeywordTypeKind::TsIntrinsicKeyword)
                } else {
                    None
                };

                let peeked_is_dot = peeked_is!(self, '.');

                match kind {
                    Some(kind) if !peeked_is_dot => {
                        bump!(self);
                        return Ok(Box::new(TsType::TsKeywordType(TsKeywordType {
                            span: span!(self, start),
                            kind,
                        })));
                    }
                    _ => {
                        return self.parse_ts_type_ref().map(TsType::from).map(Box::new);
                    }
                }
            }
            Token::BigInt { .. }
            | Token::Str { .. }
            | Token::Num { .. }
            | tok!("true")
            | tok!("false")
            | tok!('`') => {
                return self
                    .parse_ts_lit_type_node()
                    .map(TsType::from)
                    .map(Box::new);
            }
            tok!('-') => {
                let start = cur_pos!(self);

                bump!(self);

                if !matches!(*cur!(self, true), Token::Num { .. } | Token::BigInt { .. }) {
                    unexpected!(self, "numeric literal or bigint literal")
                }

                let lit = self.parse_lit()?;
                let lit = match lit {
                    Lit::Num(Number { span, value, raw }) => {
                        let mut new_raw = String::from("-");

                        match raw {
                            Some(raw) => {
                                new_raw.push_str(&raw);
                            }
                            _ => {
                                write!(new_raw, "{}", value).unwrap();
                            }
                        };

                        TsLit::Number(Number {
                            span,
                            value: -value,
                            raw: Some(new_raw.into()),
                        })
                    }
                    Lit::BigInt(BigInt { span, value, raw }) => {
                        let mut new_raw = String::from("-");

                        match raw {
                            Some(raw) => {
                                new_raw.push_str(&raw);
                            }
                            _ => {
                                write!(new_raw, "{}", value).unwrap();
                            }
                        };

                        TsLit::BigInt(BigInt {
                            span,
                            value: Box::new(-*value),
                            raw: Some(new_raw.into()),
                        })
                    }
                    _ => unreachable!(),
                };

                return Ok(Box::new(TsType::TsLitType(TsLitType {
                    span: span!(self, start),
                    lit,
                })));
            }

            tok!("import") => {
                return self.parse_ts_import_type().map(TsType::from).map(Box::new);
            }

            tok!("this") => {
                let start = cur_pos!(self);
                let this_keyword = self.parse_ts_this_type_node()?;
                if !self.input.had_line_break_before_cur() && is!(self, "is") {
                    return self
                        .parse_ts_this_type_predicate(start, false, this_keyword)
                        .map(TsType::from)
                        .map(Box::new);
                } else {
                    return Ok(Box::new(TsType::TsThisType(this_keyword)));
                }
            }
            tok!("typeof") => {
                return self.parse_ts_type_query().map(TsType::from).map(Box::new);
            }

            tok!('{') => {
                return if self.ts_look_ahead(|p| p.is_ts_start_of_mapped_type())? {
                    self.parse_ts_mapped_type().map(TsType::from).map(Box::new)
                } else {
                    self.parse_ts_type_lit().map(TsType::from).map(Box::new)
                };
            }
            tok!('[') => {
                return self.parse_ts_tuple_type().map(TsType::from).map(Box::new);
            }
            tok!('(') => {
                return self
                    .parse_ts_parenthesized_type()
                    .map(TsType::from)
                    .map(Box::new);
            }
            _ => {}
        }
        //   switch (self.state.type) {
        //   }

        unexpected!(
            self,
            "an identifier, void, yield, null, await, break, a string literal, a numeric literal, \
             true, false, `, -, import, this, typeof, {, [, ("
        )
    }

    /// `tsParseArrayTypeOrHigher`
    fn parse_ts_array_type_or_higher(&mut self, readonly: bool) -> PResult<Box<TsType>> {
        trace_cur!(self, parse_ts_array_type_or_higher);
        debug_assert!(self.input.syntax().typescript());

        let mut ty = self.parse_ts_non_array_type()?;

        while !self.input.had_line_break_before_cur() && eat!(self, '[') {
            if eat!(self, ']') {
                ty = Box::new(TsType::TsArrayType(TsArrayType {
                    span: span!(self, ty.span_lo()),
                    elem_type: ty,
                }));
            } else {
                let index_type = self.parse_ts_type()?;
                expect!(self, ']');
                ty = Box::new(TsType::TsIndexedAccessType(TsIndexedAccessType {
                    span: span!(self, ty.span_lo()),
                    readonly,
                    obj_type: ty,
                    index_type,
                }))
            }
        }

        Ok(ty)
    }

    /// `tsParseTypeOperator`
    fn parse_ts_type_operator(&mut self, op: TsTypeOperatorOp) -> PResult<TsTypeOperator> {
        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);
        match op {
            TsTypeOperatorOp::Unique => expect!(self, "unique"),
            TsTypeOperatorOp::KeyOf => expect!(self, "keyof"),
            TsTypeOperatorOp::ReadOnly => expect!(self, "readonly"),
        }

        let type_ann = self.parse_ts_type_operator_or_higher()?;
        Ok(TsTypeOperator {
            span: span!(self, start),
            op,
            type_ann,
        })
    }

    /// `tsParseInferType`
    fn parse_ts_infer_type(&mut self) -> PResult<TsInferType> {
        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);
        expect!(self, "infer");
        let type_param_name = self.parse_ident_name()?;
        let constraint = self.try_parse_ts(|p| {
            expect!(p, "extends");
            let constraint = p.parse_ts_non_conditional_type();
            if p.ctx().contains(Context::DisallowConditionalTypes) || !is!(p, '?') {
                constraint.map(Some)
            } else {
                Ok(None)
            }
        });
        let type_param = TsTypeParam {
            span: type_param_name.span(),
            name: type_param_name.into(),
            is_in: false,
            is_out: false,
            is_const: false,
            constraint,
            default: None,
        };
        Ok(TsInferType {
            span: span!(self, start),
            type_param,
        })
    }

    /// `tsParseTypeOperatorOrHigher`
    fn parse_ts_type_operator_or_higher(&mut self) -> PResult<Box<TsType>> {
        trace_cur!(self, parse_ts_type_operator_or_higher);
        debug_assert!(self.input.syntax().typescript());

        let operator = if is!(self, "keyof") {
            Some(TsTypeOperatorOp::KeyOf)
        } else if is!(self, "unique") {
            Some(TsTypeOperatorOp::Unique)
        } else if is!(self, "readonly") {
            Some(TsTypeOperatorOp::ReadOnly)
        } else {
            None
        };

        match operator {
            Some(operator) => self
                .parse_ts_type_operator(operator)
                .map(TsType::from)
                .map(Box::new),
            None => {
                trace_cur!(self, parse_ts_type_operator_or_higher__not_operator);

                if is!(self, "infer") {
                    self.parse_ts_infer_type().map(TsType::from).map(Box::new)
                } else {
                    let readonly = self.parse_ts_modifier(&["readonly"], false)?.is_some();
                    self.parse_ts_array_type_or_higher(readonly)
                }
            }
        }
    }

    /// `tsParseExpressionStatement`
    pub(super) fn parse_ts_expr_stmt(
        &mut self,
        decorators: Vec<Decorator>,
        expr: Ident,
    ) -> PResult<Option<Decl>> {
        if !cfg!(feature = "typescript") {
            return Ok(Default::default());
        }

        let start = expr.span_lo();

        match &*expr.sym {
            "declare" => {
                let decl = self.try_parse_ts_declare(start, decorators)?;
                if let Some(decl) = decl {
                    Ok(Some(make_decl_declare(decl)))
                } else {
                    Ok(None)
                }
            }
            "global" => {
                // `global { }` (with no `declare`) may appear inside an ambient module
                // declaration.
                // Would like to use tsParseAmbientExternalModuleDeclaration here, but already
                // ran past "global".
                if is!(self, '{') {
                    let global = true;
                    let id = TsModuleName::Ident(expr);
                    let body = self
                        .parse_ts_module_block()
                        .map(TsNamespaceBody::from)
                        .map(Some)?;
                    Ok(Some(
                        TsModuleDecl {
                            span: span!(self, start),
                            global,
                            declare: false,
                            namespace: false,
                            id,
                            body,
                        }
                        .into(),
                    ))
                } else {
                    Ok(None)
                }
            }
            _ => self.parse_ts_decl(start, decorators, expr.sym, /* next */ false),
        }
    }

    /// `tsTryParseDeclare`
    pub(super) fn try_parse_ts_declare(
        &mut self,
        start: BytePos,
        decorators: Vec<Decorator>,
    ) -> PResult<Option<Decl>> {
        if !self.syntax().typescript() {
            return Ok(None);
        }

        if self.ctx().contains(Context::InDeclare)
            && matches!(
                self.syntax(),
                Syntax::Typescript(TsSyntax { dts: false, .. })
            )
        {
            let span_of_declare = span!(self, start);
            self.emit_err(span_of_declare, SyntaxError::TS1038);
        }

        let declare_start = start;
        let ctx = self.ctx() | Context::InDeclare;
        self.with_ctx(ctx).parse_with(|p| {
            if is!(p, "function") {
                return p
                    .parse_fn_decl(decorators)
                    .map(|decl| match decl {
                        Decl::Fn(f) => FnDecl {
                            declare: true,
                            function: Box::new(Function {
                                span: Span {
                                    lo: declare_start,
                                    ..f.function.span
                                },
                                ..*f.function
                            }),
                            ..f
                        }
                        .into(),
                        _ => decl,
                    })
                    .map(Some);
            }

            if is!(p, "class") {
                return p
                    .parse_class_decl(start, start, decorators, false)
                    .map(|decl| match decl {
                        Decl::Class(c) => ClassDecl {
                            declare: true,
                            class: Box::new(Class {
                                span: Span {
                                    lo: declare_start,
                                    ..c.class.span
                                },
                                ..*c.class
                            }),
                            ..c
                        }
                        .into(),
                        _ => decl,
                    })
                    .map(Some);
            }

            if is!(p, "const") && peeked_is!(p, "enum") {
                assert_and_bump!(p, "const");
                let _ = cur!(p, true);
                assert_and_bump!(p, "enum");

                return p
                    .parse_ts_enum_decl(start, /* is_const */ true)
                    .map(|decl| TsEnumDecl {
                        declare: true,
                        span: Span {
                            lo: declare_start,
                            ..decl.span
                        },
                        ..*decl
                    })
                    .map(Box::new)
                    .map(From::from)
                    .map(Some);
            }
            if is_one_of!(p, "const", "var", "let") {
                return p
                    .parse_var_stmt(false)
                    .map(|decl| VarDecl {
                        declare: true,
                        span: Span {
                            lo: declare_start,
                            ..decl.span
                        },
                        ..*decl
                    })
                    .map(Box::new)
                    .map(From::from)
                    .map(Some);
            }

            if is!(p, "global") {
                return p
                    .parse_ts_ambient_external_module_decl(start)
                    .map(Decl::from)
                    .map(make_decl_declare)
                    .map(Some);
            } else if is!(p, IdentName) {
                let value = match *cur!(p, true) {
                    Token::Word(ref w) => w.clone().into(),
                    _ => unreachable!(),
                };
                return p
                    .parse_ts_decl(start, decorators, value, /* next */ true)
                    .map(|v| v.map(make_decl_declare));
            }

            Ok(None)
        })
    }

    /// `tsTryParseExportDeclaration`
    ///
    /// Note: this won't be called unless the keyword is allowed in
    /// `shouldParseExportDeclaration`.
    pub(super) fn try_parse_ts_export_decl(
        &mut self,
        decorators: Vec<Decorator>,
        value: Atom,
    ) -> Option<Decl> {
        if !cfg!(feature = "typescript") {
            return None;
        }

        self.try_parse_ts(|p| {
            let start = cur_pos!(p);
            let opt = p.parse_ts_decl(start, decorators, value, true)?;
            Ok(opt)
        })
    }

    /// Common to tsTryParseDeclare, tsTryParseExportDeclaration, and
    /// tsParseExpressionStatement.
    ///
    /// `tsParseDeclaration`
    fn parse_ts_decl(
        &mut self,
        start: BytePos,
        decorators: Vec<Decorator>,
        value: Atom,
        next: bool,
    ) -> PResult<Option<Decl>> {
        if !cfg!(feature = "typescript") {
            return Ok(Default::default());
        }

        match &*value {
            "abstract" => {
                if next || (is!(self, "class") && !self.input.had_line_break_before_cur()) {
                    if next {
                        bump!(self);
                    }
                    return Ok(Some(self.parse_class_decl(start, start, decorators, true)?));
                }
            }

            "enum" => {
                if next || is!(self, IdentRef) {
                    if next {
                        bump!(self);
                    }
                    return self
                        .parse_ts_enum_decl(start, /* is_const */ false)
                        .map(From::from)
                        .map(Some);
                }
            }

            "interface" => {
                if next || (is!(self, IdentRef)) {
                    if next {
                        bump!(self);
                    }

                    return self
                        .parse_ts_interface_decl(start)
                        .map(From::from)
                        .map(Some);
                }
            }

            "module" if !self.input.had_line_break_before_cur() => {
                if next {
                    bump!(self);
                }

                if matches!(*cur!(self, true), Token::Str { .. }) {
                    return self
                        .parse_ts_ambient_external_module_decl(start)
                        .map(From::from)
                        .map(Some);
                } else if next || is!(self, IdentRef) {
                    return self
                        .parse_ts_module_or_ns_decl(start, false)
                        .map(From::from)
                        .map(Some);
                }
            }

            "namespace" => {
                if next || is!(self, IdentRef) {
                    if next {
                        bump!(self);
                    }
                    return self
                        .parse_ts_module_or_ns_decl(start, true)
                        .map(From::from)
                        .map(Some);
                }
            }

            "type" => {
                if next || (!self.input.had_line_break_before_cur() && is!(self, IdentRef)) {
                    if next {
                        bump!(self);
                    }
                    return self
                        .parse_ts_type_alias_decl(start)
                        .map(From::from)
                        .map(Some);
                }
            }

            _ => {}
        }

        Ok(None)
    }

    /// `tsTryParseGenericAsyncArrowFunction`
    pub(super) fn try_parse_ts_generic_async_arrow_fn(
        &mut self,
        start: BytePos,
    ) -> PResult<Option<ArrowExpr>> {
        if !cfg!(feature = "typescript") {
            return Ok(Default::default());
        }

        let res = if is_one_of!(self, '<', JSXTagStart) {
            self.try_parse_ts(|p| {
                let type_params = p.parse_ts_type_params(false, false)?;
                // Don't use overloaded parseFunctionParams which would look for "<" again.
                expect!(p, '(');
                let params: Vec<Pat> = p
                    .parse_formal_params()?
                    .into_iter()
                    .map(|p| p.pat)
                    .collect();
                expect!(p, ')');
                let return_type = p.try_parse_ts_type_or_type_predicate_ann()?;
                expect!(p, "=>");

                Ok(Some((type_params, params, return_type)))
            })
        } else {
            None
        };

        let (type_params, params, return_type) = match res {
            Some(v) => v,
            None => return Ok(None),
        };

        let ctx = (self.ctx() | Context::InAsync) & !Context::InGenerator;
        self.with_ctx(ctx).parse_with(|p| {
            let is_generator = false;
            let is_async = true;
            let body = p.parse_fn_body(true, false, true, params.is_simple_parameter_list())?;
            Ok(Some(ArrowExpr {
                span: span!(p, start),
                body,
                is_async,
                is_generator,
                type_params: Some(type_params),
                params,
                return_type,
                ..Default::default()
            }))
        })
    }

    /// `tsParseTypeArguments`
    pub fn parse_ts_type_args(&mut self) -> PResult<Box<TsTypeParamInstantiation>> {
        trace_cur!(self, parse_ts_type_args);
        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self);
        let params = self.in_type().parse_with(|p| {
            // Temporarily remove a JSX parsing context, which makes us scan different
            // tokens.
            p.ts_in_no_context(|p| {
                if is!(p, "<<") {
                    p.input.cut_lshift();
                } else {
                    expect!(p, '<');
                }
                p.parse_ts_delimited_list(ParsingContext::TypeParametersOrArguments, |p| {
                    trace_cur!(p, parse_ts_type_args__arg);

                    p.parse_ts_type()
                })
            })
        })?;
        // This reads the next token after the `>` too, so do this in the enclosing
        // context. But be sure not to parse a regex in the jsx expression
        // `<C<number> />`, so set exprAllowed = false
        self.input.set_expr_allowed(false);
        expect!(self, '>');
        Ok(Box::new(TsTypeParamInstantiation {
            span: span!(self, start),
            params,
        }))
    }

    /// `tsParseIntersectionTypeOrHigher`
    fn parse_ts_intersection_type_or_higher(&mut self) -> PResult<Box<TsType>> {
        trace_cur!(self, parse_ts_intersection_type_or_higher);

        debug_assert!(self.input.syntax().typescript());

        self.parse_ts_union_or_intersection_type(
            UnionOrIntersection::Intersection,
            |p| p.parse_ts_type_operator_or_higher(),
            &tok!('&'),
        )
    }

    /// `tsParseUnionTypeOrHigher`
    fn parse_ts_union_type_or_higher(&mut self) -> PResult<Box<TsType>> {
        trace_cur!(self, parse_ts_union_type_or_higher);
        debug_assert!(self.input.syntax().typescript());

        self.parse_ts_union_or_intersection_type(
            UnionOrIntersection::Union,
            |p| p.parse_ts_intersection_type_or_higher(),
            &tok!('|'),
        )
    }

    /// `tsParseUnionOrIntersectionType`
    fn parse_ts_union_or_intersection_type<F>(
        &mut self,
        kind: UnionOrIntersection,
        mut parse_constituent_type: F,
        operator: &'static Token,
    ) -> PResult<Box<TsType>>
    where
        F: FnMut(&mut Self) -> PResult<Box<TsType>>,
    {
        trace_cur!(self, parse_ts_union_or_intersection_type);

        debug_assert!(self.input.syntax().typescript());

        let start = cur_pos!(self); // include the leading operator in the start
        self.input.eat(operator);
        trace_cur!(self, parse_ts_union_or_intersection_type__first_type);

        let ty = parse_constituent_type(self)?;
        trace_cur!(self, parse_ts_union_or_intersection_type__after_first);

        if self.input.is(operator) {
            let mut types = vec![ty];

            while self.input.eat(operator) {
                trace_cur!(self, parse_ts_union_or_intersection_type__constituent);

                types.push(parse_constituent_type(self)?);
            }

            return Ok(Box::new(TsType::TsUnionOrIntersectionType(match kind {
                UnionOrIntersection::Union => TsUnionOrIntersectionType::TsUnionType(TsUnionType {
                    span: span!(self, start),
                    types,
                }),
                UnionOrIntersection::Intersection => {
                    TsUnionOrIntersectionType::TsIntersectionType(TsIntersectionType {
                        span: span!(self, start),
                        types,
                    })
                }
            })));
        }

        Ok(ty)
    }
}

impl<I: Tokens> Parser<I> {
    /// In no lexer context
    fn ts_in_no_context<T, F>(&mut self, op: F) -> PResult<T>
    where
        F: FnOnce(&mut Self) -> PResult<T>,
    {
        debug_assert!(self.input.syntax().typescript());

        trace_cur!(self, ts_in_no_context__before);

        let saved = std::mem::take(self.input.token_context_mut());
        self.input.token_context_mut().push(saved.0[0]);
        debug_assert_eq!(self.input.token_context().len(), 1);
        let res = op(self);
        self.input.set_token_context(saved);

        trace_cur!(self, ts_in_no_context__after);

        res
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UnionOrIntersection {
    Union,
    Intersection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsingContext {
    EnumMembers,
    HeritageClauseElement,
    TupleElementTypes,
    TypeMembers,
    TypeParametersOrArguments,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SignatureParsingMode {
    TSCallSignatureDeclaration,
    TSConstructSignatureDeclaration,
}

/// Mark as declare
fn make_decl_declare(mut decl: Decl) -> Decl {
    match decl {
        Decl::Class(ref mut c) => c.declare = true,
        Decl::Fn(ref mut f) => f.declare = true,
        Decl::Var(ref mut v) => v.declare = true,
        Decl::TsInterface(ref mut i) => i.declare = true,
        Decl::TsTypeAlias(ref mut a) => a.declare = true,
        Decl::TsEnum(ref mut e) => e.declare = true,
        Decl::TsModule(ref mut m) => m.declare = true,
        Decl::Using(..) => unreachable!("Using is not a valid declaration for `declare` keyword"),
    }

    decl
}

#[cfg(test)]
mod tests {
    use swc_common::DUMMY_SP;
    use swc_ecma_ast::*;
    use swc_ecma_visit::assert_eq_ignore_span;

    use crate::{lexer::Lexer, test_parser, token::*, Capturing, Parser, Syntax};

    #[test]
    fn issue_708_1() {
        let actual = test_parser(
            "type test = -1;",
            Syntax::Typescript(Default::default()),
            |p| p.parse_module(),
        );

        let expected = Module {
            span: DUMMY_SP,
            shebang: None,
            body: {
                let first = TsTypeAliasDecl {
                    span: DUMMY_SP,
                    declare: false,
                    id: Ident::new_no_ctxt("test".into(), DUMMY_SP),
                    type_params: None,
                    type_ann: Box::new(TsType::TsLitType(TsLitType {
                        span: DUMMY_SP,
                        lit: TsLit::Number(Number {
                            span: DUMMY_SP,
                            value: -1.0,
                            raw: Some("-1".into()),
                        }),
                    })),
                }
                .into();
                vec![first]
            },
        };

        assert_eq_ignore_span!(actual, expected);
    }

    #[test]
    fn issue_708_2() {
        let actual = test_parser(
            "const t = -1;",
            Syntax::Typescript(Default::default()),
            |p| p.parse_module(),
        );

        let expected = Module {
            span: DUMMY_SP,
            shebang: None,
            body: {
                let second = VarDecl {
                    span: DUMMY_SP,
                    kind: VarDeclKind::Const,
                    declare: false,
                    decls: vec![VarDeclarator {
                        span: DUMMY_SP,
                        name: Pat::Ident(Ident::new_no_ctxt("t".into(), DUMMY_SP).into()),
                        init: Some(Box::new(Expr::Unary(UnaryExpr {
                            span: DUMMY_SP,
                            op: op!(unary, "-"),
                            arg: Box::new(Expr::Lit(Lit::Num(Number {
                                span: DUMMY_SP,
                                value: 1.0,
                                raw: Some("1".into()),
                            }))),
                        }))),
                        definite: false,
                    }],
                    ..Default::default()
                }
                .into();
                vec![second]
            },
        };

        assert_eq_ignore_span!(actual, expected);
    }

    #[test]
    fn issue_726() {
        crate::with_test_sess(
            "type Test = (
    string | number);",
            |handler, input| {
                let lexer = Lexer::new(
                    Syntax::Typescript(Default::default()),
                    EsVersion::Es2019,
                    input,
                    None,
                );
                let lexer = Capturing::new(lexer);

                let mut parser = Parser::new_from(lexer);
                parser
                    .parse_typescript_module()
                    .map_err(|e| e.into_diagnostic(handler).emit())?;
                let tokens: Vec<TokenAndSpan> = parser.input().take();
                let tokens = tokens.into_iter().map(|t| t.token).collect::<Vec<_>>();
                assert_eq!(tokens.len(), 9, "Tokens: {:#?}", tokens);
                Ok(())
            },
        )
        .unwrap();
    }

    #[test]
    fn issue_751() {
        crate::with_test_sess("t ? -(v >>> 1) : v >>> 1", |handler, input| {
            let lexer = Lexer::new(
                Syntax::Typescript(Default::default()),
                EsVersion::Es2019,
                input,
                None,
            );
            let lexer = Capturing::new(lexer);

            let mut parser = Parser::new_from(lexer);
            parser
                .parse_typescript_module()
                .map_err(|e| e.into_diagnostic(handler).emit())?;
            let tokens: Vec<TokenAndSpan> = parser.input().take();
            let token = &tokens[10];
            assert_eq!(
                token.token,
                Token::BinOp(BinOpToken::ZeroFillRShift),
                "Token: {:#?}",
                token.token
            );
            Ok(())
        })
        .unwrap();
    }
}
