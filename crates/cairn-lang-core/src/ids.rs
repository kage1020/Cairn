//! Newtype wrappers for Cairn identifiers shared across the resolver, the block-array IR and the
//! lockfile DTOs.
//!
//! # What an identifier may not carry
//!
//! The one place the rule and its reasons are written. An identifier is non-empty and carries
//! none of:
//!
//! - `.` and `:`, the scope-key separators; otherwise a connect endpoint such as `home.1.entry`
//!   would have its walkway scope key re-parse as a different `(place, port)` pair.
//! - `/` and `\`, because an identifier is the stem of the artifact file written into `--out`:
//!   either would put the file in another directory, and an absolute id would replace `--out`.
//! - `*`, `|`, `?`, `<`, `>`, `"` and U+0000 to U+001F, which a Windows file name cannot carry.
//!   They are refused on every host, so an identifier that builds on Linux is also written on
//!   Windows.
//! - U+007F to U+009F, which a Windows file name allows but which print as nothing or move the
//!   terminal cursor, so a message quoting the identifier could not show it.
//! - Whitespace: a `connect` endpoint is an identifier token, which cannot hold any, and quoted in
//!   a message such an id reads as two names.
//!
//! `place id=` takes a string literal, so the constructor is where the rule is first applied; the
//! lockfile reader applies it again. Every newtype serializes as its bare string.
//!
//! [`WalkwayScopeKey`] is the structural counterpart: a normalized
//! `walkway::SITE::PLACE.PORT__PLACE.PORT` string, built by [`WalkwayScopeKey::from_parts`] or
//! [`WalkwayScopeKey::parse`] and taken apart by [`WalkwayScopeKey::parts`].

use std::borrow::Borrow;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// Failure modes for [`PlaceId`] / [`PortId`] / [`SiteName`] construction.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IdError {
    /// Construction was attempted with an empty string.
    #[error("identifier is empty")]
    Empty,
    /// A character no identifier may carry, as the [module docs](crate::ids) list. The message
    /// escapes a character that would not read as itself.
    #[error(
        "identifier `{ident}` contains forbidden character `{}`",
        shown_char(*ch)
    )]
    ForbiddenChar {
        /// The full offending string.
        ident: String,
        /// The first character that triggered the rejection.
        ch: char,
    },
}

/// The punctuation no identifier may carry, in the order a message lists it. Whitespace and
/// control characters are the other two classes.
pub(crate) const FORBIDDEN_IDENT_PUNCTUATION: [char; 10] =
    ['.', ':', '/', '\\', '*', '|', '?', '<', '>', '"'];

/// Whether an identifier may not carry `c`, per the [module docs](crate::ids).
fn is_forbidden_ident_char(c: char) -> bool {
    FORBIDDEN_IDENT_PUNCTUATION.contains(&c) || c.is_whitespace() || c.is_control()
}

/// `c` as a message quotes it: escaped when it would not read as itself, as a control character
/// or a no-break space would not.
#[must_use]
pub(crate) fn shown_char(c: char) -> String {
    if c.is_control() || (c.is_whitespace() && c != ' ') {
        c.escape_debug().to_string()
    } else {
        c.to_string()
    }
}

fn validate_ident(s: &str) -> Result<(), IdError> {
    if s.is_empty() {
        return Err(IdError::Empty);
    }
    for c in s.chars() {
        if is_forbidden_ident_char(c) {
            return Err(IdError::ForbiddenChar {
                ident: s.to_owned(),
                ch: c,
            });
        }
    }
    Ok(())
}

fn check_no_dunder(role: KeySegmentRole, s: &str) -> Result<(), KeyConstructError> {
    if s.contains("__") {
        return Err(KeyConstructError::ConsecutiveUnderscore {
            role,
            segment: s.to_owned(),
        });
    }
    Ok(())
}

fn has_edge_underscore(s: &str) -> bool {
    s.starts_with('_') || s.ends_with('_')
}

/// Check one place or port segment of a walkway scope key. See
/// [`KeyConstructError::UnderscoreAtEdge`] for why every edge is refused.
fn check_endpoint_segment(role: EndpointSegmentRole, s: &str) -> Result<(), KeyConstructError> {
    check_no_dunder(role.into(), s)?;
    if has_edge_underscore(s) {
        return Err(KeyConstructError::UnderscoreAtEdge {
            role,
            segment: s.to_owned(),
        });
    }
    Ok(())
}

/// Which segment of a walkway scope key a
/// [`KeyConstructError::ConsecutiveUnderscore`] names.
///
/// Exhaustive on purpose: a key's segments are a closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeySegmentRole {
    /// The site name.
    Site,
    /// A `place id=`, on either end.
    Place,
    /// A port id, on either end.
    Port,
}

/// Which endpoint segment a [`KeyConstructError::UnderscoreAtEdge`] names.
///
/// Apart from [`KeySegmentRole`] because the site is exempt from the edge rule, so a site-role
/// edge error cannot be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndpointSegmentRole {
    /// A `place id=`, on either end.
    Place,
    /// A port id, on either end.
    Port,
}

impl KeySegmentRole {
    /// The lowercase noun used in messages: `site`, `place` or `port`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Site => "site",
            Self::Place => "place",
            Self::Port => "port",
        }
    }
}

impl EndpointSegmentRole {
    /// The lowercase noun used in messages: `place` or `port`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        KeySegmentRole::from(self).as_str()
    }
}

impl From<EndpointSegmentRole> for KeySegmentRole {
    fn from(role: EndpointSegmentRole) -> Self {
        match role {
            EndpointSegmentRole::Place => Self::Place,
            EndpointSegmentRole::Port => Self::Port,
        }
    }
}

impl fmt::Display for KeySegmentRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for EndpointSegmentRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

macro_rules! ident_newtype {
    ($(#[$meta:meta])* $Name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $Name(String);

        impl $Name {
            /// Build a new identifier, refusing what the
            /// [module docs](crate::ids) say no identifier may carry.
            ///
            /// # Errors
            ///
            /// Returns [`IdError::Empty`] for the empty string, or
            /// [`IdError::ForbiddenChar`] naming the first character the
            /// rule refuses.
            pub fn new<S: Into<String>>(s: S) -> Result<Self, IdError> {
                let s = s.into();
                validate_ident(&s)?;
                Ok(Self(s))
            }

            /// Borrow the inner string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consume the newtype and return the inner `String`.
            #[must_use]
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $Name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $Name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl Borrow<str> for $Name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl PartialEq<str> for $Name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $Name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<$Name> for str {
            fn eq(&self, other: &$Name) -> bool {
                self == other.0
            }
        }

        impl PartialEq<$Name> for &str {
            fn eq(&self, other: &$Name) -> bool {
                *self == other.0
            }
        }

        impl<'de> Deserialize<'de> for $Name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let s = String::deserialize(deserializer)?;
                Self::new(s).map_err(serde::de::Error::custom)
            }
        }
    };
}

ident_newtype!(
    /// `place id=` value, e.g. `home1`.
    PlaceId
);
ident_newtype!(
    /// Member `id=` exposed by a place's def, e.g. `entry`.
    PortId
);
ident_newtype!(
    /// Bare `site` name (no `site::` IR-key prefix), e.g. `hamlet`.
    SiteName
);

/// Failure modes for [`WalkwayScopeKey::parse`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum KeyParseError {
    /// The key does not start with the `walkway::` prefix.
    #[error("missing `walkway::` prefix in scope key `{0}`")]
    MissingPrefix(String),
    /// The key has the prefix but no `::` separating the site from the
    /// endpoint pair.
    #[error("missing site segment in scope key `{0}`")]
    MissingSite(String),
    /// The endpoint pair does not contain the `__` separator between
    /// `from` and `to`.
    #[error("missing `__` separator between from and to endpoints in scope key `{0}`")]
    MissingFromToSeparator(String),
    /// One endpoint is not in `PLACE.PORT` form (missing `.`).
    #[error("endpoint `{endpoint}` in scope key `{key}` is not in `PLACE.PORT` form")]
    MalformedEndpoint {
        /// The whole scope key that was being parsed.
        key: String,
        /// The endpoint substring that failed to split on `.`.
        endpoint: String,
    },
    /// A segment failed identifier validation.
    #[error("invalid segment `{segment}` in scope key `{key}`: {source}")]
    InvalidSegment {
        /// The whole scope key that was being parsed.
        key: String,
        /// The offending segment.
        segment: String,
        /// The underlying validation error.
        #[source]
        source: IdError,
    },
    /// A segment contains `__`, which collides with the `from`/`to` separator.
    #[error(
        "segment `{segment}` in scope key `{key}` contains `__`, which collides with the \
         `from`/`to` separator"
    )]
    ConsecutiveUnderscore {
        /// The whole scope key that was being parsed.
        key: String,
        /// The offending segment.
        segment: String,
    },
    /// A place or port segment starts or ends with `_`. See
    /// [`KeyConstructError::UnderscoreAtEdge`].
    #[error(
        "segment `{segment}` in scope key `{key}` starts or ends with `_`, and a walkway \
         place or port segment may carry `_` only between other characters"
    )]
    UnderscoreAtEdge {
        /// The whole scope key that was being parsed.
        key: String,
        /// The offending segment.
        segment: String,
    },
}

/// Failure modes for [`WalkwayScopeKey::from_parts`].
///
/// The lexer allows `_` anywhere in an identifier, so `home__1` and `p_` are legal ids. Lowering
/// turns this error into a diagnostic on the `connect` row rather than emit a key that two
/// endpoint pairs share.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum KeyConstructError {
    /// A segment contains the `__` substring.
    #[error(
        "walkway scope key segment `{segment}` contains `__`, which collides with the \
         `from`/`to` separator; rename the {role} so the lowered key is unambiguous"
    )]
    ConsecutiveUnderscore {
        /// Which segment the offending string is.
        role: KeySegmentRole,
        /// The offending segment.
        segment: String,
    },
    /// A place or port segment starts or ends with `_`.
    ///
    /// A `_` beside the `__` separator merges into it: `(a, p_)` to `(b, p)` and `(a, p)` to
    /// `(_b, p)` both encode as `a.p___b.p`, and a port named `_` gives `a.___b._`, which splits
    /// back into an empty port. Which edges touch the separator depends on the direction a row is
    /// written, and the edges beside a `.` cannot merge, but every edge is refused so the rule
    /// reads as one sentence: no place or port id starts or ends with `_`. The site is exempt,
    /// sitting between two `::`.
    #[error(
        "walkway scope key segment `{segment}` starts or ends with `_`, and a walkway place \
         or port id may carry `_` only between other characters; rename the {role} to drop \
         the leading or trailing `_`"
    )]
    UnderscoreAtEdge {
        /// Which endpoint segment the offending string is.
        role: EndpointSegmentRole,
        /// The offending segment.
        segment: String,
    },
}

/// IR scope key for a single walkway, of the form
/// `walkway::SITE::FROM_PLACE.FROM_PORT__TO_PLACE.TO_PORT`.
///
/// Built only by [`from_parts`](Self::from_parts) or [`parse`](Self::parse), so no segment holds
/// a `.` or `__` that would make the key ambiguous to take apart.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct WalkwayScopeKey(String);

impl WalkwayScopeKey {
    /// Build a walkway scope key from a `(site, from, to)` triple.
    ///
    /// Each end is one [`WalkwayEndpoint`], so a call site cannot swap `from_port` for
    /// `to_place`.
    ///
    /// # Errors
    ///
    /// [`KeyConstructError::ConsecutiveUnderscore`] when any segment contains `__`, and
    /// [`KeyConstructError::UnderscoreAtEdge`] when a place or port segment starts or ends with
    /// `_`.
    pub fn from_parts(
        site: &SiteName,
        from: &WalkwayEndpoint,
        to: &WalkwayEndpoint,
    ) -> Result<Self, KeyConstructError> {
        check_no_dunder(KeySegmentRole::Site, site.as_str())?;
        check_endpoint_segment(EndpointSegmentRole::Place, from.place.as_str())?;
        check_endpoint_segment(EndpointSegmentRole::Port, from.port.as_str())?;
        check_endpoint_segment(EndpointSegmentRole::Place, to.place.as_str())?;
        check_endpoint_segment(EndpointSegmentRole::Port, to.port.as_str())?;
        Ok(Self(format!(
            "walkway::{site}::{from_place}.{from_port}__{to_place}.{to_port}",
            from_place = from.place,
            from_port = from.port,
            to_place = to.place,
            to_port = to.port,
        )))
    }

    /// Parse and validate a wire-format scope key.
    ///
    /// # Errors
    ///
    /// Returns a [`KeyParseError`] variant when the input does not
    /// follow the `walkway::SITE::PLACE.PORT__PLACE.PORT` shape, or
    /// when any of the five segments fails identifier validation
    /// (e.g. a port id containing `.` or `__`, or starting or ending
    /// with `_`).
    pub fn parse(s: &str) -> Result<Self, KeyParseError> {
        let rest = s
            .strip_prefix("walkway::")
            .ok_or_else(|| KeyParseError::MissingPrefix(s.to_owned()))?;
        let (site, endpoints) = rest
            .split_once("::")
            .ok_or_else(|| KeyParseError::MissingSite(s.to_owned()))?;
        let (from, to) = endpoints
            .split_once("__")
            .ok_or_else(|| KeyParseError::MissingFromToSeparator(s.to_owned()))?;
        let malformed = |endpoint: &str| KeyParseError::MalformedEndpoint {
            key: s.to_owned(),
            endpoint: endpoint.to_owned(),
        };
        let (from_place, from_port) = from.split_once('.').ok_or_else(|| malformed(from))?;
        let (to_place, to_port) = to.split_once('.').ok_or_else(|| malformed(to))?;

        let invalid = |segment: &str, source: IdError| KeyParseError::InvalidSegment {
            key: s.to_owned(),
            segment: segment.to_owned(),
            source,
        };
        let dunder = |segment: &str| KeyParseError::ConsecutiveUnderscore {
            key: s.to_owned(),
            segment: segment.to_owned(),
        };
        // `__` first, so a separator collision is reported as one rather than as whatever
        // forbidden character the mis-split leaves.
        for seg in [site, from_place, from_port, to_place, to_port] {
            if seg.contains("__") {
                return Err(dunder(seg));
            }
        }
        let site_id = SiteName::new(site).map_err(|e| invalid(site, e))?;
        let from_place_id = PlaceId::new(from_place).map_err(|e| invalid(from_place, e))?;
        let from_port_id = PortId::new(from_port).map_err(|e| invalid(from_port, e))?;
        let to_place_id = PlaceId::new(to_place).map_err(|e| invalid(to_place, e))?;
        let to_port_id = PortId::new(to_port).map_err(|e| invalid(to_port, e))?;
        // `from_parts` judges `_` at an edge. Its `__` arm cannot fire after the loop above, but
        // is mapped rather than `unreachable!()` so removing the loop cannot add a panic.
        Self::from_parts(
            &site_id,
            &WalkwayEndpoint {
                place: from_place_id,
                port: from_port_id,
            },
            &WalkwayEndpoint {
                place: to_place_id,
                port: to_port_id,
            },
        )
        .map_err(|e| match e {
            KeyConstructError::ConsecutiveUnderscore { segment, .. } => dunder(&segment),
            KeyConstructError::UnderscoreAtEdge { segment, .. } => {
                KeyParseError::UnderscoreAtEdge {
                    key: s.to_owned(),
                    segment,
                }
            }
        })
    }

    /// Borrow the wire-format string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Decompose the key into its five segments, borrowing into the
    /// internal representation.
    ///
    /// # Panics
    ///
    /// Panics if the internal string is not canonical, which [`Self::from_parts`] and
    /// [`Self::parse`] both guarantee.
    #[must_use]
    pub fn parts(&self) -> WalkwayScopeKeyParts<'_> {
        let rest = self
            .0
            .strip_prefix("walkway::")
            .expect("WalkwayScopeKey internal repr starts with `walkway::`");
        let (site, endpoints) = rest
            .split_once("::")
            .expect("WalkwayScopeKey internal repr has site segment");
        let (from, to) = endpoints
            .split_once("__")
            .expect("WalkwayScopeKey internal repr has `__` separator");
        let (from_place, from_port) = from
            .split_once('.')
            .expect("WalkwayScopeKey internal repr from endpoint is PLACE.PORT");
        let (to_place, to_port) = to
            .split_once('.')
            .expect("WalkwayScopeKey internal repr to endpoint is PLACE.PORT");
        WalkwayScopeKeyParts {
            site,
            from_place,
            from_port,
            to_place,
            to_port,
        }
    }
}

impl fmt::Display for WalkwayScopeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for WalkwayScopeKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for WalkwayScopeKey {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for WalkwayScopeKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Borrowed view of [`WalkwayScopeKey`]'s five structural segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkwayScopeKeyParts<'a> {
    /// Site name (no `site::` prefix).
    pub site: &'a str,
    /// `from` place id.
    pub from_place: &'a str,
    /// `from` port id.
    pub from_port: &'a str,
    /// `to` place id.
    pub to_place: &'a str,
    /// `to` port id.
    pub to_port: &'a str,
}

/// `(place, port)` pair the block-array IR and the lockfile DTOs share.
///
/// The span-less counterpart of [`crate::resolve::PortRef`], so [`crate::block_array::Walkway`]
/// and [`crate::lock::LockWalkway`] spell an endpoint with one type. Unknown keys are refused
/// because it is a lockfile container (`walkways[].from`), serde does not cascade the attribute
/// from [`crate::lock`]'s structs, and a record of what was built must not carry a payload the
/// reader ignores.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WalkwayEndpoint {
    /// `place id=` value.
    pub place: PlaceId,
    /// Member `id=` exposed by the place's def.
    pub port: PortId,
}

impl fmt::Display for WalkwayEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.place, self.port)
    }
}

/// The name, without extension, of the file a scope of
/// [`crate::BlockArrayIr::structures`] is written to.
///
/// - `struct::cottage` → `cottage`
/// - `site::hamlet::home1` → `home1`: a placement is named after its `id=`
///   alone, so it shares one output directory with every struct and every
///   other site's placements (`spec/components-editing-sites` "Output
///   naming").
/// - `walkway::hamlet::home1.entry__home2.entry` →
///   `hamlet_walkway_home1_entry__home2_entry`: the site is kept, so the
///   walkways of different sites do not share a name even when their
///   endpoints are spelled alike, and the `.` between a place and its port
///   becomes `_`, so the name stays a single identifier token on every
///   operating system.
///
/// A key with none of these shapes, such as `walkway::no_site` or `site::hamlet`, is returned
/// unchanged. Kept apart from the file writers so the resolver's `E_OUTPUT_NAME_COLLISION` and the
/// name a build writes are one function.
///
/// # Panics
///
/// In a debug build, when `source_scope` starts with `walkway::`, has a further `::`, and does not
/// parse as a [`WalkwayScopeKey`] (`walkway::hamlet::bogus`); a release build returns it
/// unchanged. No source reaches this, because every walkway key a build writes comes from
/// [`WalkwayScopeKey::from_parts`].
#[must_use]
pub fn artifact_stem(source_scope: &str) -> String {
    if let Some(rest) = source_scope.strip_prefix("walkway::")
        && rest.contains("::")
    {
        match WalkwayScopeKey::parse(source_scope) {
            Ok(key) => {
                // `a_b.c__d_e.f` and `a.b_c__d.e_f` flatten to one name; the resolver reports
                // that pair as `E_OUTPUT_NAME_COLLISION`.
                let parts = key.parts();
                return format!(
                    "{site}_walkway_{from_place}_{from_port}__{to_place}_{to_port}",
                    site = parts.site,
                    from_place = parts.from_place,
                    from_port = parts.from_port,
                    to_place = parts.to_place,
                    to_port = parts.to_port,
                );
            }
            Err(e) => {
                debug_assert!(
                    false,
                    "artifact_stem received a `walkway::SITE::*` key that failed to parse \
                     ({source_scope:?}): {e}",
                );
            }
        }
    }
    source_scope
        .strip_prefix("struct::")
        .or_else(|| {
            source_scope
                .strip_prefix("site::")
                .and_then(|rest| rest.split_once("::").map(|(_, id)| id))
        })
        .unwrap_or(source_scope)
        .to_owned()
}

#[cfg(test)]
mod tests;
