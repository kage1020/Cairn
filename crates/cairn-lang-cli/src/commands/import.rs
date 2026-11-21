//! `cairn import`: a structure file written as a `.crn` of raw members, stamped with the target it
//! compiles for (`spec/ecosystem-interop` "Reverse direction: the compiler transliterates, an LLM
//! lifts").

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cairn_lang_core::check::LineStarts;
use cairn_lang_formats::import::{ImportFormat, import_raw};

use crate::report::report_diagnostics;
use crate::staging::{Kind, StagedSet};
use crate::structure_file::{file_name, open_structure, refuse_extension};

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
        eprintln!("error: {err}");
        return ExitCode::from(1);
    }
    for loss in &import.decoded.losses {
        eprintln!("warning[W_SEMANTIC_LOSS]: {loss}");
    }
    let lines = LineStarts::new(source);
    report_diagnostics(&out, source, &lines, &import.transliteration.warnings);
    let stamp = &import.decoded.stamp;
    let (edition, version) = (stamp.edition(), stamp.mc_version());
    eprintln!(
        "note: stamped {edition} {version}: {}; `cairn compile {} --edition {edition} --target \
         {version}` builds it",
        stamp.reason(),
        out.display(),
    );
    outln!("wrote {}", out.display());
    ExitCode::SUCCESS
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

/// Write `source` to `out` whole or not at all, creating the directory it is in.
fn write(out: &Path, source: &str) -> Result<(), String> {
    let failed = |err: std::io::Error| format!("writing source `{}`: {err}", out.display());
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }
    let mut staged = StagedSet::default();
    staged
        .stage(out, Kind::Source, |file| file.write_all(source.as_bytes()))
        .map_err(failed)?;
    staged
        .commit()
        .map(drop)
        .map_err(|failure| failure.to_string())
}
