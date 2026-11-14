//! Cairn surface-syntax parser: line-based, with indent-driven nesting.
//!
//! A body's `requires` lines leave the body, lifted onto the `def` or `theme` that carries them.

use crate::ast::{
    Arg, DottedRef, Expr, Header, Item, MemberRequires, Module, NamedOperand, RawRequirement,
    RawVersion, Statement, ThemeRule, TruthRow, Value, ValueKind,
};
use crate::calver::future_version_note;
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::edition::Edition;
use crate::error::{IntContext, ParseError, Position};
use crate::lex::{Lexed, Token, TokenKind, lex, lex_deferring};
use crate::lines::LineStarts;
use crate::logic_library::{CallSignature, LogicPrimitive, Notation, StageCount, infix_rank_above};
use crate::prose::{and_list, or_list};
use crate::suggest::{did_you_mean, nearest_match};
use crate::version::parse_requirement;
use indexmap::IndexSet;

/// The word that introduces a version floor, after `@` or at the start of a body line.
const REQUIRES: &str = "requires";

/// The word after `@` that opens an edition guard inside a body.
const EDITION: &str = "edition";

/// The refusal for an `@edition` guard written outside every body.
const GUARD_OUTSIDE_A_BODY: &str = "an `@edition` guard stands inside a `struct`, `def` or \
     `site` body, around the raw members and Tier 0 parts it guards: the items of a file are \
     edition-neutral, so move the guard into the body that places those blocks";

/// The refusal for an `@edition` guard written in a `theme` body.
const GUARD_IN_A_THEME: &str = "an `@edition` guard stands inside a `struct`, `def` or `site` \
     body, never in a `theme`: a theme is picked per edition by its name, so write this edition's \
     slots in a `theme NAME_java` or a `theme NAME_bedrock`";

/// The words a refusal inside a call names it by.
struct CallWording {
    /// The primitive's name.
    callee: &'static str,
    /// Its operands, in the order the library lists them.
    names: &'static [&'static str],
    /// The call written out, in backticks: `` `mux(sel=…, a=…, b=…)` ``.
    shape: String,
    /// Every operand, in backticks, as a prose list.
    every_name: String,
}

impl CallWording {
    fn new(signature: CallSignature) -> Self {
        let names = signature.operands();
        Self {
            callee: signature.callee(),
            names,
            shape: format!("`{}`", signature.template()),
            every_name: and_list(&quoted(names, "")).unwrap_or_default(),
        }
    }
}

/// Each of `names` with `suffix`, in backticks.
fn quoted(names: &[&str], suffix: &str) -> Vec<String> {
    names
        .iter()
        .map(|name| format!("`{name}{suffix}`"))
        .collect()
}

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
    /// The lines under an `@edition` guard inside a `def`.
    GuardOfAPart,
    /// The lines under an `@edition` guard inside a `struct` or `site`.
    GuardOfTheBuild,
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
            Self::GuardOfAPart => Some(
                "an `@edition` guard may not hold `requires version>=X`: the floor belongs to the \
                 whole part on every edition, so it goes at the `def` body's own level",
            ),
            Self::GuardOfTheBuild => Some(
                "an `@edition` guard may not hold `requires version>=X`, and neither may the \
                 `struct` or `site` it is in: the floor is the whole file's and is written \
                 `@requires version>=X` at the top of the file",
            ),
        }
    }

    /// The policy for the indented children of a member in this body.
    fn for_children(self) -> Self {
        match self {
            Self::Accepted | Self::MemberOfAPart | Self::GuardOfAPart => Self::MemberOfAPart,
            Self::NotAPart | Self::MemberOfTheBuild | Self::GuardOfTheBuild => {
                Self::MemberOfTheBuild
            }
        }
    }

    /// The policy for the lines under an `@edition` guard in this body.
    fn for_guard(self) -> Self {
        match self {
            Self::Accepted | Self::MemberOfAPart | Self::GuardOfAPart => Self::GuardOfAPart,
            Self::NotAPart | Self::MemberOfTheBuild | Self::GuardOfTheBuild => {
                Self::GuardOfTheBuild
            }
        }
    }
}

/// The directives a file opens with, each after `@`.
const FILE_DIRECTIVES: [&str; 3] = ["cairn", REQUIRES, "intended_targets"];

/// The refusal for `@name` written inside a body whose policy is `policy`.
fn not_a_guard(name: &str, policy: RequiresPolicy) -> String {
    if name == REQUIRES && policy == RequiresPolicy::Accepted {
        "a `def` or `theme` body's floor is written `requires version>=X`, without the `@`: \
         inside a body, `@` opens only an `@edition` guard"
            .into()
    } else if FILE_DIRECTIVES.contains(&name) {
        format!(
            "`@{name}` is a file directive, which goes at the top of the file: inside a body, `@` \
             opens only an `@edition` guard"
        )
    } else {
        format!("unknown directive `@{name}`: inside a body, `@` opens only an `@edition` guard")
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

/// Deepest boolean-expression tree the parser hands back: infix chains parse iteratively, but
/// every consumer walks the tree recursively.
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
    /// Whether the body being parsed is an `@edition` guard's own.
    in_guard: bool,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, tokens: &'a [Token]) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
            depth: 0,
            raw_values: Vec::new(),
            in_guard: false,
        }
    }

    /// Run `descend` one level deeper, refusing past [`MAX_NESTING_DEPTH`]. Every recursive step
    /// the source can repeat, in a value, expression or body, must go through here.
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
            EDITION => {
                return Err(ParseError::Syntax {
                    position,
                    message: GUARD_OUTSIDE_A_BODY.into(),
                });
            }
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
        if self.peek_is(&TokenKind::At) {
            self.advance();
            return Err(ParseError::Syntax {
                position,
                message: if self.peek_is_ident(EDITION) {
                    GUARD_OUTSIDE_A_BODY.into()
                } else {
                    "a file directive goes at the top of the file, before its first item".into()
                },
            });
        }
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
        let (_, positional) = self.parse_line_tail(Some("a `theme` header"))?;
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
                if self.peek_is(&TokenKind::At) {
                    let position = self.position();
                    self.advance();
                    let name = self.expect_ident()?;
                    return Err(ParseError::Syntax {
                        position,
                        message: if name == EDITION {
                            GUARD_IN_A_THEME.into()
                        } else {
                            not_a_guard(&name, RequiresPolicy::Accepted)
                        },
                    });
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
            positional,
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
            if self.at_block_id() {
                return Err(self.block_id_in_a_slot_row());
            }
            let value = self.parse_value()?;
            let (_, positional) = self.parse_line_tail(Some("a `slot` row"))?;
            let span = start_byte..self.last_byte();
            self.expect_newline()?;
            return Ok(ThemeRule::Slot {
                slot,
                value,
                positional,
                span,
            });
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
        let (bindings, positional) = self.parse_line_tail(None)?;
        let span = start_byte..self.last_byte();
        self.expect_newline()?;
        Ok(ThemeRule::Selector {
            keyword: kw,
            attrs,
            bindings,
            positional,
            span,
        })
    }

    fn parse_def_item(&mut self, start_byte: usize) -> Result<Item, ParseError> {
        let (name, name_span) = self.expect_ident_spanned()?;
        let (args, positional) = self.parse_line_tail(None)?;
        self.consume_optional_colon();
        self.expect_newline()?;
        let (requires, body) = self.parse_optional_command_body(RequiresPolicy::Accepted)?;
        let span = start_byte..self.last_content_byte();
        Ok(Item::Def {
            name,
            name_span,
            args,
            positional,
            requires,
            body,
            span,
        })
    }

    fn parse_site_item(&mut self, start_byte: usize) -> Result<Item, ParseError> {
        let (name, name_span) = self.expect_ident_spanned()?;
        let (_, positional) = self.parse_line_tail(Some("a `site` header"))?;
        self.consume_optional_colon();
        self.expect_newline()?;
        let (_, body) = self.parse_optional_command_body(RequiresPolicy::NotAPart)?;
        let span = start_byte..self.last_content_byte();
        Ok(Item::Site {
            name,
            name_span,
            positional,
            body,
            span,
        })
    }

    fn parse_struct_item(&mut self, start_byte: usize) -> Result<Item, ParseError> {
        let (name, name_span) = self.expect_ident_spanned()?;
        let (args, positional) = self.parse_line_tail(None)?;
        self.consume_optional_colon();
        self.expect_newline()?;
        let (_, body) = self.parse_optional_command_body(RequiresPolicy::NotAPart)?;
        let span = start_byte..self.last_content_byte();
        Ok(Item::Struct {
            name,
            name_span,
            args,
            positional,
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
        if self.peek_is(&TokenKind::At) {
            return self.parse_edition_guard(policy);
        }
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
        let in_guard = std::mem::replace(&mut self.in_guard, false);
        let children = self.nested(|p| p.parse_optional_command_body(policy.for_children()));
        self.in_guard = in_guard;
        let (_, children) = children?;
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

    /// Parse an `@edition` guard and the lines indented under it; `policy` is the enclosing body's.
    fn parse_edition_guard(&mut self, policy: RequiresPolicy) -> Result<Statement, ParseError> {
        let position = self.position();
        let start_byte = self.current_byte();
        self.advance();
        let name = self.expect_ident()?;
        if name != EDITION {
            return Err(ParseError::Syntax {
                position,
                message: not_a_guard(&name, policy),
            });
        }
        if self.in_guard {
            return Err(ParseError::Syntax {
                position,
                message: "an `@edition` guard does not nest: the guard this one stands in already \
                          picks the edition, so write its lines there or dedent this guard out"
                    .into(),
            });
        }
        let edition_position = self.position();
        let edition = match self.peek().map(|t| &t.kind) {
            Some(TokenKind::Ident(word)) => {
                word.parse::<Edition>().map_err(|_| ParseError::Syntax {
                    position: edition_position,
                    message: format!(
                        "`{word}` is not an edition: an `@edition` guard names `java` or `bedrock`"
                    ),
                })?
            }
            _ => {
                return Err(ParseError::Syntax {
                    position: edition_position,
                    message: "an `@edition` guard names the edition whose build keeps its lines: \
                              `@edition java` or `@edition bedrock`"
                        .into(),
                });
            }
        };
        self.advance();
        let span = start_byte..self.last_byte();
        if let Some(next) = self.peek().filter(|t| t.kind != TokenKind::Newline) {
            return Err(ParseError::Syntax {
                position: next.position,
                message: if next.kind == TokenKind::LBrace {
                    "an `@edition` guard holds its lines indented under it, not in braces".into()
                } else {
                    "an `@edition` guard takes nothing after its edition: the lines it guards go \
                     on the lines below it, one indent deeper"
                        .into()
                },
            });
        }
        self.expect_newline()?;
        if !self.peek_is(&TokenKind::Indent) {
            return Err(ParseError::Syntax {
                position,
                message: format!(
                    "nothing is indented under this `@edition {edition}` guard: the lines it \
                     guards go on the lines below it, one indent deeper"
                ),
            });
        }
        let in_guard = std::mem::replace(&mut self.in_guard, true);
        let children = self.nested(|p| p.parse_optional_command_body(policy.for_guard()));
        self.in_guard = in_guard;
        let (_, children) = children?;
        Ok(Statement::EditionGuard {
            edition,
            children,
            span,
        })
    }

    fn parse_logic_command(&mut self, start_byte: usize) -> Result<Statement, ParseError> {
        let lhs = self.parse_dotted_ref()?;
        // With its `=` dropped, the line still reads the expression it would have bound.
        let binds = !matches!(
            self.peek().map(|t| &t.kind),
            Some(TokenKind::Ident(_) | TokenKind::LParen)
        );
        if binds {
            self.expect(&TokenKind::Eq)?;
        }
        let rhs = self.parse_expr()?;
        let span = start_byte..self.last_byte();
        self.expect_operand_end(&[TokenKind::Newline])?;
        Ok(Statement::Logic {
            lhs,
            rhs,
            binds,
            span,
        })
    }

    fn parse_assert_command(&mut self, start_byte: usize) -> Result<Statement, ParseError> {
        // A misspelt form is wrong in the word after `assert`, so point there.
        let position = self.position();
        let head = self.expect_ident()?;
        match head.as_str() {
            "truth" => self.parse_assert_truth(start_byte),
            "always" => self.parse_assert_always(start_byte),
            "latency" => self.parse_assert_latency(start_byte),
            other => Err(ParseError::Syntax {
                position,
                message: format!(
                    "expected `truth`, `always` or `latency` after `assert`, got `{other}`"
                ),
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
            let output = match self.peek().map(|t| &t.kind) {
                // The arrow is already taken, so this `-` is the output, not the arrow's dash.
                Some(TokenKind::Minus) => None,
                Some(TokenKind::Int { lexeme }) if lexeme == "0" => Some(false),
                Some(TokenKind::Int { lexeme }) if lexeme == "1" => Some(true),
                found => {
                    let found = match found {
                        Some(TokenKind::Int { lexeme }) => format!("`{lexeme}`"),
                        _ => self.next_token_text(),
                    };
                    return Err(self.syntax_here(&format!(
                        "truth-table output must be `0`, `1`, or `-`, got {found}"
                    )));
                }
            };
            self.advance();
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
        let (_, positional) = self.parse_line_tail(Some("an `assert` line"))?;
        self.expect_newline()?;
        Ok(Statement::AssertTruth {
            inputs,
            output,
            rows,
            positional,
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
        let (_, positional) = self.parse_line_tail(Some("an `assert` line"))?;
        self.expect_newline()?;
        Ok(Statement::AssertAlways {
            antecedent,
            consequent,
            within,
            positional,
            span,
        })
    }

    fn parse_assert_latency(&mut self, start_byte: usize) -> Result<Statement, ParseError> {
        self.expect(&TokenKind::LParen)?;
        let from = self.parse_dotted_ref()?;
        self.expect(&TokenKind::Arrow)?;
        let to = self.parse_dotted_ref()?;
        self.expect(&TokenKind::RParen)?;
        if !self.peek_is(&TokenKind::LessEq) {
            let found = self.next_token_text();
            return Err(self.syntax_here(&format!(
                "expected `<=` after `latency(...)`, got {found}: a latency assertion bounds its \
                 slowest path from above, as `<= N` ticks"
            )));
        }
        self.advance();
        let bound_pos = self.position();
        let bound_lex = self.expect_int_lexeme()?;
        let bound: u32 =
            bound_lex
                .parse()
                .map_err(|err: std::num::ParseIntError| ParseError::InvalidInt {
                    position: bound_pos,
                    context: IntContext::LatencyBound,
                    lexeme: bound_lex.clone(),
                    kind: *err.kind(),
                })?;
        let span = start_byte..self.last_byte();
        let (_, positional) = self.parse_line_tail(Some("an `assert` line"))?;
        self.expect_newline()?;
        Ok(Statement::AssertLatency {
            from,
            to,
            bound,
            positional,
            span,
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        Ok(self.parse_expr_at(infix_rank_above(None))?.0)
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

    /// The expression whose infix primitives rank `rank` or higher, `None` being the prefix level
    /// above them all, and its tree depth, so an iteratively parsed chain pays for its shape.
    fn parse_expr_at(&mut self, rank: Option<u8>) -> Result<(Expr, usize), ParseError> {
        let Some(rank) = rank else {
            return self.parse_expr_prefix();
        };
        let tighter = infix_rank_above(Some(rank));
        let (mut lhs, mut depth) = self.parse_expr_at(tighter)?;
        while let Some(primitive) = self.match_infix(rank) {
            let (rhs, rhs_depth) = self.parse_expr_at(tighter)?;
            depth = depth.max(rhs_depth) + 1;
            self.charge_expr_depth(depth)?;
            lhs = Expr::Infix {
                primitive,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok((lhs, depth))
    }

    /// A prefix primitive or a call costs descent and tree depth; grouping parentheses cost descent
    /// alone.
    fn parse_expr_prefix(&mut self) -> Result<(Expr, usize), ParseError> {
        if let Some(primitive) = self
            .peek_primitive()
            .filter(|primitive| primitive.notation() == Notation::Prefix)
        {
            self.advance();
            let (operand, operand_depth) = self.nested(Self::parse_expr_prefix)?;
            let depth = operand_depth + 1;
            self.charge_expr_depth(depth)?;
            let operand = Box::new(operand);
            return Ok((Expr::Prefix { primitive, operand }, depth));
        }
        if let Some((primitive, signature)) = self
            .peek_primitive()
            .and_then(|primitive| Some((primitive, primitive.call_signature()?)))
        {
            self.advance();
            let (operands, operands_depth) =
                self.nested(|p| p.parse_call_operands(&CallWording::new(signature)))?;
            let depth = operands_depth + 1;
            self.charge_expr_depth(depth)?;
            return Ok((
                Expr::Call {
                    primitive,
                    operands,
                },
                depth,
            ));
        }
        if let Some((primitive, counted)) = self
            .peek_primitive()
            .and_then(|primitive| Some((primitive, primitive.applied_counted()?)))
        {
            self.advance();
            let (operand, stages, operand_depth) =
                self.nested(|p| p.parse_applied(primitive, counted))?;
            let depth = operand_depth + 1;
            self.charge_expr_depth(depth)?;
            let operand = Box::new(operand);
            return Ok((
                Expr::Apply {
                    primitive,
                    operand,
                    stages,
                },
                depth,
            ));
        }
        if self.peek_is(&TokenKind::LParen) {
            self.advance();
            let inner = self.nested(|p| p.parse_expr_at(infix_rank_above(None)))?;
            self.expect_operand_end(&[TokenKind::RParen])?;
            return Ok(inner);
        }
        // An infix primitive is never a signal name, so a missing operand is reported here.
        if let Some(primitive) = self.peek_primitive()
            && primitive.infix_rank().is_some()
        {
            // The token before is a primitive, `=` or `(`.
            let after = match self.pos.checked_sub(1).map(|i| &self.tokens[i].kind) {
                Some(TokenKind::Ident(previous)) => format!("`{previous}`"),
                Some(previous) => previous.to_string(),
                None => "the start of the line".to_owned(),
            };
            return Err(ParseError::Syntax {
                position: self.position(),
                message: format!(
                    "expected a signal after {after}, got the infix primitive `{}`",
                    primitive.name(),
                ),
            });
        }
        let dotted = self.parse_dotted_ref()?;
        Ok((Expr::Ref(dotted), 1))
    }

    /// The parenthesised operands of `call`, each of its names once and in any order, returned in
    /// the order of its names, and the depth of the deepest.
    fn parse_call_operands(
        &mut self,
        call: &CallWording,
    ) -> Result<(Vec<NamedOperand>, usize), ParseError> {
        let names = call.names;
        if !self.peek_is(&TokenKind::LParen) {
            let (callee, shape) = (call.callee, &call.shape);
            let found = self.found_here();
            return Err(self.syntax_here(&format!(
                "expected `(` after `{callee}`, got {found}: `{callee}` is called with its \
                 operands named, as in {shape}"
            )));
        }
        self.advance();
        let mut values: Vec<Option<Expr>> =
            std::iter::repeat_with(|| None).take(names.len()).collect();
        let mut depth = 0;
        loop {
            let unnamed: Vec<&str> = names
                .iter()
                .zip(&values)
                .filter(|(_, value)| value.is_none())
                .map(|(name, _)| *name)
                .collect();
            let index = self.parse_operand_name(call, &unnamed)?;
            self.expect_operand_value(call, names[index])?;
            let (value, value_depth) = self.parse_expr_at(infix_rank_above(None))?;
            values[index] = Some(value);
            depth = depth.max(value_depth);
            let missing: Vec<&str> = unnamed
                .into_iter()
                .filter(|left| *left != names[index])
                .collect();
            if self.expect_operand_separator(call, &missing)? {
                break;
            }
        }
        let operands: Option<Vec<NamedOperand>> = names
            .iter()
            .copied()
            .zip(values)
            .map(|(name, value)| {
                Some(NamedOperand {
                    name,
                    value: value?,
                })
            })
            .collect();
        let operands = operands.expect("the call closes only once every operand is named");
        Ok((operands, depth))
    }

    /// The parenthesised operand of the applied `primitive` and, when `counted`, the stage count
    /// after it, with the operand's depth.
    fn parse_applied(
        &mut self,
        primitive: LogicPrimitive,
        counted: bool,
    ) -> Result<(Expr, Option<StageCount>, usize), ParseError> {
        let name = primitive.name();
        let shape = primitive
            .parenthesised_template()
            .expect("an applied primitive is written with parentheses");
        let takes = if counted {
            "its operand and a stage count"
        } else {
            "its one operand"
        };
        if !self.peek_is(&TokenKind::LParen) {
            let found = self.found_here();
            return Err(self.syntax_here(&format!(
                "expected `(` after `{name}`, got {found}: `{name}` is applied to {takes}, as in \
                 `{shape}`"
            )));
        }
        self.advance();
        if self.is_at_key_eq()
            && let Some(TokenKind::Ident(key)) = self.peek().map(|token| &token.kind)
        {
            return Err(self.syntax_here(&format!(
                "`{name}` takes its operand by position, not as `{key}=`: write it as in `{shape}`"
            )));
        }
        if self.peek_is(&TokenKind::RParen) {
            return Err(self.syntax_here(&format!(
                "expected an operand, got `)`: `{name}` is applied to {takes}, as in `{shape}`"
            )));
        }
        let (operand, depth) = self.parse_expr_at(infix_rank_above(None))?;
        if !counted {
            if self.peek_is(&TokenKind::Comma) {
                return Err(self.syntax_here(&format!(
                    "expected `)`, got `,`: `{name}` counts no stages, so it is applied to its one \
                     operand alone, as in `{shape}`"
                )));
            }
            self.expect_operand_end(&[TokenKind::RParen])?;
            return Ok((operand, None, depth));
        }
        if self.peek_is(&TokenKind::RParen) {
            return Err(self.syntax_here(&format!(
                "expected `,` and a stage count, got `)`: `{name}` is applied to {takes}, as in \
                 `{shape}`"
            )));
        }
        if let Some(TokenKind::Int { .. }) = self.peek().map(|token| &token.kind) {
            let found = self.found_here();
            return Err(self.syntax_here(&format!(
                "expected `,` before the stage count, got {found}: `{name}` is applied to \
                 {takes}, as in `{shape}`"
            )));
        }
        self.expect_operand_end(&[TokenKind::Comma])?;
        let position = self.position();
        let Some(TokenKind::Int { lexeme, .. }) = self.peek().map(|token| token.kind.clone())
        else {
            let found = self.found_here();
            return Err(self.syntax_here(&format!(
                "expected a stage count, a whole number of at least 1, got {found}: `{name}` is \
                 applied to {takes}, as in `{shape}`"
            )));
        };
        let count: u32 =
            lexeme
                .parse()
                .map_err(|err: std::num::ParseIntError| ParseError::InvalidInt {
                    position,
                    context: IntContext::StageCount,
                    lexeme: lexeme.clone(),
                    kind: *err.kind(),
                })?;
        let Some(stages) = StageCount::new(count) else {
            return Err(self.syntax_here(&format!(
                "expected a stage count of at least 1, got `{lexeme}`: `{name}` with no stage \
                 builds nothing, so write a count of 1 or more, as in `{shape}`"
            )));
        };
        self.advance();
        if !self.peek_is(&TokenKind::RParen) {
            let found = self.found_here();
            let why = match self.peek().map(|token| &token.kind) {
                Some(TokenKind::Comma) => format!("`{name}` takes one stage count"),
                Some(TokenKind::Dot) => "a stage count is a whole number of stages".to_owned(),
                _ => format!("`{name}` is applied to {takes}"),
            };
            return Err(self.syntax_here(&format!(
                "expected `)`, got {found}: {why}, as in `{shape}`"
            )));
        }
        self.advance();
        Ok((operand, Some(stages), depth))
    }

    /// Refuse what follows `name=` unless an operand can start there.
    fn expect_operand_value(&self, call: &CallWording, name: &str) -> Result<(), ParseError> {
        let found = match self.peek().map(|token| &token.kind) {
            Some(TokenKind::LParen) => return Ok(()),
            Some(TokenKind::Ident(word)) => match LogicPrimitive::from_name(word) {
                Some(primitive) if primitive.infix_rank().is_some() => {
                    format!("the infix primitive `{word}`")
                }
                _ => return Ok(()),
            },
            _ => self.found_here(),
        };
        let (callee, shape) = (call.callee, &call.shape);
        Err(self.syntax_here(&format!(
            "expected a signal or expression after `{name}=`, got {found}: each operand of \
             `{callee}` takes a value, as in {shape}"
        )))
    }

    /// Take `name=` for one of `unnamed`, returning its index in the call's names.
    fn parse_operand_name(
        &mut self,
        call: &CallWording,
        unnamed: &[&str],
    ) -> Result<usize, ParseError> {
        let (callee, shape, every_name) = (call.callee, &call.shape, &call.every_name);
        if !self.is_at_key_eq() {
            let expected = or_list(&quoted(unnamed, "=")).unwrap_or_default();
            let found = self.found_here();
            return Err(self.syntax_here(&format!(
                "expected {expected}, got {found}: `{callee}` takes each operand by name, as in \
                 {shape}"
            )));
        }
        let position = self.position();
        let name = self.expect_ident()?;
        let Some(index) = call.names.iter().position(|known| *known == name) else {
            let hint = nearest_match(&name, unnamed.iter().copied())
                .map(|near| format!(" ({})", did_you_mean(near)))
                .unwrap_or_default();
            return Err(ParseError::Syntax {
                position,
                message: format!("`{callee}` has no operand `{name}`{hint}: it takes {every_name}"),
            });
        };
        if !unnamed.contains(&name.as_str()) {
            return Err(ParseError::Syntax {
                position,
                message: format!(
                    "`{callee}` names `{name}` twice: it takes {every_name}, each named once"
                ),
            });
        }
        self.advance();
        Ok(index)
    }

    /// Take what follows an operand of the call, `)` once none is `missing` and `,` before
    /// another, returning whether the call closed.
    fn expect_operand_separator(
        &mut self,
        call: &CallWording,
        missing: &[&str],
    ) -> Result<bool, ParseError> {
        let (callee, shape, every_name) = (call.callee, &call.shape, &call.every_name);
        if missing.is_empty() {
            if self.peek_is(&TokenKind::Comma) || self.is_at_key_eq() {
                let found = self.found_here();
                return Err(self.syntax_here(&format!(
                    "expected `)`, got {found}: `{callee}` takes {every_name}, and every one is \
                     named, so the call closes here"
                )));
            }
            self.expect_operand_end(&[TokenKind::RParen])?;
            return Ok(true);
        }
        if self.is_at_key_eq() {
            let found = self.found_here();
            return Err(self.syntax_here(&format!(
                "expected `,`, got {found}: the named operands of `{callee}` are separated by \
                 commas, as in {shape}"
            )));
        }
        if self.peek_is(&TokenKind::RParen) {
            let operands = if missing.len() == 1 {
                "operand"
            } else {
                "operands"
            };
            let missing = and_list(&quoted(missing, "")).unwrap_or_default();
            return Err(self.syntax_here(&format!(
                "`{callee}` is missing its {operands} {missing}: it takes {every_name}, each \
                 named once"
            )));
        }
        self.expect_operand_end(&[TokenKind::Comma, TokenKind::RParen])?;
        Ok(false)
    }

    /// The next token as a diagnostic names it, or the end of input.
    fn found_here(&self) -> String {
        self.peek()
            .map_or_else(|| "end of input".to_owned(), |token| token.kind.to_string())
    }

    /// The primitive of the built-in logic library the next token names, consuming nothing.
    fn peek_primitive(&self) -> Option<LogicPrimitive> {
        match &self.peek()?.kind {
            TokenKind::Ident(word) => LogicPrimitive::from_name(word),
            _ => None,
        }
    }

    /// Take the next token when it names an infix primitive of `rank`.
    fn match_infix(&mut self, rank: u8) -> Option<LogicPrimitive> {
        let primitive = self.peek_primitive()?;
        if primitive.infix_rank() != Some(rank) {
            return None;
        }
        self.advance();
        Some(primitive)
    }

    /// Take one of `closers` after a complete operand, or refuse what stands there with the infix
    /// primitives and closers that could have stood there instead.
    fn expect_operand_end(&mut self, closers: &[TokenKind]) -> Result<(), ParseError> {
        if closers.contains(&TokenKind::Newline) && self.at_eof() {
            return Ok(());
        }
        if closers.iter().any(|closer| self.peek_is(closer)) {
            self.advance();
            return Ok(());
        }
        let mut expected: Vec<String> = LogicPrimitive::ALL
            .iter()
            .filter(|primitive| primitive.infix_rank().is_some())
            .map(|primitive| format!("`{}`", primitive.name()))
            .collect();
        expected.extend(closers.iter().map(ToString::to_string));
        let expected = or_list(&expected).unwrap_or_default();
        let found = self.peek().map(|token| token.kind.clone());
        let why = self.why_not_after_operand(found.as_ref());
        let found = found.map_or_else(|| "end of input".to_owned(), |kind| kind.to_string());
        Err(ParseError::Syntax {
            position: self.position(),
            message: format!("expected {expected}, got {found}{why}"),
        })
    }

    /// Why `found`, standing after a complete operand, cannot stand there, or nothing when the
    /// token says no more than itself.
    fn why_not_after_operand(&self, found: Option<&TokenKind>) -> String {
        let kind_at = |offset: isize| {
            self.pos
                .checked_add_signed(offset)
                .and_then(|i| self.tokens.get(i))
                .map(|token| &token.kind)
        };
        // The operand before is one bare word only when no `.` joins it to a word before it.
        let bare_word_before = match (kind_at(-1), kind_at(-2)) {
            (Some(TokenKind::Ident(word)), before) if before != Some(&TokenKind::Dot) => {
                Some(word.as_str()).filter(|word| LogicPrimitive::from_name(word).is_none())
            }
            _ => None,
        };
        let operand_follows = |offset: isize| {
            matches!(
                kind_at(offset),
                Some(TokenKind::Ident(_) | TokenKind::LParen)
            )
        };
        let notation = |word: &str| LogicPrimitive::from_name(word).map(LogicPrimitive::notation);
        let is_prefix = |word: &str| notation(word) == Some(Notation::Prefix);
        let is_call = |word: &str| {
            matches!(
                notation(word),
                Some(Notation::Call(_) | Notation::Apply { .. })
            )
        };
        let between = ": two operands need an infix primitive between them".to_owned();
        match (found, bare_word_before) {
            (Some(TokenKind::LParen), Some(word)) => {
                let callable: Vec<String> = LogicPrimitive::ALL
                    .iter()
                    .filter(|primitive| primitive.parenthesised_template().is_some())
                    .map(|primitive| format!("`{}`", primitive.name()))
                    .collect();
                let callable = or_list(&callable).unwrap_or_default();
                format!(
                    ": the built-in logic library has no primitive `{word}` to call or apply; \
                     those it calls or applies are {callable}"
                )
            }
            (Some(TokenKind::LParen), None) => between,
            (Some(TokenKind::Ident(word)), _) if is_prefix(word) && !operand_follows(1) => {
                format!(": `{word}` goes before its operand")
            }
            (Some(TokenKind::Ident(word)), _) if is_prefix(word) || is_call(word) => between,
            (Some(TokenKind::Ident(_)), Some(unknown)) => {
                format!(": the built-in logic library has no primitive `{unknown}`")
            }
            (Some(TokenKind::Ident(_)), None) if kind_at(1) == Some(&TokenKind::Dot) => between,
            (Some(TokenKind::Ident(word)), None) => {
                format!(": the built-in logic library has no primitive `{word}`")
            }
            _ => String::new(),
        }
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

    /// Read a line's `key=value` arguments and its bare values up to the first token no value
    /// starts with, such as a header's `:`. A line that takes no argument names itself in
    /// `no_arguments`.
    fn parse_line_tail(
        &mut self,
        no_arguments: Option<&str>,
    ) -> Result<(Vec<Arg>, Vec<Value>), ParseError> {
        let mut args = Vec::new();
        let mut positional = Vec::new();
        while self.at_value_start() {
            if !self.is_at_key_eq() {
                positional.push(self.parse_value()?);
            } else if let Some(line) = no_arguments {
                let position = self.position();
                let key = self.expect_ident()?;
                return Err(ParseError::Syntax {
                    position,
                    message: format!("{line} takes no `key=value` argument, got `{key}=`"),
                });
            } else {
                args.push(self.parse_arg()?);
            }
        }
        Ok((args, positional))
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
        let value = if self.at_coordinate() {
            self.parse_coordinate()?
        } else if self.at_block_id() {
            self.parse_block_id()?
        } else {
            let value = self.parse_value()?;
            if let ValueKind::Ident(namespace) = &value.kind {
                self.refuse_a_spaced_block_id(namespace)?;
            }
            value
        };
        let span = start_byte..self.last_byte();
        Ok(Arg { key, value, span })
    }

    /// Refuse a name and a path a space parts from the `:` between them, which no value is.
    fn refuse_a_spaced_block_id(&self, namespace: &str) -> Result<(), ParseError> {
        let kind = |offset: usize| self.tokens.get(self.pos + offset).map(|t| &t.kind);
        let (Some(TokenKind::Colon), Some(TokenKind::Ident(path))) = (kind(0), kind(1)) else {
            return Ok(());
        };
        Err(ParseError::Syntax {
            position: self.position(),
            message: format!(
                "a space parts `{namespace}` and `{path}` from the `:` between them; a block id \
                 is `namespace:path` with nothing around the `:`, as `{namespace}:{path}`"
            ),
        })
    }

    /// Whether the next tokens are an integer, a `,` and another component, or a `-` before the
    /// first: only a coordinate triple puts those after `=`, since a selector's `,` precedes a key.
    fn at_coordinate(&self) -> bool {
        let kind = |offset: usize| self.tokens.get(self.pos + offset).map(|t| &t.kind);
        let first = usize::from(matches!(kind(0), Some(TokenKind::Minus)));
        matches!(kind(first), Some(TokenKind::Int { .. }))
            && matches!(kind(first + 1), Some(TokenKind::Comma))
            && (first == 1 || matches!(kind(2), Some(TokenKind::Int { .. } | TokenKind::Minus)))
    }

    /// Read a coordinate triple `X,Y,Z`, three integers joined by `,` with nothing between.
    fn parse_coordinate(&mut self) -> Result<Value, ParseError> {
        let start_byte = self.current_byte();
        let start_position = self.position();
        let mut components: Vec<u64> = Vec::with_capacity(3);
        loop {
            let position = self.position();
            let Some(TokenKind::Int { lexeme }) = self.peek().map(|t| t.kind.clone()) else {
                return Err(self.coordinate_component_refusal(position));
            };
            let component = lexeme
                .parse::<i64>()
                .map_err(|err| ParseError::InvalidInt {
                    position,
                    context: IntContext::IntLiteral,
                    lexeme: lexeme.clone(),
                    kind: *err.kind(),
                })?;
            components.push(component.cast_unsigned());
            self.advance();
            if !self.continues_coordinate() {
                break;
            }
            let comma = &self.tokens[self.pos];
            let next = &self.tokens[self.pos + 1];
            if comma.span.start != self.last_byte() || next.span.start != comma.span.end {
                return Err(ParseError::Syntax {
                    position: comma.position,
                    message: "a coordinate triple is written with nothing between its \
                              components and the `,` joining them, as `4,3,2`"
                        .into(),
                });
            }
            self.advance();
        }
        if components.len() == 3 {
            let value = ValueKind::Coord {
                x: components[0],
                y: components[1],
                z: components[2],
            };
            return Ok(Value::new(value, start_byte..self.last_byte()));
        }
        let written = &self.source[start_byte..self.last_byte()];
        let count = match components.len() {
            2 => "two components",
            _ => "more than three components",
        };
        Err(ParseError::Syntax {
            position: start_position,
            message: format!(
                "the coordinate `{written}` has {count}; a coordinate triple is three, `x,y,z`, \
                 as `4,3,2`"
            ),
        })
    }

    /// Whether a `,` and another component follow the component just read.
    fn continues_coordinate(&self) -> bool {
        let kind = |offset: usize| self.tokens.get(self.pos + offset).map(|t| &t.kind);
        matches!(kind(0), Some(TokenKind::Comma))
            && matches!(
                kind(1),
                Some(TokenKind::Int { .. } | TokenKind::Minus | TokenKind::Size(..))
            )
    }

    /// The refusal for a coordinate component that is a `-` or a size rather than an integer.
    fn coordinate_component_refusal(&self, position: Position) -> ParseError {
        let token = &self.tokens[self.pos];
        let message = if let TokenKind::Size(..) = token.kind {
            let written = &self.source[token.span.clone()];
            let rest = &written[written.find('x').unwrap_or_default()..];
            format!(
                "a coordinate component runs into `{rest}`: `{written}` reads as a size, and a \
                 coordinate triple is three integers, `x,y,z`, as `4,3,2`"
            )
        } else {
            "a coordinate component is never negative: a coordinate counts from the low corner \
             of the footprint, as `0,0,0`"
                .to_owned()
        };
        ParseError::Syntax { position, message }
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

    /// Whether the next tokens are a name and a `:` touching it, which only a block id puts after
    /// `=`: a block header's `:` follows a name and then a space or the end of the line.
    fn at_block_id(&self) -> bool {
        let (Some(name), Some(colon)) = (self.tokens.get(self.pos), self.tokens.get(self.pos + 1))
        else {
            return false;
        };
        matches!(name.kind, TokenKind::Ident(_))
            && colon.kind == TokenKind::Colon
            && colon.span.start == name.span.end
            && self.tokens.get(self.pos + 2).is_some_and(|path| {
                path.span.start == colon.span.end && path.kind != TokenKind::Newline
            })
    }

    /// The refusal of a block id a slot row binds, which binds a canonical token.
    fn block_id_in_a_slot_row(&mut self) -> ParseError {
        let position = self.position();
        let id = match self.read_block_id() {
            Ok(id) => id,
            Err(err) => return err,
        };
        let repair = match id.strip_prefix("minecraft:") {
            Some(token) => format!("write its canonical token, `@{token}`"),
            None => "write a canonical token, as `@stone`".to_owned(),
        };
        ParseError::Syntax {
            position,
            message: format!(
                "a slot row binds a canonical token, not the block id `{id}`; {repair}, or place \
                 the block as written with a `raw_block` or `raw_fill`"
            ),
        }
    }

    /// Read a block id, `namespace:path` with its state literal if any.
    fn parse_block_id(&mut self) -> Result<Value, ParseError> {
        let start_byte = self.current_byte();
        let id = self.read_block_id()?;
        Ok(Value::new(
            ValueKind::BlockId(id),
            start_byte..self.last_byte(),
        ))
    }

    /// Read a block id's text, its state literal folded in as a token's is.
    fn read_block_id(&mut self) -> Result<String, ParseError> {
        let mut text = self.expect_ident()?;
        self.expect(&TokenKind::Colon)?;
        let position = self.position();
        let Some(TokenKind::Ident(path)) = self.peek().map(|t| t.kind.clone()) else {
            let got = self.next_token_text();
            return Err(ParseError::Syntax {
                position,
                message: format!(
                    "a block id expected a path after `{text}:`, got {got}; a block id is \
                     `namespace:path`, as `minecraft:stone`"
                ),
            });
        };
        self.advance();
        text.push(':');
        text.push_str(&path);
        if self
            .peek()
            .is_some_and(|t| t.kind == TokenKind::LBracket && t.span.start == self.last_byte())
        {
            let literal = self.parse_state_literal(&text)?;
            text.push_str(&literal);
        }
        Ok(text)
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
            text.push_str(&self.parse_state_literal(&format!("@{text}"))?);
        }
        Ok(Value::new(
            ValueKind::Token(text),
            start_byte..self.last_byte(),
        ))
    }

    /// Read a block-state literal after `written`, a token or block id as written, as the text it
    /// folds into, spaces dropped. The one place a malformed literal is refused:
    /// `block_array::material` reads it back leniently.
    fn parse_state_literal(&mut self, written: &str) -> Result<String, ParseError> {
        let refuse = |position, found: &str| ParseError::Syntax {
            position,
            message: format!(
                "the state literal on `{written}` expected {found}; a state literal is \
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
                        "the state literal on `{written}` sets `{property}` twice; a block \
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

    /// Whether the next token can open a value.
    fn at_value_start(&self) -> bool {
        matches!(
            self.peek().map(|t| &t.kind),
            Some(
                TokenKind::Ident(_)
                    | TokenKind::Int { .. }
                    | TokenKind::Bool(_)
                    | TokenKind::Str(_)
                    | TokenKind::Size(..)
                    | TokenKind::At
                    | TokenKind::LBracket
            )
        )
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
            // A line no row can open is laid out as usual, so its indent or dedents come first.
            let next = self
                .tokens
                .iter()
                .skip(self.pos)
                .find(|t| !matches!(t.kind, TokenKind::Indent | TokenKind::Dedent));
            let (position, got) = match next {
                Some(token) => (token.position, token.kind.to_string()),
                // End of input is no text to point at, so the refusal ends the table's last line.
                None => (
                    self.tokens
                        .get(self.pos.wrapping_sub(1))
                        .map_or(position, |t| t.position),
                    "end of input".to_owned(),
                ),
            };
            return Err(ParseError::Syntax {
                position,
                message: format!(
                    "expected a truth-table row or the `}}` that closes the table, got {got}"
                ),
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
