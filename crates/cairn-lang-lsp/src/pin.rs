//! What the client pins a document's check to, read from the `cairn.edition` and `cairn.target`
//! settings a `workspace/didChangeConfiguration` carries (`spec/lint` "Materials and targets").

use cairn_lang_core::Edition;
use cairn_lang_formats::{resolve_bedrock_target, resolve_java_target};

/// The settings section the server reads.
const SECTION: &str = "cairn";

/// What a document's diagnostics are computed against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Pin {
    /// No edition, as `cairn check` with no `--edition`.
    #[default]
    Nothing,
    /// One edition and no version, as `cairn check --edition E`.
    Edition(Edition),
    /// A target, as `cairn compile --edition E --target V` up to what it writes.
    Target {
        /// The target's edition.
        edition: Edition,
        /// The version `cairn.target` resolves to in the edition's table.
        version: String,
    },
}

/// A setting the server refused, and the pin the rest of the settings ask for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinRefused {
    /// Why, as the sentence the client shows.
    pub reason: String,
    /// What the server pins instead.
    pub pinned: Pin,
}

impl Pin {
    /// The pin `settings`, the `settings` of a `workspace/didChangeConfiguration`, asks for.
    ///
    /// # Errors
    ///
    /// A [`PinRefused`] for an edition that is not one, a target without an edition, or a target
    /// the edition does not ship.
    pub fn from_settings(settings: &serde_json::Value) -> Result<Self, PinRefused> {
        let edition = match setting(settings, "edition") {
            Err(reason) => {
                return Err(PinRefused {
                    reason,
                    pinned: Self::Nothing,
                });
            }
            Ok(None) => None,
            Ok(Some(name)) => match name.parse::<Edition>() {
                Ok(edition) => Some(edition),
                Err(err) => {
                    return Err(PinRefused {
                        reason: format!("`{SECTION}.edition`: {err}"),
                        pinned: Self::Nothing,
                    });
                }
            },
        };
        let unversioned = edition.map_or(Self::Nothing, Self::Edition);
        let target = match setting(settings, "target") {
            Err(reason) => {
                return Err(PinRefused {
                    reason,
                    pinned: unversioned,
                });
            }
            Ok(target) => target,
        };
        match (edition, target) {
            (_, None) => Ok(unversioned),
            // As `cairn check --target` without `--edition` is (`spec/compilation` "Target axes").
            (None, Some(target)) => Err(PinRefused {
                reason: format!(
                    "`{SECTION}.target` is `{target}`, but `{SECTION}.edition` names no edition \
                     to read it in: set `{SECTION}.edition` to java or bedrock"
                ),
                pinned: Self::Nothing,
            }),
            (Some(edition), Some(target)) => {
                let resolved = match edition {
                    Edition::Java => resolve_java_target(target).map(|t| t.mc_version),
                    Edition::Bedrock => resolve_bedrock_target(target).map(|t| t.mc_version),
                };
                resolved
                    .map(|version| Self::Target { edition, version })
                    .map_err(|err| PinRefused {
                        reason: format!("`{SECTION}.target`: {err}"),
                        pinned: unversioned,
                    })
            }
        }
    }
}

/// The string `settings` gives `cairn.KEY`, `None` when it is absent, `null` or empty.
fn setting<'a>(settings: &'a serde_json::Value, key: &str) -> Result<Option<&'a str>, String> {
    match settings.get(SECTION).and_then(|section| section.get(key)) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => {
            Ok(Some(value.trim()).filter(|value| !value.is_empty()))
        }
        Some(other) => Err(format!(
            "`{SECTION}.{key}` is {other}, which is not a string; the setting was ignored"
        )),
    }
}

#[cfg(test)]
mod tests;
