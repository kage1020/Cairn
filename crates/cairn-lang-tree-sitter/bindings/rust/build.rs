//! Build script: compile the tree-sitter generated parser and the external
//! scanner into a static library linked by the Rust binding.

fn main() {
    let src_dir = std::path::Path::new("src");

    let mut cc = cc::Build::new();
    cc.include(src_dir);
    // Both files are required: `parser.c` takes the addresses of the
    // `tree_sitter_cairn_external_scanner_*` functions into the
    // `TSLanguage`'s `external_scanner` table, and only `scanner.c` defines
    // them, so a build without the scanner cannot link. Naming it
    // unconditionally turns a missing file into a C-compiler error naming
    // `src/scanner.c`, in this build script, instead of an undefined-symbol
    // error at the first link — which a library-only build, such as the one
    // `cargo publish` verifies, never reaches.
    cc.file(src_dir.join("parser.c"));
    cc.file(src_dir.join("scanner.c"));
    // Cargo watches the whole package only until a build script emits its
    // first `rerun-if-*` line, after which the script owns the list — and
    // `cc` emits `rerun-if-env-changed` for the toolchain variables it
    // reads. The C sources are therefore watched by nobody unless they are
    // named here, and an edited grammar or scanner leaves the previously
    // linked parser in place while every test goes on asserting against
    // it.
    println!("cargo::rerun-if-changed=src/parser.c");
    println!("cargo::rerun-if-changed=src/scanner.c");

    cc.flag_if_supported("-Wno-unused-parameter");
    cc.flag_if_supported("-Wno-unused-but-set-variable");
    cc.flag_if_supported("-Wno-trigraphs");

    cc.compile("tree_sitter_cairn");
}
