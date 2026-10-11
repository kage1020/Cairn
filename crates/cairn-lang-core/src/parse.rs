//! Cairn surface-syntax parser.
//!
//! Consumes the token stream from [`crate::lex()`] and produces a [`Module`]. The grammar is
//! line-based with indent-driven nesting: a command carries `key=value` arguments, an optional
//! bracketed selector, optional bare positional values (as in
//! `connect a.entry to b.entry path=@gravel`), and an optional `-> binding` tail.
//!
//! `logic` and `assert` become [`crate::ast::Statement`] variants of their own. `requires` leaves
//! the body entirely: it declares a version floor rather than anything to build, so it is lifted
//! onto the `def` or `theme` that carries it ([`crate::ast::MemberRequires`]).

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

/// The word that introduces a version floor: after `@`, a floor on the file; bare on a body line,
/// a floor on the part.
const REQUIRES: &str = "requires";

/// Whether the body being parsed may carry `requires version>=X` lines,
/// and what to say when it may not.
///
/// `spec/versioning-editions` "Fail-loud and minimum-version inference" gives the member-level
/// floor to `def` and `theme`, the kinds a build instantiates rather than is. Every body passes a
/// policy, so the rule is decided in one place.
///
/// The three refusing variants keep three messages, because one message would say something false
/// about the files it does not fit. None refuses on the word alone: only a line whose expression
/// reads as a version floor is refused, since `requires` is an ordinary identifier in a body that
/// reads no floors. An accepting body lifts the line whatever follows the word, so
/// `requires foo=1` in a `def` is a floor that states nothing (`E_INVALID_REQUIRES`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequiresPolicy {
    /// A `def` or `theme` body: lift the line onto the item, whatever
    /// follows the word.
    Accepted,
    /// A `struct` or `site` body. Each is the build, so its floor is the file's, which is what
    /// `@requires` says.
    NotAPart,
    /// A member's indented children inside a `def`. The floor is the part's, so the repair is one
    /// dedent.
    MemberOfAPart,
    /// A member's indented children inside a `struct` or `site`. One dedent would land in a body
    /// that refuses it too, so the repair is [`Self::NotAPart`]'s.
    MemberOfTheBuild,
}

impl RequiresPolicy {
    /// The refusal for a floor written where this policy holds, or `None`
    /// where one may stand.
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

    /// The policy for the indented children of a member in a body under
    /// this one.
    fn for_children(self) -> Self {
        match self {
            Self::Accepted | Self::MemberOfAPart => Self::MemberOfAPart,
            Self::NotAPart | Self::MemberOfTheBuild => Self::MemberOfTheBuild,
        }
    }
}

/// Parse a `.crn` source string into a [`Module`].
///
/// A lex failure inside a line is judged after the parse. `@cairn`, `@requires` and a part's
/// `requires` take raw text to end of line, so a character no token starts with is theirs to judge
/// (`W_INVALID_CAIRN_VERSION`, `E_INVALID_REQUIRES`). A failure none of them took is reported
/// exactly as [`lex()`] reports it, ahead of whatever the parse found.
///
/// A refused stretch holding whitespace other than a space, alone or inside an unterminated
/// string, is never taken into a value. The lexer separates tokens with spaces alone, and a tab
/// taken in and then trimmed would make `@cairn` declare the version behind it.
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

/// Render a parse failure as a [`Diagnostic`], so it can be reported
/// beside the findings of every pass that runs after one.
///
/// A [`ParseError`] has a position, not a range, so the span runs from there to the end of the
/// line, before its terminator so the underline cannot reach the row below. An
/// `expected X, got end of line` error sits at the end of its line, so its span is empty and an
/// editor draws a caret: something is missing after the line.
///
/// Takes the line index rather than building one, since every renderer needs one anyway.
///
/// The one note comes from the `@cairn` header, when it declares a later language than this build
/// or names no language version, because no check pass runs to say so. Built here rather than in
/// a front end, so the CLI and the language server both get it.
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
        // No payload: one would freeze a shape for two `#[non_exhaustive]` enums.
        data: None,
    }
}

/// Deepest value or expression nesting the parser descends into.
///
/// Recursive descent spends native stack per level, and a stack overflow aborts the process,
/// taking any embedder with it (`cairn-lsp` re-parses on every keystroke). Measured on a debug
/// build, the tightest shape (`mat=[[[…`) overflowed at 287 levels, parenthesised expressions at
/// 380 and `not` chains at 785; staying well under leaves room for a smaller thread stack and
/// deeper frames. Real sources come nowhere near.
pub const MAX_NESTING_DEPTH: usize = 64;

/// Deepest boolean-expression tree the parser hands back.
///
/// Separate from [`MAX_NESTING_DEPTH`], which bounds the parser's descent: `or` and `and` are
/// parsed iteratively, building a left-leaning tree one node per term at no cost to the parser,
/// while every consumer (`Serialize`, the check passes, `Box`'s `Drop`) walks it recursively.
/// Measured on a debug build, `cairn parse` died at roughly 570 terms, so this keeps the margin
/// [`MAX_NESTING_DEPTH`] keeps. A longer chain splits into intermediate `logic` bindings, as the
/// diagnostic says.
pub const MAX_EXPR_DEPTH: usize = 128;

/// A truth row's input pattern, and whether it stopped at an adjacent `->`, which may have eaten
/// a final `-`; a width mismatch then says so.
struct TruthPattern {
    text: String,
    ends_at_the_arrow: bool,
}

/// The file directives the language defines, named once the parser has
/// read the word after `@`.
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
    /// How many value / expression levels are currently open, bounded by
    /// [`MAX_NESTING_DEPTH`].
    depth: usize,
    /// Token-index ranges taken whole as a raw directive value, where a
    /// [`TokenKind::Unlexed`] is part of the text rather than a failure.
    /// See [`parse`].
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

    /// Run `descend` one nesting level deeper, refusing past
    /// [`MAX_NESTING_DEPTH`].
    ///
    /// Every recursive step in a value, expression or body must go through here, so the bound is
    /// enforced in one place.
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
                // The detached sub-parse counts from its own 1:1, so every diagnostic out of it
                // is rebased onto `value_start_pos`.
                let sub_tokens =
                    lex(&raw).map_err(|err| ParseError::from(err).rebased(value_start_pos))?;
                let mut p = Parser::new(&raw, &sub_tokens);
                let value = p
                    .parse_value()
                    .map_err(|err| err.rebased(value_start_pos))?;
                // `@intended_targets [..] junk` is refused at `junk`.
                if let Some(trailing) = p.peek().filter(|t| t.kind != TokenKind::Newline) {
                    return Err(ParseError::Syntax {
                        position: trailing.position.rebased(value_start_pos),
                        message: "@intended_targets has trailing tokens after the list".into(),
                    });
                }
                // Sub-parse spans count from the start of `raw`, so the position is the
                // sub-token's, rebased.
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

    /// Parse an indented body, splitting `requires` lines out of it.
    ///
    /// `policy` is applied here rather than by the caller, because a refusal needs the offending
    /// line's position.
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

    /// Read a `requires` line at the current position, if one stands there
    /// and `policy` lets it.
    ///
    /// `Ok(None)` means the caller reads the line as an ordinary body item: the word is absent, or,
    /// under a refusing policy, the line declares no floor, since there `requires` is an ordinary
    /// identifier. Deciding that needs the whole expression, so the line is read and then rewound;
    /// [`Self::read_requires_line`] opens no nesting level, so `pos` and `raw_values` are all
    /// there is to restore.
    ///
    /// The decision looks at the expression alone, so a floor with an indented line under it is
    /// still refused as a floor.
    fn requires_line_here(
        &mut self,
        policy: RequiresPolicy,
    ) -> Result<Option<MemberRequires>, ParseError> {
        if !self.peek_is_ident(REQUIRES) {
            return Ok(None);
        }
        let position = self.position();
        let mark = self.pos;
        // A rewound line gives back its raw value, so an unlexable stretch in it fails again.
        let raw_mark = self.raw_values.len();
        let (raw, span) = self.read_requires_line()?;
        match policy.refusal() {
            None => self.finish_requires_line(position, raw, span).map(Some),
            // Otherwise the author gets `unexpected `>=`` three tokens along, which names the
            // token rather than the mistake.
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

    /// Consume every token up to the end of the line and return their index
    /// range, the source text they cover (trimmed), and the byte the text
    /// ends at — the start of the first token when there is none.
    ///
    /// The text is sliced out of the source rather than rebuilt from the
    /// tokens, so what the lexer split it into does not matter, and a
    /// [`TokenKind::Unlexed`] stretch is part of it like any other token.
    /// Whether that stretch is then a failure is the caller's to say, by
    /// recording the range in `raw_values` or not.
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

    /// Read one `requires <expression>` line up to its line break, and
    /// return the expression and the line's span.
    ///
    /// The expression is taken verbatim to end of line, as [`Self::parse_header`] takes
    /// `@requires`'s, so the two share one `parse_requirement` and labels the lexer splits
    /// (`24w14a`, `1.21.4-rc1`) stay out of the grammar's way.
    fn read_requires_line(&mut self) -> Result<(String, crate::error::Span), ParseError> {
        let start_byte = self.current_byte();
        self.advance();
        let (value_tokens, raw, value_end_byte) = self.rest_of_line();
        self.raw_values.push(value_tokens);
        self.expect_newline()?;
        Ok((raw, start_byte..value_end_byte))
    }

    /// Accept a `requires` line [`Self::read_requires_line`] has read, in
    /// a body that carries floors.
    ///
    /// `keyword_position` is the keyword's, because an empty expression has nothing else to point
    /// at.
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

    /// `policy` is the enclosing body's, so a member's own children get
    /// the refusal that names the repair for the item they are in.
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
        // A body recurses like a list or an expression, so it goes through `nested`: its O(n²)
        // source cost is no bound, since 400 levels is a third of a megabyte. A member's
        // children are members, and the part's floor is read one level up.
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
        while !self.peek_is(&TokenKind::RBrace) && !self.at_eof() {
            let pattern_position = self.position();
            let row_start_byte = self.current_byte();
            let pattern = self.expect_truth_pattern()?;
            let inputs_lex = pattern.text;
            // Checked here, where the failure has a position, rather than by each pass that
            // cares. What one row cannot see (no rows, repeated patterns, missing combinations)
            // is `check::truth`'s, which is why each row keeps a span.
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
                // `->` is lexed greedily, so a final `-` before it is lost and the pattern comes
                // back exactly one bit short.
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
                // Before the output is consumed, so a refusal points at it.
                let output_position = self.position();
                let out_lex = self.expect_int_lexeme()?;
                match out_lex.as_str() {
                    "0" => Some(false),
                    "1" => Some(true),
                    other => {
                        return Err(ParseError::Syntax {
                            position: output_position,
                            message: format!(
                                "truth-table output must be `0`, `1`, or `-`, got `{other}`"
                            ),
                        });
                    }
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

    /// Refuse a boolean tree deeper than [`MAX_EXPR_DEPTH`], whose doc says why that differs from
    /// the bound [`Self::nested`] keeps.
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

    /// A `not` chain and a parenthesised sub-expression both pass through [`Self::nested`].
    /// Parentheses build no node, so they cost descent but no tree depth; `not` costs both.
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
        // `and` and `or` are never signal names; read as one, a missing operand would surface
        // downstream as an unbound signal.
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

    /// Is the next token the bare identifier `name`? Unlike [`Self::match_keyword`], consumes
    /// nothing.
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

    /// The refusal for a key that is really the `x` of a size literal with
    /// no height, as in `size=9x`, or `None` when `key` is not one.
    ///
    /// The lexer builds a size only when a digit follows the `x`, so `9x` arrives as an integer and
    /// an `x` read as the next key. Only forms whose every argument is `key=value` reach here
    /// unconditionally (an item header, a `[…]` list, a `theme` binding); a member command asks
    /// [`Self::is_at_key_eq`] first, so there `window size=9x` parses and the checks report it.
    ///
    /// Where it does arrive, the argument has failed anyway, and ``expected `=` `` would name the
    /// parser's reading rather than the author's literal. The `x` is the size's when it touches the
    /// integer before it, whichever key that was given to, since digits-`x`-digits reads as nothing
    /// but a size. The caller has seen that no `=` follows `key`, so `size=9x=2` is a `size=9` and
    /// an `x=2`.
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

    /// Read an `@` token value — `@oak_planks`, `@floor.wood`, or a
    /// canonical token with its state literal, `@oak_log[axis=x]` — whose
    /// `@` starts at `start_byte`.
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
        // Only a `[` touching an undotted name is a state literal: a dotted token is abstract and
        // names no block to have a state, and a `[` after a space is the caller's.
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

    /// Read a block-state literal, `[key=value,…]`, after the `@` token
    /// named `token`, and return it as the text it folds into the token:
    /// `[axis=x]`, with the spaces the source may hold between its parts
    /// left out.
    ///
    /// The one place a malformed literal is refused, because `block_array::material` reads the
    /// folded text back leniently. It is Minecraft's block-state syntax rather than a Cairn list,
    /// so the comma between pairs is required. Whether the block has those properties and values
    /// is the target registry's question.
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
        // A set, because the duplicate check runs once per property of a literal of any length.
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
    ///
    /// The lexer ends every stream with a `Newline`, so `end of input` answers only for a stream
    /// that breaks that rule.
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
                // Here digits must be a number, so here the `i64` ceiling applies; see
                // `TokenKind::Int`.
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
    /// For a name the AST records, since rebuilding it from the item's span would assume one space
    /// after the keyword, which `def   hut` does not have.
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

    /// The pattern left of a truth row's `->`, reassembled from the
    /// tokens the lexer split it into.
    ///
    /// A pattern holds `0`, `1` or `-` per input signal (`spec/syntax` "Lexical"), but a run of
    /// digits lexes as one `Int` and each `-` as a `Minus`, so `0-1` is three tokens. They are one
    /// pattern exactly when nothing separates their byte spans: `0- 1` is a too-narrow pattern
    /// and a stray `1`, which the width check reports. The last dash of `--->` goes into the
    /// arrow, so `11--> 0` and `11- -> 0` are the same row.
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
            return Err(ParseError::Syntax {
                position,
                message: match self.peek() {
                    Some(token) => {
                        format!("expected a truth-table input pattern, got {}", token.kind)
                    }
                    None => "expected a truth-table input pattern, got end of input".to_owned(),
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

    /// Byte offset where the next un-consumed token begins, or `source.len()`
    /// at EOF. Anchors the start of a node's [`Span`] before any token is
    /// consumed in its parser arm.
    ///
    /// [`Span`]: crate::error::Span
    fn current_byte(&self) -> usize {
        self.peek().map_or(self.source.len(), |t| t.span.start)
    }

    /// Byte offset where the most-recently-consumed token ended. Used to
    /// close a [`Span`] right after the last meaningful token for a node;
    /// callers capture this *before* `expect_newline()` so trailing layout
    /// tokens stay outside the node's span.
    ///
    /// [`Span`]: crate::error::Span
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
