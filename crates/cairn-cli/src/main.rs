//! Cairn command-line entry point.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(cmd) = args.next() else {
        print_usage();
        return ExitCode::from(2);
    };

    match cmd.as_str() {
        "--version" | "-V" => {
            println!("cairn {}", cairn_core::CAIRN_VERSION);
            ExitCode::SUCCESS
        }
        "--help" | "-h" => {
            print_usage();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("error: unknown subcommand `{other}` (the compiler is not implemented yet)");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    println!(
        "cairn — Minecraft build DSL (spec {})\n\n\
         USAGE:\n  \
             cairn <command> [options]\n\n\
         COMMANDS:\n  \
             compile        Compile a .crn file to NBT (not implemented yet)\n  \
             import         Import a schematic to .crn (not implemented yet)\n  \
             info           Show compatibility ranges and provenance (not implemented yet)\n  \
             diff-blocks    Compare a schematic against a .crn build (not implemented yet)\n\n\
         OPTIONS:\n  \
             -V, --version  Print the Cairn release version\n  \
             -h, --help     Print this help",
        cairn_core::CAIRN_VERSION
    );
}
