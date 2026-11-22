//! The target an import is stamped with: the shipped version a file's saved version maps onto
//! (`spec/ecosystem-interop` "Import stamping and pitfalls").

use std::fmt;

use cairn_lang_core::Edition;

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

/// The version a file was saved at, and how the stamped target stands to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedVersion {
    /// Its `DataVersion` on Java, its newest block-state `version` on Bedrock.
    pub version: i32,
    /// How the stamped target stands to it.
    pub fit: StampFit,
}

/// The shipped `(edition, version)` an import is stamped with, and what the file said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportStamp {
    edition: Edition,
    mc_version: String,
    saved: Option<SavedVersion>,
}

impl ImportStamp {
    /// The stamp of `edition`'s shipped `mc_version` for a file saved at `saved`.
    pub(super) fn new(edition: Edition, mc_version: String, saved: Option<SavedVersion>) -> Self {
        Self {
            edition,
            mc_version,
            saved,
        }
    }

    /// The edition the file's format belongs to.
    #[must_use]
    pub fn edition(&self) -> Edition {
        self.edition
    }

    /// The shipped version stamped.
    #[must_use]
    pub fn mc_version(&self) -> &str {
        &self.mc_version
    }

    /// The version the file was saved at, `None` for a `.mcstructure` whose palette carries none,
    /// which is stamped with the latest shipped target.
    #[must_use]
    pub fn saved(&self) -> Option<SavedVersion> {
        self.saved
    }

    /// The saved version as a reader names it: `DataVersion 3955 (1.21.1)`, or
    /// `block version 1.21.60.33`.
    #[must_use]
    pub fn saved_label(&self) -> String {
        saved_label(self.edition, self.saved.map(|saved| saved.version))
    }

    /// One line saying why [`Self::mc_version`] was stamped.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        let Some(saved) = self.saved else {
            return "the latest shipped target, since the file names no version";
        };
        match (saved.fit, self.edition) {
            (StampFit::Exact, Edition::Java) => "the shipped target the file was saved at",
            (StampFit::Exact, Edition::Bedrock) => "a shipped target that saves that block version",
            (StampFit::Later, _) => {
                "the nearest shipped target after the version the file was saved at"
            }
            (StampFit::Oldest, _) => {
                "the earliest shipped target, since the file was saved before every one"
            }
        }
    }
}

/// `edition`'s version `saved` as a reader names it.
fn saved_label(edition: Edition, saved: Option<i32>) -> String {
    match (edition, saved) {
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
        (Edition::Bedrock, Some(version)) => format!("block version {}", BlockVersion(version)),
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

/// The shipped targets of `edition` at `saved` by `key`, else at the nearest key after it, with
/// how they fit.
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
    let newest = rows
        .iter()
        .max_by_key(|(k, _)| *k)
        .map_or_else(String::new, |(_, version)| (*version).to_owned());
    Err(UnshippedVersion {
        edition,
        saved: saved_label(edition, Some(saved)),
        newest,
    })
}
