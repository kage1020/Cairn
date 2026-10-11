//! `cairn compile`: the structure artifacts and the lockfile for one
//! `(edition, version)` pin.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::LineStarts;

use crate::artifacts::{
    PrepareRefusal, check_lock_path_is_free, prepare_artifacts, prepare_out_dir,
    write_artifacts_and_lock,
};
use crate::floors::enforce_version_floor;
use crate::lockfile::{default_lock_path, report_previous_target};
use crate::pipeline::{Lowered, built_scopes, load_and_lower, report_partial_build};
use crate::report::report_diagnostics;
use crate::target::{ResolvedTarget, resolve_target};

pub(crate) fn run_compile(
    file: &Path,
    edition: Edition,
    target: &str,
    out: Option<&Path>,
    lock: Option<&Path>,
) -> ExitCode {
    // Resolved before lowering, which checks ids against the pinned
    // version, but reported below; `resolve_target` says why, and
    // `load_and_lower` why an unresolved or floor-refused target lowers
    // unpinned.
    let resolved_target = resolve_target(edition, target);
    let Lowered {
        source,
        block_ir,
        dropped_scopes,
        version_floors,
    } = match load_and_lower(file, edition, resolved_target.as_ref().ok()) {
        Ok(lowered) => lowered,
        Err(code) => return code,
    };
    if report_diagnostics(
        file,
        &source,
        &LineStarts::new(&source),
        &block_ir.diagnostics,
    ) {
        return ExitCode::from(1);
    }
    // A lockfile must not certify a build missing part of what the source
    // asked for, and every code that drops a scope (`W_STRUCT_NO_SIZE`,
    // `W_DEF_NO_SIZE`, ...) is a warning, so this is what refuses it. A
    // source of only templates and themes requests no scopes and still
    // compiles.
    if !dropped_scopes.is_empty() {
        report_partial_build(
            file,
            &dropped_scopes,
            built_scopes(&block_ir),
            "refusing to certify a partial build",
        );
        return ExitCode::from(1);
    }

    let target = match resolved_target {
        Ok(t) => t,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(1);
        }
    };
    if let Some(floors) = &version_floors
        && let Err(code) = enforce_version_floor(file, &source, floors, edition, &target)
    {
        return code;
    }

    let out_dir = match prepare_out_dir(file, out) {
        Ok(d) => d,
        Err(code) => return code,
    };

    let lock_path = lock.map_or_else(|| default_lock_path(file), Path::to_path_buf);
    check_and_write(&block_ir, &source, edition, &target, &out_dir, &lock_path)
}

/// Check every structure, then write them all and the lockfile.
///
/// [`prepare_artifacts`] refuses a structure the backend cannot write
/// before [`write_artifacts_and_lock`] stages a byte, so a refusal leaves
/// `out_dir` and the lockfile as they were.
fn check_and_write(
    block_ir: &BlockArrayIr,
    source: &str,
    edition: Edition,
    target: &ResolvedTarget,
    out_dir: &Path,
    lock_path: &Path,
) -> ExitCode {
    let prepared = match prepare_artifacts(block_ir, target, out_dir) {
        Ok(p) => p,
        Err(PrepareRefusal::Collision(collision)) => {
            collision.report();
            return ExitCode::from(1);
        }
        Err(PrepareRefusal::Other(code)) => return code,
    };
    if let Err(code) = check_lock_path_is_free(&prepared, lock_path) {
        return code;
    }
    // Before the file is replaced, not after: the lockfile about to be
    // overwritten is the only record of what was previously verified.
    report_previous_target(lock_path, edition, target);
    write_artifacts_and_lock(&prepared, source, block_ir, edition, target, lock_path)
}

#[cfg(test)]
mod tests {
    use cairn_lang_core::Edition;

    use super::*;
    use crate::pipeline::lowered_probe;

    /// The passing structure comes first, so a check folded into the write
    /// loop would stage it before reaching the refused one, whose grid is
    /// shorter than its dims.
    #[test]
    fn a_structure_the_backend_refuses_leaves_out_dir_and_lockfile_untouched() {
        let mut block_ir = lowered_probe(Edition::Java);
        let target = resolve_target(Edition::Java, "latest").expect("the latest target resolves");
        let array = block_ir
            .structures
            .values()
            .next()
            .expect("the probe lowers to one structure")
            .clone();
        let mut short = array.clone();
        short.voxels.pop();
        block_ir.structures = [
            ("struct::a".to_owned(), array),
            ("struct::b".to_owned(), short),
        ]
        .into_iter()
        .collect();
        let dir = tempfile::tempdir().expect("temp dir");
        let lock_path = dir.path().join("probe.crn.lock");

        let code = check_and_write(
            &block_ir,
            "",
            Edition::Java,
            &target,
            dir.path(),
            &lock_path,
        );

        assert_eq!(code, ExitCode::from(1));
        let left: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read temp dir")
            .map(|entry| entry.expect("dir entry").file_name())
            .collect();
        assert!(left.is_empty(), "a refused build wrote {left:?}");
    }
}
