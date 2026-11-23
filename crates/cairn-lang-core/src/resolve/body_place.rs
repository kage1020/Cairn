//! The `place` lines of `struct` and `def` bodies: each `use=` names a def of the file or of the
//! furniture library (`spec/components-editing-sites` "A `def` placed in a body").

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::furniture::{self, Placed};
use crate::intent::{DefIr, IntentModule, Member, MemberRole};

use super::unresolved_ref::unresolved_ref_diag;

/// Record the file's defs the bodies of `ir` place in `used`, and report each `use=` that names
/// neither one of them nor a library def.
pub(super) fn resolve_body_places(
    ir: &IntentModule,
    used: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let bodies = ir
        .structs
        .iter()
        .map(|s| &s.members)
        .chain(ir.defs.iter().map(|d| &d.members));
    for members in bodies {
        walk(members, ir, used, diagnostics);
    }
}

fn walk(
    members: &[Member],
    ir: &IntentModule,
    used: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for member in members {
        walk(&member.children.members, ir, used, diagnostics);
        if !matches!(member.role, MemberRole::Place) {
            continue;
        }
        // A missing or mistyped `use=` is the painter's or `check::type_mismatch`'s to report.
        let Some(name) = member.used_def() else {
            continue;
        };
        match furniture::placed(name, &ir.defs) {
            Some(Placed::File(_)) => {
                used.insert(name.to_owned());
            }
            Some(Placed::Library(_)) => {}
            None => diagnostics.push(unresolved(member, name, &ir.defs)),
        }
    }
}

/// The `E_UNRESOLVED_PLACE_REF` on `member`, whose `use=` names `name`, a def neither of `defs` nor
/// of the furniture library.
fn unresolved(member: &Member, name: &str, defs: &[DefIr]) -> Diagnostic {
    let candidates = defs
        .iter()
        .chain(furniture::defs())
        .map(|def| def.name.as_str());
    let mut finding = unresolved_ref_diag(
        DiagnosticCode::UnresolvedPlaceRef,
        &format!("`use={name}` names no def of this file and none of the furniture library"),
        member.span.clone(),
        name,
        candidates,
    );
    let library: Vec<&str> = furniture::defs().iter().map(|d| d.name.as_str()).collect();
    finding.notes.push(DiagnosticNote {
        span: None,
        message: format!(
            "declare a `def` of that name, or name one the furniture library ships: {}",
            library.join(", "),
        ),
    });
    finding
}
