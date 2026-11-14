//! The checkout the rules read: its root, its files, and its workspace members.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A workspace member as `cargo metadata` reports it.
#[derive(Debug, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    /// The directory holding its `Cargo.toml`, relative to the repository root.
    pub dir: PathBuf,
    /// Its library and binary crate roots, relative to the repository root.
    pub roots: Vec<PathBuf>,
    /// Its library crate root, relative to the repository root, when it has a library.
    pub library: Option<PathBuf>,
    /// Its README, relative to the repository root, when it has one.
    pub readme: Option<PathBuf>,
    /// Whether `cargo publish` may publish it.
    pub published: bool,
}

/// The `kind`s `cargo metadata` gives a library target: its crate types.
const LIBRARY_KINDS: &[&str] = &["lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"];

pub struct Repo {
    pub root: PathBuf,
    /// `--tag` from the command line, for the rules that compare against a release tag.
    pub tag: Option<String>,
    /// `--site` from the command line, for the rule that compares against a built site.
    pub site: Option<PathBuf>,
    /// Every file git tracks or would track, relative to `root`, sorted.
    files: Vec<PathBuf>,
}

impl Repo {
    pub fn open(tag: Option<String>, site: Option<PathBuf>) -> Result<Self, String> {
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
        Ok(Self {
            root,
            tag,
            site,
            files,
        })
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
        let mut members = metadata["packages"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|package| member(package, &self.root))
            .collect::<Result<Vec<_>, _>>()?;
        members.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(members)
    }
}

/// The member an entry of `cargo metadata`'s `packages` describes, in the workspace at `root`.
///
/// # Errors
///
/// When the entry lacks a name or manifest, or a path in it lies outside `root`.
fn member(package: &serde_json::Value, root: &Path) -> Result<Member, String> {
    let (Some(name), Some(manifest)) =
        (package["name"].as_str(), package["manifest_path"].as_str())
    else {
        return Err("cargo metadata lists a package without a name or manifest".into());
    };
    let relative = |path: &str| {
        Path::new(path)
            .strip_prefix(root)
            .map(Path::to_path_buf)
            .map_err(|_| format!("{path} is outside {}", root.display()))
    };
    let dir = relative(manifest)?
        .parent()
        .ok_or_else(|| format!("{manifest} has no parent directory"))?
        .to_path_buf();
    let mut roots = Vec::new();
    let mut library = None;
    for target in package["targets"].as_array().into_iter().flatten() {
        let kinds: Vec<&str> = target["kind"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .collect();
        let is_library = kinds.iter().any(|kind| LIBRARY_KINDS.contains(kind));
        if (is_library || kinds.contains(&"bin"))
            && let Some(src) = target["src_path"].as_str()
        {
            let src = relative(src)?;
            if is_library {
                library = Some(src.clone());
            }
            roots.push(src);
        }
    }
    Ok(Member {
        name: name.to_owned(),
        readme: package["readme"].as_str().map(|readme| dir.join(readme)),
        dir,
        roots,
        library,
        // `publish = false` lists no registry, and `null` allows every one.
        published: package["publish"]
            .as_array()
            .is_none_or(|registries| !registries.is_empty()),
    })
}

/// The 1-based line of a byte offset into `text`.
pub fn line_of(text: &str, offset: usize) -> usize {
    text[..offset].matches('\n').count() + 1
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_member_lists_its_library_and_binary_roots_and_its_readme() {
        let package = json!({
            "name": "a",
            "manifest_path": "/w/crates/a/Cargo.toml",
            "publish": null,
            "readme": "docs/README.md",
            "targets": [
                {"kind": ["cdylib", "rlib"], "src_path": "/w/crates/a/src/lib.rs"},
                {"kind": ["bin"], "src_path": "/w/crates/a/src/main.rs"},
                {"kind": ["test"], "src_path": "/w/crates/a/tests/t.rs"},
                {"kind": ["custom-build"], "src_path": "/w/crates/a/build.rs"},
            ],
        });
        assert_eq!(
            member(&package, Path::new("/w")),
            Ok(Member {
                name: "a".to_owned(),
                dir: PathBuf::from("crates/a"),
                roots: vec![
                    PathBuf::from("crates/a/src/lib.rs"),
                    PathBuf::from("crates/a/src/main.rs"),
                ],
                library: Some(PathBuf::from("crates/a/src/lib.rs")),
                readme: Some(PathBuf::from("crates/a/docs/README.md")),
                published: true,
            })
        );
    }

    #[test]
    fn publish_false_is_an_empty_list_of_registries() {
        let package = |publish| {
            json!({
                "name": "x",
                "manifest_path": "/w/x/Cargo.toml",
                "publish": publish,
                "targets": [{"kind": ["bin"], "src_path": "/w/x/src/main.rs"}],
            })
        };
        let unpublished = Member {
            name: "x".to_owned(),
            dir: PathBuf::from("x"),
            roots: vec![PathBuf::from("x/src/main.rs")],
            library: None,
            readme: None,
            published: false,
        };
        assert_eq!(
            member(&package(json!([])), Path::new("/w")).as_ref(),
            Ok(&unpublished)
        );
        assert_eq!(
            member(&package(json!(["internal"])), Path::new("/w")),
            Ok(Member {
                published: true,
                ..unpublished
            })
        );
    }

    #[test]
    fn a_target_of_any_library_crate_type_is_the_library() {
        let kinds = ["lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"];
        for kind in kinds {
            let package = json!({
                "name": "a",
                "manifest_path": "/w/a/Cargo.toml",
                "publish": null,
                "targets": [{"kind": [kind], "src_path": "/w/a/src/lib.rs"}],
            });
            assert_eq!(
                member(&package, Path::new("/w")),
                Ok(Member {
                    name: "a".to_owned(),
                    dir: PathBuf::from("a"),
                    roots: vec![PathBuf::from("a/src/lib.rs")],
                    library: Some(PathBuf::from("a/src/lib.rs")),
                    readme: None,
                    published: true,
                }),
                "{kind}"
            );
        }
    }
}
