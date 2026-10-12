//! The `cairn_version` pass: warns when the `@cairn` header's version is not a `CalVer`, or is
//! later than this build.

use crate::CAIRN_VERSION;
use crate::ast::{Header, Module};
use crate::calver::parse_language_version;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::error::Span;

use super::DiagnosticSink;

pub(super) fn run(module: &Module, sink: &mut DiagnosticSink) {
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

/// Quotes both versions as written: the author edits the string, not the parsed value.
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
