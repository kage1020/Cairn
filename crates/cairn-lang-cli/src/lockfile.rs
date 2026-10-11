//! The lockfile a compile writes, and what it says about the one it
//! replaces.

use std::path::{Path, PathBuf};

use cairn_lang_core::CAIRN_VERSION;
use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::lock::{
    HashHex, LOCK_SCHEMA_VERSION, LockEdition, LockError, LockInputs, LockPlacement, LockTarget,
    LockWalkway, Lockfile, MemberSensitivity, hash_resolved_ir, hash_source,
};

use crate::cli::EditionArg;
use crate::target::ResolvedTarget;

/// Compare the lockfile at `lock_path` with the target being built, and
/// report the divergence the way `spec/versioning-editions`
/// "Provenance and lock" does.
///
/// The lockfile is the record of what was verified, so a recompile for a
/// different target is the moment that record stops describing what is on
/// disk. Nothing here changes the build or the exit code — both lines are
/// warnings, and a first compile or an unchanged target says nothing at
/// all.
///
/// One lockfile the strict read refuses is still compared: one whose only
/// fault is an identifier the rule has since refused, which an earlier
/// Cairn's looser rule let through. [`Lockfile::refused_identifier`] reads
/// it again with exactly the strict read's rules but for its identifiers,
/// so the target it records is the one a valid document records. Any other
/// lockfile that cannot be read says so; only its absence is silent.
pub(crate) fn report_previous_target(
    lock_path: &Path,
    edition: EditionArg,
    target: &ResolvedTarget,
) {
    // Read once, here, rather than through `Lockfile::read_from_path`: a
    // document the strict read refuses is parsed a second time below, and
    // reading the file again for it would let the two parses see different
    // bytes.
    let body = match std::fs::read_to_string(lock_path) {
        Ok(body) => body,
        // No lockfile is the ordinary first-compile case, and the only one
        // that should be silent. Testing `exists()` first would fold a
        // permission error into it, and leave a window in which the file
        // vanishes between the check and the read.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => {
            warn_unreadable_lock(lock_path, &LockError::from(err));
            return;
        }
    };
    let previous = match Lockfile::from_yaml(&body) {
        Ok(previous) => previous,
        // A document from a newer Cairn is not corrupt, and replacing it
        // does lose something, so it says so in its own words.
        Err(LockError::UnsupportedSchemaVersion { found, supported }) => {
            eprintln!(
                "warning: {}: the existing lockfile is schema version {found} and this build \
                 reads {supported}; it was written by a newer Cairn and is being replaced",
                lock_path.display(),
            );
            return;
        }
        Err(err) => {
            match Lockfile::refused_identifier(&body) {
                // Nor is a document valid in everything but an identifier
                // the rule has since refused. The target it records is
                // read as strictly as a valid document's, so a target
                // change is still reported.
                Ok(Some(refused)) => {
                    eprintln!(
                        "warning: {}: the existing lockfile records an identifier this build \
                         refuses ({}) but is otherwise valid, as one an earlier Cairn wrote \
                         would be; it is being replaced",
                        lock_path.display(),
                        refused.error,
                    );
                    report_target_change(
                        &refused.target,
                        &refused.member_version_sensitivity,
                        edition,
                        target,
                    );
                }
                // The second parse names what is wrong besides any
                // identifier; the strict read's own error can be an
                // identifier it met first, which is not why this document
                // cannot be read.
                Err(cause) => warn_unreadable_lock(lock_path, &cause),
                // The second parse took the whole document, so the strict
                // read should have too. Unreachable while the probe mirrors
                // the strict schema; if they drift, the strict read's error
                // is the one there is to report.
                Ok(None) => warn_unreadable_lock(lock_path, &err),
            }
            return;
        }
    };
    report_target_change(
        &previous.target,
        &previous.member_version_sensitivity,
        edition,
        target,
    );
}

/// Say that the lockfile at `lock_path` is being replaced unread, because
/// reading it failed with `err`: an I/O error (no permission, a directory
/// at the path, bytes that are not UTF-8) or a document that is not a
/// lockfile this build reads.
///
/// A warning, not an error: the compile is valid, and an unreadable file
/// beside the source is no reason to refuse to build. Replacing it in
/// silence is what it must not do, since that is how a tampered or stale
/// lockfile would go unnoticed.
fn warn_unreadable_lock(lock_path: &Path, err: &LockError) {
    eprintln!(
        "warning: {}: the existing lockfile could not be read ({err}); replacing it",
        lock_path.display(),
    );
}

/// The `W_PREVIOUSLY_VERIFIED_TARGET` / `W_SEMANTIC_SENSITIVITY` half of
/// [`report_previous_target`]: `verified` and `sensitive` are what the
/// previous lockfile recorded.
fn report_target_change(
    verified: &LockTarget,
    sensitive: &[MemberSensitivity],
    edition: EditionArg,
    target: &ResolvedTarget,
) {
    let now = LockTarget {
        edition: edition.as_lock_edition(),
        mc_version: target.mc_version().to_owned(),
        data_version: target.version_int(),
    };
    if *verified == now {
        return;
    }
    // The edition appears only when it changed: two editions number their
    // releases differently, so `1.21.4` against `1.21.60` reads as noise
    // without it, and naming it on every line would pad the common case.
    let show_edition = verified.edition != now.edition;
    eprintln!(
        "W_PREVIOUSLY_VERIFIED_TARGET: verified for {}, now {}.",
        describe_verified(verified, show_edition),
        describe_now(&now, show_edition),
    );
    if sensitive.is_empty() {
        return;
    }
    let ids: Vec<&str> = sensitive.iter().map(|m| m.id.as_str()).collect();
    eprintln!(
        "W_SEMANTIC_SENSITIVITY: {} member{} may resolve differently: {}",
        ids.len(),
        if ids.len() == 1 { "" } else { "s" },
        ids.join(", "),
    );
}

/// The left half of the warning: `1.20.4/DataVersion 3700`.
///
/// The integer is named here and bare on the right, which is the shape
/// `spec/versioning-editions` "Provenance and lock" prints. Java's is
/// Minecraft's `DataVersion`; Bedrock's is the block palette's own
/// `version`, and calling both `DataVersion` would name the Java concept for
/// a number that is not one.
fn describe_verified(target: &LockTarget, show_edition: bool) -> String {
    let field = match target.edition {
        LockEdition::Java => "DataVersion",
        LockEdition::Bedrock => "block version",
    };
    format!(
        "{}{}/{} {}",
        edition_prefix(target, show_edition),
        target.mc_version,
        field,
        target.data_version,
    )
}

/// The right half of the warning: `1.21.4/4189`.
fn describe_now(target: &LockTarget, show_edition: bool) -> String {
    format!(
        "{}{}/{}",
        edition_prefix(target, show_edition),
        target.mc_version,
        target.data_version,
    )
}

fn edition_prefix(target: &LockTarget, show_edition: bool) -> String {
    if show_edition {
        format!("{} ", target.edition.as_str())
    } else {
        String::new()
    }
}

/// Append a `.lock` suffix to the source file name so multiple `.crn`
/// files in the same directory get distinct locks. `Path::with_extension`
/// would drop `.crn`, fusing `cottage.crn`'s lock with any other
/// `cottage.*` source's lock.
pub(crate) fn default_lock_path(source: &Path) -> PathBuf {
    let mut p = source.as_os_str().to_owned();
    p.push(".lock");
    PathBuf::from(p)
}

pub(crate) fn build_lockfile(
    source: &str,
    block_ir: &BlockArrayIr,
    edition: EditionArg,
    target: &ResolvedTarget,
) -> Result<Lockfile, cairn_lang_core::lock::HashError> {
    Ok(Lockfile {
        lock_schema_version: LOCK_SCHEMA_VERSION,
        source_hash: hash_source(source),
        cairn_version: CAIRN_VERSION.to_owned(),
        target: LockTarget {
            edition: edition.as_lock_edition(),
            mc_version: target.mc_version().to_owned(),
            data_version: target.version_int(),
        },
        inputs: LockInputs {
            // The registry pack ingest replaces the hardcoded version
            // table; its bytes hash pins the exact (mc_version, version
            // integer) resolution rules a downstream re-compile must
            // match, keyed to the edition being compiled. The constraint
            // catalog ingest will fill the second field once catalogs
            // ship; until then it stays zero (per `LockInputs::zero`'s
            // contract).
            registry_pack_hash: target.registry_pack().bytes_hash.clone(),
            constraint_catalog_hash: HashHex::zero(),
        },
        resolved_ir_hash: hash_resolved_ir(block_ir)?,
        verified: true,
        member_version_sensitivity: vec![],
        placements: block_ir
            .placements
            .values()
            .map(|p| LockPlacement {
                site: p.site.clone(),
                id: p.place_id.clone(),
                def: p.source_def.clone(),
                theme: p.theme.clone(),
                origin: [p.origin.0, p.origin.1, p.origin.2],
                dims: [p.dims.x, p.dims.y, p.dims.z],
            })
            .collect(),
        walkways: block_ir
            .walkways
            .values()
            .map(|walkway| {
                // `Footprint::to_dims_y1` is the single place that
                // re-attaches the implicit `y = 1` for the lockfile's
                // `dims: [u32; 3]` wire format; the block-array IR's
                // own `dims.y` invariant is asserted at
                // `lower_connects`'s Footprint construction site.
                let dims = walkway.footprint.to_dims_y1();
                LockWalkway {
                    site: walkway.site.clone(),
                    from: walkway.from.clone(),
                    to: walkway.to.clone(),
                    path_material: walkway.path_material.clone(),
                    origin: [walkway.origin.0, walkway.origin.1, walkway.origin.2],
                    dims: [dims.x, dims.y, dims.z],
                }
            })
            .collect(),
    })
}
