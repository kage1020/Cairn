//! Cairn surface-syntax parser.
//!
//! Consumes the token stream from [`crate::lex`] and produces a [`Module`].
//! The grammar is line-based with indent-driven nesting: a command can carry
//! `key=value` arguments, an optional bracketed selector, optional bare
//! positional values (for forms like `connect a.entry to b.entry path=@gravel`), and an
//! optional `-> binding` tail.
//!
//! Special forms `logic` and `assert` flow into dedicated
//! [`crate::ast::Statement`] variants rather than the generic command shape.

use crate::ast::{
    Arg, DottedRef, Expr, Header, Item, Module, RawRequirement, RawVersion, Statement, ThemeRule,
    TruthRow, Value, ValueKind,
};
use crate::check::{Diagnostic, DiagnosticCode, LineStarts};
use crate::error::{IntContext, ParseError, Position};
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

/// Render a parse failure as a [`Diagnostic`], so it can be reported
/// beside the findings of every pass that runs after one.
///
/// A parse failure is the one finding no `check` pass produces — nothing
/// runs without an AST — and until it had a shape of its own, every
/// consumer invented one: the CLI wrote a bare `error:` line that carried
/// no code, the language server built an LSP diagnostic directly, and
/// `cairn check --format json` had nothing to put on stdout at all for the
/// most common way a file fails.
///
/// The span runs from the position the error reports to the end of that
/// line. A [`ParseError`] carries a position and not a range — the parser
/// stops *at* a token rather than over a construct — so the rest of the
/// line is what gives an error inside one something to underline. It stops
/// before the line's terminator, so the underline cannot run into the row
/// below.
///
/// The largest class of parse failure gets nothing from that, and is meant
/// to. `expected X, got end of line` is reported *at* the end of its line,
/// so the span is empty and an editor draws a caret rather than a
/// squiggle — which is the right picture: nothing on the line is wrong,
/// something is missing after it.
///
/// Takes the line index rather than building one, which is what
/// [`LineStarts`]' own documentation asks of a caller that will look up
/// more than one position: every renderer of this diagnostic needs an
/// index to put a position in front of the message, so building a second
/// one here would walk the source twice for one finding.
#[must_use]
pub fn diagnose_parse_failure(source: &str, lines: &LineStarts, err: &ParseError) -> Diagnostic {
    let start = lines.offset_of(source, err.position());
    let end = lines.line_end(source, start).max(start);
    Diagnostic {
        code: DiagnosticCode::Parse,
        span: start..end,
        // The renderer prints the position in front of the message, from
        // the span; `ParseError`'s own `Display` prefixes it too, which is
        // why this reads `user_message` rather than `to_string`.
        primary: err.user_message(),
        notes: Vec::new(),
        // No structured payload. A consumer that wants to branch on which
        // parse failure it is has the message; giving it a payload means
        // freezing a shape for two `#[non_exhaustive]` enums, and that can
        // be added later without breaking anyone.
        data: None,
    }
}

/// Deepest value / expression nesting the parser will descend into.
///
/// Recursive descent spends native stack per level, and a Rust stack
/// overflow is an uncatchable abort: it takes the process down, and with it
/// any embedder — `cairn-lsp` re-parses on every keystroke. Refusing is the
/// only way to keep that a diagnostic rather than a crash.
///
/// Measured on a debug build, the tightest shape (`mat=[[[…`) overflowed at
/// 287 levels, parenthesised expressions at 380, and `not` chains at 785.
/// Sitting well under the tightest of those leaves room for a thread with a
/// smaller stack than the main one, and for the deeper frames a debug build
/// of a future pass may add.
///
/// Real sources come nowhere near: values are scalars and flat lists, and a
/// `logic` expression this deep would be unreadable long before it got here.
pub const MAX_NESTING_DEPTH: usize = 64;

/// Deepest boolean-expression tree the parser will hand back.
///
/// Separate from [`MAX_NESTING_DEPTH`] because it bounds a different stack.
/// That one keeps the parser's own descent finite; this one keeps the
/// *result* finite, because `or` and `and` are parsed iteratively and so
/// build a left-leaning tree one node deep per term at no cost to the
/// parser. Every consumer walks that tree recursively — `Serialize`, the
/// check passes, and `Box`'s recursive `Drop` — so leaving it unbounded
/// only moves the overflow downstream of the guard.
///
/// Measured on a debug build, `cairn parse` (which serialises the AST) died
/// at roughly 570 terms, so this keeps the same order of margin
/// [`MAX_NESTING_DEPTH`] holds against its own measurement. A chain past it
/// splits into intermediate `logic` bindings, which the diagnostic says.
pub const MAX_EXPR_DEPTH: usize = 128;

struct Parser<'a> {
    source: &'a str,
    tokens: &'a [Token],
    pos: usize,
    /// How many value / expression levels are currently open, bounded by
    /// [`MAX_NESTING_DEPTH`].
    depth: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, tokens: &'a [Token]) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
            depth: 0,
        }
    }

    /// Run `descend` one nesting level deeper, refusing past
    /// [`MAX_NESTING_DEPTH`].
    ///
    /// Every recursive step in a value or expression goes through here, so
    /// the counter is the single place the bound is enforced — a new
    /// recursive production cannot reopen the hole by forgetting to check.
    fn nested<T>(
        &mut self,
        descend: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        if self.depth >= MAX_NESTING_DEPTH {
            return Err(ParseError::NestingTooDeep {
                position: self.position(),
                limit: MAX_NESTING_DEPTH,
            });
        }
        self.depth += 1;
        let result = descend(self);
        self.depth -= 1;
        result
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
        let start_byte = self.current_byte();
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
        let span = start_byte..value_end_byte;
        self.expect_newline()?;
        if raw.is_empty() {
            return Err(ParseError::Syntax {
                position: value_start_pos,
                message: format!("@{name} requires a value"),
            });
        }
        match name.as_str() {
            "cairn" => Ok(Header::Cairn {
                version: RawVersion::new(raw),
                span,
            }),
            "requires" => Ok(Header::Requires {
                requirement: RawRequirement::new(raw),
                span,
            }),
            "intended_targets" => {
                // Re-parse the raw value as a list of strings.
                //
                // The slice is detached from the file, so the sub-parse
                // counts from its own 1:1 and every diagnostic out of it
                // has to be rebased onto `value_start_pos` before it
                // reaches a caller. Without that a bad element is
                // reported on line 1 of the file whatever line the
                // directive is on.
                let sub_tokens =
                    lex(&raw).map_err(|err| ParseError::from(err).rebased(value_start_pos))?;
                let mut p = Parser::new(&raw, &sub_tokens);
                let value = p
                    .parse_value()
                    .map_err(|err| err.rebased(value_start_pos))?;
                // Reject trailing tokens — `@intended_targets [..] junk` should fail.
                if !matches!(p.peek().map(|t| &t.kind), None | Some(TokenKind::Newline),) {
                    return Err(ParseError::Syntax {
                        position: value_start_pos,
                        message: "@intended_targets has trailing tokens after the list".into(),
                    });
                }
                let targets = match value.kind {
                    ValueKind::List(items) => items
                        .into_iter()
                        .map(|v| match v.kind {
                            ValueKind::Str(s) => Ok(s),
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
                Ok(Header::IntendedTargets { targets, span })
            }
            other => Err(ParseError::Syntax {
                position,
                message: format!("unknown directive `@{other}`"),
            }),
        }
    }

    fn parse_item(&mut self) -> Result<Item, ParseError> {
        let position = self.position();
        let start_byte = self.current_byte();
        let kw = self.expect_ident()?;
        match kw.as_str() {
            "theme" => self.parse_theme_item(start_byte),
            "def" => self.parse_def_item(start_byte),
            "site" => self.parse_site_item(start_byte),
            "struct" => self.parse_struct_item(start_byte),
            other => Err(ParseError::Syntax {
                position,
                message: format!(
                    "top-level item must be `theme`, `def`, `site`, or `struct`, got `{other}`"
                ),
            }),
        }
    }

    fn parse_theme_item(&mut self, start_byte: usize) -> Result<Item, ParseError> {
        let (name, name_span) = self.expect_ident_spanned()?;
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
        let span = start_byte..self.last_content_byte();
        Ok(Item::Theme {
            name,
            name_span,
            body,
            span,
        })
    }

    fn parse_theme_rule(&mut self) -> Result<ThemeRule, ParseError> {
        let position = self.position();
        let start_byte = self.current_byte();
        let kw = self.expect_ident()?;
        if kw == "slot" {
            let slot = self.expect_ident()?;
            self.expect(&TokenKind::Arrow)?;
            let value = self.parse_value()?;
            let span = start_byte..self.last_byte();
            self.expect_newline()?;
            return Ok(ThemeRule::Slot { slot, value, span });
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
        let span = start_byte..self.last_byte();
        self.expect_newline()?;
        Ok(ThemeRule::Selector {
            keyword: kw,
            attrs,
            bindings,
            span,
        })
    }

    fn parse_def_item(&mut self, start_byte: usize) -> Result<Item, ParseError> {
        let (name, name_span) = self.expect_ident_spanned()?;
        let args = self.parse_header_args_until_eol()?;
        self.consume_optional_colon();
        self.expect_newline()?;
        let body = self.parse_optional_command_body()?;
        let span = start_byte..self.last_content_byte();
        Ok(Item::Def {
            name,
            name_span,
            args,
            body,
            span,
        })
    }

    fn parse_site_item(&mut self, start_byte: usize) -> Result<Item, ParseError> {
        let (name, name_span) = self.expect_ident_spanned()?;
        self.consume_optional_colon();
        self.expect_newline()?;
        let body = self.parse_optional_command_body()?;
        let span = start_byte..self.last_content_byte();
        Ok(Item::Site {
            name,
            name_span,
            body,
            span,
        })
    }

    fn parse_struct_item(&mut self, start_byte: usize) -> Result<Item, ParseError> {
        let (name, name_span) = self.expect_ident_spanned()?;
        let args = self.parse_header_args_until_eol()?;
        self.consume_optional_colon();
        self.expect_newline()?;
        let body = self.parse_optional_command_body()?;
        let span = start_byte..self.last_content_byte();
        Ok(Item::Struct {
            name,
            name_span,
            args,
            body,
            span,
        })
    }

    fn parse_optional_command_body(&mut self) -> Result<Vec<Statement>, ParseError> {
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

    fn parse_command(&mut self) -> Result<Statement, ParseError> {
        let position = self.position();
        let start_byte = self.current_byte();
        let keyword = self.expect_ident()?;
        match keyword.as_str() {
            "logic" => return self.parse_logic_command(start_byte),
            "assert" => return self.parse_assert_command(position, start_byte),
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
                let arrow_pos = self.position();
                self.advance();
                if binding.is_some() {
                    return Err(ParseError::Syntax {
                        position: arrow_pos,
                        message: "a command may only have one `-> binding` tail".into(),
                    });
                }
                binding = Some(self.parse_value()?);
                continue;
            }
            if self.is_at_key_eq() {
                args.push(self.parse_arg()?);
            } else {
                positional.push(self.parse_value()?);
            }
        }
        let span = start_byte..self.last_byte();
        self.expect_newline()?;
        // An indented body is the third recursive production, alongside list
        // values and expressions: `parse_command` → here → `parse_command`.
        // Reaching a nested body costs O(n²) source bytes because each level
        // repeats its own indent, but that is a constant factor, not a
        // bound — 400 levels is a third of a megabyte and used to abort the
        // process just as `[[[…` did.
        let children = self.nested(Self::parse_optional_command_body)?;
        Ok(Statement::Generic {
            keyword,
            selector,
            positional,
            args,
            binding,
            children,
            span,
        })
    }

    fn parse_logic_command(&mut self, start_byte: usize) -> Result<Statement, ParseError> {
        let lhs = self.parse_dotted_ref()?;
        self.expect(&TokenKind::Eq)?;
        let rhs = self.parse_expr()?;
        let span = start_byte..self.last_byte();
        self.expect_newline()?;
        Ok(Statement::Logic { lhs, rhs, span })
    }

    fn parse_assert_command(
        &mut self,
        position: Position,
        start_byte: usize,
    ) -> Result<Statement, ParseError> {
        let head = self.expect_ident()?;
        match head.as_str() {
            "truth" => self.parse_assert_truth(start_byte),
            "always" => self.parse_assert_always(start_byte),
            other => Err(ParseError::Syntax {
                position,
                message: format!("expected `truth` or `always` after `assert`, got `{other}`"),
            }),
        }
    }

    fn parse_assert_truth(&mut self, start_byte: usize) -> Result<Statement, ParseError> {
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
            let pattern_position = self.position();
            let row_start_byte = self.current_byte();
            let inputs_lex = self.expect_int_lexeme()?;
            // A row assigns one bit per input signal, so the pattern is
            // checked against the list left of the arrow.
            //
            // Here rather than downstream, though `AssertIr::Truth` does
            // keep `inputs` beside `rows` and could check it again: this
            // is where the failure has a position to point at, and where
            // it is checked once instead of by every pass that grows a
            // reason to care.
            //
            // What one row cannot see is the table around it — no rows at
            // all, a pattern assigned twice, combinations left out. Those
            // are `check::truth`, which is why each row keeps a span: the
            // finding there is one row reported against another.
            //
            // A leading zero is data here rather than a numeric quirk,
            // which is why the row keeps the raw lexeme: `01` and `1` are
            // different rows of a two-input table.
            if let Some(bad) = inputs_lex.chars().find(|c| !matches!(c, '0' | '1')) {
                return Err(ParseError::Syntax {
                    position: pattern_position,
                    message: format!(
                        "truth-table input pattern `{inputs_lex}` must hold only `0` and `1`, \
                         got `{bad}`"
                    ),
                });
            }
            if inputs_lex.chars().count() != inputs.len() {
                return Err(ParseError::Syntax {
                    position: pattern_position,
                    message: format!(
                        "truth-table input pattern `{inputs_lex}` is {got} bits wide, \
                         but the table has {want} input{plural}",
                        got = inputs_lex.chars().count(),
                        want = inputs.len(),
                        plural = if inputs.len() == 1 { "" } else { "s" },
                    ),
                });
            }
            self.expect(&TokenKind::Arrow)?;
            let out_lex = self.expect_int_lexeme()?;
            let output = match out_lex.as_str() {
                "0" => false,
                "1" => true,
                other => {
                    return Err(self.syntax_here(&format!(
                        "truth-table output must be `0` or `1`, got `{other}`"
                    )));
                }
            };
            rows.push(TruthRow {
                inputs: inputs_lex,
                output,
                span: row_start_byte..self.last_byte(),
            });
            if self.peek_is(&TokenKind::Semi) {
                self.advance();
            }
        }
        self.expect(&TokenKind::RBrace)?;
        let span = start_byte..self.last_byte();
        self.expect_newline()?;
        Ok(Statement::AssertTruth {
            inputs,
            output,
            rows,
            span,
        })
    }

    fn parse_assert_always(&mut self, start_byte: usize) -> Result<Statement, ParseError> {
        self.expect(&TokenKind::LParen)?;
        let antecedent = self.parse_dotted_ref()?;
        self.expect(&TokenKind::Arrow)?;
        let eventually_pos = self.position();
        if !self.match_keyword("eventually") {
            return Err(ParseError::Syntax {
                position: eventually_pos,
                message: "expected `eventually` after `->` in always(...)".into(),
            });
        }
        let consequent = self.parse_dotted_ref()?;
        let within_pos = self.position();
        if !self.match_keyword("within") {
            return Err(ParseError::Syntax {
                position: within_pos,
                message: "expected `within N` in always(...)".into(),
            });
        }
        let within_bound_pos = self.position();
        let within_lex = self.expect_int_lexeme()?;
        let within: u32 =
            within_lex
                .parse()
                .map_err(|err: std::num::ParseIntError| ParseError::InvalidInt {
                    position: within_bound_pos,
                    context: IntContext::WithinBound,
                    lexeme: within_lex.clone(),
                    kind: *err.kind(),
                })?;
        self.expect(&TokenKind::RParen)?;
        let span = start_byte..self.last_byte();
        self.expect_newline()?;
        Ok(Statement::AssertAlways {
            antecedent,
            consequent,
            within,
            span,
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        Ok(self.parse_expr_or()?.0)
    }

    /// Refuse a boolean tree deeper than [`MAX_EXPR_DEPTH`].
    ///
    /// [`Self::nested`] bounds how deep the *parser* descends, which is a
    /// different quantity: `or` and `and` iterate, so a flat chain costs the
    /// parser nothing while still building a left-leaning tree one node deep
    /// per term. Everything downstream walks that tree recursively —
    /// `Serialize`, the check passes, and `Box`'s own recursive `Drop` — so
    /// an unbounded chain moves the overflow out of the parser and into
    /// whoever touches the result. Measured on a debug build,
    /// `cairn parse` (which serialises) died at roughly 570 terms.
    fn charge_expr_depth(&self, depth: usize) -> Result<(), ParseError> {
        if depth > MAX_EXPR_DEPTH {
            return Err(ParseError::NestingTooDeep {
                position: self.position(),
                limit: MAX_EXPR_DEPTH,
            });
        }
        Ok(())
    }

    /// Returns the expression and the depth of the tree it built, so an
    /// iteratively-parsed chain still pays for the shape it produced.
    fn parse_expr_or(&mut self) -> Result<(Expr, usize), ParseError> {
        let (mut lhs, mut depth) = self.parse_expr_and()?;
        while self.match_keyword("or") {
            let (rhs, rhs_depth) = self.parse_expr_and()?;
            depth = depth.max(rhs_depth) + 1;
            self.charge_expr_depth(depth)?;
            lhs = Expr::Or(Box::new(lhs), Box::new(rhs));
        }
        Ok((lhs, depth))
    }

    fn parse_expr_and(&mut self) -> Result<(Expr, usize), ParseError> {
        let (mut lhs, mut depth) = self.parse_expr_not()?;
        while self.match_keyword("and") {
            let (rhs, rhs_depth) = self.parse_expr_not()?;
            depth = depth.max(rhs_depth) + 1;
            self.charge_expr_depth(depth)?;
            lhs = Expr::And(Box::new(lhs), Box::new(rhs));
        }
        Ok((lhs, depth))
    }

    /// Both recursive shapes an expression can take — a `not` chain and a
    /// parenthesised sub-expression — pass through [`Self::nested`], which
    /// bounds the parser's own descent. Parentheses build no node, so they
    /// cost descent without costing tree depth; `not` costs both.
    fn parse_expr_not(&mut self) -> Result<(Expr, usize), ParseError> {
        if self.match_keyword("not") {
            let (inner, inner_depth) = self.nested(Self::parse_expr_not)?;
            let depth = inner_depth + 1;
            self.charge_expr_depth(depth)?;
            return Ok((Expr::Not(Box::new(inner)), depth));
        }
        if self.peek_is(&TokenKind::LParen) {
            self.advance();
            let inner = self.nested(Self::parse_expr_or)?;
            self.expect(&TokenKind::RParen)?;
            return Ok(inner);
        }
        let dotted = self.parse_dotted_ref()?;
        Ok((Expr::Ref(dotted), 1))
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
        let start_byte = self.current_byte();
        let key = self.expect_ident()?;
        self.expect(&TokenKind::Eq)?;
        let value = self.parse_value()?;
        let span = start_byte..self.last_byte();
        Ok(Arg { key, value, span })
    }

    fn parse_value(&mut self) -> Result<Value, ParseError> {
        let position = self.position();
        let start_byte = self.current_byte();
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
                Ok(Value::new(
                    ValueKind::Token(parts.join(".")),
                    start_byte..self.last_byte(),
                ))
            }
            TokenKind::Bool(b) => {
                self.advance();
                Ok(Value::new(ValueKind::Bool(b), token.span))
            }
            TokenKind::Size(raw_w, raw_h) => {
                let size_pos = position;
                let size_span = token.span.clone();
                self.advance();
                let w = std::num::NonZeroU32::new(raw_w).ok_or_else(|| ParseError::Syntax {
                    position: size_pos,
                    message: format!("size literal width must be non-zero, got `{raw_w}x{raw_h}`"),
                })?;
                let h = std::num::NonZeroU32::new(raw_h).ok_or_else(|| ParseError::Syntax {
                    position: size_pos,
                    message: format!("size literal height must be non-zero, got `{raw_w}x{raw_h}`"),
                })?;
                Ok(Value::new(ValueKind::Size { w, h }, size_span))
            }
            TokenKind::Int { lexeme, .. } => {
                self.advance();
                // Where the digits are asked to be a number, so where the
                // `i64` ceiling belongs. See `TokenKind::Int`.
                let value = lexeme
                    .parse::<i64>()
                    .map_err(|err: std::num::ParseIntError| ParseError::InvalidInt {
                        position,
                        context: IntContext::IntLiteral,
                        lexeme: lexeme.clone(),
                        kind: *err.kind(),
                    })?;
                Ok(Value::new(ValueKind::Int(value), token.span))
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(Value::new(ValueKind::Str(s), token.span))
            }
            TokenKind::Ident(first) => {
                self.advance();
                if self.peek_is(&TokenKind::Dot) {
                    let mut tail = Vec::new();
                    while self.peek_is(&TokenKind::Dot) {
                        self.advance();
                        tail.push(self.expect_ident()?);
                    }
                    Ok(Value::new(
                        ValueKind::DotRef(DottedRef::new(first, tail)),
                        start_byte..self.last_byte(),
                    ))
                } else {
                    Ok(Value::new(ValueKind::Ident(first), token.span))
                }
            }
            TokenKind::LBracket => {
                self.advance();
                let mut items = Vec::new();
                while !self.peek_is(&TokenKind::RBracket) && !self.at_eof() {
                    items.push(self.nested(Self::parse_value)?);
                    if self.peek_is(&TokenKind::Comma) {
                        self.advance();
                    }
                }
                self.expect(&TokenKind::RBracket)?;
                Ok(Value::new(
                    ValueKind::List(items),
                    start_byte..self.last_byte(),
                ))
            }
            other => Err(ParseError::Syntax {
                position,
                message: format!("unexpected {other} in value position"),
            }),
        }
    }

    fn parse_dotted_ref(&mut self) -> Result<DottedRef, ParseError> {
        let head = self.expect_ident()?;
        let mut tail = Vec::new();
        while self.peek_is(&TokenKind::Dot) {
            self.advance();
            tail.push(self.expect_ident()?);
        }
        Ok(DottedRef::new(head, tail))
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
                message: format!("expected {kind}, got end of input"),
            });
        };
        if std::mem::discriminant(&token.kind) == std::mem::discriminant(kind) {
            self.advance();
            Ok(())
        } else {
            let found = token.kind.clone();
            Err(ParseError::Syntax {
                position,
                message: format!("expected {kind}, got {found}"),
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
        self.expect_ident_spanned().map(|(name, _)| name)
    }

    /// [`Self::expect_ident`] keeping the identifier's own byte range.
    ///
    /// Callers that record a name in the AST need it: the enclosing
    /// item's span covers the indented body, and reconstructing the
    /// header line from `span.start` plus the keyword's length assumes a
    /// single space that `def   hut` does not have.
    fn expect_ident_spanned(&mut self) -> Result<(String, crate::error::Span), ParseError> {
        let position = self.position();
        let Some(token) = self.peek().cloned() else {
            return Err(ParseError::Syntax {
                position,
                message: "expected identifier, got end of input".into(),
            });
        };
        if let TokenKind::Ident(name) = token.kind {
            self.advance();
            Ok((name, token.span))
        } else {
            Err(ParseError::Syntax {
                position,
                message: format!("expected identifier, got {}", token.kind),
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
                message: format!("expected integer literal, got {}", token.kind),
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
        // Prefer the next token's position; if exhausted, fall back to the
        // last token actually seen so that EOF errors still point at where
        // the source ended rather than a meaningless sentinel. An empty
        // source has neither, so `Position::START` is the only sensible
        // anchor.
        self.peek()
            .or_else(|| self.tokens.last())
            .map_or(Position::START, |t| t.position)
    }

    /// Byte offset where the next un-consumed token begins, or `source.len()`
    /// at EOF. Anchors the start of a node's [`Span`] before any token is
    /// consumed in its parser arm.
    fn current_byte(&self) -> usize {
        self.peek().map_or(self.source.len(), |t| t.span.start)
    }

    /// Byte offset where the most-recently-consumed token ended. Used to
    /// close a [`Span`] right after the last meaningful token for a node;
    /// callers capture this *before* `expect_newline()` so trailing layout
    /// tokens stay outside the node's span.
    fn last_byte(&self) -> usize {
        self.tokens
            .get(self.pos.wrapping_sub(1))
            .map_or(0, |t| t.span.end)
    }

    /// Byte offset of the end of the last *content* token consumed, ignoring
    /// trailing `Newline`/`Indent`/`Dedent` layout tokens. Used for
    /// container-level spans ([`Item::Theme`] etc.) whose body may have just
    /// closed with a `Dedent`.
    fn last_content_byte(&self) -> usize {
        let mut i = self.pos;
        while i > 0 {
            let prev = &self.tokens[i - 1];
            if matches!(
                prev.kind,
                TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent
            ) {
                i -= 1;
                continue;
            }
            return prev.span.end;
        }
        0
    }
}
