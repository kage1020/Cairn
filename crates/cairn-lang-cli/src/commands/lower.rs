//! `cairn lower`: the block-array IR of a source, as an ASCII slice dump, JSON or debug output.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::block_array::{
    BlockArray, BlockArrayIr, findings_under_guards, lower_to_block_array,
};
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::intent::{IntentModule, circuit_lines};
use cairn_lang_core::resolve::resolve;
use cairn_lang_core::{Edition, Severity, lower};
use cairn_lang_formats::build::circuits::lay_circuits;
use cairn_lang_formats::build::finding::{Finding, Found};
use cairn_lang_formats::build::gate::{build_diagnostics, weighed_editions};
use cairn_lang_formats::registry::builtin_pack;

use crate::cli::LowerFormat;
use crate::pipeline::load_module;
use crate::report::{print_json, report_diagnostics, report_failure_document};

pub(crate) fn run_lower(file: &Path, edition: Option<Edition>, format: LowerFormat) -> ExitCode {
    let (source, module) = match load_module(file, format.failure_report()) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    let ir = lower(&module);
    let resolution = resolve(&ir, edition);
    // No `--target`, so no version to check ids against.
    let registry = builtin_pack(edition.unwrap_or(Edition::Java)).view(None);
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(&registry));
    // With no edition pinned the guarded lines are left out, but an error on one still fails the
    // command.
    if edition.is_none() {
        let declared = |edition, id: &str| builtin_pack(edition).may_declare(id);
        block_ir
            .diagnostics
            .extend(findings_under_guards(&ir, &declared));
    }
    // Check findings lead, ahead of the lowering deferrals they cascade into.
    block_ir.diagnostics = build_diagnostics(
        &module,
        &ir,
        edition,
        &weighed_editions(edition),
        std::mem::take(&mut block_ir.diagnostics),
    );
    let circuits = edition.map_or_else(Vec::new, |edition| {
        lay_circuits(&ir, &mut block_ir, edition, &registry)
    });
    let findings: Vec<Found> = block_ir
        .diagnostics
        .iter()
        .cloned()
        .map(Found::Core)
        .chain(circuits.into_iter().map(Found::Circuit))
        .collect();

    // Refuse before printing: `> ir.json` gets the failure document, not IR of a rejected source.
    if findings.iter().any(|d| d.severity() == Severity::Error) {
        return report_failure_document(file, &source, format.failure_report(), &findings);
    }

    // Warnings go to stderr as text in every format.
    let lines = LineStarts::new(&source);
    report_diagnostics(file, &source, &lines, &findings);
    if edition.is_none() {
        report_left_out(&ir);
    }

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

/// The notes saying what a lowering with no edition pinned leaves out.
fn report_left_out(ir: &IntentModule) {
    let guarded = ir.guarded_members().len();
    if guarded > 0 {
        eprintln!(
            "note: no edition is pinned here, so the {guarded} {} under `@edition` guards {} \
             left out; `cairn lower --edition E` lowers the ones E's guards hold",
            if guarded == 1 { "member" } else { "members" },
            if guarded == 1 { "is" } else { "are" },
        );
    }
    let circuits = circuit_lines(ir).iter().filter(|line| line.is_ok()).count();
    if circuits > 0 {
        eprintln!(
            "note: no edition is pinned here, so the {} left out; `cairn lower --edition E` lays \
             the ones E builds",
            if circuits == 1 {
                "circuit of the scope with a `circuit` line is".to_owned()
            } else {
                format!("circuits of the {circuits} scopes with a `circuit` line are")
            },
        );
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

/// `.` for air at index 0, then `#`, letters and digits; every entry past index 36 is `?`.
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
