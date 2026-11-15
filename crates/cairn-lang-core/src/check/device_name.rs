//! The `device_name` pass: a `struct` or `site` named after a device Windows reserves, the stem of
//! an artifact's file name (`spec/lint` "Sites and placements").

use crate::ast::{Item, Module};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::ids::is_windows_device_name;
use crate::prose::windows_device_name_note;

use super::DiagnosticSink;

pub(super) fn run(module: &Module, sink: &mut DiagnosticSink) {
    for item in &module.items {
        let (code, keyword, subject, name, name_span) = match item {
            Item::Struct {
                name, name_span, ..
            } => (
                DiagnosticCode::InvalidStructName,
                "struct",
                "a struct's name",
                name,
                name_span,
            ),
            Item::Site {
                name, name_span, ..
            } => (
                DiagnosticCode::InvalidSiteName,
                "site",
                "a site's name, under `--structure-format litematic`,",
                name,
                name_span,
            ),
            _ => continue,
        };
        if !is_windows_device_name(name) {
            continue;
        }
        sink.push(Diagnostic {
            code,
            span: name_span.clone(),
            primary: format!(
                "`{keyword} {name}` is not a usable name: it is a device name Windows reserves"
            ),
            notes: vec![DiagnosticNote {
                span: None,
                message: windows_device_name_note(subject, "name", name),
            }],
            data: None,
        });
    }
}
