//! The `struct_name` pass: a `struct` named after a device Windows reserves, which the artifact
//! named after it could not be written under (`spec/lint` "Sites and placements").

use crate::ast::{Item, Module};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::ids::{is_windows_device_name, windows_device_names_listed};

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
                message: format!(
                    "a struct's name is the stem of the artifact file written into `--out`, and \
                     Windows reserves {devices}, in any letter case, as device names no file \
                     can take, so the name is refused on every host, not only on Windows; \
                     rename it, for example to `{name}_1`",
                    devices = windows_device_names_listed(),
                ),
            }],
            data: None,
        });
    }
}
