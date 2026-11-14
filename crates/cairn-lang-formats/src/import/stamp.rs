//! The target an import is stamped with: the shipped version a file's saved version maps onto
//! (`spec/ecosystem-interop` "Import stamping and pitfalls").

use std::fmt;

use cairn_lang_core::Edition;

use crate::build::BuildTarget;
use crate::registry::{DataVersionEntry, builtin_pack};

/// How the stamped target stands to the version a file was saved at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StampFit {
    /// A shipped target is that version.
    Exact,
    /// The version falls between two shipped targets, and the later is stamped.
    Later,
    /// The version is older than every shipped target, and the oldest is stamped.
    Oldest,
}

/// The `(edition, version)` an import is stamped with, and what the file said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportStamp {
    /// The edition the file's format belongs to.
    pub edition: Edition,
    /// The shipped version stamped.
    pub mc_version: String,
    /// The version the file was saved at: its `DataVersion` on Java, its newest block-state
    /// `version` on Bedrock; `None` for a `.mcstructure` whose palette carries none.
    pub saved: Option<i32>,
    /// How [`Self::mc_version`] stands to [`Self::saved`].
    pub fit: StampFit,
}

impl ImportStamp {
    /// The saved version as a reader names it: `DataVersion 3955 (1.21.1)`, or
    /// `block version 1.21.60.33`.
    #[must_use]
    pub fn saved_label(&self) -> String {
        match (self.edition, self.saved) {
            (_, None) => "no block version".to_owned(),
            (Edition::Java, Some(data_version)) => {
                let release = builtin_pack(Edition::Java)
                    .data_versions()
                    .versions
                    .iter()
                    .find(|row| row.data_version == data_version);
                match release {
                    Some(row) => format!("DataVersion {data_version} ({})", row.mc_version),
                    None => format!("DataVersion {data_version}"),
                }
            }
            (Edition::Bedrock, Some(version)) => {
                format!("block version {}", BlockVersion(version))
            }
        }
    }

    /// The target a source is built at to compare it with the file.
    ///
    /// # Panics
    ///
    /// When [`Self::mc_version`] is no row of its edition's version table, which no import
    /// stamps.
    #[must_use]
    pub fn build_target(&self) -> BuildTarget<'_> {
        let order_key = builtin_pack(self.edition)
            .version_order()
            .key_of(&self.mc_version)
            .expect("an import stamps a row of its edition's version table");
        BuildTarget {
            mc_version: &self.mc_version,
            order_key,
        }
    }

    /// One line saying why [`Self::mc_version`] was stamped.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match (self.fit, self.saved, self.edition) {
            (_, None, _) => "the latest shipped target, since the file names no version",
            (StampFit::Exact, Some(_), Edition::Java) => "the shipped target the file was saved at",
            (StampFit::Exact, Some(_), Edition::Bedrock) => {
                "a shipped target that saves that block version"
            }
            (StampFit::Later, Some(_), _) => {
                "the nearest shipped target after the version the file was saved at"
            }
            (StampFit::Oldest, Some(_), _) => {
                "the earliest shipped target, since the file was saved before every one"
            }
        }
    }
}

/// A packed Bedrock block-state version, printed `major.minor.patch.revision`.
struct BlockVersion(i32);

impl fmt::Display for BlockVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [major, minor, patch, revision] = self.0.to_be_bytes();
        write!(f, "{major}.{minor}.{patch}.{revision}")
    }
}

/// A saved version newer than every shipped target of its edition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnshippedVersion {
    /// The edition.
    pub edition: Edition,
    /// The saved version, as [`ImportStamp::saved_label`] names it.
    pub saved: String,
    /// The newest shipped target.
    pub newest: String,
}

/// The shipped targets of `edition` whose key, by `key`, is `saved` or the nearest after it, and
/// how they fit: those at `saved`, else the oldest after it, else the oldest of all.
///
/// # Errors
///
/// [`UnshippedVersion`] when `saved` is past every shipped target.
pub(super) fn candidates(
    edition: Edition,
    saved: i32,
    key: impl Fn(&DataVersionEntry) -> Option<i32>,
) -> Result<(Vec<String>, StampFit), UnshippedVersion> {
    let rows: Vec<(i32, &str)> = builtin_pack(edition)
        .targetable()
        .into_iter()
        .filter_map(|row| Some((key(row)?, row.mc_version.as_str())))
        .collect();
    let at = |wanted: i32| -> Vec<String> {
        rows.iter()
            .filter(|(k, _)| *k == wanted)
            .map(|(_, version)| (*version).to_owned())
            .collect()
    };
    let exact = at(saved);
    if !exact.is_empty() {
        return Ok((exact, StampFit::Exact));
    }
    let oldest = rows.iter().map(|(k, _)| *k).min();
    if oldest.is_some_and(|oldest| saved < oldest) {
        return Ok((at(oldest.expect("checked just above")), StampFit::Oldest));
    }
    if let Some(later) = rows.iter().map(|(k, _)| *k).filter(|k| *k > saved).min() {
        return Ok((at(later), StampFit::Later));
    }
    let label = ImportStamp {
        edition,
        mc_version: String::new(),
        saved: Some(saved),
        fit: StampFit::Exact,
    }
    .saved_label();
    let newest = rows
        .iter()
        .max_by_key(|(k, _)| *k)
        .map_or_else(String::new, |(_, version)| (*version).to_owned());
    Err(UnshippedVersion {
        edition,
        saved: label,
        newest,
    })
}
