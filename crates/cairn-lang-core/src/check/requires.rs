//! `requires` pass — flags `requires` expressions that state no floor.
//!
//! The line's whole job is to declare one constraint, so an expression the
//! compiler cannot read declares nothing at all. That used to happen
//! silently: `parse_min_version` returned `None`, `derive_min_version`
//! skipped the header, and `cairn info` printed `0.0 .. latest` for a file
//! whose first line says `version>=1.21`. The constraint is still there for
//! a reader to see and no longer there for the compiler, which is worse
//! than never having written it.
//!
//! Both spellings are walked: the `@requires` header, which is a floor on
//! the file, and the member-level `requires` line a `def` or a `theme` may
//! carry (`spec/versioning-editions.md` §10.4), which is a floor on that
//! part and on every build instantiating it. One unreadable expression is
//! one finding whichever of the two it was written as, so they share a
//! code, a message, and this pass.
//!
//! Only the surface AST is walked: neither form survives lowering — a
//! header is not carried into the IR, and a member-level line is lifted
//! onto its item rather than into the body.

use crate::ast::{Header, Module};
use crate::error::Span;
use crate::resolve::{RequirementError, parse_requirement};

use super::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticSink};

pub(super) fn run(module: &Module, sink: &mut DiagnosticSink) {
    for header in &module.headers {
        let Header::Requires { requirement, span } = header else {
            continue;
        };
        report(requirement.as_str(), span, "`@requires`", sink);
    }
    // Every item, not only the parts a build instantiates: a floor that
    // states nothing is a mistake in the line, and a `def` nobody places
    // yet is a `def` somebody is about to.
    for item in &module.items {
        for line in item.requires() {
            report(
                line.requirement.as_str(),
                &line.span,
                &format!("`requires` in {} {}", item.kind().keyword(), item.name()),
                sink,
            );
        }
    }
}

/// Push `E_INVALID_REQUIRES` for one expression, if it declares no floor.
///
/// `subject` opens the message and is what tells the two spellings apart —
/// the span points at a line either of them could be on, and a member-level
/// floor also wants to say which part it was written in.
fn report(requirement: &str, span: &Span, subject: &str, sink: &mut DiagnosticSink) {
    let Err(error) = parse_requirement(requirement) else {
        return;
    };
    sink.push(Diagnostic {
        code: DiagnosticCode::InvalidRequires,
        span: span.clone(),
        primary: format!("{subject} declares no version floor: {error}"),
        notes: Vec::new(),
        data: Some(DiagnosticData::InvalidRequires {
            reason: error.kind().to_owned(),
            found: offending_text(&error),
        }),
    });
}

/// The fragment of the expression the failure is about, for a consumer
/// building a quick-fix over it.
///
/// Empty when the failure names no fragment: "this is not a version
/// requirement" and "there is no version here" are about the whole
/// expression, and inventing a substring for them would give a tool
/// something to replace that the author never wrote.
fn offending_text(error: &RequirementError) -> String {
    match error {
        RequirementError::UnknownEditionScope(scope) => scope.clone(),
        RequirementError::UnsupportedOperator(operator) => operator.clone(),
        RequirementError::Component { component, .. }
        | RequirementError::ComponentTooLarge { component, .. } => component.clone(),
        RequirementError::PreRelease { tag, .. } => tag.clone(),
        RequirementError::TrailingTokens(rest) => rest.clone(),
        _ => String::new(),
    }
}
