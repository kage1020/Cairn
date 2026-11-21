//! The `workspace/executeCommand` commands, run over the documents the editor holds open.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use cairn_lang_formats::diff_blocks::diff_blocks;
use cairn_lang_formats::import::{DecodedStructure, ImportFormat, decode_structure};
use cairn_lang_formats::info::{
    EditionsRefused, Found, InfoRequest, InfoSink, ReportFormat, RequestedEditions, info,
    requested_editions,
};
use lsp_server::ErrorCode;

use crate::store::DocumentStore;

/// The command answering with the document `cairn info --format json` prints for an open source
/// (`spec/versioning-editions` "The report in an editor").
pub const INFO: &str = "cairn.info";

/// The command answering with the document `cairn diff-blocks --format json` prints for an open
/// source and a structure file (`spec/ecosystem-interop` "The report in an editor").
pub const DIFF_BLOCKS: &str = "cairn.diffBlocks";

/// Every command the server runs, as `initialize` advertises them.
pub const COMMANDS: &[&str] = &[INFO, DIFF_BLOCKS];

/// Why a command did not run: the JSON-RPC error its response carries.
#[derive(Debug, PartialEq, Eq)]
pub struct CommandRefused {
    /// The JSON-RPC error code.
    pub code: i32,
    /// What was wrong, and what the command takes.
    pub message: String,
}

impl CommandRefused {
    fn invalid_params(message: String) -> Self {
        Self {
            code: ErrorCode::InvalidParams as i32,
            message,
        }
    }
}

/// `cairn.info`'s one argument.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InfoArguments {
    uri: lsp_types::Uri,
    /// The list `cairn info --editions` takes, or `None` for its default.
    editions: Option<Vec<String>>,
}

/// `cairn.diffBlocks`' one argument.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DiffBlocksArguments {
    uri: lsp_types::Uri,
    /// The structure file's `file:` URI.
    structure: lsp_types::Uri,
}

/// Runs the command `params` names over the documents `store` holds, returning its result.
///
/// # Errors
///
/// A [`CommandRefused`] for params that are not `ExecuteCommandParams`, a command this server
/// does not run, or arguments the command refuses.
pub fn execute_command(
    store: &DocumentStore,
    params: serde_json::Value,
) -> Result<serde_json::Value, CommandRefused> {
    let params: lsp_types::ExecuteCommandParams =
        serde_json::from_value(params).map_err(|err| {
            CommandRefused::invalid_params(format!(
                "malformed `workspace/executeCommand` params: {err}"
            ))
        })?;
    match params.command.as_str() {
        INFO => run_info(store, params.arguments),
        DIFF_BLOCKS => run_diff_blocks(store, params.arguments),
        other => Err(CommandRefused::invalid_params(format!(
            "unknown command `{other}`; this server runs {}",
            COMMANDS
                .iter()
                .map(|command| format!("`{command}`"))
                .collect::<Vec<_>>()
                .join(", "),
        ))),
    }
}

/// `cairn.info` over the open document its argument names.
fn run_info(
    store: &DocumentStore,
    arguments: Vec<serde_json::Value>,
) -> Result<serde_json::Value, CommandRefused> {
    let usage = "takes one argument, `{ \"uri\": <document>, \"editions\"?: [...] }`";
    let [argument] = <[serde_json::Value; 1]>::try_from(arguments).map_err(|arguments| {
        CommandRefused::invalid_params(format!("`{INFO}` {usage}, and got {}", arguments.len()))
    })?;
    let InfoArguments { uri, editions } = serde_json::from_value(argument)
        .map_err(|err| CommandRefused::invalid_params(format!("`{INFO}` {usage}: {err}")))?;
    let editions = match editions {
        None => RequestedEditions::default(),
        Some(editions) => requested_editions(&editions)
            .map_err(|refused| CommandRefused::invalid_params(editions_refused(&refused)))?,
    };
    let source = open_document(store, INFO, &uri)?;
    let file = file_path(&uri).ok_or_else(|| {
        CommandRefused::invalid_params(format!(
            "`{INFO}` reads the lock beside the source, so it needs a local `file:` URI, and got \
             {}",
            uri.as_str(),
        ))
    })?;
    let request = InfoRequest {
        file: &file,
        source,
        editions: &editions,
        lock: None,
        format: ReportFormat::Json,
    };
    let document = match info(&request, &mut Unsent) {
        Ok(document) => serde_json::to_value(document),
        // No finding to put in the document, and no exit code to say the run was refused.
        Err(refused) if refused.findings.is_empty() => {
            return Err(CommandRefused {
                code: ErrorCode::RequestFailed as i32,
                message: format!(
                    "`{INFO}` has no report for {}, and no finding refused it:\n{}",
                    uri.as_str(),
                    refused.palettes.join("\n"),
                ),
            });
        }
        Err(refused) => serde_json::to_value(refused.document(source)),
    };
    document.map_err(|err| unserialisable(INFO, &err))
}

/// `cairn.diffBlocks` over the open document and the structure file its argument names.
fn run_diff_blocks(
    store: &DocumentStore,
    arguments: Vec<serde_json::Value>,
) -> Result<serde_json::Value, CommandRefused> {
    let usage = "takes one argument, `{ \"uri\": <document>, \"structure\": <file> }`";
    let [argument] = <[serde_json::Value; 1]>::try_from(arguments).map_err(|arguments| {
        CommandRefused::invalid_params(format!(
            "`{DIFF_BLOCKS}` {usage}, and got {}",
            arguments.len()
        ))
    })?;
    let DiffBlocksArguments { uri, structure } = serde_json::from_value(argument)
        .map_err(|err| CommandRefused::invalid_params(format!("`{DIFF_BLOCKS}` {usage}: {err}")))?;
    let source = open_document(store, DIFF_BLOCKS, &uri)?;
    let path = file_path(&structure).ok_or_else(|| {
        CommandRefused::invalid_params(format!(
            "`{DIFF_BLOCKS}` reads the structure file from disk, so `structure` needs a local \
             `file:` URI, and got {}",
            structure.as_str(),
        ))
    })?;
    let decoded = read_structure(&path).map_err(|message| {
        CommandRefused::invalid_params(format!("`{DIFF_BLOCKS}` `structure`: {message}"))
    })?;
    let run = diff_blocks(source, &decoded);
    let document = match &run.outcome {
        Ok(document) => serde_json::to_value(document),
        Err(_) => serde_json::to_value(run.failure_document(source)),
    };
    document.map_err(|err| unserialisable(DIFF_BLOCKS, &err))
}

/// The structure file at `path`, decoded as its extension names, or why it cannot be.
fn read_structure(path: &Path) -> Result<DecodedStructure, String> {
    let Some(format) = ImportFormat::from_path(path) else {
        return Err(format!(
            "`{}` is not a structure file `cairn diff-blocks` reads; name a {} file",
            path.display(),
            ImportFormat::choices(),
        ));
    };
    let reader = File::open(path)
        .map(BufReader::new)
        .map_err(|err| format!("cannot read `{}`: {err}", path.display()))?;
    let name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    decode_structure(reader, format, &name)
        .map_err(|err| format!("cannot compare `{}`: {err}", path.display()))
}

/// The error answering a document `command` could not serialise.
fn unserialisable(command: &str, err: &serde_json::Error) -> CommandRefused {
    CommandRefused {
        code: ErrorCode::InternalError as i32,
        message: format!("failed to serialise the `{command}` document: {err}"),
    }
}

/// Why `cairn.info`'s `editions` names nothing to report on.
fn editions_refused(refused: &EditionsRefused) -> String {
    match refused {
        EditionsRefused::Empty => format!(
            "`{INFO}` `editions` names no edition; list at least one of \"java\", \"bedrock\", \
             or leave it out for both"
        ),
        EditionsRefused::EmptyEntry => format!(
            "`{INFO}` `editions` holds an empty entry; each entry is \"java\" or \"bedrock\""
        ),
        EditionsRefused::Unknown(unknown) => format!("`{INFO}` `editions`: {unknown}"),
    }
}

/// The text the editor holds for `uri`, or the refusal saying `command` needs it open.
fn open_document<'a>(
    store: &'a DocumentStore,
    command: &str,
    uri: &lsp_types::Uri,
) -> Result<&'a str, CommandRefused> {
    store.get(uri).ok_or_else(|| {
        CommandRefused::invalid_params(format!(
            "`{command}` reports on a document the editor has open, and {} is not open; send \
             `textDocument/didOpen` for it first",
            uri.as_str(),
        ))
    })
}

/// What `cairn info` writes on stderr, which a response has no place for.
struct Unsent;

impl InfoSink for Unsent {
    fn finding(&mut self, _: &Found) {}

    fn line(&mut self, _: &str) {}
}

/// The path a `file:` URI names, or `None` for another scheme, a remote host off Windows, or a
/// path that is relative or does not decode to UTF-8.
fn file_path(uri: &lsp_types::Uri) -> Option<PathBuf> {
    if !uri
        .scheme()
        .is_some_and(|scheme| scheme.eq_lowercase("file"))
    {
        return None;
    }
    let host = match uri.authority() {
        None => None,
        Some(authority) if authority.userinfo().is_some() || authority.port().is_some() => {
            return None;
        }
        Some(authority) => match authority.host().as_str() {
            "" | "localhost" => None,
            host => Some(host),
        },
    };
    let path = uri.path().as_estr().decode().into_string().ok()?;
    if !path.starts_with('/') {
        return None;
    }
    if cfg!(windows) {
        let path = path.replace('/', "\\");
        // `file://server/share/shop.crn` names `\\server\share\shop.crn`.
        if let Some(host) = host {
            return Some(PathBuf::from(format!(r"\\{host}{path}")));
        }
        // `file:///C:/site/shop.crn` names `C:\site\shop.crn`.
        let path = match path.strip_prefix('\\') {
            Some(drive) if drive.as_bytes().get(1) == Some(&b':') => drive,
            _ => &path,
        };
        return Some(PathBuf::from(path));
    }
    host.is_none().then(|| PathBuf::from(path.into_owned()))
}

#[cfg(test)]
mod tests;
