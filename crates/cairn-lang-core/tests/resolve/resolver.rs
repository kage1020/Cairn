//! `resolve` over whole sources: theme binding, member and selector resolution, `connect` rows.

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
    assert!(
        !r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
    );
}

#[test]
fn multi_theme_unbound_scope_gets_no_selector_extras() {
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
    let typo_start = src.find("a.entr").expect("typo present");
    let typo_end = typo_start + "a.entr".len();
    assert_eq!(diag.span.start, typo_start);
    assert_eq!(diag.span.end, typo_end);
    assert!(r.connects.is_empty(), "broken connect must not resolve");
}

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
    let bad_start = src.find("ghost.entry").expect("bad ref present");
    let bad_end = bad_start + "ghost.entry".len();
    assert_eq!(diag.span.start, bad_start);
    assert_eq!(diag.span.end, bad_end);
}

#[test]
fn connect_with_both_endpoints_unknown_emits_both_e_unresolved_place_ref() {
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

#[test]
fn unknown_slot_target_emits_error() {
    let src = "theme t:\n  slot wall -> plain_ident\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
    let r = resolve(&ir(src), None);
    assert!(r.diagnostics.iter().any(
            |d| d.code == DiagnosticCode::UnknownSlotTarget && d.severity() == Severity::Error
        ),);
}

// Per-edition theme fallback: `spec/versioning-editions` "Java / Bedrock portability".

fn per_edition_variants_src() -> String {
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
    // `spec/versioning-editions` "Fail-loud and minimum-version inference" asks for
    // `E_THEME_VARIANT_MISSING` here.
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

// A pin the module cannot satisfy, and a pin on the site path.

/// Theme `shop` in `variants`, and a site placing a def under `theme={reference}`.
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
    // No theme is named plain `shop`: `spec/versioning-editions` "Java / Bedrock portability".
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

/// A def reading a slot only `shop_bedrock` declares, placed under `theme={reference}`.
/// The `barn` theme stops the def's own scope auto-picking a `shop` variant.
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
    let r = resolve(&ir(&placed_reading_a_bedrock_only_slot("shop")), None);
    // Bound first, so "no unresolved slot" below is not vacuous.
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
    // `spec/versioning-editions` "Fail-loud and minimum-version inference", via the site path.
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
    assert!(r.scopes.get("site::s::a").is_none());
    assert!(r.scopes.get("site::s::b").is_none());
}

#[test]
fn a_theme_no_member_reads_a_slot_from_is_not_refused_under_a_pin() {
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

    // Control: the same module with one `mat_slot=` is refused.
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

// What a `place` row's `use=` references, whatever refuses the row.

/// `(code, primary)` of every finding for a site of `rows` over defs `hut` and `shed`.
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

/// A finding's code and a fragment of its primary message.
type Expected = (&'static str, &'static str);

const HUT_UNUSED: Expected = ("W_UNUSED_DEF", "def `hut` is never referenced");
const SHED_UNUSED: Expected = ("W_UNUSED_DEF", "def `shed` is never referenced");

/// A site's rows, and every finding `resolve` reports for them.
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

/// The last five cases are controls; `id=3` and `use=3` are `check::type_mismatch`'s to report.
#[test]
fn a_def_named_by_a_refused_place_row_is_not_unused() {
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
