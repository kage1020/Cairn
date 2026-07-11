//! `cairn-lsp` — the Cairn language server binary.
//!
//! Speaks the Language Server Protocol over stdio. Editors (or an LLM
//! acting through one) spawn this binary and receive push diagnostics for
//! every open `.crn` document; see the crate documentation for the module
//! layout.

use std::process::ExitCode;

use cairn_lang_core::CAIRN_VERSION;

const HELP: &str = "\
cairn-lsp — Cairn language server (Language Server Protocol over stdio)

USAGE:
    cairn-lsp [OPTIONS]

OPTIONS:
    -V, --version    Print version and exit
    -h, --help       Print this help and exit

With no arguments the process speaks LSP over stdin/stdout. Editors spawn
this binary and communicate via Content-Length framed JSON-RPC.
";

fn main() -> ExitCode {
    // Editors always spawn `cairn-lsp` with no arguments; the flags are a
    // support-triage affordance (log the version at activation, print help
    // when a user runs the binary by hand).
    let mut args = std::env::args().skip(1);
    if let Some(arg) = args.next() {
        match arg.as_str() {
            "-V" | "--version" => {
                println!("cairn-lsp {CAIRN_VERSION}");
                return ExitCode::SUCCESS;
            }
            "-h" | "--help" => {
                print!("{HELP}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!(
                    "error: unknown argument `{other}`. Valid: --version, --help. \
                     Fix: run `cairn-lsp` with no arguments to start the LSP server."
                );
                return ExitCode::from(2);
            }
        }
    }

    match cairn_lang_lsp::server::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(1)
        }
    }
}
