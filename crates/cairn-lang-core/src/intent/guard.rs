//! The lines an `@edition` guard holds that a build reads
//! (`spec/versioning-editions` "An `@edition` guard holds the escape hatch").

use crate::ast::{Arg, Statement};
use crate::edition::Edition;

use super::{MemberRole, role_of};

/// Whether a line of `role` writing `args` is of the escape-hatch layer: a raw member, a `spawn`,
/// a Tier 0 part, or a line whose role reads the `nbt=` it writes.
pub(crate) fn is_escape_hatch_line(role: &MemberRole, args: &[Arg]) -> bool {
    role.is_escape_hatch() || (role.reads_nbt() && args.iter().any(|arg| arg.key == "nbt"))
}

/// Whether `line`, standing directly under an `@edition` guard, is one the guard may not hold, so
/// that no build or other check reads it or anything indented under it.
pub(crate) fn refused_under_guard(line: &Statement) -> bool {
    match line {
        Statement::Generic { keyword, args, .. } => {
            let role = role_of(keyword);
            !is_escape_hatch_line(&role, args) && !matches!(role, MemberRole::Other(_))
        }
        Statement::Logic { .. }
        | Statement::AssertTruth { .. }
        | Statement::AssertAlways { .. }
        | Statement::AssertLatency { .. } => true,
        // The parser refuses a guard directly under a guard.
        Statement::EditionGuard { .. } => false,
    }
}

/// Each line of `body` a build may read, with a guard's lines in the guard's place, paired with the
/// edition of the guard it stands under.
pub(crate) fn guarded_lines(
    body: &[Statement],
) -> impl Iterator<Item = (Option<Edition>, &Statement)> {
    body.iter().flat_map(|statement| {
        let (edition, lines) = match statement {
            Statement::EditionGuard {
                edition, children, ..
            } => (Some(*edition), children.as_slice()),
            line => (None, std::slice::from_ref(line)),
        };
        lines
            .iter()
            .filter(move |line| edition.is_none() || !refused_under_guard(line))
            .map(move |line| (edition, line))
    })
}
