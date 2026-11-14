//! A top-level `edit TARGET set key=value …` line (`spec/components-editing-sites` "Editing
//! model").

use crate::address::{AddressSegment, MemberAddress};
use crate::ast::{Edit, EditTarget};
use crate::error::ParseError;
use crate::lex::TokenKind;

use super::Parser;

/// The word that opens a top-level `edit` line.
pub(super) const EDIT: &str = "edit";

/// The word that opens an edit's `key=value` list.
const SET: &str = "set";

impl Parser<'_> {
    /// The rest of an `edit` line, its keyword consumed from `start_byte`.
    pub(super) fn parse_edit(&mut self, start_byte: usize) -> Result<Edit, ParseError> {
        let target = self.parse_edit_target()?;
        if !self.match_keyword(SET) {
            return Err(ParseError::Syntax {
                position: self.position(),
                message: format!(
                    "expected `set` after the edit's selector or address, then the keys it sets: \
                     `edit {} set key=value`",
                    &self.source[target.span().clone()],
                ),
            });
        }
        let mut set = Vec::new();
        while !self.peek_is(&TokenKind::Newline) && !self.at_eof() {
            set.push(self.parse_arg()?);
        }
        if set.is_empty() {
            return Err(ParseError::Syntax {
                position: self.position(),
                message: "an edit sets at least one `key=value` after `set`".into(),
            });
        }
        let span = start_byte..self.last_byte();
        self.expect_newline()?;
        Ok(Edit { target, set, span })
    }

    /// A selector, `KEYWORD[attr=value]…`, or an address, `ROLE@SIDE[N]` steps joined by `.`.
    fn parse_edit_target(&mut self) -> Result<EditTarget, ParseError> {
        let start = self.current_byte();
        let (keyword, keyword_span) = self.expect_ident_spanned()?;
        let opens_attrs = self.peek_is(&TokenKind::LBracket)
            && !matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Int { .. })
            );
        if opens_attrs {
            let mut attrs = Vec::new();
            while self.peek_is(&TokenKind::LBracket) {
                self.advance();
                attrs.extend(self.parse_arg_list_until(&TokenKind::RBracket)?);
                self.expect(&TokenKind::RBracket)?;
            }
            return Ok(EditTarget::Selector {
                keyword,
                attrs,
                keyword_span,
                span: start..self.last_byte(),
            });
        }
        if !self.peek_is(&TokenKind::At) && !self.peek_is(&TokenKind::LBracket) {
            return Err(ParseError::Syntax {
                position: self.position(),
                message: format!(
                    "expected `[` or `@` after `{keyword}`: an edit selects by a selector, \
                     `{keyword}[class=NAME]`, or by an address, `{keyword}@front[0]`"
                ),
            });
        }
        let mut address = MemberAddress::default();
        let (mut role, mut role_span) = (keyword, keyword_span);
        loop {
            let side = if self.peek_is(&TokenKind::At) {
                self.advance();
                Some(self.parse_address_side()?)
            } else {
                None
            };
            let index = self.parse_address_index(&role)?;
            address = address.child(AddressSegment { role, side, index });
            if !self.peek_is(&TokenKind::Dot) {
                break;
            }
            self.advance();
            (role, role_span) = self.expect_ident_spanned()?;
        }
        Ok(EditTarget::Address {
            address,
            keyword_span: role_span,
            span: start..self.last_byte(),
        })
    }

    /// The wall after an address step's `@`: `front`, or a dotted one such as `inside.back`.
    fn parse_address_side(&mut self) -> Result<String, ParseError> {
        let mut side = self.expect_ident()?;
        while self.peek_is(&TokenKind::Dot) {
            self.advance();
            side.push('.');
            side.push_str(&self.expect_ident()?);
        }
        Ok(side)
    }

    /// The `[N]` that ends an address step of `role`.
    fn parse_address_index(&mut self, role: &str) -> Result<u32, ParseError> {
        let position = self.position();
        let shape = || ParseError::Syntax {
            position,
            message: format!(
                "an address step ends in its count among the `{role}` members on its wall, a \
                 whole number in brackets: `{role}[0]`"
            ),
        };
        if !self.peek_is(&TokenKind::LBracket) {
            return Err(shape());
        }
        self.advance();
        let at_count = self.position();
        let Some(TokenKind::Int { lexeme }) = self.peek().map(|t| &t.kind) else {
            return Err(shape());
        };
        let index = lexeme.parse::<u32>().map_err(|_| ParseError::Syntax {
            position: at_count,
            message: format!(
                "`{role}[{lexeme}]` counts past {max}, the most an address step counts",
                max = u32::MAX,
            ),
        })?;
        self.advance();
        self.expect(&TokenKind::RBracket)?;
        Ok(index)
    }
}
