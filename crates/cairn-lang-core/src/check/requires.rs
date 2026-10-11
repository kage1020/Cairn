//! `requires` pass — flags `requires` expressions that state no floor.
//!
//! An expression the compiler cannot read leaves a floor a reader sees and
//! the compiler ignores, which is worse than none: `cairn info` would print
//! `0.0 .. latest` for a file that says `version>=1.21`.
//!
//! Both spellings share a code and this pass: the `@requires` header, a
//! floor on the file, and the member-level `requires` line of a `def` or a
//! `theme` (`spec/versioning-editions` "Fail-loud and minimum-version
//! inference"), a floor on that part and every build instantiating it.
//!
//! Only the surface AST is walked: a header is not carried into the IR, and
//! a member-level line is lifted onto its item.

use crate::ast::{Header, Module};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData};
use crate::error::Span;
use crate::version::{RequirementError, parse_requirement};

use super::DiagnosticSink;

pub(super) fn run(module: &Module, sink: &mut DiagnosticSink) {
    for header in &module.headers {
        let Header::Requires { requirement, span } = header else {
            continue;
        };
        report(requirement.as_str(), span, "`@requires`", sink);
    }
    // Every item, placed or not: the mistake is in the line.
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
/// `subject` opens the message, naming the spelling and, for a
/// member-level floor, the part it was written in.
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
/// Empty when the failure is about the whole expression, so a tool is not
/// handed a substring to replace that the author never wrote.
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
