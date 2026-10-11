//! Newtype wrappers for Cairn identifiers shared across the resolver, the
//! block-array IR, and the lockfile DTOs.
//!
//! # What an identifier may not carry
//!
//! This section is the one place the rule and its reasons are written; the
//! constructors, [`IdError::ForbiddenChar`] and the `E_INVALID_PLACE_ID`
//! note follow it rather than restating it. An identifier is non-empty and
//! carries none of:
//!
//! - `.` and `:`, the scope-key separators. A connect endpoint such as
//!   `home.1.entry` would otherwise have its walkway scope key re-parse as a
//!   different `(place, port)` pair.
//! - `/` and `\`, the path separators. An identifier is the stem of the
//!   artifact file the compiler writes into `--out`, so either would put
//!   that file in another directory, and an absolute id would replace
//!   `--out` altogether.
//! - `*`, `|`, `?`, `<`, `>`, `"` and the control characters U+0000 to
//!   U+001F, which a Windows file name cannot carry: an identifier with one
//!   would check and build on Linux and then fail to be written on Windows.
//!   They are refused on every host, so whether an identifier is refused for
//!   a character Windows reserves in a file name does not depend on the host
//!   that checks it.
//! - The other control characters, U+007F to U+009F. A Windows file name
//!   may carry them; they are refused on their own ground, because they
//!   print as nothing or move the terminal cursor, so a message quoting the
//!   identifier could not show it.
//! - Whitespace. A `connect` endpoint is lexed as an identifier token, which
//!   cannot carry any, so a place id with a space in it could never be
//!   connected; and quoted in a message, it reads as two names.
//!
//! None of this is established before a newtype is built: `place id=`
//! takes a string literal, so the constructor is where the rule is first
//! applied, and the lockfile reader applies it again to what it reads back.
//!
//! The wire format is unchanged: every newtype is `#[serde(transparent)]`
//! over its internal `String`, so any YAML / JSON consumer keeps seeing the
//! same scalar string it used to.
//!
//! [`WalkwayScopeKey`] is the structural counterpart: its internal
//! representation is the normalized `walkway::SITE::PLACE.PORT__PLACE.PORT`
//! string, but construction goes through [`WalkwayScopeKey::from_parts`]
//! (typed) or [`WalkwayScopeKey::parse`] (validating), and decomposition
//! returns borrowed segments via [`WalkwayScopeKey::parts`].

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
    /// Construction was attempted with a string containing a character no
    /// identifier may carry; the [module docs](crate::ids) list them and say
    /// why. The message quotes the character as [`char::escape_debug`] does
    /// when it would not read as itself: a control character, or whitespace
    /// other than a plain space.
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

/// The punctuation no identifier may carry, in the order a message lists
/// it. Whitespace and the control characters are the other two classes;
/// the [module docs](crate::ids) say why each is refused.
pub(crate) const FORBIDDEN_IDENT_PUNCTUATION: [char; 10] =
    ['.', ':', '/', '\\', '*', '|', '?', '<', '>', '"'];

/// Whether an identifier may not carry `c`, per the [module docs](crate::ids).
fn is_forbidden_ident_char(c: char) -> bool {
    FORBIDDEN_IDENT_PUNCTUATION.contains(&c) || c.is_whitespace() || c.is_control()
}

/// `c` as a message quotes it: itself, or its escape when it would not read
/// as itself. A control character prints as nothing or moves the cursor,
/// and whitespace other than a plain space prints as a space, so an
/// identifier refused for a no-break space would otherwise read as refused
/// for the space everyone can see. A plain space is recognisable as itself.
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

/// Check one place or port segment of a walkway scope key: no `__`
/// anywhere, and no `_` at either edge. See
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
/// Exhaustive on purpose, like [`EndpointSegmentRole`]: a key's segments
/// are a closed set, as with `Edition` and `Cardinal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeySegmentRole {
    /// The site name.
    Site,
    /// A `place id=`, on either end.
    Place,
    /// A port id, on either end.
    Port,
}

/// Which endpoint segment a [`KeyConstructError::UnderscoreAtEdge`]
/// names. A separate type from [`KeySegmentRole`] because the site is
/// exempt from the edge rule, so a site-role edge error cannot be built.
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
    /// A segment contains the `__` substring, which collides with the
    /// `from`/`to` separator and would let the canonical encoding
    /// alias two distinct endpoint pairs. See
    /// [`WalkwayScopeKey::from_parts`] for the round-trip story.
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
/// A typed construction fails when a segment contains `__`, which
/// collides with the separator between the `from` and `to` endpoints,
/// or when a place / port segment starts or ends with `_` (see
/// [`Self::UnderscoreAtEdge`] for why every such edge is refused, not
/// only the ones next to the separator). Surface lexer rules allow `_` anywhere in identifiers, so a
/// place / port id can legally be `home__1` or `p_`; lowering must
/// convert that into a diagnostic on the originating `connect` row
/// rather than emit a wire-ambiguous scope key, since two distinct
/// endpoint pairs would otherwise encode to one string.
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
    /// This is the canonical statement of why the rule covers every edge;
    /// the other copies point here.
    ///
    /// In `walkway::SITE::FROM_PLACE.FROM_PORT__TO_PLACE.TO_PORT`, only
    /// two of the eight edges of the four place and port segments touch
    /// the `from`/`to` separator: the end of `from_port` and the start of
    /// `to_place`. A `_` there merges into it: `(a, p_)` to `(b, p)` and
    /// `(a, p)` to `(_b, p)` both encode to `a.p___b.p`, and a port named
    /// `_` gives `a.___b._`, which splits back into an empty port. A row
    /// written the other way round puts the start of `from_place` and the
    /// end of `to_port` at the separator instead, so those two are
    /// refused as well. The remaining four (the end of each place and the
    /// start of each port) sit next to a `.` in either direction and
    /// cannot merge; they are refused anyway, so the rule a user reads is
    /// one sentence, "no place or port id starts or ends with `_`", and
    /// does not depend on a segment's position or on which way a row is
    /// written. The site is exempt: it sits between two `::` separators,
    /// which no identifier can contain.
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
/// Construction goes through [`from_parts`](Self::from_parts) (typed)
/// or [`parse`](Self::parse) (validating) so the surface invariants on
/// each segment hold — in particular, neither place id nor port id can
/// contain the `.` that would otherwise make the key ambiguous to
/// decompose.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct WalkwayScopeKey(String);

impl WalkwayScopeKey {
    /// Build a walkway scope key from a `(site, from, to)` triple.
    ///
    /// Taking `&WalkwayEndpoint` for both ends (rather than four bare
    /// `&PlaceId` / `&PortId` arguments) means the call site cannot
    /// accidentally swap `from_port` for `to_place`: each endpoint is
    /// carried as a single typed value.
    ///
    /// # Errors
    ///
    /// Returns [`KeyConstructError::ConsecutiveUnderscore`] when any
    /// segment contains the `__` substring, and
    /// [`KeyConstructError::UnderscoreAtEdge`] when a place or port
    /// segment starts or ends with `_`. The surface lexer allows `_`
    /// freely in identifiers, so a user-typed place / port id may
    /// validly be `b__c` or `p_` — but the canonical scope key uses `__`
    /// as the `from`/`to` separator, so `(home, b__c, home2, entry)` and
    /// `(home, b, c__home2, entry)` would both encode to the same
    /// string, as would `(a, p_, b, p)` and `(a, p, _b, p)`. Lowering
    /// must surface this back to the user as a diagnostic on the
    /// originating `connect` row rather than emit a silent alias.
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
        // Reject `__` per segment before delegating to identifier
        // validation so the user sees the structural problem
        // (separator collision) rather than a generic "forbidden char"
        // message from `IdError`.
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
        // A place or port with `_` at an edge is left to `from_parts`,
        // whose error maps onto the matching parse-error variant. Its
        // `ConsecutiveUnderscore` arm cannot fire here, since the loop
        // above already returned for every `__`; it is mapped rather than
        // `unreachable!()` so removing that loop cannot introduce a panic.
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
    /// Panics if the internal representation is not in canonical form.
    /// Both [`Self::from_parts`] and [`Self::parse`] guarantee that
    /// form, so a panic here means the invariant was broken by a
    /// reflection-style construction.
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
/// Spanned references live on [`crate::resolve::PortRef`]; this is the
/// span-less wire DTO so [`crate::block_array::Walkway`] and
/// [`crate::lock::LockWalkway`] can both spell the endpoint with the
/// same type instead of one re-encoding the other as `"PLACE.PORT"`.
/// Unknown keys are refused here for the same reason as in
/// [`crate::lock`]'s own structs: this type is a lockfile container two
/// levels down (`walkways[].from` / `.to`), serde does not cascade the
/// attribute, and a claim about what was built must not carry a payload
/// the reader ignores.
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
/// A key with none of these shapes, such as `walkway::no_site` or
/// `site::hamlet`, is returned unchanged. It lives here rather than beside
/// the file writers so the resolver's `E_OUTPUT_NAME_COLLISION` and the
/// name a build writes are one function.
///
/// # Panics
///
/// In a debug build, when `source_scope` starts with `walkway::`, has a
/// further `::`, and does not parse as a [`WalkwayScopeKey`]
/// (`walkway::hamlet::bogus`). A release build returns such a key
/// unchanged. No source reaches this: every walkway key a build writes is
/// made by [`WalkwayScopeKey::from_parts`], which round-trips through
/// [`WalkwayScopeKey::parse`].
#[must_use]
pub fn artifact_stem(source_scope: &str) -> String {
    // Only a canonical `walkway::SITE::PLACE.PORT__PLACE.PORT` key is
    // parsed; a synthetic `walkway::no_site` fixture falls through to the
    // generic name below with every other unrecognised scope.
    if let Some(rest) = source_scope.strip_prefix("walkway::")
        && rest.contains("::")
    {
        match WalkwayScopeKey::parse(source_scope) {
            Ok(key) => {
                // `parts()` splits on the same validated boundaries the
                // lowering pass built the key from, so a `.` inside a port
                // id cannot be mistaken for the place/port separator. Ids
                // allow `_`, so `a_b.c__d_e.f` and `a.b_c__d.e_f` still
                // flatten to one name; the resolver reports that pair as
                // `E_OUTPUT_NAME_COLLISION`.
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
                // A canonical prefix that fails to parse is a lowering-pass
                // contract break; release builds fall through rather than
                // panic.
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
