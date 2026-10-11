//! `cairn-lsp`, the Cairn language server, speaking the Language Server Protocol over stdio.

use std::process::ExitCode;

use cairn_lang_core::CAIRN_VERSION;

const HELP: &str = "\
cairn-lsp — Cairn language server (Language Server Protocol over stdio)

USAGE:
    cairn-lsp [OPTIONS]

OPTIONS:
        --stdio      Speak LSP over stdin/stdout (same as no arguments)
    -V, --version    Print version and exit
    -h, --help       Print this help and exit

With no arguments the process speaks LSP over stdin/stdout. Editors spawn
this binary and communicate via Content-Length framed JSON-RPC.
";

fn main() -> ExitCode {
    // An LSP client appends `--stdio` for the stdio transport, the only one this server speaks.
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
                    "error: unknown argument `{other}`. Valid: --stdio, -V/--version, -h/--help. \
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
