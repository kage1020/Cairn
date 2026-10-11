//! The `requires` pass: a `@requires` header or member-level `requires` line that states no floor
//! (`spec/versioning-editions` "Fail-loud and minimum-version inference").

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

/// The fragment a quick-fix replaces, or empty when the failure is about the whole expression.
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
