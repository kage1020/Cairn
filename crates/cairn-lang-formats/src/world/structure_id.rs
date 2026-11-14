//! The id `/place template` and a structure block load a structure by, held to the rules the game
//! reads a generated structure's path under.

use std::fmt;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// The namespace the game reads an id without one in.
pub const DEFAULT_NAMESPACE: &str = "minecraft";

/// Stems the game refuses as a path segment, with or without an extension after them.
const RESERVED_STEMS: [&str; 5] = ["con", "prn", "aux", "nul", "com"];

/// Stems the game refuses followed by one digit from `1` to `9`.
const RESERVED_NUMBERED_STEMS: [&str; 2] = ["com", "lpt"];

/// A structure id, `namespace:name`, whose `name` may hold `/`-separated folders.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructureId {
    namespace: String,
    name: String,
}

/// Which side of the `:` in a [`StructureId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdPart {
    /// Before the `:`.
    Namespace,
    /// After the `:`.
    Name,
}

impl fmt::Display for IdPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Namespace => "namespace",
            Self::Name => "name",
        })
    }
}

/// Why a namespace and name make no structure id the game loads.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum StructureIdError {
    /// The part is empty.
    #[error("the {part} is empty")]
    Empty {
        /// The empty part.
        part: IdPart,
    },
    /// A character outside `a`-`z`, `0`-`9`, `_`, `-` and `.`, and `/` in a name.
    #[error("the {part} `{value}` holds `{ch}`")]
    Character {
        /// The part holding it.
        part: IdPart,
        /// The part as given.
        value: String,
        /// The first such character.
        ch: char,
    },
    /// A name that starts or ends with `/`, or holds `//`.
    #[error("the name `{name}` has an empty folder or file name")]
    EmptySegment {
        /// The name as given.
        name: String,
    },
    /// A segment ending in `.`, which `.` and `..` both do.
    #[error("`{segment}` in the {part} ends in `.`")]
    TrailingDot {
        /// The part holding it.
        part: IdPart,
        /// The segment.
        segment: String,
    },
    /// A segment Windows reserves as a device name, which the game refuses on every platform.
    #[error("`{segment}` in the {part} names a device Windows reserves")]
    DeviceName {
        /// The part holding it.
        part: IdPart,
        /// The segment.
        segment: String,
    },
}

impl StructureIdError {
    /// The part that breaks the rule.
    #[must_use]
    pub fn part(&self) -> IdPart {
        match self {
            Self::Empty { part }
            | Self::Character { part, .. }
            | Self::TrailingDot { part, .. }
            | Self::DeviceName { part, .. } => *part,
            Self::EmptySegment { .. } => IdPart::Name,
        }
    }
}

impl StructureId {
    /// The id `namespace:name`.
    ///
    /// # Errors
    ///
    /// [`StructureIdError`], naming the first rule `namespace` or `name` breaks.
    pub fn new(namespace: &str, name: &str) -> Result<Self, StructureIdError> {
        check_part(IdPart::Namespace, namespace)?;
        check_part(IdPart::Name, name)?;
        Ok(Self {
            namespace: namespace.to_owned(),
            name: name.to_owned(),
        })
    }

    /// The part before the `:`.
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// The part after the `:`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The file under the Java world folder `world` the game loads this id from:
    /// `generated/<namespace>/structures/<name>.nbt`.
    #[must_use]
    pub fn file_in(&self, world: &Path) -> PathBuf {
        let mut path = world
            .join("generated")
            .join(&self.namespace)
            .join("structures");
        path.extend(self.name.split('/'));
        path.as_mut_os_string().push(".nbt");
        path
    }
}

impl fmt::Display for StructureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.name)
    }
}

fn check_part(part: IdPart, value: &str) -> Result<(), StructureIdError> {
    if value.is_empty() {
        return Err(StructureIdError::Empty { part });
    }
    let allowed = |ch: char| {
        matches!(ch, 'a'..='z' | '0'..='9' | '_' | '-' | '.') || (ch == '/' && part == IdPart::Name)
    };
    if let Some(ch) = value.chars().find(|&ch| !allowed(ch)) {
        return Err(StructureIdError::Character {
            part,
            value: value.to_owned(),
            ch,
        });
    }
    for segment in value.split('/') {
        if segment.is_empty() {
            return Err(StructureIdError::EmptySegment {
                name: value.to_owned(),
            });
        }
        let segment = segment.to_owned();
        if segment.ends_with('.') {
            return Err(StructureIdError::TrailingDot { part, segment });
        }
        if is_reserved(&segment) {
            return Err(StructureIdError::DeviceName { part, segment });
        }
    }
    Ok(())
}

/// Whether the game refuses `segment`, whose stem runs to its first `.`.
fn is_reserved(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or(segment);
    RESERVED_STEMS.contains(&stem)
        || RESERVED_NUMBERED_STEMS.iter().any(|device| {
            stem.strip_prefix(device)
                .is_some_and(|digit| matches!(digit.as_bytes(), [b'1'..=b'9']))
        })
}
