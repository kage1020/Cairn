//! `cairn-lsp` — the Cairn language server binary.
//!
//! Speaks the Language Server Protocol over stdio. Editors (or an LLM
//! acting through one) spawn this binary and receive push diagnostics for
//! every open `.crn` document; see the crate documentation for the module
//! layout.

use std::process::ExitCode;

fn main() -> ExitCode {
    match cairn_lang_lsp::server::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(1)
        }
    }
}
