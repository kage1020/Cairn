use super::anchor::{PlaceAnchor, PlacementCorner, PlacementOutOfRange};
use super::canvas::{Canvas, Overflow, ScopePalette, prune_unreferenced};
use super::connect::edge_pushers;
use super::phase::Phase;
use super::plate::{PRESSURE_PLATE_BASE_ID, PRESSURE_PLATE_TOKEN};
use super::roof_fill::{GeometryMemberDescription, geometry_material_id};
use super::site_floor::FloorExtent;
use super::*;
use crate::ast::ValueKind;
use crate::block_array::material::{NoTarget, resolve_block_state, stand_in};
use crate::block_array::roof::is_stair;
use crate::block_array::walkway::{ROUTE_AREA_CAP, SearchRect, l_path_area};
use crate::block_array::wall_column::WallColumn;
use crate::block_array::{BlockState, Dims, Footprint, Palette, PaletteFull, PaletteIndex};
use crate::diagnostic::{DiagnosticCode, DiagnosticData, Severity};
use crate::intent::ValueWithSpan;

// `size=` is a `NonZeroU32`, so only a hand-built zero extent reaches the assertion.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "a placed body has a zero extent")]
fn a_zero_extent_is_not_measured_for_its_far_edge() {
    let _ = PlaceAnchor::WorldOrigin.origin(Dims { x: 0, y: 1, z: 1 });
}

#[test]
fn a_placement_whose_far_edge_leaves_i32_is_refused_on_either_axis() {
    // No source reaches the `z` case (`gap=-1` is `E_PARSE`), so the anchor is built directly.
    let dims = Dims { x: 5, y: 1, z: 5 };
    let north = |prior_origin, gap| PlaceAnchor::NorthOf { prior_origin, gap };
    assert_eq!(
        north((i32::MAX - 4, 0, i32::MAX), -1).origin(dims),
        Ok((i32::MAX - 4, 0, i32::MAX - 4)),
    );
    assert_eq!(
        north((i32::MAX - 3, 0, 0), 0).origin(dims),
        Err(PlacementOutOfRange {
            corner: PlacementCorner::FarEdge,
            axis: 'x',
            coord: i128::from(i32::MAX) + 1,
        }),
    );
    let z_past = north((0, 0, i32::MAX), -2).origin(dims);
    assert_eq!(
        z_past,
        Err(PlacementOutOfRange {
            corner: PlacementCorner::FarEdge,
            axis: 'z',
            coord: i128::from(i32::MAX) + 1,
        }),
    );
    assert_eq!(
        z_past.map_err(PlacementOutOfRange::deferral),
        Err(
            "this placement's body reaches z=2147483648, past the -2147483648 to \
             2147483647 range a placement's cells are addressed in; shrink the body with \
             its `def`'s `size=` or a roof's `overhang=`, or shorten the `gap=` on this row \
             or on a row it is placed relative to"
                .to_owned()
        ),
    );
}

/// One placed row of `width` stone cells, one deep and one tall, at `origin`.
fn one_placed_row(
    origin: (i32, i32, i32),
    width: u32,
) -> (IndexMap<String, BlockArray>, IndexMap<String, PlacedBody>) {
    let key = "site::s::b".to_owned();
    let dims = Dims {
        x: width,
        y: 1,
        z: 1,
    };
    let mut palette = Palette::new_with_air();
    let stone = palette.intern(BlockState::bare("minecraft:stone"));
    let mut structures = IndexMap::new();
    structures.insert(
        key.clone(),
        BlockArray {
            dims,
            palette,
            voxels: vec![stone; width as usize],
            block_entities: Vec::new(),
            entities: Vec::new(),
            source_scope: key.clone(),
        },
    );
    let mut placed = IndexMap::new();
    placed.insert(
        key,
        PlacedBody {
            placement: Placement {
                site: SiteName::new("s").expect("a site name"),
                place_id: PlaceId::new("b").expect("a place id"),
                source_def: "hut".to_owned(),
                theme: "t".to_owned(),
                origin,
                dims,
            },
            walls: WallColumn::default(),
            cut: HashSet::new(),
        },
    );
    (structures, placed)
}

#[test]
fn a_body_ending_on_the_i32_edge_lays_every_cell_once() {
    let (structures, placed) = one_placed_row((i32::MAX - 2, 0, 0), 3);
    let plan = collect_floor_cells(&structures, &placed);
    assert_eq!(
        plan.cells,
        HashSet::from([(i32::MAX - 2, 0, 0), (i32::MAX - 1, 0, 0), (i32::MAX, 0, 0)]),
    );
    assert!(
        plan.owners.values().all(|owners| owners.len() == 1),
        "{:?}",
        plan.owners,
    );
    assert_eq!(plan.extents["site::s::b"].max_x, i32::MAX);
}

// No source reaches this: the far-edge refusal stops the row first.
#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "has no world coordinate"))]
fn a_body_past_the_i32_edge_folds_nothing_onto_it() {
    let (structures, placed) = one_placed_row((i32::MAX - 1, 0, 0), 3);
    let plan = collect_floor_cells(&structures, &placed);
    assert_eq!(
        plan.cells,
        HashSet::from([(i32::MAX - 1, 0, 0), (i32::MAX, 0, 0)]),
    );
    assert!(
        plan.owners.values().all(|owners| owners.len() == 1),
        "{:?}",
        plan.owners,
    );
    assert_eq!(plan.extents["site::s::b"].max_x, i32::MAX);
}

// `lower_site` produces neither case, so each is built by hand.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "has no structure")]
fn a_placement_with_no_structure_is_not_a_silent_skip() {
    let (_, placed) = one_placed_row((0, 0, 0), 3);
    let _ = collect_floor_cells(&IndexMap::new(), &placed);
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "is outside")]
fn a_body_with_no_row_0_is_not_a_silent_skip() {
    let (mut structures, placed) = one_placed_row((0, 0, 0), 3);
    structures["site::s::b"].dims.y = 0;
    let _ = collect_floor_cells(&structures, &placed);
}

#[test]
fn the_redirectable_plate_id_is_not_in_that_list() {
    assert!(
        !BUILTIN_BLOCK_IDS.contains(&PRESSURE_PLATE_BASE_ID),
        "BUILTIN_BLOCK_IDS promises every target declares its entries, and no Bedrock \
         target declares `{PRESSURE_PLATE_BASE_ID}`; the plate is covered by the pack \
         tests around `{PRESSURE_PLATE_TOKEN}` instead",
    );
}
use crate::{lower, parse, resolve};

fn lowered(source: &str) -> BlockArrayIr {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, None)
}

/// [`lowered`] with each body's palette holding `palette_capacity` entries.
fn lowered_within(source: &str, palette_capacity: usize) -> BlockArrayIr {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array_within(&ir, &resolution, None, palette_capacity)
}

/// A 1x1 `body` with one `walls` row per slot of `theme t`, each overwriting the same ring cell
/// with its own state.
fn overwriting_walls_in(body: &str, states: usize) -> String {
    use std::fmt::Write as _;

    let mut source = String::from("theme t:\n");
    for i in 0..states {
        writeln!(source, "  slot s{i} -> @b{i}").expect("writing to a String");
    }
    writeln!(source, "\n{body}").expect("writing to a String");
    for i in 0..states {
        writeln!(source, "  walls mat_slot=s{i} height=1").expect("writing to a String");
    }
    source
}

/// [`overwriting_walls_in`] a `struct s`.
fn overwriting_walls(states: usize) -> String {
    overwriting_walls_in("struct s size=1x1", states)
}

/// [`overwriting_walls_in`] a `def d`, placed once as `a`.
fn placed_overwriting_walls(states: usize) -> String {
    let mut source = overwriting_walls_in("def d size=1x1:", states);
    source.push_str("\nsite v:\n  place id=a use=d theme=t at=origin\n");
    source
}

/// `theme t` binding slot `$slot` to `@$block` (default `@cobblestone`), then `$rest`.
macro_rules! theme_t {
    ($slot:literal, $rest:literal $(,)?) => {
        theme_t!($slot -> "cobblestone", $rest)
    };
    ($slot:literal -> $block:literal, $rest:literal $(,)?) => {
        concat!("theme t:\n  slot ", $slot, " -> @", $block, "\n", $rest)
    };
}

fn findings(ir: &BlockArrayIr, code: DiagnosticCode) -> Vec<&Diagnostic> {
    ir.diagnostics.iter().filter(|d| d.code == code).collect()
}

/// The `W_DEFERRED_MEMBER` findings in `out`.
fn deferrals(out: &BlockArrayIr) -> Vec<&Diagnostic> {
    findings(out, DiagnosticCode::DeferredMember)
}

fn deferred_count(out: &BlockArrayIr) -> usize {
    deferrals(out).len()
}

/// The `E_INCOMPATIBLE_MATERIAL` findings in `out`.
fn refusals(out: &BlockArrayIr) -> Vec<&Diagnostic> {
    findings(out, DiagnosticCode::IncompatibleMaterial)
}

fn primaries_of(out: &BlockArrayIr, code: DiagnosticCode) -> Vec<&str> {
    findings(out, code)
        .into_iter()
        .map(|d| d.primary.as_str())
        .collect()
}

/// Each `W_DEFERRED_MEMBER` in `out`, as the text of `src` its span covers and its primary.
fn deferred_lines<'a>(src: &'a str, out: &'a BlockArrayIr) -> Vec<(&'a str, &'a str)> {
    deferrals(out)
        .into_iter()
        .map(|d| (&src[d.span.start..d.span.end], d.primary.as_str()))
        .collect()
}

/// Each `E_INVALID_MEMBER` in `out`, as the text of `src` its span covers and its primary.
fn invalid_lines<'a>(src: &'a str, out: &'a BlockArrayIr) -> Vec<(&'a str, &'a str)> {
    findings(out, DiagnosticCode::InvalidMember)
        .into_iter()
        .map(|d| (&src[d.span.start..d.span.end], d.primary.as_str()))
        .collect()
}

/// Asserts the one finding on `src` is the `E_INVALID_MEMBER` on `line` saying `primary`, with
/// `notes` and nothing else under it.
#[track_caller]
fn assert_refused_alone(src: &str, line: &str, primary: &str, notes: &[&str]) {
    let out = lowered(src);
    assert_eq!(
        out.diagnostics
            .iter()
            .map(|d| (
                d.code,
                &src[d.span.start..d.span.end],
                d.primary.as_str(),
                d.notes
                    .iter()
                    .map(|n| n.message.as_str())
                    .collect::<Vec<_>>(),
            ))
            .collect::<Vec<_>>(),
        [(DiagnosticCode::InvalidMember, line, primary, notes.to_vec())],
    );
}

/// Asserts a `W_DEFERRED_MEMBER` on the member written as `line`, whose primary has `needle`.
#[track_caller]
fn assert_defers_on(src: &str, out: &BlockArrayIr, line: &str, needle: &str) {
    let found = deferred_lines(src, out);
    assert!(
        found
            .iter()
            .any(|&(at, primary)| at == line && primary.contains(needle)),
        "expected a deferral on `{line}` saying `{needle}`, got {found:#?}\nall findings: {:#?}",
        out.diagnostics,
    );
}

/// The prose properties `tests/check/diagnostic_text.rs` holds its corpus to.
fn assert_reads_as_prose(origin: &str, text: &str) {
    assert!(
        !text.contains("  "),
        "{origin} renders a run of spaces, which is a dropped `\\` line \
         continuation in the literal: {text:?}",
    );
    assert!(
        !text.contains('\n') && !text.contains('\t'),
        "{origin} embeds its own line break: {text:?}",
    );
    assert!(!text.trim().is_empty(), "{origin} renders nothing");
    assert_eq!(text.trim(), text, "{origin} has leading or trailing space");
}

#[test]
fn a_body_painting_more_states_than_its_palette_holds_is_refused() {
    let fits = lowered_within(&overwriting_walls(3), 4);
    assert!(
        findings(&fits, DiagnosticCode::PaletteTooLarge).is_empty(),
        "three states fit: {:#?}",
        fits.diagnostics,
    );
    let array = fits
        .structures
        .get("struct::s")
        .expect("three states build");
    assert_eq!(
        array.palette.entries.len(),
        2,
        "air and the last wall's state: the covered ones are pruned after painting",
    );
    assert!(
        !findings(&fits, DiagnosticCode::PhaseConflict).is_empty(),
        "a built body reports the walls that overwrite one another: {:#?}",
        fits.diagnostics,
    );

    let over = lowered_within(&overwriting_walls(4), 4);
    let refused = findings(&over, DiagnosticCode::PaletteTooLarge);
    assert_eq!(refused.len(), 1, "{:#?}", over.diagnostics);
    assert_eq!(refused[0].severity(), Severity::Warning);
    assert!(
        refused[0]
            .primary
            .starts_with("`s` paints more than 3 distinct non-air block states"),
        "{}",
        refused[0].primary,
    );
    assert!(
        !over.structures.contains_key("struct::s"),
        "the refused body must not reach a writer",
    );
    assert_eq!(
        findings(&over, DiagnosticCode::PhaseConflict),
        Vec::<&Diagnostic>::new(),
        "a refused body is not built, so it has no conflict to report",
    );
}

#[test]
fn the_palette_refusal_renders_as_prose() {
    let over = lowered_within(&overwriting_walls(4), 4);
    let refused = findings(&over, DiagnosticCode::PaletteTooLarge);
    assert_eq!(refused.len(), 1, "{:#?}", over.diagnostics);
    assert_reads_as_prose("W_PALETTE_TOO_LARGE primary", &refused[0].primary);
    assert_eq!(refused[0].notes.len(), 1, "{:#?}", refused[0].notes);
    assert_reads_as_prose("W_PALETTE_TOO_LARGE note 0", &refused[0].notes[0].message);
}

#[test]
fn a_place_whose_def_paints_more_states_than_its_palette_holds_is_refused() {
    let fits = lowered_within(&placed_overwriting_walls(3), 4);
    assert!(
        findings(&fits, DiagnosticCode::PaletteTooLarge).is_empty(),
        "three states fit: {:#?}",
        fits.diagnostics,
    );
    assert!(
        fits.placements.contains_key("site::v::a"),
        "{:?}",
        fits.placements.keys(),
    );

    let source = placed_overwriting_walls(4);
    let over = lowered_within(&source, 4);
    let refused = findings(&over, DiagnosticCode::PaletteTooLarge);
    assert_eq!(refused.len(), 1, "{:#?}", over.diagnostics);
    assert!(
        refused[0]
            .primary
            .starts_with("`a` paints more than 3 distinct non-air block states"),
        "{}",
        refused[0].primary,
    );
    assert!(
        source[refused[0].span.clone()].starts_with("place id=a"),
        "anchored on the row, not the def: {:?}",
        &source[refused[0].span.clone()],
    );
    assert!(over.structures.is_empty(), "{:?}", over.structures.keys());
    assert!(over.placements.is_empty(), "{:?}", over.placements.keys());
    assert_eq!(
        findings(&over, DiagnosticCode::PhaseConflict),
        Vec::<&Diagnostic>::new(),
    );
}

#[test]
fn without_a_target_the_palette_counts_no_stand_in() {
    /// Each finding as its code and the first line it covers.
    fn without_target(source: &str) -> Vec<(&'static str, &str)> {
        let ir = lower(&parse(source).expect("parse"));
        let registry = NoTarget {
            declared: &|_| true,
        };
        let out = lower_to_block_array_within(&ir, &resolve(&ir, None), Some(&registry), 4);
        out.diagnostics
            .iter()
            .map(|d| {
                let covered = source[d.span.clone()].lines().next().unwrap_or_default();
                (d.code.as_str(), covered)
            })
            .collect()
    }
    let refused = [("W_PALETTE_TOO_LARGE", "struct s size=1x1")];
    let canonical = overwriting_walls(4);
    assert_eq!(without_target(&canonical), refused);
    // The pack may lift the token to one of the other three blocks.
    let one_stand_in = canonical.replacen("@b0", "@wall.stone.cobble", 1);
    assert_eq!(
        without_target(&one_stand_in),
        [
            ("W_PHASE_CONFLICT", "walls mat_slot=s2 height=1"),
            ("W_PHASE_CONFLICT", "walls mat_slot=s3 height=1"),
        ],
    );
    let four_others = overwriting_walls(5).replacen("@b0", "@wall.stone.cobble", 1);
    assert_eq!(without_target(&four_others), refused);
}

#[test]
fn a_scope_palette_gives_every_stand_in_one_uncounted_slot() {
    let mut palette = ScopePalette::with_capacity(3);
    assert_eq!(palette.intern(stand_in("x")), PaletteIndex(1));
    assert_eq!(palette.intern(BlockState::bare("test:a")), PaletteIndex(2));
    assert_eq!(palette.intern(stand_in("y")), PaletteIndex(1));
    assert_eq!(palette.intern(BlockState::bare("test:b")), PaletteIndex(3));
    assert_eq!(palette.stand_in(), Some(PaletteIndex(1)));
    let entries = |palette: ScopePalette| palette.into_palette().map(|palette| palette.entries);
    let mut full = ScopePalette::with_capacity(3);
    for state in ["test:a", "test:b"] {
        full.intern(BlockState::bare(state));
    }
    assert_eq!(full.intern(stand_in("x")), PaletteIndex(3));
    assert_eq!(full.intern(BlockState::bare("test:c")), PaletteIndex::AIR);
    assert_eq!(
        entries(palette),
        Ok(vec![
            BlockState::air(),
            stand_in("x"),
            BlockState::bare("test:a"),
            BlockState::bare("test:b"),
        ]),
    );
    assert_eq!(
        entries(full),
        Err(Overflow::Full(PaletteFull { capacity: 3 })),
    );
}

#[test]
fn a_scope_palette_looks_nothing_up_once_it_has_overflowed() {
    let mut palette = ScopePalette::with_capacity(2);
    let kept = BlockState::bare("test:kept");
    assert_eq!(palette.intern(kept.clone()), PaletteIndex(1));
    assert_eq!(
        palette.intern(BlockState::bare("test:refused")),
        PaletteIndex::AIR,
    );
    assert_eq!(
        palette.intern(kept),
        PaletteIndex::AIR,
        "a state the palette holds is not looked up after the overflow",
    );
    assert_eq!(
        palette.into_palette(),
        Err(Overflow::Full(PaletteFull { capacity: 2 })),
        "the refusal kept is the one at the capacity in force",
    );
}

fn lowered_with_resolver(source: &str, resolver: &dyn TargetRegistry) -> BlockArrayIr {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, Some(resolver))
}

/// In-memory registry with no id table, so no target is pinned and no id refused.
struct FakeResolver {
    entries: Vec<(&'static str, &'static str)>,
}

impl TargetRegistry for FakeResolver {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        self.entries
            .iter()
            .find(|(t, _)| *t == token)
            .map(|(_, id)| BlockState::bare(format!("minecraft:{id}")))
    }

    fn known_tokens(&self) -> Vec<String> {
        self.entries.iter().map(|(t, _)| (*t).to_owned()).collect()
    }

    fn block_ids(&self) -> Option<crate::block_array::BlockIdSet<'_>> {
        None
    }
}

/// A registry whose tokens carry blockstates, unlike [`FakeResolver`]'s bare ones.
struct StatefulResolver {
    token: &'static str,
    id: &'static str,
    properties: Vec<(&'static str, &'static str)>,
}

impl TargetRegistry for StatefulResolver {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        (token == self.token).then(|| BlockState {
            id: format!("minecraft:{}", self.id),
            properties: self
                .properties
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        })
    }

    fn known_tokens(&self) -> Vec<String> {
        vec![self.token.to_owned()]
    }

    fn block_ids(&self) -> Option<crate::block_array::BlockIdSet<'_>> {
        None
    }
}

/// A registry with a pinned target, for the `E_UNKNOWN_ID` path.
struct PinnedRegistry {
    entries: Vec<(&'static str, &'static str)>,
    /// Sorted, fully namespaced ids the pinned target declares.
    ids: Vec<String>,
    /// One alias group, in the order a pack wrote it; empty for no `aliases` component.
    group: Vec<String>,
}

impl PinnedRegistry {
    fn new(entries: Vec<(&'static str, &'static str)>, ids: &[&str]) -> Self {
        let mut ids: Vec<String> = ids.iter().map(|id| (*id).to_owned()).collect();
        ids.sort();
        Self {
            entries,
            ids,
            group: Vec::new(),
        }
    }

    /// Declare one alias group over the same ids.
    fn aliasing(mut self, group: &[&str]) -> Self {
        self.group = group.iter().map(|id| (*id).to_owned()).collect();
        self
    }
}

impl TargetRegistry for PinnedRegistry {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        self.entries
            .iter()
            .find(|(t, _)| *t == token)
            .map(|(_, id)| BlockState::bare(format!("minecraft:{id}")))
    }

    fn known_tokens(&self) -> Vec<String> {
        self.entries.iter().map(|(t, _)| (*t).to_owned()).collect()
    }

    fn block_ids(&self) -> Option<crate::block_array::BlockIdSet<'_>> {
        Some(crate::block_array::BlockIdSet::new("test 1.0", &self.ids))
    }

    fn aliases_for(&self, id: &str) -> Vec<String> {
        if !self.group.iter().any(|spelling| spelling == id) {
            return Vec::new();
        }
        self.group
            .iter()
            .filter(|spelling| self.ids.binary_search(spelling).is_ok())
            .cloned()
            .collect()
    }
}

struct UnknownIdPayload {
    id: String,
    registry: String,
    origin: String,
    token: Option<String>,
    suggestion: Option<String>,
    aliases: Vec<String>,
}

fn unknown_id_payload(out: &BlockArrayIr) -> UnknownIdPayload {
    let found = findings(out, DiagnosticCode::UnknownId);
    assert_eq!(
        found.len(),
        1,
        "expected exactly one E_UNKNOWN_ID, got {:?}",
        out.diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
    );
    match found[0].data.clone() {
        Some(DiagnosticData::UnknownId {
            id,
            registry,
            origin,
            token,
            suggestion,
            aliases,
        }) => UnknownIdPayload {
            id,
            registry,
            origin,
            token,
            suggestion,
            aliases,
        },
        other => panic!("expected an UnknownId payload, got {other:?}"),
    }
}

// A payload hardcoding `None` would pass the authored half alone.
#[test]
fn the_payload_names_the_token_only_when_the_catalog_chose_the_id() {
    let registry = PinnedRegistry::new(
        vec![("floor.stone.smooth", "stone_bricks")],
        &["minecraft:stonebrick"],
    );

    let authored = lowered_with_resolver(
        "theme t:\n  slot floor -> @stone_brick\nstruct s size=2x2\n  floor mat_slot=floor\n",
        &registry,
    );
    let payload = unknown_id_payload(&authored);
    assert_eq!(payload.id, "minecraft:stone_brick");
    assert_eq!(payload.registry, "test 1.0");
    assert_eq!(payload.origin, "authored");
    assert_eq!(payload.token, None, "the author wrote this id themselves");
    assert_eq!(payload.suggestion.as_deref(), Some("minecraft:stonebrick"));

    let from_catalog = lowered_with_resolver(
        "theme t:\n  slot floor -> @floor.stone.smooth\nstruct s size=2x2\n  floor mat_slot=floor\n",
        &registry,
    );
    let payload = unknown_id_payload(&from_catalog);
    assert_eq!(payload.id, "minecraft:stone_bricks");
    assert_eq!(payload.origin, "catalog");
    assert_eq!(
        payload.token.as_deref(),
        Some("floor.stone.smooth"),
        "the catalog produced this id, so the author's token is not the fix site",
    );
}

#[test]
fn a_member_default_from_the_pack_is_checked_like_any_other_id() {
    let plate = "struct s size=3x3\n  pressure_plate id=p at=inside.front offset=1\n";

    let mapped_wrong = PinnedRegistry::new(
        vec![(PRESSURE_PLATE_TOKEN, "oak_pressure_plate")],
        &["minecraft:wooden_pressure_plate"],
    );
    let out = lowered_with_resolver(plate, &mapped_wrong);
    let payload = unknown_id_payload(&out);
    assert_eq!(payload.id, "minecraft:oak_pressure_plate");
    assert_eq!(payload.origin, "catalog");
    assert_eq!(payload.token.as_deref(), Some(PRESSURE_PLATE_TOKEN));

    let no_row = PinnedRegistry::new(vec![], &["minecraft:wooden_pressure_plate"]);
    let out = lowered_with_resolver(plate, &no_row);
    let payload = unknown_id_payload(&out);
    assert_eq!(
        payload.id, PRESSURE_PLATE_BASE_ID,
        "a pack with no row sends lowering to the compiled-in default",
    );
    assert_eq!(payload.origin, "builtin");
    assert_eq!(payload.token.as_deref(), Some(PRESSURE_PLATE_TOKEN));

    let right = PinnedRegistry::new(
        vec![(PRESSURE_PLATE_TOKEN, "wooden_pressure_plate")],
        &["minecraft:wooden_pressure_plate"],
    );
    let out = lowered_with_resolver(plate, &right);
    assert!(
        findings(&out, DiagnosticCode::UnknownId).is_empty(),
        "a pack that spells it the way the target does must not be refused: {:?}",
        out.diagnostics
            .iter()
            .map(|d| d.primary.as_str())
            .collect::<Vec<_>>(),
    );
}

#[test]
fn a_bound_plate_whose_default_the_target_lacks_says_its_signal_still_reaches_the_netlist() {
    let no_row = PinnedRegistry::new(vec![], &["minecraft:wooden_pressure_plate"]);
    let plate = "struct s size=3x3\n  pressure_plate id=p at=inside.front offset=1\n";
    let unbound = findings_of(&lowered_with_resolver(plate, &no_row));
    let bound = findings_of(&lowered_with_resolver(
        &plate.replace("offset=1", "offset=1 -> sig.a"),
        &no_row,
    ));
    assert_eq!(
        unbound.iter().map(|f| f.0).collect::<Vec<_>>(),
        [DiagnosticCode::UnknownId],
    );
    assert!(!unbound[0].2.iter().any(|n| n == BINDING_STILL_READ));
    let mut expected = unbound;
    expected[0].2.push(BINDING_STILL_READ.to_owned());
    assert_eq!(bound, expected);
}

#[test]
fn a_rename_and_a_typo_read_as_different_claims() {
    let renamed = PinnedRegistry::new(vec![], &["minecraft:standing_sign"])
        .aliasing(&["minecraft:oak_sign", "minecraft:standing_sign"]);
    let out = lowered_with_resolver(
        "theme t:\n  slot floor -> @oak_sign\nstruct s size=2x2\n  floor mat_slot=floor\n",
        &renamed,
    );
    let payload = unknown_id_payload(&out);
    assert_eq!(payload.aliases, ["minecraft:standing_sign".to_owned()]);
    let note = unknown_id_note(&out);
    assert!(
        note.contains("spells this block `minecraft:standing_sign`")
            && note.contains("alias table"),
        "the note must name the alias table it is quoting, got: {note}",
    );

    let typo = PinnedRegistry::new(vec![], &["minecraft:oak_planks"]);
    let out = lowered_with_resolver(
        "theme t:\n  slot floor -> @oak_plank\nstruct s size=2x2\n  floor mat_slot=floor\n",
        &typo,
    );
    let note = unknown_id_note(&out);
    assert!(
        note.contains("spells the nearest block") && !note.contains("alias table"),
        "a distance guess must not be dressed up as the pack's word, got: {note}",
    );
}

#[test]
fn a_split_is_summarised_in_the_note_and_kept_whole_in_the_payload() {
    let ids: Vec<String> = (0..16)
        .map(|i| format!("minecraft:light_block_{i}"))
        .collect();
    let mut group = vec!["minecraft:light".to_owned()];
    group.extend(ids.iter().cloned());
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let group_refs: Vec<&str> = group.iter().map(String::as_str).collect();
    let registry = PinnedRegistry::new(vec![], &refs).aliasing(&group_refs);
    let out = lowered_with_resolver(
        "theme t:\n  slot floor -> @light\nstruct s size=2x2\n  floor mat_slot=floor\n",
        &registry,
    );
    let payload = unknown_id_payload(&out);
    assert_eq!(payload.aliases.len(), 16, "every candidate reaches a tool");
    let note = unknown_id_note(&out);
    assert!(
        note.contains("and 12 more"),
        "the sentence counts what it does not list, got: {note}",
    );
}

/// The `E_UNKNOWN_ID` finding's candidate note, its last.
fn unknown_id_note(out: &BlockArrayIr) -> String {
    findings(out, DiagnosticCode::UnknownId)
        .first()
        .expect("an E_UNKNOWN_ID finding")
        .notes
        .last()
        .expect("the candidate note")
        .message
        .clone()
}

fn block_id(ba: &BlockArray, x: u32, y: u32, z: u32) -> &str {
    let i = ba.dims.index(x, y, z).expect("in-range coordinate");
    let pi = ba.voxels[i];
    ba.palette.entries[usize::from(pi.0)].id.as_str()
}

#[test]
fn floor_only_fills_y_zero_plane() {
    let src = theme_t!("f", "\nstruct s size=3x3\n  floor mat_slot=f\n");
    let out = lowered(src);
    let ba = out.structures.get("struct::s").expect("structure lowered");
    assert_eq!(ba.dims, Dims { x: 3, y: 1, z: 3 });
    for z in 0..3 {
        for x in 0..3 {
            assert_eq!(block_id(ba, x, 0, z), "minecraft:cobblestone");
        }
    }
    assert!(
        out.diagnostics.is_empty(),
        "no diagnostics expected, got {:?}",
        out.diagnostics,
    );
}

#[test]
fn walls_only_fills_outline_above_floor() {
    let src = theme_t!("w", "\nstruct s size=3x3\n  walls mat_slot=w height=2\n");
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims, Dims { x: 3, y: 3, z: 3 });
    for z in 0..3 {
        for x in 0..3 {
            assert_eq!(block_id(ba, x, 0, z), BlockState::AIR_ID);
        }
    }
    for y in 1..=2 {
        assert_eq!(block_id(ba, 1, y, 1), BlockState::AIR_ID, "centre at y={y}");
        for z in 0..3 {
            for x in 0..3 {
                let on_edge = x == 0 || x == 2 || z == 0 || z == 2;
                let expected = if on_edge {
                    "minecraft:cobblestone"
                } else {
                    BlockState::AIR_ID
                };
                assert_eq!(block_id(ba, x, y, z), expected, "({x},{y},{z})");
            }
        }
    }
}

#[test]
fn walls_one_row_tall_build_and_walls_with_no_positive_height_are_refused() {
    let out = lowered(theme_t!(
        "w",
        "\nstruct s size=3x3\n  walls mat_slot=w height=1\n"
    ));
    assert_eq!(out.diagnostics, []);
    assert_eq!(out.structures["struct::s"].dims, Dims { x: 3, y: 2, z: 3 });
    let form = "write `height=N`, where `N` is a positive integer: the walls fill the `N` rows \
                above the base row";
    for (walls, primary) in [
        (
            "walls mat_slot=w height=0",
            "walls `height=` must be a positive integer that fits in u32, not integer `0`",
        ),
        (
            "walls mat_slot=w height=\"3\"",
            "walls `height=` must be a positive integer that fits in u32, not string `\"3\"`",
        ),
        (
            "walls mat_slot=w",
            "walls has no `height=`, so nothing says how many rows it rises",
        ),
    ] {
        let src = format!("{}\nstruct s size=3x3\n  {walls}\n", theme_t!("w", ""));
        assert_refused_alone(&src, walls, primary, &[form]);
    }
}

#[test]
fn floor_and_walls_combine() {
    let src = "theme t:\n  slot f -> @oak_planks\n  slot w -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=f\n  walls mat_slot=w height=2\n";
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims, Dims { x: 3, y: 3, z: 3 });
    for z in 0..3 {
        for x in 0..3 {
            assert_eq!(block_id(ba, x, 0, z), "minecraft:oak_planks");
        }
    }
    assert_eq!(block_id(ba, 0, 1, 0), "minecraft:cobblestone");
    assert_eq!(block_id(ba, 1, 1, 1), BlockState::AIR_ID);
}

#[test]
fn a_stair_without_kind_is_refused_without_touching_a_voxel() {
    let src = theme_t!(
        "f",
        "\nstruct s size=3x3\n  floor mat_slot=f\n  stair side=front\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(
        invalid_lines(src, &out),
        [(
            "stair side=front",
            "stair has no `kind=`, so nothing says what it builds"
        )],
    );
    for z in 0..3 {
        for x in 0..3 {
            assert_eq!(block_id(ba, x, 0, z), "minecraft:cobblestone");
        }
    }
}

#[test]
fn circuit_region_recognised_without_a_finding() {
    let src = theme_t!(
        "f",
        "\nstruct s size=3x3\n  floor mat_slot=f\n  circuit region=floor void=2\n"
    );
    assert_eq!(lowered(src).diagnostics, []);
}

/// A member lowering refuses, and what its one `E_INVALID_MEMBER` says.
struct OneRefusal {
    /// The member lines, after the source's shared head.
    members: &'static str,
    /// Text the primary contains.
    says: &'static [&'static str],
    /// Text the primary does not contain.
    never: &'static [&'static str],
    /// A note the finding carries, word for word.
    note: Option<&'static str>,
}

impl OneRefusal {
    /// A row that checks only what the primary says.
    const fn says(members: &'static str, says: &'static [&'static str]) -> Self {
        Self {
            members,
            says,
            never: &[],
            note: None,
        }
    }
}

/// Asserts `head` plus each row's members earns one finding, an `E_INVALID_MEMBER` saying what the
/// row says.
fn assert_each_refused_once(head: &str, rows: &[OneRefusal]) {
    for row in rows {
        let src = format!("{head}{}", row.members);
        let out = lowered(&src);
        let refused = &out.diagnostics;
        assert_eq!(refused.len(), 1, "{src}{refused:#?}");
        assert_eq!(refused[0].code, DiagnosticCode::InvalidMember, "{src}");
        let primary = &refused[0].primary;
        for text in row.says {
            assert!(
                primary.contains(text),
                "expected the primary to name `{text}`, got {primary}\n{src}",
            );
        }
        for text in row.never {
            assert!(
                !primary.contains(text),
                "expected the primary not to say `{text}`, got {primary}\n{src}",
            );
        }
        if let Some(note) = row.note {
            assert!(
                refused[0].notes.iter().any(|n| n.message == note),
                "expected the note {note:?}, got {:?}\n{src}",
                refused[0].notes,
            );
        }
    }
}

#[test]
fn a_malformed_circuit_is_refused_once_and_names_its_fault() {
    assert_each_refused_once(
        theme_t!("f", "\nstruct s size=3x3\n  floor mat_slot=f\n"),
        &[
            OneRefusal::says("  circuit void=2\n", &["region="]),
            OneRefusal::says("  circuit region=floor\n", &["void="]),
            OneRefusal::says("  circuit region=floor void=0\n", &["void=0"]),
            // Past `u32`: reported once, by `nonneg_int_or_refuse`.
            OneRefusal::says("  circuit region=floor void=99999999999\n", &["void="]),
            OneRefusal::says("  circuit region=\"\" void=2\n", &["non-empty"]),
            OneRefusal::says(
                "  circuit region=42 void=2\n",
                &["identifier or string label", "integer"],
            ),
        ],
    );
}

#[test]
fn actuator_patch_opened_by_recognised_without_a_finding() {
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  \
         walls mat_slot=w height=3\n  \
         door id=front side=front at=center\n  \
         door[id=front] opened_by=sig.open\n"
    );
    assert_eq!(lowered(src).diagnostics, []);
}

#[test]
fn a_malformed_actuator_patch_is_refused_once_and_names_its_fault() {
    assert_each_refused_once(
        theme_t!(
            "w",
            "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  \
             door id=front side=front at=center\n",
        ),
        &[
            OneRefusal::says(
                "  door[id=front class=main] opened_by=sig.open\n",
                &["class", "id="],
            ),
            // The parser accepts an empty selector.
            OneRefusal {
                members: "  door[] opened_by=sig.open\n",
                says: &["id="],
                never: &["unknown attribute"],
                note: None,
            },
            OneRefusal::says("  door[id=3] opened_by=sig.open\n", &["integer"]),
            OneRefusal::says("  door[id=back] opened_by=sig.open\n", &["back", "front"]),
            OneRefusal::says("  door[id=front]\n", &["opened_by"]),
            OneRefusal::says("  door[id=front] opened_by=3\n", &["sig.", "integer"]),
            OneRefusal::says("  door[id=front] opened_by=foo.bar\n", &["sig."]),
            // `powered_by=` is reserved (`spec/redstone` "Signal binding") but not read.
            OneRefusal::says(
                "  door[id=front] opened_by=sig.open powered_by=sig.on\n",
                &["powered_by", "opened_by"],
            ),
            OneRefusal::says(
                "  door[id=front] opened_by=sig.open.extra\n",
                &["two-segment", "sig.open.extra"],
            ),
            // The per-body `duplicate` check misses a second `id=front` under a `level`.
            OneRefusal {
                members: "  level y=0\n    door id=front side=back at=center\n  \
                          door[id=front] opened_by=sig.open\n",
                says: &["2 physical doors"],
                never: &[],
                note: Some(
                    "Fix: give the 2 `door` members distinct ids, so the selector names one.",
                ),
            },
        ],
    );
    assert_each_refused_once(
        theme_t!("w", "\nstruct s size=5x5\n  walls mat_slot=w height=3\n"),
        &[
            OneRefusal {
                members: "  door side=front at=center\n  door[id=front] opened_by=sig.open\n",
                says: &["1 physical door is declared in this scope, without an `id=`"],
                never: &[],
                note: Some("Fix: add `id=front` to the `door` this patch is meant to bind."),
            },
            OneRefusal::says(
                "  door[id=front] opened_by=sig.open\n",
                &["no physical door"],
            ),
        ],
    );
}

#[test]
fn actuator_patch_selects_door_declared_inside_level() {
    // `y=0` keeps a higher level's wall-top interaction out of it.
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  \
         walls mat_slot=w height=3\n  \
         level y=0\n    \
           door id=inner side=front at=center\n  \
         door[id=inner] opened_by=sig.tick\n"
    );
    assert_eq!(lowered(src).diagnostics, []);
}

#[test]
fn actuator_patch_does_not_repaint_the_door_voxels() {
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  \
         walls mat_slot=w height=3\n  \
         door id=front side=front at=center\n  \
         door[id=front] opened_by=sig.open\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 2, 1, 4), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 2, 2, 4), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 0, 1, 4), "minecraft:cobblestone");
    let ids: Vec<&str> = ba.palette.entries.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec![BlockState::AIR_ID, "minecraft:cobblestone"]);
}

#[test]
fn missing_theme_warns_and_air_fills() {
    let src = "struct s size=3x3\n  floor mat_slot=f\n";
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(findings(&out, DiagnosticCode::NoThemeBound).len(), 1);
    for z in 0..3 {
        for x in 0..3 {
            assert_eq!(block_id(ba, x, 0, z), BlockState::AIR_ID);
        }
    }
}

/// The `W_NO_THEME_BOUND` a themeless `struct s` that reads a slot earns.
const S_HAS_NO_THEME: &str =
    "struct `s` has no theme bound, so no `mat_slot=` in it resolves to a block";

/// `src`'s `cairn check` findings and its lowering.
fn checked_and_lowered(src: &str) -> (Vec<Diagnostic>, BlockArrayIr) {
    let module = parse(src).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    (
        crate::check::check(&module, &ir, None),
        lower_to_block_array(&ir, &resolution, None),
    )
}

/// The array `struct name` lowered to; a silence about a scope it never built proves nothing.
fn lowered_struct<'a>(out: &'a BlockArrayIr, name: &str, src: &str) -> &'a BlockArray {
    out.structures
        .get(&format!("struct::{name}"))
        .unwrap_or_else(|| panic!("`struct {name}` did not lower\n{src}{:#?}", out.diagnostics))
}

/// The ids `ba` paints, air left out.
fn painted_ids(ba: &BlockArray) -> std::collections::BTreeSet<&str> {
    ba.voxels
        .iter()
        .filter(|index| index.0 != 0)
        .map(|index| ba.palette.entries[usize::from(index.0)].id.as_str())
        .collect()
}

#[test]
fn a_scope_that_reads_no_slot_is_not_told_it_has_no_theme() {
    // `spec/lint` "Materials and targets": a scope that reads no `mat_slot=` is not reported.
    let src = "struct s size=3x3\n";
    let out = lowered(src);
    lowered_struct(&out, "s", src);
    assert_eq!(
        primaries_of(&out, DiagnosticCode::NoThemeBound),
        Vec::<&str>::new(),
        "{src}",
    );

    let src = "struct s size=3x3\n  roof kind=flat\n";
    let out = lowered(src);
    assert_eq!(
        painted_ids(lowered_struct(&out, "s", src)),
        [FLAT_BASE_ID].into(),
        "{src}",
    );
    assert_eq!(
        primaries_of(&out, DiagnosticCode::NoThemeBound),
        Vec::<&str>::new(),
        "{src}",
    );

    // Per scope, not per module.
    let src = "struct a size=3x3\n  roof kind=flat\n\n\
               struct b size=3x3\n  floor mat_slot=f\n";
    let out = lowered(src);
    assert_eq!(
        painted_ids(lowered_struct(&out, "a", src)),
        [FLAT_BASE_ID].into(),
        "{src}",
    );
    assert_eq!(
        primaries_of(&out, DiagnosticCode::NoThemeBound),
        ["struct `b` has no theme bound, so no `mat_slot=` in it resolves to a block"],
        "{src}",
    );
}

/// A themeless struct per slot-reading keyword, whose member is its only reader.
const SLOT_READERS: &[(&str, &str)] = &[
    ("floor", "struct s size=3x3\n  floor mat_slot=f\n"),
    ("walls", "struct s size=3x3\n  walls mat_slot=w height=2\n"),
    (
        "window",
        "struct s size=5x5\n  window side=front y=1 offset=1 size=1x1 mat_slot=g\n",
    ),
    ("roof", "struct s size=3x3\n  roof kind=flat mat_slot=r\n"),
    // An eave band needs an overhang, from a roof that reads no slot.
    (
        "stair",
        "struct s size=3x3\n  roof kind=gable overhang=1\n  \
         stair kind=stairs side=front half=bottom facing=out shape=straight mat_slot=s\n",
    ),
    (
        "pressure_plate",
        "struct s size=3x3\n  pressure_plate at=inside.front offset=1 mat_slot=p\n",
    ),
];

/// A themeless struct per keyword whose role reads no `mat_slot=`, `mat_slot=d` on its member;
/// a misplaced `place` or `connect` still reaches the list the gate reads.
const SLOT_IGNORERS: &[(&str, &str)] = &[
    (
        "door",
        "struct s size=3x3\n  door id=front side=front at=center mat_slot=d\n",
    ),
    (
        "level",
        "struct s size=3x3\n  level y=0 mat_slot=d\n    roof kind=flat\n",
    ),
    (
        "circuit",
        "struct s size=3x3\n  circuit region=floor void=2 mat_slot=d\n",
    ),
    (
        "place",
        "struct s size=3x3\n  place id=a use=hut theme=t at=origin mat_slot=d\n",
    ),
    (
        "connect",
        "struct s size=3x3\n  connect a.entry to b.entry path=@gravel mat_slot=d\n",
    ),
];

#[test]
fn a_scope_whose_only_reader_is_any_reading_role_is_told() {
    for (keyword, src) in SLOT_READERS {
        let out = lowered(src);
        lowered_struct(&out, "s", src);
        assert_eq!(
            primaries_of(&out, DiagnosticCode::NoThemeBound),
            [S_HAS_NO_THEME],
            "`{keyword}`\n{src}",
        );
    }
    // An unknown keyword's slot reads too, as `Member::read_slot` says.
    let src = "struct s size=3x3\n  torch mat_slot=t\n";
    assert_eq!(
        primaries_of(&lowered(src), DiagnosticCode::NoThemeBound),
        [S_HAS_NO_THEME],
        "{src}",
    );
}

#[test]
fn a_mat_slot_on_a_role_that_reads_none_does_not_make_its_scope_a_reader() {
    for (keyword, src) in SLOT_IGNORERS {
        let (checked, out) = checked_and_lowered(src);
        lowered_struct(&out, "s", src);
        assert_eq!(
            primaries_of(&out, DiagnosticCode::NoThemeBound),
            Vec::<&str>::new(),
            "`{keyword}`\n{src}",
        );
        let ignored: Vec<(&str, &str)> = checked
            .iter()
            .filter(|d| d.code == DiagnosticCode::IgnoredArgument)
            .map(|d| (&src[d.span.start..d.span.end], d.primary.as_str()))
            .collect();
        assert!(
            matches!(
                ignored.as_slice(),
                [("d", primary)]
                    if primary.starts_with("`mat_slot=` ")
                        && primary.contains(&format!("`{keyword}`"))
            ),
            "`{keyword}`: {ignored:#?}\n{src}",
        );
    }
}

#[test]
fn every_keyword_is_a_case_on_the_side_its_role_reads() {
    let reads = |keyword: &str| {
        let src = format!("struct s size=3x3\n  {keyword} mat_slot=x\n");
        let ir = lower(&parse(&src).expect("parse"));
        ir.structs[0].members[0].read_slot().is_some()
    };
    let (mut readers, mut ignorers): (Vec<&str>, Vec<&str>) = crate::intent::known_keywords()
        .iter()
        .copied()
        .partition(|keyword| reads(keyword));
    readers.sort_unstable();
    ignorers.sort_unstable();
    let rows = |table: &[(&'static str, &str)]| {
        let mut keywords: Vec<&'static str> = table.iter().map(|(keyword, _)| *keyword).collect();
        keywords.sort_unstable();
        keywords
    };
    assert_eq!(
        rows(SLOT_READERS),
        readers,
        "every keyword whose role reads a `mat_slot=` needs a row in SLOT_READERS, and only those",
    );
    assert_eq!(
        rows(SLOT_IGNORERS),
        ignorers,
        "every keyword whose role reads no `mat_slot=` needs a row in SLOT_IGNORERS, and only \
         those",
    );
}

#[test]
fn a_slot_read_under_a_level_still_warns_and_one_the_level_drops_does_not() {
    for src in [
        "struct s size=3x3\n  level y=0\n    floor mat_slot=f\n",
        "struct s size=3x3\n  level y=1\n    walls mat_slot=f height=2\n",
    ] {
        assert_eq!(
            primaries_of(&lowered(src), DiagnosticCode::NoThemeBound),
            [S_HAS_NO_THEME],
            "{src}",
        );
    }
    // A dropped member paints nothing whatever the theme, so it is no reader.
    for (src, code, primary) in [
        (
            "struct s size=3x3\n  level y=1\n    floor mat_slot=f\n",
            DiagnosticCode::DeferredMember,
            "level-scoped `floor` is not yet supported",
        ),
        (
            "struct s size=3x3\n  level y=1\n    roof kind=flat mat_slot=r\n",
            DiagnosticCode::DeferredMember,
            "level-scoped `roof` is not yet supported",
        ),
        (
            "struct s size=3x3\n  level y=0\n    level y=1\n      walls mat_slot=f height=2\n",
            DiagnosticCode::DeferredMember,
            "nested `level` blocks are not yet supported; this level and every member \
             declared under it were dropped",
        ),
    ] {
        let out = lowered(src);
        lowered_struct(&out, "s", src);
        assert_eq!(
            out.diagnostics
                .iter()
                .map(|d| (d.code, d.primary.as_str()))
                .collect::<Vec<_>>(),
            [(code, primary)],
            "{src}",
        );
    }
    // `check` refuses a level with members under it and no offset.
    for src in [
        "struct s size=3x3\n  level\n    walls mat_slot=f height=2\n",
        "struct s size=3x3\n  level y=top\n    walls mat_slot=f height=2\n",
    ] {
        let out = lowered(src);
        lowered_struct(&out, "s", src);
        assert_eq!(out.diagnostics, [], "{src}");
    }
}

#[test]
fn already_diagnosed_slot_does_not_re_warn() {
    let src = theme_t!("f", "\nstruct s size=3x3\n  floor mat_slot=missing\n");
    let out = lowered(src);
    assert!(
        deferrals(&out).is_empty()
            && findings(&out, DiagnosticCode::AbstractTokenDeferred).is_empty(),
        "no follow-on diagnostics expected, got {:?}",
        out.diagnostics,
    );
}

#[test]
fn abstract_token_lifts_through_supplied_resolver() {
    let resolver = FakeResolver {
        entries: vec![
            ("floor.wood.broadleaf", "oak_planks"),
            ("wall.stone.cobble", "cobblestone"),
        ],
    };
    let src = "theme t:\n  \
               slot f -> @floor.wood.broadleaf\n  \
               slot w -> @wall.stone.cobble\n\n\
               struct s size=3x3\n  \
               floor mat_slot=f\n  \
               walls mat_slot=w height=2\n";
    let out = lowered_with_resolver(src, &resolver);
    assert!(
        findings(&out, DiagnosticCode::AbstractTokenDeferred).is_empty(),
        "no abstract-token deferral expected when the resolver covers every token, got {:?}",
        out.diagnostics,
    );
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 1, 0, 1), "minecraft:oak_planks");
    assert_eq!(block_id(ba, 0, 1, 0), "minecraft:cobblestone");
}

/// Members reading `@wood.dark` from slot `trim`, the deferral's consequence, and a voxel check;
/// `None` is a member refused before it asks for its material, which earns no deferral.
struct DeferralArm {
    members: &'static str,
    consequence: Option<&'static str>,
    voxels: fn(&BlockArrayIr),
}

fn check_deferral_arms(arms: &[DeferralArm]) {
    for arm in arms {
        let members = arm.members;
        let out = lowered(&format!(
            "theme t:\n  slot wall -> @cobblestone\n  slot trim -> @wood.dark\n\n\
             struct s size=5x5\n  {members}\n"
        ));
        let expected: Vec<String> = arm
            .consequence
            .map(|c| {
                format!(
                    "abstract token `@wood.dark` cannot be lowered without the registry \
                     pack; {c}"
                )
            })
            .into_iter()
            .collect();
        assert_eq!(
            primaries_of(&out, DiagnosticCode::AbstractTokenDeferred),
            expected,
            "{members}",
        );
        (arm.voxels)(&out);
    }
}

fn struct_s(out: &BlockArrayIr) -> &BlockArray {
    out.structures.get("struct::s").expect("struct::s lowered")
}

fn has_id(out: &BlockArrayIr, id: &str) -> bool {
    struct_s(out).palette.entries.iter().any(|e| e.id == id)
}

/// Whether `out` refuses a cut with `code` for want of a wall to cut into.
fn refused_for_no_wall(out: &BlockArrayIr, code: DiagnosticCode) -> bool {
    primaries_of(out, code)
        .iter()
        .any(|p| p.contains("has no wall to cut into"))
}

#[test]
fn abstract_token_deferral_says_what_each_member_does_instead() {
    let default_block = Some("the member is built from its default block");
    check_deferral_arms(&[
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  \
                      window side=front offset=1 y=1 size=1x2 mat_slot=trim",
            consequence: Some("the window is not cut, and the wall stays"),
            voxels: |out| {
                for y in 1..=2 {
                    assert_eq!(block_id(struct_s(out), 1, y, 4), "minecraft:cobblestone");
                }
            },
        },
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  floor mat_slot=trim",
            consequence: Some("the cell falls back to air"),
            voxels: |out| assert_eq!(block_id(struct_s(out), 1, 0, 1), BlockState::AIR_ID),
        },
        DeferralArm {
            members: "floor mat_slot=wall\n  \
                      walls mat_slot=trim height=3\n  \
                      window side=front offset=1 y=1 size=1x2 mat_slot=wall",
            consequence: Some(
                "the walls are not built and take up no rows, so a door or window cut into \
                 them is refused",
            ),
            voxels: |out| {
                assert_eq!(struct_s(out).dims.y, 1);
                assert!(refused_for_no_wall(out, DiagnosticCode::DeferredMember));
            },
        },
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  roof kind=gable mat_slot=trim",
            consequence: default_block,
            voxels: |out| assert!(has_id(out, STAIR_BASE_ID)),
        },
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  roof kind=flat mat_slot=trim",
            consequence: default_block,
            voxels: |out| assert!(has_id(out, FLAT_BASE_ID)),
        },
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  \
                      roof kind=shed slope_to=front mat_slot=trim",
            consequence: default_block,
            voxels: |out| assert!(has_id(out, STAIR_BASE_ID)),
        },
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  \
                      roof kind=flat mat_slot=wall overhang=1\n  \
                      stair kind=stairs side=front mat_slot=trim",
            consequence: default_block,
            voxels: |out| assert!(has_id(out, STAIR_BASE_ID)),
        },
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  \
                      pressure_plate at=inside.front offset=2 y=0 mat_slot=trim",
            consequence: default_block,
            voxels: |out| assert!(has_id(out, PRESSURE_PLATE_BASE_ID)),
        },
    ]);
}

#[test]
fn a_member_refused_before_its_material_defers_no_abstract_token() {
    check_deferral_arms(&[
        DeferralArm {
            members: "floor mat_slot=wall\n  \
                      window side=front offset=1 y=1 size=1x2 mat_slot=trim",
            consequence: None,
            voxels: |out| {
                assert_eq!(struct_s(out).dims.y, 1);
                assert!(refused_for_no_wall(out, DiagnosticCode::InvalidMember));
            },
        },
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  roof kind=shed mat_slot=trim",
            consequence: None,
            voxels: |out| {
                assert_eq!(struct_s(out).dims.y, 4);
                assert!(!has_id(out, STAIR_BASE_ID));
                assert_eq!(
                    primaries_of(out, DiagnosticCode::InvalidMember),
                    vec!["shed roof requires `slope_to=` (one of front, back, left, right)"],
                );
            },
        },
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  \
                      roof kind=shed slope_to=sideways mat_slot=trim",
            consequence: None,
            voxels: |out| {
                assert_eq!(struct_s(out).dims.y, 4);
                assert!(!has_id(out, STAIR_BASE_ID));
                assert_eq!(
                    primaries_of(out, DiagnosticCode::InvalidMember),
                    vec![
                        "unknown shed `slope_to=sideways` (expected one of front, back, left, \
                         right)"
                    ],
                );
            },
        },
    ]);
}

#[test]
fn unknown_abstract_token_emits_e_unknown_abstract_token() {
    let resolver = FakeResolver {
        entries: vec![("floor.wood.broadleaf", "oak_planks")],
    };
    let src = "theme t:\n  \
               slot f -> @floor.wood.broadlef\n\n\
               struct s size=3x3\n  \
               floor mat_slot=f\n";
    let out = lowered_with_resolver(src, &resolver);
    let diag = *findings(&out, DiagnosticCode::UnknownAbstractToken)
        .first()
        .expect("expected E_UNKNOWN_ABSTRACT_TOKEN, got {:?}");
    assert_eq!(diag.severity(), Severity::Error);
    assert!(
        diag.notes
            .iter()
            .any(|n| n.message.contains("floor.wood.broadleaf")),
        "expected suggestion note in {:?}",
        diag.notes,
    );
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 1, 0, 1), BlockState::AIR_ID);
}

#[test]
fn struct_without_size_is_skipped_with_warning() {
    let src = theme_t!("f", "\nstruct s\n  floor mat_slot=f\n");
    let out = lowered(src);
    assert!(!out.structures.contains_key("struct::s"));
    assert_eq!(findings(&out, DiagnosticCode::StructNoSize).len(), 1);
}

#[test]
fn state_literal_round_trips_through_palette() {
    let mut palette = Palette::new_with_air();
    let token = ValueWithSpan::from_value(crate::ast::Value::new(
        ValueKind::Token("oak_log[axis=x]".to_owned()),
        0..16,
    ));
    let bs = resolve_block_state(&token, None).unwrap();
    let idx = palette.intern(bs);
    assert_eq!(palette.entries[usize::from(idx.0)].id, "minecraft:oak_log");
    assert_eq!(
        palette.entries[usize::from(idx.0)]
            .properties
            .get("axis")
            .map(String::as_str),
        Some("x"),
    );
}

#[test]
fn phase_order_independent_of_source_order() {
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  door side=front at=center\n  walls mat_slot=w height=3\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 2, 1, 4), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 2, 2, 4), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 0, 1, 0), "minecraft:cobblestone");
}

#[test]
fn roof_increases_dims_y_by_ceil_half_span() {
    // 1 floor row, 4 wall rows and ceil(7 / 2) ridge rows.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=9x7\n  walls mat_slot=w height=4\n  roof kind=gable mat_slot=r\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims, Dims { x: 9, y: 9, z: 7 });
}

#[test]
fn roof_overhang_extends_xz_dims_and_shifts_walls() {
    let src = "theme t:\n  slot f -> @oak_planks\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=9x7\n  floor mat_slot=f\n  walls mat_slot=w height=4\n  roof kind=gable mat_slot=r overhang=1\n";
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims.x, 11);
    assert_eq!(ba.dims.z, 9);
    assert_eq!(block_id(ba, 1, 0, 1), "minecraft:oak_planks");
    assert_eq!(block_id(ba, 9, 0, 7), "minecraft:oak_planks");
    assert_eq!(block_id(ba, 0, 0, 0), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 10, 0, 8), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 1, 1, 1), "minecraft:cobblestone");
    assert_eq!(block_id(ba, 0, 1, 0), BlockState::AIR_ID);
}

fn palette_of(out: &BlockArrayIr) -> Vec<BlockState> {
    out.structures
        .get("struct::s")
        .expect("scope lowered")
        .palette
        .entries
        .clone()
}

fn ids_carrying_properties(out: &BlockArrayIr) -> Vec<String> {
    palette_of(out)
        .into_iter()
        .filter(|e| !e.properties.is_empty())
        .map(|e| e.id)
        .collect()
}

/// A walled `struct s` whose roof, overhanging by one, reads `slot r` bound to `@material`.
fn roofed(kind: &str, material: &str) -> String {
    format!(
        "{}  slot r -> @{material}\n\n\
         struct s size=9x7\n  walls mat_slot=w height=4\n  \
         roof kind={kind} mat_slot=r overhang=1\n",
        theme_t!("w", ""),
    )
}

#[test]
fn a_sloped_roof_refuses_a_material_that_is_not_a_stair() {
    for kind in ["gable", "shed", "hip"] {
        let src =
            roofed(kind, "cobblestone").replace("roof kind=shed", "roof slope_to=front kind=shed");
        let out = lowered(&src);
        let reasons: Vec<&str> = refusals(&out).iter().map(|d| d.primary.as_str()).collect();
        assert!(
            reasons
                .iter()
                .any(|r| r.contains("minecraft:cobblestone") && r.contains("is not a stair")),
            "kind={kind}: expected the family refusal, got {reasons:?}",
        );
        assert!(
            !ids_carrying_properties(&out)
                .iter()
                .any(|id| id == "minecraft:cobblestone"),
            "kind={kind}: stair states must never land on a non-stair",
        );
        assert!(
            ids_carrying_properties(&out)
                .iter()
                .any(|id| id == STAIR_BASE_ID),
            "kind={kind}: the roof still gets built, out of the fallback species",
        );
    }
}

#[test]
fn a_sloped_roof_takes_any_stair_species_without_comment() {
    let out = lowered(&roofed("gable", "dark_oak_stairs"));
    assert!(
        out.diagnostics.is_empty(),
        "a stair species is a legal roof material, got {:?}",
        out.diagnostics,
    );
    // `.all()` alone would also hold on an empty roof.
    let stated = ids_carrying_properties(&out);
    assert!(!stated.is_empty(), "the roof must paint stairs");
    assert!(
        stated.iter().all(|id| id == "minecraft:dark_oak_stairs"),
        "the species must be the one bound, got {stated:?}",
    );
}

#[test]
fn a_roof_with_no_material_binding_keeps_the_fallback_silently() {
    let src = theme_t!(
        "w",
        "\n\
         struct s size=9x7\n  walls mat_slot=w height=4\n  \
         roof kind=gable overhang=1\n"
    );
    let out = lowered(src);
    assert!(out.diagnostics.is_empty(), "got {:?}", out.diagnostics);
    let stated = ids_carrying_properties(&out);
    assert!(!stated.is_empty(), "the roof must paint stairs");
    assert!(
        stated.iter().all(|id| id == STAIR_BASE_ID),
        "got {stated:?}",
    );
}

#[test]
fn a_roof_whose_binding_resolves_to_nothing_still_says_so() {
    let src = theme_t!(
        "w",
        "\n\
         struct s size=9x7\n  walls mat_slot=w height=4\n  \
         roof kind=gable mat_slot=nosuchslot overhang=1\n"
    );
    let out = lowered(src);
    assert!(
        deferrals(&out)
            .iter()
            .any(|d| d.primary.contains("did not resolve")),
        "got {:?}",
        deferrals(&out),
    );
}

#[test]
fn a_roof_reading_a_slot_from_a_theme_that_never_bound_says_so_once() {
    // Two themes and no `place theme=`: the module binds none.
    let src = "theme a:
  slot r -> @dark_oak_stairs

theme b:
  slot r -> @birch_stairs

struct s size=9x7
  roof kind=gable mat_slot=r overhang=1
";
    let out = lowered(src);
    assert!(
        deferrals(&out)
            .iter()
            .any(|d| d.primary.contains("did not resolve")),
        "got {:?}",
        out.diagnostics,
    );
    assert!(refusals(&out).is_empty(), "got {:?}", out.diagnostics);
    let stated = ids_carrying_properties(&out);
    assert!(
        !stated.is_empty() && stated.iter().all(|id| id == STAIR_BASE_ID),
        "the roof wears the fallback, got {stated:?}",
    );
}

#[test]
fn a_flat_roof_takes_any_block_because_it_attaches_no_states() {
    // `oak_stairs` painted bare is a valid blockstate, every property at its default.
    for material in ["stone_bricks", "oak_stairs"] {
        let out = lowered(&roofed("flat", material));
        assert!(
            deferrals(&out).is_empty(),
            "material={material}: {:?}",
            deferrals(&out),
        );
        assert!(
            palette_of(&out)
                .iter()
                .any(|e| e.id == format!("minecraft:{material}") && e.properties.is_empty()),
            "material={material}: the flat deck must be that block, bare",
        );
    }
}

#[test]
fn an_eave_stair_refuses_a_material_that_is_not_a_stair() {
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\
         \x20\x20slot trim -> @cobblestone\n\n\
         struct s size=9x7\n  walls mat_slot=w height=4\n  \
         roof kind=gable mat_slot=r overhang=1\n  \
         stair id=e kind=stairs mat_slot=trim side=front half=top shape=outer_left\n"
    );
    let out = lowered(src);
    assert!(
        refusals(&out)
            .iter()
            .any(|d| d.primary.contains("eave `stair`") && d.primary.contains("is not a stair")),
        "got {:?}",
        out.diagnostics,
    );
    assert!(
        !ids_carrying_properties(&out)
            .iter()
            .any(|id| id == "minecraft:cobblestone"),
    );
}

#[test]
fn an_eave_stair_with_no_binding_falls_back_to_the_stair_id() {
    // The roof binds another species, so the eave's fallback is identifiable.
    let src = theme_t!(
        "w",
        "  slot r -> @dark_oak_stairs\n\n\
         struct s size=9x7\n  walls mat_slot=w height=4\n  \
         roof kind=gable mat_slot=r overhang=1\n  \
         stair id=e kind=stairs side=front half=top shape=outer_left\n"
    );
    let out = lowered(src);
    assert!(out.diagnostics.is_empty(), "got {:?}", out.diagnostics);
    let stated = ids_carrying_properties(&out);
    assert!(
        stated.iter().any(|id| id == STAIR_BASE_ID),
        "the eave wears the stair fallback, got {stated:?}",
    );
    assert!(
        stated.iter().all(|id| is_stair(id)),
        "nothing outside the family may carry stair states, got {stated:?}",
    );
}

#[test]
fn an_eave_stair_takes_any_stair_species_without_comment() {
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\
         \x20\x20slot trim -> @birch_stairs\n\n\
         struct s size=9x7\n  walls mat_slot=w height=4\n  \
         roof kind=gable mat_slot=r overhang=1\n  \
         stair id=e kind=stairs mat_slot=trim side=front half=top shape=outer_left\n"
    );
    let out = lowered(src);
    assert!(deferrals(&out).is_empty(), "got {:?}", deferrals(&out));
    assert!(
        ids_carrying_properties(&out)
            .iter()
            .any(|id| id == "minecraft:birch_stairs"),
        "got {:?}",
        ids_carrying_properties(&out),
    );
}

#[test]
fn a_binding_inside_the_family_keeps_its_id_and_loses_its_states() {
    let src = theme_t!(
        "w",
        "  slot r -> @roof.fancy\n\n\
         struct s size=9x7\n  walls mat_slot=w height=4\n  \
         roof kind=gable mat_slot=r overhang=1\n"
    );
    let out = lowered_with_resolver(
        src,
        &StatefulResolver {
            token: "roof.fancy",
            id: "birch_stairs",
            properties: vec![("facing", "north")],
        },
    );
    assert!(
        deferrals(&out)
            .iter()
            .any(|d| d.primary.contains("also carried properties")),
        "got {:?}",
        out.diagnostics,
    );
    assert!(refusals(&out).is_empty(), "got {:?}", out.diagnostics);
    let stated = ids_carrying_properties(&out);
    assert!(
        stated.iter().all(|id| id == "minecraft:birch_stairs"),
        "the bound id survives, got {stated:?}",
    );
    let facings: std::collections::BTreeSet<String> = palette_of(&out)
        .iter()
        .filter_map(|e| e.properties.get("facing").cloned())
        .collect();
    assert!(
        facings.len() > 1,
        "the geometry's facings, not the binding's one: {facings:?}",
    );
}

#[test]
fn the_family_is_reported_before_the_properties_it_makes_moot() {
    // Built by hand: only a state literal on a non-stair reaches this from source.
    let mut properties = IndexMap::new();
    properties.insert("facing".to_owned(), "north".to_owned());
    let state = BlockState {
        id: "minecraft:cobblestone".to_owned(),
        properties,
    };
    let module = parse(&roofed("gable", "cobblestone")).expect("parse");
    let ir = lower(&module);
    let member = ir.structs[0]
        .members
        .iter()
        .find(|m| m.role == MemberRole::Roof)
        .expect("the roof member");
    // A real scope: `None` would leave `token` empty and weaken the assertions below.
    let resolution = resolve(&ir, None);
    let scope = resolution.scopes.get("struct::s").expect("scope");
    let mut diagnostics = Vec::new();
    let id = geometry_material_id(
        member,
        Some(scope),
        Some(&state),
        STAIR_BASE_ID,
        &GeometryMemberDescription {
            subject: "`gable` roof".to_owned(),
            states_from: "the geometry",
            requires_stair: true,
        },
        &mut diagnostics,
    );
    assert_eq!(id, STAIR_BASE_ID);
    assert_eq!(diagnostics.len(), 1, "one finding, got {diagnostics:?}");
    assert_eq!(
        diagnostics[0].code,
        DiagnosticCode::IncompatibleMaterial,
        "got {:?}",
        diagnostics[0],
    );
    // `spec/lint` "Machine-readable payload": where the material came from.
    let Some(DiagnosticData::IncompatibleMaterial {
        id,
        required,
        slot,
        token,
    }) = &diagnostics[0].data
    else {
        panic!("expected the payload, got {:?}", diagnostics[0].data);
    };
    assert_eq!(id, "minecraft:cobblestone");
    assert_eq!(required, "stair");
    assert_eq!(slot.as_deref(), Some("r"));
    assert_eq!(
        token.as_deref(),
        Some("cobblestone"),
        "the theme's slot value, so a dotted one reads as a pack mapping",
    );
}

#[test]
fn gable_roof_places_stairs_with_facing() {
    let src = "theme t:\n  slot r -> @spruce_stairs\n\nstruct s size=9x7\n  walls mat_slot=r height=4\n  roof kind=gable mat_slot=r overhang=1\n";
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    let north_eave = block_state_at(ba, 0, 5, 0);
    assert_eq!(north_eave.id, "minecraft:spruce_stairs");
    assert_eq!(north_eave.properties.get("facing").unwrap(), "south");
    assert_eq!(north_eave.properties.get("half").unwrap(), "bottom");
    let south_eave = block_state_at(ba, 0, 5, 8);
    assert_eq!(south_eave.properties.get("facing").unwrap(), "north");
    let apex = block_state_at(ba, 0, 9, 4);
    assert_eq!(apex.properties.get("half").unwrap(), "top");
    assert_eq!(apex.properties.get("facing").unwrap(), "south");
}

#[test]
fn door_carves_opening_through_front_wall() {
    let src = theme_t!(
        "w",
        "\nstruct s size=9x7\n  walls mat_slot=w height=4\n  door side=front at=center\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 4, 1, 6), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 4, 2, 6), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 3, 1, 6), "minecraft:cobblestone");
    assert_eq!(block_id(ba, 4, 3, 6), "minecraft:cobblestone");
}

#[test]
fn door_at_left_carves_first_column_of_front_wall() {
    let src = theme_t!(
        "w",
        "\nstruct s size=9x7\n  walls mat_slot=w height=4\n  door side=front at=left\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 0, 1, 6), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 0, 2, 6), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 4, 1, 6), "minecraft:cobblestone");
    assert_eq!(deferred_count(&out), 0);
}

#[test]
fn door_at_right_carves_last_column_of_front_wall() {
    let src = theme_t!(
        "w",
        "\nstruct s size=9x7\n  walls mat_slot=w height=4\n  door side=front at=right\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 8, 1, 6), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 8, 2, 6), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 4, 1, 6), "minecraft:cobblestone");
    assert_eq!(deferred_count(&out), 0);
}

#[test]
fn door_at_unknown_value_is_refused_naming_the_anchors() {
    let src = theme_t!(
        "w",
        "\nstruct s size=9x7\n  walls mat_slot=w height=4\n  door side=front at=middle\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    for x in 0..9 {
        assert_eq!(block_id(ba, x, 1, 6), "minecraft:cobblestone");
    }
    assert_eq!(
        invalid_lines(src, &out),
        [(
            "door side=front at=middle",
            "door has `at=middle`, which is not one of center, left, right — use \
             `at=center | left | right`",
        )],
    );
    assert_eq!(deferred_count(&out), 0);
}

#[test]
fn window_places_glass_with_symmetry() {
    let src = theme_t!(
        "w",
        "  slot g -> @glass_pane\n\nstruct s size=9x7\n  walls mat_slot=w height=4\n  window side=front offset=2 y=2 size=2x2 sym=true mat_slot=g\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    for dx in 0..2 {
        for dy in 0..2 {
            assert_eq!(
                block_id(ba, 2 + dx, 2 + dy, 6),
                "minecraft:glass_pane",
                "primary ({},{})",
                2 + dx,
                2 + dy,
            );
        }
    }
    for dx in 0..2 {
        for dy in 0..2 {
            assert_eq!(
                block_id(ba, 5 + dx, 2 + dy, 6),
                "minecraft:glass_pane",
                "mirror ({},{})",
                5 + dx,
                2 + dy,
            );
        }
    }
}

#[test]
fn a_window_past_the_wall_end_is_refused_and_cuts_nothing() {
    let src = theme_t!(
        "w",
        "  slot g -> @glass_pane\n\nstruct s size=5x5\n  walls mat_slot=w height=4\n  window side=front offset=3 y=2 size=3x2 mat_slot=g\n"
    );
    assert_refused_alone(
        src,
        "window side=front offset=3 y=2 size=3x2 mat_slot=g",
        "window extends beyond the `front` wall (offset=3 size=3x2 repeat=1 step=0, wall \
         length=5)",
        &[
            "the window ends at `offset + size.w`, which must be at most 5; lower `offset=` or narrow \
             `size=`",
        ],
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    for x in 0..5 {
        assert_eq!(block_id(ba, x, 2, 4), "minecraft:cobblestone");
    }
}

#[test]
fn an_unknown_roof_kind_is_refused_and_adds_no_height() {
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=4\n  roof kind=pyramid mat_slot=r\n"
    );
    assert_refused_alone(
        src,
        "roof kind=pyramid mat_slot=r",
        "unknown roof `kind=pyramid` (expected one of gable, shed, hip, flat)",
        &[],
    );
    assert_eq!(lowered(src).structures["struct::s"].dims.y, 5);
}

#[test]
fn shed_roof_voxelises_with_slope_to_front() {
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=shed slope_to=front mat_slot=r\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims.y, 9);
    let layer0 = block_state_at(ba, 0, 4, 0);
    assert_eq!(layer0.id, "minecraft:spruce_stairs");
    assert_eq!(layer0.properties.get("facing").unwrap(), "south");
    assert_eq!(layer0.properties.get("half").unwrap(), "bottom");
    let apex = block_state_at(ba, 0, 8, 4);
    assert_eq!(apex.properties.get("half").unwrap(), "top");
}

#[test]
fn a_shed_roof_without_slope_to_is_refused() {
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=shed mat_slot=r\n"
    );
    assert_refused_alone(
        src,
        "roof kind=shed mat_slot=r",
        "shed roof requires `slope_to=` (one of front, back, left, right)",
        &[],
    );
}

#[test]
fn hip_roof_voxelises_square_footprint() {
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=hip mat_slot=r\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims.y, 7);
    let apex = block_state_at(ba, 2, 6, 2);
    assert_eq!(apex.id, "minecraft:spruce_stairs");
    assert_eq!(apex.properties.get("half").unwrap(), "top");
    let nw_corner = block_state_at(ba, 0, 4, 0);
    assert_eq!(nw_corner.properties.get("shape").unwrap(), "outer_left");
    assert_eq!(nw_corner.properties.get("facing").unwrap(), "south");
}

#[test]
fn flat_roof_voxelises_single_layer_of_planks() {
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_planks\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=flat mat_slot=r\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims.y, 5);
    for z in 0..5 {
        for x in 0..5 {
            assert_eq!(block_id(ba, x, 4, z), "minecraft:spruce_planks");
        }
    }
    assert_eq!(deferred_count(&out), 0);
}

#[test]
fn flat_roof_honours_bound_mat_slot_id() {
    // A flat deck attaches no states, so even a stair id lands verbatim.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=flat mat_slot=r\n"
    );
    let out = lowered(src);
    assert_eq!(
        deferred_count(&out),
        0,
        "unexpected diagnostics: {:?}",
        out.diagnostics
    );
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 2, 4, 2), "minecraft:spruce_stairs");
}

fn block_state_at(ba: &BlockArray, x: u32, y: u32, z: u32) -> &BlockState {
    let i = ba.dims.index(x, y, z).expect("in-range coord");
    let pi = ba.voxels[i];
    &ba.palette.entries[usize::from(pi.0)]
}

#[test]
fn a_door_without_side_is_refused() {
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  door at=center\n"
    );
    assert_refused_alone(
        src,
        "door at=center",
        "missing `side=` (expected one of front, back, left, right)",
        &[],
    );
}

#[test]
fn a_window_with_a_non_ident_side_is_refused() {
    let src = theme_t!(
        "w",
        "  slot g -> @glass_pane\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  window side=3 offset=1 y=1 size=1x1 mat_slot=g\n"
    );
    assert_refused_alone(
        src,
        "window side=3 offset=1 y=1 size=1x1 mat_slot=g",
        "`side=` must be one of front, back, left, right, not integer `3`",
        &[],
    );
}

#[test]
fn a_side_typo_is_refused_with_what_was_written() {
    assert_refused_alone(
        theme_t!(
            "w",
            "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  door side=frnt at=center\n"
        ),
        "door side=frnt at=center",
        "unknown `side=frnt` (expected one of front, back, left, right)",
        &[],
    );
}

#[test]
fn a_window_missing_or_misreading_an_argument_it_needs_is_refused_with_its_form() {
    let offset = "write `offset=N`, where `N` is a non-negative integer: the window's first column \
                  along the wall, counted from 0 at its left end seen from outside";
    let y = "write `y=N`, where `N` is a non-negative integer: the window's lowest row, counted up \
             from 0, the base row of the struct or of the window's `level`";
    let size = "write `size=WxH`, where `W` and `H` are positive integers: the window's width \
                along the wall and its height in rows";
    for (window, primary, note) in [
        (
            "window side=front offset=x y=1 size=1x1",
            "window has an `offset=` that is not a non-negative integer that fits in u32",
            offset,
        ),
        (
            "window side=front offset=1 size=1x1",
            "window has no `y=`",
            y,
        ),
        (
            "window side=front offset=1 y=x size=1x1",
            "window has a `y=` that is not a non-negative integer that fits in u32",
            y,
        ),
        (
            "window side=front offset=1 y=1",
            "window has no `size=WxH`",
            size,
        ),
        (
            "window side=front offset=1 y=1 size=3",
            "window has a `size=` that is not a `WxH` of two positive integers",
            size,
        ),
    ] {
        assert_refused_alone(&walled_6x5_source(window), window, primary, &[note]);
    }
}

#[test]
fn a_window_whose_rows_run_past_the_last_addressable_row_is_refused() {
    let window = "window side=front offset=1 y=4294967295 size=1x2";
    assert_refused_alone(
        &walled_6x5_source(window),
        window,
        "window rows from y=4294967295 run past the highest row a build can address (size=1x2; \
         the walls occupy y=1..=3)",
        &["lower the window with `y=`, or shorten it with `size=`"],
    );
}

#[test]
fn sym_window_overlap_skips_mirror_with_warning() {
    let src = theme_t!(
        "w",
        "  slot g -> @glass_pane\n\nstruct s size=6x5\n  walls mat_slot=w height=4\n  window side=front offset=2 y=2 size=3x1 sym=true mat_slot=g\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_defers_on(
        src,
        &out,
        "window side=front offset=2 y=2 size=3x1 sym=true mat_slot=g",
        "overlap",
    );
    for x in 2..5 {
        assert_eq!(block_id(ba, x, 2, 4), "minecraft:glass_pane");
    }
    assert_eq!(block_id(ba, 1, 2, 4), "minecraft:cobblestone");
}

#[test]
fn a_sym_window_that_coincides_with_its_mirror_is_reported() {
    // `spec/syntax` "Selectors": a mirror overlapping its primary is `W_DEFERRED_MEMBER`.
    // Every centred shape on a 5-wide wall; `(0, 5)` puts `mirror_offset` at its clamp, 0.
    for (offset, width) in [(2, 1), (1, 3), (0, 5)] {
        let src = format!(
            "{}  slot g -> @glass_pane\n\n\
             struct s size=5x5\n  walls mat_slot=w height=3\n  \
             window side=front offset={offset} y=1 size={width}x1 sym=true mat_slot=g\n",
            theme_t!("w", ""),
        );
        let out = lowered(&src);
        let found: Vec<_> = out
            .diagnostics
            .iter()
            .map(|d| (d.code, d.primary.as_str()))
            .collect();
        assert_eq!(
            found,
            vec![(
                DiagnosticCode::DeferredMember,
                format!(
                    "`sym=true` window at offset={offset} size={width}x1 on the `front` wall \
                     coincides with its mirror (wall length=5); the mirror was skipped"
                )
                .as_str(),
            )],
        );
        let ba = out.structures.get("struct::s").unwrap();
        for x in offset..offset + width {
            assert_eq!(block_id(ba, x, 1, 4), "minecraft:glass_pane", "x={x}");
        }
    }
}

#[test]
fn at_center_picks_right_of_centre_on_even_width_walls() {
    let src = theme_t!(
        "w",
        "\nstruct s size=8x5\n  walls mat_slot=w height=3\n  door side=front at=center\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 4, 1, 4), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 3, 1, 4), "minecraft:cobblestone");
}

#[test]
fn gable_honours_bound_mat_slot_id() {
    let src = theme_t!(
        "w",
        "  slot r -> @oak_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=gable mat_slot=r\n"
    );
    let out = lowered(src);
    assert_eq!(
        deferred_count(&out),
        0,
        "unexpected diagnostics: {:?}",
        out.diagnostics
    );
    let ba = out.structures.get("struct::s").unwrap();
    assert!(
        ba.palette
            .entries
            .iter()
            .any(|s| s.id == "minecraft:oak_stairs"),
        "expected oak_stairs in palette, got {:?}",
        ba.palette
            .entries
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
    );
}

#[test]
fn gable_with_matching_mat_slot_stays_silent() {
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=gable mat_slot=r\n"
    );
    let out = lowered(src);
    assert_eq!(
        deferred_count(&out),
        0,
        "expected silence on matching mat_slot, got {:?}",
        out.diagnostics,
    );
}

#[test]
fn even_span_gable_apex_uses_half_top() {
    // An even span needs two `half=top` rows, or the ridge leaves an open V.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=8x4\n  walls mat_slot=w height=4\n  roof kind=gable mat_slot=r\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    let apex_low = block_state_at(ba, 0, 6, 1);
    let apex_high = block_state_at(ba, 0, 6, 2);
    assert_eq!(apex_low.properties.get("half").unwrap(), "top");
    assert_eq!(apex_high.properties.get("half").unwrap(), "top");
}

/// The caller `debug_assert!`s on the list, so asserting on it pins what a release build does.
#[test]
fn a_slot_no_write_ever_named_is_kept_rather_than_swept_out() {
    let mut palette = Palette::new_with_air();
    palette.intern(BlockState::bare("minecraft:cobblestone"));
    let canvas = Canvas::new(Dims { x: 1, y: 1, z: 1 }, vec![None]);

    let (voxels, never_painted) = prune_unreferenced(&mut palette, &canvas);
    assert_eq!(never_painted, [1]);
    assert_eq!(
        palette
            .entries
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["minecraft:air", "minecraft:cobblestone"],
    );
    assert_eq!(voxels, [PaletteIndex::AIR]);
}

#[test]
fn a_slot_a_later_write_covered_is_pruned_and_the_rest_renumber() {
    let mut palette = Palette::new_with_air();
    let covered = palette.intern(BlockState::bare("minecraft:glass_pane"));
    let winner = palette.intern(BlockState::bare("minecraft:cobblestone"));
    let mut canvas = Canvas::new(Dims { x: 1, y: 1, z: 1 }, vec![Some(Phase::Massing)]);
    canvas.for_member(0).paint((0, 0, 0), || covered);
    canvas.for_member(0).paint((0, 0, 0), || winner);

    let (voxels, never_painted) = prune_unreferenced(&mut palette, &canvas);
    assert_eq!(never_painted, Vec::<usize>::new());
    assert_eq!(
        palette
            .entries
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["minecraft:air", "minecraft:cobblestone"],
    );
    assert_eq!(voxels, [PaletteIndex(1)]);
}

/// Unreachable from source, so called directly; a release build clips and carries on.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "the dim math and the generator disagree")]
fn painting_outside_the_volume_is_not_a_silent_drop() {
    let mut canvas = Canvas::new(Dims { x: 2, y: 2, z: 2 }, vec![Some(Phase::Massing)]);
    canvas.for_member(0).paint((2, 0, 0), || PaletteIndex(1));
}

#[test]
fn even_span_gable_apex_rows_face_away_from_the_ridge() {
    // Inward-facing caps would leave a half-block undercut along the outer face.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=8x4\n  walls mat_slot=w height=4\n  roof kind=gable mat_slot=r\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    let apex_low = block_state_at(ba, 0, 6, 1);
    let apex_high = block_state_at(ba, 0, 6, 2);
    assert_eq!(apex_low.properties.get("facing").unwrap(), "north");
    assert_eq!(apex_high.properties.get("facing").unwrap(), "south");
}

#[test]
fn place_lowers_def_with_referenced_theme() {
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def cottage size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "\n",
        "site s:\n",
        "  place id=home1 use=cottage theme=t at=origin\n",
    );
    let out = lowered(src);
    let ba = out
        .structures
        .get("site::s::home1")
        .expect("place lowered under site::s::home1 key");
    assert_eq!(
        ba.dims,
        Dims { x: 3, y: 3, z: 3 },
        "place inherits the def's interior size, no overhang",
    );
    assert_eq!(block_id(ba, 0, 1, 0), "minecraft:cobblestone");

    let placement = out
        .placements
        .get("site::s::home1")
        .expect("placement record present");
    assert_eq!(placement.site, "s");
    assert_eq!(placement.place_id, "home1");
    assert_eq!(placement.source_def, "cottage");
    assert_eq!(placement.theme, "t");
    assert_eq!(placement.origin, (0, 0, 0));
    assert_eq!(placement.dims, ba.dims);
}

#[test]
fn east_of_offset_sums_prior_dims_and_gap() {
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def cottage size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "\n",
        "site s:\n",
        "  place id=a use=cottage theme=t at=origin\n",
        "  place id=b use=cottage theme=t east_of=a gap=2\n",
    );
    let out = lowered(src);
    let b = out
        .placements
        .get("site::s::b")
        .expect("placement b present");
    assert_eq!(
        b.origin,
        (5, 0, 0),
        "x = prior.x(0) + prior.dims.x(3) + gap(2)"
    );
    assert_eq!(b.origin.2, 0, "east_of does not move along z");
}

#[test]
fn north_of_subtracts_dims_and_gap_on_z_axis() {
    // Front is +z, so north is -z: `spec/syntax` "Selectors".
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def cottage size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "\n",
        "site s:\n",
        "  place id=a use=cottage theme=t at=origin\n",
        "  place id=b use=cottage theme=t north_of=a gap=4\n",
    );
    let out = lowered(src);
    let b = out
        .placements
        .get("site::s::b")
        .expect("placement b present");
    assert_eq!(
        b.origin,
        (0, 0, -7),
        "z = prior.z(0) - new.dims.z(3) - gap(4)"
    );
}

/// A site placing `b` (from `b_def`) at `selector=a gap=gap` beside `a` (from `a_def`);
/// `eaved`'s roof overhang makes its lowered dims 5x11, unlike its `size=`.
fn unequal_pair(a_def: &str, b_def: &str, selector: &str, gap: i32) -> BlockArrayIr {
    lowered(&format!(
        concat!(
            "def small size=3x3:\n",
            "  floor id=f mat_slot=floor\n",
            "  walls id=w mat_slot=wall height=2\n",
            "\n",
            "def deep size=3x9:\n",
            "  floor id=f mat_slot=floor\n",
            "  walls id=w mat_slot=wall height=2\n",
            "\n",
            "def eaved size=3x9:\n",
            "  floor id=f mat_slot=floor\n",
            "  walls id=w mat_slot=wall height=2\n",
            "  roof  id=r kind=flat mat_slot=wall overhang=1\n",
            "\n",
            "def wide size=9x3:\n",
            "  floor id=f mat_slot=floor\n",
            "  walls id=w mat_slot=wall height=2\n",
            "\n",
            "theme t:\n",
            "  slot floor -> @oak_planks\n",
            "  slot wall  -> @cobblestone\n",
            "\n",
            "site s:\n",
            "  place id=a use={a_def} theme=t at=origin\n",
            "  place id=b use={b_def} theme=t {selector}=a gap={gap}\n",
        ),
        a_def = a_def,
        b_def = b_def,
        selector = selector,
        gap = gap,
    ))
}

#[test]
fn north_of_leaves_exactly_gap_rows_between_unequal_depths() {
    for (a_def, b_def, b_z) in [
        ("small", "deep", -9),
        ("deep", "small", -3),
        ("small", "eaved", -11),
    ] {
        for gap in [0, 3] {
            let out = unequal_pair(a_def, b_def, "north_of", gap);
            let a = out.placements.get("site::s::a").expect("placement a");
            let b = out.placements.get("site::s::b").expect("placement b");
            // Without unequal depths the gap check holds for either reading.
            assert_ne!(a.dims.z, b.dims.z, "a={a_def}, b={b_def}");
            assert_eq!(
                b.origin,
                (0, 0, b_z - gap),
                "a={a_def}, b={b_def}, gap={gap}"
            );
            let b_front = b.origin.2 + i32::try_from(b.dims.z).unwrap();
            assert_eq!(
                a.origin.2 - b_front,
                gap,
                "a={a_def} at z {}..{}, b={b_def} at z {}..{}, gap={gap}",
                a.origin.2,
                a.origin.2 + i32::try_from(a.dims.z).unwrap(),
                b.origin.2,
                b_front,
            );
        }
    }
}

#[test]
fn east_of_leaves_exactly_gap_columns_between_unequal_widths() {
    // Not `north_of`'s mirror: the step is the prior's lowered `dims.x`.
    for (a_def, b_def, b_x) in [
        ("small", "wide", 3),
        ("wide", "small", 9),
        ("eaved", "small", 5),
    ] {
        for gap in [0, 3] {
            let out = unequal_pair(a_def, b_def, "east_of", gap);
            let a = out.placements.get("site::s::a").expect("placement a");
            let b = out.placements.get("site::s::b").expect("placement b");
            assert_ne!(a.dims.x, b.dims.x, "a={a_def}, b={b_def}");
            assert_eq!(
                b.origin,
                (b_x + gap, 0, 0),
                "a={a_def}, b={b_def}, gap={gap}"
            );
            let a_east = a.origin.0 + i32::try_from(a.dims.x).unwrap();
            assert_eq!(b.origin.0 - a_east, gap, "a={a_def}, b={b_def}, gap={gap}");
        }
    }
}

fn village_pair_source(extra_connects: &str) -> String {
    format!(
        concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def cottage size=3x3:\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=entry side=front at=center\n",
            "\n",
            "site s:\n",
            "  place id=a use=cottage theme=t at=origin\n",
            "  place id=b use=cottage theme=t east_of=a gap=4\n",
            "{}",
        ),
        extra_connects,
    )
}

#[test]
fn duplicate_connect_emits_w_duplicate_walkway_and_lays_one_strip() {
    let src = village_pair_source(
        "  connect a.entry to b.entry path=@gravel\n  connect a.entry to b.entry path=@gravel\n",
    );
    let out = lowered(&src);
    let dup_count = findings(&out, DiagnosticCode::DuplicateWalkway).len();
    assert_eq!(dup_count, 1, "expected exactly one W_DUPLICATE_WALKWAY");
    assert_eq!(out.walkways.len(), 1, "second row must not lay a strip");
}

#[test]
fn reverse_connect_dedupes_against_first_row() {
    let src = village_pair_source(
        "  connect a.entry to b.entry path=@gravel\n  connect b.entry to a.entry path=@gravel\n",
    );
    let out = lowered(&src);
    let dup_count = findings(&out, DiagnosticCode::DuplicateWalkway).len();
    assert_eq!(
        dup_count, 1,
        "expected exactly one W_DUPLICATE_WALKWAY on the reversed row",
    );
    assert_eq!(
        out.walkways.len(),
        1,
        "reversed row must not lay a second strip"
    );
}

#[test]
fn a_row_refused_after_the_dedup_check_leaves_its_pair_to_the_next_row() {
    // `@path.gravel` is abstract, so the first row lays nothing for the second to duplicate.
    let src = village_pair_source(
        "  connect a.entry to b.entry path=@path.gravel\n  connect b.entry to a.entry path=@gravel\n",
    );
    let out = lowered(&src);
    let codes: Vec<DiagnosticCode> = out.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(
        codes,
        [DiagnosticCode::AbstractTokenDeferred],
        "{:#?}",
        out.diagnostics
    );
    let laid: Vec<&str> = out.walkways.keys().map(WalkwayScopeKey::as_str).collect();
    assert_eq!(laid, ["walkway::s::b.entry__a.entry"]);
    assert_eq!(out.walkways[0].path_material, "minecraft:gravel");
}

fn invalid_walkway_ident_findings(out: &BlockArrayIr) -> Vec<(String, Vec<String>)> {
    findings(out, DiagnosticCode::InvalidWalkwayIdent)
        .into_iter()
        .map(|d| {
            (
                d.primary.clone(),
                d.notes.iter().map(|n| n.message.clone()).collect(),
            )
        })
        .collect()
}

#[test]
fn walkway_ids_with_an_edge_underscore_are_named_rather_than_aliased() {
    // The first two rows both encode to `a.p___b.p`; the sound third must still lay.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def hut size=5x5:\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  door  id=p  side=front at=center\n",
        "  door  id=p_ side=back  at=center\n",
        "\n",
        "site s:\n",
        "  place id=a  use=hut theme=t at=origin\n",
        "  place id=b  use=hut theme=t east_of=a gap=4\n",
        "  place id=_b use=hut theme=t north_of=a gap=4\n",
        "  connect a.p_ to b.p path=@gravel\n",
        "  connect a.p to _b.p path=@gravel\n",
        "  connect a.p to b.p path=@gravel\n",
    );
    let out = lowered(src);
    let findings = invalid_walkway_ident_findings(&out);
    let primaries: Vec<&str> = findings.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        primaries,
        [
            "walkway `a.p_ ↔ b.p` was dropped because the port id `p_` starts or ends \
             with `_`",
            "walkway `a.p ↔ _b.p` was dropped because the place id `_b` starts or ends \
             with `_`",
        ],
    );
    let notes: Vec<&[String]> = findings.iter().map(|(_, n)| n.as_slice()).collect();
    let rule = "a walkway's place and port ids may not start or end with `_` in either \
                position, so the rule does not depend on which way the row is written";
    assert_eq!(
        notes,
        [
            [format!(
                "{rule}; rename the port so it neither starts nor ends with `_`"
            )],
            [format!(
                "{rule}; rename the place so it neither starts nor ends with `_`"
            )],
        ],
    );
    let laid: Vec<&str> = out.walkways.keys().map(WalkwayScopeKey::as_str).collect();
    assert_eq!(laid, ["walkway::s::a.p__b.p"]);
    let walkway_structures: Vec<&str> = out
        .structures
        .keys()
        .filter(|k| k.starts_with("walkway::"))
        .map(String::as_str)
        .collect();
    assert_eq!(walkway_structures, ["walkway::s::a.p__b.p"]);
}

#[test]
fn a_port_named_underscore_is_named_rather_than_lowered_to_an_unparseable_key() {
    // `_` gives `a.___b._`, which splits back at the first `__` into an empty port.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def hut size=5x5:\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  door  id=_ side=front at=center\n",
        "  door  id=q side=back  at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=hut theme=t at=origin\n",
        "  place id=b use=hut theme=t east_of=a gap=4\n",
        "  connect a._ to b._ path=@gravel\n",
        "  connect a.q to b.q path=@gravel\n",
    );
    let out = lowered(src);
    let findings = invalid_walkway_ident_findings(&out);
    let primaries: Vec<&str> = findings.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        primaries,
        [
            "walkway `a._ ↔ b._` was dropped because the port id `_` starts or ends \
          with `_`"
        ],
    );
    let laid: Vec<&str> = out.walkways.keys().map(WalkwayScopeKey::as_str).collect();
    assert_eq!(laid, ["walkway::s::a.q__b.q"]);
    for key in out.walkways.keys() {
        assert_eq!(
            WalkwayScopeKey::parse(key.as_str()).as_ref(),
            Ok(key),
            "every laid walkway's key must parse back to itself",
        );
    }
}

#[test]
fn a_walkway_id_containing_dunder_is_named_with_the_dunder_repair() {
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def hut size=5x5:\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  door  id=b__c side=front at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=hut theme=t at=origin\n",
        "  place id=b use=hut theme=t east_of=a gap=4\n",
        "  connect a.b__c to b.b__c path=@gravel\n",
    );
    let out = lowered(src);
    let findings = invalid_walkway_ident_findings(&out);
    assert_eq!(findings.len(), 1, "{findings:?}");
    let (primary, notes) = &findings[0];
    assert_eq!(
        primary,
        "walkway `a.b__c ↔ b.b__c` was dropped because the port id `b__c` contains \
         `__`, the separator between the walkway scope key's `from` and `to` halves",
    );
    assert_eq!(
        notes,
        &[
            "a walkway's site, place and port ids may not contain `__`, so no id can be \
             mistaken for the separator; rename the port so it has no `__`, e.g. by \
             replacing `__` with `_`"
        ],
    );
    assert!(out.walkways.is_empty());
}

fn walkway_with_blocked_l_path_source() -> &'static str {
    // The straight L crosses `b`'s floor, but a detour through the gap between floors exists.
    concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def back_home size=3x3:\n",
        "  floor mat_slot=wall\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=back side=back at=center\n",
        "\n",
        "def front_home size=3x3:\n",
        "  floor mat_slot=wall\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=front side=front at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=back_home theme=t at=origin\n",
        "  place id=b use=front_home theme=t east_of=a gap=2\n",
        "  connect a.back to b.front path=@gravel\n",
    )
}

#[test]
fn walkway_routes_around_obstructed_l_path_without_warning() {
    let out = lowered(walkway_with_blocked_l_path_source());
    assert!(
        findings(&out, DiagnosticCode::WalkwayBlocked).is_empty(),
        "detourable row must not warn, got {:?}",
        out.diagnostics,
    );
    let walkway = out
        .walkways
        .get("walkway::s::a.back__b.front")
        .expect("walkway IR present");
    // Both shortest detours share the L's bounding box, whichever column the tie-break picks.
    assert_eq!(walkway.origin, (1, 0, -1));
    assert_eq!(walkway.footprint, Footprint { x: 6, z: 5 });
    let ba = out
        .structures
        .get("walkway::s::a.back__b.front")
        .expect("walkway block array present");
    let gravel_count = (0..ba.dims.volume())
        .filter(|&i| ba.palette.entries[usize::from(ba.voxels[i].0)].id == "minecraft:gravel")
        .count();
    assert_eq!(gravel_count, 10, "shortest detour lays 10 gravel cells");
    assert_eq!(block_id(ba, 0, 0, 0), "minecraft:gravel");
    assert_eq!(block_id(ba, 5, 0, 4), "minecraft:gravel");
    for dz in 1..=3 {
        assert_eq!(
            block_id(ba, 5, 0, dz),
            BlockState::AIR_ID,
            "floor cell at local (5, 0, {dz}) must stay untouched",
        );
    }
}

#[test]
fn walkway_detour_is_deterministic_across_lowerings() {
    // The lockfile pins walkway placement, so the tie-break must not follow hash order.
    let first = lowered(walkway_with_blocked_l_path_source());
    let second = lowered(walkway_with_blocked_l_path_source());
    assert_eq!(
        first.structures.get("walkway::s::a.back__b.front"),
        second.structures.get("walkway::s::a.back__b.front"),
    );
}

fn walkway_with_unroutable_port_source() -> &'static str {
    // As above, plus `c` north of `a` burying `a`'s port, so the row falls back to the L.
    concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def back_home size=3x3:\n",
        "  floor mat_slot=wall\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=back side=back at=center\n",
        "\n",
        "def front_home size=3x3:\n",
        "  floor mat_slot=wall\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=front side=front at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=back_home theme=t at=origin\n",
        "  place id=b use=front_home theme=t east_of=a gap=2\n",
        "  place id=c use=front_home theme=t north_of=a gap=0\n",
        "  connect a.back to b.front path=@gravel\n",
    )
}

#[test]
fn walkway_blocked_cells_skip_with_w_walkway_blocked_count() {
    // The count keeps the lockfile from claiming voxels the `.nbt` lacks.
    let out = lowered(walkway_with_unroutable_port_source());
    let blocked = findings(&out, DiagnosticCode::WalkwayBlocked);
    assert_eq!(
        blocked.len(),
        1,
        "expected one W_WALKWAY_BLOCKED, got {:?}",
        out.diagnostics,
    );
    assert_eq!(
        blocked[0].data,
        Some(DiagnosticData::WalkwayBlocked { skipped: 5 }),
        "expected the structured payload to report five skipped cells, got data={:?} primary={}",
        blocked[0].data,
        blocked[0].primary,
    );
    let note = &blocked[0].notes[0].message;
    assert!(
        note.contains("port `a.back` is buried"),
        "note must point at the buried port, got {note}",
    );
    assert!(
        blocked[0].primary.contains("skipped 5 cells"),
        "primary text contract must still name the skip count, got {}",
        blocked[0].primary,
    );
    let source = walkway_with_unroutable_port_source();
    let lines = crate::lines::LineStarts::new(source);
    let rendered = blocked[0].render(source, &lines);
    let value = serde_json::to_value(&rendered).expect("rendered serialises");
    assert_eq!(
        value["data"],
        serde_json::json!({"kind": "walkway_blocked", "skipped": 5}),
        "JSON `data` payload must match the structured contract, got {value}",
    );
    for d in &out.diagnostics {
        if d.code != DiagnosticCode::WalkwayBlocked {
            assert!(
                d.data.is_none(),
                "code {:?} unexpectedly carries a payload: {:?}",
                d.code,
                d.data,
            );
        }
    }
    let walkway = out
        .walkways
        .get("walkway::s::a.back__b.front")
        .expect("walkway IR present despite collisions");
    assert_eq!(walkway.origin, (1, 0, -1));
    assert_eq!(walkway.footprint, Footprint { x: 6, z: 5 });
    let ba = out
        .structures
        .get("walkway::s::a.back__b.front")
        .expect("walkway block array present despite collisions");
    assert_eq!(block_id(ba, 5, 0, 0), "minecraft:gravel");
    for dx in 0..=1 {
        assert_eq!(
            block_id(ba, dx, 0, 0),
            BlockState::AIR_ID,
            "blocked cell at local ({dx}, 0, 0) should stay air",
        );
    }
    for dz in 1..=3 {
        assert_eq!(
            block_id(ba, 5, 0, dz),
            BlockState::AIR_ID,
            "blocked cell at local (5, 0, {dz}) should stay air",
        );
    }
    assert_eq!(block_id(ba, 5, 0, 4), "minecraft:gravel");
}

fn walkway_blocked_note_of(source: &str) -> String {
    let out = lowered(source);
    let blocked = findings(&out, DiagnosticCode::WalkwayBlocked);
    assert_eq!(blocked.len(), 1, "diagnostics={:?}", out.diagnostics);
    assert_eq!(blocked[0].notes.len(), 1, "{:?}", blocked[0]);
    blocked[0].notes[0].message.clone()
}

#[test]
fn a_port_on_its_own_placements_pressure_plate_is_not_blamed_on_another_floor() {
    // Each hut's pressure plate sits on its own door's port cell.
    let note = walkway_blocked_note_of(concat!(
        "def hut size=5x5:\n",
        "  floor id=f mat_slot=wall\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  roof  kind=flat mat_slot=wall overhang=1\n",
        "  door  id=e side=front at=center\n",
        "  pressure_plate id=pp at=front.outside offset=2 y=0\n",
        "\n",
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "site s:\n",
        "  place id=a use=hut theme=t at=origin\n",
        "  place id=b use=hut theme=t east_of=a gap=4\n",
        "  connect a.e to b.e path=@gravel\n",
    ));
    assert_eq!(
        note,
        "port `a.e` sits on the `minecraft:oak_pressure_plate` its own placement `a` lays \
         on that cell; move that block or the door/window; port `b.e` sits on the \
         `minecraft:oak_pressure_plate` its own placement `b` lays on that cell; move that \
         block or the door/window",
    );
}

#[test]
fn a_port_buried_by_another_sites_placement_names_that_site() {
    // `c` buries `a`'s back port from another site, so its bare id names nothing in `s`.
    let source = walkway_with_unroutable_port_source()
        .replace("  place id=c use=front_home theme=t north_of=a gap=0\n", "")
        + concat!(
            "\n",
            "site u:\n",
            "  place id=o use=front_home theme=t at=origin\n",
            "  place id=c use=front_home theme=t north_of=o gap=0\n",
        );
    let note = walkway_blocked_note_of(&source);
    assert_eq!(
        note,
        "port `a.back` is buried inside the floor of placement `c` in site `u`; move that \
         door/window to an unobstructed wall or pull the placements apart",
    );
}

#[test]
fn a_router_past_the_area_cap_names_the_placements_that_stretch_its_box() {
    // `a` and `b` stretch the box; `e2` sets no edge and `e3` alone sets the north one.
    let note = walkway_blocked_note_of(concat!(
        "def hut size=3x3:\n",
        "  floor id=f mat_slot=wall\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  door  id=front side=front at=center\n",
        "  door  id=back  side=back  at=center\n",
        "\n",
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "site s:\n",
        "  place id=a use=hut theme=t at=origin\n",
        "  place id=b use=hut theme=t north_of=a gap=2000\n",
        "  place id=c use=hut theme=t east_of=b gap=2000\n",
        "  place id=d use=hut theme=t north_of=c gap=4\n",
        "  place id=e1 use=hut theme=t north_of=a gap=500\n",
        "  place id=e2 use=hut theme=t east_of=e1 gap=500\n",
        "  place id=e3 use=hut theme=t north_of=e2 gap=1600\n",
        "  connect c.front to d.back path=@gravel\n",
    ));
    assert_eq!(
        note,
        format!(
            "the router searches the box spanning both ports and every placement floor on \
             the walk plane, here 4238888 cells, past its cap of {ROUTE_AREA_CAP} cells; \
             the two ports span only 42 of those cells, and the floors of placements `a`, \
             `b`, `c`, `d`, `e1`, and `e3` stretch the box past them, so bring those \
             placements closer to the two ports",
        ),
    );
}

#[test]
fn edge_pushers_names_each_floor_that_sets_an_edge_beyond_both_ports() {
    // `in` sets no edge, `tie` meets one only at a port, `other_plane` lies on another plane.
    let extent = |min_x, max_x, min_z, max_z| FloorExtent {
        y: 0,
        min_x,
        max_x,
        min_z,
        max_z,
    };
    let extents: IndexMap<String, FloorExtent> = [
        ("w", extent(-20, -18, 4, 6)),
        ("e", extent(28, 30, 4, 6)),
        ("n", extent(4, 6, -40, -38)),
        ("s", extent(4, 6, 48, 50)),
        ("in", extent(2, 4, 2, 4)),
        ("tie", extent(8, 10, 4, 6)),
        ("other_plane", extent(28, 30, 20, 22)),
    ]
    .into_iter()
    .map(|(k, e)| {
        let e = if k == "other_plane" {
            FloorExtent { y: 7, ..e }
        } else {
            e
        };
        (k.to_owned(), e)
    })
    .collect();
    let rect = SearchRect {
        min_x: -21,
        max_x: 31,
        min_z: -41,
        max_z: 51,
    };
    assert_eq!(
        edge_pushers(rect, (0, 0, 0), (10, 0, 10), &extents),
        vec!["w", "e", "n", "s"],
    );
    let to_tie = SearchRect { max_x: 11, ..rect };
    assert_eq!(
        edge_pushers(to_tie, (0, 0, 0), (10, 0, 10), &extents),
        vec!["w", "n", "s"],
    );
}

#[test]
fn a_router_past_the_area_cap_on_distant_ports_blames_the_ports() {
    // The straight L is exactly at the cap, and the router's margin takes it past.
    let note = walkway_blocked_note_of(concat!(
        "def hut size=3x3:\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  door  id=front side=front at=center\n",
        "  door  id=back  side=back  at=center\n",
        "\n",
        "def pad size=3x3:\n",
        "  floor id=f mat_slot=wall\n",
        "  walls id=w mat_slot=wall height=3\n",
        "\n",
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "site s:\n",
        "  place id=a use=hut theme=t at=origin\n",
        "  place id=q use=hut theme=t east_of=a gap=1996\n",
        "  place id=p use=pad theme=t north_of=q gap=500\n",
        "  place id=b use=hut theme=t north_of=q gap=1992\n",
        "  connect a.front to b.back path=@gravel\n",
    ));
    assert_eq!(
        note,
        format!(
            "the router searches the box spanning both ports and every placement floor on \
             the walk plane, here 4010006 cells, past its cap of {ROUTE_AREA_CAP} cells; \
             the two ports alone span 4008004 of those cells, so place the two structures \
             closer together",
        ),
    );
}

#[test]
fn a_straight_l_past_the_area_cap_is_not_laid_and_says_why() {
    let out = lowered(concat!(
        "def hut size=3x3:\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  door  id=front side=front at=center\n",
        "  door  id=back  side=back  at=center\n",
        "\n",
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "site s:\n",
        "  place id=a use=hut theme=t at=origin\n",
        "  place id=q use=hut theme=t east_of=a gap=2100\n",
        "  place id=b use=hut theme=t north_of=q gap=2100\n",
        "  connect a.front to b.back path=@gravel\n",
    ));
    let blocked = findings(&out, DiagnosticCode::WalkwayBlocked);
    assert_eq!(blocked.len(), 1, "{:?}", out.diagnostics);
    let area = l_path_area((1, 0, 3), (2104, 0, -2104));
    assert_eq!(
        blocked[0].primary,
        format!(
            "walkway `a.front ↔ b.back` spans {area} cells, past the {ROUTE_AREA_CAP}-cell \
             router cap, and was not laid"
        ),
    );
    let notes: Vec<&str> = blocked[0]
        .notes
        .iter()
        .map(|n| n.message.as_str())
        .collect();
    assert_eq!(
        notes,
        [format!(
            "the walkway search area ({area} cells) exceeds the router's cap of \
             {ROUTE_AREA_CAP} cells; place the two structures closer together"
        )
        .as_str()],
    );
    assert_eq!(blocked[0].data, None);
    assert!(out.walkways.is_empty());
}

#[test]
fn a_port_on_its_own_plate_and_another_floor_names_both() {
    // `o`, from another site, floors over the cell where `a`'s own plate sits.
    let note = walkway_blocked_note_of(concat!(
        "def hut size=5x5:\n",
        "  floor id=f mat_slot=wall\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  roof  kind=flat mat_slot=wall overhang=1\n",
        "  door  id=e side=front at=center\n",
        "  pressure_plate id=pp at=front.outside offset=2 y=0\n",
        "\n",
        "def slab size=7x7:\n",
        "  floor id=f mat_slot=wall\n",
        "\n",
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "site s:\n",
        "  place id=a use=hut theme=t at=origin\n",
        "  place id=b use=hut theme=t east_of=a gap=4\n",
        "  connect a.e to b.e path=@gravel\n",
        "\n",
        "site u:\n",
        "  place id=o use=slab theme=t at=origin\n",
    ));
    assert_eq!(
        note,
        "port `a.e` is buried inside the floor of placement `o` in site `u` and sits on the \
         `minecraft:oak_pressure_plate` its own placement `a` lays on that cell; move that \
         block, and move that door/window to an unobstructed wall or pull the placements \
         apart; port `b.e` sits on the `minecraft:oak_pressure_plate` its own placement `b` \
         lays on that cell; move that block or the door/window",
    );
}

fn walkway_pair_source(path_token: &str) -> String {
    // No floors, so the path never collides.
    format!(
        concat!(
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
            "  connect a.entry to b.entry path={token}\n",
        ),
        token = path_token,
    )
}

#[test]
fn walkway_abstract_path_lifts_through_registry_pack() {
    let resolver = FakeResolver {
        entries: vec![("walkway.gravel", "gravel")],
    };
    let src = walkway_pair_source("@walkway.gravel");
    let out = lowered_with_resolver(&src, &resolver);
    assert!(
        findings(&out, DiagnosticCode::AbstractTokenDeferred).is_empty()
            && findings(&out, DiagnosticCode::UnknownAbstractToken).is_empty(),
        "no abstract-token diagnostics expected, got {:?}",
        out.diagnostics,
    );
    let walkway = out
        .walkways
        .get("walkway::s::a.entry__b.entry")
        .expect("walkway lowered");
    assert_eq!(walkway.path_material, "minecraft:gravel");
    // Pinned so a coordinate swap on the abstract-token path fails on its own.
    assert_eq!(walkway.origin, (1, 0, 3));
    assert_eq!(walkway.footprint, Footprint { x: 6, z: 1 });
}

#[test]
fn walkway_abstract_path_without_pack_emits_w_abstract_token_deferred() {
    let src = walkway_pair_source("@walkway.gravel");
    let out = lowered(&src);
    let diag = *findings(&out, DiagnosticCode::AbstractTokenDeferred)
        .first()
        .unwrap_or_else(|| {
            panic!(
                "expected W_ABSTRACT_TOKEN_DEFERRED on the connect row, got {:?}",
                out.diagnostics,
            )
        });
    assert_eq!(diag.severity(), Severity::Warning);
    assert!(
        diag.primary.contains("@walkway.gravel"),
        "expected the abstract token to be named, got {}",
        diag.primary,
    );
    assert!(
        !out.walkways.contains_key("walkway::s::a.entry__b.entry"),
        "no walkway should land without a material",
    );
}

#[test]
fn walkway_unknown_abstract_path_emits_e_unknown_abstract_token() {
    // `spec/materials-themes` "Canonical vocabulary": an undeclared token fails loud.
    let resolver = FakeResolver {
        entries: vec![("walkway.gravel", "gravel")],
    };
    let src = walkway_pair_source("@walkway.grvl");
    let out = lowered_with_resolver(&src, &resolver);
    let diag = *findings(&out, DiagnosticCode::UnknownAbstractToken)
        .first()
        .unwrap_or_else(|| {
            panic!(
                "expected E_UNKNOWN_ABSTRACT_TOKEN on the connect row, got {:?}",
                out.diagnostics,
            )
        });
    assert_eq!(diag.severity(), Severity::Error);
    assert!(
        diag.notes
            .iter()
            .any(|n| n.message.contains("walkway.gravel")),
        "expected nearest-match note pointing at walkway.gravel, got {:?}",
        diag.notes,
    );
    assert!(
        !out.walkways.contains_key("walkway::s::a.entry__b.entry"),
        "no walkway should land for an unknown abstract token",
    );
}

fn endpoint_cascade_source(a_def: &str, b_def: &str, defs: &str, connect_line: &str) -> String {
    format!(
        concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "{defs}",
            "\n",
            "site s:\n",
            "  place id=a use={a_def} theme=t at=origin\n",
            "  place id=b use={b_def} theme=t at=origin\n",
            "  {connect_line}\n",
        ),
        defs = defs,
        a_def = a_def,
        b_def = b_def,
        connect_line = connect_line,
    )
}

fn sized_then_sizeless_defs() -> &'static str {
    concat!(
        "def sized size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "\n",
        "def sizeless:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "\n",
    )
}

/// The `W_DEFERRED_MEMBER` a walkway against a placement that did not lower earns.
fn cascade_warning(out: &BlockArrayIr) -> &Diagnostic {
    deferrals(out)
        .into_iter()
        .find(|d| d.primary.contains("walkway") && d.primary.contains("did not lower"))
        .unwrap_or_else(|| {
            panic!(
                "expected a cascade W_DEFERRED_MEMBER, got {:?}",
                out.diagnostics,
            )
        })
}

#[test]
fn walkway_endpoint_skipped_to_side_cascades_w_deferred_member() {
    let src = endpoint_cascade_source(
        "sized",
        "sizeless",
        sized_then_sizeless_defs(),
        "connect a.entry to b.entry path=@gravel",
    );
    let out = lowered(&src);
    let cascade = cascade_warning(&out);
    assert!(
        cascade.primary.contains("`b.entry` placement") && !cascade.primary.contains("`a.entry`"),
        "expected the cascade to single out `b.entry`, got {}",
        cascade.primary,
    );
    assert!(
        !out.walkways.contains_key("walkway::s::a.entry__b.entry"),
        "walkway must not lay against a placement that did not lower",
    );
}

#[test]
fn walkway_endpoint_skipped_from_side_cascades_w_deferred_member() {
    let src = endpoint_cascade_source(
        "sizeless",
        "sized",
        sized_then_sizeless_defs(),
        "connect a.entry to b.entry path=@gravel",
    );
    let out = lowered(&src);
    let cascade = cascade_warning(&out);
    assert!(
        cascade.primary.contains("`a.entry` placement") && !cascade.primary.contains("`b.entry`"),
        "expected the cascade to single out `a.entry`, got {}",
        cascade.primary,
    );
    assert!(
        !out.walkways.contains_key("walkway::s::a.entry__b.entry"),
        "walkway must not lay against a placement that did not lower",
    );
}

#[test]
fn walkway_endpoint_skipped_both_sides_cascades_w_deferred_member() {
    let defs = concat!(
        "def sizeless_a:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "\n",
        "def sizeless_b:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "\n",
    );
    let src = endpoint_cascade_source(
        "sizeless_a",
        "sizeless_b",
        defs,
        "connect a.entry to b.entry path=@gravel",
    );
    let out = lowered(&src);
    let cascade = cascade_warning(&out);
    assert!(
        cascade.primary.contains("`a.entry`") && cascade.primary.contains("`b.entry`"),
        "expected the cascade to name both endpoints, got {}",
        cascade.primary,
    );
    assert!(
        !out.walkways.contains_key("walkway::s::a.entry__b.entry"),
        "walkway must not lay against a pair of skipped placements",
    );
}

#[test]
fn east_of_skipped_prior_does_not_silently_stack_at_origin() {
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def sized size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "\n",
        "def sizeless:\n",
        "  walls mat_slot=wall height=2\n",
        "\n",
        "site s:\n",
        "  place id=a use=sizeless theme=t at=origin\n",
        "  place id=b use=sized theme=t east_of=a gap=2\n",
    );
    let out = lowered(src);
    assert!(
        !out.placements.contains_key("site::s::b"),
        "placement b must be skipped, not silently stacked at origin",
    );
    let cascade = deferrals(&out)
        .iter()
        .any(|d| d.primary.contains("east_of") && d.primary.contains("origin cannot be resolved"));
    assert!(
        cascade,
        "expected a cascade W_DEFERRED_MEMBER mentioning the unresolvable origin, got {:?}",
        out.diagnostics,
    );
}

#[test]
fn a_level_without_y_drops_its_children_and_leaves_its_refusal_to_check() {
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=floor2\n    walls id=upper mat_slot=w height=2\n"
    );
    let out = lowered(src);
    assert_eq!(out.diagnostics, []);
    assert_eq!(out.structures["struct::s"].dims.y, 4);
}

#[test]
fn a_level_with_no_members_and_no_y_is_refused() {
    assert_refused_alone(
        theme_t!(
            "w",
            "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=floor2\n"
        ),
        "level id=floor2",
        "level has no `y=`, so nothing says which row its members stand on",
        &[
            "write `level y=N`, where `N` is a non-negative integer: the members under it are \
             measured from row `N`, as the members of the body are from row 0",
        ],
    );
}

#[test]
fn a_level_with_a_non_integer_y_is_refused_with_the_value_written() {
    assert_refused_alone(
        theme_t!(
            "w",
            "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=floor2 y=top\n"
        ),
        "level id=floor2 y=top",
        "level `y=` must be a non-negative integer that fits in u32, not identifier `top`",
        &[
            "write `level y=N`, where `N` is a non-negative integer: the members under it are \
             measured from row `N`, as the members of the body are from row 0",
        ],
    );
}

#[test]
fn nested_level_defers_per_inner_level_child() {
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=outer y=0\n    level id=inner1 y=1\n      walls id=a mat_slot=w height=1\n    level id=inner2 y=2\n      walls id=b mat_slot=w height=1\n"
    );
    let out = lowered(src);
    let nested: Vec<(&str, &str)> = deferred_lines(src, &out)
        .into_iter()
        .filter(|(_, primary)| primary.contains("nested"))
        .collect();
    assert_eq!(
        nested.len(),
        2,
        "one defer per inner level expected, got {nested:?}",
    );
    assert_eq!(
        nested.iter().map(|&(line, _)| line).collect::<Vec<_>>(),
        ["level id=inner1 y=1", "level id=inner2 y=2"],
        "each defer sits on its inner level",
    );
}

#[test]
fn max_wall_top_aggregates_across_level_walls() {
    // 1 floor row, a wall top at y=9 and ceil(5 / 2) gable rows.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=5\n  roof kind=gable mat_slot=r\n  level id=floor2 y=5\n    walls id=upper mat_slot=w height=4\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").expect("structure lowered");
    assert_eq!(ba.dims.y, 13);
    assert_eq!(
        deferred_count(&out),
        0,
        "unexpected defers: {:?}",
        out.diagnostics
    );
    for y in 6..=9 {
        assert_eq!(block_id(ba, 0, y, 0), "minecraft:cobblestone");
    }
}

/// A 5x5 struct under an `overhang=1` gable, for an eave band, followed by `stair`.
fn eave_stair_source(stair: &str) -> String {
    format!(
        "{}  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  \
         roof kind=gable mat_slot=r overhang=1\n  {stair}\n",
        theme_t!("w", ""),
    )
}

/// The one finding on `eave_stair_source(stair)`: its `E_INVALID_MEMBER`, as primary and notes.
fn eave_refusal(stair: &str) -> (String, Vec<String>) {
    let src = eave_stair_source(stair);
    let out = lowered(&src);
    assert_eq!(
        invalid_lines(&src, &out),
        [(stair, out.diagnostics[0].primary.as_str())]
    );
    assert_eq!(out.diagnostics.len(), 1, "{:#?}", out.diagnostics);
    let refusal = &out.diagnostics[0];
    (
        refusal.primary.clone(),
        refusal.notes.iter().map(|n| n.message.clone()).collect(),
    )
}

/// The note on a stair refused for its `kind=`.
const STAIR_KINDS: &str =
    "write `kind=stairs`, the one stair kind: a band of stairs in the overhang outside a wall";

#[test]
fn a_stair_without_kind_is_refused() {
    assert_eq!(
        eave_refusal("stair id=e mat_slot=r side=front half=top facing=out"),
        (
            "stair has no `kind=`, so nothing says what it builds".to_owned(),
            vec![STAIR_KINDS.to_owned()],
        ),
    );
}

#[test]
fn a_stair_with_an_unknown_kind_is_refused() {
    assert_eq!(
        eave_refusal("stair id=e kind=spiral mat_slot=r side=front half=top"),
        (
            "unknown stair `kind=spiral`".to_owned(),
            vec![STAIR_KINDS.to_owned()],
        ),
    );
}

#[test]
fn a_stair_with_an_unknown_state_is_refused_with_the_values_it_takes() {
    for (stair, primary, notes) in [
        (
            "stair id=e kind=stairs mat_slot=r side=front half=middle",
            "unknown stair `half=middle`",
            vec!["`half=` takes one of top, bottom"],
        ),
        (
            "stair id=e kind=stairs mat_slot=r side=front half=top facing=north",
            "unknown stair `facing=north`",
            vec!["`facing=` takes one of out, in"],
        ),
        (
            "stair id=e kind=stairs mat_slot=r side=front half=top facing=out shape=inner_left",
            "unknown stair `shape=inner_left`",
            vec!["`shape=` takes one of straight, outer_left, outer_right"],
        ),
    ] {
        assert_eq!(
            eave_refusal(stair),
            (
                primary.to_owned(),
                notes.into_iter().map(str::to_owned).collect::<Vec<_>>(),
            ),
            "{stair}",
        );
    }
}

#[test]
fn a_stair_on_a_struct_no_roof_gives_an_overhang_is_refused() {
    let stair = "stair id=e kind=stairs mat_slot=r side=front half=top facing=out";
    let src = format!(
        "{}  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  \
         roof kind=flat mat_slot=r\n  {stair}\n",
        theme_t!("w", ""),
    );
    assert_refused_alone(
        &src,
        stair,
        "eave `stair` has no overhang to sit in: no roof on this struct has an `overhang=` of at \
         least 1",
        &[
            "an eave band sits one voxel outside the wall, in the ring a roof's `overhang=` adds; give a \
             `roof` in the struct's body an `overhang=` of at least 1",
        ],
    );
}

#[test]
fn a_stair_beside_a_roof_that_draws_no_overhang_defers_to_the_roof() {
    // A roof that will not draw gives no overhang, however large its `overhang=`.
    let stair = "stair id=e kind=stairs mat_slot=r side=front half=top facing=out";
    let src = format!(
        "{}  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  \
         roof mat_slot=r overhang=2\n  {stair}\n",
        theme_t!("w", ""),
    );
    let out = lowered(&src);
    assert_eq!(
        deferred_lines(&src, &out),
        [(
            stair,
            "eave `stair` has no overhang to sit in: the roof written with an `overhang=` draws \
             nothing",
        )],
    );
    assert_eq!(
        invalid_lines(&src, &out),
        [(
            "roof mat_slot=r overhang=2",
            "missing `kind=` (expected one of gable, shed, hip, flat)",
        )],
    );
}

#[test]
fn a_stair_above_the_struct_is_refused() {
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=flat mat_slot=r overhang=1\n  stair id=e kind=stairs mat_slot=r side=front half=top facing=out y=99\n"
    );
    assert_refused_alone(
        src,
        "stair id=e kind=stairs mat_slot=r side=front half=top facing=out y=99",
        "stair y=99 does not fit in the struct (dims.y=5)",
        &[
            "the struct's top row is 4",
            "lower the stair or its `level`, or raise the `walls` to reach its row",
        ],
    );
}

#[test]
fn a_stair_on_a_row_only_unpainted_walls_reach_defers_to_them() {
    let src = "theme t:\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  \
               walls mat_slot=w height=3\n  roof kind=flat mat_slot=r overhang=1\n  \
               stair id=e kind=stairs mat_slot=r side=front y=2\n";
    let out = lowered(src);
    assert_eq!(
        deferred_lines(src, &out),
        [(
            "stair id=e kind=stairs mat_slot=r side=front y=2",
            "stair y=2 does not fit in the struct (dims.y=2)",
        )],
    );
    assert_eq!(invalid_lines(src, &out), []);
}

#[test]
fn stair_facing_in_paints_the_inward_cardinal() {
    let out = lowered(&eave_stair_source(
        "stair id=e kind=stairs mat_slot=r side=front half=top facing=in",
    ));
    assert_eq!(
        deferred_count(&out),
        0,
        "unexpected defers: {:?}",
        out.diagnostics
    );
    let ba = out.structures.get("struct::s").unwrap();
    let state = &ba.palette.entries[usize::from(ba.voxels[ba.dims.index(3, 0, 6).unwrap()].0)];
    assert_eq!(
        state.properties.get("facing").map(String::as_str),
        Some("north")
    );
}

fn walled_6x5_source(window: &str) -> String {
    format!(
        "{}\nstruct s size=6x5\n  walls mat_slot=w height=3\n  {window}\n",
        theme_t!("w", ""),
    )
}

#[test]
fn a_window_repeated_zero_times_is_refused() {
    let window = "window side=front y=1 size=1x1 repeat=0";
    assert_refused_alone(
        &walled_6x5_source(window),
        window,
        "window `repeat=0` would stamp no instances; drop the window instead",
        &[],
    );
}

#[test]
fn a_repeated_window_without_step_is_refused() {
    let window = "window side=front y=1 size=1x1 repeat=3";
    assert_refused_alone(
        &walled_6x5_source(window),
        window,
        "window `repeat=` requires a positive `step=` so instances do not overlap",
        &[],
    );
}

#[test]
fn window_repeat_with_sym_defers() {
    let window = "window side=front y=1 size=1x1 repeat=2 step=2 sym=true";
    let src = walled_6x5_source(window);
    assert_defers_on(&src, &lowered(&src), window, "`repeat=` and `sym=true`");
}

#[test]
fn a_window_series_past_the_wall_end_is_refused() {
    let window = "window side=front y=1 size=2x1 repeat=4 step=2";
    assert_refused_alone(
        &walled_6x5_source(window),
        window,
        "window extends beyond the `front` wall (offset=0 size=2x1 repeat=4 step=2, wall \
         length=6)",
        &[
            "the last instance ends at `offset + step * (repeat - 1) + size.w`, which must be at most \
             6; lower `offset=`, `step=` or `repeat=`, or narrow `size=`",
        ],
    );
}

#[test]
fn window_repeat_step_leaves_columns_between_stamps_alone() {
    let out = lowered(&walled_6x5_source(
        "window side=front y=1 size=1x1 repeat=2 step=3",
    ));
    assert_eq!(
        deferred_count(&out),
        0,
        "unexpected defers: {:?}",
        out.diagnostics
    );
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 0, 1, 4), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 3, 1, 4), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 1, 1, 4), "minecraft:cobblestone");
    assert_eq!(block_id(ba, 2, 1, 4), "minecraft:cobblestone");
}

#[test]
fn a_window_without_mat_slot_carves_one_stamp_to_air() {
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  window side=front y=1 offset=2 size=1x1\n"
    );
    let out = lowered(src);
    assert_eq!(
        deferred_count(&out),
        0,
        "unexpected defers: {:?}",
        out.diagnostics
    );
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 2, 1, 4), BlockState::AIR_ID);
}

#[test]
fn window_carve_stops_at_the_top_of_the_wall_not_the_top_of_the_volume() {
    // The flat deck at y=4 is inside the volume but above the wall.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=flat mat_slot=r\n  window side=front y=3 size=1x2\n"
    );
    assert_refused_alone(
        src,
        "window side=front y=3 size=1x2",
        "window rows y=3..=4 are not all inside one wall course (size=1x2; the walls occupy \
         y=1..=3)",
        &[
            "move the window with `y=`, shorten it with `size=`, or raise the walls' `height=`, so \
           one course holds every row it cuts",
        ],
    );
}

#[test]
fn a_door_whose_level_sits_at_or_above_the_wall_it_would_carve_is_refused() {
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=roofline y=3\n    door id=hole side=front at=center\n"
    );
    assert_refused_alone(
        src,
        "door id=hole side=front at=center",
        "door opens at y=4, which is not inside any wall course (the walls occupy y=1..=3)",
        &[
            "a door opens one row above the base of its `level`, or of the body outside one; move it \
             under a `level` whose walls hold that row, or add walls there",
        ],
    );
}

/// A 3x3 struct walled two high in `@oak_pressure_plate`, followed by `member`.
fn plate_walled_3x3(member: &str) -> String {
    format!(
        "{}\nstruct s size=3x3\n  walls mat_slot=p height=2\n  {member}\n",
        theme_t!("p" -> "oak_pressure_plate", ""),
    )
}

/// A finding as its code, primary and notes.
type Finding = (DiagnosticCode, String, Vec<String>);

/// Every finding of `out`, in report order.
fn findings_of(out: &BlockArrayIr) -> Vec<Finding> {
    out.diagnostics
        .iter()
        .map(|d| {
            (
                d.code,
                d.primary.clone(),
                d.notes.iter().map(|n| n.message.clone()).collect(),
            )
        })
        .collect()
}

/// The `E_INVALID_MEMBER` finding with `primary` and `notes`.
fn invalid_member(primary: &str, notes: &[&str]) -> Finding {
    (
        DiagnosticCode::InvalidMember,
        primary.to_owned(),
        notes.iter().map(|&n| n.to_owned()).collect(),
    )
}

/// The note a refused plate with a `->` binding ends with.
const BINDING_STILL_READ: &str =
    "its signal binding still reaches the netlist, with no plate placed to drive it";

#[test]
fn pressure_plate_outside_with_overhang_paints_in_the_overhang_column() {
    let out = lowered(&plate_walled_3x3(
        "roof kind=flat mat_slot=p overhang=1\n  pressure_plate at=front.outside offset=0 y=0",
    ));
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims.x, 5);
    assert_eq!(ba.dims.z, 5);
    assert_eq!(block_id(ba, 1, 0, 4), "minecraft:oak_pressure_plate");
    assert_ne!(block_id(ba, 1, 0, 3), "minecraft:oak_pressure_plate");
}

#[test]
fn pressure_plate_outside_on_the_floor_row_without_an_overhang_is_refused() {
    let out = lowered(&plate_walled_3x3(
        "pressure_plate at=front.outside offset=1 y=0",
    ));
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(
        findings_of(&out),
        [invalid_member(
            "pressure_plate `at=front.outside` at y=0 has no exterior voxel to sit on",
            &[
                "no roof on this struct draws an overhang, so it has no voxel outside its walls",
                "give a `roof` in the struct's body an `overhang=` of at least 1, or use \
                 `at=inside.front` with an `offset=` of 1",
            ],
        )],
    );
    assert_eq!((ba.dims.x, ba.dims.z), (3, 3));
    for (x, z) in [(0, 2), (1, 2), (2, 2), (1, 1)] {
        assert_eq!(block_id(ba, x, 0, z), BlockState::AIR_ID, "({x}, 0, {z})");
    }
}

#[test]
fn pressure_plate_outside_defers_beside_a_roof_that_draws_nothing() {
    // `left` and `back` shift back onto their wall cell, `front` and `right` out of the volume.
    for (side, offset, (x, z)) in [("left", 0, (0, 1)), ("front", 1, (1, 2))] {
        for (y, wall_cell) in [(0, BlockState::AIR_ID), (1, "minecraft:oak_pressure_plate")] {
            let out = lowered(&plate_walled_3x3(&format!(
                "roof mat_slot=p overhang=1\n  pressure_plate at={side}.outside offset={offset} \
                 y={y}"
            )));
            let ba = out.structures.get("struct::s").unwrap();
            let found = findings_of(&out);
            assert_eq!(
                found
                    .iter()
                    .map(|(code, primary, _)| (*code, primary.as_str()))
                    .collect::<Vec<_>>(),
                [
                    (
                        DiagnosticCode::InvalidMember,
                        "missing `kind=` (expected one of gable, shed, hip, flat)",
                    ),
                    (
                        DiagnosticCode::DeferredMember,
                        format!(
                            "pressure_plate `at={side}.outside` at y={y} has no exterior voxel \
                             to sit on"
                        )
                        .as_str(),
                    ),
                ],
            );
            assert_eq!(
                found[1].2,
                [
                    "a `roof` written with an `overhang=` draws nothing, so the struct has no \
                     overhang to hold the plate; it fits once that roof draws"
                ],
            );
            assert_eq!((ba.dims.x, ba.dims.z), (3, 3), "{side} y={y}");
            assert_eq!(block_id(ba, x, y, z), wall_cell, "{side} y={y}");
        }
    }
}

#[test]
fn pressure_plate_inside_paints_one_voxel_toward_the_interior() {
    let out = lowered(&plate_walled_3x3(
        "pressure_plate at=inside.front offset=1 y=0",
    ));
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 1, 0, 1), "minecraft:oak_pressure_plate");
    assert_eq!(deferred_count(&out), 0);
}

#[test]
fn pressure_plate_mat_slot_resolves_into_the_palette() {
    let src = "theme t:\n  slot fixture -> @spruce_pressure_plate\n\nstruct s size=3x3\n  walls mat_slot=fixture height=2\n  pressure_plate mat_slot=fixture at=inside.front offset=1 y=0\n";
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    let ids: Vec<&str> = ba.palette.entries.iter().map(|s| s.id.as_str()).collect();
    assert!(
        ids.contains(&"minecraft:spruce_pressure_plate"),
        "palette should carry the resolved id, got {ids:?}",
    );
    assert_eq!(block_id(ba, 1, 0, 1), "minecraft:spruce_pressure_plate");
}

/// Lowers `members` and one plate line in a `size=<size>` struct, with its findings.
fn lowered_plate(size: &str, members: &str, plate: &str) -> (BlockArrayIr, Vec<Finding>) {
    let src = format!(
        "theme t:\n  slot wall -> @cobblestone\n\nstruct s size={size}\n{members}  \
         pressure_plate {plate}\n",
    );
    let out = lowered(&src);
    let findings = findings_of(&out);
    (out, findings)
}

/// [`lowered_plate`] with `walls height=3` ahead of `extra`.
fn lowered_inside_plate(size: &str, extra: &str, plate: &str) -> (BlockArrayIr, Vec<Finding>) {
    lowered_plate(
        size,
        &format!("  walls mat_slot=wall height=3\n{extra}"),
        plate,
    )
}

fn plate_count(ba: &BlockArray) -> usize {
    ba.voxels
        .iter()
        .filter(|v| ba.palette.entries[usize::from(v.0)].id == PRESSURE_PLATE_BASE_ID)
        .count()
}

/// Roof overhangs for the inside-plate tests, each moving the wall ring inward.
const OVERHANGS: [u32; 3] = [0, 1, 2];

/// The member line that gives a struct `overhang`, empty for none.
fn overhang_roof(overhang: u32) -> String {
    if overhang == 0 {
        String::new()
    } else {
        format!("  roof kind=flat mat_slot=wall overhang={overhang}\n")
    }
}

#[test]
fn pressure_plate_inside_at_a_corner_is_refused_and_keeps_the_side_wall() {
    let (out, findings) =
        lowered_inside_plate("5x5", "", "id=p at=inside.front offset=0 y=1 -> sig.a");
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(
        findings,
        [invalid_member(
            "pressure_plate `at=inside.front offset=0` is at a corner of the front wall, so the \
             voxel inside it belongs to the neighbouring wall",
            &[
                "use an `offset=` from 1 to 3 to reach an interior voxel",
                BINDING_STILL_READ,
            ],
        )],
    );
    assert_eq!(block_id(ba, 0, 1, 3), "minecraft:cobblestone");
    assert_eq!(plate_count(ba), 0);
}

#[test]
fn pressure_plate_inside_at_a_corner_is_refused_at_the_floor_row_too() {
    let (out, findings) = lowered_inside_plate("5x5", "", "at=inside.front offset=0 y=0");
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(
        findings,
        [invalid_member(
            "pressure_plate `at=inside.front offset=0` is at a corner of the front wall, so the \
             voxel inside it is not an interior cell of this struct",
            &["use an `offset=` from 1 to 3 to reach an interior voxel"],
        )],
    );
    assert_eq!(plate_count(ba), 0);
}

#[test]
fn pressure_plate_inside_at_a_corner_claims_no_wall_where_none_is_painted() {
    // `y=3` falls between the two courses, rows 1 and 5.
    let cases = [
        ("  floor mat_slot=wall\n", 0),
        (
            "  walls mat_slot=wall height=1\n  level y=4\n    walls mat_slot=wall height=1\n",
            3,
        ),
    ];
    for (members, y) in cases {
        let (out, findings) =
            lowered_plate("5x5", members, &format!("at=inside.front offset=0 y={y}"));
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(
            findings,
            [invalid_member(
                "pressure_plate `at=inside.front offset=0` is at a corner of the front wall, so \
                 the voxel inside it is not an interior cell of this struct",
                &["use an `offset=` from 1 to 3 to reach an interior voxel"],
            )],
            "{members:?} y={y}",
        );
        assert_eq!(plate_count(ba), 0);
    }
}

#[test]
fn pressure_plate_inside_at_a_corner_of_a_long_left_wall_names_that_wall() {
    let (out, findings) = lowered_inside_plate("4x6", "", "at=inside.left offset=5 y=1");
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(
        findings,
        [invalid_member(
            "pressure_plate `at=inside.left offset=5` is at a corner of the left wall, so the \
             voxel inside it belongs to the neighbouring wall",
            &["use an `offset=` from 1 to 4 to reach an interior voxel"],
        )],
    );
    assert_eq!(plate_count(ba), 0);
}

/// Each side of a 4x5 footprint with its wall length.
const SIDES_4X5: [(&str, u32); 4] = [("front", 4), ("back", 4), ("left", 5), ("right", 5)];

#[test]
fn pressure_plate_inside_is_refused_at_both_corners_of_every_side() {
    for overhang in OVERHANGS {
        for (side, length) in SIDES_4X5 {
            for offset in [0, length - 1] {
                let (out, findings) = lowered_inside_plate(
                    "4x5",
                    &overhang_roof(overhang),
                    &format!("at=inside.{side} offset={offset} y=1"),
                );
                let ba = out.structures.get("struct::s").unwrap();
                let case = format!("overhang={overhang} inside.{side} offset={offset}");
                assert_eq!(
                    findings,
                    [invalid_member(
                        &format!(
                            "pressure_plate `at=inside.{side} offset={offset}` is at a corner of \
                             the {side} wall, so the voxel inside it belongs to the neighbouring \
                             wall",
                        ),
                        &[&format!(
                            "use an `offset=` from 1 to {} to reach an interior voxel",
                            length - 2,
                        )],
                    )],
                    "{case}",
                );
                assert_eq!(plate_count(ba), 0, "{case} painted");
            }
        }
    }
}

#[test]
fn pressure_plate_inside_paints_at_every_non_corner_offset_of_every_side() {
    let cells: [&[(u32, u32)]; 4] = [
        &[(1, 3), (2, 3)],
        &[(2, 1), (1, 1)],
        &[(1, 1), (1, 2), (1, 3)],
        &[(2, 3), (2, 2), (2, 1)],
    ];
    for overhang in OVERHANGS {
        for ((side, length), cells) in SIDES_4X5.into_iter().zip(cells) {
            assert_eq!(
                cells.len(),
                usize::try_from(length - 2).unwrap(),
                "{side} has one cell per non-corner offset",
            );
            for (offset, &(x, z)) in (1..length - 1).zip(cells) {
                let (out, findings) = lowered_inside_plate(
                    "4x5",
                    &overhang_roof(overhang),
                    &format!("at=inside.{side} offset={offset} y=1"),
                );
                let ba = out.structures.get("struct::s").unwrap();
                let case = format!("overhang={overhang} inside.{side} offset={offset}");
                assert_eq!(findings, [], "{case}");
                let (x, z) = (x + overhang, z + overhang);
                assert_eq!(
                    block_id(ba, x, 1, z),
                    PRESSURE_PLATE_BASE_ID,
                    "{case} should land on ({x}, 1, {z})",
                );
            }
        }
    }
}

#[test]
fn pressure_plate_inside_a_two_deep_struct_is_refused_and_keeps_the_back_wall() {
    // With no overhang `front.outside` finds no cell, so the reason does not offer it.
    let (out, findings) =
        lowered_inside_plate("5x2", "", "id=p at=inside.front offset=2 y=1 -> sig.a");
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(
        findings,
        [invalid_member(
            "pressure_plate `at=inside.front offset=2`: the struct's size.h is 2, so it has no \
             interior voxel",
            &[
                "an interior needs a size of at least 3 on both axes",
                "grow `size.h` to at least 3",
                BINDING_STILL_READ,
            ],
        )],
    );
    assert_eq!(block_id(ba, 2, 1, 0), "minecraft:cobblestone");
    assert_eq!(plate_count(ba), 0);
}

#[test]
fn pressure_plate_inside_a_two_deep_struct_at_the_floor_row_is_offered_the_outside_anchor_beside_an_overhang()
 {
    let primary = "pressure_plate `at=inside.front offset=2`: the struct's size.h is 2, so it has \
                   no interior voxel";
    let (out, findings) = lowered_inside_plate("5x2", "", "at=inside.front offset=2 y=0");
    assert_eq!(
        findings,
        [invalid_member(
            primary,
            &[
                "an interior needs a size of at least 3 on both axes",
                "grow `size.h` to at least 3",
            ],
        )],
    );
    assert_eq!(plate_count(out.structures.get("struct::s").unwrap()), 0);
    let roof = overhang_roof(1);
    let (out, findings) = lowered_inside_plate("5x2", &roof, "at=inside.front offset=2 y=0");
    assert_eq!(
        findings,
        [invalid_member(
            primary,
            &[
                "an interior needs a size of at least 3 on both axes",
                "grow `size.h` to at least 3, or anchor the plate with `at=front.outside`",
            ],
        )],
    );
    assert_eq!(plate_count(out.structures.get("struct::s").unwrap()), 0);
    // The remedy the reason offers works on this very struct.
    let (out, findings) = lowered_inside_plate("5x2", &roof, "at=front.outside offset=2 y=0");
    assert_eq!(findings, []);
    assert_eq!(plate_count(out.structures.get("struct::s").unwrap()), 1);
}

#[test]
fn pressure_plate_inside_a_one_deep_struct_with_an_overhang_is_refused() {
    let roof = overhang_roof(1);
    let (out, findings) = lowered_inside_plate("5x1", &roof, "at=inside.front offset=2 y=1");
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(
        findings,
        [invalid_member(
            "pressure_plate `at=inside.front offset=2`: the struct's size.h is 1, so it has no \
             interior voxel",
            &[
                "an interior needs a size of at least 3 on both axes",
                "grow `size.h` to at least 3, or anchor the plate with `at=front.outside`",
            ],
        )],
    );
    assert_eq!(block_id(ba, 3, 1, 0), "minecraft:air");
    assert_eq!(plate_count(ba), 0);
    // The remedy the reason offers works on this very struct.
    let (out, findings) = lowered_inside_plate("5x1", &roof, "at=front.outside offset=2 y=1");
    assert_eq!(findings, []);
    assert_eq!(plate_count(out.structures.get("struct::s").unwrap()), 1);
}

#[test]
fn pressure_plate_inside_a_one_deep_struct_is_not_offered_the_outside_anchor_above_its_rows() {
    let (out, findings) =
        lowered_inside_plate("5x1", &overhang_roof(1), "at=inside.front offset=2 y=9");
    assert_eq!(
        findings,
        [invalid_member(
            "pressure_plate `at=inside.front offset=2`: the struct's size.h is 1, so it has no \
             interior voxel",
            &[
                "an interior needs a size of at least 3 on both axes",
                "grow `size.h` to at least 3",
            ],
        )],
    );
    assert_eq!(plate_count(out.structures.get("struct::s").unwrap()), 0);
}

#[test]
fn pressure_plate_inside_a_narrow_struct_names_the_narrow_axis() {
    let (out, findings) = lowered_inside_plate("2x5", "", "at=inside.front offset=1 y=1");
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(
        findings,
        [invalid_member(
            "pressure_plate `at=inside.front offset=1`: the struct's size.w is 2, so it has no \
             interior voxel, and `offset=1` is at a corner of the front wall besides",
            &[
                "an interior needs a size of at least 3 on both axes",
                "grow `size.w` to at least 3 and use an `offset=` away from both ends of the wall",
            ],
        )],
    );
    assert_eq!(plate_count(ba), 0);
}

#[test]
fn pressure_plate_inside_reports_a_thin_axis_and_a_corner_in_one_pass() {
    let cases = [
        (
            "2x9",
            "at=inside.left offset=0 y=1",
            "pressure_plate `at=inside.left offset=0`: the struct's size.w is 2, so it has no \
             interior voxel, and `offset=0` is at a corner of the left wall besides",
            "grow `size.w` to at least 3 and use an `offset=` from 1 to 7",
        ),
        (
            "2x2",
            "at=inside.front offset=0 y=1",
            "pressure_plate `at=inside.front offset=0`: the struct's size.w is 2 and size.h is \
             2, so it has no interior voxel, and `offset=0` is at a corner of the front wall \
             besides",
            "grow `size.w` to at least 3 and `size.h` to at least 3 and use an `offset=` away \
             from both ends of the wall",
        ),
    ];
    for (size, plate, primary, grow) in cases {
        let (out, findings) = lowered_inside_plate(size, "", plate);
        assert_eq!(
            findings,
            [invalid_member(
                primary,
                &["an interior needs a size of at least 3 on both axes", grow],
            )],
            "{size} {plate}",
        );
        assert_eq!(plate_count(out.structures.get("struct::s").unwrap()), 0);
    }
}

#[test]
fn pressure_plate_offset_past_the_wall_is_refused_before_the_wall_grid_is_asked() {
    // The first column past the wall: `INVARIANT(wall-grid-validated)` needs it refused first.
    let out = lowered(&plate_walled_3x3(
        "pressure_plate at=inside.front offset=3 y=0",
    ));
    assert_eq!(
        findings_of(&out),
        [invalid_member(
            "pressure_plate `offset=3` runs past the front wall (length 3)",
            &["an `at=inside.front` plate takes an `offset=` of 1, clear of the wall's corners"],
        )],
    );
}

#[test]
fn nonneg_int_rejects_values_that_do_not_fit_in_u32() {
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=huge y=5000000000\n"
    );
    assert_refused_alone(
        src,
        "level id=huge y=5000000000",
        "level `y=` must be a non-negative integer that fits in u32, not integer `5000000000`",
        &[
            "write `level y=N`, where `N` is a non-negative integer: the members under it are \
             measured from row `N`, as the members of the body are from row 0",
        ],
    );
}
