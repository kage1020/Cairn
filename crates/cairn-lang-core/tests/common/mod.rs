//! Helpers shared by the `cairn-lang-core` integration test binaries.
//!
//! Every binary here drives the same front half of the pipeline — parse,
//! lower, then either `check` or `resolve` + `lower_to_block_array` — and
//! reads the same `examples/` tree. The plumbing lives once so a change to
//! a pass's signature, or to where the examples are found, lands in one
//! place.

// Each test binary uses its own subset of these.
#![allow(dead_code)]

use std::path::PathBuf;

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, lower_to_block_array};
use cairn_lang_core::{Diagnostic, check, lower, parse, resolve};

/// The repository's `examples/` directory.
pub fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

/// The source of `examples/<name>`.
pub fn read_example(name: &str) -> String {
    let path = examples_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Every `.crn` under `examples/`, as `(file name, source)`, sorted by name.
///
/// Refuses to return a set too small to be the shipped one: every sweep is
/// a loop over this, and a loop over nothing passes.
pub fn examples() -> Vec<(String, String)> {
    let dir = examples_dir();
    let entries =
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    let mut found: Vec<(String, String)> = entries
        .map(|entry| {
            entry
                .unwrap_or_else(|e| panic!("cannot read an entry of {}: {e}", dir.display()))
                .path()
        })
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("crn"))
        .map(|path| {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            (
                path.file_name().expect("named").to_string_lossy().into(),
                source,
            )
        })
        .collect();
    found.sort();
    assert!(
        found.len() >= 5,
        "found only {} examples under {}, which is not the shipped set",
        found.len(),
        dir.display(),
    );
    found
}

/// Parse, lower and run the check passes over `source` with no registry
/// pack. A parse failure panics with the source so the fixture is in the
/// failure message.
pub fn diagnose(source: &str) -> Vec<Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    check(&module, &ir, None)
}

/// The codes of every finding `source` raises, in report order.
pub fn codes(source: &str) -> Vec<&'static str> {
    diagnose(source).iter().map(|d| d.code.as_str()).collect()
}

/// The text `diag` points at, so a test can assert what a finding
/// underlines rather than only where.
pub fn slice<'a>(source: &'a str, diag: &Diagnostic) -> &'a str {
    &source[diag.span.clone()]
}

/// The messages of `diag`'s notes, in order.
pub fn notes(diag: &Diagnostic) -> Vec<&str> {
    diag.notes.iter().map(|n| n.message.as_str()).collect()
}

/// The single finding in `found`, or a panic listing what was there.
pub fn exactly_one(mut found: Vec<Diagnostic>) -> Diagnostic {
    assert_eq!(found.len(), 1, "expected one finding, got {found:#?}");
    found.remove(0)
}

/// Parse, lower, resolve and lower `source` to the block-array IR with no
/// registry pack. Only the block-array pass's own diagnostics are kept.
pub fn lowered(source: &str) -> BlockArrayIr {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, None)
}

/// [`lowered`], with the resolver's diagnostics merged in front of the
/// block-array pass's the way the CLI reports them.
pub fn lowered_with_resolver_diagnostics(source: &str) -> BlockArrayIr {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    let mut out = lower_to_block_array(&ir, &resolution, None);
    let mut combined = resolution.diagnostics;
    combined.append(&mut out.diagnostics);
    out.diagnostics = combined;
    out
}

/// The single structure `ir` holds, for sources that declare exactly one.
pub fn only_structure(ir: &BlockArrayIr) -> &BlockArray {
    assert_eq!(
        ir.structures.len(),
        1,
        "these sources declare exactly one struct",
    );
    ir.structures.values().next().expect("one structure")
}

/// A theme and a `def` every site fixture can place: `hut` has a floor, a
/// wall and a front door, so a placement of it is complete on its own.
pub const PRELUDE: &str = "theme plain:\n  \
slot floor -> @oak_planks\n  \
slot wall  -> @cobblestone\n\n\
def hut size=3x3:\n  \
floor id=floor mat_slot=floor\n  \
walls id=walls class=outer mat_slot=wall height=3\n  \
door  id=entry side=front at=center\n\n";

/// [`PRELUDE`] plus a struct whose body is a floor and then `row`.
pub fn struct_with(row: &str) -> String {
    format!("{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  {row}\n")
}

/// [`PRELUDE`] plus a `def` whose body is a floor and then `row`.
///
/// The passes walk `ir.structs` and `ir.defs` in separate loops, so a
/// struct-only suite leaves the `def` loop unexecuted — and a `def` is the
/// half users reach first, since a `site` is what instantiates one.
pub fn def_with(row: &str) -> String {
    format!("{PRELUDE}def lodge size=5x5:\n  floor mat_slot=floor\n  {row}\n")
}
