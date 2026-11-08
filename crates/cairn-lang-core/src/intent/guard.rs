//! The lines an `@edition` guard holds that a build reads
//! (`spec/versioning-editions` "An `@edition` guard holds the escape hatch").

use crate::ast::Statement;
use crate::edition::Edition;

use super::{MemberRole, role_of};

/// Whether `line`, standing directly under an `@edition` guard, is one the guard may not hold, so
/// that no build or other check reads it or anything indented under it.
pub(crate) fn refused_under_guard(line: &Statement) -> bool {
    match line {
        Statement::Generic { keyword, .. } => {
            let role = role_of(keyword);
            !role.is_raw() && !matches!(role, MemberRole::Other(_))
        }
        Statement::Logic { .. }
        | Statement::AssertTruth { .. }
        | Statement::AssertAlways { .. } => true,
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
