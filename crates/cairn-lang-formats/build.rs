//! Re-run the build whenever a built-in registry pack file changes.
//!
//! `include_str!` in `src/registry/load.rs` pulls these JSON files in at
//! compile time, but Cargo only watches Rust source by default — without
//! this hook, editing `data/registry/java/*.json` would not trigger a
//! rebuild and the embedded bytes would silently lag behind the on-disk
//! source of truth.

fn main() {
    let pack_files = [
        "../../data/registry/java/pack.json",
        "../../data/registry/java/data_versions.json",
    ];
    for path in pack_files {
        println!("cargo:rerun-if-changed={path}");
    }
}
