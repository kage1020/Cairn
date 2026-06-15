//! Cairn surface-syntax parser.
//!
//! Consumes the token stream from [`crate::lex`] and produces a [`Module`].
//! The grammar is line-based with indent-driven nesting: a command can carry
//! `key=value` arguments, an optional bracketed selector, optional bare
//! positional values (for forms like `connect a to b path=@gravel`), and an
//! optional `-> binding` tail.
//!
//! Special forms `logic` and `assert` flow into [`crate::ast::Extra`] rather
//! than the generic command shape.

use crate::ast::{Arg, Command, Expr, Extra, Header, Item, Module, ThemeRule, TruthRow, Value};
use crate::error::{ParseError, Position};
use crate::lex::{Token, TokenKind, lex};

/// Parse a `.crn` source string into a [`Module`].
///
/// # Errors
/// Returns a [`ParseError`] on the first lex or parse failure.
pub fn parse(source: &str) -> Result<Module, ParseError> {
    let tokens = lex(source)?;
    let mut parser = Parser::new(source, &tokens);
    parser.parse_module()
}

struct Parser<'a> {
    source: &'a str,
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, tokens: &'a [Token]) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
        }
    }

    fn parse_module(&mut self) -> Result<Module, ParseError> {
        let mut headers = Vec::new();
        let mut items = Vec::new();
        self.skip_newlines();
        while self.peek_is(&TokenKind::At) {
            headers.push(self.parse_header()?);
            self.skip_newlines();
        }
        while !self.at_eof() {
            items.push(self.parse_item()?);
            self.skip_newlines();
        }
        Ok(Module { headers, items })
    }

    fn parse_header(&mut self) -> Result<Header, ParseError> {
        let position = self.position();
        self.expect(&TokenKind::At)?;
        let name = self.expect_ident()?;
        let value_start_pos = self.position();
        let value_start_byte = self.peek().map_or(self.source.len(), |t| t.span.start);
        let mut value_end_byte = value_start_byte;
        while let Some(t) = self.peek() {
            if matches!(t.kind, TokenKind::Newline) {
                break;
            }
            value_end_byte = t.span.end;
            self.advance();
        }
        let raw = self.source[value_start_byte..value_end_byte]
            .trim()
            .to_owned();
        self.expect_newline()?;
        match name.as_str() {
            "cairn" => Ok(Header::Cairn { version: raw }),
            "requires" => Ok(Header::Requires { requirement: raw }),
            "intended_targets" => {
                // Re-parse the raw value as a list of strings.
                let sub_tokens = lex(&raw)?;
                let mut p = Parser::new(&raw, &sub_tokens);
                let value = p.parse_value()?;
                let targets = match value {
                    Value::List(items) => items
                        .into_iter()
                        .map(|v| match v {
                            Value::Str(s) => Ok(s),
                            other => Err(ParseError::Syntax {
                                position: value_start_pos,
                                message: format!(
                                    "@intended_targets expects strings, got {other:?}"
                                ),
                            }),
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    other => {
                        return Err(ParseError::Syntax {
                            position: value_start_pos,
                            message: format!("@intended_targets expects a list, got {other:?}"),
                        });
                    }
                };
                Ok(Header::IntendedTargets { targets })
            }
            other => Err(ParseError::Syntax {
                position,
                message: format!("unknown directive `@{other}`"),
            }),
        }
    }

    fn parse_item(&mut self) -> Result<Item, ParseError> {
        let position = self.position();
        let kw = self.expect_ident()?;
        match kw.as_str() {
            "theme" => self.parse_theme_item(),
            "def" => self.parse_def_item(),
            "site" => self.parse_site_item(),
            "struct" => self.parse_struct_item(),
            other => Err(ParseError::Syntax {
                position,
                message: format!(
                    "top-level item must be `theme`, `def`, `site`, or `struct`, got `{other}`"
                ),
            }),
        }
    }

    fn parse_theme_item(&mut self) -> Result<Item, ParseError> {
        let name = self.expect_ident()?;
        self.consume_optional_colon();
        self.expect_newline()?;
        let body = if self.peek_is(&TokenKind::Indent) {
            self.advance();
            let mut rules = Vec::new();
            while !self.peek_is(&TokenKind::Dedent) && !self.at_eof() {
                rules.push(self.parse_theme_rule()?);
            }
            if self.peek_is(&TokenKind::Dedent) {
                self.advance();
            }
            rules
        } else {
            Vec::new()
        };
        Ok(Item::Theme { name, body })
    }

    fn parse_theme_rule(&mut self) -> Result<ThemeRule, ParseError> {
        let position = self.position();
        let kw = self.expect_ident()?;
        if kw == "slot" {
            let slot = self.expect_ident()?;
            self.expect(&TokenKind::Arrow)?;
            let value = self.parse_value()?;
            self.expect_newline()?;
            return Ok(ThemeRule::Slot { slot, value });
        }
        // Selector form: KEYWORD '[' attrs ']' '->' key=value (key=value)*
        let attrs = if self.peek_is(&TokenKind::LBracket) {
            self.advance();
            let attrs = self.parse_arg_list_until(&TokenKind::RBracket)?;
            self.expect(&TokenKind::RBracket)?;
            attrs
        } else {
            return Err(ParseError::Syntax {
                position,
                message: format!("expected `slot` or `<keyword>[..]`, got `{kw}`"),
            });
        };
        self.expect(&TokenKind::Arrow)?;
        let mut bindings = Vec::new();
        while !self.peek_is(&TokenKind::Newline) && !self.at_eof() {
            bindings.push(self.parse_arg()?);
        }
        self.expect_newline()?;
        Ok(ThemeRule::Selector {
            keyword: kw,
            attrs,
            bindings,
        })
    }

    fn parse_def_item(&mut self) -> Result<Item, ParseError> {
        let name = self.expect_ident()?;
        let args = self.parse_header_args_until_eol()?;
        self.consume_optional_colon();
        self.expect_newline()?;
        let body = self.parse_optional_command_body()?;
        Ok(Item::Def { name, args, body })
    }

    fn parse_site_item(&mut self) -> Result<Item, ParseError> {
        let name = self.expect_ident()?;
        self.consume_optional_colon();
        self.expect_newline()?;
        let body = self.parse_optional_command_body()?;
        Ok(Item::Site { name, body })
    }

    fn parse_struct_item(&mut self) -> Result<Item, ParseError> {
        let name = self.expect_ident()?;
        let args = self.parse_header_args_until_eol()?;
        self.consume_optional_colon();
        self.expect_newline()?;
        let body = self.parse_optional_command_body()?;
        Ok(Item::Struct { name, args, body })
    }

    fn parse_optional_command_body(&mut self) -> Result<Vec<Command>, ParseError> {
        if !self.peek_is(&TokenKind::Indent) {
            return Ok(Vec::new());
        }
        self.advance();
        let mut commands = Vec::new();
        while !self.peek_is(&TokenKind::Dedent) && !self.at_eof() {
            commands.push(self.parse_command()?);
        }
        if self.peek_is(&TokenKind::Dedent) {
            self.advance();
        }
        Ok(commands)
    }

    fn parse_command(&mut self) -> Result<Command, ParseError> {
        let position = self.position();
        let keyword = self.expect_ident()?;
        match keyword.as_str() {
            "logic" => return self.parse_logic_command(position),
            "assert" => return self.parse_assert_command(position),
            _ => {}
        }
        let selector = if self.peek_is(&TokenKind::LBracket) {
            self.advance();
            let attrs = self.parse_arg_list_until(&TokenKind::RBracket)?;
            self.expect(&TokenKind::RBracket)?;
            Some(attrs)
        } else {
            None
        };
        let mut positional = Vec::new();
        let mut args = Vec::new();
        let mut binding = None;
        loop {
            if self.peek_is(&TokenKind::Newline) || self.at_eof() {
                break;
            }
            if self.peek_is(&TokenKind::Arrow) {
                self.advance();
                binding = Some(self.parse_value()?);
                continue;
            }
            if self.is_at_key_eq() {
                args.push(self.parse_arg()?);
            } else {
                positional.push(self.parse_value()?);
            }
        }
        self.expect_newline()?;
        let children = self.parse_optional_command_body()?;
        Ok(Command {
            keyword,
            selector,
            positional,
            args,
            binding,
            extra: None,
            children,
        })
    }

    fn parse_logic_command(&mut self, _position: Position) -> Result<Command, ParseError> {
        let lhs = self.parse_dotted_ref()?;
        self.expect(&TokenKind::Eq)?;
        let rhs = self.parse_expr()?;
        self.expect_newline()?;
        Ok(Command {
            keyword: "logic".to_owned(),
            selector: None,
            positional: Vec::new(),
            args: Vec::new(),
            binding: None,
            extra: Some(Extra::LogicEq { lhs, rhs }),
            children: Vec::new(),
        })
    }

    fn parse_assert_command(&mut self, position: Position) -> Result<Command, ParseError> {
        let head = self.expect_ident()?;
        match head.as_str() {
            "truth" => self.parse_assert_truth(),
            "always" => self.parse_assert_always(),
            other => Err(ParseError::Syntax {
                position,
                message: format!("expected `truth` or `always` after `assert`, got `{other}`"),
            }),
        }
    }

    fn parse_assert_truth(&mut self) -> Result<Command, ParseError> {
        self.expect(&TokenKind::LParen)?;
        let mut inputs = Vec::new();
        loop {
            let item = self.parse_dotted_ref()?;
            if self.peek_is(&TokenKind::Comma) {
                self.advance();
                inputs.push(item);
            } else if self.peek_is(&TokenKind::Arrow) {
                inputs.push(item);
                break;
            } else {
                return Err(self.syntax_here("expected `,` or `->` in truth(...) inputs"));
            }
        }
        self.expect(&TokenKind::Arrow)?;
        let output = self.parse_dotted_ref()?;
        self.expect(&TokenKind::RParen)?;
        self.expect(&TokenKind::LBrace)?;
        let mut rows = Vec::new();
        while !self.peek_is(&TokenKind::RBrace) && !self.at_eof() {
            let inputs_lex = self.expect_int_lexeme()?;
            self.expect(&TokenKind::Arrow)?;
            let out_lex = self.expect_int_lexeme()?;
            let output = match out_lex.as_str() {
                "0" => 0u8,
                "1" => 1u8,
                other => {
                    return Err(self.syntax_here(&format!(
                        "truth-table output must be `0` or `1`, got `{other}`"
                    )));
                }
            };
            rows.push(TruthRow {
                inputs: inputs_lex,
                output,
            });
            if self.peek_is(&TokenKind::Semi) {
                self.advance();
            }
        }
        self.expect(&TokenKind::RBrace)?;
        self.expect_newline()?;
        Ok(Command {
            keyword: "assert".to_owned(),
            selector: None,
            positional: Vec::new(),
            args: Vec::new(),
            binding: None,
            extra: Some(Extra::AssertTruth {
                inputs,
                output,
                rows,
            }),
            children: Vec::new(),
        })
    }

    fn parse_assert_always(&mut self) -> Result<Command, ParseError> {
        self.expect(&TokenKind::LParen)?;
        let antecedent = self.parse_dotted_ref()?;
        self.expect(&TokenKind::Arrow)?;
        let eventually = self.expect_ident()?;
        if eventually != "eventually" {
            return Err(self.syntax_here("expected `eventually` after `->` in always(...)"));
        }
        let consequent = self.parse_dotted_ref()?;
        let within_kw = self.expect_ident()?;
        if within_kw != "within" {
            return Err(self.syntax_here("expected `within` in always(...)"));
        }
        let within_lex = self.expect_int_lexeme()?;
        let within: u32 = within_lex
            .parse()
            .map_err(|_| self.syntax_here(&format!("invalid `within` bound `{within_lex}`")))?;
        self.expect(&TokenKind::RParen)?;
        self.expect_newline()?;
        Ok(Command {
            keyword: "assert".to_owned(),
            selector: None,
            positional: Vec::new(),
            args: Vec::new(),
            binding: None,
            extra: Some(Extra::AssertAlways {
                antecedent,
                consequent,
                within,
            }),
            children: Vec::new(),
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_expr_or()
    }

    fn parse_expr_or(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_expr_and()?;
        while self.match_keyword("or") {
            let rhs = self.parse_expr_and()?;
            lhs = Expr::Or(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_expr_and(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_expr_not()?;
        while self.match_keyword("and") {
            let rhs = self.parse_expr_not()?;
            lhs = Expr::And(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_expr_not(&mut self) -> Result<Expr, ParseError> {
        if self.match_keyword("not") {
            let inner = self.parse_expr_not()?;
            return Ok(Expr::Not(Box::new(inner)));
        }
        if self.peek_is(&TokenKind::LParen) {
            self.advance();
            let inner = self.parse_expr_or()?;
            self.expect(&TokenKind::RParen)?;
            return Ok(inner);
        }
        let dotted = self.parse_dotted_ref()?;
        Ok(Expr::Ref(dotted))
    }

    fn match_keyword(&mut self, kw: &str) -> bool {
        if let Some(t) = self.peek()
            && let TokenKind::Ident(name) = &t.kind
            && name == kw
        {
            self.advance();
            return true;
        }
        false
    }

    fn parse_header_args_until_eol(&mut self) -> Result<Vec<Arg>, ParseError> {
        let mut args = Vec::new();
        while !self.peek_is(&TokenKind::Newline)
            && !self.peek_is(&TokenKind::Colon)
            && !self.at_eof()
        {
            args.push(self.parse_arg()?);
        }
        Ok(args)
    }

    fn parse_arg_list_until(&mut self, end: &TokenKind) -> Result<Vec<Arg>, ParseError> {
        let mut args = Vec::new();
        while let Some(t) = self.peek() {
            if std::mem::discriminant(&t.kind) == std::mem::discriminant(end) {
                break;
            }
            if matches!(t.kind, TokenKind::Comma) {
                self.advance();
                continue;
            }
            args.push(self.parse_arg()?);
        }
        Ok(args)
    }

    fn parse_arg(&mut self) -> Result<Arg, ParseError> {
        let key = self.expect_ident()?;
        self.expect(&TokenKind::Eq)?;
        let value = self.parse_value()?;
        Ok(Arg { key, value })
    }

    fn parse_value(&mut self) -> Result<Value, ParseError> {
        let position = self.position();
        let Some(token) = self.peek().cloned() else {
            return Err(ParseError::Syntax {
                position,
                message: "expected a value, got end of input".into(),
            });
        };
        match token.kind {
            TokenKind::At => {
                self.advance();
                let mut parts = vec![self.expect_ident()?];
                while self.peek_is(&TokenKind::Dot) {
                    self.advance();
                    parts.push(self.expect_ident()?);
                }
                Ok(Value::Token(parts.join(".")))
            }
            TokenKind::Bool(b) => {
                self.advance();
                Ok(Value::Bool(b))
            }
            TokenKind::Size(w, h) => {
                self.advance();
                Ok(Value::Size { w, h })
            }
            TokenKind::Int { value, .. } => {
                self.advance();
                Ok(Value::Int(value))
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(Value::Str(s))
            }
            TokenKind::Ident(first) => {
                self.advance();
                if self.peek_is(&TokenKind::Dot) {
                    let mut parts = vec![first];
                    while self.peek_is(&TokenKind::Dot) {
                        self.advance();
                        parts.push(self.expect_ident()?);
                    }
                    Ok(Value::DotRef(parts))
                } else {
                    Ok(Value::Ident(first))
                }
            }
            TokenKind::LBracket => {
                self.advance();
                let mut items = Vec::new();
                while !self.peek_is(&TokenKind::RBracket) && !self.at_eof() {
                    items.push(self.parse_value()?);
                    if self.peek_is(&TokenKind::Comma) {
                        self.advance();
                    }
                }
                self.expect(&TokenKind::RBracket)?;
                Ok(Value::List(items))
            }
            other => Err(ParseError::Syntax {
                position,
                message: format!("unexpected token `{other:?}` in value position"),
            }),
        }
    }

    fn parse_dotted_ref(&mut self) -> Result<Vec<String>, ParseError> {
        let first = self.expect_ident()?;
        let mut parts = vec![first];
        while self.peek_is(&TokenKind::Dot) {
            self.advance();
            parts.push(self.expect_ident()?);
        }
        Ok(parts)
    }

    fn is_at_key_eq(&self) -> bool {
        matches!(self.peek().map(|t| &t.kind), Some(TokenKind::Ident(_)))
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Eq)
            )
    }

    fn consume_optional_colon(&mut self) {
        if self.peek_is(&TokenKind::Colon) {
            self.advance();
        }
    }

    fn skip_newlines(&mut self) {
        while self.peek_is(&TokenKind::Newline) {
            self.advance();
        }
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<(), ParseError> {
        let position = self.position();
        let Some(token) = self.peek() else {
            return Err(ParseError::Syntax {
                position,
                message: format!("expected `{kind:?}`, got end of input"),
            });
        };
        if std::mem::discriminant(&token.kind) == std::mem::discriminant(kind) {
            self.advance();
            Ok(())
        } else {
            let found = token.kind.clone();
            Err(ParseError::Syntax {
                position,
                message: format!("expected `{kind:?}`, got `{found:?}`"),
            })
        }
    }

    fn expect_newline(&mut self) -> Result<(), ParseError> {
        if self.at_eof() {
            return Ok(());
        }
        self.expect(&TokenKind::Newline)
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        let position = self.position();
        let Some(token) = self.peek().cloned() else {
            return Err(ParseError::Syntax {
                position,
                message: "expected identifier, got end of input".into(),
            });
        };
        if let TokenKind::Ident(name) = token.kind {
            self.advance();
            Ok(name)
        } else {
            Err(ParseError::Syntax {
                position,
                message: format!("expected identifier, got `{:?}`", token.kind),
            })
        }
    }

    fn expect_int_lexeme(&mut self) -> Result<String, ParseError> {
        let position = self.position();
        let Some(token) = self.peek().cloned() else {
            return Err(ParseError::Syntax {
                position,
                message: "expected integer literal, got end of input".into(),
            });
        };
        if let TokenKind::Int { lexeme, .. } = token.kind {
            self.advance();
            Ok(lexeme)
        } else {
            Err(ParseError::Syntax {
                position,
                message: format!("expected integer literal, got `{:?}`", token.kind),
            })
        }
    }

    fn syntax_here(&self, message: &str) -> ParseError {
        ParseError::Syntax {
            position: self.position(),
            message: message.to_owned(),
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn peek_is(&self, kind: &TokenKind) -> bool {
        self.peek()
            .is_some_and(|t| std::mem::discriminant(&t.kind) == std::mem::discriminant(kind))
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn at_eof(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn position(&self) -> Position {
        self.peek()
            .map_or(Position { line: 0, col: 0 }, |t| t.position)
    }
}
