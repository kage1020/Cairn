//! Newtype wrappers for Cairn identifiers shared across the resolver, the
//! block-array IR, and the lockfile DTOs.
//!
//! Each newtype carries the invariants the surface lexer already
//! establishes (non-empty, no `.`, no `:`, no whitespace) so downstream
//! layers cannot accidentally pass a connect endpoint such as
//! `home.1.entry` and have the walkway scope key silently re-parse as a
//! different `(place, port)` pair. The path separators `/` and `\` are
//! refused as well: an identifier becomes an artifact's file name, and a
//! separator in it would move the artifact out of the output directory. So
//! are the characters a Windows file name cannot carry (`*`, `|`, `?`, `<`,
//! `>`, `"` and the control characters).
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
    /// Construction was attempted with a string containing a character
    /// that is reserved as a structural separator (`.`, `:`), a path
    /// separator (`/`, `\`), a character a Windows file name cannot carry
    /// (`*`, `|`, `?`, `<`, `>`, `"`, control characters), or whitespace.
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

/// The characters no identifier may carry.
///
/// `.` and `:` are the scope-key separators. `/` and `\` are the path
/// separators: an identifier is the stem of the artifact file the compiler
/// writes into `--out`, so either one would put that file in another
/// directory, and an absolute id would replace `--out` altogether.
/// `*`, `|`, `?`, `<`, `>`, `"` and the control characters cannot appear in
/// a Windows file name, so an identifier carrying one would check and build
/// on Linux and then fail to be written on Windows. Every one of them is
/// refused on every platform, so whether an identifier is accepted does not
/// depend on the host that checks it.
fn is_forbidden_ident_char(c: char) -> bool {
    matches!(
        c,
        '.' | ':' | '/' | '\\' | '*' | '|' | '?' | '<' | '>' | '"'
    ) || c.is_whitespace()
        || c.is_control()
}

/// `c` as a message quotes it: itself, or its escape when it is a control
/// character, which would otherwise print as nothing or move the cursor.
#[must_use]
pub(crate) fn shown_char(c: char) -> String {
    if c.is_control() {
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
            /// Build a new identifier, validating the surface invariants.
            ///
            /// # Errors
            ///
            /// Returns [`IdError::Empty`] for the empty string, or
            /// [`IdError::ForbiddenChar`] if the input contains a `.`,
            /// `:`, or whitespace character (any of which would break
            /// the structural separators downstream lookups rely on), a
            /// `/` or `\` (which would make the artifact file name a
            /// path), or a `*`, `|`, `?`, `<`, `>`, `"` or control
            /// character (which a Windows file name cannot carry).
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
mod tests {
    use super::*;

    #[test]
    fn artifact_stem_names_each_scope_kind() {
        assert_eq!(artifact_stem("struct::cottage"), "cottage");
        assert_eq!(artifact_stem("site::hamlet::home1"), "home1");
        assert_eq!(
            artifact_stem("walkway::hamlet::home1.entry__home2.entry"),
            "hamlet_walkway_home1_entry__home2_entry",
        );
        // Anything else passes through, including a walkway key without
        // its site separator.
        assert_eq!(artifact_stem("cottage"), "cottage");
        assert_eq!(artifact_stem("walkway::no_site"), "walkway::no_site");
    }

    #[test]
    fn ident_new_accepts_plain_identifiers() {
        assert_eq!(PlaceId::new("home1").unwrap().as_str(), "home1");
        assert_eq!(PortId::new("entry").unwrap().as_str(), "entry");
        assert_eq!(SiteName::new("hamlet").unwrap().as_str(), "hamlet");
    }

    #[test]
    fn ident_new_rejects_empty() {
        assert_eq!(PlaceId::new(""), Err(IdError::Empty));
        assert_eq!(PortId::new(""), Err(IdError::Empty));
        assert_eq!(SiteName::new(""), Err(IdError::Empty));
    }

    #[test]
    fn ident_new_rejects_dot() {
        match PortId::new("foo.bar") {
            Err(IdError::ForbiddenChar { ident, ch }) => {
                assert_eq!(ident, "foo.bar");
                assert_eq!(ch, '.');
            }
            other => panic!("expected ForbiddenChar('.'), got {other:?}"),
        }
    }

    #[test]
    fn ident_new_rejects_colon_and_whitespace() {
        assert!(matches!(
            PlaceId::new("foo:bar"),
            Err(IdError::ForbiddenChar { ch: ':', .. })
        ));
        assert!(matches!(
            SiteName::new("foo bar"),
            Err(IdError::ForbiddenChar { ch: ' ', .. })
        ));
    }

    #[test]
    fn ident_new_rejects_path_separators() {
        // Every shape the output file name could take through a separator:
        // absolute, relative with a directory, and Windows-style. Each
        // newtype is checked, since all three become file-name segments.
        for (ident, ch) in [("/tmp/x", '/'), ("sub/x", '/'), ("a\\b", '\\')] {
            for result in [
                PlaceId::new(ident).map(|_| ()),
                PortId::new(ident).map(|_| ()),
                SiteName::new(ident).map(|_| ()),
            ] {
                assert_eq!(
                    result,
                    Err(IdError::ForbiddenChar {
                        ident: ident.to_owned(),
                        ch,
                    }),
                    "`{ident}` must be refused on `{ch}`",
                );
            }
        }
    }

    #[test]
    fn ident_new_rejects_what_a_windows_file_name_cannot_carry() {
        // Each newtype is checked, since all three become file-name
        // segments, and the rule must not depend on the host.
        for ch in ['*', '|', '?', '<', '>', '"', '\u{1}', '\u{7f}'] {
            let ident = format!("a{ch}b");
            for result in [
                PlaceId::new(ident.as_str()).map(|_| ()),
                PortId::new(ident.as_str()).map(|_| ()),
                SiteName::new(ident.as_str()).map(|_| ()),
            ] {
                assert_eq!(
                    result,
                    Err(IdError::ForbiddenChar {
                        ident: ident.clone(),
                        ch,
                    }),
                    "`{}` must be refused",
                    ch.escape_debug(),
                );
            }
        }
    }

    #[test]
    fn a_forbidden_control_character_is_quoted_as_its_escape() {
        let err = PlaceId::new("a\u{1}b").unwrap_err();
        assert_eq!(
            err.to_string(),
            "identifier `a\u{1}b` contains forbidden character `\\u{1}`",
        );
        // A printable character is quoted as itself, the separators too.
        let err = PlaceId::new("a\\b").unwrap_err();
        assert_eq!(
            err.to_string(),
            "identifier `a\\b` contains forbidden character `\\`",
        );
    }

    #[test]
    fn ident_serializes_transparently() {
        // `serde_norway` emits the scalar with no extra structure, matching
        // what a bare `String` would produce — the `#[serde(transparent)]`
        // wrapper does not add a tag.
        let json = serde_json::to_string(&PlaceId::new("home1").unwrap()).unwrap();
        assert_eq!(json, "\"home1\"");
    }

    #[test]
    fn ident_deserializes_through_validation() {
        let ok: PortId = serde_norway::from_str("entry").unwrap();
        assert_eq!(ok.as_str(), "entry");
        let err = serde_norway::from_str::<PortId>("foo.bar").unwrap_err();
        assert!(err.to_string().contains("forbidden character"));
    }

    fn endpoint(place: &str, port: &str) -> WalkwayEndpoint {
        WalkwayEndpoint {
            place: PlaceId::new(place).expect("place"),
            port: PortId::new(port).expect("port"),
        }
    }

    #[test]
    fn walkway_scope_key_round_trips() {
        let site = SiteName::new("hamlet").expect("site");
        let key = WalkwayScopeKey::from_parts(
            &site,
            &endpoint("home1", "entry"),
            &endpoint("home2", "entry"),
        )
        .expect("from_parts");
        assert_eq!(key.as_str(), "walkway::hamlet::home1.entry__home2.entry");
        let parsed = WalkwayScopeKey::parse(key.as_str()).expect("parse");
        assert_eq!(parsed, key);
        let parts = parsed.parts();
        assert_eq!(parts.site, "hamlet");
        assert_eq!(parts.from_place, "home1");
        assert_eq!(parts.from_port, "entry");
        assert_eq!(parts.to_place, "home2");
        assert_eq!(parts.to_port, "entry");
    }

    #[test]
    fn walkway_scope_key_parse_rejects_dot_in_port() {
        // Hypothetical silent-disaster input: a port id that contains a
        // `.` would split as PLACE=`home1`, PORT=`a.b` and round-trip
        // back through `from_parts` would alias with PLACE=`home1.a`,
        // PORT=`b`. The validator must reject it instead.
        let err = WalkwayScopeKey::parse("walkway::hamlet::home1.a.b__home2.entry").unwrap_err();
        match err {
            KeyParseError::InvalidSegment { segment, .. } => {
                // `from` split-once on `.` consumes the first `.`, so
                // the parsed port id is `a.b` which fails validation.
                assert_eq!(segment, "a.b");
            }
            other => panic!("expected InvalidSegment, got {other:?}"),
        }
    }

    #[test]
    fn walkway_scope_key_from_parts_rejects_dunder_in_segment() {
        // The lexer permits `__` in identifiers, but the canonical
        // scope key uses `__` as the `from`/`to` separator — so
        // `from_parts((home, b__c), (home2, entry))` and
        // `from_parts((home, b), (c__home2, entry))` would otherwise
        // both encode to `walkway::s::home.b__c__home2.entry`. The
        // validator surfaces this as a typed error so lowering can
        // emit a diagnostic on the originating `connect` row instead.
        let site = SiteName::new("s").expect("site");
        let err = WalkwayScopeKey::from_parts(
            &site,
            &endpoint("home", "b__c"),
            &endpoint("home2", "entry"),
        )
        .expect_err("port id with `__` must be rejected");
        match err {
            KeyConstructError::ConsecutiveUnderscore { role, segment } => {
                assert_eq!(role, KeySegmentRole::Port);
                assert_eq!(segment, "b__c");
            }
            other @ KeyConstructError::UnderscoreAtEdge { .. } => {
                panic!("expected ConsecutiveUnderscore, got {other:?}")
            }
        }
        // Symmetric: the same protection applies to place ids and the
        // site name.
        assert!(matches!(
            WalkwayScopeKey::from_parts(
                &SiteName::new("dunder__site").expect("site"),
                &endpoint("a", "x"),
                &endpoint("b", "y"),
            ),
            Err(KeyConstructError::ConsecutiveUnderscore {
                role: KeySegmentRole::Site,
                ..
            })
        ));
        assert!(matches!(
            WalkwayScopeKey::from_parts(&site, &endpoint("a__b", "x"), &endpoint("c", "y"),),
            Err(KeyConstructError::ConsecutiveUnderscore {
                role: KeySegmentRole::Place,
                ..
            })
        ));
    }

    #[test]
    fn walkway_scope_key_parse_rejects_dunder_in_segment() {
        // Symmetric to the `from_parts` test: a hand-crafted wire
        // string carrying `__` inside a segment fails parsing with the
        // typed `ConsecutiveUnderscore` variant. Without this guard,
        // the canonical encoding would alias two distinct typed
        // endpoint pairs.
        let err = WalkwayScopeKey::parse("walkway::s::a.b__c__d.e").unwrap_err();
        match err {
            KeyParseError::ConsecutiveUnderscore { segment, .. } => {
                // `endpoints.split_once("__")` consumes the first
                // `__`, so the parsed `to_place` is `c__d`.
                assert_eq!(segment, "c__d");
            }
            other => panic!("expected ConsecutiveUnderscore, got {other:?}"),
        }
    }

    #[test]
    fn walkway_scope_key_from_parts_rejects_edge_underscore_in_place_or_port() {
        // `(a, p_) → (b, p)` and `(a, p) → (_b, p)` both used to encode
        // to `walkway::s::a.p___b.p`: the edge `_` merges into the `__`
        // separator. Both ends of every place and port are refused, not
        // only the two that touch the separator in one direction.
        let site = SiteName::new("s").expect("site");
        let cases = [
            (
                endpoint("a", "p_"),
                endpoint("b", "p"),
                EndpointSegmentRole::Port,
                "p_",
            ),
            (
                endpoint("a", "p"),
                endpoint("_b", "p"),
                EndpointSegmentRole::Place,
                "_b",
            ),
            (
                endpoint("a_", "p"),
                endpoint("b", "p"),
                EndpointSegmentRole::Place,
                "a_",
            ),
            (
                endpoint("_a", "p"),
                endpoint("b", "p"),
                EndpointSegmentRole::Place,
                "_a",
            ),
            (
                endpoint("a", "_p"),
                endpoint("b", "p"),
                EndpointSegmentRole::Port,
                "_p",
            ),
            (
                endpoint("a", "p"),
                endpoint("b_", "p"),
                EndpointSegmentRole::Place,
                "b_",
            ),
            (
                endpoint("a", "p"),
                endpoint("b", "_p"),
                EndpointSegmentRole::Port,
                "_p",
            ),
            (
                endpoint("a", "p"),
                endpoint("b", "p_"),
                EndpointSegmentRole::Port,
                "p_",
            ),
            (
                endpoint("a", "_"),
                endpoint("b", "_"),
                EndpointSegmentRole::Port,
                "_",
            ),
        ];
        for (from, to, want_role, want_segment) in cases {
            match WalkwayScopeKey::from_parts(&site, &from, &to) {
                Err(KeyConstructError::UnderscoreAtEdge { role, segment }) => {
                    assert_eq!((role, segment.as_str()), (want_role, want_segment));
                }
                other => panic!("{from} → {to}: expected UnderscoreAtEdge, got {other:?}"),
            }
        }
        // The site sits between two `::` separators, which no identifier
        // can contain, so an edge `_` there cannot alias and stays legal.
        let key = WalkwayScopeKey::from_parts(
            &SiteName::new("_s_").expect("site"),
            &endpoint("a", "p"),
            &endpoint("b", "p"),
        )
        .expect("an edge `_` on the site is unambiguous");
        assert_eq!(key.as_str(), "walkway::_s_::a.p__b.p");
    }

    #[test]
    fn walkway_scope_key_parse_rejects_the_edge_underscore_aliases() {
        // The string both aliasing rows used to produce is refused on
        // the way back in, naming the segment the first `__` leaves
        // with a leading `_`.
        match WalkwayScopeKey::parse("walkway::s::a.p___b.p") {
            Err(KeyParseError::UnderscoreAtEdge { key, segment }) => {
                assert_eq!(key, "walkway::s::a.p___b.p");
                assert_eq!(segment, "_b");
            }
            other => panic!("expected UnderscoreAtEdge, got {other:?}"),
        }
        // A port named `_` splits into an empty port at the first `__`.
        assert!(matches!(
            WalkwayScopeKey::parse("walkway::s::a.___b._"),
            Err(KeyParseError::InvalidSegment {
                source: IdError::Empty,
                ..
            })
        ));
    }

    #[test]
    fn walkway_scope_key_from_parts_is_injective_and_parses_back() {
        // Exhaustive over endpoint segments of `a` and `_` up to three
        // long — the only character that can merge into the `__`
        // separator is `_`, and three is enough to put `_` at an edge, in
        // the middle, and doubled. The site is capped at two: `::` fences
        // it on both sides, so a third character buys nothing and would
        // more than double the run. Every key `from_parts` accepts must
        // parse back to the exact parts it was built from, which rules out
        // two accepted inputs sharing a key: `parse` is a function, so a
        // shared key would parse back to only one of them.
        let mut segments = vec![String::new()];
        let mut all = Vec::new();
        for _ in 0..3 {
            segments = segments
                .iter()
                .flat_map(|s| [format!("{s}a"), format!("{s}_")])
                .collect();
            all.extend(segments.iter().cloned());
        }
        let sites: Vec<&String> = all.iter().filter(|s| s.len() <= 2).collect();
        let mut accepted = 0_usize;
        let mut rejected_dunder = 0_usize;
        let mut rejected_edge = 0_usize;
        for site in &sites {
            let site_id = SiteName::new(site.as_str()).expect("site");
            for from_place in &all {
                for from_port in &all {
                    for to_place in &all {
                        for to_port in &all {
                            let from = endpoint(from_place, from_port);
                            let to = endpoint(to_place, to_port);
                            let key = match WalkwayScopeKey::from_parts(&site_id, &from, &to) {
                                Ok(key) => key,
                                Err(KeyConstructError::ConsecutiveUnderscore { .. }) => {
                                    rejected_dunder += 1;
                                    continue;
                                }
                                Err(KeyConstructError::UnderscoreAtEdge { .. }) => {
                                    rejected_edge += 1;
                                    continue;
                                }
                            };
                            accepted += 1;
                            let parsed = WalkwayScopeKey::parse(key.as_str())
                                .unwrap_or_else(|e| panic!("`{key}` does not parse back: {e}"));
                            let parts = parsed.parts();
                            assert_eq!(
                                (
                                    parts.site,
                                    parts.from_place,
                                    parts.from_port,
                                    parts.to_place,
                                    parts.to_port,
                                ),
                                (
                                    site.as_str(),
                                    from_place.as_str(),
                                    from_port.as_str(),
                                    to_place.as_str(),
                                    to_port.as_str(),
                                ),
                                "`{key}` parses back to different parts",
                            );
                        }
                    }
                }
            }
        }
        // Both refusal arms fired, so the edge rule is exercised and not
        // only the `__` one.
        assert!(rejected_dunder > 0 && rejected_edge > 0);
        // What survives: 5 sites (`a`, `_`, `aa`, `a_`, `_a`; `__` is
        // refused) times 4 endpoint segments (`a`, `aa`, `aaa`, `a_a`) to
        // the fourth power. A rule that narrowed or widened acceptance
        // moves this count.
        assert_eq!(accepted, 1280, "5 sites x 4^4 endpoint combinations");
    }

    #[test]
    fn walkway_scope_key_parse_rejects_missing_prefix() {
        assert!(matches!(
            WalkwayScopeKey::parse("struct::cottage"),
            Err(KeyParseError::MissingPrefix(_))
        ));
    }

    #[test]
    fn walkway_scope_key_parse_rejects_missing_site() {
        // `walkway::` followed by an `__`-separated endpoint pair with
        // no `::SITE::` in between hits the structural `MissingSite`
        // arm (the `split_once("__")` later picks up the joined
        // identifier-only form).
        assert!(matches!(
            WalkwayScopeKey::parse("walkway::home1.entry__home2.entry"),
            Err(KeyParseError::MissingSite(_))
        ));
    }

    #[test]
    fn walkway_scope_key_parse_rejects_missing_separator() {
        assert!(matches!(
            WalkwayScopeKey::parse("walkway::hamlet::home1.entry"),
            Err(KeyParseError::MissingFromToSeparator(_))
        ));
    }

    #[test]
    fn walkway_scope_key_parse_malformed_endpoint_carries_full_key() {
        let err = WalkwayScopeKey::parse("walkway::s::abc__d.e").unwrap_err();
        match err {
            KeyParseError::MalformedEndpoint { key, endpoint } => {
                assert_eq!(endpoint, "abc");
                assert_eq!(key, "walkway::s::abc__d.e");
            }
            other => panic!("expected MalformedEndpoint, got {other:?}"),
        }
    }

    #[test]
    fn walkway_scope_key_serde_round_trip() {
        let site = SiteName::new("hamlet").expect("site");
        let ep = endpoint("home1", "entry");
        let key = WalkwayScopeKey::from_parts(&site, &ep, &ep).expect("from_parts");
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, "\"walkway::hamlet::home1.entry__home1.entry\"");
        let parsed: WalkwayScopeKey = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, key);
    }
}
