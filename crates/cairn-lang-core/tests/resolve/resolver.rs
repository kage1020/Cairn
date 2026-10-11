//! `resolve` over whole sources: which theme each scope binds, what its
//! members and selectors resolve to, which `connect` rows survive, and the
//! findings along the way.

use cairn_lang_core::check::{Diagnostic, DiagnosticCode, Severity};
use cairn_lang_core::resolve::{Resolution, ScopeResolution, resolve};
use cairn_lang_core::{Edition, IntentModule, Span, lower, parse};

fn ir(source: &str) -> IntentModule {
    let module = parse(source).expect("parse");
    lower(&module)
}

#[test]
fn single_theme_file_resolves_struct_slots() {
    let src = "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
    let r = resolve(&ir(src), None);
    let scope = r.scopes.get("struct::s").expect("scope present");
    assert_eq!(scope.bound_theme.as_deref(), Some("t"));
    let bound = scope.members.values().next().expect("member binding");
    assert!(bound.slot_value.is_some(), "wall slot should resolve");
    assert!(
        r.diagnostics.is_empty(),
        "no diagnostics expected, got {:?}",
        r.diagnostics,
    );
}

#[test]
fn unresolved_slot_emits_diagnostic() {
    let src = "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=4x4\n  walls mat_slot=floor height=3\n";
    let r = resolve(&ir(src), None);
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnresolvedSlot && d.severity() == Severity::Error),
        "expected E_UNRESOLVED_SLOT, got {:?}",
        r.diagnostics,
    );
}

#[test]
fn multiple_themes_leave_struct_unbound() {
    let src = "theme a:\n  slot wall -> @cobblestone\ntheme b:\n  slot wall -> @stone\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
    let r = resolve(&ir(src), None);
    let scope = r.scopes.get("struct::s").unwrap();
    assert!(scope.bound_theme.is_none());
    // No E_UNRESOLVED_SLOT because no theme was applied.
    assert!(
        !r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
    );
}

#[test]
fn multi_theme_unbound_scope_gets_no_selector_extras() {
    // Regression: an earlier version walked `themes.values_mut()`
    // unconditionally and wrote every matching selector into
    // `selector_extras`, even when the scope's `bound_theme` was None.
    // That violated the per-theme DI contract from
    // `spec/materials-themes`.
    let src = "theme a:\n  walls[class=outer] -> trim=@a_trim\ntheme b:\n  walls[class=outer] -> trim=@b_trim\n\nstruct s size=4x4\n  walls class=outer height=3\n";
    let r = resolve(&ir(src), None);
    let scope = r.scopes.get("struct::s").unwrap();
    assert!(scope.bound_theme.is_none());
    let bound = scope.members.values().next().unwrap();
    assert!(
        bound.selector_extras.is_empty(),
        "unbound scope must not absorb selectors from any theme, got {:?}",
        bound.selector_extras,
    );
}

#[test]
fn unmatched_selector_warning_is_suppressed_for_unapplied_themes() {
    // Regression: in a multi-theme file, no theme is applied to the
    // struct/def, so warning on every theme selector would be noise.
    // A theme is picked at the `place ... theme=X` boundary; until
    // that binding runs, the selectors are not "unmatched", they're
    // "not yet bound".
    let src = "theme a:\n  walls[class=outer] -> trim=@a\ntheme b:\n  walls[class=outer] -> trim=@b\n\nstruct s size=4x4\n  walls class=outer height=3\n";
    let r = resolve(&ir(src), None);
    assert!(
        !r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ThemeSelectorUnmatched),
        "no warnings expected, got {:?}",
        r.diagnostics,
    );
}

#[test]
fn selector_match_adds_extras_and_marks_matched() {
    let src = "theme t:\n  slot wall -> @cobblestone\n  walls[class=outer] -> trim=@spruce_log\n\nstruct s size=4x4\n  walls class=outer mat_slot=wall height=3\n";
    let r = resolve(&ir(src), None);
    let scope = r.scopes.get("struct::s").unwrap();
    let bound = scope.members.values().next().unwrap();
    assert!(
        bound.selector_extras.contains_key("trim"),
        "extras: {:?}",
        bound.selector_extras,
    );
    assert!(
        !r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ThemeSelectorUnmatched),
    );
}

#[test]
fn unmatched_selector_emits_warning() {
    let src = "theme t:\n  slot wall -> @cobblestone\n  walls[class=does_not_exist] -> trim=@spruce_log\n\nstruct s size=4x4\n  walls class=outer mat_slot=wall height=3\n";
    let r = resolve(&ir(src), None);
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ThemeSelectorUnmatched
                && d.severity() == Severity::Warning),
    );
}

/// The warning names the row's attributes rather than eliding them to
/// `[...]`. One theme can hold several rows on one keyword, and a
/// message that cannot tell them apart makes the reader match spans by
/// hand. `check::duplicate` renders the same way, through the same
/// helper, so two findings on one row spell the selector alike.
#[test]
fn unmatched_selector_names_the_attributes_it_filtered_on() {
    let src = "theme t:\n  slot wall -> @cobblestone\n  \
             walls[class=does_not_exist] -> trim=@a\n  \
             walls[class=\"nor_this\",side=front] -> trim=@b\n\n\
             struct s size=4x4\n  walls class=outer mat_slot=wall height=3\n";
    let r = resolve(&ir(src), None);
    let primaries: Vec<&str> = r
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::ThemeSelectorUnmatched)
        .map(|d| d.primary.as_str())
        .collect();
    assert_eq!(
        primaries,
        [
            "theme selector `walls[class=does_not_exist]` in `t` does not match any member",
            "theme selector `walls[class=\"nor_this\",side=front]` in `t` does not match any member",
        ],
    );
}

#[test]
fn unresolved_slot_attaches_did_you_mean_note() {
    // `mat_slot=wal` is one deletion away from the declared `wall` slot;
    // the resolver must surface that as a targeted suggestion so the
    // fix is unambiguous.
    let src = "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=4x4\n  walls mat_slot=wal height=3\n";
    let r = resolve(&ir(src), None);
    let diag = r
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::UnresolvedSlot)
        .unwrap_or_else(|| panic!("no E_UNRESOLVED_SLOT, got {:?}", r.diagnostics));
    assert!(
        diag.notes
            .iter()
            .any(|n| n.message.contains("did you mean `wall`")),
        "expected `did you mean` note, got {:#?}",
        diag.notes,
    );
}

#[test]
fn unresolved_slot_skips_suggestion_when_pool_is_far_away() {
    // No theme slot is close to `quartz` — the resolver must not invent
    // a guess. The remediation note stays as the lone follow-up.
    let src = "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=4x4\n  walls mat_slot=quartz height=3\n";
    let r = resolve(&ir(src), None);
    let diag = r
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::UnresolvedSlot)
        .unwrap_or_else(|| panic!("no E_UNRESOLVED_SLOT, got {:?}", r.diagnostics));
    assert!(
        !diag
            .notes
            .iter()
            .any(|n| n.message.contains("did you mean")),
        "no suggestion expected, got {:#?}",
        diag.notes,
    );
}

#[test]
fn connect_resolves_to_port_refs_with_path_material() {
    // Two cottages connected by gravel — the canonical village shape.
    // The resolver must build one `ValidatedConnect` carrying both
    // `(place, port)` pairs plus the `path=@gravel` value, with
    // no diagnostics.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def cottage size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=cottage theme=t at=origin\n",
        "  place id=b use=cottage theme=t east_of=a gap=2\n",
        "  connect a.entry to b.entry path=@gravel\n",
    );
    let r = resolve(&ir(src), None);
    assert!(
        !r.diagnostics
            .iter()
            .any(|d| d.severity() == Severity::Error),
        "no errors expected, got {:?}",
        r.diagnostics,
    );
    assert_eq!(r.connects.len(), 1, "exactly one connect resolved");
    let c = &r.connects[0];
    assert_eq!(c.site, "s");
    assert_eq!(c.from.place, "a");
    assert_eq!(c.from.port, "entry");
    assert_eq!(c.to.place, "b");
    assert_eq!(c.to.port, "entry");
}

#[test]
fn connect_unknown_port_emits_e_unresolved_port() {
    // Port `entry` is exposed by the def, but the user mistyped it.
    // The resolver must surface E_UNRESOLVED_PORT with a nearest-
    // match note pointing at the correct `place.port` shape.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def cottage size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=cottage theme=t at=origin\n",
        "  place id=b use=cottage theme=t east_of=a gap=2\n",
        "  connect a.entr to b.entry path=@gravel\n",
    );
    let r = resolve(&ir(src), None);
    let diag = r
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::UnresolvedPort)
        .unwrap_or_else(|| panic!("expected E_UNRESOLVED_PORT, got {:?}", r.diagnostics));
    assert_eq!(diag.severity(), Severity::Error);
    assert!(
        diag.notes.iter().any(|n| n.message.contains("a.entry")),
        "expected nearest-match note pointing at a.entry, got {:?}",
        diag.notes,
    );
    // Anchor: the span must point at the `a.entr` DotRef, not at
    // the whole `connect` row. Mirrors the to-side assertion in
    // `connect_unknown_port_on_to_side_emits_e_unresolved_port`.
    let typo_start = src.find("a.entr").expect("typo present");
    let typo_end = typo_start + "a.entr".len();
    assert_eq!(diag.span.start, typo_start);
    assert_eq!(diag.span.end, typo_end);
    // The failed connect must not surface as resolved — walkway
    // voxelisation only sees rows it can lay safely.
    assert!(r.connects.is_empty(), "broken connect must not resolve");
}

/// The `E_UNRESOLVED_PORT` findings `r` carries, in emission order.
fn unresolved_ports(r: &Resolution) -> Vec<&Diagnostic> {
    r.diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::UnresolvedPort)
        .collect()
}

/// The byte range of the first occurrence of `needle` in `src`.
fn span_of(src: &str, needle: &str) -> Span {
    let start = src
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` is not in the source"));
    start..start + needle.len()
}

#[test]
fn a_port_under_a_level_without_an_id_is_refused_for_the_nesting() {
    // A `level` may go without `id=`, so the refusal names it by its
    // keyword. `tests/door_wall_fit.rs` pins the named case. The
    // refusal underlines the endpoint, as the "not declared" one does,
    // and its note points at the nested door.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def cottage size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=front side=front at=center\n",
        "  level y=0\n",
        "    door id=entry side=back at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=cottage theme=t at=origin\n",
        "  place id=b use=cottage theme=t east_of=a gap=2\n",
        "  connect a.entry to b.front path=@gravel\n",
    );
    let r = resolve(&ir(src), None);
    let [refusal] = unresolved_ports(&r)[..] else {
        panic!("expected one E_UNRESOLVED_PORT, got {:?}", r.diagnostics);
    };
    assert_eq!(
        refusal.primary,
        "port `entry` of `def cottage` (used by `place a`) is declared under a `level`, \
             and a member under a `level` cannot be a port yet",
    );
    assert_eq!(refusal.span, span_of(src, "a.entry"));
    let notes: Vec<(Option<Span>, &str)> = refusal
        .notes
        .iter()
        .map(|n| (n.span.clone(), n.message.as_str()))
        .collect();
    assert_eq!(
        notes,
        [(
            Some(span_of(src, "door id=entry side=back at=center")),
            "a port has to be a door or window declared directly in the body of \
                 `def cottage`: use one of those as the endpoint; level-scoped ports are a \
                 future extension",
        )],
    );
    assert!(
        r.connects.is_empty(),
        "the refused connect must not resolve"
    );
}

#[test]
fn a_port_under_a_level_inside_a_level_names_the_inner_level() {
    // The door hangs off `inner`, which is also the `level`
    // `E_UNSUPPORTED_NESTING` notes for this input, so the refusal
    // names `inner` rather than `outer`.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def tower size=3x3:\n",
        "  walls mat_slot=wall height=4\n",
        "  door id=front side=front at=center\n",
        "  level y=0 id=outer\n",
        "    level y=1 id=inner\n",
        "      door id=up_entry side=back at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=tower theme=t at=origin\n",
        "  place id=b use=tower theme=t east_of=a gap=2\n",
        "  connect a.up_entry to b.front path=@gravel\n",
    );
    let r = resolve(&ir(src), None);
    let [refusal] = unresolved_ports(&r)[..] else {
        panic!("expected one E_UNRESOLVED_PORT, got {:?}", r.diagnostics);
    };
    assert_eq!(
        refusal.primary,
        "port `up_entry` of `def tower` (used by `place a`) is declared under \
             `level id=inner`, and a member under a `level` cannot be a port yet",
    );
    let note_spans: Vec<Option<Span>> = refusal.notes.iter().map(|n| n.span.clone()).collect();
    assert_eq!(
        note_spans,
        [Some(span_of(src, "door id=up_entry side=back at=center"))],
    );
    assert!(
        r.connects.is_empty(),
        "the refused connect must not resolve"
    );
}

#[test]
fn a_body_port_resolves_when_a_level_carries_the_same_id() {
    // The body's `front` is the port. The `front` under the `level` is
    // not looked at, since the body lookup found one: a refusal here
    // would drop a walkway the source asks for.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def tower size=3x3:\n",
        "  walls mat_slot=wall height=4\n",
        "  door id=front side=front at=center\n",
        "  level y=0 id=up\n",
        "    door id=front side=back at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=tower theme=t at=origin\n",
        "  place id=b use=tower theme=t east_of=a gap=2\n",
        "  connect a.front to b.front path=@gravel\n",
    );
    let r = resolve(&ir(src), None);
    assert_eq!(
        unresolved_ports(&r).len(),
        0,
        "a body member carries `front`: {:?}",
        r.diagnostics,
    );
    assert_eq!(r.connects.len(), 1, "{:?}", r.diagnostics);
}

#[test]
fn a_typo_in_a_def_with_a_level_is_still_not_declared() {
    // `frnt` is on no member, in the body or under the `level`, so the
    // refusal is the "not declared" one, with its did-you-mean note
    // from the body's ids and its note to add the `id=` — not the
    // level-scoped one, which would send the author to the `level`.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def tower size=3x3:\n",
        "  walls mat_slot=wall height=4\n",
        "  door id=front side=front at=center\n",
        "  level y=0 id=up\n",
        "    door id=up_entry side=back at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=tower theme=t at=origin\n",
        "  place id=b use=tower theme=t east_of=a gap=2\n",
        "  connect a.frnt to b.front path=@gravel\n",
    );
    let r = resolve(&ir(src), None);
    let [refusal] = unresolved_ports(&r)[..] else {
        panic!("expected one E_UNRESOLVED_PORT, got {:?}", r.diagnostics);
    };
    assert_eq!(
        refusal.primary,
        "port `frnt` is not declared by `def tower` (used by `place a`)",
    );
    assert_eq!(refusal.span, span_of(src, "a.frnt"));
    let notes: Vec<&str> = refusal.notes.iter().map(|n| n.message.as_str()).collect();
    assert_eq!(
        notes,
        [
            "did you mean `a.front`?",
            "add `id=frnt` to a member of `def tower` (e.g. `door id=frnt ...`)",
        ],
    );
    assert!(
        r.connects.is_empty(),
        "the refused connect must not resolve"
    );
}

#[test]
fn connect_ambiguous_port_emits_e_ambiguous_port() {
    // Two members of the def carry `id=entry`. The reference is
    // ambiguous; the resolver must say so loudly rather than picking
    // one silently.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def cottage size=5x5:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "  window id=entry side=back y=1 offset=1 size=1x1 mat_slot=wall\n",
        "\n",
        "site s:\n",
        "  place id=a use=cottage theme=t at=origin\n",
        "  place id=b use=cottage theme=t east_of=a gap=2\n",
        "  connect a.entry to b.entry path=@gravel\n",
    );
    let r = resolve(&ir(src), None);
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::AmbiguousPort),
        "expected E_AMBIGUOUS_PORT, got {:?}",
        r.diagnostics,
    );
}

#[test]
fn connect_missing_path_emits_e_missing_path_material() {
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def cottage size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=cottage theme=t at=origin\n",
        "  place id=b use=cottage theme=t east_of=a gap=2\n",
        "  connect a.entry to b.entry\n",
    );
    let r = resolve(&ir(src), None);
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::MissingPathMaterial),
        "expected E_MISSING_PATH_MATERIAL, got {:?}",
        r.diagnostics,
    );
    assert!(r.connects.is_empty());
}

#[test]
fn connect_non_token_path_emits_e_missing_path_material() {
    // `path=plain_ident` and `path="gravel"` slip through the
    // material resolver as `MaterialDeferred::AlreadyDiagnosed` (the
    // theme-slot path). The resolver must surface that as
    // E_MISSING_PATH_MATERIAL with a "must be an @ token" note,
    // otherwise the row drops silently in the walkway voxeliser.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def cottage size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=cottage theme=t at=origin\n",
        "  place id=b use=cottage theme=t east_of=a gap=2\n",
        "  connect a.entry to b.entry path=plain_ident\n",
    );
    let r = resolve(&ir(src), None);
    let diag = r
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::MissingPathMaterial)
        .unwrap_or_else(|| panic!("expected E_MISSING_PATH_MATERIAL, got {:?}", r.diagnostics));
    assert_eq!(diag.severity(), Severity::Error);
    assert!(
        diag.primary.contains("token") && diag.primary.contains("identifier"),
        "expected the message to call out the kind mismatch, got: {}",
        diag.primary,
    );
    assert!(r.connects.is_empty(), "non-token path must not resolve");
}

#[test]
fn connect_unknown_place_emits_e_unresolved_place_ref() {
    // `ghost` is not a known place id. Re-uses the existing
    // E_UNRESOLVED_PLACE_REF code so a single diagnostic family
    // covers every "unknown place" path (origin selectors and now
    // connect refs).
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def cottage size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=cottage theme=t at=origin\n",
        "  connect a.entry to ghost.entry path=@gravel\n",
    );
    let r = resolve(&ir(src), None);
    let diag = r
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::UnresolvedPlaceRef && d.primary.contains("ghost"))
        .unwrap_or_else(|| {
            panic!(
                "expected E_UNRESOLVED_PLACE_REF mentioning ghost, got {:?}",
                r.diagnostics,
            )
        });
    // Anchor: span underlines the `ghost.entry` reference itself.
    let bad_start = src.find("ghost.entry").expect("bad ref present");
    let bad_end = bad_start + "ghost.entry".len();
    assert_eq!(diag.span.start, bad_start);
    assert_eq!(diag.span.end, bad_end);
}

#[test]
fn connect_with_both_endpoints_unknown_emits_both_e_unresolved_place_ref() {
    // Pin the "fail both halves" contract: a row whose `from` and
    // `to` both name absent places must surface *two* diagnostics
    // (one per offending DotRef), not just the first one. Catches a
    // regression where short-circuit on the `from` half would let
    // the `to` typo slip through silently.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def cottage size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=cottage theme=t at=origin\n",
        "  connect ghost.entry to phantom.entry path=@gravel\n",
    );
    let r = resolve(&ir(src), None);
    let ghost_hit = r
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::UnresolvedPlaceRef && d.primary.contains("ghost"));
    let phantom_hit = r
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::UnresolvedPlaceRef && d.primary.contains("phantom"));
    assert!(
        ghost_hit && phantom_hit,
        "expected E_UNRESOLVED_PLACE_REF for both `ghost` and `phantom`, got {:?}",
        r.diagnostics,
    );
    assert!(r.connects.is_empty(), "broken connect must not resolve");
}

#[test]
fn connect_unknown_port_on_to_side_emits_e_unresolved_port() {
    // Symmetric to `connect_unknown_port_emits_e_unresolved_port`: the
    // bad port is on the `to` half (`b.entr`) instead of the `from`
    // half. The diagnostic must anchor at the offending token, not at
    // the whole `connect` line.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def cottage size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=cottage theme=t at=origin\n",
        "  place id=b use=cottage theme=t east_of=a gap=2\n",
        "  connect a.entry to b.entr path=@gravel\n",
    );
    let r = resolve(&ir(src), None);
    let diag = r
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::UnresolvedPort)
        .unwrap_or_else(|| panic!("expected E_UNRESOLVED_PORT, got {:?}", r.diagnostics));
    assert_eq!(diag.severity(), Severity::Error);
    assert!(
        diag.notes.iter().any(|n| n.message.contains("b.entry")),
        "expected nearest-match note pointing at b.entry, got {:?}",
        diag.notes,
    );
    // The span must point at the `b.entr` token itself so a renderer
    // can underline the typo rather than the whole `connect` row.
    let typo_start = src.find("b.entr").expect("typo present");
    let typo_end = typo_start + "b.entr".len();
    assert_eq!(
        diag.span.start, typo_start,
        "span should anchor at the `b.entr` DotRef, got {:?}",
        diag.span,
    );
    assert_eq!(
        diag.span.end, typo_end,
        "span should end at the `b.entr` DotRef, got {:?}",
        diag.span,
    );
    assert!(r.connects.is_empty(), "broken connect must not resolve");
}

#[test]
fn connect_unknown_place_on_from_side_emits_e_unresolved_place_ref() {
    // Symmetric to `connect_unknown_place_emits_e_unresolved_place_ref`:
    // the unknown place id is on the `from` half instead of the `to`
    // half. A single E_UNRESOLVED_PLACE_REF code must continue to
    // cover both directions.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def cottage size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=cottage theme=t at=origin\n",
        "  connect ghost.entry to a.entry path=@gravel\n",
    );
    let r = resolve(&ir(src), None);
    let diag = r
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::UnresolvedPlaceRef && d.primary.contains("ghost"))
        .unwrap_or_else(|| {
            panic!(
                "expected E_UNRESOLVED_PLACE_REF mentioning ghost, got {:?}",
                r.diagnostics,
            )
        });
    // The span must underline the `ghost.entry` reference so the
    // renderer points at the bad token rather than the whole row.
    let bad_start = src.find("ghost.entry").expect("bad ref present");
    let bad_end = bad_start + "ghost.entry".len();
    assert_eq!(
        diag.span.start, bad_start,
        "span should anchor at the `ghost.entry` DotRef, got {:?}",
        diag.span,
    );
    assert_eq!(
        diag.span.end, bad_end,
        "span should end at the `ghost.entry` DotRef, got {:?}",
        diag.span,
    );
    assert!(r.connects.is_empty(), "broken connect must not resolve");
}

/// A slot bound to something that is not a material token is an
/// error, not advisory: `walls mat_slot=wall` below lowers to air, so
/// a `cairn check` that exited 0 would certify a hollow build.
#[test]
fn unknown_slot_target_emits_error() {
    let src = "theme t:\n  slot wall -> plain_ident\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
    let r = resolve(&ir(src), None);
    assert!(r.diagnostics.iter().any(
            |d| d.code == DiagnosticCode::UnknownSlotTarget && d.severity() == Severity::Error
        ),);
}

// ------------------------------------------------------------------
// Per-edition theme fallback (`spec/versioning-editions` "Java / Bedrock
// portability", #2).
// The following tests pin the AC set that keeps the resolver honest
// about which variant it bound and when the sibling-slot union kicks in.
// ------------------------------------------------------------------

fn per_edition_variants_src() -> String {
    // One logical theme `t` split into `_java` / `_bedrock` variants.
    // Each variant has a private slot (`java_only` / `bedrock_only`) so
    // the variant-picking behaviour is observable via `slot_value`, and
    // a shared slot (`floor`) confirms both variants participate in
    // `single_logical_theme` grouping.
    [
        "theme t_java:",
        "  slot floor -> @oak_planks",
        "  slot java_only -> @spruce_planks",
        "",
        "theme t_bedrock:",
        "  slot floor -> @oak_planks",
        "  slot bedrock_only -> @dark_oak_planks",
        "",
        "struct s size=4x4",
        "  floor  mat_slot=floor",
        "",
    ]
    .join("\n")
}

#[test]
fn per_edition_java_picks_java_variant() {
    let r = resolve(&ir(&per_edition_variants_src()), Some(Edition::Java));
    let scope = r.scopes.get("struct::s").expect("scope present");
    assert_eq!(scope.bound_theme.as_deref(), Some("t_java"));
    assert!(
        r.diagnostics.is_empty(),
        "no diagnostics expected for a slot both variants declare, got {:?}",
        r.diagnostics,
    );
}

#[test]
fn per_edition_bedrock_picks_bedrock_variant() {
    let r = resolve(&ir(&per_edition_variants_src()), Some(Edition::Bedrock));
    let scope = r.scopes.get("struct::s").expect("scope present");
    assert_eq!(scope.bound_theme.as_deref(), Some("t_bedrock"));
    assert!(r.diagnostics.is_empty(), "got {:?}", r.diagnostics);
}

#[test]
fn per_edition_variant_binds_only_its_own_slot_under_compile() {
    // Under Some(Java), a member referencing a slot that only the
    // Bedrock variant declares must fail loud — sibling-slot union is
    // reserved for the edition = None case.
    let src = [
        "theme t_java:",
        "  slot floor -> @oak_planks",
        "",
        "theme t_bedrock:",
        "  slot floor -> @oak_planks",
        "  slot bedrock_only -> @dark_oak_planks",
        "",
        "struct s size=4x4",
        "  floor mat_slot=bedrock_only",
        "",
    ]
    .join("\n");
    let r = resolve(&ir(&src), Some(Edition::Java));
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
        "expected E_UNRESOLVED_SLOT under --edition java for a Bedrock-only slot, got {:?}",
        r.diagnostics,
    );
}

#[test]
fn per_edition_check_unions_slots_across_variants() {
    // AC10: resolve(ir, None) on a file with `_java` + `_bedrock` variants
    // must accept a `mat_slot=X` when X is declared by *either* variant.
    let src = [
        "theme t_java:",
        "  slot floor -> @oak_planks",
        "  slot java_only -> @spruce_planks",
        "",
        "theme t_bedrock:",
        "  slot floor -> @oak_planks",
        "  slot bedrock_only -> @dark_oak_planks",
        "",
        "struct s size=4x4",
        "  floor mat_slot=java_only",
        "  floor mat_slot=bedrock_only",
        "",
    ]
    .join("\n");
    let r = resolve(&ir(&src), None);
    assert!(
        !r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
        "sibling-slot union should suppress E_UNRESOLVED_SLOT for either variant's slot, got {:?}",
        r.diagnostics,
    );
}

#[test]
fn per_edition_check_still_errors_on_slot_absent_from_all_variants() {
    // The union softens `mat_slot=X` only when X is declared *somewhere*.
    // A slot declared by no variant of the logical theme is still an
    // error, so a genuine typo is still caught under `cairn check`.
    let src = [
        "theme t_java:",
        "  slot floor -> @oak_planks",
        "",
        "theme t_bedrock:",
        "  slot floor -> @oak_planks",
        "",
        "struct s size=4x4",
        "  floor mat_slot=totally_bogus",
        "",
    ]
    .join("\n");
    let r = resolve(&ir(&src), None);
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
        "expected E_UNRESOLVED_SLOT for a slot no variant declares, got {:?}",
        r.diagnostics,
    );
}

#[test]
fn per_edition_falls_back_to_unsuffixed_when_variant_missing() {
    // `theme t:` (no variant suffix) is the shared-default form used
    // by every existing example. It must resolve for both editions so
    // the current corpus keeps compiling untouched.
    let src = [
        "theme t:",
        "  slot floor -> @oak_planks",
        "",
        "struct s size=4x4",
        "  floor mat_slot=floor",
        "",
    ]
    .join("\n");
    for edition in [Some(Edition::Java), Some(Edition::Bedrock), None] {
        let r = resolve(&ir(&src), edition);
        let scope = r.scopes.get("struct::s").unwrap();
        assert_eq!(
            scope.bound_theme.as_deref(),
            Some("t"),
            "edition={edition:?}"
        );
        assert!(
            r.diagnostics.is_empty(),
            "edition={edition:?}: got {:?}",
            r.diagnostics,
        );
    }
}

#[test]
fn per_edition_java_preferred_over_bedrock_when_unsuffixed_absent() {
    // `edition = None` prefers the unsuffixed variant, then Java, then
    // Bedrock — the deterministic order avoids leaking source order
    // into diagnostics when only variants exist.
    let src = [
        "theme t_bedrock:",
        "  slot floor -> @dark_oak_planks",
        "",
        "theme t_java:",
        "  slot floor -> @oak_planks",
        "",
        "struct s size=4x4",
        "  floor mat_slot=floor",
        "",
    ]
    .join("\n");
    let r = resolve(&ir(&src), None);
    let scope = r.scopes.get("struct::s").unwrap();
    assert_eq!(scope.bound_theme.as_deref(), Some("t_java"));
}

#[test]
fn distinct_logical_themes_still_leave_scope_unbound() {
    // Two truly distinct logical themes (`cottage` and `keep`) remain
    // the multi-theme deferred-selection case — the `_java`/`_bedrock`
    // grouping must not collapse them into one.
    let src = [
        "theme cottage:",
        "  slot floor -> @oak_planks",
        "",
        "theme keep:",
        "  slot floor -> @stone_bricks",
        "",
        "struct s size=4x4",
        "  floor mat_slot=floor",
        "",
    ]
    .join("\n");
    let r = resolve(&ir(&src), Some(Edition::Java));
    let scope = r.scopes.get("struct::s").unwrap();
    assert!(
        scope.bound_theme.is_none(),
        "two distinct logical themes must remain unbound, got {:?}",
        scope.bound_theme,
    );
}

#[test]
fn per_edition_java_does_not_fall_back_to_bedrock_variant() {
    // Silent misrouting guard: a file with only `theme t_bedrock:` must
    // leave the scope unbound under `Some(Edition::Java)` rather than
    // silently binding the Bedrock variant, which would route
    // Bedrock-only slot values into a Java `.nbt`. The loud outcome
    // `spec/versioning-editions` "Fail-loud and minimum-version
    // inference" requires is `E_THEME_VARIANT_MISSING`, asserted below.
    let src = [
        "theme t_bedrock:",
        "  slot floor -> @dark_oak_planks",
        "",
        "struct s size=4x4",
        "  floor mat_slot=floor",
        "",
    ]
    .join("\n");
    let r = resolve(&ir(&src), Some(Edition::Java));
    let scope = r.scopes.get("struct::s").unwrap();
    assert!(
        scope.bound_theme.is_none(),
        "Some(Java) with only `_bedrock` variant must not bind, got {:?}",
        scope.bound_theme,
    );
    // Not `E_UNRESOLVED_SLOT`: the slot is declared and spelled
    // correctly, and that message has to name the theme the slot is
    // missing from — of which there is none, because the pin refused
    // the only one. The theme is what cannot be honoured, so the
    // finding names the theme. Reporting nothing at all is what let a
    // build write the requested extent out of air at exit 0.
    assert!(
        !r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
    );
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ThemeVariantMissing),
        "the refusal must be reported, got {:?}",
        r.diagnostics,
    );
}

#[test]
fn per_edition_bedrock_does_not_fall_back_to_java_variant() {
    // Symmetric guard for the opposite direction — a file with only
    // `theme t_java:` must not bind under `Some(Edition::Bedrock)`.
    let src = [
        "theme t_java:",
        "  slot floor -> @oak_planks",
        "",
        "struct s size=4x4",
        "  floor mat_slot=floor",
        "",
    ]
    .join("\n");
    let r = resolve(&ir(&src), Some(Edition::Bedrock));
    let scope = r.scopes.get("struct::s").unwrap();
    assert!(scope.bound_theme.is_none());
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ThemeVariantMissing),
        "got {:?}",
        r.diagnostics,
    );
}

// ------------------------------------------------------------------
// A pin the module cannot satisfy, and a pin the site path ignored.
// ------------------------------------------------------------------

/// One logical theme `shop` in the given variants, plus a def and a
/// site that places it under `theme={reference}`.
fn placed_under(reference: &str, variants: &[&str]) -> String {
    use std::fmt::Write as _;

    let mut declarations = String::new();
    for variant in variants {
        let value = match *variant {
            "_bedrock" => "dark_oak_planks",
            "_java" => "spruce_planks",
            _ => "oak_planks",
        };
        write!(
            declarations,
            "theme shop{variant}:\n  slot floor -> @{value}\n\n"
        )
        .expect("writing to a String cannot fail");
    }
    format!(
        "{declarations}def hut size=4x4:\n  floor mat_slot=floor\n\nsite s:\n  \
             place id=home use=hut theme={reference} at=origin\n"
    )
}

fn placed_scope(r: &Resolution) -> &ScopeResolution {
    r.scopes.get("site::s::home").expect("place scope present")
}

/// No finding about variant selection, and no error of any kind.
///
/// Narrower than "no diagnostics at all", which these tests used to
/// assert: an advisory added later for an unrelated reason would break
/// them while saying nothing about the theme reference they are about.
fn nothing_said_about_variants(r: &Resolution) -> bool {
    !r.diagnostics.iter().any(|d| {
        d.severity() == Severity::Error
            || matches!(
                d.code,
                DiagnosticCode::ThemeVariantRebound | DiagnosticCode::ThemeVariantMissing
            )
    })
}

#[test]
fn refuses_a_module_whose_only_variant_is_for_the_other_edition() {
    let src = [
        "theme shop_bedrock:",
        "  slot floor -> @dark_oak_planks",
        "",
        "struct s size=4x4",
        "  floor mat_slot=floor",
        "",
    ]
    .join("\n");
    let r = resolve(&ir(&src), Some(Edition::Java));
    let diag = r
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::ThemeVariantMissing)
        .unwrap_or_else(|| panic!("expected the refusal, got {:?}", r.diagnostics));
    assert_eq!(diag.severity(), Severity::Error);
    assert!(
        diag.primary.contains("shop") && diag.primary.contains("java"),
        "the message must name the theme and the pin: {}",
        diag.primary,
    );
    let notes = diag
        .notes
        .iter()
        .map(|n| n.message.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        notes.contains("shop_bedrock"),
        "the notes must say which variants do exist: {notes}",
    );
    assert!(
        notes.contains("shop_java") && notes.contains("drop the suffix"),
        "the notes must give both fixes: {notes}",
    );
}

#[test]
fn a_module_variant_is_only_refused_once_however_many_scopes_read_it() {
    // The auto-pick is one decision for the whole module, so repeating
    // the finding per struct would report one cause N times.
    let src = [
        "theme shop_bedrock:",
        "  slot floor -> @dark_oak_planks",
        "",
        "struct a size=4x4",
        "  floor mat_slot=floor",
        "",
        "struct b size=4x4",
        "  floor mat_slot=floor",
        "",
    ]
    .join("\n");
    let r = resolve(&ir(&src), Some(Edition::Java));
    assert_eq!(
        r.diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ThemeVariantMissing)
            .count(),
        1,
    );
}

#[test]
fn an_unpinned_resolve_refuses_nothing_about_variants() {
    // The same module is fine for `cairn check` and `cairn lower`: with
    // no edition named there is nothing the variant fails to satisfy.
    let src = [
        "theme shop_bedrock:",
        "  slot floor -> @dark_oak_planks",
        "",
        "struct s size=4x4",
        "  floor mat_slot=floor",
        "",
    ]
    .join("\n");
    let r = resolve(&ir(&src), None);
    assert_eq!(
        r.scopes
            .get("struct::s")
            .expect("scope")
            .bound_theme
            .as_deref(),
        Some("shop_bedrock"),
    );
    assert!(
        !r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ThemeVariantMissing),
        "got {:?}",
        r.diagnostics,
    );
}

#[test]
fn a_place_naming_the_other_editions_variant_binds_this_editions() {
    // The defect: `theme=shop_bedrock` bound verbatim under a Java
    // build, so Bedrock-only slot values reached a Java `.nbt` while
    // `pick_variant` — the guard written to prevent exactly that — was
    // never consulted on this path.
    let src = placed_under("shop_bedrock", &["_java", "_bedrock"]);
    let r = resolve(&ir(&src), Some(Edition::Java));
    assert_eq!(placed_scope(&r).bound_theme.as_deref(), Some("shop_java"));
}

#[test]
fn a_rebound_place_theme_says_which_variant_it_bound() {
    let src = placed_under("shop_bedrock", &["_java", "_bedrock"]);
    let r = resolve(&ir(&src), Some(Edition::Java));
    let diag = r
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::ThemeVariantRebound)
        .unwrap_or_else(|| panic!("expected the notice, got {:?}", r.diagnostics));
    assert_eq!(diag.severity(), Severity::Warning);
    assert!(
        diag.primary.contains("shop_bedrock") && diag.primary.contains("shop_java"),
        "both the written and the bound name belong in the message: {}",
        diag.primary,
    );
    assert!(
        diag.notes.iter().any(|n| n.message.contains("theme=shop")),
        "the note must offer the neutral spelling: {:?}",
        diag.notes,
    );
}

#[test]
fn a_place_naming_this_editions_variant_binds_it_without_comment() {
    let src = placed_under("shop_bedrock", &["_java", "_bedrock"]);
    let r = resolve(&ir(&src), Some(Edition::Bedrock));
    assert_eq!(
        placed_scope(&r).bound_theme.as_deref(),
        Some("shop_bedrock")
    );
    assert!(nothing_said_about_variants(&r), "got {:?}", r.diagnostics);
}

#[test]
fn a_place_naming_the_logical_theme_binds_the_pinned_variant() {
    // The spelling `spec/versioning-editions` "Java / Bedrock
    // portability" asks for. Before the reference went through variant
    // selection it was the one spelling that did not resolve, because no
    // theme is named plain `shop`.
    let src = placed_under("shop", &["_java", "_bedrock"]);
    for (edition, expected) in [
        (Edition::Java, "shop_java"),
        (Edition::Bedrock, "shop_bedrock"),
    ] {
        let r = resolve(&ir(&src), Some(edition));
        assert_eq!(placed_scope(&r).bound_theme.as_deref(), Some(expected));
        assert!(nothing_said_about_variants(&r), "got {:?}", r.diagnostics);
    }
}

#[test]
fn a_place_naming_no_declared_theme_is_still_unresolved() {
    let src = placed_under("barn", &["_java", "_bedrock"]);
    let r = resolve(&ir(&src), Some(Edition::Java));
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnresolvedThemeRef),
        "a name no variant shares is a typo, not an edition problem: {:?}",
        r.diagnostics,
    );
}

#[test]
fn a_place_theme_binds_verbatim_without_a_pin() {
    // Nothing argues for another variant here, and re-picking would let
    // `cairn lower` silently swap the variant the author named.
    let src = placed_under("shop_bedrock", &["_java", "_bedrock"]);
    let r = resolve(&ir(&src), None);
    assert_eq!(
        placed_scope(&r).bound_theme.as_deref(),
        Some("shop_bedrock")
    );
    assert!(nothing_said_about_variants(&r), "got {:?}", r.diagnostics);
}

#[test]
fn a_place_is_refused_when_the_pin_has_no_variant_to_bind() {
    let src = placed_under("shop_bedrock", &["_bedrock"]);
    let r = resolve(&ir(&src), Some(Edition::Java));
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ThemeVariantMissing),
        "got {:?}",
        r.diagnostics,
    );
    assert!(
        r.scopes.get("site::s::home").is_none(),
        "a placement whose theme cannot bind must not become a scope",
    );
}

#[test]
fn an_unsuffixed_theme_binds_under_every_pin_and_says_nothing() {
    let src = placed_under("shop", &[""]);
    for edition in [None, Some(Edition::Java), Some(Edition::Bedrock)] {
        let r = resolve(&ir(&src), edition);
        assert_eq!(
            placed_scope(&r).bound_theme.as_deref(),
            Some("shop"),
            "edition {edition:?}",
        );
        assert!(
            nothing_said_about_variants(&r),
            "edition {edition:?}: {:?}",
            r.diagnostics,
        );
    }
}

/// A def whose member reads a slot only the Bedrock variant declares,
/// placed under `theme={reference}`.
///
/// The unrelated `barn` theme is load-bearing. A def is resolved twice —
/// once as its own top-level scope, once per placement — so with `shop`
/// as the module's only logical theme the def's own scope auto-picks a
/// variant and reports the slot itself. Every assertion below would then
/// hold whatever the placement path did with its siblings. A second
/// logical theme suppresses the auto-pick, leaving the `theme=` on the
/// `place` as the only thing that can bind this member at all.
fn placed_reading_a_bedrock_only_slot(reference: &str) -> String {
    format!(
        "theme shop_java:\n  slot floor -> @oak_planks\n\n\
             theme shop_bedrock:\n  slot floor -> @oak_planks\n  \
             slot bedrock_only -> @dark_oak_planks\n\n\
             theme barn:\n  slot floor -> @hay_block\n\n\
             def hut size=4x4:\n  floor mat_slot=bedrock_only\n\nsite s:\n  \
             place id=home use=hut theme={reference} at=origin\n"
    )
}

#[test]
fn a_logical_place_theme_is_softened_by_its_siblings_without_a_pin() {
    // Opening the site path to logical names re-opens the door the
    // sibling union exists to hold: with no edition picked, a slot only
    // one variant declares must not error, or a file that compiles
    // cleanly for Bedrock fails `cairn check`.
    let r = resolve(&ir(&placed_reading_a_bedrock_only_slot("shop")), None);
    // The logical name has to bind first, or "no unresolved slot" would
    // hold for the uninteresting reason that no scope was built.
    assert_eq!(placed_scope(&r).bound_theme.as_deref(), Some("shop_java"));
    assert!(
        !r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
        "got {:?}",
        r.diagnostics,
    );
}

#[test]
fn a_pin_makes_a_logical_place_theme_authoritative() {
    let r = resolve(
        &ir(&placed_reading_a_bedrock_only_slot("shop")),
        Some(Edition::Java),
    );
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
        "a pin binds one variant and its slots are the whole answer: {:?}",
        r.diagnostics,
    );
}

#[test]
fn naming_a_variant_explicitly_keeps_its_slots_strict_without_a_pin() {
    // The author asked about `shop_java`'s slots. Softening them
    // against a sibling would answer a question they did not ask.
    let r = resolve(&ir(&placed_reading_a_bedrock_only_slot("shop_java")), None);
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
        "got {:?}",
        r.diagnostics,
    );
}

#[test]
fn a_variant_the_module_does_not_declare_is_unresolved_without_a_pin() {
    // The spelling and the declaration set disagree, and no edition is
    // named to settle it. Re-picking a sibling here would swap a variant
    // on `cairn lower`'s say-so — the very thing the pinned path is
    // written to stop doing silently — so the answer is the one a
    // misspelled theme has always had.
    //
    // This shape was missing from the helpers entirely: every source
    // they built spelled a reference that the variant list contained.
    for (reference, variants) in [
        ("shop_java", ["_bedrock"].as_slice()),
        ("shop_bedrock", ["_java"].as_slice()),
        ("shop_java", [""].as_slice()),
    ] {
        let r = resolve(&ir(&placed_under(reference, variants)), None);
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::UnresolvedThemeRef),
            "theme={reference} against {variants:?}: {:?}",
            r.diagnostics,
        );
        assert!(
            r.scopes.get("site::s::home").is_none(),
            "theme={reference} against {variants:?}: no scope may be built",
        );
    }
}

#[test]
fn a_pin_falls_back_to_the_unsuffixed_theme_and_says_the_name_was_not_declared() {
    // `theme=shop_java` with only `theme shop:` declared. The pin has no
    // `_java` variant to prefer, so it binds the unsuffixed theme — the
    // rebind is real, but describing it as "names one edition's variant"
    // would be wrong: that variant is not declared anywhere.
    let src = placed_under("shop_java", &[""]);
    let r = resolve(&ir(&src), Some(Edition::Java));
    assert_eq!(placed_scope(&r).bound_theme.as_deref(), Some("shop"));
    let diag = r
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::ThemeVariantRebound)
        .unwrap_or_else(|| panic!("expected the notice, got {:?}", r.diagnostics));
    assert!(
        diag.primary.contains("is not a declared theme"),
        "the message must not call an undeclared name a variant: {}",
        diag.primary,
    );
}

#[test]
fn a_pin_prefers_the_unsuffixed_theme_over_the_other_editions_variant() {
    // The fallback order of `spec/versioning-editions` "Fail-loud and
    // minimum-version inference", exercised through the site path: with
    // `shop` and `shop_bedrock` declared, a Java build binds `shop` and
    // does not cross to `shop_bedrock`.
    let src = placed_under("shop", &["", "_bedrock"]);
    let r = resolve(&ir(&src), Some(Edition::Java));
    assert_eq!(placed_scope(&r).bound_theme.as_deref(), Some("shop"));
    let r = resolve(&ir(&src), Some(Edition::Bedrock));
    assert_eq!(
        placed_scope(&r).bound_theme.as_deref(),
        Some("shop_bedrock")
    );
}

#[test]
fn one_unbindable_theme_is_one_finding_however_many_places_read_it() {
    // The module-level pick and both placements ask for the same edit in
    // the same `theme` block, and the three messages were byte-identical
    // apart from their span.
    let src = concat!(
        "theme shop_bedrock:\n  slot floor -> @dark_oak_planks\n\n",
        "def hut size=4x4:\n  floor mat_slot=floor\n\n",
        "site s:\n",
        "  place id=a use=hut theme=shop at=origin\n",
        "  place id=b use=hut theme=shop east_of=a gap=2\n",
    );
    let r = resolve(&ir(src), Some(Edition::Java));
    assert_eq!(
        r.diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ThemeVariantMissing)
            .count(),
        1,
        "got {:?}",
        r.diagnostics,
    );
    // Deduplicating the sentence must not smuggle a placement into the
    // build: both are still refused.
    assert!(r.scopes.get("site::s::a").is_none());
    assert!(r.scopes.get("site::s::b").is_none());
}

#[test]
fn a_theme_no_member_reads_a_slot_from_is_not_refused_under_a_pin() {
    // Nothing is starved here — every material is concrete — so the pin
    // costs this module nothing and the build is byte-identical with or
    // without it. Refusing would fail a theme-library file, or any
    // module that does not use `mat_slot=`, on a `--edition` CI job.
    let src = [
        "theme shop_bedrock:",
        "  slot floor -> @dark_oak_planks",
        "",
        "struct s size=4x4",
        "  floor mat=@stone",
        "",
    ]
    .join("\n");
    let r = resolve(&ir(&src), Some(Edition::Java));
    assert!(
        !r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ThemeVariantMissing),
        "got {:?}",
        r.diagnostics,
    );

    // The same module with one `mat_slot=` is refused, so the gate is
    // the slot and not something else about the source.
    let reading = src.replace("floor mat=@stone", "floor mat_slot=floor");
    let r = resolve(&ir(&reading), Some(Edition::Java));
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ThemeVariantMissing),
        "got {:?}",
        r.diagnostics,
    );
}

/// The rename `E_INVALID_PLACE_ID` recommends must give an id the rest
/// of the language can name. A `connect` endpoint is a dotted reference
/// of lexer identifiers and may not be a string, so a recommended
/// spelling outside that rule (`north-tower`) could be placed but never
/// connected. The examples are read off the note itself, so changing
/// them re-runs this check on the new ones.
#[test]
fn the_ids_the_invalid_place_id_note_recommends_can_be_connected() {
    let def = "def hut size=5x5:\n  floor mat_slot=floor\n  walls mat_slot=floor height=3\n  \
                   door id=entry side=front at=center\n\ntheme t:\n  slot floor -> @oak_planks\n\n";
    let refused = resolve(
        &ir(&format!(
            "{def}site s:\n  place id=\"a/b\" use=hut theme=t at=origin\n"
        )),
        None,
    );
    let note = refused
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::InvalidPlaceId)
        .and_then(|d| d.notes.first())
        .map(|n| n.message.clone())
        .expect("`a/b` is refused with a note");
    let examples_at = note.rfind('(').expect("the note ends with its examples");
    let examples: Vec<&str> = note[examples_at..].split('`').skip(1).step_by(2).collect();
    assert!(!examples.is_empty(), "no examples in: {note}");

    for id in examples {
        let src = format!(
            "{def}site s:\n  place id=home use=hut theme=t at=origin\n  \
                 place id={id} use=hut theme=t east_of=home gap=4\n  \
                 connect home.entry to {id}.entry path=@gravel\n"
        );
        let module =
            parse(&src).unwrap_or_else(|e| panic!("the recommended id `{id}` does not parse: {e}"));
        let r = resolve(&lower(&module), None);
        let errors: Vec<_> = r
            .diagnostics
            .iter()
            .filter(|d| d.severity() == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "the recommended id `{id}` cannot be placed and connected: {errors:?}",
        );
    }
}

// ------------------------------------------------------------------
// What a `place` row's `use=` references, whatever refuses the row.
// ------------------------------------------------------------------

/// Every finding `resolve` reports for a site whose rows are `rows`, as
/// `(code, primary)` in report order. The module declares two defs,
/// `hut` and then `shed`, and only a row's `use=` references either.
fn unused_def_findings(rows: &[&str]) -> Vec<(&'static str, String)> {
    let mut src = String::from(
        "theme t:\n  slot floor -> @oak_planks\n\n\
             def hut size=3x3:\n  floor mat_slot=floor\n\n\
             def shed size=3x3:\n  floor mat_slot=floor\n\nsite v:\n",
    );
    for row in rows {
        src.push_str("  ");
        src.push_str(row);
        src.push('\n');
    }
    resolve(&ir(&src), None)
        .diagnostics
        .into_iter()
        .map(|d| (d.code.as_str(), d.primary))
        .collect()
}

/// A finding [`UNUSED_DEF_CASES`] expects: its code, and a fragment of
/// its primary message.
type Expected = (&'static str, &'static str);

const HUT_UNUSED: Expected = ("W_UNUSED_DEF", "def `hut` is never referenced");
const SHED_UNUSED: Expected = ("W_UNUSED_DEF", "def `shed` is never referenced");

/// The cases `a_def_named_by_a_refused_place_row_is_not_unused` runs:
/// a site's rows, and every finding `resolve` reports for them, in
/// report order.
const UNUSED_DEF_CASES: &[(&[&str], &[Expected])] = &[
    (
        &["place use=hut theme=t at=origin"],
        &[("E_INCOMPLETE_PLACE", "is missing `id=`"), SHED_UNUSED],
    ),
    (&["place id=3 use=hut theme=t at=origin"], &[SHED_UNUSED]),
    (
        &["place id=\"a.b\" use=hut theme=t at=origin"],
        &[("E_INVALID_PLACE_ID", "is not a usable id"), SHED_UNUSED],
    ),
    (
        &[
            "place id=a use=shed theme=t at=origin",
            "place id=a use=hut theme=t at=origin",
        ],
        &[("E_DUPLICATE_PLACE_ID", "duplicate `id=a`")],
    ),
    (
        &["place id=a use=hut theme=t"],
        &[
            ("E_INVALID_PLACE_ORIGIN", "missing an origin selector"),
            SHED_UNUSED,
        ],
    ),
    (
        &["place id=a use=hut theme=t at=origin east_of=nowhere"],
        &[
            ("E_INVALID_PLACE_ORIGIN", "more than one origin selector"),
            SHED_UNUSED,
        ],
    ),
    (
        &["place id=a use=hut theme=t at=front"],
        &[
            ("E_INVALID_PLACE_ORIGIN", "`at=` only accepts `origin`"),
            SHED_UNUSED,
        ],
    ),
    (
        &["place id=a use=hut theme=t east_of=3"],
        &[
            (
                "E_INVALID_PLACE_ORIGIN",
                "`east_of=` expects a place id label",
            ),
            SHED_UNUSED,
        ],
    ),
    (
        &["place id=a use=hut theme=t east_of=nowhere"],
        &[
            (
                "E_UNRESOLVED_PLACE_REF",
                "`east_of=nowhere` in site `v` does not name a prior place id",
            ),
            SHED_UNUSED,
        ],
    ),
    (
        &["place id=a use=hut theme=t north_of=nowhere"],
        &[
            (
                "E_UNRESOLVED_PLACE_REF",
                "`north_of=nowhere` in site `v` does not name a prior place id",
            ),
            SHED_UNUSED,
        ],
    ),
    (
        &[
            "place id=\"a.b\" use=shed theme=t at=origin",
            "place id=b use=hut theme=t east_of=\"a.b\"",
        ],
        &[
            ("E_INVALID_PLACE_ID", "is not a usable id"),
            ("W_DEFERRED_PLACE", "names a `place` row refused for its id"),
        ],
    ),
    (&["place id=a use=hut theme=t at=origin"], &[SHED_UNUSED]),
    (
        &["place id=a use=\"hut\" theme=t at=origin"],
        &[SHED_UNUSED],
    ),
    (
        &["place id=a use=hutt theme=t at=origin"],
        &[
            (
                "E_UNRESOLVED_PLACE_REF",
                "`use=hutt` references an unknown def",
            ),
            HUT_UNUSED,
            SHED_UNUSED,
        ],
    ),
    (
        &["place id=a use=3 theme=t at=origin"],
        &[HUT_UNUSED, SHED_UNUSED],
    ),
    (
        &["place id=a use=@hut theme=t at=origin"],
        &[HUT_UNUSED, SHED_UNUSED],
    ),
];

/// A def named by a `place` row the resolver refuses is referenced, so
/// `W_UNUSED_DEF` does not fire on it: the finding with the repair is
/// on the row, and removing the def, as the warning would advise, only
/// brings it back as `E_UNRESOLVED_PLACE_REF` once the row is fixed.
///
/// Every way out of `usable_place_id` and `validate_place_origin` that
/// refuses the row has a case. Out of `usable_place_id`: the
/// unnamed-placement return, reached both by a missing `id=` and by
/// `id=3` (not label-shaped, so never hoisted onto `Member::id`;
/// `check::type_mismatch` reports it, so `resolve` says nothing), an id
/// `PlaceId` refuses, and an id already placed. Out of
/// `validate_place_origin`: no origin selector, two, an `at=` other
/// than `origin`, a non-label `east_of=`, an `east_of=` and a
/// `north_of=` naming no prior place, and an `east_of=` naming a row
/// refused for its id, the one refusal that is a warning alone.
///
/// The whole list is compared, so each case also pins which def every
/// `W_UNUSED_DEF` names and, by a fragment of its message, which arm
/// each refusal came from. That is what tells the two
/// `E_UNRESOLVED_PLACE_REF`s apart: the origin lookup's "does not name
/// a prior place id" and the def lookup's "references an unknown def".
/// No row but the one under test names `hut`, and `shed`, where no row
/// names it, is the standing control that the check still runs.
///
/// The last five cases are controls. An accepted row references its
/// def, whether its `use=` is an identifier or a string. A `use=`
/// naming no def references nothing, so `hut` is still unused. A
/// `use=` that is neither an identifier nor a string names no def
/// either, since `as_label_str` reads nothing from it, so the warning
/// still fires rather than guess which def was meant: `use=3`, and
/// `use=@hut`, whose text spells the def. The value itself is
/// `check::type_mismatch`'s to report.
#[test]
fn a_def_named_by_a_refused_place_row_is_not_unused() {
    // Every case is run before anything is asserted, so one failure
    // does not hide the rest.
    let mismatches: Vec<String> = UNUSED_DEF_CASES
        .iter()
        .filter_map(|(rows, expected)| {
            let found = unused_def_findings(rows);
            let matches = found.len() == expected.len()
                && found
                    .iter()
                    .zip(expected.iter())
                    .all(|((code, primary), (want, fragment))| {
                        code == want && primary.contains(fragment)
                    });
            (!matches).then(|| format!("{rows:?}: expected {expected:?}, got {found:#?}"))
        })
        .collect();
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
