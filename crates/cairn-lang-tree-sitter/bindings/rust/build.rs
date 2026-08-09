//! Build script: compile the tree-sitter generated parser and the external
//! scanner into a static library linked by the Rust binding.

fn main() {
    let src_dir = std::path::Path::new("src");

    let mut cc = cc::Build::new();
    cc.include(src_dir);
    cc.file(src_dir.join("parser.c"));
    // Cargo watches the whole package only until a build script emits its
    // first `rerun-if-*` line, after which the script owns the list — and
    // `cc` emits `rerun-if-env-changed` for the toolchain variables it
    // reads. The C sources are therefore watched by nobody unless they are
    // named here, and an edited grammar or scanner leaves the previously
    // linked parser in place while every test goes on asserting against
    // it.
    println!("cargo::rerun-if-changed=src/parser.c");
    println!("cargo::rerun-if-changed=src/scanner.c");

    let scanner = src_dir.join("scanner.c");
    if scanner.exists() {
        cc.file(scanner);
    }

    cc.flag_if_supported("-Wno-unused-parameter");
    cc.flag_if_supported("-Wno-unused-but-set-variable");
    cc.flag_if_supported("-Wno-trigraphs");

    cc.compile("tree_sitter_cairn");
}
