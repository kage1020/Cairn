//! `cairn install`: structure files placed in a world folder, each held to the world's version,
//! and the worlds, files and ids it refuses before writing anything.

mod cross_version;
mod files;
mod world;

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::Compression;
use flate2::write::GzEncoder;
use tempfile::TempDir;

use crate::common::{Run, cargo_bin, examples_dir};

/// A folder holding `saves/`, the structure files compiled into it and the runs made in it.
pub struct Saves {
    dir: TempDir,
}

impl Saves {
    pub fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        fs::copy(
            examples_dir().join("cottage.crn"),
            dir.path().join("cottage.crn"),
        )
        .expect("copy the example");
        Self { dir }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// The cottage example compiled for `edition` at `target`, as the path a run names it by.
    pub fn compiled(&self, edition: &str, target: &str) -> String {
        let out = Command::new(cargo_bin())
            .current_dir(self.path())
            .args([
                "compile",
                "cottage.crn",
                "--edition",
                edition,
                "--target",
                target,
            ])
            .args(["--out", target, "--lock"])
            .arg(format!("{target}.lock"))
            .output()
            .expect("run cairn compile");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let ext = if edition == "java" {
            "nbt"
        } else {
            "mcstructure"
        };
        format!("{target}/cottage.{ext}")
    }

    /// `saves/<world>`, its `level.dat` holding `bytes`.
    pub fn world_with(&self, world: &str, bytes: &[u8]) -> String {
        let folder = format!("saves/{world}");
        fs::create_dir_all(self.path().join(&folder)).expect("mkdir");
        fs::write(self.path().join(&folder).join("level.dat"), bytes).expect("write level.dat");
        folder
    }

    /// `saves/<world>`, a Java world the game last saved at `data_version`, releasing `name`.
    pub fn java_world(&self, world: &str, data_version: i32, name: &str) -> String {
        self.world_with(world, &java_level_dat(Some(data_version), Some(name)))
    }

    /// `cairn install` with `args`, run in this folder.
    pub fn install(&self, args: &[&str]) -> Run {
        Run::from(
            Command::new(cargo_bin())
                .current_dir(self.path())
                .arg("install")
                .args(args)
                .output()
                .expect("run cairn install"),
        )
    }

    /// The bytes at `relative`, `None` when nothing is there.
    pub fn read(&self, relative: &Path) -> Option<Vec<u8>> {
        fs::read(self.path().join(relative)).ok()
    }
}

/// The file `world` holds a structure id's file at, `generated/<namespace>/structures/<parts>`,
/// joined as the platform joins paths.
pub fn structure_at(world: &str, namespace: &str, parts: &[&str]) -> PathBuf {
    let mut path = Path::new(world)
        .join("generated")
        .join(namespace)
        .join("structures");
    path.extend(parts);
    path
}

/// A big-endian NBT name: its length, then its bytes.
fn name(text: &str) -> Vec<u8> {
    let len = u16::try_from(text.len()).expect("short");
    [&len.to_be_bytes()[..], text.as_bytes()].concat()
}

/// A Java `level.dat`: gzip NBT whose `Data` holds `DataVersion` and `Version.Name` where given.
pub fn java_level_dat(data_version: Option<i32>, release: Option<&str>) -> Vec<u8> {
    let mut data = [&[8][..], &name("LevelName"), &name("New World")].concat();
    if let Some(data_version) = data_version {
        data.extend([&[3][..], &name("DataVersion"), &data_version.to_be_bytes()].concat());
    }
    if let Some(release) = release {
        data.extend(
            [
                &[10][..],
                &name("Version"),
                &[8],
                &name("Name"),
                &name(release),
                &[0],
            ]
            .concat(),
        );
    }
    let root = [&[10][..], &name(""), &[10], &name("Data"), &data, &[0, 0]].concat();
    let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
    gzip.write_all(&root).expect("gzip");
    gzip.finish().expect("gzip")
}

/// A Bedrock `level.dat`: an 8-byte header, then little-endian NBT naming the release that last
/// opened it, 1.21.60.
pub fn bedrock_level_dat() -> Vec<u8> {
    let le_name = |text: &str| {
        let len = u16::try_from(text.len()).expect("short");
        [&len.to_le_bytes()[..], text.as_bytes()].concat()
    };
    let release: Vec<u8> = [1_i32, 21, 60, 2, 0]
        .iter()
        .flat_map(|part| part.to_le_bytes())
        .collect();
    let payload = [
        &[10][..],
        &le_name(""),
        &[9],
        &le_name("lastOpenedWithVersion"),
        &[3],
        &5_i32.to_le_bytes(),
        &release,
        &[0],
    ]
    .concat();
    let len = u32::try_from(payload.len()).expect("short");
    [&10_i32.to_le_bytes()[..], &len.to_le_bytes(), &payload].concat()
}
