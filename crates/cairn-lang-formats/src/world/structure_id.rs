//! The id `/place template` and a structure block load a structure by, held to the rules the game
//! reads a generated structure's path under.

use std::fmt;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::level_dat::WorldVersion;

/// The namespace the game reads an id without one in.
pub const DEFAULT_NAMESPACE: &str = "minecraft";

/// Stems the game refuses as a path segment, with or without an extension after them.
const RESERVED_STEMS: [&str; 5] = ["con", "prn", "aux", "nul", "com"];

/// Stems the game refuses followed by one digit from `1` to `9`.
const RESERVED_NUMBERED_STEMS: [&str; 2] = ["com", "lpt"];

/// The first `DataVersion`, 26.1 Snapshot 6's, whose world loads a generated structure from
/// `structure/`; an older world loads it from `structures/`.
const STRUCTURE_FOLDER_FROM: i32 = 4774;

/// A namespace the game loads a structure id in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Namespace(String);

/// A structure id, `namespace:name`, whose `name` may hold `/`-separated folders.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructureId {
    namespace: Namespace,
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

impl IdPart {
    /// Whether the game reads `ch` in this part.
    #[must_use]
    pub fn allows(self, ch: char) -> bool {
        matches!(ch, 'a'..='z' | '0'..='9' | '_' | '-' | '.') || (ch == '/' && self == Self::Name)
    }

    /// Checks `value` against the rules the game holds this part to.
    ///
    /// # Errors
    ///
    /// [`StructureIdError`], naming the first rule `value` breaks.
    pub fn check(self, value: &str) -> Result<(), StructureIdError> {
        if value.is_empty() {
            return Err(StructureIdError::Empty { part: self });
        }
        if let Some(ch) = value.chars().find(|&ch| !self.allows(ch)) {
            return Err(StructureIdError::Character {
                part: self,
                value: value.to_owned(),
                ch,
            });
        }
        let segments: Vec<&str> = value.split('/').collect();
        for (at, segment) in segments.iter().enumerate() {
            if segment.is_empty() {
                return Err(StructureIdError::EmptySegment {
                    name: value.to_owned(),
                });
            }
            // A name's last segment gets `.nbt` after it, so only a folder's name ends in `.`.
            let folder = self == Self::Namespace || at + 1 < segments.len();
            if folder && segment.ends_with('.') {
                return Err(StructureIdError::TrailingDot {
                    part: self,
                    segment: (*segment).to_owned(),
                });
            }
            if is_reserved(segment) {
                return Err(StructureIdError::DeviceName {
                    part: self,
                    segment: (*segment).to_owned(),
                });
            }
        }
        Ok(())
    }
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
    /// A folder's name ending in `.`, which `.` and `..` both do.
    #[error("the folder `{segment}` in the {part} ends in `.`")]
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

impl Namespace {
    /// The namespace `value`.
    ///
    /// # Errors
    ///
    /// [`StructureIdError`], naming the first rule `value` breaks.
    pub fn new(value: &str) -> Result<Self, StructureIdError> {
        IdPart::Namespace.check(value)?;
        Ok(Self(value.to_owned()))
    }

    /// The namespace as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl StructureId {
    /// The id `namespace:name`.
    ///
    /// # Errors
    ///
    /// [`StructureIdError`], naming the first rule `name` breaks.
    pub fn new(namespace: &Namespace, name: &str) -> Result<Self, StructureIdError> {
        IdPart::Name.check(name)?;
        Ok(Self {
            namespace: namespace.clone(),
            name: name.to_owned(),
        })
    }

    /// The part before the `:`.
    #[must_use]
    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    /// The part after the `:`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The file under the Java world folder `world`, saved at `version`, that the game loads this
    /// id from: `generated/<namespace>/structure/<name>.nbt`, in `structures/` before 26.1
    /// Snapshot 6.
    #[must_use]
    pub fn file_in(&self, world: &Path, version: &WorldVersion) -> PathBuf {
        let folder = if version.data_version >= STRUCTURE_FOLDER_FROM {
            "structure"
        } else {
            "structures"
        };
        let mut path = world
            .join("generated")
            .join(self.namespace.as_str())
            .join(folder);
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

/// Whether the game refuses `segment`, whose stem runs to its first `.`.
fn is_reserved(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or(segment);
    RESERVED_STEMS.contains(&stem)
        || RESERVED_NUMBERED_STEMS.iter().any(|device| {
            stem.strip_prefix(device)
                .is_some_and(|digit| matches!(digit.as_bytes(), [b'1'..=b'9']))
        })
}
