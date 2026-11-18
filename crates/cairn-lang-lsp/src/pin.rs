//! What the client pins a document's check to, read from the `cairn.edition` and `cairn.target`
//! settings a `workspace/didChangeConfiguration` carries (`spec/lint` "Materials and targets").

use cairn_lang_core::Edition;
use cairn_lang_formats::{ResolvedTarget, resolve_target};

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
    Target(PinnedTarget),
}

/// A target the settings name, resolved in its edition's table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedTarget(ResolvedTarget);

impl PinnedTarget {
    /// The target's edition.
    #[must_use]
    pub fn edition(&self) -> Edition {
        self.0.edition()
    }

    /// The version `cairn.target` resolves to in the edition's table.
    #[must_use]
    pub fn version(&self) -> &str {
        self.0.mc_version()
    }
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
    /// A [`PinRefused`] for a setting `spec/lint` "Materials and targets" refuses.
    pub fn from_settings(settings: &serde_json::Value) -> Result<Self, PinRefused> {
        let section = match settings.get(SECTION) {
            None | Some(serde_json::Value::Null) => &serde_json::Value::Null,
            Some(section @ serde_json::Value::Object(_)) => section,
            Some(other) => {
                return Err(PinRefused {
                    reason: format!(
                        "`{SECTION}` is {other}, which is not an object; the settings were ignored"
                    ),
                    pinned: Self::Nothing,
                });
            }
        };
        let edition = match setting(section, "edition") {
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
        let target = match setting(section, "target") {
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
            (Some(edition), Some(target)) => resolve_target(edition, target)
                .map(|resolved| Self::Target(PinnedTarget(resolved)))
                .map_err(|err| PinRefused {
                    reason: format!("`{SECTION}.target`: {err}"),
                    pinned: unversioned,
                }),
        }
    }
}

/// The string `section` gives `KEY`, `None` when it is absent, `null` or blank.
fn setting<'a>(section: &'a serde_json::Value, key: &str) -> Result<Option<&'a str>, String> {
    match section.get(key) {
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
