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
use crate::resolve::parse_requirement;

use super::{Diagnostic, DiagnosticCode, DiagnosticSink};

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
            data: None,
        });
    }
}
