//! The checkout the rules read: its root, its files, and its workspace members.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A workspace member as `cargo metadata` reports it.
pub struct Member {
    pub name: String,
    /// The directory holding its `Cargo.toml`, relative to the repository root.
    pub dir: PathBuf,
    /// Its library and binary crate roots, relative to the repository root.
    pub roots: Vec<PathBuf>,
}

pub struct Repo {
    pub root: PathBuf,
    /// `--tag` from the command line, for the rules that compare against a release tag.
    pub tag: Option<String>,
    /// Every file git tracks or would track, relative to `root`, sorted.
    files: Vec<PathBuf>,
}

impl Repo {
    pub fn open(tag: Option<String>) -> Result<Self, String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or("xtask has no parent directory")?
            .to_path_buf();
        let output = Command::new("git")
            .args([
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ])
            .current_dir(&root)
            .output()
            .map_err(|error| format!("cannot run git: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "git ls-files failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let mut files: Vec<PathBuf> = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(|path| PathBuf::from(String::from_utf8_lossy(path).into_owned()))
            // A tracked file deleted in the working tree is not there to read.
            .filter(|path| root.join(path).is_file())
            .collect();
        files.sort();
        files.dedup();
        Ok(Self { root, tag, files })
    }

    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// The files under `dir` whose extension is one of `extensions`.
    pub fn files_in<'a>(
        &'a self,
        dir: &'a Path,
        extensions: &'a [&str],
    ) -> impl Iterator<Item = &'a PathBuf> + 'a {
        self.files.iter().filter(move |path| {
            path.starts_with(dir)
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extensions.contains(&extension))
        })
    }

    pub fn read(&self, path: &Path) -> Result<String, String> {
        std::fs::read_to_string(self.root.join(path))
            .map_err(|error| format!("cannot read {}: {error}", path.display()))
    }

    pub fn manifest(&self, path: &Path) -> Result<toml::Table, String> {
        self.read(path)?
            .parse()
            .map_err(|error| format!("{} is not valid TOML: {error}", path.display()))
    }

    pub fn cargo(&self) -> Command {
        let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
        command.current_dir(&self.root);
        command
    }

    pub fn members(&self) -> Result<Vec<Member>, String> {
        let output = self
            .cargo()
            .args(["metadata", "--format-version", "1", "--no-deps"])
            .output()
            .map_err(|error| format!("cannot run cargo metadata: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "cargo metadata failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("cargo metadata printed invalid JSON: {error}"))?;
        let mut members = Vec::new();
        for package in metadata["packages"].as_array().into_iter().flatten() {
            let (Some(name), Some(manifest)) =
                (package["name"].as_str(), package["manifest_path"].as_str())
            else {
                return Err("cargo metadata lists a package without a name or manifest".into());
            };
            let relative = |path: &str| {
                Path::new(path)
                    .strip_prefix(&self.root)
                    .map(Path::to_path_buf)
                    .map_err(|_| format!("{path} is outside {}", self.root.display()))
            };
            let dir = relative(manifest)?
                .parent()
                .ok_or_else(|| format!("{manifest} has no parent directory"))?
                .to_path_buf();
            let mut roots = Vec::new();
            for target in package["targets"].as_array().into_iter().flatten() {
                let crate_root = target["kind"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|kind| kind == "lib" || kind == "bin");
                if crate_root && let Some(src) = target["src_path"].as_str() {
                    roots.push(relative(src)?);
                }
            }
            members.push(Member {
                name: name.to_owned(),
                dir,
                roots,
            });
        }
        members.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(members)
    }
}

/// The 1-based line of a byte offset into `text`.
pub fn line_of(text: &str, offset: usize) -> usize {
    text[..offset].matches('\n').count() + 1
}
