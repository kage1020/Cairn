//! `cairn import`: a structure file written as a `.crn` of raw members, stamped with the target it
//! compiles for (`spec/ecosystem-interop` "Reverse direction: the compiler transliterates, an LLM
//! lifts").

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cairn_lang_core::check::LineStarts;
use cairn_lang_formats::import::{ImportFormat, import_raw};

use crate::report::report_diagnostics;

pub(crate) fn run_import(file: &Path, out: Option<&Path>) -> ExitCode {
    let Some(format) = ImportFormat::from_path(file) else {
        refuse_extension(file, "import");
        return ExitCode::from(2);
    };
    let out = out.map_or_else(|| default_out(file), Path::to_path_buf);
    if names_one_file(file, &out) {
        eprintln!(
            "error: `--out {}` names the file being imported",
            out.display()
        );
        eprintln!("  note: give `--out` a `.crn` path, or leave it out to write `<stem>.raw.crn`");
        return ExitCode::from(2);
    }
    let reader = match open_structure(file) {
        Ok(reader) => reader,
        Err(code) => return code,
    };
    let name = file_name(file);
    let import = match import_raw(reader, format, &name) {
        Ok(import) => import,
        Err(err) => {
            eprintln!("error: cannot import `{}`: {err}", file.display());
            return ExitCode::from(1);
        }
    };
    let source = &import.transliteration.source;
    if let Err(err) = write(&out, source) {
        eprintln!("error: writing `{}`: {err}", out.display());
        return ExitCode::from(1);
    }
    for loss in &import.decoded.losses {
        eprintln!("warning[W_SEMANTIC_LOSS]: {loss}");
    }
    let lines = LineStarts::new(source);
    report_diagnostics(&out, source, &lines, &import.transliteration.warnings);
    let stamp = &import.decoded.stamp;
    eprintln!(
        "note: stamped {} {}: {}; `cairn compile {} --edition {} --target {}` builds it",
        stamp.edition,
        stamp.mc_version,
        stamp.reason(),
        out.display(),
        stamp.edition,
        stamp.mc_version,
    );
    outln!("wrote {}", out.display());
    ExitCode::SUCCESS
}

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
    eprintln!("  note: name a {} file", ImportFormat::choices());
}

/// `<stem>.raw.crn` beside `file`.
fn default_out(file: &Path) -> PathBuf {
    let stem = file
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    file.with_file_name(format!("{stem}.raw.crn"))
}

/// Whether `file` and `out` are one file on disk.
fn names_one_file(file: &Path, out: &Path) -> bool {
    match (std::fs::canonicalize(file), std::fs::canonicalize(out)) {
        (Ok(file), Ok(out)) => file == out,
        _ => false,
    }
}

/// Write `source` to `out`, creating the directory it is in.
fn write(out: &Path, source: &str) -> std::io::Result<()> {
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, source)
}
