//! Build script: compile the tree-sitter generated parser and the external
//! scanner into a static library linked by the Rust binding.

fn main() {
    let src_dir = std::path::Path::new("src");

    let mut cc = cc::Build::new();
    cc.include(src_dir);
    cc.file(src_dir.join("parser.c"));
    // Without this, cargo watches the manifest directory instead — and
    // `build = "bindings/rust/build.rs"` puts the C sources outside the
    // build script's own directory, so an edited grammar or scanner leaves
    // the previously linked parser in place and every test keeps asserting
    // against the old one.
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
