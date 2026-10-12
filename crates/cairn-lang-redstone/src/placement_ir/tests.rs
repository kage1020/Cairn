use super::phase::TRANSITION_CARDINALITY;
use super::*;

fn routed() -> PlacementPhase {
    PlacementPhase::Routed { wire_length: 3 }
}

fn delayed() -> PlacementPhase {
    PlacementPhase::Delayed {
        wire_length: 3,
        local_delay_ticks: 1,
    }
}

fn legalized() -> PlacementPhase {
    // The port is noise: no test below asserts on it.
    PlacementPhase::Legalized {
        wire_length: 3,
        local_delay_ticks: 1,
        buffer_coords: vec![BufferCoord::new(
            BufferSegment::Port(PortName::A),
            CellCoord::new(5, 0, 0),
        )],
    }
}

#[test]
fn route_from_unrouted_transitions_to_routed() {
    let mut phase = PlacementPhase::Unrouted;
    phase.route(7);
    assert_eq!(phase, PlacementPhase::Routed { wire_length: 7 });
}

#[test]
fn delay_from_routed_transitions_to_delayed_and_preserves_wire_length() {
    let mut phase = PlacementPhase::Routed { wire_length: 9 };
    phase.delay(4);
    assert_eq!(
        phase,
        PlacementPhase::Delayed {
            wire_length: 9,
            local_delay_ticks: 4,
        },
    );
}

#[test]
fn legalize_from_delayed_transitions_and_preserves_prior_fields() {
    let mut phase = PlacementPhase::Delayed {
        wire_length: 12,
        local_delay_ticks: 3,
    };
    let coords = vec![
        BufferCoord::new(BufferSegment::Port(PortName::A), CellCoord::new(1, 0, 0)),
        BufferCoord::new(BufferSegment::Port(PortName::A), CellCoord::new(2, 0, 0)),
    ];
    phase.legalize(coords.clone());
    assert_eq!(
        phase,
        PlacementPhase::Legalized {
            wire_length: 12,
            local_delay_ticks: 3,
            buffer_coords: coords,
        },
    );
}

#[test]
fn legalize_from_delayed_with_empty_buffers_stays_legalized() {
    let mut phase = PlacementPhase::Delayed {
        wire_length: 4,
        local_delay_ticks: 0,
    };
    phase.legalize(Vec::new());
    assert!(matches!(
        phase,
        PlacementPhase::Legalized {
            buffer_coords: ref bc,
            ..
        } if bc.is_empty()
    ));
}

fn probe_scope() -> ScopedPlacementIrEntry {
    ScopedPlacementIrEntry {
        kind: ScopeKind::Struct,
        name: "probe".to_string(),
        ir: PlacementIr::new(Edition::Java),
    }
}

fn probe_identity(scope: &ScopedPlacementIrEntry) -> CellIdentity<'_> {
    CellIdentity::new(7, CellCoord::new(1, 0, 2), scope)
}

#[test]
fn cell_identity_renders_index_coord_and_scope() {
    let scope = probe_scope();
    assert_eq!(
        probe_identity(&scope).to_string(),
        "cell #7 at (1,0,2) in struct `probe`",
    );
}

#[test]
fn cell_identity_renders_every_scope_kind() {
    for (kind, expected) in [
        (ScopeKind::Struct, "cell #7 at (1,0,2) in struct `probe`"),
        (ScopeKind::Def, "cell #7 at (1,0,2) in def `probe`"),
        (ScopeKind::Site, "cell #7 at (1,0,2) in site `probe`"),
    ] {
        let scope = ScopedPlacementIrEntry {
            kind,
            ..probe_scope()
        };
        assert_eq!(probe_identity(&scope).to_string(), expected);
    }
}

#[test]
fn cell_identity_surfaces_a_non_plane_layer() {
    let scope = probe_scope();
    let bridged = CellCoord {
        layer: RouteLayer::Bridge,
        ..CellCoord::new(1, 0, 2)
    };
    assert_eq!(
        CellIdentity::new(7, bridged, &scope).to_string(),
        "cell #7 at (1,0,2 on bridge) in struct `probe`",
    );
}

#[test]
fn route_at_from_unrouted_transitions_to_routed() {
    let scope = probe_scope();
    let mut phase = PlacementPhase::Unrouted;
    phase.route_at(7, probe_identity(&scope));
    assert_eq!(phase, PlacementPhase::Routed { wire_length: 7 });
}

#[test]
fn delay_at_from_routed_transitions_and_preserves_wire_length() {
    let scope = probe_scope();
    let mut phase = PlacementPhase::Routed { wire_length: 9 };
    phase.delay_at(4, probe_identity(&scope));
    assert_eq!(
        phase,
        PlacementPhase::Delayed {
            wire_length: 9,
            local_delay_ticks: 4,
        },
    );
}

#[test]
fn legalize_at_from_delayed_transitions_and_preserves_prior_fields() {
    let scope = probe_scope();
    let mut phase = PlacementPhase::Delayed {
        wire_length: 12,
        local_delay_ticks: 3,
    };
    let coords = vec![BufferCoord::new(
        BufferSegment::Port(PortName::A),
        CellCoord::new(1, 0, 0),
    )];
    phase.legalize_at(coords.clone(), probe_identity(&scope));
    assert_eq!(
        phase,
        PlacementPhase::Legalized {
            wire_length: 12,
            local_delay_ticks: 3,
            buffer_coords: coords,
        },
    );
}

/// One of the three transitions, as a value, so a test can loop over every refusal.
#[derive(Clone, Copy)]
enum Transition {
    Route,
    Delay,
    Legalize,
}

impl Transition {
    /// The fallible form.
    fn tried(self, phase: &mut PlacementPhase) -> Result<(), PlacementPhaseTransitionError> {
        match self {
            Self::Route => phase.try_route(5),
            Self::Delay => phase.try_delay(2),
            Self::Legalize => phase.try_legalize(vec![]),
        }
    }

    /// The panicking form.
    fn panicking(self, phase: &mut PlacementPhase) {
        match self {
            Self::Route => phase.route(5),
            Self::Delay => phase.delay(2),
            Self::Legalize => phase.legalize(vec![]),
        }
    }

    /// The panicking form that names a context.
    fn panicking_at(self, phase: &mut PlacementPhase, context: impl fmt::Display) {
        match self {
            Self::Route => phase.route_at(5, context),
            Self::Delay => phase.delay_at(2, context),
            Self::Legalize => phase.legalize_at(vec![], context),
        }
    }

    /// This transition's refusal of `phase`, tried on a clone.
    fn refusal(self, phase: &PlacementPhase) -> PlacementPhaseTransitionError {
        self.tried(&mut phase.clone())
            .expect_err("out-of-order transition must be refused")
    }
}

/// Every (transition, phase it refuses) pair: the nine illegal edges.
fn refusals() -> [(Transition, PlacementPhase); 9] {
    [
        (Transition::Route, routed()),
        (Transition::Route, delayed()),
        (Transition::Route, legalized()),
        (Transition::Delay, PlacementPhase::Unrouted),
        (Transition::Delay, delayed()),
        (Transition::Delay, legalized()),
        (Transition::Legalize, PlacementPhase::Unrouted),
        (Transition::Legalize, routed()),
        (Transition::Legalize, legalized()),
    ]
}

#[test]
fn every_refused_transition_panics_naming_the_pass() {
    let scope = probe_scope();
    for (transition, phase) in refusals() {
        let clause = match transition {
            Transition::Route => "routing must run exactly once per placement",
            Transition::Delay => "delay insertion must run exactly once per routed IR",
            Transition::Legalize => "crossing legalization must run exactly once per delayed IR",
        };
        let bare = panic_message(|| transition.panicking(&mut phase.clone()));
        assert!(bare.contains(clause), "{bare:?} lacks {clause:?}");
        let identified = format!("for cell #7 at (1,0,2) in struct `probe` — {clause}");
        let at =
            panic_message(|| transition.panicking_at(&mut phase.clone(), probe_identity(&scope)));
        assert!(at.contains(&identified), "{at:?} lacks {identified:?}");
    }
}

#[test]
fn try_route_from_unrouted_transitions_to_routed() {
    let mut phase = PlacementPhase::Unrouted;
    assert_eq!(phase.try_route(7), Ok(()));
    assert_eq!(phase, PlacementPhase::Routed { wire_length: 7 });
}

#[test]
fn try_delay_from_routed_transitions_and_preserves_wire_length() {
    let mut phase = PlacementPhase::Routed { wire_length: 9 };
    assert_eq!(phase.try_delay(4), Ok(()));
    assert_eq!(
        phase,
        PlacementPhase::Delayed {
            wire_length: 9,
            local_delay_ticks: 4,
        },
    );
}

#[test]
fn try_legalize_from_delayed_transitions_and_preserves_prior_fields() {
    let mut phase = PlacementPhase::Delayed {
        wire_length: 12,
        local_delay_ticks: 3,
    };
    let coords = vec![BufferCoord::new(
        BufferSegment::Port(PortName::A),
        CellCoord::new(1, 0, 0),
    )];
    assert_eq!(phase.try_legalize(coords.clone()), Ok(()));
    assert_eq!(
        phase,
        PlacementPhase::Legalized {
            wire_length: 12,
            local_delay_ticks: 3,
            buffer_coords: coords,
        },
    );
}

#[test]
fn try_legalize_from_delayed_with_empty_buffers_stays_legalized() {
    let mut phase = PlacementPhase::Delayed {
        wire_length: 4,
        local_delay_ticks: 0,
    };
    assert_eq!(phase.try_legalize(Vec::new()), Ok(()));
    assert!(matches!(
        phase,
        PlacementPhase::Legalized {
            buffer_coords: ref bc,
            ..
        } if bc.is_empty()
    ));
}

#[test]
fn try_route_from_every_other_phase_errs_without_mutating() {
    for phase in [routed(), delayed(), legalized()] {
        let mut subject = phase.clone();
        let error = subject
            .try_route(5)
            .expect_err("routing a non-Unrouted phase must be refused");
        assert_eq!(subject, phase, "a refused transition mutated the phase");
        assert_eq!(
            error,
            PlacementPhaseTransitionError::RouteOnNonUnrouted { current: phase },
        );
    }
}

#[test]
fn try_delay_from_every_other_phase_errs_without_mutating() {
    for phase in [PlacementPhase::Unrouted, delayed(), legalized()] {
        let mut subject = phase.clone();
        let error = subject
            .try_delay(2)
            .expect_err("delaying a non-Routed phase must be refused");
        assert_eq!(subject, phase, "a refused transition mutated the phase");
        assert_eq!(
            error,
            PlacementPhaseTransitionError::DelayOnNonRouted { current: phase },
        );
    }
}

#[test]
fn try_legalize_from_every_other_phase_errs_without_mutating() {
    for phase in [PlacementPhase::Unrouted, routed(), legalized()] {
        let mut subject = phase.clone();
        let error = subject
            .try_legalize(vec![])
            .expect_err("legalizing a non-Delayed phase must be refused");
        assert_eq!(subject, phase, "a refused transition mutated the phase");
        assert_eq!(
            error,
            PlacementPhaseTransitionError::LegalizeOnNonDelayed { current: phase },
        );
    }
}

#[test]
fn a_refused_transition_leaves_the_phase_usable() {
    let mut phase = routed();
    assert!(phase.try_route(5).is_err());
    phase.delay(2);
    assert_eq!(
        phase,
        PlacementPhase::Delayed {
            wire_length: 3,
            local_delay_ticks: 2,
        },
    );
}

#[test]
fn try_transition_errors_render_as_the_context_free_panic() {
    for (transition, phase) in refusals() {
        assert_eq!(
            transition.refusal(&phase).to_string(),
            panic_message(|| transition.panicking(&mut phase.clone())),
        );
    }
}

#[test]
fn try_transition_errors_with_context_render_as_the_at_panic() {
    let scope = probe_scope();
    for (transition, phase) in refusals() {
        let error = transition.refusal(&phase);
        assert_eq!(
            error.with_context(probe_identity(&scope)).to_string(),
            panic_message(|| transition.panicking_at(&mut phase.clone(), probe_identity(&scope))),
        );
    }
}

#[test]
fn transition_error_is_a_comparable_std_error() {
    fn assert_std_error<E: std::error::Error + Clone + Eq>(_: &E) {}

    for (transition, phase) in refusals() {
        let error = transition.refusal(&phase);
        assert_std_error(&error);
        assert_eq!(error.clone(), error);
        assert_eq!(error.current(), &phase);
        assert!(std::error::Error::source(&error).is_none());
    }
}

#[test]
fn context_free_transitions_panic_without_an_identity_clause() {
    let payloads = [
        panic_message(|| routed().route(5)),
        panic_message(|| PlacementPhase::Unrouted.delay(2)),
        panic_message(|| PlacementPhase::Unrouted.legalize(vec![])),
    ];
    for payload in &payloads {
        assert!(
            !payload.contains(" for "),
            "context-free panic grew an identity clause: {payload}",
        );
        assert!(
            !payload.contains("  "),
            "context-free panic grew a double space: {payload}",
        );
    }
    // The `#[should_panic]` sites across this crate key on this wording.
    assert_eq!(
        payloads,
        [
            "PlacementPhase::route called on Routed { wire_length: 3 } — routing must run exactly once per placement",
            "PlacementPhase::delay called on Unrouted — delay insertion must run exactly once per routed IR",
            "PlacementPhase::legalize called on Unrouted — crossing legalization must run exactly once per delayed IR",
        ],
    );
}

#[test]
fn every_transition_message_shares_one_cardinality_clause() {
    let scope = probe_scope();
    let messages = [
        panic_message(|| routed().route(5)),
        panic_message(|| PlacementPhase::Unrouted.delay(2)),
        panic_message(|| PlacementPhase::Unrouted.legalize(vec![])),
        panic_message(|| routed().route_at(5, probe_identity(&scope))),
        panic_message(|| PlacementPhase::Unrouted.delay_at(2, probe_identity(&scope))),
        panic_message(|| PlacementPhase::Unrouted.legalize_at(vec![], probe_identity(&scope))),
        Transition::Route.refusal(&routed()).to_string(),
        Transition::Delay
            .refusal(&PlacementPhase::Unrouted)
            .to_string(),
        Transition::Legalize
            .refusal(&PlacementPhase::Unrouted)
            .to_string(),
    ];
    for message in &messages {
        assert!(
            message.contains(TRANSITION_CARDINALITY),
            "transition message does not quote `{TRANSITION_CARDINALITY}`: {message}",
        );
    }
}

fn panic_message(body: impl FnOnce() + std::panic::UnwindSafe) -> String {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let payload = std::panic::catch_unwind(body).expect_err("transition must panic");
    std::panic::set_hook(previous);
    payload
        .downcast_ref::<String>()
        .cloned()
        .expect("transition panics carry a formatted String payload")
}

#[test]
fn wire_length_accessor_covers_every_variant() {
    assert_eq!(PlacementPhase::Unrouted.wire_length(), None);
    assert_eq!(routed().wire_length(), Some(3));
    assert_eq!(delayed().wire_length(), Some(3));
    assert_eq!(legalized().wire_length(), Some(3));
}

#[test]
fn local_delay_ticks_accessor_covers_every_variant() {
    assert_eq!(PlacementPhase::Unrouted.local_delay_ticks(), None);
    assert_eq!(routed().local_delay_ticks(), None);
    assert_eq!(delayed().local_delay_ticks(), Some(1));
    assert_eq!(legalized().local_delay_ticks(), Some(1));
}

#[test]
fn buffer_coords_accessor_covers_every_variant() {
    assert!(PlacementPhase::Unrouted.buffer_coords().is_empty());
    assert!(routed().buffer_coords().is_empty());
    assert!(delayed().buffer_coords().is_empty());
    assert_eq!(
        legalized().buffer_coords(),
        &[BufferCoord::new(
            BufferSegment::Port(PortName::A),
            CellCoord::new(5, 0, 0)
        )],
    );
}

#[test]
fn stage_accessor_covers_every_variant() {
    assert_eq!(PlacementPhase::Unrouted.stage(), PlacementStage::Placement);
    assert_eq!(routed().stage(), PlacementStage::Route);
    assert_eq!(delayed().stage(), PlacementStage::Delay);
    assert_eq!(legalized().stage(), PlacementStage::Crossing);
}

#[test]
fn legalized_with_zero_buffers_still_reports_the_crossing_stage() {
    let empty = PlacementPhase::Legalized {
        wire_length: 3,
        local_delay_ticks: 1,
        buffer_coords: Vec::new(),
    };
    assert_eq!(empty.stage(), PlacementStage::Crossing);
    assert_eq!(empty.stage(), legalized().stage());
    assert_ne!(empty.stage(), delayed().stage());
}

fn probe_cell(phase: PlacementPhase) -> PlacedCellNode {
    PlacedCellNode {
        cell: EditionCell::JavaRepeaterOr,
        drivers: Vec::new(),
        coord: CellCoord::new(0, 0, 0),
        phase,
        span: Span::default(),
    }
}

#[test]
fn placed_cell_stage_accessor_projects_the_phase() {
    for phase in [PlacementPhase::Unrouted, routed(), delayed(), legalized()] {
        let expected = phase.stage();
        assert_eq!(probe_cell(phase).stage(), expected);
    }
}

#[test]
fn stage_renders_the_cli_flag_vocabulary() {
    assert_eq!(PlacementStage::Placement.as_str(), "placement");
    assert_eq!(PlacementStage::Route.as_str(), "route");
    assert_eq!(PlacementStage::Delay.as_str(), "delay");
    assert_eq!(PlacementStage::Crossing.as_str(), "crossing");
}

#[test]
fn every_stage_serialises_as_a_bare_string() {
    for (stage, expected) in [
        (PlacementStage::Placement, "\"placement\""),
        (PlacementStage::Route, "\"route\""),
        (PlacementStage::Delay, "\"delay\""),
        (PlacementStage::Crossing, "\"crossing\""),
    ] {
        assert_eq!(
            serde_json::to_string(&stage).expect("stage serialises"),
            expected,
        );
    }
}

#[test]
fn delayed_and_legalized_with_zero_buffers_serialise_differently() {
    let delayed_json =
        serde_json::to_string(&probe_cell(delayed())).expect("delayed cell serialises");
    let legalized_json = serde_json::to_string(&probe_cell(PlacementPhase::Legalized {
        wire_length: 3,
        local_delay_ticks: 1,
        buffer_coords: Vec::new(),
    }))
    .expect("legalized cell serialises");

    assert_ne!(delayed_json, legalized_json);
    assert!(
        delayed_json.contains("\"stage\":\"delay\""),
        "delayed cell must carry the delay tag: {delayed_json}",
    );
    assert!(
        legalized_json.contains("\"stage\":\"crossing\""),
        "legalized cell must carry the crossing tag: {legalized_json}",
    );
    assert!(
        !legalized_json.contains("buffer_coords"),
        "empty buffer_coords must still serde-skip: {legalized_json}",
    );
    // Replace the pair, not the bare word `delay`, which `local_delay_ticks` also holds.
    assert_eq!(
        delayed_json.replace("\"stage\":\"delay\"", "\"stage\":\"crossing\""),
        legalized_json,
    );
}

/// `2147483648^2 * 4` is exactly `2^64`, which a wrapping product would turn into zero.
#[test]
fn reserved_area_saturates_on_a_reservation_no_u64_can_hold() {
    let region = |width: u32, depth: u32, void: u32| CircuitRegionReservation {
        label: "floor".to_owned(),
        void,
        width,
        depth,
        span: Span::default(),
    };
    assert_eq!(region(6, 3, 2).reserved_area(), 36, "the ordinary product");
    assert_eq!(
        region(u32::MAX, u32::MAX, 1).reserved_area(),
        u64::from(u32::MAX) * u64::from(u32::MAX),
        "two u32s fit, so this one is exact",
    );
    assert_eq!(
        region(u32::MAX, u32::MAX, 3).reserved_area(),
        u64::MAX,
        "three do not, and the answer is the ceiling rather than a panic",
    );
    assert_eq!(
        region(2_147_483_648, 2_147_483_648, 4).reserved_area(),
        u64::MAX,
        "exactly 2^64: the ceiling, not the zero a wrap would give",
    );
}

/// `probe_cell`'s dump read back, or the deserializer's refusal as a string.
fn read_back(json: &str) -> Result<PlacedCellNode, String> {
    serde_json::from_str(json).map_err(|err| err.to_string())
}

#[test]
fn every_phase_round_trips_through_the_flat_wire_form() {
    let bridged = PlacementPhase::Legalized {
        wire_length: 20,
        local_delay_ticks: 2,
        buffer_coords: vec![
            BufferCoord::new(BufferSegment::Port(PortName::Sel), CellCoord::new(4, 0, 2)),
            BufferCoord::new(BufferSegment::Port(PortName::A), CellCoord::new(9, 1, 2)),
        ],
    };
    let empty = PlacementPhase::Legalized {
        wire_length: 3,
        local_delay_ticks: 1,
        buffer_coords: Vec::new(),
    };
    for phase in [
        PlacementPhase::Unrouted,
        routed(),
        delayed(),
        legalized(),
        empty,
        bridged,
    ] {
        let mut cell = probe_cell(phase);
        cell.drivers = vec![
            CellPortDriver {
                port: PortName::Sel,
                net: NetRef::Input(0),
            },
            CellPortDriver {
                port: PortName::A,
                net: NetRef::Cell(2),
            },
        ];
        let json = serde_json::to_string(&cell).expect("cell serialises");
        assert_eq!(read_back(&json), Ok(cell), "{json}");
    }
}

#[test]
fn delayed_and_legalized_with_zero_buffers_read_back_apart() {
    let empty = PlacementPhase::Legalized {
        wire_length: 3,
        local_delay_ticks: 1,
        buffer_coords: Vec::new(),
    };
    for phase in [delayed(), empty] {
        let json = serde_json::to_string(&probe_cell(phase.clone())).expect("serialises");
        let back = read_back(&json).expect("reads back");
        assert_eq!(back.phase, phase, "{json}");
    }
}

/// A refusal's sentence, without the position `serde_json` appends.
fn sentence(err: &str) -> &str {
    err.split(" at line ").next().unwrap_or(err)
}

#[test]
fn every_payload_key_that_disagrees_with_the_stage_is_refused() {
    let body = r#""cell":"java_repeater_or","drivers":[],"coord":{"x":0,"y":0,"z":0}"#;
    let early = |key: &str, writer: &str, stage: &str| {
        format!(
            "`{key}` is written by the {writer} stage, which a cell tagged \
             `stage: \"{stage}\"` has not reached"
        )
    };
    let due = |key: &str, writer: &str, stage: &str| {
        format!(
            "a cell tagged `stage: \"{stage}\"` has been through the {writer} stage and \
             must carry `{key}`"
        )
    };
    let rows = [
        (
            "placement",
            r#","wire_length":3"#,
            early("wire_length", "route", "placement"),
        ),
        (
            "placement",
            r#","local_delay_ticks":1"#,
            early("local_delay_ticks", "delay", "placement"),
        ),
        (
            "placement",
            r#","buffer_coords":[]"#,
            early("buffer_coords", "crossing", "placement"),
        ),
        (
            "route",
            r#","wire_length":3,"local_delay_ticks":1"#,
            early("local_delay_ticks", "delay", "route"),
        ),
        (
            "route",
            r#","wire_length":3,"buffer_coords":[]"#,
            early("buffer_coords", "crossing", "route"),
        ),
        ("route", "", due("wire_length", "route", "route")),
        (
            "delay",
            r#","wire_length":3,"local_delay_ticks":1,"buffer_coords":[]"#,
            early("buffer_coords", "crossing", "delay"),
        ),
        (
            "delay",
            r#","local_delay_ticks":1"#,
            due("wire_length", "route", "delay"),
        ),
        (
            "delay",
            r#","wire_length":3"#,
            due("local_delay_ticks", "delay", "delay"),
        ),
        (
            "crossing",
            r#","local_delay_ticks":1"#,
            due("wire_length", "route", "crossing"),
        ),
        (
            "crossing",
            r#","wire_length":3"#,
            due("local_delay_ticks", "delay", "crossing"),
        ),
    ];
    for (stage, payload, expected) in rows {
        let json = format!(r#"{{"stage":"{stage}"{payload},{body}}}"#);
        let err = read_back(&json).expect_err(&json);
        assert_eq!(sentence(&err), expected, "{json}");
    }
}

#[test]
fn malformed_dumps_are_refused_rather_than_guessed_at() {
    let coord = r#""coord":{"x":0,"y":0,"z":0}"#;
    let cell = r#""cell":"java_repeater_or","drivers":[]"#;
    let crossing =
        r#""stage":"crossing","wire_length":1,"local_delay_ticks":1,"cell":"java_repeater_or""#;
    let driver_a = r#"{"port":"a","net":{"kind":"input","index":0}}"#;
    for (json, expected) in [
        (format!("{{{cell},{coord}}}"), "missing field `stage`"),
        (
            format!(r#"{{"stage":"legalize",{cell},{coord}}}"#),
            "unknown variant `legalize`, expected one of `placement`, `route`, `delay`, \
             `crossing`",
        ),
        (
            format!(
                r#"{{"stage":"placement",{cell},"coord":{{"x":0,"y":1,"z":0,"layer":"attic"}}}}"#
            ),
            "unknown variant `attic`, expected one of `plane`, `bridge`, `via`",
        ),
        (
            format!(
                r#"{{{crossing},"drivers":[],"buffer_coords":[{{"port":"c","coord":{{"x":0,"y":0,"z":0}}}}],{coord}}}"#
            ),
            "unknown variant `c`, expected one of `a`, `b`, `sel`, `out`",
        ),
        (
            format!(r#"{{"stage":"placement","edition":"java",{cell},{coord}}}"#),
            "unknown field `edition`",
        ),
        (
            format!(r#"{{"stage":"placement","stage":"route",{cell},{coord}}}"#),
            "duplicate field `stage`",
        ),
        (
            format!(r#"{{"stage":"placement","drivers":[],{coord}}}"#),
            "missing field `cell`",
        ),
        (
            format!(r#"{{"stage":"placement",{cell},"coord":{{"x":0,"y":0,"z":0,"junk":7}}}}"#),
            "unknown field `junk`",
        ),
        (
            format!(
                r#"{{"stage":"placement","cell":"java_repeater_or","drivers":[{{"port":"a","net":{{"kind":"input","index":0}},"junk":7}}],{coord}}}"#
            ),
            "unknown field `junk`",
        ),
        (
            format!(
                r#"{{"stage":"placement","cell":"java_repeater_or","drivers":[{{"port":"a","net":{{"kind":"input","index":0,"junk":7}}}}],{coord}}}"#
            ),
            // An adjacently tagged enum words its refusal this way.
            r#"invalid value: string "junk", expected "kind" or "index""#,
        ),
        (
            format!(
                r#"{{{crossing},"drivers":[{driver_a}],"buffer_coords":[{{"port":"a","coord":{{"x":0,"y":0,"z":2}},"junk":7}}],{coord}}}"#
            ),
            "unknown field `junk`",
        ),
    ] {
        let err = read_back(&json).expect_err(&json);
        assert!(err.contains(expected), "{json}: {err}");
    }
}

#[test]
fn a_coord_reads_back_only_on_the_layer_its_height_gives() {
    let read = |json: &str| serde_json::from_str::<CellCoord>(json).map_err(|e| e.to_string());
    for (json, expected) in [
        (
            r#"{"x":4,"y":0,"z":2}"#,
            CellCoord::with_layer(4, 0, 2, RouteLayer::Plane),
        ),
        (
            r#"{"x":4,"y":1,"z":2}"#,
            CellCoord::with_layer(4, 1, 2, RouteLayer::Bridge),
        ),
        (
            r#"{"x":4,"y":0,"z":2,"layer":"plane"}"#,
            CellCoord::with_layer(4, 0, 2, RouteLayer::Plane),
        ),
        (
            r#"{"x":4,"y":2,"z":2,"layer":"bridge"}"#,
            CellCoord::with_layer(4, 2, 2, RouteLayer::Bridge),
        ),
    ] {
        assert_eq!(read(json), Ok(expected), "{json}");
    }
    for (json, expected) in [
        (
            r#"{"x":0,"y":0,"z":1,"layer":"bridge"}"#,
            "a coord at `y: 0` is on the `plane` layer, not `bridge`",
        ),
        (
            r#"{"x":0,"y":1,"z":1,"layer":"plane"}"#,
            "a coord at `y: 1` is on the `bridge` layer, not `plane`",
        ),
        (
            r#"{"x":0,"y":0,"z":1,"layer":"via"}"#,
            "a coord at `y: 0` is on the `plane` layer, not `via`",
        ),
        (
            r#"{"x":0,"y":3,"z":1,"layer":"via"}"#,
            "a coord at `y: 3` is on the `bridge` layer, not `via`",
        ),
    ] {
        let err = read(json).expect_err(json);
        assert_eq!(sentence(&err), expected, "{json}");
    }
}

#[test]
fn a_buffer_on_a_segment_the_cell_has_no_driver_for_is_refused() {
    let head = r#""stage":"crossing","wire_length":3,"local_delay_ticks":1,"cell":"java_repeater_or","coord":{"x":0,"y":0,"z":1},"drivers":[{"port":"a","net":{"kind":"input","index":0}}]"#;
    let on = |port: &str, x: u32| format!(r#"{{"port":"{port}","coord":{{"x":{x},"y":0,"z":2}}}}"#);

    let fine = format!(
        r#"{{{head},"buffer_coords":[{},{}]}}"#,
        on("a", 3),
        on("a", 9)
    );
    let cell = read_back(&fine).expect(&fine);
    assert_eq!(cell.buffer_coords().len(), 2, "{fine}");

    for (buffers, expected) in [
        (
            format!("{},{}", on("a", 3), on("out", 9)),
            "`buffer_coords[1]` stands on the `out` segment, which runs to a placed output \
             and is never a cell's",
        ),
        (
            format!("{},{}", on("a", 3), on("sel", 9)),
            "`buffer_coords[1]` stands on a port no entry of this cell's `drivers` feeds",
        ),
    ] {
        let json = format!(r#"{{{head},"buffer_coords":[{buffers}]}}"#);
        let err = read_back(&json).expect_err(&json);
        assert_eq!(sentence(&err), expected, "{json}");
    }
}

#[test]
fn every_wire_word_reads_back_as_its_variant() {
    use strum::IntoEnumIterator;

    fn round_trip<T: Serialize + for<'de> Deserialize<'de> + PartialEq + fmt::Debug>(value: &T) {
        let json = serde_json::to_string(&value).expect("serialises");
        let back: T = serde_json::from_str(&json).expect("reads back");
        assert_eq!(&back, value, "{json}");
    }
    for layer in RouteLayer::iter() {
        round_trip(&layer);
        assert_eq!(
            serde_json::to_value(layer).expect("serialises"),
            layer.as_str()
        );
    }
    for stage in PlacementStage::iter() {
        round_trip(&stage);
        assert_eq!(
            serde_json::to_value(stage).expect("serialises"),
            stage.as_str()
        );
    }
    for port in PortName::iter() {
        round_trip(&BufferSegment::Port(port));
    }
    round_trip(&BufferSegment::Out);
}

#[test]
fn the_segment_vocabulary_is_every_word_a_segment_is_written_as() {
    use strum::IntoEnumIterator;

    let written: Vec<String> = PortName::iter()
        .map(BufferSegment::Port)
        .chain([BufferSegment::Out])
        .map(|segment| match serde_json::to_value(segment) {
            Ok(serde_json::Value::String(word)) => word,
            other => panic!("a segment serialises as a string: {other:?}"),
        })
        .collect();
    assert_eq!(written, BufferSegment::WORDS);
}
