//! The lockfile a compile writes, and what it says about the one it replaces.

use std::path::{Path, PathBuf};

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::lock::{
    LOCK_SCHEMA_VERSION, LockError, LockInputs, LockPlacement, LockTarget, LockWalkway, Lockfile,
    MemberSensitivity, hash_resolved_ir, hash_source,
};
use cairn_lang_core::resolve::{FloorVerdict, SemanticSensitiveFinding};
use cairn_lang_core::{CAIRN_VERSION, Edition};
use cairn_lang_formats::registry::{builtin_constraints, builtin_pack};

use crate::target::{ResolvedTarget, registry_view};

/// The members of `block_ir` the constraint catalog lists as meaning something else across a
/// boundary version of `edition`, the edition `target` is a version of.
pub(crate) fn sensitive_members(
    block_ir: &BlockArrayIr,
    edition: Edition,
    target: &ResolvedTarget,
) -> Vec<SemanticSensitiveFinding> {
    registry_view(edition, Some(target.mc_version())).semantic_sensitive(block_ir)
}

/// Warn where the lockfile at `lock_path` records another target, as `spec/versioning-editions`
/// "Provenance and lock" does, or cannot be read; an absent one is silent. `sensitive` is this
/// build's [`sensitive_members`].
pub(crate) fn report_previous_target(
    lock_path: &Path,
    edition: Edition,
    target: &ResolvedTarget,
    sensitive: &[SemanticSensitiveFinding],
) {
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
                    report_target_change(&refused.target, sensitive, edition, target);
                }
                // `cause`, not `err`: the strict read may have stopped at an identifier first.
                Err(cause) => warn_unreadable_lock(lock_path, &cause),
                // Unreachable while the probe mirrors the strict schema.
                Ok(None) => warn_unreadable_lock(lock_path, &err),
            }
            return;
        }
    };
    report_target_change(&previous.target, sensitive, edition, target);
}

fn warn_unreadable_lock(lock_path: &Path, err: &LockError) {
    eprintln!(
        "warning: {}: the existing lockfile could not be read ({err}); replacing it",
        lock_path.display(),
    );
}

/// Warn where `verified`, the previous lockfile's target, differs from this build's, naming the
/// members of `sensitive` whose boundary lies between the two.
fn report_target_change(
    verified: &LockTarget,
    sensitive: &[SemanticSensitiveFinding],
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
    let mut members: Vec<(&str, Vec<String>)> = Vec::new();
    for finding in crossed(sensitive, verified, edition, target) {
        let reason = format!("{} at {}", finding.reason, finding.boundary_version);
        match members
            .iter_mut()
            .find(|(member, _)| *member == finding.member)
        {
            Some((_, reasons)) => reasons.push(reason),
            None => members.push((&finding.member, vec![reason])),
        }
    }
    if members.is_empty() {
        return;
    }
    let listed: Vec<String> = members
        .iter()
        .map(|(member, reasons)| format!("{member} ({})", reasons.join(", ")))
        .collect();
    eprintln!(
        "W_SEMANTIC_SENSITIVITY: {} member{} may resolve differently: {}",
        members.len(),
        if members.len() == 1 { "" } else { "s" },
        listed.join(", "),
    );
}

/// The findings of `sensitive`, all of `edition`, whose boundary `target` and `verified` lie on
/// different sides of; every one when `verified` is no version of the edition's table, and none
/// when it is of the other edition.
fn crossed<'s>(
    sensitive: &'s [SemanticSensitiveFinding],
    verified: &LockTarget,
    edition: Edition,
    target: &ResolvedTarget,
) -> Vec<&'s SemanticSensitiveFinding> {
    if verified.edition != edition {
        return Vec::new();
    }
    let order = builtin_pack(edition).version_order();
    let before = order.key_of(&verified.mc_version);
    let below = |boundary: &str, key| order.verdict(boundary, key) == FloorVerdict::Below;
    sensitive
        .iter()
        .filter(|finding| {
            before.is_none_or(|before| {
                below(&finding.boundary_version, before)
                    != below(&finding.boundary_version, target.order_key())
            })
        })
        .collect()
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
            registry_pack_hash: target.registry_pack().bytes_hash().clone(),
            constraint_catalog_hash: builtin_constraints().hash().clone(),
        },
        resolved_ir_hash: hash_resolved_ir(block_ir)?,
        verified: true,
        member_version_sensitivity: sensitive_members(block_ir, edition, target)
            .into_iter()
            .map(|finding| MemberSensitivity {
                id: finding.member,
                reason: format!("{} at {}", finding.reason, finding.boundary_version),
            })
            .collect(),
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
