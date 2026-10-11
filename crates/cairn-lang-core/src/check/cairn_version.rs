//! `cairn_version` pass: reads the `@cairn` header's declared language
//! version, and says so when it cannot.
//!
//! Nothing branches on the value, but `spec/index` gives it a job ("so a
//! future compiler can parse and warn correctly") that an unreadable value
//! cannot do. The pass warns when the value is not a `CalVer`, and when it
//! is later than this build, since an unknown keyword or argument elsewhere
//! in the file may then be about the gap rather than the source. A new
//! syntactic form never reaches this pass because it fails to parse, so
//! [`crate::calver::future_version_note`] carries the gap to that parse
//! error as a note.
//!
//! Both are warnings because neither changes a voxel (`spec/lint` "Error vs
//! warning"). `@requires` is an error by the same rule: its floor reaches
//! `cairn info`'s compatible range and the `--target` gate.
//!
//! Only the surface AST is walked, because lowering does not carry headers
//! into the IR.

use crate::CAIRN_VERSION;
use crate::ast::{Header, Module};
use crate::calver::parse_language_version;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::error::Span;

use super::DiagnosticSink;

pub(super) fn run(module: &Module, sink: &mut DiagnosticSink) {
    // `None` only for a build whose own version is not a `CalVer`, which the
    // release process cannot produce; then only the future check stands down.
    let compiler = parse_language_version(CAIRN_VERSION).ok();
    for header in &module.headers {
        let Header::Cairn { version, span } = header else {
            continue;
        };
        let declared = version.as_str();
        match parse_language_version(declared) {
            Err(error) => sink.push(Diagnostic {
                code: DiagnosticCode::InvalidCairnVersion,
                span: span.clone(),
                primary: format!(
                    "`@cairn {declared}` does not name a Cairn language version: {error}",
                ),
                notes: vec![DiagnosticNote {
                    span: None,
                    message: format!(
                        "Cairn versions by date, written `YYYY.M` or `YYYY.M.PATCH`; this build is `{CAIRN_VERSION}`",
                    ),
                }],
                data: Some(DiagnosticData::InvalidCairnVersion {
                    reason: error.kind().to_owned(),
                    found: error.offending_text().to_owned(),
                }),
            }),
            Ok(parsed) => {
                if compiler.is_some_and(|compiler| parsed.is_newer_than(&compiler)) {
                    sink.push(future_diag(declared, span));
                }
            }
        }
    }
}

/// The finding for a file written against a later language than this build.
///
/// Both versions are quoted as written, not as parsed: `2026.06` and
/// `2026.6` compare equal, but the author edits the string they wrote.
fn future_diag(declared: &str, span: &Span) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::FutureCairnVersion,
        span: span.clone(),
        primary: format!(
            "this file declares Cairn `{declared}`, which is newer than this build (`{CAIRN_VERSION}`)",
        ),
        notes: vec![
            DiagnosticNote {
                span: None,
                message:
                    "a keyword or argument added after this build is reported as unknown; a whole new syntactic form — a directive, a top-level item — is a parse error instead, and no check pass runs then, this one included; another finding in this file may be about the version gap rather than about the line it names"
                        .to_owned(),
            },
            DiagnosticNote {
                span: None,
                message: format!(
                    "upgrade to Cairn {declared} or later, or lower the header to the version this file is written against",
                ),
            },
        ],
        data: Some(DiagnosticData::FutureCairnVersion {
            declared: declared.to_owned(),
            compiler: CAIRN_VERSION.to_owned(),
        }),
    }
}
