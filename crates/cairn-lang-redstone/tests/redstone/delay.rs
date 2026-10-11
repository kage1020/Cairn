//! `compile_delay`, stage 3 of `spec/redstone` "Place-and-route": each cell's
//! `local_delay_ticks` as its base delay plus the implicit buffer repeaters on the nets feeding
//! it, `E_ATTENUATION_LIMIT` past the segment cap, and a wire form that differs from the routed IR
//! only in `local_delay_ticks` and the `stage` tag. Every figure is a local wire cost, not an
//! arrival time (see the `cairn_lang_redstone::delay` module doc).

use cairn_lang_core::Edition;
use cairn_lang_core::check::Severity;
use cairn_lang_redstone::{
    DiagnosticCode, MAX_ATTENUATION_SEGMENT, PlacedCellNode, ScopedPlacementIr, compile_delay,
    compile_routing,
};

use crate::common::{
    load_example, normalize_stage_tags, placement_from_source, routed_from_source, strip_int_field,
};

/// `examples/redstone-door.crn` compiled for Java: the sole `JavaRepeaterOr` cell gets
/// `local_delay_ticks = Some(1)`, the repeater's base tick and no implicit buffers, because both
/// driver segments (2 blocks each) are under the dust-attenuation limit of 15. `wire_length` is
/// preserved verbatim from routing.
#[test]
fn redstone_door_java_local_delay_ticks_equal_base_repeater_delay() {
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
        cell.local_delay_ticks(),
        Some(1),
        "JavaRepeaterOr base = 1 tick, both driver segments ≤ 15 → 0 buffers",
    );
    assert_eq!(
        cell.wire_length(),
        Some(4),
        "routing's wire_length must survive the delay pass verbatim",
    );
}

/// The same example compiled for Bedrock: the sole `BedrockTorchOr` cell gets
/// `local_delay_ticks = Some(0)`, since a dust merge carries no cell tick and both driver segments
/// (2 blocks each) are under the attenuation limit. `wire_length` matches Java because routing is
/// edition-agnostic.
#[test]
fn redstone_door_bedrock_local_delay_ticks_are_zero() {
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
        cell.local_delay_ticks(),
        Some(0),
        "BedrockTorchOr is a bare dust merge → 0 cell tick + 0 buffers",
    );
    assert_eq!(
        cell.wire_length(),
        Some(4),
        "wire_length is edition-independent by construction",
    );
}

/// Three cascaded cells each get a pinned tick sum. The routing suite's 3-cell fixture:
/// `sig.and_ab = sig.a and sig.b`, `sig.or_ab = sig.a or sig.b`,
/// `sig.combined = sig.and_ab and sig.or_ab`. Cells stand at `x = 1, 3, 5` and the input pads at
/// `(0, 0, 0)` and `(0, 0, 2)`, so every driver segment is well under the dust limit and each
/// cell's `local_delay_ticks` is its base tick count alone.
#[test]
fn multi_cell_scope_pins_local_delay_ticks_from_base_only() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct sim size=7x5
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b

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
    // JavaComparatorAnd: base 1, segments [2, 2] → 0 buffers.
    assert_eq!(entry.ir.cells[0].local_delay_ticks(), Some(1));
    // JavaRepeaterOr: base 1, segments [4, 4] → 0 buffers.
    assert_eq!(entry.ir.cells[1].local_delay_ticks(), Some(1));
    // JavaComparatorAnd: base 1, cell-to-cell segments [6, 2] → 0 buffers.
    assert_eq!(entry.ir.cells[2].local_delay_ticks(), Some(1));
    // `wire_length` from routing is preserved verbatim on every cell. The byte-identical JSON test
    // below covers it too; pinned per cell so a divergent field write in
    // `attribute_local_delay_ticks` trips here.
    assert_eq!(
        entry
            .ir
            .cells
            .iter()
            .map(PlacedCellNode::wire_length)
            .collect::<Vec<_>>(),
        vec![Some(4), Some(8), Some(8)],
        "the routed lengths, unchanged",
    );
}

/// A scope whose output-pad segment exceeds [`MAX_ATTENUATION_SEGMENT`] gets
/// `E_ATTENUATION_LIMIT`, is elided, and never gets a partial `local_delay_ticks` set. The region
/// is wider than 256, so the sole cell's output driver spans the `x` axis to the right-edge pad.
///
/// The routing pass refuses it rather than the delay pass: a sink further from its driver than
/// the cap has no route any stage would accept, and the pass that would otherwise lay a 299-block
/// wire to find out is the one that says so. The delay pass's own check is for a segment that
/// crosses the cap by going round something rather than by distance, a shape no `.crn` produces,
/// which `delay::tests` covers from a hand-built IR.
#[test]
fn attenuation_limit_fires_at_routing_and_elides_scope() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct wide_pack size=300x5
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b

  logic sig.out = sig.a or sig.b

  door id=d side=front at=center mat_slot=wall opened_by=sig.out

  circuit region=floor void=3

struct narrow_pack size=20x5
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b

  logic sig.out = sig.a or sig.b

  door id=d side=front at=center mat_slot=wall opened_by=sig.out

  circuit region=floor void=3
";
    let routing = compile_routing(&placement_from_source(source, Edition::Java));

    let attenuation: Vec<_> = routing
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::AttenuationLimit)
        .collect();
    assert_eq!(
        attenuation.len(),
        1,
        "expected exactly one E_ATTENUATION_LIMIT, got {:?}",
        routing.diagnostics,
    );
    let d = attenuation[0];
    assert_eq!(d.severity(), Severity::Error);
    assert!(
        d.primary.starts_with("placed netlist for "),
        "primary should name the stage that refused: the routing pass sees cells that \
         are still `Unrouted`, so the noun is `placed`, not `routed`. Got {:?}",
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
    for phrase in ["split", "region", "cannot help"] {
        assert!(
            footer.message.contains(phrase),
            "footer should say what repairs a distance (missing {phrase:?}), got {:?}",
            footer.message,
        );
    }
    assert!(
        !footer.message.contains("enlarge"),
        "the delay pass's `enlarge region=` is the repair for a route that went round \
         something; nothing shortens a straight line, and for this shape — a `region=` as \
         wide as the `size=` it came from — enlarging is the wrong direction: {:?}",
        footer.message,
    );
    let routed: Vec<_> = routing.scoped.scopes.iter().map(|e| &e.name).collect();
    assert_eq!(
        routed,
        vec!["narrow_pack"],
        "the failed scope is elided and its healthy sibling is not",
    );
    // The sibling is what makes this say anything: it carries through to
    // the delay pass, so "no attenuation diagnostic downstream" is a
    // statement about a scope that is actually there to be diagnosed,
    // rather than about an empty input.
    let delayed = compile_delay(&routing.scoped);
    assert!(
        delayed.diagnostics.is_empty(),
        "the elided scope must not be half-attributed downstream: {:?}",
        delayed.diagnostics,
    );
    let delayed_names: Vec<_> = delayed.scoped.scopes.iter().map(|e| &e.name).collect();
    assert_eq!(
        delayed_names,
        vec!["narrow_pack"],
        "the sibling delays normally",
    );
}

/// A cascaded chain long enough to push a cell's shared driver segment past
/// `DUST_ATTENUATION_LIMIT` but not past [`MAX_ATTENUATION_SEGMENT`] gets `local_delay_ticks`
/// bumped by the implicit buffers. Every cell shares `sig.b` as a driver, so `cell[i]` (at
/// `x = 1 + 2i, y = 0, z = 1`) sees a `sig.b` segment that grows with its column, and the
/// previous cell contributes a short one. `cell[i]`'s implicit buffer count is the number of
/// repeaters on its segments, summed across drivers.
#[test]
fn cascaded_and_chain_records_implicit_buffer_ticks() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct chain size=40x5
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b

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
    // cell[0] .. cell[15] map to `sig.c0` .. `sig.c15`, all `JavaComparatorAnd` with
    // base_delay = 1. Each has two drivers: the previous cell, next door but one, and the shared
    // `sig.b`, whose trunk runs the length of the row; cell `i` stands at `x = 1 + 2i`.
    //
    // A comparator passes on the strength it reads rather than restoring it, so the dust from
    // `sig.b` through every comparator after it to the door is one strand. Repeaters therefore
    // stand on some cell-to-cell nets too, and the trunk's two stand closer than 16 steps apart,
    // because each comparator it feeds can take less than a full budget of dust: the trunk's
    // first reaches i = 7 and its second i = 14, and the cell-to-cell ones are the rest (into
    // i = 4, 6, 7, 11, 13, 14). Hardcoded per index rather than derived from the placement, so a
    // self-referential slip cannot slide past.
    assert_eq!(entry.ir.cells.len(), 16);
    let expected: [u32; 16] = [
        1, 1, 1, 1, 2, 1, 2, // i = 0..=6
        3, 2, 2, 2, 3, 2, 3, // i = 7..=13
        4, 3, // i = 14..=15
    ];
    for (i, cell) in entry.ir.cells.iter().enumerate() {
        assert_eq!(
            cell.local_delay_ticks(),
            Some(expected[i]),
            "cell[{i}]: expected local_delay_ticks = {}",
            expected[i],
        );
    }
    // The chain must exercise both bands of the delay model — base
    // alone (i = 0, 1, 2, 3, 5) and base plus implicit buffers (the rest)
    // — otherwise shrinking the fixture would silently lose either.
    assert!(
        entry
            .ir
            .cells
            .iter()
            .any(|c| c.local_delay_ticks() == Some(1))
    );
    assert!(
        entry
            .ir
            .cells
            .iter()
            .any(|c| c.local_delay_ticks() == Some(2))
    );
}

/// `sig.and_ab = sig.a and sig.b` on Bedrock lowers to `BedrockTorchAnd`, whose
/// `base_delay_ticks()` is 2 (two NAND torches in series): the only pinned base delay that is
/// neither 0 nor 1, so a typo of `1` or `3` in the base-delay table fails here and nowhere else.
/// Both driver segments (1 and 2 blocks) are under the attenuation limit, so
/// `local_delay_ticks = 2 + 0`.
#[test]
fn bedrock_torch_and_local_delay_ticks_pin_two_tick_base() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct band size=5x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b
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
        entry.ir.cells[0].local_delay_ticks(),
        Some(2),
        "BedrockTorchAnd base_delay must be 2 ticks",
    );
}

/// An output-pad segment of exactly `MAX_ATTENUATION_SEGMENT` blocks passes; one more block
/// fails. Pins the `>` vs `>=` boundary of the cap.
///
/// With `size=257x5` the output pad lands at `(256, 0, 0)` and the sole cell at `(1, 0, 1)`. The
/// wire out runs the length of the cell row and drops into the pad at the far corner, exactly the
/// 256-block cap. `size=258x5` adds a column and the segment becomes 257.
#[test]
fn max_attenuation_segment_boundary_at_256_is_inclusive() {
    let at_cap_source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct band size=257x5\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b\n  \
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
    assert!(entry.ir.cells[0].local_delay_ticks().is_some());
}

#[test]
fn max_attenuation_segment_boundary_at_257_is_exclusive() {
    let over_cap_source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct band size=258x5\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b\n  \
        logic sig.out = sig.a or sig.b\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.out\n  \
        circuit region=floor void=3\n";
    // One block over is a distance, not a detour, so the straight-line
    // gate answers it before a route is laid. The at-cap fixture above
    // is the other side of the same boundary and still goes all the way
    // through the delay pass, which is what makes the pair meaningful:
    // the gate has to let 256 through and stop 257.
    let routing = compile_routing(&placement_from_source(over_cap_source, Edition::Java));
    assert_eq!(
        routing
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::AttenuationLimit)
            .count(),
        1,
        "segment == MAX_ATTENUATION_SEGMENT + 1 must fail: {:?}",
        routing.diagnostics,
    );
    // The boundary fixtures above are meaningful only while the cap is 256.
    assert_eq!(MAX_ATTENUATION_SEGMENT, 256);
}

/// An empty `ScopedPlacementIr` (no scopes) delays to an empty
/// `ScopedPlacementIr` with zero diagnostics: delay insertion is a
/// per-scope map, so no scopes ⇒ no work ⇒ no findings.
#[test]
fn empty_placement_ir_produces_empty_delay_output() {
    let scoped = ScopedPlacementIr::new();
    let delayed = compile_delay(&scoped);
    assert!(delayed.diagnostics.is_empty());
    assert!(delayed.scoped.scopes.is_empty());
}

/// A scope with inputs and outputs but no cells (an identity wire) reaches the delay pass, since
/// placement gives its actuator a pad. Across a 5-wide region the segment is four blocks, well
/// inside the attenuation limit, so it is charged nothing: the point is that it is charged at all
/// rather than skipped.
#[test]
fn identity_wire_is_charged_for_its_own_segment() {
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
    let scope = delayed
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "wire")
        .expect("identity-wire scope must reach the delayed IR");
    let output = scope.ir.outputs.first().expect("the actuator is placed");
    assert_eq!(
        output.local_delay_ticks(),
        Some(0),
        "a four-block segment needs no buffer repeater",
    );
}

/// The JSON dump of a delayed Placement IR carries both `"wire_length"` (routing's field) and
/// `"local_delay_ticks"` (this pass's) on every cell object: the wire-form difference between
/// `--stage route` and `--stage delay` output.
#[test]
fn json_dump_carries_local_delay_ticks_and_preserves_wire_length() {
    let source = load_example("redstone-door.crn");
    let routed = routed_from_source(&source, Edition::Java);
    let delayed = compile_delay(&routed);

    let json = serde_json::to_string(&delayed.scoped).expect("serialise");
    assert!(
        json.contains("\"stage\":\"delay\""),
        "every delayed cell must carry the delay stage tag: {json}",
    );
    assert!(
        json.contains("\"wire_length\":"),
        "wire_length must survive the delay pass: {json}",
    );
    assert!(
        json.contains("\"local_delay_ticks\":"),
        "local_delay_ticks must appear in delayed JSON: {json}",
    );
}

/// Delay insertion perturbs no Placement IR field but `local_delay_ticks` and the `stage` tag.
/// The routed and delayed outputs, as compact JSON with the delayed side's
/// `,"local_delay_ticks":<int>` entries stripped and both stage tags normalised, are
/// byte-identical, so field reordering, added or removed fields, or key-name typos in unrelated
/// structs trip here even when the JSON parses equivalently.
#[test]
fn delay_leaves_routed_fields_byte_identical_apart_from_local_delay_ticks_and_stage() {
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

    // `local_delay_ticks` follows `wire_length` in `PlacedCellNode`'s emission order, so in
    // compact JSON it always reads `,"local_delay_ticks":<int>`.
    let stripped_delayed =
        normalize_stage_tags(&strip_int_field(&delayed_json, "local_delay_ticks"));
    assert_eq!(
        stripped_delayed,
        normalize_stage_tags(&routed_json),
        "delay must not perturb routed fields — delayed compact JSON with local_delay_ticks stripped and the stage tag normalised should match routed compact JSON byte-for-byte",
    );
}

/// Java and Bedrock `InverterTorch` both carry a base of 1 tick, so `sig.x = not sig.a` gets
/// matching `local_delay_ticks` on both editions though the cell tag differs: delay depends on
/// the edition only through the cell chosen.
#[test]
fn inverter_torch_local_delay_ticks_match_across_editions() {
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
        j.ir.cells[0].local_delay_ticks(),
        b.ir.cells[0].local_delay_ticks(),
        "InverterTorch base_delay is identical on both editions → local_delay_ticks must match",
    );
    assert_ne!(
        j.ir.cells[0].cell, b.ir.cells[0].cell,
        "cell tag still differs per edition",
    );
}

/// One scope's refusal does not disturb its sibling: a clean scope passes through with
/// `local_delay_ticks` populated while the scope that trips the attenuation cap is elided, and
/// survivors keep their input order.
///
/// The routing pass makes this refusal, so what this pins is the routing pass's independence and
/// the delay pass working on the survivor set it is handed. A delay-pass refusal that spares its
/// sibling needs a scope whose straight line fits and whose route does not, which no `.crn`
/// produces; `delay::tests::a_detour_refusal_leaves_its_sibling_alone` covers it from a
/// hand-built IR.
#[test]
fn multiple_scopes_delay_independently() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct alpha size=7x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b
  logic sig.open = sig.a or sig.b
  door id=d side=front at=center mat_slot=wall opened_by=sig.open
  circuit region=floor void=2

struct wide_pack size=300x5
  floor mat_slot=wall
  pressure_plate id=r at=front.outside offset=0 y=0 -> sig.c
  pressure_plate id=s at=inside.front  offset=1 y=0 -> sig.d
  logic sig.out = sig.c or sig.d
  door id=e side=front at=center mat_slot=wall opened_by=sig.out
  circuit region=floor void=3
";
    // `wide_pack` is refused for its distance by the routing pass, so the delay pass is handed
    // the survivor set; one scope's refusal must still not shift or poison its sibling.
    let routing = compile_routing(&placement_from_source(source, Edition::Java));
    assert!(
        routing
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::AttenuationLimit),
        "wide_pack must be refused for its distance: {:?}",
        routing.diagnostics,
    );
    let delayed = compile_delay(&routing.scoped);
    assert!(
        delayed.diagnostics.is_empty(),
        "the surviving scope delays cleanly: {:?}",
        delayed.diagnostics,
    );

    // alpha delays cleanly.
    let alpha = delayed
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "alpha")
        .expect("alpha scope survives delay");
    assert!(
        alpha
            .ir
            .cells
            .iter()
            .all(|c| c.local_delay_ticks().is_some()),
        "every alpha cell must carry a computed local_delay_ticks",
    );

    // wide_pack hits the attenuation cap → elided with a
    // diagnostic without shifting alpha.
    assert!(
        delayed.scoped.scopes.iter().all(|e| e.name != "wide_pack"),
        "wide_pack was elided upstream, so the delay pass must not resurrect it",
    );
    let wide_attenuation = routing
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

/// Chaining `compile_delay(&delayed.scoped)` is forbidden by the producer↔variant table on
/// `PlacementPhase`, and the panic names the cell that tripped it, as the routing pass's guard
/// does: without that, the backtrace points at the pass but not at the cell whose
/// `local_delay_ticks` was already committed.
#[test]
#[should_panic(
    expected = "for cell #0 at (1,0,1) in struct `gatehouse` — delay insertion must run exactly once per routed IR"
)]
fn re_running_delay_pass_panics_loudly() {
    let source = load_example("redstone-door.crn");
    let delayed = compile_delay(&routed_from_source(&source, Edition::Java));
    let _twice = compile_delay(&delayed.scoped);
}
