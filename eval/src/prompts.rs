//! The prompt set: one TOML file per prompt, named `<id>.toml`.
//!
//! A file holds the `topic` the prompt exercises and the `prompt` itself, in prose with no Cairn
//! in it, so the set outlives any change to the syntax. An edit prompt also names its `base`: the
//! prompt whose converged program it asks to change.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The part of the language a prompt exercises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Topic {
    /// Footprints, walls and floors.
    Walls,
    /// Doors and windows.
    Openings,
    /// Roof kinds and their overhangs.
    Roofs,
    /// Storeys stacked with `level`.
    Levels,
    /// Stairs, chests, fills and raw blocks.
    Members,
    /// Themes and their slots.
    Themes,
    /// Reuse through `def`, sites and repetition.
    DefRepeat,
    /// Sensors, actuators and logic.
    Redstone,
    /// Builds that need columns or arches.
    ColumnsArches,
    /// Builds that need furnished interiors.
    Interiors,
}

impl Topic {
    /// The topic as a prompt file spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Walls => "walls",
            Self::Openings => "openings",
            Self::Roofs => "roofs",
            Self::Levels => "levels",
            Self::Members => "members",
            Self::Themes => "themes",
            Self::DefRepeat => "def-repeat",
            Self::Redstone => "redstone",
            Self::ColumnsArches => "columns-arches",
            Self::Interiors => "interiors",
        }
    }
}

impl fmt::Display for Topic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One prompt of the set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Prompt {
    /// The file stem, which names the prompt in a run record.
    pub id: String,
    /// What the prompt exercises.
    pub topic: Topic,
    /// The request, in prose.
    pub prompt: String,
    /// For an edit, the prompt whose converged program it changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PromptFile {
    topic: Topic,
    prompt: String,
    base: Option<String>,
}

/// Why a prompt set cannot be used.
#[derive(Debug, Error)]
pub enum PromptError {
    /// The directory or a file in it could not be read.
    #[error("cannot read {}: {error}", path.display())]
    Io {
        /// What could not be read.
        path: PathBuf,
        /// Why.
        error: std::io::Error,
    },
    /// A file is not a prompt.
    #[error("{}: {message}", path.display())]
    Parse {
        /// The file.
        path: PathBuf,
        /// What is wrong with it.
        message: String,
    },
    /// A file stem is not an id.
    #[error(
        "{}: a prompt id is lowercase letters and digits in words joined by `-`; rename the file",
        path.display()
    )]
    InvalidId {
        /// The file.
        path: PathBuf,
    },
    /// A prompt has no text.
    #[error("prompt `{id}` is empty; write the request under `prompt`")]
    EmptyPrompt {
        /// The prompt.
        id: String,
    },
    /// An edit names a base the set does not hold.
    #[error("prompt `{id}` edits `{base}`, which the set does not hold; name a prompt it holds")]
    UnknownBase {
        /// The edit.
        id: String,
        /// The base it names.
        base: String,
    },
    /// An edit names another edit as its base.
    #[error("prompt `{id}` edits `{base}`, which is itself an edit; name a prompt with no `base`")]
    BaseIsEdit {
        /// The edit.
        id: String,
        /// The base it names.
        base: String,
    },
    /// A selection names a prompt the set does not hold.
    #[error("the set holds no prompt `{id}`")]
    UnknownId {
        /// The id asked for.
        id: String,
    },
    /// The directory holds no prompt.
    #[error("{} holds no `.toml` prompt", dir.display())]
    Empty {
        /// The directory.
        dir: PathBuf,
    },
}

/// The prompts of a set, in id order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSet {
    prompts: Vec<Prompt>,
}

fn is_id(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

impl PromptSet {
    /// The set the `.toml` files directly in `dir` make up.
    ///
    /// # Errors
    ///
    /// [`PromptError`] when a file cannot be read or is not a prompt, or the set is inconsistent.
    pub fn load(dir: &Path) -> Result<Self, PromptError> {
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |error| PromptError::Io { path, error }
        };
        let mut prompts = Vec::new();
        for entry in fs::read_dir(dir).map_err(io(dir))? {
            let path = entry.map_err(io(dir))?.path();
            if path.extension().is_none_or(|extension| extension != "toml") {
                continue;
            }
            let id = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|stem| is_id(stem))
                .ok_or_else(|| PromptError::InvalidId { path: path.clone() })?
                .to_owned();
            let text = fs::read_to_string(&path).map_err(io(&path))?;
            let file: PromptFile = toml::from_str(&text).map_err(|error| PromptError::Parse {
                path: path.clone(),
                message: error.to_string().trim_end().to_owned(),
            })?;
            prompts.push(Prompt {
                id,
                topic: file.topic,
                prompt: file.prompt.replace("\r\n", "\n").trim().to_owned(),
                base: file.base,
            });
        }
        if prompts.is_empty() {
            return Err(PromptError::Empty {
                dir: dir.to_path_buf(),
            });
        }
        Self::new(prompts)
    }

    /// The set `prompts` make up.
    ///
    /// # Errors
    ///
    /// [`PromptError`] when a prompt is empty or an edit's base is missing or is an edit.
    pub fn new(mut prompts: Vec<Prompt>) -> Result<Self, PromptError> {
        prompts.sort_by(|a, b| a.id.cmp(&b.id));
        let set = Self { prompts };
        for prompt in &set.prompts {
            if prompt.prompt.trim().is_empty() {
                return Err(PromptError::EmptyPrompt {
                    id: prompt.id.clone(),
                });
            }
            let Some(base) = &prompt.base else { continue };
            match set.get(base) {
                None => {
                    return Err(PromptError::UnknownBase {
                        id: prompt.id.clone(),
                        base: base.clone(),
                    });
                }
                Some(target) if target.base.is_some() => {
                    return Err(PromptError::BaseIsEdit {
                        id: prompt.id.clone(),
                        base: base.clone(),
                    });
                }
                Some(_) => {}
            }
        }
        Ok(set)
    }

    /// The prompts, in id order.
    #[must_use]
    pub fn prompts(&self) -> &[Prompt] {
        &self.prompts
    }

    /// The prompt named `id`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Prompt> {
        self.prompts.iter().find(|prompt| prompt.id == id)
    }

    /// The prompts `ids` name, with the base of each edit among them.
    ///
    /// # Errors
    ///
    /// [`PromptError::UnknownId`] when an id names no prompt of the set.
    pub fn select(&self, ids: &[String]) -> Result<Self, PromptError> {
        let mut chosen: Vec<&str> = Vec::new();
        for id in ids {
            let prompt = self
                .get(id)
                .ok_or_else(|| PromptError::UnknownId { id: id.clone() })?;
            chosen.push(&prompt.id);
            chosen.extend(prompt.base.as_deref());
        }
        Ok(Self {
            prompts: self
                .prompts
                .iter()
                .filter(|prompt| chosen.contains(&prompt.id.as_str()))
                .cloned()
                .collect(),
        })
    }
}
