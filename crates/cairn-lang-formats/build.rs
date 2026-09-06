fn main() {
    let pack_files = [
        "registry-data/java/pack.json",
        "registry-data/java/data_versions.json",
        "registry-data/java/materials.json",
    ];
    for path in pack_files {
        println!("cargo:rerun-if-changed={path}");
    }
}
