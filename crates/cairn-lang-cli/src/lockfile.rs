//! The lockfile a compile writes, and what it says about the one it replaces.

use std::path::{Path, PathBuf};

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::lock::{
    HashHex, LOCK_SCHEMA_VERSION, LockError, LockInputs, LockPlacement, LockTarget, LockWalkway,
    Lockfile, MemberSensitivity, hash_resolved_ir, hash_source,
};
use cairn_lang_core::{CAIRN_VERSION, Edition};

use crate::target::ResolvedTarget;

/// Warn where the lockfile at `lock_path` records another target, as `spec/versioning-editions`
/// "Provenance and lock" does, or cannot be read; an absent one is silent.
pub(crate) fn report_previous_target(lock_path: &Path, edition: Edition, target: &ResolvedTarget) {
    // Read once, so the lenient parse below sees the bytes the strict one did.
    let body = match std::fs::read_to_string(lock_path) {
        Ok(body) => body,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => {
            warn_unreadable_lock(lock_path, &LockError::from(err));
            return;
        }
    };
    let previous = match Lockfile::from_yaml(&body) {
        Ok(previous) => previous,
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
                // `cause`, not `err`: the strict read may have stopped at an identifier first.
                Err(cause) => warn_unreadable_lock(lock_path, &cause),
                // Unreachable while the probe mirrors the strict schema.
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

fn warn_unreadable_lock(lock_path: &Path, err: &LockError) {
    eprintln!(
        "warning: {}: the existing lockfile could not be read ({err}); replacing it",
        lock_path.display(),
    );
}

/// Warn where `verified`, the previous lockfile's target, differs from this build's.
fn report_target_change(
    verified: &LockTarget,
    sensitive: &[MemberSensitivity],
    edition: Edition,
    target: &ResolvedTarget,
) {
    let now = LockTarget {
        edition,
        mc_version: target.mc_version().to_owned(),
        data_version: target.version_int(),
    };
    if *verified == now {
        return;
    }
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
fn describe_verified(target: &LockTarget, show_edition: bool) -> String {
    let field = match target.edition {
        Edition::Java => "DataVersion",
        Edition::Bedrock => "block version",
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

/// `<source>.lock`, keeping `.crn` so no two sources share a lock.
pub(crate) fn default_lock_path(source: &Path) -> PathBuf {
    let mut p = source.as_os_str().to_owned();
    p.push(".lock");
    PathBuf::from(p)
}

pub(crate) fn build_lockfile(
    source: &str,
    block_ir: &BlockArrayIr,
    edition: Edition,
    target: &ResolvedTarget,
) -> Result<Lockfile, cairn_lang_core::lock::HashError> {
    Ok(Lockfile {
        lock_schema_version: LOCK_SCHEMA_VERSION,
        source_hash: hash_source(source),
        cairn_version: CAIRN_VERSION.to_owned(),
        target: LockTarget {
            edition,
            mc_version: target.mc_version().to_owned(),
            data_version: target.version_int(),
        },
        inputs: LockInputs {
            // No constraint catalog ships, so its hash is zero.
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
