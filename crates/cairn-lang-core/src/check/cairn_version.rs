//! `cairn_version` pass — reads the `@cairn` header's declared language
//! version, and says so when it cannot.
//!
//! The directive is provenance: nothing branches on the value and the
//! artifact is identical whatever it says. `spec/index` still gives it
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
//! both `E_PARSE`, and `spec/lint` "Error vs warning" says what follows —
//! parsing precedes every check pass, so a source that does not parse reaches
//! none of them. The version gap is the whole explanation there, so
//! [`future_version_note`] carries it to the one finding that *is*
//! reported: the parse error itself, as a note rather than a second
//! finding.
//!
//! Both are warnings. That same section makes a finding an error when
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

/// The note a parse failure carries when the `@cairn` header has
/// something to say about it.
///
/// [`run`] cannot reach a file that does not parse, and a whole new
/// syntactic form — the shape a later language most often adds — is
/// exactly what does not parse. The header is still readable from the
/// text, which is what [`declared_version`] reads it from. Without this
/// note the author is told "I do not understand this line" where the
/// answer is "you need a newer Cairn", which is the whole reason
/// `@cairn` is in the file.
///
/// A note rather than a second finding, because `spec/lint`
/// "Error vs warning" makes a source that does not parse report `E_PARSE`
/// alone. A note keeps that true.
///
/// Two things it says. A header naming a *later* language explains the
/// error below it. A header naming no language version at all —
/// `@cairn banana`, or a value the lexer could not have read — says that
/// this build cannot tell whether the error below is a version gap, which
/// is worth a sentence because this is the only moment the author hears
/// it: `W_INVALID_CAIRN_VERSION` is raised by a check pass, and no check
/// pass runs on a source that does not parse.
pub(crate) fn future_version_note(source: &str, line_starts: &[usize]) -> Option<DiagnosticNote> {
    let (declared, span) = declared_version(source, line_starts)?;
    let Ok(version) = parse_language_version(&declared) else {
        return Some(DiagnosticNote {
            span: Some(span),
            message: format!(
                "`@cairn {declared}` does not name a language version, so this build cannot judge whether the error below is a form a later Cairn adds",
            ),
        });
    };
    if !version.is_newer_than(&parse_language_version(CAIRN_VERSION).ok()?) {
        return None;
    }
    Some(DiagnosticNote {
        span: Some(span),
        message: format!(
            "this file declares Cairn `{declared}`, which is newer than this build (`{CAIRN_VERSION}`); the line this error names may be a form a later Cairn adds",
        ),
    })
}

/// The `@cairn` value a source declares and the span of the line
/// carrying it, read from the text.
///
/// Text rather than the AST because the caller has none, and text rather
/// than the lexer because a source that fails to *lex* has no tokens
/// either — `floor a=%` is the shape this note most needs to reach. Both
/// halves of that matter: `crate::parse` lexes the whole file before it
/// reads a header, so on a lex failure it holds no header at all, and on
/// an unknown directive above the `@cairn` it stops at that line while
/// this scan walks past it.
///
/// Only the header block is read: the run of lines before the first that
/// is neither blank, a comment, nor a top-level `@directive`, which is
/// the only place [`crate::parse`] takes a header from. A `@cairn` below
/// that, or indented under a body, is not a header, and reading one
/// would attribute a version to a file the parser never saw one in.
///
/// Where this agrees with the lexer it has to agree exactly, because the
/// note asserts what the file *declares*. Lines come from
/// [`crate::lines`] rather than from a `split` of this function's own,
/// so `\r\n` and a lone `\r` end a line here the way they do everywhere
/// else. Horizontal space is a single `' '` and nothing else, because
/// `Lexer::skip_spaces` takes that byte alone: a tab at indentation is
/// `LexError::TabIndent`, so `@cairn\t2099.1` declares nothing, and a
/// `str::trim` that stripped the tab would have this note claim a
/// version the file never carried. What survives the trim is then
/// required to hold no space at all, which is the same rule read from
/// the other end: a header value is one token. A leading byte-order mark is skipped
/// for the opposite reason — the lexer skips one, so the header behind
/// it is real.
///
/// The span matches the one [`Header::Cairn`] carries — the `@` to the
/// last byte of the value — so the note points where the check pass's own
/// finding would have.
fn declared_version(source: &str, line_starts: &[usize]) -> Option<(String, Span)> {
    for (index, &start) in line_starts.iter().enumerate() {
        let end = match line_starts.get(index + 1) {
            Some(&next) => crate::lines::end_before(source, next),
            None => source.len(),
        };
        let mut text = &source[start..end];
        let mut base = start;
        if index == 0
            && let Some(behind) = text.strip_prefix('\u{feff}')
        {
            base += text.len() - behind.len();
            text = behind;
        }
        let probe = text.trim_matches(' ');
        if probe.is_empty() || probe.starts_with('#') {
            continue;
        }
        // Anything that is not a directive at column zero ends the
        // header block, indentation included: an indented `@cairn` is a
        // body row to the lexer, not a header.
        let rest = text.strip_prefix('@')?;
        let name_len = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        if &rest[..name_len] != "cairn" {
            continue;
        }
        // Up to the comment marker, the way the lexer reads the line: a
        // `#` ends the value wherever it sits.
        let tail = &text[1 + name_len..];
        let tail = tail.split_once('#').map_or(tail, |(before, _)| before);
        let declared = tail.trim_matches(' ');
        // A header value is one token, so any space left inside it after
        // the spaces around it are gone is space the lexer would have
        // stopped on — a tab at indentation is `LexError::TabIndent`, and
        // `@cairn 2026.06 spare` is a second token the header grammar has
        // no room for. Either way `parse` built no header, and a note
        // about what the file "declares" would be about a declaration
        // that does not exist.
        if declared.is_empty() || declared.chars().any(char::is_whitespace) {
            return None;
        }
        let value_start = 1 + name_len + (tail.len() - tail.trim_start_matches(' ').len());
        return Some((
            declared.to_owned(),
            base..base + value_start + declared.len(),
        ));
    }
    None
}
