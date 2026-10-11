//! Helpers shared by the core integration test binaries, which all drive the
//! same front half of the pipeline and read the same `examples/` tree. Each
//! binary declares this module `pub`, so a helper one binary leaves unused is
//! not dead code there.

use std::path::PathBuf;

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, BlockState, lower_to_block_array};
use cairn_lang_core::{Diagnostic, DiagnosticCode, check, lower, parse, resolve};

/// The number of `.crn` files `examples/` ships. Bump it with the next
/// example; every sweep refuses a different count, and a loop over a
/// filtered-down set passes silently.
pub const SHIPPED_EXAMPLES: usize = 12;

/// The repository's `examples/` directory.
pub fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

/// The source of `examples/<name>`.
///
/// # Panics
///
/// When the file cannot be read.
pub fn read_example(name: &str) -> String {
    let path = examples_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Every `.crn` under `examples/`, as `(file name, source)`, sorted by name.
///
/// # Panics
///
/// When `examples/` cannot be read, or holds other than
/// [`SHIPPED_EXAMPLES`] files: every sweep is a loop over this, and a loop
/// over nothing passes.
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
    assert_eq!(
        found.len(),
        SHIPPED_EXAMPLES,
        "found {} examples under {}, not the {SHIPPED_EXAMPLES} shipped; bump SHIPPED_EXAMPLES \
         when adding one",
        found.len(),
        dir.display(),
    );
    found
}

/// Parse, lower and run the check passes over `source` with no registry
/// pack.
///
/// # Panics
///
/// When `source` does not parse, with the source in the message so the
/// fixture is in the failure.
pub fn diagnose(source: &str) -> Vec<Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    check(&module, &ir, None)
}

/// The findings `source` raises with `code`, in report order.
///
/// # Panics
///
/// When `source` does not parse.
pub fn of_code(source: &str, code: DiagnosticCode) -> Vec<Diagnostic> {
    diagnose(source)
        .into_iter()
        .filter(|d| d.code == code)
        .collect()
}

/// The single finding `source` raises with `code`.
///
/// # Panics
///
/// When `source` does not parse, or raises other than one such finding.
pub fn one_of_code(source: &str, code: DiagnosticCode) -> Diagnostic {
    exactly_one(of_code(source, code))
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

/// Byte offset of the `n`th (0-based) occurrence of `needle`, so a test
/// says which of several equal tokens a span starts at rather than relying
/// on "first" or "last".
///
/// # Panics
///
/// When `source` holds fewer than `n + 1` occurrences.
pub fn nth(source: &str, needle: &str, n: usize) -> usize {
    source
        .match_indices(needle)
        .nth(n)
        .unwrap_or_else(|| panic!("occurrence {n} of `{needle}` not found in:\n{source}"))
        .0
}

/// The primaries of `found`, so a failing count shows what was reported
/// rather than only how much of it there was.
pub fn primaries(found: &[Diagnostic]) -> Vec<&str> {
    found.iter().map(|d| d.primary.as_str()).collect()
}

/// The messages of `diag`'s notes, in order.
pub fn notes(diag: &Diagnostic) -> Vec<&str> {
    diag.notes.iter().map(|n| n.message.as_str()).collect()
}

/// The single finding in `found`.
///
/// # Panics
///
/// When there is not exactly one, listing what was there.
pub fn exactly_one(mut found: Vec<Diagnostic>) -> Diagnostic {
    assert_eq!(found.len(), 1, "expected one finding, got {found:#?}");
    found.remove(0)
}

/// Parse, lower, resolve and lower `source` to the block-array IR with no
/// registry pack. Only the block-array pass's own diagnostics are kept.
///
/// # Panics
///
/// When `source` does not parse, with the source in the message.
pub fn lowered(source: &str) -> BlockArrayIr {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, None)
}

/// [`lowered`], with the resolver's diagnostics merged in front of the
/// block-array pass's the way the CLI reports them.
///
/// # Panics
///
/// When `source` does not parse, with the source in the message.
pub fn lowered_with_resolver_diagnostics(source: &str) -> BlockArrayIr {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    let mut out = lower_to_block_array(&ir, &resolution, None);
    let mut combined = resolution.diagnostics;
    combined.append(&mut out.diagnostics);
    out.diagnostics = combined;
    out
}

/// The primaries of the `W_DEFERRED_MEMBER` findings `ir` carries, in
/// report order.
pub fn defers(ir: &BlockArrayIr) -> Vec<&str> {
    ir.diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::DeferredMember)
        .map(|d| d.primary.as_str())
        .collect()
}

/// Whether `source` lays its walkway rather than skipping it for a port that
/// could not be placed.
///
/// # Panics
///
/// When `source` does not parse, or when a walkway that was not skipped was
/// not laid either.
pub fn walkway_was_laid(source: &str) -> bool {
    let out = lowered(source);
    let skipped = defers(&out)
        .iter()
        .any(|primary| primary.contains("was skipped because port"));
    assert_eq!(
        skipped,
        out.walkways.is_empty(),
        "a walkway that was not skipped has to have been laid",
    );
    !skipped
}

/// The cells of `array` that are not air.
pub fn solid_cells(array: &BlockArray) -> usize {
    array.voxels.iter().filter(|c| c.0 != 0).count()
}

/// The block state at `(x, y, z)` of `array`.
///
/// # Panics
///
/// When the cell is outside `array`.
pub fn state_at(array: &BlockArray, x: u32, y: u32, z: u32) -> &BlockState {
    let i = array
        .dims
        .index(x, y, z)
        .unwrap_or_else(|| panic!("({x}, {y}, {z}) is outside {:?}", array.dims));
    &array.palette.entries[array.voxels[i].0 as usize]
}

/// The single structure `ir` holds, for sources that declare exactly one.
///
/// # Panics
///
/// When `ir` holds more or fewer.
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

/// [`PRELUDE`] plus a site placing two `hut`s, `anchor` at the origin and
/// `peer` east of it, and then `row`, so a site row can name both.
pub fn duo_with(row: &str) -> String {
    format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n  {row}\n"
    )
}

/// `header` above a one-struct body, so the header lines are the only
/// variable in a fixture about them.
pub fn with_header(header: &str) -> String {
    format!("{header}struct s size=2x2\n  floor mat_slot=f\n")
}

/// [`PRELUDE`] plus a `def` whose body is a floor and then `row`.
///
/// The passes walk `ir.structs` and `ir.defs` in separate loops, so a
/// struct-only suite leaves the `def` loop unexecuted — and a `def` is the
/// half users reach first, since a `site` is what instantiates one.
pub fn def_with(row: &str) -> String {
    format!("{PRELUDE}def lodge size=5x5:\n  floor mat_slot=floor\n  {row}\n")
}
