//! Integration tests for `cairn_lang_redstone::compile_delay`.
//!
//! Locks the observable behaviours of the delay-insertion slice
//! (`spec/redstone` §14.5, stage 3 of place-and-route): the
//! `examples/redstone-door.crn` happy path (per edition),
//! multi-cell cascade `delay_ticks` attribution pinned to base delay
//! plus implicit buffer repeaters, `E_ATTENUATION_LIMIT` when a driver
//! segment exceeds the v1 sanity cap, pass-through of scopes elided by
//! upstream stages, the JSON wire form growing a `delay_ticks` field,
//! byte-identical wire form vs the routed IR (delay is a field-write
//! on `PlacedCellNode::delay_ticks`), and per-scope independence when
//! a module carries more than one scope.

use std::path::PathBuf;

use cairn_lang_core::Edition;
use cairn_lang_core::check::Severity;
use cairn_lang_core::{lower, parse};
use cairn_lang_redstone::{
    DiagnosticCode, MAX_ATTENUATION_SEGMENT, ScopedPlacementIr, compile_delay,
    compile_edition_netlist, compile_netlist, compile_placement, compile_routing, synthesize,
};

fn load_example(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn routed_from_source(source: &str, edition: Edition) -> ScopedPlacementIr {
    let module = parse(source).expect("parse");
    let intent = lower(&module);
    let synth = synthesize(&intent);
    assert!(
        synth
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "fixture must synth cleanly: {:?}",
        synth.diagnostics,
    );
    let netlist = compile_netlist(&synth.scoped);
    let edition_netlist = compile_edition_netlist(&netlist, edition);
    let placement = compile_placement(&edition_netlist, &intent);
    assert!(
        placement.diagnostics.is_empty(),
        "fixture must place cleanly (delay tests are downstream of placement): {:?}",
        placement.diagnostics,
    );
    let routing = compile_routing(&placement.scoped);
    assert!(
        routing.diagnostics.is_empty(),
        "fixture must route cleanly (delay tests are downstream of routing): {:?}",
        routing.diagnostics,
    );
    routing.scoped
}

/// AC1 — `examples/redstone-door.crn` compiled for Java: the sole
/// `JavaRepeaterOr` cell picks up `delay_ticks = Some(1)` — base 1 tick
/// from the repeater realisation, zero implicit buffer repeaters
/// because both driver segments (1 and 2 blocks) sit under the
/// dust-attenuation limit of 15. `wire_length` is preserved verbatim
/// from routing.
#[test]
fn redstone_door_java_delay_ticks_equal_base_repeater_delay() {
    let source = load_example("redstone-door.crn");
    let routed = routed_from_source(&source, Edition::Java);
    let delayed = compile_delay(&routed);
    assert!(
        delayed.diagnostics.is_empty(),
        "clean example must not raise delay diagnostics: {:?}",
        delayed.diagnostics,
    );

    let entry = delayed
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "gatehouse")
        .expect("gatehouse scope");
    let cell = entry
        .ir
        .cells
        .first()
        .expect("gatehouse must have a placed cell");
    assert_eq!(
        cell.delay_ticks(),
        Some(1),
        "JavaRepeaterOr base = 1 tick, both driver segments ≤ 15 → 0 buffers",
    );
    assert_eq!(
        cell.wire_length(),
        Some(3),
        "routing's wire_length must survive the delay pass verbatim",
    );
}

/// AC2 — the same example compiled for Bedrock: the sole
/// `BedrockTorchOr` cell picks up `delay_ticks = Some(0)` — dust-merge
/// realisation carries no cell tick, and both driver segments (1 and
/// 2 blocks) are under the attenuation limit so no buffer ticks
/// either. `wire_length` matches Java by the edition-agnostic routing
/// invariant.
#[test]
fn redstone_door_bedrock_delay_ticks_are_zero() {
    let source = load_example("redstone-door.crn");
    let routed = routed_from_source(&source, Edition::Bedrock);
    let delayed = compile_delay(&routed);
    assert!(
        delayed.diagnostics.is_empty(),
        "clean example must not raise delay diagnostics: {:?}",
        delayed.diagnostics,
    );

    let entry = delayed
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "gatehouse")
        .expect("gatehouse scope");
    let cell = entry
        .ir
        .cells
        .first()
        .expect("gatehouse must have a placed cell");
    assert_eq!(
        cell.delay_ticks(),
        Some(0),
        "BedrockTorchOr is a bare dust merge → 0 cell tick + 0 buffers",
    );
    assert_eq!(
        cell.wire_length(),
        Some(3),
        "wire_length is edition-independent by construction",
    );
}

/// AC3 — a scope whose logic produces three cascaded cells fills
/// every cell's `delay_ticks` with a pinned tick sum. Same 3-cell
/// fixture the routing suite pins: `sig.and_ab = sig.a and sig.b`,
/// `sig.or_ab = sig.a or sig.b`, `sig.combined = sig.and_ab and
/// sig.or_ab`. Cell coords are `x = 0, 1, 2` per the placement pass,
/// input pads sit at `(0, 0, 1)` and `(0, 0, 2)`, so every driver
/// segment is ≤ 5 blocks and no cell needs an implicit buffer. Each
/// cell's `delay_ticks` therefore equals its base tick count alone.
#[test]
fn multi_cell_scope_pins_delay_ticks_from_base_only() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct sim size=7x5
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b

  logic sig.and_ab   = sig.a and sig.b
  logic sig.or_ab    = sig.a or sig.b
  logic sig.combined = sig.and_ab and sig.or_ab

  door id=d side=front at=center mat_slot=wall opened_by=sig.combined

  circuit region=floor void=3
";
    let routed = routed_from_source(source, Edition::Java);
    let delayed = compile_delay(&routed);
    assert!(
        delayed.diagnostics.is_empty(),
        "clean fixture must not raise delay diagnostics: {:?}",
        delayed.diagnostics,
    );

    let entry = delayed
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "sim")
        .expect("sim scope");
    assert_eq!(entry.ir.cells.len(), 3);
    // JavaComparatorAnd: base 1, segments [1, 2] → 0 buffers.
    assert_eq!(entry.ir.cells[0].delay_ticks(), Some(1));
    // JavaRepeaterOr: base 1, segments [2, 3] → 0 buffers.
    assert_eq!(entry.ir.cells[1].delay_ticks(), Some(1));
    // JavaComparatorAnd: base 1, cell-to-cell segments [2, 1] → 0 buffers.
    assert_eq!(entry.ir.cells[2].delay_ticks(), Some(1));
    // `wire_length` from routing must be preserved verbatim on every
    // cell (locked separately by the byte-identical JSON regression
    // below, but pinned per-cell here so a divergent field write in
    // `attribute_delay_ticks` trips this test rather than the JSON
    // one).
    assert_eq!(entry.ir.cells[0].wire_length(), Some(3));
    assert_eq!(entry.ir.cells[1].wire_length(), Some(5));
    assert_eq!(entry.ir.cells[2].wire_length(), Some(3));
}

/// AC4 — a scope whose routed output-pad segment exceeds
/// [`MAX_ATTENUATION_SEGMENT`] fires `E_ATTENUATION_LIMIT`, elides
/// the failed scope, and never writes a partial `delay_ticks` set.
/// The fixture uses a very wide region (width > 256) so the sole
/// cell's output driver spans the full `x` axis to the right-edge
/// output pad.
#[test]
fn attenuation_limit_fires_and_elides_scope() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct wide_pack size=300x5
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b

  logic sig.out = sig.a or sig.b

  door id=d side=front at=center mat_slot=wall opened_by=sig.out

  circuit region=floor void=3
";
    let routed = routed_from_source(source, Edition::Java);
    let delayed = compile_delay(&routed);

    let attenuation: Vec<_> = delayed
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::AttenuationLimit)
        .collect();
    assert_eq!(
        attenuation.len(),
        1,
        "expected exactly one E_ATTENUATION_LIMIT, got {:?}",
        delayed.diagnostics,
    );
    let d = attenuation[0];
    assert_eq!(d.severity, Severity::Error);
    assert!(
        d.primary.starts_with("routed netlist for "),
        "primary should mark the delay-side origin, got {:?}",
        d.primary,
    );
    assert!(
        d.primary.contains("struct `wide_pack`"),
        "primary should name the failed scope, got {:?}",
        d.primary,
    );
    assert!(
        d.primary.contains(&format!(
            "v1 attenuation limit of {MAX_ATTENUATION_SEGMENT}"
        )),
        "primary should quote the sanity cap, got {:?}",
        d.primary,
    );
    let source_at_span = &source[d.span.clone()];
    assert!(
        source_at_span.starts_with("circuit region="),
        "attenuation span must anchor to the `circuit region=` line, got {source_at_span:?}",
    );
    let footer = d
        .notes
        .iter()
        .find(|n| n.span.is_none())
        .expect("attenuation has a fix footer");
    for phrase in ["enlarge", "region", "split", "pin"] {
        assert!(
            footer.message.contains(phrase),
            "footer should carry the self-correction triple (missing {phrase:?}), got {:?}",
            footer.message,
        );
    }
    assert!(
        delayed.scoped.scopes.iter().all(|e| e.name != "wide_pack"),
        "failed scope must be elided from the delay output",
    );
}

/// AC5 — a scope with a cascaded chain long enough to push at least
/// one cell's shared driver segment past
/// [`DUST_ATTENUATION_LIMIT`] but not past
/// [`MAX_ATTENUATION_SEGMENT`] gets `delay_ticks` bumped by the
/// implicit-buffer contribution. Every cell in the chain shares
/// `sig.b` as one of its drivers, so `cell[i]` (placed at
/// `x = i, y = 0, z = 0`) sees a `sig.b` segment of
/// `manhattan((0, 0, 2), (i, 0, 0)) = i + 2` blocks; the previous cell
/// contributes a segment of 1 block. `cell[i]`'s implicit buffer
/// count is `(segment - 1) / DUST_ATTENUATION_LIMIT` summed across
/// drivers.
#[test]
fn cascaded_and_chain_records_implicit_buffer_ticks() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct chain size=20x5
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b

  logic sig.c0  = sig.a  and sig.b
  logic sig.c1  = sig.c0 and sig.b
  logic sig.c2  = sig.c1 and sig.b
  logic sig.c3  = sig.c2 and sig.b
  logic sig.c4  = sig.c3 and sig.b
  logic sig.c5  = sig.c4 and sig.b
  logic sig.c6  = sig.c5 and sig.b
  logic sig.c7  = sig.c6 and sig.b
  logic sig.c8  = sig.c7 and sig.b
  logic sig.c9  = sig.c8 and sig.b
  logic sig.c10 = sig.c9  and sig.b
  logic sig.c11 = sig.c10 and sig.b
  logic sig.c12 = sig.c11 and sig.b
  logic sig.c13 = sig.c12 and sig.b
  logic sig.c14 = sig.c13 and sig.b
  logic sig.c15 = sig.c14 and sig.b

  door id=d side=front at=center mat_slot=wall opened_by=sig.c15

  circuit region=floor void=4
";
    let routed = routed_from_source(source, Edition::Java);
    let delayed = compile_delay(&routed);
    assert!(
        delayed.diagnostics.is_empty(),
        "chain must not raise delay diagnostics: {:?}",
        delayed.diagnostics,
    );

    let entry = delayed
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "chain")
        .expect("chain scope");
    // cell[0] .. cell[15] map to `sig.c0` .. `sig.c15`, all
    // `JavaComparatorAnd` with base_delay = 1. Each cell has two
    // drivers: the previous cell (segment 1 block, no buffer) and
    // shared `sig.b` (segment i + 2 blocks — the input pad at
    // `(0, 0, 2)` reaches `(i, 0, 0)`). For i ≤ 13 the long segment
    // ≤ 15 so no buffer ticks accrue and `delay_ticks = 1`; at i = 14
    // it crosses the attenuation limit and `(16 - 1) / 15 = 1` buffer
    // is added, so `delay_ticks = 2` from that cell onward. Values
    // are hardcoded per index (not derived from the formula the
    // implementation itself uses) so a self-referential off-by-one in
    // `buffer_repeater_ticks_for_segment` cannot slide past the test.
    assert_eq!(entry.ir.cells.len(), 16);
    let expected: [u32; 16] = [
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // i = 0..=13, segment ≤ 15
        2, 2, // i = 14 (segment 16), i = 15 (segment 17)
    ];
    for (i, cell) in entry.ir.cells.iter().enumerate() {
        assert_eq!(
            cell.delay_ticks(),
            Some(expected[i]),
            "cell[{i}]: expected delay_ticks = {}",
            expected[i],
        );
    }
    // The chain must exercise both bands of the delay model — base
    // alone (i ≤ 13) and base + implicit buffer (i ≥ 14) — otherwise
    // shrinking the fixture would silently lose the buffer path.
    assert!(entry.ir.cells.iter().any(|c| c.delay_ticks() == Some(1)));
    assert!(entry.ir.cells.iter().any(|c| c.delay_ticks() == Some(2)));
}

/// `AC5b` — `sig.and_ab = sig.a and sig.b` on Bedrock lowers to
/// `BedrockTorchAnd` whose `base_delay_ticks() = 2` (two-torch
/// NAND→NAND stacked in series). Pins the only pinned base delay
/// that is neither 0 nor 1 — without this fixture a typo swapping
/// `BedrockTorchAnd` to `1` or `3` in the base-delay table would slip
/// past every other AC (which only cover the 0-tick `BedrockTorchOr`,
/// the 1-tick pinned Java/Bedrock cells, or the 0-buffer `and` cell
/// on Java). Both driver segments (1 and 2 blocks) sit under the
/// attenuation limit so `delay_ticks = base_delay + 0 = 2`.
#[test]
fn bedrock_torch_and_delay_ticks_pin_two_tick_base() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct band size=5x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b
  logic sig.out = sig.a and sig.b
  door id=d side=front at=center mat_slot=wall opened_by=sig.out
  circuit region=floor void=2
";
    let routed = routed_from_source(source, Edition::Bedrock);
    let delayed = compile_delay(&routed);
    assert!(delayed.diagnostics.is_empty(), "{:?}", delayed.diagnostics);
    let entry = delayed
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "band")
        .expect("band scope");
    assert_eq!(entry.ir.cells.len(), 1);
    assert_eq!(entry.ir.cells[0].cell.edition(), Edition::Bedrock);
    assert_eq!(
        entry.ir.cells[0].delay_ticks(),
        Some(2),
        "BedrockTorchAnd base_delay must be 2 ticks",
    );
}

/// `AC5c` — output-pad Manhattan segment of exactly
/// `MAX_ATTENUATION_SEGMENT` blocks passes cleanly; one more block
/// fails. Pins the `>` vs `>=` boundary in `delay_scope`'s sanity
/// check so a future rewrite cannot silently slide the cap by one.
///
/// With `size=256x5` the output pad lands at `(255, 0, 1)` and the
/// sole cell at `(0, 0, 0)`, so the output segment is exactly 256
/// blocks — matching the cap. `size=257x5` bumps the pad to
/// `(256, 0, 1)` and the segment to 257, one over the cap.
#[test]
fn max_attenuation_segment_boundary_at_256_is_inclusive() {
    let at_cap_source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct band size=256x5\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b\n  \
        logic sig.out = sig.a or sig.b\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.out\n  \
        circuit region=floor void=3\n";
    let routed = routed_from_source(at_cap_source, Edition::Java);
    let delayed = compile_delay(&routed);
    assert!(
        delayed.diagnostics.is_empty(),
        "segment == MAX_ATTENUATION_SEGMENT must pass: {:?}",
        delayed.diagnostics,
    );
    let entry = delayed
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "band")
        .expect("band scope");
    assert!(entry.ir.cells[0].delay_ticks().is_some());
}

#[test]
fn max_attenuation_segment_boundary_at_257_is_exclusive() {
    let over_cap_source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct band size=257x5\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b\n  \
        logic sig.out = sig.a or sig.b\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.out\n  \
        circuit region=floor void=3\n";
    let routed = routed_from_source(over_cap_source, Edition::Java);
    let delayed = compile_delay(&routed);
    assert_eq!(
        delayed
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::AttenuationLimit)
            .count(),
        1,
        "segment == MAX_ATTENUATION_SEGMENT + 1 must fail: {:?}",
        delayed.diagnostics,
    );
    // Assert `MAX_ATTENUATION_SEGMENT` reads as 256 today so the
    // boundary fixtures above stay meaningful — a future edit that
    // bumps the cap without updating this test would silently lose
    // its boundary coverage.
    assert_eq!(MAX_ATTENUATION_SEGMENT, 256);
}

/// AC6 — an empty `ScopedPlacementIr` (no scopes) delays to an empty
/// `ScopedPlacementIr` with zero diagnostics: delay insertion is a
/// per-scope map, so no scopes ⇒ no work ⇒ no findings.
#[test]
fn empty_placement_ir_produces_empty_delay_output() {
    let scoped = ScopedPlacementIr::new();
    let delayed = compile_delay(&scoped);
    assert!(delayed.diagnostics.is_empty());
    assert!(delayed.scoped.scopes.is_empty());
}

/// AC7 — a scope whose Placement IR carries inputs and outputs but
/// no cells (identity wire) is elided by `compile_placement` already,
/// so `compile_delay` never sees it. The delay output must still be
/// diagnostic-clean.
#[test]
fn identity_wire_scope_stays_elided_through_delay() {
    let source = r"
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct wire size=5x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  door id=d side=front at=center mat_slot=door opened_by=sig.a
  circuit region=floor void=2
";
    let routed = routed_from_source(source, Edition::Java);
    let delayed = compile_delay(&routed);
    assert!(
        delayed.diagnostics.is_empty(),
        "identity-wire scope must not raise delay diagnostics, got {:?}",
        delayed.diagnostics,
    );
    assert!(
        delayed.scoped.scopes.iter().all(|e| e.name != "wire"),
        "identity-wire scope must stay elided in the delay output",
    );
}

/// AC8 — the JSON dump of a delayed Placement IR carries both
/// `"wire_length"` (routing's field) and `"delay_ticks"` (this pass's
/// field) on every cell object. Pins the wire-form contract that
/// distinguishes `--stage route` (no `delay_ticks`) from `--stage
/// delay` (`delay_ticks` present) output.
#[test]
fn json_dump_carries_delay_ticks_and_preserves_wire_length() {
    let source = load_example("redstone-door.crn");
    let routed = routed_from_source(&source, Edition::Java);
    let delayed = compile_delay(&routed);

    let json = serde_json::to_string(&delayed.scoped).expect("serialise");
    assert!(
        json.contains("\"wire_length\":"),
        "wire_length must survive the delay pass: {json}",
    );
    assert!(
        json.contains("\"delay_ticks\":"),
        "delay_ticks must appear in delayed JSON: {json}",
    );
}

/// `AC8b` — delay insertion must not perturb any Placement IR field
/// other than `delay_ticks`. Serialise the routed and delayed outputs
/// to compact JSON, strip the delayed side's `,"delay_ticks":<int>`
/// entries, and byte-compare the remainder. Pins the "field-write
/// only" wire-form contract the pass docstring on `PlacedCellNode`
/// declares: a downstream JSON consumer that inspects `--stage route`
/// output today should see byte-identical bytes from `--stage delay`
/// once `delay_ticks` is peeled off, so field reordering, added or
/// removed fields, or key-name typos in unrelated structs trip here
/// even when the JSON parses equivalently.
#[test]
fn delay_leaves_routed_fields_byte_identical_apart_from_delay_ticks() {
    let source = load_example("redstone-door.crn");
    let routed = routed_from_source(&source, Edition::Java);
    let routed_json = serde_json::to_string(&routed).expect("serialise routed");

    let delayed = compile_delay(&routed);
    assert!(
        delayed.diagnostics.is_empty(),
        "clean fixture must delay without diagnostics: {:?}",
        delayed.diagnostics,
    );
    let delayed_json = serde_json::to_string(&delayed.scoped).expect("serialise delayed");

    // `delay_ticks` sits after `wire_length` in `PlacedCellNode`'s
    // struct declaration and `serde` emits fields in declaration
    // order, so in compact JSON it always shows up as
    // `,"delay_ticks":<int>`. Strip that pattern and the delayed and
    // routed bytes must match exactly.
    let stripped_delayed = strip_delay_ticks(&delayed_json);
    assert_eq!(
        stripped_delayed, routed_json,
        "delay must not perturb routed fields — delayed compact JSON with delay_ticks stripped should match routed compact JSON byte-for-byte",
    );
}

fn strip_delay_ticks(compact: &str) -> String {
    const PATTERN: &str = ",\"delay_ticks\":";
    let mut out = String::with_capacity(compact.len());
    let mut rest = compact;
    while let Some(idx) = rest.find(PATTERN) {
        out.push_str(&rest[..idx]);
        let after = &rest[idx + PATTERN.len()..];
        let end = after
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after.len());
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// AC9 — Java and Bedrock `InverterTorch` both carry base 1 tick, so
/// a `sig.x = not sig.a` fixture produces matching `delay_ticks` on
/// both editions even though the cell tag differs. Pins the "delay
/// is edition-specific by cell choice, but edition-agnostic when the
/// cells happen to share a base" split.
#[test]
fn inverter_torch_delay_ticks_match_across_editions() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct inv size=5x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  logic sig.na = not sig.a
  door id=d side=front at=center mat_slot=wall opened_by=sig.na
  circuit region=floor void=2
";
    let java = compile_delay(&routed_from_source(source, Edition::Java));
    let bedrock = compile_delay(&routed_from_source(source, Edition::Bedrock));
    assert!(java.diagnostics.is_empty(), "{:?}", java.diagnostics);
    assert!(bedrock.diagnostics.is_empty(), "{:?}", bedrock.diagnostics);

    let j = java
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "inv")
        .expect("Java inv scope");
    let b = bedrock
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "inv")
        .expect("Bedrock inv scope");
    assert_eq!(j.ir.cells.len(), 1);
    assert_eq!(b.ir.cells.len(), 1);
    assert_eq!(
        j.ir.cells[0].delay_ticks(),
        b.ir.cells[0].delay_ticks(),
        "InverterTorch base_delay is identical on both editions → delay_ticks must match",
    );
    assert_ne!(
        j.ir.cells[0].cell, b.ir.cells[0].cell,
        "cell tag still differs per edition",
    );
}

/// AC10 — two non-empty scopes delay independently. A clean scope
/// passes through with `delay_ticks` populated; a scope that trips
/// the attenuation cap elides without poisoning the sibling. Scope
/// order for survivors matches input order.
#[test]
fn multiple_scopes_delay_independently() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct alpha size=7x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b
  logic sig.open = sig.a or sig.b
  door id=d side=front at=center mat_slot=wall opened_by=sig.open
  circuit region=floor void=2

struct wide_pack size=300x5
  floor mat_slot=wall
  pressure_plate id=r at=front.outside offset=0 y=0 -> sig.c
  pressure_plate id=s at=inside.front  offset=0 y=0 -> sig.d
  logic sig.out = sig.c or sig.d
  door id=e side=front at=center mat_slot=wall opened_by=sig.out
  circuit region=floor void=3
";
    let routed = routed_from_source(source, Edition::Java);
    let delayed = compile_delay(&routed);

    // alpha delays cleanly.
    let alpha = delayed
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "alpha")
        .expect("alpha scope survives delay");
    assert!(
        alpha.ir.cells.iter().all(|c| c.delay_ticks().is_some()),
        "every alpha cell must carry a computed delay_ticks",
    );

    // wide_pack hits the attenuation cap → elided with a
    // diagnostic without shifting alpha.
    assert!(
        delayed.scoped.scopes.iter().all(|e| e.name != "wide_pack"),
        "wide_pack must elide because a driver segment exceeds MAX_ATTENUATION_SEGMENT",
    );
    let wide_attenuation = delayed
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::AttenuationLimit)
        .expect("wide_pack's excess segment must surface as E_ATTENUATION_LIMIT");
    assert!(
        wide_attenuation.primary.contains("struct `wide_pack`"),
        "attenuation primary must name the failed scope, got {:?}",
        wide_attenuation.primary,
    );
}

/// AC11 — chaining `compile_delay(&delayed.scoped)` is forbidden by
/// the phase table on `PlacedCellNode`, and the panic it raises names
/// the cell that tripped it. Mirrors the routing pass's equivalent
/// guard: without the breadcrumb the backtrace points at the pass but
/// not at the cell whose `delay_ticks` was already committed.
#[test]
#[should_panic(
    expected = "for cell #0 at (0,0,0) in struct `gatehouse` — delay insertion must run once per routed IR"
)]
fn re_running_delay_pass_panics_loudly() {
    let source = load_example("redstone-door.crn");
    let delayed = compile_delay(&routed_from_source(&source, Edition::Java));
    let _twice = compile_delay(&delayed.scoped);
}
