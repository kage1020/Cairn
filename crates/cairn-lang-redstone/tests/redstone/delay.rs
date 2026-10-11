//! `compile_delay`: each cell's `local_delay_ticks` as its base delay plus the implicit buffer
//! repeaters on the nets feeding it, a local wire cost rather than an arrival time.

use cairn_lang_core::Edition;
use cairn_lang_core::check::Severity;
use cairn_lang_redstone::{
    DiagnosticCode, MAX_ATTENUATION_SEGMENT, PlacedCellNode, ScopedPlacementIr, compile_delay,
    compile_routing,
};

use crate::common::{
    load_example, normalize_stage_tags, placement_from_source, routed_from_source, strip_int_field,
};

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

/// A Bedrock torch OR is a dust merge, which carries no cell tick.
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
    assert_eq!(entry.ir.cells[0].local_delay_ticks(), Some(1));
    assert_eq!(entry.ir.cells[1].local_delay_ticks(), Some(1));
    assert_eq!(entry.ir.cells[2].local_delay_ticks(), Some(1));
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

/// Routing refuses a sink past the cap by distance before the delay pass sees it.
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
        "primary should name the placed netlist, the stage that refused: {:?}",
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
        "enlarging the region does not shorten a straight line: {:?}",
        footer.message,
    );
    let routed: Vec<_> = routing.scoped.scopes.iter().map(|e| &e.name).collect();
    assert_eq!(
        routed,
        vec!["narrow_pack"],
        "the failed scope is elided and its healthy sibling is not",
    );
    // The sibling keeps the delay pass's input from being empty.
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
    // A comparator passes on the strength it reads, so the dust from `sig.b` to the door is one
    // strand and repeaters land on some cell-to-cell nets too.
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

/// `size=257x5` puts the output segment at exactly the 256-block cap, and `size=258x5` one over.
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

#[test]
fn empty_placement_ir_produces_empty_delay_output() {
    let scoped = ScopedPlacementIr::new();
    let delayed = compile_delay(&scoped);
    assert!(delayed.diagnostics.is_empty());
    assert!(delayed.scoped.scopes.is_empty());
}

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

    // `local_delay_ticks` is never the first key, so it always reads `,"local_delay_ticks":<int>`.
    let stripped_delayed =
        normalize_stage_tags(&strip_int_field(&delayed_json, "local_delay_ticks"));
    assert_eq!(
        stripped_delayed,
        normalize_stage_tags(&routed_json),
        "delay must not perturb routed fields — delayed compact JSON with local_delay_ticks stripped and the stage tag normalised should match routed compact JSON byte-for-byte",
    );
}

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

/// Routing makes the refusal here; the delay pass works on the survivors it is handed.
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

#[test]
#[should_panic(
    expected = "for cell #0 at (1,0,1) in struct `gatehouse` — delay insertion must run exactly once per routed IR"
)]
fn re_running_delay_pass_panics_loudly() {
    let source = load_example("redstone-door.crn");
    let delayed = compile_delay(&routed_from_source(&source, Edition::Java));
    let _twice = compile_delay(&delayed.scoped);
}
