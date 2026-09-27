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
        --stdio      Speak LSP over stdin/stdout (the default; accepted
                     because LSP clients pass it when asked for stdio)
    -V, --version    Print version and exit
    -h, --help       Print this help and exit

With no arguments, or with `--stdio`, the process speaks LSP over
stdin/stdout. Editors spawn this binary and communicate via Content-Length
framed JSON-RPC.
";

fn main() -> ExitCode {
    // Editors spawn `cairn-lsp` either with no arguments or with `--stdio`:
    // an LSP client told to use the stdio transport appends that flag to the
    // command line (vscode-languageclient does), and stdio is the only
    // transport this server speaks, so the flag names what happens anyway.
    // The other flags are a support-triage affordance (log the version at
    // activation, print help when a user runs the binary by hand).
    let mut args = std::env::args().skip(1);
    if let Some(arg) = args.next() {
        let extra = args.next();
        match arg.as_str() {
            "-V" | "--version" if extra.is_none() => {
                println!("cairn-lsp {CAIRN_VERSION}");
                return ExitCode::SUCCESS;
            }
            "-h" | "--help" if extra.is_none() => {
                print!("{HELP}");
                return ExitCode::SUCCESS;
            }
            "--stdio" if extra.is_none() => {}
            "--stdio" | "-V" | "--version" | "-h" | "--help" => {
                let unexpected = extra.unwrap_or_default();
                eprintln!(
                    "error: unexpected argument `{unexpected}` after `{arg}`. \
                     Fix: `cairn-lsp {arg}` takes no further arguments."
                );
                return ExitCode::from(2);
            }
            other => {
                eprintln!(
                    "error: unknown argument `{other}`. Valid: --stdio, --version, --help. \
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
