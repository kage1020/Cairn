//! `cairn_version` pass — reads the `@cairn` header's declared language
//! version, and says so when it cannot.
//!
//! The directive is provenance: nothing branches on the value and the
//! artifact is identical whatever it says. `spec/index.md` still gives it
//! a job — "so a future compiler can parse and warn correctly" — and a
//! value no compiler can read cannot do that job. Two findings come out of
//! reading it:
//!
//! - the value is not a `CalVer` at all, so the header declares nothing;
//! - the value is a later version than this build, which is the one thing
//!   an older compiler can usefully say about a file written against a
//!   newer language. An unknown keyword or argument anywhere in the file
//!   might be about the gap rather than about the source, and only the
//!   header knows.
//!
//! The second reaches the cases a later language adds *within* the
//! existing shapes. A whole new syntactic form does not get here at all:
//! an unrecognised `@directive` and an unrecognised top-level item are
//! both `E_PARSE`, and `spec/lint` §11.3 says what follows — parsing
//! precedes every check pass, so a source that does not parse reaches
//! none of them. The version gap is the whole explanation there and this
//! finding is the one thing that cannot say so.
//!
//! Both are warnings. `spec/lint` §11.3 makes a finding an error when
//! leaving it alone yields something other than what the source asked for;
//! neither of these changes a voxel. `@requires` is an error on the same
//! rule read the other way — its floor reaches `cairn info`'s compatible
//! range and the `--target` gate, so a floor that evaporates accepts a
//! target it should not.
//!
//! Only the surface AST is walked, for the reason [`super::requires`]
//! walks it: `@cairn` is a header, and lowering does not carry headers
//! into the IR.

use crate::CAIRN_VERSION;
use crate::ast::{Header, Module};
use crate::calver::parse_language_version;
use crate::error::Span;

use super::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote, DiagnosticSink};

pub(super) fn run(module: &Module, sink: &mut DiagnosticSink) {
    // Parsed once for the whole module rather than per header. `None` is a
    // build whose own `CARGO_PKG_VERSION` is not a `CalVer`, which the
    // release process cannot produce and which
    // `this_compilers_own_version_is_a_language_version` pins. There is
    // nothing to compare against then, so the future check stands down —
    // and the shape check, which needs no comparison, goes on running.
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
/// Both versions are quoted as they are written rather than as they parse:
/// the declared one is what the author has to edit, and `CAIRN_VERSION` is
/// what a bug report has to name. `2026.06` and `2026.6` are one version to
/// the comparison and two strings to a reader, and the reader is the one
/// being told which line to change.
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
