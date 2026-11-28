//! `cairn import`: a structure file written as a `.crn` of raw members, stamped with the target it
//! compiles for (`spec/ecosystem-interop` "Reverse direction: the compiler transliterates, an LLM
//! lifts").

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cairn_lang_core::check::LineStarts;
use cairn_lang_formats::file_identity::{NamedFile, Reach, Unidentified};
use cairn_lang_formats::import::{ImportFormat, import_raw};
use cairn_lang_formats::write_path::WritePath;

use crate::report::report_diagnostics;
use crate::staging::{Kind, StagedSet, blocked_scratch};
use crate::structure_file::{file_name, open_structure, refuse_extension};

pub(crate) fn run_import(file: &Path, out: Option<&Path>) -> ExitCode {
    let Some(format) = ImportFormat::from_path(file) else {
        refuse_extension(file, "import");
        return ExitCode::from(2);
    };
    let flag = out;
    let out = out.map_or_else(|| default_out(file), Path::to_path_buf);
    if let Err(code) = check_file_is_left_alone(file, flag, &out) {
        return code;
    }
    let blocked = blocked_scratch(Kind::Source, &out);
    if !blocked.is_empty() {
        for scratch in &blocked {
            scratch.report("import");
        }
        return ExitCode::from(1);
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

/// Refuse an import that would write its source at `out`, which `--out` names when `flag` is
/// given, through a path reaching `file`, the structure file it reads.
fn check_file_is_left_alone(file: &Path, flag: Option<&Path>, out: &Path) -> Result<(), ExitCode> {
    let structure = match NamedFile::at(file) {
        Ok(Some(structure)) => structure,
        Ok(None) => return Ok(()),
        Err(unidentified) => return Err(report_unidentified(file, out, &unidentified)),
    };
    let reached = match WritePath::reaching(&structure, out) {
        Ok(None) => return Ok(()),
        Ok(Some(reached)) => reached,
        Err(unidentified) => return Err(report_unidentified(file, out, &unidentified)),
    };
    let shown = match flag {
        Some(_) => format!("`--out {}`", out.display()),
        None => format!("source `{}`", out.display()),
    };
    match reached.path {
        WritePath::Final => eprintln!("error: {shown} names the file being imported"),
        WritePath::Scratch(scratch) => eprintln!(
            "error: {shown} would put its scratch file `{}` over the file being imported",
            scratch.of(out).display(),
        ),
    }
    let fix = match reached.by {
        Reach::Name => "give `--out` a `.crn` path",
        Reach::Link => "a link makes them one file; give `--out` a `.crn` path elsewhere",
    };
    let default_free = flag.is_some()
        && WritePath::reaching(&structure, &default_out(file))
            .is_ok_and(|reached| reached.is_none());
    let leave_out = if default_free {
        ", or leave it out to write `<stem>.raw.crn`"
    } else {
        ""
    };
    eprintln!("  note: {fix}{leave_out}");
    Err(ExitCode::from(2))
}

/// Say that the file system cannot tell `unidentified`'s path, one the source at `out` is written
/// through, from `file`, the structure file being imported.
fn report_unidentified(file: &Path, out: &Path, unidentified: &Unidentified) -> ExitCode {
    eprintln!(
        "error: cannot tell whether source `{}` would be written over `{}`, the file being \
         imported: {unidentified}",
        out.display(),
        file.display(),
    );
    eprintln!(
        "  note: check that `{}` can be read, or give `--out` a `.crn` path elsewhere",
        unidentified.path.display(),
    );
    ExitCode::from(1)
}

/// `<stem>.raw.crn` beside `file`.
fn default_out(file: &Path) -> PathBuf {
    let stem = file
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    file.with_file_name(format!("{stem}.raw.crn"))
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
