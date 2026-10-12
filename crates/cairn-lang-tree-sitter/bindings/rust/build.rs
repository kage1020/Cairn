//! Compiles the generated parser and the external scanner for the Rust binding.

fn main() {
    let src_dir = std::path::Path::new("src");

    let mut cc = cc::Build::new();
    cc.include(src_dir);
    // Unconditional: a library-only build never links, so would not report a missing `scanner.c`.
    cc.file(src_dir.join("parser.c"));
    cc.file(src_dir.join("scanner.c"));
    // `cc` prints a `rerun-if-env-changed`, which stops Cargo watching the whole package.
    println!("cargo::rerun-if-changed=src/parser.c");
    println!("cargo::rerun-if-changed=src/scanner.c");

    cc.flag_if_supported("-Wno-unused-parameter");
    cc.flag_if_supported("-Wno-unused-but-set-variable");
    cc.flag_if_supported("-Wno-trigraphs");

    cc.compile("tree_sitter_cairn");
}
