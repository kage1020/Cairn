//! Newtype wrappers for Cairn identifiers shared across the resolver, the
//! block-array IR, and the lockfile DTOs.
//!
//! Each newtype carries the invariants the surface lexer already
//! establishes (non-empty, no `.`, no `:`, no whitespace) so downstream
//! layers cannot accidentally pass a connect endpoint such as
//! `home.1.entry` and have the walkway scope key silently re-parse as a
//! different `(place, port)` pair. The wire format is unchanged: every
//! newtype is `#[serde(transparent)]` over its internal `String`, so any
//! YAML / JSON consumer keeps seeing the same scalar string it used to.
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
    /// that is reserved as a structural separator (`.`, `:`) or that the
    /// surface lexer would not have produced (whitespace).
    #[error("identifier `{ident}` contains forbidden character `{ch}`")]
    ForbiddenChar {
        /// The full offending string.
        ident: String,
        /// The first character that triggered the rejection.
        ch: char,
    },
}

fn validate_ident(s: &str) -> Result<(), IdError> {
    if s.is_empty() {
        return Err(IdError::Empty);
    }
    for c in s.chars() {
        if c == '.' || c == ':' || c.is_whitespace() {
            return Err(IdError::ForbiddenChar {
                ident: s.to_owned(),
                ch: c,
            });
        }
    }
    Ok(())
}

fn check_no_dunder(role: &'static str, s: &str) -> Result<(), KeyConstructError> {
    if s.contains("__") {
        return Err(KeyConstructError::ConsecutiveUnderscore {
            role,
            segment: s.to_owned(),
        });
    }
    Ok(())
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
            /// the structural separators downstream lookups rely on).
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
}

/// Failure modes for [`WalkwayScopeKey::from_parts`].
///
/// The only reason a typed construction can fail is that one of the
/// segments contains the `__` separator substring. Surface lexer rules
/// allow `_` in identifiers, so a place / port id can legally be e.g.
/// `home__1`; lowering must convert that into a diagnostic on the
/// originating `connect` row rather than emit a wire-ambiguous scope
/// key (the `parse`/`from_parts` round-trip would otherwise rebind
/// `(home, __1__home2, entry)` to `(home, _, _1_home2.entry)` style
/// pairs).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyConstructError {
    /// A segment contains the `__` substring.
    #[error(
        "walkway scope key segment `{segment}` contains `__`, which collides with the \
         `from`/`to` separator; rename the {role} so the lowered key is unambiguous"
    )]
    ConsecutiveUnderscore {
        /// Which role the offending segment plays (`"site"`, `"place"`, `"port"`).
        role: &'static str,
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
    /// segment contains the `__` substring. The surface lexer allows
    /// `_` freely in identifiers, so a user-typed place / port id may
    /// validly contain `__` — but the canonical scope key uses `__` as
    /// the `from`/`to` separator, so `(home, b__c, home2, entry)` and
    /// `(home, b, c__home2, entry)` would both encode to the same
    /// string. Lowering must surface this back to the user as a
    /// diagnostic on the originating `connect` row rather than emit a
    /// silent alias.
    pub fn from_parts(
        site: &SiteName,
        from: &WalkwayEndpoint,
        to: &WalkwayEndpoint,
    ) -> Result<Self, KeyConstructError> {
        check_no_dunder("site", site.as_str())?;
        check_no_dunder("place", from.place.as_str())?;
        check_no_dunder("port", from.port.as_str())?;
        check_no_dunder("place", to.place.as_str())?;
        check_no_dunder("port", to.port.as_str())?;
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
    /// (e.g. a port id containing `.` or `__`).
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
        // `check_no_dunder` is satisfied above; map the
        // construct-error variant onto the parse-error variant rather
        // than panic, even though it cannot fire here.
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
        .map_err(|KeyConstructError::ConsecutiveUnderscore { segment, .. }| dunder(&segment))
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn ident_serializes_transparently() {
        // `serde_yml` emits the scalar with no extra structure, matching
        // what a bare `String` would produce — the `#[serde(transparent)]`
        // wrapper does not add a tag.
        let json = serde_json::to_string(&PlaceId::new("home1").unwrap()).unwrap();
        assert_eq!(json, "\"home1\"");
    }

    #[test]
    fn ident_deserializes_through_validation() {
        let ok: PortId = serde_yml::from_str("entry").unwrap();
        assert_eq!(ok.as_str(), "entry");
        let err = serde_yml::from_str::<PortId>("foo.bar").unwrap_err();
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
        let key =
            WalkwayScopeKey::from_parts(&site, &endpoint("home1", "entry"), &endpoint("home2", "entry"))
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
                assert_eq!(role, "port");
                assert_eq!(segment, "b__c");
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
            Err(KeyConstructError::ConsecutiveUnderscore { role: "site", .. })
        ));
        assert!(matches!(
            WalkwayScopeKey::from_parts(
                &site,
                &endpoint("a__b", "x"),
                &endpoint("c", "y"),
            ),
            Err(KeyConstructError::ConsecutiveUnderscore { role: "place", .. })
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
