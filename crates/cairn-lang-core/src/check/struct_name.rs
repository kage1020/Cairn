//! The `struct_name` pass: a `struct` named after a device Windows reserves, its artifact's file
//! stem (`spec/lint` "Sites and placements").

use crate::ast::{Item, Module};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::ids::is_windows_device_name;
use crate::prose::windows_device_name_note;

use super::DiagnosticSink;

pub(super) fn run(module: &Module, sink: &mut DiagnosticSink) {
    for item in &module.items {
        let Item::Struct {
            name, name_span, ..
        } = item
        else {
            continue;
        };
        if !is_windows_device_name(name) {
            continue;
        }
        sink.push(Diagnostic {
            code: DiagnosticCode::InvalidStructName,
            span: name_span.clone(),
            primary: format!(
                "`struct {name}` is not a usable name: it is a device name Windows reserves"
            ),
            notes: vec![DiagnosticNote {
                span: None,
                message: windows_device_name_note("a struct's name", "name", name),
            }],
            data: None,
        });
    }
}
