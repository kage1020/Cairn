//! `requires` pass — flags `@requires` expressions that state no floor.
//!
//! The directive's whole job is to declare one constraint, so an expression
//! the compiler cannot read declares nothing at all. That used to happen
//! silently: `parse_min_version` returned `None`, `derive_min_version`
//! skipped the header, and `cairn info` printed `0.0 .. latest` for a file
//! whose first line says `version>=1.21`. The constraint is still there for
//! a reader to see and no longer there for the compiler, which is worse
//! than never having written it.
//!
//! Only the surface AST is walked: `@requires` is a header, and lowering
//! does not carry headers into the IR.

use crate::ast::{Header, Module};
use crate::resolve::{RequirementError, parse_requirement};

use super::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticSink};

pub(super) fn run(module: &Module, sink: &mut DiagnosticSink) {
    for header in &module.headers {
        let Header::Requires { requirement, span } = header else {
            continue;
        };
        let Err(error) = parse_requirement(requirement.as_str()) else {
            continue;
        };
        sink.push(Diagnostic {
            code: DiagnosticCode::InvalidRequires,
            span: span.clone(),
            primary: format!("`@requires` declares no version floor: {error}"),
            notes: Vec::new(),
            data: Some(DiagnosticData::InvalidRequires {
                reason: error.kind().to_owned(),
                found: offending_text(&error),
            }),
        });
    }
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
