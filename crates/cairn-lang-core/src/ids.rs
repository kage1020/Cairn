//! Identifier newtypes and the walkway scope key built from them.
//!
//! An identifier is non-empty and carries no `.`, `:`, `/`, `\`, `*`, `|`, `?`, `<`, `>`, `"`,
//! whitespace or control character. A [`PlaceId`] is also the stem of an artifact's file name, so
//! it is not a device name Windows reserves either (`spec/lint` "Sites and placements").

use std::borrow::Borrow;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// Failure modes for [`PlaceId`] / [`PortId`] / [`SiteName`] construction.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum IdError {
    /// Construction was attempted with an empty string.
    #[error("identifier is empty")]
    Empty,
    /// A character no identifier may carry, as the [module docs](crate::ids) list.
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
    /// A [`PlaceId`] that is a device name Windows reserves.
    #[error("identifier `{ident}` is a device name Windows reserves")]
    ReservedDeviceName {
        /// The full offending string.
        ident: String,
    },
}

/// The punctuation no identifier may carry, in the order a message lists it.
pub(crate) const FORBIDDEN_IDENT_PUNCTUATION: [char; 10] =
    ['.', ':', '/', '\\', '*', '|', '?', '<', '>', '"'];

fn is_forbidden_ident_char(c: char) -> bool {
    FORBIDDEN_IDENT_PUNCTUATION.contains(&c) || c.is_whitespace() || c.is_control()
}

/// `c` as a message quotes it, escaped when it would not read as itself.
#[must_use]
pub(crate) fn shown_char(c: char) -> String {
    if c.is_control() || (c.is_whitespace() && c != ' ') {
        c.escape_debug().to_string()
    } else {
        c.to_string()
    }
}

/// The device names Windows reserves in every directory, whatever the case or extension.
const WINDOWS_DEVICE_NAMES: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];

/// The device names Windows reserves with one digit after them, `¹`, `²` and `³` among the digits.
const WINDOWS_NUMBERED_DEVICE_NAMES: [&str; 2] = ["COM", "LPT"];

/// Whether `name`, as a file name's stem, is a device name Windows reserves.
#[must_use]
pub(crate) fn is_windows_device_name(name: &str) -> bool {
    if WINDOWS_DEVICE_NAMES
        .iter()
        .any(|device| name.eq_ignore_ascii_case(device))
    {
        return true;
    }
    WINDOWS_NUMBERED_DEVICE_NAMES.iter().any(|device| {
        let Some((head, tail)) = name.split_at_checked(device.len()) else {
            return false;
        };
        let mut digits = tail.chars();
        head.eq_ignore_ascii_case(device)
            && matches!(
                (digits.next(), digits.next()),
                (Some('0'..='9' | '¹' | '²' | '³'), None)
            )
    })
}

/// The names [`is_windows_device_name`] matches, as a message lists them.
#[must_use]
pub(crate) fn windows_device_names_listed() -> String {
    let quoted =
        |names: &[&str]| -> Vec<String> { names.iter().map(|name| format!("`{name}`")).collect() };
    format!(
        "{}, and {} followed by one of `0` to `9`, `¹`, `²` or `³`",
        quoted(&WINDOWS_DEVICE_NAMES).join(", "),
        quoted(&WINDOWS_NUMBERED_DEVICE_NAMES).join(" or "),
    )
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

fn validate_place_id(s: &str) -> Result<(), IdError> {
    validate_ident(s)?;
    if is_windows_device_name(s) {
        return Err(IdError::ReservedDeviceName {
            ident: s.to_owned(),
        });
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

/// Which segment of a walkway scope key a [`KeyConstructError::ConsecutiveUnderscore`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeySegmentRole {
    /// The site name.
    Site,
    /// A `place id=`, on either end.
    Place,
    /// A port id, on either end.
    Port,
}

/// Which endpoint segment a [`KeyConstructError::UnderscoreAtEdge`] names; the site is exempt.
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
    ($(#[$meta:meta])* $Name:ident, $validate:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $Name(String);

        impl $Name {
            /// Build an identifier, refusing what the [module docs](crate::ids) list.
            ///
            /// # Errors
            ///
            /// The [`IdError`] naming the first of those rules `s` breaks.
            pub fn new<S: Into<String>>(s: S) -> Result<Self, IdError> {
                let s = s.into();
                $validate(&s)?;
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
    PlaceId,
    validate_place_id
);
ident_newtype!(
    /// Member `id=` exposed by a place's def, e.g. `entry`.
    PortId,
    validate_ident
);
ident_newtype!(
    /// Bare `site` name (no `site::` IR-key prefix), e.g. `hamlet`.
    SiteName,
    validate_ident
);

/// Failure modes for [`WalkwayScopeKey::parse`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum KeyParseError {
    /// The key does not start with the `walkway::` prefix.
    #[error("missing `walkway::` prefix in scope key `{0}`")]
    MissingPrefix(String),
    /// The key has the prefix but no `::` after the site.
    #[error("missing site segment in scope key `{0}`")]
    MissingSite(String),
    /// The endpoint pair has no `__` between `from` and `to`.
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
    /// A place or port segment starts or ends with `_`.
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
    /// A place or port segment starts or ends with `_`, which could merge into the `__` separator.
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

/// IR scope key for a single walkway, `walkway::SITE::FROM_PLACE.FROM_PORT__TO_PLACE.TO_PORT`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct WalkwayScopeKey(String);

impl WalkwayScopeKey {
    /// Build a walkway scope key from a `(site, from, to)` triple.
    ///
    /// # Errors
    ///
    /// [`KeyConstructError`] for a `__` in any segment or a `_` at a place or port's edge.
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
    /// [`KeyParseError`] when the shape is wrong or a segment fails validation.
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
        // `__` first, so a separator collision is reported as one.
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
        // `from_parts` judges `_` at an edge; its `__` arm cannot fire after the loop above.
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

    /// The key's five segments, borrowed.
    ///
    /// # Panics
    ///
    /// If the internal string is not canonical, which every constructor guarantees.
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

/// `(place, port)` pair the block-array IR and the lockfile share.
///
/// Denies unknown fields itself: serde does not cascade the attribute from [`crate::lock`].
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

/// The file name, without extension, a scope of [`crate::BlockArrayIr::structures`] is written to
/// (`spec/components-editing-sites` "Output naming"); a key of no known shape comes back as is.
///
/// # Panics
///
/// In a debug build, on a `walkway::SITE::…` key that does not parse.
#[must_use]
pub fn artifact_stem(source_scope: &str) -> String {
    if let Some(rest) = source_scope.strip_prefix("walkway::")
        && rest.contains("::")
    {
        match WalkwayScopeKey::parse(source_scope) {
            Ok(key) => {
                // Two keys can flatten to one name; the resolver reports `E_OUTPUT_NAME_COLLISION`.
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
