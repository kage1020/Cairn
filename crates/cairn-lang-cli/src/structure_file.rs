//! The structure file a command reads: opening it, its name, and the refusal of a path whose
//! extension names no format.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::process::ExitCode;

use cairn_lang_formats::import::ImportFormat;

/// Open a structure file, or report why not: exit 2 for a path that names nothing, 1 for anything
/// else.
pub(crate) fn open_structure(file: &Path) -> Result<BufReader<File>, ExitCode> {
    File::open(file).map(BufReader::new).map_err(|err| {
        eprintln!("error: cannot read `{}`: {err}", file.display());
        match err.kind() {
            std::io::ErrorKind::NotFound => ExitCode::from(2),
            _ => ExitCode::from(1),
        }
    })
}

/// The name of `file`, without its directory.
pub(crate) fn file_name(file: &Path) -> String {
    file.file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

/// The refusal of a path whose extension names no format `cairn <command>` reads.
pub(crate) fn refuse_extension(file: &Path, command: &str) {
    eprintln!(
        "error: `{}` is not a structure file `cairn {command}` reads",
        file.display()
    );
    let legacy = file
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("schematic"));
    if legacy {
        eprintln!(
            "  note: a `.schematic` stores the numeric block ids of the releases before 1.13, \
             which this version does not read; save it as a `.schem` from WorldEdit 7 or later \
             and name that"
        );
    }
    let names: Vec<String> = ImportFormat::ALL
        .iter()
        .map(|format| format!("`.{}`", format.extension()))
        .collect();
    let (last, rest) = names.split_last().expect("an import reads some format");
    eprintln!("  note: name a {} or {last} file", rest.join(", "));
}
