//! Build script: compile the tree-sitter generated parser and the external
//! scanner into a static library linked by the Rust binding.

fn main() {
    let src_dir = std::path::Path::new("src");

    let mut cc = cc::Build::new();
    cc.include(src_dir);
    // Both files are required: `parser.c` references the
    // `tree_sitter_cairn_external_scanner_*` functions only `scanner.c`
    // defines. Naming it unconditionally turns a missing file into a
    // C-compiler error here rather than an undefined-symbol error at the
    // first link, which a library-only build such as `cargo publish`'s
    // verification never reaches.
    cc.file(src_dir.join("parser.c"));
    cc.file(src_dir.join("scanner.c"));
    // Cargo watches the whole package only until a build script emits its
    // first `rerun-if-*` line, and `cc` emits `rerun-if-env-changed`, so
    // the C sources are watched only if named here.
    println!("cargo::rerun-if-changed=src/parser.c");
    println!("cargo::rerun-if-changed=src/scanner.c");

    cc.flag_if_supported("-Wno-unused-parameter");
    cc.flag_if_supported("-Wno-unused-but-set-variable");
    cc.flag_if_supported("-Wno-trigraphs");

    cc.compile("tree_sitter_cairn");
}
