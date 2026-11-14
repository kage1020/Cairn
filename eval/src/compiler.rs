//! The compiler every program is checked with: this checkout's `cairn` binary, run as an author
//! runs it, so the harness sees exactly the diagnostics and blocks an author would.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use crate::conversation::{SOURCE, Target};
use crate::json::canonical;
use crate::record::DiagnosticRecord;

/// The workspace manifest the `cairn` binary is built from.
const WORKSPACE_MANIFEST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.toml");

/// Why the harness could not use the compiler: never a finding about a program.
#[derive(Debug, Error)]
pub enum CompilerError {
    /// A command could not be started or its files written.
    #[error("{what}: {error}")]
    Io {
        /// What was being done.
        what: String,
        /// Why it failed.
        error: std::io::Error,
    },
    /// A command ended in a way no program explains.
    #[error("{0}")]
    Refused(String),
}

/// A cell of a build: the structure it is in and its position there.
pub type CellKey = (String, usize, usize, usize);

/// The blocks a build holds, each cell's block written out with its properties and block entity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cells(pub BTreeMap<CellKey, String>);

/// What `cairn check`, `cairn compile` and `cairn lower` made of a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Checked and compiled clean, building these blocks.
    Pass(Cells),
    /// `cairn check` refused it, printing this.
    Check(String),
    /// `cairn compile` refused it, printing this.
    Compile(String),
    /// Checked and compiled clean, building no block.
    Empty,
}

/// A program's outcome, with the diagnostics `cairn check --format json` reported on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    /// What `cairn check` reported, warnings included.
    pub diagnostics: Vec<DiagnosticRecord>,
    /// What came of the program.
    pub outcome: Outcome,
}

/// A `cairn` binary pinned to one `(edition, version)` target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiler {
    binary: PathBuf,
    target: Target,
}

fn io(what: impl Into<String>) -> impl FnOnce(std::io::Error) -> CompilerError {
    let what = what.into();
    move |error| CompilerError::Io { what, error }
}

#[derive(Deserialize)]
struct CheckDiagnostic {
    code: String,
    severity: String,
    #[serde(default)]
    line: u64,
    #[serde(default)]
    col: u64,
}

#[derive(Deserialize)]
struct Ir {
    structures: BTreeMap<String, Structure>,
}

#[derive(Deserialize)]
struct Dims {
    x: usize,
    z: usize,
}

#[derive(Deserialize)]
struct PaletteEntry {
    id: String,
    #[serde(default)]
    properties: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
struct StructureBlockEntity {
    pos: [usize; 3],
    id: String,
    #[serde(default)]
    fields: Value,
}

#[derive(Deserialize)]
struct Structure {
    dims: Dims,
    palette: Vec<PaletteEntry>,
    voxels: Vec<usize>,
    #[serde(default)]
    block_entities: Vec<StructureBlockEntity>,
}

impl Cells {
    /// The non-air cells of the block-array IR `cairn lower --format json` printed as `json`.
    ///
    /// # Errors
    ///
    /// [`CompilerError::Refused`] when `json` is not that IR.
    pub fn from_ir(json: &str) -> Result<Self, CompilerError> {
        let ir: Ir = serde_json::from_str(json).map_err(|error| {
            CompilerError::Refused(format!(
                "`cairn lower` printed an IR the harness cannot read: {error}"
            ))
        })?;
        let mut cells = BTreeMap::new();
        for (key, structure) in ir.structures {
            let entities: BTreeMap<[usize; 3], String> = structure
                .block_entities
                .iter()
                .map(|entity| {
                    (
                        entity.pos,
                        format!("{{{}:{}}}", entity.id, canonical(&entity.fields)),
                    )
                })
                .collect();
            let blocks: Vec<Option<String>> = structure
                .palette
                .iter()
                .map(|entry| {
                    (entry.id != "minecraft:air").then(|| {
                        let properties: Vec<String> = entry
                            .properties
                            .iter()
                            .map(|(name, value)| match value {
                                Value::String(value) => format!("{name}={value}"),
                                other => format!("{name}={other}"),
                            })
                            .collect();
                        if properties.is_empty() {
                            entry.id.clone()
                        } else {
                            format!("{}[{}]", entry.id, properties.join(","))
                        }
                    })
                })
                .collect();
            let (width, depth) = (structure.dims.x, structure.dims.z);
            if width == 0 || depth == 0 {
                continue;
            }
            for (index, palette) in structure.voxels.iter().enumerate() {
                let Some(Some(block)) = blocks.get(*palette) else {
                    continue;
                };
                let (x, z, y) = (
                    index % width,
                    index / width % depth,
                    index / (width * depth),
                );
                let mut cell = block.clone();
                if let Some(entity) = entities.get(&[x, y, z]) {
                    cell.push_str(entity);
                }
                cells.insert((key.clone(), x, y, z), cell);
            }
        }
        Ok(Self(cells))
    }

    /// How many cells hold a block.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether no cell holds a block.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Compiler {
    /// The `cairn` binary at `binary`, checking and compiling for `target`.
    #[must_use]
    pub fn new(binary: PathBuf, target: Target) -> Self {
        // Each command runs in its program's directory, where a relative path would not resolve.
        let binary = std::path::absolute(&binary).unwrap_or(binary);
        Self { binary, target }
    }

    /// The target every program is checked and compiled for.
    #[must_use]
    pub fn target(&self) -> &Target {
        &self.target
    }

    /// Builds this checkout's `cairn` binary and returns its path.
    ///
    /// # Errors
    ///
    /// [`CompilerError`] when cargo cannot be run or the build fails.
    pub fn build() -> Result<PathBuf, CompilerError> {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let output = Command::new(cargo)
            .args(["build", "--quiet", "--locked", "--message-format=json"])
            .args([
                "--package",
                "cairn-lang-cli",
                "--bin",
                "cairn",
                "--manifest-path",
            ])
            .arg(WORKSPACE_MANIFEST)
            .stderr(Stdio::inherit())
            .output()
            .map_err(io("cannot run cargo to build `cairn`"))?;
        if !output.status.success() {
            return Err(CompilerError::Refused(
                "cargo could not build `cairn`".to_owned(),
            ));
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|message| message["reason"] == "compiler-artifact")
            .filter(|message| message["target"]["name"] == "cairn")
            .find_map(|message| message["executable"].as_str().map(PathBuf::from))
            .ok_or_else(|| {
                CompilerError::Refused("cargo built `cairn` but named no executable".to_owned())
            })
    }

    fn run(&self, dir: &Path, args: &[&str]) -> Result<Output, CompilerError> {
        let output = Command::new(&self.binary)
            .args(args)
            .args([
                "--edition",
                &self.target.edition,
                "--target",
                &self.target.version,
            ])
            .arg(SOURCE)
            .current_dir(dir)
            .output()
            .map_err(io(format!("cannot run {}", self.binary.display())))?;
        match output.status.code() {
            Some(0 | 1) => Ok(output),
            _ => Err(CompilerError::Refused(format!(
                "`cairn {}` ended with {}: {}",
                args.join(" "),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim_end()
            ))),
        }
    }

    /// Confirms the binary takes the target, by checking an empty program in `dir`.
    ///
    /// # Errors
    ///
    /// [`CompilerError::Refused`] with what `cairn check` printed, when it refuses the target.
    pub fn validate(&self, dir: &Path) -> Result<(), CompilerError> {
        fs::write(dir.join(SOURCE), "").map_err(io("cannot write an empty program"))?;
        let output = self.run(dir, &["check"])?;
        if output.status.success() {
            Ok(())
        } else {
            Err(CompilerError::Refused(
                String::from_utf8_lossy(&output.stderr)
                    .trim_end()
                    .to_owned(),
            ))
        }
    }

    /// Checks `program` in `dir`, then compiles and lowers it there if the check passes.
    ///
    /// # Errors
    ///
    /// [`CompilerError`] when a command cannot be run or ends in a way no program explains.
    pub fn evaluate(&self, program: &str, dir: &Path) -> Result<Evaluation, CompilerError> {
        fs::create_dir_all(dir).map_err(io(format!("cannot create {}", dir.display())))?;
        fs::write(dir.join(SOURCE), program)
            .map_err(io(format!("cannot write {}", dir.join(SOURCE).display())))?;
        let checked = self.run(dir, &["check", "--format", "json"])?;
        let diagnostics: Vec<CheckDiagnostic> =
            serde_json::from_slice(&checked.stdout).map_err(|error| {
                CompilerError::Refused(format!(
                    "`cairn check` printed JSON the harness cannot read: {error}"
                ))
            })?;
        let diagnostics = diagnostics
            .into_iter()
            .map(|diagnostic| DiagnosticRecord {
                code: diagnostic.code,
                severity: diagnostic.severity,
                line: diagnostic.line,
                col: diagnostic.col,
            })
            .collect();
        let printed = |output: &Output| String::from_utf8_lossy(&output.stderr).into_owned();
        let outcome = if checked.status.success() {
            let compiled = self.run(
                dir,
                &["compile", "--out", "out", "--lock", "out/main.crn.lock"],
            )?;
            if compiled.status.success() {
                let lowered = self.run(dir, &["lower", "--format", "json"])?;
                if !lowered.status.success() {
                    return Err(CompilerError::Refused(format!(
                        "`cairn lower` refused a program `cairn compile` built: {}",
                        printed(&lowered).trim_end()
                    )));
                }
                let cells = Cells::from_ir(&String::from_utf8_lossy(&lowered.stdout))?;
                if cells.is_empty() {
                    Outcome::Empty
                } else {
                    Outcome::Pass(cells)
                }
            } else {
                Outcome::Compile(printed(&compiled))
            }
        } else {
            let text = self.run(dir, &["check"])?;
            Outcome::Check(printed(&text))
        };
        Ok(Evaluation {
            diagnostics,
            outcome,
        })
    }
}
