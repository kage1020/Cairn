//! `--target <mc_version>` → Minecraft `DataVersion` resolution table.
//!
//! Java structure NBT carries a `DataVersion` integer that the loading
//! client uses to know whether and how to run DFU upgrades. The mapping
//! between human-facing `1.21.4` and the integer `3955` is owned by
//! Mojang's source; we hardcode the subset the M2 examples target. The
//! registry pack ingest in 2026.12.0 replaces this constant table with
//! values pulled from a versioned data file.

use thiserror::Error;

/// Resolved Minecraft target. Held by value (no `String`) so passing a
/// target around the backend is cheap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaTarget {
    /// Human-facing version string, e.g. `"1.21.4"`.
    pub mc_version: &'static str,
    /// `DataVersion` integer written into the structure NBT root.
    pub data_version: i32,
}

/// `--target` value did not match any supported version.
#[derive(Debug, Error)]
#[error("unsupported java target `{requested}`. supported in this release: {supported}")]
pub struct UnsupportedTarget {
    /// Verbatim value passed via `--target`.
    pub requested: String,
    /// Pre-formatted list of supported versions, ready to print.
    pub supported: String,
}

/// Supported Java targets, in ascending version order. Add a row when a new
/// `DataVersion` is published; remove a row only at a `CalVer` major (none
/// planned yet).
///
/// Sources:
/// - 1.20.4 → `3700` (Mojang release thread, Dec 2023)
/// - 1.21.0 → `3953` (release notes, June 2024)
/// - 1.21.4 → `4189` (release notes, Dec 2024)
const JAVA_TARGETS: &[JavaTarget] = &[
    JavaTarget {
        mc_version: "1.20.4",
        data_version: 3700,
    },
    JavaTarget {
        mc_version: "1.21",
        data_version: 3953,
    },
    JavaTarget {
        mc_version: "1.21.4",
        data_version: 4189,
    },
];

/// `"latest"` alias resolves to this row. Held as a separate const so the
/// resolver does not have to take `.last()` of a slice at runtime — that
/// expression would have to defend against an empty table at every call,
/// even though `JAVA_TARGETS` is statically guaranteed non-empty.
const LATEST_TARGET: JavaTarget = JAVA_TARGETS[JAVA_TARGETS.len() - 1];

/// Alias accepted in addition to the exact `mc_version` strings.
const LATEST: &str = "latest";

/// Resolve a `--target` string against the hardcoded table. The literal
/// `"latest"` aliases the last entry, matching the working assumption that
/// "no explicit `--target`" means "the newest supported".
///
/// # Errors
///
/// Returns [`UnsupportedTarget`] when the requested string is neither an
/// exact `mc_version` match nor the `"latest"` alias.
pub fn resolve_java_target(requested: &str) -> Result<JavaTarget, UnsupportedTarget> {
    if requested == LATEST {
        return Ok(LATEST_TARGET);
    }
    if let Some(t) = JAVA_TARGETS.iter().find(|t| t.mc_version == requested) {
        return Ok(*t);
    }
    Err(UnsupportedTarget {
        requested: requested.to_owned(),
        supported: supported_list(),
    })
}

/// Human-readable list of supported `mc_version` strings, joined for error
/// messages. Public so the CLI can print the same list in `--help`-style
/// contexts without re-deriving it.
#[must_use]
pub fn supported_list() -> String {
    let mut entries: Vec<String> = JAVA_TARGETS
        .iter()
        .map(|t| t.mc_version.to_owned())
        .collect();
    entries.push(LATEST.to_owned());
    entries.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_known_version() {
        let t = resolve_java_target("1.21.4").expect("known version");
        assert_eq!(t.mc_version, "1.21.4");
        assert_eq!(t.data_version, 4189);
    }

    #[test]
    fn resolves_latest_alias_to_last_entry() {
        let latest = resolve_java_target("latest").expect("latest");
        let last = *JAVA_TARGETS.last().expect("table non-empty");
        assert_eq!(latest, last);
    }

    #[test]
    fn rejects_unknown_version() {
        let err = resolve_java_target("0.0.0").expect_err("unknown");
        assert_eq!(err.requested, "0.0.0");
        assert!(err.supported.contains("1.20.4"));
        assert!(err.supported.contains("latest"));
    }
}
