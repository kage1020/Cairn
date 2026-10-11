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
    // `lower` takes no `--target` at all, so there is no version to
    // check ids against; see the note in `run_info`.
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(&builtin_java().view(None)));
    // Mirror `load_and_lower`: the check pass gates the pipeline, and the
    // lowering deferrals its findings tend to cascade into follow on the
    // same stream.
    block_ir.diagnostics = build_diagnostics(
        &module,
        &ir,
        None,
        &weighed_editions(None),
        std::mem::take(&mut block_ir.diagnostics),
    );

    // Refuse before printing, the way `run_info` and `run_compile` do. The
    // exit code alone does not protect a redirect: `cairn lower f.crn
    // --format json > ir.json` creates the file before the process ends, so
    // emitting the IR anyway hands a pipeline a well-formed artifact built
    // from a source `cairn check` rejects. What goes out instead is the
    // failure document, so the redirect holds a JSON file that says why.
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

    // Warnings on a run that still has a product keep going to stderr as
    // text, whichever format the dump takes. For `json` the reason is
    // `run_info`'s: folding them into the dump would change a document
    // downstream tooling already reads. For `ascii` and `debug` there is
    // no document to fold them into, and stdout is the product either
    // way — a finding written among the voxels is a finding a pipe
    // swallows.
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

/// Glyph for a palette index in ASCII slice output: air → `.`, anything
/// else → `#` for the first non-air, then digits/letters so a slice with
/// many distinct materials still reads. Any palette entry past index 36
/// renders as `?` — debug-format only, and well above the per-structure
/// palette size the current examples use (cottage uses 3 entries), but
/// worth a glance before reading a `?`-heavy slice as evidence of broken
/// lowering.
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
