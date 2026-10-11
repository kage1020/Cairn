//! `cairn lower`: the block-array IR of a source, as an ASCII slice
//! dump, JSON or debug output.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, lower_to_block_array};
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::resolve::resolve;
use cairn_lang_core::{Severity, lower};
use cairn_lang_formats::registry::builtin_java;

use crate::cli::LowerFormat;
use crate::pipeline::{build_diagnostics, load_module, weighed_editions};
use crate::report::{print_json, report_diagnostics, report_failure_document};

pub(crate) fn run_lower(file: &Path, format: LowerFormat) -> ExitCode {
    let (source, module) = match load_module(file, format.failure_report()) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    // No `--target`, so no version to check ids against.
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(&builtin_java().view(None)));
    // As in `load_and_lower`, the check pass gates the output, ahead of the
    // lowering deferrals its findings cascade into.
    block_ir.diagnostics = build_diagnostics(
        &module,
        &ir,
        None,
        &weighed_editions(None),
        std::mem::take(&mut block_ir.diagnostics),
    );

    // Refuse before printing: `> ir.json` creates the file whatever the
    // exit code, so it gets the failure document rather than IR built from
    // a source `cairn check` rejects.
    let has_error = block_ir
        .diagnostics
        .iter()
        .any(|d| d.severity() == Severity::Error);
    if has_error {
        return report_failure_document(
            file,
            &source,
            format.failure_report(),
            &block_ir.diagnostics,
        );
    }

    // Warnings go to stderr as text in every format: folded into the JSON
    // they would change a document tools already read, and among the
    // voxels a pipe would swallow them.
    let lines = LineStarts::new(&source);
    report_diagnostics(file, &source, &lines, &block_ir.diagnostics);

    match format {
        LowerFormat::Ascii => {
            print_block_ir_ascii(&block_ir);
            ExitCode::SUCCESS
        }
        LowerFormat::Json => match print_json("block-array IR", &block_ir) {
            Ok(()) => ExitCode::SUCCESS,
            Err(code) => code,
        },
        LowerFormat::Debug => {
            outln!("{block_ir:#?}");
            ExitCode::SUCCESS
        }
    }
}

fn print_block_ir_ascii(block_ir: &BlockArrayIr) {
    if block_ir.structures.is_empty() {
        outln!("(no structures lowered)");
        return;
    }
    for (key, array) in &block_ir.structures {
        outln!(
            "{key}  dims={}x{}x{}",
            array.dims.x,
            array.dims.y,
            array.dims.z
        );
        outln!("  palette:");
        for (i, state) in array.palette.entries.iter().enumerate() {
            let glyph = ascii_glyph(i);
            if state.properties.is_empty() {
                outln!("    [{i:>3}] {glyph}  {}", state.id);
            } else {
                outln!(
                    "    [{i:>3}] {glyph}  {}[{}]",
                    state.id,
                    state.properties_text()
                );
            }
        }
        for y in 0..array.dims.y {
            outln!("  y={y}");
            print_y_slice(array, y);
        }
    }
}

const ASCII_ALPHABET: &[u8] = b"#abcdefghijklmnopqrstuvwxyz0123456789";

/// Glyph for a palette index in an ASCII slice: `.` for air at index 0,
/// then `#`, letters and digits. Every entry past index 36 renders as `?`,
/// so a `?`-heavy slice means a large palette, not broken lowering.
fn ascii_glyph(palette_index: usize) -> char {
    if palette_index == 0 {
        return '.';
    }
    ASCII_ALPHABET
        .get(palette_index - 1)
        .copied()
        .map_or('?', char::from)
}

fn print_y_slice(array: &BlockArray, y: u32) {
    for z in 0..array.dims.z {
        let mut row = String::with_capacity(array.dims.x as usize);
        for x in 0..array.dims.x {
            let i = array.dims.index(x, y, z).expect("in-range coordinate");
            row.push(ascii_glyph(usize::from(array.voxels[i].0)));
        }
        outln!("    {row}");
    }
}
