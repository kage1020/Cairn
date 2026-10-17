//! Cairn surface-syntax parser: line-based, with indent-driven nesting.
//!
//! A body's `requires` lines leave the body, lifted onto the `def` or `theme` that carries them.

use crate::ast::{
    Arg, DottedRef, Expr, Header, Item, MemberRequires, Module, RawRequirement, RawVersion,
    Statement, ThemeRule, TruthRow, Value, ValueKind,
};
use crate::calver::future_version_note;
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::error::{IntContext, ParseError, Position};
use crate::lex::{Lexed, Token, TokenKind, lex, lex_deferring};
use crate::lines::LineStarts;
use crate::version::parse_requirement;
use indexmap::IndexSet;

/// The word that introduces a version floor, after `@` or at the start of a body line.
const REQUIRES: &str = "requires";

/// Whether the body being parsed may carry `requires version>=X` lines, and what to say when it
/// may not. A refusing body refuses only a line that reads as a floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequiresPolicy {
    /// A `def` or `theme` body: lift the line onto the item, whatever follows the word.
    Accepted,
    /// A `struct` or `site` body, whose floor is the file's `@requires`.
    NotAPart,
    /// A member's indented children inside a `def`; the repair is one dedent.
    MemberOfAPart,
    /// A member's indented children inside a `struct` or `site`.
    MemberOfTheBuild,
}

impl RequiresPolicy {
    /// The refusal for a floor written where this policy holds, or `None` where one may stand.
    fn refusal(self) -> Option<&'static str> {
        match self {
            Self::Accepted => None,
            Self::NotAPart => Some(
                "a `struct` or a `site` may not declare `requires version>=X`: it is the build \
                 rather than a part of one, so the floor on it is the whole file's and is \
                 written `@requires version>=X` at the top of the file",
            ),
            Self::MemberOfAPart => Some(
                "a member may not declare `requires version>=X`: the floor belongs to the whole \
                 part, so it goes at the `def` body's own level",
            ),
            Self::MemberOfTheBuild => Some(
                "a member may not declare `requires version>=X`, and neither may the `struct` \
                 or `site` it is in: that is the build rather than a part of one, so the floor \
                 is the whole file's and is written `@requires version>=X` at the top of the file",
            ),
        }
    }

    /// The policy for the indented children of a member in this body.
    fn for_children(self) -> Self {
        match self {
            Self::Accepted | Self::MemberOfAPart => Self::MemberOfAPart,
            Self::NotAPart | Self::MemberOfTheBuild => Self::MemberOfTheBuild,
        }
    }
}

/// Parse a `.crn` source string into a [`Module`].
///
/// A lex failure inside a line stands unless a raw directive value took it, which a stretch
/// holding whitespace other than a space never is.
///
/// # Errors
/// Returns a [`ParseError`] on the first lex or parse failure.
pub fn parse(source: &str) -> Result<Module, ParseError> {
    let Lexed {
        tokens,
        deferred,
        fatal,
    } = lex_deferring(source);
    let mut parser = Parser::new(source, &tokens);
    let parsed = parser.parse_module();
    let taken = |token: usize| {
        !source[tokens[token].span.clone()]
            .chars()
            .any(|c| c.is_whitespace() && c != ' ')
            && parser.raw_values.iter().any(|raw| raw.contains(&token))
    };
    if let Some(stray) = deferred.into_iter().find(|d| !taken(d.token)) {
        return Err(stray.error.into());
    }
    if let Some(fatal) = fatal {
        return Err(fatal.into());
    }
    parsed
}

/// Render a parse failure as an `E_PARSE` [`Diagnostic`] running to the end of its line, with a
/// note when the `@cairn` header names a later or unreadable version.
#[must_use]
pub fn diagnose_parse_failure(source: &str, lines: &LineStarts, err: &ParseError) -> Diagnostic {
    let start = lines.offset_of(source, err.position());
    let end = lines.line_end(source, start).max(start);
    Diagnostic {
        code: DiagnosticCode::Parse,
        span: start..end,
        // Not `to_string`, which prefixes the position the renderer already prints.
        primary: err.user_message(),
        notes: future_version_note(source, lines.line_starts())
            .into_iter()
            .collect(),
        data: None,
    }
}

/// Deepest value or expression nesting the parser descends into, well under the depth at which a
/// debug build's recursive descent overflows the stack.
pub const MAX_NESTING_DEPTH: usize = 64;

/// Deepest boolean-expression tree the parser hands back: `and` / `or` chains parse iteratively,
/// but every consumer walks the tree recursively.
pub const MAX_EXPR_DEPTH: usize = 128;

/// A truth row's input pattern, and whether an adjacent `->` may have eaten its final `-`.
struct TruthPattern {
    text: String,
    ends_at_the_arrow: bool,
}

#[derive(Debug, Clone, Copy)]
enum Directive {
    Cairn,
    Requires,
    IntendedTargets,
}

struct Parser<'a> {
    source: &'a str,
    tokens: &'a [Token],
    pos: usize,
    /// Open value / expression levels, bounded by [`MAX_NESTING_DEPTH`].
    depth: usize,
    /// Token ranges taken whole as a raw directive value, where a [`TokenKind::Unlexed`] is text.
    raw_values: Vec<std::ops::Range<usize>>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, tokens: &'a [Token]) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
            depth: 0,
            raw_values: Vec::new(),
        }
    }

    /// Run `descend` one level deeper, refusing past [`MAX_NESTING_DEPTH`]. Every recursive step
    /// in a value, expression or body must go through here.
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
        let (value_tokens, raw, value_end_byte) = self.rest_of_line();
        let span = start_byte..value_end_byte;
        // A check pass judges these two from their text, unlexable stretches included.
        if matches!(name.as_str(), "cairn" | REQUIRES) {
            self.raw_values.push(value_tokens);
        }
        self.expect_newline()?;
        // Name before value, so a bare `@crain` is an unknown name rather than a missing value.
        let directive = match name.as_str() {
            "cairn" => Directive::Cairn,
            REQUIRES => Directive::Requires,
            "intended_targets" => Directive::IntendedTargets,
            other => {
                return Err(ParseError::Syntax {
                    position,
                    message: format!("unknown directive `@{other}`"),
                });
            }
        };
        if raw.is_empty() {
            return Err(ParseError::Syntax {
                position: value_start_pos,
                message: format!("@{name} requires a value"),
            });
        }
        match directive {
            Directive::Cairn => Ok(Header::Cairn {
                version: RawVersion::new(raw),
                span,
            }),
            Directive::Requires => Ok(Header::Requires {
                requirement: RawRequirement::new(raw),
                span,
            }),
            Directive::IntendedTargets => {
                // The detached sub-parse counts from its own 1:1.
                let sub_tokens =
                    lex(&raw).map_err(|err| ParseError::from(err).rebased(value_start_pos))?;
                let mut p = Parser::new(&raw, &sub_tokens);
                let value = p
                    .parse_value()
                    .map_err(|err| err.rebased(value_start_pos))?;
                if let Some(trailing) = p.peek().filter(|t| t.kind != TokenKind::Newline) {
                    return Err(ParseError::Syntax {
                        position: trailing.position.rebased(value_start_pos),
                        message: "@intended_targets has trailing tokens after the list".into(),
                    });
                }
                let at = |value: &Value| {
                    sub_tokens
                        .iter()
                        .find(|t| t.span.start == value.span.start)
                        .map_or(value_start_pos, |t| t.position.rebased(value_start_pos))
                };
                let targets = match value.kind {
                    ValueKind::List(items) => items
                        .into_iter()
                        .map(|v| match v.kind {
                            ValueKind::Str(s) => Ok(s),
                            _ => Err(ParseError::Syntax {
                                position: at(&v),
                                message: format!(
                                    "@intended_targets expects a string such as `\"1.21.4\"`, \
                                     got `{}`",
                                    &raw[v.span.clone()]
                                ),
                            }),
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => {
                        return Err(ParseError::Syntax {
                            position: at(&value),
                            message: format!(
                                "@intended_targets expects a list of strings such as \
                                 `[\"1.21.4\"]`, got `{}`",
                                &raw[value.span.clone()]
                            ),
                        });
                    }
                };
                Ok(Header::IntendedTargets { targets, span })
            }
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
        let mut requires = Vec::new();
        let body = if self.peek_is(&TokenKind::Indent) {
            self.advance();
            let mut rules = Vec::new();
            while !self.peek_is(&TokenKind::Dedent) && !self.at_eof() {
                if let Some(line) = self.requires_line_here(RequiresPolicy::Accepted)? {
                    requires.push(line);
                    continue;
                }
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
            requires,
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
        let (requires, body) = self.parse_optional_command_body(RequiresPolicy::Accepted)?;
        let span = start_byte..self.last_content_byte();
        Ok(Item::Def {
            name,
            name_span,
            args,
            requires,
            body,
            span,
        })
    }

    fn parse_site_item(&mut self, start_byte: usize) -> Result<Item, ParseError> {
        let (name, name_span) = self.expect_ident_spanned()?;
        self.consume_optional_colon();
        self.expect_newline()?;
        let (_, body) = self.parse_optional_command_body(RequiresPolicy::NotAPart)?;
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
        let (_, body) = self.parse_optional_command_body(RequiresPolicy::NotAPart)?;
        let span = start_byte..self.last_content_byte();
        Ok(Item::Struct {
            name,
            name_span,
            args,
            body,
            span,
        })
    }

    /// Parse an indented body, splitting `requires` lines out of it under `policy`.
    fn parse_optional_command_body(
        &mut self,
        policy: RequiresPolicy,
    ) -> Result<(Vec<MemberRequires>, Vec<Statement>), ParseError> {
        if !self.peek_is(&TokenKind::Indent) {
            return Ok((Vec::new(), Vec::new()));
        }
        self.advance();
        let mut requires = Vec::new();
        let mut commands = Vec::new();
        while !self.peek_is(&TokenKind::Dedent) && !self.at_eof() {
            if let Some(line) = self.requires_line_here(policy)? {
                requires.push(line);
                continue;
            }
            commands.push(self.parse_command(policy)?);
        }
        if self.peek_is(&TokenKind::Dedent) {
            self.advance();
        }
        Ok((requires, commands))
    }

    /// Read a `requires` line here if `policy` lets it; `Ok(None)` leaves the line to be read as
    /// an ordinary member, as one stating no floor is in a body that refuses floors.
    fn requires_line_here(
        &mut self,
        policy: RequiresPolicy,
    ) -> Result<Option<MemberRequires>, ParseError> {
        if !self.peek_is_ident(REQUIRES) {
            return Ok(None);
        }
        let position = self.position();
        let mark = self.pos;
        // A rewound line gives back its raw value, so an unlexable stretch fails again.
        let raw_mark = self.raw_values.len();
        let (raw, span) = self.read_requires_line()?;
        match policy.refusal() {
            None => self.finish_requires_line(position, raw, span).map(Some),
            // Refused as a floor, not as an unexpected `>=` three tokens along.
            Some(message) if parse_requirement(&raw).is_ok() => Err(ParseError::Syntax {
                position,
                message: message.to_owned(),
            }),
            // A line stating no floor, a bare `requires` included, is a member line here.
            Some(_) => {
                self.pos = mark;
                self.raw_values.truncate(raw_mark);
                Ok(None)
            }
        }
    }

    /// Consume the rest of the line: its token range, its trimmed source text (sliced, so an
    /// `Unlexed` stretch is text like any other), and the byte the text ends at.
    fn rest_of_line(&mut self) -> (std::ops::Range<usize>, String, usize) {
        let first = self.pos;
        let value_start_byte = self.current_byte();
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
        (first..self.pos, raw, value_end_byte)
    }

    /// Read one `requires` line verbatim to its end: the expression, and the line's span.
    fn read_requires_line(&mut self) -> Result<(String, crate::error::Span), ParseError> {
        let start_byte = self.current_byte();
        self.advance();
        let (value_tokens, raw, value_end_byte) = self.rest_of_line();
        self.raw_values.push(value_tokens);
        self.expect_newline()?;
        Ok((raw, start_byte..value_end_byte))
    }

    /// Accept a `requires` line [`Self::read_requires_line`] read, in a body that carries floors.
    fn finish_requires_line(
        &self,
        keyword_position: Position,
        raw: String,
        span: crate::error::Span,
    ) -> Result<MemberRequires, ParseError> {
        if raw.is_empty() {
            return Err(ParseError::Syntax {
                position: keyword_position,
                message: "`requires` requires a value".into(),
            });
        }
        // Refused here; the body loop would report `expected identifier, got indent`.
        if self.peek_is(&TokenKind::Indent) {
            return Err(ParseError::Syntax {
                position: self.position(),
                message: "a `requires` floor takes no indented body: it declares one constraint \
                          and nothing is nested under it"
                    .into(),
            });
        }
        Ok(MemberRequires::new(RawRequirement::new(raw), span))
    }

    /// Parse one body line; `policy` is the enclosing body's.
    fn parse_command(&mut self, policy: RequiresPolicy) -> Result<Statement, ParseError> {
        let start_byte = self.current_byte();
        let keyword = self.expect_ident()?;
        match keyword.as_str() {
            "logic" => return self.parse_logic_command(start_byte),
            "assert" => return self.parse_assert_command(start_byte),
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
        // A body recurses like a list or an expression, so it goes through `nested`.
        let (_, children) =
            self.nested(|p| p.parse_optional_command_body(policy.for_children()))?;
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

    fn parse_assert_command(&mut self, start_byte: usize) -> Result<Statement, ParseError> {
        // A misspelt form is wrong in the word after `assert`, so point there.
        let position = self.position();
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
        self.skip_newlines();
        while !self.peek_is(&TokenKind::RBrace) {
            let pattern_position = self.position();
            let row_start_byte = self.current_byte();
            let pattern = self.expect_truth_pattern()?;
            let inputs_lex = pattern.text;
            // Row checks here, where a failure has a position; table checks in `check::truth`.
            if let Some(bad) = inputs_lex.chars().find(|c| !matches!(c, '0' | '1' | '-')) {
                return Err(ParseError::Syntax {
                    position: pattern_position,
                    message: format!(
                        "truth-table input pattern `{inputs_lex}` must hold only `0`, `1` and \
                         `-`, got `{bad}`"
                    ),
                });
            }
            let width = inputs_lex.chars().count();
            if width != inputs.len() {
                // `->` is lexed greedily, so a final `-` before it is lost.
                let hint = if width + 1 == inputs.len() && pattern.ends_at_the_arrow {
                    format!(
                        ". A `-` written immediately before `->` is read as part of the arrow, \
                         so a row whose last input is a don't-care is written `{inputs_lex}--> \
                         0` or `{inputs_lex}- -> 0`"
                    )
                } else {
                    String::new()
                };
                return Err(ParseError::Syntax {
                    position: pattern_position,
                    message: format!(
                        "truth-table input pattern `{inputs_lex}` is {width} bits wide, \
                         but the table has {want} input{plural}{hint}",
                        want = inputs.len(),
                        plural = if inputs.len() == 1 { "" } else { "s" },
                    ),
                });
            }
            self.expect(&TokenKind::Arrow)?;
            let output = if self.peek_is(&TokenKind::Minus) {
                // The arrow is already taken, so this `-` is the output, not the arrow's dash.
                self.advance();
                None
            } else {
                let output = match self.peek().map(|t| &t.kind) {
                    Some(TokenKind::Int { lexeme }) if lexeme == "0" => Some(false),
                    Some(TokenKind::Int { lexeme }) if lexeme == "1" => Some(true),
                    found => {
                        let found = match found {
                            Some(TokenKind::Int { lexeme }) => format!("`{lexeme}`"),
                            Some(kind) => kind.to_string(),
                            None => "end of input".to_owned(),
                        };
                        return Err(ParseError::Syntax {
                            position: self.position(),
                            message: format!(
                                "truth-table output must be `0`, `1`, or `-`, got {found}"
                            ),
                        });
                    }
                };
                self.advance();
                output
            };
            rows.push(TruthRow {
                inputs: inputs_lex,
                output,
                span: row_start_byte..self.last_byte(),
            });
            if self.peek_is(&TokenKind::Semi) {
                self.advance();
            }
            self.skip_newlines();
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
    fn charge_expr_depth(&self, depth: usize) -> Result<(), ParseError> {
        if depth > MAX_EXPR_DEPTH {
            return Err(ParseError::NestingTooDeep {
                position: self.position(),
                limit: MAX_EXPR_DEPTH,
            });
        }
        Ok(())
    }

    /// The expression and its tree depth, so an iteratively parsed chain pays for its shape.
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

    /// `not` costs descent and tree depth; parentheses cost descent alone.
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
        // `and` and `or` are never signal names, so a missing operand is reported here.
        if let Some(Token {
            kind: TokenKind::Ident(word),
            position,
            ..
        }) = self.peek()
            && matches!(word.as_str(), "and" | "or")
        {
            // The token before is an operator word, `=` or `(`.
            let after = match self.pos.checked_sub(1).map(|i| &self.tokens[i].kind) {
                Some(TokenKind::Ident(previous)) => format!("`{previous}`"),
                Some(previous) => previous.to_string(),
                None => "the start of the line".to_owned(),
            };
            return Err(ParseError::Syntax {
                position: *position,
                message: format!("expected a signal after {after}, got the operator `{word}`"),
            });
        }
        let dotted = self.parse_dotted_ref()?;
        Ok((Expr::Ref(dotted), 1))
    }

    /// Whether the next token is the bare identifier `name`, consuming nothing.
    fn peek_is_ident(&self, name: &str) -> bool {
        matches!(self.peek(), Some(t) if matches!(&t.kind, TokenKind::Ident(ident) if ident == name))
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
        if !self.peek_is(&TokenKind::Eq)
            && let Some(literal) = self.size_with_no_height_before(&key)
        {
            return Err(literal);
        }
        self.expect(&TokenKind::Eq)?;
        let value = self.parse_value()?;
        let span = start_byte..self.last_byte();
        Ok(Arg { key, value, span })
    }

    /// The refusal for a key that is the `x` of a size literal with no height, as in `size=9x`,
    /// or `None` when `key` is not one: `9x` lexes as an integer and an `x` key.
    fn size_with_no_height_before(&self, key: &str) -> Option<ParseError> {
        if key != "x" {
            return None;
        }
        let x = self.pos.checked_sub(1)?;
        let int = self.tokens.get(x.checked_sub(1)?)?;
        let TokenKind::Int { lexeme } = &int.kind else {
            return None;
        };
        (int.span.end == self.tokens[x].span.start).then(|| ParseError::Syntax {
            position: int.position,
            message: format!(
                "size literal `{lexeme}x` has no height; a size is two extents, as in `{lexeme}x7`"
            ),
        })
    }

    /// Read an `@` token value whose `@` starts at `start_byte`, with its state literal if any.
    fn parse_token_value(&mut self, start_byte: usize) -> Result<Value, ParseError> {
        self.advance();
        let mut text = self.expect_ident()?;
        let mut dotted = false;
        while self.peek_is(&TokenKind::Dot) {
            self.advance();
            text.push('.');
            text.push_str(&self.expect_ident()?);
            dotted = true;
        }
        // Only a `[` touching an undotted name is a state literal.
        if !dotted
            && self
                .peek()
                .is_some_and(|t| t.kind == TokenKind::LBracket && t.span.start == self.last_byte())
        {
            text.push_str(&self.parse_state_literal(&text)?);
        }
        Ok(Value::new(
            ValueKind::Token(text),
            start_byte..self.last_byte(),
        ))
    }

    /// Read a block-state literal after `@token` as the text it folds into, spaces dropped. The
    /// one place a malformed literal is refused: `block_array::material` reads it back leniently.
    fn parse_state_literal(&mut self, token: &str) -> Result<String, ParseError> {
        let refuse = |position, found: &str| ParseError::Syntax {
            position,
            message: format!(
                "the state literal on `@{token}` expected {found}; a state literal is \
                 `[property=value]` pairs separated by `,`, as in `@oak_log[axis=x]`"
            ),
        };
        self.expect(&TokenKind::LBracket)?;
        let mut text = String::from("[");
        let mut seen: IndexSet<String> = IndexSet::new();
        loop {
            let position = self.position();
            let Some(TokenKind::Ident(property)) = self.peek().map(|t| t.kind.clone()) else {
                let got = self.next_token_text();
                return Err(refuse(position, &format!("a property name, got {got}")));
            };
            if seen.contains(&property) {
                return Err(ParseError::Syntax {
                    position,
                    message: format!(
                        "the state literal on `@{token}` sets `{property}` twice; a block \
                         state has one value per property"
                    ),
                });
            }
            self.advance();
            let position = self.position();
            if !self.peek_is(&TokenKind::Eq) {
                let got = self.next_token_text();
                return Err(refuse(
                    position,
                    &format!("`=` after `{property}`, got {got}"),
                ));
            }
            self.advance();
            let position = self.position();
            let value = match self.peek().map(|t| &t.kind) {
                Some(TokenKind::Ident(value)) => value.clone(),
                Some(TokenKind::Int { lexeme }) => lexeme.clone(),
                Some(TokenKind::Bool(value)) => value.to_string(),
                _ => {
                    let got = self.next_token_text();
                    return Err(refuse(
                        position,
                        &format!("a value for `{property}`, got {got}"),
                    ));
                }
            };
            self.advance();
            if !seen.is_empty() {
                text.push(',');
            }
            text.push_str(&property);
            text.push('=');
            text.push_str(&value);
            seen.insert(property);
            let position = self.position();
            match self.peek().map(|t| &t.kind) {
                Some(TokenKind::Comma) => self.advance(),
                Some(TokenKind::RBracket) => {
                    self.advance();
                    text.push(']');
                    return Ok(text);
                }
                _ => {
                    let got = self.next_token_text();
                    return Err(refuse(position, &format!("`,` or `]`, got {got}")));
                }
            }
        }
    }

    /// The next token as a refusal names it: `` `]` ``, `end of line`.
    fn next_token_text(&self) -> String {
        self.peek()
            .map_or_else(|| "end of input".to_owned(), |t| t.kind.to_string())
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
            TokenKind::At => self.parse_token_value(start_byte),
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
                // Only here do digits become a number, under the `i64` ceiling.
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

    /// The pattern left of a truth row's `->`, reassembled from adjacent `Int` and `Minus` tokens;
    /// a gap ends it.
    fn expect_truth_pattern(&mut self) -> Result<TruthPattern, ParseError> {
        let position = self.position();
        let mut text = String::new();
        let mut previous_end: Option<usize> = None;
        let mut ends_at_the_arrow = false;
        while let Some(token) = self.peek() {
            let adjacent = previous_end.is_none_or(|end| token.span.start == end);
            if !adjacent {
                break;
            }
            let piece = match &token.kind {
                TokenKind::Int { lexeme } => lexeme.clone(),
                TokenKind::Minus => "-".to_owned(),
                _ => {
                    ends_at_the_arrow = matches!(token.kind, TokenKind::Arrow);
                    break;
                }
            };
            previous_end = Some(token.span.end);
            text.push_str(&piece);
            self.advance();
        }
        if text.is_empty() {
            const EXPECTED: &str = "expected a truth-table row or the `}` that closes the table";
            // Inside a table only the end of input dedents, and the refusal belongs at the end of
            // the table's last line rather than past it.
            return Err(match self.peek() {
                Some(token) if token.kind != TokenKind::Dedent => ParseError::Syntax {
                    position,
                    message: format!("{EXPECTED}, got {}", token.kind),
                },
                _ => ParseError::Syntax {
                    position: self
                        .tokens
                        .get(self.pos.wrapping_sub(1))
                        .map_or(position, |t| t.position),
                    message: format!("{EXPECTED}, got end of input"),
                },
            });
        }
        Ok(TruthPattern {
            text,
            ends_at_the_arrow,
        })
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
        // At EOF the last token seen, so the error points where the source ended.
        self.peek()
            .or_else(|| self.tokens.last())
            .map_or(Position::START, |t| t.position)
    }

    /// Byte offset where the next token begins, or `source.len()` at EOF.
    fn current_byte(&self) -> usize {
        self.peek().map_or(self.source.len(), |t| t.span.start)
    }

    /// Byte offset where the last consumed token ended; read it before `expect_newline()`.
    fn last_byte(&self) -> usize {
        self.tokens
            .get(self.pos.wrapping_sub(1))
            .map_or(0, |t| t.span.end)
    }

    /// Byte offset where the last consumed token that is not layout ended.
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
